use anyhow::{Context, Result, bail};
use fdm_core::{
    DownloadType, FdmConfig, HttpRequestOptions, KnownWebBrowser, NewDownloadRequest,
};
use fdm_ipc::{IpcClient, IpcRequest, IpcResponse, default_socket_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};
use url::Url;

pub const NATIVE_HOST_NAME: &str = "org.freedownloadmanager.fdm5.cnh";

pub const CHROME_ALLOWED_ORIGINS: &[&str] = &[
    "chrome-extension://ahmpjcflkgiildlgicmcieglgoilbfdp/",
    "chrome-extension://mdfcjfioplkdchnhcpcobaheocanedjg/",
];

pub const FIREFOX_ALLOWED_EXTENSIONS: &[&str] = &[
    "fdm_ffext@freedownloadmanager.org",
    "fdm_ffext2@freedownloadmanager.org",
];

pub const VIDEO_SNIFFER_MIME_TYPES: &[&str] = &[
    "video/x-flv",
    "video/mp4",
    "video/x-m4v",
    "video/3gpp",
    "video/quicktime",
    "video/mpeg",
    "video/webm",
    "video/mp2t",
    "application/mpegurl",
    "application/x-mpegurl",
    "audio/mpegurl",
    "audio/x-mpegurl",
    "application/vnd.apple.mpegurl",
    "application/vnd.apple.mpegurl.audio",
];

const MAX_NATIVE_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[cfg(target_os = "macos")]
const CG_EVENT_SOURCE_STATE_HID_SYSTEM: u32 = 1;
const CG_FLAG_SHIFT: u64 = 0x0002_0000;
const CG_FLAG_CONTROL: u64 = 0x0004_0000;
const CG_FLAG_ALTERNATE: u64 = 0x0008_0000;
const CG_FLAG_COMMAND: u64 = 0x0010_0000;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state_id: u32) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModifierSpec {
    Shift,
    Control,
    Alt,
    Command,
}

fn parse_modifier_spec_str(raw: &str) -> Option<ModifierSpec> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "shift" => Some(ModifierSpec::Shift),
        "ctrl" | "control" => Some(ModifierSpec::Control),
        "alt" | "option" | "opt" => Some(ModifierSpec::Alt),
        "cmd" | "command" | "meta" | "super" => Some(ModifierSpec::Command),
        _ => None,
    }
}

