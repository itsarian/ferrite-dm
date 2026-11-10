use anyhow::Result;
use base64::Engine as _;
use fdm_core::{
    AuthCredential, BandwidthLimiter, DownloadAbortReason, DownloadFileEntry, DownloadPriority,
    DownloadRecord, DownloadState, FdmConfig, NetworkProxyMode, ProxySettings, ResumeSupport,
    SegmentProgress, SegmentState, SslException,
    hash::sha256_str,
    os_integration::set_mark_of_the_web,
    routing::{classify_file_type, sanitize_filename, urlencoding_decode},
};
use futures::StreamExt;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, RANGE, REFERER, USER_AGENT};
use reqwest::{Client, Proxy, StatusCode};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream};
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use url::Url;

pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub final_url: String,
    pub total_bytes: Option<u64>,
    pub resume_support: ResumeSupport,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub suggested_filename: Option<String>,
    pub supports_range_split: bool,
}

#[derive(Debug, Clone)]
pub enum ControlCommand {
    Run,
    Pause,
    Cancel,
}

#[derive(Debug)]
struct HostPinVerifier {
    pinned_sha256_by_host: Vec<SslException>,
    inner: Arc<dyn ServerCertVerifier>,
}

impl ServerCertVerifier for HostPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(v) => Ok(v),
            Err(orig_err) => {
                let mut h = Sha256::new();
                h.update(end_entity.as_ref());
                let fp = hex::encode(h.finalize());
                let sni = match server_name {
                    ServerName::DnsName(d) => d.as_ref().to_ascii_lowercase(),
                    ServerName::IpAddress(ip) => std::net::IpAddr::from(*ip).to_string(),
                    _ => String::new(),
                };
                for exc in &self.pinned_sha256_by_host {
                    let host_only = exc
                        .host_port
                        .split(':')
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if host_only == sni && exc.sha256_fingerprint.eq_ignore_ascii_case(&fp) {
                        return Ok(ServerCertVerified::assertion());
                    }
                }
                Err(orig_err)
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn load_native_root_store() -> rustls::RootCertStore {
    let mut root_store = rustls::RootCertStore::empty();
    let res = rustls_native_certs::load_native_certs();
    for cert in res.certs {
        let _ = root_store.add(cert);
    }
    root_store
}

fn build_rustls_client_config(ssl_exceptions: &[SslException]) -> Option<rustls::ClientConfig> {
    ensure_crypto_provider();
    let root_store = load_native_root_store();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let base_verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(root_store),
        provider.clone(),
    )
    .build()
    .ok()?;

    let mut cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(HostPinVerifier {
            pinned_sha256_by_host: ssl_exceptions.to_vec(),
            inner: base_verifier,
        }))
        .with_no_client_auth();
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Some(cfg)
}

pub fn build_http_client(
    config: &FdmConfig,
    ssl_exceptions: &[SslException],
    user_agent_override: Option<&str>,
) -> Result<Client> {
    ensure_crypto_provider();
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(20));

    let ua = user_agent_override
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if config.core.use_system_default_user_agent {
                fdm_core::os_integration::system_default_user_agent()
            } else {
                "FerriteDM/1.0 (compatible; Free Download Manager 6.34)"
            }
        });
    builder = builder.user_agent(ua);

    if !ssl_exceptions.is_empty() {
        if let Some(tls_cfg) = build_rustls_client_config(ssl_exceptions) {
            builder = builder.use_preconfigured_tls(tls_cfg);
        }
    }

    builder = apply_proxy_settings(builder, &config.app.proxy)?;
    Ok(builder.build()?)
}

fn apply_proxy_settings(
    mut builder: reqwest::ClientBuilder,
    proxy: &ProxySettings,
) -> Result<reqwest::ClientBuilder> {
    match proxy.mode {
        NetworkProxyMode::NoProxy => {
            builder = builder.no_proxy();
        }
        NetworkProxyMode::SystemProxy => {
            // reqwest enables system proxy detection by default
        }
        NetworkProxyMode::ManualProxy => {
            builder = builder.no_proxy();
            if !proxy.http.host.is_empty() && proxy.http.port > 0 {
                let mut p = Proxy::http(format!("http://{}:{}", proxy.http.host, proxy.http.port))?;
                if !proxy.http.username.is_empty() {
                    p = p.basic_auth(&proxy.http.username, &proxy.http.password);
                }
                builder = builder.proxy(p);
            }
            if !proxy.https.host.is_empty() && proxy.https.port > 0 {
                let mut p =
                    Proxy::https(format!("http://{}:{}", proxy.https.host, proxy.https.port))?;
                if !proxy.https.username.is_empty() {
                    p = p.basic_auth(&proxy.https.username, &proxy.https.password);
                }
                builder = builder.proxy(p);
            }
            if !proxy.socks5.host.is_empty() && proxy.socks5.port > 0 {
                let mut p = Proxy::all(format!(
                    "socks5h://{}:{}",
                    proxy.socks5.host, proxy.socks5.port
                ))?;
                if !proxy.socks5.username.is_empty() {
                    p = p.basic_auth(&proxy.socks5.username, &proxy.socks5.password);
                }
                builder = builder.proxy(p);
            }
        }
    }
    Ok(builder)
}

fn build_request_headers(
    record: &DownloadRecord,
    cred: Option<&AuthCredential>,
) -> HeaderMap {
    let mut map = HeaderMap::new();
    if let Some(ref r) = record.http.referrer {
        if let Ok(v) = HeaderValue::from_str(r) {
            map.insert(REFERER, v);
        }
    }
    if let Some(ref c) = record.http.cookies {
        if let Ok(v) = HeaderValue::from_str(c) {
            map.insert(COOKIE, v);
        }
    }
    if let Some(ref ua) = record.http.user_agent {
        if let Ok(v) = HeaderValue::from_str(ua) {
            map.insert(USER_AGENT, v);
        }
    }
    for (k, v) in &record.http.custom_headers {
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            map.insert(hn, hv);
        }
    }
    if let Some(c) = cred {
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", c.username, c.password));
        if let Ok(hv) = HeaderValue::from_str(&format!("Basic {token}")) {
            map.insert(reqwest::header::AUTHORIZATION, hv);
        }
    }
    map
}

