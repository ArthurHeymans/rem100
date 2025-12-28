//! FPGA related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;
use std::thread;
use std::time::Duration;

/// FPGA register for device ID
pub const FPGA_REG_DEVID: u8 = 0x40;
/// FPGA register for vendor ID
pub const FPGA_REG_VENDID: u8 = 0x42;

/// Reconfigure FPGA
pub fn reconfig_fpga(em100: &Em100) -> Result<()> {
    let cmd = [0x20u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    // Specification says to wait 2s before issuing another USB command
    thread::sleep(Duration::from_secs(2));
    Ok(())
}

/// Check FPGA configuration status
pub fn check_fpga_status(em100: &Em100) -> Result<bool> {
    let cmd = [0x21u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 512)?;

    if data.len() == 1 {
        Ok(data[0] == 1)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Read FPGA register
pub fn read_fpga_register(em100: &Em100, reg: u8) -> Result<u16> {
    let cmd = [0x22u8, reg, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;

    let data = usb::get_response(&em100.interface, 3)?;

    if data.len() == 3 && data[0] == 2 {
        let val = ((data[1] as u16) << 8) | (data[2] as u16);
        Ok(val)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Write FPGA register
pub fn write_fpga_register(em100: &Em100, reg: u8, val: u16) -> Result<()> {
    let cmd = [
        0x23u8,
        reg,
        (val >> 8) as u8,
        (val & 0xff) as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}

/// Set FPGA voltage (18 for 1.8V, 33 for 3.3V)
pub fn fpga_set_voltage(em100: &Em100, voltage_code: u8) -> Result<()> {
    let mut cmd = [0u8; 16];
    cmd[0] = 0x24;
    if voltage_code == 18 {
        cmd[2] = 7;
        cmd[3] = 0x80;
    }
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}

/// Get FPGA voltage code from current state
pub fn fpga_get_voltage(em100: &Em100) -> Result<u8> {
    // The voltage is encoded in the FPGA version's high bit
    Ok(if em100.fpga & 0x8000 != 0 { 18 } else { 33 })
}

/// Reconfigure FPGA (alias for reconfig_fpga)
pub fn fpga_reconfigure(em100: &Em100) -> Result<()> {
    let cmd = [0x20u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}
