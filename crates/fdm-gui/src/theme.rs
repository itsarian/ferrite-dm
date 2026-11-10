use fdm_core::{ThemeMode, UiStyle};
use gpui::{Hsla, Pixels, px, rgb, rgba};

/// Visual palette faithfully matching Free Download Manager 6.34.4 (`Theme_V2` light & dark, plus `Classic`).
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub is_dark: bool,
    pub ui_style: UiStyle,
    pub overall_zoom_pct: u16,
    pub fonts_zoom_pct: u16,
    // FDM V2 token scale (100 = primary canvas -> 1000 = highest contrast foreground)
    pub bg_100: Hsla,
    pub bg_200: Hsla,
    pub bg_300: Hsla,
    pub bg_400: Hsla,
    pub bg_500: Hsla,
    pub bg_600: Hsla,
    pub bg_700: Hsla,
    pub bg_800: Hsla,
    pub bg_900: Hsla,
    pub bg_1000: Hsla,

    // Semantic convenience aliases used across views & widgets
    pub bg_app: Hsla,
    pub bg_sidebar: Hsla,
    pub bg_surface: Hsla,
    pub bg_elevated: Hsla,
    pub bg_row_alt: Hsla,
    pub bg_row_hover: Hsla,
    pub bg_row_selected: Hsla,
    pub bg_input: Hsla,
    pub border: Hsla,
    pub border_focus: Hsla,
    pub separator: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_soft: Hsla,
    pub on_accent: Hsla,
    pub success: Hsla,
    pub amber: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub info: Hsla,

    // Special areas & Snail switch tokens
    pub dialog_special_area: Hsla,
    pub snail_bg: Hsla,
    pub snail_border: Hsla,
    pub snail_text: Hsla,
    pub snail_on_bg: Hsla,
    pub snail_on_border: Hsla,
    pub snail_on_fg: Hsla,
    pub snail_off_track: Hsla,
    pub snail_off_border: Hsla,
    pub snail_off_thumb: Hsla,
    pub snail_off_thumb_text: Hsla,
    pub tooltip_bg: Hsla,
    pub tooltip_fg: Hsla,
    pub modal_backdrop: Hsla,
}

