use crate::theme::ThemePalette;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Div, Element, ElementId, FontWeight,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Rgba, ShapedLine,
    SharedString, Stateful, Style, Svg, TextStyle, Window, div, ease_in_out, ease_out_quint,
    prelude::*, px, svg,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
struct SwitchAnimState {
    entries: HashMap<String, (u64, Instant)>,
}

fn switch_anim_store() -> &'static Mutex<SwitchAnimState> {
    static STORE: OnceLock<Mutex<SwitchAnimState>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(SwitchAnimState::default()))
}

pub fn mark_switch_toggled(id: &str) {
    if let Ok(mut guard) = switch_anim_store().lock() {
        let next_epoch = guard.entries.get(id).map(|(e, _)| *e + 1).unwrap_or(1);
        guard.entries.insert(id.to_string(), (next_epoch, Instant::now()));
    }
}

pub fn active_switch_epoch(id: &str) -> Option<u64> {
    let guard = switch_anim_store().lock().ok()?;
    let (epoch, at) = guard.entries.get(id)?;
    if at.elapsed() < Duration::from_millis(240) {
        Some(*epoch)
    } else {
        None
    }
}

pub fn lerp_hsla(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    let ra = Rgba::from(a);
    let rb = Rgba::from(b);
    let blended = Rgba {
        r: ra.r + (rb.r - ra.r) * t,
        g: ra.g + (rb.g - ra.g) * t,
        b: ra.b + (rb.b - ra.b) * t,
        a: ra.a + (rb.a - ra.a) * t,
    };
    Hsla::from(blended)
}

pub fn svg_icon(path: &'static str, size_px: f32, color: Hsla) -> Svg {
    svg()
        .path(path)
        .w(px(size_px))
        .h(px(size_px))
        .flex_shrink_0()
        .text_color(color)
}

