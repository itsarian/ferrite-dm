#!/bin/bash
set -e

PREFIX="${PREFIX:-/usr/local}"
if [ ! -w "$PREFIX/bin" ] && [ "$EUID" -ne 0 ]; then
    PREFIX="$HOME/.local"
fi

BIN_DIR="$PREFIX/bin"

echo "Removing Ferrite DM from $BIN_DIR..."
rm -f "$BIN_DIR/fdm-gui" "$BIN_DIR/ferrite-dm" "$BIN_DIR/ferrite-native-host"
echo "Uninstalled."
