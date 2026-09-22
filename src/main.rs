//! rem100 - EM100Pro SPI flash emulator command-line utility
//!
//! A Rust port of the em100 utility for controlling the Dediprog EM100Pro
//! SPI flash emulator hardware.

use clap::Parser;
use em100::chips::ChipDatabase;
use em100::device::{Em100, HoldPinState};
use em100::download::update_all_files;
use em100::firmware::{firmware_dump, firmware_update};
use em100::image::autocorrect_image;
use em100::session::{DeviceSession, parse_address};
use em100::trace::{self, TraceState};
use futures_lite::future::block_on;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// EM100Pro command-line utility
#[derive(Parser, Debug)]
#[command(name = "rem100")]
#[command(author = "Google Inc., Rust port contributors")]
#[command(version = "0.1.0")]
#[command(about = "EM100Pro SPI flash emulator command-line utility")]
#[command(
    long_about = "A Rust port of the em100 utility for controlling the Dediprog EM100Pro SPI flash emulator hardware.

Example:
  rem100 --stop --set M25P80 -d file.bin -v --start -t -O 0xfff00000"
)]
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

    /// Allow the target to enter 4-byte address mode (on|off)
    #[arg(long = "enter-4byte-mode")]
    enter_4byte_mode: Option<String>,

    /// Upload from EM100pro into FILE
    #[arg(short = 'u', long = "upload")]
    upload: Option<String>,

    /// Pulse the target reset line for MS milliseconds
    #[arg(long = "reset")]
    reset: Option<u32>,

    /// Check the emulated memory is erased
    #[arg(long = "blank-check")]
    blank_check: bool,

    /// Show a checksum of the emulated memory
    #[arg(long = "checksum")]
    checksum: bool,

    /// Pad a short download image out to the chip size with BYTE
    #[arg(long = "fill")]
    fill: Option<String>,

    /// Allow an oversized download image to be truncated
    #[arg(long = "truncate")]
    truncate: bool,

    /// Only trace these SPI commands (hex, comma-separated)
    #[arg(long = "trace-filter")]
    trace_filter: Option<String>,

    /// Only trace accesses in this address range (hex START:END)
    #[arg(long = "trace-range")]
    trace_range: Option<String>,

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
    #[arg(short = 'P', long = "set-voltage")]
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

