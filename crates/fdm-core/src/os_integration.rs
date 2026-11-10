use crate::model::{DownloadRecord, ShutdownType};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiskSpaceStats {
    pub available_bytes: u64,
    pub total_bytes: u64,
    pub free_percent: f32,
}

/// Inspect available and total disk space for `path` (walking up to the nearest existing ancestor).
pub fn disk_space_stats(path: &Path) -> Option<DiskSpaceStats> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            probe = PathBuf::from("/");
            break;
        }
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(probe.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut st) };
        if rc != 0 {
            return None;
        }
        let frsize = if st.f_frsize > 0 {
            st.f_frsize as u64
        } else {
            st.f_bsize as u64
        };
        let available_bytes = (st.f_bavail as u64).saturating_mul(frsize);
        let total_bytes = (st.f_blocks as u64).saturating_mul(frsize);
        let free_percent = if total_bytes > 0 {
            (available_bytes as f64 / total_bytes as f64 * 100.0) as f32
        } else {
            100.0
        };
        Some(DiskSpaceStats {
            available_bytes,
            total_bytes,
            free_percent,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = probe;
        None
    }
}

/// Returns `true` when free space meets both `min_bytes` and `min_percent` thresholds.
pub fn has_sufficient_disk_space(path: &Path, min_bytes: u64, min_percent: f32) -> bool {
    if min_bytes == 0 && min_percent <= 0.0 {
        return true;
    }
    let Some(stats) = disk_space_stats(path) else {
        return true;
    };
    let bytes_ok = min_bytes == 0 || stats.available_bytes >= min_bytes;
    let pct_ok = min_percent <= 0.0 || stats.free_percent >= min_percent;
    bytes_ok && pct_ok
}

/// Cached OS-native browser-style User-Agent string when `use_system_default_user_agent` is enabled.
pub fn system_default_user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let os_ver = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().replace('.', "_"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "15_0".into());
            format!(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X {}) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
                os_ver
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            format!(
                "Mozilla/5.0 (X11; {}; x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
                std::env::consts::OS
            )
        }
    })
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn autorun_plist_content(exe_path: &Path, minimized: bool) -> String {
    let exe_escaped = xml_escape(&exe_path.to_string_lossy());
    let extra_arg = if minimized {
        "\n        <string>--hidden</string>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>org.ferritedm.FerriteDM</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_escaped}</string>{extra_arg}
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"#
    )
}

pub fn sync_autorun_plist_in_dir(
    launch_agents_dir: &Path,
    exe_path: &Path,
    enabled: bool,
    minimized: bool,
) -> Result<()> {
    let plist_path = launch_agents_dir.join("org.ferritedm.FerriteDM.plist");
    if enabled {
        std::fs::create_dir_all(launch_agents_dir)?;
        let content = autorun_plist_content(exe_path, minimized);
        std::fs::write(plist_path, content)?;
    } else if plist_path.exists() {
        std::fs::remove_file(plist_path)?;
    }
    Ok(())
}

/// Sync login autorun item (macOS LaunchAgent plist; no-op on unsupported targets).
pub fn sync_autorun_on_login(enabled: bool, minimized: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) else {
            return Ok(());
        };
        let launch_agents = home.join("Library").join("LaunchAgents");
        let exe = std::env::current_exe()?;
        sync_autorun_plist_in_dir(&launch_agents, &exe, enabled, minimized)?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (enabled, minimized);
    }
    Ok(())
}

