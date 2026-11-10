use gpui::{AnyElement, div, prelude::*, px, rgb};
use qrcode::{Color, QrCode};

/// Renders a crisp pure-GPUI QR matrix element with white quiet zone.
pub fn render_qr_element(data: &str, module_px: f32) -> AnyElement {
    let Ok(code) = QrCode::new(data.as_bytes()) else {
        return div()
            .p_3()
            .bg(rgb(0x1f2937))
            .text_color(rgb(0xfca5a5))
            .child("Failed to generate QR code")
            .into_any_element();
    };
    let width = code.width();
    let colors = code.to_colors();

    let mut rows = Vec::with_capacity(width);
    for y in 0..width {
        let mut cells = Vec::with_capacity(width);
        for x in 0..width {
            let dark = colors[y * width + x] == Color::Dark;
            cells.push(
                div()
                    .w(px(module_px))
                    .h(px(module_px))
                    .bg(if dark { rgb(0x0f172a) } else { rgb(0xffffff) }),
            );
        }
        rows.push(div().flex().flex_row().children(cells));
    }

    div()
        .p(px(module_px * 3.0))
        .rounded_lg()
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(0xcbd5e1))
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}
