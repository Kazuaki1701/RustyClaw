#!/bin/bash
set -e

BINARY="rustyclaw-cli"
TARGET="aarch64-unknown-linux-gnu"
OUTPUT_DIR="target/${TARGET}/release"

echo "=== RustyClaw cross-compile for ${TARGET} ==="
cross build --release --target "${TARGET}"

echo ""
echo "=== Build complete ==="
ls -lh "${OUTPUT_DIR}/${BINARY}"
file "${OUTPUT_DIR}/${BINARY}"
echo ""
echo "Deploy command:"
echo "  scp ${OUTPUT_DIR}/${BINARY} pi@<rpi4-ip>:~/.local/bin/rustyclaw-cli"
