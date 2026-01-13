//! Core EM100 device structure and operations

use crate::chips::ChipDesc;
use crate::error::{Error, Result};
use crate::fpga;
use crate::sdram;
use crate::spi;
use crate::system;
use crate::usb;
use nusb::transfer::{Bulk, In, Out};
use nusb::{Endpoint, MaybeFuture};
use std::cell::RefCell;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoldPinState {
    #[default]
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
    /// USB bulk OUT endpoint
    pub endpoint_out: RefCell<Endpoint<Bulk, Out>>,
    /// USB bulk IN endpoint
    pub endpoint_in: RefCell<Endpoint<Bulk, In>>,
    /// MCU firmware version
    pub mcu: u16,
    /// FPGA firmware version
    pub fpga: u16,
    /// Device serial number
    pub serial_no: u32,
    /// Hardware version
    pub hw_version: HwVersion,
}

/// USB endpoint addresses
const ENDPOINT_OUT: u8 = 0x01;
const ENDPOINT_IN: u8 = 0x82;

impl Em100 {
    /// Open an EM100 device
    ///
    /// If bus and device are specified, opens the device at that location.
    /// If serial_number is specified, opens the device with that serial number.
    /// Otherwise, opens the first EM100 device found.
    pub fn open(bus: Option<u8>, device: Option<u8>, serial_number: Option<u32>) -> Result<Self> {
        let (endpoint_out, endpoint_in) = if let (Some(bus), Some(dev)) = (bus, device) {
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
            endpoint_out: RefCell::new(endpoint_out),
            endpoint_in: RefCell::new(endpoint_in),
            mcu: 0,
            fpga: 0,
            serial_no: 0,
            hw_version: HwVersion::Unknown,
        };

        em100.init()?;
        Ok(em100)
    }