pub fn parse_content_disposition_filename(header_val: &str) -> Option<String> {
    // Matches FDM's filename*=UTF-8'', RFC 2047 =?utf-8?b?...?=, quoted filename="..." and bare filename=...
    let utf8_ext = Regex::new(r#"(?i)filename\*\s*=\s*UTF-8''([^;\s]+)"#).unwrap();
    if let Some(caps) = utf8_ext.captures(header_val) {
        let decoded = urlencoding_decode(&caps[1]);
        if !decoded.is_empty() {
            return Some(sanitize_filename(&decoded));
        }
    }
    let b64_re = Regex::new(r#"(?i)filename\s*=\s*"?=\?utf-8\?b\?([^;\s"?]+)\?="?"#).unwrap();
    if let Some(caps) = b64_re.captures(header_val) {
        if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(&caps[1]) {
            let s = String::from_utf8_lossy(&raw).into_owned();
            if !s.is_empty() {
                return Some(sanitize_filename(&s));
            }
        }
    }
    let quoted = Regex::new(r#"(?i)filename\s*=\s*"([^"]+)""#).unwrap();
    if let Some(caps) = quoted.captures(header_val) {
        return Some(sanitize_filename(&caps[1]));
    }
    let bare = Regex::new(r#"(?i)filename\s*=\s*([^;\s]+)"#).unwrap();
    if let Some(caps) = bare.captures(header_val) {
        return Some(sanitize_filename(&caps[1]));
    }
    None
}

fn parse_content_range_total(val: &str) -> Option<u64> {
    // bytes 0-0/12345
    val.rsplit('/').next().and_then(|s| {
        let t = s.trim();
        if t == "*" {
            None
        } else {
            t.parse::<u64>().ok()
        }
    })
}

pub async fn probe_http_resource(
    client: &Client,
    record: &DownloadRecord,
    ssl_exceptions: &[SslException],
    cred: Option<&AuthCredential>,
) -> Result<ProbeResult> {
    if record.url.starts_with("ftp://") || record.url.starts_with("ftps://") {
        return probe_ftp_resource(&record.url, ssl_exceptions, cred).await;
    }

    let headers = build_request_headers(record, cred);
    let head_resp = client.head(&record.url).headers(headers.clone()).send().await;

    let mut final_url = record.url.clone();
    let mut total_bytes = None;
    let mut accept_ranges = false;
    let mut etag = None;
    let mut last_modified = None;
    let mut content_type = None;
    let mut suggested_filename = None;

    if let Ok(resp) = head_resp {
        if resp.status().is_success() {
            final_url = resp.url().to_string();
            let h = resp.headers();
            total_bytes = h
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            accept_ranges = h
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_ascii_lowercase().contains("bytes"))
                .unwrap_or(false);
            etag = h
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            last_modified = h
                .get(reqwest::header::LAST_MODIFIED)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            content_type = h
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            suggested_filename = h
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_disposition_filename);
        }
    }

    // Fallback or verify byte-range capability via GET Range: bytes=0-0
    if !accept_ranges || total_bytes.is_none() {
        let mut h2 = headers;
        h2.insert(RANGE, HeaderValue::from_static("bytes=0-0"));
        if let Ok(resp) = client.get(&record.url).headers(h2).send().await {
            final_url = resp.url().to_string();
            let st = resp.status();
            let h = resp.headers();
            if etag.is_none() {
                etag = h
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
            }
            if last_modified.is_none() {
                last_modified = h
                    .get(reqwest::header::LAST_MODIFIED)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
            }
            if content_type.is_none() {
                content_type = h
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
            }
            if suggested_filename.is_none() {
                suggested_filename = h
                    .get(reqwest::header::CONTENT_DISPOSITION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_disposition_filename);
            }
            if st == StatusCode::PARTIAL_CONTENT {
                accept_ranges = true;
                if let Some(cr) = h
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                {
                    if let Some(tot) = parse_content_range_total(cr) {
                        total_bytes = Some(tot);
                    }
                }
            } else if total_bytes.is_none() {
                total_bytes = h
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());
            }
        }
    }

    Ok(ProbeResult {
        final_url,
        total_bytes,
        resume_support: if accept_ranges {
            ResumeSupport::Present
        } else {
            ResumeSupport::Absent
        },
        etag,
        last_modified,
        content_type,
        suggested_filename,
        supports_range_split: accept_ranges && total_bytes.unwrap_or(0) >= 256 * 1024,
    })
}

pub fn build_initial_segments(total_bytes: u64, sections: u32, mirrors: &[String]) -> Vec<SegmentProgress> {
    let n = sections.max(1).min(32);
    let chunk = total_bytes / n as u64;
    let mut out = Vec::with_capacity(n as usize);
    let mut offset = 0u64;
    for i in 0..n {
        let end = if i == n - 1 {
            total_bytes.saturating_sub(1)
        } else {
            (offset + chunk).saturating_sub(1)
        };
        let mirror_url = if mirrors.is_empty() {
            None
        } else {
            Some(mirrors[i as usize % mirrors.len()].clone())
        };
        out.push(SegmentProgress {
            id: i,
            start: offset,
            end,
            downloaded: 0,
            state: SegmentState::Pending,
            speed_bps: 0,
            mirror_url,
        });
        offset = end + 1;
    }
    out
}

pub async fn download_http_or_ftp<F>(
    mut record: DownloadRecord,
    config: &FdmConfig,
    ssl_exceptions: &[SslException],
    cred: Option<AuthCredential>,
    limiter: BandwidthLimiter,
    mut control_rx: watch::Receiver<ControlCommand>,
    mut on_progress: F,
) -> DownloadRecord
where
    F: FnMut(&DownloadRecord) + Send,
{
    if record.url.starts_with("ftp://") || record.url.starts_with("ftps://") {
        return download_ftp(
            record,
            config,
            ssl_exceptions,
            cred,
            limiter,
            control_rx,
            on_progress,
        )
        .await;
    }

    let client = match build_http_client(
        config,
        ssl_exceptions,
        record.http.user_agent.as_deref(),
    ) {
        Ok(c) => c,
        Err(e) => {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            record.abort_reason = DownloadAbortReason::NetworkError;
            return record;
        }
    };

    record.state = DownloadState::Starting;
    on_progress(&record);

    // Retry wrapper honoring auto_retry_failed + disable_auto_retry + max_auto_retries
    let max_attempts = if config.core.auto_retry_failed && !record.flags.disable_auto_retry {
        config.core.max_auto_retries.max(1) + 1
    } else {
        1
    };

    for attempt in 0..max_attempts {
        if matches!(*control_rx.borrow(), ControlCommand::Pause) {
            record.state = DownloadState::Paused;
            return record;
        }
        if matches!(*control_rx.borrow(), ControlCommand::Cancel) {
            record.state = DownloadState::Stopped;
            record.abort_reason = DownloadAbortReason::AbortedByUser;
            return record;
        }

        let probe = probe_http_resource(&client, &record, ssl_exceptions, cred.as_ref()).await;
        let probe = match probe {
            Ok(p) => p,
            Err(e) => {
                record.last_error = Some(e.to_string());
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
                    continue;
                }
                record.state = DownloadState::Error;
                record.abort_reason = DownloadAbortReason::NetworkError;
                return record;
            }
        };

        // Detect remote resource change if previously probed
        let remote_changed = record
            .etag
            .as_deref()
            .zip(probe.etag.as_deref())
            .map(|(old, new)| old != new)
            .unwrap_or(false)
            || record
                .total_bytes
                .zip(probe.total_bytes)
                .map(|(old, new)| old != new)
                .unwrap_or(false);
        if remote_changed && (!record.segments.is_empty() || record.downloaded_bytes > 0) {
            if config.core.auto_restart_if_remote_changed {
                record.segments.clear();
                record.downloaded_bytes = 0;
                let _ = std::fs::remove_file(record.partial_path());
            } else {
                record.state = DownloadState::Error;
                record.abort_reason = DownloadAbortReason::RemoteResourceChanged;
                record.last_error = Some("Remote resource changed on server".into());
                record.connections_active = 0;
                record.download_speed_bps = 0;
                on_progress(&record);
                return record;
            }
        }

        if let Some(ref name) = probe.suggested_filename {
            if record.file_name.is_empty() || record.file_name == "download.bin" {
                record.file_name = name.clone();
                record.title = name.clone();
            }
        }
        record.file_type = classify_file_type(&record.file_name, probe.content_type.as_deref());
        record.total_bytes = probe.total_bytes;
        record.resume_support = probe.resume_support;
        record.etag = probe.etag.clone();
        record.last_modified = probe.last_modified.clone();

        if let Err(e) = std::fs::create_dir_all(&record.target_dir) {
            record.state = DownloadState::Error;
            record.last_error = Some(format!("Create dir failed: {e}"));
            return record;
        }

        let partial_path = record.partial_path();
        let output_path = record.output_path();
        let profile = config.active_profile();

        let result = if probe.supports_range_split
            && record.http.post_data.is_none()
            && profile.max_connections_per_server > 1
        {
            download_multi_section(
                &client,
                &mut record,
                &partial_path,
                profile.max_connections_per_server,
                cred.as_ref(),
                config,
                &limiter,
                &mut control_rx,
                &mut on_progress,
            )
            .await
        } else {
            download_single_stream(
                &client,
                &mut record,
                &partial_path,
                &probe,
                cred.as_ref(),
                config,
                &limiter,
                &mut control_rx,
                &mut on_progress,
            )
            .await
        };

        match result {
            Ok(Outcome::Completed) => {
                if partial_path != output_path {
                    if output_path.exists() {
                        let _ = std::fs::remove_file(&output_path);
                    }
                    if let Err(e) = std::fs::rename(&partial_path, &output_path) {
                        record.state = DownloadState::Error;
                        record.last_error = Some(format!("Finalize rename failed: {e}"));
                        return record;
                    }
                }
                if config.core.take_last_modified_from_server {
                    if let Some(ref lm) = record.last_modified {
                        if let Ok(st) = httpdate::parse_http_date(lm) {
                            let ft = filetime::FileTime::from_system_time(st);
                            let _ = filetime::set_file_mtime(&output_path, ft);
                        }
                    }
                }
                if config.core.set_mark_of_the_web {
                    let _ = set_mark_of_the_web(&output_path, &record.url, "FerriteDM");
                }
                record.state = DownloadState::Completed;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                record.eta_secs = Some(0);
                record.completed_at = Some(chrono::Utc::now());
                record.updated_at = chrono::Utc::now();
                let algo_val = sha256_str(&record.url);
                record.files = vec![DownloadFileEntry {
                    index: 0,
                    relative_path: record.file_name.clone(),
                    size_bytes: record.total_bytes.unwrap_or(record.downloaded_bytes),
                    downloaded_bytes: record.downloaded_bytes,
                    priority: DownloadPriority::Normal,
                    media_ready_to_play: true,
                    expected_hash: None,
                    computed_hash: None,
                }];
                let _ = algo_val;
                on_progress(&record);
                return record;
            }
            Ok(Outcome::Paused) => {
                record.state = DownloadState::Paused;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                on_progress(&record);
                return record;
            }
            Ok(Outcome::Cancelled) => {
                record.state = DownloadState::Stopped;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                record.abort_reason = DownloadAbortReason::AbortedByUser;
                on_progress(&record);
                return record;
            }
            Err(err_info) => {
                record.retry_count += 1;
                record.last_error = Some(err_info.message.clone());
                record.abort_reason = err_info.reason;
                record.connections_active = 0;
                record.download_speed_bps = 0;
                if err_info.unwanted || attempt + 1 >= max_attempts {
                    record.state = DownloadState::Error;
                    on_progress(&record);
                    return record;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
            }
        }
    }

    record
}

enum Outcome {
    Completed,
    Paused,
    Cancelled,
}

struct DownloadFailure {
    message: String,
    reason: DownloadAbortReason,
    unwanted: bool,
}

async fn download_multi_section<F>(
    client: &Client,
    record: &mut DownloadRecord,
    partial_path: &Path,
    sections_limit: u32,
    cred: Option<&AuthCredential>,
    config: &FdmConfig,
    limiter: &BandwidthLimiter,
    control_rx: &mut watch::Receiver<ControlCommand>,
    on_progress: &mut F,
) -> std::result::Result<Outcome, DownloadFailure>
where
    F: FnMut(&DownloadRecord) + Send,
{
    let total = record.total_bytes.unwrap_or(0);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(partial_path)
        .map_err(|e| DownloadFailure {
            message: format!("Open partial file failed: {e}"),
            reason: DownloadAbortReason::NetworkError,
            unwanted: false,
        })?;
    file.set_len(total).map_err(|e| DownloadFailure {
        message: format!("Preallocate {} bytes failed: {e}", total),
        reason: DownloadAbortReason::NetworkError,
        unwanted: false,
    })?;
    let file = Arc::new(file);

    let mut urls_pool = vec![record.url.clone()];
    urls_pool.extend(record.http.mirrors.clone());

    if record.segments.is_empty()
        || record
            .segments
            .last()
            .map(|s| s.end + 1 != total)
            .unwrap_or(true)
    {
        record.segments = build_initial_segments(total, sections_limit, &urls_pool);
    }
    let already_done: u64 = record.segments.iter().map(|s| s.downloaded).sum();
    record.downloaded_bytes = already_done;
    record.state = DownloadState::Downloading;

    let shared_segments = Arc::new(Mutex::new(record.segments.clone()));
    let total_downloaded = Arc::new(AtomicU64::new(already_done));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let pause_flag = Arc::new(AtomicBool::new(false));
    let failed_err: Arc<Mutex<Option<DownloadFailure>>> = Arc::new(Mutex::new(None));

    let base_headers = build_request_headers(record, cred);
    let urls_pool = Arc::new(urls_pool);

    let mut handles = Vec::new();
    let num_workers = record.segments.len();

    for idx in 0..num_workers {
        let client = client.clone();
        let file = Arc::clone(&file);
        let shared_segments = Arc::clone(&shared_segments);
        let total_downloaded = Arc::clone(&total_downloaded);
        let cancel_flag = Arc::clone(&cancel_flag);
        let pause_flag = Arc::clone(&pause_flag);
        let failed_err = Arc::clone(&failed_err);
        let headers = base_headers.clone();
        let limiter = limiter.clone();
        let urls_pool = Arc::clone(&urls_pool);
        let detect_unwanted = config.core.detect_unwanted_errors;

        handles.push(tokio::spawn(async move {
            loop {
                if cancel_flag.load(Ordering::Relaxed) || pause_flag.load(Ordering::Relaxed) {
                    break;
                }
                // Pick this worker's segment first, else steal half of largest pending/slow remainder!
                let job = {
                    let mut segs = shared_segments.lock().unwrap();
                    let mine = &mut segs[idx];
                    if mine.start + mine.downloaded <= mine.end {
                        mine.state = SegmentState::Active;
                        let start_at = mine.start + mine.downloaded;
                        let end_at = mine.end;
                        let u = mine
                            .mirror_url
                            .clone()
                            .unwrap_or_else(|| urls_pool[0].clone());
                        Some((idx, start_at, end_at, u))
                    } else {
                        // Work-stealing: find segment with > 512 KiB left and split its tail!
                        let mut best: Option<(usize, u64)> = None;
                        for (j, s) in segs.iter().enumerate() {
                            let cur = s.start + s.downloaded;
                            if cur <= s.end {
                                let rem = s.end - cur + 1;
                                if rem >= 512 * 1024 && best.map(|b| rem > b.1).unwrap_or(true) {
                                    best = Some((j, rem));
                                }
                            }
                        }
                        if let Some((victim_idx, rem)) = best {
                            let victim_end = segs[victim_idx].end;
                            let cur = segs[victim_idx].start + segs[victim_idx].downloaded;
                            let split_point = cur + (rem / 2);
                            segs[victim_idx].end = split_point - 1;
                            let stolen_id = segs.len() as u32;
                            let u = urls_pool[stolen_id as usize % urls_pool.len()].clone();
                            segs.push(SegmentProgress {
                                id: stolen_id,
                                start: split_point,
                                end: victim_end,
                                downloaded: 0,
                                state: SegmentState::Active,
                                speed_bps: 0,
                                mirror_url: Some(u.clone()),
                            });
                            Some((segs.len() - 1, split_point, victim_end, u))
                        } else {
                            None
                        }
                    }
                };

                let Some((seg_idx, range_start, _orig_end, target_url)) = job else {
                    break;
                };

                // Execute HTTP Range GET (fallback to primary URL if mirror fails once)
                let try_urls = if target_url != urls_pool[0] {
                    vec![target_url, urls_pool[0].clone()]
                } else {
                    vec![target_url]
                };

                let mut seg_ok = false;
                for try_url in try_urls {
                    let current_end = {
                        let segs = shared_segments.lock().unwrap();
                        segs[seg_idx].end
                    };
                    if range_start > current_end {
                        seg_ok = true;
                        break;
                    }
                    let mut h = headers.clone();
                    if let Ok(hv) = HeaderValue::from_str(&format!("bytes={range_start}-{current_end}")) {
                        h.insert(RANGE, hv);
                    }
                    let resp = match client.get(&try_url).headers(h).send().await {
                        Ok(r) => r,
                        Err(e) => {
                            if try_url == urls_pool[0] {
                                let mut g = failed_err.lock().unwrap();
                                if g.is_none() {
                                    *g = Some(DownloadFailure {
                                        message: format!("Section {seg_idx} request error: {e}"),
                                        reason: DownloadAbortReason::NetworkError,
                                        unwanted: false,
                                    });
                                }
                            }
                            continue;
                        }
                    };

                    let st = resp.status();
                    if !(st == StatusCode::PARTIAL_CONTENT || st == StatusCode::OK) {
                        let unwanted = detect_unwanted
                            && matches!(
                                st,
                                StatusCode::UNAUTHORIZED
                                    | StatusCode::FORBIDDEN
                                    | StatusCode::NOT_FOUND
                                    | StatusCode::GONE
                                    | StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
                            );
                        if unwanted || try_url == urls_pool[0] {
                            let mut g = failed_err.lock().unwrap();
                            if g.is_none() {
                                *g = Some(DownloadFailure {
                                    message: format!("HTTP status {} on section {}", st, seg_idx),
                                    reason: if unwanted {
                                        DownloadAbortReason::UnwantedError
                                    } else {
                                        DownloadAbortReason::NetworkError
                                    },
                                    unwanted,
                                });
                            }
                        }
                        if unwanted {
                            cancel_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                        continue;
                    }

                    let mut stream = resp.bytes_stream();
                    let mut offset = range_start;
                    let mut chunk_err = false;
                    while let Some(item) = stream.next().await {
                        if cancel_flag.load(Ordering::Relaxed) || pause_flag.load(Ordering::Relaxed)
                        {
                            break;
                        }
                        let bytes = match item {
                            Ok(b) => b,
                            Err(e) => {
                                let mut g = failed_err.lock().unwrap();
                                if g.is_none() {
                                    *g = Some(DownloadFailure {
                                        message: format!("Stream read error: {e}"),
                                        reason: DownloadAbortReason::NetworkError,
                                        unwanted: false,
                                    });
                                }
                                chunk_err = true;
                                break;
                            }
                        };
                        let cur_end = {
                            let segs = shared_segments.lock().unwrap();
                            segs[seg_idx].end
                        };
                        if offset > cur_end {
                            break;
                        }
                        let allowed_len = ((cur_end - offset + 1) as usize).min(bytes.len());
                        if allowed_len == 0 {
                            break;
                        }
                        limiter.acquire(allowed_len).await;
                        let slice = &bytes[..allowed_len];
                        if let Err(e) = write_at_exact(&file, slice, offset) {
                            let mut g = failed_err.lock().unwrap();
                            if g.is_none() {
                                *g = Some(DownloadFailure {
                                    message: format!("Disk write failed: {e}"),
                                    reason: DownloadAbortReason::NetworkError,
                                    unwanted: false,
                                });
                            }
                            cancel_flag.store(true, Ordering::Relaxed);
                            chunk_err = true;
                            break;
                        }
                        offset += allowed_len as u64;
                        total_downloaded.fetch_add(allowed_len as u64, Ordering::Relaxed);
                        {
                            let mut segs = shared_segments.lock().unwrap();
                            let s = &mut segs[seg_idx];
                            s.downloaded = offset.saturating_sub(s.start);
                            if offset > s.end {
                                s.state = SegmentState::Completed;
                            }
                        }
                        if offset > cur_end {
                            break;
                        }
                    }

                    if !chunk_err {
                        seg_ok = true;
                        break;
                    }
                }

                if !seg_ok {
                    let mut segs = shared_segments.lock().unwrap();
                    if segs[seg_idx].start + segs[seg_idx].downloaded <= segs[seg_idx].end {
                        segs[seg_idx].state = SegmentState::Failed;
                    }
                    break;
                }
            }
        }));
    }

    // Monitor loop: compute speeds, enforce slow-download watchdog, propagate pause/cancel
    let mut last_bytes = already_done;
    let mut last_tick = tokio::time::Instant::now();
    let mut slow_accum_secs = 0f64;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
            changed = control_rx.changed() => {
                if changed.is_ok() {
                    match *control_rx.borrow() {
                        ControlCommand::Pause => {
                            pause_flag.store(true, Ordering::Relaxed);
                        }
                        ControlCommand::Cancel => {
                            cancel_flag.store(true, Ordering::Relaxed);
                        }
                        ControlCommand::Run => {}
                    }
                }
            }
        }

        let now = tokio::time::Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64().max(0.05);
        let cur = total_downloaded.load(Ordering::Relaxed);
        let delta = cur.saturating_sub(last_bytes);
        let speed = (delta as f64 / dt) as u64;
        last_bytes = cur;
        last_tick = now;

        {
            let segs = shared_segments.lock().unwrap();
            record.segments = segs.clone();
        }
        record.downloaded_bytes = cur.min(total);
        record.download_speed_bps = speed;
        record.connections_active = record
            .segments
            .iter()
            .filter(|s| s.state == SegmentState::Active)
            .count() as u32;
        record.eta_secs = if speed > 0 {
            Some(total.saturating_sub(cur) / speed)
        } else {
            None
        };
        record.updated_at = chrono::Utc::now();
        on_progress(record);

        if config.core.detect_unacceptably_slow && !pause_flag.load(Ordering::Relaxed) {
            if speed < config.core.unacceptably_slow_speed_bps && cur < total {
                slow_accum_secs += dt;
                if slow_accum_secs >= config.core.unacceptably_slow_interval_secs as f64 {
                    cancel_flag.store(true, Ordering::Relaxed);
                    for h in handles {
                        let _ = h.await;
                    }
                    return Err(DownloadFailure {
                        message: format!(
                            "Speed dropped below {} B/s for {}s",
                            config.core.unacceptably_slow_speed_bps,
                            config.core.unacceptably_slow_interval_secs
                        ),
                        reason: DownloadAbortReason::SlowDownload,
                        unwanted: false,
                    });
                }
            } else {
                slow_accum_secs = 0.0;
            }
        }

        if handles.iter().all(|h| h.is_finished()) {
            break;
        }
        if pause_flag.load(Ordering::Relaxed) || cancel_flag.load(Ordering::Relaxed) {
            for h in handles {
                let _ = h.await;
            }
            break;
        }
    }

    {
        let segs = shared_segments.lock().unwrap();
        record.segments = segs.clone();
    }
    let _ = file.sync_all();

    if matches!(*control_rx.borrow(), ControlCommand::Cancel) {
        return Ok(Outcome::Cancelled);
    }
    if matches!(*control_rx.borrow(), ControlCommand::Pause) {
        return Ok(Outcome::Paused);
    }
    if let Some(err) = failed_err.lock().unwrap().take() {
        return Err(err);
    }
    if total_downloaded.load(Ordering::Relaxed) >= total {
        record.downloaded_bytes = total;
        Ok(Outcome::Completed)
    } else {
        Err(DownloadFailure {
            message: "Download ended incomplete".into(),
            reason: DownloadAbortReason::NetworkError,
            unwanted: false,
        })
    }
}

