//! System level operations (version, voltage, LED)

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::usb;

/// Channels for setting voltage
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum SetVoltageChannel {
    TriggerVcc = 0,
    ResetVcc = 1,
    RefPlus = 2,
    RefMinus = 3,
    BufferVcc = 4,
}

/// Channels for getting voltage
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum GetVoltageChannel {
    V1_2 = 0,
    EVcc = 1,
    RefPlus = 2,
    RefMinus = 3,
    BufferVcc = 4,
    TriggerVcc = 5,
    ResetVcc = 6,
    V3_3 = 7,
    BufferV3_3 = 8,
    V5 = 9,
}

/// LED states
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum LedState {
    BothOff = 0,
    GreenOn = 1,
    RedOn = 2,
    BothOn = 3,
}

/// Get firmware version information
///
/// Returns (MCU version, FPGA version)
pub fn get_version(em100: &Em100) -> Result<(u16, u16)> {
    let cmd = [0x10u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(em100, &cmd)?;

    let data = usb::get_response(em100, 512)?;

    if data.len() == 5 && data[0] == 4 {
        let mcu = ((data[3] as u16) << 8) | (data[4] as u16);
        let fpga = ((data[1] as u16) << 8) | (data[2] as u16);
        Ok((mcu, fpga))
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Set voltage on a channel
pub fn set_voltage(em100: &Em100, channel: SetVoltageChannel, mv: u16) -> Result<()> {
    if matches!(channel, SetVoltageChannel::BufferVcc) && mv != 18 && mv != 25 && mv != 33 {
        return Err(Error::InvalidArgument(
            "For Buffer VCC, voltage needs to be 1.8V, 2.5V or 3.3V".to_string(),
        ));
    }

    let cmd = [
        0x11,
        channel as u8,
        (mv >> 8) as u8,
        (mv & 0xff) as u8,
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
    usb::send_cmd(em100, &cmd)?;
    Ok(())
}

/// Get voltage from a channel (returns millivolts)
pub fn get_voltage(em100: &Em100, channel: GetVoltageChannel) -> Result<u32> {
    let cmd = [
        0x12,
        channel as u8,
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
        0,
        0,
    ];
    usb::send_cmd(em100, &cmd)?;

    let data = usb::get_response(em100, 512)?;

    if data.len() == 3 && data[0] == 2 {
        let raw_voltage = ((data[1] as u32) << 8) | (data[2] as u32);

        let voltage = match channel {
            GetVoltageChannel::V1_2
            | GetVoltageChannel::EVcc
            | GetVoltageChannel::RefPlus
            | GetVoltageChannel::RefMinus => {
                // Each step is 5V/4096 (about 1.22mV)
                raw_voltage * 12207 / 10000
            }
            _ => {
                // Each step is 5V/1024 (about 4.88mV)
                raw_voltage * 48828 / 10000
            }
        };

        Ok(voltage)
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Set LED state
pub fn set_led(em100: &Em100, state: LedState) -> Result<()> {
    let cmd = [0x13, state as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(em100, &cmd)?;
    Ok(())
}
