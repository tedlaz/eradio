#!/bin/bash

# Build script for Windows cross-compilation
# Requires: cargo, rustup, and cross (cargo install cross)

set -e

echo "Building Radio Browser for Windows with embedded player..."
echo ""

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo "Installing cross tool..."
    cargo install cross
fi

# Build for 64-bit Windows (most common)
echo "Building for 64-bit Windows (x86_64)..."
cross build --release --target x86_64-pc-windows-gnu
echo "✓ Built: target/x86_64-pc-windows-gnu/release/radio_browser.exe"
echo ""

echo "Windows build complete!"
echo ""
echo "The executable is self-contained with an embedded audio player."
echo "It does NOT require ffplay, mpv, or any external player!"
echo ""
echo "Binary location:"
echo "  target/x86_64-pc-windows-gnu/release/radio_browser.exe"
echo ""
echo "File size:"
ls -lh target/x86_64-pc-windows-gnu/release/radio_browser.exe | awk '{print "  " $5}'
