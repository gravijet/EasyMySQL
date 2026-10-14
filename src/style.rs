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
    pub null_text: Color32,
    pub error_text: Color32,
    pub warn_text: Color32,
    pub ok_text: Color32,
    pub activity_fg: Color32,
    pub activity_active: Color32,
    pub sidebar_bg: Color32,
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
    pub syn_type: Color32,
    pub syn_const: Color32,
    pub syn_variable: Color32,
    pub syn_op: Color32,
    pub syn_text: Color32,
    pub syn_gutter: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub static DARK_PAL: Pal = Pal {
    dark: true,
    face: rgb(0x15, 0x17, 0x1B),
    bg: rgb(0x1D, 0x20, 0x26),
    face_light: rgb(0x26, 0x2A, 0x32),
    text: rgb(0xD9, 0xDC, 0xE3),
    text_weak: rgb(0x8E, 0x95, 0xA3),
    border: rgb(0x2C, 0x30, 0x38),
    accent: rgb(0x4F, 0x8C, 0xFF),
    null_text: rgb(0x70, 0x77, 0x85),
    error_text: rgb(0xF4, 0x87, 0x71),
    warn_text: rgb(0xE5, 0xB8, 0x4B),
    ok_text: rgb(0x89, 0xD1, 0x85),
    activity_fg: rgb(0x82, 0x89, 0x97),
    activity_active: rgb(0xF0, 0xF2, 0xF6),
    sidebar_bg: rgb(0x1A, 0x1D, 0x22),
    current_line: rgb(0x22, 0x26, 0x2D),
    selection: rgb(0x2B, 0x4A, 0x7E),
    find_match: rgb(0x61, 0x51, 0x1E),
    bracket: rgb(0x40, 0x5A, 0x40),
    hover: rgb(0x2A, 0x2E, 0x37),
    er_canvas: rgb(0x1D, 0x20, 0x26),
    er_box: rgb(0x24, 0x28, 0x30),
    er_header: rgb(0x22, 0x3F, 0x6B),
    er_view_header: rgb(0x2C, 0x4F, 0x3E),
    er_line: rgb(0x5E, 0x9E, 0xF2),
    er_line_dim: rgb(0x37, 0x42, 0x54),
    er_type: rgb(0x8E, 0x95, 0xA3),
    syn_keyword: rgb(0x56, 0x9C, 0xD6),
    syn_function: rgb(0xDC, 0xDC, 0xAA),
    syn_string: rgb(0xCE, 0x91, 0x78),
    syn_number: rgb(0xB5, 0xCE, 0xA8),
    syn_comment: rgb(0x6A, 0x99, 0x55),
    syn_ident: rgb(0x9C, 0xDC, 0xFE),
    syn_type: rgb(0x4E, 0xC9, 0xB0),
    syn_const: rgb(0x4F, 0xC1, 0xFF),
    syn_variable: rgb(0xC5, 0x86, 0xC0),
    syn_op: rgb(0xA8, 0xAE, 0xB8),
    syn_text: rgb(0xD4, 0xD4, 0xD4),
    syn_gutter: rgb(0x85, 0x85, 0x85),
};

