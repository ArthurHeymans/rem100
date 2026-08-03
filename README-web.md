# EM100Pro Web Interface

This document describes the web interface for the EM100Pro SPI flash emulator.

## Building

### Native Desktop GUI

The native desktop GUI works with the current setup:

```bash
# Enter the development environment
nix develop

# Build and run the native GUI
cargo run --no-default-features --features native-gui --bin rem100-web
```

### Web (WASM) Build

The web build uses the WebUSB support included in `nusb` 0.2.6 and later. No patched `nusb` branch is required.

Build with:

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

Shared and native-library modules:

- `chips.rs` - Chip database parsing shared by native and WebAssembly builds
- `device.rs` - Native synchronous `Em100` device operations
- `usb.rs` - Native low-level USB communication
- `sdram.rs` - Native SDRAM operations with progress callbacks
- `firmware.rs` - Native firmware operations with progress callbacks
- `fpga.rs`, `spi.rs`, `system.rs` - Native hardware operations
- `web_device.rs`, `web_usb.rs` - Asynchronous WebUSB operations for WebAssembly

### CLI (`src/main.rs`)

Command-line interface using clap. Built with `--features cli`.

### Web GUI (`src/web.rs`, `src/web_main.rs`)

egui/eframe-based GUI. Built with `--features web`.

## Feature Flags

| Feature         | Description                                              |
| --------------- | -------------------------------------------------------- |
| `cli` (default) | Builds the CLI and CLI-only download/archive helpers     |
| `web`           | Builds the egui GUI, including browser WebUSB            |
| `native-gui`    | Enables `web` plus native file dialogs                   |

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
