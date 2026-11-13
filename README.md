<div align="center">
  <img src="resources/Ferrite-1024.png" width="96" height="96" alt="Ferrite DM Logo" />

  # Ferrite DM

  *Fast, telemetry-free, modern desktop and CLI download manager built in Rust*

  [![Release](https://img.shields.io/github/v/release/itsarian/ferrite-dm?style=flat-square)](https://github.com/itsarian/ferrite-dm/releases)
  [![Build Status](https://img.shields.io/github/actions/workflow/status/itsarian/ferrite-dm/ci.yml?style=flat-square)](https://github.com/itsarian/ferrite-dm/actions)
  [![Licence](https://img.shields.io/badge/licence-MIT%20%2F%20Apache--2.0-blue?style=flat-square)](#licence)
  [![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square)](#platform-support)

  [Overview](#overview) • [Why Ferrite](#why-ferrite-dm) • [Install](#install) • [Desktop App](#desktop-app) • [CLI Usage](#cli-usage) • [Platform Support](#platform-support) • [Architecture](#architecture) • [Licence](#licence)
</div>

---

## Overview

**Ferrite DM** is a high-performance download manager and CLI built in Rust as a local-first alternative to Free Download Manager (FDM 6.x). It pairs a native GPU-accelerated desktop interface with a scriptable CLI and background daemon sharing the same underlying download engine.

### Project Status

Actively maintained in early release (`v0.1.0`). Core transfer mechanics (HTTP/FTP, BitTorrent via `librqbit`, HLS/DASH video streams), SQLite persistence, and macOS desktop integrations are verified and tested. Linux CLI and background daemon support are fully functional; Linux GUI packaging and Windows support are in active development.

---

## Why Ferrite DM?

| Requirement | Ferrite DM | Free Download Manager (FDM) | aria2 / curl |
| :--- | :--- | :--- | :--- |
| **Telemetry & Privacy** | **Zero telemetry** — 100% local SQLite WAL database | Proprietary telemetry, update pings, promotional prompts | None (clean) |
| **Desktop Interface** | **Native GPUI** (sub-millisecond GPU rendering, no Electron) | Qt / embedded Chromium views | None (CLI only) |
| **CLI & Daemon Parity** | **Shared Unix socket engine** (`ferrite.sock`) | Limited or disjointed CLI support | CLI only |
| **Protocols** | HTTP(S), FTP, BitTorrent, HLS/DASH video streams | HTTP(S), FTP, BitTorrent, YouTube | HTTP(S), FTP, BitTorrent (aria2) |
| **Browser Integration** | Native Messaging host compatible with official FDM extensions | Proprietary extension protocol | Requires third-party bridges |
| **LAN Remote Control** | Built-in self-signed HTTPS + WebSocket with QR-code pairing | Proprietary cloud or LAN sync | Web-UI wrappers required |

### When to use something else

- **Need a Windows GUI today:** Ferrite DM currently leverages Unix file positioning (`FileExt`) and Unix domain sockets. Windows support is planned but not yet available.
- **Single one-off scripts:** If you only need to download a single file in a pipeline without queue management or browser handoff, standard `curl` or `wget` is simpler.
- **Multi-user remote seedbox:** For multi-user accounts and remote web trackers, specialized torrent daemons like `transmission-daemon` or `qbittorrent-nox` are more appropriate.

---

## Install

### macOS
Download signed packages from [GitHub Releases](https://github.com/itsarian/ferrite-dm/releases):
- **`.dmg`:** Open `Ferrite-DM-<version>-macos-<arch>.dmg` and drag **Ferrite DM** to `/Applications`.
- **`.pkg`:** Installs the app bundle and symlinks `fdm-gui`, `ferrite-dm`, and `ferrite-native-host` into `/usr/local/bin`.

Or install via script:
```bash
curl -fsSL https://raw.githubusercontent.com/itsarian/ferrite-dm/main/scripts/install-cli.sh | bash
```

### Linux (CLI & Daemon)
```bash
tar -xzf ferrite-dm-*-linux-x86_64.tar.gz
cd ferrite-dm-*
./install.sh   # Installs ferrite-dm and ferrite-native-host to ~/.local/bin
```

### Building from Source
Requires Rust Edition 2024 (`rustc >= 1.85`), a C compiler, and `cmake`:

```bash
git clone https://github.com/itsarian/ferrite-dm.git
cd ferrite-dm
cargo build --release --workspace

# Verify build with the isolated offline self-test
./target/release/ferrite-dm self-test
```

---

## Quick Start

Verify the installation:

```bash
ferrite-dm --version
# ferrite-dm 0.1.0
```

### 1. Download via CLI
```bash
# Direct download (runs until finished, or enqueues if GUI/daemon is open)
ferrite-dm https://example.com/archive.tar.gz
```

### 2. Connect Your Web Browser
Enable one-click download capture from official Chrome, Firefox, Brave, and Edge extensions:

```bash
ferrite-dm native-host-install --browser all
```

> [!NOTE]
> The browser native host automatically spawns `ferrite-dm daemon` in the background if neither the GUI nor daemon is already running when a download starts.

---

## Desktop App

Launch the GPUI desktop interface or pass a URL to pre-fill the Add modal:

```bash
fdm-gui [OPTIONS] [URL | MAGNET | .TORRENT]
```

### Keyboard Shortcuts

| Shortcut | Action | Shortcut | Action |
| :--- | :--- | :--- | :--- |
| `⌘N` | Add URL, magnet link, or torrent | `Space` | Pause or resume selected download |
| `⌘B` | Batch wildcard download modal | `Delete` | Remove selected download |
| `⌘V` | Paste URL from clipboard | `⌘A` | Select all downloads |
| `⌘F` | Filter and search downloads | `⌘,` | Open Preferences |
| `⌘I` | Toggle bottom Details Inspector | `⌘=` / `⌘-` | Zoom in / Zoom out |

### Inspector Views (`⌘I`)
The bottom inspector pane provides 6 diagnostic tabs:
- **`GENERAL`** — File location, transfer progress, total size, and estimated time remaining.
- **`DETAILS`** — HTTP response headers, MIME type, ETag, redirect hops, and mirror fallbacks.
- **`FILES`** — Interactive file selection tree for multi-file torrent archives.
- **`PROGRESS`** — Visual chunk and BitTorrent piece completion map.
- **`CONNECTIONS`** — Active socket connections, remote IP addresses, and throughput per peer.
- **`SPEED`** — Real-time download and upload transfer graph.

---

## CLI Usage

When `fdm-gui` or `ferrite-dm daemon` is active, CLI commands automatically communicate over Unix domain socket `ferrite.sock`. When run standalone, commands operate directly against the local SQLite database.

```bash
# Advanced download with mirror fallback, custom filename, and priority
ferrite-dm add "https://cdn1.example.com/installer.dmg" \
  --mirror "https://cdn2.example.com/installer.dmg" \
  --dir ~/Downloads/Installers \
  --name App-Latest.dmg \
  --header "Authorization: Bearer <TOKEN>" \
  --priority high \
  --wait

# Inspect torrent metadata or download specific files sequentially
ferrite-dm torrent ~/Downloads/sample.torrent --inspect
ferrite-dm torrent "magnet:?xt=urn:btih:..." --select-files 0,2 --play-asap

# Batch download wildcard patterns (numeric ranges, character sets, brace lists)
ferrite-dm batch "https://example.com/parts/part-[01-20:2].zip" --dir ~/Downloads/Parts
ferrite-dm batch "https://example.com/builds/v[1-3]-{linux,macos}.tar.gz" --preview-only

# Manage download queue
ferrite-dm list
ferrite-dm pause 1
ferrite-dm resume 1
ferrite-dm cancel 1
ferrite-dm remove 1 --delete-files

# Switch Traffic Usage Modes (TUM throttling)
ferrite-dm tum snail    # Throttled (64 KiB/s down, 32 KiB/s up)
ferrite-dm tum medium   # Balanced (4 MiB/s down, 1 MiB/s up)
ferrite-dm tum high     # Unlimited bandwidth and connections

# Compute and verify cryptographic digest
ferrite-dm hash ~/Downloads/ubuntu.iso --algo sha256 --expect <HEX_DIGEST>

# Media remuxing and audio extraction via FFmpeg
ferrite-dm mp3 ~/Downloads/podcast.mkv ~/Music/podcast.mp3 --bitrate 320
ferrite-dm mp4 ~/Downloads/capture.ts ~/Videos/output.mp4

# Import legacy Free Download Manager database
ferrite-dm import-fdm-db ~/Library/Application\ Support/Free\ Download\ Manager/db.sqlite
```

---

## Platform Support

| Operating System | Tier | Status | Notes |
| :--- | :--- | :--- | :--- |
| **macOS** *(11+)* | Tier 1 | **Verified / Primary** | Full GPUI desktop application, menu bar item, Dock transfer badges, LaunchAgent autorun, sleep prevention (`caffeinate`), and system notifications. |
| **Linux** *(x86_64)* | Tier 2 | **CLI & Daemon** | Headless engine, CLI client, Unix socket IPC, and browser Native Messaging host. *(GPUI desktop packaging is experimental).* |
| **Windows** | Tier 3 | *In Development* | File transfer and IPC crates currently require Unix domain sockets and Unix file positioning APIs. Windows port is planned. |

### Default Data Paths
- **macOS:** `~/Library/Application Support/org.ferritedm.FerriteDM`
- **Linux:** `~/.local/share/ferritedm` (`$XDG_DATA_HOME/ferritedm`)
- **Portable Mode (`--portable`):** `./ferrite-data/` adjacent to the executable binary

---

## Architecture

Ferrite DM is structured as a modular Cargo workspace:

```
ferrite-dm/
├── crates/
│   ├── fdm-core          # SQLite WAL persistence, rate limiting, path routing, and migration
│   ├── fdm-http          # Multi-connection HTTP/HTTPS/FTP transfer engine
│   ├── fdm-bt            # BitTorrent engine (librqbit) with sequential streaming
│   ├── fdm-hls           # HLS (.m3u8) & DASH (.mpd) stream fetcher and muxer
│   ├── fdm-batch         # Wildcard range and brace expansion parser
│   ├── fdm-media         # Boa JavaScript add-ons & FFmpeg conversion helpers
│   ├── fdm-ipc           # Unix socket IPC and LAN Remote Control web server
│   ├── fdm-native-host   # WebExtension Native Messaging protocol host
│   ├── fdm-cli           # CLI executable and orchestration engine
│   └── fdm-gui           # Native GPUI desktop application
└── docs/
    └── ARCHITECTURE_BLUEPRINT.md  # Detailed FDM mapping & architecture specification
```

For design rationale, protocol contracts, and subsystem mappings, see [docs/ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md).

---

## Licence

Dual-licensed under either:

- **[MIT Licence](LICENSE-MIT)**
- **[Apache Licence, Version 2.0](LICENSE-APACHE)**

at your option.
