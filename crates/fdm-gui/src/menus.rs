use crate::app::{ActivePage, ActivePopover, FerriteApp, SettingsTab};
use crate::helpers::{
    count_by_state_filter, format_speed, sort_field_label, state_filter_label,
};
use crate::i18n;
use crate::theme::{ThemePalette, parse_hex_color};
use crate::ui_widgets::{badge_themed, chip_toggle, ellipsis_text, svg_icon};
use fdm_core::{
    DownloadPriority, DownloadState, DownloadType, ShutdownType, SortField, StateFilter,
    TrafficUsageMode,
};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, ElementId, FontWeight, MouseButton,
    SharedString, Window, div, ease_out_quint, prelude::*, px,
};
use std::sync::Arc;
use std::time::Duration;

pub fn menu_item_row<F>(
    id: impl Into<SharedString>,
    icon: &'static str,
    label: impl Into<String>,
    shortcut: Option<&'static str>,
    active: bool,
    danger: bool,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    handler: F,
) -> gpui::Stateful<gpui::Div>
where
    F: Fn(&mut FerriteApp, &gpui::ClickEvent, &mut Window, &mut Context<FerriteApp>) + 'static,
{
    let fg = if danger {
        p.danger
    } else if active {
        p.accent
    } else {
        p.text_primary
    };
    let hover_bg = p.bg_elevated;
    let muted = p.text_muted;
    div()
        .id(ElementId::Name(id.into()))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .px_2p5()
        .py(p.spx(4.))
        .rounded(p.radius(6., 2.))
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(svg_icon(icon, p.spx_f32(14.), fg))
                .child(
                    div()
                        .text_size(p.fpx(12.))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(fg)
                        .truncate()
                        .child(label.into()),
                ),
        )
        .when_some(shortcut, |el, sc| {
            el.child(div().text_size(p.fpx(11.)).text_color(muted).child(sc))
        })
}

pub fn render_popover(
    app: &mut FerriteApp,
    cx: &mut Context<FerriteApp>,
) -> Option<AnyElement> {
    let p = app.theme;
    let pop = app.active_popover.clone();
    if pop == ActivePopover::None {
        return None;
    }
    let pop_epoch = app.popover_anim_epoch;

    let card_shell = |card: gpui::Div| -> gpui::Div {
        card.bg(p.bg_surface)
            .border_1()
            .border_color(p.border)
            .rounded(p.radius(10., 3.))
            .shadow_xl()
            .p_1p5()
            .flex()
            .flex_col()
            .gap(px(2.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
    };

    let positioned_card: AnyElement = match pop {
        ActivePopover::None => return None,
        ActivePopover::SortMenu => render_sort_menu(app, &p, cx, card_shell),
        ActivePopover::FilterMenu => render_filter_menu(app, &p, cx, card_shell),
        ActivePopover::TumMenu => render_tum_menu(app, &p, cx, card_shell),
        ActivePopover::MainMenu => render_main_menu(app, &p, cx, card_shell),
        ActivePopover::RowContextMenu(id) => {
            render_row_context_menu(app, id, &p, cx, card_shell)?
        }
    };

    let anim_layer = div()
        .id("popover-anim-layer")
        .absolute()
        .inset_0()
        .child(positioned_card)
        .with_animation(
            SharedString::from(format!("popover-enter-{pop_epoch}")),
            Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
            |el, delta| {
                let offset = 6.0 * (1.0 - delta);
                el.opacity(delta).top(px(offset))
            },
        );

    Some(
        div()
            .id("popover-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.close_popover(cx);
            }))
            .child(anim_layer)
            .into_any_element(),
    )
}