async fn download_single_stream<F>(
    client: &Client,
    record: &mut DownloadRecord,
    partial_path: &Path,
    probe: &ProbeResult,
    cred: Option<&AuthCredential>,
    config: &FdmConfig,
    limiter: &BandwidthLimiter,
    control_rx: &mut watch::Receiver<ControlCommand>,
    on_progress: &mut F,
) -> std::result::Result<Outcome, DownloadFailure>
where
    F: FnMut(&DownloadRecord) + Send,
{
    let existing_len = if probe.resume_support == ResumeSupport::Present && partial_path.exists() {
        std::fs::metadata(partial_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    let mut headers = build_request_headers(record, cred);
    if existing_len > 0 {
        if let Ok(hv) = HeaderValue::from_str(&format!("bytes={existing_len}-")) {
            headers.insert(RANGE, hv);
        }
    }

    let req = if let Some(ref body) = record.http.post_data {
        client
            .post(&record.url)
            .headers(headers)
            .body(body.clone())
    } else {
        client.get(&record.url).headers(headers)
    };

    let resp = req.send().await.map_err(|e| DownloadFailure {
        message: format!("HTTP request failed: {e}"),
        reason: DownloadAbortReason::NetworkError,
        unwanted: false,
    })?;

    let st = resp.status();
    if !st.is_success() && st != StatusCode::PARTIAL_CONTENT {
        let unwanted = config.core.detect_unwanted_errors
            && matches!(
                st,
                StatusCode::UNAUTHORIZED
                    | StatusCode::FORBIDDEN
                    | StatusCode::NOT_FOUND
                    | StatusCode::GONE
            );
        return Err(DownloadFailure {
            message: format!("HTTP error status {}", st),
            reason: if unwanted {
                DownloadAbortReason::UnwantedError
            } else {
                DownloadAbortReason::NetworkError
            },
            unwanted,
        });
    }

    let append_offset = if st == StatusCode::PARTIAL_CONTENT {
        existing_len
    } else {
        0
    };
    let file = if append_offset > 0 {
        OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(partial_path)
    } else {
        File::create(partial_path)
    }
    .map_err(|e| DownloadFailure {
        message: format!("Open partial file failed: {e}"),
        reason: DownloadAbortReason::NetworkError,
        unwanted: false,
    })?;

    let mut written = append_offset;
    record.state = DownloadState::Downloading;
    record.connections_active = 1;
    record.downloaded_bytes = written;
    record.segments = vec![SegmentProgress {
        id: 0,
        start: 0,
        end: record.total_bytes.unwrap_or(0).saturating_sub(1),
        downloaded: written,
        state: SegmentState::Active,
        speed_bps: 0,
        mirror_url: Some(record.url.clone()),
    }];
    on_progress(record);

    let mut stream = resp.bytes_stream();
    let mut last_emit = tokio::time::Instant::now();
    let mut last_speed_check = last_emit;
    let mut bytes_in_window = 0u64;
    let mut slow_accum_secs = 0.0f64;
    let mut watchdog = tokio::time::interval_at(
        last_emit + std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(50),
    );
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        if matches!(*control_rx.borrow(), ControlCommand::Pause) {
            record.downloaded_bytes = written;
            if let Some(s0) = record.segments.get_mut(0) {
                s0.downloaded = written;
            }
            let _ = file.sync_all();
            return Ok(Outcome::Paused);
        }
        if matches!(*control_rx.borrow(), ControlCommand::Cancel) {
            record.downloaded_bytes = written;
            if let Some(s0) = record.segments.get_mut(0) {
                s0.downloaded = written;
            }
            return Ok(Outcome::Cancelled);
        }

        tokio::select! {
            _ = watchdog.tick() => {
                let now = tokio::time::Instant::now();
                let speed_dt = now.duration_since(last_speed_check).as_secs_f64();
                let mut should_emit = now.duration_since(last_emit) >= std::time::Duration::from_millis(50);
                if speed_dt >= 0.25 {
                    let speed = (bytes_in_window as f64 / speed_dt) as u64;
                    bytes_in_window = 0;
                    last_speed_check = now;
                    record.download_speed_bps = speed;
                    if let Some(s0) = record.segments.get_mut(0) {
                        s0.speed_bps = speed;
                    }
                    record.eta_secs = record.total_bytes.and_then(|tot| {
                        if speed > 0 { Some(tot.saturating_sub(written) / speed) } else { None }
                    });
                    if config.core.detect_unacceptably_slow {
                        if speed < config.core.unacceptably_slow_speed_bps {
                            slow_accum_secs += speed_dt;
                            if slow_accum_secs >= config.core.unacceptably_slow_interval_secs as f64 {
                                return Err(DownloadFailure {
                                    message: format!(
                                        "Speed dropped below {} B/s for {}s",
                                        config.core.unacceptably_slow_speed_bps,
                                        config.core.unacceptably_slow_interval_secs
                                    ),
                                    reason: DownloadAbortReason::SlowDownload,
                                    unwanted: false,
                                });
                            }
                        } else {
                            slow_accum_secs = 0.0;
                        }
                    }
                    should_emit = true;
                }
                if should_emit {
                    last_emit = now;
                    on_progress(record);
                }
            }
            _ = control_rx.changed() => {
                match *control_rx.borrow() {
                    ControlCommand::Pause => {
                        record.downloaded_bytes = written;
                        if let Some(s0) = record.segments.get_mut(0) {
                            s0.downloaded = written;
                        }
                        let _ = file.sync_all();
                        return Ok(Outcome::Paused);
                    }
                    ControlCommand::Cancel => {
                        record.downloaded_bytes = written;
                        if let Some(s0) = record.segments.get_mut(0) {
                            s0.downloaded = written;
                        }
                        return Ok(Outcome::Cancelled);
                    }
                    ControlCommand::Run => {}
                }
            }
            next_chunk = stream.next() => {
                match next_chunk {
                    Some(Ok(chunk)) => {
                        limiter.acquire(chunk.len()).await;
                        write_at_exact(&file, &chunk, written).map_err(|e| DownloadFailure {
                            message: format!("Disk write error: {e}"),
                            reason: DownloadAbortReason::NetworkError,
                            unwanted: false,
                        })?;
                        written += chunk.len() as u64;
                        bytes_in_window += chunk.len() as u64;
                        record.downloaded_bytes = written;
                        if let Some(s0) = record.segments.get_mut(0) {
                            s0.downloaded = written;
                        }
                        let now = tokio::time::Instant::now();
                        let speed_dt = now.duration_since(last_speed_check).as_secs_f64();
                        let mut should_emit = now.duration_since(last_emit) >= std::time::Duration::from_millis(50);
                        if speed_dt >= 0.25 {
                            let speed = (bytes_in_window as f64 / speed_dt) as u64;
                            bytes_in_window = 0;
                            last_speed_check = now;
                            if config.core.detect_unacceptably_slow {
                                if speed < config.core.unacceptably_slow_speed_bps {
                                    slow_accum_secs += speed_dt;
                                    if slow_accum_secs >= config.core.unacceptably_slow_interval_secs as f64 {
                                        return Err(DownloadFailure {
                                            message: format!(
                                                "Speed dropped below {} B/s for {}s",
                                                config.core.unacceptably_slow_speed_bps,
                                                config.core.unacceptably_slow_interval_secs
                                            ),
                                            reason: DownloadAbortReason::SlowDownload,
                                            unwanted: false,
                                        });
                                    }
                                } else {
                                    slow_accum_secs = 0.0;
                                }
                            }
                            record.download_speed_bps = speed;
                            if let Some(s0) = record.segments.get_mut(0) {
                                s0.speed_bps = speed;
                            }
                            record.eta_secs = record.total_bytes.and_then(|tot| {
                                if speed > 0 { Some(tot.saturating_sub(written) / speed) } else { None }
                            });
                            should_emit = true;
                        }
                        if should_emit {
                            last_emit = now;
                            on_progress(record);
                        }
                    }
                    Some(Err(e)) => {
                        return Err(DownloadFailure {
                            message: format!("Stream read error: {e}"),
                            reason: DownloadAbortReason::NetworkError,
                            unwanted: false,
                        });
                    }
                    None => break,
                }
            }
        }
    }

    let _ = file.sync_all();
    record.downloaded_bytes = written;
    if record.total_bytes.is_none() {
        record.total_bytes = Some(written);
    }
    if let Some(s0) = record.segments.get_mut(0) {
        s0.downloaded = written;
        s0.state = SegmentState::Completed;
    }
    Ok(Outcome::Completed)
}

fn write_at_exact(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = file.write_at(buf, offset)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "write_at returned 0",
            ));
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

