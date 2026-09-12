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
//! use em100::{Em100, Result};
//! use futures_lite::future::block_on;
//!
//! fn main() -> Result<()> {
//!     block_on(async {
//!         for (bus, address, serial) in Em100::list_devices().await? {
//!             println!("{bus}:{address} {serial}");
//!         }
//!
//!         let device = Em100::open(None, None, None).await?;
//!         println!("{}", device.serial_string());
//!         Ok(())
//!     })
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
pub mod protocol;

// Async device access and helpers. Async is the only API: native code
// drives these futures with futures_lite::future::block_on, and the
// browser build awaits them on the JS event loop.
pub mod device;
pub mod firmware;
pub mod fpga;
pub mod image;
pub mod sdram;
pub mod spi;
pub mod system;
pub mod trace;
pub mod usb;

// CLI-only modules
#[cfg(feature = "cli")]
pub mod download;
#[cfg(feature = "cli")]
pub mod tar;

// Web module (native GUI only, not wasm32)
#[cfg(all(feature = "web", not(target_arch = "wasm32")))]
pub mod web;

pub use chips::{ChipDatabase, ChipDesc, parse_dcfg};
pub use error::{Error, Result};

// Re-exports for native platforms only
#[cfg(not(target_arch = "wasm32"))]
pub use device::{DebugInfo, DeviceInfo, Em100, HoldPinState, HwVersion, Voltages};
#[cfg(not(target_arch = "wasm32"))]
pub use firmware::{
    FirmwareInfo, firmware_read, firmware_to_dpfw, firmware_write, validate_firmware,
};
#[cfg(not(target_arch = "wasm32"))]
pub use sdram::{ProgressCallback, read_sdram_with_progress, write_sdram_with_progress};