fn render_sort_menu(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    card_shell: impl FnOnce(gpui::Div) -> gpui::Div,
) -> AnyElement {
    let fields = [
        SortField::CreationTime,
        SortField::Title,
        SortField::Size,
        SortField::Progress,
        SortField::Status,
        SortField::Priority,
        SortField::Order,
    ];
    card_shell(div().w(p.spx(220.)))
        .child(
            div()
                .id("pop-sort-dir")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_2p5()
                .py(p.spx(6.))
                .rounded(p.radius(6., 2.))
                .bg(p.bg_input)
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sort_ascending = !this.sort_ascending;
                    this.persist_sort(cx);
                    this.refresh_snapshot(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(p.fpx(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_primary)
                        .child(if app.sort_ascending {
                            "Order: Ascending"
                        } else {
                            "Order: Descending"
                        }),
                )
                .child(svg_icon(
                    if app.sort_ascending {
                        "icons/arrow_up.svg"
                    } else {
                        "icons/arrow_down.svg"
                    },
                    p.spx_f32(13.),
                    p.accent,
                )),
        )
        .child(div().h(px(1.)).bg(p.separator).my_1())
        .children(fields.into_iter().map(|sf| {
            let active = app.sort_by == sf;
            menu_item_row(
                format!("pop-sf-{sf:?}"),
                if active {
                    "icons/check.svg"
                } else {
                    "icons/sort.svg"
                },
                sort_field_label(sf),
                None,
                active,
                false,
                p,
                cx,
                move |this, _, _, cx| {
                    this.sort_by = sf;
                    this.active_popover = ActivePopover::None;
                    this.persist_sort(cx);
                    this.refresh_snapshot(cx);
                    cx.notify();
                },
            )
        }))
        .absolute()
        .top(p.spx(74.))
        .left(p.spx(54.))
        .into_any_element()
}

fn render_filter_menu(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    card_shell: impl FnOnce(gpui::Div) -> gpui::Div,
) -> AnyElement {
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let filters = [
        StateFilter::All,
        StateFilter::Running,
        StateFilter::Downloading,
        StateFilter::Uploading,
        StateFilter::Finished,
        StateFilter::Stopped,
        StateFilter::Scheduled,
        StateFilter::Missing,
    ];
    let filter_rows: Vec<AnyElement> = filters
        .into_iter()
        .map(|f| {
            let count = count_by_state_filter(&app.all_downloads, f);
            let active = app.active_filter == f && app.active_tag_id.is_none();
            div()
                .id(SharedString::from(format!("pop-flt-{f:?}")))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_2p5()
                .py(p.spx(4.))
                .rounded(p.radius(6., 2.))
                .bg(if active {
                    p.accent_soft
                } else {
                    gpui::transparent_black()
                })
                .hover(|s| s.bg(p.bg_elevated))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_filter = f;
                    this.active_tag_id = None;
                    this.active_popover = ActivePopover::None;
                    this.refresh_snapshot(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .when(active, |el| {
                            el.child(svg_icon("icons/check.svg", p.spx_f32(12.), p.accent))
                        })
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(p.text_primary)
                                .child(state_filter_label(f)),
                        ),
                )
                .child(
                    div()
                        .text_size(p.fpx(12.))
                        .text_color(p.text_muted)
                        .child(count.to_string()),
                )
                .into_any_element()
        })
        .collect();

    let hidden = app.settings_cfg.app.hidden_builtin_tags.clone();
    let tag_rows: Vec<AnyElement> = app
        .tags
        .iter()
        .filter(|t| !(t.is_builtin && hidden.contains(&t.id)))
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .map(|tag| {
            let tid = tag.id;
            let active = app.active_tag_id == Some(tid);
            let dot_c = parse_hex_color(&tag.color_hex);
            let tag_count = app
                .all_downloads
                .iter()
                .filter(|d| d.tags.contains(&tid))
                .count();
            let builtin = tag.is_builtin;
            div()
                .id(SharedString::from(format!("pop-tag-{tid}")))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2p5()
                .py(p.spx(4.))
                .rounded(p.radius(6., 2.))
                .bg(if active {
                    p.accent_soft
                } else {
                    gpui::transparent_black()
                })
                .hover(|s| s.bg(p.bg_elevated))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_tag_id = Some(tid);
                    this.active_filter = StateFilter::All;
                    this.active_popover = ActivePopover::None;
                    this.refresh_snapshot(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(div().w(p.spx(9.)).h(p.spx(9.)).rounded(px(3.)).bg(dot_c))
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .text_color(p.text_primary)
                                .truncate()
                                .child(tag.name),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .text_color(p.text_muted)
                                .child(tag_count.to_string()),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("pop-edit-tag-{tid}")))
                                .w(p.spx(20.))
                                .h(p.spx(20.))
                                .rounded(p.radius(5., 2.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(p.bg_input))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    cx.stop_propagation();
                                    this.active_popover = ActivePopover::None;
                                    this.open_edit_tag_modal(Some(tid), w, cx);
                                }))
                                .child(svg_icon("icons/edit.svg", p.spx_f32(11.), p.text_muted)),
                        )
                        .when(!builtin, |el| {
                            el.child(
                                div()
                                    .id(SharedString::from(format!("pop-del-tag-{tid}")))
                                    .w(p.spx(20.))
                                    .h(p.spx(20.))
                                    .rounded(p.radius(5., 2.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.bg(p.bg_input))
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ = this.engine.delete_tag(tid);
                                        if this.active_tag_id == Some(tid) {
                                            this.active_tag_id = None;
                                        }
                                        this.refresh_snapshot(cx);
                                        cx.notify();
                                    }))
                                    .child(svg_icon("icons/close.svg", p.spx_f32(11.), p.danger)),
                            )
                        }),
                )
                .into_any_element()
        })
        .collect();

    card_shell(div().w(p.spx(280.)).max_h(p.spx(680.)))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id("pop-flt-scroll")
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .px_2()
                        .py(px(3.))
                        .text_size(p.fpx(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.text_muted)
                        .child(s.menu_sec_states),
                )
                .children(filter_rows)
                .child(div().h(px(1.)).bg(p.separator).my_1())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py(px(3.))
                        .child(
                            div()
                                .text_size(p.fpx(11.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(p.text_muted)
                                .child(s.menu_sec_tags),
                        )
                        .child(
                            div()
                                .id("pop-new-tag")
                                .px_2()
                                .py(px(2.))
                                .rounded(p.radius(5., 2.))
                                .bg(p.accent_soft)
                                .text_size(p.fpx(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.accent)
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, w, cx| {
                                    this.active_popover = ActivePopover::None;
                                    this.open_edit_tag_modal(None, w, cx);
                                }))
                                .child(s.menu_new_tag),
                        ),
                )
                .children(tag_rows),
        )
        .absolute()
        .top(p.spx(74.))
        .left(p.spx(188.))
        .into_any_element()
}

