# Ferrite DM

Fast, local-first, telemetry-free desktop and CLI download manager built in Rust (Edition 2024) for Free Download Manager (FDM 6.x) workflow compatibility — powered by Tokio, GPUI, SQLite (WAL), `rustls`, Boa JS, and `librqbit`.

---

## Highlights

- **Zero Telemetry & Local-First State** — No analytics, background phone-home endpoints, ads, or nag prompts. Downloads, tags, SSL exceptions, settings, and backups live locally in `ferrite.sqlite` (SQLite WAL).
- **Multi-Protocol Engine**
  - **HTTP / HTTPS / FTP** — Multi-section range splitting, mirror fallback, custom headers/cookies/referers, HTTP auth, per-protocol HTTP/HTTPS/SOCKS5 proxies, pinned TLS certificate exceptions, and `SetMarkOfTheWeb`.
  - **BitTorrent & Magnets** — Native `librqbit` engine with DHT, uTP, PEX, LSD, UPnP/NAT-PMP, tracker overrides, encryption policy, selective file downloads, `.torrent` watch folder, and sequential **Play ASAP** streaming.
  - **HLS (`.m3u8`) & DASH (`.mpd`)** — Manifest parsing, AES-128 segment decryption, live playlist reloading, and FFmpeg A/V muxing.
- **Desktop GUI, Scriptable CLI & Shared Daemon**
  - **`fdm-gui`** — Native GPUI desktop app with Light/Dark/System themes, New/Classic UI styles, compact mode, scaling (`⌘=` / `⌘-` / `⌘0`), 6 locales, 6-tab bottom inspector, standalone Add/Download windows, and macOS menu bar & Dock integration.
  - **`ferrite-dm`** — CLI and headless background daemon communicating over Unix domain socket IPC (`ferrite.sock`), sharing the exact same `DownloadEngine`.
  - **`ferrite-native-host`** — WebExtension Native Messaging host (`org.freedownloadmanager.fdm5.cnh`) compatible with official FDM browser extensions.
- **Automation, Shaping & Remote Control**
  - **Batch Expansion** — Numeric ranges (`[01-20:2]`), character ranges (`[a-f]`), brace lists (`{linux,macos,windows}`), and multiline URL files.
  - **4 Traffic Usage Modes (TUM)** — `Snail`, `Low`, `Medium`, and `High` bandwidth/concurrency/ratio profiles, day/time scheduler, slow-download watchdog, battery & roaming guards, and macOS sleep prevention.
  - **JS Media Add-ons** — Sandboxed Boa JavaScript parser plugins (`.fda`, `.zip`, `.js`) with optional `yt-dlp` integration.
  - **LAN Remote Control** — Built-in self-signed HTTPS + WebSocket Web UI with `fdm://remote_control/...` QR code pairing (default port `9443`).
  - **Post-Processing & Migration** — File hash verification (`MD5`, `SHA-1`, `SHA-256`, `SHA-512`), FFmpeg MP3/MP4 conversion, antivirus hook, macro-based folder routing, JSON/URL/CSV exports, and best-effort migration from legacy FDM `db.sqlite`.

---

## Requirements & Platform Support

### Build Prerequisites
- **Rust toolchain** supporting **Edition 2024** (`rustc >= 1.85`)
- **Native build tools** needed by bundled dependencies (`cmake`, a C compiler, and Xcode Command Line Tools on macOS)

### Operating Systems
| Platform | Status | Notes |
| :--- | :--- | :--- |
| **macOS** | Primary (verified) | Full GUI (`fdm-gui`), menu bar, Dock speed/count badges, LaunchAgent login autorun, `caffeinate` sleep guard, `osascript`/sound notifications, CoreGraphics modifier-key bypass, and LaunchServices `.torrent`/`magnet:` checks. |
| **Linux** | Core / CLI compatible | Core Unix I/O (`FileExt`), Unix socket IPC (`ferrite.sock`), and Linux `NativeMessagingHosts` paths are supported; macOS-only desktop integrations gracefully no-op. GUI and packaging have not been tested on Linux. |
| **Windows** | Not currently supported | Core transfer and IPC crates (`fdm-http`, `fdm-ipc`) currently rely on Unix-specific file positioning and Unix domain sockets. |

