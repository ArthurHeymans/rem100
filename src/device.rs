//! Core EM100 device structure and operations

use crate::chips::ChipDesc;
use crate::error::{Error, Result};
use crate::fpga;
use crate::sdram;
use crate::spi;
use crate::system;
use crate::usb;
use nusb::Interface;
use std::time::Duration;

/// EM100 USB Vendor ID
pub const VENDOR_ID: u16 = 0x04b4;
/// EM100 USB Product ID  
pub const PRODUCT_ID: u16 = 0x1235;

/// USB bulk transfer timeout in milliseconds
pub const BULK_SEND_TIMEOUT: Duration = Duration::from_millis(5000);

/// Hardware versions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HwVersion {
    /// Early EM100Pro (hardware version 0xff)
    Em100ProEarly = 0xff,
    /// EM100Pro (hardware version 0x04)
    Em100Pro = 0x04,
    /// EM100Pro-G2 (hardware version 0x06)
    Em100ProG2 = 0x06,
    /// Unknown hardware version
    Unknown = 0x00,
}

impl From<u8> for HwVersion {
    fn from(v: u8) -> Self {
        match v {
            0xff => HwVersion::Em100ProEarly,
            0x04 => HwVersion::Em100Pro,
            0x06 => HwVersion::Em100ProG2,
            _ => HwVersion::Unknown,
        }
    }
}

impl std::fmt::Display for HwVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HwVersion::Em100ProEarly => write!(f, "EM100Pro (early)"),
            HwVersion::Em100Pro => write!(f, "EM100Pro"),
            HwVersion::Em100ProG2 => write!(f, "EM100Pro-G2"),
            HwVersion::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Hold pin states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldPinState {
    Float = 0x2,
    Low = 0x0,
    Input = 0x3,
}

impl std::str::FromStr for HoldPinState {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_str() {
            "FLOAT" => Ok(HoldPinState::Float),
            "LOW" => Ok(HoldPinState::Low),
            "INPUT" => Ok(HoldPinState::Input),
            _ => Err(Error::InvalidArgument(format!(
                "Invalid hold pin state: {}",
                s
            ))),
        }
    }
}

impl std::fmt::Display for HoldPinState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HoldPinState::Float => write!(f, "float"),
            HoldPinState::Low => write!(f, "low"),
            HoldPinState::Input => write!(f, "input"),
        }
    }
}

/// EM100 device structure
pub struct Em100 {
    /// USB interface
    pub interface: Interface,
    /// MCU firmware version
    pub mcu: u16,
    /// FPGA firmware version
    pub fpga: u16,
    /// Device serial number
    pub serial_no: u32,
    /// Hardware version
    pub hw_version: HwVersion,
}

impl Em100 {
    /// Open an EM100 device
    ///
    /// If bus and device are specified, opens the device at that location.
    /// If serial_number is specified, opens the device with that serial number.
    /// Otherwise, opens the first EM100 device found.
    pub fn open(bus: Option<u8>, device: Option<u8>, serial_number: Option<u32>) -> Result<Self> {
        let interface = if let (Some(bus), Some(dev)) = (bus, device) {
            // Find device by bus:device
            Self::open_by_bus_device(bus, dev)?
        } else if let Some(serial) = serial_number {
            // Find device by serial number - need to open each and check
            Self::open_by_serial(serial)?
        } else {
            // Open first available device
            Self::open_first()?
        };

        let mut em100 = Em100 {
            interface,
            mcu: 0,
            fpga: 0,
            serial_no: 0,
            hw_version: HwVersion::Unknown,
        };

        em100.init()?;
        Ok(em100)
    }

