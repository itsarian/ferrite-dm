use aes::cipher::{BlockDecryptMut, KeyIvInit};
use anyhow::{bail, Context, Result};
use fdm_core::{
    BandwidthLimiter, DownloadAbortReason, DownloadFileEntry, DownloadPriority, DownloadRecord,
    DownloadState, FdmConfig, SegmentProgress, SegmentState,
    os_integration::set_mark_of_the_web,
};
use fdm_http::{build_http_client, ControlCommand};
use m3u8_rs::{MasterPlaylist, MediaPlaylist, Playlist};
use reqwest::Client;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use url::Url;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Debug, Clone)]
pub struct HlsVariantInfo {
    pub url: String,
    pub bandwidth: u64,
    pub resolution: Option<String>,
    pub codecs: Option<String>,
    pub audio_group: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DashRepresentationInfo {
    pub id: String,
    pub mime_type: String,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub init_url: Option<String>,
    pub segment_urls: Vec<String>,
}

#[derive(Debug, Clone)]
struct HlsSegmentJob {
    seq_index: usize,
    url: String,
    byte_range: Option<(u64, u64)>, // (offset, length)
    key_uri: Option<String>,
    key_iv: Option<[u8; 16]>,
    seq_num: u64,
}

fn resolve_url(base: &Url, rel: &str) -> Result<String> {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return Ok(rel.to_string());
    }
    Ok(base.join(rel)?.to_string())
}

fn parse_iv(iv_opt: &Option<String>, seq_num: u64) -> [u8; 16] {
    if let Some(iv_str) = iv_opt {
        let stripped = iv_str
            .trim()
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        let padded = format!("{:0>32}", stripped);
        if let Ok(bytes) = hex::decode(padded) {
            if bytes.len() == 16 {
                let mut out = [0u8; 16];
                out.copy_from_slice(&bytes);
                return out;
            }
        }
    }
    let mut out = [0u8; 16];
    out[8..16].copy_from_slice(&seq_num.to_be_bytes());
    out
}

fn decrypt_aes128_cbc(ciphertext: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() {
        return Ok(Vec::new());
    }
    let mut buf = ciphertext.to_vec();
    let pt = Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|_| anyhow::anyhow!("AES-128 PKCS7 decryption failed"))?;
    Ok(pt.to_vec())
}

pub async fn inspect_hls_variants(client: &Client, url: &str) -> Result<Vec<HlsVariantInfo>> {
    let body = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let base = Url::parse(url)?;
    match m3u8_rs::parse_playlist_res(&body) {
        Ok(Playlist::MasterPlaylist(master)) => Ok(extract_variants(&base, &master)),
        Ok(Playlist::MediaPlaylist(_)) => Ok(vec![HlsVariantInfo {
            url: url.to_string(),
            bandwidth: 0,
            resolution: None,
            codecs: None,
            audio_group: None,
        }]),
        Err(e) => bail!("Failed to parse M3U8 playlist: {e:?}"),
    }
}

fn extract_variants(base: &Url, master: &MasterPlaylist) -> Vec<HlsVariantInfo> {
    let mut list: Vec<HlsVariantInfo> = master
        .variants
        .iter()
        .filter(|v| !v.is_i_frame)
        .filter_map(|v| {
            let resolved = resolve_url(base, &v.uri).ok()?;
            Some(HlsVariantInfo {
                url: resolved,
                bandwidth: v.bandwidth,
                resolution: v.resolution.map(|r| format!("{}x{}", r.width, r.height)),
                codecs: v.codecs.clone(),
                audio_group: v.audio.clone(),
            })
        })
        .collect();
    list.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
    list
}

fn extract_audio_rendition(base: &Url, master: &MasterPlaylist, audio_group: &Option<String>) -> Option<String> {
    for alt in &master.alternatives {
        if matches!(alt.media_type, m3u8_rs::AlternativeMediaType::Audio) {
            if let Some(grp) = audio_group {
                if &alt.group_id != grp {
                    continue;
                }
            }
            if let Some(ref uri) = alt.uri {
                return resolve_url(base, uri).ok();
            }
        }
    }
    None
}

