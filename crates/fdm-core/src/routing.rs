use crate::hash::sha256_str;
use crate::model::{ExistingFileReaction, FileType};
use crate::settings::FdmConfig;
use chrono::Local;
use std::path::{Path, PathBuf};
use url::Url;

pub fn classify_file_type(file_name: &str, mime_type: Option<&str>) -> FileType {
    if let Some(mime) = mime_type {
        let m = mime.to_ascii_lowercase();
        if m.starts_with("video/") {
            return FileType::Video;
        }
        if m.starts_with("audio/") {
            return FileType::Audio;
        }
        if m.starts_with("image/") {
            return FileType::Image;
        }
        if m.contains("bittorrent") {
            return FileType::Torrent;
        }
    }
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "wav" | "wma" => FileType::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "flv" | "m4v" | "ts" | "m3u8" => FileType::Video,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" => FileType::Archive,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "epub" => {
            FileType::Document
        }
        "dmg" | "pkg" | "app" | "exe" | "msi" | "deb" | "rpm" | "appimage" | "iso" => {
            FileType::Program
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "tiff" => FileType::Image,
        "torrent" => FileType::Torrent,
        "" => FileType::Unknown,
        _ => FileType::Other,
    }
}

pub fn url_host_hash(url_str: &str) -> Option<String> {
    Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| sha256_str(&h.to_ascii_lowercase())))
}

/// Expand FDM-style path macros (`%date%`, `%year%`, `%month%`, `%day%`, `%host%`, `%server%`, `%path_on_server%`, `%ext%`, `%type%`, `%content_type%`).
pub fn expand_path_macros(template: &str, url_str: Option<&str>, file_name: Option<&str>) -> String {
    let now = Local::now();
    let parsed_url = url_str.and_then(|u| Url::parse(u).ok());
    let host = parsed_url
        .as_ref()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "unknown-host".into());
    let path_on_server = parsed_url
        .as_ref()
        .map(|u| {
            let p = u.path().trim_matches('/');
            if let Some((parent, _)) = p.rsplit_once('/') {
                parent.replace("..", "_")
            } else {
                String::new()
            }
        })
        .unwrap_or_default();
    let ext = file_name
        .and_then(|f| Path::new(f).extension().and_then(|e| e.to_str()))
        .unwrap_or("bin");
    let ft = file_name
        .map(|f| format!("{:?}", classify_file_type(f, None)))
        .unwrap_or_else(|| "Other".into());
    let content_type = match ext.to_ascii_lowercase().as_str() {
        "mp4" | "mkv" | "webm" | "mov" => "video/mp4",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "torrent" => "application/x-bittorrent",
        _ => "application/octet-stream",
    };

    template
        .replace("%date%", &now.format("%Y-%m-%d").to_string())
        .replace("%year%", &now.format("%Y").to_string())
        .replace("%month%", &now.format("%m").to_string())
        .replace("%day%", &now.format("%d").to_string())
        .replace("%host%", &host)
        .replace("%server%", &host)
        .replace("%path_on_server%", &path_on_server)
        .replace("%ext%", ext)
        .replace("%type%", &ft)
        .replace("%content_type%", content_type)
}

pub fn resolve_target_dir(
    config: &FdmConfig,
    explicit_dir: Option<&PathBuf>,
    url_str: &str,
    file_name: &str,
    mime_type: Option<&str>,
) -> PathBuf {
    if let Some(dir) = explicit_dir {
        let expanded = expand_path_macros(&dir.to_string_lossy(), Some(url_str), Some(file_name));
        return PathBuf::from(expanded);
    }

    if !config.core.fixed_download_path {
        if config.core.download_path_depends_on_source_url {
            if let Some(h) = url_host_hash(url_str) {
                if let Some(p) = config.path_by_url_hash.get(&h) {
                    return p.clone();
                }
            }
        }
        if config.core.download_path_depends_on_file_type {
            let ft = classify_file_type(file_name, mime_type);
            if let Some(p) = config.path_by_file_type.get(&ft) {
                return p.clone();
            }
            let sub = match ft {
                FileType::Video => Some("Videos"),
                FileType::Audio => Some("Music"),
                FileType::Archive => Some("Archives"),
                FileType::Document => Some("Documents"),
                FileType::Program => Some("Programs"),
                FileType::Image => Some("Images"),
                FileType::Torrent => Some("Torrents"),
                _ => None,
            };
            if let Some(folder) = sub {
                return config.core.default_download_path.join(folder);
            }
        }
    }

    let expanded = expand_path_macros(
        &config.core.default_download_path.to_string_lossy(),
        Some(url_str),
        Some(file_name),
    );
    PathBuf::from(expanded)
}