    fn open_first() -> Result<Interface> {
        for device in nusb::list_devices()? {
            if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                let dev = device.open()?;
                return Ok(dev.claim_interface(0)?);
            }
        }
        Err(Error::DeviceNotFound)
    }

    fn open_by_bus_device(bus: u8, dev: u8) -> Result<Interface> {
        for device in nusb::list_devices()? {
            if device.bus_number() == bus && device.device_address() == dev {
                if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                    let usb_dev = device.open()?;
                    return Ok(usb_dev.claim_interface(0)?);
                } else {
                    return Err(Error::InvalidArgument(format!(
                        "USB device on bus {:03}:{:02} is not an EM100pro",
                        bus, dev
                    )));
                }
            }
        }
        Err(Error::DeviceNotFound)
    }

    fn open_by_serial(serial: u32) -> Result<Interface> {
        for device in nusb::list_devices()? {
            if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                let usb_dev = device.open()?;
                let interface = usb_dev.claim_interface(0)?;
                let mut em100 = Em100 {
                    interface,
                    mcu: 0,
                    fpga: 0,
                    serial_no: 0,
                    hw_version: HwVersion::Unknown,
                };

                // Try to init and check serial
                if em100.init().is_ok() && em100.serial_no == serial {
                    return Ok(em100.interface);
                }
            }
        }
        Err(Error::DeviceNotFound)
    }

    /// Initialize the device
    fn init(&mut self) -> Result<()> {
        // nusb handles kernel driver detachment and interface claiming automatically

        // Check device status
        if !self.check_status()? {
            return Err(Error::StatusUnknown);
        }

        // Get version information
        self.get_version()?;

        // Get device info (serial number, hardware version)
        self.get_device_info()?;

        Ok(())
    }

    /// Check device status by reading SPI flash ID
    fn check_status(&self) -> Result<bool> {
        let id = spi::get_spi_flash_id(self)?;
        // Check for Micron M25P16 or MX77L12850F
        Ok(id == 0x202015 || id == 0xc27518)
    }

    /// Get firmware version information
    fn get_version(&mut self) -> Result<()> {
        let (mcu, fpga) = system::get_version(self)?;
        self.mcu = mcu;
        self.fpga = fpga;
        Ok(())
    }

    /// Get device serial number and hardware version
    fn get_device_info(&mut self) -> Result<()> {
        let mut data = [0u8; 256];
        spi::read_spi_flash_page(self, 0x1fff00, &mut data)?;

        self.serial_no =
            (data[5] as u32) << 24 | (data[4] as u32) << 16 | (data[3] as u32) << 8 | data[2] as u32;
        self.hw_version = HwVersion::from(data[1]);
        Ok(())
    }

    /// Start or stop emulation
    pub fn set_state(&self, run: bool) -> Result<()> {
        fpga::write_fpga_register(self, 0x28, if run { 1 } else { 0 })?;
        println!("{} EM100Pro", if run { "Started" } else { "Stopped" });
        Ok(())
    }

    /// Get current emulation state
    pub fn get_state(&self) -> Result<bool> {
        let state = fpga::read_fpga_register(self, 0x28)?;
        Ok(state != 0)
    }

    /// Set address mode (3 or 4 byte)
    pub fn set_address_mode(&self, mode: u8) -> Result<()> {
        if mode != 3 && mode != 4 {
            return Err(Error::InvalidArgument(format!(
                "Invalid address mode: {}",
                mode
            )));
        }
        fpga::write_fpga_register(self, 0x4f, if mode == 4 { 1 } else { 0 })?;
        println!("Enabled {} byte address mode", mode);
        Ok(())
    }

    /// Get current hold pin state
    pub fn get_hold_pin_state(&self) -> Result<HoldPinState> {
        let val = fpga::read_fpga_register(self, 0x2a)?;
        match val {
            0 => Ok(HoldPinState::Low),
            2 => Ok(HoldPinState::Float),
            3 => Ok(HoldPinState::Input),
            _ => Err(Error::InvalidResponse),
        }
    }

    /// Set hold pin state
    pub fn set_hold_pin_state(&self, state: HoldPinState) -> Result<()> {
        // Read and acknowledge current state
        let val = fpga::read_fpga_register(self, 0x2a)?;
        fpga::write_fpga_register(self, 0x2a, (1 << 2) | val)?;

        // Read again
        let _ = fpga::read_fpga_register(self, 0x2a)?;

        // Set desired state
        fpga::write_fpga_register(self, 0x2a, state as u16)?;

        // Verify
        let new_val = fpga::read_fpga_register(self, 0x2a)?;
        if new_val != state as u16 {
            return Err(Error::OperationFailed(format!(
                "Failed to set hold pin state. Expected {:?}, got {}",
                state, new_val
            )));
        }

        println!("Hold pin state set to {}", state);
        Ok(())
    }

    /// Set chip type for emulation
    pub fn set_chip_type(&mut self, chip: &ChipDesc) -> Result<()> {
        println!("Configuring SPI flash chip emulation.");

        let fpga_voltage = if self.fpga & 0x8000 != 0 { 1800 } else { 3300 };

        // Check if we need to switch FPGA voltage
        for entry in chip.init.iter().take(chip.init_len) {
            if entry[0] != 0x11 || entry[1] != 0x04 {
                continue;
            }

            let chip_voltage = ((entry[2] as u16) << 8) | (entry[3] as u16);

            let req_voltage = match chip_voltage {
                1601 | 1800 if fpga_voltage == 3300 => Some(18),
                3300 if fpga_voltage == 1800 => Some(33),
                _ => None,
            };

            if let Some(voltage) = req_voltage {
                if !self.set_fpga_voltage(voltage)? {
                    return Err(Error::OperationFailed(format!(
                        "The current FPGA firmware ({:.1}V) does not support {} {} ({:.1}V)",
                        fpga_voltage as f32 / 1000.0,
                        chip.vendor,
                        chip.name,
                        chip_voltage as f32 / 1000.0
                    )));
                }
            }
            break;
        }

        // Send init sequence
        for entry in chip.init.iter().take(chip.init_len) {
            usb::send_cmd(&self.interface, entry)?;
        }

        // Set FPGA registers
        fpga::write_fpga_register(self, 0xc4, 0x01)?;
        fpga::write_fpga_register(self, 0x10, 0x00)?;
        fpga::write_fpga_register(self, 0x81, 0x00)?;

        Ok(())
    }

    /// Set FPGA voltage (18 for 1.8V, 33 for 3.3V)
    pub fn set_fpga_voltage(&mut self, voltage_code: u8) -> Result<bool> {
        fpga::fpga_reconfigure(self)?;

        let mut cmd = [0u8; 16];
        cmd[0] = 0x24;
        if voltage_code == 18 {
            cmd[2] = 7;
            cmd[3] = 0x80;
        }
        usb::send_cmd(&self.interface, &cmd)?;

        // Must wait 2s before issuing any other USB command
        std::thread::sleep(Duration::from_secs(2));

        // Verify
        self.get_version().ok();
        let actual = if self.fpga & 0x8000 != 0 { 18 } else { 33 };

        if actual != voltage_code {
            return Ok(false);
        }

        println!(
            "Voltage set to {}",
            if voltage_code == 18 { "1.8V" } else { "3.3V" }
        );
        Ok(true)
    }

    /// Set serial number
    pub fn set_serial_no(&mut self, serial: u32) -> Result<()> {
        let mut data = [0u8; 512];
        spi::read_spi_flash_page(self, 0x1fff00, &mut data[..256])?;

        let old_serial =
            (data[5] as u32) << 24 | (data[4] as u32) << 16 | (data[3] as u32) << 8 | data[2] as u32;

        if old_serial == serial {
            println!("Serial number unchanged.");
            return Ok(());
        }

        data[2] = serial as u8;
        data[3] = (serial >> 8) as u8;
        data[4] = (serial >> 16) as u8;
        data[5] = (serial >> 24) as u8;

        if old_serial != 0xffffffff {
            // Preserve magic
            spi::read_spi_flash_page(self, 0x1f0000, &mut data[256..512])?;
            spi::unlock_spi_flash(self)?;
            spi::get_spi_flash_id(self)?;
            spi::erase_spi_flash_sector(self, 0x1f)?;
            spi::write_spi_flash_page(self, 0x1f0000, &data[256..512])?;
        }

        spi::write_spi_flash_page(self, 0x1fff00, &data[..256])?;

        // Re-read serial number
        self.get_device_info()?;

        if self.serial_no != 0xffffffff {
            let prefix = if self.hw_version == HwVersion::Em100ProEarly {
                "DP"
            } else {
                "EM"
            };
            println!("New serial number: {}{:06}", prefix, self.serial_no);
        } else {
            println!("New serial number: N.A.");
        }

        Ok(())
    }

    /// Download data to SDRAM
    pub fn download(&self, data: &[u8], address: u32) -> Result<()> {
        sdram::write_sdram(self, data, address)
    }

    /// Upload data from SDRAM
    pub fn upload(&self, address: u32, length: usize) -> Result<Vec<u8>> {
        sdram::read_sdram(self, address, length)
    }

    /// Get serial number as string
    pub fn serial_string(&self) -> String {
        if self.serial_no == 0xffffffff {
            "N.A.".to_string()
        } else {
            let prefix = if self.hw_version == HwVersion::Em100ProEarly {
                "DP"
            } else {
                "EM"
            };
            format!("{}{:06}", prefix, self.serial_no)
        }
    }

    /// Print device information
    pub fn print_info(&self) {
        match self.hw_version {
            HwVersion::Em100Pro | HwVersion::Em100ProEarly => {
                println!("MCU version: {}.{:02}", self.mcu >> 8, self.mcu & 0xff);
                if self.fpga > 0x0033 {
                    println!(
                        "FPGA version: {}.{:02} ({})",
                        (self.fpga >> 8) & 0x7f,
                        self.fpga & 0xff,
                        if self.fpga & 0x8000 != 0 {
                            "1.8V"
                        } else {
                            "3.3V"
                        }
                    );
                } else {
                    println!(
                        "FPGA version: {}.{:02}",
                        self.fpga >> 8,
                        self.fpga & 0xff
                    );
                }
            }
            HwVersion::Em100ProG2 => {
                println!("MCU version: {}.{}", self.mcu >> 8, self.mcu & 0xff);
                println!(
                    "FPGA version: {}.{:03}",
                    (self.fpga >> 8) & 0x7f,
                    self.fpga & 0xff
                );
            }
            _ => {
                println!("MCU version: {}.{}", self.mcu >> 8, self.mcu & 0xff);
                println!("FPGA version: {}.{}", self.fpga >> 8, self.fpga & 0xff);
            }
        }

        println!("Hardware version: {:?}", self.hw_version);
        println!("Serial number: {}", self.serial_string());
    }

    /// Debug mode - print voltages and FPGA registers
    pub fn debug(&self) -> Result<()> {
        println!("Voltages:");
        system::set_led(self, system::LedState::BothOff)?;
        println!("  1.2V:        {}mV", system::get_voltage(self, system::GetVoltageChannel::V1_2)?);
        println!("  E_VCC:       {}mV", system::get_voltage(self, system::GetVoltageChannel::EVcc)?);
        system::set_led(self, system::LedState::BothOn)?;
        println!("  REF+:        {}mV", system::get_voltage(self, system::GetVoltageChannel::RefPlus)?);
        println!("  REF-:        {}mV", system::get_voltage(self, system::GetVoltageChannel::RefMinus)?);
        system::set_led(self, system::LedState::RedOn)?;
        println!("  Buffer VCC:  {}mV", system::get_voltage(self, system::GetVoltageChannel::BufferVcc)?);
        println!("  Trig VCC:    {}mV", system::get_voltage(self, system::GetVoltageChannel::TriggerVcc)?);
        system::set_led(self, system::LedState::BothOn)?;
        println!("  RST VCC:     {}mV", system::get_voltage(self, system::GetVoltageChannel::ResetVcc)?);
        println!("  3.3V:        {}mV", system::get_voltage(self, system::GetVoltageChannel::V3_3)?);
        system::set_led(self, system::LedState::RedOn)?;
        println!("  Buffer 3.3V: {}mV", system::get_voltage(self, system::GetVoltageChannel::BufferV3_3)?);
        println!("  5V:          {}mV", system::get_voltage(self, system::GetVoltageChannel::V5)?);
        system::set_led(self, system::LedState::GreenOn)?;

        println!("\nFPGA registers:");
        for i in (0..256).step_by(2) {
            if i % 16 == 0 {
                print!("\n  {:04x}: ", i);
            }
            match fpga::read_fpga_register(self, i as u8) {
                Ok(val) => print!("{:04x} ", val),
                Err(_) => print!("XXXX "),
            }
        }
        println!();

        Ok(())
    }
}

/// List all connected EM100 devices
pub fn list_devices() -> Result<Vec<(u8, u8, String)>> {
    let mut devices = Vec::new();

    for device in nusb::list_devices()? {
        if device.vendor_id() != VENDOR_ID || device.product_id() != PRODUCT_ID {
            continue;
        }

        let bus = device.bus_number();
        let addr = device.device_address();

        // Try to get serial number
        match Em100::open(Some(bus), Some(addr), None) {
            Ok(em100) => {
                devices.push((bus, addr, em100.serial_string()));
            }
            Err(_) => {
                devices.push((bus, addr, "unknown".to_string()));
            }
        }
    }

    Ok(devices)
}