fn modifier_mask_for_value(val: &Value) -> Option<u64> {
    let spec = if let Some(n) = val.as_i64() {
        match n {
            16 => Some(ModifierSpec::Shift),
            17 => Some(ModifierSpec::Control),
            18 => Some(ModifierSpec::Alt),
            91 | 93 | 224 => Some(ModifierSpec::Command),
            _ => None,
        }
    } else if let Some(s) = val.as_str() {
        if let Some(m) = parse_modifier_spec_str(s) {
            Some(m)
        } else if let Ok(n) = s.trim().parse::<i64>() {
            match n {
                16 => Some(ModifierSpec::Shift),
                17 => Some(ModifierSpec::Control),
                18 => Some(ModifierSpec::Alt),
                91 | 93 | 224 => Some(ModifierSpec::Command),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    spec.map(|m| match m {
        ModifierSpec::Shift => CG_FLAG_SHIFT,
        ModifierSpec::Control => CG_FLAG_CONTROL,
        ModifierSpec::Alt => CG_FLAG_ALTERNATE,
        ModifierSpec::Command => CG_FLAG_COMMAND,
    })
}

pub fn is_modifier_pressed_in_flags(key_val: &Value, flags: u64) -> bool {
    modifier_mask_for_value(key_val).is_some_and(|mask| (flags & mask) != 0)
}

fn current_modifier_flags() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: CoreGraphics C function taking an enum value and returning CGEventFlags bitmask.
        unsafe { CGEventSourceFlagsState(CG_EVENT_SOURCE_STATE_HID_SYSTEM) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeUiStrings {
    pub dllink: String,
    pub dlall: String,
    pub dlselected: String,
    pub dlvideo: String,
    pub dlpage: String,
    #[serde(rename = "dlYtChannel")]
    pub dl_yt_channel: String,
    pub dlbyfdm: String,
}

impl Default for NativeUiStrings {
    fn default() -> Self {
        Self {
            dllink: "Download with Ferrite DM".into(),
            dlall: "Download all with Ferrite DM".into(),
            dlselected: "Download selected with Ferrite DM".into(),
            dlvideo: "Download video with Ferrite DM".into(),
            dlpage: "Download page media with Ferrite DM".into(),
            dl_yt_channel: "Download channel with Ferrite DM".into(),
            dlbyfdm: "Downloaded by Ferrite DM".into(),
        }
    }
}

/// Read one 4-byte native-endian length-prefixed JSON message from browser stdin.
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<Option<Value>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_ne_bytes(len_buf) as usize;
    if len > MAX_NATIVE_MESSAGE_BYTES {
        bail!("Native message exceeds 16 MiB limit: {len}");
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    let val: Value = serde_json::from_slice(&payload).context("Invalid native message JSON")?;
    Ok(Some(val))
}

/// Write one 4-byte native-endian length-prefixed JSON message to browser stdout.
pub fn write_native_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_NATIVE_MESSAGE_BYTES {
        bail!("Outgoing native message exceeds 16 MiB limit");
    }
    let len_bytes = (payload.len() as u32).to_ne_bytes();
    writer.write_all(&len_bytes)?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub trait DaemonBridge: Send + Sync {
    fn call<'a>(
        &'a self,
        req: IpcRequest,
    ) -> Pin<Box<dyn Future<Output = Result<IpcResponse>> + Send + 'a>>;
}

pub struct UnixDaemonBridge {
    pub socket_path: PathBuf,
    pub daemon_exe: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub auto_launch: bool,
}

impl UnixDaemonBridge {
    pub fn from_data_dir(data_dir: PathBuf, auto_launch: bool) -> Self {
        let socket_path = default_socket_path(&data_dir);
        let daemon_exe = std::env::current_exe().ok().and_then(|p| {
            let sibling = p.with_file_name("ferrite-dm");
            if sibling.exists() {
                Some(sibling)
            } else {
                None
            }
        });
        Self {
            socket_path,
            daemon_exe,
            data_dir,
            auto_launch,
        }
    }

    async fn ensure_running(&self) -> Result<()> {
        let client = IpcClient::new(&self.socket_path);
        if client.ping().await {
            return Ok(());
        }
        if !self.auto_launch {
            bail!(
                "Ferrite DM daemon is not running at {}",
                self.socket_path.display()
            );
        }
        let exe = self
            .daemon_exe
            .as_ref()
            .context("ferrite-dm executable not found for auto-launch")?;
        Command::new(exe)
            .args(["--hidden", "--data-dir"])
            .arg(&self.data_dir)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to spawn background ferrite-dm daemon")?;

        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if client.ping().await {
                return Ok(());
            }
        }
        bail!("Timed out waiting for ferrite-dm daemon socket")
    }
}

impl DaemonBridge for UnixDaemonBridge {
    fn call<'a>(
        &'a self,
        req: IpcRequest,
    ) -> Pin<Box<dyn Future<Output = Result<IpcResponse>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_running().await?;
            IpcClient::new(&self.socket_path).send(&req).await
        })
    }
}

pub fn is_video_mime_type(mime: &str) -> bool {
    let base = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if VIDEO_SNIFFER_MIME_TYPES.contains(&base.as_str()) {
        return true;
    }
    base.starts_with("video/") || base.contains("mpegurl") || base.contains("dash+xml")
}

