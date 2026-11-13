#!/bin/bash
set -e

# Ferrite DM CLI & Daemon Installer
PREFIX="${PREFIX:-/usr/local}"
if [ ! -w "$PREFIX/bin" ] && [ "$EUID" -ne 0 ]; then
    PREFIX="$HOME/.local"
fi

BIN_DIR="$PREFIX/bin"
mkdir -p "$BIN_DIR"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing Ferrite DM binaries to $BIN_DIR..."
install -m 755 "$SCRIPT_DIR/fdm-gui" "$BIN_DIR/fdm-gui" 2>/dev/null || cp "$SCRIPT_DIR/fdm-gui" "$BIN_DIR/fdm-gui"
install -m 755 "$SCRIPT_DIR/ferrite-dm" "$BIN_DIR/ferrite-dm" 2>/dev/null || cp "$SCRIPT_DIR/ferrite-dm" "$BIN_DIR/ferrite-dm"
install -m 755 "$SCRIPT_DIR/ferrite-native-host" "$BIN_DIR/ferrite-native-host" 2>/dev/null || cp "$SCRIPT_DIR/ferrite-native-host" "$BIN_DIR/ferrite-native-host"

chmod +x "$BIN_DIR/fdm-gui" "$BIN_DIR/ferrite-dm" "$BIN_DIR/ferrite-native-host"

echo "Ferrite DM installed successfully!"
echo "  GUI:         $BIN_DIR/fdm-gui"
echo "  CLI/Daemon:  $BIN_DIR/ferrite-dm"
echo "  Native Host: $BIN_DIR/ferrite-native-host"
echo ""
echo "To register the browser native messaging host, run:"
echo "  $BIN_DIR/ferrite-dm native-host-install"