/// Check whether Ferrite DM is currently registered as the default handler for `magnet:` links or `.torrent` files.
pub async fn is_default_torrent_client() -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) else {
            return false;
        };
        let plist = home
            .join("Library")
            .join("Preferences")
            .join("com.apple.LaunchServices")
            .join("com.apple.launchservices.secure.plist");
        if !plist.exists() {
            return false;
        }
        if let Ok(out) = Command::new("plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&plist)
            .output()
            .await
        {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(handlers) = val.get("LSHandlers").and_then(|v| v.as_array()) {
                    for entry in handlers {
                        let scheme = entry
                            .get("LSHandlerURLScheme")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tag = entry
                            .get("LSHandlerContentTag")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let role = entry
                            .get("LSHandlerRoleAll")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if (scheme.eq_ignore_ascii_case("magnet")
                            || tag.eq_ignore_ascii_case("torrent"))
                            && role.to_ascii_lowercase().contains("ferritedm")
                        {
                            return true;
                        }
                    }
                }
            }
        }
        return false;
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Returns `(is_metered_or_cellular, is_roaming)` cached for 5 seconds to avoid spawning processes on every scheduler tick.
pub async fn detect_network_restrictions() -> (bool, bool) {
    static CACHE: OnceLock<Mutex<Option<(Instant, bool, bool)>>> = OnceLock::new();
    let mutex = CACHE.get_or_init(|| Mutex::new(None));
    {
        let g = mutex.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, cellular, roaming)) = *g {
            if at.elapsed() < Duration::from_secs(5) {
                return (cellular, roaming);
            }
        }
    }

    let mut cellular = false;
    let mut roaming = false;
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("route")
            .args(["-n", "get", "default"])
            .output()
            .await
        {
            let txt = String::from_utf8_lossy(&out.stdout);
            for line in txt.lines() {
                if let Some(iface) = line.trim().strip_prefix("interface:") {
                    let iface = iface.trim();
                    if iface.starts_with("pdp_ip") {
                        cellular = true;
                        break;
                    }
                    if let Ok(services) = Command::new("networksetup")
                        .arg("-listnetworkserviceorder")
                        .output()
                        .await
                    {
                        let svc_txt = String::from_utf8_lossy(&services.stdout).to_ascii_lowercase();
                        if svc_txt.contains(&format!("device: {iface})"))
                            && (svc_txt.contains("cellular")
                                || svc_txt.contains("iphone")
                                || svc_txt.contains("hotspot")
                                || svc_txt.contains("modem"))
                        {
                            cellular = true;
                        }
                    }
                }
            }
        }
        if cellular {
            if let Ok(out) = Command::new("scutil").arg("--nwi").output().await {
                let txt = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
                if txt.contains("roaming") {
                    roaming = true;
                }
            }
        }
    }

    {
        let mut g = mutex.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some((Instant::now(), cellular, roaming));
    }
    (cellular, roaming)
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
        .replace('\r', " ")
}

/// Trigger a non-blocking OS desktop notification.
pub fn send_os_notification(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"Ferrite DM\" subtitle \"{}\"",
            applescript_escape(body),
            applescript_escape(title)
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, body);
    }
}

/// Trigger a non-blocking OS notification sound using the custom sound file or macOS default alert sound.
pub fn play_notification_sound(custom_path: Option<&Path>) {
    #[cfg(target_os = "macos")]
    {
        let chosen = custom_path
            .filter(|p| p.exists())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/System/Library/Sounds/Glass.aiff"));
        if chosen.exists() {
            let _ = std::process::Command::new("afplay")
                .arg(&chosen)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = custom_path;
    }
}

/// Apply macOS Mark-of-the-Web (`com.apple.quarantine`) extended attribute on completed files.
pub fn set_mark_of_the_web(path: &Path, source_url: &str, app_name: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let guid = uuid::Uuid::new_v4().to_string().to_uppercase();
        let val = format!("0083;{:08x};{};{}", ts, app_name, guid);
        xattr::set(path, "com.apple.quarantine", val.as_bytes())?;
        let _ = xattr::set(
            path,
            "com.apple.metadata:kMDItemWhereFroms",
            source_url.as_bytes(),
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, source_url, app_name);
    }
    Ok(())
}

/// Check macOS battery state: returns `(on_battery, battery_percentage)`.
pub async fn check_power_status() -> (bool, u8) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("pmset").args(["-g", "batt"]).output().await {
            let txt = String::from_utf8_lossy(&out.stdout);
            let on_battery = txt.contains("Battery Power");
            let mut pct = 100u8;
            for tok in txt.split_whitespace() {
                if let Some(num) = tok.strip_suffix("%;") {
                    if let Ok(v) = num.parse::<u8>() {
                        pct = v;
                        break;
                    }
                } else if let Some(num) = tok.strip_suffix('%') {
                    if let Ok(v) = num.parse::<u8>() {
                        pct = v;
                        break;
                    }
                }
            }
            return (on_battery, pct);
        }
    }
    (false, 100)
}

