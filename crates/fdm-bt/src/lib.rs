use anyhow::{Context, Result};
use base64::Engine as _;
use fdm_core::{
    DownloadAbortReason, DownloadFileEntry, DownloadPriority, DownloadRecord, DownloadState,
    FdmConfig, FileType, PeerInfo, TrackerInfo,
    os_integration::set_mark_of_the_web,
    routing::classify_file_type,
};
use fdm_http::ControlCommand;
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::Id20;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode,
    ListenerOptions, Magnet, ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig,
    TorrentStatsState, dht::DhtPersistenceConfig, torrent_from_bytes,
};
use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::watch;

type ManagedTorrentHandle = Arc<ManagedTorrent>;

#[derive(Debug, Clone)]
pub struct TorrentInspection {
    pub info_hash: String,
    pub name: String,
    pub total_bytes: u64,
    pub files: Vec<DownloadFileEntry>,
    pub trackers: Vec<String>,
    pub raw_torrent_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct WatchFolderItem {
    pub path: PathBuf,
    pub is_magnet_file: bool,
    pub source: String,
}

fn non_zero_limit(bps: u64) -> Option<NonZeroU32> {
    if bps == 0 {
        None
    } else {
        NonZeroU32::new(bps.min(u32::MAX as u64) as u32)
    }
}

pub fn build_session_options(config: &FdmConfig, persistence_dir: Option<PathBuf>) -> SessionOptions {
    let profile = config.active_profile();
    let mode = if config.core.bt_enable_utp {
        ListenerMode::TcpAndUtp
    } else {
        ListenerMode::TcpOnly
    };
    let listen_port = if config.core.bt_use_random_port {
        0
    } else {
        config.core.bt_session_port
    };
    let listen_addr: SocketAddr = (Ipv6Addr::UNSPECIFIED, listen_port).into();

    let dht = if config.core.bt_enable_dht {
        let mut d = DhtSessionConfig::default();
        if let Some(ref p) = persistence_dir {
            d.persistence = Some(DhtPersistenceConfig {
                config_filename: Some(p.join("dht.json")),
                ..Default::default()
            });
        }
        Some(d)
    } else {
        None
    };

    let persistence = persistence_dir.map(|folder| SessionPersistenceConfig::Json {
        folder: Some(folder),
    });

    let mut trackers = HashSet::new();
    if config.core.bt_enable_tracker_list {
        for tr in &config.core.bt_tracker_list {
            if let Ok(u) = url::Url::parse(tr.trim()) {
                trackers.insert(u);
            }
        }
    }

    let connect = if config.app.proxy.mode == fdm_core::NetworkProxyMode::ManualProxy
        && !config.app.proxy.socks5.host.trim().is_empty()
        && config.app.proxy.socks5.port > 0
    {
        let s = &config.app.proxy.socks5;
        let proxy_url = if !s.username.is_empty() {
            format!("socks5h://{}:{}@{}:{}", s.username, s.password, s.host.trim(), s.port)
        } else {
            format!("socks5h://{}:{}", s.host.trim(), s.port)
        };
        Some(librqbit::ConnectionOptions {
            proxy_url: Some(proxy_url),
            ..Default::default()
        })
    } else {
        None
    };

    SessionOptions {
        dht,
        fastresume: persistence.is_some(),
        persistence,
        listen: Some(ListenerOptions {
            mode,
            listen_addr,
            enable_upnp_port_forwarding: config.core.bt_enable_port_forwarding,
            ..Default::default()
        }),
        ratelimits: LimitsConfig {
            upload_bps: non_zero_limit(profile.max_upload_speed_bps),
            download_bps: non_zero_limit(profile.max_download_speed_bps),
        },
        trackers,
        peer_limit: Some(profile.max_connections.max(4) as usize),
        disable_local_service_discovery: !config.core.bt_enable_lsd,
        connect,
        client_name_and_version: Some("FerriteDM/1.0 (Free Download Manager compatible)".into()),
        ..Default::default()
    }
}

/// Stable signature of BitTorrent session-level settings requiring session reload when changed.
pub fn bt_session_signature(config: &FdmConfig) -> String {
    format!(
        "port={};rand={};dht={};utp={};lsd={};upnp={};pex={};enc={:?};trk_en={};trks={};proxy={:?}:{}:{}:{}:{}",
        config.core.bt_session_port,
        config.core.bt_use_random_port,
        config.core.bt_enable_dht,
        config.core.bt_enable_utp,
        config.core.bt_enable_lsd,
        config.core.bt_enable_port_forwarding,
        config.core.bt_enable_pex,
        config.core.bt_encryption_policy,
        config.core.bt_enable_tracker_list,
        config.core.bt_tracker_list.join(","),
        config.app.proxy.mode,
        config.app.proxy.socks5.host,
        config.app.proxy.socks5.port,
        config.app.proxy.socks5.username,
        !config.app.proxy.socks5.password.is_empty(),
    )
}

pub struct BtSessionManager {
    pub session: Arc<Session>,
    pub default_dir: PathBuf,
}

impl BtSessionManager {
    pub async fn new(
        default_download_dir: PathBuf,
        config: &FdmConfig,
        persistence_dir: Option<PathBuf>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&default_download_dir)?;
        if let Some(ref pdir) = persistence_dir {
            std::fs::create_dir_all(pdir)?;
        }
        let opts = build_session_options(config, persistence_dir);
        let session = Session::new_with_opts(default_download_dir.clone(), opts)
            .await
            .context("Failed to initialize BitTorrent session")?;
        Ok(Self {
            session,
            default_dir: default_download_dir,
        })
    }

