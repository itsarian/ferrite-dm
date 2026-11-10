use crate::model::{
    BtEncryptionPolicy, ExistingFileReaction, FileType, NotificationEvent, ProxySettings,
    ScheduleRule, ShutdownType, SortField, TrafficUsageMode,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiStyle {
    #[default]
    New,
    Classic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeleteButtonAction {
    #[default]
    AlwaysAsk,
    DeleteFiles,
    RemoveOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrafficUsageProfile {
    /// 0 = unlimited
    pub max_download_speed_bps: u64,
    /// 0 = unlimited
    pub max_upload_speed_bps: u64,
    pub max_connections: u32,
    pub max_connections_per_server: u32,
    pub max_concurrent_downloads: u32,
    /// 0.0 = unlimited
    pub max_upload_ratio: f32,
}

impl TrafficUsageProfile {
    pub fn defaults_map() -> BTreeMap<TrafficUsageMode, TrafficUsageProfile> {
        let mut map = BTreeMap::new();
        map.insert(
            TrafficUsageMode::Snail,
            TrafficUsageProfile {
                max_download_speed_bps: 64 * 1024,
                max_upload_speed_bps: 32 * 1024,
                max_connections: 4,
                max_connections_per_server: 2,
                max_concurrent_downloads: 1,
                max_upload_ratio: 1.0,
            },
        );
        map.insert(
            TrafficUsageMode::Low,
            TrafficUsageProfile {
                max_download_speed_bps: 512 * 1024,
                max_upload_speed_bps: 128 * 1024,
                max_connections: 12,
                max_connections_per_server: 4,
                max_concurrent_downloads: 2,
                max_upload_ratio: 1.5,
            },
        );
        map.insert(
            TrafficUsageMode::Medium,
            TrafficUsageProfile {
                max_download_speed_bps: 4 * 1024 * 1024,
                max_upload_speed_bps: 1024 * 1024,
                max_connections: 32,
                max_connections_per_server: 8,
                max_concurrent_downloads: 4,
                max_upload_ratio: 2.0,
            },
        );
        map.insert(
            TrafficUsageMode::High,
            TrafficUsageProfile {
                max_download_speed_bps: 0,
                max_upload_speed_bps: 0,
                max_connections: 64,
                max_connections_per_server: 16,
                max_concurrent_downloads: 8,
                max_upload_ratio: 0.0,
            },
        );
        map
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub proxy: ProxySettings,
    pub wb_intercept_downloads: bool,
    pub wb_dont_intercept_too_small: bool,
    pub wb_too_small_size_bytes: u64,
    pub wb_dont_intercept_unwanted_extensions: bool,
    pub wb_unwanted_extensions: Vec<String>,
    pub wb_intercept_only_extensions: bool,
    pub wb_intercept_extensions: Vec<String>,
    pub wb_dont_intercept_unwanted_hosts: bool,
    pub wb_unwanted_hosts: Vec<String>,
    pub wb_forward_aborted_to_browser: bool,
    pub wb_dont_intercept_if_key_pressed: bool,
    pub wb_bypass_key: String,
    pub notify_added: bool,
    pub notify_finished: bool,
    pub notify_failed: bool,
    pub notify_when_inactive_only: bool,
    pub enable_sound_notifications: bool,
    pub notification_sounds: BTreeMap<NotificationEvent, PathBuf>,
    pub silent_mode_for_new_downloads: bool,
    pub locale_name: String,
    pub sort_by: SortField,
    pub sort_ascending: bool,
    pub show_total_upload_speed_in_dock: bool,
    pub indicate_dl_ul_in_tray: bool,
    pub hide_dock_icon: bool,
    pub torrents_watch_folder: Option<PathBuf>,
    pub force_silent_from_torrent_folder: bool,
    pub update_plugins_automatically: bool,
    pub plugins_allow_wb_cookies: bool,
    pub enable_remote_access: bool,
    pub remote_access_port: u16,
    pub remote_access_bind: String,
    pub remote_control_server_id: String,
    pub portable_mode: bool,
    pub autorun_on_login: bool,
    pub quit_confirmation: bool,
    pub theme_mode: ThemeMode,
    pub ui_style: UiStyle,
    pub launch_minimized: bool,
    pub enable_standalone_downloads_windows: bool,
    pub enable_standalone_create_downloads_windows: bool,
    pub close_to_background: bool,
    pub toggle_bottom_panel_on_click: bool,
    pub show_save_as_button: bool,
    pub show_support_button: bool,
    pub compact_downloads_view: bool,
    pub hidden_builtin_tags: Vec<i64>,
    pub overall_zoom_pct: u16,
    pub fonts_zoom_pct: u16,
    pub delete_button_action: DeleteButtonAction,
    pub db_backup_interval_secs: i64,
    pub batch_download_max_urls_count: u32,
    pub auto_check_updates: bool,
    pub auto_install_updates: bool,
    pub check_if_default_torrent_client: bool,
    pub enable_logging: bool,
    pub enhanced_logging: bool,
    pub lt_alerts_logging: bool,
    pub downloads_mgmt_logging: bool,
    pub remote_access_password: String,
    pub last_remote_app_id: String,
    pub plugins_dev_ignore_sign: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy: ProxySettings::default(),
            wb_intercept_downloads: true,
            wb_dont_intercept_too_small: true,
            wb_too_small_size_bytes: 1024 * 100, // 100 KB default threshold
            wb_dont_intercept_unwanted_extensions: true,
            wb_unwanted_extensions: vec!["pls".into(), "m3u".into()],
            wb_intercept_only_extensions: false,
            wb_intercept_extensions: vec![
                "zip", "rar", "7z", "tar", "gz", "iso", "dmg", "pkg", "mp4", "mkv", "mp3", "flac",
                "pdf",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            wb_dont_intercept_unwanted_hosts: true,
            wb_unwanted_hosts: vec![
                "mega.nz",
                "uploaded.net",
                "rockfile.eu",
                "nitroflare.com",
                "rapidgator.net",
                "mail-attachment.googleusercontent.com",
                "dl-mail.ymail.com",
                "web.telegram.org",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            wb_forward_aborted_to_browser: true,
            wb_dont_intercept_if_key_pressed: true,
            wb_bypass_key: "Alt".into(),
            notify_added: false,
            notify_finished: true,
            notify_failed: true,
            notify_when_inactive_only: false,
            enable_sound_notifications: false,
            notification_sounds: BTreeMap::new(),
            silent_mode_for_new_downloads: false,
            locale_name: "en_US".into(),
            sort_by: SortField::CreationTime,
            sort_ascending: false,
            show_total_upload_speed_in_dock: false,
            indicate_dl_ul_in_tray: true,
            hide_dock_icon: false,
            torrents_watch_folder: None,
            force_silent_from_torrent_folder: false,
            update_plugins_automatically: false,
            plugins_allow_wb_cookies: true,
            enable_remote_access: false,
            remote_access_port: 9443,
            remote_access_bind: "0.0.0.0".into(),
            remote_control_server_id: uuid::Uuid::new_v4().to_string(),
            portable_mode: false,
            autorun_on_login: false,
            quit_confirmation: true,
            theme_mode: ThemeMode::System,
            ui_style: UiStyle::New,
            launch_minimized: false,
            enable_standalone_downloads_windows: false,
            enable_standalone_create_downloads_windows: false,
            close_to_background: false,
            toggle_bottom_panel_on_click: false,
            show_save_as_button: true,
            show_support_button: false,
            compact_downloads_view: false,
            hidden_builtin_tags: Vec::new(),
            overall_zoom_pct: 100,
            fonts_zoom_pct: 100,
            delete_button_action: DeleteButtonAction::AlwaysAsk,
            db_backup_interval_secs: 3600,
            batch_download_max_urls_count: 1000,
            auto_check_updates: false,
            auto_install_updates: false,
            check_if_default_torrent_client: false,
            enable_logging: false,
            enhanced_logging: false,
            lt_alerts_logging: false,
            downloads_mgmt_logging: false,
            remote_access_password: String::new(),
            last_remote_app_id: String::new(),
            plugins_dev_ignore_sign: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DmCoreSettings {
    pub default_download_path: PathBuf,
    pub fixed_download_path: bool,
    pub download_path_depends_on_file_type: bool,
    pub download_path_depends_on_source_url: bool,
    pub tum_profiles: BTreeMap<TrafficUsageMode, TrafficUsageProfile>,
    pub current_tum: TrafficUsageMode,
    pub restore_tum_at_startup: bool,
    pub max_additional_query_info_downloads: u32,
    pub max_additional_small_downloads: u32,
    pub max_additional_downloads_if_slow: u32,
    pub traffic_limits_compliant_mode: bool,
    pub partial_download_suffix: String,
    pub allow_mobile_networks: bool,
    pub allow_roaming_networks: bool,
    pub take_last_modified_from_server: bool,
    pub auto_remove_missing_files: bool,
    pub prevent_sleep_if_running: bool,
    pub prevent_sleep_ignore_finished: bool,
    pub prevent_sleep_ignore_resumable: bool,
    pub prevent_sleep_include_scheduled: bool,
    pub auto_remove_finished: bool,
    pub auto_remove_finished_keep_days: u32,
    pub antivirus_uid: String,
    pub antivirus_path: Option<PathBuf>,
    pub antivirus_args: String,
    pub auto_launch_antivirus: bool,
    pub auto_restart_if_remote_changed: bool,
    pub existing_file_reaction: ExistingFileReaction,
    pub auto_retry_failed: bool,
    pub max_auto_retries: u32,
    pub disable_post_finished_on_battery: bool,
    pub disable_post_finished_tasks: bool,
    pub battery_min_level_percent: u8,
    pub delete_service_files_when_finished: bool,
    pub bt_session_port: u16,
    pub bt_enable_tracker_list: bool,
    pub bt_tracker_list: Vec<String>,
    pub bt_encryption_policy: BtEncryptionPolicy,
    pub bt_enable_dht: bool,
    pub bt_enable_utp: bool,
    pub bt_enable_lsd: bool,
    pub bt_enable_port_forwarding: bool,
    pub bt_enable_pex: bool,
    pub launch_external_on_completion: bool,
    pub external_app_path: Option<PathBuf>,
    pub external_app_args_template: String,
    pub enable_scheduler_for_new_downloads: bool,
    pub default_schedule: ScheduleRule,
    pub detect_unwanted_errors: bool,
    pub use_system_default_user_agent: bool,
    pub set_mark_of_the_web: bool,
    pub detect_unacceptably_slow: bool,
    pub unacceptably_slow_speed_bps: u64,
    pub unacceptably_slow_interval_secs: u64,
    pub shutdown_when_done: ShutdownType,
    pub low_disk_space_min_bytes: u64,
    pub low_disk_space_min_percent: f32,
    pub ffmpeg_path: Option<PathBuf>,
    pub yt_dlp_path: Option<PathBuf>,
    pub bt_use_random_port: bool,
    pub antivirus_custom_mode: bool,
    pub wb_small_size_unit_mb: bool,
}

impl Default for DmCoreSettings {
    fn default() -> Self {
        let default_dl = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("./downloads"));
        let ffmpeg = [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
        let yt_dlp = [
            "/opt/homebrew/bin/yt-dlp",
            "/usr/local/bin/yt-dlp",
            "/usr/bin/yt-dlp",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists());

        Self {
            default_download_path: default_dl,
            fixed_download_path: false,
            download_path_depends_on_file_type: false,
            download_path_depends_on_source_url: false,
            tum_profiles: TrafficUsageProfile::defaults_map(),
            current_tum: TrafficUsageMode::High,
            restore_tum_at_startup: true,
            max_additional_query_info_downloads: 2,
            max_additional_small_downloads: 2,
            max_additional_downloads_if_slow: 2,
            traffic_limits_compliant_mode: true,
            partial_download_suffix: ".fdmdownload".into(),
            allow_mobile_networks: true,
            allow_roaming_networks: false,
            take_last_modified_from_server: true,
            auto_remove_missing_files: false,
            prevent_sleep_if_running: true,
            prevent_sleep_ignore_finished: true,
            prevent_sleep_ignore_resumable: false,
            prevent_sleep_include_scheduled: false,
            auto_remove_finished: false,
            auto_remove_finished_keep_days: 30,
            antivirus_uid: String::new(),
            antivirus_path: None,
            antivirus_args: "\"{file}\"".into(),
            auto_launch_antivirus: false,
            auto_restart_if_remote_changed: false,
            existing_file_reaction: ExistingFileReaction::Rename,
            auto_retry_failed: true,
            max_auto_retries: 3,
            disable_post_finished_on_battery: true,
            disable_post_finished_tasks: false,
            battery_min_level_percent: 15,
            delete_service_files_when_finished: true,
            bt_session_port: 6881,
            bt_enable_tracker_list: false,
            bt_tracker_list: vec![
                "udp://tracker.opentrackr.org:1337/announce".into(),
                "udp://open.stealth.si:80/announce".into(),
            ],
            bt_encryption_policy: BtEncryptionPolicy::PreferEncryption,
            bt_enable_dht: true,
            bt_enable_utp: true,
            bt_enable_lsd: true,
            bt_enable_port_forwarding: true,
            bt_enable_pex: true,
            launch_external_on_completion: false,
            external_app_path: None,
            external_app_args_template: "\"{file}\"".into(),
            enable_scheduler_for_new_downloads: false,
            default_schedule: ScheduleRule::default(),
            detect_unwanted_errors: true,
            use_system_default_user_agent: false,
            set_mark_of_the_web: true,
            detect_unacceptably_slow: false,
            unacceptably_slow_speed_bps: 16 * 1024,
            unacceptably_slow_interval_secs: 60,
            shutdown_when_done: ShutdownType::None,
            low_disk_space_min_bytes: 256 * 1024 * 1024,
            low_disk_space_min_percent: 2.0,
            ffmpeg_path: ffmpeg,
            yt_dlp_path: yt_dlp,
            bt_use_random_port: false,
            antivirus_custom_mode: false,
            wb_small_size_unit_mb: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FdmConfig {
    pub app: AppSettings,
    pub core: DmCoreSettings,
    pub path_by_file_type: BTreeMap<FileType, PathBuf>,
    pub path_by_url_hash: BTreeMap<String, PathBuf>,
    pub recent_folders: Vec<PathBuf>,
}

impl FdmConfig {
    pub fn active_profile(&self) -> TrafficUsageProfile {
        self.core
            .tum_profiles
            .get(&self.core.current_tum)
            .cloned()
            .unwrap_or_else(|| {
                TrafficUsageProfile::defaults_map()
                    .remove(&self.core.current_tum)
                    .unwrap()
            })
    }

    pub fn data_dir(portable: bool) -> PathBuf {
        if portable {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    return parent.join("ferrite-data");
                }
            }
            return PathBuf::from("./ferrite-data");
        }
        if let Some(proj) = directories::ProjectDirs::from("org", "ferritedm", "FerriteDM") {
            proj.data_local_dir().to_path_buf()
        } else {
            PathBuf::from("./ferrite-data")
        }
    }
}