pub static LIGHT_PAL: Pal = Pal {
    dark: false,
    face: rgb(0xE8, 0xEB, 0xF0),
    bg: rgb(0xFF, 0xFF, 0xFF),
    face_light: rgb(0xF4, 0xF6, 0xF9),
    text: rgb(0x1E, 0x22, 0x29),
    text_weak: rgb(0x63, 0x6B, 0x7A),
    border: rgb(0xD6, 0xDB, 0xE3),
    accent: rgb(0x2B, 0x6C, 0xE6),
    null_text: rgb(0x98, 0xA0, 0xAD),
    error_text: rgb(0xB0, 0x00, 0x00),
    warn_text: rgb(0xB0, 0x7A, 0x00),
    ok_text: rgb(0x00, 0x60, 0x00),
    activity_fg: rgb(0x63, 0x6B, 0x7A),
    activity_active: rgb(0x2B, 0x6C, 0xE6),
    sidebar_bg: rgb(0xF5, 0xF7, 0xFA),
    current_line: rgb(0xF3, 0xF6, 0xFC),
    selection: rgb(0xB9, 0xD3, 0xFB),
    find_match: rgb(0xFF, 0xE0, 0x80),
    bracket: rgb(0xC8, 0xE6, 0xC8),
    hover: rgb(0xE6, 0xEA, 0xF1),
    er_canvas: rgb(0xFF, 0xFF, 0xFF),
    er_box: rgb(0xFF, 0xFF, 0xFF),
    er_header: rgb(0xDC, 0xE8, 0xFA),
    er_view_header: rgb(0xE3, 0xF2, 0xE6),
    er_line: rgb(0x3A, 0x62, 0xB0),
    er_line_dim: rgb(0xC3, 0xCC, 0xDC),
    er_type: rgb(0x6B, 0x73, 0x82),
    syn_keyword: rgb(0x00, 0x00, 0xE0),
    syn_function: rgb(0x79, 0x5E, 0x26),
    syn_string: rgb(0xA3, 0x15, 0x15),
    syn_number: rgb(0x09, 0x86, 0x58),
    syn_comment: rgb(0x00, 0x80, 0x00),
    syn_ident: rgb(0x00, 0x10, 0x80),
    syn_type: rgb(0x26, 0x7F, 0x99),
    syn_const: rgb(0x00, 0x70, 0xC1),
    syn_variable: rgb(0xAF, 0x00, 0xDB),
    syn_op: rgb(0x5C, 0x63, 0x70),
    syn_text: rgb(0x1E, 0x1E, 0x1E),
    syn_gutter: rgb(0x23, 0x78, 0x93),
};

/// Grundradius der Bedienelemente (Schaltflaechen, Eingabefelder, Listeneintraege)
pub const RADIUS: u8 = 7;
/// Radius der grossen Flaechen (Seitenleiste, Inhalt, Karten)
pub const PANEL_RADIUS: u8 = 12;

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
        sp.item_spacing = egui::vec2(8.0, 6.0);
        sp.button_padding = egui::vec2(14.0, 5.0);
        sp.interact_size.y = 28.0;
        sp.icon_width = 16.0;
        sp.icon_width_inner = 8.0;
        sp.icon_spacing = 8.0;
        sp.slider_width = 200.0;
        sp.slider_rail_height = 4.0;
        sp.window_margin = Margin::same(14);
        sp.menu_margin = Margin::same(6);
        // Schlanke Bildlaufleisten wie in Visual Studio Code: dezent, ueber dem Inhalt, beim Darueberfahren breiter
        sp.scroll = egui::style::ScrollStyle {
            floating: true,
            bar_width: 10.0,
            floating_width: 6.0,
            floating_allocated_width: 6.0,
            handle_min_length: 28.0,
            foreground_color: true,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.0,
            interact_background_opacity: 0.1,
            dormant_handle_opacity: 0.3,
            active_handle_opacity: 0.5,
            interact_handle_opacity: 0.75,
            ..egui::style::ScrollStyle::solid()
        };
        sp.combo_width = 160.0;
        style.animation_time = 0.0;
        style.visuals = visuals(pal());
    });
}

