//! SDRAM related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;
use nusb::transfer::Buffer;
use std::time::Duration;

/// Transfer chunk size (2MB)
const TRANSFER_LENGTH: usize = 0x200000;

/// Default timeout for USB transfers
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5000);

/// Round up to the next multiple of max packet size for IN transfers
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Progress callback type for reporting transfer progress
/// Arguments: (bytes_transferred, total_bytes)
pub type ProgressCallback<'a> = Option<&'a mut dyn FnMut(usize, usize)>;

/// Read data from SDRAM with optional progress callback
pub fn read_sdram_with_progress(
    em100: &Em100,
    address: u32,
    length: usize,
    mut progress: ProgressCallback,
) -> Result<Vec<u8>> {
    let cmd = [
        0x41u8,
        ((address >> 24) & 0xff) as u8,
        ((address >> 16) & 0xff) as u8,
        ((address >> 8) & 0xff) as u8,
        (address & 0xff) as u8,
        ((length >> 24) & 0xff) as u8,
        ((length >> 16) & 0xff) as u8,
        ((length >> 8) & 0xff) as u8,
        (length & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];

    usb::send_cmd(em100, &cmd)?;

    let mut data = vec![0u8; length];
    let mut bytes_read = 0;

    while bytes_read < length {
        let bytes_to_read = std::cmp::min(length - bytes_read, TRANSFER_LENGTH);

        let mut ep = em100.endpoint_in.borrow_mut();
        let max_packet_size = ep.max_packet_size();
        let requested_len = round_up_to_max_packet(bytes_to_read, max_packet_size);
        let mut buf = Buffer::new(requested_len);
        buf.set_requested_len(requested_len);
        let completion = ep.transfer_blocking(buf, DEFAULT_TIMEOUT);
        completion.status?;
        let actual = std::cmp::min(completion.actual_len, bytes_to_read);

        data[bytes_read..bytes_read + actual].copy_from_slice(&completion.buffer[..actual]);
        bytes_read += actual;

        if let Some(ref mut cb) = progress {
            cb(bytes_read, length);
        }

        if actual < bytes_to_read {
            break;
        }
    }

    if bytes_read != length {
        return Err(Error::Communication(format!(
            "SDRAM read failed: read {} of {} bytes",
            bytes_read, length
        )));
    }

    Ok(data)
}

/// Read data from SDRAM (convenience wrapper with CLI progress bar)
#[cfg(feature = "cli")]
pub fn read_sdram(em100: &Em100, address: u32, length: usize) -> Result<Vec<u8>> {
    use indicatif::{ProgressBar, ProgressStyle};

    let pb = ProgressBar::new(length as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let result = read_sdram_with_progress(
        em100,
        address,
        length,
        Some(&mut |bytes_read, _total| {
            pb.set_position(bytes_read as u64);
        }),
    );

    match &result {
        Ok(_) => pb.finish_with_message("Read complete"),
        Err(_) => pb.abandon_with_message("Read failed"),
    }

    result
}

/// Read data from SDRAM (no progress display)
#[cfg(not(feature = "cli"))]
pub fn read_sdram(em100: &Em100, address: u32, length: usize) -> Result<Vec<u8>> {
    read_sdram_with_progress(em100, address, length, None)
}

/// Write data to SDRAM with optional progress callback
pub fn write_sdram_with_progress(
    em100: &Em100,
    data: &[u8],
    address: u32,
    mut progress: ProgressCallback,
) -> Result<()> {
    let length = data.len();

    let cmd = [
        0x40u8,
        ((address >> 24) & 0xff) as u8,
        ((address >> 16) & 0xff) as u8,
        ((address >> 8) & 0xff) as u8,
        (address & 0xff) as u8,
        ((length >> 24) & 0xff) as u8,
        ((length >> 16) & 0xff) as u8,
        ((length >> 8) & 0xff) as u8,
        (length & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];

    usb::send_cmd(em100, &cmd)?;

    let mut bytes_sent = 0;

    while bytes_sent < length {
        let bytes_to_send = std::cmp::min(length - bytes_sent, TRANSFER_LENGTH);

        let buf = Buffer::from(data[bytes_sent..bytes_sent + bytes_to_send].to_vec());
        let completion = em100
            .endpoint_out
            .borrow_mut()
            .transfer_blocking(buf, DEFAULT_TIMEOUT);
        completion.status?;
        let actual = completion.actual_len;

        bytes_sent += actual;

        if let Some(ref mut cb) = progress {
            cb(bytes_sent, length);
        }

        if actual < bytes_to_send {
            break;
        }
    }

    if bytes_sent != length {
        return Err(Error::Communication(format!(
            "SDRAM write failed: sent {} of {} bytes",
            bytes_sent, length
        )));
    }

    Ok(())
}

/// Write data to SDRAM (convenience wrapper with CLI progress bar)
#[cfg(feature = "cli")]
pub fn write_sdram(em100: &Em100, data: &[u8], address: u32) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};

    let length = data.len();
    let pb = ProgressBar::new(length as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let result = write_sdram_with_progress(
        em100,
        data,
        address,
        Some(&mut |bytes_sent, _total| {
            pb.set_position(bytes_sent as u64);
        }),
    );

    match &result {
        Ok(_) => pb.finish_with_message("Transfer complete"),
        Err(_) => pb.abandon_with_message("Transfer failed"),
    }

    result
}

/// Write data to SDRAM (no progress display)
#[cfg(not(feature = "cli"))]
pub fn write_sdram(em100: &Em100, data: &[u8], address: u32) -> Result<()> {
    write_sdram_with_progress(em100, data, address, None)
}
