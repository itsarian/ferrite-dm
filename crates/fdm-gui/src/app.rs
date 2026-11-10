use crate::bottom_panel::render_bottom_panel;
use crate::helpers::{
    LaunchOptions, UiAsyncMsg, UiMsgQueue, browser_display_name, detect_lan_ip,
    download_icon_and_color, extract_domain_label, extract_first_download_url,
    format_added_timestamp, format_bytes, format_compact_dock_badge, format_eta,
    format_kb_limit, format_minute_hhmm, format_speed, install_native_host_for_browser,
    macos_bring_all_to_front, macos_hide_all_windows, macos_restore_windows_and_activate,
    macos_unhide_all_applications, open_path_os, parse_kb_limit, parse_list_tokens,
    parse_minute_hhmm, parse_wb_threshold_bytes,
    preventer_label, reveal_path_os, set_macos_hide_dock_icon, sort_field_short_label,
    state_badge_info, state_filter_short_label, sync_portable_marker, update_macos_dock_badge,
    zoom_rem_size_px,
};
use crate::i18n;
use crate::menus::render_popover;
use crate::modals::render_active_modal;
use crate::plugins_page::render_plugins_page;
use crate::settings_page::render_settings_page;
use crate::standalone::{open_standalone_add_window, open_standalone_download_window};
use crate::text_input::{InputCopy, InputCut, InputPaste, InputRedo, InputSelectAll, InputUndo, TextInput};
use crate::theme::{ThemePalette, parse_hex_color};
use crate::ui_widgets::{
    action_btn, badge_themed, danger_btn, ellipsis_text, fdm_checkbox, fdm_indeterminate_checkbox,
    ghost_icon_btn, icon_action_btn, slim_progress_bar_active, snail_switch_btn, svg_icon,
};
use fdm_cli::{AddProbeOutcome, DownloadEngine, EngineStateSummary};
use fdm_core::os_integration::{
    is_default_torrent_client, play_notification_sound, send_os_notification, sync_autorun_on_login,
};
use fdm_core::{
    DeleteButtonAction, DownloadId, DownloadPriority, DownloadRecord, DownloadState, DownloadType,
    EngineEvent, FdmConfig, FileExistsAction, FilterQuery, HashAlgorithm, KnownWebBrowser,
    NewDownloadRequest, NotificationEvent, ProxyEndpoint, ProxySettings, ScheduleRule,
    ShutdownType, SortField, SslException, StateFilter, Tag, ThemeMode, TrafficUsageMode,
};
use fdm_ipc::RemotePairingInfo;
use fdm_media::{JsPluginInfo, Mp3QualityOption, convert_to_mp3_with_options, convert_to_mp4};
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, Entity, FocusHandle, Focusable,
    FontWeight, Global, KeyBinding, Menu, MenuItem, MouseButton, OsAction, PathPromptOptions,
    Render, SharedString, SystemMenuType, Window, WindowControlArea, actions, div,
    ease_out_quint, prelude::*, px, rgb,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

actions!(
    ferrite_dm,
    [
        QuitApp,
        AboutAppAction,
        HideAppAction,
        HideOtherAppsAction,
        ShowAllAppsAction,
        CloseWindowAction,
        AddUrlAction,
        BatchAddAction,
        PasteUrlAction,
        OpenSettingsAction,
        OpenPluginsAction,
        OpenQrAction,
        FocusSearchAction,
        ToggleSelectedAction,
        EscapeModalAction,
        DeleteSelectedAction,
        ZoomInAction,
        ZoomOutAction,
        ZoomResetAction,
        ToggleThemeAction,
        ToggleInspectorAction,
        ToggleFullscreenAction,
        MinimizeWindowAction,
        ZoomWindowAction,
        StartAllAction,
        PauseAllAction,
        SelectAllDownloadsAction,
        ShowTroubleshootingAction,
        FilterAllAction,
        FilterActiveAction,
        FilterDownloadingAction,
        FilterFinishedAction,
        FilterStoppedAction,
        OpenExportSettingsAction,
        OpenImportSettingsAction,
        CheckForUpdatesAction,
        BringAllToFrontAction,
    ]
);

#[derive(Clone)]
pub struct AppWindowGlobal {
    pub entity: gpui::WeakEntity<FerriteApp>,
    pub window: gpui::WindowHandle<FerriteApp>,
}

impl Global for AppWindowGlobal {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePage {
    Downloads,
    Settings(SettingsTab),
    Plugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorTab {
    General,
    Details,
    Files,
    Progress,
    Connections,
    SpeedGraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Browser,
    Network,
    Traffic,
    Antivirus,
    BitTorrent,
    RemoteAccess,
    Advanced,
    Troubleshooting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivePopover {
    None,
    SortMenu,
    FilterMenu,
    MainMenu,
    TumMenu,
    RowContextMenu(DownloadId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveModal {
    None,
    AddDownload,
    BatchAdd,
    FileConflict { id: DownloadId, path: PathBuf },
    HashVerify { id: DownloadId },
    MediaConvert { id: DownloadId },
    RemoteQr,
    EditTag { id: Option<i64> },
    ChangeSource { id: DownloadId },
    MoveDir { id: DownloadId },
    RenameDownload { id: DownloadId },
    DownloadSchedule { id: DownloadId },
    DeleteConfirm { ids: Vec<DownloadId> },
    QuitConfirm,
    About,
    CreatePortable,
}

pub fn ftp_mirrors_http(proxy: &ProxySettings) -> bool {
    (proxy.ftp.host.is_empty() && proxy.http.host.is_empty())
        || (proxy.ftp.host == proxy.http.host
            && proxy.ftp.port == proxy.http.port
            && proxy.ftp.username == proxy.http.username
            && proxy.ftp.password == proxy.http.password)
}

pub(crate) fn apply_proxy_inputs_to_settings(
    proxy: &mut ProxySettings,
    p_host: String,
    p_port: u16,
    s5_host: String,
    s5_port: u16,
    p_user: String,
    p_pass: String,
    use_http_for_ftp: bool,
    previous_proxy: &ProxySettings,
) {
    proxy.http.host = p_host.clone();
    proxy.http.port = p_port;
    proxy.http.username = p_user.clone();
    proxy.http.password = p_pass.clone();
    proxy.https.host = p_host;
    proxy.https.port = p_port;
    proxy.https.username = p_user.clone();
    proxy.https.password = p_pass.clone();

    let s5_active = !s5_host.is_empty();
    proxy.socks5.host = s5_host;
    proxy.socks5.port = s5_port;
    if s5_active {
        proxy.socks5.username = p_user;
        proxy.socks5.password = p_pass;
    } else {
        proxy.socks5.username.clear();
        proxy.socks5.password.clear();
    }

    if use_http_for_ftp {
        proxy.ftp = proxy.http.clone();
    } else if ftp_mirrors_http(previous_proxy) {
        proxy.ftp = ProxyEndpoint::default();
    }
}

pub fn bind_app_keys_and_menus(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", QuitApp, None),
        KeyBinding::new("cmd-h", HideAppAction, None),
        KeyBinding::new("alt-cmd-h", HideOtherAppsAction, None),
        KeyBinding::new("cmd-w", CloseWindowAction, None),
        KeyBinding::new("cmd-n", AddUrlAction, None),
        KeyBinding::new("cmd-b", BatchAddAction, None),
        KeyBinding::new("cmd-shift-v", PasteUrlAction, None),
        KeyBinding::new("cmd-v", PasteUrlAction, Some("FerriteList && !FerriteInput")),
        KeyBinding::new("cmd-,", OpenSettingsAction, None),
        KeyBinding::new("cmd-f", FocusSearchAction, None),
        KeyBinding::new("cmd-=", ZoomInAction, None),
        KeyBinding::new("cmd-+", ZoomInAction, None),
        KeyBinding::new("cmd--", ZoomOutAction, None),
        KeyBinding::new("cmd-0", ZoomResetAction, None),
        KeyBinding::new("cmd-i", ToggleInspectorAction, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreenAction, None),
        KeyBinding::new("cmd-m", MinimizeWindowAction, None),
        KeyBinding::new("cmd-shift-a", SelectAllDownloadsAction, None),
        KeyBinding::new("escape", EscapeModalAction, None),
        KeyBinding::new("space", ToggleSelectedAction, Some("FerriteList && !FerriteInput")),
        KeyBinding::new(
            "backspace",
            DeleteSelectedAction,
            Some("FerriteList && !FerriteInput"),
        ),
        KeyBinding::new(
            "delete",
            DeleteSelectedAction,
            Some("FerriteList && !FerriteInput"),
        ),
    ]);

    cx.set_menus(vec![
        Menu {
            name: "Ferrite DM".into(),
            items: vec![
                MenuItem::action("About Ferrite DM", AboutAppAction),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettingsAction),
                MenuItem::action("Check for Updates…", CheckForUpdatesAction),
                MenuItem::action("Add-ons…", OpenPluginsAction),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Ferrite DM", HideAppAction),
                MenuItem::action("Hide Others", HideOtherAppsAction),
                MenuItem::action("Show All", ShowAllAppsAction),
                MenuItem::separator(),
                MenuItem::action("Quit Ferrite DM", QuitApp),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Add URL / Torrent…", AddUrlAction),
                MenuItem::action("Batch Download…", BatchAddAction),
                MenuItem::separator(),
                MenuItem::action("Import / Legacy FDM…", OpenImportSettingsAction),
                MenuItem::action("Export Downloads / Settings…", OpenExportSettingsAction),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindowAction),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", InputUndo, OsAction::Undo),
                MenuItem::os_action("Redo", InputRedo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", InputCut, OsAction::Cut),
                MenuItem::os_action("Copy", InputCopy, OsAction::Copy),
                MenuItem::os_action("Paste", InputPaste, OsAction::Paste),
                MenuItem::os_action("Select All", InputSelectAll, OsAction::SelectAll),
                MenuItem::separator(),
                MenuItem::action("Find…", FocusSearchAction),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Toggle Details Inspector", ToggleInspectorAction),
                MenuItem::separator(),
                MenuItem::action("Zoom In", ZoomInAction),
                MenuItem::action("Zoom Out", ZoomOutAction),
                MenuItem::action("Actual Size", ZoomResetAction),
                MenuItem::separator(),
                MenuItem::action("Toggle Light / Dark Theme", ToggleThemeAction),
                MenuItem::submenu(Menu {
                    name: "Filter Downloads".into(),
                    items: vec![
                        MenuItem::action("All Downloads", FilterAllAction),
                        MenuItem::action("Active", FilterActiveAction),
                        MenuItem::action("Downloading", FilterDownloadingAction),
                        MenuItem::action("Completed", FilterFinishedAction),
                        MenuItem::action("Paused / Stopped", FilterStoppedAction),
                    ],
                }),
            ],
        },
        Menu {
            name: "Downloads".into(),
            items: vec![
                MenuItem::action("Add Download URL…", AddUrlAction),
                MenuItem::action("Paste URL from Clipboard…", PasteUrlAction),
                MenuItem::action("Batch Download…", BatchAddAction),
                MenuItem::separator(),
                MenuItem::action("Start All Downloads", StartAllAction),
                MenuItem::action("Pause All Downloads", PauseAllAction),
                MenuItem::action("Select / Deselect All Visible", SelectAllDownloadsAction),
                MenuItem::separator(),
                MenuItem::action("Remote Control QR…", OpenQrAction),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", MinimizeWindowAction),
                MenuItem::action("Zoom", ZoomWindowAction),
                MenuItem::separator(),
                MenuItem::action("Bring All to Front", BringAllToFrontAction),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindowAction),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![
                MenuItem::action("Check for Updates…", CheckForUpdatesAction),
                MenuItem::separator(),
                MenuItem::action("Troubleshooting & Diagnostics…", ShowTroubleshootingAction),
                MenuItem::separator(),
                MenuItem::action("About Ferrite DM", AboutAppAction),
            ],
        },
    ]);

    cx.set_dock_menu(vec![
        MenuItem::action("Add Download URL…", AddUrlAction),
        MenuItem::action("Paste URL from Clipboard…", PasteUrlAction),
        MenuItem::action("Batch Download…", BatchAddAction),
        MenuItem::separator(),
        MenuItem::action("Start All Downloads", StartAllAction),
        MenuItem::action("Pause All Downloads", PauseAllAction),
        MenuItem::separator(),
        MenuItem::action("Settings…", OpenSettingsAction),
    ]);

    cx.on_action(|_: &QuitApp, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                if g.window
                    .update(cx, |view, _window, cx| {
                        view.request_quit(cx);
                    })
                    .is_err()
                {
                    cx.quit();
                }
            });
        } else {
            cx.quit();
        }
    });
    cx.on_action(|_: &AboutAppAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_about_modal(cx);
                });
            });
        }
    });
    cx.on_action(|_: &HideAppAction, cx| {
        cx.hide();
    });
    cx.on_action(|_: &HideOtherAppsAction, cx| {
        cx.hide_other_apps();
    });
    cx.on_action(|_: &ShowAllAppsAction, cx| {
        cx.unhide_other_apps();
        macos_unhide_all_applications();
    });
    cx.on_action(|_: &CloseWindowAction, cx| {
        cx.defer(|cx| {
            if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                if !g.window.is_active(cx).unwrap_or(false) {
                    if let Some(aw) = cx.active_window() {
                        let _ = aw.update(cx, |_, w, _| w.remove_window());
                    }
                    return;
                }
                let _ = g.window.update(cx, |view, window, cx| {
                    view.handle_close_window_request(window, cx);
                });
            } else if let Some(aw) = cx.active_window() {
                let _ = aw.update(cx, |_, w, _| w.remove_window());
            }
        });
    });
    cx.on_action(|_: &AddUrlAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.open_add_modal("", window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &BatchAddAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.open_batch_modal(window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &PasteUrlAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.paste_from_clipboard_add(window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &OpenSettingsAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_settings_page(SettingsTab::General, cx);
                });
            });
        }
    });
    cx.on_action(|_: &CheckForUpdatesAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.entity.update(cx, |view, cx| {
                    view.check_for_app_updates_action(true, cx);
                });
            });
        }
    });
    cx.on_action(|_: &OpenPluginsAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_plugins_page(cx);
                });
            });
        }
    });
    cx.on_action(|_: &OpenImportSettingsAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_settings_page(SettingsTab::Advanced, cx);
                });
            });
        }
    });
    cx.on_action(|_: &OpenExportSettingsAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_settings_page(SettingsTab::Advanced, cx);
                });
            });
        }
    });
    cx.on_action(|_: &ShowTroubleshootingAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.show_troubleshooting = true;
                    view.open_settings_page(SettingsTab::Troubleshooting, cx);
                });
            });
        }
    });
    cx.on_action(|_: &OpenQrAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.open_remote_qr_modal(cx);
                });
            });
        }
    });
    cx.on_action(|_: &FocusSearchAction, cx| {
        macos_restore_windows_and_activate();
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.set_active_page(ActivePage::Downloads, cx);
                    let fh = view.search_input.read(cx).focus_handle.clone();
                    window.focus(&fh);
                });
            });
        }
    });
    cx.on_action(|_: &ZoomInAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.adjust_zoom(Some(10), window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &ZoomOutAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.adjust_zoom(Some(-10), window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &ZoomResetAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    view.adjust_zoom(None, window, cx);
                });
            });
        }
    });
    cx.on_action(|_: &ToggleThemeAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.toggle_theme(cx);
                });
            });
        }
    });
    cx.on_action(|_: &ToggleInspectorAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, _window, cx| {
                    view.set_active_page(ActivePage::Downloads, cx);
                    let next = !view.inspector_expanded;
                    view.set_inspector_expanded(next, cx);
                });
            });
        }
    });
    cx.on_action(|_: &ToggleFullscreenAction, cx| {
        cx.defer(|cx| {
            if let Some(aw) = cx.active_window() {
                let _ = aw.update(cx, |_, window, _| window.toggle_fullscreen());
            }
        });
    });
    cx.on_action(|_: &MinimizeWindowAction, cx| {
        cx.defer(|cx| {
            if let Some(aw) = cx.active_window() {
                let _ = aw.update(cx, |_, window, _| window.minimize_window());
            }
        });
    });
    cx.on_action(|_: &ZoomWindowAction, cx| {
        cx.defer(|cx| {
            if let Some(aw) = cx.active_window() {
                let _ = aw.update(cx, |_, window, _| window.zoom_window());
            }
        });
    });
    cx.on_action(|_: &BringAllToFrontAction, cx| {
        cx.defer(|_| {
            macos_bring_all_to_front();
        });
    });
    cx.on_action(|_: &StartAllAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.entity.update(cx, |view, cx| {
                    let eng = Arc::clone(&view.engine);
                    view.rt.spawn(async move {
                        let _ = eng.start_all().await;
                    });
                    cx.notify();
                });
            });
        }
    });
    cx.on_action(|_: &PauseAllAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.entity.update(cx, |view, cx| {
                    let eng = Arc::clone(&view.engine);
                    view.rt.spawn(async move {
                        let _ = eng.pause_all().await;
                    });
                    cx.notify();
                });
            });
        }
    });
    cx.on_action(|_: &SelectAllDownloadsAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.entity.update(cx, |view, cx| {
                    view.set_active_page(ActivePage::Downloads, cx);
                    view.toggle_select_all_visible(cx);
                });
            });
        }
    });
    for (filter_action_id, state_f) in [
        (0usize, StateFilter::All),
        (1, StateFilter::Running),
        (2, StateFilter::Downloading),
        (3, StateFilter::Finished),
        (4, StateFilter::Stopped),
    ] {
        match filter_action_id {
            0 => cx.on_action(move |_: &FilterAllAction, cx| {
                if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                    cx.defer(move |cx| {
                        let _ = g.entity.update(cx, |view, cx| {
                            view.apply_state_filter_shortcut(state_f, cx);
                        });
                    });
                }
            }),
            1 => cx.on_action(move |_: &FilterActiveAction, cx| {
                if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                    cx.defer(move |cx| {
                        let _ = g.entity.update(cx, |view, cx| {
                            view.apply_state_filter_shortcut(state_f, cx);
                        });
                    });
                }
            }),
            2 => cx.on_action(move |_: &FilterDownloadingAction, cx| {
                if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                    cx.defer(move |cx| {
                        let _ = g.entity.update(cx, |view, cx| {
                            view.apply_state_filter_shortcut(state_f, cx);
                        });
                    });
                }
            }),
            3 => cx.on_action(move |_: &FilterFinishedAction, cx| {
                if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                    cx.defer(move |cx| {
                        let _ = g.entity.update(cx, |view, cx| {
                            view.apply_state_filter_shortcut(state_f, cx);
                        });
                    });
                }
            }),
            _ => cx.on_action(move |_: &FilterStoppedAction, cx| {
                if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                    cx.defer(move |cx| {
                        let _ = g.entity.update(cx, |view, cx| {
                            view.apply_state_filter_shortcut(state_f, cx);
                        });
                    });
                }
            }),
        }
    }
    cx.on_action(|_: &ToggleSelectedAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.entity.update(cx, |view, cx| {
                    view.toggle_selected_download(cx);
                });
            });
        }
    });
    cx.on_action(|_: &DeleteSelectedAction, cx| {
        if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
            cx.defer(move |cx| {
                let _ = g.window.update(cx, |view, window, cx| {
                    if view.active_page == ActivePage::Downloads
                        && view.active_modal == ActiveModal::None
                    {
                        let ids: Vec<DownloadId> = if !view.checked_ids.is_empty() {
                            view.checked_ids.iter().copied().collect()
                        } else if let Some(id) = view.selected_id {
                            vec![id]
                        } else {
                            Vec::new()
                        };
                        if !ids.is_empty() {
                            view.request_delete_downloads(ids, window, cx);
                        }
                    }
                });
            });
        }
    });
    cx.on_action(|_: &EscapeModalAction, cx| {
        cx.defer(|cx| {
            if let Some(g) = cx.try_global::<AppWindowGlobal>().cloned() {
                if !g.window.is_active(cx).unwrap_or(false) {
                    if let Some(aw) = cx.active_window() {
                        let _ = aw.update(cx, |_, w, _| w.remove_window());
                    }
                    return;
                }
                let _ = g.window.update(cx, |view, window, cx| {
                    view.handle_escape(window, cx);
                });
            } else if let Some(aw) = cx.active_window() {
                let _ = aw.update(cx, |_, w, _| w.remove_window());
            }
        });
    });
}

