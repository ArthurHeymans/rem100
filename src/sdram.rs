//! SDRAM related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;
use futures_lite::future::block_on;
use nusb::transfer::RequestBuffer;

/// Transfer chunk size (2MB)
const TRANSFER_LENGTH: usize = 0x200000;

/// USB endpoint for receiving responses
const ENDPOINT_IN: u8 = 0x82;
/// USB endpoint for sending data
const ENDPOINT_OUT: u8 = 0x01;

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

    usb::send_cmd(&em100.interface, &cmd)?;

    let mut data = vec![0u8; length];
    let mut bytes_read = 0;

    while bytes_read < length {
        let bytes_to_read = std::cmp::min(length - bytes_read, TRANSFER_LENGTH);

        let buf = RequestBuffer::new(bytes_to_read);
        let completion = block_on(em100.interface.bulk_in(ENDPOINT_IN, buf));
        completion.status?;
        let actual = completion.data.len();

        data[bytes_read..bytes_read + actual].copy_from_slice(&completion.data);
        bytes_read += actual;

        if actual < bytes_to_read {
            println!(
                "Warning: tried reading {} bytes, got {}",
                bytes_to_read, actual
            );
            break;
        }

        println!("Read {} bytes of {}", bytes_read, length);
    }

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

    usb::send_cmd(&em100.interface, &cmd)?;

    let mut bytes_sent = 0;

    while bytes_sent < length {
        let bytes_to_send = std::cmp::min(length - bytes_sent, TRANSFER_LENGTH);

        let completion = block_on(
            em100
                .interface
                .bulk_out(ENDPOINT_OUT, data[bytes_sent..bytes_sent + bytes_to_send].to_vec()),
        );
        completion.status?;
        let actual = completion.data.actual_length();

        bytes_sent += actual;

        if actual < bytes_to_send {
            println!(
                "Warning: tried sending {} bytes, sent {}",
                bytes_to_send, actual
            );
            break;
        }

        println!("Sent {} bytes of {}", bytes_sent, length);
    }

    println!(
        "Transfer {}",
        if bytes_sent == length {
            "Succeeded"
        } else {
            "Failed"
        }
    );

    if bytes_sent != length {
        return Err(Error::Communication(format!(
            "SDRAM write failed: sent {} of {} bytes",
            bytes_sent, length
        )));
    }

    Ok(())
}
