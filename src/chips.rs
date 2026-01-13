//! Chip configuration parsing

use crate::error::{Error, Result};
#[cfg(feature = "cli")]
use crate::tar::TarFile;
use byteorder::{ByteOrder, LittleEndian};

/// Number of init entries in chip configuration
pub const NUM_INIT_ENTRIES: usize = 212;
/// Bytes per init entry
pub const BYTES_PER_INIT_ENTRY: usize = 4;

/// Chip description
#[derive(Debug, Clone)]
pub struct ChipDesc {
    /// Vendor name
    pub vendor: String,
    /// Chip name
    pub name: String,
    /// Chip size in bytes
    pub size: u32,
    /// Initialization sequence
    pub init: [[u8; BYTES_PER_INIT_ENTRY]; NUM_INIT_ENTRIES],
    /// Number of valid init entries
    pub init_len: usize,
}

impl Default for ChipDesc {
    fn default() -> Self {
        Self {
            vendor: String::new(),
            name: String::new(),
            size: 0,
            init: [[0u8; BYTES_PER_INIT_ENTRY]; NUM_INIT_ENTRIES],
            init_len: 0,
        }
    }
}

// Dediprog configuration file constants
const DEDIPROG_CFG_PRO_SIZE: usize = 176;
const DEDIPROG_CFG_PRO_SIZE_SFDP: usize = 256;
const DEDIPROG_CFG_PRO_SIZE_SRST: usize = 144;

const DEDIPROG_CFG_MAGIC: u32 = 0x67666344; // 'Dcfg'
const DEDIPROG_SFDP_MAGIC: u32 = 0x50444653; // 'SFDP'
const DEDIPROG_SRST_MAGIC: u32 = 0x54535253; // 'SRST'
const DEDIPROG_PROT_MAGIC: u32 = 0x544f5250; // 'PROT'

const INIT_SEQUENCE_REGISTER_OFFSET_0: u16 = 0x2300;
const INIT_SEQUENCE_REGISTER_OFFSET_1: u16 = 0x1100;

/// Parse a Dediprog chip configuration file
pub fn parse_dcfg(data: &[u8]) -> Result<ChipDesc> {
    if data.len() < DEDIPROG_CFG_PRO_SIZE {
        return Err(Error::InvalidConfig("File too small".to_string()));
    }

    let mut chip = ChipDesc::default();
    let mut init_len = 0;

    // Parse header
    let magic = LittleEndian::read_u32(&data[0..4]);
    if magic != DEDIPROG_CFG_MAGIC {
        return Err(Error::InvalidConfig(format!(
            "Invalid magic number: 0x{:x}",
            magic
        )));
    }

    let ver_min = LittleEndian::read_u16(&data[4..6]);
    let ver_maj = LittleEndian::read_u16(&data[6..8]);
    if ver_maj != 1 || ver_min != 1 {
        return Err(Error::InvalidConfig(format!(
            "Invalid version: {}.{}",
            ver_maj, ver_min
        )));
    }

    let init_offset = LittleEndian::read_u32(&data[8..12]) as usize;
    chip.size = LittleEndian::read_u32(&data[12..16]);
    let vendor_offset = LittleEndian::read_u32(&data[16..20]) as usize;
    let chip_name_offset = LittleEndian::read_u32(&data[20..24]) as usize;

    // Read vendor and chip name as null-terminated strings
    if vendor_offset < data.len() {
        let end = data[vendor_offset..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len() - vendor_offset);
        chip.vendor =
            String::from_utf8_lossy(&data[vendor_offset..vendor_offset + end]).to_string();
    }

    if chip_name_offset < data.len() {
        let end = data[chip_name_offset..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len() - chip_name_offset);
        chip.name =
            String::from_utf8_lossy(&data[chip_name_offset..chip_name_offset + end]).to_string();
    }

    // Parse init sequence
    let mut reg_offset = INIT_SEQUENCE_REGISTER_OFFSET_0;
    let mut pos = init_offset;

    while pos + 4 <= DEDIPROG_CFG_PRO_SIZE && init_len < NUM_INIT_ENTRIES {
        let value = LittleEndian::read_u16(&data[pos..pos + 2]);
        let reg = LittleEndian::read_u16(&data[pos + 2..pos + 4]);

        if value == 0xffff && reg == 0xffff {
            reg_offset = INIT_SEQUENCE_REGISTER_OFFSET_1;
            pos += 4;
            continue;
        }

        let full_reg = reg + reg_offset;

        // Convert to big endian for output
        let be_value = value.to_be_bytes();
        let be_reg = full_reg.to_be_bytes();

        chip.init[init_len][0] = be_reg[0];
        chip.init[init_len][1] = be_reg[1];
        chip.init[init_len][2] = be_value[0];
        chip.init[init_len][3] = be_value[1];
        init_len += 1;
        pos += 4;
    }

    // Handle extra data (SFDP, SRST)
    let mut ptr = DEDIPROG_CFG_PRO_SIZE;
    let mut length = data.len() - DEDIPROG_CFG_PRO_SIZE;

    while length >= 4 {
        let magic = LittleEndian::read_u32(&data[ptr..ptr + 4]);
        ptr += 4;
        length -= 4;

        match magic {
            DEDIPROG_SFDP_MAGIC => {
                let added = parse_sfdp(&data[ptr..], &mut chip, init_len)?;
                init_len += added;
                ptr += DEDIPROG_CFG_PRO_SIZE_SFDP;
                length = length.saturating_sub(DEDIPROG_CFG_PRO_SIZE_SFDP);
            }
            DEDIPROG_SRST_MAGIC => {
                let added = parse_srst(&data[ptr..], &mut chip, init_len)?;
                init_len += added;
                ptr += DEDIPROG_CFG_PRO_SIZE_SRST;
                length = length.saturating_sub(DEDIPROG_CFG_PRO_SIZE_SRST);
            }
            _ => {
                // Unknown section, skip
                break;
            }
        }
    }

    chip.init_len = init_len;
    Ok(chip)
}

