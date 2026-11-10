use anyhow::{Context, Result};
use fdm_core::{
    DownloadId, DownloadRecord, FdmConfig, FileExistsAction, FileType, FilterQuery,
    NewDownloadRequest, Tag, TrafficUsageMode,
    hash::sha256_str,
};
use futures::{SinkExt, StreamExt};
use qrcode::QrCode;
use qrcode::render::svg;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum IpcRequest {
    Ping,
    AddDownload(NewDownloadRequest),
    ListDownloads(FilterQuery),
    GetDownload { id: DownloadId },
    Pause { id: DownloadId },
    Resume { id: DownloadId },
    Cancel { id: DownloadId },
    Remove { id: DownloadId, delete_files: bool },
    StartAll,
    PauseAll,
    SetTrafficUsageMode { mode: TrafficUsageMode },
    GetConfig,
    UpdateConfig(FdmConfig),
    ListTags,
    CreateTag {
        name: String,
        color_hex: String,
        file_types: Vec<FileType>,
    },
    AssignTag { id: DownloadId, tag_id: i64 },
    ResolveConflict {
        id: DownloadId,
        action: FileExistsAction,
    },
    BrowserQuerySettings,
    BrowserPostSettings(serde_json::Value),
    RemoteCommand {
        action: String,
        payload: serde_json::Value,
    },
    RaiseWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IpcResponse {
    Ok { data: serde_json::Value },
    Err { code: String, message: String },
}

impl IpcResponse {
    pub fn ok<T: Serialize>(v: T) -> Self {
        match serde_json::to_value(v) {
            Ok(data) => Self::Ok { data },
            Err(e) => Self::Err {
                code: "serialize_error".into(),
                message: e.to_string(),
            },
        }
    }

    pub fn err(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Err {
            code: code.into(),
            message: msg.into(),
        }
    }
}

pub fn default_socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ferrite.sock")
}

pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub async fn send(&self, req: &IpcRequest) -> Result<IpcResponse> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("Failed to connect to daemon socket {}", self.socket_path.display()))?;
        let (reader, mut writer) = stream.into_split();
        let mut line = serde_json::to_string(req)?;
        line.push('\n');
        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;

        let mut buf_reader = BufReader::new(reader);
        let mut resp_line = String::new();
        buf_reader.read_line(&mut resp_line).await?;
        if resp_line.trim().is_empty() {
            anyhow::bail!("Daemon closed IPC socket without response");
        }
        Ok(serde_json::from_str(&resp_line)?)
    }

    pub async fn ping(&self) -> bool {
        matches!(self.send(&IpcRequest::Ping).await, Ok(IpcResponse::Ok { .. }))
    }
}

pub async fn start_ipc_server<F, Fut>(
    socket_path: PathBuf,
    handler: Arc<F>,
) -> Result<tokio::task::JoinHandle<()>>
where
    F: Fn(IpcRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IpcResponse> + Send + 'static,
{
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        // Check if existing daemon is actually alive; if stale, remove socket
        if IpcClient::new(&socket_path).ping().await {
            anyhow::bail!("Ferrite DM daemon is already running on {}", socket_path.display());
        }
        let _ = std::fs::remove_file(&socket_path);
    }

    let listener = UnixListener::bind(&socket_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
    }

    let handle = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let h = Arc::clone(&handler);
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let resp = match serde_json::from_str::<IpcRequest>(&line) {
                        Ok(req) => h(req).await,
                        Err(e) => IpcResponse::err("invalid_request", e.to_string()),
                    };
                    if let Ok(mut out) = serde_json::to_string(&resp) {
                        out.push('\n');
                        if writer.write_all(out.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
            });
        }
    });
    Ok(handle)
}

pub fn build_browser_settings_payload(config: &FdmConfig) -> serde_json::Value {
    serde_json::json!({
        "skipSmallerThan": if config.app.wb_dont_intercept_too_small {
            config.app.wb_too_small_size_bytes
        } else {
            0
        },
        "skipExtensions": if config.app.wb_dont_intercept_unwanted_extensions {
            config.app.wb_unwanted_extensions.join(" ")
        } else {
            String::new()
        },
        "catchExtensions": if config.app.wb_intercept_only_extensions {
            config.app.wb_intercept_extensions.join(" ")
        } else {
            String::new()
        },
        "skipServersEnabled": config.app.wb_dont_intercept_unwanted_hosts,
        "skipServers": config.app.wb_unwanted_hosts,
        "skipIfKeyPressed": if config.app.wb_dont_intercept_if_key_pressed {
            config.app.wb_bypass_key.clone()
        } else {
            String::new()
        },
        "forwardAbortedToBrowser": config.app.wb_forward_aborted_to_browser,
        "monitor": config.app.wb_intercept_downloads,
        "allowDownload": true
    })
}

