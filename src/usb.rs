//! Low-level USB communication functions

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::protocol::Command;
use nusb::transfer::Buffer;
use std::time::Duration;

/// Default timeout for USB transfers
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5000);

/// Round up to the next multiple of max packet size for IN transfers
/// nusb 0.2 requires requested_len to be a multiple of max_packet_size
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Send a typed 16-byte command to the EM100.
pub fn send_command(em100: &Em100, command: Command) -> Result<()> {
    use zerocopy::IntoBytes;

    send_bytes(em100, command.as_bytes())
}

/// Send a command prefix, padding or truncating it to 16 bytes.
///
/// Prefer [`send_command`] for commands represented by the shared protocol API.
pub fn send_cmd(em100: &Em100, data: &[u8]) -> Result<()> {
    let mut command = [0; 16];
    let length = data.len().min(command.len());
    command[..length].copy_from_slice(&data[..length]);
    send_bytes(em100, &command)
}

fn send_bytes(em100: &Em100, command: &[u8]) -> Result<()> {
    let buf = Buffer::from(command.to_vec());
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
