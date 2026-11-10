use anyhow::{Context, Result, bail};
use base64::Engine as _;
use chrono::{Local, Utc};
use fdm_batch::{expand_batch_pattern, parse_batch_text_input};
use fdm_bt::{
    BtSessionManager, TorrentInspection, bt_session_signature, inspect_magnet,
    inspect_torrent_bytes, scan_torrent_watch_folder,
};
use fdm_core::{
    AuthCredential, AutoStartPreventer, BandwidthLimiter, DownloadAbortReason, DownloadFileEntry,
    DownloadFlags, DownloadId, DownloadPriority, DownloadRecord, DownloadState, DownloadType,
    ExistingFileReaction, FdmConfig, FileExistsAction, FileType, FilterQuery, HashAlgorithm,
    NewDownloadRequest, ResumeSupport, ScheduleRule, ShutdownType,
    SslException, Tag, TrackerInfo, TrafficUsageMode,
    db::{Database, filter_and_sort_downloads},
    export_import::{
        export_downloads_csv, export_downloads_json, export_downloads_urls_txt,
        export_settings_json, import_downloads_file, import_settings_json,
        inspect_legacy_fdm_sqlite,
    },
    hash::{compute_file_hash, sha256_str, verify_file_hash},
    os_integration::{
        SleepPreventer, check_power_status, detect_network_restrictions, has_sufficient_disk_space,
        perform_shutdown_action, run_external_completion_app, scan_file_with_antivirus,
        set_mark_of_the_web,
    },
    routing::{
        classify_file_type, expand_path_macros, infer_filename_from_url,
        remember_target_dir_preference, resolve_existing_file_name, resolve_target_dir,
        sanitize_filename,
    },
    scheduler::is_schedule_active,
};
use fdm_hls::{download_dash_stream, download_hls_stream};
use fdm_http::{ControlCommand, download_http_or_ftp, probe_http_resource, build_http_client};
use fdm_ipc::{
    IpcRequest, IpcResponse, RemotePairingInfo, apply_browser_settings_patch,
    build_browser_settings_payload, default_socket_path, generate_pairing_info_with_secret,
    start_ipc_server, start_remote_control_server,
};
use fdm_media::{
    JsPluginInfo, MediaParseResult, PluginUpdateOutcome, check_and_update_plugins,
    compare_semver, install_plugin_from_path_with_options, list_plugins, merge_av_files,
    probe_media_url, set_plugin_enabled, uninstall_plugin,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStateSummary {
    pub total_count: usize,
    pub active_count: usize,
    pub queued_count: usize,
    pub paused_count: usize,
    pub completed_count: usize,
    pub error_count: usize,
    pub missing_count: usize,
    pub total_download_bps: u64,
    pub total_upload_bps: u64,
    pub tum: TrafficUsageMode,
    pub preventer: AutoStartPreventer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub configured: bool,
    pub release_notes: Option<String>,
    pub download_url: Option<String>,
    pub staged_path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct AddProbeOutcome {
    pub title: String,
    pub download_type: DownloadType,
    pub total_bytes: Option<u64>,
    pub resume_support: ResumeSupport,
    pub files: Vec<DownloadFileEntry>,
    pub media: Option<MediaParseResult>,
    pub torrent: Option<TorrentInspection>,
    pub resolved_dir: PathBuf,
    pub resolved_file_name: String,
}

pub struct DownloadEngine {
    data_dir: PathBuf,
    plugins_dir: PathBuf,
    db: Database,
    config: RwLock<FdmConfig>,
    downloads: RwLock<Vec<DownloadRecord>>,
    controls: Mutex<HashMap<DownloadId, watch::Sender<ControlCommand>>>,
    tasks: Mutex<HashMap<DownloadId, JoinHandle<()>>>,
    pending_conflicts: Mutex<HashSet<DownloadId>>,
    limiter: BandwidthLimiter,
    bt_manager: tokio::sync::Mutex<Option<Arc<BtSessionManager>>>,
    events_tx: broadcast::Sender<EngineEventAlias>,
    raise_tx: broadcast::Sender<()>,
    seen_watch_files: Mutex<HashSet<PathBuf>>,
    sleep_preventer: Mutex<SleepPreventer>,
    last_preventer: Mutex<AutoStartPreventer>,
    speed_history: Mutex<VecDeque<(u64, u64)>>,
    remote_pairing: Mutex<Option<RemotePairingInfo>>,
    remote_handle: Mutex<Option<JoinHandle<()>>>,
    remote_target_addr: Mutex<Option<SocketAddr>>,
    bt_sig: Mutex<String>,
    ephemeral_bt: bool,
}

pub type EngineEventAlias = fdm_core::EngineEvent;

impl DownloadEngine {
    pub fn open(data_dir: impl Into<PathBuf>, ephemeral_bt: bool) -> Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;
        let plugins_dir = data_dir.join("plugins");
        std::fs::create_dir_all(&plugins_dir)?;

        let db = Database::open(&data_dir.join("ferrite.sqlite"))?;
        let mut config = db.load_config()?;
        if !config.core.restore_tum_at_startup {
            config.core.current_tum = TrafficUsageMode::High;
        }
        let max_dl = config.active_profile().max_download_speed_bps;
        let limiter = BandwidthLimiter::new(max_dl);
        limiter.set_compliant_mode(config.core.traffic_limits_compliant_mode);
        let init_bt_sig = bt_session_signature(&config);

        let mut records = db.list_all_downloads()?;
        // Any downloads that were active when the app exited unexpectedly transition safely back to Paused
        let now = Utc::now();
        for rec in &mut records {
            if rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued {
                rec.state = DownloadState::Paused;
                rec.download_speed_bps = 0;
                rec.upload_speed_bps = 0;
                rec.connections_active = 0;
                rec.updated_at = now;
                let _ = db.upsert_download(rec);
            }
        }

        let (events_tx, _) = broadcast::channel(512);
        let (raise_tx, _) = broadcast::channel(32);

        Ok(Arc::new(Self {
            data_dir,
            plugins_dir,
            db,
            config: RwLock::new(config),
            downloads: RwLock::new(records),
            controls: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            pending_conflicts: Mutex::new(HashSet::new()),
            limiter,
            bt_manager: tokio::sync::Mutex::new(None),
            events_tx,
            raise_tx,
            seen_watch_files: Mutex::new(HashSet::new()),
            sleep_preventer: Mutex::new(SleepPreventer::default()),
            last_preventer: Mutex::new(AutoStartPreventer::None),
            speed_history: Mutex::new(VecDeque::with_capacity(120)),
            remote_pairing: Mutex::new(None),
            remote_handle: Mutex::new(None),
            remote_target_addr: Mutex::new(None),
            bt_sig: Mutex::new(init_bt_sig),
            ephemeral_bt,
        }))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEventAlias> {
        self.events_tx.subscribe()
    }

    pub fn subscribe_raise(&self) -> broadcast::Receiver<()> {
        self.raise_tx.subscribe()
    }

    pub fn emit_raise(&self) {
        let _ = self.raise_tx.send(());
    }

    pub fn get_config(&self) -> FdmConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update_config(self: &Arc<Self>, new_cfg: FdmConfig) -> Result<()> {
        let max_dl = new_cfg.active_profile().max_download_speed_bps;
        self.limiter.set_limit_bps(max_dl);
        self.limiter
            .set_compliant_mode(new_cfg.core.traffic_limits_compliant_mode);

        let new_bt_sig = bt_session_signature(&new_cfg);
        let bt_changed = {
            let mut sig_g = self.bt_sig.lock().unwrap();
            let changed = *sig_g != new_bt_sig;
            *sig_g = new_bt_sig;
            changed
        };
        let bt_busy = self
            .downloads
            .read()
            .unwrap()
            .iter()
            .any(|d| d.download_type == DownloadType::BitTorrent && d.state.is_running_or_seeding());

        let bind_ip: std::net::IpAddr = new_cfg
            .app
            .remote_access_bind
            .parse()
            .unwrap_or_else(|_| "0.0.0.0".parse().unwrap());
        let desired_remote = SocketAddr::new(bind_ip, new_cfg.app.remote_access_port);
        let remote_changed =
            *self.remote_target_addr.lock().unwrap() != Some(desired_remote);

        let prev_remote_pw = self.config.read().unwrap().app.remote_access_password.clone();
        let existing_remote_host = self
            .remote_pairing
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|p| Url::parse(&p.https_url).ok())
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "127.0.0.1".into());

        self.db.save_config(&new_cfg)?;
        if !new_cfg.app.remote_access_password.trim().is_empty()
            && new_cfg.app.remote_access_password != prev_remote_pw
        {
            self.db.save_rc_password_hash(
                "default-pair",
                &sha256_str(&new_cfg.app.remote_access_password),
            )?;
            if let Some(ref mut pair) = *self.remote_pairing.lock().unwrap() {
                if let Ok(updated) = generate_pairing_info_with_secret(
                    &new_cfg.app.remote_control_server_id,
                    &existing_remote_host,
                    new_cfg.app.remote_access_port,
                    &new_cfg.app.remote_access_password,
                ) {
                    *pair = updated;
                }
            }
        }
        *self.config.write().unwrap() = new_cfg.clone();

        if !new_cfg.app.enable_remote_access {
            self.stop_remote_control();
        } else if remote_changed && self.remote_handle.lock().unwrap().is_some() {
            self.stop_remote_control();
        }

        let eng = Arc::clone(self);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if bt_changed && !bt_busy {
                    *eng.bt_manager.lock().await = None;
                    eng.log_lt_alert("Reconfigured BitTorrent session settings");
                } else if let Some(ref bt) = *eng.bt_manager.lock().await {
                    bt.apply_rate_limits(&new_cfg);
                }
                if new_cfg.app.enable_remote_access && remote_changed {
                    let _ = eng.ensure_remote_control(&existing_remote_host).await;
                }
            });
        }
        let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        Ok(())
    }

    pub fn set_traffic_usage_mode(self: &Arc<Self>, mode: TrafficUsageMode) -> Result<()> {
        let mut cfg = self.get_config();
        cfg.core.current_tum = mode;
        self.update_config(cfg)?;
        let _ = self
            .events_tx
            .send(EngineEventAlias::TrafficModeChanged { mode });
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        self.db.list_tags()
    }

    pub fn create_tag(&self, name: &str, color_hex: &str) -> Result<Tag> {
        self.db.create_or_update_tag(name, color_hex)
    }

    pub fn edit_tag(
        &self,
        tag_id: i64,
        name: &str,
        color_hex: &str,
        default_extensions: &[String],
        default_download_folder: Option<&Path>,
    ) -> Result<Tag> {
        let tag = self.db.update_tag(
            tag_id,
            name,
            color_hex,
            default_extensions,
            default_download_folder,
        )?;
        let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        Ok(tag)
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<()> {
        self.db.delete_tag(tag_id)?;
        let mut list = self.downloads.write().unwrap();
        for rec in list.iter_mut() {
            if rec.tags.contains(&tag_id) {
                rec.tags.retain(|&t| t != tag_id);
                let _ = self.db.upsert_download(rec);
            }
        }
        Ok(())
    }

    pub fn assign_tag(&self, download_id: DownloadId, tag_id: i64) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == download_id)
            .context("Download not found")?;
        if !rec.tags.contains(&tag_id) {
            rec.tags.push(tag_id);
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
            let _ = self
                .events_tx
                .send(EngineEventAlias::Updated { id: download_id });
        }
        Ok(())
    }

    pub fn unassign_tag(&self, download_id: DownloadId, tag_id: i64) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == download_id)
            .context("Download not found")?;
        rec.tags.retain(|&t| t != tag_id);
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self
            .events_tx
            .send(EngineEventAlias::Updated { id: download_id });
        Ok(())
    }

    pub fn list_downloads(&self, query: &FilterQuery) -> Vec<DownloadRecord> {
        let items = self.downloads.read().unwrap();
        let tags = self.db.list_tags().unwrap_or_default();
        filter_and_sort_downloads(&items, &tags, query)
    }

    pub fn get_download(&self, id: DownloadId) -> Option<DownloadRecord> {
        self.downloads
            .read()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .cloned()
    }

    pub fn summary(&self) -> EngineStateSummary {
        let items = self.downloads.read().unwrap();
        let cfg = self.config.read().unwrap();
        let prev = *self.last_preventer.lock().unwrap();
        let mut active_count = 0usize;
        let mut queued_count = 0usize;
        let mut paused_count = 0usize;
        let mut completed_count = 0usize;
        let mut error_count = 0usize;
        let mut missing_count = 0usize;
        let mut total_download_bps = 0u64;
        let mut total_upload_bps = 0u64;

        for d in items.iter() {
            if d.state.is_running_or_seeding() {
                active_count += 1;
            } else if d.state == DownloadState::Queued {
                queued_count += 1;
            } else if matches!(d.state, DownloadState::Paused | DownloadState::Stopped | DownloadState::Scheduled) {
                paused_count += 1;
            } else if d.state == DownloadState::Completed {
                completed_count += 1;
            } else if d.state == DownloadState::Error {
                error_count += 1;
            }
            if d.flags.is_missing_files {
                missing_count += 1;
            }
            total_download_bps = total_download_bps.saturating_add(d.download_speed_bps);
            total_upload_bps = total_upload_bps.saturating_add(d.upload_speed_bps);
        }

        EngineStateSummary {
            total_count: items.len(),
            active_count,
            queued_count,
            paused_count,
            completed_count,
            error_count,
            missing_count,
            total_download_bps,
            total_upload_bps,
            tum: cfg.core.current_tum,
            preventer: prev,
        }
    }

    pub fn speed_history(&self) -> Vec<(u64, u64)> {
        self.speed_history.lock().unwrap().iter().copied().collect()
    }

    pub fn add_ssl_exception(&self, host_port: &str, sha256_fp: &str) -> Result<()> {
        self.db.add_ssl_exception(&SslException {
            host_port: host_port.to_string(),
            sha256_fingerprint: sha256_fp.to_string(),
            added_at: Utc::now(),
        })
    }

    pub fn list_ssl_exceptions(&self) -> Result<Vec<SslException>> {
        self.db.list_ssl_exceptions()
    }

    pub fn save_credential(&self, scope: &str, username: &str, password: &str) -> Result<()> {
        self.db.save_credential(&AuthCredential {
            scope: scope.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    async fn get_or_init_bt(&self) -> Result<Arc<BtSessionManager>> {
        let mut guard = self.bt_manager.lock().await;
        if let Some(ref existing) = *guard {
            return Ok(Arc::clone(existing));
        }
        let cfg = self.get_config();
        let mgr = if self.ephemeral_bt {
            BtSessionManager::new_ephemeral(cfg.core.default_download_path.clone()).await?
        } else {
            let persist = self.data_dir.join("bt_state");
            BtSessionManager::new(
                cfg.core.default_download_path.clone(),
                &cfg,
                Some(persist),
            )
            .await?
        };
        let arc = Arc::new(mgr);
        *guard = Some(Arc::clone(&arc));
        Ok(arc)
    }

    /// Inspect any URL / magnet / .torrent path before enqueueing (backing FDM's "Tune & Add" dialog).
    pub async fn probe_source(&self, source: &str) -> Result<AddProbeOutcome> {
        let trimmed = source.trim();
        let cfg = self.get_config();
        let lower = trimmed.to_ascii_lowercase();

        if trimmed.starts_with("magnet:")
            || lower.ends_with(".torrent")
            || Path::new(trimmed)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("torrent"))
                .unwrap_or(false)
        {
            let info = if trimmed.starts_with("magnet:") {
                if self.ephemeral_bt {
                    inspect_magnet(trimmed)?
                } else {
                    let bt = self.get_or_init_bt().await?;
                    bt.inspect_source(trimmed).await.unwrap_or_else(|_| {
                        inspect_magnet(trimmed).unwrap_or(TorrentInspection {
                            info_hash: String::new(),
                            name: "Magnet Download".into(),
                            total_bytes: 0,
                            files: vec![],
                            trackers: vec![],
                            raw_torrent_bytes: None,
                        })
                    })
                }
            } else if Path::new(trimmed).is_file() {
                inspect_torrent_bytes(&std::fs::read(trimmed)?)?
            } else {
                let bt = self.get_or_init_bt().await?;
                bt.inspect_source(trimmed).await?
            };
            let resolved_file_name = sanitize_filename(&info.name);
            let resolved_dir = resolve_target_dir(&cfg, None, trimmed, &resolved_file_name, None);
            return Ok(AddProbeOutcome {
                title: info.name.clone(),
                download_type: DownloadType::BitTorrent,
                total_bytes: if info.total_bytes > 0 {
                    Some(info.total_bytes)
                } else {
                    None
                },
                resume_support: ResumeSupport::Present,
                files: info.files.clone(),
                media: None,
                torrent: Some(info),
                resolved_dir,
                resolved_file_name,
            });
        }

        if lower.contains(".m3u8") || lower.contains(".mpd") {
            let dl_type = if lower.contains(".mpd") {
                DownloadType::Dash
            } else {
                DownloadType::HlsM3u8
            };
            let media = probe_media_url(trimmed, &self.plugins_dir, &cfg).await.ok();
            let raw_name = media
                .as_ref()
                .map(|m| sanitize_filename(&format!("{}.mp4", m.title)))
                .unwrap_or_else(|| {
                    let base = infer_filename_from_url(trimmed);
                    format!(
                        "{}.mp4",
                        base.trim_end_matches(".m3u8").trim_end_matches(".mpd")
                    )
                });
            let resolved_dir =
                resolve_target_dir(&cfg, None, trimmed, &raw_name, Some("video/mp4"));
            return Ok(AddProbeOutcome {
                title: media
                    .as_ref()
                    .map(|m| m.title.clone())
                    .unwrap_or_else(|| raw_name.clone()),
                download_type: dl_type,
                total_bytes: None,
                resume_support: ResumeSupport::Present,
                files: vec![],
                media,
                torrent: None,
                resolved_dir,
                resolved_file_name: raw_name,
            });
        }

        // Check media plugins / yt-dlp first if it looks like a watch page (no obvious binary extension)
        if let Ok(parsed_media) = probe_media_url(trimmed, &self.plugins_dir, &cfg).await {
            let name = sanitize_filename(&format!(
                "{}.{}",
                parsed_media.title,
                parsed_media
                    .variants
                    .first()
                    .map(|v| v.ext.as_str())
                    .unwrap_or("mp4")
            ));
            let resolved_dir = resolve_target_dir(&cfg, None, trimmed, &name, Some("video/mp4"));
            return Ok(AddProbeOutcome {
                title: parsed_media.title.clone(),
                download_type: DownloadType::MediaParser,
                total_bytes: parsed_media.variants.first().and_then(|v| v.filesize),
                resume_support: ResumeSupport::Present,
                files: vec![],
                media: Some(parsed_media),
                torrent: None,
                resolved_dir,
                resolved_file_name: name,
            });
        }

        // Fallback to HTTP/FTP header probe
        let mut dummy = self.draft_record(&NewDownloadRequest::from_url(trimmed), &cfg);
        let ssl = self.db.list_ssl_exceptions().unwrap_or_default();
        let cred = self.lookup_credential(trimmed);
        if let Ok(client) = build_http_client(&cfg, &ssl, None) {
            if let Ok(probe) = probe_http_resource(&client, &dummy, &ssl, cred.as_ref()).await {
                if let Some(ref s) = probe.suggested_filename {
                    dummy.file_name = s.clone();
                    dummy.title = s.clone();
                }
                let resolved_dir = resolve_target_dir(
                    &cfg,
                    None,
                    &probe.final_url,
                    &dummy.file_name,
                    probe.content_type.as_deref(),
                );
                return Ok(AddProbeOutcome {
                    title: dummy.title,
                    download_type: DownloadType::Www,
                    total_bytes: probe.total_bytes,
                    resume_support: probe.resume_support,
                    files: vec![],
                    media: None,
                    torrent: None,
                    resolved_dir,
                    resolved_file_name: dummy.file_name,
                });
            }
        }

        Ok(AddProbeOutcome {
            title: dummy.title,
            download_type: DownloadType::Www,
            total_bytes: None,
            resume_support: ResumeSupport::Unknown,
            files: vec![],
            media: None,
            torrent: None,
            resolved_dir: dummy.target_dir,
            resolved_file_name: dummy.file_name,
        })
    }

    fn lookup_credential(&self, url_str: &str) -> Option<AuthCredential> {
        if let Ok(Some(exact)) = self.db.get_credential(url_str) {
            return Some(exact);
        }
        if let Ok(u) = Url::parse(url_str) {
            if let Some(host) = u.host_str() {
                if let Ok(Some(c)) = self.db.get_credential(host) {
                    return Some(c);
                }
            }
        }
        None
    }

    fn draft_record(&self, req: &NewDownloadRequest, cfg: &FdmConfig) -> DownloadRecord {
        let lower = req.url.to_ascii_lowercase();
        let dl_type = req.download_type.unwrap_or_else(|| {
            if req.url.starts_with("magnet:")
                || lower.ends_with(".torrent")
                || req.torrent_bytes_b64.is_some()
            {
                DownloadType::BitTorrent
            } else if lower.contains(".m3u8") {
                DownloadType::HlsM3u8
            } else if lower.contains(".mpd") {
                DownloadType::Dash
            } else {
                DownloadType::Www
            }
        });

        let mut file_name = req
            .file_name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(sanitize_filename)
            .unwrap_or_else(|| infer_filename_from_url(&req.url));
        if matches!(dl_type, DownloadType::HlsM3u8 | DownloadType::Dash)
            && Path::new(&file_name).extension().is_none()
        {
            file_name.push_str(".mp4");
        }
        if matches!(dl_type, DownloadType::HlsM3u8 | DownloadType::Dash)
            && (file_name.ends_with(".m3u8") || file_name.ends_with(".mpd"))
        {
            file_name = format!(
                "{}.mp4",
                Path::new(&file_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("stream")
            );
        }

        let mut target_dir = resolve_target_dir(
            cfg,
            req.target_dir.as_ref(),
            &req.url,
            &file_name,
            None,
        );
        let file_type = if dl_type == DownloadType::BitTorrent {
            FileType::Torrent
        } else {
            classify_file_type(&file_name, None)
        };
        let file_ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();

        let mut tags = req.tags.clone();
        if let Ok(all_tags) = self.db.list_tags() {
            for t in all_tags {
                let builtin_match = t.is_builtin && t.file_types.contains(&file_type);
                let ext_match = !file_ext.is_empty()
                    && t.default_extensions
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&file_ext));
                if (builtin_match || ext_match) && !tags.contains(&t.id) {
                    tags.push(t.id);
                }
                if req.target_dir.is_none() && (ext_match || tags.contains(&t.id)) {
                    if let Some(ref tag_folder) = t.default_download_folder {
                        let expanded = expand_path_macros(
                            &tag_folder.to_string_lossy(),
                            Some(&req.url),
                            Some(&file_name),
                        );
                        target_dir = PathBuf::from(expanded);
                    }
                }
            }
        }

        let schedule = req.schedule.clone().or_else(|| {
            if cfg.core.enable_scheduler_for_new_downloads {
                Some(cfg.core.default_schedule.clone())
            } else {
                None
            }
        });

        let initial_state = if req.start_paused {
            DownloadState::Paused
        } else if let Some(ref s) = schedule {
            if s.enabled && !is_schedule_active(s, Local::now()) {
                DownloadState::Scheduled
            } else {
                DownloadState::Queued
            }
        } else {
            DownloadState::Queued
        };

        let mut http = req.http.clone();
        if http.url.is_empty() {
            http.url = req.url.clone();
        }

        let mut torrent_source = req.torrent_bytes_b64.clone();
        if dl_type == DownloadType::BitTorrent
            && torrent_source.is_none()
            && Path::new(&req.url).is_file()
        {
            if let Ok(raw) = std::fs::read(&req.url) {
                torrent_source = Some(base64::engine::general_purpose::STANDARD.encode(raw));
            }
        }

        let mut files = Vec::new();
        if let Some(ref sel) = req.selected_file_indices {
            if let Some(ref b64) = torrent_source {
                if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    if let Ok(parsed) = inspect_torrent_bytes(&raw) {
                        files = parsed
                            .files
                            .into_iter()
                            .map(|mut f| {
                                if !sel.contains(&f.index) {
                                    f.priority = DownloadPriority::DontDownload;
                                }
                                f
                            })
                            .collect();
                    }
                }
            }
        }

        let title = req
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| file_name.clone());

        let now = Utc::now();
        DownloadRecord {
            id: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: req.parent_id,
            children_ids: Vec::new(),
            order_index: 0,
            download_type: dl_type,
            state: initial_state,
            priority: req.priority,
            resume_support: ResumeSupport::Unknown,
            flags: DownloadFlags {
                sequential_play_asap: req.sequential_play_asap,
                allow_change_source_url: dl_type == DownloadType::Www,
                supports_mirrors: dl_type == DownloadType::Www,
                supports_restart: true,
                can_detect_remote_changes: dl_type == DownloadType::Www,
                silent_mode: req.silent || cfg.app.silent_mode_for_new_downloads,
                delete_service_files_when_done: cfg.core.delete_service_files_when_finished,
                ..Default::default()
            },
            title,
            url: req.url.clone(),
            target_dir,
            file_name,
            partial_suffix: cfg.core.partial_download_suffix.clone(),
            total_bytes: None,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections_active: 0,
            etag: None,
            last_modified: None,
            file_type,
            http,
            torrent_info_hash: None,
            torrent_source,
            tags,
            files,
            segments: Vec::new(),
            peers: Vec::new(),
            trackers: Vec::new(),
            schedule,
            retry_count: 0,
            forbid_auto_retry_count: 0,
            last_error: None,
            abort_reason: DownloadAbortReason::None,
            preview_path: None,
            module_data: req
                .existing_file_reaction
                .map(|r| serde_json::json!({ "existing_file_reaction": r }))
                .unwrap_or(serde_json::Value::Null),
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    pub async fn add_download(self: &Arc<Self>, req: NewDownloadRequest) -> Result<DownloadRecord> {
        let mut cfg = self.get_config();
        let draft = self.draft_record(&req, &cfg);
        let saved = self.db.next_download_id_and_insert(draft)?;
        if req.target_dir.is_some()
            && remember_target_dir_preference(&mut cfg, &saved.url, &saved.target_dir)
        {
            let _ = self.db.save_config(&cfg);
            *self.config.write().unwrap() = cfg;
            let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        }
        {
            self.downloads.write().unwrap().push(saved.clone());
        }
        self.log_dl_mgmt(&format!("Added download #{} ({}) -> {}", saved.id, saved.file_name, saved.url));
        let _ = self.events_tx.send(EngineEventAlias::Added { id: saved.id });
        if saved.state == DownloadState::Queued {
            self.schedule_queue().await;
        }
        Ok(self.get_download(saved.id).unwrap_or(saved))
    }

    pub async fn add_batch_pattern(
        self: &Arc<Self>,
        pattern_or_multiline: &str,
        target_dir: Option<PathBuf>,
        start_paused: bool,
    ) -> Result<Vec<DownloadRecord>> {
        let cfg = self.get_config();
        let mut urls = if pattern_or_multiline.contains('\n') {
            parse_batch_text_input(pattern_or_multiline)?
        } else {
            expand_batch_pattern(pattern_or_multiline)?
        };
        let max_urls = cfg.app.batch_download_max_urls_count.max(1) as usize;
        if urls.len() > max_urls {
            urls.truncate(max_urls);
        }
        let mut out = Vec::with_capacity(urls.len());
        for url in urls {
            let mut req = NewDownloadRequest::from_url(url);
            req.target_dir = target_dir.clone();
            req.start_paused = start_paused;
            req.silent = true;
            let mut rec = self.draft_record(&req, &cfg);
            rec.flags.created_by_batch = true;
            let saved = self.db.next_download_id_and_insert(rec)?;
            self.downloads.write().unwrap().push(saved.clone());
            let _ = self.events_tx.send(EngineEventAlias::Added { id: saved.id });
            out.push(saved);
        }
        self.log_dl_mgmt(&format!("Added batch of {} downloads", out.len()));
        if !start_paused {
            self.schedule_queue().await;
        }
        Ok(out)
    }

    /// Creates a parent Batch download record containing child downloads (used by FDM's Batch drill-down view).
    pub async fn add_batch_group(
        self: &Arc<Self>,
        pattern_or_multiline: &str,
        title: Option<&str>,
        target_dir: Option<PathBuf>,
        start_paused: bool,
    ) -> Result<(DownloadRecord, Vec<DownloadRecord>)> {
        let cfg = self.get_config();
        let mut urls = if pattern_or_multiline.contains('\n') {
            parse_batch_text_input(pattern_or_multiline)?
        } else {
            expand_batch_pattern(pattern_or_multiline)?
        };
        let max_urls = cfg.app.batch_download_max_urls_count.max(1) as usize;
        if urls.len() > max_urls {
            urls.truncate(max_urls);
        }
        let dir = target_dir
            .clone()
            .unwrap_or_else(|| cfg.core.default_download_path.clone());
        let parent_title = title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Batch ({} downloads)", urls.len()));
        let now = Utc::now();
        let parent_draft = DownloadRecord {
            id: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            children_ids: Vec::new(),
            order_index: 0,
            download_type: DownloadType::Batch,
            state: DownloadState::Completed,
            priority: DownloadPriority::Normal,
            resume_support: ResumeSupport::Present,
            flags: DownloadFlags::default(),
            title: parent_title.clone(),
            url: urls.first().cloned().unwrap_or_default(),
            target_dir: dir.clone(),
            file_name: parent_title,
            partial_suffix: String::new(),
            total_bytes: None,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections_active: 0,
            etag: None,
            last_modified: None,
            file_type: FileType::Other,
            http: Default::default(),
            torrent_info_hash: None,
            torrent_source: None,
            tags: Vec::new(),
            files: Vec::new(),
            segments: Vec::new(),
            peers: Vec::new(),
            trackers: Vec::new(),
            schedule: None,
            retry_count: 0,
            forbid_auto_retry_count: 0,
            last_error: None,
            abort_reason: DownloadAbortReason::None,
            preview_path: None,
            module_data: serde_json::json!({ "batch_count": urls.len() }),
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
        };
        let mut parent = self.db.next_download_id_and_insert(parent_draft)?;
        let mut children = Vec::with_capacity(urls.len());
        for url in urls {
            let mut req = NewDownloadRequest::from_url(url);
            req.target_dir = Some(dir.clone());
            req.start_paused = start_paused;
            req.silent = true;
            req.parent_id = Some(parent.id);
            let mut rec = self.draft_record(&req, &cfg);
            rec.flags.created_by_batch = true;
            let saved = self.db.next_download_id_and_insert(rec)?;
            parent.children_ids.push(saved.id);
            self.downloads.write().unwrap().push(saved.clone());
            let _ = self.events_tx.send(EngineEventAlias::Added { id: saved.id });
            children.push(saved);
        }
        let _ = self.db.upsert_download(&parent);
        self.downloads.write().unwrap().push(parent.clone());
        let _ = self.events_tx.send(EngineEventAlias::Added { id: parent.id });
        if !start_paused && !children.is_empty() {
            self.schedule_queue().await;
        }
        Ok((parent, children))
    }

    pub async fn pause_download(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        if let Some(tx) = self.controls.lock().unwrap().get(&id) {
            let _ = tx.send(ControlCommand::Pause);
        }
        {
            let mut list = self.downloads.write().unwrap();
            if let Some(rec) = list.iter_mut().find(|d| d.id == id) {
                if !matches!(rec.state, DownloadState::Completed | DownloadState::Stopped) {
                    if !rec.state.is_active() {
                        rec.state = DownloadState::Paused;
                        rec.download_speed_bps = 0;
                        rec.connections_active = 0;
                        rec.updated_at = Utc::now();
                        let _ = self.db.upsert_download(rec);
                    }
                }
            } else {
                bail!("Download {id} not found");
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        self.schedule_queue().await;
        Ok(())
    }

    pub async fn resume_download(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            if rec.state == DownloadState::Completed {
                return Ok(());
            }
            if !rec.state.is_running_or_seeding() {
                rec.state = DownloadState::Queued;
                rec.last_error = None;
                rec.abort_reason = DownloadAbortReason::None;
                rec.updated_at = Utc::now();
                self.db.upsert_download(rec)?;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        self.schedule_queue().await;
        Ok(())
    }

    pub async fn cancel_download(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        if let Some(tx) = self.controls.lock().unwrap().get(&id) {
            let _ = tx.send(ControlCommand::Cancel);
        }
        {
            let mut list = self.downloads.write().unwrap();
            if let Some(rec) = list.iter_mut().find(|d| d.id == id) {
                if !rec.state.is_active() {
                    rec.state = DownloadState::Stopped;
                    rec.abort_reason = DownloadAbortReason::AbortedByUser;
                    rec.download_speed_bps = 0;
                    rec.upload_speed_bps = 0;
                    rec.connections_active = 0;
                    rec.updated_at = Utc::now();
                    let _ = self.db.upsert_download(rec);
                }
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        self.schedule_queue().await;
        Ok(())
    }

    pub async fn remove_download(self: &Arc<Self>, id: DownloadId, delete_files: bool) -> Result<()> {
        let _ = self.cancel_download(id).await;
        let removed = {
            let mut list = self.downloads.write().unwrap();
            if let Some(pos) = list.iter().position(|d| d.id == id) {
                Some(list.remove(pos))
            } else {
                None
            }
        };
        if let Some(rec) = removed {
            if delete_files {
                let p1 = rec.output_path();
                let p2 = rec.partial_path();
                if p1.is_file() {
                    let _ = std::fs::remove_file(&p1);
                } else if p1.is_dir() && rec.download_type == DownloadType::BitTorrent {
                    let _ = std::fs::remove_dir_all(&p1);
                }
                if p2.is_file() {
                    let _ = std::fs::remove_file(&p2);
                }
            }
            self.db.remove_download(id)?;
            self.controls.lock().unwrap().remove(&id);
            self.pending_conflicts.lock().unwrap().remove(&id);
            let _ = self.events_tx.send(EngineEventAlias::Removed { id });
        }
        Ok(())
    }

    pub async fn start_all(self: &Arc<Self>) -> Result<()> {
        let ids: Vec<DownloadId> = {
            let list = self.downloads.read().unwrap();
            list.iter()
                .filter(|d| {
                    matches!(
                        d.state,
                        DownloadState::Paused | DownloadState::Stopped | DownloadState::Error | DownloadState::Scheduled
                    )
                })
                .map(|d| d.id)
                .collect()
        };
        for id in ids {
            let _ = self.resume_download(id).await;
        }
        Ok(())
    }

    pub async fn pause_all(self: &Arc<Self>) -> Result<()> {
        let ids: Vec<DownloadId> = {
            let list = self.downloads.read().unwrap();
            list.iter()
                .filter(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued)
                .map(|d| d.id)
                .collect()
        };
        for id in ids {
            let _ = self.pause_download(id).await;
        }
        Ok(())
    }

    pub async fn set_priority(&self, id: DownloadId, priority: DownloadPriority) -> Result<()> {
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            rec.priority = priority;
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn change_source_url(self: &Arc<Self>, id: DownloadId, new_url: &str) -> Result<()> {
        let trimmed = new_url.trim();
        let _ = Url::parse(trimmed).context("Invalid replacement URL")?;
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            rec.url = trimmed.to_string();
            rec.http.url = trimmed.to_string();
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn update_mirrors(self: &Arc<Self>, id: DownloadId, mirrors: Vec<String>) -> Result<()> {
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            rec.http.mirrors = mirrors;
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn toggle_sequential_play_asap(&self, id: DownloadId, enabled: bool) -> Result<()> {
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            rec.flags.sequential_play_asap = enabled;
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn update_torrent_file_selection(
        &self,
        id: DownloadId,
        selected_indices: HashSet<usize>,
    ) -> Result<()> {
        let info_hash = {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            for f in &mut rec.files {
                f.priority = if selected_indices.contains(&f.index) {
                    if f.priority == DownloadPriority::DontDownload {
                        DownloadPriority::Normal
                    } else {
                        f.priority
                    }
                } else {
                    DownloadPriority::DontDownload
                };
            }
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
            rec.torrent_info_hash.clone()
        };
        if let Some(ih) = info_hash {
            if let Some(ref bt) = *self.bt_manager.lock().await {
                let _ = bt.update_file_selection(&ih, selected_indices).await;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn resolve_conflict(
        self: &Arc<Self>,
        id: DownloadId,
        action: FileExistsAction,
    ) -> Result<()> {
        self.pending_conflicts.lock().unwrap().remove(&id);
        let should_resume = {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            match action {
                FileExistsAction::Overwrite => {
                    rec.module_data = serde_json::json!({
                        "existing_file_reaction": ExistingFileReaction::Overwrite
                    });
                    rec.state = DownloadState::Queued;
                    true
                }
                FileExistsAction::Rename => {
                    if let Some(next_name) = resolve_existing_file_name(
                        &rec.target_dir,
                        &rec.file_name,
                        ExistingFileReaction::Rename,
                    ) {
                        rec.file_name = next_name.clone();
                        rec.title = next_name;
                    }
                    rec.module_data = serde_json::json!({
                        "existing_file_reaction": ExistingFileReaction::Rename
                    });
                    rec.state = DownloadState::Queued;
                    true
                }
                FileExistsAction::Abort | FileExistsAction::Skip => {
                    rec.state = DownloadState::Stopped;
                    rec.abort_reason = DownloadAbortReason::AlreadyExists;
                    false
                }
            }
        };
        if let Some(rec) = self.get_download(id) {
            let _ = self.db.upsert_download(&rec);
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        if should_resume {
            self.schedule_queue().await;
        }
        Ok(())
    }

    pub fn verify_download_hash(
        &self,
        id: DownloadId,
        algo: HashAlgorithm,
        expected: Option<&str>,
    ) -> Result<(String, Option<bool>)> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == id)
            .context("Download not found")?;
        let path = rec.output_path();
        if !path.is_file() {
            bail!("File does not exist at {}", path.display());
        }
        let actual = compute_file_hash(&path, algo)?;
        let matches = expected.map(|exp| {
            let exp_t = exp.trim();
            !exp_t.is_empty() && actual.eq_ignore_ascii_case(exp_t)
        });
        if let Some(first) = rec.files.first_mut() {
            first.computed_hash = Some((algo, actual.clone()));
            if let Some(exp) = expected {
                if !exp.trim().is_empty() {
                    first.expected_hash = Some((algo, exp.trim().to_string()));
                }
            }
        }
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok((actual, matches))
    }

    pub async fn move_download_files(&self, id: DownloadId, dest_dir: &Path) -> Result<PathBuf> {
        let (src, file_name, is_active) = {
            let list = self.downloads.read().unwrap();
            let rec = list
                .iter()
                .find(|d| d.id == id)
                .context("Download not found")?;
            (rec.output_path(), rec.file_name.clone(), rec.state.is_active())
        };
        if is_active {
            bail!("Pause download before moving files");
        }
        std::fs::create_dir_all(dest_dir)?;
        let dst = dest_dir.join(&file_name);
        if src.exists() && src != dst {
            if let Err(_) = std::fs::rename(&src, &dst) {
                std::fs::copy(&src, &dst)?;
                let _ = std::fs::remove_file(&src);
            }
        }
        {
            let mut list = self.downloads.write().unwrap();
            if let Some(rec) = list.iter_mut().find(|d| d.id == id) {
                rec.target_dir = dest_dir.to_path_buf();
                rec.flags.is_missing_files = !dst.exists();
                rec.updated_at = Utc::now();
                self.db.upsert_download(rec)?;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(dst)
    }

    pub fn list_js_plugins(&self) -> Vec<JsPluginInfo> {
        list_plugins(&self.plugins_dir)
    }

    pub fn set_js_plugin_enabled(&self, uid: &str, enabled: bool) -> Result<()> {
        set_plugin_enabled(&self.plugins_dir, uid, enabled)
    }

    pub fn uninstall_js_plugin(&self, uid: &str) -> Result<()> {
        uninstall_plugin(&self.plugins_dir, uid)
    }

    pub fn install_js_plugin_from_path(&self, source: &Path) -> Result<JsPluginInfo> {
        let cfg = self.get_config();
        let info = install_plugin_from_path_with_options(
            &self.plugins_dir,
            source,
            cfg.app.plugins_dev_ignore_sign,
        )?;
        self.log_dl_mgmt(&format!("Installed plugin {} v{}", info.name, info.version));
        let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        Ok(info)
    }

    pub async fn check_and_update_js_plugins(self: &Arc<Self>) -> Result<Vec<PluginUpdateOutcome>> {
        let cfg = self.get_config();
        let outcomes = check_and_update_plugins(&self.plugins_dir, &cfg).await?;
        if !outcomes.is_empty() {
            let summary: Vec<String> = outcomes
                .iter()
                .map(|o| format!("{} -> v{}", o.name, o.new_version))
                .collect();
            self.log_dl_mgmt(&format!("Updated plugins: {}", summary.join(", ")));
            let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        }
        Ok(outcomes)
    }

    pub async fn check_for_app_updates(
        &self,
        manifest_url_override: Option<&str>,
    ) -> Result<AppUpdateCheckResult> {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let cfg = self.get_config();
        let override_str = manifest_url_override
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let env_url = std::env::var("FERRITE_UPDATE_MANIFEST_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let local_manifest = self.data_dir.join("update-manifest.json");

        let source = if let Some(u) = override_str.or(env_url) {
            u
        } else if local_manifest.is_file() {
            local_manifest.to_string_lossy().into_owned()
        } else {
            return Ok(AppUpdateCheckResult {
                current_version: current_version.clone(),
                latest_version: Some(current_version.clone()),
                update_available: false,
                configured: false,
                release_notes: None,
                download_url: None,
                staged_path: None,
                message: format!(
                    "Ferrite DM v{} — offline privacy mode (no update manifest configured; zero telemetry sent).",
                    current_version
                ),
            });
        };

        let ssl = self.db.list_ssl_exceptions().unwrap_or_default();
        let client = build_http_client(&cfg, &ssl, None)?;

        let raw_json: String = if source.starts_with("http://") || source.starts_with("https://") {
            let resp = client.get(&source).send().await?.error_for_status()?;
            resp.text().await?
        } else if let Some(path_str) = source.strip_prefix("file://") {
            std::fs::read_to_string(path_str)
                .with_context(|| format!("Failed reading update manifest {path_str}"))?
        } else {
            std::fs::read_to_string(&source)
                .with_context(|| format!("Failed reading update manifest {source}"))?
        };

        let val: serde_json::Value =
            serde_json::from_str(&raw_json).context("Invalid update manifest JSON")?;
        let latest_version_raw = val
            .get("version")
            .or_else(|| val.get("tag_name"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .context("Update manifest missing 'version' or 'tag_name'")?;
        let latest_version = latest_version_raw.trim_start_matches(['v', 'V']).to_string();

        let release_notes = val
            .get("notes")
            .or_else(|| val.get("release_notes"))
            .or_else(|| val.get("body"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let expected_sha256 = val
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let download_url = val
            .get("download_url")
            .or_else(|| val.get("url"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                val.get("assets")
                    .and_then(|a| a.as_array())
                    .and_then(|arr| {
                        arr.iter()
                            .filter_map(|item| item.get("browser_download_url").and_then(|u| u.as_str()))
                            .next()
                    })
                    .map(str::to_string)
            })
            .or_else(|| {
                val.get("html_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });

        let update_available =
            compare_semver(&latest_version, &current_version) == std::cmp::Ordering::Greater;

        let mut staged_path: Option<PathBuf> = None;
        if update_available && cfg.app.auto_install_updates {
            if let Some(ref dl_url) = download_url {
                let updates_dir = self.data_dir.join("updates");
                std::fs::create_dir_all(&updates_dir)?;
                let file_name = Url::parse(dl_url)
                    .ok()
                    .and_then(|u| {
                        u.path_segments()
                            .and_then(|mut segs| segs.next_back().map(str::to_string))
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("ferrite-dm-{latest_version}.pkg"));
                let dest = updates_dir.join(sanitize_filename(&file_name));

                if dl_url.starts_with("http://") || dl_url.starts_with("https://") {
                    let bytes = client
                        .get(dl_url)
                        .send()
                        .await?
                        .error_for_status()?
                        .bytes()
                        .await?;
                    std::fs::write(&dest, &bytes)?;
                } else {
                    let src_path = dl_url.strip_prefix("file://").unwrap_or(dl_url.as_str());
                    std::fs::copy(src_path, &dest)
                        .with_context(|| format!("Failed staging update payload from {src_path}"))?;
                }

                if let Some(ref sha) = expected_sha256 {
                    if !verify_file_hash(&dest, HashAlgorithm::Sha256, sha)? {
                        let _ = std::fs::remove_file(&dest);
                        bail!(
                            "Staged update payload for v{} failed SHA-256 verification",
                            latest_version
                        );
                    }
                }
                self.log_dl_mgmt(&format!(
                    "Downloaded and staged Ferrite DM v{} update to {}",
                    latest_version,
                    dest.display()
                ));
                staged_path = Some(dest);
            }
        }

        let message = if !update_available {
            format!("Ferrite DM v{} is up to date.", current_version)
        } else if let Some(ref path) = staged_path {
            format!(
                "Update v{} ready (staged at {}).",
                latest_version,
                path.display()
            )
        } else {
            format!("Ferrite DM v{} is available (current: v{}).", latest_version, current_version)
        };

        Ok(AppUpdateCheckResult {
            current_version,
            latest_version: Some(latest_version),
            update_available,
            configured: true,
            release_notes,
            download_url,
            staged_path,
            message,
        })
    }

    pub fn log_line(&self, level: &str, message: &str) {
        let cfg = self.get_config();
        if !cfg.app.enable_logging {
            return;
        }
        let log_dir = self.data_dir.join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("ferrite.log");
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
        {
            let _ = writeln!(f, "{} [{}] {}", Utc::now().to_rfc3339(), level, message);
        }
    }

    pub fn log_dl_mgmt(&self, message: &str) {
        let cfg = self.get_config();
        if cfg.app.enable_logging && (cfg.app.downloads_mgmt_logging || cfg.app.enhanced_logging) {
            self.log_line("DL", message);
        }
    }

    pub fn log_lt_alert(&self, message: &str) {
        let cfg = self.get_config();
        if cfg.app.enable_logging && (cfg.app.lt_alerts_logging || cfg.app.enhanced_logging) {
            self.log_line("BT", message);
        }
    }

    pub fn log_enhanced(&self, message: &str) {
        let cfg = self.get_config();
        if cfg.app.enable_logging && cfg.app.enhanced_logging {
            self.log_line("TRACE", message);
        }
    }

    pub async fn restart_download(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        let _ = self.cancel_download(id).await;
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            let p_out = rec.output_path();
            let p_part = rec.partial_path();
            if p_part.is_file() {
                let _ = std::fs::remove_file(&p_part);
            }
            if p_out.is_file() {
                let _ = std::fs::remove_file(&p_out);
            }
            rec.downloaded_bytes = 0;
            rec.segments.clear();
            rec.last_error = None;
            rec.abort_reason = DownloadAbortReason::None;
            rec.completed_at = None;
            rec.flags.is_missing_files = false;
            rec.state = DownloadState::Queued;
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        self.log_line("INFO", &format!("Restarted download #{id}"));
        self.schedule_queue().await;
        Ok(())
    }

    pub async fn rename_download_file(&self, id: DownloadId, new_name: &str) -> Result<PathBuf> {
        let cleaned = sanitize_filename(new_name);
        let (old_out, target_dir, is_active) = {
            let list = self.downloads.read().unwrap();
            let rec = list
                .iter()
                .find(|d| d.id == id)
                .context("Download not found")?;
            (rec.output_path(), rec.target_dir.clone(), rec.state.is_active())
        };
        if is_active {
            bail!("Pause download before renaming");
        }
        let new_out = target_dir.join(&cleaned);
        if old_out.exists() && old_out != new_out {
            std::fs::rename(&old_out, &new_out)?;
        }
        {
            let mut list = self.downloads.write().unwrap();
            if let Some(rec) = list.iter_mut().find(|d| d.id == id) {
                rec.file_name = cleaned.clone();
                rec.title = cleaned;
                rec.updated_at = Utc::now();
                self.db.upsert_download(rec)?;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(new_out)
    }

    pub async fn add_bt_trackers(self: &Arc<Self>, id: DownloadId, trackers: Vec<String>) -> Result<()> {
        let was_active;
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            was_active = rec.state.is_running_or_seeding();
            for tr in trackers {
                let t = tr.trim().to_string();
                if !t.is_empty() && !rec.trackers.iter().any(|x| x.url == t) {
                    rec.trackers.push(TrackerInfo {
                        url: t,
                        status: "Added".into(),
                        peers: 0,
                        message: None,
                    });
                }
            }
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        if was_active {
            let _ = self.force_reannounce_bt(id).await;
        }
        Ok(())
    }

    pub async fn force_reannounce_bt(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        let is_bt = self
            .get_download(id)
            .map(|r| r.download_type == DownloadType::BitTorrent)
            .unwrap_or(false);
        if !is_bt {
            return Ok(());
        }
        let was_running = self
            .get_download(id)
            .map(|r| r.state.is_running_or_seeding())
            .unwrap_or(false);
        if was_running {
            let _ = self.pause_download(id).await;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let _ = self.resume_download(id).await;
        }
        Ok(())
    }

    pub fn toggle_bt_disable_seeding(&self, id: DownloadId, val: bool) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == id)
            .context("Download not found")?;
        rec.flags.bt_disable_seeding = val;
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub fn toggle_bt_ignore_ratio(&self, id: DownloadId, val: bool) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == id)
            .context("Download not found")?;
        rec.flags.bt_ignore_upload_ratio = val;
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub fn toggle_disable_auto_retry(&self, id: DownloadId, allow_auto_retry: bool) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == id)
            .context("Download not found")?;
        rec.flags.disable_auto_retry = !allow_auto_retry;
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub fn reset_settings_to_defaults(self: &Arc<Self>) -> Result<()> {
        self.update_config(FdmConfig::default())
    }

    pub fn export_downloads_formatted(
        &self,
        path: &Path,
        format: &str,
        selected_ids: Option<&HashSet<DownloadId>>,
        only_completed: bool,
    ) -> Result<usize> {
        let items: Vec<DownloadRecord> = self
            .downloads
            .read()
            .unwrap()
            .iter()
            .filter(|d| {
                selected_ids
                    .map(|set| set.contains(&d.id))
                    .unwrap_or(true)
            })
            .filter(|d| !only_completed || d.state == DownloadState::Completed)
            .cloned()
            .collect();
        let n = items.len();
        match format.to_ascii_lowercase().as_str() {
            "txt" | "urls" => export_downloads_urls_txt(&items, path)?,
            "csv" => export_downloads_csv(&items, path)?,
            _ => export_downloads_json(&items, path)?,
        }
        Ok(n)
    }

    pub fn create_portable_bundle(&self, target_dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(target_dir)?;
        let data_dst = target_dir.join("ferrite-data");
        std::fs::create_dir_all(&data_dst)?;
        self.db.backup_to(&data_dst.join("ferrite.sqlite"))?;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(name) = exe.file_name() {
                let _ = std::fs::copy(&exe, target_dir.join(name));
            }
        }
        let readme = target_dir.join("PORTABLE.txt");
        let _ = std::fs::write(
            &readme,
            "Ferrite DM Portable Bundle\nRun with --portable or --data-dir ./ferrite-data\n",
        );
        Ok(target_dir.to_path_buf())
    }

    pub fn archive_diagnostics_bundle(&self, dest_zip: &Path, logs_only: bool) -> Result<PathBuf> {
        if let Some(parent) = dest_zip.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let stage = self.data_dir.join(format!(".diag-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage)?;
        let log_file = self.data_dir.join("logs").join("ferrite.log");
        if log_file.is_file() {
            let _ = std::fs::copy(&log_file, stage.join("ferrite.log"));
        } else {
            let _ = std::fs::write(
                stage.join("ferrite.log"),
                format!("No log entries recorded yet (generated {}).\n", Utc::now().to_rfc3339()),
            );
        }
        if !logs_only {
            let _ = self.db.backup_to(&stage.join("ferrite.sqlite"));
            let cfg_json = serde_json::to_string_pretty(&self.get_config())?;
            let _ = std::fs::write(stage.join("config.redacted.json"), cfg_json);
        }
        let status = std::process::Command::new("zip")
            .args(["-q", "-r"])
            .arg(dest_zip)
            .arg(".")
            .current_dir(&stage)
            .output();
        let _ = std::fs::remove_dir_all(&stage);
        if matches!(status, Ok(ref o) if o.status.success()) {
            Ok(dest_zip.to_path_buf())
        } else {
            bail!("Failed creating diagnostic archive at {}", dest_zip.display())
        }
    }

    pub fn ensure_remote_password(&self) -> String {
        let mut cfg = self.get_config();
        if cfg.app.remote_access_password.trim().is_empty() {
            let secret = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
            cfg.app.remote_access_password = secret.clone();
            let _ = self.db.save_config(&cfg);
            let _ = self
                .db
                .save_rc_password_hash("default-pair", &sha256_str(&secret));
            *self.config.write().unwrap() = cfg;
            secret
        } else {
            let secret = cfg.app.remote_access_password.clone();
            let _ = self
                .db
                .save_rc_password_hash("default-pair", &sha256_str(&secret));
            secret
        }
    }

    pub fn regenerate_remote_password(self: &Arc<Self>) -> Result<String> {
        let mut cfg = self.get_config();
        let secret = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
        cfg.app.remote_access_password = secret.clone();
        self.db.save_config(&cfg)?;
        self.db
            .save_rc_password_hash("default-pair", &sha256_str(&secret))?;
        *self.config.write().unwrap() = cfg.clone();
        if let Some(ref mut pair) = *self.remote_pairing.lock().unwrap() {
            let host = Url::parse(&pair.https_url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "127.0.0.1".into());
            if let Ok(updated) = generate_pairing_info_with_secret(
                &cfg.app.remote_control_server_id,
                &host,
                cfg.app.remote_access_port,
                &secret,
            ) {
                *pair = updated;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::SettingsUpdated);
        Ok(secret)
    }

    pub fn export_downloads(&self, path: &Path) -> Result<usize> {
        let items = self.downloads.read().unwrap().clone();
        let n = items.len();
        export_downloads_json(&items, path)?;
        Ok(n)
    }

    pub async fn import_downloads(self: &Arc<Self>, path: &Path, start_paused: bool) -> Result<usize> {
        let cfg = self.get_config();
        let items = import_downloads_file(path, &cfg)?;
        let count = items.len();
        for mut rec in items {
            rec.id = 0;
            rec.uuid = uuid::Uuid::new_v4().to_string();
            rec.state = if start_paused {
                DownloadState::Paused
            } else {
                DownloadState::Queued
            };
            let saved = self.db.next_download_id_and_insert(rec)?;
            self.downloads.write().unwrap().push(saved.clone());
            let _ = self.events_tx.send(EngineEventAlias::Added { id: saved.id });
        }
        if !start_paused && count > 0 {
            self.schedule_queue().await;
        }
        Ok(count)
    }

    pub async fn import_legacy_fdm_db(self: &Arc<Self>, fdm_db_path: &Path) -> Result<usize> {
        let cfg = self.get_config();
        let items = inspect_legacy_fdm_sqlite(fdm_db_path, &cfg)?;
        let count = items.len();
        for mut rec in items {
            rec.id = 0;
            rec.uuid = uuid::Uuid::new_v4().to_string();
            rec.state = DownloadState::Paused;
            let saved = self.db.next_download_id_and_insert(rec)?;
            self.downloads.write().unwrap().push(saved.clone());
            let _ = self.events_tx.send(EngineEventAlias::Added { id: saved.id });
        }
        Ok(count)
    }

    pub fn export_settings(&self, path: &Path) -> Result<()> {
        let cfg = self.get_config();
        export_settings_json(&cfg, path)
    }

    pub fn import_settings(self: &Arc<Self>, path: &Path) -> Result<()> {
        let cfg = import_settings_json(path)?;
        self.update_config(cfg)
    }

    pub async fn schedule_queue(self: &Arc<Self>) {
        // Check low battery, network restrictions, and low disk space guards
        let cfg = self.get_config();
        let (on_battery, pct) = check_power_status().await;
        if on_battery && pct < cfg.core.battery_min_level_percent && cfg.core.battery_min_level_percent > 0 {
            *self.last_preventer.lock().unwrap() = AutoStartPreventer::LowBattery;
            return;
        }
        let (is_cellular, is_roaming) = detect_network_restrictions().await;
        if is_roaming && !cfg.core.allow_roaming_networks {
            *self.last_preventer.lock().unwrap() = AutoStartPreventer::NetworkRoaming;
            return;
        }
        if is_cellular && !cfg.core.allow_mobile_networks {
            *self.last_preventer.lock().unwrap() = AutoStartPreventer::NoAllowedNetworkInterface;
            return;
        }
        if !has_sufficient_disk_space(
            &cfg.core.default_download_path,
            cfg.core.low_disk_space_min_bytes,
            cfg.core.low_disk_space_min_percent,
        ) {
            *self.last_preventer.lock().unwrap() = AutoStartPreventer::LowDiskSpace;
            return;
        }
        *self.last_preventer.lock().unwrap() = AutoStartPreventer::None;

        let profile = cfg.active_profile();
        let base_max = profile.max_concurrent_downloads.max(1) as usize;

        let to_spawn: Vec<DownloadRecord> = {
            let mut list = self.downloads.write().unwrap();
            let mut running_main = 0usize;
            let mut running_small = 0usize;
            let mut running_probe = 0usize;
            let mut total_dl_speed = 0u64;

            for d in list.iter() {
                if d.state.is_active() {
                    total_dl_speed = total_dl_speed.saturating_add(d.download_speed_bps);
                    if matches!(d.state, DownloadState::Starting | DownloadState::Checking) {
                        running_probe += 1;
                    } else if d.total_bytes.map(|sz| sz <= 10 * 1024 * 1024).unwrap_or(false) {
                        running_small += 1;
                    } else {
                        running_main += 1;
                    }
                }
            }

            let slow_bonus = if running_main > 0
                && (total_dl_speed < 128 * 1024
                    || (profile.max_download_speed_bps > 0
                        && total_dl_speed < profile.max_download_speed_bps / 4))
            {
                cfg.core.max_additional_downloads_if_slow as usize
            } else {
                0
            };

            let max_total = base_max
                + (cfg.core.max_additional_small_downloads as usize).min(running_small + 1)
                + (cfg.core.max_additional_query_info_downloads as usize).min(running_probe + 1)
                + slow_bonus;
            let total_running = running_main + running_small + running_probe;
            if total_running >= max_total && running_main >= base_max {
                Vec::new()
            } else {
                let mut candidates: Vec<(usize, DownloadPriority, i64)> = list
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.state == DownloadState::Queued)
                    .filter(|(_, d)| {
                        has_sufficient_disk_space(
                            &d.target_dir,
                            cfg.core.low_disk_space_min_bytes,
                            cfg.core.low_disk_space_min_percent,
                        )
                    })
                    .map(|(idx, d)| (idx, d.priority, d.order_index))
                    .collect();
                let had_queued = list.iter().any(|d| d.state == DownloadState::Queued);
                if had_queued && candidates.is_empty() {
                    *self.last_preventer.lock().unwrap() = AutoStartPreventer::LowDiskSpace;
                }
                candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));

                let slots = max_total.saturating_sub(total_running).max(1);
                let mut picked = Vec::new();
                for (idx, _, _) in candidates.into_iter().take(slots) {
                    list[idx].state = DownloadState::Starting;
                    list[idx].updated_at = Utc::now();
                    let _ = self.db.upsert_download(&list[idx]);
                    picked.push(list[idx].clone());
                }
                picked
            }
        };

        for rec in to_spawn {
            self.log_dl_mgmt(&format!("Starting queued download #{} ({})", rec.id, rec.file_name));
            self.spawn_worker(rec);
        }
    }

    fn spawn_worker(self: &Arc<Self>, rec: DownloadRecord) {
        let id = rec.id;
        let (tx, rx) = watch::channel(ControlCommand::Run);
        self.controls.lock().unwrap().insert(id, tx);

        let eng = Arc::clone(self);
        let jh = tokio::spawn(async move {
            eng.run_single_download(rec, rx).await;
            eng.controls.lock().unwrap().remove(&id);
            eng.tasks.lock().unwrap().remove(&id);
            eng.schedule_queue().await;
        });
        self.tasks.lock().unwrap().insert(id, jh);
    }

    async fn run_single_download(
        self: &Arc<Self>,
        mut record: DownloadRecord,
        control_rx: watch::Receiver<ControlCommand>,
    ) {
        let cfg = self.get_config();
        let id = record.id;

        // Check existing file conflict before transferring
        if matches!(
            record.download_type,
            DownloadType::Www | DownloadType::HlsM3u8 | DownloadType::Dash | DownloadType::MediaParser
        ) && record.downloaded_bytes == 0
        {
            let reaction = record
                .module_data
                .get("existing_file_reaction")
                .and_then(|v| serde_json::from_value::<ExistingFileReaction>(v.clone()).ok())
                .unwrap_or(cfg.core.existing_file_reaction);
            let target = record.output_path();
            if target.exists() {
                match reaction {
                    ExistingFileReaction::Ask => {
                        record.state = DownloadState::Paused;
                        record.last_error = Some(format!("File already exists: {}", target.display()));
                        record.abort_reason = DownloadAbortReason::AlreadyExists;
                        self.pending_conflicts.lock().unwrap().insert(id);
                        self.sync_record_update(&record, true);
                        let _ = self.events_tx.send(EngineEventAlias::FileConflict { id, path: target });
                        return;
                    }
                    ExistingFileReaction::Skip => {
                        record.state = DownloadState::Stopped;
                        record.abort_reason = DownloadAbortReason::AlreadyExists;
                        self.sync_record_update(&record, true);
                        return;
                    }
                    ExistingFileReaction::Rename => {
                        if let Some(new_name) = resolve_existing_file_name(
                            &record.target_dir,
                            &record.file_name,
                            ExistingFileReaction::Rename,
                        ) {
                            record.file_name = new_name.clone();
                            record.title = new_name;
                            self.sync_record_update(&record, true);
                        }
                    }
                    ExistingFileReaction::Overwrite => {}
                }
            }
        }

        let limiter = self.limiter.clone();
        let ssl_exceptions = self.db.list_ssl_exceptions().unwrap_or_default();
        let cred = self.lookup_credential(&record.url);

        let last_db_flush = Arc::new(Mutex::new(tokio::time::Instant::now()));
        let emit_progress = |snapshot: &DownloadRecord| {
            let now = tokio::time::Instant::now();
            let mut flush = false;
            {
                let mut g = last_db_flush.lock().unwrap();
                if now.duration_since(*g).as_millis() >= 750 {
                    *g = now;
                    flush = true;
                }
            }
            self.sync_record_update(snapshot, flush);
        };

        let final_rec = match record.download_type {
            DownloadType::Www | DownloadType::Unknown => {
                download_http_or_ftp(
                    record,
                    &cfg,
                    &ssl_exceptions,
                    cred,
                    limiter,
                    control_rx,
                    emit_progress,
                )
                .await
            }
            DownloadType::HlsM3u8 => {
                download_hls_stream(record, &cfg, limiter, control_rx, emit_progress).await
            }
            DownloadType::Dash => {
                download_dash_stream(record, &cfg, limiter, control_rx, emit_progress).await
            }
            DownloadType::BitTorrent => match self.get_or_init_bt().await {
                Ok(bt) => {
                    bt.run_torrent_download(record, &cfg, control_rx, emit_progress)
                        .await
                }
                Err(e) => {
                    record.state = DownloadState::Error;
                    record.last_error = Some(format!("BitTorrent init error: {e}"));
                    record
                }
            },
            DownloadType::MediaParser => {
                self.run_media_parser_download(record, &cfg, &ssl_exceptions, cred, limiter, control_rx, emit_progress)
                    .await
            }
            DownloadType::Batch | DownloadType::YoutubeChannel => {
                record.state = DownloadState::Completed;
                record.completed_at = Some(Utc::now());
                record
            }
        };

        self.sync_record_update(&final_rec, true);

        match final_rec.state {
            DownloadState::Completed => {
                self.log_dl_mgmt(&format!(
                    "Completed download #{} ({}) [{} bytes]",
                    id, final_rec.file_name, final_rec.downloaded_bytes
                ));
                if final_rec.download_type == DownloadType::BitTorrent {
                    self.log_lt_alert(&format!(
                        "Torrent #{} ({}) finished downloading",
                        id, final_rec.file_name
                    ));
                }
                let _ = self.events_tx.send(EngineEventAlias::Completed { id });
                self.run_post_completion_tasks(&final_rec, &cfg).await;
            }
            DownloadState::Error => {
                let err_msg = final_rec
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "Download failed".into());
                self.log_dl_mgmt(&format!("Failed download #{id} ({}): {err_msg}", final_rec.file_name));
                self.log_enhanced(&format!(
                    "Failure detail id={id} reason={:?} retries={}",
                    final_rec.abort_reason, final_rec.retry_count
                ));
                let _ = self.events_tx.send(EngineEventAlias::Failed { id, error: err_msg });
            }
            DownloadState::Paused | DownloadState::Stopped => {
                self.log_dl_mgmt(&format!(
                    "Download #{} transitioned to {:?}",
                    id, final_rec.state
                ));
            }
            _ => {}
        }
    }

    async fn run_media_parser_download<F>(
        &self,
        mut record: DownloadRecord,
        cfg: &FdmConfig,
        ssl_exceptions: &[SslException],
        cred: Option<AuthCredential>,
        limiter: BandwidthLimiter,
        control_rx: watch::Receiver<ControlCommand>,
        on_progress: F,
    ) -> DownloadRecord
    where
        F: FnMut(&DownloadRecord) + Send,
    {
        match probe_media_url(&record.url, &self.plugins_dir, cfg).await {
            Ok(parsed) if !parsed.variants.is_empty() => {
                let best = parsed
                    .variants
                    .iter()
                    .max_by_key(|v| (v.height.unwrap_or(0), v.bitrate_kbps.unwrap_or(0)))
                    .cloned()
                    .unwrap();
                if record.title.is_empty() || record.title == "download.bin" {
                    record.title = parsed.title.clone();
                }
                let ext = if best.ext.is_empty() {
                    "mp4"
                } else {
                    best.ext.as_str()
                };
                if !record.file_name.ends_with(&format!(".{ext}")) {
                    record.file_name = sanitize_filename(&format!("{}.{}", parsed.title, ext));
                }

                if best.url.to_ascii_lowercase().contains(".m3u8") {
                    record.url = best.url;
                    return download_hls_stream(record, cfg, limiter, control_rx, on_progress).await;
                }

                if let Some(ref aud_url) = best.audio_url {
                    // Download video & audio temporarily and mux via FFmpeg
                    let orig_dir = record.target_dir.clone();
                    let orig_name = record.file_name.clone();
                    let tmp_dir = orig_dir.join(format!(".{}.media_tmp", record.uuid));
                    let _ = std::fs::create_dir_all(&tmp_dir);

                    let mut v_rec = record.clone();
                    v_rec.url = best.url.clone();
                    v_rec.http.url = best.url.clone();
                    v_rec.target_dir = tmp_dir.clone();
                    v_rec.file_name = "video.mp4".into();
                    let v_done = download_http_or_ftp(
                        v_rec,
                        cfg,
                        ssl_exceptions,
                        cred.clone(),
                        limiter.clone(),
                        control_rx.clone(),
                        on_progress,
                    )
                    .await;
                    if v_done.state != DownloadState::Completed {
                        return v_done;
                    }

                    let mut a_rec = record.clone();
                    a_rec.url = aud_url.clone();
                    a_rec.http.url = aud_url.clone();
                    a_rec.target_dir = tmp_dir.clone();
                    a_rec.file_name = "audio.m4a".into();
                    let a_done = download_http_or_ftp(
                        a_rec,
                        cfg,
                        ssl_exceptions,
                        cred,
                        limiter,
                        control_rx,
                        |_| {},
                    )
                    .await;
                    if a_done.state != DownloadState::Completed {
                        return a_done;
                    }

                    let final_out = orig_dir.join(&orig_name);
                    std::fs::create_dir_all(&orig_dir).ok();
                    if merge_av_files(cfg, &tmp_dir.join("video.mp4"), &tmp_dir.join("audio.m4a"), &final_out)
                        .await
                        .is_ok()
                    {
                        if cfg.core.delete_service_files_when_finished {
                            let _ = std::fs::remove_dir_all(&tmp_dir);
                        }
                        if cfg.core.set_mark_of_the_web {
                            let _ = set_mark_of_the_web(&final_out, &record.url, "FerriteDM");
                        }
                        record.target_dir = orig_dir;
                        record.file_name = orig_name;
                        record.state = DownloadState::Completed;
                        record.downloaded_bytes =
                            std::fs::metadata(&final_out).map(|m| m.len()).unwrap_or(0);
                        record.total_bytes = Some(record.downloaded_bytes);
                        record.completed_at = Some(Utc::now());
                        return record;
                    }
                    let _ = std::fs::rename(tmp_dir.join("video.mp4"), &final_out);
                    if cfg.core.delete_service_files_when_finished {
                        let _ = std::fs::remove_dir_all(&tmp_dir);
                    }
                    if cfg.core.set_mark_of_the_web {
                        let _ = set_mark_of_the_web(&final_out, &record.url, "FerriteDM");
                    }
                    record.state = DownloadState::Completed;
                    record.completed_at = Some(Utc::now());
                    return record;
                }

                record.url = best.url.clone();
                record.http.url = best.url;
                download_http_or_ftp(
                    record,
                    cfg,
                    ssl_exceptions,
                    cred,
                    limiter,
                    control_rx,
                    on_progress,
                )
                .await
            }
            _ => {
                // Fallback direct HTTP download
                download_http_or_ftp(
                    record,
                    cfg,
                    ssl_exceptions,
                    cred,
                    limiter,
                    control_rx,
                    on_progress,
                )
                .await
            }
        }
    }

    async fn run_post_completion_tasks(&self, record: &DownloadRecord, cfg: &FdmConfig) {
        if cfg.core.disable_post_finished_tasks {
            return;
        }
        if cfg.core.disable_post_finished_on_battery {
            let (on_battery, _) = check_power_status().await;
            if on_battery {
                return;
            }
        }
        if cfg.core.auto_launch_antivirus {
            if let Some(ref av_path) = cfg.core.antivirus_path {
                let _ = scan_file_with_antivirus(av_path, &cfg.core.antivirus_args, record).await;
            }
        }
        if cfg.core.launch_external_on_completion {
            if let Some(ref ext_app) = cfg.core.external_app_path {
                let _ = run_external_completion_app(
                    ext_app,
                    &cfg.core.external_app_args_template,
                    record,
                )
                .await;
            }
        }
        if cfg.core.shutdown_when_done != ShutdownType::None {
            let any_left = self
                .downloads
                .read()
                .unwrap()
                .iter()
                .any(|d| d.state.is_active() || d.state == DownloadState::Queued);
            if !any_left {
                if cfg.core.shutdown_when_done == ShutdownType::QuitApp {
                    self.log_dl_mgmt("All downloads complete: requesting application quit");
                    let _ = self.events_tx.send(EngineEventAlias::QuitRequested);
                } else {
                    let _ = perform_shutdown_action(cfg.core.shutdown_when_done).await;
                }
            }
        }
    }

    fn sync_record_update(&self, snapshot: &DownloadRecord, flush_db: bool) {
        {
            let mut list = self.downloads.write().unwrap();
            if let Some(slot) = list.iter_mut().find(|d| d.id == snapshot.id) {
                *slot = snapshot.clone();
            }
        }
        if flush_db {
            let _ = self.db.upsert_download(snapshot);
        }
        let _ = self
            .events_tx
            .send(EngineEventAlias::Updated { id: snapshot.id });
    }

    pub fn spawn_supervisor(self: &Arc<Self>) -> JoinHandle<()> {
        let eng = Arc::clone(self);
        tokio::spawn(async move {
        let mut tick_count: u64 = 0;
        let mut last_backup_tick: u64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            tick_count += 1;

            let cfg = eng.get_config();
            if cfg.app.db_backup_interval_secs > 0
                && tick_count.saturating_sub(last_backup_tick)
                    >= cfg.app.db_backup_interval_secs as u64
            {
                last_backup_tick = tick_count;
                let backups_dir = eng.data_dir.join("backups");
                let backup_name = format!(
                    "ferrite-backup-{}.sqlite",
                    Local::now().format("%Y%m%d-%H%M%S")
                );
                let _ = eng.db.backup_to(&backups_dir.join(backup_name));
                if let Ok(rd) = std::fs::read_dir(&backups_dir) {
                    let mut files: Vec<_> = rd
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_file())
                        .collect();
                    files.sort();
                    while files.len() > 5 {
                        let old = files.remove(0);
                        let _ = std::fs::remove_file(old);
                    }
                }
            }
                let mut dl_bps = 0u64;
                let mut ul_bps = 0u64;
                let mut active_count = 0usize;
                let mut prevent_sleep = false;
                let mut to_start = Vec::new();
                let mut to_pause = Vec::new();
                let mut to_remove_expired = Vec::new();

                {
                    let mut list = eng.downloads.write().unwrap();
                    let now_local = Local::now();
                    let now_utc = Utc::now();

                    for d in list.iter_mut() {
                        dl_bps = dl_bps.saturating_add(d.download_speed_bps);
                        ul_bps = ul_bps.saturating_add(d.upload_speed_bps);
                        if d.state.is_active() {
                            active_count += 1;
                        }

                        if cfg.core.prevent_sleep_if_running {
                            let is_running = d.state.is_active()
                                || (d.state == DownloadState::Scheduled
                                    && cfg.core.prevent_sleep_include_scheduled);
                            if is_running {
                                let ignore_resumable = cfg.core.prevent_sleep_ignore_resumable
                                    && d.resume_support == ResumeSupport::Present;
                                let ignore_fin = cfg.core.prevent_sleep_ignore_finished
                                    && d.state == DownloadState::Completed;
                                if !ignore_resumable && !ignore_fin {
                                    prevent_sleep = true;
                                }
                            }
                        }

                        // Scheduler tick check
                        if let Some(ref rule) = d.schedule {
                            if rule.enabled {
                                let active_now = is_schedule_active(rule, now_local);
                                if active_now && d.state == DownloadState::Scheduled {
                                    to_start.push(d.id);
                                } else if !active_now && d.state.is_active() {
                                    to_pause.push(d.id);
                                }
                            }
                        }

                        // Periodic checks every 5 seconds: missing files & expired auto-remove
                        if tick_count % 5 == 0 && d.state == DownloadState::Completed {
                            let missing = !d.output_path().exists();
                            if d.flags.is_missing_files != missing {
                                d.flags.is_missing_files = missing;
                                let _ = eng.db.upsert_download(d);
                                let _ = eng.events_tx.send(EngineEventAlias::Updated { id: d.id });
                            }
                            if missing && cfg.core.auto_remove_missing_files {
                                to_remove_expired.push(d.id);
                            }
                            if cfg.core.auto_remove_finished {
                                if let Some(done_at) = d.completed_at {
                                    let age = now_utc.signed_duration_since(done_at).num_days();
                                    if age >= cfg.core.auto_remove_finished_keep_days as i64 {
                                        to_remove_expired.push(d.id);
                                    }
                                }
                            }
                        }
                    }
                }

                {
                    let mut hist = eng.speed_history.lock().unwrap();
                    if hist.len() >= 120 {
                        hist.pop_front();
                    }
                    hist.push_back((dl_bps, ul_bps));
                }

                eng.sleep_preventer.lock().unwrap().update(prevent_sleep);

                let preventer = *eng.last_preventer.lock().unwrap();
                let _ = eng.events_tx.send(EngineEventAlias::QueueStats {
                    total_download_bps: dl_bps,
                    total_upload_bps: ul_bps,
                    active_count,
                    preventer,
                });

                for id in to_start {
                    let _ = eng.resume_download(id).await;
                }
                for id in to_pause {
                    let _ = eng.pause_download(id).await;
                }
                for id in to_remove_expired {
                    let _ = eng.remove_download(id, false).await;
                }

                // Torrent watch folder scanner
                if tick_count % 3 == 0 {
                    if let Some(ref watch_dir) = cfg.app.torrents_watch_folder {
                        let items = {
                            let mut seen = eng.seen_watch_files.lock().unwrap();
                            scan_torrent_watch_folder(watch_dir, &mut seen)
                        };
                        for item in items {
                            let mut req = NewDownloadRequest::from_url(item.source);
                            req.silent = cfg.app.force_silent_from_torrent_folder;
                            let _ = eng.add_download(req).await;
                        }
                    }
                }

                eng.schedule_queue().await;
            }
        })
    }

    pub async fn start_ipc_listener(self: &Arc<Self>) -> Result<JoinHandle<()>> {
        let sock = default_socket_path(&self.data_dir);
        let eng = Arc::clone(self);
        start_ipc_server(
            sock,
            Arc::new(move |req| {
                let eng = Arc::clone(&eng);
                async move { eng.handle_ipc_request(req).await }
            }),
        )
        .await
    }

    pub async fn ensure_remote_control(self: &Arc<Self>, host_hint: &str) -> Result<RemotePairingInfo> {
        let cfg = self.get_config();
        let secret = self.ensure_remote_password();
        let bind_ip: std::net::IpAddr = cfg
            .app
            .remote_access_bind
            .parse()
            .unwrap_or_else(|_| "0.0.0.0".parse().unwrap());
        let bind_addr = SocketAddr::new(bind_ip, cfg.app.remote_access_port);

        let pairing = generate_pairing_info_with_secret(
            &cfg.app.remote_control_server_id,
            host_hint,
            cfg.app.remote_access_port,
            &secret,
        )?;

        let prev_addr = *self.remote_target_addr.lock().unwrap();
        if self.remote_handle.lock().unwrap().is_some() && prev_addr != Some(bind_addr) {
            self.stop_remote_control();
        }

        if self.remote_handle.lock().unwrap().is_none() {
            let db_clone = self.db.clone();
            let eng = Arc::clone(self);
            let cert_dir = self.data_dir.join("certs");
            let verify_fn = Arc::new(move |tok: &str| {
                let cand = sha256_str(tok);
                db_clone
                    .list_rc_passwords()
                    .map(|m| m.values().any(|h| h.eq_ignore_ascii_case(&cand)))
                    .unwrap_or(false)
            });
            let (bound_addr, handle) = start_remote_control_server(
                bind_addr,
                &cert_dir,
                verify_fn,
                Arc::new(move |req| {
                    let eng = Arc::clone(&eng);
                    async move { eng.handle_ipc_request(req).await }
                }),
            )
            .await?;

            // Recompute pairing URLs with actual bound port (helpful when port 0 is passed)
            let updated_pair = generate_pairing_info_with_secret(
                &cfg.app.remote_control_server_id,
                host_hint,
                bound_addr.port(),
                &secret,
            )?;
            *self.remote_pairing.lock().unwrap() = Some(updated_pair.clone());
            *self.remote_handle.lock().unwrap() = Some(handle);
            *self.remote_target_addr.lock().unwrap() = Some(bind_addr);
            return Ok(updated_pair);
        }

        *self.remote_pairing.lock().unwrap() = Some(pairing.clone());
        Ok(pairing)
    }

    pub async fn handle_ipc_request(self: &Arc<Self>, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::Ping => IpcResponse::ok(serde_json::json!({ "pong": true, "app": "FerriteDM" })),
            IpcRequest::AddDownload(dl) => match self.add_download(dl).await {
                Ok(rec) => IpcResponse::ok(rec),
                Err(e) => IpcResponse::err("add_failed", e.to_string()),
            },
            IpcRequest::ListDownloads(q) => IpcResponse::ok(self.list_downloads(&q)),
            IpcRequest::GetDownload { id } => match self.get_download(id) {
                Some(rec) => IpcResponse::ok(rec),
                None => IpcResponse::err("not_found", format!("Download {id} not found")),
            },
            IpcRequest::Pause { id } => match self.pause_download(id).await {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("pause_failed", e.to_string()),
            },
            IpcRequest::Resume { id } => match self.resume_download(id).await {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("resume_failed", e.to_string()),
            },
            IpcRequest::Cancel { id } => match self.cancel_download(id).await {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("cancel_failed", e.to_string()),
            },
            IpcRequest::Remove { id, delete_files } => {
                match self.remove_download(id, delete_files).await {
                    Ok(()) => IpcResponse::ok(true),
                    Err(e) => IpcResponse::err("remove_failed", e.to_string()),
                }
            }
            IpcRequest::StartAll => match self.start_all().await {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("start_all_failed", e.to_string()),
            },
            IpcRequest::PauseAll => match self.pause_all().await {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("pause_all_failed", e.to_string()),
            },
            IpcRequest::SetTrafficUsageMode { mode } => {
                match self.set_traffic_usage_mode(mode) {
                    Ok(()) => IpcResponse::ok(mode),
                    Err(e) => IpcResponse::err("tum_failed", e.to_string()),
                }
            }
            IpcRequest::GetConfig => IpcResponse::ok(self.get_config()),
            IpcRequest::UpdateConfig(cfg) => match self.update_config(cfg) {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("config_failed", e.to_string()),
            },
            IpcRequest::ListTags => match self.list_tags() {
                Ok(tags) => IpcResponse::ok(tags),
                Err(e) => IpcResponse::err("tags_failed", e.to_string()),
            },
            IpcRequest::CreateTag {
                name,
                color_hex,
                file_types: _,
            } => match self.create_tag(&name, &color_hex) {
                Ok(tag) => IpcResponse::ok(tag),
                Err(e) => IpcResponse::err("create_tag_failed", e.to_string()),
            },
            IpcRequest::AssignTag { id, tag_id } => match self.assign_tag(id, tag_id) {
                Ok(()) => IpcResponse::ok(true),
                Err(e) => IpcResponse::err("assign_tag_failed", e.to_string()),
            },
            IpcRequest::ResolveConflict { id, action } => {
                match self.resolve_conflict(id, action).await {
                    Ok(()) => IpcResponse::ok(true),
                    Err(e) => IpcResponse::err("conflict_failed", e.to_string()),
                }
            }
            IpcRequest::BrowserQuerySettings => {
                let cfg = self.get_config();
                IpcResponse::ok(build_browser_settings_payload(&cfg))
            }
            IpcRequest::BrowserPostSettings(patch) => {
                let mut cfg = self.get_config();
                apply_browser_settings_patch(&mut cfg, &patch);
                match self.update_config(cfg.clone()) {
                    Ok(()) => IpcResponse::ok(build_browser_settings_payload(&cfg)),
                    Err(e) => IpcResponse::err("post_settings_failed", e.to_string()),
                }
            }
            IpcRequest::RemoteCommand { action, payload } => {
                IpcResponse::ok(serde_json::json!({ "action": action, "payload": payload, "summary": self.summary() }))
            }
            IpcRequest::RaiseWindow => {
                self.emit_raise();
                IpcResponse::ok(serde_json::json!({ "raised": true }))
            }
        }
    }

    pub fn format_human_bytes(bytes: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;
        let b = bytes as f64;
        if b >= GB {
            format!("{:.2} GB", b / GB)
        } else if b >= MB {
            format!("{:.2} MB", b / MB)
        } else if b >= KB {
            format!("{:.1} KB", b / KB)
        } else {
            format!("{bytes} B")
        }
    }

    pub fn verify_path_hash(path: &Path, algo: HashAlgorithm, expected: &str) -> Result<bool> {
        verify_file_hash(path, algo, expected)
    }

    pub fn set_schedule(&self, id: DownloadId, schedule: Option<ScheduleRule>) -> Result<()> {
        let mut list = self.downloads.write().unwrap();
        let rec = list
            .iter_mut()
            .find(|d| d.id == id)
            .context("Download not found")?;
        rec.schedule = schedule;
        rec.updated_at = Utc::now();
        self.db.upsert_download(rec)?;
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn update_torrent_file_priority(
        &self,
        id: DownloadId,
        file_index: usize,
        priority: DownloadPriority,
    ) -> Result<()> {
        let (info_hash, selected_indices) = {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            let mut found = false;
            for f in &mut rec.files {
                if f.index == file_index {
                    f.priority = priority;
                    found = true;
                }
            }
            if !found && !rec.files.is_empty() {
                bail!("File index {file_index} not found in download {id}");
            }
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
            let selected: HashSet<usize> = rec
                .files
                .iter()
                .filter(|f| f.priority != DownloadPriority::DontDownload)
                .map(|f| f.index)
                .collect();
            (rec.torrent_info_hash.clone(), selected)
        };
        if let Some(ih) = info_hash {
            if let Some(ref bt) = *self.bt_manager.lock().await {
                let _ = bt.update_file_selection(&ih, selected_indices).await;
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(())
    }

    pub async fn check_download_virus(self: &Arc<Self>, id: DownloadId) -> Result<String> {
        let cfg = self.get_config();
        let rec = self.get_download(id).context("Download not found")?;
        let Some(ref av_path) = cfg.core.antivirus_path else {
            bail!("No antivirus configured. Choose one in Preferences -> Antivirus.");
        };
        if av_path.as_os_str().is_empty() {
            bail!("No antivirus configured. Choose one in Preferences -> Antivirus.");
        }
        let out_path = rec.output_path();
        if !out_path.exists() {
            bail!("File does not exist at {}", out_path.display());
        }
        scan_file_with_antivirus(av_path, &cfg.core.antivirus_args, &rec).await?;
        Ok(format!(
            "Clean (no threats detected by antivirus in {})",
            rec.file_name
        ))
    }

    pub async fn check_download_for_update(self: &Arc<Self>, id: DownloadId) -> Result<bool> {
        let cfg = self.get_config();
        let rec = self.get_download(id).context("Download not found")?;
        if rec.url.trim().is_empty() || rec.download_type == DownloadType::BitTorrent {
            bail!("Update check is only supported for HTTP/FTP downloads");
        }
        let ssl = self.db.list_ssl_exceptions().unwrap_or_default();
        let cred = self.lookup_credential(&rec.url);
        let client = build_http_client(&cfg, &ssl, None)?;
        let probe = probe_http_resource(&client, &rec, &ssl, cred.as_ref()).await?;

        let etag_changed = matches!((&rec.etag, &probe.etag), (Some(a), Some(b)) if a != b);
        let lm_changed =
            matches!((&rec.last_modified, &probe.last_modified), (Some(a), Some(b)) if a != b);
        let size_changed =
            matches!((rec.total_bytes, probe.total_bytes), (Some(a), Some(b)) if a != b);
        let changed = etag_changed || lm_changed || size_changed;

        {
            let mut list = self.downloads.write().unwrap();
            if let Some(slot) = list.iter_mut().find(|d| d.id == id) {
                slot.flags.can_detect_remote_changes = probe.etag.is_some()
                    || probe.last_modified.is_some()
                    || probe.total_bytes.is_some();
                if changed {
                    slot.last_error = Some("Remote file updated on server".into());
                }
                slot.updated_at = Utc::now();
                let _ = self.db.upsert_download(slot);
            }
        }
        let _ = self.events_tx.send(EngineEventAlias::Updated { id });
        Ok(changed)
    }

    pub async fn save_and_complete_download(self: &Arc<Self>, id: DownloadId) -> Result<()> {
        let _ = self.pause_download(id).await;
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        {
            let mut list = self.downloads.write().unwrap();
            let rec = list
                .iter_mut()
                .find(|d| d.id == id)
                .context("Download not found")?;
            let part = rec.partial_path();
            let out = rec.output_path();
            if part != out && part.is_file() {
                let _ = std::fs::rename(&part, &out);
            }
            if out.is_file() {
                if let Ok(meta) = std::fs::metadata(&out) {
                    rec.downloaded_bytes = meta.len();
                    rec.total_bytes = Some(meta.len());
                }
                rec.flags.is_missing_files = false;
            }
            rec.state = DownloadState::Completed;
            rec.download_speed_bps = 0;
            rec.connections_active = 0;
            rec.last_error = None;
            rec.abort_reason = DownloadAbortReason::None;
            rec.completed_at = Some(Utc::now());
            rec.updated_at = Utc::now();
            self.db.upsert_download(rec)?;
        }
        let _ = self.events_tx.send(EngineEventAlias::Completed { id });
        self.schedule_queue().await;
        Ok(())
    }

    pub fn stop_remote_control(&self) {
        if let Some(handle) = self.remote_handle.lock().unwrap().take() {
            handle.abort();
        }
        *self.remote_target_addr.lock().unwrap() = None;
        *self.remote_pairing.lock().unwrap() = None;
    }

    pub fn remove_ssl_exception(&self, host_port: &str) -> Result<()> {
        self.db.remove_ssl_exception(host_port)
    }

    pub fn clear_ssl_exceptions(&self) -> Result<()> {
        self.db.clear_ssl_exceptions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn engine_enforces_batch_cap_disk_guard_logging_and_quit_event() {
        let dir = std::env::temp_dir().join(format!("ferrite_eng_unit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let eng = DownloadEngine::open(dir.join("data"), true).unwrap();
        let mut cfg = eng.get_config();
        cfg.app.batch_download_max_urls_count = 3;
        cfg.app.enable_logging = true;
        cfg.app.downloads_mgmt_logging = true;
        cfg.core.traffic_limits_compliant_mode = false;
        cfg.core.default_download_path = dir.join("downloads");
        std::fs::create_dir_all(&cfg.core.default_download_path).unwrap();
        eng.update_config(cfg).unwrap();
        assert!(!eng.limiter.compliant_mode());

        // Batch limit truncates 10 expanded URLs down to 3.
        let batch = eng
            .add_batch_pattern("https://example.org/file_[1-10].bin", None, true)
            .await
            .unwrap();
        assert_eq!(batch.len(), 3);

        // Downloads management logging wrote entries into logs/ferrite.log.
        let log_txt = std::fs::read_to_string(dir.join("data/logs/ferrite.log")).unwrap();
        assert!(log_txt.contains("Added batch of 3 downloads"));

        // Unachievable min disk bytes blocks queued starts with AutoStartPreventer::LowDiskSpace.
        let mut cfg2 = eng.get_config();
        cfg2.core.low_disk_space_min_bytes = u64::MAX;
        eng.update_config(cfg2).unwrap();
        let mut req = NewDownloadRequest::from_url("https://example.org/blocked.bin");
        req.start_paused = true;
        let rec = eng.add_download(req).await.unwrap();
        eng.resume_download(rec.id).await.unwrap();
        assert_eq!(eng.summary().preventer, AutoStartPreventer::LowDiskSpace);
        assert_eq!(eng.get_download(rec.id).unwrap().state, DownloadState::Queued);

        // ShutdownType::QuitApp emits EngineEvent::QuitRequested once last download finishes.
        let mut rx = eng.subscribe();
        eng.pause_download(rec.id).await.unwrap();
        let mut cfg3 = eng.get_config();
        cfg3.core.low_disk_space_min_bytes = 0;
        cfg3.core.low_disk_space_min_percent = 0.0;
        cfg3.core.shutdown_when_done = ShutdownType::QuitApp;
        eng.update_config(cfg3.clone()).unwrap();
        let finished_rec = eng.get_download(rec.id).unwrap();
        eng.run_post_completion_tasks(&finished_rec, &cfg3).await;
        let mut saw_quit = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, EngineEventAlias::QuitRequested) {
                saw_quit = true;
                break;
            }
        }
        assert!(saw_quit, "expected QuitRequested event when QuitApp active");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn engine_updates_rc_hash_remembers_host_dir_and_stages_app_updates() {
        let dir = std::env::temp_dir().join(format!("ferrite_eng_set_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let eng = DownloadEngine::open(dir.join("data"), true).unwrap();

        // 1) Updating remote_access_password syncs DB rc_password_hash immediately.
        let mut cfg = eng.get_config();
        cfg.app.remote_access_password = "CustomSecret123".into();
        cfg.core.fixed_download_path = false;
        cfg.core.download_path_depends_on_source_url = true;
        cfg.app.auto_install_updates = true;
        eng.update_config(cfg).unwrap();
        let rc_map = eng.db().list_rc_passwords().unwrap();
        assert_eq!(
            rc_map.get("default-pair").map(String::as_str),
            Some(sha256_str("CustomSecret123").as_str())
        );

        // 2) Choosing an explicit target_dir on add_download remembers per-host preference.
        let custom_dir = dir.join("ubuntu-isos");
        std::fs::create_dir_all(&custom_dir).unwrap();
        let mut req = NewDownloadRequest::from_url("https://releases.ubuntu.com/24.04/ubuntu.iso");
        req.target_dir = Some(custom_dir.clone());
        req.start_paused = true;
        let _ = eng.add_download(req).await.unwrap();
        let resolved = resolve_target_dir(
            &eng.get_config(),
            None,
            "https://releases.ubuntu.com/24.04/other.iso",
            "other.iso",
            None,
        );
        assert_eq!(resolved, custom_dir);

        // 3) Unconfigured update check does zero network calls and reports offline mode.
        let uncfg = eng.check_for_app_updates(None).await.unwrap();
        assert!(!uncfg.configured);
        assert!(!uncfg.update_available);

        // 4) Local update manifest + payload stages update when auto_install_updates = true.
        let payload = dir.join("ferrite-dm-99.0.0.pkg");
        std::fs::write(&payload, b"ferrite-update-binary").unwrap();
        let sha = compute_file_hash(&payload, HashAlgorithm::Sha256).unwrap();
        let manifest_path = dir.join("data/update-manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::json!({
                "version": "99.0.0",
                "url": payload.to_string_lossy(),
                "sha256": sha,
                "notes": "Test release"
            })
            .to_string(),
        )
        .unwrap();
        let upd = eng.check_for_app_updates(None).await.unwrap();
        assert!(upd.configured);
        assert!(upd.update_available);
        assert_eq!(upd.latest_version.as_deref(), Some("99.0.0"));
        assert!(upd.staged_path.as_ref().is_some_and(|p| p.is_file()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
