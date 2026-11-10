use crate::app::{FerriteApp, SettingsTab};
use crate::helpers::{
    browser_display_name, convert_wb_threshold_input, detect_antivirus_presets,
    expand_path_preview, is_native_host_installed, open_path_os, reaction_label,
    set_macos_hide_dock_icon,
};
use crate::i18n::l;
use crate::theme::{ThemePalette, parse_hex_color};
use crate::ui_widgets::{
    action_btn, badge_themed, chip_toggle, danger_btn, fdm_checkbox, labeled_field,
    password_field_row, radio_row, section_heading, sidebar_nav_item, svg_icon, switch_row_desc,
};
use fdm_core::os_integration::play_notification_sound;
use fdm_core::{
    BtEncryptionPolicy, DeleteButtonAction, ExistingFileReaction, KnownWebBrowser,
    NetworkProxyMode, ShutdownType, ThemeMode, TrafficUsageMode, UiStyle,
};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, SharedString, div,
    ease_out_quint, prelude::*, px,
};
use std::time::Duration;

pub fn render_settings_page(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let s = l(&app.settings_cfg.app.locale_name);
    let dirty = app.has_unsaved_settings_changes(cx);
    let active_tab = match app.active_page {
        crate::app::ActivePage::Settings(t) => t,
        _ => SettingsTab::General,
    };

    let mut nav_items: Vec<(SettingsTab, &'static str, &'static str)> = vec![
        (SettingsTab::General, "icons/settings.svg", s.nav_general),
        (SettingsTab::Browser, "icons/link.svg", s.nav_browser),
        (SettingsTab::Network, "icons/speed.svg", s.nav_network),
        (SettingsTab::Traffic, "icons/trending_up.svg", s.nav_traffic),
        (SettingsTab::Antivirus, "icons/shield.svg", s.nav_antivirus),
        (SettingsTab::BitTorrent, "icons/type_torrent.svg", s.nav_bittorrent),
        (SettingsTab::RemoteAccess, "icons/qr.svg", s.nav_remote),
        (SettingsTab::Advanced, "icons/folder_open.svg", s.nav_advanced),
    ];
    if app.show_troubleshooting {
        nav_items.push((SettingsTab::Troubleshooting, "icons/info.svg", s.nav_troubleshooting));
    }

    let body: AnyElement = match active_tab {
        SettingsTab::General => render_general_tab(app, &p, cx),
        SettingsTab::Browser => render_browser_tab(app, &p, cx),
        SettingsTab::Network => render_network_tab(app, &p, cx),
        SettingsTab::Traffic => render_traffic_tab(app, &p, cx),
        SettingsTab::Antivirus => render_antivirus_tab(app, &p, cx),
        SettingsTab::BitTorrent => render_bittorrent_tab(app, &p, cx),
        SettingsTab::RemoteAccess => render_remote_tab(app, &p, cx),
        SettingsTab::Advanced => render_advanced_tab(app, &p, cx),
        SettingsTab::Troubleshooting => render_troubleshooting_tab(app, &p, cx),
    };

    div()
        .flex_1()
        .min_h_0()
        .bg(p.bg_100)
        .flex()
        .flex_row()
        .child(
            // Left Sidebar matching SettingsPage.qml
            div()
                .w(p.spx(236.))
                .flex_shrink_0()
                .bg(p.bg_200)
                .border_r_1()
                .border_color(p.separator)
                .p_3()
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .id("prefs-title-nclicks")
                                .px_2p5()
                                .py_2()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.prefs_title_clicks =
                                        this.prefs_title_clicks.saturating_add(1);
                                    if this.prefs_title_clicks >= 5 {
                                        this.prefs_title_clicks = 0;
                                        this.show_troubleshooting = !this.show_troubleshooting;
                                        this.set_toast(
                                            if this.show_troubleshooting {
                                                "Troubleshooting diagnostics tab unlocked"
                                            } else {
                                                "Troubleshooting tab hidden"
                                            },
                                            false,
                                        );
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_size(p.fpx(18.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(p.text_primary)
                                        .child(s.preferences),
                                ),
                        )
                        .children(nav_items.into_iter().map(|(tab, icon, lbl)| {
                            sidebar_nav_item(
                                format!("pref-nav-{tab:?}"),
                                icon,
                                lbl,
                                active_tab == tab,
                                &p,
                                cx,
                                move |this, _, _, cx| {
                                    this.set_settings_tab(tab, cx);
                                },
                            )
                        })),
                )
                .child(
                    div()
                        .px_2p5()
                        .py_2()
                        .rounded(p.radius(8., 3.))
                        .bg(p.bg_100)
                        .border_1()
                        .border_color(p.separator)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(svg_icon("icons/shield.svg", p.spx_f32(14.), p.success))
                        .child(
                            div()
                                .text_size(p.fpx(11.))
                                .text_color(p.text_muted)
                                .child(s.local_config_zero_telemetry),
                        ),
                ),
        )
        .child(
            // Right detail pane + bottom Apply/Save bar
            div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .id("prefs-scroll")
                        .overflow_y_scroll()
                        .p_6()
                        .child({
                            let tab_epoch = app.settings_tab_anim_epoch;
                            div()
                                .id("prefs-tab-body")
                                .max_w(p.spx(860.))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(body)
                                .with_animation(
                                    SharedString::from(format!("settings-tab-enter-{tab_epoch}")),
                                    Animation::new(Duration::from_millis(150))
                                        .with_easing(ease_out_quint()),
                                    |el, delta| {
                                        el.opacity((0.4 + 0.6 * delta).min(1.0))
                                            .mt(px(6.0 * (1.0 - delta)))
                                    },
                                )
                        }),
                )
                .child(
                    div()
                        .h(p.spx(54.))
                        .flex_shrink_0()
                        .px_6()
                        .bg(p.bg_200)
                        .border_t_1()
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
                                .child(danger_btn(
                                    "prefs-reset",
                                    s.reset_to_defaults,
                                    &p,
                                    cx,
                                    |this, _, w, cx| {
                                        this.reset_settings_to_defaults_action(w, cx);
                                    },
                                ))
                                .when(dirty, |el| {
                                    el.child(badge_themed(
                                        s.unsaved_changes,
                                        p.amber,
                                        p.bg_300,
                                        &p,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(action_btn(
                                    "prefs-cancel",
                                    s.cancel,
                                    &p,
                                    false,
                                    cx,
                                    |this, _, w, cx| {
                                        this.cancel_settings(w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "prefs-apply",
                                    s.apply,
                                    &p,
                                    false,
                                    cx,
                                    |this, _, w, cx| {
                                        this.apply_settings(false, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "prefs-ok",
                                    s.save_and_close,
                                    &p,
                                    true,
                                    cx,
                                    |this, _, w, cx| {
                                        this.apply_settings(true, w, cx);
                                    },
                                )),
                        ),
                ),
        )
        .into_any_element()
}

fn render_general_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_app = app.settings_cfg.app.clone();
    let cfg_core = app.settings_cfg.core.clone();
    let def_dir_preview = expand_path_preview(
        &app.set_default_dir.read(cx).text(),
        "https://releases.example.org/isos/linux-amd64.iso",
        "linux-amd64.iso",
    );

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.general_settings_title),
        )
        .child(section_heading(s.sec_appearance, p))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child(s.color_theme))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1p5()
                        .children([
                            (ThemeMode::System, "System"),
                            (ThemeMode::Dark, "Dark"),
                            (ThemeMode::Light, "Light"),
                        ].into_iter().map(|(tm, lbl)| {
                            chip_toggle(
                                format!("gen-tm-{tm:?}"),
                                lbl,
                                cfg_app.theme_mode == tm,
                                p,
                                cx,
                                move |this, _, w, cx| {
                                    this.set_theme_mode(tm, w, cx);
                                },
                            )
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child(s.layout_density_style))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1p5()
                        .child(chip_toggle(
                            "gen-ui-new",
                            "Theme V2",
                            cfg_app.ui_style == UiStyle::New,
                            p,
                            cx,
                            |this, _, w, cx| {
                                this.settings_cfg.app.ui_style = UiStyle::New;
                                this.preview_appearance(w, cx);
                            },
                        ))
                        .child(chip_toggle(
                            "gen-ui-classic",
                            "Classic",
                            cfg_app.ui_style == UiStyle::Classic,
                            p,
                            cx,
                            |this, _, w, cx| {
                                this.settings_cfg.app.ui_style = UiStyle::Classic;
                                this.preview_appearance(w, cx);
                            },
                        ))
                        .child(chip_toggle(
                            "gen-compact",
                            "Compact Rows",
                            cfg_app.compact_downloads_view,
                            p,
                            cx,
                            |this, _, _, cx| {
                                this.settings_cfg.app.compact_downloads_view =
                                    !this.settings_cfg.app.compact_downloads_view;
                                cx.notify();
                            },
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child(s.interface_zoom))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([85u16, 100, 115, 130].into_iter().map(|z| {
                            chip_toggle(
                                format!("gen-zoom-{z}"),
                                format!("{z}%"),
                                cfg_app.overall_zoom_pct == z,
                                p,
                                cx,
                                move |this, _, w, cx| {
                                    this.settings_cfg.app.overall_zoom_pct = z;
                                    this.preview_appearance(w, cx);
                                },
                            )
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child(s.font_size))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([85u16, 100, 115, 130].into_iter().map(|z| {
                            chip_toggle(
                                format!("gen-fzoom-{z}"),
                                format!("{z}%"),
                                cfg_app.fonts_zoom_pct == z,
                                p,
                                cx,
                                move |this, _, w, cx| {
                                    this.settings_cfg.app.fonts_zoom_pct = z;
                                    this.preview_appearance(w, cx);
                                },
                            )
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child(s.interface_language))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap_1()
                        .children(
                            [
                                ("en_US", "English (US)"),
                                ("en_GB", "English (UK)"),
                                ("de_DE", "Deutsch"),
                                ("fr_FR", "Français"),
                                ("es_ES", "Español"),
                                ("ja_JP", "日本語"),
                            ]
                            .into_iter()
                            .map(|(code, lbl)| {
                                chip_toggle(
                                    format!("gen-lang-{code}"),
                                    lbl,
                                    cfg_app.locale_name == code,
                                    p,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.settings_cfg.app.locale_name = code.into();
                                        this.apply_theme_and_appearance_to_inputs(cx);
                                        cx.notify();
                                    },
                                )
                            }),
                        ),
                ),
        )
        .child(switch_row_desc(
            "gen-toggle-panel-click",
            "Toggle bottom details panel on item click",
            None,
            cfg_app.toggle_bottom_panel_on_click,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.toggle_bottom_panel_on_click =
                    !this.settings_cfg.app.toggle_bottom_panel_on_click;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-show-save-as",
            "Show Browse / Save As button in New Download dialog",
            None,
            cfg_app.show_save_as_button,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.show_save_as_button =
                    !this.settings_cfg.app.show_save_as_button;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-show-support",
            "Show Support / Community entry in main menu",
            None,
            cfg_app.show_support_button,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.show_support_button =
                    !this.settings_cfg.app.show_support_button;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-standalone-dl",
            "Open selected downloads in standalone windows when requested",
            None,
            cfg_app.enable_standalone_downloads_windows,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enable_standalone_downloads_windows =
                    !this.settings_cfg.app.enable_standalone_downloads_windows;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-standalone-add",
            "Use standalone New Download dialog windows",
            None,
            cfg_app.enable_standalone_create_downloads_windows,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enable_standalone_create_downloads_windows =
                    !this.settings_cfg.app.enable_standalone_create_downloads_windows;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_startup_system, p))
        .child(switch_row_desc(
            "gen-autorun",
            "Launch Ferrite DM automatically on login",
            None,
            cfg_app.autorun_on_login,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.autorun_on_login = !this.settings_cfg.app.autorun_on_login;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-minimized",
            "Start minimized",
            None,
            cfg_app.launch_minimized,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.launch_minimized = !this.settings_cfg.app.launch_minimized;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-close-bg",
            "Keep running in background when window is closed",
            None,
            cfg_app.close_to_background,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.close_to_background =
                    !this.settings_cfg.app.close_to_background;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-quit-conf",
            "Ask confirmation before quitting with active transfers",
            None,
            cfg_app.quit_confirmation,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.quit_confirmation =
                    !this.settings_cfg.app.quit_confirmation;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-hide-dock",
            "Hide macOS Dock icon (accessory background mode)",
            None,
            cfg_app.hide_dock_icon,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.hide_dock_icon = !this.settings_cfg.app.hide_dock_icon;
                set_macos_hide_dock_icon(this.settings_cfg.app.hide_dock_icon);
                this.refresh_dock_badge();
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-tray-ind",
            "Show active download count / speed badge on Dock icon",
            None,
            cfg_app.indicate_dl_ul_in_tray,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.indicate_dl_ul_in_tray =
                    !this.settings_cfg.app.indicate_dl_ul_in_tray;
                this.refresh_dock_badge();
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-dock-ul",
            "Include total upload speed in Dock / status badge",
            None,
            cfg_app.show_total_upload_speed_in_dock,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.show_total_upload_speed_in_dock =
                    !this.settings_cfg.app.show_total_upload_speed_in_dock;
                this.refresh_dock_badge();
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div().flex_1().child(switch_row_desc(
                        "gen-def-bt",
                        "Check if Ferrite DM is the default BitTorrent client",
                        None,
                        cfg_app.check_if_default_torrent_client,
                        p,
                        cx,
                        |this, _, _, cx| {
                            this.settings_cfg.app.check_if_default_torrent_client =
                                !this.settings_cfg.app.check_if_default_torrent_client;
                            cx.notify();
                        },
                    )),
                )
                .child(action_btn(
                    "gen-chk-def-bt-btn",
                    s.check_now,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.check_default_torrent_action(cx);
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div().flex_1().child(switch_row_desc(
                        "gen-auto-chk-upd",
                        "Check for application updates automatically",
                        None,
                        cfg_app.auto_check_updates,
                        p,
                        cx,
                        |this, _, _, cx| {
                            this.settings_cfg.app.auto_check_updates =
                                !this.settings_cfg.app.auto_check_updates;
                            cx.notify();
                        },
                    )),
                )
                .child(action_btn(
                    "gen-chk-app-upd-btn",
                    s.check_now,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.check_for_app_updates_action(true, cx);
                    },
                )),
        )
        .child(switch_row_desc(
            "gen-auto-inst-upd",
            "Install available updates automatically",
            None,
            cfg_app.auto_install_updates,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.auto_install_updates =
                    !this.settings_cfg.app.auto_install_updates;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-portable",
            "Use portable storage mode",
            Some("Stores SQLite DB and config inside ./ferrite-data beside the executable".into()),
            cfg_app.portable_mode,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.portable_mode = !this.settings_cfg.app.portable_mode;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child("Delete action default"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([
                            (DeleteButtonAction::AlwaysAsk, "Always Ask"),
                            (DeleteButtonAction::RemoveOnly, "List Only"),
                            (DeleteButtonAction::DeleteFiles, "List + Files"),
                        ].into_iter().map(|(da, lbl)| {
                            chip_toggle(
                                format!("gen-delact-{da:?}"),
                                lbl,
                                cfg_app.delete_button_action == da,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.settings_cfg.app.delete_button_action = da;
                                    cx.notify();
                                },
                            )
                        })),
                ),
        )
        .child(section_heading(s.sec_folders_routing, p))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "Default download folder (supports macros %date%, %host%, %type%, %ext%)",
                    app.set_default_dir.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "gen-browse-def-dir",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_folder_into(
                            this.set_default_dir.clone(),
                            "Choose Default Download Folder",
                            cx,
                        );
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(div().text_size(p.fpx(11.)).text_color(p.text_muted).child("Insert macro:"))
                .children(["%date%", "%host%", "%type%", "%ext%"].into_iter().map(|m| {
                    chip_toggle(
                        format!("gen-macro-{m}"),
                        m,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            let cur = this.set_default_dir.read(cx).text();
                            let sep = if cur.ends_with('/') { "" } else { "/" };
                            this.set_default_dir.update(cx, |i, cx| {
                                i.set_text(&format!("{cur}{sep}{m}"), cx);
                            });
                            cx.notify();
                        },
                    )
                })),
        )
        .child(
            div()
                .text_size(p.fpx(11.))
                .text_color(p.text_muted)
                .truncate()
                .child(format!("Preview path: {def_dir_preview}")),
        )
        .child(switch_row_desc(
            "gen-fixed-dir",
            "Always save into the fixed default folder without routing",
            None,
            cfg_core.fixed_download_path,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.fixed_download_path =
                    !this.settings_cfg.core.fixed_download_path;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-type-dir",
            "Subdirectory by file type / assigned tag folder",
            None,
            cfg_core.download_path_depends_on_file_type,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.download_path_depends_on_file_type =
                    !this.settings_cfg.core.download_path_depends_on_file_type;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-url-dir",
            "Remember last chosen folder per source host",
            None,
            cfg_core.download_path_depends_on_source_url,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.download_path_depends_on_source_url =
                    !this.settings_cfg.core.download_path_depends_on_source_url;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_files_cleanup, p))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "In-progress filename suffix",
                    app.set_partial_suffix.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Auto-remove finished after (days)",
                    app.set_keep_days.clone().into_any_element(),
                    p,
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child("If destination file already exists"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([
                            ExistingFileReaction::Ask,
                            ExistingFileReaction::Rename,
                            ExistingFileReaction::Overwrite,
                            ExistingFileReaction::Skip,
                        ].into_iter().map(|r| {
                            chip_toggle(
                                format!("gen-react-{r:?}"),
                                reaction_label(r),
                                cfg_core.existing_file_reaction == r,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.settings_cfg.core.existing_file_reaction = r;
                                    cx.notify();
                                },
                            )
                        })),
                ),
        )
        .child(switch_row_desc(
            "gen-silent",
            "Silent mode (auto-start browser & CLI downloads without Add dialog)",
            None,
            cfg_app.silent_mode_for_new_downloads,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.silent_mode_for_new_downloads =
                    !this.settings_cfg.app.silent_mode_for_new_downloads;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-mtime",
            "Preserve Last-Modified timestamp reported by server",
            None,
            cfg_core.take_last_modified_from_server,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.take_last_modified_from_server =
                    !this.settings_cfg.core.take_last_modified_from_server;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-remote-chg",
            "Restart download automatically if server file changes",
            None,
            cfg_core.auto_restart_if_remote_changed,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.auto_restart_if_remote_changed =
                    !this.settings_cfg.core.auto_restart_if_remote_changed;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-rm-fin",
            "Auto-remove finished downloads after retention days",
            None,
            cfg_core.auto_remove_finished,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.auto_remove_finished =
                    !this.settings_cfg.core.auto_remove_finished;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-rm-miss",
            "Automatically remove entries with missing files on disk",
            None,
            cfg_core.auto_remove_missing_files,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.auto_remove_missing_files =
                    !this.settings_cfg.core.auto_remove_missing_files;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "gen-del-svc",
            "Delete service/state files when download completes",
            None,
            cfg_core.delete_service_files_when_finished,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.delete_service_files_when_finished =
                    !this.settings_cfg.core.delete_service_files_when_finished;
                cx.notify();
            },
        ))
        .into_any_element()
}

fn render_browser_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_app = app.settings_cfg.app.clone();
    let unit_mb = app.settings_cfg.core.wb_small_size_unit_mb;
    let browsers = [
        KnownWebBrowser::Chrome,
        KnownWebBrowser::Firefox,
        KnownWebBrowser::Brave,
        KnownWebBrowser::Edge,
        KnownWebBrowser::Vivaldi,
        KnownWebBrowser::Opera,
        KnownWebBrowser::Safari,
    ];

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.browser_integration_title),
        )
        .child(section_heading(s.sec_interception, p))
        .child(switch_row_desc(
            "wb-intercept",
            "Intercept downloads from web browser extensions",
            None,
            cfg_app.wb_intercept_downloads,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_intercept_downloads =
                    !this.settings_cfg.app.wb_intercept_downloads;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "wb-small",
            "Do not intercept files smaller than threshold",
            None,
            cfg_app.wb_dont_intercept_too_small,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_dont_intercept_too_small =
                    !this.settings_cfg.app.wb_dont_intercept_too_small;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_3()
                .child(labeled_field(
                    if unit_mb {
                        "Minimum intercepted size (MB)"
                    } else {
                        "Minimum intercepted size (KB)"
                    },
                    app.set_wb_small_kb.clone().into_any_element(),
                    p,
                ))
                .child(chip_toggle(
                    "wb-unit-kb",
                    "KB",
                    !unit_mb,
                    p,
                    cx,
                    |this, _, _, cx| {
                        if this.settings_cfg.core.wb_small_size_unit_mb {
                            let cur = this.set_wb_small_kb.read(cx).text();
                            let conv = convert_wb_threshold_input(&cur, false);
                            this.set_wb_small_kb.update(cx, |i, cx| i.set_text(&conv, cx));
                            this.settings_cfg.core.wb_small_size_unit_mb = false;
                            cx.notify();
                        }
                    },
                ))
                .child(chip_toggle(
                    "wb-unit-mb",
                    "MB",
                    unit_mb,
                    p,
                    cx,
                    |this, _, _, cx| {
                        if !this.settings_cfg.core.wb_small_size_unit_mb {
                            let cur = this.set_wb_small_kb.read(cx).text();
                            let conv = convert_wb_threshold_input(&cur, true);
                            this.set_wb_small_kb.update(cx, |i, cx| i.set_text(&conv, cx));
                            this.settings_cfg.core.wb_small_size_unit_mb = true;
                            cx.notify();
                        }
                    },
                )),
        )
        .child(switch_row_desc(
            "wb-only-ext-tog",
            "Intercept ONLY listed file extensions",
            None,
            cfg_app.wb_intercept_only_extensions,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_intercept_only_extensions =
                    !this.settings_cfg.app.wb_intercept_only_extensions;
                cx.notify();
            },
        ))
        .child(labeled_field(
            "Allowed extensions (space/comma separated)",
            app.set_wb_only_ext.clone().into_any_element(),
            p,
        ))
        .child(switch_row_desc(
            "wb-skip-ext-tog",
            "Ignore unwanted file extensions",
            None,
            cfg_app.wb_dont_intercept_unwanted_extensions,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_dont_intercept_unwanted_extensions =
                    !this.settings_cfg.app.wb_dont_intercept_unwanted_extensions;
                cx.notify();
            },
        ))
        .child(labeled_field(
            "Ignored extensions",
            app.set_wb_skip_ext.clone().into_any_element(),
            p,
        ))
        .child(switch_row_desc(
            "wb-skip-hosts-tog",
            "Do not monitor downloads from excluded websites",
            None,
            cfg_app.wb_dont_intercept_unwanted_hosts,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_dont_intercept_unwanted_hosts =
                    !this.settings_cfg.app.wb_dont_intercept_unwanted_hosts;
                cx.notify();
            },
        ))
        .child(labeled_field(
            "Excluded domains",
            app.set_wb_skip_hosts.clone().into_any_element(),
            p,
        ))
        .child(switch_row_desc(
            "wb-fwd-aborted",
            "Send cancelled intercepts back to browser download manager",
            None,
            cfg_app.wb_forward_aborted_to_browser,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_forward_aborted_to_browser =
                    !this.settings_cfg.app.wb_forward_aborted_to_browser;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "wb-bypass-tog",
            "Skip interception while holding bypass modifier key",
            None,
            cfg_app.wb_dont_intercept_if_key_pressed,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.wb_dont_intercept_if_key_pressed =
                    !this.settings_cfg.app.wb_dont_intercept_if_key_pressed;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().text_xs().text_color(p.text_muted).child("Bypass key:"))
                .children(["Alt", "Ctrl", "Shift"].into_iter().map(|k| {
                    chip_toggle(
                        format!("wb-key-{k}"),
                        k,
                        cfg_app.wb_bypass_key.eq_ignore_ascii_case(k),
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.settings_cfg.app.wb_bypass_key = k.into();
                            cx.notify();
                        },
                    )
                })),
        )
        .child(section_heading(s.sec_native_hosts, p))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .children(browsers.into_iter().map(|b| {
                    let installed = is_native_host_installed(b);
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .p_2p5()
                        .rounded(p.radius(8., 3.))
                        .bg(p.bg_200)
                        .border_1()
                        .border_color(p.separator)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text_primary)
                                        .child(browser_display_name(b)),
                                )
                                .child(badge_themed(
                                    if installed { "Installed" } else { "Not Installed" },
                                    if installed { p.success } else { p.text_muted },
                                    p.bg_300,
                                    p,
                                )),
                        )
                        .child(action_btn(
                            format!("wb-inst-{b:?}"),
                            if installed { "Reinstall Host" } else { "Install Host" },
                            p,
                            !installed,
                            cx,
                            move |this, _, _, cx| {
                                this.install_native_host(Some(b), cx);
                            },
                        ))
                })),
        )
        .child(
            div().flex().justify_end().child(action_btn(
                "wb-inst-all",
                "Install All Native Hosts",
                p,
                true,
                cx,
                |this, _, _, cx| {
                    this.install_native_host(None, cx);
                },
            )),
        )
        .into_any_element()
}

