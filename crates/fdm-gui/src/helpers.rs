use crate::theme::ThemePalette;
use fdm_cli::{AddProbeOutcome, DownloadEngine};
use fdm_core::{
    AutoStartPreventer, DownloadId, DownloadRecord, DownloadState, DownloadType,
    ExistingFileReaction, FdmConfig, FileType, HashAlgorithm, KnownWebBrowser, SortField,
    StateFilter, TrafficUsageMode, TrafficUsageProfile,
};
use fdm_ipc::RemotePairingInfo;
use fdm_native_host::{default_native_manifest_dir, install_native_manifest_to_dir};
use gpui::{Hsla, rgb};
use std::collections::HashSet;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub hidden: bool,
    pub portable: bool,
    pub data_dir: Option<PathBuf>,
    pub initial_url: Option<String>,
}

pub fn parse_launch_args(args: &[String]) -> LaunchOptions {
    let mut opts = LaunchOptions::default();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--hidden" | "-service" | "--service" => opts.hidden = true,
            "--portable" => opts.portable = true,
            "--data-dir" => {
                if let Some(next) = args.get(i + 1) {
                    opts.data_dir = Some(PathBuf::from(next));
                    i += 1;
                }
            }
            val if !val.starts_with('-') => {
                if opts.initial_url.is_none() {
                    opts.initial_url = Some(val.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    opts
}

pub fn format_bytes(bytes: u64) -> String {
    DownloadEngine::format_human_bytes(bytes)
}

pub fn format_speed(bps: u64) -> String {
    if bps == 0 {
        "0 B/s".into()
    } else {
        format!("{}/s", DownloadEngine::format_human_bytes(bps))
    }
}

pub fn format_eta(secs: Option<u64>) -> String {
    let Some(s) = secs else {
        return "--".into();
    };
    if s >= 86_400 {
        format!("{}d {}h", s / 86_400, (s % 86_400) / 3600)
    } else if s >= 3600 {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

pub fn format_kb_limit(bps: u64) -> String {
    if bps == 0 {
        "0".into()
    } else {
        (bps / 1024).max(1).to_string()
    }
}

pub fn parse_kb_limit(s: &str) -> u64 {
    let v = s.trim().parse::<u64>().unwrap_or(0);
    v.saturating_mul(1024)
}

pub fn format_minute_hhmm(mins: u16) -> String {
    let m = mins.min(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn parse_minute_hhmm(s: &str, fallback: u16) -> u16 {
    let trimmed = s.trim();
    if let Some((h, m)) = trimmed.split_once(':') {
        if let (Ok(hh), Ok(mm)) = (h.trim().parse::<u16>(), m.trim().parse::<u16>()) {
            return (hh.min(24) * 60 + mm.min(59)).min(1440);
        }
    }
    if let Ok(hh) = trimmed.parse::<u16>() {
        return (hh.min(24) * 60).min(1440);
    }
    fallback
}

pub fn parse_list_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| x.trim_start_matches('.').to_ascii_lowercase())
        .collect()
}

pub fn state_badge_info(rec: &DownloadRecord, p: &ThemePalette) -> (&'static str, Hsla) {
    if rec.flags.is_missing_files && rec.state == DownloadState::Completed {
        return ("Missing File", p.warning);
    }
    let color = match rec.state {
        DownloadState::Downloading => p.accent,
        DownloadState::Starting | DownloadState::Checking | DownloadState::PostProcessing | DownloadState::Moving => p.info,
        DownloadState::Seeding => p.success,
        DownloadState::Completed => p.success,
        DownloadState::Queued | DownloadState::WaitingForNetwork => p.info,
        DownloadState::Scheduled => p.warning,
        DownloadState::Paused | DownloadState::Stopped => p.text_muted,
        DownloadState::Error => p.danger,
    };
    (rec.state.display_name(), color)
}

pub fn state_filter_label(f: StateFilter) -> &'static str {
    match f {
        StateFilter::All => "All Downloads",
        StateFilter::Running => "Active",
        StateFilter::Downloading => "Downloading",
        StateFilter::Uploading => "Seeding",
        StateFilter::Finished => "Completed",
        StateFilter::Stopped => "Paused / Stopped",
        StateFilter::Scheduled => "Scheduled",
        StateFilter::Missing => "Missing Files",
    }
}

pub fn state_filter_short_label(f: StateFilter) -> &'static str {
    match f {
        StateFilter::All => "All files",
        StateFilter::Running => "Active",
        StateFilter::Downloading => "Downloading",
        StateFilter::Uploading => "Seeding",
        StateFilter::Finished => "Completed",
        StateFilter::Stopped => "Stopped",
        StateFilter::Scheduled => "Scheduled",
        StateFilter::Missing => "Missing",
    }
}

pub fn sort_field_label(f: SortField) -> &'static str {
    match f {
        SortField::CreationTime => "Date Added",
        SortField::Order => "Queue Order",
        SortField::Size => "Size",
        SortField::Status => "Status",
        SortField::Title => "Name",
        SortField::Progress => "Progress",
        SortField::Priority => "Priority",
    }
}

pub fn sort_field_short_label(f: SortField) -> &'static str {
    match f {
        SortField::CreationTime => "Added",
        SortField::Order => "Order",
        SortField::Size => "Size",
        SortField::Status => "Status",
        SortField::Title => "Name",
        SortField::Progress => "Progress",
        SortField::Priority => "Priority",
    }
}

pub fn download_icon_and_color(rec: &DownloadRecord) -> (&'static str, Hsla) {
    if rec.download_type == DownloadType::BitTorrent || rec.file_type == FileType::Torrent {
        return ("icons/type_torrent.svg", rgb(0x6366f1).into());
    }
    let resolved_type = match rec.file_type {
        FileType::Other | FileType::Unknown => infer_file_type_from_name(&rec.file_name),
        other => other,
    };
    match resolved_type {
        FileType::Video => ("icons/type_video.svg", rgb(0xe11d48).into()),
        FileType::Audio => ("icons/type_audio.svg", rgb(0x8b5cf6).into()),
        FileType::Archive => ("icons/type_archive.svg", rgb(0xd97706).into()),
        FileType::Document => ("icons/type_document.svg", rgb(0x0284c7).into()),
        FileType::Program => ("icons/type_program.svg", rgb(0x10b981).into()),
        FileType::Image => ("icons/type_image.svg", rgb(0x0d9488).into()),
        FileType::Torrent => ("icons/type_torrent.svg", rgb(0x6366f1).into()),
        FileType::Other | FileType::Unknown => ("icons/type_file.svg", rgb(0x3f80e0).into()),
    }
}

fn infer_file_type_from_name(name: &str) -> FileType {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "flv" => FileType::Video,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "opus" => FileType::Audio,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" => FileType::Archive,
        "pdf" | "doc" | "docx" | "txt" | "md" | "epub" | "csv" | "xlsx" => FileType::Document,
        "dmg" | "pkg" | "exe" | "msi" | "appimage" | "deb" | "rpm" | "iso" => FileType::Program,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => FileType::Image,
        "torrent" => FileType::Torrent,
        _ => FileType::Other,
    }
}

