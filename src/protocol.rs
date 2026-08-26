//! EM100 USB command encoding.

use zerocopy::{Immutable, IntoBytes};

pub const COMMAND_LEN: usize = 16;

/// One fixed-size command sent to the EM100 USB bulk endpoint.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Immutable, IntoBytes)]
pub struct Command([u8; COMMAND_LEN]);

impl Command {
    const fn new(opcode: u8) -> Self {
        let mut bytes = [0; COMMAND_LEN];
        bytes[0] = opcode;
        Self(bytes)
    }

    const fn with_u8(mut self, offset: usize, value: u8) -> Self {
        self.0[offset] = value;
        self
    }

    const fn with_be_u16(mut self, offset: usize, value: u16) -> Self {
        let bytes = value.to_be_bytes();
        self.0[offset] = bytes[0];
        self.0[offset + 1] = bytes[1];
        self
    }

    const fn with_be_u24(mut self, offset: usize, value: u32) -> Self {
        let bytes = value.to_be_bytes();
        self.0[offset] = bytes[1];
        self.0[offset + 1] = bytes[2];
        self.0[offset + 2] = bytes[3];
        self
    }

    const fn with_be_u32(mut self, offset: usize, value: u32) -> Self {
        let bytes = value.to_be_bytes();
        self.0[offset] = bytes[0];
        self.0[offset + 1] = bytes[1];
        self.0[offset + 2] = bytes[2];
        self.0[offset + 3] = bytes[3];
        self
    }

    fn from_prefix(prefix: &[u8]) -> Self {
        debug_assert!(prefix.len() <= COMMAND_LEN);
        let mut command = Self([0; COMMAND_LEN]);
        command.0[..prefix.len()].copy_from_slice(prefix);
        command
    }
}

pub mod chip {
    use super::Command;

    pub fn initialize(entry: &[u8; 4]) -> Command {
        Command::from_prefix(entry)
    }
}

pub mod system {
    use super::Command;

    pub const fn get_version() -> Command {
        Command::new(0x10)
    }

    pub const fn set_voltage(channel: u8, millivolts: u16) -> Command {
        Command::new(0x11)
            .with_u8(1, channel)
            .with_be_u16(2, millivolts)
    }

    pub const fn get_voltage(channel: u8) -> Command {
        Command::new(0x12).with_u8(1, channel)
    }

    pub const fn set_led(state: u8) -> Command {
        Command::new(0x13).with_u8(1, state)
    }
}

pub mod fpga {
    use super::Command;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct Register(u8);

    impl Register {
        pub const fn from_raw(address: u8) -> Self {
            Self(address)
        }

        pub const fn address(self) -> u8 {
            self.0
        }

        pub const EMULATION_STATE: Self = Self(0x28);
        pub const HOLD_PIN: Self = Self(0x2a);
        pub const ADDRESS_MODE: Self = Self(0x4f);
        pub const SPI_COMMAND: Self = Self(0x82);
        pub const CHIP_CONFIG_C4: Self = Self(0xc4);
        pub const CHIP_CONFIG_10: Self = Self(0x10);
        pub const CHIP_CONFIG_81: Self = Self(0x81);
    }

    pub const fn reconfigure() -> Command {
        Command::new(0x20)
    }

    pub const fn status() -> Command {
        Command::new(0x21)
    }

    pub const fn read_register(register: Register) -> Command {
        Command::new(0x22).with_u8(1, register.address())
    }

    pub const fn write_register(register: Register, value: u16) -> Command {
        Command::new(0x23)
            .with_u8(1, register.address())
            .with_be_u16(2, value)
    }

    pub const fn set_voltage(voltage_code: u8) -> Command {
        if voltage_code == 18 {
            Command::new(0x24).with_u8(2, 7).with_u8(3, 0x80)
        } else {
            Command::new(0x24)
        }
    }
}

pub mod spi {
    use super::Command;

    pub const fn get_id() -> Command {
        Command::new(0x30)
    }

    pub const fn erase() -> Command {
        Command::new(0x31)
    }

    pub const fn poll_status() -> Command {
        Command::new(0x32)
    }

    pub const fn read_page(address: u32) -> Command {
        Command::new(0x33).with_be_u24(1, address)
    }

    pub const fn write_page(address: u32) -> Command {
        Command::new(0x34).with_be_u24(1, address)
    }

    pub const fn unlock() -> Command {
        Command::new(0x36)
    }

    pub const fn erase_sector(sector: u8) -> Command {
        Command::new(0x37).with_u8(1, sector)
    }

    pub const fn read_ht_register(register: u8) -> Command {
        Command::new(0x50).with_u8(1, register)
    }

