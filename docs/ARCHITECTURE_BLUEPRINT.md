# Free Download Manager 6.34.4 — Reverse Engineering Analysis & Rust Architecture Blueprint

> [!NOTE]
> **Scope & Provenance:** Every finding below distinguishes **REA Shipped-Artifact Evidence** (with `evidence_id` and SHA-256), **Supplementary Static Extraction** (`strings`, `c++filt`, zlib Qt-RCC extraction), **Inferences**, and **Unknowns**. No target binaries were executed; static observations do not prove runtime execution paths.
>
> **Environment Caveats Observed:**
> - **Hopper Disassembler** (`provider_id: "hopper"`): Unavailable in this environment because macOS denied Apple Events Automation permission (`Not authorized to send Apple events to Hopper Disassembler`).
> - **REA MCP in Antigravity**: Configuration at `~/.gemini/config/mcp_config.json` is aligned, so analysis was executed directly via the `rea` CLI (`rea-agents@6.8.0`).
> - **Rust Toolchain**: Neither `cargo`, `rustc`, nor `rustup` is currently installed on this host.

---

## 1. Bundle Architecture & Binary Inventory (REA Evidence)

Inspection of [Free Download Manager.app](file:///Applications/Free%20Download%20Manager.app) via REA `inspect-desktop-application` (`ev_ea22013d8e848e1a2acc5d3898b2453e3b4b0fddb144c5a72718d0a92c366b29`, summary `ev_e299c43b921aaadb51f6e719cbda4c2087321a6c4ab46687fdc5e36b3f82c21d`, snapshot SHA-256 `edafcf135d7740ef535cbd6175b9333d486005ac253d340bfceb2b0783618952`, 2,527 entries), `inspect-plist` (`ev_20ac663a9661e2cac0a7c23c5ea07726bc35b27681c05cfa9865fd1a9a2164a5`), `inspect-signature` (`ev_147a9e4cb98835cd901d0f1f050055df031f09cbdbed744592750d33c40e7ac1`), `list-architectures` (`ev_76e6ef75d629adf18caea1ad60b2c7cec544b55753ba17eaafb6c0b43298a4e0`), and `trace-dylib-resolution` (`ev_58cbf1df33e0e353cfbfe7b01865370062e3455186d05ce66353441caf509ff5`) establishes:

- **Application Identity:** `org.freedownloadmanager.fdm6`, version `6.34.4`, single-architecture `arm64` Mach-O with Hardened Runtime, signed by `Developer ID Application: Tatyana Livinskaya (8CK79SY7LA)`.
- **Registered Document & URL Handlers:** `.torrent` (`org.bittorrent.torrent`), `magnet://`, `http://`, `https://`, `ftp://` (with `NSAllowsArbitraryLoads: true`).
- **Four Executable Roots:**
  1. [fdm](file:///Applications/Free%20Download%20Manager.app/Contents/MacOS/fdm) (17.6 MB, `ev_33e565dc...`, SHA-256 `db2be8c4450e7214cc7e1a103c7b8d9d94a4e0384215e7709c2ea4148097201b`) — Main Qt 6.10.3 / QML desktop client, core backend engine, IPC servers (`fdm6bs`, `fdm6fs`), and Remote Control server/client.
  2. [wenativehost](file:///Applications/Free%20Download%20Manager.app/Contents/MacOS/wenativehost) (1.36 MB, `ev_397bf055...`, SHA-256 `76e6f82db92175c119bbca5ebe1b60c481f1e736306c6d95b7d3ad9ff3026969`) — Browser WebExtension Native Messaging host (`ev_e2adedb7...` dispatch metadata).
  3. [importwizard](file:///Applications/Free%20Download%20Manager.app/Contents/MacOS/importwizard) (1.89 MB, `ev_bca27cf1...`, SHA-256 `3f59f79a96eef4e5ed6ee59c13f2dcca148e8edcdce301e46c450205844cbad9`) — Legacy FDM 5 → FDM 6 SQLite/settings migration tool.
  4. [ffmpeg](file:///Applications/Free%20Download%20Manager.app/Contents/MacOS/ffmpeg) (22.8 MB, SHA-256 `989d6bca93048887bcb9292ce3bb9f4e16d348a5d62c32d0da3cb4cad921ee70`) — Media muxing, MP3/MP4 transcoding, and video thumbnail extraction.
- **Modular Custom Libraries (`Contents/Frameworks/`, all `@rpath` resolved):**

| Library | Size | REA `inspect-macho` Evidence | SHA-256 (prefix) | Role (observed exported classes) |
| :--- | :---: | :--- | :--- | :--- |
| `libvmsclshared.6.34.4.dylib` | 2.0 MB | `ev_ca942f3f...` | `36229003...` | Shared runtime: `AbstractDownload`, `AbstractMediaDownload`, `IndependentSegmentsDownloader`, `QtProcessLaunchManager`, `QtFfmpegAvMerger`, `QtDenoManager`, `QtPythonManager`, auth/SSL/proxy keepers, SQLite abstractions |
| `libdownloadswww.6.34.4.dylib` | 6.4 MB | `ev_9d339640...` | `9fef9b72...` | Multi-section HTTP/HTTPS/FTP engine (`WwwDownload`, `WwwDownloadSection`, `CurlDownloader`, `DownloadMirrorsUsageManager`, `HttpHeadersParser`, `QtAuthenticatorsKeeper`) |
| `libdownloadsbt.6.34.4.dylib` | 13.6 MB | `ev_4c645ed5...` | `4ec25f43...` | BitTorrent engine wrapping `libtorrent` + OpenSSL + Boost.Asio (`BtDownload`, `QtLtSession`, `QtLtTorrent`, `QtLtTorrentFileParser`, `QtFileAllocator`, `AbstractDownloadMediaPlayAsapManager`) |
| `libdownloadsms.6.34.4.dylib` | 1.5 MB | `ev_8d5e14aa...` | `aa296532...` | Media site & playlist parser (`MsDownload`, `MsBatchDownload`, `MsMediaResourceVersions2`, `QtMsJsEngine`, `QtMsJsPythonManager`, `QtJsAbstractDownload`, `MsSubtitlesInfo`) |
| `libdownloadsm3u.6.34.4.dylib` | 500 KB | `ev_55bea65f...` | `585beecd...` | HLS/M3U8 parser & live/VOD stream downloader (`ExtendedM3U`, `M3UStreamDownload`, `M3UStreamComplexDownload`, `M3UMediaPlaylistReloader`) |
| `libdownloadsfragments.6.34.4.dylib` | 599 KB | `ev_03152ae2...` | `445d5f59...` | Fragmented/segmented stream downloads (`FragmentsDownload`, `FragmentDownloadsModule`) |
| `libdownloadsbatch.6.34.4.dylib` | 422 KB | `ev_63c46539...` | `659ce676...` | Batch/wildcard URL downloads (`BatchDownload`, `BatchDownloadsModule`) |
| `libdownloadsjsp.6.34.4.dylib` | 228 KB | `ev_3043b02b...` | `88016b22...` | JavaScript plugin loader & manager (`JsPluginInfo`, `JsPluginsEnumMgr`, `JspMsDownloadsModule`, `JspMsBatchDownloadsModule`) |
| `liblogger.6.34.4.dylib` | 101 KB | — | `72194ad0...` | Logging subsystem |
| `libquazip.6.34.4.dylib` | 415 KB | — | `4844420a...` | ZIP extraction for plugins/updates (`QuazipArchiveExtractor`) |

> [!NOTE]
> `rea inspect-macho` export listings report `exhaustive: false` on large binaries due to tool collection caps (e.g., 20,000 exports returned for `fdm`), so unreturned internal/non-exported symbols exist.

---

## 2. Complete Feature Surface (Observed in Shipped Artifacts)

From REA symbol inventories and supplementary static extraction of `AppFeatures`, `AppSettings`, `DmCoreSettings`, `AbstractDownloadsUi`, `KnownWebBrowsers`, embedded QML (`V2/*_V2.qml`, `desktop/main.qml`, `mobile/main.qml`), and `qtJsMsParserLib.js`:

### A. Download Engines & Media Capabilities
1. **HTTP / HTTPS / FTP Multi-Section Engine (`libdownloadswww`)**:
   - Multi-connection segmented downloading (`WwwDownloadSection`), dynamic section splitting, mirror management (`DownloadMirrorsUsageManager`, `AddMirrorPage.qml`), URL replacement (`ChangeUrlPage.qml`).
   - HTTP auth (`QtAuthenticatorsKeeper`, `AuthenticationPage.qml`), cookies, custom headers, POST data, proxy per-protocol (`NoProxy`, `SystemProxy`, `ManualProxy` for HTTP/HTTPS/FTP/SOCKS5 with credentials).
   - SSL/TLS certificate inspection & interactive exception persistence (`QtKnownSslHostsKeeperInSql`, `QtSslCertificatesKeeperInSql`).
   - Remote resource change detection & restart prompt (`AbstractDownloadsOnRemoteResourceChangedLogics`, `RemoteResourceChangedDialog.qml`).
2. **BitTorrent & Magnet Engine (`libdownloadsbt`)**:
   - `.torrent` file & `magnet:` link parsing, DHT, uTP, PEX, LSD, UPnP/NAT-PMP port forwarding, configurable session port, custom tracker lists, encryption policy (`NoEncryptionAllowed`, `PreferEncryption`, `RequireEncryption`).
   - Selective file priority (`DownloadPriorityDontDownload`, `Low`, `Normal`, `High`), sequential download & "Play ASAP" media streaming (`AbstractDownloadMediaPlayAsapManager`), upload ratio limit (`MaxURatio`), watch folder for `.torrent` files (`TorrentsFolder`, `ForceSilentDownloadsFromTorrentFolder`).
3. **HLS/M3U8 & Fragmented Streams (`libdownloadsm3u`, `libdownloadsfragments`)**:
   - Master & media playlist parsing (`EXT-X-MAP`, `EXT-X-BYTERANGE`), live playlist reloading (`M3UMediaPlaylistReloader`), complex multi-track A/V muxing (`QtFfmpegAvMerger`), endless-stream handling (`EndlessStream`).
4. **Batch & Media Site Parsers (`libdownloadsbatch`, `libdownloadsms`, `libdownloadsjsp`)**:
   - Batch URL expansion (`BatchDownload`).
   - Extensible JS plugin system (`JsPluginInfo`, `manifest.js`, `AppJsPluginInstaller`, auto-updater against `files2.freedownloadmanager.org/mediasiteparser/fdm6/`).
   - Embedded JS runtime API (`qtJsMsParserLib.js`: `downloadUrlAsUtf8Text`, `setTimeout`/`clearTimeout`, `qtJsDownloadsFactory`, `qtJsNetworkCookie`, `qtJsNetworkProxy`, `qtJsTmpFile`, and `launchPythonScript` / Deno / Python dependency managers!).
   - Video/audio/subtitle version selection (`MsMediaResourceVersions2`, `MsSubtitlesInfo`) and channel/playlist batch extraction (`MsBatchDownload`).
5. **Post-Processing & File Lifecycle**:
   - FFmpeg MP3/MP4 conversion (`Mp3ConverterPage.qml`, `Mp4ConverterPage.qml`) and video thumbnail previews (`QtFfmpegVideoFileThumbnailExtractor2`).
   - Cryptographic hash verification (`MD5`, `SHA-1`, `SHA-256`, `SHA-512` via `FileIntegrityPage.qml`).
   - Duplicate/existing-file reactions (`DefrAsk`, `DefrOverwrite`, `DefrRename`; per-file `Overwrite`/`Rename`/`Skip`).
   - Move/rename active or finished files (`AbstractDownloadsMoveFilesManager`, `AbstractDownloadsRenameFilesManager`), path auto-routing by file type (`PathByFileTypeSqlTable`) and URL hash (`PathByUrlHashSqlTable`), path macros (`DownloadPathMacrosDecoder`).
   - Mark of the Web (`SetMarkOfTheWeb`), antivirus auto-launch (`QtKnownAntivirusesManager` + custom AV path/args), custom external command on completion (`LaunchExternalAppOnDownloadCompletion_*`), ZIP extraction, QuickLook / Finder / Share integration.

### B. Traffic Shaping, Scheduling, Reliability & Power
- **4 Traffic Usage Modes (`TrafficUsageMode`)**: `Snail` (one-click minimal throttle), `Low`, `Medium`, `High` — each tuning `MaxDownloadSpeed`, `MaxUploadSpeed`, `MaxConnections`, `MaxConnectionsPerServer`, `MaxDownloads`, `MaxURatio`, plus `TrafficLimitsCompliantMode` and `MaxAdditional*` slow-download boosts.
- **Scheduler (`AbstractDownloadsScheduler`)**: Day-of-week & time-window rules to auto-start/stop downloads (`ScheduleForNewDownloads`, `EnableSchedulerForNewDownloads`).
- **Auto-Retry & Slow Detection**: `AutoRetryFailedDownloads`, `DetectUnwantedDownloadErrors`, `DetectUnacceptablySlowDownloads` (`UnacceptablySlowSpeed`, `UnacceptablySlowSpeedInterval`).
- **Power & Network Guards**: Prevent OS auto-sleep while downloading (with options to ignore finished, ignore resumable, or include scheduled), battery minimum level threshold (`BatteryMinimumPowerLevelToRunDownloads`), disable post-finished tasks on battery, OS computer shutdown on completion, mobile/roaming network blocks (`AllowToUseMobileNetworks`, `AllowToUseRoamingNetworks`).
- **Tags, Filtering & Persistence**: Built-in & user tags (`DownloadsBuiltInTagsManager`, `DownloadsUserTagsManager`), filters (`Running`, `Finished`, `Downloading`, `Uploading`, `Stopped`, `NonFinished`, missing-files filter), SQLite `db.sqlite` persistence + automatic backup/verify/vacuum, JSON/list export & import, Portable Mode (`PortableVersionManager`), and FDM 5 migration (`importwizard`).

### C. Browser Integration (`wenativehost`) & Remote Control (`AppRc*`)
- **WebExtension Native Messaging (`wenativehost`)**:
  - Manifest `org.freedownloadmanager.fdm5.cnh.json` for `Google/Chrome/NativeMessagingHosts` and `Mozilla/NativeMessagingHosts` (Chrome IDs `ahmpjcflkgiildlgicmcieglgoilbfdp`, `mdfcjfioplkdchnhcpcobaheocanedjg`; Firefox IDs `fdm_ffext@freedownloadmanager.org`, `fdm_ffext2@freedownloadmanager.org`).
  - Launches `fdm -service` via `AppServerLauncher` if not running and communicates over local sockets (`fdm6bs` / `fdm6fs`) using `VhIpc`.
  - Observed JSON/IPC task types: `handshake`, `ui_strings`, `create_downloads`, `query_settings`, `post_settings`, `browser_proxy`, `video_sniffer`, `network_request_notification`, `browser_download_state_report`, `key_state`, `fdm_json_task`.
  - Filtering rules: intercept on/off, min file size (`WbDownloadsTooSmallSizeValue`), unwanted/wanted extension lists, unwanted host lists, hold-key bypass (`WbDontInterceptDownloadsIfKeyIsPressed`), forward aborted downloads back to browser.
- **Split Backend/Frontend & Remote Control (`AppRc*`)**:
  - `fdm` already separates core backend (`fdm6bs`) from UI frontend (`fdm6fs`) via `VhIpc`.
  - Remote Control Server/Client (`AppRcServerManager`, `AppRcClientManager`) supports SSL socket connections, password checking, and QR-code pairing (`fdm://remote_control/?id=...&password=...`) via relay `fdmrc.freedownloadmanager.org`.

---

## 3. Why a Rust Rewrite Can Be Strictly Better

Reverse engineering FDM 6.34.4 exposes clear architectural weaknesses in the shipped binary that a Rust implementation directly eliminates:

1. **Zero Telemetry, Ads, or Update Imitation**: FDM ships `AppAnalyticsModule`, `AppBackendTelemetryModule`, `AppDownloadsLostTelemetry`, `AppSettingsTelemetry`, `RemoteBannersManager`, `PluginsBannersManager`, `AppUpdateImitationManager`, and background phone-home endpoints (`up.freedownloadmanager.org/fdm6/...`). Our Rust version removes all telemetry, ad banners, and review/donation prompts by design.
2. **Massive Footprint & Dependency Reduction**: FDM bundles ~333 MB of Qt 6.10.3 frameworks, Boost, libcurl, OpenSSL, libtorrent, QuaZip, and downloads external Deno/Python binaries for site parsers. A Rust binary using `tokio` + `hyper`/`reqwest` + `rustls` + `rusqlite` + native async BitTorrent/HLS reduces footprint by an order of magnitude with memory safety.
3. **Unified Async Architecture vs. `VhIpc` Boilerplate**: FDM maintains dozens of duplicated `LocalUi*` / `RemoteUi*` / `VhIpc*Client` / `VhIpc*Server` C++ classes to bridge backend and frontend. In Rust, one typed async RPC/event bus (`tokio::sync::broadcast` + WebSocket/Unix-socket JSON-RPC) cleanly powers CLI, GUI, Web UI, `wenativehost`, and Remote Control simultaneously—without needing an external cloud relay (`fdmrc.freedownloadmanager.org`) unless optionally configured.
4. **Better Observability, Resumability & Plugin Sandboxing**: Replacing ad-hoc `curl`/Boost error state with deterministic section state machines, transactional SQLite WAL persistence, and capability-scoped JS/yt-dlp plugin execution makes paused/crashed transfers strictly recoverable.

---

## 4. Remaining Unknowns (Pending Runtime or Hopper Access)

- Exact binary wire-framing byte layout inside `QtAbstractSocketVhIpc` (would require Hopper decompilation of `QtAbstractSocketBasicIpc`/`VhIpc` or live socket capture).
- Exact SQLite table column names inside `db.sqlite` (schema builders are constructed dynamically across `*SqlTable` classes; can be read from an initialized DB or via Hopper).
- Exact `fdmrc.freedownloadmanager.org` relay handshake payload (not needed if our Rust Remote Control uses direct TLS/Tailscale/LAN WebSocket + optional self-hosted relay).