pub fn format_added_timestamp(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    if local.date_naive() == today {
        local.format("Today %H:%M").to_string()
    } else {
        local.format("%b %d %H:%M").to_string()
    }
}

pub fn extract_domain_label(raw_url: &str) -> String {
    if raw_url.starts_with("magnet:") {
        return "magnet".into();
    }
    if let Ok(parsed) = url::Url::parse(raw_url) {
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
        return parsed.scheme().to_string();
    }
    "local".into()
}

pub fn preventer_label(p: AutoStartPreventer) -> Option<&'static str> {
    match p {
        AutoStartPreventer::None => None,
        AutoStartPreventer::NotOnline => Some("Offline"),
        AutoStartPreventer::LowBattery => Some("Low Battery Guard"),
        AutoStartPreventer::NetworkRoaming => Some("Roaming Guard"),
        AutoStartPreventer::NoAllowedNetworkInterface => Some("Network Restricted"),
        AutoStartPreventer::LowDiskSpace => Some("Low Disk Space"),
    }
}

pub fn zoom_rem_size_px(overall_zoom_pct: u16) -> f32 {
    16.0 * (overall_zoom_pct as f32 / 100.0).clamp(0.75, 1.50)
}

pub fn convert_wb_threshold_input(raw_text: &str, to_mb: bool) -> String {
    let val = raw_text.trim().parse::<u64>().unwrap_or(0);
    if val == 0 {
        return "0".into();
    }
    if to_mb {
        (val / 1024).max(1).to_string()
    } else {
        val.saturating_mul(1024).to_string()
    }
}

