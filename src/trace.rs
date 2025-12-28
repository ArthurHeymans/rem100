//! SPI trace related operations

use crate::device::Em100;
use crate::error::{Error, Result};
use crate::fpga;
use crate::spi;
use crate::usb;
use std::io::{self, Write};

/// Report buffer length
const REPORT_BUFFER_LENGTH: usize = 8192;
/// Number of report buffers
const REPORT_BUFFER_COUNT: usize = 8;

/// EM100 specific command
pub const EM100_SPECIFIC_CMD: u8 = 0x11;
/// EM100 message signature
pub const EM100_MSG_SIGNATURE: u32 = 0x47364440;

/// Address mode types
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum AddressType {
    None,
    NoOff3B,
    Addr3B,
    Addr4B,
    Dynamic,
}

/// SPI command values
struct SpiCmdValues {
    name: &'static str,
    cmd: u8,
    address_type: AddressType,
    pad_bytes: u8,
}

static SPI_COMMAND_LIST: &[SpiCmdValues] = &[
    SpiCmdValues {
        name: "read SFDP",
        cmd: 0x5a,
        address_type: AddressType::NoOff3B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "write status register",
        cmd: 0x01,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "page program",
        cmd: 0x02,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "read",
        cmd: 0x03,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "write disable",
        cmd: 0x04,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "read status register",
        cmd: 0x05,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "write enable",
        cmd: 0x06,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "fast read",
        cmd: 0x0b,
        address_type: AddressType::Dynamic,
        pad_bytes: 1,
    },
    SpiCmdValues {
        name: "EM100 specific",
        cmd: 0x11,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "fast dual read",
        cmd: 0x3b,
        address_type: AddressType::Dynamic,
        pad_bytes: 2,
    },
    SpiCmdValues {
        name: "chip erase",
        cmd: 0x60,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "read JEDEC ID",
        cmd: 0x9f,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "chip erase c7h",
        cmd: 0xc7,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "sector erase d8h",
        cmd: 0xd8,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "dual I/O read",
        cmd: 0xbb,
        address_type: AddressType::Dynamic,
        pad_bytes: 2,
    },
    SpiCmdValues {
        name: "quad I/O read",
        cmd: 0xeb,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad read",
        cmd: 0x6b,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad I/O dt read",
        cmd: 0xed,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad page program",
        cmd: 0x38,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "sector erase 20h",
        cmd: 0x20,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "block erase 32KB",
        cmd: 0x52,
        address_type: AddressType::Dynamic,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "enter 4b mode",
        cmd: 0xb7,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "exit 4b mode",
        cmd: 0xe9,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "read 4b",
        cmd: 0x13,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "fast read 4b",
        cmd: 0x0c,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "dual I/O read 4b",
        cmd: 0xbc,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "dual out read 4b",
        cmd: 0x3c,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad I/O read 4b",
        cmd: 0xec,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad out read 4b",
        cmd: 0x6c,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad I/O dt read 4b",
        cmd: 0xee,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "page program 4b",
        cmd: 0x12,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "quad page program 4b",
        cmd: 0x3e,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "block erase 64KB 4b",
        cmd: 0xdc,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "block erase 32KB 4b",
        cmd: 0x5c,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "sector erase 4b",
        cmd: 0x21,
        address_type: AddressType::Addr4B,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "enter quad I/O mode",
        cmd: 0x35,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "exit quad I/O mode",
        cmd: 0xf5,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
    SpiCmdValues {
        name: "unknown command",
        cmd: 0xff,
        address_type: AddressType::None,
        pad_bytes: 0,
    },
];

fn get_command_vals(command: u8) -> &'static SpiCmdValues {
    SPI_COMMAND_LIST
        .iter()
        .find(|c| c.cmd == command)
        .unwrap_or(&SPI_COMMAND_LIST[SPI_COMMAND_LIST.len() - 1])
}

/// SPI trace state
pub struct TraceState {
    counter: u32,
    curpos: u8,
    cmdid: u8,
    address_mode: u8,
    outbytes: usize,
    additional_pad_bytes: usize,
    address: u64,
    timestamp: u64,
    start_timestamp: u64,
    brief: bool,
}