fn render_network_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let proxy_mode = app.settings_cfg.app.proxy.mode;
    let proxy_epoch = app.proxy_box_anim_epoch;
    let cfg_core = &app.settings_cfg.core;
    let ssl_list = app.settings_ssl_exceptions.clone();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.network_title),
        )
        .child(section_heading(s.sec_proxy, p))
        .child(radio_row(
            "net-proxy-sys",
            "Use System Proxy Settings",
            Some("Follow macOS / OS network proxy preferences automatically".into()),
            proxy_mode == NetworkProxyMode::SystemProxy,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.proxy.mode = NetworkProxyMode::SystemProxy;
                cx.notify();
            },
        ))
        .child(radio_row(
            "net-proxy-none",
            "No Proxy (Direct Connection)",
            None,
            proxy_mode == NetworkProxyMode::NoProxy,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.proxy.mode = NetworkProxyMode::NoProxy;
                cx.notify();
            },
        ))
        .child(radio_row(
            "net-proxy-manual",
            "Manual Proxy Server",
            Some("Route HTTP, HTTPS, FTP & SOCKS5 traffic through custom proxy host".into()),
            proxy_mode == NetworkProxyMode::ManualProxy,
            p,
            cx,
            |this, _, _, cx| {
                if this.settings_cfg.app.proxy.mode != NetworkProxyMode::ManualProxy {
                    this.proxy_box_anim_epoch = this.proxy_box_anim_epoch.wrapping_add(1);
                }
                this.settings_cfg.app.proxy.mode = NetworkProxyMode::ManualProxy;
                cx.notify();
            },
        ))
        .when(proxy_mode == NetworkProxyMode::ManualProxy, |el| {
            el.child(
                div()
                    .id("net-proxy-manual-box")
                    .p_3()
                    .rounded(p.radius(8., 3.))
                    .bg(p.bg_200)
                    .border_1()
                    .border_color(p.border)
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(labeled_field(
                                "HTTP / HTTPS Proxy Host / IP",
                                app.set_proxy_host.clone().into_any_element(),
                                p,
                            ))
                            .child(labeled_field(
                                "Port",
                                app.set_proxy_port.clone().into_any_element(),
                                p,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(labeled_field(
                                "SOCKS5 Host (optional)",
                                app.set_socks5_host.clone().into_any_element(),
                                p,
                            ))
                            .child(labeled_field(
                                "SOCKS5 Port",
                                app.set_socks5_port.clone().into_any_element(),
                                p,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(labeled_field(
                                "Username (optional)",
                                app.set_proxy_user.clone().into_any_element(),
                                p,
                            ))
                            .child(labeled_field(
                                "Password",
                                password_field_row(
                                    "net-proxy-pass-eye",
                                    app.set_proxy_pass.clone().into_any_element(),
                                    app.show_proxy_pass,
                                    p,
                                    cx,
                                    |this, _, _, cx| {
                                        this.show_proxy_pass = !this.show_proxy_pass;
                                        let masked = !this.show_proxy_pass;
                                        this.set_proxy_pass
                                            .update(cx, |i, cx| i.set_masked(masked, cx));
                                        cx.notify();
                                    },
                                )
                                .into_any_element(),
                                p,
                            )),
                    )
                    .child(switch_row_desc(
                        "net-proxy-ftp-eq",
                        "Use HTTP proxy settings for FTP downloads",
                        None,
                        app.proxy_use_http_for_ftp,
                        p,
                        cx,
                        |this, _, _, cx| {
                            this.proxy_use_http_for_ftp = !this.proxy_use_http_for_ftp;
                            cx.notify();
                        },
                    ))
                    .with_animation(
                        SharedString::from(format!("net-proxy-anim-{proxy_epoch}")),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta).mt(px(-4.0 * (1.0 - delta))),
                    ),
            )
        })
        .child(section_heading(s.sec_headers_security, p))
        .child(switch_row_desc(
            "net-sys-ua",
            "Use system / browser default User-Agent string",
            None,
            cfg_core.use_system_default_user_agent,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.use_system_default_user_agent =
                    !this.settings_cfg.core.use_system_default_user_agent;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "net-motw",
            "Tag downloaded files with macOS Quarantine / Mark-of-the-Web",
            Some("Preserves OS origin verification attributes on downloaded executables and archives".into()),
            cfg_core.set_mark_of_the_web,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.set_mark_of_the_web =
                    !this.settings_cfg.core.set_mark_of_the_web;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "net-mobile",
            "Allow downloads on metered / cellular connections",
            None,
            cfg_core.allow_mobile_networks,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.allow_mobile_networks =
                    !this.settings_cfg.core.allow_mobile_networks;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "net-roaming",
            "Allow downloads while roaming",
            None,
            cfg_core.allow_roaming_networks,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.allow_roaming_networks =
                    !this.settings_cfg.core.allow_roaming_networks;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_ssl_exceptions, p))
        .when(ssl_list.is_empty(), |el| {
            el.child(
                div()
                    .text_xs()
                    .text_color(p.text_muted)
                    .child("No custom SSL certificate exceptions stored."),
            )
        })
        .when(!ssl_list.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .children(ssl_list.into_iter().map(|ex| {
                        let hp = ex.host_port.clone();
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .p_2p5()
                            .rounded(p.radius(8., 3.))
                            .bg(p.bg_200)
                            .border_1()
                            .border_color(p.separator)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text_primary)
                                            .child(ex.host_port),
                                    )
                                    .child(
                                        div()
                                            .text_size(p.fpx(10.))
                                            .text_color(p.text_muted)
                                            .truncate()
                                            .child(ex.sha256_fingerprint),
                                    ),
                            )
                            .child(danger_btn(
                                SharedString::from(format!("net-rm-ssl-{hp}")),
                                "Remove",
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    let _ = this.engine.remove_ssl_exception(&hp);
                                    this.settings_ssl_exceptions =
                                        this.engine.list_ssl_exceptions().unwrap_or_default();
                                    cx.notify();
                                },
                            ))
                    }))
                    .child(
                        div().flex().justify_end().child(danger_btn(
                            "net-clr-ssl",
                            "Clear All SSL Exceptions",
                            p,
                            cx,
                            |this, _, _, cx| {
                                let _ = this.engine.clear_ssl_exceptions();
                                this.settings_ssl_exceptions.clear();
                                cx.notify();
                            },
                        )),
                    ),
            )
        })
        .into_any_element()
}

