//! rem100 - EM100Pro SPI flash emulator command-line utility
//!
//! A Rust port of the em100 utility for controlling the Dediprog EM100Pro
//! SPI flash emulator hardware.

use clap::Parser;
use rem100::chips::ChipDatabase;
use rem100::device::{list_devices, Em100, HoldPinState};
use rem100::download::update_all_files;
use rem100::firmware::{firmware_dump, firmware_update};
use rem100::image::autocorrect_image;
use rem100::trace::{self, TraceState};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// EM100Pro command-line utility
#[derive(Parser, Debug)]
#[command(name = "rem100")]
#[command(author = "Google Inc., Rust port contributors")]
#[command(version = "0.1.0")]
#[command(about = "EM100Pro SPI flash emulator command-line utility")]
#[command(long_about = "A Rust port of the em100 utility for controlling the Dediprog EM100Pro SPI flash emulator hardware.

Example:
  rem100 --stop --set M25P80 -d file.bin -v --start -t -O 0xfff00000")]
struct Args {
    /// Select chip emulation
    #[arg(short = 'c', long = "set")]
    chip: Option<String>,

    /// Download FILE into EM100pro
    #[arg(short = 'd', long = "download")]
    download: Option<String>,

    /// Start address for download (e.g., -a 0x300000)
    #[arg(short = 'a', long = "start-address")]
    start_address: Option<String>,

    /// Force 3 or 4 byte address mode
    #[arg(short = 'm', long = "address-mode")]
    address_mode: Option<u8>,

    /// Upload from EM100pro into FILE
    #[arg(short = 'u', long = "upload")]
    upload: Option<String>,

    /// Start emulation
    #[arg(short = 'r', long = "start")]
    start: bool,

    /// Stop emulation
    #[arg(short = 's', long = "stop")]
    stop: bool,

    /// Verify EM100 content matches the file
    #[arg(short = 'v', long = "verify")]
    verify: bool,

    /// Enable trace mode
    #[arg(short = 't', long = "trace")]
    trace: bool,

    /// Address offset for trace mode (hex)
    #[arg(short = 'O', long = "offset")]
    offset: Option<String>,

    /// Enable terminal mode
    #[arg(short = 'T', long = "terminal")]
    terminal: bool,

    /// Enable trace console mode
    #[arg(short = 'R', long = "traceconsole")]
    traceconsole: bool,

    /// Length of buffer for traceconsole mode (hex)
    #[arg(short = 'L', long = "length")]
    length: Option<String>,

    /// Brief mode for traces
    #[arg(short = 'b', long = "brief")]
    brief: bool,

    /// Update EM100pro firmware (dangerous). Use "auto" for automatic update.
    #[arg(short = 'F', long = "firmware-update")]
    firmware_update: Option<String>,

    /// Export raw EM100pro firmware to file
    #[arg(short = 'f', long = "firmware-dump")]
    firmware_dump: Option<String>,

    /// Export EM100pro firmware to DPFW file
    #[arg(short = 'g', long = "firmware-write")]
    firmware_write: Option<String>,

    /// Set serial number
    #[arg(short = 'S', long = "set-serialno")]
    set_serialno: Option<String>,

    /// Switch FPGA voltage (1.8 or 3.3) - obsolete
    #[arg(short = 'V', long = "set-voltage")]
    set_voltage: Option<String>,

    /// Set hold pin state (LOW, FLOAT, INPUT)
    #[arg(short = 'p', long = "holdpin")]
    holdpin: Option<String>,

    /// Use EM100pro on USB bus:device or serial number (e.g., 001:003 or EM123456)
    #[arg(short = 'x', long = "device")]
    device: Option<String>,

    /// List all connected EM100pro devices
    #[arg(short = 'l', long = "list-devices")]
    list_devices: bool,

    /// Update device (chip) and firmware database
    #[arg(short = 'U', long = "update-files")]
    update_files: bool,

    /// Enable compatibility mode (patch image for EM100Pro)
    #[arg(short = 'C', long = "compatible")]
    compatible: bool,

    /// Print debug information
    #[arg(short = 'D', long = "debug")]
    debug: bool,
}

fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_device(s: &str) -> (Option<u8>, Option<u8>, Option<u32>) {
    let s = s.to_uppercase();
    if s.starts_with("DP") || s.starts_with("EM") {
        // Serial number
        if let Ok(serial) = s[2..].parse::<u32>() {
            return (None, None, Some(serial));
        }
    } else if s.contains(':') {
        // Bus:device
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            if let (Ok(bus), Ok(dev)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>()) {
                return (Some(bus), Some(dev), None);
            }
        }
    }
    (None, None, None)
}