pub fn badge_themed(text: impl Into<String>, fg: Hsla, bg: Hsla, p: &ThemePalette) -> Div {
    div()
        .flex_shrink_0()
        .px_1p5()
        .py(px(2.))
        .rounded(p.radius(4., 2.))
        .bg(bg)
        .text_color(fg)
        .text_size(p.fpx(11.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(text.into())
}

pub fn action_btn<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    p: &ThemePalette,
    primary: bool,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let (bg, hover_bg, fg, border_c) = if primary {
        (p.accent, p.accent_hover, p.on_accent, p.accent)
    } else {
        (p.bg_surface, p.bg_elevated, p.text_primary, p.border)
    };
    div()
        .id(ElementId::Name(id.into()))
        .px_3()
        .py(p.spx(6.))
        .rounded(p.radius(8., 3.))
        .bg(bg)
        .hover(move |s| s.bg(hover_bg))
        .border_1()
        .border_color(border_c)
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .text_size(p.fpx(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .whitespace_nowrap()
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(label.into())
}

pub fn icon_action_btn<T: 'static>(
    id: impl Into<SharedString>,
    icon_path: &'static str,
    label: Option<String>,
    p: &ThemePalette,
    primary: bool,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let (bg, hover_bg, fg, border_c) = if primary {
        (p.accent, p.accent_hover, p.on_accent, p.accent)
    } else {
        (p.bg_surface, p.bg_elevated, p.text_primary, p.border)
    };
    let mut el = div()
        .id(ElementId::Name(id.into()))
        .h(p.spx(32.))
        .px(if label.is_some() { p.spx(12.) } else { p.spx(8.) })
        .rounded(p.radius(8., 3.))
        .bg(bg)
        .hover(move |s| s.bg(hover_bg))
        .border_1()
        .border_color(if primary || p.is_classic() {
            border_c
        } else {
            gpui::transparent_black()
        })
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_1p5()
        .text_size(p.fpx(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .whitespace_nowrap()
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(svg_icon(icon_path, 15. * p.ui_scale(), fg));
    if let Some(txt) = label {
        el = el.child(txt);
    }
    el
}

pub fn ghost_icon_btn<T: 'static>(
    id: impl Into<SharedString>,
    icon_path: &'static str,
    icon_size: f32,
    icon_color: Hsla,
    hover_bg: Hsla,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .w(px(28.))
        .h(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .hover(move |s| s.bg(hover_bg))
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(svg_icon(icon_path, icon_size, icon_color))
}

pub fn danger_btn<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let fg = p.danger;
    div()
        .id(ElementId::Name(id.into()))
        .px_3()
        .py(p.spx(6.))
        .rounded(p.radius(8., 3.))
        .bg(p.bg_surface)
        .hover(move |s| s.bg(p.bg_elevated))
        .border_1()
        .border_color(fg)
        .text_size(p.fpx(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .whitespace_nowrap()
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(label.into())
}

pub fn chip_toggle<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    active: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let (bg, border, fg) = if active {
        (p.accent_soft, p.accent, p.text_primary)
    } else {
        (p.bg_surface, p.border, p.text_secondary)
    };
    div()
        .id(ElementId::Name(id.into()))
        .px_2p5()
        .py(p.spx(5.))
        .rounded(p.radius(6., 2.))
        .bg(bg)
        .hover(|s| s.bg(p.bg_elevated))
        .border_1()
        .border_color(border)
        .text_size(p.fpx(12.))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(fg)
        .whitespace_nowrap()
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(label.into())
}

pub fn fdm_checkbox(checked: bool, p: &ThemePalette) -> Div {
    div()
        .w(p.spx(16.))
        .h(p.spx(16.))
        .rounded(p.radius(4., 2.))
        .flex_shrink_0()
        .bg(if checked { p.accent } else { p.bg_input })
        .border_1()
        .border_color(if checked { p.accent } else { p.border })
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |el| el.child(svg_icon("icons/check.svg", 12. * p.ui_scale(), p.on_accent)))
}

fn switch_pill_track(id_key: &str, checked: bool, p: &ThemePalette) -> AnyElement {
    let p_copy = *p;
    let track_w = p.spx(34.);
    let track_h = p.spx(18.);
    let pad = p.spx(2.);
    let thumb_sz = p.spx(12.);
    let travel = 16.0 * p.ui_scale();

    if let Some(epoch) = active_switch_epoch(id_key) {
        let anim_id = SharedString::from(format!("sw-anim-{id_key}-{epoch}"));
        div()
            .w(track_w)
            .h(track_h)
            .rounded_full()
            .flex_shrink_0()
            .p(pad)
            .border_1()
            .flex()
            .flex_row()
            .items_center()
            .with_animation(
                anim_id,
                Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                move |el, delta| {
                    let t = if checked { delta } else { 1.0 - delta };
                    let track_bg = lerp_hsla(p_copy.bg_elevated, p_copy.accent, t);
                    let border_c = lerp_hsla(p_copy.border, p_copy.accent, t);
                    let thumb_c = lerp_hsla(p_copy.text_muted, p_copy.on_accent, t);
                    el.bg(track_bg)
                        .border_color(border_c)
                        .child(
                            div()
                                .ml(px(travel * t))
                                .w(thumb_sz)
                                .h(thumb_sz)
                                .rounded_full()
                                .bg(thumb_c),
                        )
                },
            )
            .into_any_element()
    } else {
        let ml_val = if checked { travel } else { 0.0 };
        div()
            .w(track_w)
            .h(track_h)
            .rounded_full()
            .flex_shrink_0()
            .p(pad)
            .bg(if checked { p.accent } else { p.bg_elevated })
            .border_1()
            .border_color(if checked { p.accent } else { p.border })
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .ml(px(ml_val))
                    .w(thumb_sz)
                    .h(thumb_sz)
                    .rounded_full()
                    .bg(if checked { p.on_accent } else { p.text_muted }),
            )
            .into_any_element()
    }
}

pub fn switch_row<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    checked: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let id_ss: SharedString = id.into();
    let id_for_click = id_ss.to_string();
    let pill = switch_pill_track(&id_ss, checked, p);
    div()
        .id(ElementId::Name(id_ss))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .py(p.spx(5.))
        .px_2p5()
        .rounded(p.radius(6., 2.))
        .hover(|s| s.bg(p.bg_elevated))
        .cursor_pointer()
        .on_click(cx.listener(move |this, ev, w, cx| {
            mark_switch_toggled(&id_for_click);
            handler(this, ev, w, cx);
        }))
        .child(div().text_size(p.fpx(12.)).text_color(p.text_primary).child(label.into()))
        .child(pill)
}

pub fn labeled_field(label: impl Into<String>, input: AnyElement, p: &ThemePalette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text_size(p.fpx(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_secondary)
                .child(label.into()),
        )
        .child(input)
}

pub fn section_heading(title: impl Into<String>, p: &ThemePalette) -> Div {
    div()
        .text_size(p.fpx(12.))
        .font_weight(FontWeight::BOLD)
        .text_color(p.text_muted)
        .pt_2()
        .pb_1()
        .border_b_1()
        .border_color(p.border)
        .child(title.into())
}

pub fn fdm_indeterminate_checkbox(checked: bool, indeterminate: bool, p: &ThemePalette) -> Div {
    let filled = checked || indeterminate;
    div()
        .w(p.spx(16.))
        .h(p.spx(16.))
        .rounded(p.radius(4., 2.))
        .flex_shrink_0()
        .bg(if filled { p.accent } else { p.bg_input })
        .border_1()
        .border_color(if filled { p.accent } else { p.border })
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |el| el.child(svg_icon("icons/check.svg", 12. * p.ui_scale(), p.on_accent)))
        .when(!checked && indeterminate, |el| {
            el.child(div().w(p.spx(8.)).h(px(2.)).rounded_full().bg(p.on_accent))
        })
}

