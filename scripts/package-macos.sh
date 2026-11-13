#!/bin/bash
set -euo pipefail

# Ferrite DM macOS Packaging Script
# Builds Ferrite DM.app, .dmg installer, .pkg installer, and CLI tarball.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
RESOURCES_DIR="$ROOT_DIR/resources"

VERSION="0.1.0"
ARCH="$(uname -m)"
SKIP_BUILD=false
CLEAN=false

for arg in "$@"; do
    case "$arg" in
        --skip-build)
            SKIP_BUILD=true
            ;;
        --clean)
            CLEAN=true
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Usage: $0 [--skip-build] [--clean]"
            exit 1
            ;;
    esac
done

cd "$ROOT_DIR"

if [ "$CLEAN" = true ]; then
    echo "==> Cleaning dist/..."
    rm -rf "$DIST_DIR"
fi

mkdir -p "$DIST_DIR" "$RESOURCES_DIR"

# 1. Build release binaries
if [ "$SKIP_BUILD" = false ]; then
    echo "==> Building release binaries with cargo..."
    cargo build --release -p fdm-gui -p fdm-cli -p fdm-native-host
else
    echo "==> Skipping cargo build (using existing target/release binaries)..."
fi

# Verify binaries exist
for bin in fdm-gui ferrite-dm ferrite-native-host; do
    if [ ! -f "target/release/$bin" ]; then
        echo "Error: target/release/$bin does not exist. Run without --skip-build."
        exit 1
    fi
done

# 2. Generate Apple Icon (.icns) if needed
if [ ! -f "$RESOURCES_DIR/Ferrite.icns" ]; then
    echo "==> Generating Ferrite.icns from vector assets..."
    swift "$SCRIPT_DIR/generate-icon.swift" "$RESOURCES_DIR"
fi

# 3. Assemble Ferrite DM.app
APP_NAME="Ferrite DM.app"
APP_DIR="$DIST_DIR/$APP_NAME"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RES_DIR="$CONTENTS_DIR/Resources"

echo "==> Assembling $APP_NAME..."
rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RES_DIR"

# Copy Info.plist and Icon
cp "$RESOURCES_DIR/Info.plist" "$CONTENTS_DIR/Info.plist"
cp "$RESOURCES_DIR/Ferrite.icns" "$RES_DIR/Ferrite.icns"

# Copy binaries
cp "target/release/fdm-gui" "$MACOS_DIR/fdm-gui"
cp "target/release/ferrite-dm" "$MACOS_DIR/ferrite-dm"
cp "target/release/ferrite-native-host" "$MACOS_DIR/ferrite-native-host"

chmod +x "$MACOS_DIR/"*

# 4. Ad-hoc code sign the bundle
echo "==> Code signing $APP_NAME (ad-hoc)..."
codesign --force --deep --sign - "$APP_DIR"

# 5. Build .dmg disk image
DMG_NAME="Ferrite-DM-${VERSION}-macos-${ARCH}.dmg"
DMG_PATH="$DIST_DIR/$DMG_NAME"
DMG_STAGE="$DIST_DIR/dmg-stage"

echo "==> Building Disk Image ($DMG_NAME)..."
rm -rf "$DMG_STAGE" "$DMG_PATH"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"

hdiutil create \
    -volname "Ferrite DM" \
    -srcfolder "$DMG_STAGE" \
    -ov \
    -format UDZO \
    "$DMG_PATH" > /dev/null

rm -rf "$DMG_STAGE"

# 6. Build .pkg installer package
PKG_NAME="Ferrite-DM-${VERSION}-macos-${ARCH}.pkg"
PKG_PATH="$DIST_DIR/$PKG_NAME"
PKG_STAGE="$DIST_DIR/pkg-stage"

echo "==> Building Installer Package ($PKG_NAME)..."
rm -rf "$PKG_STAGE" "$PKG_PATH"
mkdir -p "$PKG_STAGE"
cp -R "$APP_DIR" "$PKG_STAGE/"

chmod +x "$RESOURCES_DIR/pkg-scripts/postinstall"

pkgbuild \
    --root "$PKG_STAGE" \
    --install-location "/Applications" \
    --scripts "$RESOURCES_DIR/pkg-scripts" \
    --identifier "org.ferritedm.FerriteDM" \
    --version "$VERSION" \
    "$PKG_PATH" > /dev/null

rm -rf "$PKG_STAGE"

# 7. Build standalone CLI / portable tarball
TAR_NAME="ferrite-dm-${VERSION}-macos-${ARCH}.tar.gz"
TAR_PATH="$DIST_DIR/$TAR_NAME"
TAR_STAGE="$DIST_DIR/tar-stage/ferrite-dm-${VERSION}"

echo "==> Building Standalone Tarball ($TAR_NAME)..."
rm -rf "$DIST_DIR/tar-stage" "$TAR_PATH"
mkdir -p "$TAR_STAGE"

cp "target/release/fdm-gui" "$TAR_STAGE/"
cp "target/release/ferrite-dm" "$TAR_STAGE/"
cp "target/release/ferrite-native-host" "$TAR_STAGE/"
cp "$SCRIPT_DIR/install-cli.sh" "$TAR_STAGE/install.sh"
cp "$SCRIPT_DIR/uninstall-cli.sh" "$TAR_STAGE/uninstall.sh"
cp "$ROOT_DIR/README.md" "$TAR_STAGE/"
cp "$ROOT_DIR/LICENSE-MIT" "$TAR_STAGE/" 2>/dev/null || true

chmod +x "$TAR_STAGE/install.sh" "$TAR_STAGE/uninstall.sh" "$TAR_STAGE/fdm-gui" "$TAR_STAGE/ferrite-dm" "$TAR_STAGE/ferrite-native-host"

tar -czf "$TAR_PATH" -C "$DIST_DIR/tar-stage" "ferrite-dm-${VERSION}"
rm -rf "$DIST_DIR/tar-stage"

# 8. Compute Checksums
echo "==> Generating SHA256 checksums..."
cd "$DIST_DIR"
shasum -a 256 "$DMG_NAME" "$PKG_NAME" "$TAR_NAME" > SHA256SUMS.txt
cd "$ROOT_DIR"

echo ""
echo "=========================================================="
echo " Packaging Complete! Generated Install Binaries:"
echo "=========================================================="
ls -lh "$DIST_DIR"
echo "=========================================================="
cat "$DIST_DIR/SHA256SUMS.txt"
echo "=========================================================="
