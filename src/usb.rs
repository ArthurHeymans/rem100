//! Low-level async USB communication.
//!
//! Used natively over nusb as well as in browsers through WebUSB.

use crate::error::{Error, Result};
use crate::protocol::Command;
use nusb::Endpoint;
use nusb::transfer::{Buffer, Bulk, BulkOrInterrupt, Completion, EndpointDirection, In, Out};

/// Round up to the next multiple of max packet size for IN transfers
fn round_up_to_max_packet(len: usize, max_packet_size: usize) -> usize {
    len.div_ceil(max_packet_size) * max_packet_size
}

/// Wait for the completion of a previously submitted transfer.
async fn await_completion<EpType, Dir>(endpoint: &mut Endpoint<EpType, Dir>) -> Result<Completion>
where
    EpType: BulkOrInterrupt,
    Dir: EndpointDirection,
{
    Ok(endpoint.next_complete().await)
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

    let completion = await_completion(endpoint_out).await?;
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

    let completion = await_completion(endpoint_in).await?;
    completion.status?;

    // Return only the bytes actually requested (up to actual_len)
    let actual = std::cmp::min(completion.actual_len, length);
    Ok(completion.buffer[..actual].to_vec())
}

/// Send a bulk transfer for large data (async)
pub async fn bulk_write(endpoint_out: &mut Endpoint<Bulk, Out>, data: &[u8]) -> Result<usize> {
    let buf = Buffer::from(data.to_vec());
    endpoint_out.submit(buf);

    let completion = await_completion(endpoint_out).await?;
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

    let completion = await_completion(endpoint_in).await?;
    completion.status?;

    let received = std::cmp::min(completion.actual_len, length);
    Ok(completion.buffer[..received].to_vec())
}

/// Wait between USB operations where pacing is required.
///
/// Never blocks the thread, so the native GUI stays responsive during
/// the multi-second FPGA and SPI flash waits; on wasm32 this is a JS timer.
pub async fn sleep_ms(ms: u32) {
    futures_timer::Delay::new(std::time::Duration::from_millis(ms as u64)).await;
}
