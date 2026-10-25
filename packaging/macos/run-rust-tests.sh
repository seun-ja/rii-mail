#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    printf 'usage: %s backend|llm\n' "$(basename -- "$0")" >&2
    exit 2
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)

case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
        BUILD_DIR="$PROJECT_ROOT/packaging/macos-arm64/build"
        PYO3_SIGNATURE="riimail-python-3.12.15-macos-arm64-tests"
        ;;
    Darwin:x86_64)
        BUILD_DIR="$PROJECT_ROOT/packaging/macos-intel/build"
        PYO3_SIGNATURE="riimail-python-3.12.15-macos-x86_64-tests"
        ;;
    *)
        printf '%s\n' 'Bundled-runtime Rust tests require a native Apple Silicon or Intel Mac.' >&2
        exit 1
        ;;
esac

if [ ! -f "$BUILD_DIR/pyo3-config.txt" ]; then
    printf 'Pinned runtime is not staged; run the matching packaging/*/scripts/prepare-runtime.py first.\n' >&2
    exit 1
fi

export PYO3_CONFIG_FILE="$BUILD_DIR/pyo3-config.txt"
export PYO3_ENVIRONMENT_SIGNATURE="$PYO3_SIGNATURE"
export DYLD_LIBRARY_PATH="$BUILD_DIR${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"

cd "$PROJECT_ROOT"
case "$1" in
    backend)
        exec cargo test --manifest-path src-tauri/Cargo.toml
        ;;
    llm)
        exec cargo test --manifest-path src-tauri/Cargo.toml --package llm --lib
        ;;
    *)
        printf 'usage: %s backend|llm\n' "$(basename -- "$0")" >&2
        exit 2
        ;;
esac
