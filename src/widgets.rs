// Eigene Bedienelemente im Stil moderner Editoren: Schalter, Segmentwahl, Tastenkappen, Karten.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, FontId, Frame, Margin, Rect, Response, Sense, Stroke, StrokeKind, pos2, vec2};

/// Kartenflaeche fuer zusammengehoerige Einstellungen
pub fn card() -> Frame {
    let p = style::pal();
    Frame::new().fill(if p.dark { Color32::from_rgb(0x22, 0x25, 0x2C) } else { Color32::from_rgb(0xF7, 0xF8, 0xFB) }).stroke(Stroke::new(1.0, p.border)).corner_radius(style::PANEL_RADIUS - 2).inner_margin(Margin::symmetric(18, 14))
}

/// Ankreuzfeld: abgerundetes Quadrat, bei Haken in Akzentfarbe. Beschriftung rechts, mittig zur Zeile.
pub fn check(ui: &mut egui::Ui, on: &mut bool, label: impl Into<egui::WidgetText>) -> Response {
    let p = style::pal();
    let label: egui::WidgetText = label.into();
    let galley = label.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
    let (box_s, gap) = (16.0, 8.0);
    let has_label = !galley.text().is_empty();
    let size = vec2(box_s + if has_label { gap + galley.size().x } else { 0.0 }, ui.spacing().interact_size.y.min(26.0).max(box_s + 4.0));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let b = Rect::from_min_size(pos2(rect.left(), rect.center().y - box_s / 2.0), vec2(box_s, box_s));
    let painter = ui.painter();
    if *on {
        painter.rect_filled(b, 5.0, if resp.hovered() { p.accent.lerp_to_gamma(Color32::WHITE, 0.12) } else { p.accent });
        icons::paint(painter, b.shrink(2.5), Icon::Check, Color32::WHITE);
    } else {
        let edge = if resp.hovered() { p.text_weak } else if p.dark { Color32::from_gray(0x55) } else { Color32::from_gray(0xB0) };
        painter.rect(b, 5.0, if p.dark { Color32::from_rgb(0x14, 0x16, 0x1A) } else { Color32::WHITE }, Stroke::new(1.2, edge), StrokeKind::Inside);
    }
    if has_label {
        let color = if ui.is_enabled() { p.text } else { p.text_weak };
        ui.painter().galley(pos2(b.right() + gap, rect.center().y - galley.size().y / 2.0), galley, color);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(b.expand(2.0), 6.0, Stroke::new(1.0, p.accent), StrokeKind::Outside);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    resp
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


/// Hervorgehobene Hauptaktion nur mit Text (Akzentfarbe, weisse Schrift)
pub fn primary(ui: &mut egui::Ui, text: &str) -> Response {
    let p = style::pal();
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(p.accent).stroke(Stroke::NONE).min_size(vec2(0.0, ui.spacing().interact_size.y)))
}

// ---------------------------------------------------------------------------
// Formulare: einheitliche Hoehen, Beschriftungen links (bei schmalem Platz darueber) und Karten

/// Hoehe aller Eingabefelder und Schaltflaechen in Formularen
pub const CONTROL_H: f32 = 28.0;
/// Breite der Beschriftungsspalte
pub const LABEL_W: f32 = 176.0;
/// Ab dieser Breite stehen Beschriftung und Feld nebeneinander
const ROW_WIDE: f32 = 460.0;

/// Zeile eines Formulars: Beschriftung links, Feld rechts (bei wenig Platz uebereinander)
pub fn row<R>(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = style::pal();
    if ui.available_width() >= ROW_WIDE {
        ui.horizontal(|ui| {
            let galley = ui.painter().layout(label.to_string(), FontId::proportional(13.0), p.text_weak, LABEL_W - 12.0);
            let (rect, _) = ui.allocate_exact_size(vec2(LABEL_W, CONTROL_H.max(galley.size().y)), Sense::hover());
            ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, p.text_weak);
            // Textfelder fuellen den Rest, hoechstens 360 breit
            ui.spacing_mut().text_edit_width = (ui.available_width() - 8.0).clamp(120.0, 360.0);
            add(ui)
        })
        .inner
    } else {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).small().color(p.text_weak));
            ui.spacing_mut().text_edit_width = ui.available_width().clamp(120.0, 360.0);
            add(ui)
        })
        .inner
    }
}

/// Einzeiliges Textfeld mit einheitlicher Hoehe und mittig stehendem Text; `width` None = restliche Breite
pub fn text_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: Option<f32>) -> Response {
    let w = width.unwrap_or_else(|| ui.available_width()).max(40.0);
    ui.add_sized([w, CONTROL_H], egui::TextEdit::singleline(text).hint_text(hint).margin(Margin::symmetric(8, 0)).vertical_align(egui::Align::Center))
}

pub fn password_field(ui: &mut egui::Ui, text: &mut String, hint: &str, width: Option<f32>) -> Response {
    let w = width.unwrap_or_else(|| ui.available_width()).max(40.0);
    ui.add_sized([w, CONTROL_H], egui::TextEdit::singleline(text).password(true).hint_text(hint).margin(Margin::symmetric(8, 0)).vertical_align(egui::Align::Center))
}

/// Zahlenfeld fester Breite
pub fn number_field<N: egui::emath::Numeric>(ui: &mut egui::Ui, value: &mut N, range: std::ops::RangeInclusive<N>) -> Response {
    ui.add_sized([88.0, CONTROL_H], egui::DragValue::new(value).range(range))
}

/// Abschnitt mit Titel in einer Karte
pub fn section<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = style::pal();
    if !title.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(title).strong().size(14.0).color(p.text));
        ui.add_space(2.0);
    }
    card()
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            add(ui)
        })
        .inner
}

/// Auswahl aus mehreren Moeglichkeiten als umbrechende Reihe von Schaltflaechen
pub fn chips<T: PartialEq + Clone>(ui: &mut egui::Ui, value: &mut T, items: &[(T, String)]) -> bool {
    let p = style::pal();
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (v, label) in items {
            let selected = value == v;
            let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(13.0), p.text);
            let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 26.0, CONTROL_H), Sense::click());
            let (fill, fg, stroke) = if selected {
                (p.accent, Color32::WHITE, Stroke::NONE)
            } else if resp.hovered() {
                (p.hover, p.text, Stroke::new(1.0, p.border))
            } else {
                (Color32::TRANSPARENT, p.text, Stroke::new(1.0, p.border))
            };
            ui.painter().rect(r, style::RADIUS as f32, fill, stroke, StrokeKind::Inside);
            ui.painter().galley_with_override_text_color(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, fg);
            if resp.clicked() && !selected {
                *value = v.clone();
                changed = true;
            }
        }
    });
    changed
}

/// Zahlenfeld mit Einheit (z. B. " h") und einheitlicher Hoehe
pub fn drag<N: egui::emath::Numeric>(ui: &mut egui::Ui, value: &mut N, range: std::ops::RangeInclusive<N>, suffix: &str) -> Response {
    ui.add_sized([if suffix.len() > 3 { 104.0 } else { 78.0 }, CONTROL_H], egui::DragValue::new(value).range(range).suffix(suffix))
}

/// Einzeiliges Textfeld in der einheitlichen Hoehe (Text mittig); Breite wie bei `TextEdit` einstellbar
pub fn edit(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(Margin::symmetric(8, 6)).vertical_align(egui::Align::Center)
}