pub fn radio_row<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    description: Option<String>,
    selected: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let icon = if selected {
        "icons/radio_checked.svg"
    } else {
        "icons/radio_unchecked.svg"
    };
    let icon_color = if selected { p.accent } else { p.text_muted };
    div()
        .id(ElementId::Name(id.into()))
        .flex()
        .flex_row()
        .items_start()
        .gap_2p5()
        .px_2p5()
        .py(p.spx(6.))
        .rounded(p.radius(6., 2.))
        .hover(|s| s.bg(p.bg_row_hover))
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(div().pt(px(1.)).child(svg_icon(icon, 16. * p.ui_scale(), icon_color)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_size(p.fpx(12.))
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(p.text_primary)
                        .child(label.into()),
                )
                .when_some(description, |el, desc| {
                    el.child(div().text_size(p.fpx(11.)).text_color(p.text_muted).child(desc))
                }),
        )
}

pub fn switch_row_desc<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    description: Option<String>,
    checked: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let id_ss: SharedString = id.into();
    let id_for_click = id_ss.to_string();
    let pill = switch_pill_track(&id_ss, checked, p);
    div()
        .id(ElementId::Name(id_ss))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_3()
        .py(p.spx(6.))
        .px_2p5()
        .rounded(p.radius(6., 2.))
        .hover(|s| s.bg(p.bg_row_hover))
        .cursor_pointer()
        .on_click(cx.listener(move |this, ev, w, cx| {
            mark_switch_toggled(&id_for_click);
            handler(this, ev, w, cx);
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .flex_1()
                .child(div().text_size(p.fpx(12.)).text_color(p.text_primary).child(label.into()))
                .when_some(description, |el, desc| {
                    el.child(div().text_size(p.fpx(11.)).text_color(p.text_muted).child(desc))
                }),
        )
        .child(pill)
}

pub fn snail_switch_btn<T: 'static>(
    id: impl Into<SharedString>,
    active: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    let id_ss: SharedString = id.into();
    let id_for_click = id_ss.to_string();
    let p_copy = *p;
    let track_w = p.spx(46.);
    let track_h = p.spx(24.);
    let thumb_sz = p.spx(18.);
    let travel = 22.0 * p.ui_scale();

    let inner: AnyElement = if let Some(epoch) = active_switch_epoch(&id_ss) {
        let anim_id = SharedString::from(format!("snail-anim-{id_ss}-{epoch}"));
        div()
            .w(track_w)
            .h(track_h)
            .rounded_full()
            .border_1()
            .p(p.spx(2.))
            .flex()
            .flex_row()
            .items_center()
            .with_animation(
                anim_id,
                Animation::new(Duration::from_millis(180)).with_easing(ease_out_quint()),
                move |el, delta| {
                    let t = if active { delta } else { 1.0 - delta };
                    let track_bg = lerp_hsla(p_copy.snail_off_track, p_copy.snail_on_bg, t);
                    let track_border = lerp_hsla(p_copy.snail_off_border, p_copy.snail_on_border, t);
                    let thumb_bg = lerp_hsla(p_copy.snail_off_thumb, p_copy.amber, t);
                    let thumb_fg = lerp_hsla(p_copy.snail_off_thumb_text, p_copy.snail_on_bg, t);
                    el.bg(track_bg)
                        .border_color(track_border)
                        .child(
                            div()
                                .ml(px(travel * t))
                                .w(thumb_sz)
                                .h(thumb_sz)
                                .rounded_full()
                                .bg(thumb_bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(svg_icon("icons/snail.svg", 13. * p_copy.ui_scale(), thumb_fg)),
                        )
                },
            )
            .into_any_element()
    } else {
        let (track_bg, track_border, thumb_bg, thumb_fg) = if active {
            (p.snail_on_bg, p.snail_on_border, p.amber, p.snail_on_bg)
        } else {
            (
                p.snail_off_track,
                p.snail_off_border,
                p.snail_off_thumb,
                p.snail_off_thumb_text,
            )
        };
        div()
            .w(track_w)
            .h(track_h)
            .rounded_full()
            .bg(track_bg)
            .border_1()
            .border_color(track_border)
            .p(p.spx(2.))
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .ml(px(if active { travel } else { 0.0 }))
                    .w(thumb_sz)
                    .h(thumb_sz)
                    .rounded_full()
                    .bg(thumb_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(svg_icon("icons/snail.svg", 13. * p.ui_scale(), thumb_fg)),
            )
            .into_any_element()
    };

    div()
        .id(ElementId::Name(id_ss))
        .cursor_pointer()
        .on_click(cx.listener(move |this, ev, w, cx| {
            mark_switch_toggled(&id_for_click);
            handler(this, ev, w, cx);
        }))
        .child(inner)
}

pub fn sidebar_nav_item<T: 'static>(
    id: impl Into<SharedString>,
    icon_path: &'static str,
    label: impl Into<String>,
    active: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .h(p.spx(34.))
        .px_2p5()
        .rounded(p.radius(8., 3.))
        .flex()
        .flex_row()
        .items_center()
        .gap_2p5()
        .bg(if active { p.bg_300 } else { gpui::transparent_black() })
        .hover(|s| s.bg(if active { p.bg_300 } else { p.bg_row_hover }))
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .child(svg_icon(
            icon_path,
            15. * p.ui_scale(),
            if active { p.accent } else { p.text_muted },
        ))
        .child(
            div()
                .text_size(p.fpx(13.))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if active { p.text_primary } else { p.text_secondary })
                .child(label.into()),
        )
}

