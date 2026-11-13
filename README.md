<div align="center">

<img src="resources/Ferrite-1024.png" alt="Ferrite DM Logo" width="120" height="120" />

# Ferrite DM

**Fast, telemetry-free, modern download manager and CLI built in Rust.**  
*A clean, local-first alternative to Free Download Manager (FDM) powered by GPUI, Tokio, and `librqbit`.*

[![CI](https://github.com/itsarian/ferrite-dm/actions/workflows/ci.yml/badge.svg)](https://github.com/itsarian/ferrite-dm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/itsarian/ferrite-dm?include_prereleases&style=flat-square)](https://github.com/itsarian/ferrite-dm/releases)
[![License](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg?style=flat-square)](#license)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg?style=flat-square)](#platform-support)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg?style=flat-square)](#building-from-source)

[**Download**](#installation) • [**Quick Start**](#quick-start) • [**Desktop App**](#desktop-app-fdm-gui) • [**CLI Reference**](#cli-reference-ferrite-dm) • [**Browser Integration**](#browser-integration) • [**Architecture**](#workspace-architecture)

</div>

---

## Overview

**Ferrite DM** is a high-performance download manager designed as a lightweight, private, and scriptable alternative to Free Download Manager (FDM 6.x). It pairs a native GPU-accelerated desktop interface with a scriptable CLI and headless background daemon sharing the same underlying download engine.

### Why Ferrite DM?

| Feature | Ferrite DM | Traditional Download Managers |
| :--- | :--- | :--- |
| **Privacy** | **100% Local-first** — zero telemetry, analytics, or phone-home | Often bundled with telemetry, analytics, or ads |
| **GUI Engine** | **Native GPUI** — buttery smooth, sub-millisecond response | Heavy WebViews or Electron bloat |
| **CLI & Daemon** | **Unified Unix socket IPC** (`ferrite.sock`) | GUI-only or disjointed third-party CLIs |
| **Protocols** | HTTP(S), FTP, BitTorrent, Magnet, HLS (`.m3u8`), DASH (`.mpd`) | Often limited to HTTP or requires plugins |
| **Browser Support** | Compatible with official FDM extensions via Native Messaging | Proprietary or manual browser capture |
| **Remote Control** | Built-in self-signed HTTPS + WebSocket Web UI with QR pairing | Cloud relay or third-party accounts required |

---

## Platform Support

| Operating System | Support Level | Capabilities |
| :--- | :--- | :--- |
| **macOS** *(11+)* | **Primary / Full** | Native GPUI application (`fdm-gui`), menu bar, Dock badges, LaunchAgent autorun, sleep prevention (`caffeinate`), system notifications, file associations (`.torrent`, `magnet:`, `fdm:`). |
| **Linux** *(x86_64)* | **Core / CLI** | Full CLI (`ferrite-dm`), background daemon, Unix socket IPC, and browser Native Messaging host. *(GUI build is experimental/headless-first).* |
| **Windows** | *Planned* | Transfer crates currently leverage Unix-specific file positioning (`FileExt`) and Unix domain sockets. |

---

## Quick Start

### 1. Download & Launch the Desktop App (macOS)
1. Download the latest `Ferrite-DM-*.dmg` or `.pkg` from [Releases](https://github.com/itsarian/ferrite-dm/releases).
2. Drag **Ferrite DM** into `/Applications`.
3. Launch the app. Paste any URL or magnet link with `⌘V` or press `⌘N` to start downloading.

### 2. Download via CLI in 5 Seconds
```bash
# Direct download (runs until finished, or queues to running GUI/daemon)
ferrite-dm https://example.com/archive.tar.gz

# Start headless background daemon
ferrite-dm daemon
```

### 3. Connect to Your Browser
```bash
# Register Ferrite DM with all installed Chromium browsers and Firefox
ferrite-dm native-host-install --browser all
```

---

## Installation

### Option A: Pre-built Binaries (Recommended)
Download signed packages directly from [GitHub Releases](https://github.com/itsarian/ferrite-dm/releases):

- **macOS:**
  - `Ferrite-DM-<version>-macos-<arch>.dmg` — Drag-and-drop installer
  - `Ferrite-DM-<version>-macos-<arch>.pkg` — System installer (places app in `/Applications` and symlinks CLI tools to `/usr/local/bin`)
  - `ferrite-dm-<version>-macos-<arch>.tar.gz` — Standalone portable tarball with `install.sh`
- **Linux:**
  - `ferrite-dm-<version>-linux-x86_64.tar.gz` — Portable CLI & native host with `install.sh`

To install the CLI from a downloaded tarball:
```bash
tar -xzf ferrite-dm-*-macos-arm64.tar.gz
cd ferrite-dm-*
./install.sh   # Installs binaries to ~/.local/bin or /usr/local/bin
```

---

### Option B: Building from Source

#### Prerequisites
- **Rust Toolchain:** `rustc >= 1.85` (Rust Edition 2024)
- **C Compiler & CMake:** Required by bundled native libraries (e.g. `librqbit`, `sqlite3`)
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)
- **Optional Tools:** `ffmpeg` / `ffprobe` (video muxing/audio conversion), `yt-dlp` (media parsers)

#### Compilation
```bash
git clone https://github.com/itsarian/ferrite-dm.git
cd ferrite-dm

# Build all binaries in release mode
cargo build --release --workspace
```

The resulting binaries will be in `target/release/`:
- `fdm-gui` — GPUI desktop interface
- `ferrite-dm` — CLI tool and background daemon
- `ferrite-native-host` — Browser Native Messaging bridge

#### Packaging macOS Installers
```bash
# Build binaries and generate DMG, PKG, App Bundle, and Tarball in dist/
./scripts/package-macos.sh

# Or package existing target/release binaries without recompiling:
./scripts/package-macos.sh --skip-build
```

#### Verification Suite
```bash
# Run offline loopback self-test (spawns an ephemeral HTTP range server)
./target/release/ferrite-dm self-test

# Run full workspace test suite
cargo test --workspace
```

---

## Desktop App (`fdm-gui`)

Launch the desktop interface directly or pass a download link to pre-fill the Add modal:

```bash
fdm-gui [OPTIONS] [URL | MAGNET | .TORRENT]
```

### Launch Flags
- `--hidden` — Launch minimized in the background with the download engine and IPC socket active.
- `--portable` — Keep all databases, settings, and plugins in `./ferrite-data/` beside the executable.
- `--data-dir <DIR>` — Override the default data directory path.

### Keyboard Shortcuts

| Shortcut | Action | Shortcut | Action |
| :--- | :--- | :--- | :--- |
| `⌘N` | Add Download (URL / Magnet / Torrent) | `Space` | Start / Pause selected download |
| `⌘B` | Batch Wildcard Download modal | `Delete` | Remove selected download(s) |
| `⌘V` | Paste URL from clipboard | `⌘A` | Select all downloads |
| `⌘F` | Focus search bar | `⌘,` | Open Preferences |
| `⌘I` | Toggle bottom Details Inspector | `⌘=` / `⌘-` | Zoom In / Zoom Out |

### Features & Inspector Tabs
- **Inspector Tabs (`⌘I`):** View `GENERAL` summary, technical `DETAILS` (ETag, MIME, redirects), `FILES` tree (for multi-file torrents), chunk `PROGRESS`, active `CONNECTIONS`, and real-time `SPEED` graphs.
- **Visual Styles & Themes:** System, Light, and Dark modes with New and Classic interface styling.
- **Localization:** English (`en_US`, `en_GB`), German (`de_DE`), French (`fr_FR`), Spanish (`es_ES`), and Japanese (`ja_JP`).
- **Diagnostic Mode:** Click the **Preferences** or **Add-ons** header 5 times to reveal hidden diagnostics and unsigned plugin developer tools.

---

## CLI Reference (`ferrite-dm`)

When `fdm-gui` or `ferrite-dm daemon` is running, CLI commands automatically communicate with the live process over `ferrite.sock`. If no daemon is running, commands operate directly against the local SQLite database.

```bash
ferrite-dm [OPTIONS] [COMMAND]
```

### 1. Basic & Advanced Downloads
```bash
# Quick download with default settings
ferrite-dm https://example.com/file.zip

# Full download specification
ferrite-dm add "https://cdn1.example.com/installer.dmg" \
  --mirror "https://cdn2.example.com/installer.dmg" \
  --dir ~/Downloads/Installers \
  --name App-Latest.dmg \
  --referer "https://example.com/" \
  --header "Authorization: Bearer <TOKEN>" \
  --priority high \
  --wait
```

### 2. Torrents & Magnet Links
```bash
# Inspect files, piece count, and info-hash without downloading
ferrite-dm torrent ~/Downloads/linux.torrent --inspect

# Download specific files (0-based indices) and stream sequentially
ferrite-dm torrent "magnet:?xt=urn:btih:..." \
  --select-files 0,2 \
  --play-asap \
  --dir ~/Downloads/Linux
```

### 3. Batch Wildcard Downloads
Generate complex multi-file downloads using numeric ranges, alphabetic ranges, or brace lists:

```bash
# Preview generated URLs without downloading
ferrite-dm batch "https://example.com/builds/v[1-3]-{linux,macos}-[a-b].tar.gz" --preview-only

# Download a sequence of numbered files
ferrite-dm batch "https://example.com/parts/part-[01-20:2].zip" --dir ~/Downloads/Parts

# Enqueue from a text file containing URLs (one per line)
ferrite-dm batch --file urls.txt --paused
```

### 4. Queue Management
```bash
# List all active and queued downloads
ferrite-dm list
ferrite-dm list --state downloading --json

# Pause, resume, or cancel by ID
ferrite-dm pause 1
ferrite-dm resume 1
ferrite-dm cancel 1
ferrite-dm remove 1 --delete-files

# Global actions
ferrite-dm start-all
ferrite-dm pause-all
```

### 5. Traffic Usage Modes (TUM)
Switch global throttling profiles on the fly:

```bash
ferrite-dm tum snail    # Low bandwidth profile
ferrite-dm tum high     # Max performance profile
```

| Mode | Download Speed | Upload Speed | Max Connections (Per Server) | Active Downloads |
| :--- | :--- | :--- | :--- | :--- |
| `snail` | 64 KiB/s | 32 KiB/s | 4 (2) | 1 |
| `low` | 512 KiB/s | 128 KiB/s | 12 (4) | 2 |
| `medium` | 4 MiB/s | 1 MiB/s | 32 (8) | 4 |
| `high` | **Unlimited** | **Unlimited** | 64 (16) | 8 |

### 6. Media Conversion & Integrity Verification
```bash
# Verify cryptographic hash against expected digest (md5, sha1, sha256, sha512)
ferrite-dm hash ~/Downloads/ubuntu.iso --algo sha256 --expect <HEX_DIGEST>

# Convert video to MP3 (uses FFmpeg)
ferrite-dm mp3 ~/Downloads/podcast.mkv ~/Music/podcast.mp3 --bitrate 320

# Remux or transcode video to MP4
ferrite-dm mp4 ~/Downloads/capture.ts ~/Videos/output.mp4              # Fast remux (stream copy)
ferrite-dm mp4 ~/Downloads/legacy.avi ~/Videos/output.mp4 --transcode  # Full transcode
```

### 7. Migration & Backup
```bash
# Export and import download lists
ferrite-dm export downloads.json
ferrite-dm import downloads.json --paused

# Backup and restore settings
ferrite-dm export-settings settings.json
ferrite-dm import-settings settings.json

# Import existing database from Free Download Manager
ferrite-dm import-fdm-db ~/Library/Application\ Support/Free\ Download\ Manager/db.sqlite
```

---

## Browser Integration

Ferrite DM implements the Free Download Manager WebExtension Native Messaging protocol (`org.freedownloadmanager.fdm5.cnh`). This allows official FDM extensions in Chrome, Firefox, Brave, Edge, Opera, and Vivaldi to pass downloads directly into Ferrite DM.

### Setup
Run the registration command for your browser:

```bash
# Install for all supported browsers
ferrite-dm native-host-install --browser all

# Or install for a specific browser
ferrite-dm native-host-install --browser chrome
ferrite-dm native-host-install --browser firefox
```

> [!NOTE]
> The native host automatically starts a background `ferrite-dm daemon` if neither the GUI nor the daemon is currently running when a download is intercepted.

---

## LAN Remote Control

Manage your downloads from any phone, tablet, or browser on your local network using the built-in web dashboard:

```bash
# Start daemon with Remote Control enabled
ferrite-dm daemon --remote-control --remote-host 192.168.1.50
```

- **HTTPS Web UI:** Opens on `https://<YOUR-IP>:9443/?token=...`
- **Zero Cloud Relays:** Uses locally generated self-signed TLS certificates (`rc_cert.der` / `rc_key.der`).
- **QR Pairing:** The terminal displays an ASCII QR code and `fdm://remote_control/...` link for one-click pairing.

---

## Configuration & Paths

### Default Storage Paths
- **macOS:** `~/Library/Application Support/org.ferritedm.FerriteDM`
- **Linux:** `~/.local/share/ferritedm` (`$XDG_DATA_HOME/ferritedm`)
- **Portable Mode (`--portable`):** `./ferrite-data/` in the same directory as the executable

### Destination Path Macros
When specifying download folders in Preferences or via `--dir`, dynamic variables can be used:

| Macro | Description | Example Output |
| :--- | :--- | :--- |
| `%date%` | Full ISO date (`YYYY-MM-DD`) | `2026-11-13` |
| `%year%`, `%month%`, `%day%` | Date parts | `2026`, `11`, `13` |
| `%host%`, `%server%` | Hostname from download URL | `releases.ubuntu.com` |
| `%path_on_server%` | Remote server path hierarchy | `releases/noble` |
| `%type%` | Detected media category | `Video`, `Audio`, `Archive`, `Document` |
| `%ext%` | Target file extension | `iso`, `dmg`, `zip` |

*Example template:* `~/Downloads/%host%/%type%/%date%`

### Environment Variables
- `FERRITE_THEME` — Force theme mode (`light` or `dark`).
- `FERRITE_UPDATE_MANIFEST_URL` — Custom update check JSON manifest. Unset by default to ensure 100% offline privacy.

---

## Workspace Architecture

Ferrite DM is built as a modular Rust workspace:

```
ferrite-dm/
├── crates/
│   ├── fdm-core          # SQLite persistence, rate limiting, routing, and migration
│   ├── fdm-http          # Multi-connection HTTP/HTTPS/FTP transfer engine
│   ├── fdm-bt            # BitTorrent engine (librqbit) with sequential streaming
│   ├── fdm-hls           # HLS (.m3u8) & DASH (.mpd) segment fetcher & muxer
│   ├── fdm-batch         # Wildcard range & brace expansion parser
│   ├── fdm-media         # Boa JavaScript add-ons & FFmpeg conversion helpers
│   ├── fdm-ipc           # Unix socket IPC and LAN Remote Control server
│   ├── fdm-native-host   # WebExtension Native Messaging protocol host
│   ├── fdm-cli           # CLI executable and orchestration engine
│   └── fdm-gui           # Native GPUI desktop application
└── docs/
    └── ARCHITECTURE_BLUEPRINT.md  # Detailed FDM mapping & architecture specification
```

For in-depth design details, refer to [ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md).

---

## License

Dual-licensed under either:

- **[MIT License](LICENSE-MIT)**
- **[Apache License, Version 2.0](LICENSE-APACHE)**

at your option.
