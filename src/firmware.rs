//! Firmware update/dump operations

#[cfg(feature = "cli")]
use crate::chips::get_em100_file;
use crate::device::{Em100, HwVersion};
use crate::error::{Error, Result};
use crate::spi;
#[cfg(feature = "cli")]
use crate::tar::TarFile;
use byteorder::{ByteOrder, LittleEndian};
#[cfg(feature = "cli")]
use indicatif::{ProgressBar, ProgressStyle};
#[cfg(feature = "cli")]
use std::fs::File;
#[cfg(feature = "cli")]
use std::io::{Read, Write};

/// Size constants
const MB: usize = 1024 * 1024;

fn get_le32(data: &[u8]) -> u32 {
    LittleEndian::read_u32(data)
}

fn put_le32(data: &mut [u8], val: u32) {
    LittleEndian::write_u32(data, val);
}

/// Progress callback type for reporting firmware operations
pub type FirmwareProgressCallback<'a> = Option<&'a mut dyn FnMut(usize, usize, &str)>;

/// Read firmware from device into memory
pub fn firmware_read(em100: &Em100, mut progress: FirmwareProgressCallback) -> Result<Vec<u8>> {
    let id = spi::get_spi_flash_id(em100)?;
    let rom_size = match id {
        0x202015 => 2 * MB,  // M25P16
        0xc27518 => 16 * MB, // MX77L12850F
        _ => {
            return Err(Error::InvalidFirmware(format!(
                "Unknown SPI flash id = {:06x}. Please report",
                id
            )));
        }
    };

    let mut data = vec![0u8; rom_size];

    for i in (0..rom_size).step_by(256) {
        // Retry up to 3 times
        for retry in 0..3 {
            if spi::read_spi_flash_page(em100, i as u32, &mut data[i..i + 256]).is_ok() {
                break;
            }
            if retry == 2 {
                return Err(Error::Communication(format!("Couldn't read @{:08x}", i)));
            }
        }
        if let Some(ref mut cb) = progress {
            cb(i + 256, rom_size, "Reading");
        }
    }

    Ok(data)
}

/// Convert raw firmware data to DPFW format
pub fn firmware_to_dpfw(em100: &Em100, data: &[u8]) -> Result<Vec<u8>> {
    let hdr_version = match em100.hw_version {
        HwVersion::Em100ProEarly | HwVersion::Em100Pro => 1,
        HwVersion::Em100ProG2 => 2,
        _ => {
            return Err(Error::UnsupportedHardware(em100.hw_version as u8));
        }
    };

    // Find FPGA firmware end
    let all_ff = [0xffu8; 256];
    let mut fpga_size = 0;
    for i in (0..0x100000).step_by(0x100) {
        if data[i..i + 256] == all_ff {
            fpga_size = i;
            break;
        }
    }
    if fpga_size == 0 {
        return Err(Error::InvalidFirmware(
            "Can't parse device firmware. Please extract raw firmware instead.".to_string(),
        ));
    }

    // Find MCU firmware end
    let mut mcu_size = 0;
    for i in (0..0xfff00).step_by(0x100) {
        if data[0x100100 + i..0x100100 + i + 256] == all_ff {
            mcu_size = i;
            break;
        }
    }
    if mcu_size == 0 {
        return Err(Error::InvalidFirmware(
            "Can't parse device firmware. Please extract raw firmware instead.".to_string(),
        ));
    }

    let mcu_version = format!("{}.{}", em100.mcu >> 8, em100.mcu & 0xff);
    let fpga_version = format!("{}.{}", (em100.fpga >> 8) & 0x7f, em100.fpga & 0xff);

    let mut header = [0u8; 0x100];
    match hdr_version {
        1 => header[..8].copy_from_slice(b"em100pro"),
        2 => header[..11].copy_from_slice(b"EM100Pro-G2"),
        _ => {}
    }
    header[0x28..0x2c].copy_from_slice(b"WFPD");
    header[0x14..0x14 + mcu_version.len().min(4)]
        .copy_from_slice(&mcu_version.as_bytes()[..mcu_version.len().min(4)]);
    header[0x1e..0x1e + fpga_version.len().min(4)]
        .copy_from_slice(&fpga_version.as_bytes()[..fpga_version.len().min(4)]);
    put_le32(&mut header[0x38..], 0x100);
    put_le32(&mut header[0x3c..], fpga_size as u32);
    put_le32(&mut header[0x40..], 0x100 + fpga_size as u32);
    put_le32(&mut header[0x44..], mcu_size as u32);

    let mut output = Vec::with_capacity(0x100 + fpga_size + mcu_size);
    output.extend_from_slice(&header);
    output.extend_from_slice(&data[..fpga_size]);
    output.extend_from_slice(&data[0x100100..0x100100 + mcu_size]);

    Ok(output)
}

