// Helles Standard-Design und gemeinsame Navigation.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

/// Farbpalette der Oberflaeche.
pub struct Pal {
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

pub static LIGHT_PAL: Pal = Pal {
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
    activity_bg: rgb(0xE8, 0xE8, 0xE8),
    activity_fg: rgb(0x61, 0x61, 0x61),
    activity_active: rgb(0x00, 0x78, 0xD7),
    sidebar_bg: rgb(0xF3, 0xF3, 0xF3),
    tab_bar_bg: rgb(0xEC, 0xEC, 0xEC),
    tab_active: rgb(0xFF, 0xFF, 0xFF),
    tab_inactive: rgb(0xE4, 0xE4, 0xE4),
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

pub fn pal() -> &'static Pal {
    &LIGHT_PAL
}

fn load_font(candidates: &[&str]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn apply(ctx: &egui::Context) {
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
    set_theme(ctx);
}

fn set_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
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
    let mut v = Visuals::light();
    let r = CornerRadius::same(2);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(50) };
    v.popup_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(40) };
    v.window_fill = p.face;
    v.panel_fill = p.face;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.bg;
    v.text_edit_bg_color = Some(Color32::WHITE);
    v.faint_bg_color = rgb(0xF6, 0xF6, 0xF6);
    v.code_bg_color = p.bg;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.selection;
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.window_highlight_topmost = false;
    v.striped = true;
    v.override_text_color = Some(p.text);

    let w = &mut v.widgets;
    let (btn, btn_hover, btn_active) = (rgb(0xFD, 0xFD, 0xFD), rgb(0xE5, 0xF1, 0xFB), rgb(0xCC, 0xE4, 0xF7));
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
        ws.fg_stroke = Stroke::new(1.0, p.text);
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

/// Navigation mit einheitlichem Einzug, Hover-Feedback und aktiver Kategorie.
pub fn nav_item(ui: &mut egui::Ui, title: &str, active: bool, indent: f32) -> egui::Response {
    let p = pal();
    let width = ui.available_width();
    let color = if active { p.accent } else { p.text };
    let text = ui.painter().layout(title.to_string(), FontId::proportional(13.0), color, (width - indent - 24.0).max(40.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, text.size().y + 14.0), egui::Sense::click());
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 3.0, if active { p.selection } else { p.hover });
    }
    if active {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 1.0, p.accent);
    }
    ui.painter().galley(rect.min + egui::vec2(indent + 12.0, 7.0), text, color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), active, title));
    response
}
