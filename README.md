# cfg_bin_text_editor

A Rust CLI tool that extracts text fields from Level-5 game engine `cfg.bin` files to JSON, and writes modified JSON back into `cfg.bin` files.

## Usage

### Extract text

```sh
cfg_bin_text_editor -e <file.cfg.bin>
```

Produces `<file.cfg.bin>.json`.

### Update text

```sh
cfg_bin_text_editor -w <file.cfg.bin> <file.cfg.bin.json>
```

By default, the original file is overwritten. Use `-o` to write to a different file:

```sh
cfg_bin_text_editor -w <file.cfg.bin> <file.cfg.bin.json> -o <output.cfg.bin>
```

### Bulk operations (Windows)

`cbte_bulk.bat` processes all `cfg.bin` files in a folder recursively (including subfolders).

Extract all:

```bat
cbte_bulk.bat -e <folder>
```

Update all (each `*.cfg.bin.json` is matched to its `*.cfg.bin`):

```bat
cbte_bulk.bat -w <folder>
```

Requires `cfg_bin_text_editor.exe` in the same directory or in PATH. Progress is displayed during processing.

## JSON format

The extracted JSON is an array where each item represents a single text field:

```json
[
  {
    "index": 0,
    "entry": "TEXT_INFO",
    "variable_index": 2,
    "value": "カメラのスピード　上下"
  },
  {
    "index": 1,
    "entry": "TEXT_INFO",
    "variable_index": 2,
    "value": "カメラのスピード　左右"
  }
]
```

| Field | Description |
|-------|-------------|
| `index` | Global sequence number. Used for matching during extract/update |
| `entry` | Name of the Entry this text belongs to (resolved from CRC32 key table) |
| `variable_index` | Index of this variable within the Entry |
| `value` | The actual text content. Modify this field for translation |

Only modify `value`. Do not change the other fields.

## cfg.bin file format

### Overall structure

```
[Header 16B] [Entries] [String Table] [Key Table] [Footer]
```

### Header (16 bytes, little-endian)

| Offset | Size | Description |
|--------|------|-------------|
| 0x00 | 4B | entries_count - Total number of entries |
| 0x04 | 4B | string_table_offset - Byte offset where String Table begins |
| 0x08 | 4B | string_table_length - Byte size of the String Table |
| 0x0C | 4B | string_table_count - Number of distinct strings |

### Entries (0x10 ~ string_table_offset)

Each Entry consists of:

1. **CRC32** (4B) - CRC32 hash of the entry name
2. **param_count** (1B) - Number of variables
3. **Type descriptor** (variable length) - Variable types encoded as 2 bits each
   - `00` = String, `01` = Int, `02` = Float, `03` = Unknown
   - 4 types per byte, padded with `0xFF` to 4-byte alignment (based on `param_count + 1`)
4. **Variable values** (4B each)
   - String: byte offset into String Table (i32, `-1` for null)
   - Int: i32
   - Float: f32

Entries form a hierarchy. Names ending with `BEGIN`/`BEG`/`START` open a child scope, and names ending with `END` close it. End entries are written as CRC32 + `00 FF FF FF` (4B).

### String Table

A sequence of null-terminated strings. String variables in entries reference this table by byte offset. Aligned to 16 bytes (padded with `0xFF`).

### Key Table

Maps CRC32 hashes to entry names. Used to resolve entry names when parsing.

| Section | Description |
|---------|-------------|
| Header (16B) | key_length, key_count, key_string_offset, key_string_length |
| Key Entries | CRC32 (4B) + string_offset (4B), repeated |
| Key Strings | Null-terminated strings |

Each section is aligned to 16 bytes (padded with `0xFF`).

### Footer

```
01 74 32 62 FE  (magic bytes)
01 XX 00 01     (XX: encoding flag, 0x00=SHIFT-JIS, 0x01=UTF-8)
00              (separator)
FF...           (padding to 16-byte alignment)
```

### CRC32

- Polynomial: `0xedb88320`
- Seed: `0xffffffff`
- Standard CRC32 algorithm (bitwise NOT applied to final result)

### Byte order

All integers are **little-endian**.

## How extract/update works

### Extract (-e)

1. Parse the cfg.bin binary: read Header, Entries, String Table, and Key Table
2. Walk all entries recursively, collecting variables of type `String`
3. Assign a global index to each text field and output as a JSON array

### Update (-w)

1. Parse the original cfg.bin
2. Read modified texts from JSON, match by `index`, and replace values in memory
3. Rebuild the entire file:
   - Header (placeholder) → Entries (string offsets recalculated) → String Table (distinct strings only) → Key Table → Footer
   - 16-byte alignment at each section boundary
   - Header is overwritten with correct values at the end
4. Save to the original file (or to `-o` path if specified)

Extracting and immediately updating without modifying any text produces a byte-identical copy of the original (roundtrip guarantee).

## Build

```sh
cargo build --release
```

Output: `target/release/cfg_bin_text_editor`

## Reference

Binary parsing logic ported from [CfgBinEditor](https://github.com/rretrogamerr/CfgBinEditor) (C#).