fn render_traffic_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_core = &app.settings_cfg.core;
    let tum_tab = app.settings_tum_tab;

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.traffic_limits_title),
        )
        .child(section_heading(s.sec_tum_profiles, p))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1p5()
                .children([
                    (TrafficUsageMode::Snail, "Snail Mode"),
                    (TrafficUsageMode::Low, "Low Speed"),
                    (TrafficUsageMode::Medium, "Medium Speed"),
                    (TrafficUsageMode::High, "High Speed"),
                ].into_iter().map(|(m, lbl)| {
                    chip_toggle(
                        format!("tum-tab-{m:?}"),
                        lbl,
                        tum_tab == m,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.switch_settings_tum(m, cx);
                        },
                    )
                })),
        )
        .child(
            div()
                .p_3p5()
                .rounded(p.radius(10., 3.))
                .bg(p.bg_200)
                .border_1()
                .border_color(p.border)
                .flex()
                .flex_col()
                .gap_2p5()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(labeled_field(
                            "Max download speed (KB/s, 0 = ∞)",
                            app.set_tum_dl_kb.clone().into_any_element(),
                            p,
                        ))
                        .child(labeled_field(
                            "Max upload speed (KB/s, 0 = ∞)",
                            app.set_tum_ul_kb.clone().into_any_element(),
                            p,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(labeled_field(
                            "Max total connections",
                            app.set_tum_conns.clone().into_any_element(),
                            p,
                        ))
                        .child(labeled_field(
                            "Max connections per server",
                            app.set_tum_per_srv.clone().into_any_element(),
                            p,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_3()
                        .child(labeled_field(
                            "Max concurrent downloads",
                            app.set_tum_concurrent.clone().into_any_element(),
                            p,
                        ))
                        .child(labeled_field(
                            "Max upload seed ratio (0.0 = ∞)",
                            app.set_tum_ratio.clone().into_any_element(),
                            p,
                        )),
                ),
        )
        .child(switch_row_desc(
            "tr-restore-tum",
            "Restore last Traffic Usage Mode on startup",
            None,
            cfg_core.restore_tum_at_startup,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.restore_tum_at_startup =
                    !this.settings_cfg.core.restore_tum_at_startup;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "tr-compliant",
            "Strict compliant bandwidth throttling across HTTP + BitTorrent",
            None,
            cfg_core.traffic_limits_compliant_mode,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.traffic_limits_compliant_mode =
                    !this.settings_cfg.core.traffic_limits_compliant_mode;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_queue_slow, p))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "Extra slots: info queries",
                    app.set_max_query_dl.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Extra slots: small files",
                    app.set_max_small_dl.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Extra slots when slow",
                    app.set_max_slow_dl.clone().into_any_element(),
                    p,
                )),
        )
        .child(switch_row_desc(
            "tr-detect-slow",
            "Stop downloads when speed drops below acceptable threshold",
            None,
            cfg_core.detect_unacceptably_slow,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.detect_unacceptably_slow =
                    !this.settings_cfg.core.detect_unacceptably_slow;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "Minimum acceptable speed (KB/s)",
                    app.set_slow_speed_kb.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Evaluation interval (seconds)",
                    app.set_slow_interval_s.clone().into_any_element(),
                    p,
                )),
        )
        .child(switch_row_desc(
            "tr-unwanted-err",
            "Detect unexpected HTML error pages returned instead of target files",
            None,
            cfg_core.detect_unwanted_errors,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.detect_unwanted_errors =
                    !this.settings_cfg.core.detect_unwanted_errors;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "tr-auto-retry",
            "Automatically retry failed transfers",
            None,
            cfg_core.auto_retry_failed,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.auto_retry_failed =
                    !this.settings_cfg.core.auto_retry_failed;
                cx.notify();
            },
        ))
        .child(labeled_field(
            "Maximum auto-retries per download",
            app.set_max_retries.clone().into_any_element(),
            p,
        ))
        .into_any_element()
}