### Optional Runtime Tools
- **`ffmpeg` / `ffprobe`** — HLS/DASH audio-video muxing, `mp3`/`mp4` conversion, and media probing/thumbnails (auto-detected in `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, or configurable in **Preferences**).
- **`yt-dlp`** — Extended media-site extraction via JavaScript parser add-ons.
- **`unzip` / `zip`** — Required to unpack `.fda`/`.zip` add-on archives (`unzip`) and export diagnostics bundles (`zip`).
- **ClamAV (`clamscan` / `clamdscan`)** or any custom AV executable — Post-download virus scanning.

---

## Building from Source

```bash
git clone https://github.com/itsarian/ferrite-dm.git
cd ferrite-dm

# Build all binaries in release mode
cargo build --release --workspace
```

Compiled binaries are placed together in `target/release/`:
- `target/release/fdm-gui` — GPUI desktop application
- `target/release/ferrite-dm` — CLI client and headless daemon
- `target/release/ferrite-native-host` — Browser native messaging host

### Packaging Install Binaries (macOS)

To build macOS install packages (`.app` application bundle, mountable `.dmg`, `.pkg` system installer, and standalone `.tar.gz` with install script):

```bash
# Build release binaries and generate all install packages in dist/
./scripts/package-macos.sh

# Or reuse existing target/release binaries without recompiling:
./scripts/package-macos.sh --skip-build
```

Generated packages are placed in `dist/`:
- `dist/Ferrite DM.app` — Standalone macOS app bundle (signed ad-hoc with `org.ferritedm.FerriteDM`, Retina icon, and registered `magnet:`, `fdm:`, and `.torrent` handlers)
- `dist/Ferrite-DM-<version>-macos-<arch>.dmg` — Apple Disk Image with drag-and-drop `/Applications` installer
- `dist/Ferrite-DM-<version>-macos-<arch>.pkg` — System installer (installs app to `/Applications` and symlinks CLI tools to `/usr/local/bin`)
- `dist/ferrite-dm-<version>-macos-<arch>.tar.gz` — Standalone portable archive with `install.sh` and `uninstall.sh`

### Installing Into Your `PATH`

Copy or symlink all three binaries into the **same** directory on your `PATH` (for example `/usr/local/bin` or `~/.local/bin`):

```bash
install -m 755 target/release/fdm-gui target/release/ferrite-dm target/release/ferrite-native-host ~/.local/bin/
```

> [!IMPORTANT]
> Keep `ferrite-dm` and `ferrite-native-host` in the **same directory**. `ferrite-dm native-host-install` looks for sibling `ferrite-native-host` by default, and `ferrite-native-host` automatically spawns sibling `ferrite-dm --hidden daemon` when a browser triggers a download while no daemon or GUI is open.

### Verify Your Build

Run the offline self-test (spawns an ephemeral loopback HTTP range server in a temporary directory and verifies SHA-256 without touching user data) and/or the workspace test suite:

```bash
./target/release/ferrite-dm self-test
cargo test --workspace
```

---

## Desktop GUI (`fdm-gui`)

Launch directly from Cargo or run the compiled binary:

```bash
cargo run --release -p fdm-gui
# or
fdm-gui [OPTIONS] [URL]
```

### Launch Flags
- `--hidden` / `--service` / `-service` — Start hidden in the background while keeping the engine and Unix IPC socket active
- `--portable` — Store `ferrite.sqlite`, settings, logs, and `plugins/` in `ferrite-data/` beside the executable (also activated automatically if a `ferrite-portable.flag` file sits next to `fdm-gui`)
- `--data-dir <DIR>` — Use a custom data directory
- `[URL]` — Pre-fill and probe a URL, `magnet:` link, or `.torrent` path in the Add Download dialog (or standalone Add window if enabled)

### Keyboard Shortcuts (macOS)
| Shortcut | Action |
| :--- | :--- |
| `⌘N` | Add URL / magnet / `.torrent` |
| `⌘B` | Batch Download dialog |
| `⇧⌘V` (or `⌘V` on list) | Paste URL from clipboard |
| `⌘F` | Focus download search |
| `⌘I` | Toggle bottom Details Inspector (`GENERAL`, `DETAILS`, `FILES`, `PROGRESS`, `CONNECTIONS`, `SPEED`) |
| `Space` | Start / Pause selected download |
| `Delete` / `Backspace` | Remove / Delete selected download(s) |
| `⇧⌘A` | Select / Deselect all visible downloads |
| `⌘,` | Open Preferences |
| `⌘=` / `⌘-` / `⌘0` | Zoom In / Zoom Out / Reset zoom |
| `Esc` | Close active modal or popover |

> [!TIP]
> **Hidden Developer & Diagnostic Panels:**
> - Click the **Preferences** heading 5 times (or use **Help → Troubleshooting & Diagnostics…**) to reveal the **Troubleshooting** tab (logging toggles and diagnostics `.zip` export).
> - Click the **Add-ons** heading 5 times to unlock unsigned local `.fda` / `.js` testing in **Add-ons**.

Supported UI languages in **Preferences → General**: `en_US`, `en_GB`, `de_DE`, `fr_FR`, `es_ES`, and `ja_JP`.

---

## CLI & Background Daemon (`ferrite-dm`)

If `fdm-gui` or `ferrite-dm daemon` is already running against the same `--data-dir`, CLI commands (`add`, `list`, `pause`, `resume`, `cancel`, `remove`, `start-all`, `pause-all`, `tum`) talk to it over `ferrite.sock`. Otherwise, standalone commands open SQLite directly and exit when done.

```bash
ferrite-dm --help
```

### Common Recipes

#### 1. Run a headless daemon (with optional LAN Remote Control)
```bash
ferrite-dm daemon
# or expose HTTPS + WebSocket Remote Control on LAN IP
ferrite-dm daemon --remote-control --remote-host 192.168.1.50
```

#### 2. Add HTTP/FTP/HLS/DASH or Torrent downloads
```bash
# Direct URL (runs and waits until completion if no daemon is running)
ferrite-dm https://example.com/archive.tar.gz

# Full options: custom path, filename, mirrors, headers, priority
ferrite-dm add "https://cdn1.example.com/app.dmg" \
  --mirror "https://cdn2.example.com/app.dmg" \
  --dir ~/Downloads/Installers \
  --name App-Latest.dmg \
  --referer "https://example.com/" \
  --header "Authorization: Bearer TOKEN" \
  --priority high \
  --wait

# Queue paused or stream video/torrent sequentially
ferrite-dm add "https://example.com/stream/master.m3u8" --play-asap --paused
```

#### 3. Batch wildcard downloads
`ferrite-dm batch` supports zero-padded/stepped numeric ranges (`[01-20:2]`), alphabetic ranges (`[a-f]`, `[A-Z:2]`), brace lists (`{linux,macos}`), Cartesian combinations, and newline-separated URL files:

```bash
# Preview expanded URLs without downloading
ferrite-dm batch "https://example.com/builds/v[1-3]-{linux,macos}-[a-b].tar.gz" --preview-only

# Enqueue from a pattern or URL list file
ferrite-dm batch "https://example.com/parts/part-[01-12:2].zip" --dir ~/Downloads/Parts
ferrite-dm batch --file urls.txt --paused
```

#### 4. BitTorrent & Magnet inspection / selective files
```bash
# List files and info-hash before downloading
ferrite-dm torrent ~/Downloads/linux.torrent --inspect

# Download only 0-based file indices 0 and 2 sequentially
ferrite-dm torrent "magnet:?xt=urn:btih:..." --select-files 0,2 --play-asap --dir ~/Downloads/Linux
```

#### 5. Manage the queue & Traffic Usage Modes
```bash
ferrite-dm list
ferrite-dm list --state downloading --json
ferrite-dm pause 1
ferrite-dm resume 1
ferrite-dm cancel 1
ferrite-dm remove 1 --delete-files
ferrite-dm start-all
ferrite-dm pause-all

# Switch global throttling profile: snail | low | medium | high
ferrite-dm tum snail
```

Default Traffic Usage Mode (TUM) profiles:
| Mode | Download Cap | Upload Cap | Max Connections (Per Server) | Concurrent Downloads | Max Ratio |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `snail` | 64 KiB/s | 32 KiB/s | 4 (2) | 1 | 1.0 |
| `low` | 512 KiB/s | 128 KiB/s | 12 (4) | 2 | 1.5 |
| `medium` | 4 MiB/s | 1 MiB/s | 32 (8) | 4 | 2.0 |
| `high` | Unlimited | Unlimited | 64 (16) | 8 | Unlimited |

#### 6. Tags, settings & proxies
```bash
ferrite-dm tags list
ferrite-dm tags create "Linux ISOs" --color "#3B82F6"
ferrite-dm tags assign 1 10

ferrite-dm settings show
ferrite-dm settings set-download-dir ~/Downloads
ferrite-dm settings set-max-concurrent medium 4
ferrite-dm settings set-existing-reaction rename   # ask | overwrite | skip | rename
ferrite-dm settings set-proxy manual --host 127.0.0.1 --port 8080   # none | system | manual
```

#### 7. Hashing, FFmpeg media conversion & backups
```bash
# Compute or verify integrity digest (md5 | sha1 | sha256 | sha512)
ferrite-dm hash ~/Downloads/ubuntu.iso --algo sha256 --expect <HEX_DIGEST>

# Extract/convert audio or remux/transcode video via FFmpeg
ferrite-dm mp3 ~/Downloads/video.mkv ~/Music/audio.mp3 --bitrate 320
ferrite-dm mp4 ~/Downloads/stream.ts ~/Videos/movie.mp4          # stream copy by default
ferrite-dm mp4 ~/Downloads/raw.avi ~/Videos/movie.mp4 --transcode

# Export/import downloads, verified settings bundles, or migrate legacy FDM db.sqlite
ferrite-dm export downloads.json
ferrite-dm import downloads.json --paused
ferrite-dm export-settings settings.json
ferrite-dm import-settings settings.json
ferrite-dm import-fdm-db ~/Library/Application\ Support/Free\ Download\ Manager/db.sqlite
```

#### 8. Remote Control server & QR pairing
```bash
ferrite-dm remote-control --host 192.168.1.50
```
Prints the HTTPS Web UI URL (`https://192.168.1.50:9443/?token=...`), the `fdm://remote_control/...` pairing URI, and an ASCII QR code. Uses a locally generated self-signed TLS certificate (`rc_cert.der` / `rc_key.der`) stored in your data directory — no cloud relay required.

---

## Browser Integration (`ferrite-native-host`)

Ferrite DM implements the Free Download Manager WebExtension Native Messaging protocol (`org.freedownloadmanager.fdm5.cnh`), allowing official FDM browser extensions for Chromium-based browsers and Firefox to hand off downloads, query interception rules, and pass proxy/cookie/video-sniffer payloads directly to Ferrite DM.

Install manifests from the CLI (or toggle per-browser integration in **Preferences → Browser Integration**):

```bash
# Supported targets: chrome | firefox | brave | edge | vivaldi | opera | all
ferrite-dm native-host-install --browser chrome
ferrite-dm native-host-install --browser all

# Custom native host executable or output directory
ferrite-dm native-host-install --browser firefox --host-exe ~/.local/bin/ferrite-native-host
```

> [!NOTE]
> Registered extension IDs in the manifest are `ahmpjcflkgiildlgicmcieglgoilbfdp` / `mdfcjfioplkdchnhcpcobaheocanedjg` (Chrome/Chromium family) and `fdm_ffext@freedownloadmanager.org` / `fdm_ffext2@freedownloadmanager.org` (Firefox). Safari is not currently supported for native messaging.

---

## Path Macros, Data Directory & Environment Variables

### Download Path Macros
Destination templates (in **Preferences → General** or `--dir`) support dynamic expansion:
- `%date%` (`YYYY-MM-DD`), `%year%`, `%month%`, `%day%`
- `%host%`, `%server%` (source URL hostname)
- `%path_on_server%` (parent path segments from the URL)
- `%ext%` (file extension), `%type%` (`Video`, `Audio`, `Archive`, etc.), `%content_type%`

Example: `~/Downloads/%host%/%type%/%date%`

### Data Directory Locations
- **macOS (default):** `~/Library/Application Support/org.ferritedm.FerriteDM`
- **Linux (default):** `$XDG_DATA_HOME/ferritedm` (typically `~/.local/share/ferritedm`)
- **Portable (`--portable`):** `./ferrite-data` beside the executable

Contents include `ferrite.sqlite` (and `.sqlite-wal`/`.sqlite-shm`), `ferrite.sock`, `plugins/`, `rc_cert.der` / `rc_key.der`, and optional `logs/ferrite.log`.

### Environment Variables
| Variable | Purpose |
| :--- | :--- |
| `FERRITE_THEME` | Override Light/Dark theme (`light` or `dark`) when theme mode is set to `Light`/`Dark` |
| `FERRITE_UPDATE_MANIFEST_URL` | Optional local file path, `file://`, or `https://` JSON manifest URL for app update checks (`version`/`tag_name`, `download_url`, optional `sha256` & `notes`). Unset by default so update checks remain 100% offline. |

---

## JavaScript Add-ons

Site and media parser plugins live in `<data_dir>/plugins/` and can be managed via CLI (`ferrite-dm plugins list|enable|disable|uninstall`) or the GUI **Add-ons** view (`.fda`, `.zip`, or single `.js` files):
- Scripts run in an embedded Boa JS runtime with async `fetch`, timers, optional browser cookie forwarding, and optional `yt-dlp` helpers.
- Installing `.fda` / `.zip` archives uses system `unzip` and verifies `manifest.json` checksums unless developer mode is unlocked.

---

## Workspace Architecture

| Crate | Role |
| :--- | :--- |
| [`crates/fdm-core`](crates/fdm-core) | Domain models, SQLite WAL persistence & backups, 4-profile bandwidth limiter, scheduler, macro/type/host routing, file hashing, OS integration, export/import & FDM SQLite import |
| [`crates/fdm-http`](crates/fdm-http) | Multi-section HTTP/HTTPS/FTP downloader, per-protocol proxies, TLS pin exceptions, mirrors, auth, slow-transfer watchdog |
| [`crates/fdm-hls`](crates/fdm-hls) | HLS (`.m3u8`) & DASH (`.mpd`) downloader, AES-128 segment decryption, FFmpeg A/V muxing |
| [`crates/fdm-bt`](crates/fdm-bt) | BitTorrent & magnet engine (`librqbit`) with DHT/uTP/PEX/LSD, encryption policy, `.torrent` watch folder, and sequential **Play ASAP** streaming |
| [`crates/fdm-batch`](crates/fdm-batch) | Wildcard numeric/alpha ranges, brace alternation, and multiline batch URL parser |
| [`crates/fdm-media`](crates/fdm-media) | Boa JS plugin runtime, `.fda`/`.zip`/`.js` installer, plugin auto-updater, FFmpeg conversion & thumbnail/probe helpers |
| [`crates/fdm-ipc`](crates/fdm-ipc) | Unix-socket JSON-line IPC (`ferrite.sock`) and self-signed HTTPS/WebSocket LAN Remote Control with QR pairing |
| [`crates/fdm-native-host`](crates/fdm-native-host) | Stdio length-prefixed Native Messaging host (`ferrite-native-host`) compatible with FDM browser extensions |
| [`crates/fdm-cli`](crates/fdm-cli) | Shared `DownloadEngine` orchestration and `ferrite-dm` CLI binary |
| [`crates/fdm-gui`](crates/fdm-gui) | Native GPUI desktop interface, localization, theming, standalone windows, and macOS system integration |

For design background on the underlying FDM feature mapping, see [docs/ARCHITECTURE_BLUEPRINT.md](docs/ARCHITECTURE_BLUEPRINT.md).

---

## License

Dual-licensed under either of:
- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