pub fn apply_browser_settings_patch(config: &mut FdmConfig, patch: &serde_json::Value) {
    if let Some(m) = patch.get("monitor").and_then(|v| v.as_bool()) {
        config.app.wb_intercept_downloads = m;
    }
    if let Some(b) = patch.get("forwardAbortedToBrowser").and_then(|v| v.as_bool()) {
        config.app.wb_forward_aborted_to_browser = b;
    }
    if let Some(sz) = patch.get("skipSmallerThan").and_then(|v| v.as_u64()) {
        if sz == 0 {
            config.app.wb_dont_intercept_too_small = false;
        } else {
            config.app.wb_dont_intercept_too_small = true;
            config.app.wb_too_small_size_bytes = sz;
        }
    }
    if let Some(ext_str) = patch.get("skipExtensions").and_then(|v| v.as_str()) {
        config.app.wb_unwanted_extensions = ext_str
            .split_whitespace()
            .map(str::to_string)
            .collect();
        config.app.wb_dont_intercept_unwanted_extensions = !config.app.wb_unwanted_extensions.is_empty();
    }
    if let Some(ext_str) = patch.get("catchExtensions").and_then(|v| v.as_str()) {
        config.app.wb_intercept_extensions = ext_str
            .split_whitespace()
            .map(str::to_string)
            .collect();
        config.app.wb_intercept_only_extensions = !config.app.wb_intercept_extensions.is_empty();
    }
    if let Some(b) = patch.get("skipServersEnabled").and_then(|v| v.as_bool()) {
        config.app.wb_dont_intercept_unwanted_hosts = b;
    }
    if let Some(arr) = patch.get("skipServers").and_then(|v| v.as_array()) {
        config.app.wb_unwanted_hosts = arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(key) = patch.get("skipIfKeyPressed").and_then(|v| v.as_str()) {
        if key.is_empty() {
            config.app.wb_dont_intercept_if_key_pressed = false;
        } else {
            config.app.wb_dont_intercept_if_key_pressed = true;
            config.app.wb_bypass_key = key.to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePairingInfo {
    pub server_id: String,
    pub pairing_secret: String,
    pub password_hash: String,
    pub fdm_uri: String,
    pub https_url: String,
    pub qr_svg: String,
    pub qr_ascii: String,
}

pub fn generate_pairing_info(server_id: &str, host: &str, port: u16) -> Result<RemotePairingInfo> {
    let secret = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
    generate_pairing_info_with_secret(server_id, host, port, &secret)
}

pub fn generate_pairing_info_with_secret(
    server_id: &str,
    host: &str,
    port: u16,
    secret: &str,
) -> Result<RemotePairingInfo> {
    let password_hash = sha256_str(secret);
    let fdm_uri = format!(
        "fdm://remote_control/?id={}&password={}&host={}&port={}",
        server_id, secret, host, port
    );
    let https_url = format!("https://{}:{}/?token={}", host, port, secret);
    let code = QrCode::new(fdm_uri.as_bytes())?;
    let qr_svg = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#111827"))
        .light_color(svg::Color("#ffffff"))
        .build();
    let qr_ascii = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build();
    Ok(RemotePairingInfo {
        server_id: server_id.to_string(),
        pairing_secret: secret.to_string(),
        password_hash,
        fdm_uri,
        https_url,
        qr_svg,
        qr_ascii,
    })
}

pub fn load_or_create_self_signed_tls(cert_dir: &Path) -> Result<Arc<rustls::ServerConfig>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(cert_dir)?;
    let cert_path = cert_dir.join("rc_cert.der");
    let key_path = cert_dir.join("rc_key.der");

    let (cert_der, key_der) = if cert_path.is_file() && key_path.is_file() {
        (std::fs::read(&cert_path)?, std::fs::read(&key_path)?)
    } else {
        let mut sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        if let Ok(hn) = std::env::var("HOSTNAME") {
            if !hn.is_empty() {
                sans.push(hn);
            }
        }
        let ck = rcgen::generate_simple_self_signed(sans)?;
        let c = ck.cert.der().to_vec();
        let k = ck.signing_key.serialize_der();
        std::fs::write(&cert_path, &c)?;
        std::fs::write(&key_path, &k)?;
        (c, k)
    };

    let cert = CertificateDer::from(cert_der);
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)?;
    Ok(Arc::new(cfg))
}

struct PrefixedStream<S> {
    prefix: std::io::Cursor<Vec<u8>>,
    inner: S,
}

impl<S> PrefixedStream<S> {
    fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self {
            prefix: std::io::Cursor::new(prefix),
            inner,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let pos = self.prefix.position() as usize;
        let total = self.prefix.get_ref().len();
        if pos < total {
            let rem = &self.prefix.get_ref()[pos..];
            let n = rem.len().min(buf.remaining());
            buf.put_slice(&rem[..n]);
            self.prefix.set_position((pos + n) as u64);
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

const REMOTE_WEB_UI_HTML: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ferrite DM Remote Control</title>
<style>
body{font-family:system-ui,-apple-system,sans-serif;background:#0f172a;color:#e2e8f0;margin:0;padding:16px}
.card{background:#1e293b;border-radius:10px;padding:14px;margin-bottom:12px;border:1px solid #334155}
input,button,select{padding:8px 12px;border-radius:6px;border:1px solid #475569;background:#0f172a;color:#fff}
button{background:#2563eb;border:none;cursor:pointer;font-weight:600}
.row{display:flex;gap:8px;flex-wrap:wrap;align-items:center}
.dl{display:flex;justify-content:space-between;align-items:center;padding:8px 0;border-bottom:1px solid #334155}
</style>
</head>
<body>
<h2>Ferrite DM Remote Control</h2>
<div class="card row">
  <input id="tok" placeholder="Pairing token" style="width:160px">
  <button onclick="connectWs()">Connect</button>
  <select id="tum" onchange="sendCmd('set_tum',{mode:this.value})">
    <option value="high">High Speed</option>
    <option value="medium">Medium</option>
    <option value="low">Low</option>
    <option value="snail">Snail</option>
  </select>
  <span id="st">Disconnected</span>
</div>
<div class="card row">
  <input id="url" placeholder="https://... or magnet:?xt=..." style="flex:1;min-width:200px">
  <button onclick="addUrl()">Add Download</button>
</div>
<div id="list" class="card">No downloads loaded yet.</div>
<script>
let ws;
const qp = new URLSearchParams(location.search);
if (qp.get('token')) document.getElementById('tok').value = qp.get('token');
function connectWs() {
  ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host);
  ws.onopen = () => {
    document.getElementById('st').textContent = 'Connected';
    sendCmd('auth', { token: document.getElementById('tok').value });
    setInterval(() => sendCmd('list', {}), 1500);
  };
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.type === 'list') render(msg.downloads || []);
  };
}
function sendCmd(action, payload) {
  if (ws && ws.readyState === 1) ws.send(JSON.stringify({ action, payload }));
}
function addUrl() {
  const u = document.getElementById('url').value.trim();
  if (u) { sendCmd('add', { url: u }); document.getElementById('url').value = ''; }
}
function render(items) {
  document.getElementById('list').innerHTML = items.map(d =>
    `<div class="dl"><div><b>${d.file_name}</b><br><small>${d.state} • ${(d.downloaded_bytes/1048576).toFixed(1)} MB (${Math.round((d.download_speed_bps||0)/1024)} KB/s)</small></div>`+
    `<div class="row"><button onclick="sendCmd('pause',{id:${d.id}})">Pause</button><button onclick="sendCmd('resume',{id:${d.id}})">Start</button></div></div>`
  ).join('') || 'Queue empty';
}
</script>
</body>
</html>"#;

pub async fn start_remote_control_server<F, Fut, V>(
    bind_addr: SocketAddr,
    cert_dir: &Path,
    verify_token: Arc<V>,
    ipc_handler: Arc<F>,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>)>
where
    V: Fn(&str) -> bool + Send + Sync + 'static,
    F: Fn(IpcRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = IpcResponse> + Send + 'static,
{
    let tls_cfg = load_or_create_self_signed_tls(cert_dir)?;
    let acceptor = TlsAcceptor::from(tls_cfg);
    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;

    let handle = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let verify_token = Arc::clone(&verify_token);
            let ipc_handler = Arc::clone(&ipc_handler);

            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut hdr_buf = Vec::with_capacity(2048);
                let mut byte = [0u8; 1];
                while hdr_buf.len() < 16 * 1024 {
                    match tls.read(&mut byte).await {
                        Ok(1) => {
                            hdr_buf.push(byte[0]);
                            if hdr_buf.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        _ => return,
                    }
                }
                let hdr_str = String::from_utf8_lossy(&hdr_buf).to_ascii_lowercase();
                if hdr_str.contains("upgrade: websocket") {
                    let prefixed = PrefixedStream::new(hdr_buf, tls);
                    let Ok(mut ws) = tokio_tungstenite::accept_async(prefixed).await else {
                        return;
                    };
                    let mut authed = false;
                    while let Some(Ok(msg)) = ws.next().await {
                        if let Message::Text(txt) = msg {
                            let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) else {
                                continue;
                            };
                            let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("");
                            let payload = val.get("payload").cloned().unwrap_or(serde_json::Value::Null);
                            if action == "auth" {
                                let tok = payload.get("token").and_then(|v| v.as_str()).unwrap_or("");
                                authed = verify_token(tok);
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::json!({ "type": "auth", "ok": authed })
                                            .to_string()
                                            .into(),
                                    ))
                                    .await;
                                continue;
                            }
                            if !authed {
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::json!({ "type": "error", "error": "unauthorized" })
                                            .to_string()
                                            .into(),
                                    ))
                                    .await;
                                continue;
                            }

                            let req = match action {
                                "list" => Some(IpcRequest::ListDownloads(FilterQuery::default())),
                                "add" => payload
                                    .get("url")
                                    .and_then(|u| u.as_str())
                                    .map(|u| IpcRequest::AddDownload(NewDownloadRequest::from_url(u))),
                                "pause" => payload
                                    .get("id")
                                    .and_then(|i| i.as_i64())
                                    .map(|id| IpcRequest::Pause { id }),
                                "resume" => payload
                                    .get("id")
                                    .and_then(|i| i.as_i64())
                                    .map(|id| IpcRequest::Resume { id }),
                                "cancel" => payload
                                    .get("id")
                                    .and_then(|i| i.as_i64())
                                    .map(|id| IpcRequest::Cancel { id }),
                                "set_tum" => payload
                                    .get("mode")
                                    .and_then(|m| serde_json::from_value::<TrafficUsageMode>(m.clone()).ok())
                                    .map(|mode| IpcRequest::SetTrafficUsageMode { mode }),
                                _ => Some(IpcRequest::RemoteCommand {
                                    action: action.to_string(),
                                    payload,
                                }),
                            };
                            if let Some(ipc_req) = req {
                                let resp = ipc_handler(ipc_req).await;
                                let wire = match (action, resp) {
                                    ("list", IpcResponse::Ok { data }) => {
                                        serde_json::json!({ "type": "list", "downloads": data })
                                    }
                                    (_, IpcResponse::Ok { data }) => {
                                        serde_json::json!({ "type": "ok", "action": action, "data": data })
                                    }
                                    (_, IpcResponse::Err { code, message }) => {
                                        serde_json::json!({ "type": "error", "code": code, "message": message })
                                    }
                                };
                                let _ = ws.send(Message::Text(wire.to_string().into())).await;
                            }
                        }
                    }
                } else {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        REMOTE_WEB_UI_HTML.len(),
                        REMOTE_WEB_UI_HTML
                    );
                    let _ = tls.write_all(resp.as_bytes()).await;
                    let _ = tls.flush().await;
                }
            });
        }
    });

    Ok((local_addr, handle))
}

