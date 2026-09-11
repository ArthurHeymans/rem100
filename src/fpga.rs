//! FPGA related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::protocol::{fpga as command, fpga::Register};
use crate::usb;

/// Reconfigure FPGA
pub async fn reconfig_fpga(em100: &mut Em100) -> Result<()> {
    usb::send_command(&mut em100.endpoint_out, command::reconfigure()).await?;

    // Specification says to wait 2s before issuing another USB command
    usb::sleep_ms(2000).await;
    Ok(())
}

/// Check FPGA configuration status
pub async fn check_fpga_status(em100: &mut Em100) -> Result<bool> {
    usb::send_command(&mut em100.endpoint_out, command::status()).await?;

    let data = usb::get_response(&mut em100.endpoint_in, 512).await?;

    if data.len() == 1 {
        Ok(data[0] == 1)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Read FPGA register
pub async fn read_fpga_register(em100: &mut Em100, reg: u8) -> Result<u16> {
    usb::send_command(
        &mut em100.endpoint_out,
        command::read_register(Register::from_raw(reg)),
    )
    .await?;

    let data = usb::get_response(&mut em100.endpoint_in, 3).await?;

    if data.len() == 3 && data[0] == 2 {
        let val = ((data[1] as u16) << 8) | (data[2] as u16);
        Ok(val)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Write FPGA register
pub async fn write_fpga_register(em100: &mut Em100, reg: u8, val: u16) -> Result<()> {
    usb::send_command(
        &mut em100.endpoint_out,
        command::write_register(Register::from_raw(reg), val),
    )
    .await?;
    Ok(())
}

/// Set FPGA voltage (18 for 1.8V, 33 for 3.3V)
pub async fn fpga_set_voltage(em100: &mut Em100, voltage_code: u8) -> Result<()> {
    usb::send_command(&mut em100.endpoint_out, command::set_voltage(voltage_code)).await?;
    Ok(())
}

/// Get FPGA voltage code from current state
pub fn fpga_get_voltage(em100: &Em100) -> Result<u8> {
    // The voltage is encoded in the FPGA version's high bit
    Ok(if em100.fpga & 0x8000 != 0 { 18 } else { 33 })
}

/// Reconfigure FPGA (without waiting)
///
/// This is used internally before switching FPGA voltage, where the caller
/// handles the required 2-second wait after the voltage switch command.
/// For standalone FPGA reconfiguration with proper timing, use `reconfig_fpga`.
pub async fn fpga_reconfigure(em100: &mut Em100) -> Result<()> {
    usb::send_command(&mut em100.endpoint_out, command::reconfigure()).await?;
    Ok(())
}
