#!/bin/bash
# Build script for the EM100Pro web interface
#
# Prerequisites:
#   cargo install wasm-pack
#   cargo install trunk (alternative)
#
# Or use the Nix flake which provides all dependencies.

set -e

BUILD_DIR="web-dist"
PROFILE="${1:-release}"

echo "Building EM100Pro web interface..."

# Ensure wasm32 target is installed
rustup target add wasm32-unknown-unknown 2>/dev/null || true

# Build the WASM binary
echo "Compiling to WebAssembly..."
if [ "$PROFILE" = "debug" ]; then
    cargo build --target wasm32-unknown-unknown --features web --no-default-features --bin rem100-web
    WASM_FILE="target/wasm32-unknown-unknown/debug/rem100_web.wasm"
else
    cargo build --target wasm32-unknown-unknown --features web --no-default-features --bin rem100-web --release
    WASM_FILE="target/wasm32-unknown-unknown/release/rem100_web.wasm"
fi

# Create output directory
mkdir -p "$BUILD_DIR"

# Generate JS bindings with wasm-bindgen
echo "Generating JavaScript bindings..."
if command -v wasm-bindgen &> /dev/null; then
    wasm-bindgen "$WASM_FILE" --out-dir "$BUILD_DIR" --target web --no-typescript
else
    echo "wasm-bindgen not found. Install it with: cargo install wasm-bindgen-cli"
    exit 1
fi

# Optimize WASM if wasm-opt is available
if command -v wasm-opt &> /dev/null && [ "$PROFILE" = "release" ]; then
    echo "Optimizing WASM..."
    wasm-opt -Os "$BUILD_DIR/rem100_web_bg.wasm" -o "$BUILD_DIR/rem100_web_bg.wasm"
fi

# Copy HTML file
cp web/index.html "$BUILD_DIR/"

echo ""
echo "Build complete! Output in: $BUILD_DIR/"
echo ""
echo "To test locally, run a web server in the $BUILD_DIR directory:"
echo "  cd $BUILD_DIR && python3 -m http.server 8080"
echo ""
echo "Then open: http://localhost:8080"
echo ""
echo "Note: WebUSB requires HTTPS in production (localhost is exempt)."