pub fn parse_wb_threshold_bytes(raw_text: &str, unit_mb: bool) -> u64 {
    let raw_val = raw_text.trim().parse::<u64>().unwrap_or(0);
    if unit_mb {
        raw_val.saturating_mul(1024 * 1024)
    } else {
        raw_val.saturating_mul(1024)
    }
}

pub fn extract_first_download_url(clipboard: &str) -> Option<String> {
    for line in clipboard.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("ftp://")
            || lower.starts_with("magnet:")
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn compact_speed_label(bps: u64) -> String {
    if bps >= 1_073_741_824 {
        format!("{:.1}G", bps as f64 / 1_073_741_824.0)
    } else if bps >= 1_048_576 {
        format!("{:.1}M", bps as f64 / 1_048_576.0)
    } else if bps >= 1024 {
        format!("{}K", (bps / 1024).max(1))
    } else if bps > 0 {
        format!("{bps}B")
    } else {
        "0".into()
    }
}

pub fn format_compact_dock_badge(
    total_dl_bps: u64,
    total_ul_bps: u64,
    active_count: usize,
    show_upload_speed: bool,
    indicate_dl_ul: bool,
) -> Option<String> {
    let show_dl = indicate_dl_ul && total_dl_bps > 0;
    let show_ul = show_upload_speed && total_ul_bps > 0;
    match (show_dl, show_ul) {
        (true, true) => Some(format!(
            "↓{} ↑{}",
            compact_speed_label(total_dl_bps),
            compact_speed_label(total_ul_bps)
        )),
        (true, false) => Some(format!("↓{}", compact_speed_label(total_dl_bps))),
        (false, true) => Some(format!("↑{}", compact_speed_label(total_ul_bps))),
        (false, false) => {
            if indicate_dl_ul && active_count > 0 {
                Some(active_count.to_string())
            } else {
                None
            }
        }
    }
}

pub fn portable_marker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    Some(parent.join("ferrite-portable.flag"))
}

pub fn is_portable_active(cli_portable: bool) -> bool {
    if cli_portable {
        return true;
    }
    portable_marker_path().is_some_and(|p| p.exists())
}

pub fn sync_portable_marker(enabled: bool) -> anyhow::Result<()> {
    if let Some(flag) = portable_marker_path() {
        if enabled {
            if let Some(parent) = flag.parent() {
                std::fs::create_dir_all(parent.join("ferrite-data"))?;
            }
            std::fs::write(flag, b"1")?;
        } else if flag.exists() {
            std::fs::remove_file(flag)?;
        }
    }
    Ok(())
}

#[allow(deprecated, unexpected_cfgs)]
pub fn update_macos_dock_badge(label: Option<&str>) {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use cocoa::base::{id, nil};
        use cocoa::foundation::{NSAutoreleasePool, NSString};
        use objc::{msg_send, sel, sel_impl};

        let pool = NSAutoreleasePool::new(nil);
        let app = NSApp();
        if app != nil {
            let dock_tile: id = msg_send![app, dockTile];
            if dock_tile != nil {
                match label {
                    Some(txt) if !txt.is_empty() => {
                        let ns_label = NSString::alloc(nil).init_str(txt).autorelease();
                        let _: () = msg_send![dock_tile, setBadgeLabel: ns_label];
                    }
                    _ => {
                        let _: () = msg_send![dock_tile, setBadgeLabel: nil];
                    }
                }
            }
        }
        pool.drain();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = label;
    }
}

#[allow(deprecated, unexpected_cfgs)]
pub fn set_macos_hide_dock_icon(hide: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::{NSApp, NSApplicationActivationPolicy};
        use cocoa::base::nil;
        use objc::{msg_send, sel, sel_impl};

        let app = NSApp();
        if app != nil {
            let policy = if hide {
                NSApplicationActivationPolicy::NSApplicationActivationPolicyAccessory
            } else {
                NSApplicationActivationPolicy::NSApplicationActivationPolicyRegular
            };
            let _: bool = msg_send![app, setActivationPolicy: policy];
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = hide;
    }
}

