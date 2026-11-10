use crate::app::{
    CloseWindowAction, EscapeModalAction, MinimizeWindowAction, ToggleFullscreenAction,
    ZoomWindowAction,
};
use crate::helpers::{
    available_disk_space_bytes, download_icon_and_color, expand_path_preview, extract_domain_label,
    format_bytes, format_eta, format_speed, open_path_os, reveal_path_os, state_badge_info,
    zoom_rem_size_px,
};
use crate::i18n;
use crate::text_input::TextInput;
use crate::theme::ThemePalette;
use crate::ui_widgets::{
    action_btn, badge_themed, chip_toggle, danger_btn, ellipsis_text, fdm_checkbox, ghost_icon_btn,
    labeled_field, slim_progress_bar_active, svg_icon, switch_row,
};
use fdm_cli::{AddProbeOutcome, DownloadEngine};
use fdm_core::{
    DownloadId, DownloadPriority, DownloadRecord, DownloadState, DownloadType, EngineEvent,
    NewDownloadRequest,
};
use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight,
    PathPromptOptions, Render, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowControlArea, WindowOptions, div, ease_out_quint, prelude::*, px, rgb, size,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub fn open_standalone_add_window(
    rt: Arc<tokio::runtime::Runtime>,
    engine: Arc<DownloadEngine>,
    prefill_url: String,
    cx: &mut App,
) {
    let bounds = Bounds::centered(None, size(px(580.0), px(590.0)), cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Add Download — Ferrite DM".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    let _ = cx.open_window(opts, move |window, cx| {
        cx.new(|cx| StandaloneAddWindow::new(rt, engine, prefill_url, window, cx))
    });
}

pub fn open_standalone_download_window(
    rt: Arc<tokio::runtime::Runtime>,
    engine: Arc<DownloadEngine>,
    id: DownloadId,
    cx: &mut App,
) {
    let bounds = Bounds::centered(None, size(px(540.0), px(340.0)), cx);
    let opts = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(format!("Download #{id} — Ferrite DM").into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.), px(8.))),
        }),
        ..Default::default()
    };
    let _ = cx.open_window(opts, move |window, cx| {
        cx.new(|cx| StandaloneDownloadWindow::new(rt, engine, id, window, cx))
    });
}

pub struct StandaloneAddWindow {
    rt: Arc<tokio::runtime::Runtime>,
    engine: Arc<DownloadEngine>,
    focus_handle: FocusHandle,
    theme: ThemePalette,
    url_input: Entity<TextInput>,
    dir_input: Entity<TextInput>,
    name_input: Entity<TextInput>,
    ref_input: Entity<TextInput>,
    priority: DownloadPriority,
    start_paused: bool,
    play_asap: bool,
    apply_schedule: bool,
    probe_busy: bool,
    probe_outcome: Option<AddProbeOutcome>,
    selected_files: HashSet<usize>,
    selected_variant: usize,
    status_msg: Option<(String, bool)>,
    enter_epoch: u64,
    probe_anim_epoch: u64,
}

