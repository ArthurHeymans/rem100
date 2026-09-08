//! Async USB communication functions for WebUSB (wasm32)
//!
//! This module provides async versions of USB operations that work
//! with the WebUSB API in browsers.

use crate::error::{Error, Result};
use crate::protocol::Command;
use nusb::Endpoint;
use nusb::transfer::{Buffer, Bulk, In, Out};

/// Round up to the next multiple of max packet size for IN transfers
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Send a typed 16-byte command to the EM100 (async).
pub async fn send_command(endpoint_out: &mut Endpoint<Bulk, Out>, command: Command) -> Result<()> {
    use zerocopy::IntoBytes;

    send_bytes(endpoint_out, command.as_bytes()).await
}

/// Send a command prefix, padding or truncating it to 16 bytes.
///
/// Prefer [`send_command`] for commands represented by the shared protocol API.
pub async fn send_cmd(endpoint_out: &mut Endpoint<Bulk, Out>, data: &[u8]) -> Result<()> {
    let mut command = [0; 16];
    let length = data.len().min(command.len());
    command[..length].copy_from_slice(&data[..length]);
    send_bytes(endpoint_out, &command).await
}

async fn send_bytes(endpoint_out: &mut Endpoint<Bulk, Out>, command: &[u8]) -> Result<()> {
    let buf = Buffer::from(command.to_vec());
    endpoint_out.submit(buf);

    let completion = std::future::poll_fn(|cx| endpoint_out.poll_next_complete(cx)).await;
    completion.status?;

    if completion.actual_len != 16 {
        return Err(Error::Communication(format!(
            "Expected to send 16 bytes, sent {}",
            completion.actual_len
        )));
    }

    Ok(())
}

/// Get a response from the EM100 (async)
pub async fn get_response(endpoint_in: &mut Endpoint<Bulk, In>, length: usize) -> Result<Vec<u8>> {
    let max_packet_size = endpoint_in.max_packet_size();
    let requested_len = round_up_to_max_packet(length, max_packet_size);
    let mut buf = Buffer::new(requested_len);
    buf.set_requested_len(requested_len);

    endpoint_in.submit(buf);

    let completion = std::future::poll_fn(|cx| endpoint_in.poll_next_complete(cx)).await;
    completion.status?;

    // Return only the bytes actually requested (up to actual_len)
    let actual = std::cmp::min(completion.actual_len, length);
    Ok(completion.buffer[..actual].to_vec())
}

/// Send a bulk transfer for large data (async)
pub async fn bulk_write(endpoint_out: &mut Endpoint<Bulk, Out>, data: &[u8]) -> Result<usize> {
    let buf = Buffer::from(data.to_vec());
    endpoint_out.submit(buf);

    let completion = std::future::poll_fn(|cx| endpoint_out.poll_next_complete(cx)).await;
    completion.status?;

    Ok(completion.actual_len)
}

/// Receive a bulk transfer for large data (async)
pub async fn bulk_read(endpoint_in: &mut Endpoint<Bulk, In>, length: usize) -> Result<Vec<u8>> {
    let max_packet_size = endpoint_in.max_packet_size();
    let requested_len = round_up_to_max_packet(length, max_packet_size);
    let mut buf = Buffer::new(requested_len);
    buf.set_requested_len(requested_len);

    endpoint_in.submit(buf);

    let completion = std::future::poll_fn(|cx| endpoint_in.poll_next_complete(cx)).await;
    completion.status?;

    let received = std::cmp::min(completion.actual_len, length);
    Ok(completion.buffer[..received].to_vec())
}
