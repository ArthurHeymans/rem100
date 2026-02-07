//! Async EM100 device operations for WebUSB (wasm32)
//!
//! This module provides async versions of device operations that work
//! with the WebUSB API in browsers.

use crate::chips::ChipDesc;
use crate::error::{Error, Result};
use crate::web_usb;
use nusb::transfer::{Bulk, In, Out};
use nusb::{Endpoint, Interface};

/// EM100 USB Vendor ID
pub const VENDOR_ID: u16 = 0x04b4;
/// EM100 USB Product ID
pub const PRODUCT_ID: u16 = 0x1235;

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

/// Device information structure
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub mcu_version: String,
    pub fpga_version: String,
    pub hw_version: HwVersion,
    pub serial: String,
    pub fpga_voltage: u16,
}

/// Async EM100 device structure for WebUSB
pub struct Em100Async {
    /// USB interface (held to keep the device claim alive)
    _interface: Interface,
    /// USB bulk OUT endpoint
    pub endpoint_out: Endpoint<Bulk, Out>,
    /// USB bulk IN endpoint
    pub endpoint_in: Endpoint<Bulk, In>,
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

impl Em100Async {
    /// List available EM100 devices
    pub async fn list_devices() -> Result<Vec<nusb::DeviceInfo>> {
        let devices: Vec<_> = nusb::list_devices()
            .await?
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
            .collect();
        Ok(devices)
    }

    /// Request access to an EM100 device via WebUSB permission prompt
    ///
    /// This must be called from a user gesture (e.g., button click) in the browser.
    #[cfg(target_arch = "wasm32")]
    pub async fn request_device() -> Result<nusb::DeviceInfo> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{UsbDevice, UsbDeviceFilter, UsbDeviceRequestOptions};

        web_sys::console::log_1(&"request_device: starting...".into());

        let usb = web_sys::window()
            .ok_or(Error::DeviceNotFound)?
            .navigator()
            .usb();

        // Create filter for EM100 devices
        let filter = UsbDeviceFilter::new();
        filter.set_vendor_id(VENDOR_ID);
        filter.set_product_id(PRODUCT_ID);

        let filters = js_sys::Array::new();
        filters.push(&filter);

        let options = UsbDeviceRequestOptions::new(&filters);

        web_sys::console::log_1(&"request_device: calling usb.request_device()...".into());

        // request_device returns a Promise that resolves to a UsbDevice
        let device_promise = usb.request_device(&options);

        let device_js = JsFuture::from(device_promise).await.map_err(|e| {
            let err = format!("WebUSB request failed: {:?}", e);
            web_sys::console::error_1(&err.clone().into());
            Error::Communication(err)
        })?;

        web_sys::console::log_1(&"request_device: got device from picker".into());

        // Cast to UsbDevice
        let device: UsbDevice = device_js
            .dyn_into()
            .map_err(|_| Error::Communication("Failed to get USB device".to_string()))?;

        web_sys::console::log_1(
            &format!(
                "request_device: device vid=0x{:04x} pid=0x{:04x} opened={}",
                device.vendor_id(),
                device.product_id(),
                device.opened()
            )
            .into(),
        );

        // Use nusb's function to create DeviceInfo from the already-granted device
        web_sys::console::log_1(&"request_device: calling device_info_from_webusb...".into());

        let device_info = nusb::device_info_from_webusb(device).await.map_err(|e| {
            let err = format!("Failed to get device info: {}", e);
            web_sys::console::error_1(&err.clone().into());
            Error::Communication(err)
        })?;

        web_sys::console::log_1(&"request_device: success!".into());
        Ok(device_info)
    }

    /// Open an EM100 device from a DeviceInfo
    pub async fn open(device_info: nusb::DeviceInfo) -> Result<Self> {
        let device = device_info.open().await?;
        let interface = device.claim_interface(0).await?;
        let endpoint_out = interface.endpoint::<Bulk, Out>(ENDPOINT_OUT)?;
        let endpoint_in = interface.endpoint::<Bulk, In>(ENDPOINT_IN)?;

        let mut em100 = Em100Async {
            _interface: interface,
            endpoint_out,
            endpoint_in,
            mcu: 0,
            fpga: 0,
            serial_no: 0,
            hw_version: HwVersion::Unknown,
        };

        em100.init().await?;
        Ok(em100)
    }

    /// Open the first available EM100 device
    pub async fn open_first() -> Result<Self> {
        let devices = Self::list_devices().await?;
        let device_info = devices.into_iter().next().ok_or(Error::DeviceNotFound)?;
        Self::open(device_info).await
    }

    /// Initialize the device
    async fn init(&mut self) -> Result<()> {
        // Check device status
        if !self.check_status().await? {
            return Err(Error::StatusUnknown);
        }

        // Get version information
        self.get_version().await?;

        // Get device info (serial number, hardware version)
        self.get_device_info().await?;

        Ok(())
    }

