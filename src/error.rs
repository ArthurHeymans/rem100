//! Error types for rem100

use thiserror::Error;

/// Result type for rem100 operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during EM100 operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("USB error: {0}")]
    Usb(#[from] nusb::Error),

    #[error("USB transfer error: {0}")]
    UsbTransfer(#[from] nusb::transfer::TransferError),

    #[error("IO error: {0}")]
    Io(std::io::Error),

    #[error("Device not found")]
    DeviceNotFound,

    #[error("Device communication failed: {0}")]
    Communication(String),

    #[error("Invalid response from device")]
    InvalidResponse,

    #[error("Device status unknown")]
    StatusUnknown,

    #[error("Failed to claim USB interface")]
    ClaimInterface,

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("Invalid chip: {0}")]
    InvalidChip(String),

    #[error("Invalid firmware file: {0}")]
    InvalidFirmware(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),

    #[error("Verification failed")]
    VerificationFailed,

    #[error("Unsupported hardware version: {0}")]
    UnsupportedHardware(u8),
}