pub fn should_ignore_download_by_settings(
    url_str: &str,
    suggested_name: Option<&str>,
    settings: &Value,
    key_pressed: bool,
) -> bool {
    if !settings
        .get("monitor")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
    {
        return true;
    }
    let skip_key = settings
        .get("skipIfKeyPressed")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !skip_key.is_empty() && key_pressed {
        return true;
    }

    let parsed = Url::parse(url_str).ok();
    if settings
        .get("skipServersEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let (Some(host), Some(servers)) = (
            parsed.as_ref().and_then(|u| u.host_str()),
            settings.get("skipServers").and_then(|v| v.as_array()),
        ) {
            let host_l = host.to_ascii_lowercase();
            for s in servers.iter().filter_map(|x| x.as_str()) {
                let s_l = s.trim().to_ascii_lowercase();
                if !s_l.is_empty() && (host_l == s_l || host_l.ends_with(&format!(".{s_l}"))) {
                    return true;
                }
            }
        }
    }

    let ext = suggested_name
        .and_then(|n| Path::new(n).extension().and_then(|e| e.to_str()))
        .or_else(|| {
            parsed.as_ref().and_then(|u| {
                u.path_segments()
                    .and_then(|s| s.last())
                    .and_then(|seg| Path::new(seg).extension().and_then(|e| e.to_str()))
            })
        })
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if !ext.is_empty() {
        let skip_exts: Vec<String> = settings
            .get("skipExtensions")
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().map(|x| x.trim_start_matches('.').to_ascii_lowercase()).collect())
            .unwrap_or_default();
        if skip_exts.iter().any(|e| e == &ext) {
            return true;
        }
        let catch_exts: Vec<String> = settings
            .get("catchExtensions")
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().map(|x| x.trim_start_matches('.').to_ascii_lowercase()).collect())
            .unwrap_or_default();
        if !catch_exts.is_empty() && !catch_exts.iter().any(|e| e == &ext) {
            return true;
        }
    }

    false
}