fn render_tum_menu(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    card_shell: impl FnOnce(gpui::Div) -> gpui::Div,
) -> AnyElement {
    let tum_profiles = app.settings_cfg.core.tum_profiles.clone();
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let current_tum = app.summary.tum;
    let modes = [
        (TrafficUsageMode::High, "High Speed"),
        (TrafficUsageMode::Medium, "Medium Speed"),
        (TrafficUsageMode::Low, "Low Speed"),
        (TrafficUsageMode::Snail, "Snail Mode"),
    ];
    card_shell(div().w(p.spx(265.)))
        .child(
            div()
                .px_2()
                .py(px(3.))
                .text_size(p.fpx(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text_muted)
                .child(s.menu_tum_header),
        )
        .children(modes.into_iter().map(|(mode, title)| {
            let active = current_tum == mode;
            let prof = tum_profiles.get(&mode);
            let desc = match prof {
                Some(pr) => {
                    let dl = if pr.max_download_speed_bps == 0 {
                        "∞".into()
                    } else {
                        format_speed(pr.max_download_speed_bps)
                    };
                    let ul = if pr.max_upload_speed_bps == 0 {
                        "∞".into()
                    } else {
                        format_speed(pr.max_upload_speed_bps)
                    };
                    format!("↓ {dl} · ↑ {ul} · {} conns", pr.max_connections)
                }
                None => "Balanced limits".into(),
            };
            div()
                .id(SharedString::from(format!("pop-tum-{mode:?}")))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2p5()
                .py(p.spx(6.))
                .rounded(p.radius(6., 2.))
                .bg(if active {
                    p.accent_soft
                } else {
                    gpui::transparent_black()
                })
                .hover(|s| s.bg(p.bg_elevated))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_popover = ActivePopover::None;
                    this.set_tum(mode, cx);
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(p.fpx(12.))
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(if active {
                                    p.accent
                                } else {
                                    p.text_primary
                                })
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(p.fpx(10.))
                                .text_color(p.text_muted)
                                .child(desc),
                        ),
                )
                .when(active, |el| el.child(svg_icon("icons/check.svg", p.spx_f32(13.), p.accent)))
        }))
        .child(div().h(px(1.)).bg(p.separator).my_1())
        .child(menu_item_row(
            "tum-prefs-link",
            "icons/settings.svg",
            "Configure Speed Profiles...",
            None,
            false,
            false,
            p,
            cx,
            |this, _, _, cx| {
                this.active_popover = ActivePopover::None;
                this.open_settings_page(SettingsTab::Traffic, cx);
            },
        ))
        .absolute()
        .bottom(p.spx(42.))
        .left(p.spx(58.))
        .into_any_element()
}

