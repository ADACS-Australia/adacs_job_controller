#!/bin/bash
#
# Build release binary for ADACS Job Controller (Rust)
#
# This script builds an optimized release binary. Unlike the client,
# the server does not need zigbuild since it doesn't need to link
# against external C++ libraries.
#
# Usage:
#   ./build_release.sh
#
# Output:
#   target/release/adacs_job_controller
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building release binary..."
echo ""

cargo build --release

BINARY_PATH="target/release/adacs_job_controller"

echo ""
echo "Build complete: $BINARY_PATH"
echo ""
echo "Verifying dependencies..."
if command -v ldd &> /dev/null; then
    ldd "$BINARY_PATH"
fi

echo ""
echo "Binary size:"
ls -lh "$BINARY_PATH" | awk '{print $5}'