fn parse_download_item(item: &Value) -> Option<NewDownloadRequest> {
    let url = item
        .get("url")
        .or_else(|| item.get("Url"))
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();
    if url.is_empty() {
        return None;
    }

    let mut req = NewDownloadRequest::from_url(&url);
    req.file_name = item
        .get("suggestedName")
        .or_else(|| item.get("fileName"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    req.title = req.file_name.clone();
    req.silent = item.get("silent").and_then(|v| v.as_bool()).unwrap_or(false);

    let lower = url.to_ascii_lowercase();
    if lower.contains(".m3u8") {
        req.download_type = Some(DownloadType::HlsM3u8);
    } else if lower.contains(".mpd") {
        req.download_type = Some(DownloadType::Dash);
    } else if lower.starts_with("magnet:") || lower.ends_with(".torrent") {
        req.download_type = Some(DownloadType::BitTorrent);
    }

    req.http = HttpRequestOptions {
        url: url.clone(),
        original_url: item
            .get("originalUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        referrer: item
            .get("httpReferer")
            .or_else(|| item.get("referrer"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        document_url: item
            .get("documentUrl")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        user_agent: item
            .get("userAgent")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        cookies: item
            .get("httpCookies")
            .or_else(|| item.get("cookies"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        post_data: item
            .get("httpPostData")
            .or_else(|| item.get("postData"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ..Default::default()
    };
    Some(req)
}

fn ok_reply(id: Value, task_type: &str, result: Value) -> Value {
    json!({
        "id": id,
        "type": task_type,
        "error": "",
        "result": result,
    })
}

fn err_reply(id: Value, task_type: &str, error_code: &str, message: &str) -> Value {
    json!({
        "id": id,
        "type": task_type,
        "error": error_code,
        "error_message": message,
        "result": Value::Null,
    })
}

pub async fn dispatch_native_task<B: DaemonBridge>(bridge: &B, task: &Value) -> Value {
    let id = task.get("id").cloned().unwrap_or(json!(0));
    let Some(task_type) = task.get("type").and_then(|v| v.as_str()) else {
        return err_reply(id, "", "invalid_parameter", "Missing task type");
    };

    match task_type {
        "handshake" => {
            let api_version = task
                .get("api_version")
                .and_then(|v| v.as_str())
                .unwrap_or("1");
            let browser = task
                .get("browser")
                .cloned()
                .unwrap_or_else(|| json!("chrome"));
            let daemon_ok = matches!(bridge.call(IpcRequest::Ping).await, Ok(IpcResponse::Ok { .. }));
            ok_reply(
                id,
                task_type,
                json!({
                    "api_version": api_version,
                    "browser": browser,
                    "engine": "FerriteDM",
                    "compatible_version": "6.34.4",
                    "daemon_connected": daemon_ok,
                }),
            )
        }
        "ui_strings" => {
            let strings = NativeUiStrings::default();
            ok_reply(id, task_type, serde_json::to_value(strings).unwrap_or(json!({})))
        }
        "query_settings" => match bridge.call(IpcRequest::BrowserQuerySettings).await {
            Ok(IpcResponse::Ok { data }) => ok_reply(
                id,
                task_type,
                json!({
                    "settings": data,
                    "allowDownload": true,
                }),
            ),
            Ok(IpcResponse::Err { code, message }) => err_reply(id, task_type, &code, &message),
            Err(e) => err_reply(id, task_type, "app_not_installed", &e.to_string()),
        },
        "post_settings" => {
            let patch = task
                .get("settings")
                .cloned()
                .unwrap_or_else(|| task.clone());
            match bridge.call(IpcRequest::BrowserPostSettings(patch)).await {
                Ok(IpcResponse::Ok { data }) => ok_reply(id, task_type, json!({ "settings": data })),
                Ok(IpcResponse::Err { code, message }) => err_reply(id, task_type, &code, &message),
                Err(e) => err_reply(id, task_type, "app_not_installed", &e.to_string()),
            }
        }
        "create_downloads" => {
            let items = task
                .get("downloads")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return err_reply(
                    id,
                    task_type,
                    "invalid_parameter",
                    "Missing or empty downloads array",
                );
            }

            let ignore_settings = task
                .get("ignoreSettings")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let browser_settings = if !ignore_settings {
                match bridge.call(IpcRequest::BrowserQuerySettings).await {
                    Ok(IpcResponse::Ok { data }) => Some(data),
                    _ => None,
                }
            } else {
                None
            };

            let key_pressed = match task.get("bypassKeyPressed").and_then(|v| v.as_bool()) {
                Some(explicit) => explicit,
                None => browser_settings
                    .as_ref()
                    .and_then(|s| s.get("skipIfKeyPressed"))
                    .is_some_and(|kv| is_modifier_pressed_in_flags(kv, current_modifier_flags())),
            };

            let mut catched_downloads = Vec::new();
            let mut download_ids = Vec::new();
            let mut ignored_count = 0usize;

            for item in &items {
                let Some(req) = parse_download_item(item) else {
                    continue;
                };
                if let Some(ref settings) = browser_settings {
                    if should_ignore_download_by_settings(
                        &req.url,
                        req.file_name.as_deref(),
                        settings,
                        key_pressed,
                    ) {
                        ignored_count += 1;
                        continue;
                    }
                }
                let url_copy = req.url.clone();
                match bridge.call(IpcRequest::AddDownload(req)).await {
                    Ok(IpcResponse::Ok { data }) => {
                        if let Some(dl_id) = data.get("id").and_then(|v| v.as_i64()) {
                            download_ids.push(dl_id);
                        }
                        catched_downloads.push(url_copy);
                    }
                    Ok(IpcResponse::Err { code, message }) => {
                        return err_reply(id, task_type, &code, &message);
                    }
                    Err(e) => return err_reply(id, task_type, "app_not_installed", &e.to_string()),
                }
            }

            if catched_downloads.is_empty() && ignored_count > 0 {
                return err_reply(
                    id,
                    task_type,
                    "ignoredDueSettings",
                    "Download skipped by browser monitoring filter rules",
                );
            }

            ok_reply(
                id,
                task_type,
                json!({
                    "catchedDownloads": catched_downloads,
                    "downloadIds": download_ids,
                }),
            )
        }
        "video_sniffer" => {
            let mut detected = Vec::new();
            let mut created_ids = Vec::new();
            let create_now = task
                .get("createDownload")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let candidates = task
                .get("requests")
                .or_else(|| task.get("downloads"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_else(|| {
                    if task.get("url").is_some() {
                        vec![task.clone()]
                    } else {
                        vec![]
                    }
                });

            for item in &candidates {
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if url.is_empty() {
                    continue;
                }
                let mime = item
                    .get("mimeType")
                    .or_else(|| item.get("contentType"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let lower = url.to_ascii_lowercase();
                let is_media = is_video_mime_type(mime)
                    || lower.contains(".m3u8")
                    || lower.contains(".mpd")
                    || lower.ends_with(".mp4")
                    || lower.ends_with(".webm")
                    || lower.ends_with(".flv");
                if is_media {
                    detected.push(json!({
                        "url": url,
                        "mimeType": mime,
                        "title": item.get("suggestedName").or_else(|| item.get("title")).cloned().unwrap_or(json!("Video")),
                        "isHls": lower.contains(".m3u8"),
                        "isDash": lower.contains(".mpd"),
                    }));
                    if create_now {
                        if let Some(mut req) = parse_download_item(item) {
                            if req.download_type.is_none() {
                                req.download_type = Some(DownloadType::MediaParser);
                            }
                            if let Ok(IpcResponse::Ok { data }) =
                                bridge.call(IpcRequest::AddDownload(req)).await
                            {
                                if let Some(dl_id) = data.get("id").and_then(|v| v.as_i64()) {
                                    created_ids.push(dl_id);
                                }
                            }
                        }
                    }
                }
            }

            ok_reply(
                id,
                task_type,
                json!({
                    "video": !detected.is_empty(),
                    "dlvideo": detected,
                    "downloadIds": created_ids,
                }),
            )
        }
        "browser_proxy" => match bridge.call(IpcRequest::GetConfig).await {
            Ok(IpcResponse::Ok { data }) => {
                let cfg: FdmConfig = serde_json::from_value(data).unwrap_or_default();
                ok_reply(
                    id,
                    task_type,
                    json!({
                        "mode": cfg.app.proxy.mode,
                        "http": cfg.app.proxy.http,
                        "https": cfg.app.proxy.https,
                        "socks5": cfg.app.proxy.socks5,
                    }),
                )
            }
            _ => ok_reply(id, task_type, json!({ "mode": "system_proxy" })),
        },
        "window" => {
            let _ = bridge.call(IpcRequest::RaiseWindow).await;
            ok_reply(id, task_type, json!({ "raised": true }))
        }
        "key_state" => {
            let raw_key = task.get("keyCode").or_else(|| task.get("key")).cloned();
            let needs_cfg = match &raw_key {
                None => true,
                Some(v) if v.is_null() => true,
                Some(v) if v.as_i64() == Some(0) => true,
                Some(v)
                    if v.as_str()
                        .is_some_and(|s| s.trim().is_empty() || s.trim() == "0") =>
                {
                    true
                }
                _ => false,
            };
            let (resolved_key, enabled) = if needs_cfg {
                match bridge.call(IpcRequest::GetConfig).await {
                    Ok(IpcResponse::Ok { data }) => {
                        let cfg: FdmConfig = serde_json::from_value(data).unwrap_or_default();
                        if cfg.app.wb_dont_intercept_if_key_pressed
                            && !cfg.app.wb_bypass_key.trim().is_empty()
                        {
                            (json!(cfg.app.wb_bypass_key), true)
                        } else {
                            (raw_key.unwrap_or_else(|| json!(0)), false)
                        }
                    }
                    _ => (raw_key.unwrap_or_else(|| json!(0)), false),
                }
            } else {
                (raw_key.unwrap_or_else(|| json!(0)), true)
            };
            let pressed = if enabled {
                is_modifier_pressed_in_flags(&resolved_key, current_modifier_flags())
            } else {
                false
            };
            ok_reply(
                id,
                task_type,
                json!({
                    "keyCode": resolved_key,
                    "pressed": pressed,
                }),
            )
        }
        "network_request_notification" | "browser_download_state_report" | "fdm_json_task" => {
            ok_reply(id, task_type, json!({ "accepted": true }))
        }
        _ => err_reply(
            id,
            task_type,
            "unknown_task_type",
            &format!("Unsupported native host task type: {task_type}"),
        ),
    }
}

pub fn build_native_manifest_json(browser: KnownWebBrowser, host_exe_path: &Path) -> Value {
    let base = json!({
        "name": NATIVE_HOST_NAME,
        "description": "Ferrite DM Native Messaging Host (Free Download Manager extension compatible)",
        "path": host_exe_path.to_string_lossy(),
        "type": "stdio",
    });
    let mut obj = base.as_object().cloned().unwrap();
    match browser {
        KnownWebBrowser::Firefox => {
            obj.insert(
                "allowed_extensions".into(),
                Value::Array(
                    FIREFOX_ALLOWED_EXTENSIONS
                        .iter()
                        .map(|s| json!(s))
                        .collect(),
                ),
            );
        }
        _ => {
            obj.insert(
                "allowed_origins".into(),
                Value::Array(CHROME_ALLOWED_ORIGINS.iter().map(|s| json!(s)).collect()),
            );
        }
    }
    Value::Object(obj)
}

pub fn default_native_manifest_dir(browser: KnownWebBrowser) -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    #[cfg(target_os = "macos")]
    {
        let app_support = base.data_local_dir(); // ~/Library/Application Support
        let rel = match browser {
            KnownWebBrowser::Chrome => "Google/Chrome/NativeMessagingHosts",
            KnownWebBrowser::Brave => "BraveSoftware/Brave-Browser/NativeMessagingHosts",
            KnownWebBrowser::Edge => "Microsoft Edge/NativeMessagingHosts",
            KnownWebBrowser::Vivaldi => "Vivaldi/NativeMessagingHosts",
            KnownWebBrowser::Opera => "com.operasoftware.Opera/NativeMessagingHosts",
            KnownWebBrowser::Firefox => "Mozilla/NativeMessagingHosts",
            KnownWebBrowser::Safari => return None,
        };
        return Some(app_support.join(rel));
    }
    #[cfg(target_os = "linux")]
    {
        let cfg = base.config_dir();
        let home = base.home_dir();
        return match browser {
            KnownWebBrowser::Chrome => Some(cfg.join("google-chrome/NativeMessagingHosts")),
            KnownWebBrowser::Brave => Some(cfg.join("BraveSoftware/Brave-Browser/NativeMessagingHosts")),
            KnownWebBrowser::Edge => Some(cfg.join("microsoft-edge/NativeMessagingHosts")),
            KnownWebBrowser::Vivaldi => Some(cfg.join("vivaldi/NativeMessagingHosts")),
            KnownWebBrowser::Opera => Some(cfg.join("opera/NativeMessagingHosts")),
            KnownWebBrowser::Firefox => Some(home.join(".mozilla/native-messaging-hosts")),
            KnownWebBrowser::Safari => None,
        };
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub fn install_native_manifest_to_dir(
    browser: KnownWebBrowser,
    host_exe_path: &Path,
    target_dir: &Path,
) -> Result<PathBuf> {
    std::fs::create_dir_all(target_dir)?;
    let dest = target_dir.join(format!("{NATIVE_HOST_NAME}.json"));
    let manifest = build_native_manifest_json(browser, host_exe_path);
    std::fs::write(&dest, serde_json::to_string_pretty(&manifest)?)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct MockBridge {
        requests: Arc<Mutex<Vec<IpcRequest>>>,
        settings: FdmConfig,
    }

    impl MockBridge {
        fn new() -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                settings: FdmConfig::default(),
            }
        }
    }

    impl DaemonBridge for MockBridge {
        fn call<'a>(
            &'a self,
            req: IpcRequest,
        ) -> Pin<Box<dyn Future<Output = Result<IpcResponse>> + Send + 'a>> {
            let settings = self.settings.clone();
            let log = Arc::clone(&self.requests);
            Box::pin(async move {
                log.lock().unwrap().push(req.clone());
                match req {
                    IpcRequest::Ping => Ok(IpcResponse::ok(json!({ "pong": true }))),
                    IpcRequest::BrowserQuerySettings => {
                        Ok(IpcResponse::ok(fdm_ipc::build_browser_settings_payload(&settings)))
                    }
                    IpcRequest::BrowserPostSettings(patch) => {
                        let mut cfg = settings.clone();
                        fdm_ipc::apply_browser_settings_patch(&mut cfg, &patch);
                        Ok(IpcResponse::ok(fdm_ipc::build_browser_settings_payload(&cfg)))
                    }
                    IpcRequest::AddDownload(dl) => Ok(IpcResponse::ok(json!({
                        "id": 42,
                        "url": dl.url,
                    }))),
                    IpcRequest::GetConfig => Ok(IpcResponse::ok(&settings)),
                    IpcRequest::RaiseWindow => Ok(IpcResponse::ok(json!({ "ok": true }))),
                    _ => Ok(IpcResponse::err("unsupported", "mock")),
                }
            })
        }
    }

    #[test]
    fn stdio_length_prefixed_framing_roundtrip() {
        let mut buf = Vec::new();
        let msg = json!({ "id": 7, "type": "handshake", "browser": "chrome" });
        write_native_message(&mut buf, &msg).unwrap();
        let expected_len = (buf.len() - 4) as u32;
        assert_eq!(&buf[..4], &expected_len.to_ne_bytes());

        let mut cursor = std::io::Cursor::new(buf);
        let decoded = read_native_message(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded, msg);
        assert!(read_native_message(&mut cursor).unwrap().is_none());
    }

    #[tokio::test]
    async fn handles_all_wenativehost_tasks_and_filters() {
        let bridge = MockBridge::new();

        // 1. handshake & ui_strings
        let hs = dispatch_native_task(&bridge, &json!({ "id": 1, "type": "handshake", "browser": "chrome" })).await;
        assert_eq!(hs["error"], "");
        assert_eq!(hs["result"]["daemon_connected"], true);

        let ui = dispatch_native_task(&bridge, &json!({ "id": 2, "type": "ui_strings" })).await;
        assert_eq!(ui["result"]["dlvideo"], "Download video with Ferrite DM");

        // 2. create_downloads passes matching extension and preserves cookies/referer
        let create = dispatch_native_task(
            &bridge,
            &json!({
                "id": 3,
                "type": "create_downloads",
                "downloads": [{
                    "url": "https://cdn.example.com/archive.zip",
                    "httpReferer": "https://example.com/",
                    "httpCookies": "session=abc",
                    "suggestedName": "archive.zip"
                }]
            }),
        )
        .await;
        assert_eq!(create["error"], "");
        assert_eq!(create["result"]["catchedDownloads"][0], "https://cdn.example.com/archive.zip");
        assert_eq!(create["result"]["downloadIds"][0], 42);

        // 3. create_downloads respects skipServers filter (rapidgator.net is default skipped)
        let skipped = dispatch_native_task(
            &bridge,
            &json!({
                "id": 4,
                "type": "create_downloads",
                "downloads": [{ "url": "https://rapidgator.net/file/123/test.zip" }]
            }),
        )
        .await;
        assert_eq!(skipped["error"], "ignoredDueSettings");

        // 4. video_sniffer identifies HLS / video MIME types
        let sniff = dispatch_native_task(
            &bridge,
            &json!({
                "id": 5,
                "type": "video_sniffer",
                "requests": [
                    { "url": "https://cdn.example.com/seg/master.m3u8", "mimeType": "application/vnd.apple.mpegurl" },
                    { "url": "https://cdn.example.com/style.css", "mimeType": "text/css" }
                ]
            }),
        )
        .await;
        assert_eq!(sniff["result"]["video"], true);
        assert_eq!(sniff["result"]["dlvideo"].as_array().unwrap().len(), 1);

        // 5. create_downloads respects bypassKeyPressed = true with default Alt bypass key
        let bypassed = dispatch_native_task(
            &bridge,
            &json!({
                "id": 6,
                "type": "create_downloads",
                "bypassKeyPressed": true,
                "downloads": [{ "url": "https://cdn.example.com/archive.zip" }]
            }),
        )
        .await;
        assert_eq!(bypassed["error"], "ignoredDueSettings");

        // 6. key_state resolves configured bypass key when keyCode is 0 or omitted
        let ks = dispatch_native_task(&bridge, &json!({ "id": 7, "type": "key_state", "keyCode": 0 })).await;
        assert_eq!(ks["error"], "");
        assert_eq!(ks["result"]["keyCode"], "Alt");

        // 7. unknown task returns unknown_task_type
        let unk = dispatch_native_task(&bridge, &json!({ "id": 8, "type": "nonexistent" })).await;
        assert_eq!(unk["error"], "unknown_task_type");
    }

    #[test]
    fn modifier_flag_matching_supports_names_and_keycodes() {
        assert!(is_modifier_pressed_in_flags(&json!("Alt"), CG_FLAG_ALTERNATE));
        assert!(is_modifier_pressed_in_flags(&json!(18), CG_FLAG_ALTERNATE));
        assert!(!is_modifier_pressed_in_flags(&json!("Alt"), CG_FLAG_SHIFT));
        assert!(is_modifier_pressed_in_flags(&json!("Ctrl"), CG_FLAG_CONTROL));
        assert!(is_modifier_pressed_in_flags(&json!(17), CG_FLAG_CONTROL));
        assert!(is_modifier_pressed_in_flags(&json!("Shift"), CG_FLAG_SHIFT));
        assert!(is_modifier_pressed_in_flags(&json!(16), CG_FLAG_SHIFT));
        assert!(is_modifier_pressed_in_flags(&json!("Cmd"), CG_FLAG_COMMAND));
        assert!(is_modifier_pressed_in_flags(&json!(91), CG_FLAG_COMMAND));
    }

    #[test]
    fn generates_and_installs_native_host_manifests() {
        let dir = std::env::temp_dir().join(format!("ferrite_nm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let chrome_path = install_native_manifest_to_dir(
            KnownWebBrowser::Chrome,
            Path::new("/usr/local/bin/ferrite-native-host"),
            &dir.join("chrome"),
        )
        .unwrap();
        let raw_chrome: Value =
            serde_json::from_str(&std::fs::read_to_string(&chrome_path).unwrap()).unwrap();
        assert_eq!(raw_chrome["name"], NATIVE_HOST_NAME);
        assert!(raw_chrome["allowed_origins"].as_array().unwrap().len() >= 2);

        let ff_path = install_native_manifest_to_dir(
            KnownWebBrowser::Firefox,
            Path::new("/usr/local/bin/ferrite-native-host"),
            &dir.join("firefox"),
        )
        .unwrap();
        let raw_ff: Value = serde_json::from_str(&std::fs::read_to_string(&ff_path).unwrap()).unwrap();
        assert_eq!(
            raw_ff["allowed_extensions"][0],
            "fdm_ffext@freedownloadmanager.org"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
