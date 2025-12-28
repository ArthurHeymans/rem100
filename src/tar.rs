//! Tar archive handling

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

/// Tar file structure
pub struct TarFile {
    data: Vec<u8>,
    entries: HashMap<String, (usize, usize)>, // name -> (offset, size)
}

impl TarFile {
    /// Load and decompress a .tar.xz file
    pub fn load_compressed(filename: &std::path::Path) -> Result<Self> {
        let mut file = File::open(filename).map_err(|e| {
            Error::FileNotFound(format!("{}: {}", filename.display(), e))
        })?;

        let mut compressed = Vec::new();
        file.read_to_end(&mut compressed)?;

        // Decompress XZ
        let mut decompressor = xz2::read::XzDecoder::new(&compressed[..]);
        let mut data = Vec::new();
        decompressor.read_to_end(&mut data).map_err(|e| {
            Error::Decompression(format!("XZ decompression failed: {}", e))
        })?;

        // Parse tar entries
        let entries = parse_tar_entries(&data)?;

        Ok(Self { data, entries })
    }

    /// Find a file in the archive
    pub fn find(&self, name: &str) -> Result<Vec<u8>> {
        // Try exact match first
        if let Some(&(offset, size)) = self.entries.get(name) {
            return Ok(self.data[offset..offset + size].to_vec());
        }

        // Try case-insensitive match
        let name_lower = name.to_lowercase();
        for (entry_name, &(offset, size)) in &self.entries {
            if entry_name.to_lowercase() == name_lower {
                return Ok(self.data[offset..offset + size].to_vec());
            }
        }

        Err(Error::FileNotFound(format!(
            "File '{}' not found in archive",
            name
        )))
    }

    /// Get list of entries
    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|s| s.as_str())
    }

    /// Iterate over all files
    pub fn for_each<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&str, &[u8]) -> Result<bool>,
    {
        for (name, &(offset, size)) in &self.entries {
            let data = &self.data[offset..offset + size];
            if f(name, data)? {
                break;
            }
        }
        Ok(())
    }
}

/// Tar header structure
#[repr(C)]
#[allow(dead_code)]
struct TarHeader {
    name: [u8; 100],
    mode: [u8; 8],
    owner: [u8; 8],
    group: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typeflag: u8,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    padding: [u8; 12],
}

const TAR_HEADER_SIZE: usize = 512;

fn parse_tar_entries(data: &[u8]) -> Result<HashMap<String, (usize, usize)>> {
    let mut entries = HashMap::new();
    let mut pos = 0;

    while pos + TAR_HEADER_SIZE <= data.len() {
        // Check for null header (end of archive)
        if data[pos..pos + TAR_HEADER_SIZE].iter().all(|&b| b == 0) {
            break;
        }

        // Parse header
        let name_end = data[pos..pos + 100]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(100);
        let name = String::from_utf8_lossy(&data[pos..pos + name_end]).to_string();

        // Parse size (octal)
        let size_str = String::from_utf8_lossy(&data[pos + 124..pos + 136]);
        let size = usize::from_str_radix(size_str.trim().trim_end_matches('\0'), 8)
            .unwrap_or(0);

        // Verify checksum
        let stored_checksum_str = String::from_utf8_lossy(&data[pos + 148..pos + 156]);
        let stored_checksum = u32::from_str_radix(
            stored_checksum_str.trim().trim_end_matches('\0'),
            8,
        )
        .unwrap_or(0);
        let computed_checksum = compute_checksum(&data[pos..pos + TAR_HEADER_SIZE]);

        if stored_checksum != computed_checksum {
            break;
        }

        // Type flag
        let typeflag = data[pos + 156];

        // Only process regular files ('0' or null)
        if typeflag == b'0' || typeflag == 0 {
            let data_offset = pos + TAR_HEADER_SIZE;
            entries.insert(name, (data_offset, size));
        }

        // Advance to next header (size rounded up to 512 bytes)
        let padded_size = (size + 511) & !511;
        pos += TAR_HEADER_SIZE + padded_size;
    }

    Ok(entries)
}

fn compute_checksum(header: &[u8]) -> u32 {
    let mut sum: u32 = 256; // Checksum field treated as spaces

    for (i, &byte) in header.iter().enumerate() {
        if i >= 148 && i < 156 {
            // Skip checksum field
            continue;
        }
        sum += byte as u32;
    }

    sum
}