#[allow(deprecated, unexpected_cfgs)]
pub fn macos_restore_windows_and_activate() {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use cocoa::base::{YES, id, nil};
        use objc::{msg_send, sel, sel_impl};

        let app = NSApp();
        if app != nil {
            let _: () = msg_send![app, unhide: nil];
            let windows: id = msg_send![app, windows];
            if windows != nil {
                let count: usize = msg_send![windows, count];
                for i in 0..count {
                    let win: id = msg_send![windows, objectAtIndex: i];
                    if win != nil {
                        let is_mini: bool = msg_send![win, isMiniaturized];
                        if is_mini {
                            let _: () = msg_send![win, deminiaturize: nil];
                        }
                        let _: () = msg_send![win, makeKeyAndOrderFront: nil];
                    }
                }
            }
            let _: () = msg_send![app, activateIgnoringOtherApps: YES];
        }
    }
}

#[allow(deprecated, unexpected_cfgs)]
pub fn macos_hide_all_windows() {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use cocoa::base::{id, nil};
        use objc::{msg_send, sel, sel_impl};

        let app = NSApp();
        if app != nil {
            let windows: id = msg_send![app, windows];
            if windows != nil {
                let count: usize = msg_send![windows, count];
                for i in 0..count {
                    let win: id = msg_send![windows, objectAtIndex: i];
                    if win != nil {
                        let _: () = msg_send![win, orderOut: nil];
                    }
                }
            }
        }
    }
}

#[allow(deprecated, unexpected_cfgs)]
pub fn macos_unhide_all_applications() {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use cocoa::base::nil;
        use objc::{msg_send, sel, sel_impl};

        let app = NSApp();
        if app != nil {
            let _: () = msg_send![app, unhideAllApplications: nil];
        }
    }
}

#[allow(deprecated, unexpected_cfgs)]
pub fn macos_bring_all_to_front() {
    #[cfg(target_os = "macos")]
    unsafe {
        use cocoa::appkit::NSApp;
        use cocoa::base::{YES, id, nil};
        use objc::{msg_send, sel, sel_impl};

        let app = NSApp();
        if app != nil {
            let _: () = msg_send![app, unhide: nil];
            let windows: id = msg_send![app, windows];
            let mut brought_any = false;
            if windows != nil {
                let count: usize = msg_send![windows, count];
                for i in (0..count).rev() {
                    let win: id = msg_send![windows, objectAtIndex: i];
                    if win != nil {
                        let visible: bool = msg_send![win, isVisible];
                        if visible {
                            let _: () = msg_send![win, orderFrontRegardless];
                            brought_any = true;
                        }
                    }
                }
            }
            if !brought_any {
                macos_restore_windows_and_activate();
            } else {
                let _: () = msg_send![app, activateIgnoringOtherApps: YES];
            }
        }
    }
}

pub fn count_by_state_filter(all: &[DownloadRecord], filter: StateFilter) -> usize {
    all.iter()
        .filter(|d| match filter {
            StateFilter::All => true,
            StateFilter::Running => d.state.is_running_or_seeding() || d.state == DownloadState::Queued,
            StateFilter::Downloading => matches!(
                d.state,
                DownloadState::Starting
                    | DownloadState::Downloading
                    | DownloadState::Checking
                    | DownloadState::Queued
                    | DownloadState::PostProcessing
            ),
            StateFilter::Uploading => d.state == DownloadState::Seeding,
            StateFilter::Finished => d.state == DownloadState::Completed,
            StateFilter::Stopped => matches!(
                d.state,
                DownloadState::Paused | DownloadState::Stopped | DownloadState::Error
            ),
            StateFilter::Scheduled => d.state == DownloadState::Scheduled,
            StateFilter::Missing => d.flags.is_missing_files,
        })
        .count()
}

pub fn detect_lan_ip() -> String {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local) = sock.local_addr() {
                let ip = local.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    return ip.to_string();
                }
            }
        }
    }
    "127.0.0.1".into()
}

pub fn open_path_os(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    anyhow::bail!("OS open is not implemented on this platform");

    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Failed to open {}", path.display());
    }
    Ok(())
}