fn render_antivirus_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_core = &app.settings_cfg.core;
    let custom_mode = cfg_core.antivirus_custom_mode;
    let cur_uid = cfg_core.antivirus_uid.clone();
    let av_epoch = app.av_preset_anim_epoch;
    let presets = detect_antivirus_presets();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.antivirus_title),
        )
        .child(section_heading(s.sec_antivirus, p))
        .child(switch_row_desc(
            "av-enable",
            "Scan newly completed downloads automatically",
            Some("Runs the configured local scanner executable against each finished file".into()),
            cfg_core.auto_launch_antivirus,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.auto_launch_antivirus =
                    !this.settings_cfg.core.auto_launch_antivirus;
                if this.settings_cfg.core.auto_launch_antivirus
                    && !this.settings_cfg.core.antivirus_custom_mode
                    && this.set_av_path.read(cx).text().trim().is_empty()
                {
                    let presets = detect_antivirus_presets();
                    if let Some((uid, _, bin, args, _)) = presets
                        .iter()
                        .find(|(_, _, _, _, exists)| *exists)
                        .or_else(|| presets.first())
                    {
                        this.settings_cfg.core.antivirus_uid = (*uid).into();
                        this.set_av_path.update(cx, |i, cx| i.set_text(bin, cx));
                        this.set_av_args.update(cx, |i, cx| i.set_text(args, cx));
                    }
                }
                cx.notify();
            },
        ))
        .child(radio_row(
            "av-mode-preset",
            "Detected Local Scanner Preset",
            None,
            !custom_mode,
            p,
            cx,
            |this, _, _, cx| {
                if this.settings_cfg.core.antivirus_custom_mode {
                    this.av_preset_anim_epoch = this.av_preset_anim_epoch.wrapping_add(1);
                }
                this.settings_cfg.core.antivirus_custom_mode = false;
                if this.set_av_path.read(cx).text().trim().is_empty() {
                    let presets = detect_antivirus_presets();
                    let cur = this.settings_cfg.core.antivirus_uid.clone();
                    if let Some((uid, _, bin, args, _)) = presets
                        .iter()
                        .find(|(uid, _, _, _, _)| *uid == cur)
                        .or_else(|| presets.iter().find(|(_, _, _, _, exists)| *exists))
                        .or_else(|| presets.first())
                    {
                        this.settings_cfg.core.antivirus_uid = (*uid).into();
                        this.set_av_path.update(cx, |i, cx| i.set_text(bin, cx));
                        this.set_av_args.update(cx, |i, cx| i.set_text(args, cx));
                    }
                }
                cx.notify();
            },
        ))
        .when(!custom_mode, |el| {
            el.child(
                div()
                    .id("av-presets-box")
                    .pl_6()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .children(presets.into_iter().map(|(uid, name, bin, args, exists)| {
                        let sel = cur_uid == uid;
                        div()
                            .id(SharedString::from(format!("av-preset-{uid}")))
                            .p_2p5()
                            .rounded(p.radius(8., 3.))
                            .bg(if sel { p.accent_soft } else { p.bg_200 })
                            .border_1()
                            .border_color(if sel { p.accent } else { p.separator })
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings_cfg.core.antivirus_uid = uid.into();
                                this.set_av_path.update(cx, |i, cx| i.set_text(bin, cx));
                                this.set_av_args.update(cx, |i, cx| i.set_text(args, cx));
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text_primary)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(p.fpx(11.))
                                            .text_color(p.text_muted)
                                            .child(bin),
                                    ),
                            )
                            .child(badge_themed(
                                if exists { "Found" } else { "Not Installed" },
                                if exists { p.success } else { p.text_muted },
                                p.bg_300,
                                p,
                            ))
                    }))
                    .with_animation(
                        SharedString::from(format!("av-presets-anim-{av_epoch}")),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta).mt(px(-4.0 * (1.0 - delta))),
                    ),
            )
        })
        .child(radio_row(
            "av-mode-custom",
            "Custom Antivirus Command",
            Some("Specify any local scanner executable & argument template".into()),
            custom_mode,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.antivirus_custom_mode = true;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_3()
                .child(labeled_field(
                    "Scanner executable path",
                    app.set_av_path.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "av-browse-bin",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_file_into(
                            this.set_av_path.clone(),
                            "Choose Antivirus Executable",
                            cx,
                        );
                    },
                ))
                .child(labeled_field(
                    "Command arguments ({file} placeholder)",
                    app.set_av_args.clone().into_any_element(),
                    p,
                )),
        )
        .into_any_element()
}