pub struct FerriteApp {
    pub(crate) rt: Arc<tokio::runtime::Runtime>,
    pub(crate) engine: Arc<DownloadEngine>,
    pub(crate) msgs: UiMsgQueue,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) theme: ThemePalette,

    pub(crate) active_page: ActivePage,
    pub(crate) summary: EngineStateSummary,
    pub(crate) all_downloads: Vec<DownloadRecord>,
    pub(crate) visible_downloads: Vec<DownloadRecord>,
    pub(crate) tags: Vec<Tag>,
    pub(crate) selected_id: Option<DownloadId>,
    pub(crate) parent_folder_id: Option<DownloadId>,
    pub(crate) hovered_row_id: Option<DownloadId>,
    pub(crate) checked_ids: HashSet<DownloadId>,
    pub(crate) active_filter: StateFilter,
    pub(crate) active_tag_id: Option<i64>,
    pub(crate) sort_by: SortField,
    pub(crate) sort_ascending: bool,
    pub(crate) inspector_tab: InspectorTab,
    pub(crate) inspector_expanded: bool,
    pub(crate) last_non_snail_tum: TrafficUsageMode,
    pub(crate) active_popover: ActivePopover,
    pub(crate) shutdown_submenu_open: bool,
    pub(crate) active_modal: ActiveModal,
    pub(crate) delete_remember_choice: bool,
    pub(crate) toast: Option<(String, bool, Instant)>,

    // One-shot animation epochs (0% idle CPU) & OS window tracking
    pub(crate) modal_anim_epoch: u64,
    pub(crate) popover_anim_epoch: u64,
    pub(crate) plugins_gear_anim_epoch: u64,
    pub(crate) inspector_anim_epoch: u64,
    pub(crate) inspector_closing_since: Option<Instant>,
    pub(crate) toast_anim_epoch: u64,
    pub(crate) page_anim_epoch: u64,
    pub(crate) settings_tab_anim_epoch: u64,
    pub(crate) inspector_tab_anim_epoch: u64,
    pub(crate) adv_panel_anim_epoch: u64,
    pub(crate) probe_anim_epoch: u64,
    pub(crate) shutdown_sub_anim_epoch: u64,
    pub(crate) proxy_box_anim_epoch: u64,
    pub(crate) av_preset_anim_epoch: u64,
    pub(crate) last_inspected_modal: ActiveModal,
    pub(crate) last_dock_badge: Option<String>,
    pub(crate) window_active: bool,
    pub(crate) manual_torrent_check_pending: bool,

    pub(crate) search_input: Entity<TextInput>,

    // Add / Tune & Add modal
    pub(crate) add_url_input: Entity<TextInput>,
    pub(crate) add_dir_input: Entity<TextInput>,
    pub(crate) add_name_input: Entity<TextInput>,
    pub(crate) add_ref_input: Entity<TextInput>,
    pub(crate) add_user_input: Entity<TextInput>,
    pub(crate) add_pass_input: Entity<TextInput>,
    pub(crate) add_show_pass: bool,
    pub(crate) add_priority: DownloadPriority,
    pub(crate) add_start_paused: bool,
    pub(crate) add_play_asap: bool,
    pub(crate) add_apply_schedule: bool,
    pub(crate) add_show_advanced: bool,
    pub(crate) add_probe_busy: bool,
    pub(crate) add_probe_outcome: Option<AddProbeOutcome>,
    pub(crate) add_selected_files: HashSet<usize>,
    pub(crate) add_selected_variant: usize,

    // Batch modal
    pub(crate) batch_pattern_input: Entity<TextInput>,
    pub(crate) batch_group_title_input: Entity<TextInput>,
    pub(crate) batch_dir_input: Entity<TextInput>,
    pub(crate) batch_start_paused: bool,
    pub(crate) batch_as_group: bool,

    // Hash verification modal
    pub(crate) hash_algo: HashAlgorithm,
    pub(crate) hash_expected_input: Entity<TextInput>,
    pub(crate) hash_computed: Option<String>,
    pub(crate) hash_matched: Option<bool>,

    // Media convert modal
    pub(crate) convert_mode_mp3: bool,
    pub(crate) convert_mp3_bitrate: u32,
    pub(crate) convert_use_vbr: bool,
    pub(crate) convert_vbr_quality: u8,
    pub(crate) convert_stream_copy: bool,
    pub(crate) convert_out_input: Entity<TextInput>,
    pub(crate) convert_busy: bool,
    pub(crate) convert_status: Option<String>,

    // Remote QR modal
    pub(crate) remote_pairing: Option<RemotePairingInfo>,
    pub(crate) remote_loading: bool,
    pub(crate) remote_error: Option<String>,

    // New / Edit tag modal
    pub(crate) new_tag_name_input: Entity<TextInput>,
    pub(crate) new_tag_color_hex: String,
    pub(crate) tag_exts_input: Entity<TextInput>,
    pub(crate) tag_dir_input: Entity<TextInput>,

    // Change URL / mirrors modal
    pub(crate) source_url_input: Entity<TextInput>,
    pub(crate) source_mirrors_input: Entity<TextInput>,

    // Move dir modal
    pub(crate) move_dir_input: Entity<TextInput>,

    // Rename output file modal
    pub(crate) rename_file_input: Entity<TextInput>,

    // Portable bundle modal
    pub(crate) portable_dir_input: Entity<TextInput>,

    // Per-download schedule modal
    pub(crate) sched_enabled: bool,
    pub(crate) sched_days_mask: u8,
    pub(crate) sched_start_input: Entity<TextInput>,
    pub(crate) sched_end_input: Entity<TextInput>,

    // Bottom panel BT tracker input
    pub(crate) bt_add_tracker_input: Entity<TextInput>,

    // Add-ons page state
    pub(crate) plugins_title_clicks: u8,
    pub(crate) show_plugin_dev: bool,
    pub(crate) plugins_gear_menu_open: bool,
    pub(crate) plugin_install_input: Entity<TextInput>,

    // Settings page draft + state + inputs
    pub(crate) settings_cfg: FdmConfig,
    pub(crate) settings_tum_tab: TrafficUsageMode,
    pub(crate) settings_plugins: Vec<JsPluginInfo>,
    pub(crate) settings_ssl_exceptions: Vec<SslException>,
    pub(crate) prefs_title_clicks: u8,
    pub(crate) show_troubleshooting: bool,
    pub(crate) show_proxy_pass: bool,
    pub(crate) show_remote_pass: bool,
    pub(crate) proxy_use_http_for_ftp: bool,

    pub(crate) set_default_dir: Entity<TextInput>,
    pub(crate) set_partial_suffix: Entity<TextInput>,
    pub(crate) set_keep_days: Entity<TextInput>,
    pub(crate) set_max_retries: Entity<TextInput>,
    pub(crate) set_slow_speed_kb: Entity<TextInput>,
    pub(crate) set_slow_interval_s: Entity<TextInput>,
    pub(crate) set_max_query_dl: Entity<TextInput>,
    pub(crate) set_max_small_dl: Entity<TextInput>,
    pub(crate) set_max_slow_dl: Entity<TextInput>,
    pub(crate) set_tum_dl_kb: Entity<TextInput>,
    pub(crate) set_tum_ul_kb: Entity<TextInput>,
    pub(crate) set_tum_conns: Entity<TextInput>,
    pub(crate) set_tum_per_srv: Entity<TextInput>,
    pub(crate) set_tum_concurrent: Entity<TextInput>,
    pub(crate) set_tum_ratio: Entity<TextInput>,
    pub(crate) set_proxy_host: Entity<TextInput>,
    pub(crate) set_proxy_port: Entity<TextInput>,
    pub(crate) set_socks5_host: Entity<TextInput>,
    pub(crate) set_socks5_port: Entity<TextInput>,
    pub(crate) set_proxy_user: Entity<TextInput>,
    pub(crate) set_proxy_pass: Entity<TextInput>,
    pub(crate) set_bt_port: Entity<TextInput>,
    pub(crate) set_bt_watch_dir: Entity<TextInput>,
    pub(crate) set_bt_trackers: Entity<TextInput>,
    pub(crate) set_sched_start: Entity<TextInput>,
    pub(crate) set_sched_end: Entity<TextInput>,
    pub(crate) set_battery_min: Entity<TextInput>,
    pub(crate) set_wb_small_kb: Entity<TextInput>,
    pub(crate) set_wb_skip_ext: Entity<TextInput>,
    pub(crate) set_wb_only_ext: Entity<TextInput>,
    pub(crate) set_wb_skip_hosts: Entity<TextInput>,
    pub(crate) set_ffmpeg_path: Entity<TextInput>,
    pub(crate) set_ytdlp_path: Entity<TextInput>,
    pub(crate) set_av_path: Entity<TextInput>,
    pub(crate) set_av_args: Entity<TextInput>,
    pub(crate) set_ext_path: Entity<TextInput>,
    pub(crate) set_ext_args: Entity<TextInput>,
    pub(crate) set_remote_bind: Entity<TextInput>,
    pub(crate) set_remote_port: Entity<TextInput>,
    pub(crate) set_remote_pass: Entity<TextInput>,
    pub(crate) set_notify_sound_path: Entity<TextInput>,
    pub(crate) set_low_disk_mb: Entity<TextInput>,
    pub(crate) set_low_disk_pct: Entity<TextInput>,
    pub(crate) set_backup_interval_m: Entity<TextInput>,
    pub(crate) set_batch_max_urls: Entity<TextInput>,
    pub(crate) set_data_io_path: Entity<TextInput>,
    pub(crate) set_diag_zip_path: Entity<TextInput>,
}