pub fn reveal_path_os(path: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        if path.exists() {
            cmd.arg("-R").arg(path);
        } else if let Some(parent) = path.parent() {
            cmd.arg(parent);
        } else {
            cmd.arg(path);
        }
        let status = cmd.status()?;
        if !status.success() {
            anyhow::bail!("Failed to reveal {}", path.display());
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let target = path.parent().unwrap_or(path);
        open_path_os(target)
    }
}

pub fn find_native_host_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name("ferrite-native-host");
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("ferrite-native-host")
}

pub fn install_native_host_for_browser(browser: KnownWebBrowser) -> anyhow::Result<PathBuf> {
    let dir = default_native_manifest_dir(browser)
        .ok_or_else(|| anyhow::anyhow!("Native Messaging host not supported for {:?}", browser))?;
    let host_bin = find_native_host_binary();
    install_native_manifest_to_dir(browser, &host_bin, &dir)
}

pub fn is_native_host_installed(browser: KnownWebBrowser) -> bool {
    default_native_manifest_dir(browser)
        .map(|d| d.join(format!("{}.json", fdm_native_host::NATIVE_HOST_NAME)).is_file())
        .unwrap_or(false)
}

pub fn browser_display_name(browser: KnownWebBrowser) -> &'static str {
    match browser {
        KnownWebBrowser::Chrome => "Chrome",
        KnownWebBrowser::Firefox => "Firefox",
        KnownWebBrowser::Brave => "Brave",
        KnownWebBrowser::Edge => "Edge",
        KnownWebBrowser::Vivaldi => "Vivaldi",
        KnownWebBrowser::Opera => "Opera",
        KnownWebBrowser::Safari => "Safari",
    }
}

pub fn reaction_label(r: ExistingFileReaction) -> &'static str {
    match r {
        ExistingFileReaction::Ask => "Ask Me",
        ExistingFileReaction::Overwrite => "Overwrite",
        ExistingFileReaction::Rename => "Auto Rename",
        ExistingFileReaction::Skip => "Skip",
    }
}

#[derive(Debug, Clone)]
pub enum UiAsyncMsg {
    Toast {
        text: String,
        is_error: bool,
    },
    ProbeCompleted {
        outcome: Box<AddProbeOutcome>,
    },
    ProbeFailed {
        error: String,
    },
    EnqueueFinished {
        count: usize,
        label: String,
    },
    FileConflict {
        id: DownloadId,
        path: PathBuf,
    },
    HashResult {
        download_id: DownloadId,
        algo: HashAlgorithm,
        digest: String,
        matched: Option<bool>,
    },
    ConvertFinished {
        out_path: PathBuf,
    },
    RemoteControlReady {
        pairing: RemotePairingInfo,
    },
    RemoteControlFailed {
        error: String,
    },
    RemotePasswordRegenerated {
        password: String,
    },
    PortableCreated {
        dir: PathBuf,
    },
    DiagnosticsArchived {
        zip_path: PathBuf,
    },
    PluginActionFinished {
        message: String,
        is_error: bool,
    },
    PluginUpdatesFinished {
        updated: Vec<fdm_media::PluginUpdateOutcome>,
        manual: bool,
    },
    AppUpdateCheckFinished {
        result: fdm_cli::AppUpdateCheckResult,
        manual: bool,
    },
    VirusCheckFinished {
        download_id: DownloadId,
        clean: bool,
        summary: String,
    },
    UpdateCheckFinished {
        download_id: DownloadId,
        changed: bool,
        message: String,
    },
    DownloadAddedNotification {
        id: DownloadId,
    },
    DownloadCompletedNotification {
        id: DownloadId,
    },
    DownloadFailedNotification {
        id: DownloadId,
        error: String,
    },
    DefaultTorrentCheckResult {
        is_default: bool,
    },
    QuitRequested,
    RaiseRequested,
}

pub fn available_disk_space_bytes(path: &Path) -> Option<u64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            probe = PathBuf::from("/");
            break;
        }
    }
    let out = Command::new("df").arg("-Pk").arg(&probe).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let data_line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())?;
    let tokens: Vec<&str> = data_line.split_whitespace().collect();
    let pct_idx = tokens.iter().position(|t| t.ends_with('%'))?;
    if pct_idx == 0 {
        return None;
    }
    let avail_kb = tokens[pct_idx - 1].parse::<u64>().ok()?;
    Some(avail_kb.saturating_mul(1024))
}

