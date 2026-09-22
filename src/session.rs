//! Shared high-level device workflows for the CLI and WebUSB interface.
//!
//! [`DeviceSession`] owns an [`Em100`] and keeps frontend-visible state in one
//! place. Frontends remain responsible for scheduling and presentation, but
//! should not duplicate hardware operations or guess their resulting state.

use crate::chips::ChipDesc;
use crate::device::{DeviceInfo, Em100, HoldPinState};
use crate::error::{Error, Result};
use crate::usb;

const MAX_EMULATION_SIZE: usize = 0x4000000;

/// Parse a decimal address or one prefixed with 0x, without lossy casts.
pub fn parse_address(value: &str) -> Result<u32> {
    let value = value.trim();
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16)
    } else {
        value.parse::<u32>()
    };
    parsed.map_err(|_| Error::InvalidArgument(format!("Invalid address: {value}")))
}

/// Reject addresses which wrap or extend past the emulated capacity.
fn validate_memory_range(address: u32, length: usize, capacity: usize) -> Result<()> {
    if (address as usize)
        .checked_add(length)
        .is_some_and(|end| end <= capacity && length <= u32::MAX as usize)
    {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "Memory range 0x{address:08x} + {length} exceeds {capacity} bytes"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_address, validate_memory_range};

    #[test]
    fn addresses_reject_typos_and_overflow() {
        assert_eq!(parse_address("0x100").unwrap(), 256);
        assert_eq!(parse_address("100").unwrap(), 100);
        assert!(parse_address("not-an-address").is_err());
        assert!(parse_address("0x100000000").is_err());
    }

    #[test]
    fn memory_writes_must_fit_the_chip() {
        assert!(validate_memory_range(0x100, 0x100, 0x200).is_ok());
        assert!(validate_memory_range(0x101, 0x100, 0x200).is_err());
        assert!(validate_memory_range(u32::MAX, 2, 0x4000000).is_err());
    }
}
use crate::sdram::{ProgressCallback, read_sdram_with_progress, write_sdram_with_progress};

/// Last device state known to the application.
#[derive(Clone)]
pub struct DeviceState {
    info: Option<DeviceInfo>,
    is_running: Option<bool>,
    hold_pin_state: Option<HoldPinState>,
    configured_chip: Option<ChipDesc>,
    /// The device has no readable address-mode register, so this is the mode
    /// selected by the application. It starts as a conservative 3-byte guess.
    address_mode: u8,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            info: None,
            is_running: None,
            hold_pin_state: None,
            configured_chip: None,
            address_mode: 3,
        }
    }
}

impl DeviceState {
    pub fn info(&self) -> Option<&DeviceInfo> {
        self.info.as_ref()
    }

    pub fn is_running(&self) -> Option<bool> {
        self.is_running
    }

    pub fn hold_pin_state(&self) -> Option<HoldPinState> {
        self.hold_pin_state
    }

    pub fn configured_chip(&self) -> Option<&ChipDesc> {
        self.configured_chip.as_ref()
    }

    pub fn address_mode(&self) -> u8 {
        self.address_mode
    }
}

/// A connected EM100 together with its frontend-visible state.
pub struct DeviceSession {
    device: Em100,
    state: DeviceState,
}

impl DeviceSession {
    /// Create a session and read the state exposed by the hardware.
    pub async fn new(mut device: Em100) -> Self {
        let state = DeviceState {
            info: Some(device.get_info()),
            is_running: device.get_state().await.ok(),
            hold_pin_state: device.get_hold_pin_state().await.ok(),
            ..DeviceState::default()
        };
        Self { device, state }
    }

    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    pub fn device(&self) -> &Em100 {
        &self.device
    }

