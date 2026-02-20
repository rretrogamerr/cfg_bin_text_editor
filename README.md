# cfg_bin_text_editor

A Rust CLI tool that extracts text fields from Level-5 game engine `cfg.bin` files to JSON/TXT, and writes modified data back into `cfg.bin` files.

It supports two workflows:

- `standard` mode: index-based full rebuild
- `nnk` mode: address-based in-place string-table patching (Ni no Kuni workflow)

## Usage

### Extract

```sh
cfg_bin_text_editor -e <file.cfg.bin> [--mode standard|nnk] [--extract-format json|txt]
```

Default is `--mode standard --extract-format json`.

Examples:

```sh
# Standard JSON
cfg_bin_text_editor -e file.cfg.bin

# NNK JSON (address-keyed)
cfg_bin_text_editor -e file.cfg.bin --mode nnk

# NNK TXT (line-by-line)
cfg_bin_text_editor -e file.cfg.bin --mode nnk --extract-format txt
```

### Update

```sh
cfg_bin_text_editor -w <file.cfg.bin> <input.json|input.txt> [--mode standard|nnk] [--update-format json|txt] [-o <output.cfg.bin>]
```

Default is `--mode standard --update-format json`. Without `-o`, the original file is overwritten.

Examples:

```sh
# Standard JSON update
cfg_bin_text_editor -w file.cfg.bin file.cfg.bin.json

# NNK JSON update
cfg_bin_text_editor -w file.cfg.bin file.cfg.bin.json --mode nnk

# NNK TXT update
cfg_bin_text_editor -w file.cfg.bin file.cfg.bin.txt --mode nnk --update-format txt
```

### Bulk operations (Windows)

`cbte_bulk.bat` (standard mode, JSON input/output):

```bat
cbte_bulk.bat -e <folder>
cbte_bulk.bat -w <folder>
```

`cbte_bulk_nnk.bat` (NNK mode, TXT input/output):

```bat
cbte_bulk_nnk.bat -e <folder>
cbte_bulk_nnk.bat -w <folder>
```

Both scripts process folders recursively. Requires `cfg_bin_text_editor.exe` in the same directory or in PATH.

## Data formats

### Standard JSON format (`--mode standard --extract-format json`)

The extracted JSON is an array where each item represents one text field:

```json
[
  {
    "index": 0,
    "entry": "TEXT_INFO",
    "variable_index": 2,
    "value": "カメラのスピード　上下"
  }
]
```

| Field | Description |
|-------|-------------|
| `index` | Global sequence number |
| `entry` | Entry name |
| `variable_index` | Variable index inside the entry |
| `value` | Text content |

### NNK JSON format (`--mode nnk --extract-format json`)

The extracted JSON is an object keyed by absolute address of each string-offset field:

```json
{
  "0x00000018": "text 1",
  "0x0000001C": "text 2"
}
```

### TXT format (`--extract-format txt` / `--update-format txt`)

One text entry per line.

- Use `\n` and `\r` escapes for embedded line breaks inside a single entry.
- Backslashes are escaped as `\\`.
- During update, line count must match the number of text entries, otherwise update fails.
- Special case for some Japanese NNK files:
  - If the first original text line is a timestamp in `YYYY/MM/DD HH:MM:SS` format, update also accepts `expected - 3` lines.
  - In that case, the first three original metadata lines are preserved and TXT line 1 is applied to cfg.bin line 4.

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
| 0x0C | 4B | string_table_count - Number of strings (layout-dependent) |

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
u32 magic (0x62327401)
u16 unk1  (0x01FE)
u16 encoding (0=SHIFT-JIS, non-zero=UTF-8 variant)
u16 unk2  (1)
FF...     (padding to 16-byte alignment)
```

### CRC32

- Polynomial: `0xedb88320`
- Seed: `0xffffffff`
- Standard CRC32 algorithm (bitwise NOT applied to final result)

### Byte order

All integers are **little-endian**.

## How extract/update works

### Standard mode (`--mode standard`)

1. Parse the cfg.bin binary: read Header, Entries, String Table, and Key Table
2. Collect all string variables in traversal order
3. Extract to JSON array or TXT lines
4. Update from JSON/TXT and rebuild:
   - Header (placeholder) → Entries (string offsets recalculated) → String Table (distinct strings only) → Key Table → Footer
5. Save to output path (or overwrite original)

### NNK mode (`--mode nnk`)

1. Parse entries and collect string-offset field addresses in file order
2. Extract:
   - JSON: address-keyed object (`0xADDR -> text`)
   - TXT: values only, line-by-line in address order
3. Update:
   - JSON: strict key/count match required
   - TXT: line-count match required, with one exception:
     - if original line 1 is `YYYY/MM/DD HH:MM:SS`, `expected - 3` lines are also accepted and mapped from line 4
4. Rebuild only string table region, patch string offsets in place, preserve entry/key/footer structure

NNK mode is designed for compatibility with Ni no Kuni text workflows. Binary output may differ from source bytes while preserving text mapping.

## NNK mode version history (v0.4.0+)

- `v0.4.0`
  - Improved compatibility for string-table suffix offsets and footer encoding variants.
- `v0.5.0`
  - Added initial `nnk` mode (address-based workflow for Ni no Kuni).
- `v0.6.0`
  - Added TXT extraction support and NNK TXT bulk workflow.
- `v0.7.0`
  - Added stricter update validation in NNK mode.

## Build

```sh
cargo build --release
```

Output: `target/release/cfg_bin_text_editor`

## Reference

Binary parsing logic ported from [CfgBinEditor](https://github.com/rretrogamerr/CfgBinEditor) (C#).
