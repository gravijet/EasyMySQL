// Eigene Bedienelemente im Stil moderner Editoren: Schalter, Segmentwahl, Tastenkappen, Karten.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, FontId, Frame, Margin, Rect, Response, Sense, Stroke, StrokeKind, pos2, vec2};

/// Kartenflaeche fuer zusammengehoerige Einstellungen
pub fn card() -> Frame {
    let p = style::pal();
    Frame::new().fill(if p.dark { Color32::from_rgb(0x22, 0x25, 0x2C) } else { Color32::from_rgb(0xF7, 0xF8, 0xFB) }).stroke(Stroke::new(1.0, p.border)).corner_radius(style::PANEL_RADIUS - 2).inner_margin(Margin::symmetric(18, 14))
}

/// Ein-/Ausschalter mit Beschriftung
pub fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> Response {
    let p = style::pal();
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), p.text);
    let (track_w, track_h, gap) = (30.0, 16.0, 9.0);
    let size = vec2(track_w + if label.is_empty() { 0.0 } else { gap + galley.size().x }, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - track_h / 2.0), vec2(track_w, track_h));
    let off = if p.dark { Color32::from_gray(0x4A) } else { Color32::from_gray(0xBB) };
    let fill = if *on { p.accent } else { off };
    let fill = if resp.hovered() { fill.lerp_to_gamma(Color32::WHITE, 0.12) } else { fill };
    ui.painter().rect_filled(track, track_h / 2.0, fill);
    let knob_x = if *on { track.right() - track_h / 2.0 } else { track.left() + track_h / 2.0 };
    ui.painter().circle_filled(pos2(knob_x, track.center().y), track_h / 2.0 - 3.0, Color32::WHITE);
    if !label.is_empty() {
        ui.painter().galley(pos2(track.right() + gap, rect.center().y - galley.size().y / 2.0), galley, p.text);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(track.expand(2.0), track_h / 2.0 + 2.0, Stroke::new(1.0, p.accent), StrokeKind::Outside);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label));
    resp
}

/// Auswahl einer von wenigen Moeglichkeiten als verbundene Schaltflaechen. Gibt true zurueck, wenn sich der Wert aendert.
pub fn segmented<T: PartialEq + Clone>(ui: &mut egui::Ui, value: &mut T, items: &[(T, &str)]) -> bool {
    let p = style::pal();
    let font = FontId::proportional(13.0);
    let galleys: Vec<_> = items.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), font.clone(), p.text)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 24.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 26.0), Sense::hover());
    let back = if p.dark { Color32::from_gray(0x2F) } else { Color32::from_gray(0xE8) };
    ui.painter().rect_filled(rect, 6.0, back);
    let mut changed = false;
    let mut x = rect.left() + 2.0;
    for (((v, label), g), w) in items.iter().zip(galleys).zip(&widths) {
        let seg = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(*w, rect.height() - 4.0));
        let resp = ui.interact(seg, ui.id().with(label), Sense::click());
        let selected = value == v;
        if selected {
            ui.painter().rect_filled(seg, 5.0, p.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(seg, 5.0, p.hover);
        }
        let color = if selected { Color32::WHITE } else { p.text };
        ui.painter().galley(pos2(seg.center().x - g.size().x / 2.0, seg.center().y - g.size().y / 2.0), g, color);
        if resp.clicked() && !selected {
            *value = v.clone();
            changed = true;
        }
        x += w;
    }
    changed
}

#[derive(Clone, Copy, PartialEq)]
pub enum Keycap {
    Normal,
    Recording,
    Conflict,
}

/// Tastenkappe (Tastenkuerzel); klickbar
pub fn keycap(ui: &mut egui::Ui, text: &str, state: Keycap) -> Response {
    let p = style::pal();
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::monospace(12.0), p.text);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 22.0), Sense::click());
    let (fill, edge, fg) = match state {
        Keycap::Recording => (p.accent.gamma_multiply(0.25), p.accent, p.text),
        Keycap::Conflict => (p.error_text.gamma_multiply(0.12), p.error_text, p.error_text),
        Keycap::Normal if resp.hovered() => (p.hover, p.accent, p.text),
        Keycap::Normal => (if p.dark { Color32::from_gray(0x2D) } else { Color32::from_gray(0xF3) }, if p.dark { Color32::from_gray(0x44) } else { Color32::from_gray(0xCC) }, p.text),
    };
    let r = rect.shrink2(vec2(0.0, 1.0));
    ui.painter().rect_filled(r, 5.0, fill);
    ui.painter().rect_stroke(r, 5.0, Stroke::new(1.0, edge), StrokeKind::Inside);
    // Unterkante wie bei einer echten Taste
    ui.painter().hline(r.left() + 5.0..=r.right() - 5.0, r.bottom() - 0.5, Stroke::new(1.0, edge.gamma_multiply(0.6)));
    ui.painter().galley(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0 - 0.5), galley, fg);
    resp
}

/// Kleiner Knopf mit Plus (z. B. "weiteres Kuerzel")
pub fn ghost_plus(ui: &mut egui::Ui, tip: &str) -> Response {
    icons::button_sized(ui, Icon::Plus, tip, 22.0)
}

/// Auswahlfeld mit dem Pfeil aus dem Symbolsatz
pub fn combo(id: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox {
    egui::ComboBox::from_id_salt(id).icon(|ui: &egui::Ui, rect: Rect, visuals: &egui::style::WidgetVisuals, _open: bool| {
        let c = visuals.fg_stroke.color;
        icons::paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(13.0, 13.0)), Icon::ChevronDown, c);
    })
}