fn resolve_ffmpeg_binary(config: &FdmConfig) -> Option<PathBuf> {
    if let Some(ref p) = config.core.ffmpeg_path {
        if p.exists() {
            return Some(p.clone());
        }
    }
    for cand in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        let p = PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub async fn download_hls_stream<F>(
    mut record: DownloadRecord,
    config: &FdmConfig,
    limiter: BandwidthLimiter,
    control_rx: watch::Receiver<ControlCommand>,
    mut on_progress: F,
) -> DownloadRecord
where
    F: FnMut(&DownloadRecord) + Send,
{
    let client = match build_http_client(config, &[], record.http.user_agent.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            return record;
        }
    };

    record.state = DownloadState::Starting;
    on_progress(&record);

    let max_attempts = if config.core.auto_retry_failed && !record.flags.disable_auto_retry {
        config.core.max_auto_retries.max(1) + 1
    } else {
        1
    };

    for attempt in 0..max_attempts {
        let res = run_hls_inner(
            &client,
            &mut record,
            config,
            &limiter,
            control_rx.clone(),
            &mut on_progress,
        )
        .await;

        match res {
            Ok(OutcomeHls::Completed) => {
                if config.core.set_mark_of_the_web {
                    let _ = set_mark_of_the_web(&record.output_path(), &record.url, "FerriteDM");
                }
                record.state = DownloadState::Completed;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                record.completed_at = Some(chrono::Utc::now());
                record.files = vec![DownloadFileEntry {
                    index: 0,
                    relative_path: record.file_name.clone(),
                    size_bytes: record.downloaded_bytes,
                    downloaded_bytes: record.downloaded_bytes,
                    priority: DownloadPriority::Normal,
                    media_ready_to_play: true,
                    expected_hash: None,
                    computed_hash: None,
                }];
                on_progress(&record);
                return record;
            }
            Ok(OutcomeHls::Paused) => {
                record.state = DownloadState::Paused;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                on_progress(&record);
                return record;
            }
            Ok(OutcomeHls::Cancelled) => {
                record.state = DownloadState::Stopped;
                record.abort_reason = DownloadAbortReason::AbortedByUser;
                record.download_speed_bps = 0;
                record.connections_active = 0;
                on_progress(&record);
                return record;
            }
            Err(e) => {
                record.retry_count += 1;
                record.last_error = Some(e.to_string());
                record.download_speed_bps = 0;
                record.connections_active = 0;
                if attempt + 1 < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_secs(1 + attempt as u64)).await;
                    continue;
                }
                record.state = DownloadState::Error;
                on_progress(&record);
                return record;
            }
        }
    }
    record
}

enum OutcomeHls {
    Completed,
    Paused,
    Cancelled,
}