fn main() {
    let args = Args::parse();

    // Handle --list-devices
    if args.list_devices {
        match list_devices() {
            Ok(devices) => {
                if devices.is_empty() {
                    println!("No EM100pro devices found.");
                } else {
                    for (bus, dev, serial) in devices {
                        println!(" Bus {:03} Device {:03}: EM100pro {}", bus, dev, serial);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error listing devices: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Handle --update-files
    if args.update_files {
        if let Err(e) = update_all_files() {
            eprintln!("Error updating files: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Parse device selection
    let (bus, device, serial) = args
        .device
        .as_ref()
        .map(|d| parse_device(d))
        .unwrap_or((None, None, None));

    // Open device
    let mut em100 = match Em100::open(bus, device, serial) {
        Ok(em100) => em100,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Load chip database
    let chip_db = ChipDatabase::load().ok();

    // Setup chips if requested
    let chip = if let Some(chip_name) = &args.chip {
        match chip_db.as_ref() {
            Some(db) => match db.find_chip(chip_name) {
                Ok(chip) => Some(chip),
                Err(_) => {
                    println!("Supported chips:\n");
                    for chip in db.list_chips() {
                        println!("  - {} {}", chip.vendor, chip.name);
                    }
                    println!(
                        "\nCould not find a chip matching '{}' to be emulated.",
                        chip_name
                    );
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!("Can't find chip configs. Please run: rem100 --update-files");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Set up signal handler
    let exit_requested = Arc::new(AtomicBool::new(false));
    let exit_clone = exit_requested.clone();
    ctrlc::set_handler(move || {
        exit_clone.store(true, Ordering::SeqCst);
    })
    .ok();

    // Print device info
    em100.print_info();
    if let Some(db) = &chip_db {
        println!("SPI flash database: {}", db.version);
    }

    // Print current state
    match em100.get_state() {
        Ok(running) => println!(
            "EM100Pro currently {}",
            if running { "running" } else { "stopped" }
        ),
        Err(_) => println!("EM100Pro state unknown"),
    }

    match em100.get_hold_pin_state() {
        Ok(state) => println!("EM100Pro hold pin currently {}", state),
        Err(_) => {}
    }
    println!();

    // Debug mode
    if args.debug {
        if let Err(e) = em100.debug() {
            eprintln!("Debug error: {}", e);
        }
    }

    // Firmware update
    if let Some(firmware_in) = &args.firmware_update {
        if let Err(e) = firmware_update(&em100, firmware_in, args.verify) {
            eprintln!("Firmware update error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Firmware dump
    if let Some(firmware_out) = &args.firmware_dump {
        if let Err(e) = firmware_dump(&em100, firmware_out, false) {
            eprintln!("Firmware dump error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Firmware write (DPFW format)
    if let Some(firmware_out) = &args.firmware_write {
        if let Err(e) = firmware_dump(&em100, firmware_out, true) {
            eprintln!("Firmware write error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Set serial number
    if let Some(serialno) = &args.set_serialno {
        let mut s = serialno.as_str();
        if s.to_uppercase().starts_with("DP") || s.to_uppercase().starts_with("EM") {
            s = &s[2..];
        }
        match s.parse::<u32>() {
            Ok(serial) => {
                if let Err(e) = em100.set_serial_no(serial) {
                    eprintln!("Error setting serial number: {}", e);
                    std::process::exit(1);
                }
            }
            Err(_) => {
                eprintln!("Error: Can't parse serial number '{}'", serialno);
                std::process::exit(1);
            }
        }
        return;
    }

    // Stop emulation
    if args.stop {
        if let Err(e) = em100.set_state(false) {
            eprintln!("Error stopping emulation: {}", e);
        }
    }

    // Set chip type
    if let Some(chip) = &chip {
        if let Err(e) = em100.set_chip_type(chip) {
            eprintln!("Failed configuring chip type: {}", e);
            std::process::exit(1);
        }
        println!("Chip set to {} {}.", chip.vendor, chip.name);

        // Auto-enable 4-byte mode for large chips
        if args.address_mode.is_none() && chip.size > 16 * 1024 * 1024 {
            if let Err(e) = em100.set_address_mode(4) {
                eprintln!("Warning: {}", e);
            }
        }
    }

    // Set address mode
    if let Some(mode) = args.address_mode {
        if let Err(e) = em100.set_address_mode(mode) {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    // Set voltage (obsolete)
    if let Some(voltage) = &args.set_voltage {
        let voltage_code = match voltage.as_str() {
            "3.3" => 33,
            "1.8" => 18,
            _ => {
                eprintln!("Invalid voltage, use 1.8 or 3.3.");
                std::process::exit(1);
            }
        };

        println!("Setting the voltage on the command line is known to cause problems.");
        println!("Please report to the coreboot mailing list why this is necessary.");

        if args.debug {
            println!("Setting anyways on your own risk (debug mode enabled)");
            if em100.set_fpga_voltage(voltage_code).is_err() {
                eprintln!("Failed configuring FPGA voltage.");
                std::process::exit(1);
            }
        }
    }

    // Set hold pin
    if let Some(holdpin) = &args.holdpin {
        match holdpin.parse::<HoldPinState>() {
            Ok(state) => {
                if let Err(e) = em100.set_hold_pin_state(state) {
                    eprintln!("Failed configuring hold pin state: {}", e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    }

    // Upload from device
    if let Some(upload_file) = &args.upload {
        let maxlen = chip.as_ref().map(|c| c.size as usize).unwrap_or(0x4000000);

        match em100.upload(0, maxlen) {
            Ok(data) => {
                let mut file = match File::create(upload_file) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("Could not open download file: {}", e);
                        std::process::exit(1);
                    }
                };
                if let Err(e) = file.write_all(&data) {
                    eprintln!("Error writing file: {}", e);
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("Upload error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Download to device
    if let Some(download_file) = &args.download {
        let spi_start_address = args
            .start_address
            .as_ref()
            .and_then(|s| parse_hex(s))
            .unwrap_or(0) as u32;

        if spi_start_address != 0 {
            println!("SPI address: 0x{:08x}", spi_start_address);
        }

        let maxlen = chip.as_ref().map(|c| c.size as usize).unwrap_or(0x4000000);

        let mut file = match File::open(download_file) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Can't open file '{}': {}", download_file, e);
                std::process::exit(1);
            }
        };

        let mut data = Vec::new();
        if let Err(e) = file.read_to_end(&mut data) {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }

        if data.is_empty() {
            eprintln!("FATAL: No file to upload.");
            std::process::exit(1);
        }

        if data.len() > maxlen {
            eprintln!("FATAL: file size exceeds maximum");
            std::process::exit(1);
        }

        // Apply image auto-correction if requested
        if args.compatible {
            autocorrect_image(&em100, &mut data).ok();
        }

        // Handle start address
        if spi_start_address != 0 {
            // Read existing data and merge
            match em100.upload(0, maxlen) {
                Ok(mut existing) => {
                    let start = spi_start_address as usize;
                    let end = start + data.len();
                    if end <= existing.len() {
                        existing[start..end].copy_from_slice(&data);
                        if let Err(e) = em100.download(&existing, 0) {
                            eprintln!("Download error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("SDRAM readback failed: {}", e);
                    std::process::exit(1);
                }
            }
        } else if let Err(e) = em100.download(&data, 0) {
            eprintln!("Download error: {}", e);
            std::process::exit(1);
        }

        // Verify
        if args.verify {
            match em100.upload(spi_start_address, data.len()) {
                Ok(readback) => {
                    if readback == data {
                        println!("Verify: PASS");
                    } else {
                        println!("Verify: FAIL");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Verification error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    // Start emulation
    if args.start {
        if let Err(e) = em100.set_state(true) {
            eprintln!("Error starting emulation: {}", e);
        }
    }

    // Trace/terminal mode
    if args.trace || args.terminal || args.traceconsole {
        const MAX_USB_ERRORS: u32 = 10;

        // Set hold pin to input if not explicitly set
        if args.holdpin.is_none() {
            if let Err(e) = em100.set_hold_pin_state(HoldPinState::Input) {
                eprintln!("Error: Failed to set EM100 to input: {}", e);
                std::process::exit(1);
            }
        }

        // Start emulation if not explicitly started or stopped
        if !args.start && !args.stop {
            em100.set_state(true).ok();
        }

        print!("Starting ");

        if args.trace || args.traceconsole {
            trace::reset_spi_trace(&em100).ok();
            print!("trace{}", if args.terminal { " & " } else { "" });
        }

        if args.terminal {
            trace::init_spi_terminal(&em100).ok();
            print!("terminal");
        }

        println!(". Press CTRL-C to exit.\n");
        std::io::stdout().flush().ok();

        let address_offset = args
            .offset
            .as_ref()
            .and_then(|s| parse_hex(s))
            .unwrap_or(0);

        if address_offset != 0 {
            println!("Address offset: 0x{:08x}", address_offset);
        }

        let address_length = args
            .length
            .as_ref()
            .and_then(|s| parse_hex(s))
            .unwrap_or(0);

        let mut trace_state = TraceState::new(args.brief, args.address_mode.unwrap_or(3));
        let mut usb_errors = 0u32;

        while !exit_requested.load(Ordering::SeqCst) && usb_errors < MAX_USB_ERRORS {
            let ret = if args.traceconsole {
                trace::read_spi_trace_console(
                    &em100,
                    &mut trace_state,
                    address_offset,
                    address_length,
                )
            } else if args.trace {
                trace::read_spi_trace(&em100, &mut trace_state, args.terminal, address_offset)
            } else if args.terminal {
                trace::read_spi_terminal(&em100, false)
            } else {
                Ok(true)
            };

            match ret {
                Ok(false) => usb_errors += 1,
                Err(_) => break,
                _ => {}
            }
        }

        if usb_errors >= MAX_USB_ERRORS {
            eprintln!("Error: Bailed out with too many USB errors.");
        }

        // Stop emulation if not explicitly started or stopped
        if !args.start && !args.stop {
            em100.set_state(false).ok();
        }

        if args.trace {
            trace::reset_spi_trace(&em100).ok();
        }

        // Reset hold pin to float
        if args.holdpin.is_none() {
            if let Err(e) = em100.set_hold_pin_state(HoldPinState::Float) {
                eprintln!("Error: Failed to set EM100 to float: {}", e);
            }
        }
    }
}