impl Default for TraceState {
    fn default() -> Self {
        Self {
            counter: 0,
            curpos: 0,
            cmdid: 0xff, // timestamp, never a valid command id
            address_mode: 3,
            outbytes: 0,
            additional_pad_bytes: 0,
            address: 0,
            timestamp: 0,
            start_timestamp: 0,
            brief: false,
        }
    }
}

impl TraceState {
    pub fn new(brief: bool, address_mode: u8) -> Self {
        Self {
            brief,
            address_mode,
            ..Default::default()
        }
    }
}

/// Reset SPI trace buffer
pub fn reset_spi_trace(em100: &Em100) -> Result<()> {
    let cmd = [0xbdu8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    usb::send_cmd(&em100.interface, &cmd)?;
    Ok(())
}

/// Read report buffer from device
fn read_report_buffer(em100: &Em100) -> Result<[[u8; REPORT_BUFFER_LENGTH]; REPORT_BUFFER_COUNT]> {
    let mut cmd = [0u8; 16];
    cmd[0] = 0xbc; // read SPI trace buffer
    cmd[4] = REPORT_BUFFER_COUNT as u8;
    cmd[9] = 0x15; // TraceConfig

    usb::send_cmd(&em100.interface, &cmd)?;

    let mut reportdata = [[0u8; REPORT_BUFFER_LENGTH]; REPORT_BUFFER_COUNT];

    for report in 0..REPORT_BUFFER_COUNT {
        let data = usb::get_response(&em100.interface, REPORT_BUFFER_LENGTH)?;
        if data.len() != REPORT_BUFFER_LENGTH {
            return Err(Error::Communication(format!(
                "Report length = {} instead of {}",
                data.len(),
                REPORT_BUFFER_LENGTH
            )));
        }
        reportdata[report][..].copy_from_slice(&data);
    }

    Ok(reportdata)
}

/// Read SPI trace data
pub fn read_spi_trace(
    em100: &Em100,
    state: &mut TraceState,
    display_terminal: bool,
    addr_offset: u64,
) -> Result<bool> {
    let reportdata = read_report_buffer(em100)?;

    for report in 0..REPORT_BUFFER_COUNT {
        let data = &reportdata[report];
        let count = ((data[0] as usize) << 8) | (data[1] as usize);
        if count == 0 {
            continue;
        }
        let count = count.min(1023);

        for i in 0..count {
            let mut j = state.additional_pad_bytes;
            state.additional_pad_bytes = 0;
            let cmd = data[2 + i * 8];

            if cmd == 0x00 {
                // Packet without valid data
                continue;
            }
            if cmd == 0xff {
                // Timestamp
                state.timestamp = (data[2 + i * 8 + 2] as u64) << 40
                    | (data[2 + i * 8 + 3] as u64) << 32
                    | (data[2 + i * 8 + 4] as u64) << 24
                    | (data[2 + i * 8 + 5] as u64) << 16
                    | (data[2 + i * 8 + 6] as u64) << 8
                    | (data[2 + i * 8 + 7] as u64);
                if display_terminal {
                    read_spi_terminal(em100, true)?;
                }
                continue;
            }

            // Data packet
            if cmd != state.cmdid {
                let spi_command = data[i * 8 + 4];
                let spi_cmd_vals = get_command_vals(spi_command);

                state.cmdid = cmd;
                if state.counter == 0 {
                    state.start_timestamp = state.timestamp;
                }

                // Special commands
                match spi_command {
                    0xb7 => state.address_mode = 4,
                    0xe9 => state.address_mode = 3,
                    _ => {}
                }

                j = 1; // Skip command byte

                let address_bytes = match spi_cmd_vals.address_type {
                    AddressType::Dynamic => state.address_mode,
                    AddressType::NoOff3B | AddressType::Addr3B => 3,
                    AddressType::Addr4B => 4,
                    AddressType::None => 0,
                };

                if address_bytes == 3 {
                    state.address = ((data[i * 8 + 5] as u64) << 16)
                        | ((data[i * 8 + 6] as u64) << 8)
                        | (data[i * 8 + 7] as u64);
                } else if address_bytes == 4 {
                    state.address = ((data[i * 8 + 5] as u64) << 24)
                        | ((data[i * 8 + 6] as u64) << 16)
                        | ((data[i * 8 + 7] as u64) << 8)
                        | (data[i * 8 + 8] as u64);
                }

                state.address &= 0xffffffff;

                j += address_bytes as usize + spi_cmd_vals.pad_bytes as usize;

                const MAX_TRACE_BLOCKLENGTH: usize = 6;
                if j > MAX_TRACE_BLOCKLENGTH {
                    state.additional_pad_bytes = j - MAX_TRACE_BLOCKLENGTH;
                    j = MAX_TRACE_BLOCKLENGTH;
                }

                if state.brief {
                    if state.start_timestamp != 0 {
                        state.start_timestamp = 0;
                    }
                    if spi_cmd_vals.address_type != AddressType::None {
                        println!(
                            "0x{:02x} @ 0x{:08x} ({})",
                            spi_command, state.address, spi_cmd_vals.name
                        );
                    } else {
                        println!("0x{:02x} ({})", spi_command, spi_cmd_vals.name);
                    }
                } else {
                    state.counter += 1;
                    let rel_time = state.timestamp - state.start_timestamp;
                    print!(
                        "\nTime: {:06}.{:08} command # {:<6} : 0x{:02x} - {}",
                        rel_time / 100000000,
                        rel_time % 100000000,
                        state.counter,
                        spi_command,
                        spi_cmd_vals.name
                    );
                }

                state.curpos = 0;
                state.outbytes = 0;
            }

            if state.brief {
                if state.outbytes > 0 {
                    state.outbytes += 1;
                }
            } else {
                let blocklen = ((data[2 + i * 8 + 1].wrapping_sub(state.curpos)) / 8) as usize;
                let spi_cmd_vals = get_command_vals(data[i * 8 + 4]);

                while j < blocklen {
                    if state.outbytes == 0 {
                        match spi_cmd_vals.address_type {
                            AddressType::Dynamic | AddressType::Addr3B | AddressType::Addr4B => {
                                print!("\n{:08x} : ", addr_offset + state.address);
                            }
                            AddressType::NoOff3B => {
                                print!("\n{:08x} : ", state.address);
                            }
                            AddressType::None => {
                                print!("\n         : ");
                            }
                        }
                    }
                    print!("{:02x} ", data[i * 8 + 4 + j]);
                    state.outbytes += 1;
                    if state.outbytes == 16 {
                        state.outbytes = 0;
                        state.address += 16;
                    }
                    j += 1;
                }
            }

            state.curpos = data[2 + i * 8 + 1].wrapping_add(0x10);
            io::stdout().flush().ok();
        }
    }

    Ok(true)
}

/// HT message types
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum HtMsgType {
    Checkpoint1Byte = 0x01,
    Checkpoint2Bytes = 0x02,
    Checkpoint4Bytes = 0x03,
    HexadecimalData = 0x04,
    AsciiData = 0x05,
    TimestampData = 0x06,
    LookupTable = 0x07,
}

const UFIFO_SIZE: usize = 512;

use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

static MSG_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Read SPI terminal messages
pub fn read_spi_terminal(em100: &Em100, show_counter: bool) -> Result<bool> {
    let data = spi::read_ufifo(em100, UFIFO_SIZE, 0)?;

    // First two bytes are the amount of valid data
    let data_length = ((data[0] as usize) << 8) | (data[1] as usize);
    if data_length == 0 {
        return Ok(true);
    }

    // Actual data starts after the length
    let data_start = 2;
    let mut j = 0;

    while j < data_length && j + 6 < UFIFO_SIZE - data_start {
        let offset = data_start + j;

        // Check for signature
        let sig = ((data[offset] as u32) << 24)
            | ((data[offset + 1] as u32) << 16)
            | ((data[offset + 2] as u32) << 8)
            | (data[offset + 3] as u32);

        if sig == EM100_MSG_SIGNATURE {
            let data_type = data[offset + 4];
            let msg_len = data[offset + 5] as usize;

            if show_counter {
                print!("\nHT{:06}: ", MSG_COUNTER.load(AtomicOrdering::Relaxed));
            }

            // Print message bytes according to format
            for k in 0..msg_len {
                if offset + 6 + k >= data.len() {
                    break;
                }
                if offset + 6 + k >= data_start + data_length {
                    break;
                }

                let byte = data[offset + 6 + k];
                match data_type {
                    0x01..=0x04 | 0x06 => print!("{:02x} ", byte),
                    0x05 => print!("{}", byte as char),
                    0x07 => {
                        // Lookup table - not fully supported
                        if k + 1 < msg_len && offset + 6 + k + 1 < data.len() {
                            print!("Lookup: {:02x}{:02x}", byte, data[offset + 6 + k + 1]);
                        }
                    }
                    _ => print!("{:02x} ", byte),
                }
            }

            j += 6 + msg_len;
            MSG_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            io::stdout().flush().ok();
        } else {
            j += 1;
        }
    }

    Ok(true)
}

/// Initialize SPI terminal
pub fn init_spi_terminal(em100: &Em100) -> Result<()> {
    spi::write_ht_register(em100, spi::HtRegister::UfifoDataFmt, 0)?;
    spi::write_ht_register(em100, spi::HtRegister::Status, spi::START_SPI_EMULATION)?;

    // Set EM100 to recognize SPI command 0x11
    fpga::write_fpga_register(em100, 0x82, EM100_SPECIFIC_CMD as u16)?;
    let _ = fpga::read_fpga_register(em100, 0x28)?;

    Ok(())
}

/// Read SPI trace in console mode
pub fn read_spi_trace_console(
    em100: &Em100,
    state: &mut TraceState,
    addr_offset: u64,
    addr_len: u64,
) -> Result<bool> {
    if addr_offset == 0 {
        return Err(Error::InvalidArgument(
            "Address offset for console buffer required".to_string(),
        ));
    }
    if addr_len == 0 {
        return Err(Error::InvalidArgument(
            "Console buffer length required".to_string(),
        ));
    }

    let reportdata = read_report_buffer(em100)?;

    for report in 0..REPORT_BUFFER_COUNT {
        let data = &reportdata[report];
        let count = ((data[0] as usize) << 8) | (data[1] as usize);
        if count == 0 {
            continue;
        }
        let count = count.min(1023);

        let mut do_write = false;

        for i in 0..count {
            let mut j = state.additional_pad_bytes;
            state.additional_pad_bytes = 0;
            let cmd = data[2 + i * 8];

            if cmd != state.cmdid {
                let spi_command = data[i * 8 + 4];
                let spi_cmd_vals = get_command_vals(spi_command);

                state.cmdid = cmd;

                match spi_command {
                    0xb7 => state.address_mode = 4,
                    0xe9 => state.address_mode = 3,
                    _ => {}
                }

                j = 1;

                let address_bytes = match spi_cmd_vals.address_type {
                    AddressType::Dynamic => state.address_mode,
                    AddressType::NoOff3B | AddressType::Addr3B => 3,
                    AddressType::Addr4B => 4,
                    AddressType::None => 0,
                };

                if address_bytes == 3 {
                    state.address = ((data[i * 8 + 5] as u64) << 16)
                        | ((data[i * 8 + 6] as u64) << 8)
                        | (data[i * 8 + 7] as u64);
                } else if address_bytes == 4 {
                    state.address = ((data[i * 8 + 5] as u64) << 24)
                        | ((data[i * 8 + 6] as u64) << 16)
                        | ((data[i * 8 + 7] as u64) << 8)
                        | (data[i * 8 + 8] as u64);
                }

                j += address_bytes as usize + spi_cmd_vals.pad_bytes as usize;

                const MAX_TRACE_BLOCKLENGTH: usize = 6;
                if j > MAX_TRACE_BLOCKLENGTH {
                    state.additional_pad_bytes = j - MAX_TRACE_BLOCKLENGTH;
                    j = MAX_TRACE_BLOCKLENGTH;
                }

                state.curpos = 0;
                do_write = spi_command == 0x02;
            }

            if !do_write
                || spi_cmd_vals_address_type(data[i * 8 + 4]) == AddressType::None
                || state.address < addr_offset
                || state.address > addr_offset + addr_len
            {
                state.curpos = data[2 + i * 8 + 1].wrapping_add(0x10);
                continue;
            }

            let blocklen = ((data[2 + i * 8 + 1].wrapping_sub(state.curpos)) / 8) as usize;

            while j < blocklen {
                print!("{}", data[i * 8 + 4 + j] as char);
                j += 1;
            }

            state.curpos = data[2 + i * 8 + 1].wrapping_add(0x10);
            io::stdout().flush().ok();
        }
    }

    Ok(true)
}

fn spi_cmd_vals_address_type(cmd: u8) -> AddressType {
    get_command_vals(cmd).address_type
}