fn render_bittorrent_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_core = &app.settings_cfg.core;
    let cfg_app = &app.settings_cfg.app;
    let enc = cfg_core.bt_encryption_policy;

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.bittorrent_title),
        )
        .child(section_heading(s.sec_bt_port_enc, p))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_3()
                .child(labeled_field(
                    "Incoming listen port",
                    app.set_bt_port.clone().into_any_element(),
                    p,
                ))
                .child(switch_row_desc(
                    "bt-rand-port",
                    "Use random port on startup",
                    None,
                    cfg_core.bt_use_random_port,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.settings_cfg.core.bt_use_random_port =
                            !this.settings_cfg.core.bt_use_random_port;
                        cx.notify();
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(div().text_xs().text_color(p.text_primary).child("Peer traffic encryption"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([
                            (BtEncryptionPolicy::PreferEncryption, "Prefer"),
                            (BtEncryptionPolicy::RequireEncryption, "Require"),
                            (BtEncryptionPolicy::NoEncryptionAllowed, "Allow Unencrypted"),
                        ].into_iter().map(|(pol, lbl)| {
                            chip_toggle(
                                format!("bt-enc-{pol:?}"),
                                lbl,
                                enc == pol,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.settings_cfg.core.bt_encryption_policy = pol;
                                    cx.notify();
                                },
                            )
                        })),
                ),
        )
        .child(section_heading(s.sec_bt_protocol, p))
        .child(
            div()
                .text_size(p.fpx(11.))
                .text_color(p.text_muted)
                .child("Applied globally when the BitTorrent session initializes or restarts; existing active torrents retain current session capabilities."),
        )
        .child(switch_row_desc(
            "bt-dht",
            "Enable DHT (Distributed Hash Table)",
            None,
            cfg_core.bt_enable_dht,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_dht = !this.settings_cfg.core.bt_enable_dht;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "bt-utp",
            "Enable uTP (Micro Transport Protocol)",
            None,
            cfg_core.bt_enable_utp,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_utp = !this.settings_cfg.core.bt_enable_utp;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "bt-lsd",
            "Enable Local Peer Discovery (LSD)",
            None,
            cfg_core.bt_enable_lsd,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_lsd = !this.settings_cfg.core.bt_enable_lsd;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "bt-upnp",
            "Enable UPnP / NAT-PMP port mapping",
            None,
            cfg_core.bt_enable_port_forwarding,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_port_forwarding =
                    !this.settings_cfg.core.bt_enable_port_forwarding;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "bt-pex",
            "Enable Peer Exchange (PEX)",
            None,
            cfg_core.bt_enable_pex,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_pex = !this.settings_cfg.core.bt_enable_pex;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_bt_watch, p))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "Automatically load .torrent files from folder",
                    app.set_bt_watch_dir.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "bt-browse-watch",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_folder_into(
                            this.set_bt_watch_dir.clone(),
                            "Choose .torrent Watch Folder",
                            cx,
                        );
                    },
                )),
        )
        .child(switch_row_desc(
            "bt-watch-silent",
            "Start torrents from watch folder silently without dialog",
            None,
            cfg_app.force_silent_from_torrent_folder,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.force_silent_from_torrent_folder =
                    !this.settings_cfg.app.force_silent_from_torrent_folder;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "bt-tr-enable",
            "Append additional trackers to new BitTorrent downloads",
            None,
            cfg_core.bt_enable_tracker_list,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.bt_enable_tracker_list =
                    !this.settings_cfg.core.bt_enable_tracker_list;
                cx.notify();
            },
        ))
        .child(labeled_field(
            "Additional tracker announce URLs (comma/space separated)",
            app.set_bt_trackers.clone().into_any_element(),
            p,
        ))
        .into_any_element()
}

