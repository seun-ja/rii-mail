#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
BUILD_DIR="$SCRIPT_DIR/build"
APP="$PROJECT_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/RiiMail.app"
DMG_DIR="$PROJECT_ROOT/src-tauri/target/aarch64-apple-darwin/release/bundle/dmg"
DMG="$DMG_DIR/RiiMail_0.1.0_aarch64.dmg"

python3 "$SCRIPT_DIR/scripts/prepare-runtime.py"

export PYO3_CONFIG_FILE="$BUILD_DIR/pyo3-config.txt"
export PYO3_ENVIRONMENT_SIGNATURE="riimail-python-3.12.15-macos-arm64-python-executable"
export MACOSX_DEPLOYMENT_TARGET="14.0"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-Wl,-rpath,@executable_path/../Frameworks"

cd "$PROJECT_ROOT/src-tauri"
cargo tauri build --target aarch64-apple-darwin --bundles app

python3 "$SCRIPT_DIR/scripts/sign-app.py" "$APP"
python3 "$SCRIPT_DIR/scripts/audit-app.py" "$APP"
"$APP/Contents/MacOS/riimail" --self-test-local-inference

mkdir -p "$DMG_DIR"
rm -f "$DMG"
DMG_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/riimail-dmg.XXXXXX")
trap 'rm -rf "$DMG_ROOT"' EXIT INT TERM
cp -R "$APP" "$DMG_ROOT/RiiMail.app"
ln -s /Applications "$DMG_ROOT/Applications"
hdiutil create -volname RiiMail -srcfolder "$DMG_ROOT" -ov -format UDZO "$DMG"
hdiutil verify "$DMG"
printf 'Self-contained DMG: %s\n' "$DMG"
