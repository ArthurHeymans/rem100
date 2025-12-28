//! SPI flash related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;
use futures_lite::future::block_on;
use std::thread;
use std::time::Duration;

/// USB endpoint for sending data
const ENDPOINT_OUT: u8 = 0x01;

/// Get SPI flash ID
pub fn get_spi_flash_id(em100: &Em100) -> Result<u32> {
    let cmd = [0x30u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 512)?;

    if data.len() == 3 {
        let id = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);
        Ok(id)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Erase entire SPI flash
pub fn erase_spi_flash(em100: &Em100) -> Result<()> {
    let cmd = [0x31u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    // Specification says to wait 5s before issuing another USB command
    thread::sleep(Duration::from_secs(5));
    Ok(())
}

/// Poll SPI flash status
pub fn poll_spi_flash_status(em100: &Em100) -> Result<bool> {
    let cmd = [0x32u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 1)?;

    if data.len() == 1 && data[0] == 1 {
        Ok(true) // ready
    } else {
        Ok(false) // busy
    }
}

/// Read a 256-byte page from SPI flash
pub fn read_spi_flash_page(em100: &Em100, address: u32, buffer: &mut [u8]) -> Result<()> {
    if buffer.len() < 256 {
        return Err(Error::InvalidArgument(
            "Buffer must be at least 256 bytes".to_string(),
        ));
    }

    let cmd = [
        0x33u8,
        ((address >> 16) & 0xff) as u8,
        ((address >> 8) & 0xff) as u8,
        (address & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 256)?;

    if data.len() == 256 {
        buffer[..256].copy_from_slice(&data);
        Ok(())
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Write a 256-byte page to SPI flash
pub fn write_spi_flash_page(em100: &Em100, address: u32, data: &[u8]) -> Result<()> {
    if data.len() > 256 {
        return Err(Error::InvalidArgument(
            "Data must be at most 256 bytes".to_string(),
        ));
    }

    let cmd = [
        0x34u8,
        ((address >> 16) & 0xff) as u8,
        ((address >> 8) & 0xff) as u8,
        (address & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;

    // Pad data to 256 bytes if needed
    let mut page = [0xffu8; 256];
    page[..data.len()].copy_from_slice(data);

    let completion = block_on(em100.interface.bulk_out(ENDPOINT_OUT, page.to_vec()));
    completion.status?;
    let bytes_sent = completion.data.actual_length();

    if bytes_sent != 256 {
        return Err(Error::Communication(format!(
            "SPI transfer failed: sent {} of 256 bytes",
            bytes_sent
        )));
    }

    Ok(())
}

/// Unlock SPI flash
pub fn unlock_spi_flash(em100: &Em100) -> Result<()> {
    let cmd = [0x36u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}

/// Erase a 64KB SPI flash sector
pub fn erase_spi_flash_sector(em100: &Em100, sector: u8) -> Result<()> {
    if sector > 31 {
        return Err(Error::InvalidArgument(format!(
            "Can't erase sector at address {:x}",
            (sector as u32) << 16
        )));
    }

    let cmd = [0x37u8, sector, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    // Specification says to wait 5s before issuing another USB command
    thread::sleep(Duration::from_secs(5));
    Ok(())
}

// SPI Hyper Terminal related operations

/// HT register types
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum HtRegister {
    Status = 0,
    DfifoBytes = 1,
    UfifoBytes = 2,
    Em100Id = 3,
    UfifoDataFmt = 4,
    Timestamp = 5,
}

/// Status register bits
pub const UFIFO_OVERFLOW: u8 = 1 << 0;
pub const BIT8_UFIFO_BYTES: u8 = 1 << 3;
pub const START_SPI_EMULATION: u8 = 1 << 4;
pub const UFIFO_EMPTY: u8 = 1 << 5;
pub const DFIFO_EMPTY: u8 = 1 << 6;

/// Read HT register
pub fn read_ht_register(em100: &Em100, reg: HtRegister) -> Result<u8> {
    let cmd = [0x50u8, reg as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 2)?;

    if data.len() == 2 && data[0] == 1 {
        Ok(data[1])
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Write HT register
pub fn write_ht_register(em100: &Em100, reg: HtRegister, val: u8) -> Result<()> {
    let cmd = [
        0x51u8, reg as u8, val, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}

/// Write to dFIFO
pub fn write_dfifo(em100: &Em100, data: &[u8], timeout: u16) -> Result<()> {
    if data.len() > 512 {
        return Err(Error::InvalidArgument(
            "Length of data to be written to dFIFO can't be > 512".to_string(),
        ));
    }

    let length = data.len();
    let cmd = [
        0x52u8,
        ((length >> 8) & 0xff) as u8,
        (length & 0xff) as u8,
        ((timeout >> 8) & 0xff) as u8,
        (timeout & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;

    let completion = block_on(em100.interface.bulk_out(ENDPOINT_OUT, data.to_vec()));
    completion.status?;
    let bytes_sent = completion.data.actual_length();

    let response = usb::get_response(&em100.interface, 512)?;

    if response.len() == 2 && ((response[0] as usize) << 8 | response[1] as usize) == length && bytes_sent == length {
        Ok(())
    } else {
        Err(Error::Communication("dFIFO write failed".to_string()))
    }
}

/// Read from uFIFO
pub fn read_ufifo(em100: &Em100, length: usize, timeout: u16) -> Result<Vec<u8>> {
    if length > 512 {
        return Err(Error::InvalidArgument(
            "Length of data to be read from uFIFO can't be > 512".to_string(),
        ));
    }

    let cmd = [
        0x53u8,
        ((length >> 8) & 0xff) as u8,
        (length & 0xff) as u8,
        ((timeout >> 8) & 0xff) as u8,
        (timeout & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 512)?;

    // Get second response from read ufifo command
    let _ = usb::get_response(&em100.interface, 2);

    if data.len() == length {
        Ok(data)
    } else {
        Err(Error::InvalidResponse)
    }
}