impl Focusable for FerriteApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FerriteApp {
    pub fn new(
        rt: Arc<tokio::runtime::Runtime>,
        engine: Arc<DownloadEngine>,
        launch: LaunchOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let init_remote_pw = engine.ensure_remote_password();
        let cfg = engine.get_config();
        let theme = ThemePalette::from_config(
            cfg.app.theme_mode,
            cfg.app.ui_style,
            cfg.app.overall_zoom_pct,
            cfg.app.fonts_zoom_pct,
        );
        window.set_rem_size(px(zoom_rem_size_px(cfg.app.overall_zoom_pct)));
        let msgs = UiMsgQueue::default();
        let init_ssl = engine.list_ssl_exceptions().unwrap_or_default();

        // Forward DownloadEngine broadcast events + raise notifications into UiMsgQueue
        let mut rx_events = engine.subscribe();
        let mut rx_raise = engine.subscribe_raise();
        let q_ev = msgs.clone();
        let engine_dirty = Arc::new(AtomicBool::new(false));
        let dirty_ev = Arc::clone(&engine_dirty);
        rt.spawn(async move {
            let mut prev_stats_active = false;
            loop {
                tokio::select! {
                    Ok(ev) = rx_events.recv() => {
                        match ev {
                            EngineEvent::FileConflict { id, path } => {
                                q_ev.push(UiAsyncMsg::FileConflict { id, path });
                                dirty_ev.store(true, Ordering::Release);
                            }
                            EngineEvent::Added { id } => {
                                q_ev.push(UiAsyncMsg::DownloadAddedNotification { id });
                                dirty_ev.store(true, Ordering::Release);
                            }
                            EngineEvent::Completed { id } => {
                                q_ev.push(UiAsyncMsg::DownloadCompletedNotification { id });
                                dirty_ev.store(true, Ordering::Release);
                            }
                            EngineEvent::Failed { id, error } => {
                                q_ev.push(UiAsyncMsg::DownloadFailedNotification { id, error });
                                dirty_ev.store(true, Ordering::Release);
                            }
                            EngineEvent::QuitRequested => {
                                q_ev.push(UiAsyncMsg::QuitRequested);
                                dirty_ev.store(true, Ordering::Release);
                            }
                            EngineEvent::QueueStats {
                                total_download_bps,
                                total_upload_bps,
                                active_count,
                                ..
                            } => {
                                let now_active = active_count > 0
                                    || total_download_bps > 0
                                    || total_upload_bps > 0;
                                if now_active || prev_stats_active {
                                    dirty_ev.store(true, Ordering::Release);
                                }
                                prev_stats_active = now_active;
                            }
                            EngineEvent::Updated { .. }
                            | EngineEvent::Removed { .. }
                            | EngineEvent::TrafficModeChanged { .. }
                            | EngineEvent::SettingsUpdated => {
                                dirty_ev.store(true, Ordering::Release);
                            }
                        }
                    }
                    Ok(()) = rx_raise.recv() => {
                        q_ev.push(UiAsyncMsg::RaiseRequested);
                        dirty_ev.store(true, Ordering::Release);
                    }
                    else => break,
                }
            }
        });

        // Event-driven GPUI UI refresh loop (skips notify when completely idle -> ~0% CPU)
        let dirty_timer = Arc::clone(&engine_dirty);
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| loop {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let keep = this
                .update(cx, |app, cx| {
                    let had_msgs = app.msgs.has_pending();
                    if had_msgs {
                        app.drain_async_messages(cx);
                    }
                    let ev_dirty = dirty_timer.swap(false, Ordering::AcqRel);
                    let has_active = app.summary.active_count > 0
                        || app
                            .all_downloads
                            .iter()
                            .any(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued);
                    let toast_expired = app
                        .toast
                        .as_ref()
                        .is_some_and(|(_, _, when)| when.elapsed() > Duration::from_secs(5));
                    let closing_done = app
                        .inspector_closing_since
                        .as_ref()
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(200));
                    if had_msgs || ev_dirty || has_active || toast_expired || closing_done {
                        if closing_done {
                            app.inspector_closing_since = None;
                        }
                        app.refresh_snapshot(cx);
                        cx.notify();
                    }
                })
                .is_ok();
            if !keep {
                break;
            }
        })
        .detach();

        let mk_input = |cx: &mut Context<Self>, ph: &str, init: &str| -> Entity<TextInput> {
            cx.new(|cx| {
                let mut t = TextInput::new(cx, ph.to_string(), init);
                t.apply_colors(
                    theme.bg_input,
                    theme.border,
                    theme.border_focus,
                    theme.text_primary,
                    theme.text_muted,
                    theme.accent,
                );
                t.apply_appearance(theme.font_scale(), theme.ui_scale(), theme.is_classic());
                t
            })
        };

        let mk_masked = |cx: &mut Context<Self>, ph: &str, init: &str| -> Entity<TextInput> {
            cx.new(|cx| {
                let mut t = TextInput::new(cx, ph.to_string(), init);
                t.masked = true;
                t.apply_colors(
                    theme.bg_input,
                    theme.border,
                    theme.border_focus,
                    theme.text_primary,
                    theme.text_muted,
                    theme.accent,
                );
                t.apply_appearance(theme.font_scale(), theme.ui_scale(), theme.is_classic());
                t
            })
        };

        let def_dir_s = cfg.core.default_download_path.to_string_lossy().into_owned();
        let init_tum = cfg.core.current_tum;
        let init_prof = cfg.active_profile();
        let loc = i18n::l(&cfg.app.locale_name);

        let initial_url = launch.initial_url.unwrap_or_default();
        let use_standalone_create = cfg.app.enable_standalone_create_downloads_windows;
        let initial_modal = if !initial_url.is_empty() && !use_standalone_create {
            ActiveModal::AddDownload
        } else {
            ActiveModal::None
        };

        let add_url_input = mk_input(cx, "https://..., magnet:?xt=..., or .torrent path", &initial_url);

        let search_input = cx.new(|cx| {
            let mut t = TextInput::new(cx, loc.search_placeholder, "");
            t.borderless = true;
            t.apply_colors(
                theme.bg_surface,
                theme.border,
                theme.border_focus,
                theme.text_primary,
                theme.text_muted,
                theme.accent,
            );
            t.apply_appearance(theme.font_scale(), theme.ui_scale(), theme.is_classic());
            t
        });

        let wb_small_val = if cfg.core.wb_small_size_unit_mb {
            cfg.app.wb_too_small_size_bytes / (1024 * 1024)
        } else {
            cfg.app.wb_too_small_size_bytes / 1024
        };
        let proxy_http_ftp_eq = ftp_mirrors_http(&cfg.app.proxy);
        let init_sound_s = cfg
            .app
            .notification_sounds
            .get(&NotificationEvent::DownloadComplete)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut app = Self {
            rt,
            engine: Arc::clone(&engine),
            msgs,
            focus_handle: cx.focus_handle(),
            theme,
            active_page: ActivePage::Downloads,
            summary: engine.summary(),
            all_downloads: Vec::new(),
            visible_downloads: Vec::new(),
            tags: engine.list_tags().unwrap_or_default(),
            selected_id: None,
            parent_folder_id: None,
            hovered_row_id: None,
            checked_ids: HashSet::new(),
            active_filter: StateFilter::All,
            active_tag_id: None,
            sort_by: cfg.app.sort_by,
            sort_ascending: cfg.app.sort_ascending,
            inspector_tab: InspectorTab::General,
            inspector_expanded: true,
            last_non_snail_tum: if init_tum == TrafficUsageMode::Snail {
                TrafficUsageMode::High
            } else {
                init_tum
            },
            active_popover: ActivePopover::None,
            shutdown_submenu_open: false,
            active_modal: initial_modal.clone(),
            delete_remember_choice: false,
            toast: None,

            modal_anim_epoch: if initial_modal == ActiveModal::None { 0 } else { 1 },
            popover_anim_epoch: 0,
            plugins_gear_anim_epoch: 0,
            inspector_anim_epoch: 0,
            inspector_closing_since: None,
            toast_anim_epoch: 0,
            page_anim_epoch: 0,
            settings_tab_anim_epoch: 0,
            inspector_tab_anim_epoch: 0,
            adv_panel_anim_epoch: 0,
            probe_anim_epoch: if !initial_url.is_empty() && !use_standalone_create {
                1
            } else {
                0
            },
            shutdown_sub_anim_epoch: 0,
            proxy_box_anim_epoch: 0,
            av_preset_anim_epoch: 0,
            last_inspected_modal: initial_modal,
            last_dock_badge: None,
            window_active: !launch.hidden,
            manual_torrent_check_pending: false,

            search_input,

            add_url_input,
            add_dir_input: mk_input(cx, "Destination directory", &def_dir_s),
            add_name_input: mk_input(cx, "Optional output filename", ""),
            add_ref_input: mk_input(cx, "HTTP Referer (optional)", ""),
            add_user_input: mk_input(cx, "Username (optional)", ""),
            add_pass_input: mk_masked(cx, "Password (optional)", ""),
            add_show_pass: false,
            add_priority: DownloadPriority::Normal,
            add_start_paused: false,
            add_play_asap: false,
            add_apply_schedule: cfg.core.enable_scheduler_for_new_downloads,
            add_show_advanced: false,
            add_probe_busy: false,
            add_probe_outcome: None,
            add_selected_files: HashSet::new(),
            add_selected_variant: 0,

            batch_pattern_input: mk_input(
                cx,
                "https://host/files/part-[01-20].zip or multiline URLs",
                "",
            ),
            batch_group_title_input: mk_input(cx, "Optional group folder name", ""),
            batch_dir_input: mk_input(cx, "Target directory", &def_dir_s),
            batch_start_paused: false,
            batch_as_group: true,

            hash_algo: HashAlgorithm::Sha256,
            hash_expected_input: mk_input(cx, "Optional expected hex digest to verify against", ""),
            hash_computed: None,
            hash_matched: None,

            convert_mode_mp3: true,
            convert_mp3_bitrate: 320,
            convert_use_vbr: false,
            convert_vbr_quality: 2,
            convert_stream_copy: true,
            convert_out_input: mk_input(cx, "Output file path", ""),
            convert_busy: false,
            convert_status: None,

            remote_pairing: None,
            remote_loading: false,
            remote_error: None,

            new_tag_name_input: mk_input(cx, "Tag name (e.g. Research ISOs)", ""),
            new_tag_color_hex: "#3b82f6".into(),
            tag_exts_input: mk_input(cx, "zip tar.gz iso", ""),
            tag_dir_input: mk_input(cx, "Optional folder path for this tag", ""),

            source_url_input: mk_input(cx, "Primary download URL", ""),
            source_mirrors_input: mk_input(cx, "Comma-separated mirror URLs", ""),

            move_dir_input: mk_input(cx, "New folder path", &def_dir_s),
            rename_file_input: mk_input(cx, "New file name", ""),
            portable_dir_input: mk_input(
                cx,
                "Target portable directory",
                &engine
                    .data_dir()
                    .join("ferrite-portable")
                    .to_string_lossy(),
            ),

            sched_enabled: cfg.core.default_schedule.enabled,
            sched_days_mask: cfg.core.default_schedule.days_mask,
            sched_start_input: mk_input(cx, "HH:MM", &format_minute_hhmm(cfg.core.default_schedule.start_minute)),
            sched_end_input: mk_input(cx, "HH:MM", &format_minute_hhmm(cfg.core.default_schedule.end_minute)),

            bt_add_tracker_input: mk_input(cx, "udp://tracker.example.org:6969/announce", ""),

            plugins_title_clicks: 0,
            show_plugin_dev: false,
            plugins_gear_menu_open: false,
            plugin_install_input: mk_input(cx, "/path/to/plugin.js or plugin manifest folder", ""),

            settings_plugins: engine.list_js_plugins(),
            settings_ssl_exceptions: init_ssl,
            prefs_title_clicks: 0,
            show_troubleshooting: false,
            show_proxy_pass: false,
            show_remote_pass: false,
            proxy_use_http_for_ftp: proxy_http_ftp_eq,

            set_default_dir: mk_input(cx, "Default folder", &def_dir_s),
            set_partial_suffix: mk_input(cx, ".fdmdownload", &cfg.core.partial_download_suffix),
            set_keep_days: mk_input(cx, "Days", &cfg.core.auto_remove_finished_keep_days.to_string()),
            set_max_retries: mk_input(cx, "Retries", &cfg.core.max_auto_retries.to_string()),
            set_slow_speed_kb: mk_input(
                cx,
                "KB/s (0 = off)",
                &format_kb_limit(cfg.core.unacceptably_slow_speed_bps),
            ),
            set_slow_interval_s: mk_input(
                cx,
                "60",
                &cfg.core.unacceptably_slow_interval_secs.to_string(),
            ),
            set_max_query_dl: mk_input(
                cx,
                "2",
                &cfg.core.max_additional_query_info_downloads.to_string(),
            ),
            set_max_small_dl: mk_input(
                cx,
                "2",
                &cfg.core.max_additional_small_downloads.to_string(),
            ),
            set_max_slow_dl: mk_input(
                cx,
                "2",
                &cfg.core.max_additional_downloads_if_slow.to_string(),
            ),
            set_tum_dl_kb: mk_input(cx, "0 = unlimited", &format_kb_limit(init_prof.max_download_speed_bps)),
            set_tum_ul_kb: mk_input(cx, "0 = unlimited", &format_kb_limit(init_prof.max_upload_speed_bps)),
            set_tum_conns: mk_input(cx, "Connections", &init_prof.max_connections.to_string()),
            set_tum_per_srv: mk_input(cx, "Per server", &init_prof.max_connections_per_server.to_string()),
            set_tum_concurrent: mk_input(cx, "Concurrent", &init_prof.max_concurrent_downloads.to_string()),
            set_tum_ratio: mk_input(cx, "0.0 = unlimited", &format!("{:.1}", init_prof.max_upload_ratio)),
            set_proxy_host: mk_input(cx, "127.0.0.1", &cfg.app.proxy.http.host),
            set_proxy_port: mk_input(cx, "8080", &cfg.app.proxy.http.port.to_string()),
            set_socks5_host: mk_input(cx, "127.0.0.1", &cfg.app.proxy.socks5.host),
            set_socks5_port: mk_input(cx, "1080", &cfg.app.proxy.socks5.port.to_string()),
            set_proxy_user: mk_input(cx, "Username", &cfg.app.proxy.http.username),
            set_proxy_pass: mk_masked(cx, "Password", &cfg.app.proxy.http.password),
            set_bt_port: mk_input(cx, "6881", &cfg.core.bt_session_port.to_string()),
            set_bt_watch_dir: mk_input(
                cx,
                "Optional watch folder for .torrent files",
                &cfg.app
                    .torrents_watch_folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            set_bt_trackers: mk_input(
                cx,
                "Comma-separated default tracker URLs",
                &cfg.core.bt_tracker_list.join(", "),
            ),
            set_sched_start: mk_input(cx, "00:00", &format_minute_hhmm(cfg.core.default_schedule.start_minute)),
            set_sched_end: mk_input(cx, "06:00", &format_minute_hhmm(cfg.core.default_schedule.end_minute)),
            set_battery_min: mk_input(cx, "15", &cfg.core.battery_min_level_percent.to_string()),
            set_wb_small_kb: mk_input(cx, "100", &wb_small_val.to_string()),
            set_wb_skip_ext: mk_input(cx, "pls m3u", &cfg.app.wb_unwanted_extensions.join(" ")),
            set_wb_only_ext: mk_input(cx, "zip iso mp4", &cfg.app.wb_intercept_extensions.join(" ")),
            set_wb_skip_hosts: mk_input(cx, "mega.nz rapidgator.net", &cfg.app.wb_unwanted_hosts.join(", ")),
            set_ffmpeg_path: mk_input(
                cx,
                "/opt/homebrew/bin/ffmpeg",
                &cfg.core
                    .ffmpeg_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            set_ytdlp_path: mk_input(
                cx,
                "Optional yt-dlp path",
                &cfg.core
                    .yt_dlp_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            set_av_path: mk_input(
                cx,
                "Antivirus binary path",
                &cfg.core
                    .antivirus_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            set_av_args: mk_input(cx, "\"{file}\"", &cfg.core.antivirus_args),
            set_ext_path: mk_input(
                cx,
                "Post-completion binary/script path",
                &cfg.core
                    .external_app_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            set_ext_args: mk_input(cx, "\"{file}\"", &cfg.core.external_app_args_template),
            set_remote_bind: mk_input(cx, "0.0.0.0", &cfg.app.remote_access_bind),
            set_remote_port: mk_input(cx, "9443", &cfg.app.remote_access_port.to_string()),
            set_remote_pass: mk_masked(cx, "Remote access password", &init_remote_pw),
            set_notify_sound_path: mk_input(cx, "/System/Library/Sounds/Glass.aiff", &init_sound_s),
            set_low_disk_mb: mk_input(
                cx,
                "256",
                &(cfg.core.low_disk_space_min_bytes / (1024 * 1024)).to_string(),
            ),
            set_low_disk_pct: mk_input(
                cx,
                "2.0",
                &format!("{:.1}", cfg.core.low_disk_space_min_percent),
            ),
            set_backup_interval_m: mk_input(
                cx,
                "60",
                &(cfg.app.db_backup_interval_secs.max(0) / 60).to_string(),
            ),
            set_batch_max_urls: mk_input(
                cx,
                "1000",
                &cfg.app.batch_download_max_urls_count.to_string(),
            ),
            set_data_io_path: mk_input(
                cx,
                "Path to .json / legacy FDM db.sqlite",
                &engine
                    .data_dir()
                    .join("ferrite-downloads-export.json")
                    .to_string_lossy(),
            ),
            set_diag_zip_path: mk_input(
                cx,
                "Path for diagnostics .zip",
                &engine
                    .data_dir()
                    .join("ferrite-diagnostics.zip")
                    .to_string_lossy(),
            ),
            settings_cfg: cfg,
            settings_tum_tab: init_tum,
        };

        let weak_close = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            weak_close
                .update(cx, |view, cx| view.confirm_or_hide_on_window_close(cx))
                .unwrap_or(true)
        });

        cx.observe(&app.search_input, |this, _, cx| {
            this.refresh_snapshot(cx);
            cx.notify();
        })
        .detach();
        for watched_input in [
            &app.set_default_dir,
            &app.add_url_input,
            &app.add_dir_input,
            &app.add_name_input,
            &app.batch_pattern_input,
        ] {
            cx.observe(watched_input, |_, _, cx| cx.notify()).detach();
        }

        set_macos_hide_dock_icon(app.settings_cfg.app.hide_dock_icon);
        if app.settings_cfg.app.launch_minimized && !launch.hidden {
            window.minimize_window();
        }
        if app.settings_cfg.app.check_if_default_torrent_client {
            let q = app.msgs.clone();
            app.rt.spawn(async move {
                let is_default = is_default_torrent_client().await;
                q.push(UiAsyncMsg::DefaultTorrentCheckResult { is_default });
            });
        }

        app.refresh_snapshot(cx);
        if app.settings_cfg.app.auto_check_updates {
            app.check_for_app_updates_action(false, cx);
        }
        if app.settings_cfg.app.update_plugins_automatically {
            app.check_plugin_updates_action(false, cx);
        }
        if !initial_url.is_empty() && use_standalone_create {
            let rt_sa = Arc::clone(&app.rt);
            let eng_sa = Arc::clone(&app.engine);
            let url_sa = initial_url;
            cx.spawn(async move |_, cx: &mut gpui::AsyncApp| {
                let _ = cx.update(|cx| open_standalone_add_window(rt_sa, eng_sa, url_sa, cx));
            })
            .detach();
            window.focus(&app.focus_handle);
        } else if !initial_url.is_empty() {
            let fh = app.add_url_input.read(cx).focus_handle.clone();
            window.focus(&fh);
            app.trigger_probe(cx);
        } else {
            window.focus(&app.focus_handle);
        }
        app
    }

    pub(crate) fn refresh_snapshot(&mut self, cx: &App) {
        self.summary = self.engine.summary();
        self.all_downloads = self.engine.list_downloads(&FilterQuery::default());
        self.tags = self.engine.list_tags().unwrap_or_default();
        if let Some(pid) = self.parent_folder_id {
            if !self.all_downloads.iter().any(|d| d.id == pid) {
                self.parent_folder_id = None;
            }
        }
        let query = FilterQuery {
            state_filter: self.active_filter,
            tag_id: self.active_tag_id,
            missing_filter: Default::default(),
            search_text: self.search_input.read(cx).text(),
            sort_by: self.sort_by,
            sort_ascending: self.sort_ascending,
            only_top_level: self.parent_folder_id.is_none(),
            parent_id_filter: self.parent_folder_id,
        };
        self.visible_downloads = self.engine.list_downloads(&query);
        self.checked_ids
            .retain(|id| self.all_downloads.iter().any(|d| d.id == *id));
        if let Some(sel) = self.selected_id {
            if !self.all_downloads.iter().any(|d| d.id == sel) {
                self.selected_id = self.visible_downloads.first().map(|d| d.id);
            }
        } else {
            self.selected_id = self.visible_downloads.first().map(|d| d.id);
        }

        if let Some((_, _, when)) = self.toast {
            if when.elapsed() > Duration::from_secs(5) {
                self.toast = None;
            }
        }

        self.refresh_dock_badge();
    }

    pub(crate) fn refresh_dock_badge(&mut self) {
        let cfg = &self.settings_cfg.app;
        let active_count = self
            .all_downloads
            .iter()
            .filter(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued)
            .count();
        let next = if cfg.hide_dock_icon {
            None
        } else {
            format_compact_dock_badge(
                self.summary.total_download_bps,
                self.summary.total_upload_bps,
                active_count,
                cfg.show_total_upload_speed_in_dock,
                cfg.indicate_dl_ul_in_tray,
            )
        };
        if next != self.last_dock_badge {
            update_macos_dock_badge(next.as_deref());
            self.last_dock_badge = next;
        }
    }

    fn drain_async_messages(&mut self, cx: &mut Context<Self>) {
        for msg in self.msgs.drain() {
            match msg {
                UiAsyncMsg::Toast { text, is_error } => {
                    self.set_toast(text, is_error);
                }
                UiAsyncMsg::ProbeCompleted { outcome } => {
                    self.add_probe_busy = false;
                    self.add_dir_input.update(cx, |inp, cx| {
                        inp.set_text(&outcome.resolved_dir.to_string_lossy(), cx);
                    });
                    self.add_name_input.update(cx, |inp, cx| {
                        inp.set_text(&outcome.resolved_file_name, cx);
                    });
                    self.add_selected_files = outcome
                        .files
                        .iter()
                        .filter(|f| f.priority != DownloadPriority::DontDownload)
                        .map(|f| f.index)
                        .collect();
                    self.add_selected_variant = 0;
                    self.add_probe_outcome = Some(*outcome);
                    self.probe_anim_epoch = self.probe_anim_epoch.wrapping_add(1);
                }
                UiAsyncMsg::ProbeFailed { error } => {
                    self.add_probe_busy = false;
                    self.set_toast(format!("Probe failed: {error}"), true);
                }
                UiAsyncMsg::EnqueueFinished { count, label } => {
                    if matches!(self.active_modal, ActiveModal::AddDownload | ActiveModal::BatchAdd) {
                        self.active_modal = ActiveModal::None;
                    }
                    self.set_toast(format!("Enqueued {count} download(s): {label}"), false);
                }
                UiAsyncMsg::FileConflict { id, path } => {
                    self.active_modal = ActiveModal::FileConflict { id, path };
                    self.sync_modal_anim_epoch();
                }
                UiAsyncMsg::HashResult {
                    download_id,
                    algo,
                    digest,
                    matched,
                } => {
                    if matches!(self.active_modal, ActiveModal::HashVerify { id } if id == download_id) {
                        self.hash_computed = Some(digest);
                        self.hash_matched = matched;
                    }
                    self.set_toast(format!("{} digest computed", algo.label()), false);
                }
                UiAsyncMsg::ConvertFinished { out_path } => {
                    self.convert_busy = false;
                    self.convert_status = Some(format!("Saved: {}", out_path.display()));
                    self.set_toast(format!("Converted -> {}", out_path.display()), false);
                }
                UiAsyncMsg::RemoteControlReady { pairing } => {
                    self.remote_loading = false;
                    self.remote_error = None;
                    self.remote_pairing = Some(pairing);
                }
                UiAsyncMsg::RemoteControlFailed { error } => {
                    self.remote_loading = false;
                    self.remote_error = Some(error.clone());
                    self.set_toast(format!("Remote server error: {error}"), true);
                }
                UiAsyncMsg::RemotePasswordRegenerated { password } => {
                    self.settings_cfg.app.remote_access_password = password.clone();
                    self.set_remote_pass.update(cx, |i, cx| i.set_text(&password, cx));
                    self.set_toast("Regenerated Remote Access password", false);
                }
                UiAsyncMsg::PortableCreated { dir } => {
                    if self.active_modal == ActiveModal::CreatePortable {
                        self.active_modal = ActiveModal::None;
                    }
                    self.set_toast(format!("Portable version created at {}", dir.display()), false);
                }
                UiAsyncMsg::DiagnosticsArchived { zip_path } => {
                    self.set_toast(
                        format!("Saved diagnostics archive to {}", zip_path.display()),
                        false,
                    );
                }
                UiAsyncMsg::PluginActionFinished { message, is_error } => {
                    self.settings_plugins = self.engine.list_js_plugins();
                    self.set_toast(message, is_error);
                }
                UiAsyncMsg::PluginUpdatesFinished { updated, manual } => {
                    self.settings_plugins = self.engine.list_js_plugins();
                    if !updated.is_empty() {
                        let summary: Vec<String> = updated
                            .iter()
                            .map(|u| format!("{} v{}", u.name, u.new_version))
                            .collect();
                        self.set_toast(
                            format!("Updated {} add-on(s): {}", updated.len(), summary.join(", ")),
                            false,
                        );
                    } else if manual {
                        self.set_toast("All installed add-ons are up to date", false);
                    }
                }
                UiAsyncMsg::AppUpdateCheckFinished { result, manual } => {
                    if result.update_available || manual {
                        self.set_toast(result.message, false);
                    }
                }
                UiAsyncMsg::VirusCheckFinished {
                    download_id,
                    clean,
                    summary,
                } => {
                    self.set_toast(format!("#{download_id}: {summary}"), !clean);
                }
                UiAsyncMsg::UpdateCheckFinished {
                    download_id,
                    changed,
                    message,
                } => {
                    let state = if changed { "update available" } else { "up to date" };
                    self.set_toast(format!("#{download_id} ({state}): {message}"), false);
                }
                UiAsyncMsg::DownloadAddedNotification { id } => {
                    let cfg = self.engine.get_config();
                    let allow_notify = !cfg.app.notify_when_inactive_only || !self.window_active;
                    if cfg.app.notify_added && allow_notify {
                        let title = self
                            .engine
                            .get_download(id)
                            .map(|r| r.title)
                            .unwrap_or_else(|| format!("Download #{id}"));
                        send_os_notification("Download Added", &title);
                    }
                }
                UiAsyncMsg::DownloadCompletedNotification { id } => {
                    let title = self
                        .engine
                        .get_download(id)
                        .map(|r| r.title)
                        .unwrap_or_else(|| format!("Download #{id}"));
                    self.set_toast(format!("Download #{id} completed: {title}"), false);
                    let cfg = self.engine.get_config();
                    let allow_notify = !cfg.app.notify_when_inactive_only || !self.window_active;
                    if cfg.app.notify_finished && allow_notify {
                        send_os_notification("Download Completed", &title);
                    }
                    if cfg.app.enable_sound_notifications {
                        let custom = cfg
                            .app
                            .notification_sounds
                            .get(&NotificationEvent::DownloadComplete)
                            .map(|p| p.as_path());
                        play_notification_sound(custom);
                    }
                }
                UiAsyncMsg::DownloadFailedNotification { id, error } => {
                    let title = self
                        .engine
                        .get_download(id)
                        .map(|r| r.title)
                        .unwrap_or_else(|| format!("Download #{id}"));
                    self.set_toast(format!("Download #{id} failed: {error}"), true);
                    let cfg = self.engine.get_config();
                    let allow_notify = !cfg.app.notify_when_inactive_only || !self.window_active;
                    if cfg.app.notify_failed && allow_notify {
                        send_os_notification("Download Failed", &format!("{title}: {error}"));
                    }
                }
                UiAsyncMsg::DefaultTorrentCheckResult { is_default } => {
                    let manual = std::mem::take(&mut self.manual_torrent_check_pending);
                    if manual {
                        if is_default {
                            self.set_toast("Ferrite DM is the default BitTorrent client", false);
                        } else {
                            self.set_toast(
                                "Ferrite DM is not the default .torrent / magnet handler in macOS",
                                true,
                            );
                        }
                    } else if !is_default && self.settings_cfg.app.check_if_default_torrent_client {
                        self.set_toast(
                            "Ferrite DM is not set as the default BitTorrent client",
                            false,
                        );
                    }
                }
                UiAsyncMsg::QuitRequested => {
                    cx.quit();
                }
                UiAsyncMsg::RaiseRequested => {
                    macos_restore_windows_and_activate();
                    cx.activate(true);
                }
            }
        }
    }

    pub(crate) fn set_toast(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some((text.into(), is_error, Instant::now()));
        self.toast_anim_epoch = self.toast_anim_epoch.wrapping_add(1);
    }

    pub(crate) fn sync_modal_anim_epoch(&mut self) {
        if self.active_modal != self.last_inspected_modal {
            self.last_inspected_modal = self.active_modal.clone();
            if self.active_modal != ActiveModal::None {
                self.modal_anim_epoch = self.modal_anim_epoch.wrapping_add(1);
            }
        }
    }

    pub(crate) fn set_inspector_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if !self.inspector_expanded && expanded {
            self.inspector_closing_since = None;
            self.inspector_anim_epoch = self.inspector_anim_epoch.wrapping_add(1);
        } else if self.inspector_expanded && !expanded {
            self.inspector_closing_since = Some(Instant::now());
            self.inspector_anim_epoch = self.inspector_anim_epoch.wrapping_add(1);
        }
        self.inspector_expanded = expanded;
        cx.notify();
    }

    pub(crate) fn set_active_page(&mut self, page: ActivePage, cx: &mut Context<Self>) {
        let same_discriminant =
            std::mem::discriminant(&self.active_page) == std::mem::discriminant(&page);
        if self.active_page != page {
            if !same_discriminant {
                self.page_anim_epoch = self.page_anim_epoch.wrapping_add(1);
            }
            if let (ActivePage::Settings(old_tab), ActivePage::Settings(new_tab)) =
                (self.active_page, page)
            {
                if old_tab != new_tab {
                    self.settings_tab_anim_epoch = self.settings_tab_anim_epoch.wrapping_add(1);
                }
            }
            self.active_page = page;
            cx.notify();
        }
    }

    pub(crate) fn set_settings_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.set_active_page(ActivePage::Settings(tab), cx);
    }

    pub(crate) fn set_inspector_tab(&mut self, tab: InspectorTab, cx: &mut Context<Self>) {
        if self.inspector_tab != tab {
            self.inspector_tab = tab;
            self.inspector_tab_anim_epoch = self.inspector_tab_anim_epoch.wrapping_add(1);
        }
        self.set_inspector_expanded(true, cx);
    }

    pub(crate) fn check_for_app_updates_action(&mut self, manual: bool, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        if manual {
            self.set_toast("Checking for Ferrite DM updates…", false);
        }
        self.rt.spawn(async move {
            match eng.check_for_app_updates(None).await {
                Ok(result) => q.push(UiAsyncMsg::AppUpdateCheckFinished { result, manual }),
                Err(e) => {
                    if manual {
                        q.push(UiAsyncMsg::Toast {
                            text: format!("App update check failed: {e}"),
                            is_error: true,
                        });
                    }
                }
            }
        });
        cx.notify();
    }

    pub(crate) fn check_plugin_updates_action(&mut self, manual: bool, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        if manual {
            self.set_toast("Checking for add-on updates…", false);
        }
        self.rt.spawn(async move {
            match eng.check_and_update_js_plugins().await {
                Ok(updated) => q.push(UiAsyncMsg::PluginUpdatesFinished { updated, manual }),
                Err(e) => {
                    if manual {
                        q.push(UiAsyncMsg::Toast {
                            text: format!("Add-on update check failed: {e}"),
                            is_error: true,
                        });
                    }
                }
            }
        });
        cx.notify();
    }

    pub(crate) fn apply_theme_and_appearance_to_inputs(&self, cx: &mut Context<Self>) {
        let p = self.theme;
        let loc = i18n::l(&self.settings_cfg.app.locale_name);
        self.search_input.update(cx, |inp, cx| {
            inp.placeholder = loc.search_placeholder.into();
            inp.apply_colors(
                p.bg_surface,
                p.border,
                p.border_focus,
                p.text_primary,
                p.text_muted,
                p.accent,
            );
            inp.apply_appearance(p.font_scale(), p.ui_scale(), p.is_classic());
            cx.notify();
        });
        let all_inputs = [
            &self.add_url_input,
            &self.add_dir_input,
            &self.add_name_input,
            &self.add_ref_input,
            &self.add_user_input,
            &self.add_pass_input,
            &self.batch_pattern_input,
            &self.batch_group_title_input,
            &self.batch_dir_input,
            &self.hash_expected_input,
            &self.convert_out_input,
            &self.new_tag_name_input,
            &self.tag_exts_input,
            &self.tag_dir_input,
            &self.source_url_input,
            &self.source_mirrors_input,
            &self.move_dir_input,
            &self.rename_file_input,
            &self.portable_dir_input,
            &self.sched_start_input,
            &self.sched_end_input,
            &self.bt_add_tracker_input,
            &self.plugin_install_input,
            &self.set_default_dir,
            &self.set_partial_suffix,
            &self.set_keep_days,
            &self.set_max_retries,
            &self.set_slow_speed_kb,
            &self.set_slow_interval_s,
            &self.set_max_query_dl,
            &self.set_max_small_dl,
            &self.set_max_slow_dl,
            &self.set_tum_dl_kb,
            &self.set_tum_ul_kb,
            &self.set_tum_conns,
            &self.set_tum_per_srv,
            &self.set_tum_concurrent,
            &self.set_tum_ratio,
            &self.set_proxy_host,
            &self.set_proxy_port,
            &self.set_socks5_host,
            &self.set_socks5_port,
            &self.set_proxy_user,
            &self.set_proxy_pass,
            &self.set_bt_port,
            &self.set_bt_watch_dir,
            &self.set_bt_trackers,
            &self.set_sched_start,
            &self.set_sched_end,
            &self.set_battery_min,
            &self.set_wb_small_kb,
            &self.set_wb_skip_ext,
            &self.set_wb_only_ext,
            &self.set_wb_skip_hosts,
            &self.set_ffmpeg_path,
            &self.set_ytdlp_path,
            &self.set_av_path,
            &self.set_av_args,
            &self.set_ext_path,
            &self.set_ext_args,
            &self.set_remote_bind,
            &self.set_remote_port,
            &self.set_remote_pass,
            &self.set_notify_sound_path,
            &self.set_low_disk_mb,
            &self.set_low_disk_pct,
            &self.set_backup_interval_m,
            &self.set_batch_max_urls,
            &self.set_data_io_path,
            &self.set_diag_zip_path,
        ];
        for e in all_inputs {
            e.update(cx, |inp, cx| {
                inp.apply_colors(
                    p.bg_input,
                    p.border,
                    p.border_focus,
                    p.text_primary,
                    p.text_muted,
                    p.accent,
                );
                inp.apply_appearance(p.font_scale(), p.ui_scale(), p.is_classic());
                cx.notify();
            });
        }
    }

    pub(crate) fn preview_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme = ThemePalette::from_config(
            self.settings_cfg.app.theme_mode,
            self.settings_cfg.app.ui_style,
            self.settings_cfg.app.overall_zoom_pct,
            self.settings_cfg.app.fonts_zoom_pct,
        );
        window.set_rem_size(px(zoom_rem_size_px(self.settings_cfg.app.overall_zoom_pct)));
        self.apply_theme_and_appearance_to_inputs(cx);
        cx.notify();
    }

    pub(crate) fn set_theme_mode(
        &mut self,
        mode: ThemeMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_cfg.app.theme_mode = mode;
        self.preview_appearance(window, cx);
    }

    pub(crate) fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let next_mode = if self.theme.is_dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        self.settings_cfg.app.theme_mode = next_mode;
        self.theme = ThemePalette::from_config(
            next_mode,
            self.settings_cfg.app.ui_style,
            self.settings_cfg.app.overall_zoom_pct,
            self.settings_cfg.app.fonts_zoom_pct,
        );
        let mut live = self.engine.get_config();
        live.app.theme_mode = next_mode;
        let _ = self.engine.update_config(live);
        self.apply_theme_and_appearance_to_inputs(cx);
        cx.notify();
    }

    pub(crate) fn adjust_zoom(
        &mut self,
        delta: Option<i16>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = match delta {
            None => 100,
            Some(d) => (self.settings_cfg.app.overall_zoom_pct as i16 + d).clamp(75, 150) as u16,
        };
        self.settings_cfg.app.overall_zoom_pct = next;
        if !matches!(self.active_page, ActivePage::Settings(_)) {
            let mut live = self.engine.get_config();
            live.app.overall_zoom_pct = next;
            let _ = self.engine.update_config(live);
        }
        self.preview_appearance(window, cx);
    }

    pub(crate) fn check_default_torrent_action(&mut self, cx: &mut Context<Self>) {
        self.manual_torrent_check_pending = true;
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            let is_default = is_default_torrent_client().await;
            q.push(UiAsyncMsg::DefaultTorrentCheckResult { is_default });
        });
        cx.notify();
    }

    pub(crate) fn browse_folder_into(
        &mut self,
        input: Entity<TextInput>,
        prompt: &'static str,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(chosen) = paths.into_iter().next() {
                    let s = chosen.to_string_lossy().into_owned();
                    let _ = this.update(cx, |_, cx| {
                        input.update(cx, |inp, cx| inp.set_text(&s, cx));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn browse_file_into(
        &mut self,
        input: Entity<TextInput>,
        prompt: &'static str,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(chosen) = paths.into_iter().next() {
                    let s = chosen.to_string_lossy().into_owned();
                    let _ = this.update(cx, |_, cx| {
                        input.update(cx, |inp, cx| inp.set_text(&s, cx));
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn persist_sort(&mut self, cx: &mut Context<Self>) {
        self.settings_cfg.app.sort_by = self.sort_by;
        self.settings_cfg.app.sort_ascending = self.sort_ascending;
        let mut live = self.engine.get_config();
        live.app.sort_by = self.sort_by;
        live.app.sort_ascending = self.sort_ascending;
        let _ = self.engine.update_config(live);
        cx.notify();
    }

    pub(crate) fn open_download_window(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        open_standalone_download_window(Arc::clone(&self.rt), Arc::clone(&self.engine), id, cx);
    }

    pub(crate) fn open_about_modal(&mut self, cx: &mut Context<Self>) {
        self.active_popover = ActivePopover::None;
        self.active_modal = ActiveModal::About;
        self.sync_modal_anim_epoch();
        cx.notify();
    }

    pub(crate) fn open_portable_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.active_popover = ActivePopover::None;
        self.active_modal = ActiveModal::CreatePortable;
        self.sync_modal_anim_epoch();
        let fh = self.portable_dir_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn apply_state_filter_shortcut(
        &mut self,
        filter: StateFilter,
        cx: &mut Context<Self>,
    ) {
        self.set_active_page(ActivePage::Downloads, cx);
        self.active_tag_id = None;
        self.active_filter = filter;
        self.refresh_snapshot(cx);
        cx.notify();
    }

    pub(crate) fn handle_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_page == ActivePage::Plugins && self.plugins_gear_menu_open {
            self.plugins_gear_menu_open = false;
            cx.notify();
            return;
        }
        if self.active_popover != ActivePopover::None {
            self.close_popover(cx);
            return;
        }
        if self.active_modal != ActiveModal::None {
            self.close_modal(window, cx);
            return;
        }
        if matches!(self.active_page, ActivePage::Settings(_)) {
            let dirty = self.has_unsaved_settings_changes(cx);
            if dirty {
                self.set_toast("Discarded unsaved settings changes", false);
            }
            self.cancel_settings_internal(false, window, cx);
            return;
        }
        if self.active_page == ActivePage::Plugins {
            self.set_active_page(ActivePage::Downloads, cx);
            window.focus(&self.focus_handle);
            cx.notify();
            return;
        }
        if self.parent_folder_id.is_some() {
            self.parent_folder_id = None;
            self.refresh_snapshot(cx);
            window.focus(&self.focus_handle);
            cx.notify();
            return;
        }
        if !self.search_input.read(cx).text().is_empty() {
            self.search_input.update(cx, |i, cx| i.set_text("", cx));
            self.refresh_snapshot(cx);
            window.focus(&self.focus_handle);
            cx.notify();
            return;
        }
        if !self.checked_ids.is_empty() {
            self.checked_ids.clear();
            cx.notify();
            return;
        }
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn confirm_or_hide_on_window_close(&mut self, cx: &mut Context<Self>) -> bool {
        let cfg = self.engine.get_config();
        if cfg.app.close_to_background {
            self.active_popover = ActivePopover::None;
            macos_hide_all_windows();
            return false;
        }
        let has_active = self
            .all_downloads
            .iter()
            .any(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued);
        if cfg.app.quit_confirmation && has_active {
            self.active_popover = ActivePopover::None;
            self.active_modal = ActiveModal::QuitConfirm;
            self.sync_modal_anim_epoch();
            cx.notify();
            return false;
        }
        true
    }

    pub(crate) fn handle_close_window_request(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.confirm_or_hide_on_window_close(cx) {
            window.remove_window();
        }
    }

    pub(crate) fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_popover != ActivePopover::None {
            self.active_popover = ActivePopover::None;
            self.shutdown_submenu_open = false;
            cx.notify();
            return;
        }
        self.active_modal = ActiveModal::None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(crate) fn close_popover(&mut self, cx: &mut Context<Self>) {
        if self.active_popover != ActivePopover::None {
            self.active_popover = ActivePopover::None;
            self.shutdown_submenu_open = false;
            cx.notify();
        }
    }

    pub(crate) fn toggle_popover(&mut self, pop: ActivePopover, cx: &mut Context<Self>) {
        self.shutdown_submenu_open = false;
        if self.active_popover == pop {
            self.active_popover = ActivePopover::None;
        } else {
            self.active_popover = pop;
            self.popover_anim_epoch = self.popover_anim_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    pub(crate) fn toggle_check(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        if !self.checked_ids.remove(&id) {
            self.checked_ids.insert(id);
        }
        cx.notify();
    }

    pub(crate) fn toggle_select_all_visible(&mut self, cx: &mut Context<Self>) {
        let vis: Vec<DownloadId> = self.visible_downloads.iter().map(|d| d.id).collect();
        if vis.is_empty() {
            self.checked_ids.clear();
        } else {
            let all_selected = vis.iter().all(|id| self.checked_ids.contains(id));
            if all_selected {
                for id in vis {
                    self.checked_ids.remove(&id);
                }
            } else {
                for id in vis {
                    self.checked_ids.insert(id);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn batch_start_checked(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = self.checked_ids.iter().copied().collect();
        let eng = Arc::clone(&self.engine);
        self.rt.spawn(async move {
            for id in ids {
                let _ = eng.resume_download(id).await;
            }
        });
        cx.notify();
    }

    pub(crate) fn batch_pause_checked(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<DownloadId> = self.checked_ids.iter().copied().collect();
        let eng = Arc::clone(&self.engine);
        self.rt.spawn(async move {
            for id in ids {
                let _ = eng.pause_download(id).await;
            }
        });
        cx.notify();
    }

    pub(crate) fn paste_from_clipboard_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let clip = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        self.active_popover = ActivePopover::None;
        if let Some(url) = extract_first_download_url(&clip) {
            let standalone = self.engine.get_config().app.enable_standalone_create_downloads_windows;
            self.open_add_modal(&url, window, cx);
            if !standalone {
                self.trigger_probe(cx);
            }
        } else {
            self.open_add_modal("", window, cx);
            if clip.trim().is_empty() {
                self.set_toast("Clipboard empty — enter a URL or magnet URI", false);
            }
        }
    }

    pub(crate) fn set_sort_column(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.sort_by == field {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_by = field;
            self.sort_ascending = match field {
                SortField::CreationTime | SortField::Size | SortField::Progress => false,
                _ => true,
            };
        }
        self.persist_sort(cx);
        self.refresh_snapshot(cx);
        cx.notify();
    }

    pub(crate) fn open_add_modal(&mut self, prefill_url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let cfg = self.engine.get_config();
        if cfg.app.enable_standalone_create_downloads_windows {
            self.active_popover = ActivePopover::None;
            open_standalone_add_window(
                Arc::clone(&self.rt),
                Arc::clone(&self.engine),
                prefill_url.to_string(),
                cx,
            );
            cx.notify();
            return;
        }
        let def_dir = cfg.core.default_download_path.to_string_lossy().into_owned();
        self.add_probe_outcome = None;
        self.add_probe_busy = false;
        self.add_selected_files.clear();
        self.add_selected_variant = 0;
        self.add_priority = DownloadPriority::Normal;
        self.add_start_paused = false;
        self.add_play_asap = false;
        self.add_show_pass = false;
        self.add_apply_schedule = cfg.core.enable_scheduler_for_new_downloads;
        self.add_url_input.update(cx, |inp, cx| inp.set_text(prefill_url, cx));
        self.add_dir_input.update(cx, |inp, cx| inp.set_text(&def_dir, cx));
        self.add_name_input.update(cx, |inp, cx| inp.set_text("", cx));
        self.add_ref_input.update(cx, |inp, cx| inp.set_text("", cx));
        self.add_user_input.update(cx, |inp, cx| inp.set_text("", cx));
        self.add_pass_input.update(cx, |inp, cx| {
            inp.set_masked(true, cx);
            inp.set_text("", cx);
        });
        self.active_modal = ActiveModal::AddDownload;
        self.sync_modal_anim_epoch();
        let fh = self.add_url_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn open_batch_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let def_dir = self.engine.get_config().core.default_download_path.to_string_lossy().into_owned();
        self.batch_dir_input.update(cx, |inp, cx| inp.set_text(&def_dir, cx));
        self.batch_group_title_input.update(cx, |inp, cx| inp.set_text("", cx));
        self.batch_start_paused = false;
        self.active_modal = ActiveModal::BatchAdd;
        self.sync_modal_anim_epoch();
        let fh = self.batch_pattern_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn trigger_probe(&mut self, cx: &mut Context<Self>) {
        let url = self.add_url_input.read(cx).text().trim().to_string();
        if url.is_empty() {
            self.set_toast("Enter a URL, magnet URI, or .torrent file path first", true);
            return;
        }
        self.add_probe_busy = true;
        self.add_probe_outcome = None;
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.probe_source(&url).await {
                Ok(outcome) => q.push(UiAsyncMsg::ProbeCompleted {
                    outcome: Box::new(outcome),
                }),
                Err(e) => q.push(UiAsyncMsg::ProbeFailed { error: e.to_string() }),
            }
        });
        cx.notify();
    }

    pub(crate) fn submit_add_download(&mut self, cx: &mut Context<Self>) {
        let mut source_url = self.add_url_input.read(cx).text().trim().to_string();
        if source_url.is_empty() {
            self.set_toast("Download source URL cannot be empty", true);
            return;
        }

        let dir_str = self.add_dir_input.read(cx).text().trim().to_string();
        let name_str = self.add_name_input.read(cx).text().trim().to_string();
        let referer = self.add_ref_input.read(cx).text().trim().to_string();
        let user = self.add_user_input.read(cx).text().trim().to_string();
        let pass = self.add_pass_input.read(cx).text().to_string();

        if !user.is_empty() || !pass.is_empty() {
            if let Ok(parsed) = url::Url::parse(&source_url) {
                if let Some(host) = parsed.host_str() {
                    let _ = self.engine.save_credential(host, &user, &pass);
                }
            }
        }

        let mut req = NewDownloadRequest::from_url(source_url.clone());
        let mut chosen_type: Option<DownloadType> = None;

        if let Some(ref outcome) = self.add_probe_outcome {
            if outcome.download_type == DownloadType::MediaParser {
                if let Some(ref media) = outcome.media {
                    if let Some(var) = media.variants.get(self.add_selected_variant) {
                        if var.audio_url.is_none() {
                            source_url = var.url.clone();
                            req = NewDownloadRequest::from_url(source_url.clone());
                            chosen_type = if source_url.to_ascii_lowercase().contains(".m3u8") {
                                Some(DownloadType::HlsM3u8)
                            } else {
                                Some(DownloadType::Www)
                            };
                        } else {
                            chosen_type = Some(DownloadType::MediaParser);
                        }
                    }
                }
            } else {
                chosen_type = Some(outcome.download_type);
            }

            if outcome.download_type == DownloadType::BitTorrent && !outcome.files.is_empty() {
                let mut indices: Vec<usize> = self.add_selected_files.iter().copied().collect();
                indices.sort_unstable();
                req.selected_file_indices = Some(indices);
            }
        }

        if let Some(dt) = chosen_type {
            req.download_type = Some(dt);
        }
        if !dir_str.is_empty() {
            req.target_dir = Some(PathBuf::from(dir_str));
        }
        if !name_str.is_empty() {
            req.file_name = Some(name_str.clone());
            req.title = Some(name_str);
        }
        if !referer.is_empty() {
            req.http.referrer = Some(referer);
        }
        req.priority = self.add_priority;
        req.start_paused = self.add_start_paused;
        req.sequential_play_asap = self.add_play_asap;
        if self.add_apply_schedule {
            let mut s = self.engine.get_config().core.default_schedule;
            s.enabled = true;
            req.schedule = Some(s);
        }

        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.add_download(req).await {
                Ok(rec) => q.push(UiAsyncMsg::EnqueueFinished {
                    count: 1,
                    label: rec.title,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Failed to add download: {e}"),
                    is_error: true,
                }),
            }
        });
    }

    pub(crate) fn submit_batch(&mut self, cx: &mut Context<Self>) {
        let pattern = self.batch_pattern_input.read(cx).text();
        if pattern.trim().is_empty() {
            self.set_toast("Batch pattern cannot be empty", true);
            return;
        }
        let dir_s = self.batch_dir_input.read(cx).text();
        let target_dir = if dir_s.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(dir_s.trim()))
        };
        let paused = self.batch_start_paused;
        let as_group = self.batch_as_group;
        let title_raw = self.batch_group_title_input.read(cx).text();
        let group_title = {
            let t = title_raw.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        };
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            if as_group {
                match eng
                    .add_batch_group(&pattern, group_title.as_deref(), target_dir, paused)
                    .await
                {
                    Ok((parent, children)) => q.push(UiAsyncMsg::EnqueueFinished {
                        count: children.len(),
                        label: parent.title,
                    }),
                    Err(e) => q.push(UiAsyncMsg::Toast {
                        text: format!("Batch failed: {e}"),
                        is_error: true,
                    }),
                }
            } else {
                match eng.add_batch_pattern(&pattern, target_dir, paused).await {
                    Ok(list) => q.push(UiAsyncMsg::EnqueueFinished {
                        count: list.len(),
                        label: "Batch queue".into(),
                    }),
                    Err(e) => q.push(UiAsyncMsg::Toast {
                        text: format!("Batch failed: {e}"),
                        is_error: true,
                    }),
                }
            }
        });
    }

    pub(crate) fn toggle_download(&mut self, id: DownloadId) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        if rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued {
            self.rt.spawn(async move {
                if let Err(e) = eng.pause_download(id).await {
                    q.push(UiAsyncMsg::Toast {
                        text: format!("Pause failed: {e}"),
                        is_error: true,
                    });
                }
            });
        } else {
            self.rt.spawn(async move {
                if let Err(e) = eng.resume_download(id).await {
                    q.push(UiAsyncMsg::Toast {
                        text: format!("Resume failed: {e}"),
                        is_error: true,
                    });
                }
            });
        }
    }

    pub(crate) fn toggle_selected_download(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_id {
            self.toggle_download(id);
            cx.notify();
        }
    }

    pub(crate) fn cancel_download(&mut self, id: DownloadId) {
        let eng = Arc::clone(&self.engine);
        self.rt.spawn(async move {
            let _ = eng.cancel_download(id).await;
        });
    }

    pub(crate) fn remove_download(&mut self, id: DownloadId, delete_files: bool) {
        self.checked_ids.remove(&id);
        let eng = Arc::clone(&self.engine);
        self.rt.spawn(async move {
            let _ = eng.remove_download(id, delete_files).await;
        });
    }

    pub(crate) fn request_delete_downloads(
        &mut self,
        ids: Vec<DownloadId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let action = self.engine.get_config().app.delete_button_action;
        match action {
            DeleteButtonAction::RemoveOnly => {
                self.confirm_delete_downloads(ids, false, false, window, cx);
            }
            DeleteButtonAction::DeleteFiles => {
                self.confirm_delete_downloads(ids, true, false, window, cx);
            }
            DeleteButtonAction::AlwaysAsk => {
                self.delete_remember_choice = false;
                self.active_popover = ActivePopover::None;
                self.active_modal = ActiveModal::DeleteConfirm { ids };
                self.sync_modal_anim_epoch();
                cx.notify();
            }
        }
    }

    pub(crate) fn confirm_delete_downloads(
        &mut self,
        ids: Vec<DownloadId>,
        delete_files: bool,
        remember: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if remember {
            let new_policy = if delete_files {
                DeleteButtonAction::DeleteFiles
            } else {
                DeleteButtonAction::RemoveOnly
            };
            let mut live = self.engine.get_config();
            live.app.delete_button_action = new_policy;
            self.settings_cfg.app.delete_button_action = new_policy;
            let _ = self.engine.update_config(live);
        }
        for id in &ids {
            self.checked_ids.remove(id);
        }
        let count = ids.len();
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            for id in ids {
                let _ = eng.remove_download(id, delete_files).await;
            }
            q.push(UiAsyncMsg::Toast {
                text: if delete_files {
                    format!("Deleted {count} download(s) and files")
                } else {
                    format!("Removed {count} download(s) from list")
                },
                is_error: false,
            });
        });
        if matches!(self.active_modal, ActiveModal::DeleteConfirm { .. }) {
            self.active_modal = ActiveModal::None;
            window.focus(&self.focus_handle);
        }
        cx.notify();
    }

    pub(crate) fn request_quit(&mut self, cx: &mut Context<Self>) {
        let cfg = self.engine.get_config();
        let has_active = self
            .all_downloads
            .iter()
            .any(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued);
        if cfg.app.quit_confirmation && has_active {
            self.active_popover = ActivePopover::None;
            self.active_modal = ActiveModal::QuitConfirm;
            self.sync_modal_anim_epoch();
            cx.notify();
        } else {
            cx.quit();
        }
    }

    pub(crate) fn copy_to_clipboard(
        &mut self,
        text: String,
        toast_msg: &str,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        self.set_toast(toast_msg, false);
        cx.notify();
    }

    pub(crate) fn open_download_file(&mut self, id: DownloadId) {
        if let Some(rec) = self.engine.get_download(id) {
            if let Err(e) = open_path_os(&rec.output_path()) {
                self.set_toast(format!("Open failed: {e}"), true);
            }
        }
    }

    pub(crate) fn reveal_download_file(&mut self, id: DownloadId) {
        if let Some(rec) = self.engine.get_download(id) {
            if let Err(e) = reveal_path_os(&rec.output_path()) {
                self.set_toast(format!("Reveal failed: {e}"), true);
            }
        }
    }

    pub(crate) fn set_tum(&mut self, mode: TrafficUsageMode, cx: &mut Context<Self>) {
        if mode != TrafficUsageMode::Snail {
            self.last_non_snail_tum = mode;
        }
        if let Err(e) = self.engine.set_traffic_usage_mode(mode) {
            self.set_toast(format!("Failed to set speed mode: {e}"), true);
        } else {
            self.refresh_snapshot(cx);
            self.set_toast(format!("Traffic profile switched to {}", mode.label()), false);
        }
        cx.notify();
    }

    pub(crate) fn toggle_snail_mode(&mut self, cx: &mut Context<Self>) {
        if self.summary.tum == TrafficUsageMode::Snail {
            let restore = self.last_non_snail_tum;
            self.set_tum(restore, cx);
        } else {
            if self.summary.tum != TrafficUsageMode::Snail {
                self.last_non_snail_tum = self.summary.tum;
            }
            self.set_tum(TrafficUsageMode::Snail, cx);
        }
    }

    pub(crate) fn set_shutdown_when_done(&mut self, st: ShutdownType, cx: &mut Context<Self>) {
        let mut live = self.engine.get_config();
        live.core.shutdown_when_done = st;
        self.settings_cfg.core.shutdown_when_done = st;
        if let Err(e) = self.engine.update_config(live) {
            self.set_toast(format!("Failed setting auto-shutdown: {e}"), true);
        } else {
            let label = match st {
                ShutdownType::None => "Auto-shutdown disabled",
                ShutdownType::Sleep => "Auto-action: Sleep when done",
                ShutdownType::Hibernate => "Auto-action: Hibernate when done",
                ShutdownType::Shutdown => "Auto-action: Shut down computer when done",
                ShutdownType::QuitApp => "Auto-action: Quit Ferrite DM when done",
            };
            self.set_toast(label, false);
        }
        cx.notify();
    }

    pub(crate) fn bump_priority_action(&mut self, id: DownloadId, up: bool, cx: &mut Context<Self>) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let next = match (rec.priority, up) {
            (DownloadPriority::Low, true) => DownloadPriority::Normal,
            (DownloadPriority::Normal, true) => DownloadPriority::High,
            (DownloadPriority::High, false) => DownloadPriority::Normal,
            (DownloadPriority::Normal, false) => DownloadPriority::Low,
            (same, _) => same,
        };
        if next != rec.priority {
            self.set_priority_action(id, next, cx);
        }
    }

    pub(crate) fn set_priority_action(
        &mut self,
        id: DownloadId,
        prio: DownloadPriority,
        cx: &mut Context<Self>,
    ) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.set_priority(id, prio).await {
                Ok(()) => q.push(UiAsyncMsg::Toast {
                    text: format!("Set #{id} priority to {:?}", prio),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Priority error: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn restart_download_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.restart_download(id).await {
                Ok(()) => q.push(UiAsyncMsg::Toast {
                    text: format!("Restarted download #{id}"),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Restart failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn save_stream_complete_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.save_and_complete_download(id).await {
                Ok(()) => q.push(UiAsyncMsg::Toast {
                    text: format!("Saved & finalized stream #{id}"),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Save stream failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn check_virus_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.set_toast(format!("Scanning download #{id} with configured antivirus..."), false);
        self.rt.spawn(async move {
            match eng.check_download_virus(id).await {
                Ok(summary) => q.push(UiAsyncMsg::VirusCheckFinished {
                    download_id: id,
                    clean: true,
                    summary,
                }),
                Err(e) => q.push(UiAsyncMsg::VirusCheckFinished {
                    download_id: id,
                    clean: false,
                    summary: format!("Antivirus check: {e}"),
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn check_update_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.set_toast(format!("Checking server for updates on #{id}..."), false);
        self.rt.spawn(async move {
            match eng.check_download_for_update(id).await {
                Ok(changed) => q.push(UiAsyncMsg::UpdateCheckFinished {
                    download_id: id,
                    changed,
                    message: if changed {
                        format!("Remote file changed for #{id} — ready to restart")
                    } else {
                        format!("Remote file unchanged for #{id}")
                    },
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Update check failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn toggle_auto_retry_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let next_allow = rec.flags.disable_auto_retry;
        match self.engine.toggle_disable_auto_retry(id, next_allow) {
            Ok(()) => {
                self.refresh_snapshot(cx);
                self.set_toast(
                    if next_allow {
                        "Auto-retry enabled for this download"
                    } else {
                        "Auto-retry disabled for this download"
                    },
                    false,
                );
            }
            Err(e) => self.set_toast(format!("Toggle auto-retry failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn bt_reannounce_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.force_reannounce_bt(id).await {
                Ok(()) => q.push(UiAsyncMsg::Toast {
                    text: format!("Reannounced trackers for #{id}"),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Reannounce failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn bt_toggle_seeding_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let next = !rec.flags.bt_disable_seeding;
        match self.engine.toggle_bt_disable_seeding(id, next) {
            Ok(()) => {
                self.refresh_snapshot(cx);
                self.set_toast(
                    if next {
                        "Seeding disabled for this torrent"
                    } else {
                        "Seeding enabled for this torrent"
                    },
                    false,
                );
            }
            Err(e) => self.set_toast(format!("Failed toggling seeding: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn bt_toggle_ignore_ratio_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let next = !rec.flags.bt_ignore_upload_ratio;
        match self.engine.toggle_bt_ignore_ratio(id, next) {
            Ok(()) => {
                self.refresh_snapshot(cx);
                self.set_toast(
                    if next {
                        "Ignoring upload ratio limit for this torrent"
                    } else {
                        "Respecting global upload ratio limit"
                    },
                    false,
                );
            }
            Err(e) => self.set_toast(format!("Failed toggling ignore-ratio: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn add_bt_tracker_action(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let raw = self.bt_add_tracker_input.read(cx).text();
        let trackers: Vec<String> = raw
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if trackers.is_empty() {
            self.set_toast("Enter a tracker announce URL first", true);
            return;
        }
        self.bt_add_tracker_input.update(cx, |i, cx| i.set_text("", cx));
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.add_bt_trackers(id, trackers).await {
                Ok(()) => q.push(UiAsyncMsg::Toast {
                    text: "Added tracker URL(s)".into(),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Add tracker failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn open_hash_modal(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        self.hash_computed = None;
        self.hash_matched = None;
        self.hash_expected_input.update(cx, |i, cx| i.set_text("", cx));
        self.active_modal = ActiveModal::HashVerify { id };
        let fh = self.hash_expected_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn run_hash_check(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let algo = self.hash_algo;
        let expected = self.hash_expected_input.read(cx).text();
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn_blocking(move || {
            let exp_opt = if expected.trim().is_empty() {
                None
            } else {
                Some(expected.as_str())
            };
            match eng.verify_download_hash(id, algo, exp_opt) {
                Ok((digest, matched)) => q.push(UiAsyncMsg::HashResult {
                    download_id: id,
                    algo,
                    digest,
                    matched,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Hash error: {e}"),
                    is_error: true,
                }),
            }
        });
    }

    pub(crate) fn open_convert_modal(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        self.convert_mode_mp3 = true;
        self.convert_mp3_bitrate = 320;
        self.convert_use_vbr = false;
        self.convert_vbr_quality = 2;
        self.convert_stream_copy = true;
        self.convert_busy = false;
        self.convert_status = None;
        if let Some(rec) = self.engine.get_download(id) {
            let out = rec.output_path().with_extension("mp3");
            self.convert_out_input
                .update(cx, |i, cx| i.set_text(&out.to_string_lossy(), cx));
        }
        self.active_modal = ActiveModal::MediaConvert { id };
        let fh = self.convert_out_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn set_convert_target_ext(&mut self, mp3: bool, id: DownloadId, cx: &mut Context<Self>) {
        self.convert_mode_mp3 = mp3;
        if let Some(rec) = self.engine.get_download(id) {
            let ext = if mp3 { "mp3" } else { "mp4" };
            let mut out = rec.output_path().with_extension(ext);
            if out == rec.output_path() {
                let stem = rec
                    .output_path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("converted")
                    .to_string();
                out = rec.target_dir.join(format!("{stem}.converted.{ext}"));
            }
            self.convert_out_input
                .update(cx, |i, cx| i.set_text(&out.to_string_lossy(), cx));
        }
        cx.notify();
    }

    pub(crate) fn run_media_convert(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(rec) = self.engine.get_download(id) else {
            return;
        };
        let input = rec.output_path();
        let output = PathBuf::from(self.convert_out_input.read(cx).text().trim().to_string());
        let cfg = self.engine.get_config();
        let mp3 = self.convert_mode_mp3;
        let bitrate = self.convert_mp3_bitrate;
        let use_vbr = self.convert_use_vbr;
        let vbr_q = self.convert_vbr_quality;
        let copy = self.convert_stream_copy;
        let q = self.msgs.clone();
        self.convert_busy = true;
        self.convert_status = Some("Converting with FFmpeg...".into());
        self.rt.spawn(async move {
            let res = if mp3 {
                let opt = if use_vbr {
                    Mp3QualityOption::Vbr(vbr_q)
                } else {
                    Mp3QualityOption::Cbr(bitrate)
                };
                convert_to_mp3_with_options(&cfg, &input, &output, opt, |_| {}).await
            } else {
                convert_to_mp4(&cfg, &input, &output, copy, |_| {}).await
            };
            match res {
                Ok(()) => q.push(UiAsyncMsg::ConvertFinished { out_path: output }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Conversion failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn open_remote_qr_modal(&mut self, cx: &mut Context<Self>) {
        self.active_modal = ActiveModal::RemoteQr;
        self.remote_loading = true;
        self.remote_error = None;
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        let host = detect_lan_ip();
        self.rt.spawn(async move {
            match eng.ensure_remote_control(&host).await {
                Ok(pairing) => q.push(UiAsyncMsg::RemoteControlReady { pairing }),
                Err(e) => q.push(UiAsyncMsg::RemoteControlFailed {
                    error: e.to_string(),
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn regenerate_remote_password_action(&mut self, cx: &mut Context<Self>) {
        match self.engine.regenerate_remote_password() {
            Ok(pw) => {
                self.msgs.push(UiAsyncMsg::RemotePasswordRegenerated { password: pw });
            }
            Err(e) => self.set_toast(format!("Failed regenerating password: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn open_edit_tag_modal(
        &mut self,
        id: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tid) = id {
            if let Some(tag) = self.tags.iter().find(|t| t.id == tid).cloned() {
                self.new_tag_name_input
                    .update(cx, |i, cx| i.set_text(&tag.name, cx));
                self.new_tag_color_hex = tag.color_hex;
                self.tag_exts_input.update(cx, |i, cx| {
                    i.set_text(&tag.default_extensions.join(", "), cx);
                });
                self.tag_dir_input.update(cx, |i, cx| {
                    let s = tag
                        .default_download_folder
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    i.set_text(&s, cx);
                });
            }
        } else {
            self.new_tag_name_input.update(cx, |i, cx| i.set_text("", cx));
            self.new_tag_color_hex = "#3b82f6".into();
            self.tag_exts_input.update(cx, |i, cx| i.set_text("", cx));
            self.tag_dir_input.update(cx, |i, cx| i.set_text("", cx));
        }
        self.active_modal = ActiveModal::EditTag { id };
        let fh = self.new_tag_name_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        cx.notify();
    }

    pub(crate) fn submit_edit_tag(
        &mut self,
        id: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.new_tag_name_input.read(cx).text().trim().to_string();
        if name.is_empty() {
            self.set_toast("Tag name cannot be empty", true);
            return;
        }
        let res = match id {
            Some(tid) => {
                let exts = parse_list_tokens(&self.tag_exts_input.read(cx).text());
                let dir_s = self.tag_dir_input.read(cx).text().trim().to_string();
                let dir_pb = (!dir_s.is_empty()).then(|| PathBuf::from(dir_s));
                self.engine
                    .edit_tag(tid, &name, &self.new_tag_color_hex, &exts, dir_pb.as_deref())
            }
            None => self.engine.create_tag(&name, &self.new_tag_color_hex),
        };
        match res {
            Ok(_) => {
                self.refresh_snapshot(cx);
                self.set_toast(
                    if id.is_some() {
                        format!("Updated tag '{name}'")
                    } else {
                        format!("Created tag '{name}'")
                    },
                    false,
                );
                self.close_modal(window, cx);
            }
            Err(e) => self.set_toast(format!("Failed saving tag: {e}"), true),
        }
    }

    pub(crate) fn open_change_source_modal(
        &mut self,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(rec) = self.engine.get_download(id) {
            self.source_url_input.update(cx, |i, cx| i.set_text(&rec.url, cx));
            self.source_mirrors_input
                .update(cx, |i, cx| i.set_text(&rec.http.mirrors.join(", "), cx));
            self.active_modal = ActiveModal::ChangeSource { id };
            let fh = self.source_url_input.read(cx).focus_handle.clone();
            window.focus(&fh);
            cx.notify();
        }
    }

    pub(crate) fn submit_change_source(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        let new_url = self.source_url_input.read(cx).text().trim().to_string();
        let mirrors: Vec<String> = self
            .source_mirrors_input
            .read(cx)
            .text()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            if let Err(e) = eng.change_source_url(id, &new_url).await {
                q.push(UiAsyncMsg::Toast {
                    text: format!("Invalid URL: {e}"),
                    is_error: true,
                });
                return;
            }
            let _ = eng.update_mirrors(id, mirrors).await;
            q.push(UiAsyncMsg::Toast {
                text: "Updated source URL & mirrors".into(),
                is_error: false,
            });
        });
        self.close_modal(window, cx);
    }

    pub(crate) fn open_move_dir_modal(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rec) = self.engine.get_download(id) {
            self.move_dir_input
                .update(cx, |i, cx| i.set_text(&rec.target_dir.to_string_lossy(), cx));
            self.active_modal = ActiveModal::MoveDir { id };
            let fh = self.move_dir_input.read(cx).focus_handle.clone();
            window.focus(&fh);
            cx.notify();
        }
    }

    pub(crate) fn submit_move_dir(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        let dest = PathBuf::from(self.move_dir_input.read(cx).text().trim());
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.move_download_files(id, &dest).await {
                Ok(p) => q.push(UiAsyncMsg::Toast {
                    text: format!("Moved to {}", p.display()),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Move failed: {e}"),
                    is_error: true,
                }),
            }
        });
        self.close_modal(window, cx);
    }

    pub(crate) fn open_rename_modal(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rec) = self.engine.get_download(id) {
            self.rename_file_input
                .update(cx, |i, cx| i.set_text(&rec.file_name, cx));
            self.active_modal = ActiveModal::RenameDownload { id };
            let fh = self.rename_file_input.read(cx).focus_handle.clone();
            window.focus(&fh);
            cx.notify();
        }
    }

    pub(crate) fn submit_rename_download(
        &mut self,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_name = self.rename_file_input.read(cx).text().trim().to_string();
        if new_name.is_empty() {
            self.set_toast("File name cannot be empty", true);
            return;
        }
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            match eng.rename_download_file(id, &new_name).await {
                Ok(p) => q.push(UiAsyncMsg::Toast {
                    text: format!("Renamed to {}", p.display()),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Rename failed: {e}"),
                    is_error: true,
                }),
            }
        });
        self.close_modal(window, cx);
    }

    pub(crate) fn open_schedule_modal(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rec) = self.engine.get_download(id) {
            let r = rec
                .schedule
                .unwrap_or_else(|| self.engine.get_config().core.default_schedule);
            self.sched_enabled = r.enabled;
            self.sched_days_mask = r.days_mask;
            self.sched_start_input
                .update(cx, |i, cx| i.set_text(&format_minute_hhmm(r.start_minute), cx));
            self.sched_end_input
                .update(cx, |i, cx| i.set_text(&format_minute_hhmm(r.end_minute), cx));
            self.active_modal = ActiveModal::DownloadSchedule { id };
            let fh = self.sched_start_input.read(cx).focus_handle.clone();
            window.focus(&fh);
            cx.notify();
        }
    }

    pub(crate) fn submit_download_schedule(&mut self, id: DownloadId, window: &mut Window, cx: &mut Context<Self>) {
        let rule = if self.sched_enabled {
            Some(ScheduleRule {
                enabled: true,
                days_mask: self.sched_days_mask,
                start_minute: parse_minute_hhmm(&self.sched_start_input.read(cx).text(), 0),
                end_minute: parse_minute_hhmm(&self.sched_end_input.read(cx).text(), 360),
            })
        } else {
            None
        };
        if let Err(e) = self.engine.set_schedule(id, rule) {
            self.set_toast(format!("Schedule update failed: {e}"), true);
        } else {
            self.set_toast("Updated download schedule", false);
        }
        self.close_modal(window, cx);
    }

    pub(crate) fn open_plugins_page(&mut self, cx: &mut Context<Self>) {
        self.settings_cfg = self.engine.get_config();
        self.settings_plugins = self.engine.list_js_plugins();
        self.plugins_gear_menu_open = false;
        self.active_popover = ActivePopover::None;
        self.set_active_page(ActivePage::Plugins, cx);
        cx.notify();
    }

    pub(crate) fn install_plugin_from_input(&mut self, cx: &mut Context<Self>) {
        let raw = self.plugin_install_input.read(cx).text().trim().to_string();
        if raw.is_empty() {
            self.set_toast("Enter a path to a JS plugin file or folder first", true);
            return;
        }
        match self.engine.install_js_plugin_from_path(std::path::Path::new(&raw)) {
            Ok(info) => {
                self.plugin_install_input.update(cx, |i, cx| i.set_text("", cx));
                self.msgs.push(UiAsyncMsg::PluginActionFinished {
                    message: format!("Installed add-on '{}' (v{})", info.name, info.version),
                    is_error: false,
                });
            }
            Err(e) => self.set_toast(format!("Install add-on failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn toggle_plugin_enabled(&mut self, uid: String, enabled: bool, cx: &mut Context<Self>) {
        match self.engine.set_js_plugin_enabled(&uid, enabled) {
            Ok(()) => {
                self.settings_plugins = self.engine.list_js_plugins();
                self.set_toast(
                    if enabled {
                        format!("Enabled add-on {uid}")
                    } else {
                        format!("Disabled add-on {uid}")
                    },
                    false,
                );
            }
            Err(e) => self.set_toast(format!("Plugin toggle failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn uninstall_plugin_action(&mut self, uid: String, cx: &mut Context<Self>) {
        match self.engine.uninstall_js_plugin(&uid) {
            Ok(()) => {
                self.settings_plugins = self.engine.list_js_plugins();
                self.set_toast(format!("Removed add-on {uid}"), false);
            }
            Err(e) => self.set_toast(format!("Uninstall failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn open_settings_page(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        let _ = self.engine.ensure_remote_password();
        let cfg = self.engine.get_config();
        let tum = cfg.core.current_tum;
        self.proxy_use_http_for_ftp = ftp_mirrors_http(&cfg.app.proxy);
        self.settings_cfg = cfg;
        self.settings_tum_tab = tum;
        self.settings_plugins = self.engine.list_js_plugins();
        self.settings_ssl_exceptions = self.engine.list_ssl_exceptions().unwrap_or_default();
        self.show_proxy_pass = false;
        self.show_remote_pass = false;
        self.sync_settings_inputs_from_draft(cx);
        self.load_tum_inputs_from_draft(cx);
        self.active_popover = ActivePopover::None;
        self.set_settings_tab(tab, cx);
        cx.notify();
    }

    pub(crate) fn sync_settings_inputs_from_draft(&mut self, cx: &mut Context<Self>) {
        let c = &self.settings_cfg;
        let remote_pw = if c.app.remote_access_password.trim().is_empty() {
            self.engine.ensure_remote_password()
        } else {
            c.app.remote_access_password.clone()
        };
        let wb_small_val = if c.core.wb_small_size_unit_mb {
            c.app.wb_too_small_size_bytes / (1024 * 1024)
        } else {
            c.app.wb_too_small_size_bytes / 1024
        };
        let sound_s = c
            .app
            .notification_sounds
            .get(&NotificationEvent::DownloadComplete)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.set_default_dir.update(cx, |i, cx| {
            i.set_text(&c.core.default_download_path.to_string_lossy(), cx)
        });
        self.set_partial_suffix
            .update(cx, |i, cx| i.set_text(&c.core.partial_download_suffix, cx));
        self.set_keep_days.update(cx, |i, cx| {
            i.set_text(&c.core.auto_remove_finished_keep_days.to_string(), cx)
        });
        self.set_max_retries
            .update(cx, |i, cx| i.set_text(&c.core.max_auto_retries.to_string(), cx));
        self.set_slow_speed_kb.update(cx, |i, cx| {
            i.set_text(&format_kb_limit(c.core.unacceptably_slow_speed_bps), cx)
        });
        self.set_slow_interval_s.update(cx, |i, cx| {
            i.set_text(&c.core.unacceptably_slow_interval_secs.to_string(), cx)
        });
        self.set_max_query_dl.update(cx, |i, cx| {
            i.set_text(&c.core.max_additional_query_info_downloads.to_string(), cx)
        });
        self.set_max_small_dl.update(cx, |i, cx| {
            i.set_text(&c.core.max_additional_small_downloads.to_string(), cx)
        });
        self.set_max_slow_dl.update(cx, |i, cx| {
            i.set_text(&c.core.max_additional_downloads_if_slow.to_string(), cx)
        });
        self.set_proxy_host
            .update(cx, |i, cx| i.set_text(&c.app.proxy.http.host, cx));
        self.set_proxy_port
            .update(cx, |i, cx| i.set_text(&c.app.proxy.http.port.to_string(), cx));
        self.set_socks5_host
            .update(cx, |i, cx| i.set_text(&c.app.proxy.socks5.host, cx));
        self.set_socks5_port
            .update(cx, |i, cx| i.set_text(&c.app.proxy.socks5.port.to_string(), cx));
        self.set_proxy_user
            .update(cx, |i, cx| i.set_text(&c.app.proxy.http.username, cx));
        self.set_proxy_pass.update(cx, |i, cx| {
            i.set_masked(true, cx);
            i.set_text(&c.app.proxy.http.password, cx);
        });
        self.set_bt_port
            .update(cx, |i, cx| i.set_text(&c.core.bt_session_port.to_string(), cx));
        self.set_bt_watch_dir.update(cx, |i, cx| {
            i.set_text(
                &c.app
                    .torrents_watch_folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cx,
            )
        });
        self.set_bt_trackers
            .update(cx, |i, cx| i.set_text(&c.core.bt_tracker_list.join(", "), cx));
        self.set_sched_start.update(cx, |i, cx| {
            i.set_text(&format_minute_hhmm(c.core.default_schedule.start_minute), cx)
        });
        self.set_sched_end.update(cx, |i, cx| {
            i.set_text(&format_minute_hhmm(c.core.default_schedule.end_minute), cx)
        });
        self.set_battery_min.update(cx, |i, cx| {
            i.set_text(&c.core.battery_min_level_percent.to_string(), cx)
        });
        self.set_wb_small_kb
            .update(cx, |i, cx| i.set_text(&wb_small_val.to_string(), cx));
        self.set_wb_skip_ext.update(cx, |i, cx| {
            i.set_text(&c.app.wb_unwanted_extensions.join(" "), cx)
        });
        self.set_wb_only_ext.update(cx, |i, cx| {
            i.set_text(&c.app.wb_intercept_extensions.join(" "), cx)
        });
        self.set_wb_skip_hosts.update(cx, |i, cx| {
            i.set_text(&c.app.wb_unwanted_hosts.join(", "), cx)
        });
        self.set_ffmpeg_path.update(cx, |i, cx| {
            i.set_text(
                &c.core
                    .ffmpeg_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cx,
            )
        });
        self.set_ytdlp_path.update(cx, |i, cx| {
            i.set_text(
                &c.core
                    .yt_dlp_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cx,
            )
        });
        self.set_av_path.update(cx, |i, cx| {
            i.set_text(
                &c.core
                    .antivirus_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cx,
            )
        });
        self.set_av_args
            .update(cx, |i, cx| i.set_text(&c.core.antivirus_args, cx));
        self.set_ext_path.update(cx, |i, cx| {
            i.set_text(
                &c.core
                    .external_app_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cx,
            )
        });
        self.set_ext_args
            .update(cx, |i, cx| i.set_text(&c.core.external_app_args_template, cx));
        self.set_remote_bind
            .update(cx, |i, cx| i.set_text(&c.app.remote_access_bind, cx));
        self.set_remote_port
            .update(cx, |i, cx| i.set_text(&c.app.remote_access_port.to_string(), cx));
        self.set_remote_pass.update(cx, |i, cx| {
            i.set_masked(true, cx);
            i.set_text(&remote_pw, cx);
        });
        self.set_notify_sound_path
            .update(cx, |i, cx| i.set_text(&sound_s, cx));
        self.set_low_disk_mb.update(cx, |i, cx| {
            i.set_text(&(c.core.low_disk_space_min_bytes / (1024 * 1024)).to_string(), cx)
        });
        self.set_low_disk_pct.update(cx, |i, cx| {
            i.set_text(&format!("{:.1}", c.core.low_disk_space_min_percent), cx)
        });
        self.set_backup_interval_m.update(cx, |i, cx| {
            i.set_text(&(c.app.db_backup_interval_secs.max(0) / 60).to_string(), cx)
        });
        self.set_batch_max_urls.update(cx, |i, cx| {
            i.set_text(&c.app.batch_download_max_urls_count.to_string(), cx)
        });
    }

    pub(crate) fn flush_tum_inputs_into_draft(&mut self, cx: &App) {
        let dl = parse_kb_limit(&self.set_tum_dl_kb.read(cx).text());
        let ul = parse_kb_limit(&self.set_tum_ul_kb.read(cx).text());
        let conns = self.set_tum_conns.read(cx).text().trim().parse().unwrap_or(16).max(1);
        let per_srv = self
            .set_tum_per_srv
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(4)
            .max(1);
        let conc = self
            .set_tum_concurrent
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(4)
            .max(1);
        let ratio = self
            .set_tum_ratio
            .read(cx)
            .text()
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
            .max(0.0);

        if let Some(p) = self.settings_cfg.core.tum_profiles.get_mut(&self.settings_tum_tab) {
            p.max_download_speed_bps = dl;
            p.max_upload_speed_bps = ul;
            p.max_connections = conns;
            p.max_connections_per_server = per_srv;
            p.max_concurrent_downloads = conc;
            p.max_upload_ratio = ratio;
        }
    }

    pub(crate) fn load_tum_inputs_from_draft(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.settings_cfg.core.tum_profiles.get(&self.settings_tum_tab).cloned() {
            self.set_tum_dl_kb
                .update(cx, |i, cx| i.set_text(&format_kb_limit(p.max_download_speed_bps), cx));
            self.set_tum_ul_kb
                .update(cx, |i, cx| i.set_text(&format_kb_limit(p.max_upload_speed_bps), cx));
            self.set_tum_conns
                .update(cx, |i, cx| i.set_text(&p.max_connections.to_string(), cx));
            self.set_tum_per_srv
                .update(cx, |i, cx| i.set_text(&p.max_connections_per_server.to_string(), cx));
            self.set_tum_concurrent
                .update(cx, |i, cx| i.set_text(&p.max_concurrent_downloads.to_string(), cx));
            self.set_tum_ratio
                .update(cx, |i, cx| i.set_text(&format!("{:.1}", p.max_upload_ratio), cx));
        }
    }

    pub(crate) fn switch_settings_tum(&mut self, next: TrafficUsageMode, cx: &mut Context<Self>) {
        self.flush_tum_inputs_into_draft(cx);
        self.settings_tum_tab = next;
        self.load_tum_inputs_from_draft(cx);
        cx.notify();
    }

    pub(crate) fn build_draft_config_with_inputs(&self, cx: &App) -> FdmConfig {
        let mut next = self.settings_cfg.clone();

        let dl = parse_kb_limit(&self.set_tum_dl_kb.read(cx).text());
        let ul = parse_kb_limit(&self.set_tum_ul_kb.read(cx).text());
        let conns = self.set_tum_conns.read(cx).text().trim().parse().unwrap_or(16).max(1);
        let per_srv = self
            .set_tum_per_srv
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(4)
            .max(1);
        let conc = self
            .set_tum_concurrent
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(4)
            .max(1);
        let ratio = self
            .set_tum_ratio
            .read(cx)
            .text()
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
            .max(0.0);
        if let Some(p) = next.core.tum_profiles.get_mut(&self.settings_tum_tab) {
            p.max_download_speed_bps = dl;
            p.max_upload_speed_bps = ul;
            p.max_connections = conns;
            p.max_connections_per_server = per_srv;
            p.max_concurrent_downloads = conc;
            p.max_upload_ratio = ratio;
        }

        let def_dir = self.set_default_dir.read(cx).text().trim().to_string();
        if !def_dir.is_empty() {
            next.core.default_download_path = PathBuf::from(def_dir);
        }
        next.core.partial_download_suffix = self.set_partial_suffix.read(cx).text().trim().to_string();
        next.core.auto_remove_finished_keep_days = self
            .set_keep_days
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(30);
        next.core.max_auto_retries = self
            .set_max_retries
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(3);
        next.core.unacceptably_slow_speed_bps =
            parse_kb_limit(&self.set_slow_speed_kb.read(cx).text());
        next.core.unacceptably_slow_interval_secs = self
            .set_slow_interval_s
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(60);
        next.core.max_additional_query_info_downloads = self
            .set_max_query_dl
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(2);
        next.core.max_additional_small_downloads = self
            .set_max_small_dl
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(2);
        next.core.max_additional_downloads_if_slow = self
            .set_max_slow_dl
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(2);

        let p_host = self.set_proxy_host.read(cx).text().trim().to_string();
        let parsed_p_port = self
            .set_proxy_port
            .read(cx)
            .text()
            .trim()
            .parse::<u16>()
            .unwrap_or(0);
        let p_port = if parsed_p_port == 0 && !p_host.is_empty() {
            8080
        } else {
            parsed_p_port
        };
        let p_user = self.set_proxy_user.read(cx).text();
        let p_pass = self.set_proxy_pass.read(cx).text();
        let s5_host = self.set_socks5_host.read(cx).text().trim().to_string();
        let parsed_s5_port = self
            .set_socks5_port
            .read(cx)
            .text()
            .trim()
            .parse::<u16>()
            .unwrap_or(0);
        let s5_port = if parsed_s5_port == 0 && !s5_host.is_empty() {
            1080
        } else {
            parsed_s5_port
        };
        apply_proxy_inputs_to_settings(
            &mut next.app.proxy,
            p_host,
            p_port,
            s5_host,
            s5_port,
            p_user,
            p_pass,
            self.proxy_use_http_for_ftp,
            &self.settings_cfg.app.proxy,
        );

        next.core.bt_session_port = self
            .set_bt_port
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(6881);
        let watch_s = self.set_bt_watch_dir.read(cx).text().trim().to_string();
        next.app.torrents_watch_folder = if watch_s.is_empty() {
            None
        } else {
            Some(PathBuf::from(watch_s))
        };
        next.core.bt_tracker_list = self
            .set_bt_trackers
            .read(cx)
            .text()
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        next.core.default_schedule.start_minute =
            parse_minute_hhmm(&self.set_sched_start.read(cx).text(), 0);
        next.core.default_schedule.end_minute =
            parse_minute_hhmm(&self.set_sched_end.read(cx).text(), 360);
        next.core.battery_min_level_percent = self
            .set_battery_min
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(15);

        next.app.wb_too_small_size_bytes = parse_wb_threshold_bytes(
            &self.set_wb_small_kb.read(cx).text(),
            next.core.wb_small_size_unit_mb,
        );
        next.app.wb_unwanted_extensions =
            parse_list_tokens(&self.set_wb_skip_ext.read(cx).text());
        next.app.wb_intercept_extensions =
            parse_list_tokens(&self.set_wb_only_ext.read(cx).text());
        next.app.wb_unwanted_hosts = self
            .set_wb_skip_hosts
            .read(cx)
            .text()
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| x.to_ascii_lowercase())
            .collect();

        let ff = self.set_ffmpeg_path.read(cx).text().trim().to_string();
        next.core.ffmpeg_path = if ff.is_empty() { None } else { Some(PathBuf::from(ff)) };
        let ytdlp = self.set_ytdlp_path.read(cx).text().trim().to_string();
        next.core.yt_dlp_path = if ytdlp.is_empty() { None } else { Some(PathBuf::from(ytdlp)) };

        let av = self.set_av_path.read(cx).text().trim().to_string();
        next.core.antivirus_path = if av.is_empty() { None } else { Some(PathBuf::from(av)) };
        next.core.antivirus_args = self.set_av_args.read(cx).text();
        let ext = self.set_ext_path.read(cx).text().trim().to_string();
        next.core.external_app_path = if ext.is_empty() { None } else { Some(PathBuf::from(ext)) };
        next.core.external_app_args_template = self.set_ext_args.read(cx).text();

        let rbind = self.set_remote_bind.read(cx).text().trim().to_string();
        if !rbind.is_empty() {
            next.app.remote_access_bind = rbind;
        }
        next.app.remote_access_port = self
            .set_remote_port
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(9443);
        let rpass = self.set_remote_pass.read(cx).text();
        if !rpass.trim().is_empty() {
            next.app.remote_access_password = rpass;
        }

        let sound_raw = self.set_notify_sound_path.read(cx).text().trim().to_string();
        if sound_raw.is_empty() {
            next.app
                .notification_sounds
                .remove(&NotificationEvent::DownloadComplete);
        } else {
            next.app
                .notification_sounds
                .insert(NotificationEvent::DownloadComplete, PathBuf::from(sound_raw));
        }

        let low_disk_mb = self
            .set_low_disk_mb
            .read(cx)
            .text()
            .trim()
            .parse::<u64>()
            .unwrap_or(256);
        next.core.low_disk_space_min_bytes = low_disk_mb.saturating_mul(1024 * 1024);
        next.core.low_disk_space_min_percent = self
            .set_low_disk_pct
            .read(cx)
            .text()
            .trim()
            .parse::<f32>()
            .unwrap_or(2.0)
            .clamp(0.0, 95.0);

        let backup_mins = self
            .set_backup_interval_m
            .read(cx)
            .text()
            .trim()
            .parse::<i64>()
            .unwrap_or(60);
        next.app.db_backup_interval_secs = backup_mins.max(1).saturating_mul(60);
        next.app.batch_download_max_urls_count = self
            .set_batch_max_urls
            .read(cx)
            .text()
            .trim()
            .parse()
            .unwrap_or(1000);

        let live_cfg = self.engine.get_config();
        next.path_by_url_hash = live_cfg.path_by_url_hash;
        next.path_by_file_type = live_cfg.path_by_file_type;
        next.recent_folders = live_cfg.recent_folders;

        next
    }

    pub(crate) fn has_unsaved_settings_changes(&self, cx: &App) -> bool {
        let draft = self.build_draft_config_with_inputs(cx);
        let saved = self.engine.get_config();
        serde_json::to_value(&draft).ok() != serde_json::to_value(&saved).ok()
    }

    pub(crate) fn apply_settings(&mut self, close_page: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_cfg = self.build_draft_config_with_inputs(cx);

        match self.engine.update_config(self.settings_cfg.clone()) {
            Ok(()) => {
                let mut warn_toast: Option<String> = None;
                if let Err(e) = sync_autorun_on_login(
                    self.settings_cfg.app.autorun_on_login,
                    self.settings_cfg.app.launch_minimized,
                ) {
                    warn_toast = Some(format!("Settings saved, but LaunchAgent sync failed: {e}"));
                }
                if let Err(e) = sync_portable_marker(self.settings_cfg.app.portable_mode) {
                    warn_toast = Some(format!("Settings saved, but portable flag failed: {e}"));
                }
                set_macos_hide_dock_icon(self.settings_cfg.app.hide_dock_icon);
                self.preview_appearance(window, cx);
                self.refresh_dock_badge();

                if self.settings_cfg.app.enable_remote_access {
                    let eng_rc = Arc::clone(&self.engine);
                    let host = detect_lan_ip();
                    self.rt.spawn(async move {
                        let _ = eng_rc.ensure_remote_control(&host).await;
                    });
                } else {
                    self.engine.stop_remote_control();
                }

                if let Some(warn) = warn_toast {
                    self.set_toast(warn, true);
                } else {
                    self.set_toast("Saved all settings", false);
                }
                if close_page {
                    self.set_active_page(ActivePage::Downloads, cx);
                    window.focus(&self.focus_handle);
                }
            }
            Err(e) => self.set_toast(format!("Failed saving settings: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn cancel_settings_internal(
        &mut self,
        show_toast: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_dirty = show_toast && self.has_unsaved_settings_changes(cx);
        let cfg = self.engine.get_config();
        self.settings_cfg = cfg.clone();
        self.settings_tum_tab = cfg.core.current_tum;
        self.proxy_use_http_for_ftp = ftp_mirrors_http(&cfg.app.proxy);
        set_macos_hide_dock_icon(cfg.app.hide_dock_icon);
        self.preview_appearance(window, cx);
        self.sync_settings_inputs_from_draft(cx);
        self.load_tum_inputs_from_draft(cx);
        self.refresh_dock_badge();
        if was_dirty {
            self.set_toast("Reverted unsaved settings", false);
        }
        self.set_active_page(ActivePage::Downloads, cx);
        window.focus(&self.focus_handle);
        cx.notify();
    }

    pub(crate) fn cancel_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_settings_internal(true, window, cx);
    }

    pub(crate) fn reset_settings_to_defaults_action(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.engine.reset_settings_to_defaults() {
            Ok(()) => {
                let _ = self.engine.ensure_remote_password();
                let cfg = self.engine.get_config();
                self.settings_cfg = cfg.clone();
                self.settings_tum_tab = cfg.core.current_tum;
                self.proxy_use_http_for_ftp = ftp_mirrors_http(&cfg.app.proxy);
                let _ = sync_autorun_on_login(
                    cfg.app.autorun_on_login,
                    cfg.app.launch_minimized,
                );
                let _ = sync_portable_marker(cfg.app.portable_mode);
                set_macos_hide_dock_icon(cfg.app.hide_dock_icon);
                self.preview_appearance(window, cx);
                self.sync_settings_inputs_from_draft(cx);
                self.load_tum_inputs_from_draft(cx);
                self.refresh_dock_badge();
                self.set_toast("Reset all settings to defaults", false);
            }
            Err(e) => self.set_toast(format!("Reset failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn install_native_host(&mut self, browser: Option<KnownWebBrowser>, cx: &mut Context<Self>) {
        let targets = match browser {
            Some(b) => vec![b],
            None => vec![
                KnownWebBrowser::Chrome,
                KnownWebBrowser::Firefox,
                KnownWebBrowser::Brave,
                KnownWebBrowser::Edge,
                KnownWebBrowser::Vivaldi,
                KnownWebBrowser::Opera,
            ],
        };
        let mut ok_names = Vec::new();
        let mut last_err = None;
        for b in targets {
            match install_native_host_for_browser(b) {
                Ok(_) => ok_names.push(browser_display_name(b)),
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        if !ok_names.is_empty() {
            self.set_toast(
                format!("Installed Native Host for: {}", ok_names.join(", ")),
                false,
            );
        } else if let Some(err) = last_err {
            self.set_toast(format!("Install failed: {err}"), true);
        }
        cx.notify();
    }

    pub(crate) fn export_downloads_action(&mut self, cx: &mut Context<Self>) {
        let path = PathBuf::from(self.set_data_io_path.read(cx).text().trim());
        let fmt = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json")
            .to_string();
        match self
            .engine
            .export_downloads_formatted(&path, &fmt, None, false)
        {
            Ok(n) => self.set_toast(format!("Exported {n} downloads to {}", path.display()), false),
            Err(e) => self.set_toast(format!("Export failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn export_settings_action(&mut self, cx: &mut Context<Self>) {
        let raw = PathBuf::from(self.set_data_io_path.read(cx).text().trim());
        let path = raw
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(self.engine.data_dir())
            .join("ferrite-settings-export.json");
        match self.engine.export_settings(&path) {
            Ok(()) => self.set_toast(format!("Exported settings to {}", path.display()), false),
            Err(e) => self.set_toast(format!("Settings export failed: {e}"), true),
        }
        cx.notify();
    }

    pub(crate) fn import_downloads_action(&mut self, cx: &mut Context<Self>) {
        let path = PathBuf::from(self.set_data_io_path.read(cx).text().trim());
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            let is_sqlite = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("sqlite") || e.eq_ignore_ascii_case("db"))
                .unwrap_or(false);
            let res = if is_sqlite {
                eng.import_legacy_fdm_db(&path).await
            } else {
                eng.import_downloads(&path, true).await
            };
            match res {
                Ok(n) => q.push(UiAsyncMsg::Toast {
                    text: format!("Imported {n} downloads from {}", path.display()),
                    is_error: false,
                }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Import failed: {e}"),
                    is_error: true,
                }),
            }
        });
    }

    pub(crate) fn archive_diagnostics_action(&mut self, logs_only: bool, cx: &mut Context<Self>) {
        let raw = self.set_diag_zip_path.read(cx).text().trim().to_string();
        let dest = if raw.is_empty() {
            self.engine.data_dir().join("ferrite-diagnostics.zip")
        } else {
            PathBuf::from(raw)
        };
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn_blocking(move || {
            match eng.archive_diagnostics_bundle(&dest, logs_only) {
                Ok(zip_path) => q.push(UiAsyncMsg::DiagnosticsArchived { zip_path }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Diagnostics failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn create_portable_action(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.portable_dir_input.read(cx).text().trim().to_string();
        let target = if raw.is_empty() {
            self.engine.data_dir().join("ferrite-portable")
        } else {
            PathBuf::from(raw)
        };
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn_blocking(move || {
            match eng.create_portable_bundle(&target) {
                Ok(dir) => q.push(UiAsyncMsg::PortableCreated { dir }),
                Err(e) => q.push(UiAsyncMsg::Toast {
                    text: format!("Portable creation failed: {e}"),
                    is_error: true,
                }),
            }
        });
        cx.notify();
    }

    pub(crate) fn resolve_conflict_action(
        &mut self,
        id: DownloadId,
        action: FileExistsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let eng = Arc::clone(&self.engine);
        let q = self.msgs.clone();
        self.rt.spawn(async move {
            if let Err(e) = eng.resolve_conflict(id, action).await {
                q.push(UiAsyncMsg::Toast {
                    text: format!("Conflict error: {e}"),
                    is_error: true,
                });
            }
        });
        self.close_modal(window, cx);
    }

    fn render_window_titlebar(&self) -> AnyElement {
        let p = self.theme;
        div()
            .id("custom-titlebar")
            .h(p.spx(28.))
            .flex_shrink_0()
            .bg(p.bg_200)
            .border_b_1()
            .border_color(p.separator)
            .px_3()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .window_control_area(WindowControlArea::Drag)
            .on_click(|ev, window, _| {
                if ev.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .child(div().w(p.spx(76.)).flex_shrink_0())
            .child(
                div()
                    .text_size(p.fpx(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.text_secondary)
                    .child("Ferrite DM"),
            )
            .child(div().w(p.spx(76.)).flex_shrink_0())
            .into_any_element()
    }

    fn render_top_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.theme;
        let s = i18n::l(&self.settings_cfg.app.locale_name);
        let menu_open = self.active_popover == ActivePopover::MainMenu;

        if self.active_page != ActivePage::Downloads {
            let page_title = match self.active_page {
                ActivePage::Settings(_) => s.preferences,
                ActivePage::Plugins => s.addons,
                ActivePage::Downloads => s.downloads,
            };
            let is_settings = matches!(self.active_page, ActivePage::Settings(_));

            return div()
                .h(p.spx(54.))
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .px_4()
                .bg(p.bg_200)
                .border_b_1()
                .border_color(p.separator)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .child(icon_action_btn(
                            "tb-back-downloads",
                            "icons/arrow_left.svg",
                            Some(s.back_to_downloads.into()),
                            &p,
                            false,
                            cx,
                            move |this, _, w, cx| {
                                if is_settings {
                                    if this.has_unsaved_settings_changes(cx) {
                                        this.apply_settings(true, w, cx);
                                    } else {
                                        this.cancel_settings_internal(false, w, cx);
                                    }
                                } else {
                                    this.set_active_page(ActivePage::Downloads, cx);
                                    w.focus(&this.focus_handle);
                                    cx.notify();
                                }
                            },
                        ))
                        .child(div().w(px(1.)).h(p.spx(18.)).bg(p.separator))
                        .child(
                            div()
                                .text_size(p.fpx(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .child(page_title),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(ghost_icon_btn(
                            "btn-theme",
                            if p.is_dark { "icons/sun.svg" } else { "icons/moon.svg" },
                            p.spx_f32(16.),
                            p.text_secondary,
                            p.bg_elevated,
                            cx,
                            |this, _, _, cx| {
                                this.toggle_theme(cx);
                            },
                        ))
                        .child(
                            div()
                                .id("btn-main-menu")
                                .w(p.spx(32.))
                                .h(p.spx(32.))
                                .rounded(p.radius(8., 3.))
                                .bg(if menu_open { p.bg_elevated } else { gpui::transparent_black() })
                                .hover(move |s| s.bg(p.bg_elevated))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_popover(ActivePopover::MainMenu, cx);
                                }))
                                .child(svg_icon("icons/main_menu_btn.svg", p.spx_f32(17.), p.text_primary)),
                        ),
                )
                .into_any_element();
        }

        let visible_count = self.visible_downloads.len();
        let checked_count = self.checked_ids.len();
        let all_checked = !self.visible_downloads.is_empty()
            && self
                .visible_downloads
                .iter()
                .all(|d| self.checked_ids.contains(&d.id));
        let some_checked = checked_count > 0 && !all_checked;

        let is_filtered = self.active_filter != StateFilter::All || self.active_tag_id.is_some();
        let active_tag = self
            .active_tag_id
            .and_then(|tid| self.tags.iter().find(|t| t.id == tid).cloned());
        let search_non_empty = !self.search_input.read(cx).text().is_empty();

        let left_cluster: AnyElement = if checked_count > 0 {
            let checked_ids_vec: Vec<DownloadId> = self.checked_ids.iter().copied().collect();
            let first_checked = checked_ids_vec.first().copied();
            let ids_for_del = checked_ids_vec.clone();

            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1p5()
                .child(badge_themed(
                    format!("{checked_count} {}", s.selected_word),
                    p.accent,
                    p.accent_soft,
                    &p,
                ))
                .child(icon_action_btn(
                    "sel-start",
                    "icons/play.svg",
                    Some(s.start.into()),
                    &p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.batch_start_checked(cx);
                    },
                ))
                .child(icon_action_btn(
                    "sel-pause",
                    "icons/pause.svg",
                    Some(s.pause.into()),
                    &p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.batch_pause_checked(cx);
                    },
                ))
                .when_some(first_checked, |el, first_id| {
                    el.child(icon_action_btn(
                        "sel-move",
                        "icons/file_move.svg",
                        Some(s.move_to.into()),
                        &p,
                        false,
                        cx,
                        move |this, _, w, cx| {
                            this.open_move_dir_modal(first_id, w, cx);
                        },
                    ))
                })
                .child(danger_btn(
                    "sel-delete",
                    s.delete,
                    &p,
                    cx,
                    move |this, _, w, cx| {
                        this.request_delete_downloads(ids_for_del.clone(), w, cx);
                    },
                ))
                .child(ghost_icon_btn(
                    "sel-clear",
                    "icons/close_sm.svg",
                    p.spx_f32(14.),
                    p.text_muted,
                    p.bg_elevated,
                    cx,
                    |this, _, _, cx| {
                        this.checked_ids.clear();
                        cx.notify();
                    },
                ))
                .into_any_element()
        } else {
            let sort_open = self.active_popover == ActivePopover::SortMenu;
            let filter_open = self.active_popover == ActivePopover::FilterMenu;

            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("tb-sort-btn")
                        .h(p.spx(32.))
                        .px_2p5()
                        .rounded(p.radius(8., 3.))
                        .bg(if sort_open { p.bg_elevated } else { p.bg_surface })
                        .hover(move |s| s.bg(p.bg_elevated))
                        .border_1()
                        .border_color(if sort_open { p.accent } else { p.border })
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_popover(ActivePopover::SortMenu, cx);
                        }))
                        .child(svg_icon("icons/sort_icon.svg", p.spx_f32(14.), p.text_secondary))
                        .child(div().text_size(p.fpx(12.)).text_color(p.text_muted).child(s.sort_prefix))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_primary)
                                .child(sort_field_short_label(self.sort_by)),
                        )
                        .child(svg_icon(
                            if self.sort_ascending {
                                "icons/arrow_up.svg"
                            } else {
                                "icons/arrow_down.svg"
                            },
                            p.spx_f32(11.),
                            p.accent,
                        ))
                        .child(svg_icon("icons/chevron_down.svg", p.spx_f32(12.), p.text_muted)),
                )
                .child(
                    div()
                        .id("tb-filter-btn")
                        .h(p.spx(32.))
                        .px_2p5()
                        .rounded(p.radius(8., 3.))
                        .bg(if filter_open {
                            p.bg_elevated
                        } else if is_filtered {
                            p.accent_soft
                        } else {
                            p.bg_surface
                        })
                        .hover(move |s| s.bg(p.bg_elevated))
                        .border_1()
                        .border_color(if filter_open || is_filtered {
                            p.accent
                        } else {
                            p.border
                        })
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_popover(ActivePopover::FilterMenu, cx);
                        }))
                        .child(svg_icon(
                            "icons/filter.svg",
                            p.spx_f32(14.),
                            if is_filtered { p.accent } else { p.text_secondary },
                        ))
                        .when_some(active_tag.clone(), |el, tag| {
                            el.child(
                                div()
                                    .w(p.spx(9.))
                                    .h(p.spx(9.))
                                    .rounded(px(2.))
                                    .bg(parse_hex_color(&tag.color_hex)),
                            )
                            .child(
                                div()
                                    .text_size(p.fpx(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_primary)
                                    .child(tag.name),
                            )
                        })
                        .when(active_tag.is_none(), |el| {
                            el.child(
                                div()
                                    .text_size(p.fpx(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_primary)
                                    .child(state_filter_short_label(self.active_filter)),
                            )
                        })
                        .child(
                            div()
                                .px_1p5()
                                .py(px(1.))
                                .rounded_full()
                                .bg(p.bg_app)
                                .text_size(p.fpx(11.))
                                .text_color(p.text_muted)
                                .child(visible_count.to_string()),
                        )
                        .child(svg_icon("icons/chevron_down.svg", p.spx_f32(12.), p.text_muted)),
                )
                .when(is_filtered, |el| {
                    el.child(ghost_icon_btn(
                        "tb-filter-clear",
                        "icons/close_sm.svg",
                        p.spx_f32(13.),
                        p.text_muted,
                        p.bg_elevated,
                        cx,
                        |this, _, _, cx| {
                            this.active_filter = StateFilter::All;
                            this.active_tag_id = None;
                            this.refresh_snapshot(cx);
                            cx.notify();
                        },
                    ))
                })
                .into_any_element()
        };

        div()
            .h(p.spx(54.))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .bg(p.bg_200)
            .border_b_1()
            .border_color(p.separator)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2p5()
                    .child(
                        div()
                            .id("tb-check-all")
                            .w(p.spx(28.))
                            .h(p.spx(28.))
                            .rounded(p.radius(6., 2.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(move |s| s.bg(p.bg_elevated))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_select_all_visible(cx);
                            }))
                            .child(fdm_indeterminate_checkbox(all_checked, some_checked, &p)),
                    )
                    .child(left_cluster),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(icon_action_btn(
                        "btn-add-url",
                        "icons/plus_icon.svg",
                        Some(s.add_download.into()),
                        &p,
                        true,
                        cx,
                        |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_add_modal("", w, cx);
                        },
                    ))
                    .child(icon_action_btn(
                        "btn-batch",
                        "icons/batch_download_icon.svg",
                        Some(s.batch.into()),
                        &p,
                        false,
                        cx,
                        |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_batch_modal(w, cx);
                        },
                    ))
                    .child(
                        div()
                            .w(p.spx(230.))
                            .h(p.spx(32.))
                            .px_2p5()
                            .rounded(p.radius(8., 3.))
                            .bg(p.bg_surface)
                            .border_1()
                            .border_color(p.border)
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1p5()
                            .child(svg_icon("icons/search_icon.svg", p.spx_f32(14.), p.text_muted))
                            .child(div().flex_1().min_w_0().h_full().child(self.search_input.clone()))
                            .when(search_non_empty, |el| {
                                el.child(ghost_icon_btn(
                                    "search-clear",
                                    "icons/close_sm.svg",
                                    p.spx_f32(12.),
                                    p.text_muted,
                                    p.bg_elevated,
                                    cx,
                                    |this, _, _, cx| {
                                        this.search_input.update(cx, |i, cx| i.set_text("", cx));
                                        this.refresh_snapshot(cx);
                                        cx.notify();
                                    },
                                ))
                            }),
                    )
                    .child(ghost_icon_btn(
                        "btn-theme",
                        if p.is_dark { "icons/sun.svg" } else { "icons/moon.svg" },
                        p.spx_f32(16.),
                        p.text_secondary,
                        p.bg_elevated,
                        cx,
                        |this, _, _, cx| {
                            this.toggle_theme(cx);
                        },
                    ))
                    .child(
                        div()
                            .id("btn-main-menu")
                            .w(p.spx(32.))
                            .h(p.spx(32.))
                            .rounded(p.radius(8., 3.))
                            .bg(if menu_open { p.bg_elevated } else { gpui::transparent_black() })
                            .hover(move |s| s.bg(p.bg_elevated))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_popover(ActivePopover::MainMenu, cx);
                            }))
                            .child(svg_icon("icons/main_menu_btn.svg", p.spx_f32(17.), p.text_primary)),
                    ),
            )
            .into_any_element()
    }

    fn render_sort_header_cell(
        &self,
        id: &'static str,
        label: impl Into<String>,
        field: SortField,
        p: &ThemePalette,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let active = self.sort_by == field;
        let arrow = if self.sort_ascending {
            "icons/chevron_up.svg"
        } else {
            "icons/chevron_down.svg"
        };
        let hover_fg = p.text_primary;
        div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_size(p.fpx(12.))
            .text_color(if active { p.text_primary } else { p.text_muted })
            .hover(move |s| s.text_color(hover_fg))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_sort_column(field, cx);
            }))
            .child(label.into())
            .when(active, |el| el.child(svg_icon(arrow, p.spx_f32(11.), p.accent)))
    }

    fn render_downloads_table(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.theme;
        let s = i18n::l(&self.settings_cfg.app.locale_name);
        let rows_data = self.visible_downloads.clone();
        let has_any_downloads = !self.all_downloads.is_empty();
        let is_snail = self.summary.tum == TrafficUsageMode::Snail;
        let compact = self.settings_cfg.app.compact_downloads_view;
        let tag_map: std::collections::HashMap<i64, Tag> =
            self.tags.iter().map(|t| (t.id, t.clone())).collect();

        let parent_banner: Option<AnyElement> = self.parent_folder_id.map(|pid| {
            let parent_title = self
                .all_downloads
                .iter()
                .find(|d| d.id == pid)
                .map(|d| d.title.clone())
                .unwrap_or_else(|| format!("Batch #{pid}"));
            let child_cnt = rows_data.len();

            div()
                .h(p.spx(36.))
                .flex_shrink_0()
                .px_4()
                .bg(p.bg_200)
                .border_b_1()
                .border_color(p.separator)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(icon_action_btn(
                            "batch-up-level",
                            "icons/level_up.svg",
                            Some(s.back_to_all.into()),
                            &p,
                            false,
                            cx,
                            |this, _, _, cx| {
                                this.parent_folder_id = None;
                                this.refresh_snapshot(cx);
                                cx.notify();
                            },
                        ))
                        .child(svg_icon("icons/folder_mac.svg", p.spx_f32(14.), p.accent))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .child(parent_title),
                        )
                        .child(badge_themed(
                            format!("{child_cnt} items"),
                            p.text_muted,
                            p.bg_surface,
                            &p,
                        )),
                )
                .into_any_element()
        });

        let show_table_header = !rows_data.is_empty();

        let header = div()
            .h(p.spx(32.))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .bg(p.bg_100)
            .border_b_1()
            .border_color(p.separator)
            .text_size(p.fpx(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.text_muted)
            .child(
                div()
                    .w(p.spx(62.))
                    .pl(p.spx(30.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child("#"),
            )
            .child(
                self.render_sort_header_cell("hdr-name", s.col_name, SortField::Title, &p, cx)
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                self.render_sort_header_cell("hdr-size", s.col_size, SortField::Size, &p, cx)
                    .w(p.spx(134.))
                    .flex_shrink_0(),
            )
            .child(
                self.render_sort_header_cell("hdr-status", s.col_status, SortField::Status, &p, cx)
                    .w(p.spx(156.))
                    .flex_shrink_0(),
            )
            .child(div().w(p.spx(76.)).flex_shrink_0().child(s.col_download))
            .child(div().w(p.spx(68.)).flex_shrink_0().child(s.col_upload))
            .child(
                self.render_sort_header_cell(
                    "hdr-added",
                    s.col_added,
                    SortField::CreationTime,
                    &p,
                    cx,
                )
                .w(p.spx(100.))
                .flex_shrink_0(),
            )
            .child(div().w(p.spx(148.)).flex_shrink_0());

        let body: AnyElement = if rows_data.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_8()
                .child(
                    div()
                        .w(p.spx(72.))
                        .h(p.spx(72.))
                        .rounded_full()
                        .bg(p.bg_surface)
                        .border_1()
                        .border_color(p.border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(svg_icon("icons/tray_empty.svg", p.spx_f32(34.), p.accent)),
                )
                .child(
                    div()
                        .text_size(p.fpx(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_primary)
                        .child(if self.parent_folder_id.is_some() {
                            s.empty_batch_title
                        } else if has_any_downloads {
                            s.empty_filter_title
                        } else {
                            s.empty_title
                        }),
                )
                .child(
                    div()
                        .text_size(p.fpx(12.))
                        .text_color(p.text_muted)
                        .child(s.empty_hint),
                )
                .child(
                    div()
                        .pt_1()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(icon_action_btn(
                            "empty-add-btn",
                            "icons/plus_icon.svg",
                            Some(s.add_download.into()),
                            &p,
                            true,
                            cx,
                            |this, _, w, cx| {
                                this.open_add_modal("", w, cx);
                            },
                        ))
                        .when(self.parent_folder_id.is_some(), |el| {
                            el.child(action_btn(
                                "empty-back-batch",
                                s.back_to_all,
                                &p,
                                false,
                                cx,
                                |this, _, _, cx| {
                                    this.parent_folder_id = None;
                                    this.refresh_snapshot(cx);
                                    cx.notify();
                                },
                            ))
                        })
                        .when(self.parent_folder_id.is_none() && has_any_downloads, |el| {
                            el.child(action_btn(
                                "empty-reset-flt",
                                s.show_all_files,
                                &p,
                                false,
                                cx,
                                |this, _, _, cx| {
                                    this.active_filter = StateFilter::All;
                                    this.active_tag_id = None;
                                    this.search_input.update(cx, |i, cx| i.set_text("", cx));
                                    this.refresh_snapshot(cx);
                                    cx.notify();
                                },
                            ))
                        }),
                )
                .into_any_element()
        } else {
            let mut rendered_rows = Vec::with_capacity(rows_data.len());
            for (idx, rec) in rows_data.into_iter().enumerate() {
                let id = rec.id;
                let selected = self.selected_id == Some(id);
                let hovered = self.hovered_row_id == Some(id);
                let is_checked = self.checked_ids.contains(&id);
                let is_batch = rec.download_type == DownloadType::Batch;
                let is_multi_file_folder = is_batch || rec.files.len() > 1;
                let frac = rec.progress_fraction();
                let pct_int = (frac * 100.0).round() as u32;
                let (st_label, st_color) = state_badge_info(&rec, &p);
                let (type_icon, type_color) = if is_batch {
                    ("icons/single_download_folder_mac.svg", p.accent)
                } else {
                    download_icon_and_color(&rec)
                };

                let is_running =
                    rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued;
                let is_completed = rec.state == DownloadState::Completed;

                let size_text = if is_batch {
                    let count = rec
                        .module_data
                        .get("batch_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(rec.children_ids.len() as u64);
                    format!("{count} items")
                } else {
                    match rec.total_bytes {
                        Some(total) if is_completed || rec.downloaded_bytes >= total => {
                            format_bytes(total)
                        }
                        Some(total) => format!(
                            "{} / {}",
                            format_bytes(rec.downloaded_bytes),
                            format_bytes(total)
                        ),
                        None if rec.downloaded_bytes > 0 => format_bytes(rec.downloaded_bytes),
                        None => "--".into(),
                    }
                };

                let row_bg = if selected {
                    p.bg_row_selected
                } else if idx % 2 == 1 {
                    p.bg_row_alt
                } else {
                    p.bg_app
                };

                let tag_swatches: Vec<AnyElement> = rec
                    .tags
                    .iter()
                    .filter_map(|tid| tag_map.get(tid))
                    .take(4)
                    .map(|t| {
                        let c = parse_hex_color(&t.color_hex);
                        div()
                            .flex_shrink_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .px_1p5()
                            .py(px(1.))
                            .rounded(p.radius(4., 2.))
                            .bg(p.bg_surface)
                            .border_1()
                            .border_color(p.border)
                            .child(div().w(p.spx(7.)).h(p.spx(7.)).rounded(px(2.)).bg(c))
                            .child(
                                div()
                                    .text_size(p.fpx(10.))
                                    .text_color(p.text_secondary)
                                    .whitespace_nowrap()
                                    .child(t.name.clone()),
                            )
                            .into_any_element()
                    })
                    .collect();

                let progress_fill_color = if rec.state == DownloadState::Downloading && is_snail {
                    p.warning
                } else {
                    st_color
                };

                let status_cell: AnyElement = if is_batch {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(svg_icon("icons/batch.svg", p.spx_f32(13.), p.accent))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.accent)
                                .truncate()
                                .child("Batch Group (Open)"),
                        )
                        .into_any_element()
                } else if is_completed && !rec.flags.is_missing_files {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(svg_icon("icons/checkmark_v2.svg", p.spx_f32(13.), p.success))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.success)
                                .child("Completed"),
                        )
                        .into_any_element()
                } else if rec.flags.is_missing_files && is_completed {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(svg_icon("icons/triangle_alert.svg", p.spx_f32(12.), p.warning))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.warning)
                                .child("Missing Files"),
                        )
                        .into_any_element()
                } else if rec.state == DownloadState::Error {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(svg_icon("icons/single_download_error.svg", p.spx_f32(13.), p.danger))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.danger)
                                .child("Error"),
                        )
                        .into_any_element()
                } else if rec.state == DownloadState::Scheduled {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .child(svg_icon("icons/single_download_scheduler.svg", p.spx_f32(13.), p.warning))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.warning)
                                .child(format!("Scheduled ({pct_int}%)")),
                        )
                        .into_any_element()
                } else {
                    div()
                        .w(p.spx(156.))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div().flex_1().child(slim_progress_bar_active(
                                        format!("row-prog-{id}"),
                                        frac,
                                        p.spx_f32(4.0),
                                        progress_fill_color,
                                        p.bg_400,
                                        rec.state == DownloadState::Downloading,
                                    )),
                                )
                                .child(
                                    div()
                                        .w(p.spx(34.))
                                        .text_right()
                                        .text_size(p.fpx(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text_primary)
                                        .child(format!("{pct_int}%")),
                                ),
                        )
                        .child(
                            div()
                                .text_size(p.fpx(11.))
                                .text_color(if rec.state == DownloadState::Downloading {
                                    progress_fill_color
                                } else {
                                    p.text_muted
                                })
                                .child(st_label),
                        )
                        .into_any_element()
                };

                let added_or_eta_cell: AnyElement = if is_running && rec.eta_secs.is_some() {
                    div()
                        .w(p.spx(100.))
                        .flex_shrink_0()
                        .text_size(p.fpx(12.))
                        .text_color(if is_snail { p.warning } else { p.text_secondary })
                        .truncate()
                        .child(format!("{} {}", s.lbl_remaining, format_eta(rec.eta_secs)))
                        .into_any_element()
                } else {
                    div()
                        .w(p.spx(100.))
                        .flex_shrink_0()
                        .text_size(p.fpx(12.))
                        .text_color(p.text_muted)
                        .child(format_added_timestamp(rec.created_at))
                        .into_any_element()
                };

                let subtitle_text = if let Some(ref err) = rec.last_error {
                    err.clone()
                } else {
                    format!(
                        "{} · {}",
                        extract_domain_label(&rec.url),
                        rec.target_dir.display()
                    )
                };

                let primary_row_icon = if is_batch {
                    "icons/folder_open.svg"
                } else if is_completed {
                    if is_multi_file_folder {
                        "icons/single_download_folder_mac.svg"
                    } else {
                        "icons/single_download_folder.svg"
                    }
                } else if is_running {
                    "icons/single_download_pause.svg"
                } else if rec.state == DownloadState::Error {
                    "icons/single_download_error.svg"
                } else if rec.state == DownloadState::Scheduled {
                    "icons/single_download_scheduler.svg"
                } else {
                    "icons/single_download_play.svg"
                };

                let show_prio_arrows = hovered || selected;
                let is_hi_prio = rec.priority == DownloadPriority::High;
                let is_lo_prio = rec.priority == DownloadPriority::Low;

                rendered_rows.push(
                    div()
                        .id(SharedString::from(format!("dl-row-{id}")))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py(p.spx(if compact { 6. } else { 9. }))
                        .bg(row_bg)
                        .hover(move |s| s.bg(p.bg_row_hover))
                        .border_b_1()
                        .border_color(p.separator)
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
                            if *is_hovered {
                                this.hovered_row_id = Some(id);
                            } else if this.hovered_row_id == Some(id) {
                                this.hovered_row_id = None;
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.selected_id = Some(id);
                                this.active_popover = ActivePopover::RowContextMenu(id);
                                this.popover_anim_epoch =
                                    this.popover_anim_epoch.wrapping_add(1);
                                cx.notify();
                            }),
                        )
                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                            this.active_popover = ActivePopover::None;
                            if is_batch && ev.click_count() >= 2 {
                                this.parent_folder_id = Some(id);
                                this.refresh_snapshot(cx);
                                cx.notify();
                                return;
                            }
                            if !is_batch
                                && ev.click_count() >= 2
                                && this.settings_cfg.app.enable_standalone_downloads_windows
                            {
                                this.selected_id = Some(id);
                                this.open_download_window(id, cx);
                                cx.notify();
                                return;
                            }
                            if this.selected_id == Some(id)
                                && this.settings_cfg.app.toggle_bottom_panel_on_click
                            {
                                let next = !this.inspector_expanded;
                                this.set_inspector_expanded(next, cx);
                            } else {
                                this.selected_id = Some(id);
                                if this.settings_cfg.app.toggle_bottom_panel_on_click {
                                    this.set_inspector_expanded(true, cx);
                                } else {
                                    cx.notify();
                                }
                            }
                        }))
                        .child(
                            div()
                                .w(p.spx(62.))
                                .flex_shrink_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("chk-{id}")))
                                        .w(p.spx(22.))
                                        .h(p.spx(22.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_check(id, cx);
                                        }))
                                        .child(fdm_checkbox(is_checked, &p)),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("type-btn-{id}")))
                                        .w(p.spx(if compact { 28. } else { 32. }))
                                        .h(p.spx(if compact { 28. } else { 32. }))
                                        .rounded(p.radius(8., 3.))
                                        .bg(type_color)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if is_batch {
                                                this.parent_folder_id = Some(id);
                                                this.refresh_snapshot(cx);
                                            } else if is_completed {
                                                this.open_download_file(id);
                                            } else {
                                                this.toggle_download(id);
                                            }
                                            cx.notify();
                                        }))
                                        .child(svg_icon(
                                            type_icon,
                                            p.spx_f32(16.),
                                            rgb(0xffffff).into(),
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1p5()
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_shrink()
                                                .overflow_hidden()
                                                .text_size(p.fpx(13.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.text_primary)
                                                .child(ellipsis_text(rec.title.clone())),
                                        )
                                        .when(rec.download_type != DownloadType::Www, |el| {
                                            el.child(badge_themed(
                                                rec.download_type.display_name(),
                                                p.accent,
                                                p.accent_soft,
                                                &p,
                                            ))
                                        })
                                        .when(is_hi_prio, |el| {
                                            el.child(badge_themed("High", p.amber, p.bg_surface, &p))
                                        })
                                        .when(is_lo_prio, |el| {
                                            el.child(badge_themed(
                                                "Low",
                                                p.text_muted,
                                                p.bg_surface,
                                                &p,
                                            ))
                                        })
                                        .when(rec.flags.sequential_play_asap, |el| {
                                            el.child(badge_themed(
                                                "Play ASAP",
                                                p.success,
                                                p.bg_surface,
                                                &p,
                                            ))
                                        })
                                        .children(tag_swatches),
                                )
                                .when(!compact || rec.last_error.is_some(), |el| {
                                    el.child(
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_size(p.fpx(11.))
                                            .text_color(if rec.last_error.is_some() {
                                                p.danger
                                            } else {
                                                p.text_muted
                                            })
                                            .child(ellipsis_text(subtitle_text)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .w(p.spx(134.))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(p.fpx(12.))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(size_text),
                        )
                        .child(status_cell)
                        .child(
                            div()
                                .w(p.spx(76.))
                                .flex_shrink_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .text_size(p.fpx(12.))
                                .when(rec.download_speed_bps > 0, |el| {
                                    el.child(svg_icon("icons/arrow_down.svg", p.spx_f32(11.), p.accent))
                                        .child(
                                            div()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.text_primary)
                                                .child(format_speed(rec.download_speed_bps)),
                                        )
                                })
                                .when(rec.download_speed_bps == 0, |el| {
                                    el.child(div().text_color(p.text_subtle).child("--"))
                                }),
                        )
                        .child(
                            div()
                                .w(p.spx(68.))
                                .flex_shrink_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .text_size(p.fpx(12.))
                                .when(rec.upload_speed_bps > 0, |el| {
                                    el.child(svg_icon("icons/arrow_up.svg", p.spx_f32(11.), p.success))
                                        .child(
                                            div()
                                                .text_color(p.text_primary)
                                                .child(format_speed(rec.upload_speed_bps)),
                                        )
                                })
                                .when(rec.upload_speed_bps == 0, |el| {
                                    el.child(div().text_color(p.text_subtle).child("--"))
                                }),
                        )
                        .child(added_or_eta_cell)
                        .child(
                            div()
                                .w(p.spx(148.))
                                .flex_shrink_0()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_end()
                                .gap_0p5()
                                .when(show_prio_arrows && !is_batch, |el| {
                                    el.child(ghost_icon_btn(
                                        format!("row-prio-up-{id}"),
                                        "icons/priority_up.svg",
                                        p.spx_f32(13.),
                                        if is_hi_prio { p.accent } else { p.text_muted },
                                        p.bg_elevated,
                                        cx,
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.bump_priority_action(id, true, cx);
                                        },
                                    ))
                                    .child(ghost_icon_btn(
                                        format!("row-prio-dn-{id}"),
                                        "icons/priority_down.svg",
                                        p.spx_f32(13.),
                                        if is_lo_prio { p.warning } else { p.text_muted },
                                        p.bg_elevated,
                                        cx,
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.bump_priority_action(id, false, cx);
                                        },
                                    ))
                                })
                                .child(ghost_icon_btn(
                                    format!("row-act-{id}"),
                                    primary_row_icon,
                                    p.spx_f32(14.),
                                    if is_batch || is_completed {
                                        p.accent
                                    } else if is_running {
                                        p.text_primary
                                    } else if rec.state == DownloadState::Error {
                                        p.danger
                                    } else {
                                        p.success
                                    },
                                    p.bg_elevated,
                                    cx,
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if is_batch {
                                            this.parent_folder_id = Some(id);
                                            this.refresh_snapshot(cx);
                                        } else if is_completed {
                                            this.open_download_file(id);
                                        } else {
                                            this.toggle_download(id);
                                        }
                                        cx.notify();
                                    },
                                ))
                                .child(ghost_icon_btn(
                                    format!("row-rev-{id}"),
                                    "icons/folder_mac.svg",
                                    p.spx_f32(14.),
                                    p.text_muted,
                                    p.bg_elevated,
                                    cx,
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.reveal_download_file(id);
                                        cx.notify();
                                    },
                                ))
                                .child(ghost_icon_btn(
                                    format!("row-more-{id}"),
                                    "icons/menu_dots.svg",
                                    p.spx_f32(14.),
                                    p.text_muted,
                                    p.bg_elevated,
                                    cx,
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.selected_id = Some(id);
                                        this.toggle_popover(ActivePopover::RowContextMenu(id), cx);
                                    },
                                )),
                        ),
                );
            }

            div()
                .flex_1()
                .id("dl-rows-scroll")
                .overflow_y_scroll()
                .children(rendered_rows)
                .into_any_element()
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(p.bg_100)
            .when_some(parent_banner, |el, banner| el.child(banner))
            .when(show_table_header, |el| el.child(header))
            .child(body)
            .into_any_element()
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.theme;
        let s = i18n::l(&self.settings_cfg.app.locale_name);
        let current_tum = self.summary.tum;
        let is_snail = current_tum == TrafficUsageMode::Snail;
        let bar_bg = if is_snail { p.snail_bg } else { p.bg_200 };
        let bar_fg = if is_snail { p.snail_text } else { p.text_secondary };
        let bar_border = if is_snail { p.snail_border } else { p.separator };
        let tum_badge_color = match current_tum {
            TrafficUsageMode::High => p.accent,
            TrafficUsageMode::Medium => p.success,
            TrafficUsageMode::Low => p.warning,
            TrafficUsageMode::Snail => p.snail_border,
        };
        let tum_pill_bg = if is_snail {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        } else if current_tum != TrafficUsageMode::High {
            p.accent_soft
        } else {
            p.bg_surface
        };
        let pill_bg = if is_snail {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        } else {
            p.bg_surface
        };
        let pill_border = if is_snail { p.snail_border } else { p.border };

        let dl_speed = format_speed(self.summary.total_download_bps);
        let ul_speed = format_speed(self.summary.total_upload_bps);
        let prev = preventer_label(self.summary.preventer);
        let total_cnt = self.all_downloads.len();
        let active_cnt = self
            .all_downloads
            .iter()
            .filter(|d| d.state.is_running_or_seeding() || d.state == DownloadState::Queued)
            .count();
        let shutdown_active = self.engine.get_config().core.shutdown_when_done;
        let shutdown_badge_lbl = match shutdown_active {
            ShutdownType::None => None,
            ShutdownType::Sleep => Some("Auto-Sleep"),
            ShutdownType::Hibernate => Some("Auto-Hibernate"),
            ShutdownType::Shutdown => Some("Auto-Shutdown"),
            ShutdownType::QuitApp => Some("Auto-Quit"),
        };

        div()
            .h(p.spx(36.))
            .flex_shrink_0()
            .px_3()
            .bg(bar_bg)
            .border_t_1()
            .border_color(bar_border)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(snail_switch_btn("sb-snail-btn", is_snail, &p, cx, |this, _, _, cx| {
                        this.toggle_snail_mode(cx);
                    }))
                    .when(is_snail, |el| {
                        el.child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.snail_text)
                                .child(s.snail_mode),
                        )
                    })
                    .child(
                        div()
                            .id("sb-tum-btn")
                            .h(p.spx(26.))
                            .px_2p5()
                            .rounded(p.radius(6., 2.))
                            .bg(tum_pill_bg)
                            .hover(|s| s.opacity(0.9))
                            .border_1()
                            .border_color(if !is_snail && current_tum != TrafficUsageMode::High {
                                tum_badge_color
                            } else {
                                pill_border
                            })
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .text_size(p.fpx(12.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_popover(ActivePopover::TumMenu, cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(svg_icon(
                                        "icons/arrow_down.svg",
                                        p.spx_f32(11.),
                                        if is_snail { bar_fg } else { p.accent },
                                    ))
                                    .child(div().text_color(bar_fg).child(dl_speed)),
                            )
                            .child(div().w(px(1.)).h(p.spx(12.)).bg(pill_border))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(svg_icon(
                                        "icons/arrow_up.svg",
                                        p.spx_f32(11.),
                                        if is_snail { bar_fg } else { p.success },
                                    ))
                                    .child(div().text_color(bar_fg).child(ul_speed)),
                            )
                            .child(div().w(px(1.)).h(p.spx(12.)).bg(pill_border))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .w(p.spx(16.))
                                            .h(p.spx(16.))
                                            .rounded(p.radius(4., 2.))
                                            .bg(if is_snail {
                                                gpui::hsla(0.0, 0.0, 1.0, 0.22)
                                            } else {
                                                tum_badge_color
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(svg_icon(
                                                "icons/trending_up.svg",
                                                p.spx_f32(11.),
                                                if is_snail { p.snail_text } else { p.on_accent },
                                            )),
                                    )
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(bar_fg)
                                            .child(current_tum.label().to_uppercase()),
                                    )
                                    .child(svg_icon("icons/arrow_drop_up.svg", p.spx_f32(10.), bar_fg)),
                            ),
                    )
                    .when_some(prev, |el, lbl| {
                        el.child(
                            div()
                                .px_1p5()
                                .py(px(2.))
                                .rounded(p.radius(4., 2.))
                                .bg(pill_bg)
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(svg_icon("icons/triangle_alert.svg", p.spx_f32(11.), p.warning))
                                .child(
                                    div()
                                        .text_size(p.fpx(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.warning)
                                        .child(lbl),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("sb-qr-btn")
                            .h(p.spx(26.))
                            .px_2()
                            .rounded(p.radius(6., 2.))
                            .hover(move |s| s.bg(pill_bg))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .text_size(p.fpx(12.))
                            .text_color(bar_fg)
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_remote_qr_modal(cx);
                            }))
                            .child(svg_icon("icons/qr.svg", p.spx_f32(13.), bar_fg))
                            .child(s.remote),
                    )
                    .child(
                        div()
                            .id("sb-privacy-pill")
                            .h(p.spx(26.))
                            .px_2()
                            .rounded(p.radius(6., 2.))
                            .hover(move |s| s.bg(pill_bg))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .text_size(p.fpx(12.))
                            .text_color(if is_snail { bar_fg } else { p.text_muted })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_toast(
                                    "Zero telemetry, zero ads, open local SQLite storage",
                                    false,
                                );
                                cx.notify();
                            }))
                            .child(svg_icon(
                                "icons/shield.svg",
                                p.spx_f32(13.),
                                if is_snail { bar_fg } else { p.success },
                            ))
                            .child(s.no_telemetry),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2p5()
                    .when_some(shutdown_badge_lbl, |el, s_lbl| {
                        el.child(badge_themed(s_lbl, p.amber, pill_bg, &p))
                    })
                    .child(
                        div()
                            .text_size(p.fpx(12.))
                            .text_color(bar_fg)
                            .child(if active_cnt > 0 {
                                format!("{total_cnt} downloads · {active_cnt} active")
                            } else {
                                format!("{total_cnt} downloads")
                            }),
                    )
                    .when(
                        self.active_page == ActivePage::Downloads && self.selected_id.is_some(),
                        |el| {
                            el.child(
                                div()
                                    .id("sb-details-toggle")
                                    .h(p.spx(26.))
                                    .px_2()
                                    .rounded(p.radius(6., 2.))
                                    .bg(pill_bg)
                                    .border_1()
                                    .border_color(pill_border)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .text_size(p.fpx(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(bar_fg)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let next = !this.inspector_expanded;
                                        this.set_inspector_expanded(next, cx);
                                    }))
                                    .child(s.details)
                                    .child(svg_icon(
                                        if self.inspector_expanded {
                                            "icons/vertical_expand_arrow.svg"
                                        } else {
                                            "icons/vertical_collapse_arrow.svg"
                                        },
                                        p.spx_f32(12.),
                                        bar_fg,
                                    )),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}

impl Render for FerriteApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_modal_anim_epoch();
        self.window_active = window.is_window_active();
        let p = self.theme;
        let modal_epoch = self.modal_anim_epoch;
        let toast_epoch = self.toast_anim_epoch;
        let page_epoch = self.page_anim_epoch;

        let titlebar = self.render_window_titlebar();
        let top_bar = self.render_top_bar(cx);
        let main_content: AnyElement = match self.active_page {
            ActivePage::Downloads => {
                let table = self.render_downloads_table(cx);
                let bottom_panel = render_bottom_panel(self, cx);
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(table)
                    .child(bottom_panel)
                    .into_any_element()
            }
            ActivePage::Settings(_) => render_settings_page(self, cx),
            ActivePage::Plugins => render_plugins_page(self, cx),
        };
        let status_bar = self.render_status_bar(cx);
        let popover = render_popover(self, cx);
        let modal = render_active_modal(self, window, cx);
        let toast = self.toast.clone();

        div()
            .relative()
            .size_full()
            .bg(p.bg_100)
            .text_color(p.text_primary)
            .key_context("FerriteList")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &QuitApp, _, cx| {
                this.request_quit(cx);
            }))
            .on_action(cx.listener(|this, _: &AboutAppAction, _, cx| {
                this.open_about_modal(cx);
            }))
            .on_action(cx.listener(|_, _: &HideAppAction, _, cx| {
                cx.hide();
            }))
            .on_action(cx.listener(|_, _: &HideOtherAppsAction, _, cx| {
                cx.hide_other_apps();
            }))
            .on_action(cx.listener(|_, _: &ShowAllAppsAction, _, cx| {
                cx.unhide_other_apps();
                macos_unhide_all_applications();
            }))
            .on_action(cx.listener(|this, _: &CloseWindowAction, w, cx| {
                this.handle_close_window_request(w, cx);
            }))
            .on_action(cx.listener(|this, _: &AddUrlAction, w, cx| {
                this.open_add_modal("", w, cx);
            }))
            .on_action(cx.listener(|this, _: &BatchAddAction, w, cx| {
                this.open_batch_modal(w, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteUrlAction, w, cx| {
                this.paste_from_clipboard_add(w, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettingsAction, _, cx| {
                this.open_settings_page(SettingsTab::General, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenPluginsAction, _, cx| {
                this.open_plugins_page(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenQrAction, _, cx| {
                this.open_remote_qr_modal(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenImportSettingsAction, _, cx| {
                this.open_settings_page(SettingsTab::Advanced, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenExportSettingsAction, _, cx| {
                this.open_settings_page(SettingsTab::Advanced, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowTroubleshootingAction, _, cx| {
                this.show_troubleshooting = true;
                this.open_settings_page(SettingsTab::Troubleshooting, cx);
            }))
            .on_action(cx.listener(|this, _: &CheckForUpdatesAction, _, cx| {
                this.check_for_app_updates_action(true, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusSearchAction, w, cx| {
                this.set_active_page(ActivePage::Downloads, cx);
                let fh = this.search_input.read(cx).focus_handle.clone();
                w.focus(&fh);
            }))
            .on_action(cx.listener(|this, _: &EscapeModalAction, w, cx| {
                this.handle_escape(w, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomInAction, w, cx| {
                this.adjust_zoom(Some(10), w, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomOutAction, w, cx| {
                this.adjust_zoom(Some(-10), w, cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomResetAction, w, cx| {
                this.adjust_zoom(None, w, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleThemeAction, _, cx| {
                this.toggle_theme(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleInspectorAction, _, cx| {
                this.set_active_page(ActivePage::Downloads, cx);
                let next = !this.inspector_expanded;
                this.set_inspector_expanded(next, cx);
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreenAction, w, _| {
                w.toggle_fullscreen();
            }))
            .on_action(cx.listener(|_, _: &MinimizeWindowAction, w, _| {
                w.minimize_window();
            }))
            .on_action(cx.listener(|_, _: &ZoomWindowAction, w, _| {
                w.zoom_window();
            }))
            .on_action(cx.listener(|_, _: &BringAllToFrontAction, _, _| {
                macos_bring_all_to_front();
            }))
            .on_action(cx.listener(|this, _: &StartAllAction, _, cx| {
                let eng = Arc::clone(&this.engine);
                this.rt.spawn(async move {
                    let _ = eng.start_all().await;
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PauseAllAction, _, cx| {
                let eng = Arc::clone(&this.engine);
                this.rt.spawn(async move {
                    let _ = eng.pause_all().await;
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FilterAllAction, _, cx| {
                this.apply_state_filter_shortcut(StateFilter::All, cx);
            }))
            .on_action(cx.listener(|this, _: &FilterActiveAction, _, cx| {
                this.apply_state_filter_shortcut(StateFilter::Running, cx);
            }))
            .on_action(cx.listener(|this, _: &FilterDownloadingAction, _, cx| {
                this.apply_state_filter_shortcut(StateFilter::Downloading, cx);
            }))
            .on_action(cx.listener(|this, _: &FilterFinishedAction, _, cx| {
                this.apply_state_filter_shortcut(StateFilter::Finished, cx);
            }))
            .on_action(cx.listener(|this, _: &FilterStoppedAction, _, cx| {
                this.apply_state_filter_shortcut(StateFilter::Stopped, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSelectedAction, _, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    this.toggle_selected_download(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DeleteSelectedAction, w, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    let ids: Vec<DownloadId> = if !this.checked_ids.is_empty() {
                        this.checked_ids.iter().copied().collect()
                    } else if let Some(id) = this.selected_id {
                        vec![id]
                    } else {
                        Vec::new()
                    };
                    if !ids.is_empty() {
                        this.request_delete_downloads(ids, w, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &SelectAllDownloadsAction, _, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    this.toggle_select_all_visible(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &InputCopy, _, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    if let Some(id) = this.selected_id {
                        if let Some(rec) = this.all_downloads.iter().find(|d| d.id == id).cloned() {
                            this.copy_to_clipboard(rec.url, "Copied download URL", cx);
                        }
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &InputPaste, w, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    this.paste_from_clipboard_add(w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &InputSelectAll, _, cx| {
                if this.active_page == ActivePage::Downloads
                    && this.active_modal == ActiveModal::None
                    && this.active_popover == ActivePopover::None
                {
                    this.toggle_select_all_visible(cx);
                }
            }))
            .flex()
            .flex_col()
            .child(titlebar)
            .child(top_bar)
            .when_some(toast, |el, (msg, is_err, _)| {
                el.child(
                    div()
                        .id("toast-banner")
                        .px_4()
                        .py(p.spx(5.))
                        .bg(if is_err { p.danger } else { p.accent_soft })
                        .border_b_1()
                        .border_color(p.border)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .text_color(if is_err { p.on_accent } else { p.text_primary })
                                .child(msg),
                        )
                        .child(ghost_icon_btn(
                            "toast-dismiss",
                            "icons/close_sm.svg",
                            p.spx_f32(13.),
                            if is_err { p.on_accent } else { p.text_muted },
                            p.bg_elevated,
                            cx,
                            |this, _, _, cx| {
                                this.toast = None;
                                cx.notify();
                            },
                        ))
                        .with_animation(
                            SharedString::from(format!("toast-enter-{toast_epoch}")),
                            Animation::new(Duration::from_millis(160))
                                .with_easing(ease_out_quint()),
                            |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(main_content)
                    .with_animation(
                        SharedString::from(format!("page-enter-{page_epoch}")),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |el, d| el.opacity((0.35 + 0.65 * d).min(1.0)).pt(px(8.0 * (1.0 - d))),
                    ),
            )
            .child(status_bar)
            .when_some(popover, |el, pop| el.child(pop))
            .when_some(modal, |el, modal_card| {
                el.child(
                    div()
                        .id("modal-backdrop")
                        .absolute()
                        .inset_0()
                        .bg(p.modal_backdrop)
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_6()
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, w, cx| {
                            this.close_modal(w, cx);
                        }))
                        .child(modal_card)
                        .with_animation(
                            SharedString::from(format!("modal-backdrop-fade-{modal_epoch}")),
                            Animation::new(Duration::from_millis(130))
                                .with_easing(ease_out_quint()),
                            |el, delta| el.opacity(delta),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ftp_proxy_mirroring_detection_handles_empty_and_identical_endpoints() {
        let mut p = ProxySettings::default();
        assert!(ftp_mirrors_http(&p));

        p.http = ProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 8080,
            username: "u".into(),
            password: "p".into(),
        };
        assert!(!ftp_mirrors_http(&p));

        p.ftp = p.http.clone();
        assert!(ftp_mirrors_http(&p));
    }

    #[test]
    fn proxy_input_application_clears_socks5_creds_when_host_empty() {
        let mut p = ProxySettings::default();
        let prev = ProxySettings::default();
        apply_proxy_inputs_to_settings(
            &mut p,
            "127.0.0.1".into(),
            8080,
            "".into(),
            1080,
            "alice".into(),
            "secret".into(),
            true,
            &prev,
        );
        assert_eq!(p.http.username, "alice");
        assert_eq!(p.http.password, "secret");
        assert_eq!(p.ftp.host, "127.0.0.1");
        assert_eq!(p.socks5.username, "");
        assert_eq!(p.socks5.password, "");

        apply_proxy_inputs_to_settings(
            &mut p,
            "127.0.0.1".into(),
            8080,
            "127.0.0.1".into(),
            1080,
            "alice".into(),
            "secret".into(),
            false,
            &prev,
        );
        assert_eq!(p.socks5.username, "alice");
        assert_eq!(p.socks5.password, "secret");
        assert!(p.ftp.host.is_empty());
    }

    #[test]
    fn config_defaults_match_settings_input_fallbacks() {
        let cfg = FdmConfig::default();
        assert_eq!(cfg.app.remote_access_port, 9443);
        assert_eq!(cfg.app.db_backup_interval_secs, 3600);
        assert_eq!(cfg.app.batch_download_max_urls_count, 1000);
        assert_eq!(cfg.core.unacceptably_slow_interval_secs, 60);
        assert_eq!(cfg.core.low_disk_space_min_bytes, 256 * 1024 * 1024);
        assert!((cfg.core.low_disk_space_min_percent - 2.0).abs() < f32::EPSILON);
    }
}