pub fn format_ratio(downloaded: u64, uploaded: u64) -> String {
    if downloaded == 0 {
        if uploaded == 0 {
            "0.00".into()
        } else {
            "∞".into()
        }
    } else {
        format!("{:.2}", uploaded as f64 / downloaded as f64)
    }
}

pub fn expand_path_preview(template: &str, url: &str, filename: &str) -> String {
    let u = (!url.trim().is_empty()).then_some(url.trim());
    let f = (!filename.trim().is_empty()).then_some(filename.trim());
    fdm_core::routing::expand_path_macros(template, u, f)
}

pub fn detect_antivirus_presets() -> Vec<(&'static str, &'static str, &'static str, &'static str, bool)> {
    let candidates = [
        ("clamav_brew", "ClamAV (clamscan)", "/opt/homebrew/bin/clamscan", "\"{file}\""),
        ("clamav_local", "ClamAV (clamscan /usr/local)", "/usr/local/bin/clamscan", "\"{file}\""),
        ("clamav_usr", "ClamAV (/usr/bin/clamscan)", "/usr/bin/clamscan", "\"{file}\""),
        ("clamd_brew", "ClamAV Daemon (clamdscan)", "/opt/homebrew/bin/clamdscan", "\"{file}\""),
    ];
    candidates
        .into_iter()
        .map(|(uid, name, bin, args)| (uid, name, bin, args, Path::new(bin).is_file()))
        .collect()
}

