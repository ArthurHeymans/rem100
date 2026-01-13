//! rem100 - EM100Pro SPI flash emulator command-line utility
//!
//! A Rust port of the em100 utility for controlling the Dediprog EM100Pro
//! SPI flash emulator hardware.
//!
//! Copyright 2012-2019 Google Inc.
//! Rust port copyright 2024
//!
//! This program is free software; you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation; version 2 of the License.

pub mod chips;
pub mod error;
pub mod hexdump;

// Image module requires device types
#[cfg(not(target_arch = "wasm32"))]
pub mod image;

// Modules that require blocking USB operations (not available on wasm32)
#[cfg(not(target_arch = "wasm32"))]
pub mod device;
#[cfg(not(target_arch = "wasm32"))]
pub mod firmware;
#[cfg(not(target_arch = "wasm32"))]
pub mod fpga;
#[cfg(not(target_arch = "wasm32"))]
pub mod sdram;
#[cfg(not(target_arch = "wasm32"))]
pub mod spi;
#[cfg(not(target_arch = "wasm32"))]
pub mod system;
#[cfg(not(target_arch = "wasm32"))]
pub mod trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod usb;

// CLI-only modules
#[cfg(feature = "cli")]
pub mod download;
#[cfg(feature = "cli")]
pub mod tar;

// Web module (native GUI only, not wasm32)
#[cfg(all(feature = "web", not(target_arch = "wasm32")))]
pub mod web;

// Async WebUSB modules (for wasm32)
#[cfg(target_arch = "wasm32")]
pub mod web_device;
#[cfg(target_arch = "wasm32")]
pub mod web_usb;

pub use chips::{parse_dcfg, ChipDatabase, ChipDesc};
pub use error::{Error, Result};

// Re-exports for native platforms only
#[cfg(not(target_arch = "wasm32"))]
pub use device::{list_devices, DebugInfo, DeviceInfo, Em100, HoldPinState, HwVersion, Voltages};
#[cfg(not(target_arch = "wasm32"))]
pub use firmware::{
    firmware_read, firmware_to_dpfw, firmware_write, validate_firmware, FirmwareInfo,
};
#[cfg(not(target_arch = "wasm32"))]
pub use sdram::{read_sdram_with_progress, write_sdram_with_progress, ProgressCallback};
