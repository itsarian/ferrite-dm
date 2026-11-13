<div align="center">
  <img src="resources/Ferrite-1024.png" width="96" height="96" alt="Ferrite DM Logo" />

  # Ferrite DM

  *Fast, telemetry-free, modern desktop and CLI download manager built in Rust*

  [![Build Status](https://img.shields.io/github/actions/workflow/status/itsarian/ferrite-dm/ci.yml?style=flat-square)](https://github.com/itsarian/ferrite-dm/actions)
  [![Release](https://img.shields.io/github/v/release/itsarian/ferrite-dm?include_prereleases&style=flat-square)](https://github.com/itsarian/ferrite-dm/releases)
  [![Rust Version](https://img.shields.io/badge/Rust-2024%20edition-orange?style=flat-square)](Cargo.toml)
  [![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux-lightgrey?style=flat-square)](#platform-support)

  [Features](#features) • [Platform Support](#platform-support) • [Quick Start](#quick-start) • [Installation](#installation) • [Desktop App](#desktop-app-fdm-gui) • [CLI Usage](#cli-usage-ferrite-dm) • [Browser Integration](#browser-integration) • [Architecture](#architecture)
</div>

---

Ferrite DM is a high-performance download manager built in Rust as a lightweight, local-first alternative to Free Download Manager (FDM 6.x). It combines a native GPU-accelerated desktop interface with a scriptable CLI and headless daemon sharing the same underlying download engine.

## Features

- **Zero Telemetry & Local-First** — Completely private with no tracking, analytics, background phone-homes, or ads. Downloads, tags, and settings stay in a local SQLite WAL database.
- **Multi-Protocol Engine** — Multi-connection HTTP/HTTPS/FTP with range splitting, BitTorrent and magnet links (powered by `librqbit`), and HLS (`.m3u8`) / DASH (`.mpd`) video stream extraction.
- **Native GPUI Desktop App** — GPU-accelerated desktop interface with light and dark themes, 6 locales, real-time speed graphs, and connection inspectors.
- **Unified CLI & Daemon** — Command-line interface and background daemon communicating over Unix domain sockets (`ferrite.sock`).
- **Browser Extension Integration** — Drop-in Native Messaging host (`org.freedownloadmanager.fdm5.cnh`) compatible with official FDM extensions for Chromium browsers and Firefox.
- **LAN Remote Control** — Built-in self-signed HTTPS and WebSocket web interface with QR-code pairing for local network management.
- **Batch Wildcards & Automation** — Pattern expansion for numeric ranges (`[01-20]`), character sets (`[a-z]`), and brace lists (`{linux,macos}`).
- **Traffic Usage Modes** — Four global throttling profiles (`snail`, `low`, `medium`, `high`) with scheduler, battery guard, and sleep prevention.

## Platform Support

| Platform | Support | Notes |
| :--- | :--- | :--- |
| **macOS** *(11+)* | **Full** | Native GPUI desktop app, menu bar, Dock badges, LaunchAgent autorun, sleep prevention (`caffeinate`), and system notifications. |
| **Linux** *(x86_64)* | **Core / CLI** | Full CLI, headless daemon, Unix socket IPC, and browser Native Messaging host. *(Desktop GUI is experimental).* |
| **Windows** | *Planned* | Transfer crates currently rely on Unix-specific file positioning (`FileExt`) and Unix domain sockets. |

## Quick Start

### Desktop App (macOS)
1. Download `Ferrite-DM-*.dmg` from [Releases](https://github.com/itsarian/ferrite-dm/releases).
2. Drag **Ferrite DM** to `/Applications` and launch.
3. Paste any URL or magnet link with `⌘V` or press `⌘N` to add a download.

### CLI in 5 Seconds
```bash
# Start a direct download (waits until finished, or queues to running daemon)
ferrite-dm https://example.com/archive.tar.gz

# Start headless background daemon
ferrite-dm daemon
```

### Browser Extension Setup
```bash
# Register native host with all supported browsers (Chrome, Firefox, Brave, Edge)
ferrite-dm native-host-install --browser all
```

## Installation

### Pre-built Binaries
Download release packages from [GitHub Releases](https://github.com/itsarian/ferrite-dm/releases):

- **macOS:**
  - `Ferrite-DM-<version>-macos-<arch>.dmg` — Drag-and-drop installer
  - `Ferrite-DM-<version>-macos-<arch>.pkg` — System installer (places app in `/Applications` and symlinks CLI tools to `/usr/local/bin`)
  - `ferrite-dm-<version>-macos-<arch>.tar.gz` — Portable archive with `install.sh`
- **Linux:**
  - `ferrite-dm-<version>-linux-x86_64.tar.gz` — Standalone CLI tarball with `install.sh`

To install the CLI from a downloaded archive:
```bash
tar -xzf ferrite-dm-*-macos-arm64.tar.gz
cd ferrite-dm-*
./install.sh
```

### Building from Source

#### Prerequisites
- Rust toolchain supporting **Edition 2024** (`rustc >= 1.85`)
- C compiler and CMake (for bundled C dependencies)
- macOS: Xcode Command Line Tools (`xcode-select --install`)
- Optional: `ffmpeg` / `ffprobe` (video muxing/audio conversion), `yt-dlp` (media parsers)

#### Build
```bash
git clone https://github.com/itsarian/ferrite-dm.git
cd ferrite-dm

# Build all workspace binaries in release mode
cargo build --release --workspace
```

Binaries are generated in `target/release/`:
- `fdm-gui` — GPUI desktop interface
- `ferrite-dm` — CLI client and background daemon
- `ferrite-native-host` — Browser Native Messaging bridge

#### Package macOS Installers
```bash
# Generate DMG, PKG, App Bundle, and Tarball in dist/
./scripts/package-macos.sh
```

#### Self-Test & Verification
```bash
# Run offline loopback self-test
./target/release/ferrite-dm self-test

# Run workspace unit and integration tests
cargo test --workspace
```

## Desktop App (`fdm-gui`)

```bash
fdm-gui [OPTIONS] [URL | MAGNET | .TORRENT]
```

### Launch Flags
- `--hidden` — Start minimized in the background with the engine and IPC socket active.
- `--portable` — Store database, settings, and plugins in `./ferrite-data/` beside the executable.
- `--data-dir <DIR>` — Use a custom data directory.

### Keyboard Shortcuts

| Shortcut | Action | Shortcut | Action |
| :--- | :--- | :--- | :--- |
| `⌘N` | Add Download | `Space` | Start / Pause download |
| `⌘B` | Batch Download modal | `Delete` | Remove selected download(s) |
| `⌘V` | Paste URL from clipboard | `⌘A` | Select all downloads |
| `⌘F` | Focus search bar | `⌘,` | Open Preferences |
| `⌘I` | Toggle Details Inspector | `⌘=` / `⌘-` | Zoom In / Zoom Out |

### Inspector Tabs (`⌘I`)
The bottom inspector pane provides live diagnostic views:
- **`GENERAL`** — Status, file path, download progress, and ETA.
- **`DETAILS`** — Technical HTTP headers, ETag, MIME type, redirects, and mirrors.
- **`FILES`** — Interactive file selection tree for multi-file torrents.
- **`PROGRESS`** — Visual chunk and piece completion map.
- **`CONNECTIONS`** — Active peer and connection list with transfer rates.
- **`SPEED`** — Real-time download and upload throughput graph.

> [!TIP]
> Click the **Preferences** heading 5 times to reveal hidden developer diagnostics and logging controls.

## CLI Usage (`ferrite-dm`)

When the GUI or daemon is running, CLI commands communicate through `ferrite.sock`. When run standalone, commands operate directly against the local SQLite database.

```bash
ferrite-dm [OPTIONS] [COMMAND]
```

### Basic Downloads
```bash
# Direct download with default settings
ferrite-dm https://example.com/file.zip

# Full download specification with mirror, headers, and destination
ferrite-dm add "https://cdn1.example.com/installer.dmg" \
  --mirror "https://cdn2.example.com/installer.dmg" \
  --dir ~/Downloads/Installers \
  --name App-Latest.dmg \
  --header "Authorization: Bearer <TOKEN>" \
  --priority high \
  --wait
```

### Torrents & Magnet Links
```bash
# Inspect torrent metadata without downloading
ferrite-dm torrent ~/Downloads/linux.torrent --inspect

# Download specific files (0-based indices) and stream sequentially
ferrite-dm torrent "magnet:?xt=urn:btih:..." \
  --select-files 0,2 \
  --play-asap \
  --dir ~/Downloads/Linux
```

### Batch Wildcards
```bash
# Preview expanded URLs
ferrite-dm batch "https://example.com/builds/v[1-3]-{linux,macos}-[a-b].tar.gz" --preview-only

# Download a sequence of files
ferrite-dm batch "https://example.com/parts/part-[01-20:2].zip" --dir ~/Downloads/Parts

# Enqueue from a text file (one URL per line)
ferrite-dm batch --file urls.txt --paused
```

### Queue Management
```bash
# List active downloads
ferrite-dm list
ferrite-dm list --state downloading --json

# Pause, resume, or cancel by ID
ferrite-dm pause 1
ferrite-dm resume 1
ferrite-dm cancel 1
ferrite-dm remove 1 --delete-files

# Batch queue controls
ferrite-dm start-all
ferrite-dm pause-all
```

### Traffic Usage Modes (TUM)
```bash
ferrite-dm tum snail    # Low bandwidth profile
ferrite-dm tum high     # Unlimited profile
```

| Mode | Download Cap | Upload Cap | Max Connections (Per Server) | Active Downloads |
| :--- | :--- | :--- | :--- | :--- |
| `snail` | 64 KiB/s | 32 KiB/s | 4 (2) | 1 |
| `low` | 512 KiB/s | 128 KiB/s | 12 (4) | 2 |
| `medium` | 4 MiB/s | 1 MiB/s | 32 (8) | 4 |
| `high` | Unlimited | Unlimited | 64 (16) | 8 |

### Media & Utility Tools
```bash
# Verify cryptographic file hash (md5, sha1, sha256, sha512)
ferrite-dm hash ~/Downloads/ubuntu.iso --algo sha256 --expect <HEX_DIGEST>

# Convert video to MP3 via FFmpeg
ferrite-dm mp3 ~/Downloads/podcast.mkv ~/Music/podcast.mp3 --bitrate 320

# Remux or transcode video to MP4
ferrite-dm mp4 ~/Downloads/capture.ts ~/Videos/output.mp4              # Fast remux (stream copy)
ferrite-dm mp4 ~/Downloads/legacy.avi ~/Videos/output.mp4 --transcode  # Full transcode

# Import existing Free Download Manager database
ferrite-dm import-fdm-db ~/Library/Application\ Support/Free\ Download\ Manager/db.sqlite
```

## Browser Integration

Ferrite DM implements the Free Download Manager WebExtension Native Messaging protocol (`org.freedownloadmanager.fdm5.cnh`), supporting official FDM extensions for Chrome, Firefox, Brave, Edge, Opera, and Vivaldi.

```bash
# Register native host manifest with all browsers
ferrite-dm native-host-install --browser all

# Or install for a specific browser
ferrite-dm native-host-install --browser chrome
ferrite-dm native-host-install --browser firefox
```

> [!NOTE]
> When a download is intercepted while neither the GUI nor daemon is running, the native messaging host automatically spawns a background `ferrite-dm daemon`.

## LAN Remote Control

Manage downloads from any device on your local network:

```bash
ferrite-dm daemon --remote-control --remote-host 192.168.1.50
```

- **Local Web UI:** Accessible at `https://<YOUR-IP>:9443/?token=...`
- **Zero Cloud Infrastructure:** Uses locally generated self-signed TLS certificates (`rc_cert.der` / `rc_key.der`).
- **QR Pairing:** Prints an ASCII QR code in the terminal for instant mobile pairing.

## Configuration & Paths

### Default Locations
- **macOS:** `~/Library/Application Support/org.ferritedm.FerriteDM`
- **Linux:** `~/.local/share/ferritedm` (`$XDG_DATA_HOME/ferritedm`)
- **Portable Mode:** `./ferrite-data/` beside the executable

### Path Expansion Macros
Templates configured in Preferences or via `--dir` support dynamic variables:

| Macro | Description | Example Output |
| :--- | :--- | :--- |
| `%date%` | Full ISO date (`YYYY-MM-DD`) | `2026-11-13` |
| `%year%`, `%month%`, `%day%` | Date parts | `2026`, `11`, `13` |
| `%host%`, `%server%` | Download URL host | `releases.ubuntu.com` |
| `%path_on_server%` | Remote server path segments | `releases/noble` |
| `%type%` | Detected file category | `Video`, `Audio`, `Archive`, `Document` |
| `%ext%` | Target file extension | `iso`, `dmg`, `zip` |

### Environment Variables
- `FERRITE_THEME` — Force theme (`light` or `dark`).
- `FERRITE_UPDATE_MANIFEST_URL` — Custom update check JSON manifest. Unset by default to remain 100% offline.

## Architecture

Ferrite DM is organized as a modular Cargo workspace:

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

See [ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md) for architectural details and subsystem mappings.