impl Focusable for StandaloneAddWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl StandaloneAddWindow {
    pub fn new(
        rt: Arc<tokio::runtime::Runtime>,
        engine: Arc<DownloadEngine>,
        prefill_url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cfg = engine.get_config();
        let theme = ThemePalette::from_config(
            cfg.app.theme_mode,
            cfg.app.ui_style,
            cfg.app.overall_zoom_pct,
            cfg.app.fonts_zoom_pct,
        );
        window.set_rem_size(px(zoom_rem_size_px(cfg.app.overall_zoom_pct)));

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

        let def_dir = cfg.core.default_download_path.to_string_lossy().into_owned();
        let url_input = mk_input(cx, "https://..., magnet:?xt=..., or .torrent path", &prefill_url);
        let dir_input = mk_input(cx, "Destination directory", &def_dir);
        let name_input = mk_input(cx, "Optional output filename", "");
        let ref_input = mk_input(cx, "HTTP Referer (optional)", "");

        let mut view = Self {
            rt,
            engine,
            focus_handle: cx.focus_handle(),
            theme,
            url_input: url_input.clone(),
            dir_input,
            name_input,
            ref_input,
            priority: DownloadPriority::Normal,
            start_paused: false,
            play_asap: false,
            apply_schedule: cfg.core.enable_scheduler_for_new_downloads,
            probe_busy: false,
            probe_outcome: None,
            selected_files: HashSet::new(),
            selected_variant: 0,
            status_msg: None,
            enter_epoch: 1,
            probe_anim_epoch: if !prefill_url.trim().is_empty() { 1 } else { 0 },
        };

        cx.observe(&view.url_input, |_, _, cx| cx.notify()).detach();
        cx.observe(&view.dir_input, |_, _, cx| cx.notify()).detach();
        cx.observe(&view.name_input, |_, _, cx| cx.notify()).detach();
        let fh = url_input.read(cx).focus_handle.clone();
        window.focus(&fh);
        if !prefill_url.trim().is_empty() {
            view.trigger_probe(cx);
        }
        view
    }

