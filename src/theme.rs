//! Cache Deck colours and the egui look built from them.

use eframe::egui::{self, Color32, CornerRadius, Margin, Shadow, Stroke, Theme, Visuals, style::Selection};

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub brand: Color32,
    pub on_brand: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub error: Color32,
    pub info: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: hex(0x0F1420),
    panel: hex(0x171E2C),
    card: hex(0x202A3B),
    card_hover: hex(0x27334A),
    border: hex(0x2C3850),
    text: hex(0xE6EAF2),
    muted: hex(0x8A96AD),
    brand: hex(0x22D3C5),
    on_brand: hex(0x0F1420),
    accent: hex(0xF5B83D),
    success: hex(0x3DDC84),
    error: hex(0xF05D5E),
    info: hex(0x5AA9FF),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: hex(0xF6F8FB),
    panel: hex(0xFFFFFF),
    card: hex(0xEEF2F7),
    card_hover: hex(0xE3E9F1),
    border: hex(0xD9E0EA),
    text: hex(0x1A2233),
    muted: hex(0x5E6B82),
    brand: hex(0x0FA89C),
    on_brand: hex(0xFFFFFF),
    accent: hex(0xC98A12),
    success: hex(0x1F9D57),
    error: hex(0xD8434A),
    info: hex(0x2F7FD8),
};

pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// Colour of the small tag shown for each key type.
pub fn type_color(kind: &str, p: &Palette) -> Color32 {
    match kind {
        "string" => p.brand,
        "hash" => hex(0xA78BFA),
        "list" => hex(0x5AA9FF),
        "set" => hex(0xF5B83D),
        "zset" => hex(0xF472B6),
        "stream" => hex(0xA3E635),
        "ReJSON-RL" | "json" => hex(0xFB923C),
        _ => p.muted,
    }
}

pub fn type_label(kind: &str) -> &str {
    match kind {
        "string" => "STRING",
        "hash" => "HASH",
        "list" => "LIST",
        "set" => "SET",
        "zset" => "ZSET",
        "stream" => "STREAM",
        "ReJSON-RL" => "JSON",
        "none" => "GONE",
        other => other,
    }
}

fn visuals_for(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(7);
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }),
    };
    v.extreme_bg_color = p.bg;
    v.text_edit_bg_color = Some(p.bg);
    v.faint_bg_color = p.card;
    v.code_bg_color = p.card;
    v.hyperlink_color = p.brand;
    v.warn_fg_color = p.accent;
    v.error_fg_color = p.error;
    v.override_text_color = Some(p.text);
    v.selection = Selection {
        bg_fill: p.brand.gamma_multiply(if p.dark { 0.35 } else { 0.25 }),
        stroke: Stroke::new(1.0, p.brand),
    };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.card;
    w.inactive.weak_bg_fill = p.card;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = p.card_hover;
    w.hovered.weak_bg_fill = p.card_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.brand.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = r;
    w.active.bg_fill = p.card_hover;
    w.active.weak_bg_fill = p.card_hover;
    w.active.bg_stroke = Stroke::new(1.0, p.brand);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.corner_radius = r;
    w.open.bg_fill = p.card_hover;
    w.open.weak_bg_fill = p.card_hover;
    w.open.bg_stroke = Stroke::new(1.0, p.brand);
    w.open.corner_radius = r;
    v
}

/// Install both themes and pick the starting one.
pub fn install(ctx: &egui::Context, dark: bool) {
    ctx.set_visuals_of(Theme::Dark, visuals_for(&DARK));
    ctx.set_visuals_of(Theme::Light, visuals_for(&LIGHT));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.window_margin = Margin::same(18);
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
    });
    set_dark(ctx, dark);
}

pub fn set_dark(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark { Theme::Dark } else { Theme::Light });
}
