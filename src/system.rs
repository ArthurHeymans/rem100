//! System level operations (version, voltage, LED)

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::protocol::system as command;
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
pub async fn get_version(em100: &mut Em100) -> Result<(u16, u16)> {
    usb::send_command(&mut em100.endpoint_out, command::get_version()).await?;

    let data = usb::get_response(&mut em100.endpoint_in, 512).await?;

    if data.len() == 5 && data[0] == 4 {
        let mcu = ((data[3] as u16) << 8) | (data[4] as u16);
        let fpga = ((data[1] as u16) << 8) | (data[2] as u16);
        Ok((mcu, fpga))
    } else {
        Err(Error::InvalidResponse)
    }
}

/// Set voltage on a channel
pub async fn set_voltage(em100: &mut Em100, channel: SetVoltageChannel, mv: u16) -> Result<()> {
    if matches!(channel, SetVoltageChannel::BufferVcc) && mv != 18 && mv != 25 && mv != 33 {
        return Err(Error::InvalidArgument(
            "For Buffer VCC, voltage needs to be 1.8V, 2.5V or 3.3V".to_string(),
        ));
    }

    usb::send_command(
        &mut em100.endpoint_out,
        command::set_voltage(channel as u8, mv),
    )
    .await?;
    Ok(())
}

/// Get voltage from a channel (returns millivolts)
pub async fn get_voltage(em100: &mut Em100, channel: GetVoltageChannel) -> Result<u32> {
    usb::send_command(&mut em100.endpoint_out, command::get_voltage(channel as u8)).await?;

    let data = usb::get_response(&mut em100.endpoint_in, 512).await?;

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
pub async fn set_led(em100: &mut Em100, state: LedState) -> Result<()> {
    usb::send_command(&mut em100.endpoint_out, command::set_led(state as u8)).await?;
    Ok(())
}
