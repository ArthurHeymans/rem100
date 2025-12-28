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
pub mod device;
pub mod download;
pub mod error;
pub mod firmware;
pub mod fpga;
pub mod hexdump;
pub mod image;
pub mod sdram;
pub mod spi;
pub mod system;
pub mod tar;
pub mod trace;
pub mod usb;

pub use device::{Em100, HwVersion};
pub use error::{Error, Result};