/// Record the chosen folder for a URL host when `download_path_depends_on_source_url`
/// is active and update `recent_folders`. Returns `true` if config changed.
pub fn remember_target_dir_preference(
    config: &mut FdmConfig,
    url_str: &str,
    chosen_dir: &Path,
) -> bool {
    if config.core.fixed_download_path {
        return false;
    }
    let chosen_s = chosen_dir.to_string_lossy();
    if chosen_s.trim().is_empty() || chosen_s.contains('%') {
        return false;
    }
    let mut changed = false;
    if config.core.download_path_depends_on_source_url {
        if let Some(h) = url_host_hash(url_str) {
            let is_custom_or_known = chosen_dir != config.core.default_download_path
                || config.path_by_url_hash.contains_key(&h);
            if is_custom_or_known && config.path_by_url_hash.get(&h) != Some(&chosen_dir.to_path_buf())
            {
                config.path_by_url_hash.insert(h, chosen_dir.to_path_buf());
                changed = true;
            }
        }
    }
    if config.recent_folders.first() != Some(&chosen_dir.to_path_buf()) {
        config.recent_folders.retain(|p| p != chosen_dir);
        config.recent_folders.insert(0, chosen_dir.to_path_buf());
        if config.recent_folders.len() > 16 {
            config.recent_folders.truncate(16);
        }
        changed = true;
    }
    changed
}

pub fn sanitize_filename(name: &str) -> String {
    let trimmed = name.trim();
    let cleaned: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download.bin".into()
    } else {
        cleaned
    }
}

pub fn infer_filename_from_url(url_str: &str) -> String {
    if url_str.starts_with("magnet:") {
        if let Ok(u) = Url::parse(url_str) {
            for (k, v) in u.query_pairs() {
                if k == "dn" && !v.trim().is_empty() {
                    return sanitize_filename(&v);
                }
            }
        }
        return "magnet-download".into();
    }
    if let Ok(u) = Url::parse(url_str) {
        if let Some(seg) = u.path_segments().and_then(|s| s.last()) {
            let decoded = urlencoding_decode(seg);
            if !decoded.is_empty() && decoded != "/" {
                return sanitize_filename(&decoded);
            }
        }
        if let Some(host) = u.host_str() {
            return sanitize_filename(&format!("{host}.bin"));
        }
    }
    "download.bin".into()
}

pub fn urlencoding_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let raw = input.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let (Some(h), Some(l)) = (
                char::from(raw[i + 1]).to_digit(16),
                char::from(raw[i + 2]).to_digit(16),
            ) {
                bytes.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn resolve_existing_file_name(
    dir: &Path,
    file_name: &str,
    reaction: ExistingFileReaction,
) -> Option<String> {
    let target = dir.join(file_name);
    if !target.exists() {
        return Some(file_name.to_string());
    }
    match reaction {
        ExistingFileReaction::Overwrite => Some(file_name.to_string()),
        ExistingFileReaction::Skip | ExistingFileReaction::Ask => None,
        ExistingFileReaction::Rename => {
            let p = Path::new(file_name);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            let ext = p.extension().and_then(|s| s.to_str());
            for n in 1..10_000 {
                let candidate = match ext {
                    Some(e) => format!("{stem} ({n}).{e}"),
                    None => format!("{stem} ({n})"),
                };
                if !dir.join(&candidate).exists() {
                    return Some(candidate);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_filename_renaming() {
        let dir = std::env::temp_dir().join(format!("ferrite_route_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("archive.zip"), b"1").unwrap();
        std::fs::write(dir.join("archive (1).zip"), b"2").unwrap();
        let next = resolve_existing_file_name(&dir, "archive.zip", ExistingFileReaction::Rename);
        assert_eq!(next.as_deref(), Some("archive (2).zip"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn macro_expansion() {
        let out = expand_path_macros(
            "/Users/Downloads/%host%/%type%",
            Some("https://cdn.example.com/file.mp4"),
            Some("file.mp4"),
        );
        assert_eq!(out, "/Users/Downloads/cdn.example.com/Video");
    }

    #[test]
    fn remembers_per_host_folder_when_enabled() {
        let mut cfg = FdmConfig::default();
        cfg.core.fixed_download_path = false;
        cfg.core.download_path_depends_on_source_url = true;
        cfg.core.default_download_path = PathBuf::from("/tmp/default");

        let custom = PathBuf::from("/tmp/linux-isos");
        assert!(remember_target_dir_preference(
            &mut cfg,
            "https://mirrors.example.org/ubuntu.iso",
            &custom
        ));
        let resolved = resolve_target_dir(
            &cfg,
            None,
            "https://mirrors.example.org/debian.iso",
            "debian.iso",
            None,
        );
        assert_eq!(resolved, custom);
        assert_eq!(cfg.recent_folders.first(), Some(&custom));
    }
}