// Keep unused imports clean for downstream convenience
pub type RecordList = Vec<DownloadRecord>;
pub type TagList = Vec<Tag>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unix_ipc_server_client_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ferrite_ipc_sock_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = default_socket_path(&dir);

        let srv = start_ipc_server(
            sock.clone(),
            Arc::new(|req| async move {
                match req {
                    IpcRequest::Ping => IpcResponse::ok(serde_json::json!({"pong": true})),
                    IpcRequest::SetTrafficUsageMode { mode } => IpcResponse::ok(mode),
                    _ => IpcResponse::err("unhandled", "unsupported"),
                }
            }),
        )
        .await
        .unwrap();

        let client = IpcClient::new(&sock);
        assert!(client.ping().await);
        let resp = client
            .send(&IpcRequest::SetTrafficUsageMode {
                mode: TrafficUsageMode::Low,
            })
            .await
            .unwrap();
        match resp {
            IpcResponse::Ok { data } => assert_eq!(data, serde_json::json!("low")),
            _ => panic!("expected Ok"),
        }

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generates_qr_pairing_and_browser_settings() {
        let pair = generate_pairing_info("srv-123", "192.168.1.50", 9443).unwrap();
        assert!(pair.fdm_uri.starts_with("fdm://remote_control/?id=srv-123&password="));
        assert!(pair.qr_svg.contains("<svg"));
        assert!(!pair.qr_ascii.is_empty());
        assert_eq!(pair.password_hash, sha256_str(&pair.pairing_secret));

        let mut cfg = FdmConfig::default();
        let payload = build_browser_settings_payload(&cfg);
        assert_eq!(payload["monitor"], serde_json::json!(true));
        assert_eq!(payload["forwardAbortedToBrowser"], serde_json::json!(true));
        apply_browser_settings_patch(
            &mut cfg,
            &serde_json::json!({
                "monitor": false,
                "forwardAbortedToBrowser": false,
                "skipSmallerThan": 204800,
                "skipExtensions": "tmp log"
            }),
        );
        assert!(!cfg.app.wb_intercept_downloads);
        assert!(!cfg.app.wb_forward_aborted_to_browser);
        assert_eq!(cfg.app.wb_too_small_size_bytes, 204800);
        assert_eq!(cfg.app.wb_unwanted_extensions, vec!["tmp", "log"]);
    }
}