async fn run_hls_inner<F>(
    client: &Client,
    record: &mut DownloadRecord,
    config: &FdmConfig,
    limiter: &BandwidthLimiter,
    control_rx: watch::Receiver<ControlCommand>,
    on_progress: &mut F,
) -> Result<OutcomeHls>
where
    F: FnMut(&DownloadRecord) + Send,
{
    let raw = client
        .get(&record.url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let base = Url::parse(&record.url)?;
    let (media_url, audio_url) = match m3u8_rs::parse_playlist_res(&raw) {
        Ok(Playlist::MasterPlaylist(master)) => {
            let variants = extract_variants(&base, &master);
            let best = variants
                .first()
                .context("Master HLS playlist has no video variants")?;
            let aud = extract_audio_rendition(&base, &master, &best.audio_group);
            (best.url.clone(), aud)
        }
        Ok(Playlist::MediaPlaylist(_)) => (record.url.clone(), None),
        Err(e) => bail!("Invalid M3U8 playlist: {e:?}"),
    };

    std::fs::create_dir_all(&record.target_dir)?;
    let work_dir = record
        .target_dir
        .join(format!(".{}.hls_tmp", record.uuid));
    std::fs::create_dir_all(&work_dir)?;

    let video_raw_path = if audio_url.is_some() {
        work_dir.join("video_raw.ts")
    } else {
        record.partial_path()
    };

    let outcome = download_media_playlist_to_file(
        client,
        &media_url,
        &video_raw_path,
        &work_dir.join("v"),
        record,
        config,
        limiter,
        control_rx.clone(),
        on_progress,
    )
    .await?;

    if !matches!(outcome, OutcomeHls::Completed) {
        return Ok(outcome);
    }

    let output_path = record.output_path();
    if let Some(aud_url) = audio_url {
        let audio_raw_path = work_dir.join("audio_raw.aac");
        let a_outcome = download_media_playlist_to_file(
            client,
            &aud_url,
            &audio_raw_path,
            &work_dir.join("a"),
            record,
            config,
            limiter,
            control_rx,
            on_progress,
        )
        .await?;
        if !matches!(a_outcome, OutcomeHls::Completed) {
            return Ok(a_outcome);
        }

        if let Some(ffmpeg) = resolve_ffmpeg_binary(config) {
            record.state = DownloadState::PostProcessing;
            on_progress(record);
            let status = tokio::process::Command::new(ffmpeg)
                .args(["-y", "-i"])
                .arg(&video_raw_path)
                .args(["-i"])
                .arg(&audio_raw_path)
                .args(["-c", "copy"])
                .arg(&output_path)
                .status()
                .await?;
            if !status.success() {
                bail!("FFmpeg A/V mux exited with status {}", status);
            }
        } else {
            std::fs::rename(&video_raw_path, &output_path)?;
        }
    } else if video_raw_path != output_path {
        if output_path.exists() {
            let _ = std::fs::remove_file(&output_path);
        }
        std::fs::rename(&video_raw_path, &output_path)?;
    }

    if config.core.delete_service_files_when_finished {
        let _ = std::fs::remove_dir_all(&work_dir);
    }
    Ok(OutcomeHls::Completed)
}

async fn download_media_playlist_to_file<F>(
    client: &Client,
    playlist_url: &str,
    dest_file: &Path,
    seg_dir: &Path,
    record: &mut DownloadRecord,
    config: &FdmConfig,
    limiter: &BandwidthLimiter,
    mut control_rx: watch::Receiver<ControlCommand>,
    on_progress: &mut F,
) -> Result<OutcomeHls>
where
    F: FnMut(&DownloadRecord) + Send,
{
    std::fs::create_dir_all(seg_dir)?;
    let mut seen_urls = HashSet::<String>::new();
    let mut seq_files: Vec<PathBuf> = Vec::new();
    let total_downloaded = Arc::new(AtomicU64::new(record.downloaded_bytes));
    let key_cache: Arc<Mutex<std::collections::HashMap<String, [u8; 16]>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let workers = config.active_profile().max_connections_per_server.max(1).min(8) as usize;

    record.state = DownloadState::Downloading;

    // Live playlist reload loop (runs once for VOD with EXT-X-ENDLIST, polls for live streams!)
    let mut max_live_polls = 240u32;
    loop {
        let pl_bytes = client
            .get(playlist_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let base = Url::parse(playlist_url)?;
        let media: MediaPlaylist = match m3u8_rs::parse_media_playlist_res(&pl_bytes) {
            Ok(m) => m,
            Err(e) => bail!("Failed to parse media M3U8: {e:?}"),
        };

        let mut cur_key_uri: Option<String> = None;
        let mut cur_iv: Option<String> = None;
        let mut init_downloaded = false;
        let mut new_jobs: Vec<HlsSegmentJob> = Vec::new();
        let start_seq = media.media_sequence;

        for (idx, seg) in media.segments.iter().enumerate() {
            if let Some(ref k) = seg.key {
                if k.method == m3u8_rs::KeyMethod::AES128 {
                    cur_key_uri = k
                        .uri
                        .as_ref()
                        .and_then(|u| resolve_url(&base, u).ok());
                    cur_iv = k.iv.clone();
                } else {
                    cur_key_uri = None;
                    cur_iv = None;
                }
            }

            if !init_downloaded {
                if let Some(ref map) = seg.map {
                    let map_url = resolve_url(&base, &map.uri)?;
                    if seen_urls.insert(format!("MAP:{map_url}")) {
                        let map_path = seg_dir.join("000000_init.mp4");
                        let b = client.get(&map_url).send().await?.error_for_status()?.bytes().await?;
                        limiter.acquire(b.len()).await;
                        std::fs::write(&map_path, &b)?;
                        total_downloaded.fetch_add(b.len() as u64, Ordering::Relaxed);
                        seq_files.push(map_path);
                    }
                    init_downloaded = true;
                }
            }

            let seg_url = resolve_url(&base, &seg.uri)?;
            let range_key = seg
                .byte_range
                .as_ref()
                .map(|r| format!("@{}#{}", r.length, r.offset.unwrap_or(0)))
                .unwrap_or_default();
            let uniq = format!("{seg_url}{range_key}");
            if seen_urls.insert(uniq) {
                let seq_num = start_seq + idx as u64;
                let seq_idx = seq_files.len() + new_jobs.len();
                let mut byte_offset = 0u64;
                let byte_range = seg.byte_range.as_ref().map(|r| {
                    let off = r.offset.unwrap_or(byte_offset);
                    byte_offset = off + r.length;
                    (off, r.length)
                });
                new_jobs.push(HlsSegmentJob {
                    seq_index: seq_idx,
                    url: seg_url,
                    byte_range,
                    key_uri: cur_key_uri.clone(),
                    key_iv: cur_key_uri
                        .as_ref()
                        .map(|_| parse_iv(&cur_iv, seq_num)),
                    seq_num,
                });
            }
        }

        if !new_jobs.is_empty() {
            record.segments = new_jobs
                .iter()
                .map(|j| SegmentProgress {
                    id: j.seq_index as u32,
                    start: j.seq_index as u64,
                    end: j.seq_index as u64,
                    downloaded: 0,
                    state: SegmentState::Pending,
                    speed_bps: 0,
                    mirror_url: Some(j.url.clone()),
                })
                .collect();

            let jobs_q = Arc::new(Mutex::new(std::collections::VecDeque::from(new_jobs.clone())));
            let paths_map = Arc::new(Mutex::new(vec![None::<PathBuf>; new_jobs.len()]));
            let cancel_flag = Arc::new(AtomicBool::new(false));
            let pause_flag = Arc::new(AtomicBool::new(false));
            let err_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

            let mut handles = Vec::new();
            for _ in 0..workers {
                let client = client.clone();
                let jobs_q = Arc::clone(&jobs_q);
                let paths_map = Arc::clone(&paths_map);
                let seg_dir = seg_dir.to_path_buf();
                let limiter = limiter.clone();
                let total_downloaded = Arc::clone(&total_downloaded);
                let key_cache = Arc::clone(&key_cache);
                let cancel_flag = Arc::clone(&cancel_flag);
                let pause_flag = Arc::clone(&pause_flag);
                let err_slot = Arc::clone(&err_slot);
                let offset_base = seq_files.len();

                handles.push(tokio::spawn(async move {
                    loop {
                        if cancel_flag.load(Ordering::Relaxed)
                            || pause_flag.load(Ordering::Relaxed)
                        {
                            break;
                        }
                        let next_job = jobs_q.lock().unwrap().pop_front();
                        let Some(job) = next_job else {
                            break;
                        };
                        let local_idx = job.seq_index - offset_base;
                        let part_path = seg_dir.join(format!("{:06}.ts", job.seq_num));

                        if !part_path.exists() {
                            let mut req = client.get(&job.url);
                            if let Some((off, len)) = job.byte_range {
                                req = req.header(
                                    reqwest::header::RANGE,
                                    format!("bytes={}-{}", off, off + len - 1),
                                );
                            }
                            let bytes_res = match req.send().await {
                                Ok(r) => match r.error_for_status() {
                                    Ok(ok) => ok.bytes().await,
                                    Err(e) => Err(e),
                                },
                                Err(e) => Err(e),
                            };
                            let mut raw_bytes = match bytes_res {
                                Ok(b) => b.to_vec(),
                                Err(e) => {
                                    let mut g = err_slot.lock().unwrap();
                                    if g.is_none() {
                                        *g = Some(format!("HLS segment {} error: {e}", job.url));
                                    }
                                    cancel_flag.store(true, Ordering::Relaxed);
                                    break;
                                }
                            };
                            limiter.acquire(raw_bytes.len()).await;

                            if let (Some(ref key_url), Some(ref iv)) = (job.key_uri, job.key_iv) {
                                let cached = key_cache.lock().unwrap().get(key_url).copied();
                                let key = match cached {
                                    Some(k) => k,
                                    None => {
                                        match client.get(key_url).send().await {
                                            Ok(r) => match r.bytes().await {
                                                Ok(kb) if kb.len() == 16 => {
                                                    let mut arr = [0u8; 16];
                                                    arr.copy_from_slice(&kb);
                                                    key_cache
                                                        .lock()
                                                        .unwrap()
                                                        .insert(key_url.clone(), arr);
                                                    arr
                                                }
                                                _ => {
                                                    let mut g = err_slot.lock().unwrap();
                                                    *g = Some("Invalid AES-128 key length".into());
                                                    cancel_flag.store(true, Ordering::Relaxed);
                                                    break;
                                                }
                                            },
                                            Err(e) => {
                                                let mut g = err_slot.lock().unwrap();
                                                *g = Some(format!("Key fetch error: {e}"));
                                                cancel_flag.store(true, Ordering::Relaxed);
                                                break;
                                            }
                                        }
                                    }
                                };
                                match decrypt_aes128_cbc(&raw_bytes, &key, iv) {
                                    Ok(pt) => raw_bytes = pt,
                                    Err(e) => {
                                        let mut g = err_slot.lock().unwrap();
                                        *g = Some(format!("Segment decrypt error: {e}"));
                                        cancel_flag.store(true, Ordering::Relaxed);
                                        break;
                                    }
                                }
                            }

                            if let Err(e) = std::fs::write(&part_path, &raw_bytes) {
                                let mut g = err_slot.lock().unwrap();
                                *g = Some(format!("Write segment failed: {e}"));
                                cancel_flag.store(true, Ordering::Relaxed);
                                break;
                            }
                            total_downloaded
                                .fetch_add(raw_bytes.len() as u64, Ordering::Relaxed);
                        }

                        paths_map.lock().unwrap()[local_idx] = Some(part_path);
                    }
                }));
            }

            let mut last_bytes = total_downloaded.load(Ordering::Relaxed);
            let mut last_tick = tokio::time::Instant::now();
            let mut slow_accum_secs = 0.0f64;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
                    changed = control_rx.changed() => {
                        if changed.is_ok() {
                            match *control_rx.borrow() {
                                ControlCommand::Pause => pause_flag.store(true, Ordering::Relaxed),
                                ControlCommand::Cancel => cancel_flag.store(true, Ordering::Relaxed),
                                ControlCommand::Run => {}
                            }
                        }
                    }
                }
                let now = tokio::time::Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f64().max(0.05);
                let cur = total_downloaded.load(Ordering::Relaxed);
                let speed = (cur.saturating_sub(last_bytes) as f64 / dt) as u64;
                last_bytes = cur;
                last_tick = now;

                record.downloaded_bytes = cur;
                record.download_speed_bps = speed;
                record.connections_active = workers as u32;
                on_progress(record);

                if config.core.detect_unacceptably_slow && !pause_flag.load(Ordering::Relaxed) {
                    if speed < config.core.unacceptably_slow_speed_bps {
                        slow_accum_secs += dt;
                        if slow_accum_secs >= config.core.unacceptably_slow_interval_secs as f64 {
                            record.abort_reason = DownloadAbortReason::SlowDownload;
                            *err_slot.lock().unwrap() = Some(format!(
                                "HLS speed dropped below {} B/s for {}s",
                                config.core.unacceptably_slow_speed_bps,
                                config.core.unacceptably_slow_interval_secs
                            ));
                            cancel_flag.store(true, Ordering::Relaxed);
                            break;
                        }
                    } else {
                        slow_accum_secs = 0.0;
                    }
                }

                if handles.iter().all(|h| h.is_finished()) {
                    break;
                }
            }

            for h in handles {
                let _ = h.await;
            }
            if matches!(*control_rx.borrow(), ControlCommand::Pause) {
                return Ok(OutcomeHls::Paused);
            }
            if matches!(*control_rx.borrow(), ControlCommand::Cancel) {
                return Ok(OutcomeHls::Cancelled);
            }
            if let Some(msg) = err_slot.lock().unwrap().take() {
                bail!(msg);
            }
            let finished_paths = paths_map.lock().unwrap();
            for p in finished_paths.iter().flatten() {
                seq_files.push(p.clone());
            }
        }

        if media.end_list || max_live_polls == 0 {
            break;
        }
        max_live_polls -= 1;
        let wait_secs = (media.target_duration as u64).clamp(1, 5);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(wait_secs)) => {}
            _ = control_rx.changed() => {
                match *control_rx.borrow() {
                    ControlCommand::Pause => return Ok(OutcomeHls::Paused),
                    ControlCommand::Cancel => return Ok(OutcomeHls::Cancelled),
                    ControlCommand::Run => {}
                }
            }
        }
    }

    // Concatenate segments cleanly into dest_file
    let mut out = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest_file)?;
    let mut total_written = 0u64;
    for sp in &seq_files {
        let mut f = File::open(sp)?;
        let n = std::io::copy(&mut f, &mut out)?;
        total_written += n;
    }
    out.flush()?;
    record.downloaded_bytes = total_written;
    record.total_bytes = Some(total_written);
    Ok(OutcomeHls::Completed)
}

