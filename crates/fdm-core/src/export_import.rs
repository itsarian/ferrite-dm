use crate::hash::sha256_str;
use crate::model::{
    DownloadFlags, DownloadPriority, DownloadRecord, DownloadState, DownloadType,
    HttpRequestOptions, ResumeSupport,
};
use crate::routing::{classify_file_type, infer_filename_from_url};
use crate::settings::FdmConfig;
use anyhow::{bail, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedDownloadsBundle {
    #[serde(rename = "FormatName")]
    pub format_name: String,
    #[serde(rename = "FormatVersion")]
    pub format_version: u32,
    #[serde(rename = "FileContents")]
    pub file_contents: ExportedDownloadsContents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedDownloadsContents {
    pub downloads: Vec<DownloadRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedSettingsBundle {
    #[serde(rename = "AppName")]
    pub app_name: String,
    #[serde(rename = "SettingsFile")]
    pub settings_file: String,
    #[serde(rename = "SettingsHash")]
    pub settings_hash: String,
}

pub fn export_downloads_json(downloads: &[DownloadRecord], path: &Path) -> Result<()> {
    let bundle = ExportedDownloadsBundle {
        format_name: "FerriteDM-Downloads".into(),
        format_version: 1,
        file_contents: ExportedDownloadsContents {
            downloads: downloads.to_vec(),
        },
    };
    let text = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(path, text)?;
    Ok(())
}

pub fn export_downloads_urls_txt(downloads: &[DownloadRecord], path: &Path) -> Result<()> {
    let lines: Vec<String> = downloads
        .iter()
        .filter(|d| !d.url.trim().is_empty())
        .map(|d| d.url.clone())
        .collect();
    std::fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn export_downloads_csv(downloads: &[DownloadRecord], path: &Path) -> Result<()> {
    let mut out = String::from("id,title,url,state,downloaded_bytes,total_bytes,output_path,created_at\n");
    for d in downloads {
        let row = [
            d.id.to_string(),
            csv_escape(&d.title),
            csv_escape(&d.url),
            csv_escape(d.state.display_name()),
            d.downloaded_bytes.to_string(),
            d.total_bytes.map(|v| v.to_string()).unwrap_or_default(),
            csv_escape(&d.output_path().to_string_lossy()),
            csv_escape(&d.created_at.to_rfc3339()),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    std::fs::write(path, out)?;
    Ok(())
}

pub fn import_downloads_file(path: &Path, config: &FdmConfig) -> Result<Vec<DownloadRecord>> {
    let raw = std::fs::read_to_string(path)?;
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        if let Ok(bundle) = serde_json::from_str::<ExportedDownloadsBundle>(trimmed) {
            return Ok(bundle.file_contents.downloads);
        }
        // Fallback: parse generic FDM JSON export shapes if they contain a `downloads` or `FileContents` object
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let arr = val
                .get("FileContents")
                .and_then(|fc| fc.get("downloads"))
                .and_then(|d| d.as_array())
                .or_else(|| val.get("downloads").and_then(|d| d.as_array()));
            if let Some(items) = arr {
                let mut out = Vec::new();
                for item in items {
                    if let Ok(rec) = serde_json::from_value::<DownloadRecord>(item.clone()) {
                        out.push(rec);
                    } else if let Some(url) = item
                        .get("Url")
                        .or_else(|| item.get("url"))
                        .and_then(|u| u.as_str())
                    {
                        out.push(record_from_imported_url(url, config));
                    }
                }
                if !out.is_empty() {
                    return Ok(out);
                }
            }
        }
    }

    // Plain URL list (one URL per line, ignoring empty lines & # comments)
    let mut out = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.contains("://") || l.starts_with("magnet:") {
            out.push(record_from_imported_url(l, config));
        }
    }
    Ok(out)
}

pub fn export_settings_json(config: &FdmConfig, path: &Path) -> Result<()> {
    let inner = serde_json::to_string_pretty(config)?;
    let hash = sha256_str(&inner);
    let bundle = ExportedSettingsBundle {
        app_name: "FerriteDM".into(),
        settings_file: inner,
        settings_hash: hash,
    };
    std::fs::write(path, serde_json::to_string_pretty(&bundle)?)?;
    Ok(())
}

pub fn import_settings_json(path: &Path) -> Result<FdmConfig> {
    let raw = std::fs::read_to_string(path)?;
    if let Ok(bundle) = serde_json::from_str::<ExportedSettingsBundle>(&raw) {
        let actual = sha256_str(&bundle.settings_file);
        if !actual.eq_ignore_ascii_case(&bundle.settings_hash) {
            bail!("SettingsHash verification failed (corrupted or tampered settings file)");
        }
        let cfg: FdmConfig = serde_json::from_str(&bundle.settings_file)?;
        return Ok(cfg);
    }
    Ok(serde_json::from_str::<FdmConfig>(&raw)?)
}

fn record_from_imported_url(url: &str, config: &FdmConfig) -> DownloadRecord {
    let file_name = infer_filename_from_url(url);
    let dl_type = if url.starts_with("magnet:") || url.ends_with(".torrent") {
        DownloadType::BitTorrent
    } else if url.contains(".m3u8") {
        DownloadType::HlsM3u8
    } else {
        DownloadType::Www
    };
    DownloadRecord {
        id: 0,
        uuid: uuid::Uuid::new_v4().to_string(),
        parent_id: None,
        children_ids: vec![],
        order_index: 0,
        download_type: dl_type,
        state: DownloadState::Stopped,
        priority: DownloadPriority::Normal,
        resume_support: ResumeSupport::Unknown,
        flags: DownloadFlags::default(),
        title: file_name.clone(),
        url: url.to_string(),
        target_dir: config.core.default_download_path.clone(),
        file_name: file_name.clone(),
        partial_suffix: config.core.partial_download_suffix.clone(),
        total_bytes: None,
        downloaded_bytes: 0,
        uploaded_bytes: 0,
        download_speed_bps: 0,
        upload_speed_bps: 0,
        eta_secs: None,
        connections_active: 0,
        etag: None,
        last_modified: None,
        file_type: classify_file_type(&file_name, None),
        http: HttpRequestOptions {
            url: url.to_string(),
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
        abort_reason: Default::default(),
        preview_path: None,
        module_data: serde_json::Value::Null,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        completed_at: None,
    }
}

/// Best-effort reader for an existing Free Download Manager SQLite database (`db.sqlite`),
/// scanning tables dynamically via `PRAGMA table_info` so user migration works safely without hardcoding internal schema versions.
pub fn inspect_legacy_fdm_sqlite(db_path: &Path, config: &FdmConfig) -> Result<Vec<DownloadRecord>> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut tbl_stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let tables: Vec<String> = tbl_stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut imported = Vec::new();
    for table in tables {
        let mut info_stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table))?;
        let cols: Vec<String> = info_stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();

        let url_col = cols
            .iter()
            .find(|c| c.eq_ignore_ascii_case("url") || c.eq_ignore_ascii_case("sourceUrl"));
        if let Some(ucol) = url_col {
            let file_col = cols
                .iter()
                .find(|c| c.eq_ignore_ascii_case("fileName") || c.eq_ignore_ascii_case("outputPath"));
            let sql = if let Some(fcol) = file_col {
                format!("SELECT \"{}\", \"{}\" FROM \"{}\"", ucol, fcol, table)
            } else {
                format!("SELECT \"{}\", '' FROM \"{}\"", ucol, table)
            };
            if let Ok(mut stmt) = conn.prepare(&sql) {
                if let Ok(rows) = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    ))
                }) {
                    for row in rows.flatten() {
                        if !row.0.is_empty() {
                            let mut rec = record_from_imported_url(&row.0, config);
                            if !row.1.is_empty() {
                                let p = Path::new(&row.1);
                                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                                    rec.file_name = name.to_string();
                                    rec.title = name.to_string();
                                }
                                if let Some(dir) = p.parent() {
                                    if !dir.as_os_str().is_empty() {
                                        rec.target_dir = dir.to_path_buf();
                                    }
                                }
                            }
                            imported.push(rec);
                        }
                    }
                }
            }
        }
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_export_import_verifies_hash() {
        let dir = std::env::temp_dir().join(format!("ferrite_exp_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("settings.json");
        let cfg = FdmConfig::default();
        export_settings_json(&cfg, &p).unwrap();
        let loaded = import_settings_json(&p).unwrap();
        assert_eq!(
            loaded.core.partial_download_suffix,
            cfg.core.partial_download_suffix
        );

        let rec = record_from_imported_url("https://example.org/test.iso", &cfg);
        let txt_p = dir.join("urls.txt");
        let csv_p = dir.join("downloads.csv");
        export_downloads_urls_txt(&[rec.clone()], &txt_p).unwrap();
        export_downloads_csv(&[rec], &csv_p).unwrap();
        assert!(std::fs::read_to_string(&txt_p).unwrap().contains("test.iso"));
        assert!(std::fs::read_to_string(&csv_p).unwrap().contains("id,title,url"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
