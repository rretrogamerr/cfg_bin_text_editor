mod cfgbin;
mod crc32;

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};

use cfgbin::{CfgBin, TextEntry};

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum Mode {
    Standard,
    Nnk,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum ExtractFormat {
    Json,
    Txt,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum UpdateFormat {
    Json,
    Txt,
}

#[derive(Parser)]
#[command(name = "cfg_bin_text_editor")]
#[command(about = "Extract and update text fields in Level-5 cfg.bin files")]
struct Cli {
    /// Extract text fields to JSON
    #[arg(short = 'e', value_name = "CFG_BIN_FILE", conflicts_with_all = ["write_file", "json_file", "output_file"])]
    extract_file: Option<PathBuf>,

    /// Write updated text fields back to cfg.bin
    #[arg(short = 'w', value_name = "CFG_BIN_FILE", requires = "json_file")]
    write_file: Option<PathBuf>,

    /// Input file for update (json or txt; use with -w)
    #[arg(value_name = "INPUT_FILE")]
    json_file: Option<PathBuf>,

    /// Output file path (used with -w, defaults to overwriting the original)
    #[arg(short = 'o', value_name = "OUTPUT_FILE")]
    output_file: Option<PathBuf>,

    /// Processing mode: standard(index-based rebuild) or nnk(address-based in-place patch)
    #[arg(long, value_enum, default_value_t = Mode::Standard)]
    mode: Mode,

    /// Extract output format: json (default) or txt (line-by-line values)
    #[arg(long, value_enum, default_value_t = ExtractFormat::Json)]
    extract_format: ExtractFormat,

    /// Update input format: json (default) or txt (line-by-line values)
    #[arg(long, value_enum, default_value_t = UpdateFormat::Json)]
    update_format: UpdateFormat,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cfg_path) = cli.extract_file {
        extract(&cfg_path, cli.mode, cli.extract_format)?;
    } else if let Some(cfg_path) = cli.write_file {
        let input_path = cli.json_file.unwrap();
        let out_path = cli.output_file.unwrap_or_else(|| cfg_path.clone());
        update(
            &cfg_path,
            &input_path,
            &out_path,
            cli.mode,
            cli.update_format,
        )?;
    } else {
        eprintln!("Usage:");
        eprintln!("  Extract: cfg_bin_text_editor -e <file.cfg.bin>");
        eprintln!("  Update:  cfg_bin_text_editor -w <file.cfg.bin> <input.json|input.txt>");
        eprintln!("  Update:  cfg_bin_text_editor -w <file.cfg.bin> <input.json|input.txt> -o <output.cfg.bin>");
        eprintln!("  Mode:    --mode standard|nnk");
        eprintln!("  Format:  --extract-format json|txt --update-format json|txt");
        std::process::exit(1);
    }

    Ok(())
}

fn normalize_txt_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

fn decode_txt_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }

    out
}

fn read_txt_lines(input_path: &PathBuf) -> Result<Vec<String>> {
    let raw = fs::read(input_path).context("Failed to read TXT file")?;
    let mut content = String::from_utf8(raw).context("TXT file must be UTF-8")?;
    if content.starts_with('\u{FEFF}') {
        content.remove(0);
    }
    if content.is_empty() {
        return Ok(Vec::new());
    }

    content = content.replace("\r\n", "\n").replace('\r', "\n");
    let has_trailing_newline = content.ends_with('\n');
    let mut lines: Vec<&str> = content.split('\n').collect();
    if has_trailing_newline && lines.last() == Some(&"") {
        lines.pop();
    }

    Ok(lines.into_iter().map(decode_txt_line).collect())
}

fn is_datetime_timestamp_line(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 19 {
        return false;
    }
    let is_digit = |c: u8| c.is_ascii_digit();
    is_digit(b[0])
        && is_digit(b[1])
        && is_digit(b[2])
        && is_digit(b[3])
        && b[4] == b'/'
        && is_digit(b[5])
        && is_digit(b[6])
        && b[7] == b'/'
        && is_digit(b[8])
        && is_digit(b[9])
        && b[10] == b' '
        && is_digit(b[11])
        && is_digit(b[12])
        && b[13] == b':'
        && is_digit(b[14])
        && is_digit(b[15])
        && b[16] == b':'
        && is_digit(b[17])
        && is_digit(b[18])
}

fn resolve_txt_update_offset(
    expected: usize,
    actual: usize,
    first_original_line: Option<&str>,
    input_path: &PathBuf,
) -> Result<usize> {
    if expected == actual {
        return Ok(0);
    }

    let can_skip_three_header_lines = expected >= 3
        && expected - 3 == actual
        && first_original_line.is_some_and(is_datetime_timestamp_line);

    if can_skip_three_header_lines {
        return Ok(3);
    }

    if first_original_line.is_some_and(is_datetime_timestamp_line) {
        bail!(
            "Line count mismatch in {}: expected {} (or {} when skipping 3 metadata lines), got {}. Keep one line per text entry and represent embedded newlines as \\n.",
            input_path.display(),
            expected,
            expected.saturating_sub(3),
            actual
        );
    }

    bail!(
        "Line count mismatch in {}: expected {}, got {}. Keep one line per text entry and represent embedded newlines as \\n.",
        input_path.display(),
        expected,
        actual
    );
}