fn render_main_menu(
    app: &mut FerriteApp,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    card_shell: impl FnOnce(gpui::Div) -> gpui::Div,
) -> AnyElement {
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let cur_shutdown = app.settings_cfg.core.shutdown_when_done;
    let show_support = app.settings_cfg.app.show_support_button;
    let shutdown_expanded = app.shutdown_submenu_open;
    let sd_epoch = app.shutdown_sub_anim_epoch;
    let shutdown_label = match cur_shutdown {
        ShutdownType::None => "Off",
        ShutdownType::Sleep => "Sleep",
        ShutdownType::Hibernate => "Hibernate",
        ShutdownType::Shutdown => "Shut Down",
        ShutdownType::QuitApp => "Quit App",
    };

    card_shell(div().w(p.spx(275.)).max_h(p.spx(680.)))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id("main-menu-scroll")
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(menu_item_row(
                    "mm-add",
                    "icons/plus.svg",
                    s.menu_add_url,
                    Some("⌘N"),
                    false,
                    false,
                    p,
                    cx,
                    |this, _, w, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_add_modal("", w, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-batch",
                    "icons/batch.svg",
                    s.menu_batch,
                    Some("⌘B"),
                    false,
                    false,
                    p,
                    cx,
                    |this, _, w, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_batch_modal(w, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-paste",
                    "icons/clipboard.svg",
                    s.menu_paste_clipboard,
                    Some("⇧⌘V"),
                    false,
                    false,
                    p,
                    cx,
                    |this, _, w, cx| {
                        this.paste_from_clipboard_add(w, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-import-urls",
                    "icons/folder_open.svg",
                    "Import / Legacy FDM / Backup...",
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_settings_page(SettingsTab::Advanced, cx);
                    },
                ))
                .child(div().h(px(1.)).bg(p.separator).my_1())
                .child(menu_item_row(
                    "mm-start-all",
                    "icons/play.svg",
                    s.menu_start_all,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        let eng = Arc::clone(&this.engine);
                        this.rt.spawn(async move {
                            let _ = eng.start_all().await;
                        });
                        cx.notify();
                    },
                ))
                .child(menu_item_row(
                    "mm-pause-all",
                    "icons/pause.svg",
                    s.menu_pause_all,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        let eng = Arc::clone(&this.engine);
                        this.rt.spawn(async move {
                            let _ = eng.pause_all().await;
                        });
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .id("mm-shutdown-hdr")
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px_2p5()
                        .py(p.spx(4.5))
                        .rounded(p.radius(6., 2.))
                        .hover(|s| s.bg(p.bg_elevated))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shutdown_submenu_open = !this.shutdown_submenu_open;
                            if this.shutdown_submenu_open {
                                this.shutdown_sub_anim_epoch =
                                    this.shutdown_sub_anim_epoch.wrapping_add(1);
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(svg_icon("icons/hourglass.svg", p.spx_f32(14.), p.text_primary))
                                .child(
                                    div()
                                        .text_size(p.fpx(12.))
                                        .text_color(p.text_primary)
                                        .child(s.menu_auto_shutdown),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(badge_themed(
                                    shutdown_label,
                                    if cur_shutdown == ShutdownType::None {
                                        p.text_muted
                                    } else {
                                        p.warning
                                    },
                                    p.bg_input,
                                    p,
                                ))
                                .child(svg_icon(
                                    if shutdown_expanded {
                                        "icons/chevron_down.svg"
                                    } else {
                                        "icons/arrow_drop_right.svg"
                                    },
                                    p.spx_f32(13.),
                                    p.text_muted,
                                )),
                        ),
                )
                .when(shutdown_expanded, |el| {
                    let opts = [
                        (ShutdownType::None, "Turn Off (Do Nothing)"),
                        (ShutdownType::Sleep, "Sleep When Done"),
                        (ShutdownType::Hibernate, "Hibernate When Done"),
                        (ShutdownType::Shutdown, "Shut Down Computer"),
                        (ShutdownType::QuitApp, "Quit Ferrite DM"),
                    ];
                    el.child(
                        div()
                            .id("mm-sd-sub")
                            .pl_5()
                            .pr_1()
                            .py_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(opts.into_iter().map(|(st, lbl)| {
                                let act = cur_shutdown == st;
                                menu_item_row(
                                    format!("mm-sd-{st:?}"),
                                    if act {
                                        "icons/check.svg"
                                    } else {
                                        "icons/radio_unchecked.svg"
                                    },
                                    lbl,
                                    None,
                                    act,
                                    false,
                                    p,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.set_shutdown_when_done(st, cx);
                                        this.active_popover = ActivePopover::None;
                                    },
                                )
                            }))
                            .with_animation(
                                SharedString::from(format!("mm-sd-anim-{sd_epoch}")),
                                Animation::new(Duration::from_millis(140))
                                    .with_easing(ease_out_quint()),
                                |el, delta| el.opacity(delta).mt(px(-4.0 * (1.0 - delta))),
                            ),
                    )
                })
                .child(div().h(px(1.)).bg(p.separator).my_1())
                .child(menu_item_row(
                    "mm-addons",
                    "icons/plugin.svg",
                    s.menu_addons,
                    None,
                    app.active_page == ActivePage::Plugins,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_plugins_page(cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-prefs",
                    "icons/settings.svg",
                    s.menu_preferences,
                    Some("⌘,"),
                    matches!(app.active_page, ActivePage::Settings(_)),
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_settings_page(SettingsTab::General, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-qr",
                    "icons/qr.svg",
                    s.menu_remote_qr,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_remote_qr_modal(cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-portable",
                    "icons/folder.svg",
                    s.menu_create_portable,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, w, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_portable_modal(w, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-theme",
                    if p.is_dark {
                        "icons/sun.svg"
                    } else {
                        "icons/moon.svg"
                    },
                    if p.is_dark {
                        s.menu_switch_light
                    } else {
                        s.menu_switch_dark
                    },
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.toggle_theme(cx);
                    },
                ))
                .when(show_support, |el| {
                    el.child(menu_item_row(
                        "mm-support",
                        "icons/shield.svg",
                        s.menu_help_diagnostics,
                        None,
                        false,
                        false,
                        p,
                        cx,
                        |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.show_troubleshooting = true;
                            this.open_settings_page(SettingsTab::Troubleshooting, cx);
                        },
                    ))
                })
                .child(menu_item_row(
                    "mm-check-updates",
                    "icons/refresh.svg",
                    s.menu_check_updates,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.check_for_app_updates_action(true, cx);
                    },
                ))
                .child(menu_item_row(
                    "mm-about",
                    "icons/info.svg",
                    s.menu_about,
                    None,
                    false,
                    false,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.open_about_modal(cx);
                    },
                ))
                .child(div().h(px(1.)).bg(p.separator).my_1())
                .child(menu_item_row(
                    "mm-quit",
                    "icons/close.svg",
                    s.menu_quit,
                    Some("⌘Q"),
                    false,
                    true,
                    p,
                    cx,
                    |this, _, _, cx| {
                        this.active_popover = ActivePopover::None;
                        this.request_quit(cx);
                    },
                )),
        )
        .absolute()
        .top(p.spx(74.))
        .right(p.spx(16.))
        .into_any_element()
}

fn render_row_context_menu(
    app: &mut FerriteApp,
    id: fdm_core::DownloadId,
    p: &ThemePalette,
    cx: &mut Context<FerriteApp>,
    card_shell: impl FnOnce(gpui::Div) -> gpui::Div,
) -> Option<AnyElement> {
    let rec = app.all_downloads.iter().find(|d| d.id == id)?.clone();
    let is_running = rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued;
    let is_completed = rec.state == DownloadState::Completed;
    let is_bt = rec.download_type == DownloadType::BitTorrent;
    let is_batch = rec.download_type == DownloadType::Batch;
    let is_stream = matches!(
        rec.download_type,
        DownloadType::HlsM3u8 | DownloadType::Dash | DownloadType::MediaParser
    );
    let url_for_copy = rec.url.clone();
    let all_tags = app.tags.clone();

    Some(
        card_shell(div().w(p.spx(300.)).max_h(p.spx(718.)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .id("rcm-scroll")
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .px_2()
                            .py(px(3.))
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(p.fpx(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_muted)
                            .child(ellipsis_text(rec.title.clone())),
                    )
                    .child(div().h(px(1.)).bg(p.separator).my(px(2.)))
                    .when(is_batch, |el| {
                        el.child(menu_item_row(
                            "rcm-open-batch",
                            "icons/folder_open.svg",
                            format!("Open Batch ({} items)", rec.children_ids.len()),
                            None,
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.parent_folder_id = Some(id);
                                this.refresh_snapshot(cx);
                                cx.notify();
                            },
                        ))
                    })
                    .when(!is_completed && !is_batch, |el| {
                        el.child(menu_item_row(
                            "rcm-tog",
                            if is_running {
                                "icons/pause.svg"
                            } else {
                                "icons/play.svg"
                            },
                            if is_running {
                                "Pause Download"
                            } else {
                                "Resume / Start"
                            },
                            Some("Space"),
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.toggle_download(id);
                                cx.notify();
                            },
                        ))
                    })
                    .when(is_completed && !is_batch, |el| {
                        el.child(menu_item_row(
                            "rcm-open",
                            "icons/folder_open.svg",
                            "Open File",
                            None,
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.open_download_file(id);
                                cx.notify();
                            },
                        ))
                    })
                    .child(menu_item_row(
                        "rcm-rev",
                        "icons/folder.svg",
                        "Show in Finder",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.reveal_download_file(id);
                            cx.notify();
                        },
                    ))
                    .when(!is_batch, |el| {
                        el.child(menu_item_row(
                            "rcm-restart",
                            "icons/restart.svg",
                            "Restart Download",
                            None,
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.restart_download_action(id, cx);
                            },
                        ))
                    })
                    .when(is_stream && !is_completed, |el| {
                        el.child(menu_item_row(
                            "rcm-save-done",
                            "icons/check.svg",
                            "Save & Complete Stream",
                            None,
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.save_stream_complete_action(id, cx);
                            },
                        ))
                    })
                    .child(menu_item_row(
                        "rcm-insp",
                        "icons/speed.svg",
                        "Inspect Details Panel",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.selected_id = Some(id);
                            this.set_inspector_expanded(true, cx);
                        },
                    ))
                    .when(!is_batch && app.settings_cfg.app.enable_standalone_downloads_windows, |el| {
                        el.child(menu_item_row(
                            "rcm-standalone",
                            "icons/link.svg",
                            "Open in Standalone Window",
                            None,
                            false,
                            false,
                            p,
                            cx,
                            move |this, _, _, cx| {
                                this.active_popover = ActivePopover::None;
                                this.open_download_window(id, cx);
                            },
                        ))
                    })
                    .child(div().h(px(1.)).bg(p.separator).my_1())
                    .child(
                        div()
                            .px_2()
                            .py(px(4.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(p.fpx(12.))
                                    .text_color(p.text_muted)
                                    .child("Priority"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .children([
                                        (DownloadPriority::Low, "Low"),
                                        (DownloadPriority::Normal, "Normal"),
                                        (DownloadPriority::High, "High"),
                                    ].into_iter().map(|(pr, lbl)| {
                                        chip_toggle(
                                            format!("rcm-prio-{pr:?}"),
                                            lbl,
                                            rec.priority == pr,
                                            p,
                                            cx,
                                            move |this, _, _, cx| {
                                                this.set_priority_action(id, pr, cx);
                                            },
                                        )
                                    })),
                            ),
                    )
                    .child(menu_item_row(
                        "rcm-seq",
                        if rec.flags.sequential_play_asap {
                            "icons/check.svg"
                        } else {
                            "icons/play.svg"
                        },
                        "Sequential Play ASAP",
                        None,
                        rec.flags.sequential_play_asap,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            let next = !rec.flags.sequential_play_asap;
                            let eng = Arc::clone(&this.engine);
                            this.rt.spawn(async move {
                                let _ = eng.toggle_sequential_play_asap(id, next).await;
                            });
                            cx.notify();
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-autoretry",
                        if !rec.flags.disable_auto_retry {
                            "icons/check.svg"
                        } else {
                            "icons/repeat.svg"
                        },
                        "Allow Auto-Retry on Failure",
                        None,
                        !rec.flags.disable_auto_retry,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.toggle_auto_retry_action(id, cx);
                        },
                    ))
                    .child(div().h(px(1.)).bg(p.separator).my_1())
                    .child(menu_item_row(
                        "rcm-hash",
                        "icons/shield.svg",
                        "Check File Integrity (Hash)...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_hash_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-av",
                        "icons/shield.svg",
                        "Scan with Antivirus",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.check_virus_action(id, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-upd",
                        "icons/refresh.svg",
                        "Check Remote for Update",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.check_update_action(id, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-conv",
                        "icons/type_video.svg",
                        "Convert MP3 / MP4...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_convert_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-src",
                        "icons/link.svg",
                        "Change URL / Mirrors...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_change_source_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-move",
                        "icons/file_move.svg",
                        "Move To Folder...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_move_dir_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-rename",
                        "icons/edit.svg",
                        "Rename File...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_rename_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-sched",
                        "icons/single_download_scheduler.svg",
                        "Schedule Window...",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.open_schedule_modal(id, w, cx);
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-copy-url",
                        "icons/content_copy.svg",
                        "Copy Download URL",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.copy_to_clipboard(url_for_copy.clone(), "Copied download URL", cx);
                        },
                    ))
                    .when(is_bt, |el| {
                        el.child(div().h(px(1.)).bg(p.separator).my_1())
                            .child(menu_item_row(
                                "rcm-bt-reann",
                                "icons/refresh.svg",
                                "Force Tracker Reannounce",
                                None,
                                false,
                                false,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.active_popover = ActivePopover::None;
                                    this.bt_reannounce_action(id, cx);
                                },
                            ))
                            .child(menu_item_row(
                                "rcm-bt-noseed",
                                if rec.flags.bt_disable_seeding {
                                    "icons/check.svg"
                                } else {
                                    "icons/pause.svg"
                                },
                                "Disable Seeding",
                                None,
                                rec.flags.bt_disable_seeding,
                                false,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.bt_toggle_seeding_action(id, cx);
                                },
                            ))
                            .child(menu_item_row(
                                "rcm-bt-ratio",
                                if rec.flags.bt_ignore_upload_ratio {
                                    "icons/check.svg"
                                } else {
                                    "icons/arrow_up.svg"
                                },
                                "Ignore Upload Ratio Limit",
                                None,
                                rec.flags.bt_ignore_upload_ratio,
                                false,
                                p,
                                cx,
                                move |this, _, _, cx| {
                                    this.bt_toggle_ignore_ratio_action(id, cx);
                                },
                            ))
                    })
                    .when(!all_tags.is_empty(), |el| {
                        el.child(div().h(px(1.)).bg(p.separator).my_1()).child(
                            div()
                                .px_2()
                                .py(px(4.))
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap_1()
                                .children(all_tags.into_iter().map(|t| {
                                    let tid = t.id;
                                    let assigned = rec.tags.contains(&tid);
                                    let dot = parse_hex_color(&t.color_hex);
                                    div()
                                        .id(SharedString::from(format!("rcm-tag-{id}-{tid}")))
                                        .px_1p5()
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .bg(if assigned { p.accent_soft } else { p.bg_input })
                                        .border_1()
                                        .border_color(if assigned { p.accent } else { p.border })
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if assigned {
                                                let _ = this.engine.unassign_tag(id, tid);
                                            } else {
                                                let _ = this.engine.assign_tag(id, tid);
                                            }
                                            this.refresh_snapshot(cx);
                                            cx.notify();
                                        }))
                                        .child(div().w(px(7.)).h(px(7.)).rounded(px(2.)).bg(dot))
                                        .child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(p.text_primary)
                                                .child(t.name),
                                        )
                                })),
                        )
                    })
                    .child(div().h(px(1.)).bg(p.separator).my(px(2.)))
                    .child(menu_item_row(
                        "rcm-stop",
                        "icons/stop.svg",
                        "Stop Download",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.cancel_download(id);
                            cx.notify();
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-rm",
                        "icons/trash.svg",
                        "Remove from List",
                        None,
                        false,
                        false,
                        p,
                        cx,
                        move |this, _, _, cx| {
                            this.active_popover = ActivePopover::None;
                            this.remove_download(id, false);
                            cx.notify();
                        },
                    ))
                    .child(menu_item_row(
                        "rcm-rm-files",
                        "icons/delete.svg",
                        "Delete...",
                        Some("Del"),
                        false,
                        true,
                        p,
                        cx,
                        move |this, _, w, cx| {
                            this.active_popover = ActivePopover::None;
                            this.request_delete_downloads(vec![id], w, cx);
                        },
                    )),
            )
            .absolute()
            .top(p.spx(74.))
            .right(p.spx(20.))
            .into_any_element(),
    )
}
