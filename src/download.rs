//! Network download functionality

use crate::chips::get_em100_file;
use crate::error::{Error, Result};
use std::fs::File;
use std::io::{Read, Write};

/// Google Drive file IDs for updates
const FIRMWARE_ID: &str = "1UmzGZbRkF9duwTLPi467EyfIZ6EhnMKA";
const FIRMWARE_NAME: &str = "firmware.tar.xz";

const CONFIGS_ID: &str = "19jT6kNYV1TE6WNx6lUkgH0TYyKbxXcd4";
const CONFIGS_NAME: &str = "configs.tar.xz";

const VERSION_ID: &str = "1YC755W_c4nRN4qVgosegFrvfyWllqb0b";
const VERSION_NAME: &str = "VERSION";

/// Download a file from Google Drive
fn download_from_drive(id: &str, filename: &std::path::Path) -> Result<()> {
    let url = format!("https://drive.google.com/uc?export=download&id={}", id);

    let client = reqwest::blocking::Client::builder()
        .user_agent("em100-agent/1.0")
        .build()
        .map_err(|e| Error::Network(e.to_string()))?;

    let response = client
        .get(&url)
        .send()
        .map_err(|e| Error::Network(e.to_string()))?;

    if !response.status().is_success() {
        return Err(Error::Network(format!("HTTP error: {}", response.status())));
    }

    let bytes = response
        .bytes()
        .map_err(|e| Error::Network(e.to_string()))?;

    let mut file = File::create(filename)?;
    file.write_all(&bytes)?;

    Ok(())
}

/// Download a named file
fn download(name: &str, id: &str) -> Result<()> {
    let filename = get_em100_file(name)?;
    print!("Downloading {}: ", name);
    std::io::stdout().flush().ok();

    match download_from_drive(id, &filename) {
        Ok(_) => {
            println!("OK");
            Ok(())
        }
        Err(e) => {
            println!("FAILED.");
            Err(e)
        }
    }
}

/// Version information
struct VersionInfo {
    time: i64,
    version: String,
}

fn parse_version(content: &str) -> Option<VersionInfo> {
    let mut time = 0i64;
    let mut version = String::new();

    for line in content.lines() {
        if let Some(t) = line.strip_prefix("Time: ") {
            time = t.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Version: ") {
            version = v.trim().to_string();
        }
    }

    if !version.is_empty() {
        Some(VersionInfo { time, version })
    } else {
        None
    }
}

/// Update all configuration and firmware files
pub fn update_all_files() -> Result<()> {
    // Read existing version
    let version_path = get_em100_file(VERSION_NAME)?;
    let old_version = if version_path.exists() {
        let mut file = File::open(&version_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        parse_version(&content)
    } else {
        None
    };

    // Download and check upstream version
    let tmp_version_path = get_em100_file(".VERSION.new")?;
    download_from_drive(VERSION_ID, &tmp_version_path)?;

    let new_version = {
        let mut file = File::open(&tmp_version_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        parse_version(&content)
    };

    // Clean up temp file
    std::fs::remove_file(&tmp_version_path).ok();

    let new_version =
        new_version.ok_or_else(|| Error::Parse("Parse error in upstream VERSION.".to_string()))?;

    // Compare timestamps
    if let Some(old) = &old_version {
        if old.time >= new_version.time {
            println!(
                "Current version: {}. No newer version available.",
                old.version
            );
            return Ok(());
        }
        println!(
            "Update available: {} (installed: {})",
            new_version.version, old.version
        );
    } else {
        println!("Downloading latest version: {}", new_version.version);
    }

    // Download everything
    download(CONFIGS_NAME, CONFIGS_ID)?;
    download(FIRMWARE_NAME, FIRMWARE_ID)?;
    download(VERSION_NAME, VERSION_ID)?;

    Ok(())
}
