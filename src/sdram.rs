//! SDRAM related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;
use indicatif::{ProgressBar, ProgressStyle};
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

/// Read data from SDRAM
pub fn read_sdram(em100: &Em100, address: u32, length: usize) -> Result<Vec<u8>> {
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

    let pb = ProgressBar::new(length as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

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

        pb.set_position(bytes_read as u64);

        if actual < bytes_to_read {
            pb.abandon_with_message(format!(
                "Warning: tried reading {} bytes, got {}",
                bytes_to_read, actual
            ));
            break;
        }
    }

    pb.finish_with_message("Read complete");

    if bytes_read != length {
        return Err(Error::Communication(format!(
            "SDRAM read failed: read {} of {} bytes",
            bytes_read, length
        )));
    }

    Ok(data)
}

/// Write data to SDRAM
pub fn write_sdram(em100: &Em100, data: &[u8], address: u32) -> Result<()> {
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

    let pb = ProgressBar::new(length as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

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

        pb.set_position(bytes_sent as u64);

        if actual < bytes_to_send {
            pb.abandon_with_message(format!(
                "Warning: tried sending {} bytes, sent {}",
                bytes_to_send, actual
            ));
            break;
        }
    }

    if bytes_sent == length {
        pb.finish_with_message("Transfer complete");
    } else {
        pb.abandon_with_message("Transfer failed");
        return Err(Error::Communication(format!(
            "SDRAM write failed: sent {} of {} bytes",
            bytes_sent, length
        )));
    }

    Ok(())
}