    pub const fn write_ht_register(register: u8, value: u8) -> Command {
        Command::new(0x51).with_u8(1, register).with_u8(2, value)
    }

    pub const fn write_dfifo(length: u16, timeout: u16) -> Command {
        Command::new(0x52)
            .with_be_u16(1, length)
            .with_be_u16(3, timeout)
    }

    pub const fn read_ufifo(length: u16, timeout: u16) -> Command {
        Command::new(0x53)
            .with_be_u16(1, length)
            .with_be_u16(3, timeout)
    }
}

pub mod trace {
    use super::Command;

    pub const fn reset() -> Command {
        Command::new(0xbd)
    }

    pub const fn read(report_count: u8, config: u8) -> Command {
        Command::new(0xbc)
            .with_u8(4, report_count)
            .with_u8(9, config)
    }
}

pub mod sdram {
    use super::Command;

    pub const fn write(address: u32, length: u32) -> Command {
        Command::new(0x40)
            .with_be_u32(1, address)
            .with_be_u32(5, length)
    }

    pub const fn read(address: u32, length: u32) -> Command {
        Command::new(0x41)
            .with_be_u32(1, address)
            .with_be_u32(5, length)
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, chip, fpga, sdram, spi, system, trace};
    use zerocopy::IntoBytes;

    fn padded(prefix: &[u8]) -> [u8; 16] {
        let mut expected = [0; 16];
        expected[..prefix.len()].copy_from_slice(prefix);
        expected
    }

    fn assert_command(command: Command, prefix: &[u8]) {
        assert_eq!(command.as_bytes(), &padded(prefix));
    }

    #[test]
    fn command_is_exactly_sixteen_bytes() {
        assert_eq!(size_of::<Command>(), 16);
    }

    #[test]
    fn encodes_system_commands() {
        assert_command(system::get_version(), &[0x10]);
        assert_command(system::set_voltage(4, 3300), &[0x11, 4, 0x0c, 0xe4]);
        assert_command(system::get_voltage(8), &[0x12, 8]);
        assert_command(system::set_led(3), &[0x13, 3]);
    }

    #[test]
    fn encodes_fpga_commands() {
        assert_command(fpga::reconfigure(), &[0x20]);
        assert_command(fpga::status(), &[0x21]);
        assert_command(
            fpga::read_register(fpga::Register::from_raw(0x42)),
            &[0x22, 0x42],
        );
        assert_command(
            fpga::write_register(fpga::Register::from_raw(0x42), 0x1234),
            &[0x23, 0x42, 0x12, 0x34],
        );
        assert_command(fpga::set_voltage(18), &[0x24, 0, 7, 0x80]);
        assert_command(fpga::set_voltage(33), &[0x24]);
    }

    #[test]
    fn encodes_spi_commands() {
        assert_command(spi::get_id(), &[0x30]);
        assert_command(spi::erase(), &[0x31]);
        assert_command(spi::poll_status(), &[0x32]);
        assert_command(spi::read_page(0x123456), &[0x33, 0x12, 0x34, 0x56]);
        assert_command(spi::write_page(0xabcdef), &[0x34, 0xab, 0xcd, 0xef]);
        assert_command(spi::unlock(), &[0x36]);
        assert_command(spi::erase_sector(7), &[0x37, 7]);
        assert_command(spi::read_ht_register(5), &[0x50, 5]);
        assert_command(spi::write_ht_register(4, 0xaa), &[0x51, 4, 0xaa]);
        assert_command(
            spi::write_dfifo(0x123, 0x4567),
            &[0x52, 1, 0x23, 0x45, 0x67],
        );
        assert_command(spi::read_ufifo(0x123, 0x4567), &[0x53, 1, 0x23, 0x45, 0x67]);
    }

    #[test]
    fn encodes_sdram_and_trace_commands() {
        assert_command(
            sdram::write(0x12345678, 0x01020304),
            &[0x40, 0x12, 0x34, 0x56, 0x78, 1, 2, 3, 4],
        );
        assert_command(
            sdram::read(0x12345678, 0x01020304),
            &[0x41, 0x12, 0x34, 0x56, 0x78, 1, 2, 3, 4],
        );
        assert_command(trace::reset(), &[0xbd]);
        assert_command(trace::read(8, 0x15), &[0xbc, 0, 0, 0, 8, 0, 0, 0, 0, 0x15]);
    }

    #[test]
    fn pads_chip_initialization_entries() {
        assert_command(
            chip::initialize(&[0x23, 0xc4, 0x00, 0x01]),
            &[0x23, 0xc4, 0, 1],
        );
    }
}
