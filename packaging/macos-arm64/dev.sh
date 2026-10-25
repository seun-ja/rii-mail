#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
BUILD_DIR="$SCRIPT_DIR/build"

python3 "$SCRIPT_DIR/scripts/prepare-runtime.py"

export PYO3_CONFIG_FILE="$BUILD_DIR/pyo3-config.txt"
export PYO3_ENVIRONMENT_SIGNATURE="riimail-python-3.12.15-macos-arm64-dev"
export MACOSX_DEPLOYMENT_TARGET="14.0"
export DYLD_LIBRARY_PATH="$BUILD_DIR${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-Wl,-rpath,$BUILD_DIR"

cd "$PROJECT_ROOT/src-tauri"
cargo tauri dev --no-watch --config tauri.macos.arm64.conf.json
