#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
        exec "$SCRIPT_DIR/../macos-arm64/dev.sh" "$@"
        ;;
    Darwin:x86_64)
        exec "$SCRIPT_DIR/../macos-intel/dev.sh" "$@"
        ;;
    *)
        printf '%s\n' 'RiiMail local inference development requires a native Apple Silicon or Intel Mac.' >&2
        exit 1
        ;;
esac