    /// Create an ephemeral localhost session suitable for offline metadata inspection / unit tests
    pub async fn new_ephemeral(default_download_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&default_download_dir)?;
        let opts = SessionOptions {
            dht: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                enable_upnp_port_forwarding: false,
                ipv4_only: true,
                ..Default::default()
            }),
            fastresume: false,
            persistence: None,
            ..Default::default()
        };
        let session = Session::new_with_opts(default_download_dir.clone(), opts).await?;
        Ok(Self {
            session,
            default_dir: default_download_dir,
        })
    }

    pub fn apply_rate_limits(&self, config: &FdmConfig) {
        let profile = config.active_profile();
        self.session
            .ratelimits
            .set_download_bps(non_zero_limit(profile.max_download_speed_bps));
        self.session
            .ratelimits
            .set_upload_bps(non_zero_limit(profile.max_upload_speed_bps));
    }

    pub async fn inspect_source(&self, source: &str) -> Result<TorrentInspection> {
        let trimmed = source.trim();
        let path = Path::new(trimmed);
        if path.is_file() {
            let raw = std::fs::read(path)?;
            return inspect_torrent_bytes(&raw);
        }

        if trimmed.starts_with("magnet:") {
            // Fast offline parse fallback first, or resolve via session list_only with short timeout
            let add_res = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                self.session.add_torrent(
                    AddTorrent::from_url(trimmed),
                    Some(AddTorrentOptions {
                        list_only: true,
                        overwrite: true,
                        ..Default::default()
                    }),
                ),
            )
            .await;

            match add_res {
                Ok(Ok(AddTorrentResponse::ListOnly(lo))) => {
                    let info_hash = lo.info_hash.as_string();
                    let bytes = lo.torrent_bytes.to_vec();
                    let mut parsed = inspect_torrent_bytes(&bytes).unwrap_or(TorrentInspection {
                        info_hash,
                        name: lo
                            .info
                            .name()
                            .map(|c| c.into_owned())
                            .unwrap_or_else(|| "Torrent".into()),
                        total_bytes: 0,
                        files: vec![],
                        trackers: vec![],
                        raw_torrent_bytes: Some(bytes),
                    });
                    if parsed.files.is_empty() {
                        parsed.files = lo
                            .info
                            .iter_file_details_ext()
                            .enumerate()
                            .map(|(i, fd)| DownloadFileEntry {
                                index: i,
                                relative_path: fd.details.filename.to_string(),
                                size_bytes: fd.details.len,
                                downloaded_bytes: 0,
                                priority: DownloadPriority::Normal,
                                media_ready_to_play: false,
                                expected_hash: None,
                                computed_hash: None,
                            })
                            .collect();
                        parsed.total_bytes = parsed.files.iter().map(|f| f.size_bytes).sum();
                    }
                    return Ok(parsed);
                }
                _ => return inspect_magnet(trimmed),
            }
        }

        // HTTP/HTTPS .torrent URL
        let resp = self
            .session
            .add_torrent(
                AddTorrent::from_url(trimmed),
                Some(AddTorrentOptions {
                    list_only: true,
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;
        match resp {
            AddTorrentResponse::ListOnly(lo) => {
                let bytes = lo.torrent_bytes.to_vec();
                inspect_torrent_bytes(&bytes)
            }
            AddTorrentResponse::AlreadyManaged(_, h) | AddTorrentResponse::Added(_, h) => {
                Ok(inspection_from_handle(&h))
            }
        }
    }

    pub async fn run_torrent_download<F>(
        &self,
        mut record: DownloadRecord,
        config: &FdmConfig,
        mut control_rx: watch::Receiver<ControlCommand>,
        mut on_progress: F,
    ) -> DownloadRecord
    where
        F: FnMut(&DownloadRecord) + Send,
    {
        self.apply_rate_limits(config);
        std::fs::create_dir_all(&record.target_dir).ok();

        let only_files: Option<Vec<usize>> = {
            let selected: Vec<usize> = record
                .files
                .iter()
                .filter(|f| f.priority != DownloadPriority::DontDownload)
                .map(|f| f.index)
                .collect();
            if record.files.is_empty() || selected.len() == record.files.len() {
                None
            } else {
                Some(selected)
            }
        };

        let mut merged_trackers: Vec<String> = record.trackers.iter().map(|t| t.url.clone()).collect();
        if config.core.bt_enable_tracker_list {
            for tr in &config.core.bt_tracker_list {
                let s = tr.trim().to_string();
                if !s.is_empty() && !merged_trackers.contains(&s) {
                    merged_trackers.push(s);
                }
            }
        }
        let mut opts = AddTorrentOptions {
            overwrite: true,
            paused: false,
            output_folder: Some(record.target_dir.to_string_lossy().into_owned()),
            only_files,
            disable_trackers: false,
            ..Default::default()
        };
        if !merged_trackers.is_empty() {
            opts.trackers = Some(merged_trackers);
        }

        let add_input = if let Some(ref b64) = record.torrent_source {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(AddTorrent::from_bytes(bytes))
            } else if Path::new(b64).is_file() {
                AddTorrent::from_local_filename(b64)
            } else {
                Ok(AddTorrent::from_url(record.url.as_str()))
            }
        } else if Path::new(&record.url).is_file() {
            AddTorrent::from_local_filename(&record.url)
        } else {
            Ok(AddTorrent::from_url(record.url.as_str()))
        };

        let add_torrent = match add_input {
            Ok(a) => a,
            Err(e) => {
                record.state = DownloadState::Error;
                record.last_error = Some(format!("Invalid torrent source: {e}"));
                return record;
            }
        };

        record.state = DownloadState::Starting;
        on_progress(&record);

        let handle = match self.session.add_torrent(add_torrent, Some(opts)).await {
            Ok(AddTorrentResponse::Added(_, h)) | Ok(AddTorrentResponse::AlreadyManaged(_, h)) => {
                h
            }
            Ok(AddTorrentResponse::ListOnly(_)) => {
                record.state = DownloadState::Error;
                record.last_error = Some("Unexpected ListOnly response".into());
                return record;
            }
            Err(e) => {
                record.state = DownloadState::Error;
                record.abort_reason = DownloadAbortReason::NetworkError;
                record.last_error = Some(format!("Torrent start failed: {e}"));
                return record;
            }
        };

        let ih = handle.info_hash().as_string();
        record.torrent_info_hash = Some(ih);
        let meta_info = inspection_from_handle(&handle);
        if !meta_info.name.is_empty()
            && (record.file_name.is_empty() || record.file_name == "download.bin")
        {
            record.file_name = meta_info.name.clone();
            record.title = meta_info.name.clone();
        }
        if record.files.is_empty() && !meta_info.files.is_empty() {
            record.files = meta_info.files;
        }
        if !meta_info.trackers.is_empty() {
            record.trackers = meta_info
                .trackers
                .into_iter()
                .map(|url| TrackerInfo {
                    url,
                    status: "Active".into(),
                    peers: 0,
                    message: None,
                })
                .collect();
        }

        // Play ASAP sequential hint: open stream handle for first playable video/audio file if requested
        let mut _stream_guard = None;
        if record.flags.sequential_play_asap {
            if let Some(f) = record
                .files
                .iter()
                .find(|f| f.priority != DownloadPriority::DontDownload)
            {
                let ft = classify_file_type(&f.relative_path, None);
                if matches!(ft, FileType::Video | FileType::Audio) {
                    _stream_guard = handle.clone().stream(f.index).await.ok();
                }
            }
        }

        let max_ratio = config.active_profile().max_upload_ratio;
        let default_stop_when_done = !record.flags.sequential_play_asap && max_ratio <= 0.001;
        let stop_when_done = record.flags.bt_disable_seeding
            || (default_stop_when_done && !record.flags.bt_ignore_upload_ratio);

        loop {
            let cmd = control_rx.borrow().clone();
            match cmd {
                ControlCommand::Pause => {
                    let _ = self.session.pause(&handle).await;
                    record.state = DownloadState::Paused;
                    record.download_speed_bps = 0;
                    record.upload_speed_bps = 0;
                    record.connections_active = 0;
                    on_progress(&record);
                    return record;
                }
                ControlCommand::Cancel => {
                    let _ = self
                        .session
                        .delete(TorrentIdOrHash::Id(handle.id()), false)
                        .await;
                    record.state = DownloadState::Stopped;
                    record.abort_reason = DownloadAbortReason::AbortedByUser;
                    record.download_speed_bps = 0;
                    record.upload_speed_bps = 0;
                    record.connections_active = 0;
                    on_progress(&record);
                    return record;
                }
                ControlCommand::Run => {}
            }

            let stats = handle.stats();
            record.downloaded_bytes = stats.progress_bytes;
            record.uploaded_bytes = stats.uploaded_bytes;
            if stats.total_bytes > 0 {
                record.total_bytes = Some(stats.total_bytes);
            }
            for (idx, bytes_done) in stats.file_progress.iter().enumerate() {
                if let Some(entry) = record.files.get_mut(idx) {
                    entry.downloaded_bytes = *bytes_done;
                    if entry.size_bytes > 0 && *bytes_done >= entry.size_bytes {
                        entry.media_ready_to_play = true;
                    } else if record.flags.sequential_play_asap && *bytes_done >= 512 * 1024 {
                        entry.media_ready_to_play = true;
                    }
                }
            }

            if let Some(ref live) = stats.live {
                record.download_speed_bps = live.download_speed.as_bytes();
                record.upload_speed_bps = live.upload_speed.as_bytes();
                record.connections_active = live.snapshot.peer_stats.live as u32;
                record.eta_secs = live
                    .time_remaining
                    .as_ref()
                    .and_then(|_| {
                        let rem = stats.total_bytes.saturating_sub(stats.progress_bytes);
                        if record.download_speed_bps > 0 {
                            Some(rem / record.download_speed_bps)
                        } else {
                            None
                        }
                    });
                if let Some(live_handle) = handle.live() {
                    let snap = live_handle.per_peer_stats_snapshot(Default::default());
                    record.peers = snap
                        .peers
                        .into_iter()
                        .take(64)
                        .map(|(addr, p)| PeerInfo {
                            address: addr,
                            client: p.client_name.unwrap_or_else(|| "BitTorrent".into()),
                            download_bps: p.counters.fetched_bytes,
                            upload_bps: p.counters.uploaded_bytes,
                            progress: 0.0,
                            flags: p.state.to_string(),
                        })
                        .collect();
                }
            } else {
                record.download_speed_bps = 0;
                record.upload_speed_bps = 0;
            }

            match stats.state {
                TorrentStatsState::Initializing { .. } => {
                    record.state = DownloadState::Checking;
                }
                TorrentStatsState::Error => {
                    record.state = DownloadState::Error;
                    record.last_error = stats.error.clone();
                    on_progress(&record);
                    return record;
                }
                TorrentStatsState::Paused => {
                    record.state = DownloadState::Paused;
                    on_progress(&record);
                    return record;
                }
                TorrentStatsState::Live => {
                    if stats.finished {
                        let ratio = if stats.total_bytes > 0 {
                            stats.uploaded_bytes as f32 / stats.total_bytes as f32
                        } else {
                            0.0
                        };
                        let reached_ratio = !record.flags.bt_ignore_upload_ratio
                            && max_ratio > 0.0
                            && ratio >= max_ratio;
                        if stop_when_done || reached_ratio {
                            let _ = self.session.pause(&handle).await;
                            if config.core.set_mark_of_the_web {
                                let out = record.output_path();
                                if out.exists() {
                                    let _ = set_mark_of_the_web(&out, &record.url, "FerriteDM");
                                }
                            }
                            record.state = DownloadState::Completed;
                            record.download_speed_bps = 0;
                            record.upload_speed_bps = 0;
                            record.connections_active = 0;
                            record.completed_at = Some(chrono::Utc::now());
                            on_progress(&record);
                            return record;
                        } else {
                            record.state = DownloadState::Seeding;
                            if record.completed_at.is_none() {
                                record.completed_at = Some(chrono::Utc::now());
                            }
                        }
                    } else {
                        record.state = DownloadState::Downloading;
                    }
                }
            }

            record.updated_at = chrono::Utc::now();
            on_progress(&record);

            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                _ = control_rx.changed() => {}
            }
        }
    }

    pub async fn update_file_selection(
        &self,
        info_hash_or_id: &str,
        selected_indices: HashSet<usize>,
    ) -> Result<()> {
        let id = if let Ok(num) = info_hash_or_id.parse::<usize>() {
            TorrentIdOrHash::Id(num)
        } else {
            let ih = Id20::from_str(info_hash_or_id)
                .map_err(|e| anyhow::anyhow!("Invalid info hash: {e}"))?;
            TorrentIdOrHash::Hash(ih)
        };
        let handle = self
            .session
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Torrent not found in active session"))?;
        self.session
            .update_only_files(&handle, &selected_indices)
            .await
    }
}