/// Minimal DASH MPD parser & downloader supporting SegmentTemplate & SegmentList representations.
pub fn parse_dash_mpd(mpd_url: &str, xml_text: &str) -> Result<Vec<DashRepresentationInfo>> {
    let base = Url::parse(mpd_url)?;
    let doc = roxmltree::Document::parse(xml_text)?;
    let mut reps = Vec::new();

    for adap in doc.descendants().filter(|n| n.has_tag_name("AdaptationSet")) {
        let adap_mime = adap.attribute("mimeType").unwrap_or("").to_string();
        let adap_template = adap.children().find(|c| c.has_tag_name("SegmentTemplate"));
        for rep in adap.children().filter(|c| c.has_tag_name("Representation")) {
            let id = rep.attribute("id").unwrap_or("0").to_string();
            let mime = rep
                .attribute("mimeType")
                .map(str::to_string)
                .unwrap_or_else(|| adap_mime.clone());
            let bandwidth = rep
                .attribute("bandwidth")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let width = rep.attribute("width").and_then(|s| s.parse::<u32>().ok());
            let height = rep.attribute("height").and_then(|s| s.parse::<u32>().ok());

            let rep_base = rep
                .children()
                .find(|c| c.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
                .and_then(|t| resolve_url(&base, t.trim()).ok())
                .unwrap_or_else(|| mpd_url.to_string());
            let rep_base_url = Url::parse(&rep_base).unwrap_or_else(|_| base.clone());

            let tmpl = rep
                .children()
                .find(|c| c.has_tag_name("SegmentTemplate"))
                .or(adap_template);

            let mut init_url = None;
            let mut segment_urls = Vec::new();

            if let Some(t) = tmpl {
                if let Some(init_attr) = t.attribute("initialization") {
                    let s = init_attr.replace("$RepresentationID$", &id);
                    init_url = resolve_url(&rep_base_url, &s).ok();
                }
                if let Some(media_attr) = t.attribute("media") {
                    let start_num = t
                        .attribute("startNumber")
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(1);
                    if let Some(timeline) = t.children().find(|c| c.has_tag_name("SegmentTimeline")) {
                        let mut cur_t = 0u64;
                        let mut cur_num = start_num;
                        for seg in timeline.children().filter(|c| c.has_tag_name("S")) {
                            if let Some(t_val) = seg.attribute("t").and_then(|s| s.parse::<u64>().ok())
                            {
                                cur_t = t_val;
                            }
                            let d_val = seg
                                .attribute("d")
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(0);
                            let repeat = seg
                                .attribute("r")
                                .and_then(|s| s.parse::<u64>().ok())
                                .unwrap_or(0);
                            for _ in 0..=repeat {
                                let u = media_attr
                                    .replace("$RepresentationID$", &id)
                                    .replace("$Number$", &cur_num.to_string())
                                    .replace("$Time$", &cur_t.to_string());
                                if let Ok(full) = resolve_url(&rep_base_url, &u) {
                                    segment_urls.push(full);
                                }
                                cur_t += d_val;
                                cur_num += 1;
                            }
                        }
                    } else {
                        // Single template entry fallback if duration/timeline unknown
                        let u = media_attr
                            .replace("$RepresentationID$", &id)
                            .replace("$Number$", &start_num.to_string());
                        if let Ok(full) = resolve_url(&rep_base_url, &u) {
                            segment_urls.push(full);
                        }
                    }
                }
            } else if let Some(slist) = rep.children().find(|c| c.has_tag_name("SegmentList")) {
                if let Some(init) = slist
                    .children()
                    .find(|c| c.has_tag_name("Initialization"))
                    .and_then(|n| n.attribute("sourceURL"))
                {
                    init_url = resolve_url(&rep_base_url, init).ok();
                }
                for su in slist.children().filter(|c| c.has_tag_name("SegmentURL")) {
                    if let Some(m) = su.attribute("media") {
                        if let Ok(full) = resolve_url(&rep_base_url, m) {
                            segment_urls.push(full);
                        }
                    }
                }
            } else if rep.children().any(|c| c.has_tag_name("BaseURL")) {
                segment_urls.push(rep_base);
            }

            reps.push(DashRepresentationInfo {
                id,
                mime_type: mime,
                bandwidth,
                width,
                height,
                init_url,
                segment_urls,
            });
        }
    }

    Ok(reps)
}

pub async fn download_dash_stream<F>(
    mut record: DownloadRecord,
    config: &FdmConfig,
    limiter: BandwidthLimiter,
    control_rx: watch::Receiver<ControlCommand>,
    mut on_progress: F,
) -> DownloadRecord
where
    F: FnMut(&DownloadRecord) + Send,
{
    let client = match build_http_client(config, &[], record.http.user_agent.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            return record;
        }
    };

    record.state = DownloadState::Starting;
    on_progress(&record);

    let xml = match client.get(&record.url).send().await {
        Ok(r) => match r.text().await {
            Ok(t) => t,
            Err(e) => {
                record.state = DownloadState::Error;
                record.last_error = Some(e.to_string());
                return record;
            }
        },
        Err(e) => {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            return record;
        }
    };

    let reps = match parse_dash_mpd(&record.url, &xml) {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            record.state = DownloadState::Error;
            record.last_error = Some("No DASH representations found in MPD".into());
            return record;
        }
        Err(e) => {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            return record;
        }
    };

    let best_video = reps
        .iter()
        .filter(|r| r.mime_type.starts_with("video/") || r.width.is_some())
        .max_by_key(|r| r.bandwidth)
        .or_else(|| reps.first())
        .cloned();
    let best_audio = reps
        .iter()
        .filter(|r| r.mime_type.starts_with("audio/"))
        .max_by_key(|r| r.bandwidth)
        .cloned();

    let _ = std::fs::create_dir_all(&record.target_dir);
    let work_dir = record.target_dir.join(format!(".{}.dash_tmp", record.uuid));
    let _ = std::fs::create_dir_all(&work_dir);

    let mut total_done = 0u64;
    let video_path = work_dir.join("video_dash.mp4");
    let audio_path = work_dir.join("audio_dash.m4a");

    record.state = DownloadState::Downloading;
    if let Some(ref vr) = best_video {
        if let Err(e) =
            fetch_dash_rep(&client, vr, &video_path, &limiter, &control_rx, &mut total_done).await
        {
            record.state = DownloadState::Error;
            record.last_error = Some(e.to_string());
            return record;
        }
        record.downloaded_bytes = total_done;
        on_progress(&record);
    }

    let output_path = record.output_path();
    if let Some(ref ar) = best_audio {
        if best_video
            .as_ref()
            .map(|v| v.id != ar.id)
            .unwrap_or(true)
        {
            if let Err(e) =
                fetch_dash_rep(&client, ar, &audio_path, &limiter, &control_rx, &mut total_done)
                    .await
            {
                record.state = DownloadState::Error;
                record.last_error = Some(e.to_string());
                return record;
            }
            if let Some(ffmpeg) = resolve_ffmpeg_binary(config) {
                record.state = DownloadState::PostProcessing;
                on_progress(&record);
                if let Ok(st) = tokio::process::Command::new(ffmpeg)
                    .args(["-y", "-i"])
                    .arg(&video_path)
                    .args(["-i"])
                    .arg(&audio_path)
                    .args(["-c", "copy"])
                    .arg(&output_path)
                    .status()
                    .await
                {
                    if !st.success() {
                        let _ = std::fs::rename(&video_path, &output_path);
                    }
                }
            } else {
                let _ = std::fs::rename(&video_path, &output_path);
            }
        } else {
            let _ = std::fs::rename(&video_path, &output_path);
        }
    } else {
        let _ = std::fs::rename(&video_path, &output_path);
    }

    if config.core.delete_service_files_when_finished {
        let _ = std::fs::remove_dir_all(&work_dir);
    }
    if config.core.set_mark_of_the_web {
        let _ = set_mark_of_the_web(&output_path, &record.url, "FerriteDM");
    }
    record.downloaded_bytes = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(total_done);
    record.total_bytes = Some(record.downloaded_bytes);
    record.state = DownloadState::Completed;
    record.completed_at = Some(chrono::Utc::now());
    on_progress(&record);
    record
}

