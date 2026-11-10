use crate::app::{ActiveModal, FerriteApp};
use crate::helpers::{available_disk_space_bytes, expand_path_preview, format_bytes};
use crate::i18n;
use crate::qr_view::render_qr_element;
use crate::theme::{ThemePalette, parse_hex_color};
use crate::ui_widgets::{
    action_btn, badge_themed, chip_toggle, danger_btn, ellipsis_text, fdm_checkbox, ghost_icon_btn,
    labeled_field, password_field_row, svg_icon, switch_row,
};
use fdm_batch::{expand_batch_pattern, parse_batch_text_input};
use fdm_core::{DownloadId, DownloadPriority, FileExistsAction, HashAlgorithm};
use gpui::{
    Animation, AnimationExt as _, AnyElement, ClipboardItem, Context, FontWeight, MouseButton,
    SharedString, Window, div, ease_out_quint, prelude::*, px,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

fn modal_shell(
    id: &'static str,
    title: impl Into<String>,
    width: f32,
    anim_epoch: u64,
    p: &ThemePalette,
    body: AnyElement,
    footer: AnyElement,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    div()
        .id(id)
        .w(p.spx(width))
        .max_h(p.spx(720.))
        .bg(p.bg_surface)
        .border_1()
        .border_color(p.border)
        .rounded(p.radius(10., 3.))
        .shadow_xl()
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_4()
                .py_3()
                .bg(p.bg_sidebar)
                .border_b_1()
                .border_color(p.border)
                .child(
                    div()
                        .text_size(p.fpx(14.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(p.text_primary)
                        .child(title.into()),
                )
                .child(ghost_icon_btn(
                    "modal-close-x",
                    "icons/close.svg",
                    p.spx_f32(16.),
                    p.text_muted,
                    p.bg_elevated,
                    cx,
                    |this, _, w, cx| {
                        this.close_modal(w, cx);
                    },
                )),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id(SharedString::from(format!("{id}-scroll")))
                .overflow_y_scroll()
                .p_4()
                .child(body),
        )
        .child(
            div()
                .px_4()
                .py_3()
                .bg(p.bg_sidebar)
                .border_t_1()
                .border_color(p.border)
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap_2()
                .child(footer),
        )
        .with_animation(
            SharedString::from(format!("{id}-card-{anim_epoch}")),
            Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
            |el, delta| el.opacity(delta).mt(px(14.0 * (1.0 - delta))),
        )
        .into_any_element()
}

pub fn render_active_modal(
    app: &mut FerriteApp,
    _window: &mut Window,
    cx: &mut Context<FerriteApp>,
) -> Option<AnyElement> {
    let modal = app.active_modal.clone();
    match modal {
        ActiveModal::None => None,
        ActiveModal::AddDownload => Some(render_add_modal(app, cx)),
        ActiveModal::BatchAdd => Some(render_batch_modal(app, cx)),
        ActiveModal::FileConflict { id, path } => Some(render_conflict_modal(app, id, path, cx)),
        ActiveModal::HashVerify { id } => Some(render_hash_modal(app, id, cx)),
        ActiveModal::MediaConvert { id } => Some(render_convert_modal(app, id, cx)),
        ActiveModal::RemoteQr => Some(render_remote_qr_modal(app, cx)),
        ActiveModal::EditTag { id } => Some(render_edit_tag_modal(app, id, cx)),
        ActiveModal::ChangeSource { id } => Some(render_change_source_modal(app, id, cx)),
        ActiveModal::MoveDir { id } => Some(render_move_dir_modal(app, id, cx)),
        ActiveModal::RenameDownload { id } => Some(render_rename_modal(app, id, cx)),
        ActiveModal::DownloadSchedule { id } => Some(render_schedule_modal(app, id, cx)),
        ActiveModal::DeleteConfirm { ids } => Some(render_delete_confirm_modal(app, ids, cx)),
        ActiveModal::QuitConfirm => Some(render_quit_confirm_modal(app, cx)),
        ActiveModal::About => Some(render_about_modal(app, cx)),
        ActiveModal::CreatePortable => Some(render_portable_modal(app, cx)),
    }
}

fn render_add_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let outcome = app.add_probe_outcome.clone();
    let busy = app.add_probe_busy;
    let adv = app.add_show_advanced;
    let probe_epoch = app.probe_anim_epoch;
    let adv_epoch = app.adv_panel_anim_epoch;
    let prio = app.add_priority;
    let start_paused = app.add_start_paused;
    let play_asap = app.add_play_asap;
    let apply_sched = app.add_apply_schedule;
    let sel_files = app.add_selected_files.clone();
    let sel_var = app.add_selected_variant;
    let show_browse_btn = app.settings_cfg.app.show_save_as_button;

    let raw_url = app.add_url_input.read(cx).text();
    let dir_str = app.add_dir_input.read(cx).text();
    let name_str = app.add_name_input.read(cx).text();
    let expanded_dir = expand_path_preview(&dir_str, &raw_url, &name_str);
    let free_space_txt = available_disk_space_bytes(std::path::Path::new(&expanded_dir))
        .map(|b| format!("Free space: {}", format_bytes(b)))
        .unwrap_or_else(|| "Free space: --".into());

    let mut content = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    s.url_input_label,
                    app.add_url_input.clone().into_any_element(),
                    &p,
                ))
                .child(action_btn(
                    "add-probe-btn",
                    if busy { s.probing } else { s.probe },
                    &p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.trigger_probe(cx);
                    },
                )),
        )
        .when_some(outcome.clone(), |el, out| {
            let sz = out
                .total_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "Unknown size".into());
            let mut box_div = div()
                .p_2p5()
                .rounded(p.radius(6., 2.))
                .bg(p.bg_input)
                .border_1()
                .border_color(p.border)
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(badge_themed(
                            out.download_type.display_name(),
                            p.accent,
                            p.accent_soft,
                            &p,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_primary)
                                .truncate()
                                .child(out.title.clone()),
                        )
                        .child(div().text_xs().text_color(p.text_muted).child(sz)),
                );

            if let Some(ref media) = out.media {
                if !media.variants.is_empty() {
                    box_div = box_div.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.text_muted)
                                    .child("Media Format / Quality:"),
                            )
                            .child(div().flex().flex_row().flex_wrap().gap_1().children(
                                media.variants.iter().enumerate().take(10).map(|(idx, v)| {
                                    chip_toggle(
                                        format!("add-var-{idx}"),
                                        format!("{} ({})", v.title, v.ext),
                                        sel_var == idx,
                                        &p,
                                        cx,
                                        move |this, _, _, cx| {
                                            this.add_selected_variant = idx;
                                            cx.notify();
                                        },
                                    )
                                }),
                            )),
                    );
                }
            }

            if !out.files.is_empty() {
                let all_idx: HashSet<usize> = out.files.iter().map(|f| f.index).collect();
                box_div = box_div.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_muted)
                                        .child(format!(
                                            "Files ({}/{} selected)",
                                            sel_files.len(),
                                            out.files.len()
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap_1()
                                        .child(action_btn(
                                            "add-files-all",
                                            "All",
                                            &p,
                                            false,
                                            cx,
                                            move |this, _, _, cx| {
                                                this.add_selected_files = all_idx.clone();
                                                cx.notify();
                                            },
                                        ))
                                        .child(action_btn(
                                            "add-files-none",
                                            "None",
                                            &p,
                                            false,
                                            cx,
                                            |this, _, _, cx| {
                                                this.add_selected_files.clear();
                                                cx.notify();
                                            },
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .max_h(p.spx(145.))
                                .id("add-torrent-files-scroll")
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .children(out.files.iter().map(|f| {
                                    let fi = f.index;
                                    let checked = sel_files.contains(&fi);
                                    div()
                                        .id(SharedString::from(format!("add-tf-{fi}")))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .px_2()
                                        .py(px(3.))
                                        .rounded_sm()
                                        .bg(p.bg_surface)
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if this.add_selected_files.contains(&fi) {
                                                this.add_selected_files.remove(&fi);
                                            } else {
                                                this.add_selected_files.insert(fi);
                                            }
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_1p5()
                                                .min_w_0()
                                                .child(fdm_checkbox(checked, &p))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(p.text_primary)
                                                        .truncate()
                                                        .child(f.relative_path.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(p.text_muted)
                                                .child(format_bytes(f.size_bytes)),
                                        )
                                })),
                        ),
                );
            }
            el.child(
                box_div
                    .id("add-probe-card")
                    .with_animation(
                        SharedString::from(format!("add-probe-anim-{probe_epoch}")),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta).mt(px(6.0 * (1.0 - delta))),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                .child(labeled_field(
                    s.save_folder_label,
                    app.add_dir_input.clone().into_any_element(),
                    &p,
                ))
                .when(show_browse_btn, |row| {
                    row.child(action_btn(
                        "add-browse-dir",
                        s.browse,
                        &p,
                        false,
                        cx,
                        |this, _, _, cx| {
                            this.browse_folder_into(
                                this.add_dir_input.clone(),
                                "Choose Save Folder",
                                cx,
                            );
                        },
                    ))
                })
                .child(labeled_field(
                    s.file_name_label,
                    app.add_name_input.clone().into_any_element(),
                    &p,
                )),
        )
        .child(
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
                        .gap_1()
                        .child(div().text_size(p.fpx(11.)).text_color(p.text_muted).child("Macros:"))
                        .children(["%date%", "%host%", "%type%"].into_iter().map(|m| {
                            chip_toggle(
                                format!("add-mac-{m}"),
                                m,
                                false,
                                &p,
                                cx,
                                move |this, _, _, cx| {
                                    let cur = this.add_dir_input.read(cx).text();
                                    let sep = if cur.ends_with('/') { "" } else { "/" };
                                    this.add_dir_input.update(cx, |i, cx| {
                                        i.set_text(&format!("{cur}{sep}{m}"), cx);
                                    });
                                    cx.notify();
                                },
                            )
                        })),
                )
                .child(badge_themed(free_space_txt, p.text_secondary, p.bg_input, &p)),
        )
        .when(dir_str.contains('%'), |el| {
            el.child(
                div()
                    .text_size(p.fpx(11.))
                    .text_color(p.text_muted)
                    .truncate()
                    .child(format!("Expanded folder: {expanded_dir}")),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .flex_wrap()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.text_secondary)
                        .child("Priority:"),
                )
                .children(
                    [
                        (DownloadPriority::Low, "Low"),
                        (DownloadPriority::Normal, "Normal"),
                        (DownloadPriority::High, "High"),
                    ]
                    .into_iter()
                    .map(|(pr, lbl)| {
                        chip_toggle(
                            format!("add-prio-{pr:?}"),
                            lbl,
                            prio == pr,
                            &p,
                            cx,
                            move |this, _, _, cx| {
                                this.add_priority = pr;
                                cx.notify();
                            },
                        )
                    }),
                )
                .child(chip_toggle(
                    "add-paused",
                    "Start Paused",
                    start_paused,
                    &p,
                    cx,
                    |this, _, _, cx| {
                        this.add_start_paused = !this.add_start_paused;
                        cx.notify();
                    },
                ))
                .child(chip_toggle(
                    "add-asap",
                    "Play ASAP",
                    play_asap,
                    &p,
                    cx,
                    |this, _, _, cx| {
                        this.add_play_asap = !this.add_play_asap;
                        cx.notify();
                    },
                ))
                .child(chip_toggle(
                    "add-sched",
                    "Schedule",
                    apply_sched,
                    &p,
                    cx,
                    |this, _, _, cx| {
                        this.add_apply_schedule = !this.add_apply_schedule;
                        cx.notify();
                    },
                ))
                .child(chip_toggle(
                    "add-adv",
                    "HTTP Auth / Referer",
                    adv,
                    &p,
                    cx,
                    |this, _, _, cx| {
                        this.add_show_advanced = !this.add_show_advanced;
                        if this.add_show_advanced {
                            this.adv_panel_anim_epoch = this.adv_panel_anim_epoch.wrapping_add(1);
                        }
                        cx.notify();
                    },
                )),
        );

    if adv {
        content = content.child(
            div()
                .id("add-adv-panel")
                .flex()
                .flex_row()
                .gap_2()
                .p_2()
                .rounded_md()
                .bg(p.bg_input)
                .border_1()
                .border_color(p.border)
                .child(labeled_field(
                    "Referer",
                    app.add_ref_input.clone().into_any_element(),
                    &p,
                ))
                .child(labeled_field(
                    "Username",
                    app.add_user_input.clone().into_any_element(),
                    &p,
                ))
                .child(labeled_field(
                    "Password",
                    password_field_row(
                        "add-pass-eye",
                        app.add_pass_input.clone().into_any_element(),
                        app.add_show_pass,
                        &p,
                        cx,
                        |this, _, _, cx| {
                            this.add_show_pass = !this.add_show_pass;
                            let masked = !this.add_show_pass;
                            this.add_pass_input
                                .update(cx, |i, cx| i.set_masked(masked, cx));
                            cx.notify();
                        },
                    )
                    .into_any_element(),
                    &p,
                ))
                .with_animation(
                    SharedString::from(format!("add-adv-anim-{adv_epoch}")),
                    Animation::new(Duration::from_millis(150)).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(-4.0 * (1.0 - delta))),
                ),
        );
    }

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "add-cancel",
            s.cancel,
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "add-submit",
            s.download,
            &p,
            true,
            cx,
            |this, _, _, cx| {
                this.submit_add_download(cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-add",
        s.new_download,
        670.0,
        app.modal_anim_epoch,
        &p,
        content.into_any_element(),
        footer,
        cx,
    )
}

