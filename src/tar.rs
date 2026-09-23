//! Archive handling for the downloaded chip and firmware databases.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

/// Files extracted from a compressed tar archive.
pub struct TarFile {
    entries: HashMap<String, Vec<u8>>,
}

impl TarFile {
    /// Load and decompress a .tar.xz file, validating each entry with `tar`.
    pub fn load_compressed(filename: &std::path::Path) -> Result<Self> {
        let file = File::open(filename)
            .map_err(|e| Error::FileNotFound(format!("{}: {}", filename.display(), e)))?;
        let mut archive = tar::Archive::new(xz2::read::XzDecoder::new(file));
        let mut entries = HashMap::new();
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let name = entry.path()?.to_string_lossy().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            entries.insert(name, data);
        }
        Ok(Self { entries })
    }

    /// Find a file in the archive (case-insensitive fallback for legacy data).
    pub fn find(&self, name: &str) -> Result<Vec<u8>> {
        self.entries
            .get(name)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(entry, _)| entry.eq_ignore_ascii_case(name))
                    .map(|(_, data)| data)
            })
            .cloned()
            .ok_or_else(|| Error::FileNotFound(format!("File '{name}' not found in archive")))
    }

    /// Get list of entries.
    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Iterate over all files.
    pub fn for_each<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(&str, &[u8]) -> Result<bool>,
    {
        for (name, data) in &self.entries {
            if f(name, data)? {
                break;
            }
        }
        Ok(())
    }
}