fn render_remote_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_app = &app.settings_cfg.app;
    let srv_id = cfg_app.remote_control_server_id.clone();
    let srv_id_copy = srv_id.clone();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.remote_access_title),
        )
        .child(section_heading(s.sec_remote_server, p))
        .child(switch_row_desc(
            "rem-enable",
            "Enable local Remote Control server",
            Some("Control downloads over LAN using companion client & QR code pairing (zero cloud relay)".into()),
            cfg_app.enable_remote_access,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enable_remote_access =
                    !this.settings_cfg.app.enable_remote_access;
                cx.notify();
            },
        ))
        .child(
            div()
                .p_3()
                .rounded(p.radius(8., 3.))
                .bg(p.bg_200)
                .border_1()
                .border_color(p.separator)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(p.fpx(11.)).text_color(p.text_muted).child("Server Instance ID"))
                        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.text_primary).child(srv_id)),
                )
                .child(action_btn(
                    "rem-copy-id",
                    "Copy ID",
                    p,
                    false,
                    cx,
                    move |this, _, _, cx| {
                        this.copy_to_clipboard(srv_id_copy.clone(), "Copied Server ID", cx);
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "Bind address",
                    app.set_remote_bind.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Port",
                    app.set_remote_port.clone().into_any_element(),
                    p,
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "Access password",
                    password_field_row(
                        "rem-pass-eye",
                        app.set_remote_pass.clone().into_any_element(),
                        app.show_remote_pass,
                        p,
                        cx,
                        |this, _, _, cx| {
                            this.show_remote_pass = !this.show_remote_pass;
                            let masked = !this.show_remote_pass;
                            this.set_remote_pass
                                .update(cx, |i, cx| i.set_masked(masked, cx));
                            cx.notify();
                        },
                    )
                    .into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "rem-regen-pw",
                    "Regenerate",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.regenerate_remote_password_action(cx);
                    },
                ))
                .child(action_btn(
                    "rem-copy-pw",
                    "Copy",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        let pw = this.set_remote_pass.read(cx).text();
                        this.copy_to_clipboard(pw, "Copied Remote Access password", cx);
                    },
                )),
        )
        .child(switch_row_desc(
            "rem-bypass-proxy",
            "Bypass configured proxy for Remote Access traffic",
            None,
            cfg_app.proxy.dont_use_for_remote_access,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.proxy.dont_use_for_remote_access =
                    !this.settings_cfg.app.proxy.dont_use_for_remote_access;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "rem-bypass-listener",
            "Bypass proxy when listening for incoming remote connections",
            None,
            cfg_app.proxy.dont_use_for_remote_access_listener,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.proxy.dont_use_for_remote_access_listener =
                    !this.settings_cfg.app.proxy.dont_use_for_remote_access_listener;
                cx.notify();
            },
        ))
        .child(
            div().pt_1().child(action_btn(
                "rem-show-qr",
                "Show Remote Pairing QR Code...",
                p,
                true,
                cx,
                |this, _, _, cx| {
                    this.open_remote_qr_modal(cx);
                },
            )),
        )
        .into_any_element()
}