fn inspection_from_handle(handle: &ManagedTorrentHandle) -> TorrentInspection {
    let ih = handle.info_hash().as_string();
    let res = handle.with_metadata(|m| {
        let name = m
            .info
            .name()
            .map(|c| c.into_owned())
            .unwrap_or_else(|| ih.clone());
        let files: Vec<DownloadFileEntry> = m
            .file_infos
            .iter()
            .enumerate()
            .map(|(idx, f)| DownloadFileEntry {
                index: idx,
                relative_path: f.relative_filename.to_string_lossy().into_owned(),
                size_bytes: f.len,
                downloaded_bytes: 0,
                priority: DownloadPriority::Normal,
                media_ready_to_play: false,
                expected_hash: None,
                computed_hash: None,
            })
            .collect();
        let total = files.iter().map(|f| f.size_bytes).sum();
        TorrentInspection {
            info_hash: ih.clone(),
            name,
            total_bytes: total,
            files,
            trackers: vec![],
            raw_torrent_bytes: Some(m.torrent_bytes.to_vec()),
        }
    });
    res.unwrap_or(TorrentInspection {
        info_hash: ih.clone(),
        name: ih,
        total_bytes: 0,
        files: vec![],
        trackers: vec![],
        raw_torrent_bytes: None,
    })
}