    fn browse_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Save Folder".into()),
        });
        let input = self.dir_input.clone();
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

    fn trigger_probe(&mut self, cx: &mut Context<Self>) {
        let url = self.url_input.read(cx).text().trim().to_string();
        if url.is_empty() {
            self.status_msg = Some(("Enter a URL, magnet URI, or .torrent path first".into(), true));
            cx.notify();
            return;
        }
        self.probe_busy = true;
        self.probe_outcome = None;
        self.status_msg = None;

        let eng = Arc::clone(&self.engine);
        let (tx, rx) = oneshot::channel();
        self.rt.spawn(async move {
            let _ = tx.send(eng.probe_source(&url).await.map_err(|e| e.to_string()));
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(res) = rx.await {
                let _ = this.update(cx, |view, cx| {
                    view.probe_busy = false;
                    match res {
                        Ok(outcome) => {
                            let dir_s = outcome.resolved_dir.to_string_lossy().into_owned();
                            let name_s = outcome.resolved_file_name.clone();
                            view.dir_input.update(cx, |i, cx| i.set_text(&dir_s, cx));
                            view.name_input.update(cx, |i, cx| i.set_text(&name_s, cx));
                            view.selected_files = outcome
                                .files
                                .iter()
                                .filter(|f| f.priority != DownloadPriority::DontDownload)
                                .map(|f| f.index)
                                .collect();
                            view.selected_variant = 0;
                            view.probe_outcome = Some(outcome);
                            view.probe_anim_epoch = view.probe_anim_epoch.wrapping_add(1);
                        }
                        Err(e) => {
                            view.status_msg = Some((format!("Probe failed: {e}"), true));
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut source_url = self.url_input.read(cx).text().trim().to_string();
        if source_url.is_empty() {
            self.status_msg = Some(("Download source URL cannot be empty".into(), true));
            cx.notify();
            return;
        }
        let dir_str = self.dir_input.read(cx).text().trim().to_string();
        let name_str = self.name_input.read(cx).text().trim().to_string();
        let referer = self.ref_input.read(cx).text().trim().to_string();

        let mut req = NewDownloadRequest::from_url(source_url.clone());
        let mut chosen_type: Option<DownloadType> = None;

        if let Some(ref outcome) = self.probe_outcome {
            if outcome.download_type == DownloadType::MediaParser {
                if let Some(ref media) = outcome.media {
                    if let Some(var) = media.variants.get(self.selected_variant) {
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
                let mut indices: Vec<usize> = self.selected_files.iter().copied().collect();
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
        req.priority = self.priority;
        req.start_paused = self.start_paused;
        req.sequential_play_asap = self.play_asap;
        if self.apply_schedule {
            let mut s = self.engine.get_config().core.default_schedule;
            s.enabled = true;
            req.schedule = Some(s);
        }

        let eng = Arc::clone(&self.engine);
        let (tx, rx) = oneshot::channel();
        self.rt.spawn(async move {
            let _ = tx.send(eng.add_download(req).await.map(|r| r.id).map_err(|e| e.to_string()));
        });

        let win = window.window_handle();
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(res) = rx.await {
                match res {
                    Ok(_) => {
                        let _ = win.update(cx, |_, w, _| {
                            w.remove_window();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.status_msg = Some((format!("Failed to add download: {e}"), true));
                            cx.notify();
                        });
                    }
                }
            }
        })
        .detach();
    }
}

impl Render for StandaloneAddWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme;
        let loc = i18n::l(&self.engine.get_config().app.locale_name);
        let enter_epoch = self.enter_epoch;
        let probe_epoch = self.probe_anim_epoch;
        let show_browse = self.engine.get_config().app.show_save_as_button;
        let url_val = self.url_input.read(cx).text();
        let dir_raw = self.dir_input.read(cx).text();
        let name_val = self.name_input.read(cx).text();
        let expanded_dir = expand_path_preview(
            &dir_raw,
            &url_val,
            if name_val.trim().is_empty() {
                "download.bin"
            } else {
                &name_val
            },
        );
        let avail_disk = available_disk_space_bytes(std::path::Path::new(&expanded_dir));

        div()
            .size_full()
            .bg(p.bg_100)
            .text_color(p.text_primary)
            .key_context("FerriteStandalone")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &EscapeModalAction, w, _| {
                w.remove_window();
            }))
            .on_action(cx.listener(|_, _: &CloseWindowAction, w, _| {
                w.remove_window();
            }))
            .on_action(cx.listener(|_, _: &MinimizeWindowAction, w, _| {
                w.minimize_window();
            }))
            .on_action(cx.listener(|_, _: &ZoomWindowAction, w, _| {
                w.zoom_window();
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreenAction, w, _| {
                w.toggle_fullscreen();
            }))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(28.))
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
                    .child(div().w(px(72.)))
                    .child(
                        div()
                            .text_size(p.fpx(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(loc.new_download),
                    )
                    .child(div().w(px(72.))),
            )
            .when_some(self.status_msg.clone(), |el, (msg, is_err)| {
                el.child(
                    div()
                        .px_4()
                        .py_1p5()
                        .bg(if is_err { p.danger } else { p.accent_soft })
                        .text_size(p.fpx(11.))
                        .text_color(if is_err { p.on_accent } else { p.text_primary })
                        .child(msg),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .id("sa-add-scroll")
                    .overflow_y_scroll()
                    .p_4()
                    .child(
                        div()
                            .id("sa-add-anim-wrap")
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
                                        loc.url_input_label,
                                        self.url_input.clone().into_any_element(),
                                        &p,
                                    ))
                                    .child(action_btn(
                                        "sa-probe",
                                        if self.probe_busy { loc.probing } else { loc.probe },
                                        &p,
                                        false,
                                        cx,
                                        |this, _, _, cx| {
                                            this.trigger_probe(cx);
                                        },
                                    )),
                            )
                            .when_some(self.probe_outcome.clone(), |el, out| {
                                let sel_files = self.selected_files.clone();
                                let sel_var = self.selected_variant;
                                el.child(
                                    div()
                                        .id("sa-probe-card")
                                        .p_2p5()
                                        .rounded(p.radius(8., 3.))
                                        .bg(p.bg_200)
                                        .border_1()
                                        .border_color(p.border)
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
                                                        .text_size(p.fpx(12.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(p.text_primary)
                                                        .truncate()
                                                        .child(out.title),
                                                )
                                                .child(badge_themed(
                                                    format!("{:?}", out.download_type),
                                                    p.accent,
                                                    p.accent_soft,
                                                    &p,
                                                )),
                                        )
                                        .when(!out.files.is_empty(), |el2| {
                                            el2.child(
                                                div()
                                                    .max_h(p.spx(110.))
                                                    .id("sa-bt-files")
                                                    .overflow_y_scroll()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .children(out.files.into_iter().take(40).map(|f| {
                                                        let f_idx = f.index;
                                                        let checked = sel_files.contains(&f_idx);
                                                        div()
                                                            .id(SharedString::from(format!("sa-f-{f_idx}")))
                                                            .flex()
                                                            .flex_row()
                                                            .items_center()
                                                            .justify_between()
                                                            .gap_2()
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                                if !this.selected_files.remove(&f_idx) {
                                                                    this.selected_files.insert(f_idx);
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
                                                                            .text_size(p.fpx(11.))
                                                                            .text_color(p.text_primary)
                                                                            .truncate()
                                                                            .child(f.relative_path),
                                                                    ),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(p.fpx(11.))
                                                                    .text_color(p.text_muted)
                                                                    .child(format_bytes(f.size_bytes)),
                                                            )
                                                    })),
                                            )
                                        })
                                        .when_some(out.media, |el2, m| {
                                            el2.child(
                                                div().flex().flex_row().flex_wrap().gap_1().children(
                                                    m.variants.into_iter().enumerate().take(8).map(|(i, v)| {
                                                        chip_toggle(
                                                            format!("sa-var-{i}"),
                                                            format!("{} ({})", v.title, v.ext),
                                                            sel_var == i,
                                                            &p,
                                                            cx,
                                                            move |this, _, _, cx| {
                                                                this.selected_variant = i;
                                                                cx.notify();
                                                            },
                                                        )
                                                    }),
                                                ),
                                            )
                                        })
                                        .with_animation(
                                            SharedString::from(format!("sa-probe-anim-{probe_epoch}")),
                                            Animation::new(Duration::from_millis(160))
                                                .with_easing(ease_out_quint()),
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
                                        loc.save_folder_label,
                                        self.dir_input.clone().into_any_element(),
                                        &p,
                                    ))
                                    .when(show_browse, |row| {
                                        row.child(action_btn(
                                            "sa-browse-dir",
                                            loc.browse,
                                            &p,
                                            false,
                                            cx,
                                            |this, _, _, cx| {
                                                this.browse_dir(cx);
                                            },
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .text_size(p.fpx(11.))
                                    .text_color(p.text_muted)
                                    .child(div().truncate().child(format!("Resolved: {expanded_dir}")))
                                    .when_some(avail_disk, |el, free| {
                                        el.child(badge_themed(
                                            format!("Free: {}", format_bytes(free)),
                                            p.text_secondary,
                                            p.bg_300,
                                            &p,
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_3()
                                    .child(labeled_field(
                                        loc.file_name_label,
                                        self.name_input.clone().into_any_element(),
                                        &p,
                                    ))
                                    .child(labeled_field(
                                        "HTTP Referer",
                                        self.ref_input.clone().into_any_element(),
                                        &p,
                                    )),
                            )
                            .child({
                                let cur_prio = self.priority;
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_size(p.fpx(12.)).text_color(p.text_secondary).child("Priority"))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_1()
                                            .children(
                                                [
                                                    (DownloadPriority::Low, "Low"),
                                                    (DownloadPriority::Normal, "Normal"),
                                                    (DownloadPriority::High, "High"),
                                                ]
                                                .into_iter()
                                                .map(|(pr, lbl)| {
                                                    chip_toggle(
                                                        format!("sa-prio-{pr:?}"),
                                                        lbl,
                                                        cur_prio == pr,
                                                        &p,
                                                        cx,
                                                        move |this, _, _, cx| {
                                                            this.priority = pr;
                                                            cx.notify();
                                                        },
                                                    )
                                                }),
                                            ),
                                    )
                            })
                            .child(switch_row(
                                "sa-paused",
                                "Add in paused state",
                                self.start_paused,
                                &p,
                                cx,
                                |this, _, _, cx| {
                                    this.start_paused = !this.start_paused;
                                    cx.notify();
                                },
                            ))
                            .child(switch_row(
                                "sa-asap",
                                "Download sequentially (Play ASAP)",
                                self.play_asap,
                                &p,
                                cx,
                                |this, _, _, cx| {
                                    this.play_asap = !this.play_asap;
                                    cx.notify();
                                },
                            ))
                            .child(switch_row(
                                "sa-sched",
                                "Apply default scheduler window",
                                self.apply_schedule,
                                &p,
                                cx,
                                |this, _, _, cx| {
                                    this.apply_schedule = !this.apply_schedule;
                                    cx.notify();
                                },
                            ))
                            .with_animation(
                                SharedString::from(format!("sa-win-enter-{enter_epoch}")),
                                Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                                |el, delta| {
                                    el.opacity((0.4 + 0.6 * delta).min(1.0))
                                        .mt(px(8.0 * (1.0 - delta)))
                                },
                            ),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_3()
                    .bg(p.bg_200)
                    .border_t_1()
                    .border_color(p.separator)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(action_btn("sa-cancel", loc.cancel, &p, false, cx, |_, _, w, _| {
                        w.remove_window();
                    }))
                    .child(action_btn(
                        "sa-submit",
                        loc.download,
                        &p,
                        true,
                        cx,
                        |this, _, w, cx| {
                            this.submit(w, cx);
                        },
                    )),
            )
    }
}

pub struct StandaloneDownloadWindow {
    rt: Arc<tokio::runtime::Runtime>,
    engine: Arc<DownloadEngine>,
    id: DownloadId,
    focus_handle: FocusHandle,
    theme: ThemePalette,
    record: Option<DownloadRecord>,
    error_banner: Option<String>,
    enter_epoch: u64,
}

impl Focusable for StandaloneDownloadWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl StandaloneDownloadWindow {
    pub fn new(
        rt: Arc<tokio::runtime::Runtime>,
        engine: Arc<DownloadEngine>,
        id: DownloadId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cfg = engine.get_config();
        let theme = ThemePalette::from_config(
            cfg.app.theme_mode,
            cfg.app.ui_style,
            cfg.app.overall_zoom_pct,
            cfg.app.fonts_zoom_pct,
        );
        window.set_rem_size(px(zoom_rem_size_px(cfg.app.overall_zoom_pct)));

        let mut rx = engine.subscribe();
        let (tx, mut mpsc_rx) = mpsc::unbounded_channel();
        rt.spawn(async move {
            while let Ok(ev) = rx.recv().await {
                let relevant = match &ev {
                    EngineEvent::Updated { id: ev_id }
                    | EngineEvent::Completed { id: ev_id }
                    | EngineEvent::Removed { id: ev_id }
                    | EngineEvent::Failed { id: ev_id, .. }
                    | EngineEvent::FileConflict { id: ev_id, .. } => *ev_id == id,
                    EngineEvent::SettingsUpdated => true,
                    _ => false,
                };
                if relevant && tx.send(ev).is_err() {
                    break;
                }
            }
        });

        let win = window.window_handle();
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            while let Some(ev) = mpsc_rx.recv().await {
                if matches!(ev, EngineEvent::Removed { id: ev_id } if ev_id == id) {
                    let _ = win.update(cx, |_, w, _| w.remove_window());
                    break;
                }
                let ok = this
                    .update(cx, |view, cx| {
                        view.record = view.engine.get_download(view.id);
                        let cfg = view.engine.get_config();
                        view.theme = ThemePalette::from_config(
                            cfg.app.theme_mode,
                            cfg.app.ui_style,
                            cfg.app.overall_zoom_pct,
                            cfg.app.fonts_zoom_pct,
                        );
                        cx.notify();
                    })
                    .is_ok();
                if !ok {
                    break;
                }
            }
        })
        .detach();

        let record = engine.get_download(id);
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);

        Self {
            rt,
            engine,
            id,
            focus_handle,
            theme,
            record,
            error_banner: None,
            enter_epoch: 1,
        }
    }

    fn toggle_start_pause(&mut self, cx: &mut Context<Self>) {
        let Some(rec) = self.record.clone() else {
            return;
        };
        let eng = Arc::clone(&self.engine);
        let id = self.id;
        let running = rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued;
        let (tx, rx) = oneshot::channel();
        self.rt.spawn(async move {
            let res = if running {
                eng.pause_download(id).await
            } else {
                eng.resume_download(id).await
            };
            let _ = tx.send(res.map_err(|e| e.to_string()));
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |view, cx| {
                    view.error_banner = Some(e);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn stop_download(&mut self, cx: &mut Context<Self>) {
        let eng = Arc::clone(&self.engine);
        let id = self.id;
        self.rt.spawn(async move {
            let _ = eng.cancel_download(id).await;
        });
        cx.notify();
    }
}

impl Render for StandaloneDownloadWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.theme;
        let loc = i18n::l(&self.engine.get_config().app.locale_name);
        let enter_epoch = self.enter_epoch;
        let Some(rec) = self.record.clone() else {
            return div()
                .size_full()
                .bg(p.bg_100)
                .text_color(p.text_primary)
                .key_context("FerriteStandalone")
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(|_, _: &EscapeModalAction, w, _| w.remove_window()))
                .on_action(cx.listener(|_, _: &CloseWindowAction, w, _| w.remove_window()))
                .on_action(cx.listener(|_, _: &MinimizeWindowAction, w, _| w.minimize_window()))
                .on_action(cx.listener(|_, _: &ZoomWindowAction, w, _| w.zoom_window()))
                .on_action(cx.listener(|_, _: &ToggleFullscreenAction, w, _| w.toggle_fullscreen()))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(div().text_size(p.fpx(13.)).child("Download removed"))
                .child(action_btn("sd-close-empty", loc.close, &p, true, cx, |_, _, w, _| {
                    w.remove_window();
                }));
        };

        let (type_icon, type_color) = download_icon_and_color(&rec);
        let (st_label, st_color) = state_badge_info(&rec, &p);
        let frac = rec.progress_fraction();
        let pct = (frac * 100.0).round() as u32;
        let running = rec.state.is_running_or_seeding() || rec.state == DownloadState::Queued;
        let completed = rec.state == DownloadState::Completed;
        let size_txt = match rec.total_bytes {
            Some(t) => format!("{} / {}", format_bytes(rec.downloaded_bytes), format_bytes(t)),
            None => format_bytes(rec.downloaded_bytes),
        };
        let url_for_copy = rec.url.clone();
        let out_path = rec.output_path();
        let out_for_open = out_path.clone();
        let out_for_rev = out_path.clone();

        div()
            .size_full()
            .bg(p.bg_100)
            .text_color(p.text_primary)
            .key_context("FerriteStandalone")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &EscapeModalAction, w, _| {
                w.remove_window();
            }))
            .on_action(cx.listener(|_, _: &CloseWindowAction, w, _| {
                w.remove_window();
            }))
            .on_action(cx.listener(|_, _: &MinimizeWindowAction, w, _| {
                w.minimize_window();
            }))
            .on_action(cx.listener(|_, _: &ZoomWindowAction, w, _| {
                w.zoom_window();
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreenAction, w, _| {
                w.toggle_fullscreen();
            }))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(28.))
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
                    .child(div().w(px(72.)))
                    .child(
                        div()
                            .text_size(p.fpx(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(format!("Download #{}", rec.id)),
                    )
                    .child(div().w(px(72.))),
            )
            .when_some(self.error_banner.clone(), |el, err| {
                el.child(
                    div()
                        .px_4()
                        .py_1p5()
                        .bg(p.danger)
                        .text_size(p.fpx(11.))
                        .text_color(p.on_accent)
                        .child(err),
                )
            })
            .child(
                div()
                    .id("sd-win-enter")
                    .flex_1()
                    .min_h_0()
                    .p_5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(p.spx(40.))
                                    .h(p.spx(40.))
                                    .rounded(p.radius(10., 3.))
                                    .bg(type_color)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(svg_icon(type_icon, p.spx_f32(20.), rgb(0xffffff).into())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(p.fpx(15.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(p.text_primary)
                                            .child(ellipsis_text(rec.title.clone())),
                                    )
                                    .child(
                                        div()
                                            .text_size(p.fpx(11.))
                                            .text_color(p.text_muted)
                                            .child(ellipsis_text(format!(
                                                "{} · {}",
                                                extract_domain_label(&rec.url),
                                                out_path.display()
                                            ))),
                                    ),
                            )
                            .child(badge_themed(st_label, st_color, p.bg_300, &p)),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded(p.radius(8., 3.))
                            .bg(p.bg_200)
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
                                    .child(
                                        div()
                                            .text_size(p.fpx(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(p.text_primary)
                                            .child(format!("{pct}% · {size_txt}")),
                                    )
                                    .child(
                                        div()
                                            .text_size(p.fpx(11.))
                                            .text_color(p.text_muted)
                                            .child(format!("ETA: {}", format_eta(rec.eta_secs))),
                                    ),
                            )
                            .child(slim_progress_bar_active(
                                format!("sd-prog-{}", rec.id),
                                frac,
                                6.0 * p.ui_scale(),
                                st_color,
                                p.bg_400,
                                rec.state == DownloadState::Downloading,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .text_size(p.fpx(11.))
                                    .text_color(p.text_secondary)
                                    .child(format!(
                                        "↓ {}  ·  ↑ {}",
                                        format_speed(rec.download_speed_bps),
                                        format_speed(rec.upload_speed_bps)
                                    ))
                                    .child(format!("{} active connections", rec.connections_active)),
                            ),
                    )
                    .when_some(rec.last_error.clone(), |el, err| {
                        el.child(
                            div()
                                .text_size(p.fpx(11.))
                                .text_color(p.danger)
                                .truncate()
                                .child(err),
                        )
                    })
                    .with_animation(
                        SharedString::from(format!("sd-win-enter-{enter_epoch}")),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        |el, delta| {
                            el.opacity((0.4 + 0.6 * delta).min(1.0))
                                .mt(px(8.0 * (1.0 - delta)))
                        },
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_3()
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
                            .child(ghost_icon_btn(
                                "sd-copy",
                                "icons/content_copy.svg",
                                p.spx_f32(15.),
                                p.text_secondary,
                                p.bg_300,
                                cx,
                                move |_, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        url_for_copy.clone(),
                                    ));
                                },
                            ))
                            .child(action_btn(
                                "sd-reveal",
                                "Finder",
                                &p,
                                false,
                                cx,
                                move |this, _, _, cx| {
                                    if let Err(e) = reveal_path_os(&out_for_rev) {
                                        this.error_banner = Some(e.to_string());
                                        cx.notify();
                                    }
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .when(!completed, |el| {
                                el.child(danger_btn(
                                    "sd-stop",
                                    "Stop",
                                    &p,
                                    cx,
                                    |this, _, _, cx| {
                                        this.stop_download(cx);
                                    },
                                ))
                                .child(action_btn(
                                    "sd-toggle",
                                    if running { "Pause" } else { "Start" },
                                    &p,
                                    true,
                                    cx,
                                    |this, _, _, cx| {
                                        this.toggle_start_pause(cx);
                                    },
                                ))
                            })
                            .when(completed, |el| {
                                el.child(action_btn(
                                    "sd-open",
                                    "Open File",
                                    &p,
                                    true,
                                    cx,
                                    move |this, _, _, cx| {
                                        if let Err(e) = open_path_os(&out_for_open) {
                                            this.error_banner = Some(e.to_string());
                                            cx.notify();
                                        }
                                    },
                                ))
                            })
                            .child(action_btn(
                                "sd-close",
                                loc.close,
                                &p,
                                false,
                                cx,
                                |_, _, w, _| {
                                    w.remove_window();
                                },
                            )),
                    ),
            )
    }
}
