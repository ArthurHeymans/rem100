//! Low-level USB communication functions

use crate::device::Em100;
use crate::error::{Error, Result};
use nusb::transfer::Buffer;
use std::time::Duration;

/// Default timeout for USB transfers
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5000);

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
    let mut buf = Buffer::new(length);
    buf.set_requested_len(length);
    let completion = em100
        .endpoint_in
        .borrow_mut()
        .transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    Ok(completion.buffer[..completion.actual_len].to_vec())
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
    let mut buf = Buffer::new(buffer.len());
    buf.set_requested_len(buffer.len());
    let completion = em100
        .endpoint_in
        .borrow_mut()
        .transfer_blocking(buf, DEFAULT_TIMEOUT);
    completion.status?;
    let received = completion.actual_len;
    buffer[..received].copy_from_slice(&completion.buffer[..received]);
    Ok(received)
}