/// Dump firmware from device to file (CLI version)
#[cfg(feature = "cli")]
pub fn firmware_dump(em100: &Em100, filename: &str, firmware_is_dpfw: bool) -> Result<()> {
    let id = spi::get_spi_flash_id(em100)?;
    let rom_size = match id {
        0x202015 => 2 * MB,
        0xc27518 => 16 * MB,
        _ => {
            return Err(Error::InvalidFirmware(format!(
                "Unknown SPI flash id = {:06x}. Please report",
                id
            )));
        }
    };

    println!("\nWriting EM100Pro firmware to file {}", filename);

    let pb = ProgressBar::new(rom_size as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:50}] {percent}%")
            .unwrap()
            .progress_chars("=> "),
    );

    let data = firmware_read(
        em100,
        Some(&mut |pos, _total, _msg| {
            if pos & 0x7fff == 0 {
                pb.set_position(pos as u64);
            }
        }),
    )?;
    pb.finish();

    let mut file = File::create(filename)?;

    if firmware_is_dpfw {
        let dpfw_data = firmware_to_dpfw(em100, &data)?;
        file.write_all(&dpfw_data)?;
    } else {
        file.write_all(&data)?;
    }

    Ok(())
}

/// Firmware update info
pub struct FirmwareInfo {
    pub mcu_version: String,
    pub fpga_version: String,
    pub fpga_offset: usize,
    pub fpga_len: usize,
    pub mcu_offset: usize,
    pub mcu_len: usize,
}

/// Validate and parse firmware file
pub fn validate_firmware(em100: &Em100, fw: &[u8]) -> Result<FirmwareInfo> {
    // Validate firmware file
    match em100.hw_version {
        HwVersion::Em100ProEarly | HwVersion::Em100Pro => {
            if fw.len() < 0x48 || &fw[..8] != b"em100pro" || &fw[0x28..0x2c] != b"WFPD" {
                return Err(Error::InvalidFirmware(
                    "Not an EM100Pro (original) firmware file.".to_string(),
                ));
            }
        }
        HwVersion::Em100ProG2 => {
            if fw.len() < 0x48 || &fw[..11] != b"EM100Pro-G2" || &fw[0x28..0x2c] != b"WFPD" {
                return Err(Error::InvalidFirmware(
                    "Not an EM100Pro-G2 firmware file.".to_string(),
                ));
            }
        }
        _ => {
            return Err(Error::UnsupportedHardware(em100.hw_version as u8));
        }
    }

    let fpga_offset = get_le32(&fw[0x38..]) as usize;
    let fpga_len = get_le32(&fw[0x3c..]) as usize;
    let mcu_offset = get_le32(&fw[0x40..]) as usize;
    let mcu_len = get_le32(&fw[0x44..]) as usize;

    let mcu_version = String::from_utf8_lossy(&fw[0x14..0x1e])
        .trim_end_matches('\0')
        .to_string();
    let fpga_version = String::from_utf8_lossy(&fw[0x1e..0x28])
        .trim_end_matches('\0')
        .to_string();

    if fpga_len < 256 || mcu_len < 256 || fpga_len > 0x100000 || mcu_len > 0xf0000 {
        return Err(Error::InvalidFirmware(
            "Firmware file not valid.".to_string(),
        ));
    }

    Ok(FirmwareInfo {
        mcu_version,
        fpga_version,
        fpga_offset,
        fpga_len,
        mcu_offset,
        mcu_len,
    })
}

