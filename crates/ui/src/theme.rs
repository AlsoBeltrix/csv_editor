//! Design tokens and egui visuals.
//!
//! Adapted from GridCraft's `crates/ui-egui/src/theme.rs` (MIT, see `ATTRIBUTION.md`): the colour
//! tokens and the visuals setup. Fonts stay egui's defaults for now.

use egui::{Color32, FontFamily, FontId, Stroke, Visuals};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    pub window: Color32,
    pub toolbar: Color32,
    pub toolbar_border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub accent: Color32,
    pub accent_dark: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub separator: Color32,
    pub grid_bg: Color32,
    pub gridline: Color32,
    pub header_bg: Color32,
    pub header_text: Color32,
    pub header_line: Color32,
    pub header_sel_bg: Color32,
    pub header_sel_text: Color32,
    pub header_all_bg: Color32,
    pub sel_fill: Color32,
    pub sel_border: Color32,
    pub input_bg: Color32,
    pub input_border: Color32,
    pub tab_bar: Color32,
    pub tab_active: Color32,
    pub status_bar: Color32,
    pub cell_text: Color32,
    pub menu_bg: Color32,
    pub shadow: Color32,
    pub danger: Color32,
    pub notice_warn_bg: Color32,
    pub notice_warn_border: Color32,
    pub notice_info_bg: Color32,
    pub notice_info_border: Color32,
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            dark: false,
            window: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            toolbar: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            toolbar_border: Color32::from_rgb(0xE3, 0xE3, 0xE3),
            text: Color32::from_rgb(0x24, 0x24, 0x24),
            text_dim: Color32::from_rgb(0x61, 0x61, 0x61),
            text_disabled: Color32::from_rgb(0xB0, 0xB0, 0xB0),
            accent: Color32::from_rgb(0x10, 0x7C, 0x41),
            accent_dark: Color32::from_rgb(0x0B, 0x5E, 0x31),
            accent_soft: Color32::from_rgb(0xDF, 0xF1, 0xE6),
            hover: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            pressed: Color32::from_rgb(0xDD, 0xDD, 0xDD),
            separator: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            grid_bg: Color32::WHITE,
            gridline: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_bg: Color32::from_rgb(0xF8, 0xF8, 0xF8),
            header_text: Color32::from_rgb(0x42, 0x42, 0x42),
            header_line: Color32::from_rgb(0xD6, 0xD6, 0xD6),
            header_sel_bg: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_sel_text: Color32::from_rgb(0x10, 0x7C, 0x41),
            header_all_bg: Color32::from_rgb(0xC8, 0xE6, 0xD3),
            sel_fill: Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x1C),
            sel_border: Color32::from_rgb(0x10, 0x7C, 0x41),
            input_bg: Color32::WHITE,
            input_border: Color32::from_rgb(0xD1, 0xD1, 0xD1),
            tab_bar: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            tab_active: Color32::WHITE,
            status_bar: Color32::from_rgb(0xF3, 0xF3, 0xF3),
            cell_text: Color32::BLACK,
            menu_bg: Color32::WHITE,
            shadow: Color32::from_black_alpha(40),
            danger: Color32::from_rgb(0xC4, 0x2B, 0x1C),
            notice_warn_bg: Color32::from_rgb(0xFF, 0xF4, 0xCE),
            notice_warn_border: Color32::from_rgb(0xF0, 0xC3, 0x6E),
            notice_info_bg: Color32::from_rgb(0xEA, 0xF2, 0xFB),
            notice_info_border: Color32::from_rgb(0xB9, 0xD2, 0xEC),
        }
    }

    pub fn dark() -> Tokens {
        Tokens {
            dark: true,
            window: Color32::from_rgb(0x29, 0x29, 0x29),
            toolbar: Color32::from_rgb(0x33, 0x33, 0x33),
            toolbar_border: Color32::from_rgb(0x40, 0x40, 0x40),
            text: Color32::from_rgb(0xEE, 0xEE, 0xEE),
            text_dim: Color32::from_rgb(0xB0, 0xB0, 0xB0),
            text_disabled: Color32::from_rgb(0x70, 0x70, 0x70),
            accent: Color32::from_rgb(0x5C, 0xC5, 0x87),
            accent_dark: Color32::from_rgb(0x3F, 0xA8, 0x6C),
            accent_soft: Color32::from_rgb(0x23, 0x45, 0x31),
            hover: Color32::from_rgb(0x42, 0x42, 0x42),
            pressed: Color32::from_rgb(0x4D, 0x4D, 0x4D),
            separator: Color32::from_rgb(0x48, 0x48, 0x48),
            // Cells stay paper-white in dark mode, as GridCraft does: the data reads the same
            // whatever the chrome looks like.
            grid_bg: Color32::WHITE,
            gridline: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            header_bg: Color32::from_rgb(0x30, 0x30, 0x30),
            header_text: Color32::from_rgb(0xCC, 0xCC, 0xCC),
            header_line: Color32::from_rgb(0x45, 0x45, 0x45),
            header_sel_bg: Color32::from_rgb(0x45, 0x45, 0x45),
            header_sel_text: Color32::from_rgb(0x5C, 0xC5, 0x87),
            header_all_bg: Color32::from_rgb(0x2E, 0x55, 0x3E),
            sel_fill: Color32::from_rgba_unmultiplied(0x10, 0x7C, 0x41, 0x24),
            sel_border: Color32::from_rgb(0x10, 0x7C, 0x41),
            input_bg: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            input_border: Color32::from_rgb(0x50, 0x50, 0x50),
            tab_bar: Color32::from_rgb(0x29, 0x29, 0x29),
            tab_active: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            status_bar: Color32::from_rgb(0x29, 0x29, 0x29),
            cell_text: Color32::BLACK,
            menu_bg: Color32::from_rgb(0x30, 0x30, 0x30),
            shadow: Color32::from_black_alpha(90),
            danger: Color32::from_rgb(0xF1, 0x70, 0x5F),
            notice_warn_bg: Color32::from_rgb(0x4A, 0x3F, 0x1E),
            notice_warn_border: Color32::from_rgb(0x7A, 0x67, 0x30),
            notice_info_bg: Color32::from_rgb(0x24, 0x33, 0x44),
            notice_info_border: Color32::from_rgb(0x3B, 0x55, 0x72),
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { Tokens::dark() } else { Tokens::light() }
    }
}

pub fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn cell_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Installs both palettes and selects one. Called once at start and whenever the theme changes.
pub fn apply(ctx: &egui::Context, dark: bool) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let t = if dark { Tokens::dark() } else { Tokens::light() };
        let mut v = if dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = t.window;
        v.window_fill = t.menu_bg;
        v.extreme_bg_color = t.input_bg;
        v.selection.bg_fill = t.accent_soft;
        v.selection.stroke = Stroke::new(1.0, t.accent);
        v.hyperlink_color = t.accent;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.active.weak_bg_fill = t.pressed;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
        v.window_corner_radius = egui::CornerRadius::same(10);
        v.menu_corner_radius = egui::CornerRadius::same(8);
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: t.shadow };
        ctx.set_visuals_of(theme, v);
    }
    ctx.options_mut(|o| o.fallback_theme = egui::Theme::Light);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Button, ui_font(13.0));
        s.text_styles.insert(egui::TextStyle::Small, ui_font(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, ui_font(18.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    });
}