    fn open_first() -> Result<(Endpoint<Bulk, Out>, Endpoint<Bulk, In>)> {
        for device in nusb::list_devices().wait()? {
            if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                let dev = device.open().wait()?;
                let interface = dev.claim_interface(0).wait()?;
                let endpoint_out = interface.endpoint::<Bulk, Out>(ENDPOINT_OUT)?;
                let endpoint_in = interface.endpoint::<Bulk, In>(ENDPOINT_IN)?;
                return Ok((endpoint_out, endpoint_in));
            }
        }
        Err(Error::DeviceNotFound)
    }

    fn open_by_bus_device(bus: u8, dev: u8) -> Result<(Endpoint<Bulk, Out>, Endpoint<Bulk, In>)> {
        for device in nusb::list_devices().wait()? {
            if device.busnum() == bus && device.device_address() == dev {
                if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                    let usb_dev = device.open().wait()?;
                    let interface = usb_dev.claim_interface(0).wait()?;
                    let endpoint_out = interface.endpoint::<Bulk, Out>(ENDPOINT_OUT)?;
                    let endpoint_in = interface.endpoint::<Bulk, In>(ENDPOINT_IN)?;
                    return Ok((endpoint_out, endpoint_in));
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

    fn open_by_serial(serial: u32) -> Result<(Endpoint<Bulk, Out>, Endpoint<Bulk, In>)> {
        for device in nusb::list_devices().wait()? {
            if device.vendor_id() == VENDOR_ID && device.product_id() == PRODUCT_ID {
                let usb_dev = device.open().wait()?;
                let interface = usb_dev.claim_interface(0).wait()?;
                let endpoint_out = interface.endpoint::<Bulk, Out>(ENDPOINT_OUT)?;
                let endpoint_in = interface.endpoint::<Bulk, In>(ENDPOINT_IN)?;
                let mut em100 = Em100 {
                    endpoint_out: RefCell::new(endpoint_out),
                    endpoint_in: RefCell::new(endpoint_in),
                    mcu: 0,
                    fpga: 0,
                    serial_no: 0,
                    hw_version: HwVersion::Unknown,
                };

                // Try to init and check serial
                if em100.init().is_ok() && em100.serial_no == serial {
                    // Re-extract the endpoints (can't return from a moved em100)
                    let endpoint_out = em100.endpoint_out.into_inner();
                    let endpoint_in = em100.endpoint_in.into_inner();
                    return Ok((endpoint_out, endpoint_in));
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

        self.serial_no = (data[5] as u32) << 24
            | (data[4] as u32) << 16
            | (data[3] as u32) << 8
            | data[2] as u32;
        self.hw_version = HwVersion::from(data[1]);
        Ok(())
    }

    /// Start or stop emulation
    pub fn set_state(&self, run: bool) -> Result<()> {
        fpga::write_fpga_register(self, 0x28, if run { 1 } else { 0 })?;
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

        Ok(())
    }

    /// Set chip type for emulation
    pub fn set_chip_type(&mut self, chip: &ChipDesc) -> Result<()> {
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
            usb::send_cmd(self, entry)?;
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
        usb::send_cmd(self, &cmd)?;

        // Must wait 2s before issuing any other USB command
        std::thread::sleep(Duration::from_secs(2));

        // Verify
        self.get_version().ok();
        let actual = if self.fpga & 0x8000 != 0 { 18 } else { 33 };

        if actual != voltage_code {
            return Ok(false);
        }

        Ok(true)
    }

    /// Set serial number
    pub fn set_serial_no(&mut self, serial: u32) -> Result<()> {
        let mut data = [0u8; 512];
        spi::read_spi_flash_page(self, 0x1fff00, &mut data[..256])?;

        let old_serial = (data[5] as u32) << 24
            | (data[4] as u32) << 16
            | (data[3] as u32) << 8
            | data[2] as u32;

        if old_serial == serial {
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

    /// Get device information as structured data
    pub fn get_info(&self) -> DeviceInfo {
        let mcu_version = format!("{}.{:02}", self.mcu >> 8, self.mcu & 0xff);

        let fpga_version = match self.hw_version {
            HwVersion::Em100Pro | HwVersion::Em100ProEarly => {
                if self.fpga > 0x0033 {
                    format!(
                        "{}.{:02} ({})",
                        (self.fpga >> 8) & 0x7f,
                        self.fpga & 0xff,
                        if self.fpga & 0x8000 != 0 {
                            "1.8V"
                        } else {
                            "3.3V"
                        }
                    )
                } else {
                    format!("{}.{:02}", self.fpga >> 8, self.fpga & 0xff)
                }
            }
            HwVersion::Em100ProG2 => {
                format!("{}.{:03}", (self.fpga >> 8) & 0x7f, self.fpga & 0xff)
            }
            _ => format!("{}.{}", self.fpga >> 8, self.fpga & 0xff),
        };

        DeviceInfo {
            mcu_version,
            fpga_version,
            hw_version: self.hw_version,
            serial: self.serial_string(),
            fpga_voltage: if self.fpga & 0x8000 != 0 { 1800 } else { 3300 },
        }
    }

    /// Print device information (CLI convenience)
    #[cfg(feature = "cli")]
    pub fn print_info(&self) {
        let info = self.get_info();
        println!("MCU version: {}", info.mcu_version);
        println!("FPGA version: {}", info.fpga_version);
        println!("Hardware version: {:?}", info.hw_version);
        println!("Serial number: {}", info.serial);
    }

    /// Get debug information (voltages and FPGA registers)
    pub fn get_debug_info(&self) -> Result<DebugInfo> {
        system::set_led(self, system::LedState::BothOff)?;
        let v1_2 = system::get_voltage(self, system::GetVoltageChannel::V1_2)?;
        let e_vcc = system::get_voltage(self, system::GetVoltageChannel::EVcc)?;
        system::set_led(self, system::LedState::BothOn)?;
        let ref_plus = system::get_voltage(self, system::GetVoltageChannel::RefPlus)?;
        let ref_minus = system::get_voltage(self, system::GetVoltageChannel::RefMinus)?;
        system::set_led(self, system::LedState::RedOn)?;
        let buffer_vcc = system::get_voltage(self, system::GetVoltageChannel::BufferVcc)?;
        let trig_vcc = system::get_voltage(self, system::GetVoltageChannel::TriggerVcc)?;
        system::set_led(self, system::LedState::BothOn)?;
        let rst_vcc = system::get_voltage(self, system::GetVoltageChannel::ResetVcc)?;
        let v3_3 = system::get_voltage(self, system::GetVoltageChannel::V3_3)?;
        system::set_led(self, system::LedState::RedOn)?;
        let buffer_v3_3 = system::get_voltage(self, system::GetVoltageChannel::BufferV3_3)?;
        let v5 = system::get_voltage(self, system::GetVoltageChannel::V5)?;
        system::set_led(self, system::LedState::GreenOn)?;

        let mut fpga_registers = [0u16; 128];
        for i in 0..128 {
            fpga_registers[i] = fpga::read_fpga_register(self, (i * 2) as u8).unwrap_or(0xFFFF);
        }

        Ok(DebugInfo {
            voltages: Voltages {
                v1_2,
                e_vcc,
                ref_plus,
                ref_minus,
                buffer_vcc,
                trig_vcc,
                rst_vcc,
                v3_3,
                buffer_v3_3,
                v5,
            },
            fpga_registers,
        })
    }

    /// Debug mode - print voltages and FPGA registers (CLI convenience)
    #[cfg(feature = "cli")]
    pub fn debug(&self) -> Result<()> {
        let info = self.get_debug_info()?;

        println!("Voltages:");
        println!("  1.2V:        {}mV", info.voltages.v1_2);
        println!("  E_VCC:       {}mV", info.voltages.e_vcc);
        println!("  REF+:        {}mV", info.voltages.ref_plus);
        println!("  REF-:        {}mV", info.voltages.ref_minus);
        println!("  Buffer VCC:  {}mV", info.voltages.buffer_vcc);
        println!("  Trig VCC:    {}mV", info.voltages.trig_vcc);
        println!("  RST VCC:     {}mV", info.voltages.rst_vcc);
        println!("  3.3V:        {}mV", info.voltages.v3_3);
        println!("  Buffer 3.3V: {}mV", info.voltages.buffer_v3_3);
        println!("  5V:          {}mV", info.voltages.v5);

        println!("\nFPGA registers:");
        for i in 0..128 {
            if i % 8 == 0 {
                print!("\n  {:04x}: ", i * 2);
            }
            print!("{:04x} ", info.fpga_registers[i]);
        }
        println!();

        Ok(())
    }
}

/// Device information structure
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub mcu_version: String,
    pub fpga_version: String,
    pub hw_version: HwVersion,
    pub serial: String,
    pub fpga_voltage: u16,
}

/// Voltage readings
#[derive(Debug, Clone, Copy)]
pub struct Voltages {
    pub v1_2: u32,
    pub e_vcc: u32,
    pub ref_plus: u32,
    pub ref_minus: u32,
    pub buffer_vcc: u32,
    pub trig_vcc: u32,
    pub rst_vcc: u32,
    pub v3_3: u32,
    pub buffer_v3_3: u32,
    pub v5: u32,
}

/// Debug information structure
#[derive(Debug, Clone)]
pub struct DebugInfo {
    pub voltages: Voltages,
    pub fpga_registers: [u16; 128],
}

/// List all connected EM100 devices
pub fn list_devices() -> Result<Vec<(u8, u8, String)>> {
    let mut devices = Vec::new();

    for device in nusb::list_devices().wait()? {
        if device.vendor_id() != VENDOR_ID || device.product_id() != PRODUCT_ID {
            continue;
        }

        let bus = device.busnum();
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
