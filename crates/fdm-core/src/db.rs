use crate::model::{
    AuthCredential, DownloadRecord, DownloadState, FileType, FilterQuery, MissingFilesFilter,
    SortField, SslException, StateFilter, Tag,
};
use crate::settings::FdmConfig;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        db.seed_builtin_tags()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        db.seed_builtin_tags()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS downloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid TEXT NOT NULL UNIQUE,
                parent_id INTEGER,
                order_index INTEGER NOT NULL DEFAULT 0,
                download_type TEXT NOT NULL,
                state TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 2,
                url TEXT NOT NULL,
                title TEXT NOT NULL,
                target_dir TEXT NOT NULL,
                file_name TEXT NOT NULL,
                total_bytes INTEGER,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                uploaded_bytes INTEGER NOT NULL DEFAULT 0,
                file_type TEXT NOT NULL DEFAULT 'unknown',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                completed_at TEXT,
                payload_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_downloads_state ON downloads(state);
            CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at);

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                color_hex TEXT NOT NULL,
                is_builtin INTEGER NOT NULL DEFAULT 0,
                file_types_json TEXT NOT NULL DEFAULT '[]',
                default_extensions_json TEXT NOT NULL DEFAULT '[]',
                default_download_folder TEXT
            );

            CREATE TABLE IF NOT EXISTS downloads_tags (
                download_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                PRIMARY KEY (download_id, tag_id),
                FOREIGN KEY (download_id) REFERENCES downloads(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS ssl_known_hosts (
                host_port TEXT PRIMARY KEY,
                sha256_fingerprint TEXT NOT NULL,
                added_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS network_passwords (
                scope TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                password TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS rc_passwords (
                client_id TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )?;
        let existing_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(\"tags\")")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if !existing_cols.iter().any(|c| c == "default_extensions_json") {
            conn.execute(
                "ALTER TABLE tags ADD COLUMN default_extensions_json TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        if !existing_cols.iter().any(|c| c == "default_download_folder") {
            conn.execute(
                "ALTER TABLE tags ADD COLUMN default_download_folder TEXT",
                [],
            )?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO schema_meta (key, value) VALUES ('version', '2')",
            [],
        )?;
        Ok(())
    }

    fn seed_builtin_tags(&self) -> Result<()> {
        let builtins = vec![
            (-101_i64, "Video", "#3B82F6", vec![FileType::Video]),
            (-102, "Music", "#EC4899", vec![FileType::Audio]),
            (-103, "Archives", "#F59E0B", vec![FileType::Archive]),
            (-104, "Documents", "#10B981", vec![FileType::Document]),
            (-105, "Programs", "#8B5CF6", vec![FileType::Program]),
            (-106, "Images", "#06B6D4", vec![FileType::Image]),
            (-107, "Torrents", "#22C55E", vec![FileType::Torrent]),
        ];
        let conn = self.conn.lock().unwrap();
        for (id, name, color, fts) in builtins {
            let fts_json = serde_json::to_string(&fts)?;
            conn.execute(
                "INSERT OR IGNORE INTO tags (id, name, color_hex, is_builtin, file_types_json) VALUES (?1, ?2, ?3, 1, ?4)",
                params![id, name, color, fts_json],
            )?;
        }
        Ok(())
    }

    pub fn next_download_id_and_insert(&self, mut record: DownloadRecord) -> Result<DownloadRecord> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let max_order: i64 = tx.query_row(
            "SELECT COALESCE(MAX(order_index), 0) + 1 FROM downloads",
            [],
            |r| r.get(0),
        )?;
        record.order_index = max_order;

        // First insert to get AUTOINCREMENT id, then update payload_json with exact id.
        let dt_str = serde_json::to_string(&record.download_type)?;
        let st_str = serde_json::to_string(&record.state)?;
        let ft_str = serde_json::to_string(&record.file_type)?;
        let draft_json = serde_json::to_string(&record)?;
        tx.execute(
            "INSERT INTO downloads (
                uuid, parent_id, order_index, download_type, state, priority, url, title,
                target_dir, file_name, total_bytes, downloaded_bytes, uploaded_bytes,
                file_type, created_at, updated_at, completed_at, payload_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                record.uuid,
                record.parent_id,
                record.order_index,
                dt_str,
                st_str,
                record.priority as i64,
                record.url,
                record.title,
                record.target_dir.to_string_lossy().to_string(),
                record.file_name,
                record.total_bytes.map(|v| v as i64),
                record.downloaded_bytes as i64,
                record.uploaded_bytes as i64,
                ft_str,
                record.created_at.to_rfc3339(),
                record.updated_at.to_rfc3339(),
                record.completed_at.map(|d| d.to_rfc3339()),
                draft_json,
            ],
        )?;
        let new_id = tx.last_insert_rowid();
        record.id = new_id;
        let final_json = serde_json::to_string(&record)?;
        tx.execute(
            "UPDATE downloads SET payload_json = ?1 WHERE id = ?2",
            params![final_json, new_id],
        )?;
        for &tag_id in &record.tags {
            let _ = tx.execute(
                "INSERT OR IGNORE INTO downloads_tags (download_id, tag_id) VALUES (?1, ?2)",
                params![new_id, tag_id],
            );
        }
        tx.commit()?;
        Ok(record)
    }

    pub fn upsert_download(&self, record: &DownloadRecord) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let dt_str = serde_json::to_string(&record.download_type)?;
        let st_str = serde_json::to_string(&record.state)?;
        let ft_str = serde_json::to_string(&record.file_type)?;
        let json = serde_json::to_string(record)?;
        tx.execute(
            "INSERT OR REPLACE INTO downloads (
                id, uuid, parent_id, order_index, download_type, state, priority, url, title,
                target_dir, file_name, total_bytes, downloaded_bytes, uploaded_bytes,
                file_type, created_at, updated_at, completed_at, payload_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                record.id,
                record.uuid,
                record.parent_id,
                record.order_index,
                dt_str,
                st_str,
                record.priority as i64,
                record.url,
                record.title,
                record.target_dir.to_string_lossy().to_string(),
                record.file_name,
                record.total_bytes.map(|v| v as i64),
                record.downloaded_bytes as i64,
                record.uploaded_bytes as i64,
                ft_str,
                record.created_at.to_rfc3339(),
                record.updated_at.to_rfc3339(),
                record.completed_at.map(|d| d.to_rfc3339()),
                json,
            ],
        )?;
        tx.execute(
            "DELETE FROM downloads_tags WHERE download_id = ?1",
            params![record.id],
        )?;
        for &tag_id in &record.tags {
            let _ = tx.execute(
                "INSERT OR IGNORE INTO downloads_tags (download_id, tag_id) VALUES (?1, ?2)",
                params![record.id, tag_id],
            );
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_download(&self, id: i64) -> Result<Option<DownloadRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT payload_json FROM downloads WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let mut rec: DownloadRecord = serde_json::from_str(&json)?;
            rec.id = id;
            return Ok(Some(rec));
        }
        Ok(None)
    }

    pub fn list_all_downloads(&self) -> Result<Vec<DownloadRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, payload_json FROM downloads ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((id, json))
        })?;
        let mut out = Vec::new();
        for item in rows {
            let (id, json) = item?;
            if let Ok(mut rec) = serde_json::from_str::<DownloadRecord>(&json) {
                rec.id = id;
                out.push(rec);
            }
        }
        Ok(out)
    }

    pub fn remove_download(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, color_hex, is_builtin, file_types_json, default_extensions_json, default_download_folder FROM tags ORDER BY is_builtin DESC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let color_hex: String = row.get(2)?;
            let is_builtin: i64 = row.get(3)?;
            let fts_json: String = row.get(4)?;
            let ext_json: String = row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "[]".into());
            let folder_opt: Option<String> = row.get(6)?;
            let file_types: Vec<FileType> = serde_json::from_str(&fts_json).unwrap_or_default();
            let default_extensions: Vec<String> = serde_json::from_str(&ext_json).unwrap_or_default();
            Ok(Tag {
                id,
                name,
                color_hex,
                is_builtin: is_builtin != 0,
                file_types,
                default_extensions,
                default_download_folder: folder_opt.filter(|s| !s.trim().is_empty()).map(std::path::PathBuf::from),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn create_or_update_tag(&self, name: &str, color_hex: &str) -> Result<Tag> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tags (id, name, color_hex, is_builtin, file_types_json, default_extensions_json, default_download_folder)
             VALUES ((SELECT COALESCE(MAX(CASE WHEN id > 0 THEN id END), 0) + 1 FROM tags), ?1, ?2, 0, '[]', '[]', NULL)
             ON CONFLICT(name) DO UPDATE SET color_hex = excluded.color_hex",
            params![name, color_hex],
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, name, color_hex, is_builtin, file_types_json, default_extensions_json, default_download_folder FROM tags WHERE name = ?1",
        )?;
        let tag = stmt.query_row(params![name], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let color_hex: String = row.get(2)?;
            let is_builtin: i64 = row.get(3)?;
            let fts_json: String = row.get(4)?;
            let ext_json: String = row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "[]".into());
            let folder_opt: Option<String> = row.get(6)?;
            Ok(Tag {
                id,
                name,
                color_hex,
                is_builtin: is_builtin != 0,
                file_types: serde_json::from_str(&fts_json).unwrap_or_default(),
                default_extensions: serde_json::from_str(&ext_json).unwrap_or_default(),
                default_download_folder: folder_opt.filter(|s| !s.trim().is_empty()).map(std::path::PathBuf::from),
            })
        })?;
        Ok(tag)
    }

    pub fn update_tag(
        &self,
        tag_id: i64,
        name: &str,
        color_hex: &str,
        default_extensions: &[String],
        default_download_folder: Option<&Path>,
    ) -> Result<Tag> {
        let exts_clean: Vec<String> = default_extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        let exts_json = serde_json::to_string(&exts_clean)?;
        let folder_str = default_download_folder.map(|p| p.to_string_lossy().into_owned());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tags SET name = ?1, color_hex = ?2, default_extensions_json = ?3, default_download_folder = ?4 WHERE id = ?5",
            params![name.trim(), color_hex, exts_json, folder_str, tag_id],
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, name, color_hex, is_builtin, file_types_json, default_extensions_json, default_download_folder FROM tags WHERE id = ?1",
        )?;
        let tag = stmt.query_row(params![tag_id], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let color_hex: String = row.get(2)?;
            let is_builtin: i64 = row.get(3)?;
            let fts_json: String = row.get(4)?;
            let ext_json: String = row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "[]".into());
            let folder_opt: Option<String> = row.get(6)?;
            Ok(Tag {
                id,
                name,
                color_hex,
                is_builtin: is_builtin != 0,
                file_types: serde_json::from_str(&fts_json).unwrap_or_default(),
                default_extensions: serde_json::from_str(&ext_json).unwrap_or_default(),
                default_download_folder: folder_opt.filter(|s| !s.trim().is_empty()).map(std::path::PathBuf::from),
            })
        })?;
        Ok(tag)
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM tags WHERE id = ?1 AND is_builtin = 0",
            params![tag_id],
        )?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<FdmConfig> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value_json FROM settings WHERE key = 'fdm_config'")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let cfg: FdmConfig = serde_json::from_str(&json)?;
            return Ok(cfg);
        }
        Ok(FdmConfig::default())
    }

    pub fn save_config(&self, config: &FdmConfig) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string_pretty(config)?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES ('fdm_config', ?1)",
            params![json],
        )?;
        Ok(())
    }

    pub fn add_ssl_exception(&self, exc: &SslException) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO ssl_known_hosts (host_port, sha256_fingerprint, added_at) VALUES (?1, ?2, ?3)",
            params![exc.host_port, exc.sha256_fingerprint, exc.added_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_ssl_exceptions(&self) -> Result<Vec<SslException>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT host_port, sha256_fingerprint, added_at FROM ssl_known_hosts ORDER BY host_port",
        )?;
        let rows = stmt.query_map([], |row| {
            let host_port: String = row.get(0)?;
            let sha256_fingerprint: String = row.get(1)?;
            let added_str: String = row.get(2)?;
            let added_at = DateTime::parse_from_rfc3339(&added_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(SslException {
                host_port,
                sha256_fingerprint,
                added_at,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn remove_ssl_exception(&self, host_port: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM ssl_known_hosts WHERE host_port = ?1",
            params![host_port],
        )?;
        Ok(())
    }

    pub fn clear_ssl_exceptions(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM ssl_known_hosts", [])?;
        Ok(())
    }

    pub fn save_credential(&self, cred: &AuthCredential) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO network_passwords (scope, username, password) VALUES (?1, ?2, ?3)",
            params![cred.scope, cred.username, cred.password],
        )?;
        Ok(())
    }

    pub fn get_credential(&self, scope: &str) -> Result<Option<AuthCredential>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT scope, username, password FROM network_passwords WHERE scope = ?1",
        )?;
        let mut rows = stmt.query(params![scope])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(AuthCredential {
                scope: row.get(0)?,
                username: row.get(1)?,
                password: row.get(2)?,
            }));
        }
        Ok(None)
    }

    pub fn save_rc_password_hash(&self, client_id: &str, hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO rc_passwords (client_id, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![client_id, hash, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_rc_passwords(&self) -> Result<BTreeMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT client_id, password_hash FROM rc_passwords")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for r in rows {
            let (k, v) = r?;
            out.insert(k, v);
        }
        Ok(out)
    }

    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = self.conn.lock().unwrap();
        let mut dst = Connection::open(dest)?;
        let backup = rusqlite::backup::Backup::new(&conn, &mut dst)?;
        backup.run_to_completion(64, std::time::Duration::from_millis(5), None)?;
        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let status: String = conn.query_row("PRAGMA integrity_check;", [], |r| r.get(0))?;
        Ok(status.eq_ignore_ascii_case("ok"))
    }

    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }
}

