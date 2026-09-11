//! SDRAM related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::protocol::sdram as command;
use crate::usb;

/// Transfer chunk size (2MB)
const TRANSFER_LENGTH: usize = 0x200000;

/// Progress callback type for reporting transfer progress
/// Arguments: (bytes_transferred, total_bytes)
pub type ProgressCallback<'a> = Option<&'a mut dyn FnMut(usize, usize)>;

/// Read data from SDRAM with optional progress callback
pub async fn read_sdram_with_progress(
    em100: &mut Em100,
    address: u32,
    length: usize,
    mut progress: ProgressCallback<'_>,
) -> Result<Vec<u8>> {
    usb::send_command(
        &mut em100.endpoint_out,
        command::read(address, length as u32),
    )
    .await?;

    let mut data = Vec::with_capacity(length);
    let mut bytes_read = 0;

    while bytes_read < length {
        let bytes_to_read = std::cmp::min(length - bytes_read, TRANSFER_LENGTH);
        let chunk = usb::bulk_read(&mut em100.endpoint_in, bytes_to_read).await?;
        let actual = chunk.len();
        data.extend_from_slice(&chunk);
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
pub async fn read_sdram(em100: &mut Em100, address: u32, length: usize) -> Result<Vec<u8>> {
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
    )
    .await;

    match &result {
        Ok(_) => pb.finish_with_message("Read complete"),
        Err(_) => pb.abandon_with_message("Read failed"),
    }

    result
}

/// Read data from SDRAM (no progress display)
#[cfg(not(feature = "cli"))]
pub async fn read_sdram(em100: &mut Em100, address: u32, length: usize) -> Result<Vec<u8>> {
    read_sdram_with_progress(em100, address, length, None).await
}

/// Write data to SDRAM with optional progress callback
pub async fn write_sdram_with_progress(
    em100: &mut Em100,
    data: &[u8],
    address: u32,
    mut progress: ProgressCallback<'_>,
) -> Result<()> {
    let length = data.len();

    usb::send_command(
        &mut em100.endpoint_out,
        command::write(address, length as u32),
    )
    .await?;

    let mut bytes_sent = 0;

    while bytes_sent < length {
        let bytes_to_send = std::cmp::min(length - bytes_sent, TRANSFER_LENGTH);
        let actual = usb::bulk_write(
            &mut em100.endpoint_out,
            &data[bytes_sent..bytes_sent + bytes_to_send],
        )
        .await?;
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
pub async fn write_sdram(em100: &mut Em100, data: &[u8], address: u32) -> Result<()> {
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
    )
    .await;

    match &result {
        Ok(_) => pb.finish_with_message("Transfer complete"),
        Err(_) => pb.abandon_with_message("Transfer failed"),
    }

    result
}

/// Write data to SDRAM (no progress display)
#[cfg(not(feature = "cli"))]
pub async fn write_sdram(em100: &mut Em100, data: &[u8], address: u32) -> Result<()> {
    write_sdram_with_progress(em100, data, address, None).await
}
