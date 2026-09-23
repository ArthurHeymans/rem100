//! Build script to generate chip data
//!
//! This script generates embedded chip data for the web version.
//! It downloads and parses chip definitions from Google Drive.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use tar::Archive;
use xz2::read::XzDecoder;

const CONFIGS_ID: &str = "19jT6kNYV1TE6WNx6lUkgH0TYyKbxXcd4";

fn download_configs() -> io::Result<Vec<u8>> {
    let url = format!(
        "https://drive.google.com/uc?export=download&id={}",
        CONFIGS_ID
    );

    // Try to download using curl
    let output = Command::new("curl").args(["-L", "-o", "-", &url]).output();

    match output {
        Ok(output) if output.status.success() => Ok(output.stdout),
        _ => {
            eprintln!("Warning: Could not download chip configs (curl failed or not available)");
            Err(io::Error::new(io::ErrorKind::NotFound, "curl failed"))
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = env::var("OUT_DIR")?;
    let dest_path = PathBuf::from(&out_dir).join("chip_data.rs");
    let mut f = File::create(&dest_path)?;

    // Try to download and parse chip configs
    let chip_data = match download_configs() {
        Ok(compressed_data) => {
            println!("cargo:warning=Downloaded chip configs, decompressing...");

            // Decompress xz
            let mut decoder = XzDecoder::new(&compressed_data[..]);
            let mut tar_data = Vec::new();
            decoder.read_to_end(&mut tar_data)?;

            // Extract tar
            let mut archive = Archive::new(&tar_data[..]);
            let mut chips = Vec::new();

            for entry in archive.entries()? {
                let mut entry = entry?;
                let path = entry.path()?;

                if path.extension().and_then(|s| s.to_str()) == Some("cfg") {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;

                    if data.len() >= 176 && data.starts_with(b"Dcfg") {
                        chips.push(data);
                    }
                }
            }

            println!("cargo:warning=Embedded {} chip configurations", chips.len());
            chips
        }
        Err(_) => {
            println!(
                "cargo:warning=Generated empty chip data (build environment issue prevents download)"
            );
            Vec::new()
        }
    };

    // Generate the embedded data
    writeln!(f, "// Generated chip definitions")?;
    writeln!(f, "// Automatically generated at build time")?;
    writeln!(f)?;
    writeln!(f, "// Embedded chip configuration data")?;
    writeln!(f, "const EMBEDDED_CHIP_CONFIGS: &[&[u8]] = &[")?;

    for data in &chip_data {
        write!(f, "    &[")?;
        for (i, byte) in data.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            if i % 16 == 0 {
                writeln!(f)?;
                write!(f, "        ")?;
            }
            write!(f, "{:#04x}", byte)?;
        }
        writeln!(f)?;
        writeln!(f, "    ],")?;
    }

    writeln!(f, "];")?;

    Ok(())
}
