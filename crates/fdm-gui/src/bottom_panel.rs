use crate::app::{FerriteApp, InspectorTab};
use crate::helpers::{
    download_icon_and_color, format_bytes, format_datetime_full, format_eta, format_minute_hhmm,
    format_ratio, format_speed, state_badge_info,
};
use crate::i18n;
use crate::theme::parse_hex_color;
use crate::ui_widgets::{
    action_btn, badge_themed, chip_toggle, danger_btn, ellipsis_text, fdm_checkbox, ghost_icon_btn,
    slim_progress_bar, slim_progress_bar_active, svg_icon, tab_pill,
};
use fdm_core::{
    DownloadPriority, DownloadState, DownloadType, ResumeSupport, SegmentState, TrafficUsageMode,
};
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, FontWeight, SharedString, div,
    ease_out_quint, prelude::*, px, rgb,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

pub fn render_bottom_panel(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let is_closing = !app.inspector_expanded
        && app
            .inspector_closing_since
            .is_some_and(|t| t.elapsed() < Duration::from_millis(200));
    if !app.inspector_expanded && !is_closing {
        return div().into_any_element();
    }
    let p = app.theme;
    let Some(rec) = app
        .selected_id
        .and_then(|id| app.all_downloads.iter().find(|d| d.id == id).cloned())
    else {
        return div().into_any_element();
    };

    let id = rec.id;
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let is_snail = app.summary.tum == TrafficUsageMode::Snail;
    let insp_epoch = app.inspector_anim_epoch;
    let insp_tab_epoch = app.inspector_tab_anim_epoch;
    let tabs = [
        (InspectorTab::General, s.tab_general),
        (InspectorTab::Details, s.tab_details),
        (InspectorTab::Files, s.tab_files),
        (InspectorTab::Progress, s.tab_progress),
        (InspectorTab::Connections, s.tab_connections),
        (InspectorTab::SpeedGraph, s.tab_speed),
    ];

    let tab_bar = div()
        .h(p.spx(34.))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .bg(p.bg_200)
        .border_b_1()
        .border_color(p.separator)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .children(tabs.into_iter().map(|(tab, lbl)| {
                    tab_pill(
                        format!("insp-tab-{tab:?}"),
                        lbl,
                        app.inspector_tab == tab,
                        &p,
                        cx,
                        move |this, _, _, cx| {
                            this.set_inspector_tab(tab, cx);
                        },
                    )
                })),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_size(p.fpx(11.))
                        .text_color(p.text_muted)
                        .truncate()
                        .child(format!("#{} · {}", rec.id, rec.title)),
                )
                .child(ghost_icon_btn(
                    "insp-close",
                    "icons/close.svg",
                    p.spx_f32(14.),
                    p.text_muted,
                    p.bg_300,
                    cx,
                    |this, _, _, cx| {
                        this.set_inspector_expanded(false, cx);
                    },
                )),
        );

    let content: AnyElement = match app.inspector_tab {
        InspectorTab::General => {
            let (type_icon, type_color) = download_icon_and_color(&rec);
            let is_folder_item = rec.files.len() > 1
                || matches!(
                    rec.download_type,
                    DownloadType::BitTorrent | DownloadType::Batch
                );
            let frac = rec.progress_fraction();
            let pct_int = (frac * 100.0).round() as u32;
            let (st_label, st_color) = state_badge_info(&rec, &p);
            let bar_color = if rec.state == DownloadState::Downloading && is_snail {
                p.warning
            } else {
                st_color
            };
            let is_completed = rec.state == DownloadState::Completed;
            let remaining_str = match rec.total_bytes {
                Some(total) if total > rec.downloaded_bytes => {
                    let rem = total - rec.downloaded_bytes;
                    if let Some(eta) = rec.eta_secs {
                        format!("{} ({})", format_bytes(rem), format_eta(Some(eta)))
                    } else {
                        format_bytes(rem)
                    }
                }
                Some(_) if is_completed => "Completed".into(),
                _ => "--".into(),
            };
            let dl_str = match rec.total_bytes {
                Some(total) => format!(
                    "{} / {}",
                    format_bytes(rec.downloaded_bytes),
                    format_bytes(total)
                ),
                None => format_bytes(rec.downloaded_bytes),
            };
            let ratio_str = format_ratio(rec.downloaded_bytes, rec.uploaded_bytes);
            let ul_ratio_str = format!("{} (ratio {})", format_bytes(rec.uploaded_bytes), ratio_str);
            let out_path_str = rec.output_path().to_string_lossy().into_owned();
            let out_copy = out_path_str.clone();
            let url_copy = rec.url.clone();
            let prio_label = match rec.priority {
                DownloadPriority::High => "High",
                DownloadPriority::Low => "Low",
                DownloadPriority::DontDownload => "Skip",
                _ => "Normal",
            };
            let sched_summary = match &rec.schedule {
                Some(s) if s.enabled => format!(
                    "{}–{}",
                    format_minute_hhmm(s.start_minute),
                    format_minute_hhmm(s.end_minute)
                ),
                _ => "Off".into(),
            };

            let all_tags = app.tags.clone();
            let mut tag_buttons = Vec::new();
            for t in all_tags {
                let has = rec.tags.contains(&t.id);
                let tid = t.id;
                let swatch = parse_hex_color(&t.color_hex);
                tag_buttons.push(
                    div()
                        .id(SharedString::from(format!("insp-tag-{tid}")))
                        .px_2()
                        .py(px(3.))
                        .rounded(px(5.))
                        .bg(if has { p.accent_soft } else { p.bg_200 })
                        .hover(move |s| s.bg(p.bg_300))
                        .border_1()
                        .border_color(if has { p.accent } else { p.border })
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.))
                        .font_weight(if has {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if has { p.text_primary } else { p.text_secondary })
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if has {
                                let _ = this.engine.unassign_tag(id, tid);
                            } else {
                                let _ = this.engine.assign_tag(id, tid);
                            }
                            this.refresh_snapshot(cx);
                            cx.notify();
                        }))
                        .child(div().w(px(7.)).h(px(7.)).rounded(px(2.)).bg(swatch))
                        .child(t.name)
                        .into_any_element(),
                );
            }

            div()
                .py_2p5()
                .px_4()
                .flex()
                .flex_row()
                .gap_4()
                .child(
                    // Left preview box (FDM GeneralTab_V2 left card)
                    div()
                        .w(p.spx(128.))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("insp-preview-{id}")))
                                .w(p.spx(112.))
                                .h(p.spx(92.))
                                .rounded(p.radius(10., 3.))
                                .bg(p.bg_200)
                                .border_1()
                                .border_color(p.border)
                                .hover(|s| s.bg(p.bg_300))
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if is_completed {
                                        this.open_download_file(id);
                                    } else {
                                        this.reveal_download_file(id);
                                    }
                                    cx.notify();
                                }))
                                .when(is_folder_item, |el| {
                                    el.child(svg_icon("icons/folder_mac.svg", p.spx_f32(52.), p.accent))
                                })
                                .when(!is_folder_item, |el| {
                                    el.child(
                                        div()
                                            .w(p.spx(48.))
                                            .h(p.spx(48.))
                                            .rounded(p.radius(12., 4.))
                                            .bg(type_color)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(svg_icon(
                                                type_icon,
                                                p.spx_f32(26.),
                                                rgb(0xffffff).into(),
                                            )),
                                    )
                                })
                                .child(
                                    div()
                                        .text_size(p.fpx(10.))
                                        .text_color(p.text_muted)
                                        .child(if is_completed {
                                            "Click to open"
                                        } else {
                                            "Show in Finder"
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_1()
                                .child(ghost_icon_btn(
                                    format!("insp-prio-down-{id}"),
                                    "icons/priority_down.svg",
                                    p.spx_f32(13.),
                                    p.text_muted,
                                    p.bg_300,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.bump_priority_action(id, false, cx);
                                    },
                                ))
                                .child(badge_themed(prio_label, p.accent, p.accent_soft, &p))
                                .child(ghost_icon_btn(
                                    format!("insp-prio-up-{id}"),
                                    "icons/priority_up.svg",
                                    p.spx_f32(13.),
                                    p.text_muted,
                                    p.bg_300,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.bump_priority_action(id, true, cx);
                                    },
                                )),
                        ),
                )
                .child(
                    // Right metadata & actions area
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            // Header title + status + destination path
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .flex_shrink()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_shrink()
                                                .overflow_hidden()
                                                .text_size(p.fpx(15.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(p.text_primary)
                                                .child(ellipsis_text(rec.title.clone())),
                                        )
                                        .child(badge_themed(st_label, st_color, p.bg_200, &p))
                                        .child(badge_themed(
                                            rec.download_type.display_name(),
                                            p.text_secondary,
                                            p.bg_200,
                                            &p,
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1p5()
                                        .flex_shrink_0()
                                        .child(
                                            div()
                                                .max_w(p.spx(280.))
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_size(p.fpx(11.))
                                                .text_color(p.text_muted)
                                                .child(ellipsis_text(out_path_str)),
                                        )
                                        .child(ghost_icon_btn(
                                            format!("insp-copy-path-{id}"),
                                            "icons/content_copy.svg",
                                            p.spx_f32(13.),
                                            p.text_muted,
                                            p.bg_300,
                                            cx,
                                            move |this, _, _, cx| {
                                                this.copy_to_clipboard(
                                                    out_copy.clone(),
                                                    "Copied output path",
                                                    cx,
                                                );
                                            },
                                        ))
                                        .child(ghost_icon_btn(
                                            format!("insp-open-dir-{id}"),
                                            "icons/folder_open.svg",
                                            p.spx_f32(14.),
                                            p.accent,
                                            p.bg_300,
                                            cx,
                                            move |this, _, _, cx| {
                                                this.reveal_download_file(id);
                                                cx.notify();
                                            },
                                        )),
                                ),
                        )
                        .child(
                            // Progress bar + Remaining
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(div().flex_1().child(slim_progress_bar_active(
                                    format!("insp-prog-{id}"),
                                    frac,
                                    p.spx_f32(7.0),
                                    bar_color,
                                    p.bg_300,
                                    rec.state == DownloadState::Downloading,
                                )))
                                .child(
                                    div()
                                        .text_size(p.fpx(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text_primary)
                                        .child(format!("{pct_int}%")),
                                )
                                .child(
                                    div()
                                        .text_size(p.fpx(12.))
                                        .text_color(p.text_muted)
                                        .child(format!("{}: {remaining_str}", s.lbl_remaining)),
                                ),
                        )
                        .child(
                            // 2x3 metadata grid matching GeneralTab_V2.qml
                            div()
                                .grid()
                                .grid_cols(3)
                                .gap_x_4()
                                .gap_y_1()
                                .p_1p5()
                                .rounded(p.radius(8., 3.))
                                .bg(p.bg_200)
                                .border_1()
                                .border_color(p.separator)
                                .child(meta_cell(s.lbl_downloaded, dl_str, &p))
                                .child(meta_cell(
                                    s.lbl_uploaded_ratio,
                                    ul_ratio_str,
                                    &p,
                                ))
                                .child(meta_cell(
                                    s.lbl_added_at,
                                    format_datetime_full(rec.created_at),
                                    &p,
                                ))
                                .child(meta_cell(
                                    s.lbl_dl_speed,
                                    format_speed(rec.download_speed_bps),
                                    &p,
                                ))
                                .child(meta_cell(
                                    s.lbl_ul_speed,
                                    format_speed(rec.upload_speed_bps),
                                    &p,
                                ))
                                .child(meta_cell(s.lbl_priority, prio_label.into(), &p)),
                        )
                        .child(
                            // Source URL row + error if any
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1p5()
                                        .min_w_0()
                                        .flex_1()
                                        .child(svg_icon("icons/link.svg", 13., p.text_muted))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_xs()
                                                .text_color(p.text_muted)
                                                .child(ellipsis_text(rec.url.clone())),
                                        ),
                                )
                                .child(ghost_icon_btn(
                                    format!("insp-copy-url-{id}"),
                                    "icons/content_copy.svg",
                                    13.,
                                    p.text_muted,
                                    p.bg_300,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.copy_to_clipboard(
                                            url_copy.clone(),
                                            "Copied source URL",
                                            cx,
                                        );
                                    },
                                )),
                        )
                        .when_some(rec.last_error.clone(), |el, err| {
                            el.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(p.danger)
                                    .child(ellipsis_text(format!("Error: {err}"))),
                            )
                        })
                        .child(
                            // Quick actions bar
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap_1p5()
                                .child(chip_toggle(
                                    "insp-play-asap",
                                    "Play ASAP",
                                    rec.flags.sequential_play_asap,
                                    &p,
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
                                .child(action_btn(
                                    "insp-hash",
                                    "Hash",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_hash_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-conv",
                                    "Convert",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_convert_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-move",
                                    "Move",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_move_dir_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-rename",
                                    "Rename",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_rename_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-src",
                                    "URL / Mirrors",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_change_source_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-sched",
                                    format!("Schedule ({sched_summary})"),
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.open_schedule_modal(id, w, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-av",
                                    "AV Scan",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.check_virus_action(id, cx);
                                    },
                                ))
                                .child(action_btn(
                                    "insp-restart",
                                    "Restart",
                                    &p,
                                    false,
                                    cx,
                                    move |this, _, _, cx| {
                                        this.restart_download_action(id, cx);
                                    },
                                ))
                                .child(danger_btn(
                                    "insp-del",
                                    "Delete...",
                                    &p,
                                    cx,
                                    move |this, _, w, cx| {
                                        this.request_delete_downloads(vec![id], w, cx);
                                    },
                                )),
                        )
                        .child(
                            // Tags row
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap_1()
                                .child(div().text_xs().text_color(p.text_muted).child("Tags:"))
                                .children(tag_buttons)
                                .child(
                                    div()
                                        .id("insp-add-tag")
                                        .px_2()
                                        .py(px(2.))
                                        .rounded(px(5.))
                                        .bg(p.bg_200)
                                        .border_1()
                                        .border_color(p.border)
                                        .text_size(px(11.))
                                        .text_color(p.accent)
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, w, cx| {
                                            this.open_edit_tag_modal(None, w, cx);
                                        }))
                                        .child("+ Tag"),
                                ),
                        ),
                )
                .into_any_element()
        }
        InspectorTab::Details => {
            let resume_lbl = match rec.resume_support {
                ResumeSupport::Present => "Supported (HTTP Ranges)",
                ResumeSupport::Absent => "Not Supported",
                ResumeSupport::Unknown => "Unknown",
            };
            div()
                .p_3p5()
                .px_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .grid()
                        .grid_cols(4)
                        .gap_3()
                        .p_2p5()
                        .rounded(px(8.))
                        .bg(p.bg_200)
                        .border_1()
                        .border_color(p.separator)
                        .child(meta_cell("Resume Support", resume_lbl.into(), &p))
                        .child(meta_cell(
                            "Active Connections",
                            rec.connections_active.to_string(),
                            &p,
                        ))
                        .child(meta_cell(
                            "ETag",
                            rec.etag.clone().unwrap_or_else(|| "--".into()),
                            &p,
                        ))
                        .child(meta_cell(
                            "Last-Modified",
                            rec.last_modified.clone().unwrap_or_else(|| "--".into()),
                            &p,
                        )),
                )
                .when(rec.segments.is_empty(), |el| {
                    el.child(
                        div()
                            .p_3()
                            .rounded(px(8.))
                            .bg(p.bg_200)
                            .text_xs()
                            .text_color(p.text_muted)
                            .child("No segmented HTTP byte ranges recorded for this download."),
                    )
                })
                .when(!rec.segments.is_empty(), |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .children(rec.segments.iter().map(|seg| {
                                let len = seg.end.saturating_sub(seg.start).max(1);
                                let frac = (seg.downloaded as f64 / len as f64).clamp(0.0, 1.0);
                                let col = match seg.state {
                                    SegmentState::Completed => p.success,
                                    SegmentState::Active => p.accent,
                                    SegmentState::Failed => p.danger,
                                    SegmentState::Pending => p.text_muted,
                                };
                                div()
                                    .w(px(190.))
                                    .p_2()
                                    .rounded(px(8.))
                                    .bg(p.bg_200)
                                    .border_1()
                                    .border_color(p.border)
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .text_xs()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(p.text_primary)
                                                    .child(format!("Segment #{}", seg.id + 1)),
                                            )
                                            .child(
                                                div()
                                                    .text_color(col)
                                                    .child(format!("{:.0}%", frac * 100.0)),
                                            ),
                                    )
                                    .child(slim_progress_bar(frac, 5.0, col, p.bg_300))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .justify_between()
                                            .text_size(px(10.))
                                            .text_color(p.text_muted)
                                            .child(format!(
                                                "{} / {}",
                                                format_bytes(seg.downloaded),
                                                format_bytes(len)
                                            ))
                                            .child(format_speed(seg.speed_bps)),
                                    )
                            })),
                    )
                })
                .into_any_element()
        }
        InspectorTab::Files => {
            if rec.files.is_empty() {
                div()
                    .p_4()
                    .text_xs()
                    .text_color(p.text_muted)
                    .child(format!("Single output file: {}", rec.output_path().display()))
                    .into_any_element()
            } else {
                let cur_selected: HashSet<usize> = rec
                    .files
                    .iter()
                    .filter(|f| f.priority != DownloadPriority::DontDownload)
                    .map(|f| f.index)
                    .collect();
                div()
                    .p_2p5()
                    .px_4()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .children(rec.files.iter().map(|f| {
                        let f_idx = f.index;
                        let checked = f.priority != DownloadPriority::DontDownload;
                        let f_frac = if f.size_bytes > 0 {
                            (f.downloaded_bytes as f64 / f.size_bytes as f64).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let next_set = {
                            let mut s = cur_selected.clone();
                            if checked {
                                s.remove(&f_idx);
                            } else {
                                s.insert(f_idx);
                            }
                            s
                        };
                        div()
                            .id(SharedString::from(format!("insp-file-{id}-{f_idx}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .px_2p5()
                            .py(px(6.))
                            .rounded(px(8.))
                            .bg(p.bg_200)
                            .border_1()
                            .border_color(p.separator)
                            .child(
                                div()
                                    .id(SharedString::from(format!("insp-file-toggle-{id}-{f_idx}")))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .flex_1()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let eng = Arc::clone(&this.engine);
                                        let sel = next_set.clone();
                                        this.rt.spawn(async move {
                                            let _ = eng.update_torrent_file_selection(id, sel).await;
                                        });
                                        cx.notify();
                                    }))
                                    .child(fdm_checkbox(checked, &p))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(p.text_primary)
                                            .truncate()
                                            .child(f.relative_path.clone()),
                                    )
                                    .when(f.media_ready_to_play, |el| {
                                        el.child(badge_themed(
                                            "Ready to Play",
                                            p.success,
                                            p.bg_input,
                                            &p,
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_3()
                                    .flex_shrink_0()
                                    .child(
                                        div()
                                            .w(px(76.))
                                            .child(slim_progress_bar(f_frac, 4.0, p.accent, p.bg_300)),
                                    )
                                    .child(
                                        div()
                                            .w(px(110.))
                                            .text_right()
                                            .text_size(px(11.))
                                            .text_color(p.text_muted)
                                            .child(format!(
                                                "{} / {}",
                                                format_bytes(f.downloaded_bytes),
                                                format_bytes(f.size_bytes)
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(3.))
                                            .children([
                                                (DownloadPriority::DontDownload, "Skip"),
                                                (DownloadPriority::Low, "Low"),
                                                (DownloadPriority::Normal, "Norm"),
                                                (DownloadPriority::High, "High"),
                                            ].into_iter().map(|(pr, lbl)| {
                                                chip_toggle(
                                                    format!("insp-fp-{id}-{f_idx}-{pr:?}"),
                                                    lbl,
                                                    f.priority == pr,
                                                    &p,
                                                    cx,
                                                    move |this, _, _, cx| {
                                                        let eng = Arc::clone(&this.engine);
                                                        this.rt.spawn(async move {
                                                            let _ = eng
                                                                .update_torrent_file_priority(
                                                                    id, f_idx, pr,
                                                                )
                                                                .await;
                                                        });
                                                        cx.notify();
                                                    },
                                                )
                                            })),
                                    ),
                            )
                    }))
                    .into_any_element()
            }
        }
        InspectorTab::Progress => {
            let frac = rec.progress_fraction();
            let pct_str = format!("{:.1}%", frac * 100.0);
            let total_blocks = 48usize;
            let cells: Vec<gpui::Div> = if !rec.segments.is_empty() {
                let mut v = Vec::with_capacity(rec.segments.len().min(96));
                for seg in rec.segments.iter().take(96) {
                    let c = match seg.state {
                        SegmentState::Completed => p.success,
                        SegmentState::Active => p.accent,
                        SegmentState::Failed => p.danger,
                        SegmentState::Pending => p.bg_300,
                    };
                    v.push(div().flex_1().h(px(18.)).rounded(px(3.)).bg(c));
                }
                v
            } else {
                let filled = (frac * total_blocks as f64).round() as usize;
                (0..total_blocks)
                    .map(|i| {
                        let c = if i < filled {
                            if rec.state == DownloadState::Error {
                                p.danger
                            } else {
                                p.success
                            }
                        } else if i == filled && rec.state.is_active() {
                            p.accent
                        } else {
                            p.bg_300
                        };
                        div().flex_1().h(px(18.)).rounded(px(3.)).bg(c)
                    })
                    .collect()
            };

            let remaining = rec
                .total_bytes
                .map(|t| t.saturating_sub(rec.downloaded_bytes))
                .unwrap_or(0);

            div()
                .p_3p5()
                .px_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .child("Piece / Segment Availability Map"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(p.accent)
                                .child(pct_str),
                        ),
                )
                .child(
                    div()
                        .p_2()
                        .rounded(px(8.))
                        .bg(p.bg_200)
                        .border_1()
                        .border_color(p.border)
                        .flex()
                        .flex_row()
                        .gap(px(2.))
                        .children(cells),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(4)
                        .gap_3()
                        .p_2p5()
                        .rounded(px(8.))
                        .bg(p.bg_200)
                        .child(meta_cell(
                            "Total Size",
                            rec.total_bytes
                                .map(format_bytes)
                                .unwrap_or_else(|| "Unknown".into()),
                            &p,
                        ))
                        .child(meta_cell(
                            s.lbl_downloaded,
                            format_bytes(rec.downloaded_bytes),
                            &p,
                        ))
                        .child(meta_cell(s.lbl_remaining, format_bytes(remaining), &p))
                        .child(meta_cell(
                            "Share Ratio",
                            format_ratio(rec.downloaded_bytes, rec.uploaded_bytes),
                            &p,
                        )),
                )
                .into_any_element()
        }
        InspectorTab::Connections => {
            let is_bt = rec.download_type == DownloadType::BitTorrent;
            if !is_bt && rec.peers.is_empty() && rec.trackers.is_empty() {
                div()
                    .p_3p5()
                    .px_4()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(
                        div()
                            .grid()
                            .grid_cols(3)
                            .gap_3()
                            .p_2p5()
                            .rounded(px(8.))
                            .bg(p.bg_200)
                            .child(meta_cell(
                                "Active Connections",
                                rec.connections_active.to_string(),
                                &p,
                            ))
                            .child(meta_cell(
                                "HTTP Referer",
                                rec.http.referrer.clone().unwrap_or_else(|| "--".into()),
                                &p,
                            ))
                            .child(meta_cell(
                                "User-Agent",
                                rec.http.user_agent.clone().unwrap_or_else(|| "Default".into()),
                                &p,
                            )),
                    )
                    .child(
                        div()
                            .p_2p5()
                            .rounded(px(8.))
                            .bg(p.bg_200)
                            .border_1()
                            .border_color(p.separator)
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.text_muted)
                                    .child(format!("MIRRORS ({})", rec.http.mirrors.len())),
                            )
                            .when(rec.http.mirrors.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_subtle)
                                        .child("No alternate mirror URLs configured."),
                                )
                            })
                            .children(rec.http.mirrors.iter().map(|m| {
                                div()
                                    .text_xs()
                                    .text_color(p.text_primary)
                                    .truncate()
                                    .child(m.clone())
                            })),
                    )
                    .into_any_element()
            } else {
                div()
                    .p_3p5()
                    .px_4()
                    .flex()
                    .flex_row()
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
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(p.text_muted)
                                    .child(format!("PEERS ({})", rec.peers.len())),
                            )
                            .when(rec.peers.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_subtle)
                                        .child("No connected swarm peers yet."),
                                )
                            })
                            .children(rec.peers.iter().take(25).map(|peer| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .gap_2()
                                    .text_xs()
                                    .px_2p5()
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(p.bg_200)
                                    .child(
                                        div()
                                            .text_color(p.text_primary)
                                            .truncate()
                                            .child(format!("{} ({})", peer.address, peer.client)),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(p.accent)
                                            .child(format!(
                                                "↓{} ↑{}",
                                                format_speed(peer.download_bps),
                                                format_speed(peer.upload_bps)
                                            )),
                                    )
                            })),
                    )
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
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.text_muted)
                                            .child(format!("TRACKERS ({})", rec.trackers.len())),
                                    )
                                    .child(action_btn(
                                        "insp-bt-reann",
                                        "Reannounce",
                                        &p,
                                        false,
                                        cx,
                                        move |this, _, _, cx| {
                                            this.bt_reannounce_action(id, cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_1p5()
                                    .child(div().flex_1().child(app.bt_add_tracker_input.clone()))
                                    .child(action_btn(
                                        "insp-bt-add-tr",
                                        "Add Tracker",
                                        &p,
                                        false,
                                        cx,
                                        move |this, _, _, cx| {
                                            this.add_bt_tracker_action(id, cx);
                                        },
                                    )),
                            )
                            .children(rec.trackers.iter().take(25).map(|tr| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_between()
                                    .gap_2()
                                    .text_xs()
                                    .px_2p5()
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(p.bg_200)
                                    .child(
                                        div()
                                            .text_color(p.text_primary)
                                            .truncate()
                                            .child(tr.url.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(p.text_muted)
                                            .child(format!("{} ({})", tr.status, tr.peers)),
                                    )
                            })),
                    )
                    .into_any_element()
            }
        }
        InspectorTab::SpeedGraph => {
            let hist = app.engine.speed_history();
            let peak = hist
                .iter()
                .flat_map(|(d, u)| [*d, *u])
                .max()
                .unwrap_or(1)
                .max(1024);
            div()
                .p_3p5()
                .px_4()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .text_xs()
                        .child(div().text_color(p.text_secondary).child(format!(
                            "Last {}s throughput · Peak {}",
                            hist.len(),
                            format_speed(peak)
                        )))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(div().text_color(p.accent).child(format!(
                                    "↓ {}",
                                    format_speed(app.summary.total_download_bps)
                                )))
                                .child(div().text_color(p.success).child(format!(
                                    "↑ {}",
                                    format_speed(app.summary.total_upload_bps)
                                ))),
                        ),
                )
                .child(
                    div()
                        .h(px(125.))
                        .w_full()
                        .p_2()
                        .rounded(px(8.))
                        .bg(p.bg_200)
                        .border_1()
                        .border_color(p.border)
                        .flex()
                        .flex_row()
                        .items_end()
                        .gap(px(2.))
                        .children(
                            hist.into_iter()
                                .rev()
                                .take(60)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .map(|(d, u)| {
                                    let frac = (d.max(u) as f32 / peak as f32).clamp(0.03, 1.0);
                                    div()
                                        .flex_1()
                                        .h(gpui::relative(frac))
                                        .rounded_sm()
                                        .bg(if d >= u { p.accent } else { p.success })
                                }),
                        ),
                )
                .into_any_element()
        }
    };

    let panel_h = p.spx_f32(304.);
    let expanding = app.inspector_expanded;
    let panel = div()
        .id("bottom-inspector-panel")
        .flex_shrink_0()
        .bg(p.bg_100)
        .border_t_1()
        .border_color(p.border)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(tab_bar)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id("insp-scroll")
                .overflow_y_scroll()
                .child(
                    div()
                        .id("insp-tab-body")
                        .child(content)
                        .with_animation(
                            SharedString::from(format!("insp-tab-enter-{insp_tab_epoch}")),
                            Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                            |el, delta| {
                                el.opacity((0.4 + 0.6 * delta).min(1.0))
                                    .mt(px(5.0 * (1.0 - delta)))
                            },
                        ),
                ),
        );

    if expanding {
        panel
            .with_animation(
                SharedString::from(format!("insp-expand-{insp_epoch}")),
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                move |el, delta| el.h(px(panel_h * delta)).opacity((0.4 + 0.6 * delta).min(1.0)),
            )
            .into_any_element()
    } else {
        panel
            .with_animation(
                SharedString::from(format!("insp-collapse-{insp_epoch}")),
                Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                move |el, delta| {
                    el.h(px((panel_h * (1.0 - delta)).max(0.0)))
                        .opacity((1.0 - delta * 0.6).max(0.0))
                },
            )
            .into_any_element()
    }
}

fn meta_cell(label: &str, value: String, p: &crate::theme::ThemePalette) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(1.))
        .min_w_0()
        .child(
            div()
                .text_size(p.fpx(10.))
                .text_color(p.text_muted)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(p.fpx(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_primary)
                .truncate()
                .child(value),
        )
}
