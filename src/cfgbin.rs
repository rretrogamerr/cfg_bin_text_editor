use std::collections::HashMap;

use anyhow::{Context, Result};
use encoding_rs::SHIFT_JIS;
use serde::{Deserialize, Serialize};

use crate::crc32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VarType {
    String,
    Int,
    Float,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum VarValue {
    String(Option<String>),
    Int(i32),
    Float(f32),
    Unknown(i32),
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub var_type: VarType,
    pub value: VarValue,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub variables: Vec<Variable>,
    pub children: Vec<Entry>,
    pub end_terminator: bool,
}

impl Entry {
    fn get_name(&self) -> String {
        let parts: Vec<&str> = self.name.split('_').collect();
        if parts.len() > 1 {
            parts[..parts.len() - 1].join("_")
        } else {
            self.name.clone()
        }
    }

    fn count(&self) -> i32 {
        let mut total = 1 + if self.end_terminator { 1 } else { 0 };
        for child in &self.children {
            total += child.count();
        }
        total
    }

    fn get_unique_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let current_name = self.get_name();
        if !keys.contains(&current_name) {
            keys.push(current_name.clone());
        }
        for child in &self.children {
            for key in child.get_unique_keys() {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        if self.end_terminator {
            let end_name = if current_name.starts_with("PTREE") {
                "_PTREE".to_string()
            } else {
                current_name
                    .replace("BEGIN", "END")
                    .replace("BEG", "END")
            };
            if !keys.contains(&end_name) {
                keys.push(end_name);
            }
        }
        keys
    }

    fn encode_types(types: &[VarType]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let groups = (types.len() as f64 / 4.0).ceil() as usize;
        for i in 0..groups {
            let mut type_desc: u8 = 0;
            for j in (4 * i)..std::cmp::min(4 * (i + 1), types.len()) {
                let tag = match types[j] {
                    VarType::String => 0,
                    VarType::Int => 1,
                    VarType::Float => 2,
                    VarType::Unknown => 0,
                };
                type_desc |= tag << ((j % 4) * 2);
            }
            bytes.push(type_desc);
        }
        // Pad so (len + 1) % 4 == 0
        while (bytes.len() + 1) % 4 != 0 {
            bytes.push(0xFF);
        }
        bytes
    }

    fn encode_entry(&self, strings_table: &HashMap<String, i32>, encoding: &CfgBinEncoding) -> Vec<u8> {
        let mut buf = Vec::new();
        let entry_name = self.get_name();
        let crc = crc32::compute(&encode_string_bytes(&entry_name, encoding));

        buf.extend_from_slice(&crc.to_le_bytes());

        let types: Vec<VarType> = self.variables.iter().map(|v| v.var_type).collect();
        buf.push(types.len() as u8);
        buf.extend_from_slice(&Self::encode_types(&types));

        for var in &self.variables {
            match &var.value {
                VarValue::String(Some(s)) => {
                    if let Some(&offset) = strings_table.get(s) {
                        buf.extend_from_slice(&offset.to_le_bytes());
                    } else {
                        buf.extend_from_slice(&(-1i32).to_le_bytes());
                    }
                }
                VarValue::String(None) => {
                    buf.extend_from_slice(&(-1i32).to_le_bytes());
                }
                VarValue::Int(v) => buf.extend_from_slice(&v.to_le_bytes()),
                VarValue::Float(v) => buf.extend_from_slice(&v.to_le_bytes()),
                VarValue::Unknown(v) => buf.extend_from_slice(&v.to_le_bytes()),
            }
        }

        for child in &self.children {
            buf.extend_from_slice(&child.encode_entry(strings_table, encoding));
        }

        if self.end_terminator {
            let end_name = if entry_name.starts_with("PTREE") {
                "_PTREE".to_string()
            } else {
                self.get_name().replace("BEGIN", "END").replace("BEG", "END")
            };
            let end_crc = crc32::compute(&encode_string_bytes(&end_name, encoding));
            buf.extend_from_slice(&end_crc.to_le_bytes());
            buf.extend_from_slice(&[0x00, 0xFF, 0xFF, 0xFF]);
        }

        buf
    }

    fn collect_strings(&self) -> Vec<String> {
        let mut strings = Vec::new();
        for var in &self.variables {
            if let VarValue::String(Some(s)) = &var.value {
                if !strings.contains(s) {
                    strings.push(s.clone());
                }
            }
        }
        for child in &self.children {
            for s in child.collect_strings() {
                if !strings.contains(&s) {
                    strings.push(s);
                }
            }
        }
        strings
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfgBinEncoding {
    Utf8,
    ShiftJis,
}

impl CfgBinEncoding {
    fn byte_value(&self) -> u8 {
        match self {
            CfgBinEncoding::ShiftJis => 0,
            CfgBinEncoding::Utf8 => 1,
        }
    }
}

pub struct CfgBin {
    pub encoding: CfgBinEncoding,
    pub entries: Vec<Entry>,
}

fn read_i32(data: &[u8], pos: usize) -> i32 {
    i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn read_u32(data: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn read_f32(data: &[u8], pos: usize) -> f32 {
    f32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
}

fn decode_string(data: &[u8], encoding: &CfgBinEncoding) -> String {
    match encoding {
        CfgBinEncoding::Utf8 => String::from_utf8_lossy(data).to_string(),
        CfgBinEncoding::ShiftJis => {
            let (cow, _, _) = SHIFT_JIS.decode(data);
            cow.to_string()
        }
    }
}

fn encode_string_bytes(s: &str, encoding: &CfgBinEncoding) -> Vec<u8> {
    match encoding {
        CfgBinEncoding::Utf8 => s.as_bytes().to_vec(),
        CfgBinEncoding::ShiftJis => {
            let (cow, _, _) = SHIFT_JIS.encode(s);
            cow.to_vec()
        }
    }
}

fn read_null_terminated_string(data: &[u8], pos: &mut usize, encoding: &CfgBinEncoding) -> String {
    let start = *pos;
    while *pos < data.len() && data[*pos] != 0 {
        *pos += 1;
    }
    let s = decode_string(&data[start..*pos], encoding);
    if *pos < data.len() {
        *pos += 1; // skip null
    }
    s
}

fn round_up(n: usize, exp: usize) -> usize {
    ((n + exp - 1) / exp) * exp
}

fn write_alignment(buf: &mut Vec<u8>, alignment: usize, pad_byte: u8) {
    let remainder = buf.len() % alignment;
    if remainder != 0 {
        let padding = alignment - remainder;
        buf.extend(std::iter::repeat(pad_byte).take(padding));
    }
}

impl CfgBin {
    pub fn open(data: &[u8]) -> Result<Self> {
        // Read encoding flag at offset -0x0A from end
        let enc_byte = if data.len() >= 10 {
            data[data.len() - 10]
        } else {
            1 // default UTF-8
        };
        let encoding = if enc_byte == 0 {
            CfgBinEncoding::ShiftJis
        } else {
            CfgBinEncoding::Utf8
        };

        // Read header (16 bytes)
        let entries_count = read_i32(data, 0) as usize;
        let string_table_offset = read_i32(data, 4) as usize;
        let string_table_length = read_i32(data, 8) as usize;
        let string_table_count = read_i32(data, 12) as usize;

        // Parse string table
        let string_table_data = &data[string_table_offset..string_table_offset + string_table_length];
        let strings = Self::parse_strings(string_table_count, string_table_data, &encoding);

        // Parse key table
        let key_table_offset = round_up(string_table_offset + string_table_length, 16);
        let key_table_size = read_i32(data, key_table_offset) as usize;
        let key_table_data = &data[key_table_offset..key_table_offset + key_table_size];
        let key_table = Self::parse_key_table(key_table_data, &encoding);

        // Parse entries
        let entries_data = &data[0x10..string_table_offset];
        let entries = Self::parse_entries(entries_count, entries_data, &key_table, &strings, &encoding)?;

        Ok(CfgBin {
            encoding,
            entries,
        })
    }

    fn parse_strings(
        count: usize,
        data: &[u8],
        encoding: &CfgBinEncoding,
    ) -> HashMap<i32, String> {
        let mut result = HashMap::new();
        let mut pos = 0usize;
        for _ in 0..count {
            let offset = pos as i32;
            if !result.contains_key(&offset) {
                let s = read_null_terminated_string(data, &mut pos, encoding);
                result.insert(offset, s);
            }
        }
        result
    }

    fn parse_key_table(data: &[u8], encoding: &CfgBinEncoding) -> HashMap<u32, String> {
        let mut table = HashMap::new();

        // KeyHeader: key_length(4) + key_count(4) + key_string_offset(4) + key_string_length(4)
        let key_count = read_i32(data, 4) as usize;
        let key_string_offset = read_i32(data, 8) as usize;
        let key_string_length = read_i32(data, 12) as usize;

        let key_string_data = &data[key_string_offset..key_string_offset + key_string_length];

        let mut pos = 0x10; // after header
        for _ in 0..key_count {
            let crc = read_u32(data, pos);
            pos += 4;
            let string_start = read_i32(data, pos) as usize;
            pos += 4;

            // Find null terminator in key_string_data
            let mut end = string_start;
            while end < key_string_data.len() && key_string_data[end] != 0 {
                end += 1;
            }
            let key = decode_string(&key_string_data[string_start..end], encoding);
            table.insert(crc, key);
        }

        table
    }

    fn parse_entries(
        entries_count: usize,
        data: &[u8],
        key_table: &HashMap<u32, String>,
        strings: &HashMap<i32, String>,
        _encoding: &CfgBinEncoding,
    ) -> Result<Vec<Entry>> {
        let mut temp = Vec::new();
        let mut pos = 0usize;

        for _ in 0..entries_count {
            let crc = read_u32(data, pos);
            pos += 4;

            let name = key_table
                .get(&crc)
                .context(format!("Unknown CRC32: 0x{:08x}", crc))?
                .clone();

            let param_count = data[pos] as usize;
            pos += 1;

            let mut param_types = Vec::with_capacity(param_count);
            let type_byte_count = ((param_count as f64) / 4.0).ceil() as usize;

            for _ in 0..type_byte_count {
                let param_type_byte = data[pos];
                pos += 1;
                for k in 0..4 {
                    if param_types.len() < param_count {
                        let tag = (param_type_byte >> (2 * k)) & 3;
                        param_types.push(match tag {
                            0 => VarType::String,
                            1 => VarType::Int,
                            2 => VarType::Float,
                            _ => VarType::Unknown,
                        });
                    }
                }
            }

            // Alignment: if (ceil(paramCount/4) + 1) % 4 != 0, align to 4
            if (type_byte_count + 1) % 4 != 0 {
                pos = pos + (4 - (pos % 4));
            }

            let mut variables = Vec::with_capacity(param_count);
            for j in 0..param_count {
                match param_types[j] {
                    VarType::String => {
                        let offset = read_i32(data, pos);
                        pos += 4;
                        let text = if offset != -1 {
                            strings.get(&offset).cloned()
                        } else {
                            None
                        };
                        variables.push(Variable {
                            var_type: VarType::String,
                            value: VarValue::String(text),
                        });
                    }
                    VarType::Int => {
                        let v = read_i32(data, pos);
                        pos += 4;
                        variables.push(Variable {
                            var_type: VarType::Int,
                            value: VarValue::Int(v),
                        });
                    }
                    VarType::Float => {
                        let v = read_f32(data, pos);
                        pos += 4;
                        variables.push(Variable {
                            var_type: VarType::Float,
                            value: VarValue::Float(v),
                        });
                    }
                    VarType::Unknown => {
                        let v = read_i32(data, pos);
                        pos += 4;
                        variables.push(Variable {
                            var_type: VarType::Unknown,
                            value: VarValue::Unknown(v),
                        });
                    }
                }
            }

            temp.push(Entry {
                name,
                variables,
                children: Vec::new(),
                end_terminator: false,
            });
        }

        // Rename entries with occurrence indices
        let mut occurrences: HashMap<String, usize> = HashMap::new();
        for entry in &mut temp {
            let count = occurrences.entry(entry.name.clone()).or_insert(0);
            entry.name = format!("{}_{}", entry.name, count);
            *occurrences.get_mut(&entry.name.split('_').collect::<Vec<_>>()[..entry.name.split('_').count() - 1].join("_")).unwrap() += 1;
        }

        Ok(Self::process_entries(temp))
    }

    fn process_entries(entries: Vec<Entry>) -> Vec<Entry> {
        let mut stack: Vec<Entry> = Vec::new();
        let mut output: Vec<Entry> = Vec::new();
        let mut depth: Vec<(String, usize)> = Vec::new(); // ordered map

        fn depth_get(depth: &[(String, usize)], key: &str) -> Option<usize> {
            depth.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
        }
        fn depth_remove(depth: &mut Vec<(String, usize)>, key: &str) {
            depth.retain(|(k, _)| k != key);
        }
        fn depth_max_key(depth: &[(String, usize)]) -> String {
            depth.iter().max_by_key(|(_, v)| v).map(|(k, _)| k.clone()).unwrap_or_default()
        }

        let mut i = 0;
        while i < entries.len() {
            let name = entries[i].name.clone();
            let variables = entries[i].variables.clone();

            let name_parts: Vec<&str> = name.split('_').collect();
            let node_type = name_parts[name_parts.len() - 2].to_lowercase();
            let _node_name = name_parts[..name_parts.len() - 1].join("_").to_lowercase();

            let is_begin = (node_type.ends_with("beg")
                || node_type.ends_with("begin")
                || node_type.ends_with("start")
                || node_type.ends_with("ptree"))
                && !name.contains("_PTREE");

            let is_end = node_type.ends_with("end") || name.contains("_PTREE");

            if is_begin {
                let new_node = Entry {
                    name: name.clone(),
                    variables,
                    children: Vec::new(),
                    end_terminator: false,
                };

                if !stack.is_empty() {
                    let entry_name_max = depth_max_key(&depth);
                    let adjusted = entry_name_max.replace("_LIST_BEG_", "_BEG_");
                    let parts: Vec<&str> = adjusted.split('_').collect();
                    let base_name = parts[..parts.len().saturating_sub(2)].join("_");

                    if name.starts_with(&base_name)
                        && (node_type.ends_with("beg") || node_type.ends_with("begin"))
                    {
                        let stack_top = stack.last_mut().unwrap();
                        let last_child = stack_top.children.last_mut().unwrap();
                        last_child.children.push(new_node.clone());
                    } else {
                        stack.last_mut().unwrap().children.push(new_node.clone());
                    }
                } else {
                    output.push(new_node.clone());
                }

                stack.push(new_node);
                depth.push((name.clone(), stack.len()));
            } else if is_end {
                if let Some(top) = stack.last_mut() {
                    top.end_terminator = true;
                }

                let key = if depth_get(&depth, &name.replace("_END_", "_BEG_")).is_some() {
                    name.replace("_END_", "_BEG_")
                } else if depth_get(&depth, &name.replace("_END_", "_BEGIN_")).is_some() {
                    name.replace("_END_", "_BEGIN_")
                } else if depth_get(&depth, &name.replace("_END_", "_START_")).is_some() {
                    name.replace("_END_", "_START_")
                } else if depth_get(&depth, &name.replace("_PTREE", "PTREE")).is_some() {
                    name.replace("_PTREE", "PTREE")
                } else {
                    String::new()
                };

                if depth.len() > 1 {
                    if let Some(current_depth) = depth_get(&depth, &key) {
                        let previous_depth = current_depth - 1;
                        let pop_count = current_depth - previous_depth;
                        for _ in 0..pop_count {
                            if let Some(finished) = stack.pop() {
                                // Propagate end_terminator and children up
                                if let Some(parent) = stack.last_mut() {
                                    if let Some(child) = parent.children.iter_mut().find(|c| c.name == finished.name) {
                                        child.children = finished.children;
                                        child.end_terminator = finished.end_terminator;
                                    }
                                } else {
                                    // Update in output
                                    if let Some(out_entry) = output.iter_mut().find(|c| c.name == finished.name) {
                                        out_entry.children = finished.children;
                                        out_entry.end_terminator = finished.end_terminator;
                                    }
                                }
                            }
                        }
                        depth_remove(&mut depth, &key);
                    }
                } else {
                    if let Some(finished) = stack.pop() {
                        if let Some(out_entry) = output.iter_mut().find(|c| c.name == finished.name) {
                            out_entry.children = finished.children;
                            out_entry.end_terminator = finished.end_terminator;
                        }
                    }
                    depth_remove(&mut depth, &key);
                }
            } else {
                let new_item = Entry {
                    name: name.clone(),
                    variables,
                    children: Vec::new(),
                    end_terminator: false,
                };

                if depth.is_empty() {
                    let mut node = new_item;
                    node.end_terminator = true;
                    output.push(node);
                } else {
                    let entry_name_max = depth_max_key(&depth);
                    let adjusted = entry_name_max.replace("_LIST_BEG_", "_BEG_");
                    let parts: Vec<&str> = adjusted.split('_').collect();
                    let base_name = parts[..parts.len().saturating_sub(2)].join("_");

                    if !name.starts_with(&base_name) {
                        let is_begin_type = entry_name_max.contains("BEGIN")
                            || entry_name_max.contains("BEG")
                            || entry_name_max.contains("START")
                            || entry_name_max.contains("PTREE");

                        if !is_begin_type && !name.contains("_PTREE") {
                            if let Some(finished) = stack.pop() {
                                if let Some(parent) = stack.last_mut() {
                                    if let Some(child) = parent.children.iter_mut().find(|c| c.name == finished.name) {
                                        child.children = finished.children;
                                        child.end_terminator = finished.end_terminator;
                                    }
                                }
                                depth_remove(&mut depth, &entry_name_max);
                            }
                            stack.last_mut().unwrap().children.push(new_item);
                        } else {
                            let stack_top = stack.last_mut().unwrap();
                            let last_child = stack_top.children.last_mut().unwrap();
                            last_child.children.push(new_item.clone());
                            stack.push(new_item);
                            depth.push((name.clone(), stack.len()));
                        }
                    } else {
                        stack.last_mut().unwrap().children.push(new_item);
                    }
                }
            }

            i += 1;
        }

        output
    }

    pub fn save(&self) -> Vec<u8> {
        let distinct_strings = self.get_distinct_strings();
        let strings_table = self.build_strings_table(&distinct_strings);

        let mut buf = Vec::new();

        // Reserve 16 bytes for header
        buf.extend_from_slice(&[0u8; 16]);

        // Encode entries
        for entry in &self.entries {
            buf.extend_from_slice(&entry.encode_entry(&strings_table, &self.encoding));
        }

        // Align to 16 bytes with 0xFF
        write_alignment(&mut buf, 16, 0xFF);
        let string_table_offset = buf.len() as i32;

        let mut string_table_length = 0i32;
        if !distinct_strings.is_empty() {
            let strings_data = self.encode_strings(&distinct_strings);
            string_table_length = strings_data.len() as i32;
            buf.extend_from_slice(&strings_data);
            write_alignment(&mut buf, 16, 0xFF);
        }

        // Key table
        let unique_keys: Vec<String> = self
            .entries
            .iter()
            .flat_map(|e| e.get_unique_keys())
            .collect::<Vec<_>>()
            .into_iter()
            .fold(Vec::new(), |mut acc, k| {
                if !acc.contains(&k) {
                    acc.push(k);
                }
                acc
            });

        let key_table_data = self.encode_key_table(&unique_keys);
        buf.extend_from_slice(&key_table_data);

        // Footer
        buf.extend_from_slice(&[0x01, 0x74, 0x32, 0x62, 0xFE]);
        buf.extend_from_slice(&[0x01, self.encoding.byte_value(), 0x00, 0x01]);
        // WriteAlignment(): write 0x00 then align to 16 with 0xFF
        buf.push(0x00);
        write_alignment(&mut buf, 16, 0xFF);

        // Write header
        let entries_count = self.count_entries();
        buf[0..4].copy_from_slice(&(entries_count as i32).to_le_bytes());
        buf[4..8].copy_from_slice(&string_table_offset.to_le_bytes());
        buf[8..12].copy_from_slice(&string_table_length.to_le_bytes());
        buf[12..16].copy_from_slice(&(distinct_strings.len() as i32).to_le_bytes());

        buf
    }

    fn count_entries(&self) -> i32 {
        self.entries.iter().map(|e| e.count()).sum()
    }

    fn get_distinct_strings(&self) -> Vec<String> {
        let mut strings = Vec::new();
        for entry in &self.entries {
            for s in entry.collect_strings() {
                if !strings.contains(&s) {
                    strings.push(s);
                }
            }
        }
        strings
    }

    fn build_strings_table(&self, distinct_strings: &[String]) -> HashMap<String, i32> {
        let mut table = HashMap::new();
        let mut pos = 0i32;
        for s in distinct_strings {
            table.insert(s.clone(), pos);
            pos += encode_string_bytes(s, &self.encoding).len() as i32 + 1;
        }
        table
    }

    fn encode_strings(&self, distinct_strings: &[String]) -> Vec<u8> {
        let mut buf = Vec::new();
        for s in distinct_strings {
            buf.extend_from_slice(&encode_string_bytes(s, &self.encoding));
            buf.push(0x00);
        }
        buf
    }

    fn encode_key_table(&self, key_list: &[String]) -> Vec<u8> {
        let mut buf = vec![0u8; 16]; // header placeholder

        let mut string_offset = 0i32;
        let mut key_entries = Vec::new();
        for key in key_list {
            let crc = crc32::compute(&encode_string_bytes(key, &self.encoding));
            key_entries.extend_from_slice(&crc.to_le_bytes());
            key_entries.extend_from_slice(&string_offset.to_le_bytes());
            string_offset += encode_string_bytes(key, &self.encoding).len() as i32 + 1;
        }

        // Write entries starting at 0x10
        buf.extend_from_slice(&key_entries);
        write_alignment(&mut buf, 16, 0xFF);

        let key_string_offset = buf.len() as i32;

        // Write key strings
        let mut key_strings_data = Vec::new();
        for key in key_list {
            key_strings_data.extend_from_slice(&encode_string_bytes(key, &self.encoding));
            key_strings_data.push(0x00);
        }
        let key_string_length = key_strings_data.len() as i32;
        buf.extend_from_slice(&key_strings_data);
        write_alignment(&mut buf, 16, 0xFF);

        let key_length = buf.len() as i32;

        // Write header
        buf[0..4].copy_from_slice(&key_length.to_le_bytes());
        buf[4..8].copy_from_slice(&(key_list.len() as i32).to_le_bytes());
        buf[8..12].copy_from_slice(&key_string_offset.to_le_bytes());
        buf[12..16].copy_from_slice(&key_string_length.to_le_bytes());

        buf
    }

    /// Extract all text fields as a list of TextEntry for JSON export
    pub fn extract_texts(&self) -> Vec<TextEntry> {
        let mut texts = Vec::new();
        let mut global_index = 0usize;
        for entry in &self.entries {
            Self::collect_texts_recursive(entry, &mut texts, &mut global_index);
        }
        texts
    }

    fn collect_texts_recursive(entry: &Entry, texts: &mut Vec<TextEntry>, global_index: &mut usize) {
        let entry_name = entry.get_name();
        for (var_idx, var) in entry.variables.iter().enumerate() {
            if let VarValue::String(opt) = &var.value {
                texts.push(TextEntry {
                    index: *global_index,
                    entry: entry_name.clone(),
                    variable_index: var_idx,
                    value: opt.clone().unwrap_or_default(),
                });
                *global_index += 1;
            }
        }
        for child in &entry.children {
            Self::collect_texts_recursive(child, texts, global_index);
        }
    }

    /// Update text fields from a list of TextEntry (from JSON import)
    pub fn update_texts(&mut self, texts: &[TextEntry]) {
        let mut text_iter_index = 0usize;
        let mut global_index = 0usize;
        for entry in &mut self.entries {
            Self::update_texts_recursive(entry, texts, &mut text_iter_index, &mut global_index);
        }
    }

    fn update_texts_recursive(
        entry: &mut Entry,
        texts: &[TextEntry],
        text_iter_index: &mut usize,
        global_index: &mut usize,
    ) {
        for (_var_idx, var) in entry.variables.iter_mut().enumerate() {
            if let VarValue::String(_) = &var.value {
                if let Some(te) = texts.iter().find(|t| t.index == *global_index) {
                    if te.value.is_empty() {
                        var.value = VarValue::String(None);
                    } else {
                        var.value = VarValue::String(Some(te.value.clone()));
                    }
                }
                *global_index += 1;
            }
        }
        for child in &mut entry.children {
            Self::update_texts_recursive(child, texts, text_iter_index, global_index);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEntry {
    pub index: usize,
    pub entry: String,
    pub variable_index: usize,
    pub value: String,
}
