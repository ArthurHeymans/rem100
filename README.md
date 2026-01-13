# rem100: EM100-Pro command-line utility (Rust port)

This is a Rust port of the em100 utility for controlling the Dediprog EM100-Pro [1] in Linux. It supports both the original version and the new -G2 variant.

The 'em100' device provides a way to emulate a SPI-flash chip. Various connectors are available to allow it to take over from the in-circuit SPI chip so that the SoC sees the em100's internal memory as the contents of the SPI flash. Images can be loaded into the em100 over USB in a few seconds, thus providing a much faster development cycle than is possible by reprogramming the SPI flash each time.

## Features

Major features provided by the tool include:

- Set the chip being emulated (the tool supports about 600)
- Adjust the state of the hold pin, which supports overriding the internal SPI
- Use of several em100 devices, distinguished by their serial number
- Terminal mode, allowing the SoC to send messages
- Output a trace of SPI commands issued by the SoC
- Reading / writing em100 firmware (dangerous as it can brick your em100)

## Linux USB Permissions

On Linux, you need to set up udev rules to access the EM100 without root permissions. Create a udev rule:

```bash
echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="04b4", ATTR{idProduct}=="1235", MODE="0666", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/99-em100.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then unplug and replug the EM100.

## Building

### CLI

```bash
cargo build --release --features cli
```

The binary will be available at `target/release/rem100`.

### Web Interface

A WebUSB-based web interface is also available:

```bash
# Install trunk (if not already installed)
cargo install trunk

# Build and serve the web interface
trunk serve
```

Then open http://127.0.0.1:8080 in Chrome or Edge (WebUSB is not supported in Firefox).

**Note:** The Linux udev rules above are also required for the web interface.

## Usage

Example:
```bash
rem100 --stop --set M25P80 -d file.bin -v --start -t -O 0xfff00000
```

### Command-line options

```
-c, --set CHIP                      Select chip emulation
-d, --download FILE                 Download FILE into EM100pro
-a, --start-address ADDRESS         Start address for download (e.g., -a 0x300000)
-m, --address-mode MODE             Force 3 or 4 byte address mode
-u, --upload FILE                   Upload from EM100pro into FILE
-r, --start                         Start emulation
-s, --stop                          Stop emulation
-v, --verify                        Verify EM100 content matches the file
-t, --trace                         Enable trace mode
-O, --offset HEX_VAL                Address offset for trace mode
-T, --terminal                      Enable terminal mode
-R, --traceconsole                  Enable trace console mode
-L, --length HEX_VAL                Length of buffer for traceconsole mode
-b, --brief                         Brief mode for traces
-F, --firmware-update FILE|auto     Update EM100pro firmware (dangerous)
-f, --firmware-dump FILE            Export raw EM100pro firmware to file
-g, --firmware-write FILE           Export EM100pro firmware to DPFW file
-S, --set-serialno NUM              Set serial number to NUM
-V, --set-voltage [1.8|3.3]         Switch FPGA voltage
-p, --holdpin [LOW|FLOAT|INPUT]     Set the hold pin state
-x, --device BUS:DEV                Use EM100pro on USB bus/device
-x, --device EMxxxxxx               Use EM100pro with serial no EMxxxxxx
-l, --list-devices                  List all connected EM100pro devices
-U, --update-files                  Update device (chip) and firmware database
-C, --compatible                    Enable compatibility mode (patch image for EM100Pro)
-D, --debug                         Print debug information
-h, --help                          Display help text
```

## License

This project is licensed under the GNU General Public License v2.0 only - see the COPYING file for details.

## References

[1] https://www.dediprog.com/product/EM100Pro-G2