fn parse_sfdp(data: &[u8], chip: &mut ChipDesc, entries: usize) -> Result<usize> {
    if data.len() < DEDIPROG_CFG_PRO_SIZE_SFDP {
        return Err(Error::InvalidConfig("SFDP data too small".to_string()));
    }

    let mut len = 0;
    let mut init_len = entries;

    // Enable SFDP
    chip.init[init_len][0] = 0x23;
    chip.init[init_len][1] = 0xc9;
    chip.init[init_len][2] = 0x00;
    chip.init[init_len][3] = 0x01;
    init_len += 1;
    len += 1;

    for i in (0..DEDIPROG_CFG_PRO_SIZE_SFDP).step_by(2) {
        if init_len >= NUM_INIT_ENTRIES {
            break;
        }
        chip.init[init_len][0] = 0x23;
        chip.init[init_len][1] = 0xc1;
        chip.init[init_len][2] = data[i + 1];
        chip.init[init_len][3] = data[i];
        init_len += 1;
        len += 1;
    }

    Ok(len)
}

fn parse_srst(data: &[u8], chip: &mut ChipDesc, entries: usize) -> Result<usize> {
    if data.len() < DEDIPROG_CFG_PRO_SIZE_SRST {
        return Err(Error::InvalidConfig("SRST data too small".to_string()));
    }

    let mut len = 0;
    let mut init_len = entries;

    // Check for PROT magic at start
    let magic = LittleEndian::read_u32(&data[0..4]);
    let start_offset = if magic != DEDIPROG_PROT_MAGIC {
        // 3 SRST entries before PROT
        for j in 0..3 {
            if init_len >= NUM_INIT_ENTRIES {
                break;
            }
            chip.init[init_len][0] = 0x23;
            chip.init[init_len][1] = data[j * 4 + 2];
            chip.init[init_len][2] = data[j * 4 + 1];
            chip.init[init_len][3] = data[j * 4];
            init_len += 1;
            len += 1;
        }
        16 // Skip SFDP data and PROT magic
    } else {
        4 // Start after PROT magic
    };

    // Enable PROT
    if init_len < NUM_INIT_ENTRIES {
        chip.init[init_len][0] = 0x23;
        chip.init[init_len][1] = 0xc4;
        chip.init[init_len][2] = 0x00;
        chip.init[init_len][3] = 0x01;
        init_len += 1;
        len += 1;
    }

    for i in (start_offset..DEDIPROG_CFG_PRO_SIZE_SRST).step_by(2) {
        if init_len >= NUM_INIT_ENTRIES {
            break;
        }
        chip.init[init_len][0] = 0x23;
        chip.init[init_len][1] = 0xc5;
        chip.init[init_len][2] = data[i + 1];
        chip.init[init_len][3] = data[i];
        init_len += 1;
        len += 1;
    }

    Ok(len)
}