pub fn filter_and_sort_downloads(
    items: &[DownloadRecord],
    tags_catalog: &[Tag],
    query: &FilterQuery,
) -> Vec<DownloadRecord> {
    let q = query.search_text.trim().to_ascii_lowercase();
    let tag_info = query
        .tag_id
        .and_then(|tid| tags_catalog.iter().find(|t| t.id == tid));

    let mut filtered: Vec<DownloadRecord> = items
        .iter()
        .filter(|d| {
            if let Some(pid) = query.parent_id_filter {
                d.parent_id == Some(pid)
            } else if query.only_top_level {
                d.parent_id.is_none()
            } else {
                true
            }
        })
        .filter(|d| match query.state_filter {
            StateFilter::All => true,
            StateFilter::Running => d.state.is_running_or_seeding(),
            StateFilter::Downloading => matches!(
                d.state,
                DownloadState::Downloading
                    | DownloadState::Starting
                    | DownloadState::Checking
                    | DownloadState::Queued
                    | DownloadState::PostProcessing
            ),
            StateFilter::Uploading => d.state == DownloadState::Seeding,
            StateFilter::Finished => d.state == DownloadState::Completed,
            StateFilter::Stopped => d.state.is_stopped_or_paused(),
            StateFilter::Scheduled => d.state == DownloadState::Scheduled,
            StateFilter::Missing => d.flags.is_missing_files,
        })
        .filter(|d| match query.missing_filter {
            MissingFilesFilter::Off => true,
            MissingFilesFilter::AcceptNormal => !d.flags.is_missing_files,
            MissingFilesFilter::AcceptMissingFiles => d.flags.is_missing_files,
        })
        .filter(|d| {
            if let Some(t) = tag_info {
                if t.is_builtin && !t.file_types.is_empty() {
                    t.file_types.contains(&d.file_type) || d.tags.contains(&t.id)
                } else {
                    d.tags.contains(&t.id)
                }
            } else {
                true
            }
        })
        .filter(|d| {
            if q.is_empty() {
                true
            } else {
                d.title.to_ascii_lowercase().contains(&q)
                    || d.file_name.to_ascii_lowercase().contains(&q)
                    || d.url.to_ascii_lowercase().contains(&q)
            }
        })
        .cloned()
        .collect();

    filtered.sort_by(|a, b| {
        let ord = match query.sort_by {
            SortField::CreationTime => a.created_at.cmp(&b.created_at),
            SortField::Order => a.order_index.cmp(&b.order_index),
            SortField::Size => a.total_bytes.unwrap_or(0).cmp(&b.total_bytes.unwrap_or(0)),
            SortField::Status => a.state.display_name().cmp(b.state.display_name()),
            SortField::Title => a
                .title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase()),
            SortField::Progress => a
                .progress_fraction()
                .partial_cmp(&b.progress_fraction())
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Priority => a.priority.cmp(&b.priority),
        };
        if query.sort_ascending {
            ord
        } else {
            ord.reverse()
        }
    });

    filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn sqlite_roundtrip_backup_and_verify() {
        let db = Database::open_in_memory().unwrap();
        let rec = DownloadRecord {
            id: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            children_ids: vec![],
            order_index: 0,
            download_type: DownloadType::Www,
            state: DownloadState::Downloading,
            priority: DownloadPriority::High,
            resume_support: ResumeSupport::Present,
            flags: DownloadFlags::default(),
            title: "ubuntu.iso".into(),
            url: "https://example.org/ubuntu.iso".into(),
            target_dir: "/tmp".into(),
            file_name: "ubuntu.iso".into(),
            partial_suffix: ".fdmdownload".into(),
            total_bytes: Some(1024),
            downloaded_bytes: 512,
            uploaded_bytes: 0,
            download_speed_bps: 100,
            upload_speed_bps: 0,
            eta_secs: Some(5),
            connections_active: 4,
            etag: None,
            last_modified: None,
            file_type: FileType::Program,
            http: HttpRequestOptions::default(),
            torrent_info_hash: None,
            torrent_source: None,
            tags: vec![-105],
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        let saved = db.next_download_id_and_insert(rec).unwrap();
        assert_eq!(saved.id, 1);
        assert!(db.verify_integrity().unwrap());

        let tags = db.list_tags().unwrap();
        let res = filter_and_sort_downloads(
            &db.list_all_downloads().unwrap(),
            &tags,
            &FilterQuery {
                state_filter: StateFilter::Downloading,
                tag_id: Some(-105),
                ..Default::default()
            },
        );
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "ubuntu.iso");

        let custom = db.create_or_update_tag("SDKs", "#EF5350").unwrap();
        let updated = db
            .update_tag(
                custom.id,
                "SDK Packages",
                "#2983FE",
                &[".tar.zst".into(), "pkg".into()],
                Some(Path::new("/tmp/sdk")),
            )
            .unwrap();
        assert_eq!(updated.name, "SDK Packages");
        assert_eq!(updated.default_extensions, vec!["tar.zst", "pkg"]);
        assert_eq!(updated.default_download_folder, Some(std::path::PathBuf::from("/tmp/sdk")));

        // Legacy partial config JSON deserializes via #[serde(default)]
        let legacy_cfg: FdmConfig = serde_json::from_str(r#"{"app":{"quit_confirmation":false}}"#).unwrap();
        assert!(!legacy_cfg.app.quit_confirmation);
        assert!(legacy_cfg.app.show_save_as_button);
        assert!(!legacy_cfg.app.show_support_button);
    }
}
