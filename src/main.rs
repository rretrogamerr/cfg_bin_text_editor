mod cfgbin;
mod crc32;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};

use cfgbin::{CfgBin, TextEntry};

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum Mode {
    Standard,
    Nnk,
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

    /// JSON file with updated text fields (used with -w)
    #[arg(value_name = "JSON_FILE")]
    json_file: Option<PathBuf>,

    /// Output file path (used with -w, defaults to overwriting the original)
    #[arg(short = 'o', value_name = "OUTPUT_FILE")]
    output_file: Option<PathBuf>,

    /// Processing mode: standard(index-based rebuild) or nnk(address-based in-place patch)
    #[arg(long, value_enum, default_value_t = Mode::Standard)]
    mode: Mode,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cfg_path) = cli.extract_file {
        extract(&cfg_path, cli.mode)?;
    } else if let Some(cfg_path) = cli.write_file {
        let json_path = cli.json_file.unwrap();
        let out_path = cli.output_file.unwrap_or_else(|| cfg_path.clone());
        update(&cfg_path, &json_path, &out_path, cli.mode)?;
    } else {
        eprintln!("Usage:");
        eprintln!("  Extract: cfg_bin_text_editor -e <file.cfg.bin>");
        eprintln!("  Update:  cfg_bin_text_editor -w <file.cfg.bin> <file.cfg.bin.json>");
        eprintln!("  Update:  cfg_bin_text_editor -w <file.cfg.bin> <file.cfg.bin.json> -o <output.cfg.bin>");
        eprintln!("  Mode:    --mode standard|nnk");
        std::process::exit(1);
    }

    Ok(())
}

fn extract(cfg_path: &PathBuf, mode: Mode) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let json = match mode {
        Mode::Standard => {
            let cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let texts = cfg.extract_texts();
            serde_json::to_string_pretty(&texts).context("Failed to serialize to JSON")?
        }
        Mode::Nnk => {
            let texts = CfgBin::extract_texts_by_address_for_json(&data)
                .context("Failed to parse cfg.bin file in nnk mode")?;
            serde_json::to_string_pretty(&texts).context("Failed to serialize to JSON")?
        }
    };

    let json_path = format!("{}.json", cfg_path.display());
    fs::write(&json_path, &json).context("Failed to write JSON file")?;

    println!("Extracted text entries to {}", json_path);
    Ok(())
}

fn update(cfg_path: &PathBuf, json_path: &PathBuf, out_path: &PathBuf, mode: Mode) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let json_data = fs::read_to_string(json_path).context("Failed to read JSON file")?;
    let output = match mode {
        Mode::Standard => {
            let mut cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;
            let texts: Vec<TextEntry> =
                serde_json::from_str(&json_data).context("Failed to parse JSON file")?;
            let text_count = texts.len();
            cfg.update_texts(&texts);
            let output = cfg.save();
            println!(
                "Written {} ({} text entries, mode=standard)",
                out_path.display(),
                text_count
            );
            output
        }
        Mode::Nnk => {
            let texts = CfgBin::parse_address_texts_json(&json_data)
                .context("Failed to parse address-based JSON for nnk mode")?;
            let text_count = texts.len();
            let output = CfgBin::patch_texts_by_address_in_place(&data, &texts)
                .context("Failed to patch cfg.bin in nnk mode")?;
            println!(
                "Written {} ({} text entries, mode=nnk)",
                out_path.display(),
                text_count
            );
            output
        }
    };
    fs::write(out_path, &output).context("Failed to write cfg.bin file")?;
    Ok(())
}