    /// Access operations that are not yet represented as session workflows.
    ///
    /// The device returned here is untracked: calling [`Em100::set_state`],
    /// [`Em100::set_hold_pin_state`], [`Em100::set_address_mode`], or
    /// [`Em100::set_chip_type`] through it would leave [`DeviceState`] stale.
    /// Use [`Self::set_emulation_state`], [`Self::set_hold_pin`],
    /// [`Self::set_address_mode`], or [`Self::configure_chip`] instead.
    pub fn device_mut(&mut self) -> &mut Em100 {
        &mut self.device
    }

    pub async fn set_emulation_state(&mut self, running: bool) -> Result<()> {
        self.device.set_state(running).await?;
        self.state.is_running = Some(running);
        Ok(())
    }

    pub async fn set_hold_pin(&mut self, state: HoldPinState) -> Result<()> {
        self.device.set_hold_pin_state(state).await?;
        self.state.hold_pin_state = Some(state);
        Ok(())
    }

    /// Configure a chip without changing the emulation state first.
    pub async fn configure_chip(&mut self, chip: &ChipDesc) -> Result<()> {
        // A failed initialization sequence may leave the FPGA partially
        // configured, so do not keep advertising the previous chip.
        self.state.configured_chip = None;
        self.device.set_chip_type(chip).await?;
        self.state.address_mode = chip.default_address_mode();
        self.state.configured_chip = Some(chip.clone());
        Ok(())
    }

    /// Stop emulation and confirm that it stopped before changing memory or
    /// chip configuration. A state write can succeed before the FPGA settles.
    pub async fn stop_for_mutation(&mut self) -> Result<()> {
        self.set_emulation_state(false).await?;
        for _ in 0..3 {
            match self.device.get_state().await {
                Ok(false) => return Ok(()),
                Ok(true) | Err(_) => usb::sleep_ms(50).await,
            }
        }
        self.state.is_running = None;
        Err(Error::OperationFailed(
            "Could not confirm that emulation stopped".to_string(),
        ))
    }

    /// Stop emulation before configuring a chip.
    pub async fn stop_and_configure_chip(&mut self, chip: &ChipDesc) -> Result<()> {
        self.stop_for_mutation().await?;
        self.configure_chip(chip).await
    }

    /// Set the address mode, optionally allowing the target to enter 4-byte
    /// addressing on its own (device register bit shared with the default
    /// address length).
    pub async fn set_address_mode(&mut self, mode: u8, enter_4byte: bool) -> Result<()> {
        self.device.set_address_mode(mode, enter_4byte).await?;
        self.state.address_mode = mode;
        Ok(())
    }

    pub async fn write_memory(
        &mut self,
        data: &[u8],
        address: u32,
        progress: ProgressCallback<'_>,
    ) -> Result<()> {
        validate_memory_range(
            address,
            data.len(),
            self.state
                .configured_chip
                .as_ref()
                .map_or(MAX_EMULATION_SIZE, |c| c.size as usize),
        )?;
        write_sdram_with_progress(&mut self.device, data, address, progress).await
    }

    /// Stop emulation before writing data into SDRAM.
    pub async fn stop_and_write_memory(
        &mut self,
        data: &[u8],
        address: u32,
        progress: ProgressCallback<'_>,
    ) -> Result<()> {
        // Reject invalid writes before stopping a running device.
        validate_memory_range(
            address,
            data.len(),
            self.state
                .configured_chip
                .as_ref()
                .map_or(MAX_EMULATION_SIZE, |c| c.size as usize),
        )?;
        self.stop_for_mutation().await?;
        self.write_memory(data, address, progress).await
    }

    pub async fn read_memory(
        &mut self,
        address: u32,
        length: usize,
        progress: ProgressCallback<'_>,
    ) -> Result<Vec<u8>> {
        validate_memory_range(address, length, MAX_EMULATION_SIZE)?;
        read_sdram_with_progress(&mut self.device, address, length, progress).await
    }

    pub async fn reset_spi_trace(&mut self) -> Result<()> {
        self.device.reset_spi_trace().await
    }

    pub async fn read_spi_trace_reports(&mut self) -> Result<Vec<Vec<u8>>> {
        self.device.read_spi_trace_reports().await
    }
}
