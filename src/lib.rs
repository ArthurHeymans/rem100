//! Rust library for controlling Dediprog EM100Pro SPI flash emulators.
//!
//! The crates.io package is named `rem100`; its library crate is named `em100`.
//! The package also provides the `rem100` command-line utility.
//!
//! Library-only users should disable the default `cli` feature:
//!
//! ```toml
//! [dependencies]
//! rem100 = { version = "0.1", default-features = false }
//! ```
//!
//! # Example
//!
//! ```no_run
//! use em100::{list_devices, Em100, Result};
//!
//! fn main() -> Result<()> {
//!     for (bus, address, serial) in list_devices()? {
//!         println!("{bus}:{address} {serial}");
//!     }
//!
//!     let device = Em100::open(None, None, None)?;
//!     println!("{}", device.serial_string());
//!     Ok(())
//! }
//! ```
//!
//! The `cli` feature enables the command-line binary and CLI-only downloading
//! and archive helpers. The `web` feature enables the egui interface; on
//! `wasm32-unknown-unknown`, USB access uses WebUSB through `nusb`.
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

pub use chips::{ChipDatabase, ChipDesc, parse_dcfg};
pub use error::{Error, Result};

// Re-exports for native platforms only
#[cfg(not(target_arch = "wasm32"))]
pub use device::{DebugInfo, DeviceInfo, Em100, HoldPinState, HwVersion, Voltages, list_devices};
#[cfg(not(target_arch = "wasm32"))]
pub use firmware::{
    FirmwareInfo, firmware_read, firmware_to_dpfw, firmware_write, validate_firmware,
};
#[cfg(not(target_arch = "wasm32"))]
pub use sdram::{ProgressCallback, read_sdram_with_progress, write_sdram_with_progress};
