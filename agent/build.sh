#!/bin/bash
# Cross-compilation helper for dockup-agent
# Usage: ./build.sh [target]
# Supported targets: x86_64, aarch64, armv7

set -e

TARGET=${1:-x86_64}

case $TARGET in
  x86_64)
    RUST_TARGET="x86_64-unknown-linux-musl"
    ;;
  aarch64)
    RUST_TARGET="aarch64-unknown-linux-musl"
    ;;
  armv7)
    RUST_TARGET="armv7-unknown-linux-musleabihf"
    ;;
  *)
    echo "Unknown target: $TARGET"
    exit 1
    ;;
esac

echo "Building for $RUST_TARGET..."
cargo build --release --target $RUST_TARGET
echo "Binary at: target/$RUST_TARGET/release/dockup-agent"
