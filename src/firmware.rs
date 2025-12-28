//! Firmware update/dump operations

use crate::chips::get_em100_file;
use crate::device::{Em100, HwVersion};
use crate::error::{Error, Result};
use crate::spi;
use crate::tar::TarFile;
use byteorder::{LittleEndian, ByteOrder};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{Read, Write};

/// Size constants
const MB: usize = 1024 * 1024;

fn get_le32(data: &[u8]) -> u32 {
    LittleEndian::read_u32(data)
}

fn put_le32(data: &mut [u8], val: u32) {
    LittleEndian::write_u32(data, val);
}

/// Dump firmware from device to file
pub fn firmware_dump(em100: &Em100, filename: &str, firmware_is_dpfw: bool) -> Result<()> {
    let id = spi::get_spi_flash_id(em100)?;
    let rom_size = match id {
        0x202015 => 2 * MB, // M25P16
        0xc27518 => 16 * MB, // MX77L12850F
        _ => {
            return Err(Error::InvalidFirmware(format!(
                "Unknown SPI flash id = {:06x}. Please report",
                id
            )));
        }
    };

    let mut data = vec![0u8; rom_size];

    println!("\nWriting EM100Pro firmware to file {}", filename);

    let pb = ProgressBar::new(rom_size as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:50}] {percent}%")
            .unwrap()
            .progress_chars("=> "),
    );

    for i in (0..rom_size).step_by(256) {
        if i & 0x7fff == 0 {
            pb.set_position(i as u64);
        }
        
        // Retry up to 3 times
        for retry in 0..3 {
            if spi::read_spi_flash_page(em100, i as u32, &mut data[i..i + 256]).is_ok() {
                break;
            }
            if retry == 2 {
                println!("\nERROR: Couldn't read @{:08x}", i);
            }
        }
    }
    pb.finish();

    let mut file = File::create(filename)?;

    if firmware_is_dpfw {
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

        file.write_all(&header)?;
        file.write_all(&data[..fpga_size])?;
        file.write_all(&data[0x100100..0x100100 + mcu_size])?;
    } else {
        file.write_all(&data)?;
    }

    Ok(())
}

/// Update firmware from file
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

    // Validate firmware file
    match em100.hw_version {
        HwVersion::Em100ProEarly | HwVersion::Em100Pro => {
            if fw.len() < 0x48
                || &fw[..8] != b"em100pro"
                || &fw[0x28..0x2c] != b"WFPD"
            {
                return Err(Error::InvalidFirmware(
                    "Not an EM100Pro (original) firmware file.".to_string(),
                ));
            }
        }
        HwVersion::Em100ProG2 => {
            if fw.len() < 0x48
                || &fw[..11] != b"EM100Pro-G2"
                || &fw[0x28..0x2c] != b"WFPD"
            {
                return Err(Error::InvalidFirmware(
                    "Not an EM100Pro-G2 firmware file.".to_string(),
                ));
            }
        }
        _ => {}
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
        mcu_version, fpga_version
    );

    if fpga_len < 256 || mcu_len < 256 || fpga_len > 0x100000 || mcu_len > 0xf0000 {
        return Err(Error::InvalidFirmware(
            "Firmware file not valid.".to_string(),
        ));
    }

    // Unlock and erase
    spi::unlock_spi_flash(em100)?;
    spi::get_spi_flash_id(em100)?;

    println!("Erasing firmware:");
    let pb = ProgressBar::new(0x1f);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:50}] {percent}%")
            .unwrap()
            .progress_chars("=> "),
    );

    for i in 0..=0x1e {
        pb.set_position(i);
        spi::erase_spi_flash_sector(em100, i as u8)?;
    }
    pb.finish();

    spi::get_spi_flash_id(em100)?;

    println!("Writing firmware:");
    let total_len = fpga_len + mcu_len;
    let pb = ProgressBar::new(total_len as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:50}] {percent}%")
            .unwrap()
            .progress_chars("=> "),
    );

    // Write FPGA firmware
    let mut page = [0xffu8; 256];
    for i in (0..fpga_len).step_by(256) {
        page.fill(0xff);
        let chunk_len = (fpga_len - i).min(256);
        page[..chunk_len].copy_from_slice(&fw[fpga_offset + i..fpga_offset + i + chunk_len]);
        spi::write_spi_flash_page(em100, i as u32, &page)?;
        if i & 0xfff == 0 {
            pb.set_position(i as u64);
        }
    }

    // Write MCU firmware
    for i in (0..mcu_len).step_by(256) {
        page.fill(0xff);
        let chunk_len = (mcu_len - i).min(256);
        page[..chunk_len].copy_from_slice(&fw[mcu_offset + i..mcu_offset + i + chunk_len]);
        spi::write_spi_flash_page(em100, (i + 0x100100) as u32, &page)?;
        if i & 0xfff == 0 {
            pb.set_position((fpga_len + i) as u64);
        }
    }
    pb.finish();

    if verify {
        println!("Verifying firmware:");
        let pb = ProgressBar::new(total_len as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:50}] {percent}%")
                .unwrap()
                .progress_chars("=> "),
        );

        let mut vpage = [0u8; 256];

        // Verify FPGA
        for i in (0..fpga_len).step_by(256) {
            page.fill(0xff);
            let chunk_len = (fpga_len - i).min(256);
            page[..chunk_len].copy_from_slice(&fw[fpga_offset + i..fpga_offset + i + chunk_len]);
            spi::read_spi_flash_page(em100, i as u32, &mut vpage)?;
            if i & 0xfff == 0 {
                pb.set_position(i as u64);
            }
            if page != vpage {
                println!("\nERROR: Could not write FPGA firmware ({:x}).", i);
            }
        }

        // Verify MCU
        for i in (0..mcu_len).step_by(256) {
            page.fill(0xff);
            let chunk_len = (mcu_len - i).min(256);
            page[..chunk_len].copy_from_slice(&fw[mcu_offset + i..mcu_offset + i + chunk_len]);
            spi::read_spi_flash_page(em100, (i + 0x100100) as u32, &mut vpage)?;
            if i & 0xfff == 0 {
                pb.set_position((fpga_len + i) as u64);
            }
            if page != vpage {
                println!("\nERROR: Could not write MCU firmware ({:x}).", i);
            }
        }
        pb.finish();
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
            println!("ERROR: Could not write update tag.");
        }
    }

    println!("\nDisconnect and reconnect your EM100pro");

    Ok(())
}

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

    selected
        .map(|(_, data)| data)
        .ok_or_else(|| Error::InvalidFirmware("Could not find suitable firmware for autoupdate".to_string()))
}