    /// Check device status by reading SPI flash ID
    async fn check_status(&mut self) -> Result<bool> {
        let id = self.get_spi_flash_id().await?;
        // Check for Micron M25P16 or MX77L12850F
        Ok(id == 0x202015 || id == 0xc27518)
    }

    /// Get firmware version information
    async fn get_version(&mut self) -> Result<()> {
        let cmd = [0x10u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        let data = web_usb::get_response(&mut self.endpoint_in, 512).await?;

        if data.len() == 5 && data[0] == 4 {
            self.mcu = ((data[3] as u16) << 8) | (data[4] as u16);
            self.fpga = ((data[1] as u16) << 8) | (data[2] as u16);
            Ok(())
        } else {
            Err(Error::InvalidResponse)
        }
    }

    /// Get device serial number and hardware version
    async fn get_device_info(&mut self) -> Result<()> {
        let data = self.read_spi_flash_page(0x1fff00).await?;

        self.serial_no = (data[5] as u32) << 24
            | (data[4] as u32) << 16
            | (data[3] as u32) << 8
            | data[2] as u32;
        self.hw_version = HwVersion::from(data[1]);
        Ok(())
    }

    /// Get SPI flash ID
    async fn get_spi_flash_id(&mut self) -> Result<u32> {
        let cmd = [0x30u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        let data = web_usb::get_response(&mut self.endpoint_in, 512).await?;

        if data.len() >= 3 {
            let id = ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | (data[2] as u32);
            Ok(id)
        } else {
            Err(Error::InvalidResponse)
        }
    }

    /// Read a 256-byte page from SPI flash
    async fn read_spi_flash_page(&mut self, address: u32) -> Result<Vec<u8>> {
        let cmd = [
            0x33u8,
            ((address >> 16) & 0xff) as u8,
            ((address >> 8) & 0xff) as u8,
            (address & 0xff) as u8,
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
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        let data = web_usb::get_response(&mut self.endpoint_in, 256).await?;

        if data.len() == 256 {
            Ok(data)
        } else {
            Err(Error::InvalidResponse)
        }
    }

    /// Read FPGA register
    pub async fn read_fpga_register(&mut self, reg: u8) -> Result<u16> {
        let cmd = [0x22u8, reg, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        let data = web_usb::get_response(&mut self.endpoint_in, 3).await?;

        if data.len() == 3 && data[0] == 2 {
            let val = ((data[1] as u16) << 8) | (data[2] as u16);
            Ok(val)
        } else {
            Err(Error::InvalidResponse)
        }
    }

    /// Write FPGA register
    pub async fn write_fpga_register(&mut self, reg: u8, val: u16) -> Result<()> {
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
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;
        Ok(())
    }

    /// Start or stop emulation
    pub async fn set_state(&mut self, run: bool) -> Result<()> {
        self.write_fpga_register(0x28, if run { 1 } else { 0 })
            .await?;

        // Verify the state was actually set (read back and check)
        let actual_state = self.get_state().await?;
        if actual_state != run {
            return Err(Error::OperationFailed(format!(
                "Failed to {} emulation. Device reports: {}",
                if run { "start" } else { "stop" },
                if actual_state { "running" } else { "stopped" }
            )));
        }

        Ok(())
    }

    /// Get current emulation state
    pub async fn get_state(&mut self) -> Result<bool> {
        let state = self.read_fpga_register(0x28).await?;
        Ok(state != 0)
    }

    /// Set address mode (3 or 4 byte)
    pub async fn set_address_mode(&mut self, mode: u8) -> Result<()> {
        if mode != 3 && mode != 4 {
            return Err(Error::InvalidArgument(format!(
                "Invalid address mode: {}",
                mode
            )));
        }
        self.write_fpga_register(0x4f, if mode == 4 { 1 } else { 0 })
            .await?;
        Ok(())
    }

    /// Get current hold pin state
    pub async fn get_hold_pin_state(&mut self) -> Result<HoldPinState> {
        let val = self.read_fpga_register(0x2a).await?;
        match val {
            0 => Ok(HoldPinState::Low),
            2 => Ok(HoldPinState::Float),
            3 => Ok(HoldPinState::Input),
            _ => Err(Error::InvalidResponse),
        }
    }

    /// Set hold pin state
    pub async fn set_hold_pin_state(&mut self, state: HoldPinState) -> Result<()> {
        // Read and acknowledge current state
        let val = self.read_fpga_register(0x2a).await?;
        self.write_fpga_register(0x2a, (1 << 2) | val).await?;

        // Read again
        let _ = self.read_fpga_register(0x2a).await?;

        // Set desired state
        self.write_fpga_register(0x2a, state as u16).await?;

        // Verify
        let new_val = self.read_fpga_register(0x2a).await?;
        if new_val != state as u16 {
            return Err(Error::OperationFailed(format!(
                "Failed to set hold pin state. Expected {:?}, got {}",
                state, new_val
            )));
        }

        Ok(())
    }

    /// Set chip type for emulation
    pub async fn set_chip_type(&mut self, chip: &ChipDesc) -> Result<()> {
        // Stop emulation before changing chip type (matches CLI behavior)
        // Use write_fpga_register directly to avoid verification during chip setup
        self.write_fpga_register(0x28, 0).await?;

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
                if !self.set_fpga_voltage(voltage).await? {
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
            web_usb::send_cmd(&mut self.endpoint_out, entry).await?;
        }

        // Set FPGA registers
        self.write_fpga_register(0xc4, 0x01).await?;
        self.write_fpga_register(0x10, 0x00).await?;
        self.write_fpga_register(0x81, 0x00).await?;

        // Auto-enable 4-byte address mode for large chips (>16MB)
        // This matches CLI behavior in main.rs
        if chip.size > 16 * 1024 * 1024 {
            self.set_address_mode(4).await?;
        }

        Ok(())
    }

    /// Set FPGA voltage (18 for 1.8V, 33 for 3.3V)
    async fn set_fpga_voltage(&mut self, voltage_code: u8) -> Result<bool> {
        // Reconfigure FPGA
        let cmd = [0x20u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        let mut cmd = [0u8; 16];
        cmd[0] = 0x24;
        if voltage_code == 18 {
            cmd[2] = 7;
            cmd[3] = 0x80;
        }
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        // Must wait 2s before issuing any other USB command
        // In wasm, we use a JS timeout
        #[cfg(target_arch = "wasm32")]
        {
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 2000)
                    .unwrap();
            });
            wasm_bindgen_futures::JsFuture::from(promise).await.ok();
        }

        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Verify
        let _ = self.get_version().await;
        let actual = if self.fpga & 0x8000 != 0 { 18 } else { 33 };

        Ok(actual == voltage_code)
    }

    /// Download data to SDRAM
    pub async fn download(&mut self, data: &[u8], address: u32) -> Result<()> {
        self.write_sdram(data, address).await
    }

    /// Upload data from SDRAM
    pub async fn upload(&mut self, address: u32, length: usize) -> Result<Vec<u8>> {
        self.read_sdram(address, length).await
    }

    /// Write data to SDRAM
    ///
    /// Matches CLI protocol: send one command with the full transfer length,
    /// then stream data in 2MB chunks.
    async fn write_sdram(&mut self, data: &[u8], address: u32) -> Result<()> {
        const TRANSFER_LENGTH: usize = 0x200000; // 2MB chunks, matches CLI

        let length = data.len();

        // Send single write command for the entire transfer
        let cmd = [
            0x40u8,
            (address >> 24) as u8,
            (address >> 16) as u8,
            (address >> 8) as u8,
            address as u8,
            (length >> 24) as u8,
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        // Stream data in 2MB chunks
        let mut bytes_sent = 0;
        while bytes_sent < length {
            let chunk_len = std::cmp::min(TRANSFER_LENGTH, length - bytes_sent);
            let chunk = &data[bytes_sent..bytes_sent + chunk_len];
            let actual = web_usb::bulk_write(&mut self.endpoint_out, chunk).await?;
            bytes_sent += actual;

            if actual < chunk_len {
                break;
            }
        }

        if bytes_sent != length {
            return Err(Error::Communication(format!(
                "SDRAM write failed: sent {} of {} bytes",
                bytes_sent, length
            )));
        }

        Ok(())
    }

    /// Read data from SDRAM
    ///
    /// Matches CLI protocol: send one command with the full transfer length,
    /// then read data in 2MB chunks.
    async fn read_sdram(&mut self, address: u32, length: usize) -> Result<Vec<u8>> {
        const TRANSFER_LENGTH: usize = 0x200000; // 2MB chunks, matches CLI

        // Send single read command for the entire transfer
        let cmd = [
            0x41u8,
            (address >> 24) as u8,
            (address >> 16) as u8,
            (address >> 8) as u8,
            address as u8,
            (length >> 24) as u8,
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        web_usb::send_cmd(&mut self.endpoint_out, &cmd).await?;

        // Read data in 2MB chunks
        let mut result = Vec::with_capacity(length);
        let mut bytes_read = 0;

        while bytes_read < length {
            let chunk_len = std::cmp::min(TRANSFER_LENGTH, length - bytes_read);
            let chunk = web_usb::bulk_read(&mut self.endpoint_in, chunk_len).await?;
            let actual = chunk.len();
            result.extend_from_slice(&chunk);
            bytes_read += actual;

            if actual < chunk_len {
                break;
            }
        }

        if bytes_read != length {
            return Err(Error::Communication(format!(
                "SDRAM read failed: read {} of {} bytes",
                bytes_read, length
            )));
        }

        Ok(result)
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
}