fn render_advanced_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_app = app.settings_cfg.app.clone();
    let cfg_core = app.settings_cfg.core.clone();
    let hidden_tags = cfg_app.hidden_builtin_tags.clone();
    let builtin_tags: Vec<_> = app.tags.iter().filter(|t| t.is_builtin).cloned().collect();
    let day_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_size(p.fpx(18.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_primary)
                .child(s.advanced_title),
        )
        .child(section_heading(s.sec_notifications, p))
        .child(switch_row_desc(
            "adv-not-add",
            "Notify when download is added",
            None,
            cfg_app.notify_added,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.notify_added = !this.settings_cfg.app.notify_added;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-not-fin",
            "Notify when downloads complete",
            None,
            cfg_app.notify_finished,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.notify_finished = !this.settings_cfg.app.notify_finished;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-not-fail",
            "Notify on download errors",
            None,
            cfg_app.notify_failed,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.notify_failed = !this.settings_cfg.app.notify_failed;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-not-inact",
            "Only show notifications when app window is inactive",
            None,
            cfg_app.notify_when_inactive_only,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.notify_when_inactive_only =
                    !this.settings_cfg.app.notify_when_inactive_only;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-sound",
            "Play notification sound events",
            None,
            cfg_app.enable_sound_notifications,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enable_sound_notifications =
                    !this.settings_cfg.app.enable_sound_notifications;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "Completion sound file (.aiff / .wav — empty = macOS Glass)",
                    app.set_notify_sound_path.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "adv-sound-browse",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_file_into(
                            this.set_notify_sound_path.clone(),
                            "Choose Notification Sound",
                            cx,
                        );
                    },
                ))
                .child(action_btn(
                    "adv-sound-test",
                    "Test Sound",
                    p,
                    false,
                    cx,
                    |this, _, _, _cx| {
                        let raw = this.set_notify_sound_path.read(_cx).text();
                        let p = raw.trim();
                        if p.is_empty() {
                            play_notification_sound(None);
                        } else {
                            play_notification_sound(Some(std::path::Path::new(p)));
                        }
                    },
                )),
        )
        .child(section_heading(s.sec_power_sched, p))
        .child(switch_row_desc(
            "adv-sleep-prev",
            "Prevent system sleep while downloads are running",
            None,
            cfg_core.prevent_sleep_if_running,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.prevent_sleep_if_running =
                    !this.settings_cfg.core.prevent_sleep_if_running;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-sleep-ign-fin",
            "Do not prevent sleep when only finished/seeding items remain",
            None,
            cfg_core.prevent_sleep_ignore_finished,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.prevent_sleep_ignore_finished =
                    !this.settings_cfg.core.prevent_sleep_ignore_finished;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-sleep-ign-resum",
            "Do not prevent sleep when all running downloads are resumable",
            None,
            cfg_core.prevent_sleep_ignore_resumable,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.prevent_sleep_ignore_resumable =
                    !this.settings_cfg.core.prevent_sleep_ignore_resumable;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-sleep-inc-sched",
            "Keep awake when scheduled downloads are pending",
            None,
            cfg_core.prevent_sleep_include_scheduled,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.prevent_sleep_include_scheduled =
                    !this.settings_cfg.core.prevent_sleep_include_scheduled;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-batt-post",
            "Pause heavy post-processing (FFmpeg) on low battery",
            None,
            cfg_core.disable_post_finished_on_battery,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.disable_post_finished_on_battery =
                    !this.settings_cfg.core.disable_post_finished_on_battery;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-post-fin-disable",
            "Disable post-download actions (shutdown & auto-launch)",
            None,
            cfg_core.disable_post_finished_tasks,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.disable_post_finished_tasks =
                    !this.settings_cfg.core.disable_post_finished_tasks;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "adv-sched-def",
            "Apply default active schedule window to new downloads",
            None,
            cfg_core.enable_scheduler_for_new_downloads,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.enable_scheduler_for_new_downloads =
                    !this.settings_cfg.core.enable_scheduler_for_new_downloads;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "Schedule start (HH:MM)",
                    app.set_sched_start.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Schedule end (HH:MM)",
                    app.set_sched_end.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Min battery %",
                    app.set_battery_min.clone().into_any_element(),
                    p,
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1p5()
                .children(day_names.into_iter().enumerate().map(|(i, dn)| {
                    let bit = 1u8 << i;
                    let on = (cfg_core.default_schedule.days_mask & bit) != 0;
                    chip_toggle(
                        format!("adv-day-{i}"),
                        dn,
                        on,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.settings_cfg.core.default_schedule.days_mask ^= bit;
                            cx.notify();
                        },
                    )
                })),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().text_xs().text_color(p.text_primary).child("Auto shutdown when queue completes"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .children([
                            (ShutdownType::None, "Off"),
                            (ShutdownType::Sleep, "Sleep"),
                            (ShutdownType::Hibernate, "Hibernate"),
                            (ShutdownType::Shutdown, "Shutdown"),
                            (ShutdownType::QuitApp, "Quit"),
                        ].into_iter().map(|(st, lbl)| {
                            chip_toggle(
                                format!("adv-sd-{st:?}"),
                                lbl,
                                cfg_core.shutdown_when_done == st,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.settings_cfg.core.shutdown_when_done = st;
                                    cx.notify();
                                },
                            )
                        })),
                ),
        )
        .child(section_heading(s.sec_post_tools, p))
        .child(switch_row_desc(
            "adv-ext-run",
            "Run external application/script on download completion",
            None,
            cfg_core.launch_external_on_completion,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.core.launch_external_on_completion =
                    !this.settings_cfg.core.launch_external_on_completion;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "External program path",
                    app.set_ext_path.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "adv-browse-ext",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_file_into(
                            this.set_ext_path.clone(),
                            "Choose External Application",
                            cx,
                        );
                    },
                ))
                .child(labeled_field(
                    "Arguments ({file})",
                    app.set_ext_args.clone().into_any_element(),
                    p,
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    "FFmpeg executable path",
                    app.set_ffmpeg_path.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "adv-browse-ffmpeg",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_file_into(
                            this.set_ffmpeg_path.clone(),
                            "Choose FFmpeg Executable",
                            cx,
                        );
                    },
                ))
                .child(labeled_field(
                    "yt-dlp executable path",
                    app.set_ytdlp_path.clone().into_any_element(),
                    p,
                ))
                .child(action_btn(
                    "adv-browse-ytdlp",
                    s.browse,
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.browse_file_into(
                            this.set_ytdlp_path.clone(),
                            "Choose yt-dlp Executable",
                            cx,
                        );
                    },
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(labeled_field(
                    "Low disk space floor (MB)",
                    app.set_low_disk_mb.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Min free disk %",
                    app.set_low_disk_pct.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "DB backup interval (mins)",
                    app.set_backup_interval_m.clone().into_any_element(),
                    p,
                ))
                .child(labeled_field(
                    "Max batch URLs",
                    app.set_batch_max_urls.clone().into_any_element(),
                    p,
                )),
        )
        .child(section_heading(s.sec_tag_visibility, p))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1p5()
                .children(builtin_tags.into_iter().map(|t| {
                    let tid = t.id;
                    let visible = !hidden_tags.contains(&tid);
                    let dot = parse_hex_color(&t.color_hex);
                    div()
                        .id(SharedString::from(format!("adv-htag-{tid}")))
                        .px_2p5()
                        .py(p.spx(4.))
                        .rounded(p.radius(6., 2.))
                        .bg(if visible { p.accent_soft } else { p.bg_200 })
                        .border_1()
                        .border_color(if visible { p.accent } else { p.border })
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let list = &mut this.settings_cfg.app.hidden_builtin_tags;
                            if let Some(pos) = list.iter().position(|&x| x == tid) {
                                list.remove(pos);
                            } else {
                                list.push(tid);
                            }
                            cx.notify();
                        }))
                        .child(fdm_checkbox(visible, p))
                        .child(div().w(p.spx(7.)).h(p.spx(7.)).rounded(p.radius(2., 1.)).bg(dot))
                        .child(div().text_xs().text_color(p.text_primary).child(t.name))
                })),
        )
        .child(section_heading(s.sec_backup_migration, p))
        .child(labeled_field(
            "Import / Export path (.json, .txt, .csv, or legacy db.sqlite)",
            app.set_data_io_path.clone().into_any_element(),
            p,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(action_btn(
                    "adv-exp-dl",
                    "Export Downloads",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.export_downloads_action(cx);
                    },
                ))
                .child(action_btn(
                    "adv-imp-dl",
                    "Import / Migrate Legacy FDM DB",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.import_downloads_action(cx);
                    },
                ))
                .child(action_btn(
                    "adv-exp-cfg",
                    "Export Settings",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.export_settings_action(cx);
                    },
                ))
                .child(action_btn(
                    "adv-portable",
                    "Create Portable Version...",
                    p,
                    false,
                    cx,
                    |this, _, w, cx| {
                        this.open_portable_modal(w, cx);
                    },
                )),
        )
        .into_any_element()
}