impl ThemePalette {
    pub fn from_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => {
                if let Ok(override_theme) = std::env::var("FERRITE_THEME") {
                    if override_theme.eq_ignore_ascii_case("dark") {
                        return Self::dark();
                    }
                }
                Self::light()
            }
            ThemeMode::Dark => {
                if let Ok(override_theme) = std::env::var("FERRITE_THEME") {
                    if override_theme.eq_ignore_ascii_case("light") {
                        return Self::light();
                    }
                }
                Self::dark()
            }
            ThemeMode::System => Self::system_default(),
        }
    }

    pub fn from_config(
        mode: ThemeMode,
        ui_style: UiStyle,
        overall_zoom_pct: u16,
        fonts_zoom_pct: u16,
    ) -> Self {
        Self::from_mode(mode).with_appearance(ui_style, overall_zoom_pct, fonts_zoom_pct)
    }

    pub fn with_appearance(
        mut self,
        ui_style: UiStyle,
        overall_zoom_pct: u16,
        fonts_zoom_pct: u16,
    ) -> Self {
        self.ui_style = ui_style;
        self.overall_zoom_pct = overall_zoom_pct.clamp(75, 150);
        self.fonts_zoom_pct = fonts_zoom_pct.clamp(75, 160);
        if ui_style == UiStyle::Classic {
            if self.is_dark {
                self.bg_app = rgb(0x18191b).into();
                self.bg_200 = rgb(0x222427).into();
                self.bg_300 = rgb(0x2d3034).into();
                self.border = rgb(0x454950).into();
                self.separator = rgb(0x32353b).into();
            } else {
                self.bg_app = rgb(0xfafafa).into();
                self.bg_200 = rgb(0xe7eaee).into();
                self.bg_300 = rgb(0xdde2e8).into();
                self.border = rgb(0xb4becb).into();
                self.separator = rgb(0xd2d8e0).into();
            }
        }
        self
    }

    pub fn ui_scale(&self) -> f32 {
        (self.overall_zoom_pct as f32 / 100.0).clamp(0.75, 1.50)
    }

    pub fn font_scale(&self) -> f32 {
        (self.fonts_zoom_pct as f32 / 100.0).clamp(0.75, 1.60)
    }

    pub fn spx_f32(&self, base: f32) -> f32 {
        (base * self.ui_scale()).round()
    }

    pub fn fpx_f32(&self, base: f32) -> f32 {
        (base * self.font_scale()).round()
    }

    pub fn spx(&self, base: f32) -> Pixels {
        px(self.spx_f32(base))
    }

    pub fn fpx(&self, base: f32) -> Pixels {
        px(self.fpx_f32(base))
    }

    pub fn is_classic(&self) -> bool {
        self.ui_style == UiStyle::Classic
    }

    pub fn radius(&self, modern_px: f32, classic_px: f32) -> Pixels {
        let base = if self.is_classic() { classic_px } else { modern_px };
        px((base * self.ui_scale()).round().max(1.0))
    }

    /// Detect macOS system Dark/Light mode on startup, matching FDM's `'system'` theme default.
    pub fn system_default() -> Self {
        if let Ok(override_theme) = std::env::var("FERRITE_THEME") {
            if override_theme.eq_ignore_ascii_case("dark") {
                return Self::dark();
            } else if override_theme.eq_ignore_ascii_case("light") {
                return Self::light();
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Ok(out) = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleInterfaceStyle"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.trim().eq_ignore_ascii_case("dark") {
                    return Self::dark();
                }
            }
            return Self::light();
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::dark()
        }
    }

    pub fn dark() -> Self {
        // FDM 6.34.4 Theme_V2 Dark:
        // bg100=#141415, bg200=#1C1D1F, bg300=#27282A, bg400=#383A3D, bg500=#54575C,
        // bg600=#7A7E85, bg700=#9D9FA3, bg800=#B0B3B8, bg900=#E1E4EB, bg1000=#F4F5F5
        // primary=#73CEFC, secondary=#4EC284, amber=#C29B4E, danger=#C24E5D
        Self {
            is_dark: true,
            ui_style: UiStyle::New,
            overall_zoom_pct: 100,
            fonts_zoom_pct: 100,
            bg_100: rgb(0x141415).into(),
            bg_200: rgb(0x1c1d1f).into(),
            bg_300: rgb(0x27282a).into(),
            bg_400: rgb(0x383a3d).into(),
            bg_500: rgb(0x54575c).into(),
            bg_600: rgb(0x7a7e85).into(),
            bg_700: rgb(0x9d9fa3).into(),
            bg_800: rgb(0xb0b3b8).into(),
            bg_900: rgb(0xe1e4eb).into(),
            bg_1000: rgb(0xf4f5f5).into(),

            bg_app: rgb(0x141415).into(),
            bg_sidebar: rgb(0x1c1d1f).into(),
            bg_surface: rgb(0x27282a).into(),
            bg_elevated: rgb(0x383a3d).into(),
            bg_row_alt: rgb(0x17181a).into(),
            bg_row_hover: rgb(0x1f2023).into(),
            bg_row_selected: rgba(0x73cefc22).into(),
            bg_input: rgb(0x1c1d1f).into(),
            border: rgb(0x383a3d).into(),
            border_focus: rgb(0x73cefc).into(),
            separator: rgb(0x27282a).into(),
            text_primary: rgb(0xf4f5f5).into(),
            text_secondary: rgb(0xe1e4eb).into(),
            text_muted: rgb(0xb0b3b8).into(),
            text_subtle: rgb(0x9d9fa3).into(),
            accent: rgb(0x73cefc).into(),
            accent_hover: rgb(0x8cd7fd).into(),
            accent_soft: rgba(0x73cefc24).into(),
            on_accent: rgb(0x141415).into(),
            success: rgb(0x4ec284).into(),
            amber: rgb(0xc29b4e).into(),
            warning: rgb(0xc29b4e).into(),
            danger: rgb(0xc24e5d).into(),
            info: rgb(0x73cefc).into(),

            dialog_special_area: rgb(0x1c1d1f).into(),
            snail_bg: rgb(0x8f6d28).into(),
            snail_border: rgb(0xc29b4e).into(),
            snail_text: rgb(0xfff1d6).into(),
            snail_on_bg: rgb(0xffebc4).into(),
            snail_on_border: rgb(0x8f6d28).into(),
            snail_on_fg: rgb(0x8f6d28).into(),
            snail_off_track: rgb(0x27282a).into(),
            snail_off_border: rgb(0x383a3d).into(),
            snail_off_thumb: rgb(0x969ba3).into(),
            snail_off_thumb_text: rgb(0x141415).into(),
            tooltip_bg: rgb(0x27282a).into(),
            tooltip_fg: rgb(0xf4f5f5).into(),
            modal_backdrop: gpui::hsla(0.0, 0.0, 0.03, 0.68),
        }
    }

    pub fn light() -> Self {
        // FDM 6.34.4 Theme_V2 Light:
        // bg100=#FDFDFD, bg200=#ECEFF3, bg300=#ECEFF3, bg400=#CED6E0, bg500=#929DAD,
        // bg600=#5A6677, bg700=#424B57, bg800=#272C33, bg900=#1F2329, bg1000=#171A1F
        // primary=#3F80E0, secondary=#288F58, amber=#8F6D28, danger=#C93B4B
        Self {
            is_dark: false,
            ui_style: UiStyle::New,
            overall_zoom_pct: 100,
            fonts_zoom_pct: 100,
            bg_100: rgb(0xfdfdfd).into(),
            bg_200: rgb(0xf2f4f7).into(),
            bg_300: rgb(0xeceff3).into(),
            bg_400: rgb(0xced6e0).into(),
            bg_500: rgb(0x929dad).into(),
            bg_600: rgb(0x5a6677).into(),
            bg_700: rgb(0x424b57).into(),
            bg_800: rgb(0x272c33).into(),
            bg_900: rgb(0x1f2329).into(),
            bg_1000: rgb(0x171a1f).into(),

            bg_app: rgb(0xfdfdfd).into(),
            bg_sidebar: rgb(0xf2f4f7).into(),
            bg_surface: rgb(0xeceff3).into(),
            bg_elevated: rgb(0xdfe5ec).into(),
            bg_row_alt: rgb(0xf8f9fb).into(),
            bg_row_hover: rgb(0xeef2f7).into(),
            bg_row_selected: rgba(0x3f80e01e).into(),
            bg_input: rgb(0xfdfdfd).into(),
            border: rgb(0xced6e0).into(),
            border_focus: rgb(0x3f80e0).into(),
            separator: rgb(0xe2e6ec).into(),
            text_primary: rgb(0x171a1f).into(),
            text_secondary: rgb(0x272c33).into(),
            text_muted: rgb(0x5a6677).into(),
            text_subtle: rgb(0x929dad).into(),
            accent: rgb(0x3f80e0).into(),
            accent_hover: rgb(0x316cc7).into(),
            accent_soft: rgba(0x3f80e018).into(),
            on_accent: rgb(0xfdfdfd).into(),
            success: rgb(0x288f58).into(),
            amber: rgb(0x8f6d28).into(),
            warning: rgb(0x8f6d28).into(),
            danger: rgb(0xc93b4b).into(),
            info: rgb(0x3f80e0).into(),

            dialog_special_area: rgb(0xeceff3).into(),
            snail_bg: rgb(0x8f6d28).into(),
            snail_border: rgb(0x8f6d28).into(),
            snail_text: rgb(0xffebc4).into(),
            snail_on_bg: rgb(0xffebc4).into(),
            snail_on_border: rgb(0x8f6d28).into(),
            snail_on_fg: rgb(0x8f6d28).into(),
            snail_off_track: rgb(0xfdfdfd).into(),
            snail_off_border: rgb(0x929dad).into(),
            snail_off_thumb: rgb(0xacb5c3).into(),
            snail_off_thumb_text: rgb(0xf7fafd).into(),
            tooltip_bg: rgb(0x272c33).into(),
            tooltip_fg: rgb(0xfdfdfd).into(),
            modal_backdrop: gpui::hsla(0.6, 0.15, 0.1, 0.36),
        }
    }
}

pub fn parse_hex_color(hex: &str) -> Hsla {
    let clean = hex.trim().trim_start_matches('#');
    if let Ok(v) = u32::from_str_radix(clean, 16) {
        rgb(v).into()
    } else {
        rgb(0x3f80e0).into()
    }
}