pub fn slim_progress_bar(fraction: f64, height_px: f32, fill_color: Hsla, track_color: Hsla) -> Div {
    let clamped = fraction.clamp(0.0, 1.0) as f32;
    div()
        .w_full()
        .h(px(height_px))
        .rounded_full()
        .bg(track_color)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(clamped))
                .rounded_full()
                .bg(fill_color),
        )
}

/// Animated progress bar mounted only for actively downloading transfers (zero CPU when idle).
pub fn slim_progress_bar_active(
    id: impl Into<SharedString>,
    fraction: f64,
    height_px: f32,
    fill_color: Hsla,
    track_color: Hsla,
    active: bool,
) -> AnyElement {
    let clamped = fraction.clamp(0.0, 1.0) as f32;
    if !active {
        return slim_progress_bar(fraction, height_px, fill_color, track_color).into_any_element();
    }
    let fill_w = clamped.max(0.04);
    let anim_id = ElementId::Name(id.into());
    div()
        .w_full()
        .h(px(height_px))
        .rounded_full()
        .bg(track_color)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(fill_w))
                .rounded_full()
                .overflow_hidden()
                .with_animation(
                    anim_id,
                    Animation::new(Duration::from_millis(1200))
                        .repeat()
                        .with_easing(ease_in_out),
                    move |el, delta| {
                        let pulse = if delta < 0.5 {
                            delta * 2.0
                        } else {
                            (1.0 - delta) * 2.0
                        };
                        let c = fill_color.opacity(0.76 + 0.24 * pulse);
                        el.bg(c)
                    },
                ),
        )
        .into_any_element()
}

