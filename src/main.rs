mod cfgbin;
mod crc32;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use cfgbin::{CfgBin, TextEntry};

#[derive(Parser)]
#[command(name = "cfg_bin_text_editor")]
#[command(about = "Extract and update text fields in Level-5 cfg.bin files")]
struct Cli {
    /// Extract text fields to JSON
    #[arg(short = 'e', value_name = "CFG_BIN_FILE", conflicts_with_all = ["write_file", "json_file"])]
    extract_file: Option<PathBuf>,

    /// Write updated text fields back to cfg.bin
    #[arg(short = 'w', value_name = "CFG_BIN_FILE", requires = "json_file")]
    write_file: Option<PathBuf>,

    /// JSON file with updated text fields (used with -w)
    #[arg(value_name = "JSON_FILE")]
    json_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cfg_path) = cli.extract_file {
        extract(&cfg_path)?;
    } else if let Some(cfg_path) = cli.write_file {
        let json_path = cli.json_file.unwrap();
        update(&cfg_path, &json_path)?;
    } else {
        eprintln!("Usage:");
        eprintln!("  Extract: cfg_bin_text_editor -e <file.cfg.bin>");
        eprintln!("  Update:  cfg_bin_text_editor -w <file.cfg.bin> <file.cfg.bin.json>");
        std::process::exit(1);
    }

    Ok(())
}

fn extract(cfg_path: &PathBuf) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;

    let texts = cfg.extract_texts();
    let json = serde_json::to_string_pretty(&texts).context("Failed to serialize to JSON")?;

    let json_path = format!("{}.json", cfg_path.display());
    fs::write(&json_path, &json).context("Failed to write JSON file")?;

    println!("Extracted {} text entries to {}", texts.len(), json_path);
    Ok(())
}

fn update(cfg_path: &PathBuf, json_path: &PathBuf) -> Result<()> {
    let data = fs::read(cfg_path).context("Failed to read cfg.bin file")?;
    let mut cfg = CfgBin::open(&data).context("Failed to parse cfg.bin file")?;

    let json_data = fs::read_to_string(json_path).context("Failed to read JSON file")?;
    let texts: Vec<TextEntry> =
        serde_json::from_str(&json_data).context("Failed to parse JSON file")?;

    cfg.update_texts(&texts);

    let output = cfg.save();

    // Write to *_updated.cfg.bin to preserve the original
    let stem = cfg_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    // Handle double extensions like "a.cfg.bin" -> "a_updated.cfg.bin"
    let out_name = if stem.ends_with(".cfg") {
        let base = &stem[..stem.len() - 4];
        format!("{}_updated.cfg.bin", base)
    } else {
        format!("{}_updated.cfg.bin", stem)
    };
    let out_path = cfg_path.with_file_name(&out_name);

    fs::write(&out_path, &output).context("Failed to write updated cfg.bin file")?;

    println!("Written {} ({} text entries)", out_path.display(), texts.len());
    Ok(())
}