fn extract(cfg_path: &PathBuf, mode: Mode, extract_format: ExtractFormat) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let (content, out_path, count) = match (mode, extract_format) {
        (Mode::Standard, ExtractFormat::Json) => {
            let cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let texts = cfg.extract_texts();
            let json =
                serde_json::to_string_pretty(&texts).context("Failed to serialize to JSON")?;
            (json, format!("{}.json", cfg_path.display()), texts.len())
        }
        (Mode::Standard, ExtractFormat::Txt) => {
            let cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let texts = cfg.extract_texts();
            let lines: Vec<String> = texts.iter().map(|t| normalize_txt_line(&t.value)).collect();
            (
                lines.join("\n"),
                format!("{}.txt", cfg_path.display()),
                texts.len(),
            )
        }
        (Mode::Nnk, ExtractFormat::Json) => {
            let texts = CfgBin::extract_texts_by_address_for_json(&data)
                .context("Failed to parse cfg.bin file in nnk mode")?;
            let json =
                serde_json::to_string_pretty(&texts).context("Failed to serialize to JSON")?;
            (json, format!("{}.json", cfg_path.display()), texts.len())
        }
        (Mode::Nnk, ExtractFormat::Txt) => {
            let texts = CfgBin::extract_texts_by_address(&data)
                .context("Failed to parse cfg.bin file in nnk mode")?;
            let lines: Vec<String> = texts.values().map(|v| normalize_txt_line(v)).collect();
            (
                lines.join("\n"),
                format!("{}.txt", cfg_path.display()),
                texts.len(),
            )
        }
    };
    fs::write(&out_path, &content).context("Failed to write extracted file")?;
    println!("Extracted {} text entries to {}", count, out_path);
    Ok(())
}

fn update(
    cfg_path: &PathBuf,
    input_path: &PathBuf,
    out_path: &PathBuf,
    mode: Mode,
    update_format: UpdateFormat,
) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let output = match (mode, update_format) {
        (Mode::Standard, UpdateFormat::Json) => {
            let json_data = fs::read_to_string(input_path).context("Failed to read JSON file")?;
            let mut cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let texts: Vec<TextEntry> =
                serde_json::from_str(&json_data).context("Failed to parse JSON file")?;
            let text_count = texts.len();
            cfg.update_texts(&texts);
            let output = cfg.save();
            println!(
                "Written {} ({} text entries, mode=standard, update=json)",
                out_path.display(),
                text_count
            );
            output
        }
        (Mode::Standard, UpdateFormat::Txt) => {
            let mut cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let mut texts = cfg.extract_texts();
            let expected = texts.len();
            let lines = read_txt_lines(input_path)?;
            let first_original_line = texts.first().map(|te| te.value.as_str());
            let offset =
                resolve_txt_update_offset(expected, lines.len(), first_original_line, input_path)?;

            for (te, line) in texts.iter_mut().skip(offset).zip(lines.into_iter()) {
                te.value = line;
            }

            cfg.update_texts(&texts);
            let output = cfg.save();
            println!(
                "Written {} ({} text entries, mode=standard, update=txt)",
                out_path.display(),
                expected
            );
            output
        }
        (Mode::Nnk, UpdateFormat::Json) => {
            let json_data = fs::read_to_string(input_path).context("Failed to read JSON file")?;
            let texts = CfgBin::parse_address_texts_json(&json_data)
                .context("Failed to parse address-based JSON for nnk mode")?;
            let text_count = texts.len();
            let output = CfgBin::patch_texts_by_address_in_place(&data, &texts)
                .context("Failed to patch cfg.bin in nnk mode")?;
            println!(
                "Written {} ({} text entries, mode=nnk, update=json)",
                out_path.display(),
                text_count
            );
            output
        }
        (Mode::Nnk, UpdateFormat::Txt) => {
            let mut texts = CfgBin::extract_texts_by_address(&data)
                .context("Failed to parse cfg.bin file in nnk mode")?;
            let expected = texts.len();
            let lines = read_txt_lines(input_path)?;
            let first_original_line = texts.values().next().map(String::as_str);
            let offset =
                resolve_txt_update_offset(expected, lines.len(), first_original_line, input_path)?;

            for ((_, value), line) in texts.iter_mut().skip(offset).zip(lines.into_iter()) {
                *value = line;
            }

            let output = CfgBin::patch_texts_by_address_in_place(&data, &texts)
                .context("Failed to patch cfg.bin in nnk mode")?;
            println!(
                "Written {} ({} text entries, mode=nnk, update=txt)",
                out_path.display(),
                expected
            );
            output
        }
    };
    fs::write(out_path, &output).context("Failed to write cfg.bin file")?;
    Ok(())
}