pub fn inspect_torrent_bytes(raw: &[u8]) -> Result<TorrentInspection> {
    let parsed = torrent_from_bytes(raw).map_err(|e| anyhow::anyhow!("Invalid .torrent: {e}"))?;
    let info_hash = parsed.info_hash.as_string();
    let name = parsed
        .info
        .data
        .name
        .as_ref()
        .map(|b| String::from_utf8_lossy(b.as_ref()).into_owned())
        .unwrap_or_else(|| info_hash.clone());
    let mut trackers = Vec::new();
    if let Some(ref ann) = parsed.announce {
        let s = String::from_utf8_lossy(ann.as_ref()).into_owned();
        if !s.is_empty() {
            trackers.push(s);
        }
    }
    for tier in parsed.iter_announce() {
        let s = String::from_utf8_lossy(tier.as_ref()).into_owned();
        if !s.is_empty() && !trackers.contains(&s) {
            trackers.push(s);
        }
    }

    let info_ref = &parsed.info.data;
    let files: Vec<DownloadFileEntry> = if let Some(ref multi) = info_ref.files {
        multi
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let rel = f
                    .path
                    .iter()
                    .map(|seg| String::from_utf8_lossy(seg.as_ref()).into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                DownloadFileEntry {
                    index: idx,
                    relative_path: rel,
                    size_bytes: f.length,
                    downloaded_bytes: 0,
                    priority: DownloadPriority::Normal,
                    media_ready_to_play: false,
                    expected_hash: None,
                    computed_hash: None,
                }
            })
            .collect()
    } else if let Some(len) = info_ref.length {
        vec![DownloadFileEntry {
            index: 0,
            relative_path: name.clone(),
            size_bytes: len,
            downloaded_bytes: 0,
            priority: DownloadPriority::Normal,
            media_ready_to_play: false,
            expected_hash: None,
            computed_hash: None,
        }]
    } else {
        Vec::new()
    };
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();

    Ok(TorrentInspection {
        info_hash,
        name,
        total_bytes,
        files,
        trackers,
        raw_torrent_bytes: Some(raw.to_vec()),
    })
}

