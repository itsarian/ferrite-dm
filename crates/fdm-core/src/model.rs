use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub type DownloadId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadType {
    Www,
    BitTorrent,
    HlsM3u8,
    Dash,
    MediaParser,
    Batch,
    YoutubeChannel,
    Unknown,
}

impl DownloadType {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Www => "HTTP/FTP",
            Self::BitTorrent => "BitTorrent",
            Self::HlsM3u8 => "HLS Stream",
            Self::Dash => "DASH Stream",
            Self::MediaParser => "Media",
            Self::Batch => "Batch",
            Self::YoutubeChannel => "Channel",
            Self::Unknown => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Stopped,
    Queued,
    Starting,
    Downloading,
    Checking,
    Paused,
    PostProcessing,
    Seeding,
    Completed,
    Error,
    WaitingForNetwork,
    Scheduled,
    Moving,
}

impl DownloadState {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Downloading | Self::Checking | Self::PostProcessing | Self::Moving
        )
    }

    pub fn is_running_or_seeding(&self) -> bool {
        self.is_active() || matches!(self, Self::Seeding)
    }

    pub fn is_stopped_or_paused(&self) -> bool {
        matches!(self, Self::Stopped | Self::Paused | Self::Error | Self::Scheduled)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Queued => "Queued",
            Self::Starting => "Starting",
            Self::Downloading => "Downloading",
            Self::Checking => "Checking",
            Self::Paused => "Paused",
            Self::PostProcessing => "Post-processing",
            Self::Seeding => "Seeding",
            Self::Completed => "Completed",
            Self::Error => "Error",
            Self::WaitingForNetwork => "Waiting for network",
            Self::Scheduled => "Scheduled",
            Self::Moving => "Moving files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPriority {
    DontDownload = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Unknown = 4,
}

impl Default for DownloadPriority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeSupport {
    Unknown,
    Present,
    Absent,
}

impl Default for ResumeSupport {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Audio,
    Video,
    Archive,
    Document,
    Program,
    Image,
    Torrent,
    Other,
    Unknown,
}

