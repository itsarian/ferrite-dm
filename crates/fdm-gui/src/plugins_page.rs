use crate::app::FerriteApp;
use crate::helpers::open_path_os;
use crate::i18n;
use crate::ui_widgets::{
    action_btn, badge_themed, danger_btn, ghost_icon_btn, svg_icon, switch_row_desc,
};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, SharedString, div,
    ease_out_quint, prelude::*, px,
};
use std::time::Duration;

pub fn render_plugins_page(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let plugins = app.settings_plugins.clone();
    let cfg_app = &app.settings_cfg.app;
    let plugins_dir_str = app.engine.plugins_dir().to_string_lossy().into_owned();
    let card_radius = p.radius(10., 3.);
    let gear_epoch = app.plugins_gear_anim_epoch;

    div()
        .flex_1()
        .min_h_0()
        .bg(p.bg_100)
        .flex()
        .flex_col()
        .child(
            // Header strip matching FDM PluginsPage.qml
            div()
                .px_6()
                .py_4()
                .bg(p.bg_200)
                .border_b_1()
                .border_color(p.separator)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .id("plugins-title-trigger")
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.plugins_title_clicks =
                                        this.plugins_title_clicks.saturating_add(1);
                                    if this.plugins_title_clicks >= 5 {
                                        this.plugins_title_clicks = 0;
                                        this.show_plugin_dev = !this.show_plugin_dev;
                                        this.set_toast(
                                            if this.show_plugin_dev {
                                                "Add-on developer options unlocked"
                                            } else {
                                                "Add-on developer options hidden"
                                            },
                                            false,
                                        );
                                    }
                                    cx.notify();
                                }))
                                .child(svg_icon("icons/plugin.svg", p.spx_f32(20.), p.accent))
                                .child(
                                    div()
                                        .text_size(p.fpx(20.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(p.text_primary)
                                        .child(s.addons),
                                )
                                .when(app.show_plugin_dev, |el| {
                                    el.child(badge_themed("Developer", p.warning, p.bg_300, &p))
                                }),
                        )
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "Manage site & media parser add-ons stored locally in {}",
                                    plugins_dir_str
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(action_btn(
                            "pl-open-dir",
                            s.open_folder,
                            &p,
                            false,
                            cx,
                            |this, _, _, cx| {
                                let dir = this.engine.plugins_dir().to_path_buf();
                                let _ = std::fs::create_dir_all(&dir);
                                if let Err(e) = open_path_os(&dir) {
                                    this.set_toast(format!("Failed to open folder: {e}"), true);
                                    cx.notify();
                                }
                            },
                        ))
                        .child(action_btn(
                            "pl-refresh",
                            s.reload,
                            &p,
                            false,
                            cx,
                            |this, _, _, cx| {
                                this.settings_plugins = this.engine.list_js_plugins();
                                this.set_toast(
                                    format!("Loaded {} add-on(s)", this.settings_plugins.len()),
                                    false,
                                );
                                cx.notify();
                            },
                        ))
                        .child(action_btn(
                            "pl-check-upd",
                            s.check_updates,
                            &p,
                            false,
                            cx,
                            |this, _, _, cx| {
                                this.check_plugin_updates_action(true, cx);
                            },
                        ))
                        .child(ghost_icon_btn(
                            "pl-gear",
                            "icons/settings_gear.svg",
                            p.spx_f32(18.),
                            if app.plugins_gear_menu_open {
                                p.accent
                            } else {
                                p.text_secondary
                            },
                            p.bg_300,
                            cx,
                            |this, _, _, cx| {
                                this.plugins_gear_menu_open = !this.plugins_gear_menu_open;
                                if this.plugins_gear_menu_open {
                                    this.plugins_gear_anim_epoch =
                                        this.plugins_gear_anim_epoch.wrapping_add(1);
                                }
                                cx.notify();
                            },
                        )),
                ),
        )
        .when(app.plugins_gear_menu_open, |el| {
            let gear_base_mt = p.spx_f32(12.);
            el.child(
                div()
                    .id("plugins-gear-drawer")
                    .mx_6()
                    .p_3()
                    .rounded(card_radius)
                    .bg(p.bg_200)
                    .border_1()
                    .border_color(p.border)
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(p.fpx(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.text_muted)
                            .child(s.addon_settings),
                    )
                    .child(switch_row_desc(
                        "pl-opt-autoupd",
                        "Automatically check & update compatible add-ons",
                        Some("Checks configured add-on manifest endpoints locally".into()),
                        cfg_app.update_plugins_automatically,
                        &p,
                        cx,
                        |this, _, _, cx| {
                            this.settings_cfg.app.update_plugins_automatically =
                                !this.settings_cfg.app.update_plugins_automatically;
                            let _ = this.engine.update_config(this.settings_cfg.clone());
                            cx.notify();
                        },
                    ))
                    .child(switch_row_desc(
                        "pl-opt-cookies",
                        "Pass browser session cookies to site parser scripts",
                        Some("Allows authenticated site parsing using browser cookies".into()),
                        cfg_app.plugins_allow_wb_cookies,
                        &p,
                        cx,
                        |this, _, _, cx| {
                            this.settings_cfg.app.plugins_allow_wb_cookies =
                                !this.settings_cfg.app.plugins_allow_wb_cookies;
                            let _ = this.engine.update_config(this.settings_cfg.clone());
                            cx.notify();
                        },
                    ))
                    .when(app.show_plugin_dev, |dev| {
                        dev.child(switch_row_desc(
                            "pl-opt-dev-unsign",
                            "Ignore add-on signature checks (Developer)",
                            Some("Permit unsigned local .fda / .js add-ons for testing".into()),
                            cfg_app.plugins_dev_ignore_sign,
                            &p,
                            cx,
                            |this, _, _, cx| {
                                this.settings_cfg.app.plugins_dev_ignore_sign =
                                    !this.settings_cfg.app.plugins_dev_ignore_sign;
                                let _ = this.engine.update_config(this.settings_cfg.clone());
                                cx.notify();
                            },
                        ))
                    })
                    .with_animation(
                        SharedString::from(format!("pl-gear-anim-{gear_epoch}")),
                        Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                        move |el, delta| {
                            let slide = -8.0 * (1.0 - delta);
                            el.opacity(delta).mt(px(gear_base_mt + slide))
                        },
                    ),
            )
        })
        .child(
            // Install from local file bar
            div()
                .mx_6()
                .mt_4()
                .p_3()
                .rounded(card_radius)
                .bg(p.bg_200)
                .border_1()
                .border_color(p.border)
                .flex()
                .flex_row()
                .items_center()
                .gap_2p5()
                .child(div().flex_1().child(app.plugin_install_input.clone()))
                .child(action_btn(
                    "pl-install-btn",
                    s.install_addon,
                    &p,
                    true,
                    cx,
                    |this, _, _, cx| {
                        this.install_plugin_from_input(cx);
                    },
                )),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id("plugins-scroll")
                .overflow_y_scroll()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .when(plugins.is_empty(), |el| {
                    el.child(
                        div()
                            .p_8()
                            .rounded(p.radius(12., 4.))
                            .bg(p.bg_200)
                            .border_1()
                            .border_color(p.separator)
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .child(svg_icon("icons/plugin.svg", p.spx_f32(36.), p.text_muted))
                            .child(
                                div()
                                    .text_size(p.fpx(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text_primary)
                                    .child(s.no_addons_installed),
                            )
                            .child(
                                div()
                                    .text_size(p.fpx(12.))
                                    .text_color(p.text_muted)
                                    .child(s.addons_empty_hint),
                            ),
                    )
                })
                .children(plugins.into_iter().map(|pl| {
                    let uid = pl.uid.clone();
                    let uid_rm = pl.uid.clone();
                    let enabled = pl.enabled;
                    div()
                        .p_4()
                        .rounded(card_radius)
                        .bg(p.bg_200)
                        .border_1()
                        .border_color(p.border)
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .child(svg_icon("icons/plugin.svg", p.spx_f32(16.), p.accent))
                                        .child(
                                            div()
                                                .text_size(p.fpx(14.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(p.text_primary)
                                                .child(pl.name),
                                        )
                                        .child(badge_themed(
                                            format!("v{}", pl.version),
                                            p.accent,
                                            p.accent_soft,
                                            &p,
                                        ))
                                        .when(!enabled, |el| {
                                            el.child(badge_themed(
                                                "Disabled",
                                                p.text_muted,
                                                p.bg_300,
                                                &p,
                                            ))
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(p.fpx(12.))
                                        .text_color(p.text_secondary)
                                        .child(if pl.description.is_empty() {
                                            "Local JavaScript media/site extractor add-on".into()
                                        } else {
                                            pl.description
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(p.fpx(11.))
                                                .text_color(p.text_muted)
                                                .child(format!("Author: {} · UID: {}", pl.author, pl.uid)),
                                        )
                                        .children(pl.domains.into_iter().take(6).map(|d| {
                                            badge_themed(d, p.text_secondary, p.bg_300, &p)
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .flex_shrink_0()
                                .child(action_btn(
                                    SharedString::from(format!("pl-tog-{}", pl.uid)),
                                    if enabled { s.disable } else { s.enable },
                                    &p,
                                    !enabled,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.toggle_plugin_enabled(uid.clone(), !enabled, cx);
                                    },
                                ))
                                .child(danger_btn(
                                    SharedString::from(format!("pl-rm-{}", pl.uid)),
                                    s.uninstall,
                                    &p,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.uninstall_plugin_action(uid_rm.clone(), cx);
                                    },
                                )),
                        )
                })),
        )
        .into_any_element()
}