/// Write firmware to device (core function)
pub fn firmware_write(
    em100: &Em100,
    fw: &[u8],
    info: &FirmwareInfo,
    verify: bool,
    mut progress: FirmwareProgressCallback,
) -> Result<()> {
    // Unlock and erase
    spi::unlock_spi_flash(em100)?;
    spi::get_spi_flash_id(em100)?;

    for i in 0..=0x1e {
        spi::erase_spi_flash_sector(em100, i as u8)?;
        if let Some(ref mut cb) = progress {
            cb(i as usize + 1, 0x1f, "Erasing");
        }
    }

    spi::get_spi_flash_id(em100)?;

    let total_len = info.fpga_len + info.mcu_len;
    let mut written = 0;

    // Write FPGA firmware
    let mut page = [0xffu8; 256];
    for i in (0..info.fpga_len).step_by(256) {
        page.fill(0xff);
        let chunk_len = (info.fpga_len - i).min(256);
        page[..chunk_len]
            .copy_from_slice(&fw[info.fpga_offset + i..info.fpga_offset + i + chunk_len]);
        spi::write_spi_flash_page(em100, i as u32, &page)?;
        written += chunk_len;
        if let Some(ref mut cb) = progress {
            cb(written, total_len, "Writing");
        }
    }

    // Write MCU firmware
    for i in (0..info.mcu_len).step_by(256) {
        page.fill(0xff);
        let chunk_len = (info.mcu_len - i).min(256);
        page[..chunk_len]
            .copy_from_slice(&fw[info.mcu_offset + i..info.mcu_offset + i + chunk_len]);
        spi::write_spi_flash_page(em100, (i + 0x100100) as u32, &page)?;
        written += chunk_len;
        if let Some(ref mut cb) = progress {
            cb(written, total_len, "Writing");
        }
    }

    if verify {
        let mut vpage = [0u8; 256];
        let mut verified = 0;

        // Verify FPGA
        for i in (0..info.fpga_len).step_by(256) {
            page.fill(0xff);
            let chunk_len = (info.fpga_len - i).min(256);
            page[..chunk_len]
                .copy_from_slice(&fw[info.fpga_offset + i..info.fpga_offset + i + chunk_len]);
            spi::read_spi_flash_page(em100, i as u32, &mut vpage)?;
            if page != vpage {
                return Err(Error::VerificationFailed);
            }
            verified += chunk_len;
            if let Some(ref mut cb) = progress {
                cb(verified, total_len, "Verifying");
            }
        }

        // Verify MCU
        for i in (0..info.mcu_len).step_by(256) {
            page.fill(0xff);
            let chunk_len = (info.mcu_len - i).min(256);
            page[..chunk_len]
                .copy_from_slice(&fw[info.mcu_offset + i..info.mcu_offset + i + chunk_len]);
            spi::read_spi_flash_page(em100, (i + 0x100100) as u32, &mut vpage)?;
            if page != vpage {
                return Err(Error::VerificationFailed);
            }
            verified += chunk_len;
            if let Some(ref mut cb) = progress {
                cb(verified, total_len, "Verifying");
            }
        }
    }

    // Write magic update tag '.UBOOTU.'
    let mut page = [0u8; 256];
    page[0] = 0xaa;
    page[1] = 0x55;
    page[2] = 0x42; // 'B'
    page[3] = 0x4f; // 'O'
    page[4] = 0x4f; // 'O'
    page[5] = 0x54; // 'T'
    page[6] = 0x55;
    page[7] = 0xaa;
    spi::write_spi_flash_page(em100, 0x100000, &page)?;

    if verify {
        let mut vpage = [0u8; 256];
        spi::read_spi_flash_page(em100, 0x100000, &mut vpage)?;
        if page != vpage {
            return Err(Error::VerificationFailed);
        }
    }

    Ok(())
}