/// Chip configuration database (CLI version with file loading)
#[cfg(feature = "cli")]
pub struct ChipDatabase {
    pub configs: TarFile,
    pub version: String,
}

#[cfg(feature = "cli")]
impl ChipDatabase {
    /// Load chip database from configs.tar.xz
    pub fn load() -> Result<Self> {
        let config_path = get_em100_file("configs.tar.xz")?;
        let configs = TarFile::load_compressed(&config_path)?;

        // Read version
        let version_data = configs.find("configs/VERSION")?;
        let version = String::from_utf8_lossy(&version_data).trim().to_string();

        Ok(Self { configs, version })
    }

    /// Find a chip by name
    pub fn find_chip(&self, name: &str) -> Result<ChipDesc> {
        let cfg_name = format!("configs/{}.cfg", name);
        let data = self
            .configs
            .find(&cfg_name)
            .map_err(|_| Error::InvalidChip(format!("Could not find chip '{}'", name)))?;
        parse_dcfg(&data)
    }

    /// List all available chips
    pub fn list_chips(&self) -> Vec<ChipDesc> {
        let mut chips = Vec::new();
        for entry in self.configs.entries() {
            if entry.ends_with(".cfg") {
                if let Ok(data) = self.configs.find(entry) {
                    if let Ok(chip) = parse_dcfg(&data) {
                        chips.push(chip);
                    }
                }
            }
        }
        chips
    }
}

/// In-memory chip database (for web)
#[cfg(not(feature = "cli"))]
pub struct ChipDatabase {
    pub chips: Vec<ChipDesc>,
    pub version: String,
}

#[cfg(not(feature = "cli"))]
impl ChipDatabase {
    /// Create an empty chip database
    ///
    /// For now, returns an empty database. In the future, we could embed
    /// common chip configs using include_bytes!().
    pub fn load_embedded() -> Self {
        Self {
            chips: Vec::new(),
            version: "embedded".to_string(),
        }
    }

    /// Create chip database from in-memory data
    pub fn from_data(chip_configs: Vec<(&str, &[u8])>, version: String) -> Result<Self> {
        let mut chips = Vec::new();
        for (_name, data) in chip_configs {
            if let Ok(chip) = parse_dcfg(data) {
                chips.push(chip);
            }
        }
        Ok(Self { chips, version })
    }

    /// Find a chip by name
    pub fn find_chip(&self, name: &str) -> Result<ChipDesc> {
        self.chips
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| Error::InvalidChip(format!("Could not find chip '{}'", name)))
    }

    /// List all available chips
    pub fn list_chips(&self) -> Vec<ChipDesc> {
        self.chips.clone()
    }
}

/// Get path to EM100 configuration file
#[cfg(feature = "cli")]
pub fn get_em100_file(name: &str) -> Result<std::path::PathBuf> {
    let base = if let Ok(home) = std::env::var("EM100_HOME") {
        std::path::PathBuf::from(home)
    } else if let Some(home) = dirs::home_dir() {
        home.join(".em100")
    } else {
        return Err(Error::FileNotFound(
            "Could not determine home directory".to_string(),
        ));
    };

    // Create directory if it doesn't exist
    if !base.exists() {
        std::fs::create_dir_all(&base)?;
    }

    Ok(base.join(name))
}