async fn fetch_dash_rep(
    client: &Client,
    rep: &DashRepresentationInfo,
    out_path: &Path,
    limiter: &BandwidthLimiter,
    control_rx: &watch::Receiver<ControlCommand>,
    total_done: &mut u64,
) -> Result<()> {
    let mut f = File::create(out_path)?;
    if let Some(ref init_u) = rep.init_url {
        let b = client.get(init_u).send().await?.error_for_status()?.bytes().await?;
        limiter.acquire(b.len()).await;
        f.write_all(&b)?;
        *total_done += b.len() as u64;
    }
    for u in &rep.segment_urls {
        if matches!(*control_rx.borrow(), ControlCommand::Cancel | ControlCommand::Pause) {
            bail!("Aborted DASH transfer");
        }
        let b = client.get(u).send().await?.error_for_status()?.bytes().await?;
        limiter.acquire(b.len()).await;
        f.write_all(&b)?;
        *total_done += b.len() as u64;
    }
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;
    use fdm_core::{DownloadFlags, DownloadType, HttpRequestOptions, ResumeSupport};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

    #[tokio::test]
    async fn downloads_and_decrypts_aes128_hls_stream() {
        let key = [0x42u8; 16];
        let seg1_plain = b"HELLO_SEGMENT_ONE_PAYLOAD_123456".to_vec();
        let seg2_plain = b"WORLD_SEGMENT_TWO_PAYLOAD_7890!".to_vec();

        let encrypt = |pt: &[u8], seq: u64| -> Vec<u8> {
            let iv = parse_iv(&None, seq);
            let mut buf = pt.to_vec();
            let pos = buf.len();
            buf.resize(pos + 16, 0);
            Aes128CbcEnc::new(&key.into(), &iv.into())
                .encrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf, pos)
                .unwrap()
                .to_vec()
        };
        let seg1_enc = Arc::new(encrypt(&seg1_plain, 0));
        let seg2_enc = Arc::new(encrypt(&seg2_plain, 1));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let s1 = Arc::clone(&seg1_enc);
        let s2 = Arc::clone(&seg2_enc);

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let s1 = Arc::clone(&s1);
                let s2 = Arc::clone(&s2);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (ctype, body): (&str, Vec<u8>) = match path {
                        "/master.m3u8" => (
                            "application/vnd.apple.mpegurl",
                            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\n/media.m3u8\n"
                                .to_vec(),
                        ),
                        "/media.m3u8" => (
                            "application/vnd.apple.mpegurl",
                            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"/key.bin\"\n#EXTINF:4.0,\n/seg0.ts\n#EXTINF:4.0,\n/seg1.ts\n#EXT-X-ENDLIST\n".to_vec(),
                        ),
                        "/key.bin" => ("application/octet-stream", key.to_vec()),
                        "/seg0.ts" => ("video/mp2t", s1.to_vec()),
                        "/seg1.ts" => ("video/mp2t", s2.to_vec()),
                        _ => ("text/plain", b"not found".to_vec()),
                    };
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(hdr.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_hls_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.core.set_mark_of_the_web = false;
        cfg.core.delete_service_files_when_finished = true;

        let url = format!("http://{addr}/master.m3u8");
        let rec = DownloadRecord {
            id: 2,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            children_ids: vec![],
            order_index: 2,
            download_type: DownloadType::HlsM3u8,
            state: DownloadState::Queued,
            priority: DownloadPriority::Normal,
            resume_support: ResumeSupport::Present,
            flags: DownloadFlags::default(),
            title: "stream.ts".into(),
            url: url.clone(),
            target_dir: dir.clone(),
            file_name: "stream.ts".into(),
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
            file_type: fdm_core::FileType::Video,
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
        let done =
            download_hls_stream(rec, &cfg, BandwidthLimiter::new(0), rx, |_| {}).await;
        assert_eq!(done.state, DownloadState::Completed, "err={:?}", done.last_error);

        let final_bytes = std::fs::read(dir.join("stream.ts")).unwrap();
        let mut expected = seg1_plain;
        expected.extend_from_slice(&seg2_plain);
        assert_eq!(final_bytes, expected);

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_dash_segment_timeline() {
        let xml = r#"<?xml version="1.0"?>
        <MPD>
          <Period>
            <AdaptationSet mimeType="video/mp4">
              <Representation id="v720" bandwidth="1500000" width="1280" height="720">
                <SegmentTemplate initialization="init-$RepresentationID$.mp4" media="chunk-$RepresentationID$-$Number$.m4s" startNumber="1">
                  <SegmentTimeline>
                    <S t="0" d="1000" r="1"/>
                  </SegmentTimeline>
                </SegmentTemplate>
              </Representation>
            </AdaptationSet>
          </Period>
        </MPD>"#;
        let reps = parse_dash_mpd("https://cdn.example.com/live/manifest.mpd", xml).unwrap();
        assert_eq!(reps.len(), 1);
        assert_eq!(
            reps[0].init_url.as_deref(),
            Some("https://cdn.example.com/live/init-v720.mp4")
        );
        assert_eq!(
            reps[0].segment_urls,
            vec![
                "https://cdn.example.com/live/chunk-v720-1.m4s",
                "https://cdn.example.com/live/chunk-v720-2.m4s",
            ]
        );
    }
}
