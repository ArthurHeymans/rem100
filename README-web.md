# EM100Pro Web Interface

This document describes the web interface for the EM100Pro SPI flash emulator.

## Building

### Native Desktop GUI

The native desktop GUI works with the current setup:

```bash
# Enter the development environment
nix develop

# Build and run the native GUI
cargo run --features web --no-default-features --bin rem100-web
```

### Web (WASM) Build

The web build requires the nusb WebUSB branch to properly support wasm32 targets.

**Current Status**: The nusb `task/webusb-rebased` branch has compilation issues when targeting wasm32. The web-sys crate needs specific features enabled for WebUSB types (`Usb`, `UsbDevice`, `UsbTransferStatus`, etc.).

Once the upstream nusb branch is fixed, you can build with:

```bash
# Using trunk (recommended for development)
nix develop
trunk serve

# Or manual build
nix develop
trunk build --release
```

The built files will be in `dist/` directory.

## Development Setup

### Prerequisites with Nix

The `flake.nix` includes all necessary tools:
- Rust with wasm32-unknown-unknown target
- trunk (WASM bundler)
- wasm-bindgen-cli

```bash
nix develop
```

### Without Nix

```bash
# Install wasm32 target
rustup target add wasm32-unknown-unknown

# Install trunk
cargo install trunk

# Install wasm-bindgen-cli
cargo install wasm-bindgen-cli
```

## Architecture

The codebase is structured to maximize code reuse between CLI and web interfaces:

### Core Library (`src/lib.rs`)

Platform-independent modules:
- `device.rs` - Core Em100 device operations
- `usb.rs` - Low-level USB communication
- `chips.rs` - Chip database parsing
- `sdram.rs` - SDRAM operations with progress callbacks
- `firmware.rs` - Firmware operations with progress callbacks
- `fpga.rs`, `spi.rs`, `system.rs` - Hardware operations

### CLI (`src/main.rs`)

Command-line interface using clap. Built with `--features cli`.

### Web GUI (`src/web.rs`, `src/web_main.rs`)

egui/eframe-based GUI. Built with `--features web`.

## Feature Flags

| Feature | Description |
|---------|-------------|
| `cli` (default) | Command-line interface with progress bars |
| `web` | Web/native GUI using egui/eframe |
| `native-gui` | Native GUI with file dialogs (rfd) |

## WebUSB Requirements

WebUSB requires:
1. HTTPS (or localhost for development)
2. User gesture to request device access
3. Chromium-based browser (Chrome, Edge, Opera)

Firefox and Safari do not support WebUSB.

## Files

- `index.html` - HTML entry point for WASM
- `assets/style.css` - Styles for loading screen
- `Trunk.toml` - trunk configuration
- `build-web.sh` - Manual build script (alternative to trunk)