fn visuals(p: &Pal) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(RADIUS);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 150 } else { 46 }) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }) };
    v.window_fill = p.face_light;
    v.panel_fill = p.face;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.bg;
    v.text_edit_bg_color = Some(if p.dark { rgb(0x14, 0x16, 0x1A) } else { Color32::WHITE });
    v.faint_bg_color = if p.dark { rgb(0x22, 0x25, 0x2C) } else { rgb(0xF6, 0xF8, 0xFB) };
    v.code_bg_color = p.bg;
    v.hyperlink_color = if p.dark { rgb(0x6C, 0xA4, 0xFF) } else { p.accent };
    v.selection.bg_fill = p.selection;
    // Fokusrahmen von Eingabefeldern und Schaltflaechen
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_highlight_topmost = false;
    v.striped = false;
    v.override_text_color = Some(p.text);

    let w = &mut v.widgets;
    // Flache Schaltflaechen: dunkles Grau, beim Darueberfahren heller, beim Druecken blau (wie die Schaltflaechen in VS Code)
    let (btn, btn_hover, btn_active) = if p.dark {
        (rgb(0x2B, 0x2F, 0x38), rgb(0x36, 0x3B, 0x47), rgb(0x2A, 0x55, 0xA8))
    } else {
        (rgb(0xE6, 0xE9, 0xEF), rgb(0xDA, 0xDF, 0xE8), rgb(0xBC, 0xD3, 0xF7))
    };
    // Rand der Eingabefelder und Haken: im hellen Design sichtbar, im dunklen unauffaellig
    let edge = if p.dark { Stroke::new(1.0, rgb(0x37, 0x3C, 0x47)) } else { Stroke::new(1.0, rgb(0xC9, 0xCF, 0xDA)) };
    w.noninteractive.bg_fill = p.face;
    w.noninteractive.weak_bg_fill = p.face;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;
    for (ws, fill, stroke) in [
        (&mut w.inactive, btn, edge),
        (&mut w.hovered, btn_hover, edge),
        (&mut w.active, btn_active, Stroke::new(1.0, p.accent)),
        (&mut w.open, btn_hover, edge),
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

/// Einträge in Menüs: mehr Innenabstand, beim Darüberfahren eine abgerundete Fläche in der Auswahlfarbe
pub fn menu_item_look(ui: &mut egui::Ui) {
    let p = pal();
    ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
    let w = &mut ui.visuals_mut().widgets;
    w.hovered.weak_bg_fill = p.selection;
    w.hovered.bg_fill = p.selection;
    w.hovered.fg_stroke = Stroke::new(1.0, if p.dark { Color32::WHITE } else { p.text });
    w.active.weak_bg_fill = p.selection.gamma_multiply(1.25);
    w.active.bg_fill = p.selection.gamma_multiply(1.25);
}

/// Umrandeter Inhaltsbereich (wie ein Listenfeld).
pub fn sunken_frame() -> egui::Frame {
    egui::Frame::new().fill(pal().bg).stroke(Stroke::new(1.0, pal().border)).corner_radius(CornerRadius::same(RADIUS + 3)).inner_margin(Margin::same(3))
}

/// Abgerundete Flaeche, die genau den freien Platz fuellt – unabhaengig davon, wie gross der Inhalt
/// ist (zu breiter Inhalt wird abgeschnitten statt die Flaeche zu verbreitern).
pub fn panel_card(ui: &mut egui::Ui, fill: Color32, margin: Margin, add: impl FnOnce(&mut egui::Ui)) {
    let outer = ui.available_rect_before_wrap();
    ui.painter().rect(outer, CornerRadius::same(PANEL_RADIUS), fill, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
    let inner = egui::Rect::from_min_max(
        outer.min + egui::vec2(margin.left as f32 + 1.0, margin.top as f32 + 1.0),
        outer.max - egui::vec2(margin.right as f32 + 1.0, margin.bottom as f32 + 1.0),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(outer.shrink(1.0).intersect(ui.clip_rect()));
    add(&mut child);
    ui.advance_cursor_after_rect(outer);
}

/// Gruppenrahmen (Karte).
pub fn group_frame() -> egui::Frame {
    crate::widgets::card()
}

/// Navigation mit einheitlichem Einzug, Hover-Feedback und aktiver Kategorie.
pub fn nav_item(ui: &mut egui::Ui, title: &str, active: bool, indent: f32) -> egui::Response {
    let p = pal();
    let width = ui.available_width();
    let color = if active && !p.dark { p.accent } else { p.text };
    let text = ui.painter().layout(title.to_string(), FontId::proportional(13.0), color, (width - indent - 24.0).max(40.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, text.size().y + 14.0), egui::Sense::click());
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, RADIUS as f32, if active { p.selection } else { p.hover });
    }
    ui.painter().galley(rect.min + egui::vec2(indent + 14.0, 7.0), text, color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), active, title));
    response
}
