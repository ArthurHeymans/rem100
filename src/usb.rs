//! Low-level USB communication functions

use crate::device::Em100;
use crate::error::{Error, Result};
use nusb::transfer::Buffer;
use std::time::Duration;

/// Default timeout for USB transfers
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5000);

/// Round up to the next multiple of max packet size for IN transfers
/// nusb 0.2 requires requested_len to be a multiple of max_packet_size
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Send a 16-byte command to the EM100
pub fn send_cmd(em100: &Em100, data: &[u8]) -> Result<()> {
    let mut cmd = [0u8; 16];
    let len = std::cmp::min(data.len(), 16);
    cmd[..len].copy_from_slice(&data[..len]);

    let buf = Buffer::from(cmd.to_vec());
    let completion = em100
        .endpoint_out
        .borrow_mut()
        .transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    let written = completion.actual_len;

    if written != 16 {
        return Err(Error::Communication(format!(
            "Expected to send 16 bytes, sent {}",
            written
        )));
    }

    Ok(())
}

/// Get a response from the EM100
pub fn get_response(em100: &Em100, length: usize) -> Result<Vec<u8>> {
    let mut ep = em100.endpoint_in.borrow_mut();
    let max_packet_size = ep.max_packet_size();
    let requested_len = round_up_to_max_packet(length, max_packet_size);
    let mut buf = Buffer::new(requested_len);
    buf.set_requested_len(requested_len);
    let completion = ep.transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    // Return only the bytes actually requested (up to actual_len)
    let actual = std::cmp::min(completion.actual_len, length);
    Ok(completion.buffer[..actual].to_vec())
}

/// Send a bulk transfer (for large data transfers)
pub fn bulk_write(em100: &Em100, data: &[u8]) -> Result<usize> {
    let buf = Buffer::from(data.to_vec());
    let completion = em100
        .endpoint_out
        .borrow_mut()
        .transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    Ok(completion.actual_len)
}

/// Receive a bulk transfer (for large data transfers)
pub fn bulk_read(em100: &Em100, buffer: &mut [u8]) -> Result<usize> {
    let mut ep = em100.endpoint_in.borrow_mut();
    let max_packet_size = ep.max_packet_size();
    let requested_len = round_up_to_max_packet(buffer.len(), max_packet_size);
    let mut buf = Buffer::new(requested_len);
    buf.set_requested_len(requested_len);
    let completion = ep.transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    let received = std::cmp::min(completion.actual_len, buffer.len());
    buffer[..received].copy_from_slice(&completion.buffer[..received]);
    Ok(received)
}
