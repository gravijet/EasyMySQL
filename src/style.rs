// Aussehen: dunkles Design im Stil von Visual Studio Code (Standard) oder helles Standard-Design.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Farbpalette der Oberflaeche.
pub struct Pal {
    pub dark: bool,
    /// Grundflaeche (Seitenleiste, Dialoge)
    pub face: Color32,
    /// Inhaltsflaeche (Editor, Tabellen)
    pub bg: Color32,
    /// Leicht abgesetzte Flaeche (Tabellenkopf, Zeilennummern)
    pub face_light: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub grid_line: Color32,
    pub null_text: Color32,
    pub error_text: Color32,
    pub ok_text: Color32,
    pub activity_bg: Color32,
    pub activity_fg: Color32,
    pub activity_active: Color32,
    pub sidebar_bg: Color32,
    pub tab_bar_bg: Color32,
    pub tab_active: Color32,
    pub tab_inactive: Color32,
    pub status_bg: Color32,
    pub status_fg: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub find_match: Color32,
    pub bracket: Color32,
    pub hover: Color32,
    // ER-Diagramm
    pub er_canvas: Color32,
    pub er_box: Color32,
    pub er_header: Color32,
    pub er_view_header: Color32,
    pub er_line: Color32,
    pub er_line_dim: Color32,
    pub er_type: Color32,
    // Syntax
    pub syn_keyword: Color32,
    pub syn_function: Color32,
    pub syn_string: Color32,
    pub syn_number: Color32,
    pub syn_comment: Color32,
    pub syn_ident: Color32,
    pub syn_text: Color32,
    pub syn_gutter: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub static DARK_PAL: Pal = Pal {
    dark: true,
    face: rgb(0x25, 0x25, 0x26),
    bg: rgb(0x1E, 0x1E, 0x1E),
    face_light: rgb(0x2D, 0x2D, 0x30),
    text: rgb(0xCC, 0xCC, 0xCC),
    text_weak: rgb(0x9D, 0x9D, 0x9D),
    border: rgb(0x45, 0x45, 0x45),
    accent: rgb(0x00, 0x7A, 0xCC),
    grid_line: rgb(0x33, 0x33, 0x33),
    null_text: rgb(0x80, 0x80, 0x80),
    error_text: rgb(0xF4, 0x87, 0x71),
    ok_text: rgb(0x89, 0xD1, 0x85),
    activity_bg: rgb(0x33, 0x33, 0x33),
    activity_fg: rgb(0x85, 0x85, 0x85),
    activity_active: rgb(0xFF, 0xFF, 0xFF),
    sidebar_bg: rgb(0x25, 0x25, 0x26),
    tab_bar_bg: rgb(0x25, 0x25, 0x26),
    tab_active: rgb(0x1E, 0x1E, 0x1E),
    tab_inactive: rgb(0x2D, 0x2D, 0x2D),
    status_bg: rgb(0x00, 0x7A, 0xCC),
    status_fg: rgb(0xFF, 0xFF, 0xFF),
    current_line: rgb(0x28, 0x28, 0x28),
    selection: rgb(0x26, 0x4F, 0x78),
    find_match: rgb(0x61, 0x51, 0x1E),
    bracket: rgb(0x40, 0x5A, 0x40),
    hover: rgb(0x2A, 0x2D, 0x2E),
    er_canvas: rgb(0x1E, 0x1E, 0x1E),
    er_box: rgb(0x25, 0x25, 0x26),
    er_header: rgb(0x0E, 0x3A, 0x5E),
    er_view_header: rgb(0x2E, 0x4A, 0x2E),
    er_line: rgb(0x4F, 0x9D, 0xDE),
    er_line_dim: rgb(0x3A, 0x4A, 0x5A),
    er_type: rgb(0x8A, 0x8A, 0x8A),
    syn_keyword: rgb(0x56, 0x9C, 0xD6),
    syn_function: rgb(0xDC, 0xDC, 0xAA),
    syn_string: rgb(0xCE, 0x91, 0x78),
    syn_number: rgb(0xB5, 0xCE, 0xA8),
    syn_comment: rgb(0x6A, 0x99, 0x55),
    syn_ident: rgb(0x9C, 0xDC, 0xFE),
    syn_text: rgb(0xD4, 0xD4, 0xD4),
    syn_gutter: rgb(0x85, 0x85, 0x85),
};

pub static LIGHT_PAL: Pal = Pal {
    dark: false,
    face: rgb(0xF0, 0xF0, 0xF0),
    bg: rgb(0xFF, 0xFF, 0xFF),
    face_light: rgb(0xF3, 0xF3, 0xF3),
    text: rgb(0x1E, 0x1E, 0x1E),
    text_weak: rgb(0x61, 0x61, 0x61),
    border: rgb(0xCE, 0xCE, 0xCE),
    accent: rgb(0x00, 0x78, 0xD7),
    grid_line: rgb(0xE0, 0xE0, 0xE0),
    null_text: rgb(0x90, 0x90, 0x90),
    error_text: rgb(0xB0, 0x00, 0x00),
    ok_text: rgb(0x00, 0x60, 0x00),
    activity_bg: rgb(0x2C, 0x2C, 0x2C),
    activity_fg: rgb(0x9A, 0x9A, 0x9A),
    activity_active: rgb(0xFF, 0xFF, 0xFF),
    sidebar_bg: rgb(0xF3, 0xF3, 0xF3),
    tab_bar_bg: rgb(0xEC, 0xEC, 0xEC),
    tab_active: rgb(0xFF, 0xFF, 0xFF),
    tab_inactive: rgb(0xE4, 0xE4, 0xE4),
    status_bg: rgb(0x00, 0x7A, 0xCC),
    status_fg: rgb(0xFF, 0xFF, 0xFF),
    current_line: rgb(0xF2, 0xF6, 0xFC),
    selection: rgb(0xAD, 0xD6, 0xFF),
    find_match: rgb(0xFF, 0xE0, 0x80),
    bracket: rgb(0xC8, 0xE6, 0xC8),
    hover: rgb(0xE8, 0xE8, 0xE8),
    er_canvas: rgb(0xFF, 0xFF, 0xFF),
    er_box: rgb(0xFF, 0xFF, 0xFF),
    er_header: rgb(0xDC, 0xE8, 0xF5),
    er_view_header: rgb(0xE8, 0xF3, 0xE0),
    er_line: rgb(0x30, 0x50, 0x90),
    er_line_dim: rgb(0xB8, 0xC4, 0xD8),
    er_type: rgb(0x70, 0x70, 0x70),
    syn_keyword: rgb(0x00, 0x00, 0xE0),
    syn_function: rgb(0x79, 0x5E, 0x26),
    syn_string: rgb(0xA3, 0x15, 0x15),
    syn_number: rgb(0x09, 0x86, 0x58),
    syn_comment: rgb(0x00, 0x80, 0x00),
    syn_ident: rgb(0x26, 0x7F, 0x99),
    syn_text: rgb(0x1E, 0x1E, 0x1E),
    syn_gutter: rgb(0x23, 0x78, 0x93),
};

static DARK: AtomicBool = AtomicBool::new(true);

pub fn pal() -> &'static Pal {
    if DARK.load(Ordering::Relaxed) { &DARK_PAL } else { &LIGHT_PAL }
}