async fn connect_ftp_stream(
    parsed: &Url,
    ssl_exceptions: &[SslException],
    cred: Option<&AuthCredential>,
) -> Result<AsyncRustlsFtpStream> {
    let host = parsed.host_str().unwrap_or("localhost");
    let is_ftps = parsed.scheme().eq_ignore_ascii_case("ftps");
    let port = parsed.port().unwrap_or(21);
    let user = cred
        .map(|c| c.username.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if parsed.username().is_empty() {
                "anonymous"
            } else {
                parsed.username()
            }
        });
    let pass = cred
        .map(|c| c.password.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| parsed.password().unwrap_or("ferrite@example.com"));

    let mut ftp = AsyncRustlsFtpStream::connect(format!("{host}:{port}")).await?;
    if is_ftps {
        let mut tls_cfg = build_rustls_client_config(ssl_exceptions).unwrap_or_else(|| {
            ensure_crypto_provider();
            rustls::ClientConfig::builder()
                .with_root_certificates(load_native_root_store())
                .with_no_client_auth()
        });
        tls_cfg.alpn_protocols.clear();
        let connector = AsyncRustlsConnector::from(suppaftp::tokio_rustls::TlsConnector::from(
            Arc::new(tls_cfg),
        ));
        ftp = ftp.into_secure(connector, host).await?;
    }
    ftp.login(user, pass).await?;
    Ok(ftp)
}