pub fn format_datetime_full(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[derive(Clone, Default)]
pub struct UiMsgQueue {
    inner: Arc<Mutex<Vec<UiAsyncMsg>>>,
}

impl UiMsgQueue {
    pub fn push(&self, msg: UiAsyncMsg) {
        self.inner.lock().unwrap().push(msg);
    }

    pub fn has_pending(&self) -> bool {
        !self.inner.lock().unwrap().is_empty()
    }

    pub fn drain(&self) -> Vec<UiAsyncMsg> {
        let mut g = self.inner.lock().unwrap();
        std::mem::take(&mut *g)
    }
}

#[allow(dead_code)]
pub fn default_profile_for(cfg: &FdmConfig, tum: TrafficUsageMode) -> TrafficUsageProfile {
    cfg.core
        .tum_profiles
        .get(&tum)
        .cloned()
        .unwrap_or_else(|| TrafficUsageProfile::defaults_map().remove(&tum).unwrap())
}

#[allow(dead_code)]
pub fn selected_indices_from_record(rec: &DownloadRecord) -> HashSet<usize> {
    rec.files
        .iter()
        .filter(|f| f.priority != fdm_core::DownloadPriority::DontDownload)
        .map(|f| f.index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fdm_core::NewDownloadRequest;

    #[test]
    fn parses_launch_flags_and_optional_url() {
        let args = vec![
            "fdm-gui".into(),
            "--portable".into(),
            "--data-dir".into(),
            "/tmp/custom-ferrite".into(),
            "--hidden".into(),
            "https://example.com/archive.tar.gz".into(),
        ];
        let opts = parse_launch_args(&args);
        assert!(opts.portable);
        assert!(opts.hidden);
        assert_eq!(opts.data_dir, Some(PathBuf::from("/tmp/custom-ferrite")));
        assert_eq!(
            opts.initial_url.as_deref(),
            Some("https://example.com/archive.tar.gz")
        );
    }

    #[test]
    fn formats_units_speeds_and_etas_accurately() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(2 * 1024 * 1024), "2.00 MB/s");
        assert_eq!(format_eta(None), "--");
        assert_eq!(format_eta(Some(45)), "45s");
        assert_eq!(format_eta(Some(125)), "2m 5s");
        assert_eq!(format_eta(Some(3725)), "1h 2m");
        assert_eq!(format_minute_hhmm(90), "01:30");
        assert_eq!(parse_minute_hhmm("23:45", 0), 23 * 60 + 45);
        assert_eq!(parse_kb_limit("256"), 256 * 1024);
        assert_eq!(format_kb_limit(512 * 1024), "512");
    }

    #[test]
    fn counts_state_filters_and_parses_extension_lists() {
        let req1 = NewDownloadRequest::from_url("https://example.com/a.zip");
        assert_eq!(req1.url, "https://example.com/a.zip");
        assert_eq!(
            parse_list_tokens(".tmp, .pls  m3u8"),
            vec!["tmp", "pls", "m3u8"]
        );

        let base = fdm_core::DownloadRecord {
            id: 1,
            uuid: "u1".into(),
            parent_id: None,
            children_ids: vec![],
            order_index: 0,
            download_type: fdm_core::DownloadType::Www,
            state: DownloadState::Downloading,
            priority: fdm_core::DownloadPriority::Normal,
            resume_support: fdm_core::ResumeSupport::Present,
            flags: Default::default(),
            title: "a.zip".into(),
            url: "https://example.com/a.zip".into(),
            target_dir: PathBuf::from("/tmp"),
            file_name: "a.zip".into(),
            partial_suffix: ".fdmdownload".into(),
            total_bytes: Some(100),
            downloaded_bytes: 50,
            uploaded_bytes: 0,
            download_speed_bps: 10,
            upload_speed_bps: 0,
            eta_secs: Some(5),
            connections_active: 2,
            etag: None,
            last_modified: None,
            file_type: fdm_core::FileType::Archive,
            http: Default::default(),
            torrent_info_hash: None,
            torrent_source: None,
            tags: vec![],
            files: vec![],
            segments: vec![],
            peers: vec![],
            trackers: vec![],
            schedule: None,
            retry_count: 0,
            forbid_auto_retry_count: 0,
            last_error: None,
            abort_reason: fdm_core::DownloadAbortReason::None,
            preview_path: None,
            module_data: serde_json::Value::Null,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };
        let mut completed = base.clone();
        completed.id = 2;
        completed.state = DownloadState::Completed;
        completed.flags.is_missing_files = true;
        let all = vec![base, completed];

        assert_eq!(count_by_state_filter(&all, StateFilter::All), 2);
        assert_eq!(count_by_state_filter(&all, StateFilter::Downloading), 1);
        assert_eq!(count_by_state_filter(&all, StateFilter::Finished), 1);
        assert_eq!(count_by_state_filter(&all, StateFilter::Missing), 1);
    }

    #[test]
    fn formats_ratios_expands_macros_and_reads_disk_space() {
        assert_eq!(format_ratio(0, 0), "0.00");
        assert_eq!(format_ratio(0, 1024), "∞");
        assert_eq!(format_ratio(1000, 1500), "1.50");

        let expanded = expand_path_preview(
            "/Users/demo/Downloads/%host%/%type%",
            "https://cdn.example.com/files/ubuntu.iso",
            "ubuntu.iso",
        );
        assert!(expanded.contains("/cdn.example.com/"));
        assert!(expanded.contains("Program"));

        let free = available_disk_space_bytes(Path::new("/tmp"));
        assert!(free.unwrap_or(0) > 0);
    }

    #[test]
    fn converts_wb_threshold_units_and_formats_dock_badge() {
        assert_eq!(convert_wb_threshold_input("1024", true), "1");
        assert_eq!(convert_wb_threshold_input("512", true), "1");
        assert_eq!(convert_wb_threshold_input("2", false), "2048");
        assert_eq!(convert_wb_threshold_input("0", true), "0");
        assert_eq!(parse_wb_threshold_bytes("2", true), 2 * 1024 * 1024);
        assert_eq!(parse_wb_threshold_bytes("100", false), 100 * 1024);

        assert_eq!(zoom_rem_size_px(100), 16.0);
        assert_eq!(zoom_rem_size_px(125), 20.0);

        assert_eq!(
            format_compact_dock_badge(2 * 1024 * 1024, 512 * 1024, 1, true, true).as_deref(),
            Some("↓2.0M ↑512K")
        );
        assert_eq!(
            format_compact_dock_badge(2 * 1024 * 1024, 512 * 1024, 1, false, true).as_deref(),
            Some("↓2.0M")
        );
        assert_eq!(
            format_compact_dock_badge(0, 0, 0, true, true),
            None
        );

        assert_eq!(
            extract_first_download_url("hello\n  https://example.com/file.zip  \n"),
            Some("https://example.com/file.zip".to_string())
        );
        assert_eq!(
            extract_first_download_url("magnet:?xt=urn:btih:0123456789abcdef"),
            Some("magnet:?xt=urn:btih:0123456789abcdef".to_string())
        );
        assert_eq!(extract_first_download_url("just some plain text"), None);
    }
}