fn load_font(candidates: &[&str]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    // Systemschriften laden (Segoe UI / Consolas wie in VS Code unter Windows)
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
        fonts.font_data.insert("sys".into(), Arc::new(FontData::from_owned(data)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "sys".into());
    }
    if let Some(data) = mono {
        fonts.font_data.insert("sysmono".into(), Arc::new(FontData::from_owned(data)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "sysmono".into());
    }
    ctx.set_fonts(fonts);
    set_theme(ctx, dark);
}

pub fn set_theme(ctx: &egui::Context, dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| {
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
        sp.menu_margin = Margin::same(4);
        sp.scroll = egui::style::ScrollStyle::solid();
        sp.combo_width = 160.0;
        style.animation_time = 0.0;
        style.visuals = visuals(pal());
    });
}

fn visuals(p: &Pal) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(2);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(if p.dark { 120 } else { 50 }) };
    v.popup_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
    v.window_fill = p.face;
    v.panel_fill = p.face;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.bg;
    v.text_edit_bg_color = Some(if p.dark { rgb(0x3C, 0x3C, 0x3C) } else { Color32::WHITE });
    v.faint_bg_color = if p.dark { rgb(0x2A, 0x2A, 0x2A) } else { rgb(0xF6, 0xF6, 0xF6) };
    v.code_bg_color = p.bg;
    v.hyperlink_color = if p.dark { rgb(0x37, 0x94, 0xFF) } else { p.accent };
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.window_highlight_topmost = false;
    v.striped = true;
    v.override_text_color = Some(p.text);

    let w = &mut v.widgets;
    let (btn, btn_hover, btn_active) = if p.dark {
        (rgb(0x3A, 0x3D, 0x41), rgb(0x45, 0x49, 0x4E), rgb(0x0E, 0x63, 0x9C))
    } else {
        (rgb(0xFD, 0xFD, 0xFD), rgb(0xE5, 0xF1, 0xFB), rgb(0xCC, 0xE4, 0xF7))
    };
    w.noninteractive.bg_fill = p.face;
    w.noninteractive.weak_bg_fill = p.face;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;
    for (ws, fill, stroke) in [
        (&mut w.inactive, btn, Stroke::new(1.0, p.border)),
        (&mut w.hovered, btn_hover, Stroke::new(1.0, p.accent)),
        (&mut w.active, btn_active, Stroke::new(1.0, p.accent)),
        (&mut w.open, btn_hover, Stroke::new(1.0, p.border)),
    ] {
        ws.bg_fill = fill;
        ws.weak_bg_fill = fill;
        ws.bg_stroke = stroke;
        ws.fg_stroke = Stroke::new(1.0, if p.dark { rgb(0xE0, 0xE0, 0xE0) } else { p.text });
        ws.corner_radius = r;
        ws.expansion = 0.0;
    }
    v
}

/// Umrandeter Inhaltsbereich (wie ein Listenfeld).
pub fn sunken_frame() -> egui::Frame {
    egui::Frame::new().fill(pal().bg).stroke(Stroke::new(1.0, pal().border)).inner_margin(Margin::same(2))
}

/// Gruppenrahmen.
pub fn group_frame() -> egui::Frame {
    egui::Frame::new().stroke(Stroke::new(1.0, pal().border)).inner_margin(Margin::same(6))
}
