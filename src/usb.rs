//! Low-level USB communication functions

use crate::error::{Error, Result};
use futures_lite::future::block_on;
use nusb::transfer::RequestBuffer;
use nusb::Interface;

/// USB endpoint for sending commands
const ENDPOINT_OUT: u8 = 0x01;
/// USB endpoint for receiving responses
const ENDPOINT_IN: u8 = 0x82;

/// Send a 16-byte command to the EM100
pub fn send_cmd(interface: &Interface, data: &[u8]) -> Result<()> {
    let mut cmd = [0u8; 16];
    let len = std::cmp::min(data.len(), 16);
    cmd[..len].copy_from_slice(&data[..len]);

    let completion = block_on(interface.bulk_out(ENDPOINT_OUT, cmd.to_vec()));
    completion.status?;
    let written = completion.data.actual_length();

    if written != 16 {
        return Err(Error::Communication(format!(
            "Expected to send 16 bytes, sent {}",
            written
        )));
    }

    Ok(())
}

/// Get a response from the EM100
pub fn get_response(interface: &Interface, length: usize) -> Result<Vec<u8>> {
    let buf = RequestBuffer::new(length);
    let completion = block_on(interface.bulk_in(ENDPOINT_IN, buf));
    completion.status?;
    Ok(completion.data)
}

/// Send a bulk transfer (for large data transfers)
pub fn bulk_write(interface: &Interface, data: &[u8]) -> Result<usize> {
    let completion = block_on(interface.bulk_out(ENDPOINT_OUT, data.to_vec()));
    completion.status?;
    Ok(completion.data.actual_length())
}

/// Receive a bulk transfer (for large data transfers)
pub fn bulk_read(interface: &Interface, buffer: &mut [u8]) -> Result<usize> {
    let buf = RequestBuffer::new(buffer.len());
    let completion = block_on(interface.bulk_in(ENDPOINT_IN, buf));
    completion.status?;
    let received = completion.data.len();
    buffer[..received].copy_from_slice(&completion.data);
    Ok(received)
}