/// Update firmware from file (CLI version)
#[cfg(feature = "cli")]
pub fn firmware_update(em100: &Em100, filename: &str, verify: bool) -> Result<()> {
    match em100.hw_version {
        HwVersion::Em100ProEarly | HwVersion::Em100Pro => {
            println!("Detected EM100Pro (original).");
        }
        HwVersion::Em100ProG2 => {
            println!("Detected EM100Pro-G2.");
        }
        _ => {
            return Err(Error::UnsupportedHardware(em100.hw_version as u8));
        }
    }

    let fw = if filename.eq_ignore_ascii_case("auto") {
        println!("\nAutomatic firmware update.");
        load_auto_firmware(em100)?
    } else {
        println!("\nFirmware update with file {}", filename);
        let mut file = File::open(filename)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        data
    };

    let info = validate_firmware(em100, &fw)?;

    println!(
        "EM100Pro{} Update File: {}",
        if em100.hw_version == HwVersion::Em100ProG2 {
            "-G2"
        } else {
            ""
        },
        filename
    );

    if em100.hw_version == HwVersion::Em100Pro {
        println!(
            "  Installed version:  MCU {}.{}, FPGA {}.{} ({})",
            em100.mcu >> 8,
            em100.mcu & 0xff,
            (em100.fpga >> 8) & 0x7f,
            em100.fpga & 0xff,
            if em100.fpga & 0x8000 != 0 {
                "1.8V"
            } else {
                "3.3V"
            }
        );
    } else {
        println!(
            "  Installed version:  MCU {}.{}, FPGA {}.{:03}",
            em100.mcu >> 8,
            em100.mcu & 0xff,
            (em100.fpga >> 8) & 0x7f,
            em100.fpga & 0xff
        );
    }

    println!(
        "  New version:        MCU {}, FPGA {}",
        info.mcu_version, info.fpga_version
    );

    let total_len = info.fpga_len + info.mcu_len;
    let pb = ProgressBar::new(total_len as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:50}] {percent}% {msg}")
            .unwrap()
            .progress_chars("=> "),
    );

    firmware_write(
        em100,
        &fw,
        &info,
        verify,
        Some(&mut |pos, _total, msg| {
            pb.set_message(msg.to_string());
            pb.set_position(pos as u64);
        }),
    )?;

    pb.finish_with_message("Complete");

    println!("\nDisconnect and reconnect your EM100pro");

    Ok(())
}

#[cfg(feature = "cli")]
fn load_auto_firmware(em100: &Em100) -> Result<Vec<u8>> {
    let firmware_path = get_em100_file("firmware.tar.xz")?;
    let tar = TarFile::load_compressed(&firmware_path)?;

    // Find appropriate firmware
    let firmware_prefix = match em100.hw_version {
        HwVersion::Em100ProEarly | HwVersion::Em100Pro => "firmware/em100pro_fw_",
        HwVersion::Em100ProG2 => {
            return Err(Error::InvalidFirmware(
                "EM100Pro-G2 currently does not support auto-updating firmware.".to_string(),
            ));
        }
        _ => {
            return Err(Error::UnsupportedHardware(em100.hw_version as u8));
        }
    };

    let voltage_suffix = if em100.fpga & 0x8000 != 0 {
        "1.8V"
    } else {
        "3.3V"
    };

    // Find the latest firmware file that matches
    let mut selected: Option<(String, Vec<u8>)> = None;
    for entry in tar.entries() {
        if entry.starts_with(firmware_prefix) && entry.contains(voltage_suffix) {
            if let Ok(data) = tar.find(entry) {
                println!("select {}", entry);
                selected = Some((entry.to_string(), data));
            }
        }
    }

    selected.map(|(_, data)| data).ok_or_else(|| {
        Error::InvalidFirmware("Could not find suitable firmware for autoupdate".to_string())
    })
}
