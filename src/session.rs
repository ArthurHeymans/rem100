//! Shared high-level device workflows for the CLI and WebUSB interface.
//!
//! [`DeviceSession`] owns an [`Em100`] and keeps frontend-visible state in one
//! place. Frontends remain responsible for scheduling and presentation, but
//! should not duplicate hardware operations or guess their resulting state.

use crate::chips::ChipDesc;
use crate::device::{DeviceInfo, Em100, HoldPinState};
use crate::error::Result;
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

    /// Stop emulation before configuring a chip.
    pub async fn stop_and_configure_chip(&mut self, chip: &ChipDesc) -> Result<()> {
        self.set_emulation_state(false).await?;
        self.configure_chip(chip).await
    }

    pub async fn set_address_mode(&mut self, mode: u8) -> Result<()> {
        self.device.set_address_mode(mode).await?;
        self.state.address_mode = mode;
        Ok(())
    }

    pub async fn write_memory(
        &mut self,
        data: &[u8],
        address: u32,
        progress: ProgressCallback<'_>,
    ) -> Result<()> {
        write_sdram_with_progress(&mut self.device, data, address, progress).await
    }

    /// Stop emulation before writing data into SDRAM.
    pub async fn stop_and_write_memory(
        &mut self,
        data: &[u8],
        address: u32,
        progress: ProgressCallback<'_>,
    ) -> Result<()> {
        self.set_emulation_state(false).await?;
        self.write_memory(data, address, progress).await
    }

    pub async fn read_memory(
        &mut self,
        address: u32,
        length: usize,
        progress: ProgressCallback<'_>,
    ) -> Result<Vec<u8>> {
        read_sdram_with_progress(&mut self.device, address, length, progress).await
    }

    pub async fn reset_spi_trace(&mut self) -> Result<()> {
        self.device.reset_spi_trace().await
    }

    pub async fn read_spi_trace_reports(&mut self) -> Result<Vec<Vec<u8>>> {
        self.device.read_spi_trace_reports().await
    }
}