/// Holds a macOS `caffeinate` child process while active downloads prevent auto-sleep.
#[derive(Debug, Default)]
pub struct SleepPreventer {
    child: Option<std::process::Child>,
}

impl SleepPreventer {
    pub fn update(&mut self, should_prevent: bool) {
        if should_prevent {
            if let Some(ref mut c) = self.child {
                if let Ok(None) = c.try_wait() {
                    return;
                }
            }
            #[cfg(target_os = "macos")]
            {
                let pid = std::process::id().to_string();
                if let Ok(child) = std::process::Command::new("caffeinate")
                    .args(["-i", "-d", "-w", &pid])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    self.child = Some(child);
                }
            }
        } else if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for SleepPreventer {
    fn drop(&mut self) {
        self.update(false);
    }
}

fn split_template_args(template: &str, record: &DownloadRecord) -> Vec<String> {
    let file_str = record.output_path().to_string_lossy().to_string();
    let dir_str = record.target_dir.to_string_lossy().to_string();
    let replaced = template
        .replace("{file}", &file_str)
        .replace("%path%", &file_str)
        .replace("{path}", &file_str)
        .replace("%1", &file_str)
        .replace("{dir}", &dir_str)
        .replace("{url}", &record.url)
        .replace("{title}", &record.title);

    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';
    for ch in replaced.chars() {
        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
            } else {
                cur.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
        } else if ch.is_whitespace() {
            if !cur.is_empty() {
                args.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

pub async fn run_external_completion_app(
    app_path: &Path,
    args_template: &str,
    record: &DownloadRecord,
) -> Result<()> {
    let args = split_template_args(args_template, record);
    let mut cmd = Command::new(app_path);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "External command exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub async fn scan_file_with_antivirus(
    av_path: &Path,
    args_template: &str,
    record: &DownloadRecord,
) -> Result<()> {
    run_external_completion_app(av_path, args_template, record).await
}

pub async fn perform_shutdown_action(action: ShutdownType) -> Result<()> {
    match action {
        ShutdownType::None | ShutdownType::QuitApp => Ok(()),
        #[cfg(target_os = "macos")]
        ShutdownType::Sleep => {
            Command::new("pmset").arg("sleepnow").status().await?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        ShutdownType::Hibernate => {
            Command::new("pmset").arg("sleepnow").status().await?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        ShutdownType::Shutdown => {
            Command::new("osascript")
                .args(["-e", "tell application \"System Events\" to shut down"])
                .status()
                .await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_space_stats_checks_thresholds() {
        let cwd = std::env::current_dir().unwrap();
        let stats = disk_space_stats(&cwd).expect("statvfs should succeed on workspace dir");
        assert!(stats.total_bytes > 0);
        assert!(has_sufficient_disk_space(&cwd, 1, 0.01));
        assert!(!has_sufficient_disk_space(&cwd, u64::MAX, 0.0));
        assert!(!has_sufficient_disk_space(&cwd, 0, 100.1));
    }

    #[test]
    fn autorun_plist_sync_writes_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("ferrite-autorun-{}", uuid::Uuid::new_v4()));
        let exe = PathBuf::from("/Applications/Ferrite DM.app/Contents/MacOS/fdm-gui");
        sync_autorun_plist_in_dir(&dir, &exe, true, true).unwrap();
        let plist = dir.join("org.ferritedm.FerriteDM.plist");
        let content = std::fs::read_to_string(&plist).unwrap();
        assert!(content.contains("org.ferritedm.FerriteDM"));
        assert!(content.contains("--hidden"));
        assert!(content.contains("<true/>"));

        sync_autorun_plist_in_dir(&dir, &exe, false, true).unwrap();
        assert!(!plist.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_default_user_agent_is_non_empty() {
        let ua = system_default_user_agent();
        assert!(ua.contains("Mozilla/5.0"));
    }
}