pub fn inspect_magnet(magnet_uri: &str) -> Result<TorrentInspection> {
    let m = Magnet::parse(magnet_uri)?;
    let info_hash = m
        .as_id20()
        .map(|id| id.as_string())
        .unwrap_or_else(|| "unknown".into());
    let name = m.name.unwrap_or_else(|| info_hash.clone());
    Ok(TorrentInspection {
        info_hash,
        name,
        total_bytes: 0,
        files: vec![],
        trackers: m.trackers,
        raw_torrent_bytes: None,
    })
}

pub fn scan_torrent_watch_folder(
    watch_dir: &Path,
    already_seen: &mut HashSet<PathBuf>,
) -> Vec<WatchFolderItem> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(watch_dir) else {
        return found;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if !p.is_file() || already_seen.contains(&p) {
            continue;
        }
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "torrent" {
            already_seen.insert(p.clone());
            found.push(WatchFolderItem {
                source: p.to_string_lossy().into_owned(),
                path: p,
                is_magnet_file: false,
            });
        } else if ext == "magnet" {
            if let Ok(content) = std::fs::read_to_string(&p) {
                let trimmed = content.trim().to_string();
                if trimmed.starts_with("magnet:") {
                    already_seen.insert(p.clone());
                    found.push(WatchFolderItem {
                        source: trimmed,
                        path: p,
                        is_magnet_file: true,
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_synthetic_torrent_bytes() -> Vec<u8> {
        let ann = "udp://tracker.opentrackr.org:1337/announce";
        let mut out = Vec::new();
        out.extend_from_slice(
            format!(
                "d8:announce{}:{ann}4:infod6:lengthi1024e4:name9:hello.txt12:piece lengthi16384e6:pieces20:",
                ann.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&[0xABu8; 20]);
        out.extend_from_slice(b"ee");
        out
    }

    fn make_multifile_torrent_bytes() -> Vec<u8> {
        let ann = "https://tracker.example.org/announce";
        let mut out = Vec::new();
        out.extend_from_slice(
            format!(
                "d8:announce{}:{ann}4:infod5:filesld6:lengthi512e4:pathl4:docs9:readme.mdeed6:lengthi2048e4:pathl3:bin7:app.bineee4:name6:bundle12:piece lengthi16384e6:pieces20:",
                ann.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&[0xCDu8; 20]);
        out.extend_from_slice(b"ee");
        out
    }

    #[test]
    fn parses_torrent_metainfo_offline() {
        let bytes = make_synthetic_torrent_bytes();
        let info = inspect_torrent_bytes(&bytes).unwrap();
        assert_eq!(info.name, "hello.txt");
        assert_eq!(info.total_bytes, 1024);
        assert_eq!(info.files.len(), 1);
        assert_eq!(info.files[0].relative_path, "hello.txt");
        assert_eq!(info.info_hash.len(), 40);
        assert!(!info.trackers.is_empty());

        let mf = inspect_torrent_bytes(&make_multifile_torrent_bytes()).unwrap();
        assert_eq!(mf.name, "bundle");
        assert_eq!(mf.total_bytes, 2560);
        assert_eq!(mf.files.len(), 2);
        assert_eq!(mf.files[0].relative_path, "docs/readme.md");
        assert_eq!(mf.files[1].relative_path, "bin/app.bin");
    }

    #[test]
    fn parses_magnet_link_with_dn_and_trackers() {
        let mag = "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=Ubuntu%20ISO&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce";
        let info = inspect_magnet(mag).unwrap();
        assert_eq!(info.info_hash, "cab507494d02ebb1178b38f2e9d7be299c86b862");
        assert_eq!(info.name, "Ubuntu ISO");
        assert_eq!(info.trackers.len(), 1);
    }

    #[test]
    fn scans_watch_folder_for_torrent_and_magnet_files() {
        let dir = std::env::temp_dir().join(format!("ferrite_bt_watch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("linux.torrent"), make_synthetic_torrent_bytes()).unwrap();
        std::fs::write(
            dir.join("sample.magnet"),
            "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=Test",
        )
        .unwrap();
        std::fs::write(dir.join("ignore.txt"), "not a torrent").unwrap();

        let mut seen = HashSet::new();
        let items = scan_torrent_watch_folder(&dir, &mut seen);
        assert_eq!(items.len(), 2);
        let items2 = scan_torrent_watch_folder(&dir, &mut seen);
        assert_eq!(items2.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn creates_ephemeral_session_and_applies_limits() {
        let dir = std::env::temp_dir().join(format!("ferrite_bt_sess_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mgr = BtSessionManager::new_ephemeral(dir.clone()).await.unwrap();
        let mut cfg = FdmConfig::default();
        cfg.core.current_tum = fdm_core::TrafficUsageMode::Snail;
        mgr.apply_rate_limits(&cfg);
        assert!(mgr.session.ratelimits.get_download_bps().is_some());
        drop(mgr);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_options_include_manual_socks5_proxy_and_signature_tracks_changes() {
        let mut cfg = FdmConfig::default();
        let sig0 = bt_session_signature(&cfg);
        cfg.app.proxy.mode = fdm_core::NetworkProxyMode::ManualProxy;
        cfg.app.proxy.socks5.host = "127.0.0.1".into();
        cfg.app.proxy.socks5.port = 1080;
        let opts = build_session_options(&cfg, None);
        let proxy_url = opts
            .connect
            .and_then(|c| c.proxy_url)
            .expect("expected socks5 proxy url on ConnectionOptions");
        assert_eq!(proxy_url, "socks5h://127.0.0.1:1080");
        assert_ne!(sig0, bt_session_signature(&cfg));
    }
}
