# Ferrite DM

Fast, local-first, telemetry-free Rust desktop & CLI download manager modeled for 100% Free Download Manager (FDM 6.34) feature parity—built with Rust (Edition 2024), Tokio, GPUI, SQLite, and `librqbit`.

## Workspace Structure

| Crate | Purpose |
| :--- | :--- |
| `crates/fdm-core` | Core domain models, SQLite persistence + backups, speed limiter (4 TUM profiles), scheduler, path/macro routing, hashing, OS integration, export/import & FDM 5 DB migration |
| `crates/fdm-http` | Multi-section HTTP/HTTPS/FTP downloader, proxy & SSL exception handling, auth, mirrors, slow-transfer watchdog |
| `crates/fdm-hls` | HLS / M3U8 & DASH stream downloader with AES-128 decryption and FFmpeg A/V muxing |
| `crates/fdm-bt` | BitTorrent & magnet engine (`librqbit`) with sequential **Play ASAP** streaming, DHT/uTP/PEX/LSD, port/encryption policies, and `.torrent` watch folder |
| `crates/fdm-batch` | Batch/wildcard/range URL expansion and multiline batch parsing |
| `crates/fdm-media` | Sandboxed JavaScript site-parser add-ons runtime, plugin auto-updater, FFmpeg conversion & thumbnail extraction |
| `crates/fdm-ipc` | Local Unix-socket IPC and LAN Remote Control pairing (`fdm://remote_control/...` QR codes) |
| `crates/fdm-native-host` | Chrome / Edge / Firefox / Brave / Safari WebExtension Native Messaging host (`ferrite-native-host`) |
| `crates/fdm-cli` | Shared `DownloadEngine` orchestration and `ferrite-dm` CLI binary |
| `crates/fdm-gui` | Native GPUI desktop application (macOS menu bar + Dock integration, Light/Dark themes, 6 locales, standalone dialogs) |

## Quick Start

### Launch the Desktop GUI
```bash
cargo run -p fdm-gui
```

Optional flags:
- `--hidden` — Start minimized / in background
- `--portable` — Store config, SQLite DB, and plugins beside the executable (`ferrite-data/`)
- `--data-dir <DIR>` — Use a custom data directory

### Run the CLI (`ferrite-dm`)
```bash
cargo run -p fdm-cli -- --help
```

### Build All Binaries (Release)
```bash
cargo build --release --workspace
```

Outputs in `target/release/`:
- `fdm-gui` — Desktop application
- `ferrite-dm` — Command-line client & background engine
- `ferrite-native-host` — Browser native messaging host

### Run Tests
```bash
cargo test --workspace
```

## Optional External Tools
- **FFmpeg** (`ffmpeg` / `ffprobe`): Video/audio muxing, MP3/MP4 conversion, and media probing
- **yt-dlp**: Extended site extraction via add-ons
- **ClamAV** (`clamscan` / `clamdscan`): Automatic post-download antivirus scanning

## Architecture Reference
See [docs/ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md) for the full architectural design and feature mapping.