async fn probe_ftp_resource(
    url_str: &str,
    ssl_exceptions: &[SslException],
    cred: Option<&AuthCredential>,
) -> Result<ProbeResult> {
    let parsed = Url::parse(url_str)?;
    let path = parsed.path().to_string();

    let mut ftp = connect_ftp_stream(&parsed, ssl_exceptions, cred).await?;
    let size = ftp.size(&path).await.ok().map(|s| s as u64);
    let lm = ftp.mdtm(&path).await.ok().map(|dt| dt.to_string());
    let _ = ftp.quit().await;

    Ok(ProbeResult {
        final_url: url_str.to_string(),
        total_bytes: size,
        resume_support: ResumeSupport::Present,
        etag: None,
        last_modified: lm,
        content_type: None,
        suggested_filename: Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(sanitize_filename),
        supports_range_split: false,
    })
}

async fn download_ftp<F>(
    mut record: DownloadRecord,
    config: &FdmConfig,
    ssl_exceptions: &[SslException],
    cred: Option<AuthCredential>,
    limiter: BandwidthLimiter,
    mut control_rx: watch::Receiver<ControlCommand>,
    mut on_progress: F,
) -> DownloadRecord
where
    F: FnMut(&DownloadRecord) + Send,
{
    let Ok(parsed) = Url::parse(&record.url) else {
        record.state = DownloadState::Error;
        record.abort_reason = DownloadAbortReason::InvalidUrl;
        record.last_error = Some("Invalid FTP URL".into());
        return record;
    };
    let remote_path = parsed.path().to_string();

    record.state = DownloadState::Starting;
    on_progress(&record);

    let max_attempts = if config.core.auto_retry_failed && !record.flags.disable_auto_retry {
        config.core.max_auto_retries.max(1) + 1
    } else {
        1
    };

    for attempt in 0..max_attempts {
        let mut ftp = match connect_ftp_stream(&parsed, ssl_exceptions, cred.as_ref()).await {
            Ok(f) => f,
            Err(e) => {
                record.last_error = Some(format!("FTP connect/login error: {e}"));
                record.abort_reason = DownloadAbortReason::NetworkError;
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
                    continue;
                }
                record.state = DownloadState::Error;
                return record;
            }
        };
        let _ = ftp.transfer_type(suppaftp::types::FileType::Binary).await;
        if let Ok(sz) = ftp.size(&remote_path).await {
            record.total_bytes = Some(sz as u64);
        }

        let _ = std::fs::create_dir_all(&record.target_dir);
        let partial_path = record.partial_path();
        let offset = std::fs::metadata(&partial_path).map(|m| m.len()).unwrap_or(0);
        if offset > 0 {
            let _ = ftp.resume_transfer(offset as usize).await;
        }
        let mut reader = match ftp.retr_as_stream(&remote_path).await {
            Ok(r) => r,
            Err(e) => {
                record.last_error = Some(format!("FTP RETR error: {e}"));
                record.abort_reason = DownloadAbortReason::NetworkError;
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
                    continue;
                }
                record.state = DownloadState::Error;
                return record;
            }
        };

        let file = if offset > 0 {
            OpenOptions::new().create(true).write(true).open(&partial_path)
        } else {
            File::create(&partial_path)
        };
        let file = match file {
            Ok(f) => f,
            Err(e) => {
                record.state = DownloadState::Error;
                record.last_error = Some(format!("Open file failed: {e}"));
                return record;
            }
        };

        let mut written = offset;
        record.state = DownloadState::Downloading;
        record.connections_active = 1;
        let mut buf = vec![0u8; 64 * 1024];
        let mut last_tick = tokio::time::Instant::now();
        let mut bytes_since_tick = 0u64;
        let mut slow_accum_secs = 0.0f64;
        let mut watchdog = tokio::time::interval(std::time::Duration::from_millis(250));
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut failed_retryable = false;
        let mut eof = false;
        while !eof {
            if matches!(*control_rx.borrow(), ControlCommand::Pause) {
                record.state = DownloadState::Paused;
                record.connections_active = 0;
                record.download_speed_bps = 0;
                on_progress(&record);
                return record;
            }
            if matches!(*control_rx.borrow(), ControlCommand::Cancel) {
                record.state = DownloadState::Stopped;
                record.connections_active = 0;
                record.download_speed_bps = 0;
                record.abort_reason = DownloadAbortReason::AbortedByUser;
                on_progress(&record);
                return record;
            }
            tokio::select! {
                _ = watchdog.tick() => {
                    let now = tokio::time::Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64();
                    if dt >= 0.25 {
                        let speed = (bytes_since_tick as f64 / dt) as u64;
                        bytes_since_tick = 0;
                        last_tick = now;
                        record.download_speed_bps = speed;
                        record.eta_secs = record.total_bytes.and_then(|tot| {
                            if speed > 0 { Some(tot.saturating_sub(written) / speed) } else { None }
                        });
                        on_progress(&record);
                        if config.core.detect_unacceptably_slow {
                            if speed < config.core.unacceptably_slow_speed_bps {
                                slow_accum_secs += dt;
                                if slow_accum_secs >= config.core.unacceptably_slow_interval_secs as f64 {
                                    record.abort_reason = DownloadAbortReason::SlowDownload;
                                    record.last_error = Some(format!(
                                        "FTP speed dropped below {} B/s for {}s",
                                        config.core.unacceptably_slow_speed_bps,
                                        config.core.unacceptably_slow_interval_secs
                                    ));
                                    failed_retryable = true;
                                    break;
                                }
                            } else {
                                slow_accum_secs = 0.0;
                            }
                        }
                    }
                }
                _ = control_rx.changed() => {}
                res = reader.read(&mut buf) => {
                    match res {
                        Ok(0) => {
                            eof = true;
                        }
                        Ok(n) => {
                            limiter.acquire(n).await;
                            if let Err(e) = write_at_exact(&file, &buf[..n], written) {
                                record.state = DownloadState::Error;
                                record.connections_active = 0;
                                record.download_speed_bps = 0;
                                record.last_error = Some(format!("Disk write error: {e}"));
                                return record;
                            }
                            written += n as u64;
                            bytes_since_tick += n as u64;
                            record.downloaded_bytes = written;
                        }
                        Err(e) => {
                            record.abort_reason = DownloadAbortReason::NetworkError;
                            record.last_error = Some(format!("FTP stream error: {e}"));
                            failed_retryable = true;
                            break;
                        }
                    }
                }
            }
        }

        if failed_retryable {
            record.retry_count += 1;
            record.connections_active = 0;
            record.download_speed_bps = 0;
            if attempt + 1 < max_attempts {
                tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
                continue;
            }
            record.state = DownloadState::Error;
            on_progress(&record);
            return record;
        }

        let _ = reader.finish().await;
        let _ = ftp.quit().await;

        let output_path = record.output_path();
        if partial_path != output_path {
            let _ = std::fs::rename(&partial_path, &output_path);
        }
        if config.core.set_mark_of_the_web {
            let _ = set_mark_of_the_web(&output_path, &record.url, "FerriteDM");
        }
        record.state = DownloadState::Completed;
        record.connections_active = 0;
        record.download_speed_bps = 0;
        record.completed_at = Some(chrono::Utc::now());
        on_progress(&record);
        return record;
    }

    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use fdm_core::{DownloadFlags, HttpRequestOptions, TrafficUsageMode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn parses_content_disposition_variants() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename*=UTF-8''my%20file.zip")
                .as_deref(),
            Some("my file.zip")
        );
        assert_eq!(
            parse_content_disposition_filename("inline; filename=\"report-2026.pdf\"").as_deref(),
            Some("report-2026.pdf")
        );
    }

    #[tokio::test]
    async fn multi_section_range_download_and_sha256_verify() {
        // Generate 600 KiB deterministic payload (> 256 KiB triggers 4-section split!)
        let payload: Arc<Vec<u8>> = Arc::new((0..(600 * 1024)).map(|i| (i % 251) as u8).collect());
        let expected_hash = {
            let mut h = Sha256::new();
            h.update(&*payload);
            hex::encode(h.finalize())
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload_srv = Arc::clone(&payload);

        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let data = Arc::clone(&payload_srv);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let is_head = req.starts_with("HEAD ");
                    let range_hdr = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split_once(':'))
                        .map(|(_, v)| v.trim().to_string());

                    if let Some(rh) = range_hdr {
                        if let Some(spec) = rh.strip_prefix("bytes=") {
                            if let Some((a, b)) = spec.split_once('-') {
                                let start: usize = a.parse().unwrap_or(0);
                                let end: usize = b
                                    .parse::<usize>()
                                    .unwrap_or(data.len() - 1)
                                    .min(data.len() - 1);
                                let slice = &data[start..=end];
                                let hdr = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"bundle.bin\"\r\nConnection: close\r\n\r\n",
                                    slice.len(),
                                    data.len()
                                );
                                let _ = sock.write_all(hdr.as_bytes()).await;
                                let _ = sock.write_all(slice).await;
                                return;
                            }
                        }
                    }

                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"bundle.bin\"\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    if !is_head {
                        let _ = sock.write_all(&data).await;
                    }
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_http_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.core.default_download_path = dir.clone();
        cfg.core.current_tum = TrafficUsageMode::Medium; // 8 connections max
        cfg.core.set_mark_of_the_web = false;

        let url = format!("http://{}/bundle.bin", addr);
        let rec = DownloadRecord {
            id: 1,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            children_ids: vec![],
            order_index: 1,
            download_type: fdm_core::DownloadType::Www,
            state: DownloadState::Queued,
            priority: DownloadPriority::Normal,
            resume_support: ResumeSupport::Unknown,
            flags: DownloadFlags::default(),
            title: "bundle.bin".into(),
            url: url.clone(),
            target_dir: dir.clone(),
            file_name: "bundle.bin".into(),
            partial_suffix: ".fdmdownload".into(),
            total_bytes: None,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections_active: 0,
            etag: None,
            last_modified: None,
            file_type: fdm_core::FileType::Other,
            http: HttpRequestOptions {
                url,
                ..Default::default()
            },
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
            abort_reason: DownloadAbortReason::None,
            preview_path: None,
            module_data: serde_json::Value::Null,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        };

        let (_tx, rx) = watch::channel(ControlCommand::Run);
        let limiter = BandwidthLimiter::new(0);
        let done = download_http_or_ftp(rec, &cfg, &[], None, limiter, rx, |_| {}).await;
        assert_eq!(done.state, DownloadState::Completed, "err={:?}", done.last_error);
        assert!(done.segments.len() >= 4, "expected >=4 sections");
        assert_eq!(done.downloaded_bytes, 600 * 1024);

        let actual_hash =
            fdm_core::hash::compute_file_hash(&dir.join("bundle.bin"), fdm_core::HashAlgorithm::Sha256)
                .unwrap();
        assert_eq!(actual_hash, expected_hash);

        srv_handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn make_test_record(url: String, dir: &Path, file_name: &str) -> DownloadRecord {
        DownloadRecord {
            id: 1,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            children_ids: vec![],
            order_index: 1,
            download_type: fdm_core::DownloadType::Www,
            state: DownloadState::Queued,
            priority: DownloadPriority::Normal,
            resume_support: ResumeSupport::Unknown,
            flags: DownloadFlags::default(),
            title: file_name.into(),
            url: url.clone(),
            target_dir: dir.to_path_buf(),
            file_name: file_name.into(),
            partial_suffix: ".fdmdownload".into(),
            total_bytes: None,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections_active: 0,
            etag: None,
            last_modified: None,
            file_type: fdm_core::FileType::Other,
            http: HttpRequestOptions {
                url,
                ..Default::default()
            },
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
            abort_reason: DownloadAbortReason::None,
            preview_path: None,
            module_data: serde_json::Value::Null,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn pause_and_resume_preserves_partial_and_completes() {
        let payload: Arc<Vec<u8>> = Arc::new((0..(128 * 1024)).map(|i| (i % 199) as u8).collect());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let payload_srv = Arc::clone(&payload);
        let saw_resume_range = Arc::new(AtomicBool::new(false));
        let saw_resume_srv = Arc::clone(&saw_resume_range);

        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let data = Arc::clone(&payload_srv);
                let saw_resume = Arc::clone(&saw_resume_srv);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let is_head = req.starts_with("HEAD ");
                    let range_hdr = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|l| l.split_once(':'))
                        .map(|(_, v)| v.trim().to_string());

                    if let Some(rh) = range_hdr {
                        if let Some(spec) = rh.strip_prefix("bytes=") {
                            if let Some((a, b)) = spec.split_once('-') {
                                let start: usize = a.parse().unwrap_or(0);
                                let end: usize = b
                                    .parse::<usize>()
                                    .unwrap_or(data.len() - 1)
                                    .min(data.len() - 1);
                                if start > 0 {
                                    saw_resume.store(true, Ordering::SeqCst);
                                }
                                let slice = &data[start..=end];
                                let hdr = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                    slice.len(),
                                    data.len()
                                );
                                let _ = sock.write_all(hdr.as_bytes()).await;
                                for chunk in slice.chunks(8192) {
                                    if sock.write_all(chunk).await.is_err() {
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_millis(12)).await;
                                }
                                return;
                            }
                        }
                    }

                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    if !is_head {
                        for chunk in data.chunks(8192) {
                            if sock.write_all(chunk).await.is_err() {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(12)).await;
                        }
                    }
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_http_pause_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.core.default_download_path = dir.clone();
        cfg.core.set_mark_of_the_web = false;

        let rec = make_test_record(format!("http://{addr}/paused.bin"), &dir, "paused.bin");
        let (tx, rx) = watch::channel(ControlCommand::Run);
        let paused_sent = Arc::new(AtomicBool::new(false));

        let paused = download_http_or_ftp(rec, &cfg, &[], None, BandwidthLimiter::new(0), rx, |p| {
            if p.downloaded_bytes >= 16384 && !paused_sent.swap(true, Ordering::SeqCst) {
                let _ = tx.send(ControlCommand::Pause);
            }
        })
        .await;
        assert_eq!(paused.state, DownloadState::Paused);
        assert!(dir.join("paused.bin.fdmdownload").exists());
        assert!(paused.downloaded_bytes > 0);
        assert!(paused.downloaded_bytes < payload.len() as u64);

        let (_tx2, rx2) = watch::channel(ControlCommand::Run);
        let completed =
            download_http_or_ftp(paused, &cfg, &[], None, BandwidthLimiter::new(0), rx2, |_| {})
                .await;
        assert_eq!(completed.state, DownloadState::Completed);
        assert!(saw_resume_range.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(dir.join("paused.bin")).unwrap(), *payload);

        srv_handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn unwanted_404_fails_fast_without_retrying() {
        let hits = Arc::new(AtomicU64::new(0));
        let hits_srv = Arc::clone(&hits);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let hits = Arc::clone(&hits_srv);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.starts_with("GET ") {
                        let range_hdr = req.lines().any(|l| l.to_ascii_lowercase().starts_with("range: bytes=0-0"));
                        if !range_hdr {
                            hits.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_http_404_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.core.auto_retry_failed = true;
        cfg.core.max_auto_retries = 5;
        cfg.core.detect_unwanted_errors = true;

        let rec = make_test_record(format!("http://{addr}/missing.zip"), &dir, "missing.zip");
        let (_tx, rx) = watch::channel(ControlCommand::Run);
        let done = download_http_or_ftp(rec, &cfg, &[], None, BandwidthLimiter::new(0), rx, |_| {})
            .await;

        assert_eq!(done.state, DownloadState::Error);
        assert_eq!(done.abort_reason, DownloadAbortReason::UnwantedError);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn system_default_user_agent_header_is_sent_when_enabled() {
        let seen_ua = Arc::new(Mutex::new(String::new()));
        let seen_ua_srv = Arc::clone(&seen_ua);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let seen_ua = Arc::clone(&seen_ua_srv);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if let Some(ua_line) = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("user-agent:"))
                    {
                        if let Some((_, val)) = ua_line.split_once(':') {
                            *seen_ua.lock().unwrap() = val.trim().to_string();
                        }
                    }
                    let resp = "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc";
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let mut cfg = FdmConfig::default();
        cfg.core.use_system_default_user_agent = true;
        let client = build_http_client(&cfg, &[], None).unwrap();
        let _ = client.get(format!("http://{addr}/ua")).send().await.unwrap();
        let captured = seen_ua.lock().unwrap().clone();
        assert_eq!(captured, fdm_core::os_integration::system_default_user_agent());

        srv.abort();
    }

    #[tokio::test]
    async fn disable_auto_retry_and_slow_download_watchdog_work() {
        let hits = Arc::new(AtomicU64::new(0));
        let hits_srv = Arc::clone(&hits);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let hits = Arc::clone(&hits_srv);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.starts_with("GET ") && req.contains("/err500") {
                        let range_hdr = req.lines().any(|l| l.to_ascii_lowercase().starts_with("range: bytes=0-0"));
                        if !range_hdr {
                            hits.fetch_add(1, Ordering::SeqCst);
                        }
                        let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = sock.write_all(resp.as_bytes()).await;
                    } else if req.contains("/stall") {
                        let is_head = req.starts_with("HEAD ");
                        let hdr = "HTTP/1.1 200 OK\r\nContent-Length: 10240\r\nConnection: close\r\n\r\n";
                        let _ = sock.write_all(hdr.as_bytes()).await;
                        if !is_head {
                            let _ = sock.write_all(&[1u8; 8]).await;
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_http_flags_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.core.auto_retry_failed = true;
        cfg.core.max_auto_retries = 3;
        cfg.core.set_mark_of_the_web = false;

        let mut rec = make_test_record(format!("http://{addr}/err500"), &dir, "err.bin");
        rec.flags.disable_auto_retry = true;
        let (_tx, rx) = watch::channel(ControlCommand::Run);
        let done = download_http_or_ftp(rec, &cfg, &[], None, BandwidthLimiter::new(0), rx, |_| {})
            .await;
        assert_eq!(done.state, DownloadState::Error);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Now verify single-stream slow download detection aborts stalled transfers.
        cfg.core.auto_retry_failed = false;
        cfg.core.detect_unacceptably_slow = true;
        cfg.core.unacceptably_slow_speed_bps = 10_000;
        cfg.core.unacceptably_slow_interval_secs = 1;
        let slow_rec = make_test_record(format!("http://{addr}/stall"), &dir, "stall.bin");
        let (_tx2, rx2) = watch::channel(ControlCommand::Run);
        let slow_done =
            download_http_or_ftp(slow_rec, &cfg, &[], None, BandwidthLimiter::new(0), rx2, |_| {})
                .await;
        assert_eq!(slow_done.state, DownloadState::Error);
        assert_eq!(slow_done.abort_reason, DownloadAbortReason::SlowDownload);

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
