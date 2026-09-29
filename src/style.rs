// Schlichtes Standard-Aussehen im Stil normaler Windows-Programme.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

pub const FACE: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);
pub const FACE_LIGHT: Color32 = Color32::from_rgb(0xFD, 0xFD, 0xFD);
pub const FACE_DARK: Color32 = Color32::from_rgb(0xCC, 0xE4, 0xF7);
pub const SHADOW: Color32 = Color32::from_rgb(0xAD, 0xAD, 0xAD);
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);
pub const GRID_LINE: Color32 = Color32::from_rgb(0xD8, 0xD8, 0xD8);
pub const NULL_TEXT: Color32 = Color32::from_rgb(0x90, 0x90, 0x90);
pub const ERROR_TEXT: Color32 = Color32::from_rgb(0xB0, 0x00, 0x00);
pub const OK_TEXT: Color32 = Color32::from_rgb(0x00, 0x60, 0x00);

fn load_font(candidates: &[&str]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn apply(ctx: &egui::Context) {
    // Systemschriften laden (Segoe UI wie in normalen Windows-Programmen)
    let mut fonts = FontDefinitions::default();
    let prop = load_font(&[
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\tahoma.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]);
    let mono = load_font(&[
        "C:\\Windows\\Fonts\\consola.ttf",
        "C:\\Windows\\Fonts\\cour.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ]);
    if let Some(data) = prop {
        fonts
            .font_data
            .insert("sys".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "sys".into());
    }
    if let Some(data) = mono {
        fonts
            .font_data
            .insert("sysmono".into(), Arc::new(FontData::from_owned(data)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "sysmono".into());
    }
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Light);
    ctx.global_style_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::proportional(16.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();

        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(6.0, 4.0);
        sp.button_padding = egui::vec2(8.0, 3.0);
        sp.interact_size.y = 20.0;
        sp.window_margin = Margin::same(8);
        sp.menu_margin = Margin::same(2);
        sp.scroll = egui::style::ScrollStyle::solid();
        sp.combo_width = 160.0;

        style.animation_time = 0.0;
        style.visuals = standard_visuals();
    });
}

fn standard_visuals() -> Visuals {
    let mut v = Visuals::light();
    let zero = CornerRadius::same(2);
    v.window_corner_radius = zero;
    v.menu_corner_radius = zero;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow {
        offset: [2, 2],
        blur: 0,
        spread: 0,
        color: Color32::from_black_alpha(60),
    };
    v.window_fill = FACE;
    v.panel_fill = FACE;
    v.window_stroke = Stroke::new(1.0, SHADOW);
    v.extreme_bg_color = Color32::WHITE;
    v.text_edit_bg_color = Some(Color32::WHITE);
    v.faint_bg_color = Color32::from_rgb(0xF4, 0xF4, 0xF0);
    v.code_bg_color = Color32::WHITE;
    v.hyperlink_color = ACCENT;
    v.override_text_color = None;
    v.selection.bg_fill = Color32::from_rgb(0xCC, 0xE8, 0xFF);
    v.selection.stroke = Stroke::new(1.0, Color32::BLACK);
    v.window_highlight_topmost = false;
    v.striped = true;
    v.indent_has_left_vline = true;

    let black = Stroke::new(1.0, Color32::BLACK);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = FACE;
    w.noninteractive.weak_bg_fill = FACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SHADOW);
    w.noninteractive.fg_stroke = black;
    w.noninteractive.corner_radius = zero;

    w.inactive.bg_fill = FACE_LIGHT;
    w.inactive.weak_bg_fill = FACE_LIGHT;
    w.inactive.bg_stroke = Stroke::new(1.0, SHADOW);
    w.inactive.fg_stroke = black;
    w.inactive.corner_radius = zero;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = Color32::from_rgb(0xE5, 0xF1, 0xFB);
    w.hovered.weak_bg_fill = Color32::from_rgb(0xE5, 0xF1, 0xFB);
    w.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = black;
    w.hovered.corner_radius = zero;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = FACE_DARK;
    w.active.weak_bg_fill = FACE_DARK;
    w.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x00, 0x54, 0x99));
    w.active.fg_stroke = black;
    w.active.corner_radius = zero;
    w.active.expansion = 0.0;

    w.open.bg_fill = FACE_DARK;
    w.open.weak_bg_fill = FACE_DARK;
    w.open.bg_stroke = Stroke::new(1.0, SHADOW);
    w.open.fg_stroke = black;
    w.open.corner_radius = zero;
    v
}

/// Weisser, umrandeter Bereich (wie ein Listenfeld).
pub fn sunken_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, SHADOW))
        .inner_margin(Margin::same(2))
}

/// Gruppenrahmen (wie eine "GroupBox").
pub fn group_frame() -> egui::Frame {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_rgb(0xDC, 0xDC, 0xDC)))
        .inner_margin(Margin::same(6))
}