fn render_troubleshooting_tab(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let s = l(&app.settings_cfg.app.locale_name);
    let cfg_app = &app.settings_cfg.app;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(p.fpx(18.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.text_primary)
                        .child(s.troubleshooting_title),
                )
                .child(badge_themed("Local Only · Zero Upload", p.success, p.bg_200, p)),
        )
        .child(section_heading(s.sec_diag_logging, p))
        .child(switch_row_desc(
            "trb-log",
            "Write engine logs locally to logs/ferrite.log",
            None,
            cfg_app.enable_logging,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enable_logging = !this.settings_cfg.app.enable_logging;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "trb-enh",
            "Enhanced verbose HTTP & parser logging",
            None,
            cfg_app.enhanced_logging,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.enhanced_logging = !this.settings_cfg.app.enhanced_logging;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "trb-bt",
            "BitTorrent swarm alerts logging",
            None,
            cfg_app.lt_alerts_logging,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.lt_alerts_logging =
                    !this.settings_cfg.app.lt_alerts_logging;
                cx.notify();
            },
        ))
        .child(switch_row_desc(
            "trb-dlm",
            "Downloads queue management state logging",
            None,
            cfg_app.downloads_mgmt_logging,
            p,
            cx,
            |this, _, _, cx| {
                this.settings_cfg.app.downloads_mgmt_logging =
                    !this.settings_cfg.app.downloads_mgmt_logging;
                cx.notify();
            },
        ))
        .child(section_heading(s.sec_diag_archive, p))
        .child(labeled_field(
            "Archive destination .zip path",
            app.set_diag_zip_path.clone().into_any_element(),
            p,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(action_btn(
                    "trb-zip-logs",
                    "Save Logs Archive (.zip)",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.archive_diagnostics_action(true, cx);
                    },
                ))
                .child(action_btn(
                    "trb-zip-full",
                    "Save Full Diagnostics Bundle (.zip)",
                    p,
                    true,
                    cx,
                    |this, _, _, cx| {
                        this.archive_diagnostics_action(false, cx);
                    },
                ))
                .child(action_btn(
                    "trb-open-dir",
                    "Open Data Folder",
                    p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        let d = this.engine.data_dir().to_path_buf();
                        if let Err(e) = open_path_os(&d) {
                            this.set_toast(format!("Open failed: {e}"), true);
                            cx.notify();
                        }
                    },
                )),
        )
        .into_any_element()
}
