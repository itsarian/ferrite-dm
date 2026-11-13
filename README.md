# Ferrite DM

<div align="center">
  <img src="resources/Ferrite-1024.png" width="96" height="96" alt="Ferrite DM Logo" />

  <p><strong>Fast, telemetry-free desktop download manager and CLI built in Rust.</strong></p>

  [![Release](https://img.shields.io/github/v/release/itsarian/ferrite-dm?style=flat-square)](https://github.com/itsarian/ferrite-dm/releases)
  [![CI](https://img.shields.io/github/actions/workflow/status/itsarian/ferrite-dm/ci.yml?style=flat-square)](https://github.com/itsarian/ferrite-dm/actions)
  [![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue?style=flat-square)](#license)
  [![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square)](#prerequisites)
</div>

---

## Table of Contents

- [Description](#description)
- [Why Ferrite DM?](#why-ferrite-dm)
- [Features](#features)
- [Screenshots & Demo](#screenshots--demo)
- [Tech Stack](#tech-stack)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
  - [macOS Pre-built Packages](#macos-pre-built-packages)
  - [Linux Standalone CLI & Daemon](#linux-standalone-cli--daemon)
  - [Building from Source](#building-from-source)
- [Configuration](#configuration)
  - [Data Directories](#data-directories)
  - [Persistent Settings File](#persistent-settings-file)
  - [Traffic Usage Mode (TUM) Profiles](#traffic-usage-mode-tum-profiles)
  - [Environment Variables](#environment-variables)
- [Usage](#usage)
  - [Quick Start](#quick-start)
  - [Browser Extension Integration](#browser-extension-integration)
  - [CLI Command Reference](#cli-command-reference)
  - [Desktop GUI Shortcuts & Navigation](#desktop-gui-shortcuts--navigation)
  - [LAN Remote Control](#lan-remote-control)
- [Project Structure](#project-structure)
- [Deployment](#deployment)
- [Contributing](#contributing)
- [License](#license)

---

## Description

**Ferrite DM** is a high-performance download manager designed as a modern, local-first alternative to Free Download Manager (FDM 6.x), aria2, and curl. It combines a native GPU-accelerated desktop interface built with GPUI with a scriptable CLI tool and background daemon.

Unlike traditional download managers that bundle web runtimes or embed tracking services, Ferrite DM is 100% telemetry-free and stores all state locally in an SQLite database using Write-Ahead Logging (WAL). The desktop GUI, CLI client, and browser native messaging host share a single transfer engine communicating over a local Unix domain socket (`ferrite.sock`), ensuring consistent download state and rate limits across workflows.

---

## Why Ferrite DM?

| Capability | Ferrite DM | Free Download Manager (FDM 6.x) | aria2 / curl |
| :--- | :--- | :--- | :--- |
| **Privacy & Telemetry** | **Zero telemetry** — 100% local SQLite WAL database | Analytics, telemetry pings, promotional prompts | None (clean CLI) |
| **Desktop Interface** | **Native GPUI** (sub-millisecond GPU rendering, zero Chromium) | Heavy Qt 6 / embedded Chromium views | None (terminal only) |
| **Engine Architecture** | Shared Unix domain socket daemon (`ferrite.sock`) | Split C++ `VhIpc` internal layers | Standalone process per command |
| **Protocol Support** | HTTP(S), FTP, BitTorrent (`librqbit`), HLS & DASH | HTTP(S), FTP, BitTorrent, YouTube | HTTP(S), FTP, BitTorrent (aria2 only) |
| **Browser Integration** | Native Messaging host compatible with official FDM extensions | Proprietary extension protocol | Requires third-party wrappers |
| **Remote Control** | Built-in self-signed HTTPS + WebSocket with QR-code terminal pairing | Proprietary relay service (`fdmrc`) | External web UI wrappers |

### When to Consider Alternatives

- **Windows Desktop GUI:** The file allocation and IPC layers currently target Unix domain sockets and POSIX file positioning APIs. Windows support is in development.
- **Single One-off Transfers:** For simple file downloads inside shell pipelines without pause/resume tracking, standard `curl` or `wget` is lighter.
- **Dedicated Multi-user Seedboxes:** Ferrite DM is designed for personal desktop and workstation use. For multi-user seedbox servers, dedicated torrent daemons such as `transmission-daemon` or `qbittorrent-nox` are recommended.

---

## Features

- **Multi-Protocol Download Engine:**
  - Multi-connection segmented HTTP/HTTPS and FTP downloads with dynamic chunk splitting and mirror fallbacks.
  - BitTorrent engine powered by `librqbit` supporting magnet links, `.torrent` files, DHT, PEX, LSD, uTP, and sequential "Play ASAP" streaming.
  - HLS (`.m3u8`) and MPEG-DASH (`.mpd`) video stream fetching with AES-128 segment decryption.
- **Native GPUI Desktop Application:**
  - GPU-accelerated interface rendering at native display refresh rates with zero Electron or Chromium overhead.
  - 6-tab diagnostic inspector: `General`, `Details`, `Files` (interactive torrent file tree), `Progress` (visual chunk map), `Connections` (per-peer throughput), and `Speed` (live I/O transfer graph).
  - Native macOS integration: menu bar item, Dock transfer progress badges, sleep prevention (`caffeinate`), and system notifications.
- **Unified IPC & Daemon Mode:**
  - Background daemon (`ferrite-dm daemon`) exposing a Unix domain socket (`ferrite.sock`).
  - Seamless handoff: CLI commands talk to the running GUI or daemon automatically.
- **Browser Extension Compatibility:**
  - Drop-in WebExtension Native Messaging host (`ferrite-native-host`) compatible with official Free Download Manager browser extensions for Chrome, Firefox, Edge, Brave, Opera, and Vivaldi.
  - Automatically launches the background daemon when downloads are triggered from the browser.
- **Traffic Usage Modes (TUM):**
  - Instant bandwidth throttling profiles: `snail` (64 KiB/s), `low` (512 KiB/s), `medium` (4 MiB/s), and `high` (unlimited).
- **Extensible Scripting & Post-Processing:**
  - Wildcard pattern batch expansion (`[01-20:2]`, `{linux,macos}`).
  - Embedded Boa JavaScript engine for media site parsers.
  - Post-processing hooks for FFmpeg MP3 audio extraction, MP4 remuxing, and cryptographic digest verification (MD5, SHA-1, SHA-256, SHA-512).
- **Migration & Portability:**
  - Built-in importer for existing Free Download Manager databases (`import-fdm-db`).
  - Portable mode (`--portable`) storing all database and configuration files in a self-contained folder adjacent to the binary.

---

## Screenshots & Demo

The desktop interface pairs a responsive download table with a detailed diagnostic inspector:

```
+---------------------------------------------------------------------------------------+
|  Ferrite DM     [All] [Downloading] [Completed] [Paused]             [Q Search]  (TUM) |
+---------------------------------------------------------------------------------------+
|  ID   Name                     Progress         Size         Speed        Status      |
|  #1   ubuntu-24.04.iso         =========>-- 72% 2.8 GB       18.4 MB/s    Downloading |
|  #2   archive-dataset.tar.gz   ============ 100% 840 MB      -            Completed   |
|  #3   sample-stream.m3u8       ===>-------- 34% 180 MB       2.1 MB/s     Downloading |
+---------------------------------------------------------------------------------------+
|  [GENERAL]  [DETAILS]  [FILES]  [PROGRESS]  [CONNECTIONS]  [SPEED]                    |
|  -----------------------------------------------------------------------------------  |
|  Target File:  /Users/user/Downloads/ubuntu-24.04.iso                                 |
|  Chunk Map:    [■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■□□□□□□□□□□□□□]     |
|  Active Peers: 14 connections (3 mirrors, 11 BitTorrent swarm peers)                  |
|  Throughput:   ↓ 18.4 MB/s  |  ↑ 1.2 MB/s  |  ETA: 48s                                |
+---------------------------------------------------------------------------------------+
```

*(Visual desktop screenshots can be reviewed directly in the [resources/](resources/) directory, which contains application icon assets and bundle branding).*

---

## Tech Stack

- **Language:** Rust (Edition 2024, `rustc >= 1.85`)
- **Desktop UI Framework:** [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (GPU-accelerated windowing and UI engine)
- **Async Runtime:** [Tokio](https://tokio.rs/) (Multi-threaded async I/O, timers, signals)
- **HTTP & FTP Engine:** [reqwest](https://github.com/seanmonstar/reqwest) (HTTP/2, connection pooling), [rustls](https://github.com/rustls/rustls) (TLS 1.2/1.3), [suppaftp](https://github.com/veeso/suppaftp) (FTP/FTPS)
- **BitTorrent Engine:** [librqbit](https://github.com/ikatson/rqbit) (DHT, PEX, uTP, sequential chunk streaming)
- **Stream Processing:** [m3u8-rs](https://github.com/creativcoder/m3u8-rs) (HLS playlist parsing), AES-128 CBC segment decryptor
- **Embedded JavaScript Engine:** [Boa Engine](https://github.com/boa-dev/boa) (ECMAScript parser and evaluator for media plugins)
- **Database Persistence:** [rusqlite](https://github.com/rusqlite/rusqlite) (SQLite 3 with WAL mode, transactional rollbacks)
- **CLI Parser:** [Clap v4](https://github.com/clap-rs/clap) (Derive API)
- **IPC & Networking:** Unix Domain Sockets, [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) (WebSocket), [rcgen](https://github.com/rustls/rcgen) (Self-signed X.509 certs), [qrcode](https://github.com/kennytm/qrcode-rust)

---

## Prerequisites

### Platform Support

- **macOS:** macOS 11 (Big Sur) or newer. Fully supported on Apple Silicon (`arm64`) and Intel (`x86_64`). Provides full GPUI desktop GUI, CLI tools, native notifications, and menu bar integrations.
- **Linux:** Modern 64-bit Linux distribution (`x86_64`) with glibc 2.31+. Provides CLI client, background daemon, Unix domain socket IPC, and browser Native Messaging host. *(Linux GPUI desktop packaging is experimental).*

### Source Build Requirements

If building from source, ensure the following are installed:

- **Rust:** Version 1.85+ with Edition 2024 support (`rustup update stable`).
- **C/C++ Compiler:** `clang` (Xcode Command Line Tools on macOS) or `gcc` on Linux.
- **Build Tools:** `cmake`, `pkg-config`, and `libssl-dev` (Linux).
- **Optional Tools:** [FFmpeg](https://ffmpeg.org/) (for MP3/MP4 conversion subcommands) and [yt-dlp](https://github.com/yt-dlp/yt-dlp) (for extended media link extraction).

---

## Installation

### macOS Pre-built Packages

Download the latest release packages from [GitHub Releases](https://github.com/itsarian/ferrite-dm/releases):

1. **Disk Image (`.dmg`):**
   - Open `Ferrite-DM-<version>-macos-<arch>.dmg`.
   - Drag **Ferrite DM.app** to your `/Applications` folder.
2. **Installer Package (`.pkg`):**
   - Run `Ferrite-DM-<version>-macos-<arch>.pkg`.
   - Automatically installs the desktop application and configures CLI symlinks (`fdm-gui`, `ferrite-dm`, and `ferrite-native-host`) into `/usr/local/bin`.

Alternatively, install the CLI binaries via the repository script:

```bash
curl -fsSL https://raw.githubusercontent.com/itsarian/ferrite-dm/main/scripts/install-cli.sh | bash
```

### Linux Standalone CLI & Daemon

Download the release archive `ferrite-dm-<version>-linux-x86_64.tar.gz` and run the installer:

```bash
tar -xzf ferrite-dm-*-linux-x86_64.tar.gz
cd ferrite-dm-*
./install.sh
```

This installs `ferrite-dm` and `ferrite-native-host` into `~/.local/bin` (or `/usr/local/bin` if run as root).

### Building from Source

```bash
# 1. Clone the repository
git clone https://github.com/itsarian/ferrite-dm.git
cd ferrite-dm

# 2. Build the workspace in release mode
cargo build --release --workspace

# 3. Verify the build with the offline loopback self-test suite
./target/release/ferrite-dm self-test
```

Compiled binaries will be available in `target/release/`:
- `fdm-gui` – Native GPUI desktop application.
- `ferrite-dm` – CLI tool and background daemon engine.
- `ferrite-native-host` – Browser WebExtension Native Messaging host.

---

## Configuration

### Data Directories

By default, Ferrite DM organizes state, configuration, download history, and plugins under the operating system's standard data directory:

- **macOS:** `~/Library/Application Support/org.ferritedm.FerriteDM`
- **Linux:** `~/.local/share/ferritedm` (or `$XDG_DATA_HOME/ferritedm`)
- **Portable Mode (`--portable`):** `./ferrite-data` relative to the executable binary.

Inside the data directory:
- `db.sqlite` – SQLite database storing download records, file chunk states, mirrors, and tags.
- `config.json` – Serialized application preferences and bandwidth profiles.
- `ferrite.sock` – Unix domain socket used for IPC communication.
- `plugins/` – Extensible JavaScript media parser plugins.

### Persistent Settings File

Settings are saved in `config.json`. Below is an annotated excerpt of configurable properties:

```json
{
  "core": {
    "default_download_path": "/Users/username/Downloads",
    "current_tum": "high",
    "prevent_sleep_if_running": true,
    "existing_file_reaction": "rename",
    "auto_retry_failed": true,
    "max_auto_retries": 3,
    "bt_session_port": 6881,
    "bt_enable_dht": true,
    "bt_enable_utp": true,
    "bt_enable_pex": true,
    "bt_encryption_policy": "prefer_encryption"
  }
}
```

### Traffic Usage Mode (TUM) Profiles

Ferrite DM includes four pre-configured traffic shaping profiles that regulate bandwidth and connection concurrency:

| Mode | Max Download Speed | Max Upload Speed | Max Conns | Conns / Server | Max Active Downloads | Upload Ratio |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`snail`** | 64 KiB/s | 32 KiB/s | 4 | 2 | 1 | 1.0x |
| **`low`** | 512 KiB/s | 128 KiB/s | 12 | 4 | 2 | 1.5x |
| **`medium`** | 4 MiB/s | 1 MiB/s | 32 | 8 | 4 | 2.0x |
| **`high`** | Unlimited | Unlimited | 64 | 16 | 8 | Unlimited |

### Environment Variables

| Variable | Description |
| :--- | :--- |
| `FERRITE_THEME` | Overrides desktop interface theme (`dark`, `light`, or `system`). |
| `FERRITE_UPDATE_MANIFEST_URL` | Overrides the endpoint used for application update checks. |
| `HOSTNAME` | Sets the server hostname when generating self-signed TLS certificates for LAN Remote Control. |

---

## Usage

### Quick Start

Verify that the CLI is accessible:

```bash
ferrite-dm --version
# Output: ferrite-dm 0.1.0
```

Start a direct download immediately:

```bash
# Downloads file to default download directory (or enqueues if GUI/daemon is open)
ferrite-dm https://example.com/archive.tar.gz
```

### Browser Extension Integration

Ferrite DM supports one-click download capture using official Free Download Manager browser extensions.

To register the native messaging host manifest:

```bash
# Register for Google Chrome
ferrite-dm native-host-install --browser chrome

# Register for all supported browsers (Chrome, Firefox, Brave, Edge, Opera, Vivaldi)
ferrite-dm native-host-install --browser all
```

> [!NOTE]
> When a download is initiated from your web browser, `ferrite-native-host` automatically starts `ferrite-dm daemon` in the background if neither the GUI nor the daemon is already running.

### CLI Command Reference

When the GUI or daemon is active, CLI commands automatically dispatch requests over `ferrite.sock`. When run standalone, commands operate directly against the local SQLite database.

#### Adding Downloads

```bash
# Advanced download with mirror fallbacks, custom directory, headers, and priority
ferrite-dm add "https://cdn1.example.com/installer.dmg" \
  --mirror "https://cdn2.example.com/installer.dmg" \
  --dir ~/Downloads/Installers \
  --name App-Latest.dmg \
  --header "Authorization: Bearer <TOKEN>" \
  --priority high \
  --wait

# Expand batch URL pattern (numeric ranges with step sizes or brace expansions)
ferrite-dm batch "https://example.com/parts/part-[01-20:2].zip" --dir ~/Downloads/Parts
ferrite-dm batch "https://example.com/builds/v[1-3]-{linux,macos}.tar.gz" --preview-only

# Inspect torrent metadata or download specific files sequentially
ferrite-dm torrent ~/Downloads/sample.torrent --inspect
ferrite-dm torrent "magnet:?xt=urn:btih:..." --select-files 0,2 --play-asap
```

#### Queue Management

```bash
# View active downloads
ferrite-dm list

# Output download queue in JSON format
ferrite-dm list --json

# Pause, resume, or cancel downloads by ID
ferrite-dm pause 1
ferrite-dm resume 1
ferrite-dm cancel 1

# Remove download and delete associated files from disk
ferrite-dm remove 1 --delete-files

# Start or pause all downloads at once
ferrite-dm start-all
ferrite-dm pause-all
```

#### Traffic Shaping (TUM)

```bash
ferrite-dm tum snail    # 64 KiB/s download throttle
ferrite-dm tum medium   # 4 MiB/s download throttle
ferrite-dm tum high     # Unlimited bandwidth and connections
```

#### Integrity Checking & Media Utilities

```bash
# Compute and verify cryptographic file hash
ferrite-dm hash ~/Downloads/ubuntu.iso --algo sha256 --expect <HEX_DIGEST>

# Audio extraction to MP3 via FFmpeg
ferrite-dm mp3 ~/Downloads/podcast.mkv ~/Music/podcast.mp3 --bitrate 320

# Video remuxing/transcoding to MP4 via FFmpeg
ferrite-dm mp4 ~/Downloads/capture.ts ~/Videos/output.mp4 --transcode

# Import existing Free Download Manager SQLite database
ferrite-dm import-fdm-db ~/Library/Application\ Support/Free\ Download\ Manager/db.sqlite
```

#### Background Daemon Mode

```bash
# Run headless background daemon with IPC socket listener
ferrite-dm daemon

# Run daemon with local LAN Remote Control server enabled
ferrite-dm daemon --remote-control
```

### Desktop GUI Shortcuts & Navigation

Launch the desktop interface by opening `/Applications/Ferrite DM.app` or running `fdm-gui` from the terminal.

| Shortcut | Action | Shortcut | Action |
| :--- | :--- | :--- | :--- |
| `⌘N` | Open Add Download modal | `⌘⇧V` / `⌘V` | Paste URL into Add modal |
| `⌘B` | Open Batch Download modal | `Space` | Toggle Pause / Resume on selected item |
| `⌘F` | Focus search and filter bar | `Delete` | Remove selected download |
| `⌘I` | Toggle bottom Inspector panel | `⌘⇧A` | Select all downloads in view |
| `⌘,` | Open Settings preferences | `⌘=` / `⌘-` | Zoom in / Zoom out UI |
| `⌘M` | Minimize window | `⌘W` / `⌘Q` | Close window / Quit application |

#### Inspector Panel Tabs (`⌘I`)

- **`GENERAL`:** File target path, transfer state, total size, ETA, and progress metrics.
- **`DETAILS`:** HTTP headers, server MIME type, ETag, redirect hops, and mirror status.
- **`FILES`:** Interactive file tree for multi-file torrents and batch packages.
- **`PROGRESS`:** Visual completion map of segmented chunks and BitTorrent pieces.
- **`CONNECTIONS`:** Remote IP addresses, socket protocol types, and real-time throughput per peer.
- **`SPEED`:** Live download and upload transfer graphs.

### LAN Remote Control

Ferrite DM includes an embedded HTTPS and WebSocket server for remote monitoring within your local network:

```bash
ferrite-dm remote-control --host 192.168.1.100
```

This outputs a terminal QR code and a pairing URL:
```text
https://192.168.1.100:8443/?token=...
```
Scanning the QR code or visiting the URL allows inspecting active downloads and pausing/resuming transfers from a mobile device or secondary computer on the same local network.

---

## Project Structure

Ferrite DM is structured as a modular Cargo workspace:

```text
ferrite-dm/
├── Cargo.toml                  # Workspace definition and common metadata
├── crates/
│   ├── fdm-core/               # SQLite WAL persistence, data models, rate limiter, routing
│   ├── fdm-http/               # Segmented HTTP/HTTPS and FTP multi-connection engine
│   ├── fdm-bt/                 # BitTorrent engine wrapping librqbit with sequential streaming
│   ├── fdm-hls/                # HLS (.m3u8) & MPEG-DASH (.mpd) stream parser and decryptor
│   ├── fdm-batch/              # Wildcard pattern range and brace expansion parser
│   ├── fdm-media/              # Boa JavaScript engine and FFmpeg conversion helpers
│   ├── fdm-ipc/                # Unix domain socket protocol and LAN Remote Control server
│   ├── fdm-native-host/        # WebExtension Native Messaging protocol host executable
│   ├── fdm-cli/                # Command-line interface executable and daemon supervisor
│   └── fdm-gui/                # Native GPUI desktop application interface
├── docs/
│   └── ARCHITECTURE_BLUEPRINT.md # Reverse engineering analysis and technical specifications
├── resources/                  # App bundle icons (Ferrite.icns), PNGs, and Info.plist
└── scripts/
    ├── generate-icon.swift     # Vector icon renderer using Apple Swift Canvas APIs
    ├── install-cli.sh          # Shell installer for CLI binaries
    ├── package-macos.sh        # macOS bundle, DMG, PKG, and tarball packaging script
    └── uninstall-cli.sh        # Uninstaller for CLI binaries
```

For detailed protocol specifications and structural comparisons against legacy FDM architectures, refer to [docs/ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md).

---

## Deployment

### Packaging macOS Installers

The repository includes a dedicated build script [scripts/package-macos.sh](scripts/package-macos.sh) that compiles release binaries and packages distributable artifacts:

```bash
# Build release binaries and assemble .app, .dmg, .pkg, and standalone tarballs
./scripts/package-macos.sh

# Skip the cargo build step and reuse existing target/release binaries
./scripts/package-macos.sh --skip-build
```

Generated outputs are placed in `dist/`:
- `Ferrite DM.app` – Signed macOS application bundle.
- `Ferrite-DM-<version>-macos-<arch>.dmg` – Drag-to-Applications disk image.
- `Ferrite-DM-<version>-macos-<arch>.pkg` – System installer with `/usr/local/bin` symlinks.
- `ferrite-dm-<version>-macos-<arch>.tar.gz` – Standalone portable tarball.
- `SHA256SUMS.txt` – Checksum manifest for release assets.

### Continuous Integration & Release Automation

GitHub Actions workflows are defined in [.github/workflows/](.github/workflows/):
- **CI ([ci.yml](.github/workflows/ci.yml)):** Runs `cargo check --workspace`, executes the test suite with `cargo test --workspace`, and validates packaging assets on every pull request and push to `main`.
- **Release ([release.yml](.github/workflows/release.yml)):** Automatically builds macOS disk images, installer packages, and Linux `x86_64` standalone tarballs upon pushing release tags (`v*`).

---

## Contributing

Contributions from the open-source community are welcome.

### Development Workflow

1. **Fork and clone the repository:**
   ```bash
   git clone https://github.com/<your-username>/ferrite-dm.git
   cd ferrite-dm
   ```
2. **Create a topic branch:**
   ```bash
   git checkout -b feature/my-enhancement
   ```
3. **Verify checks and tests:**
   ```bash
   # Check workspace compilation
   cargo check --workspace

   # Run automated unit and integration tests
   cargo test --workspace

   # Run offline loopback transfer verification
   cargo run --release -p fdm-cli -- self-test
   ```
4. **Code Guidelines:**
   - Adhere to idiomatic Rust (Edition 2024).
   - Maintain zero-telemetry and local-first data invariants.
   - Ensure all new CLI flags or configuration parameters are properly handled in both standalone and daemon IPC modes.
5. **Open a Pull Request:** Submit your PR against the `main` branch with a clear description of changes.

---

## License

Ferrite DM is dual-licensed under either:

- **[MIT License](LICENSE-MIT)**
- **[Apache License, Version 2.0](LICENSE-APACHE)**

at your option.