fn render_batch_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let raw_pat = app.batch_pattern_input.read(cx).text();
    let preview: Vec<String> = if raw_pat.trim().is_empty() {
        Vec::new()
    } else if raw_pat.contains('\n') {
        parse_batch_text_input(&raw_pat).unwrap_or_default()
    } else {
        expand_batch_pattern(&raw_pat).unwrap_or_default()
    };
    let preview_count = preview.len();

    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(labeled_field(
            "Batch Pattern ([01-20:2], [a-f], {linux,macos} or URL list)",
            app.batch_pattern_input.clone().into_any_element(),
            &p,
        ))
        .child(labeled_field(
            "Target Directory",
            app.batch_dir_input.clone().into_any_element(),
            &p,
        ))
        .child(switch_row(
            "batch-group-sw",
            "Group generated items inside one collapsible Batch folder entry",
            app.batch_as_group,
            &p,
            cx,
            |this, _, _, cx| {
                this.batch_as_group = !this.batch_as_group;
                cx.notify();
            },
        ))
        .when(app.batch_as_group, |el| {
            el.child(labeled_field(
                "Batch Group Title (optional)",
                app.batch_group_title_input.clone().into_any_element(),
                &p,
            ))
        })
        .child(switch_row(
            "batch-paused-sw",
            "Add all batch items paused",
            app.batch_start_paused,
            &p,
            cx,
            |this, _, _, cx| {
                this.batch_start_paused = !this.batch_start_paused;
                cx.notify();
            },
        ))
        .child(
            div()
                .p_2()
                .rounded_md()
                .bg(p.bg_input)
                .border_1()
                .border_color(p.border)
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_secondary)
                        .child(format!("Preview: {preview_count} expanded URL(s)")),
                )
                .children(preview.into_iter().take(6).map(|u| {
                    div().text_xs().text_color(p.text_muted).truncate().child(u)
                })),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "batch-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "batch-go",
            format!("Enqueue {preview_count}"),
            &p,
            true,
            cx,
            |this, _, _, cx| {
                this.submit_batch(cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-batch",
        "Batch Download",
        600.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_conflict_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    path: PathBuf,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(p.text_primary)
                .child("A file already exists at the destination:"),
        )
        .child(
            div()
                .p_2()
                .rounded_md()
                .bg(p.bg_input)
                .border_1()
                .border_color(p.border)
                .text_xs()
                .text_color(p.warning)
                .child(path.display().to_string()),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "conf-skip",
            "Skip",
            &p,
            false,
            cx,
            move |this, _, w, cx| {
                this.resolve_conflict_action(id, FileExistsAction::Skip, w, cx);
            },
        ))
        .child(danger_btn(
            "conf-over",
            "Overwrite",
            &p,
            cx,
            move |this, _, w, cx| {
                this.resolve_conflict_action(id, FileExistsAction::Overwrite, w, cx);
            },
        ))
        .child(action_btn(
            "conf-ren",
            "Auto Rename",
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.resolve_conflict_action(id, FileExistsAction::Rename, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-conflict",
        "File Already Exists",
        500.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_hash_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let cur_algo = app.hash_algo;
    let computed = app.hash_computed.clone();
    let matched = app.hash_matched;

    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1p5()
                .children(
                    [
                        HashAlgorithm::Md5,
                        HashAlgorithm::Sha1,
                        HashAlgorithm::Sha256,
                        HashAlgorithm::Sha512,
                    ]
                    .into_iter()
                    .map(|alg| {
                        chip_toggle(
                            format!("hash-alg-{alg:?}"),
                            alg.label(),
                            cur_algo == alg,
                            &p,
                            cx,
                            move |this, _, _, cx| {
                                this.hash_algo = alg;
                                cx.notify();
                            },
                        )
                    }),
                ),
        )
        .child(labeled_field(
            "Expected Hex Checksum (optional)",
            app.hash_expected_input.clone().into_any_element(),
            &p,
        ))
        .when_some(computed, |el, digest| {
            let dig_copy = digest.clone();
            el.child(
                div()
                    .p_2()
                    .rounded_md()
                    .bg(p.bg_input)
                    .border_1()
                    .border_color(p.border)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.text_muted)
                                    .child("Computed Digest"),
                            )
                            .when_some(matched, |m_el, ok| {
                                m_el.child(badge_themed(
                                    if ok { "MATCH" } else { "MISMATCH" },
                                    if ok { p.success } else { p.danger },
                                    p.bg_surface,
                                    &p,
                                ))
                            }),
                    )
                    .child(div().text_xs().text_color(p.text_primary).child(digest))
                    .child(action_btn(
                        "hash-copy",
                        "Copy Digest",
                        &p,
                        false,
                        cx,
                        move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(dig_copy.clone()));
                            this.set_toast("Copied hash digest", false);
                            cx.notify();
                        },
                    )),
            )
        })
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "hash-close",
            "Close",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "hash-run",
            "Compute & Verify",
            &p,
            true,
            cx,
            move |this, _, _, cx| {
                this.run_hash_check(id, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-hash",
        "Check File Integrity (Hash)",
        560.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_convert_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let mp3 = app.convert_mode_mp3;
    let br = app.convert_mp3_bitrate;
    let use_vbr = app.convert_use_vbr;
    let vbr_q = app.convert_vbr_quality;
    let copy = app.convert_stream_copy;
    let busy = app.convert_busy;
    let status = app.convert_status.clone();

    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(chip_toggle(
                    "conv-mp3",
                    "Extract Audio (MP3)",
                    mp3,
                    &p,
                    cx,
                    move |this, _, _, cx| {
                        this.set_convert_target_ext(true, id, cx);
                    },
                ))
                .child(chip_toggle(
                    "conv-mp4",
                    "Convert / Remux (MP4)",
                    !mp3,
                    &p,
                    cx,
                    move |this, _, _, cx| {
                        this.set_convert_target_ext(false, id, cx);
                    },
                )),
        )
        .when(mp3, |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_center()
                            .child(div().text_xs().text_color(p.text_muted).child("Mode:"))
                            .child(chip_toggle(
                                "conv-mode-cbr",
                                "Constant Bitrate (CBR)",
                                !use_vbr,
                                &p,
                                cx,
                                |this, _, _, cx| {
                                    this.convert_use_vbr = false;
                                    cx.notify();
                                },
                            ))
                            .child(chip_toggle(
                                "conv-mode-vbr",
                                "Variable Quality (VBR)",
                                use_vbr,
                                &p,
                                cx,
                                |this, _, _, cx| {
                                    this.convert_use_vbr = true;
                                    cx.notify();
                                },
                            )),
                    )
                    .when(!use_vbr, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_xs().text_color(p.text_muted).child("Bitrate:"))
                                .children([128u32, 192, 256, 320].into_iter().map(|rate| {
                                    chip_toggle(
                                        format!("conv-cbr-{rate}"),
                                        format!("{rate} kbps"),
                                        br == rate,
                                        &p,
                                        cx,
                                        move |this, _, _, cx| {
                                            this.convert_mp3_bitrate = rate;
                                            cx.notify();
                                        },
                                    )
                                })),
                        )
                    })
                    .when(use_vbr, |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_xs().text_color(p.text_muted).child("VBR Level:"))
                                .children([(0u8, "V0 (Best)"), (2, "V2 (High)"), (4, "V4 (Standard)")].into_iter().map(|(q, lbl)| {
                                    chip_toggle(
                                        format!("conv-vbr-{q}"),
                                        lbl,
                                        vbr_q == q,
                                        &p,
                                        cx,
                                        move |this, _, _, cx| {
                                            this.convert_vbr_quality = q;
                                            cx.notify();
                                        },
                                    )
                                })),
                        )
                    }),
            )
        })
        .when(!mp3, |el| {
            el.child(switch_row(
                "conv-copy-sw",
                "Lossless stream copy (-c copy) when compatible",
                copy,
                &p,
                cx,
                |this, _, _, cx| {
                    this.convert_stream_copy = !this.convert_stream_copy;
                    cx.notify();
                },
            ))
        })
        .child(labeled_field(
            "Output File Path",
            app.convert_out_input.clone().into_any_element(),
            &p,
        ))
        .when_some(status, |el, s| {
            el.child(div().text_xs().text_color(p.accent).child(s))
        })
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "conv-cancel",
            "Close",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "conv-go",
            if busy { "Converting..." } else { "Convert Now" },
            &p,
            true,
            cx,
            move |this, _, _, cx| {
                this.run_media_convert(id, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-convert",
        "Convert Media (FFmpeg)",
        560.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_remote_qr_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let pairing = app.remote_pairing.clone();
    let loading = app.remote_loading;
    let pw = app.engine.ensure_remote_password();
    let pw_copy = pw.clone();

    let body = if loading && pairing.is_none() {
        div()
            .p_6()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(p.text_muted)
            .child("Starting local TLS/WebSocket Remote Control server...")
            .into_any_element()
    } else if let Some(pair) = pairing {
        let fdm_uri = pair.fdm_uri.clone();
        let https_url = pair.https_url.clone();
        let fdm_copy = fdm_uri.clone();
        let web_copy = https_url.clone();
        div()
            .flex()
            .flex_row()
            .gap_4()
            .items_center()
            .child(render_qr_element(&fdm_uri, 4.5))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_primary)
                            .child("Scan with FDM Remote Mobile App or open LAN Web UI"),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .p_2()
                            .rounded_md()
                            .bg(p.bg_input)
                            .border_1()
                            .border_color(p.border)
                            .overflow_hidden()
                            .text_xs()
                            .text_color(p.accent)
                            .child(ellipsis_text(fdm_uri)),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .p_2()
                            .rounded_md()
                            .bg(p.bg_input)
                            .border_1()
                            .border_color(p.border)
                            .overflow_hidden()
                            .text_xs()
                            .text_color(p.success)
                            .child(ellipsis_text(https_url)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.text_muted)
                            .child(format!("Password: {pw}")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_1p5()
                            .child(action_btn(
                                "qr-copy-fdm",
                                "Copy fdm:// URI",
                                &p,
                                false,
                                cx,
                                move |this, _, _, cx| {
                                    this.copy_to_clipboard(
                                        fdm_copy.clone(),
                                        "Copied fdm:// pairing URI",
                                        cx,
                                    );
                                },
                            ))
                            .child(action_btn(
                                "qr-copy-web",
                                "Copy Web URL",
                                &p,
                                false,
                                cx,
                                move |this, _, _, cx| {
                                    this.copy_to_clipboard(
                                        web_copy.clone(),
                                        "Copied HTTPS Remote URL",
                                        cx,
                                    );
                                },
                            ))
                            .child(action_btn(
                                "qr-copy-pw",
                                "Copy Password",
                                &p,
                                false,
                                cx,
                                move |this, _, _, cx| {
                                    this.copy_to_clipboard(
                                        pw_copy.clone(),
                                        "Copied Remote password",
                                        cx,
                                    );
                                },
                            ))
                            .child(action_btn(
                                "qr-regen",
                                "Regenerate Password",
                                &p,
                                false,
                                cx,
                                |this, _, _, cx| {
                                    this.regenerate_remote_password_action(cx);
                                    this.open_remote_qr_modal(cx);
                                },
                            )),
                    ),
            )
            .into_any_element()
    } else {
        let err_msg = app
            .remote_error
            .clone()
            .unwrap_or_else(|| "Remote control server is not running yet.".into());
        div()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(p.danger)
                    .child(format!("Remote control server unavailable: {err_msg}")),
            )
            .child(
                div().flex().flex_row().child(action_btn(
                    "qr-retry",
                    "Start / Retry Remote Server",
                    &p,
                    false,
                    cx,
                    |this, _, _, cx| {
                        this.open_remote_qr_modal(cx);
                    },
                )),
            )
            .into_any_element()
    };

    let footer = action_btn("qr-close", "Done", &p, true, cx, |this, _, w, cx| {
        this.close_modal(w, cx);
    })
    .into_any_element();

    modal_shell(
        "modal-qr",
        "Remote Control Pairing (LAN HTTPS + WebSocket)",
        660.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_edit_tag_modal(
    app: &mut FerriteApp,
    tag_id: Option<i64>,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let cur_hex = app.new_tag_color_hex.clone();
    let palette = [
        "#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#ec4899", "#06b6d4", "#84cc16",
    ];
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(labeled_field(
            "Tag Name",
            app.new_tag_name_input.clone().into_any_element(),
            &p,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(div().text_xs().text_color(p.text_secondary).child("Color:"))
                .children(palette.into_iter().map(|hex| {
                    let active = cur_hex.eq_ignore_ascii_case(hex);
                    div()
                        .id(SharedString::from(format!("tag-clr-{hex}")))
                        .w(p.spx(22.))
                        .h(p.spx(22.))
                        .rounded_full()
                        .bg(parse_hex_color(hex))
                        .border_2()
                        .border_color(if active { p.text_primary } else { p.border })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.new_tag_color_hex = hex.to_string();
                            cx.notify();
                        }))
                })),
        )
        .when(tag_id.is_some(), |el| {
            el.child(labeled_field(
                "Auto-assign extensions (comma/space separated)",
                app.tag_exts_input.clone().into_any_element(),
                &p,
            ))
            .child(labeled_field(
                "Default download folder for this tag (optional)",
                app.tag_dir_input.clone().into_any_element(),
                &p,
            ))
        })
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "tag-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "tag-save",
            if tag_id.is_some() {
                "Save Tag"
            } else {
                "Create Tag"
            },
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.submit_edit_tag(tag_id, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-tag",
        if tag_id.is_some() {
            "Edit Tag"
        } else {
            "Create New Tag"
        },
        440.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_change_source_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(labeled_field(
            "Primary Source URL",
            app.source_url_input.clone().into_any_element(),
            &p,
        ))
        .child(labeled_field(
            "Additional Mirrors (comma separated)",
            app.source_mirrors_input.clone().into_any_element(),
            &p,
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "src-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "src-save",
            "Save Source & Mirrors",
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.submit_change_source(id, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-src",
        "Update URL / Add Mirrors",
        560.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_move_dir_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(labeled_field(
            "New Destination Folder",
            app.move_dir_input.clone().into_any_element(),
            &p,
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "mv-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "mv-save",
            "Move Files",
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.submit_move_dir(id, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-move",
        "Move Download Files",
        480.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_rename_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(labeled_field(
            "New File Name",
            app.rename_file_input.clone().into_any_element(),
            &p,
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "ren-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "ren-save",
            "Rename",
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.submit_rename_download(id, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-rename",
        "Rename Output File",
        460.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_schedule_modal(
    app: &mut FerriteApp,
    id: DownloadId,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let mask = app.sched_days_mask;
    let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(switch_row(
            "dsch-en",
            "Enable Per-Download Schedule",
            app.sched_enabled,
            &p,
            cx,
            |this, _, _, cx| {
                this.sched_enabled = !this.sched_enabled;
                cx.notify();
            },
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_1()
                .children(days.into_iter().enumerate().map(|(bit, lbl)| {
                    let active = (mask & (1 << bit)) != 0;
                    chip_toggle(
                        format!("dsch-d-{bit}"),
                        lbl,
                        active,
                        &p,
                        cx,
                        move |this, _, _, cx| {
                            this.sched_days_mask ^= 1 << bit;
                            cx.notify();
                        },
                    )
                })),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(labeled_field(
                    "Start Time (HH:MM)",
                    app.sched_start_input.clone().into_any_element(),
                    &p,
                ))
                .child(labeled_field(
                    "End Time (HH:MM)",
                    app.sched_end_input.clone().into_any_element(),
                    &p,
                )),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "dsch-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "dsch-save",
            "Save Schedule",
            &p,
            true,
            cx,
            move |this, _, w, cx| {
                this.submit_download_schedule(id, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-sched",
        "Download Schedule",
        480.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_delete_confirm_modal(
    app: &mut FerriteApp,
    ids: Vec<DownloadId>,
    cx: &mut Context<FerriteApp>,
) -> AnyElement {
    let p = app.theme;
    let count = ids.len();
    let rem = app.delete_remember_choice;
    let ids_list_only = ids.clone();
    let ids_files = ids;

    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(p.text_primary)
                .child(if count == 1 {
                    "Remove this download from Ferrite DM?".to_string()
                } else {
                    format!("Remove {count} selected downloads from Ferrite DM?")
                }),
        )
        .child(
            div()
                .id("del-remember-row")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.delete_remember_choice = !this.delete_remember_choice;
                    cx.notify();
                }))
                .child(fdm_checkbox(rem, &p))
                .child(
                    div()
                        .text_xs()
                        .text_color(p.text_muted)
                        .child("Remember my choice (can be reset in Preferences -> General)"),
                ),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "del-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "del-list-only",
            "Remove from List",
            &p,
            false,
            cx,
            move |this, _, w, cx| {
                this.confirm_delete_downloads(ids_list_only.clone(), false, rem, w, cx);
            },
        ))
        .child(danger_btn(
            "del-with-files",
            "Delete Files from Disk",
            &p,
            cx,
            move |this, _, w, cx| {
                this.confirm_delete_downloads(ids_files.clone(), true, rem, w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-delete-confirm",
        "Confirm Delete",
        470.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_quit_confirm_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let active_count = app
        .all_downloads
        .iter()
        .filter(|d| d.state.is_running_or_seeding() || d.state == fdm_core::DownloadState::Queued)
        .count();
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(p.text_primary)
                .child(format!(
                    "There are {active_count} active download(s) currently running. Are you sure you want to exit Ferrite DM?"
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(p.text_muted)
                .child("Resumable transfers will pause gracefully and resume on next launch."),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "quit-cancel",
            "Keep Running",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(danger_btn(
            "quit-confirm",
            "Quit Ferrite DM",
            &p,
            cx,
            |_, _, _, cx| {
                cx.quit();
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-quit",
        "Exit Confirmation",
        440.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_about_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let s = i18n::l(&app.settings_cfg.app.locale_name);
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .w(p.spx(48.))
                        .h(p.spx(48.))
                        .rounded(p.radius(12., 4.))
                        .bg(p.accent_soft)
                        .border_1()
                        .border_color(p.accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(svg_icon("icons/logo.svg", p.spx_f32(28.), p.accent)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(p.text_primary)
                                .child("Ferrite DM"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.text_muted)
                                .child(format!(
                                    "Version {} · Clean-room Rust engine with 100% FDM feature compatibility",
                                    env!("CARGO_PKG_VERSION")
                                )),
                        ),
                ),
        )
        .child(
            div()
                .p_3()
                .rounded(p.radius(8., 3.))
                .bg(p.bg_input)
                .border_1()
                .border_color(p.separator)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.success).child("Privacy & Safety Guarantees"))
                .child(div().text_xs().text_color(p.text_secondary).child("• Zero telemetry, zero analytics pings, zero cloud accounts"))
                .child(div().text_xs().text_color(p.text_secondary).child("• Zero sponsored offers or bundled installers"))
                .child(div().text_xs().text_color(p.text_secondary).child("• Local SQLite state store & FDM legacy db.sqlite migration"))
                .child(div().text_xs().text_color(p.text_secondary).child("• Multi-connection HTTP/FTP, BitTorrent, HLS/DASH, JS Add-ons & LAN QR Remote")),
        )
        .into_any_element();

    let footer = action_btn("about-ok", s.close, &p, true, cx, |this, _, w, cx| {
        this.close_modal(w, cx);
    })
    .into_any_element();

    modal_shell(
        "modal-about",
        "About Ferrite DM",
        500.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}

fn render_portable_modal(app: &mut FerriteApp, cx: &mut Context<FerriteApp>) -> AnyElement {
    let p = app.theme;
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_xs()
                .text_color(p.text_secondary)
                .child("Creates a self-contained portable folder with a snapshot of your download database and executable."),
        )
        .child(labeled_field(
            "Portable Output Directory",
            app.portable_dir_input.clone().into_any_element(),
            &p,
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(action_btn(
            "port-cancel",
            "Cancel",
            &p,
            false,
            cx,
            |this, _, w, cx| {
                this.close_modal(w, cx);
            },
        ))
        .child(action_btn(
            "port-create",
            "Create Portable Bundle",
            &p,
            true,
            cx,
            |this, _, w, cx| {
                this.create_portable_action(w, cx);
            },
        ))
        .into_any_element();

    modal_shell(
        "modal-portable",
        "Create Portable Version",
        500.0,
        app.modal_anim_epoch,
        &p,
        body,
        footer,
        cx,
    )
}