pub fn tab_pill<T: 'static>(
    id: impl Into<SharedString>,
    label: impl Into<String>,
    active: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    handler: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .px_2p5()
        .py(p.spx(4.))
        .rounded(p.radius(6., 2.))
        .bg(if active { p.bg_300 } else { gpui::transparent_black() })
        .hover(|s| s.bg(p.bg_300))
        .cursor_pointer()
        .on_click(cx.listener(handler))
        .text_size(p.fpx(11.))
        .font_weight(if active {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(if active { p.text_primary } else { p.text_muted })
        .whitespace_nowrap()
        .child(label.into())
}

pub fn password_field_row<T: 'static>(
    toggle_id: impl Into<SharedString>,
    input: AnyElement,
    revealed: bool,
    p: &ThemePalette,
    cx: &mut Context<T>,
    on_toggle: impl Fn(&mut T, &gpui::ClickEvent, &mut Window, &mut Context<T>) + 'static,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .w_full()
        .child(div().flex_1().min_w_0().child(input))
        .child(ghost_icon_btn(
            toggle_id,
            if revealed { "icons/eye_off.svg" } else { "icons/eye.svg" },
            15.,
            p.text_muted,
            p.bg_elevated,
            cx,
            on_toggle,
        ))
}

pub struct EllipsisText {
    text: SharedString,
}

/// Single-line text element that measures its true post-layout bounds in `prepaint`
/// before truncating with `…`, avoiding GPUI 0.2's Taffy min-content caching bug.
pub fn ellipsis_text(text: impl Into<SharedString>) -> EllipsisText {
    EllipsisText { text: text.into() }
}

impl IntoElement for EllipsisText {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct EllipsisLayoutState {
    text_style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
    natural_width: Pixels,
    shaped_full: Option<ShapedLine>,
}

impl Element for EllipsisText {
    type RequestLayoutState = EllipsisLayoutState;
    type PrepaintState = Option<(ShapedLine, Pixels)>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = text_style.line_height_in_pixels(window.rem_size());
        let (natural_width, shaped_full) = if self.text.is_empty() {
            (px(0.), None)
        } else {
            let runs = [text_style.to_run(self.text.len())];
            let shaped = window
                .text_system()
                .shape_line(self.text.clone(), font_size, &runs, None);
            (shaped.width.ceil(), Some(shaped))
        };

        let mut style = Style::default();
        style.min_size.width = px(0.).into();
        style.flex_shrink = 1.0;

        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, _window, _cx| {
                let width = if let Some(w) = known_dimensions.width {
                    w
                } else {
                    match available_space.width {
                        gpui::AvailableSpace::Definite(w) => natural_width.min(w),
                        gpui::AvailableSpace::MinContent | gpui::AvailableSpace::MaxContent => {
                            natural_width
                        }
                    }
                };
                gpui::size(width, line_height)
            },
        );
        (
            layout_id,
            EllipsisLayoutState {
                text_style,
                font_size,
                line_height,
                natural_width,
                shaped_full,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.text.is_empty() || bounds.size.width <= px(2.) {
            return None;
        }
        if bounds.size.width + px(0.5) >= state.natural_width {
            if let Some(full) = state.shaped_full.take() {
                return Some((full, state.line_height));
            }
        }
        let mut runs = vec![state.text_style.to_run(self.text.len())];
        let display_text = {
            let mut lw = cx
                .text_system()
                .line_wrapper(state.text_style.font(), state.font_size);
            lw.truncate_line(self.text.clone(), bounds.size.width, "…", &mut runs)
        };
        let shaped = window
            .text_system()
            .shape_line(display_text, state.font_size, &runs, None);
        Some((shaped, state.line_height))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some((line, line_height)) = prepaint.take() {
            let _ = line.paint(bounds.origin, line_height, window, cx);
        }
    }
}
