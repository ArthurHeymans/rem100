//! Async USB communication functions for WebUSB (wasm32)
//!
//! This module provides async versions of USB operations that work
//! with the WebUSB API in browsers.

use crate::error::{Error, Result};
use nusb::transfer::{Buffer, Bulk, In, Out};
use nusb::Endpoint;

/// Round up to the next multiple of max packet size for IN transfers
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Send a 16-byte command to the EM100 (async)
pub async fn send_cmd(endpoint_out: &mut Endpoint<Bulk, Out>, data: &[u8]) -> Result<()> {
    let mut cmd = [0u8; 16];
    let len = std::cmp::min(data.len(), 16);
    cmd[..len].copy_from_slice(&data[..len]);

    let buf = Buffer::from(cmd.to_vec());
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