/// Parse a bare-hex value (with optional 0x prefix), as used by the
/// trace filter options where command bytes and addresses are hex.
fn parse_hex_strict(s: &str) -> Option<u64> {
    let s = s.trim();
    let hex = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if hex.is_empty() {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

/// Parse a number with optional 0x hex prefix, else decimal.
///
/// Deliberately broader than em100, which scans addresses with %x (bare
/// values are hex there): switching would reinterpret existing rem100
/// scripts, so 0x-prefixed values stay hex and the rest stay decimal.
fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_device(s: &str) -> Result<(Option<u8>, Option<u8>, Option<u32>), String> {
    let selection = s.to_ascii_uppercase();
    if let Some(serial) = selection
        .strip_prefix("DP")
        .or_else(|| selection.strip_prefix("EM"))
    {
        return serial
            .parse::<u32>()
            .map(|serial| (None, None, Some(serial)))
            .map_err(|_| format!("Invalid device selector: {s}"));
    }
    if let Some((bus, device)) = selection.split_once(':') {
        if let (Ok(bus), Ok(device)) = (bus.parse::<u8>(), device.parse::<u8>()) {
            return Ok((Some(bus), Some(device), None));
        }
    }
    Err(format!("Invalid device selector: {s}"))
}

fn transfer_progress(length: usize) -> ProgressBar {
    let progress = ProgressBar::new(length as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    progress
}

async fn read_memory_with_progress(
    session: &mut DeviceSession,
    address: u32,
    length: usize,
) -> em100::Result<Vec<u8>> {
    let progress = transfer_progress(length);
    let result = session
        .read_memory(
            address,
            length,
            Some(&mut |bytes_read, _| progress.set_position(bytes_read as u64)),
        )
        .await;
    match &result {
        Ok(_) => progress.finish_with_message("Read complete"),
        Err(_) => progress.abandon_with_message("Read failed"),
    }
    result
}

/// Write a download image into SDRAM, stopping emulation first.
///
/// The target must not be reading the emulated memory while it is being
/// rewritten, so this goes through the session's stop-before-mutation
/// workflow instead of calling `write_memory` directly.
async fn write_memory_with_progress(
    session: &mut DeviceSession,
    data: &[u8],
    address: u32,
) -> em100::Result<()> {
    let progress = transfer_progress(data.len());
    let result = session
        .stop_and_write_memory(
            data,
            address,
            Some(&mut |bytes_sent, _| progress.set_position(bytes_sent as u64)),
        )
        .await;
    match &result {
        Ok(_) => progress.finish_with_message("Transfer complete"),
        Err(_) => progress.abandon_with_message("Transfer failed"),
    }
    result
}

fn main() {
    block_on(run(Args::parse()));
}

async fn run(args: Args) {
    // Handle --list-devices
    if args.list_devices {
        match Em100::list_devices().await {
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
    let (bus, device, serial) = match args.device.as_deref().map(parse_device).transpose() {
        Ok(selection) => selection.unwrap_or((None, None, None)),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    // Open device
    let em100 = match Em100::open(bus, device, serial).await {
        Ok(em100) => em100,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut session = DeviceSession::new(em100).await;

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
    session.device().print_info();
    if let Some(db) = &chip_db {
        println!("SPI flash database: {}", db.version);
    }

    // Print current state
    match session.state().is_running() {
        Some(running) => println!(
            "EM100Pro currently {}",
            if running { "running" } else { "stopped" }
        ),
        None => println!("EM100Pro state unknown"),
    }

    if let Some(state) = session.state().hold_pin_state() {
        println!("EM100Pro hold pin currently {}", state)
    }
    println!();

    // Debug mode
    if args.debug {
        if let Err(e) = session.device_mut().debug().await {
            eprintln!("Debug error: {}", e);
        }
    }

    // Firmware update
    if let Some(firmware_in) = &args.firmware_update {
        if let Err(e) = firmware_update(session.device_mut(), firmware_in, args.verify).await {
            eprintln!("Firmware update error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Firmware dump
    if let Some(firmware_out) = &args.firmware_dump {
        if let Err(e) = firmware_dump(session.device_mut(), firmware_out, false).await {
            eprintln!("Firmware dump error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Firmware write (DPFW format)
    if let Some(firmware_out) = &args.firmware_write {
        if let Err(e) = firmware_dump(session.device_mut(), firmware_out, true).await {
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
                if let Err(e) = session.device_mut().set_serial_no(serial).await {
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
        if let Err(e) = session.set_emulation_state(false).await {
            eprintln!("Error stopping emulation: {}", e);
        } else {
            println!("Stopped EM100Pro");
        }
    }

    // Set chip type. Stop emulation first: the FPGA is reconfigured and the
    // address width changes, so the target must not be reading the device.
    if let Some(chip) = &chip {
        println!("Configuring SPI flash chip emulation.");
        if let Err(e) = session.stop_and_configure_chip(chip).await {
            eprintln!("Failed configuring chip type: {}", e);
            std::process::exit(1);
        }
        println!("Chip set to {} {}.", chip.vendor, chip.name);
    }

    // Work out the address mode. -m forces it; otherwise a chip larger than
    // 16MB is switched to 4-byte mode automatically. The register is only
    // written when there is a reason to.
    let enter_4byte: Option<bool> = match &args.enter_4byte_mode {
        None => None,
        Some(enter) => match enter.to_lowercase().as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => {
                eprintln!("Invalid 4 byte mode entry: {}", enter);
                std::process::exit(1);
            }
        },
    };
    let auto_4byte =
        args.address_mode.is_none() && chip.as_ref().is_some_and(|c| c.size > 16 * 1024 * 1024);
    let address_mode = args.address_mode.unwrap_or(if auto_4byte { 4 } else { 3 });
    if args.address_mode.is_some() || enter_4byte.is_some() || auto_4byte {
        if let Err(e) = session
            .set_address_mode(address_mode, enter_4byte.unwrap_or(false))
            .await
        {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
        println!("Enabled {} byte address mode", address_mode);
        if enter_4byte == Some(true) {
            println!("Enabled entry into 4 byte address mode");
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
            if session
                .device_mut()
                .set_fpga_voltage(voltage_code)
                .await
                .is_err()
            {
                eprintln!("Failed configuring FPGA voltage.");
                std::process::exit(1);
            }
        }
    }

    // Set hold pin
    if let Some(holdpin) = &args.holdpin {
        match holdpin.parse::<HoldPinState>() {
            Ok(state) => {
                if let Err(e) = session.set_hold_pin(state).await {
                    eprintln!("Failed configuring hold pin state: {}", e);
                    std::process::exit(1);
                }
                println!("Hold pin state set to {}", state);
            }
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    }

    // Upload from device
    if let Some(upload_file) = &args.upload {
        let maxlen = session
            .device_mut()
            .emulation_size(chip.as_ref(), chip_db.as_ref())
            .await;

        match read_memory_with_progress(&mut session, 0, maxlen).await {
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
        let spi_start_address = match args.start_address.as_deref().map(parse_address).transpose() {
            Ok(address) => address.unwrap_or(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };

        if spi_start_address != 0 {
            println!("SPI address: 0x{:08x}", spi_start_address);
        }

        let maxlen = chip.as_ref().map(|c| c.size as usize).unwrap_or(0x4000000);

        if (spi_start_address as usize) > maxlen {
            eprintln!(
                "FATAL: start address 0x{:08x} is beyond the {} byte emulation buffer.",
                spi_start_address, maxlen
            );
            std::process::exit(1);
        }

        let file = match File::open(download_file) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Can't open file '{}': {}", download_file, e);
                std::process::exit(1);
            }
        };

        // Read at most one byte past the chip size: enough to detect an
        // oversized image (C stops reading at maxlen) without
        // allocating unboundedly for huge files.
        let mut data = Vec::new();
        if let Err(e) = file.take(maxlen as u64 + 1).read_to_end(&mut data) {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }

        if data.is_empty() {
            eprintln!("FATAL: No file to upload.");
            std::process::exit(1);
        }

        if data.len() > maxlen {
            if !args.truncate {
                eprintln!("Image is larger than the chip; pass --truncate to discard excess bytes");
                std::process::exit(1);
            }
            data.truncate(maxlen);
        }

        let fill_value = match &args.fill {
            Some(s) => match parse_hex(s) {
                Some(v) if v <= 0xff => Some(v as u8),
                _ => {
                    eprintln!("Invalid fill byte: {}", s);
                    std::process::exit(1);
                }
            },
            None => None,
        };

        // When a chip is specified, pad a short image or validate the size
        if chip.is_some() {
            let expected_size = maxlen - spi_start_address as usize;
            if let Some(fill) = fill_value {
                if data.len() < expected_size {
                    let pad = expected_size - data.len();
                    println!("Filling the remaining {} bytes with 0x{:02x}", pad, fill);
                    data.resize(expected_size, fill);
                }
            }
            if data.len() != expected_size {
                eprintln!(
                    "FATAL: file size ({}) does not match chip size minus start address ({}).",
                    data.len(),
                    expected_size
                );
                std::process::exit(1);
            }
        }

        // Apply image auto-correction if requested
        if args.compatible {
            autocorrect_image(session.device(), &mut data).ok();
        }

        // Handle start address
        if spi_start_address != 0 {
            // Stop before reading: the target must not change flash while
            // the snapshot is merged and written back.
            if let Err(e) = session.stop_for_mutation().await {
                eprintln!("Could not stop emulation: {e}");
                std::process::exit(1);
            }
            match read_memory_with_progress(&mut session, 0, maxlen).await {
                Ok(mut existing) => {
                    let start = spi_start_address as usize;
                    let end = start + data.len();
                    if end <= existing.len() {
                        existing[start..end].copy_from_slice(&data);
                        let progress = transfer_progress(existing.len());
                        let result = session
                            .write_memory(
                                &existing,
                                0,
                                Some(&mut |sent, _| progress.set_position(sent as u64)),
                            )
                            .await;
                        if let Err(e) = result {
                            progress.abandon_with_message("Transfer failed");
                            eprintln!("Download error: {e}");
                            std::process::exit(1);
                        }
                        progress.finish_with_message("Transfer complete");
                    } else {
                        eprintln!(
                            "FATAL: image does not fit: start address 0x{:08x} plus file size {} exceeds the {} byte emulation buffer.",
                            spi_start_address,
                            data.len(),
                            existing.len()
                        );
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("SDRAM readback failed: {}", e);
                    std::process::exit(1);
                }
            }
        } else if let Err(e) = write_memory_with_progress(&mut session, &data, 0).await {
            eprintln!("Download error: {}", e);
            std::process::exit(1);
        }

        // Verify
        if args.verify {
            match read_memory_with_progress(&mut session, spi_start_address, data.len()).await {
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

    // Deliberately ordered after downloading, unlike em100: a single
    // invocation verifies the image just written instead of the old
    // contents. The erase-then-confirm workflow needs separate runs.
    if args.blank_check {
        let length = session
            .device_mut()
            .emulation_size(chip.as_ref(), chip_db.as_ref())
            .await;
        if let Err(e) = session.device_mut().blank_check(length).await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    if args.checksum {
        let length = session
            .device_mut()
            .emulation_size(chip.as_ref(), chip_db.as_ref())
            .await;
        if let Err(e) = session.device_mut().checksum(length).await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    // The Windows tool resets the target before starting emulation
    if let Some(ms) = args.reset {
        if !(1..=10000).contains(&ms) {
            eprintln!("Reset time must be between 1 and 10000 ms");
            std::process::exit(1);
        }
        if let Err(e) = session.device_mut().reset_target(ms).await {
            eprintln!("Error: Failed to reset the target: {}", e);
            std::process::exit(1);
        }
        println!("Pulsed the target reset line for {} ms", ms);
    }

    // Start emulation
    if args.start {
        if let Err(e) = session.set_emulation_state(true).await {
            eprintln!("Error starting emulation: {}", e);
        } else {
            println!("Started EM100Pro");
        }
    }

    // Trace/terminal mode
    if args.trace || args.terminal || args.traceconsole {
        const MAX_USB_ERRORS: u32 = 10;

        // Let the target drive the hold pin while tracing, but only if it is
        // floating, meaning nothing has asked for a particular state. Any
        // other state was set deliberately, and boards with their own flash
        // chip on the bus need it held low throughout, or they do not boot.
        let take_over_hold_pin = if args.holdpin.is_none() {
            match session.device_mut().get_hold_pin_state().await {
                Ok(HoldPinState::Float) => true,
                Ok(_) => false,
                Err(e) => {
                    eprintln!("Error: Failed to read the hold pin state: {}", e);
                    std::process::exit(1);
                }
            }
        } else {
            false
        };
        if take_over_hold_pin {
            if let Err(e) = session.set_hold_pin(HoldPinState::Input).await {
                eprintln!("Error: Failed to set EM100 to input: {}", e);
                std::process::exit(1);
            }
        }

        // Start emulation if not explicitly started or stopped
        if !args.start && !args.stop {
            session.set_emulation_state(true).await.ok();
        }

        print!("Starting ");

        if args.trace || args.traceconsole {
            session.reset_spi_trace().await.ok();
            print!("trace{}", if args.terminal { " & " } else { "" });
        }

        if args.terminal {
            trace::init_spi_terminal(session.device_mut()).await.ok();
            print!("terminal");
        }

        println!(". Press CTRL-C to exit.\n");
        std::io::stdout().flush().ok();

        let address_offset = args.offset.as_ref().and_then(|s| parse_hex(s)).unwrap_or(0);

        if address_offset != 0 {
            println!("Address offset: 0x{:08x}", address_offset);
        }

        let address_length = args.length.as_ref().and_then(|s| parse_hex(s)).unwrap_or(0);

        // `--address-mode` is applied to the session above and `--set` records
        // the chip default, so decode with the same width instead of assuming
        // the 3-byte default.
        let mut trace_state = TraceState::new(args.brief, session.state().address_mode());

        if let Some(filter) = &args.trace_filter {
            let cmds: Option<Vec<u8>> = filter
                .split(',')
                .map(|part| {
                    parse_hex_strict(part)
                        .filter(|&cmd| cmd <= 0xff)
                        .map(|cmd| cmd as u8)
                })
                .collect();
            match cmds {
                Some(cmds) => {
                    for cmd in cmds {
                        trace_state.filter_command(cmd);
                    }
                }
                None => {
                    eprintln!("Invalid trace filter: {}", filter);
                    std::process::exit(1);
                }
            }
        }

        if let Some(range) = &args.trace_range {
            let parts: Vec<&str> = range.split(':').collect();
            let valid = match parts.as_slice() {
                [start, end] => match (parse_hex_strict(start), parse_hex_strict(end)) {
                    (Some(start), Some(end)) if end >= start => {
                        trace_state.filter_address(start, end);
                        true
                    }
                    _ => false,
                },
                _ => false,
            };
            if !valid {
                eprintln!("Invalid trace range: {}", range);
                std::process::exit(1);
            }
        }

        let mut usb_errors = 0u32;

        while !exit_requested.load(Ordering::SeqCst) && usb_errors < MAX_USB_ERRORS {
            let ret = if args.traceconsole {
                trace::read_spi_trace_console(
                    session.device_mut(),
                    &mut trace_state,
                    address_offset,
                    address_length,
                )
                .await
            } else if args.trace {
                trace::read_spi_trace(
                    session.device_mut(),
                    &mut trace_state,
                    args.terminal,
                    address_offset,
                )
                .await
            } else if args.terminal {
                trace::read_spi_terminal(session.device_mut(), false).await
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
            session.set_emulation_state(false).await.ok();
        }

        if args.trace {
            session.reset_spi_trace().await.ok();
        }

        // Put the hold pin back only if it was taken over above
        if take_over_hold_pin {
            if let Err(e) = session.set_hold_pin(HoldPinState::Float).await {
                eprintln!("Error: Failed to set EM100 to float: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_device;

    #[test]
    fn invalid_device_selection_never_falls_back_to_default_device() {
        assert_eq!(parse_device("EM1234").unwrap(), (None, None, Some(1234)));
        assert_eq!(parse_device("1:2").unwrap(), (Some(1), Some(2), None));
        assert!(parse_device("EMbad").is_err());
        assert!(parse_device("1:2:3").is_err());
        assert!(parse_device("anything").is_err());
    }
}