impl Default for FileType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl HashAlgorithm {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingFileReaction {
    Ask,
    Overwrite,
    Rename,
    Skip,
}

impl Default for ExistingFileReaction {
    fn default() -> Self {
        Self::Rename
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileExistsAction {
    Rename,
    Overwrite,
    Abort,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtEncryptionPolicy {
    NoEncryptionAllowed,
    PreferEncryption,
    RequireEncryption,
}

impl Default for BtEncryptionPolicy {
    fn default() -> Self {
        Self::PreferEncryption
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadAbortReason {
    None,
    AbortedByUser,
    AlreadyExists,
    InvalidUrl,
    NetworkError,
    UnwantedError,
    SlowDownload,
    MissingFiles,
    RemoteResourceChanged,
    LowBattery,
    NoAllowedNetwork,
    Shutdown,
}

impl Default for DownloadAbortReason {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoStartPreventer {
    None,
    NotOnline,
    LowBattery,
    NetworkRoaming,
    NoAllowedNetworkInterface,
    LowDiskSpace,
}

impl Default for AutoStartPreventer {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingFilesFilter {
    Off,
    AcceptNormal,
    AcceptMissingFiles,
}

impl Default for MissingFilesFilter {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    CreationTime,
    Order,
    Size,
    Status,
    Title,
    Progress,
    Priority,
}

impl Default for SortField {
    fn default() -> Self {
        Self::CreationTime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficUsageMode {
    Snail,
    Low,
    Medium,
    High,
}

impl Default for TrafficUsageMode {
    fn default() -> Self {
        Self::High
    }
}

impl TrafficUsageMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Snail => "Snail",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProxyMode {
    NoProxy,
    SystemProxy,
    ManualProxy,
}

impl Default for NetworkProxyMode {
    fn default() -> Self {
        Self::SystemProxy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownWebBrowser {
    Chrome,
    Firefox,
    Edge,
    Vivaldi,
    Opera,
    Brave,
    Safari,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownType {
    None,
    Sleep,
    Hibernate,
    Shutdown,
    QuitApp,
}

impl Default for ShutdownType {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEvent {
    DownloadAdded,
    DownloadComplete,
    DownloadsComplete,
    DownloadFailed,
    DownloadsFailed,
    NoActiveDownloads,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateFilter {
    All,
    Running,
    Downloading,
    Uploading,
    Finished,
    Stopped,
    Scheduled,
    Missing,
}

impl Default for StateFilter {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadFlags {
    pub sequential_play_asap: bool,
    pub allow_change_source_url: bool,
    pub supports_mirrors: bool,
    pub supports_restart: bool,
    pub can_detect_remote_changes: bool,
    pub endless_stream: bool,
    pub delete_service_files_when_done: bool,
    pub silent_mode: bool,
    pub is_missing_files: bool,
    pub created_by_batch: bool,
    pub bt_disable_seeding: bool,
    pub bt_ignore_upload_ratio: bool,
    pub disable_auto_retry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadFileEntry {
    pub index: usize,
    pub relative_path: String,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    pub priority: DownloadPriority,
    pub media_ready_to_play: bool,
    pub expected_hash: Option<(HashAlgorithm, String)>,
    pub computed_hash: Option<(HashAlgorithm, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    Pending,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub id: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub state: SegmentState,
    pub speed_bps: u64,
    pub mirror_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub address: String,
    pub client: String,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub progress: f32,
    pub flags: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerInfo {
    pub url: String,
    pub status: String,
    pub peers: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpRequestOptions {
    pub url: String,
    pub original_url: Option<String>,
    pub referrer: Option<String>,
    pub document_url: Option<String>,
    pub user_agent: Option<String>,
    pub cookies: Option<String>,
    pub post_data: Option<String>,
    pub custom_headers: BTreeMap<String, String>,
    pub mirrors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub enabled: bool,
    /// Bitmask where bit 0 = Monday .. bit 6 = Sunday (127 = every day)
    pub days_mask: u8,
    /// Minutes since midnight (0..=1439)
    pub start_minute: u16,
    /// Minutes since midnight (0..=1440), may wrap past midnight if end < start
    pub end_minute: u16,
}

impl Default for ScheduleRule {
    fn default() -> Self {
        Self {
            enabled: false,
            days_mask: 0b0111_1111,
            start_minute: 0,
            end_minute: 360, // 00:00 -> 06:00 default window
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub uuid: String,
    pub parent_id: Option<DownloadId>,
    pub children_ids: Vec<DownloadId>,
    pub order_index: i64,
    pub download_type: DownloadType,
    pub state: DownloadState,
    pub priority: DownloadPriority,
    pub resume_support: ResumeSupport,
    pub flags: DownloadFlags,
    pub title: String,
    pub url: String,
    pub target_dir: PathBuf,
    pub file_name: String,
    pub partial_suffix: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub connections_active: u32,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub file_type: FileType,
    pub http: HttpRequestOptions,
    pub torrent_info_hash: Option<String>,
    pub torrent_source: Option<String>,
    pub tags: Vec<i64>,
    pub files: Vec<DownloadFileEntry>,
    pub segments: Vec<SegmentProgress>,
    pub peers: Vec<PeerInfo>,
    pub trackers: Vec<TrackerInfo>,
    pub schedule: Option<ScheduleRule>,
    pub retry_count: u32,
    pub forbid_auto_retry_count: u32,
    pub last_error: Option<String>,
    pub abort_reason: DownloadAbortReason,
    pub preview_path: Option<PathBuf>,
    pub module_data: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl DownloadRecord {
    pub fn output_path(&self) -> PathBuf {
        self.target_dir.join(&self.file_name)
    }

    pub fn partial_path(&self) -> PathBuf {
        if self.partial_suffix.is_empty() {
            self.output_path()
        } else {
            self.target_dir
                .join(format!("{}{}", self.file_name, self.partial_suffix))
        }
    }

    pub fn progress_fraction(&self) -> f64 {
        match self.total_bytes {
            Some(total) if total > 0 => (self.downloaded_bytes as f64 / total as f64).clamp(0.0, 1.0),
            _ => {
                if self.state == DownloadState::Completed {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color_hex: String,
    pub is_builtin: bool,
    pub file_types: Vec<FileType>,
    pub default_extensions: Vec<String>,
    pub default_download_folder: Option<PathBuf>,
}

impl Default for Tag {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            color_hex: "#2983FE".into(),
            is_builtin: false,
            file_types: Vec::new(),
            default_extensions: Vec::new(),
            default_download_folder: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySettings {
    pub mode: NetworkProxyMode,
    pub http: ProxyEndpoint,
    pub https: ProxyEndpoint,
    pub ftp: ProxyEndpoint,
    pub socks5: ProxyEndpoint,
    pub dont_use_for_remote_access: bool,
    pub dont_use_for_remote_access_listener: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: NetworkProxyMode::SystemProxy,
            http: ProxyEndpoint::default(),
            https: ProxyEndpoint::default(),
            ftp: ProxyEndpoint::default(),
            socks5: ProxyEndpoint::default(),
            dont_use_for_remote_access: true,
            dont_use_for_remote_access_listener: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SslException {
    pub host_port: String,
    pub sha256_fingerprint: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredential {
    pub scope: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterQuery {
    pub state_filter: StateFilter,
    pub tag_id: Option<i64>,
    pub missing_filter: MissingFilesFilter,
    pub search_text: String,
    pub sort_by: SortField,
    pub sort_ascending: bool,
    pub only_top_level: bool,
    pub parent_id_filter: Option<DownloadId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDownloadRequest {
    pub url: String,
    pub title: Option<String>,
    pub target_dir: Option<PathBuf>,
    pub file_name: Option<String>,
    pub download_type: Option<DownloadType>,
    pub priority: DownloadPriority,
    pub existing_file_reaction: Option<ExistingFileReaction>,
    pub http: HttpRequestOptions,
    pub tags: Vec<i64>,
    pub schedule: Option<ScheduleRule>,
    pub silent: bool,
    pub start_paused: bool,
    pub sequential_play_asap: bool,
    pub selected_file_indices: Option<Vec<usize>>,
    pub parent_id: Option<DownloadId>,
    pub torrent_bytes_b64: Option<String>,
}

impl NewDownloadRequest {
    pub fn from_url(url: impl Into<String>) -> Self {
        let u = url.into();
        Self {
            http: HttpRequestOptions {
                url: u.clone(),
                ..Default::default()
            },
            url: u,
            title: None,
            target_dir: None,
            file_name: None,
            download_type: None,
            priority: DownloadPriority::Normal,
            existing_file_reaction: None,
            tags: Vec::new(),
            schedule: None,
            silent: false,
            start_paused: false,
            sequential_play_asap: false,
            selected_file_indices: None,
            parent_id: None,
            torrent_bytes_b64: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineEvent {
    Added { id: DownloadId },
    Updated { id: DownloadId },
    Removed { id: DownloadId },
    Completed { id: DownloadId },
    Failed { id: DownloadId, error: String },
    FileConflict { id: DownloadId, path: PathBuf },
    TrafficModeChanged { mode: TrafficUsageMode },
    SettingsUpdated,
    QuitRequested,
    QueueStats {
        total_download_bps: u64,
        total_upload_bps: u64,
        active_count: usize,
        preventer: AutoStartPreventer,
    },
}
