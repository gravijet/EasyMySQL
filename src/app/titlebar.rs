// Eigene Titelleiste: Minimieren, Maximieren und Schliessen sitzen direkt in der Menueleiste
// (statt in einer zusaetzlichen Zeile des Betriebssystems). Dazu gehoeren Ziehen, Doppelklick
// und die Griffe an den Fensterkanten.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, Pos2, Rect, ResizeDirection, Sense, Stroke, ViewportCommand, pos2, vec2};

/// Hoehe der Titelleiste
pub const HEIGHT: f32 = 40.0;
/// Breite eines Fensterknopfs
const BUTTON_W: f32 = 46.0;
/// Breite der Griffe an den Fensterkanten
const EDGE: f32 = 5.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn paint_glyph(p: &egui::Painter, c: Pos2, kind: Kind, color: Color32) {
    let s = Stroke::new(1.2, color);
    let h = 5.0;
    match kind {
        Kind::Minimize => {
            p.line_segment([pos2(c.x - h, c.y + 0.5), pos2(c.x + h, c.y + 0.5)], s);
        }
        Kind::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, vec2(h * 2.0, h * 2.0)), 1.5, s, egui::StrokeKind::Middle);
        }
        Kind::Restore => {
            // Zwei uebereinanderliegende Quadrate
            let back = Rect::from_center_size(c + vec2(1.5, -1.5), vec2(h * 2.0 - 2.0, h * 2.0 - 2.0));
            let front = Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(h * 2.0 - 2.0, h * 2.0 - 2.0));
            p.line_segment([back.left_top() + vec2(2.0, 0.0), back.right_top()], s);
            p.line_segment([back.right_top(), back.right_bottom() - vec2(0.0, 2.0)], s);
            p.rect_stroke(front, 1.5, s, egui::StrokeKind::Middle);
        }
        Kind::Close => {
            p.line_segment([pos2(c.x - h + 0.5, c.y - h + 0.5), pos2(c.x + h - 0.5, c.y + h - 0.5)], s);
            p.line_segment([pos2(c.x - h + 0.5, c.y + h - 0.5), pos2(c.x + h - 0.5, c.y - h + 0.5)], s);
        }
    }
}

/// Die drei Fensterknoepfe rechts in der Leiste (Layout von rechts nach links)
pub fn controls(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let pal = style::pal();
    let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
    let buttons = [
        (Kind::Close, crate::i18n::tr("Schließen", "Close"), ViewportCommand::Close),
        (if maximized { Kind::Restore } else { Kind::Maximize }, if maximized { crate::i18n::tr("Verkleinern", "Restore") } else { crate::i18n::tr("Maximieren", "Maximize") }, ViewportCommand::Maximized(!maximized)),
        (Kind::Minimize, crate::i18n::tr("Minimieren", "Minimize"), ViewportCommand::Minimized(true)),
    ];
    let saved = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (kind, tip, cmd) in buttons {
        let (r, resp) = ui.allocate_exact_size(vec2(BUTTON_W, HEIGHT), Sense::click());
        let danger = kind == Kind::Close;
        if resp.hovered() {
            let fill = if danger { Color32::from_rgb(0xE8, 0x11, 0x23) } else if pal.dark { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(16) };
            ui.painter().rect_filled(r, 0.0, fill);
        }
        let color = if danger && resp.hovered() { Color32::WHITE } else { pal.text };
        paint_glyph(ui.painter(), r.center(), kind, color);
        if resp.on_hover_text(tip).clicked() {
            ctx.send_viewport_cmd(cmd);
        }
    }
    ui.spacing_mut().item_spacing.x = saved;
}

/// Was links in einem Knopf der Leiste steht
pub enum Lead {
    Icon(Icon, Color32),
    /// Farbiges Kaestchen mit Kuerzel (Projekt wie in IntelliJ IDEA)
    Badge(String, Color32),
    /// Zustandspunkt (Server)
    Dot(Color32),
}

/// Hoehe der Knoepfe in der Leiste
pub const TOOL_H: f32 = 28.0;

/// Flacher Knopf der Leiste (wie die Widgets der Hauptwerkzeugleiste von IntelliJ IDEA): erst beim
/// Darueberfahren eine Flaeche; Symbol, Text und Pfeil liegen auf einer gemeinsamen Mittellinie.
pub fn tool_button(ui: &mut egui::Ui, lead: Lead, text: &str, color: Color32, chevron: bool) -> egui::Response {
    let pal = style::pal();
    let font = egui::FontId::proportional(13.0);
    // Lange Namen (Projekt, Branch) mit "…" kuerzen, damit die Leiste nicht ueberlaeuft
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: (ui.ctx().content_rect().width() * 0.18).clamp(90.0, 260.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    // Breite des Vorspanns (ohne Abstand zum Text)
    let lead_w = match &lead {
        Lead::Icon(..) => 16.0,
        Lead::Badge(..) => 20.0,
        Lead::Dot(_) => 8.0,
    };
    let text_w = if text.is_empty() { 0.0 } else { galley.size().x };
    let gap = if lead_w > 0.0 && text_w > 0.0 { 8.0 } else { 0.0 };
    let chev_w = if chevron { 4.0 + 12.0 } else { 0.0 };
    let pad = if text_w > 0.0 { 8.0 } else { 6.0 };
    let content = lead_w + gap + text_w + chev_w;
    let (r, resp) = ui.allocate_exact_size(vec2((content + 2.0 * pad).max(TOOL_H), TOOL_H), Sense::click());
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, style::RADIUS as f32, pal.hover.gamma_multiply(1.3));
    } else if resp.hovered() {
        p.rect_filled(r, style::RADIUS as f32, pal.hover);
    }
    let cy = r.center().y;
    // Inhalt waagrecht mittig (bei reinen Symbolknoepfen ist die Flaeche quadratisch)
    let mut x = r.center().x - content / 2.0;
    match lead {
        Lead::Icon(icon, c) => icons::paint(p, Rect::from_center_size(pos2(x + 8.0, cy), vec2(16.0, 16.0)), icon, c),
        Lead::Badge(t, c) => {
            let b = Rect::from_center_size(pos2(x + 10.0, cy), vec2(20.0, 20.0));
            icons::badge(p, b, &t, c, Color32::WHITE, 5.0);
        }
        Lead::Dot(c) => {
            p.circle_filled(pos2(x + 4.0, cy), 4.0, c);
        }
    }
    x += lead_w + gap;
    if !text.is_empty() {
        p.galley(pos2(x, cy - galley.size().y / 2.0), galley, color);
        x += text_w;
    }
    if chevron {
        icons::paint(p, Rect::from_center_size(pos2(x + 4.0 + 6.0, cy), vec2(12.0, 12.0)), Icon::ChevronDown, pal.text_weak);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Suchfeld in der Mitte der Leiste (wie die Befehlszentrale von Visual Studio Code): Klick oeffnet die Befehle
pub fn search_box(ui: &mut egui::Ui, r: Rect, hint: &str, keys: &str) -> egui::Response {
    let pal = style::pal();
    let resp = ui.interact(r, ui.id().with("titlebar-search"), Sense::click());
    let fill = if resp.hovered() { pal.face_light } else { pal.bg };
    let p = ui.painter();
    p.rect(r, style::RADIUS as f32 + 1.0, fill, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
    let cy = r.center().y;
    icons::paint(p, Rect::from_center_size(pos2(r.left() + 18.0, cy), vec2(14.0, 14.0)), Icon::Search, pal.text_weak);
    let keys_g = p.layout_no_wrap(keys.to_string(), egui::FontId::proportional(12.0), pal.text_weak);
    let show_keys = !keys.is_empty() && r.width() >= 300.0;
    let max_text = r.width() - 36.0 - if show_keys { keys_g.size().x + 20.0 } else { 10.0 };
    let mut job = egui::text::LayoutJob::simple_singleline(hint.to_string(), egui::FontId::proportional(13.0), pal.text_weak);
    job.wrap = egui::text::TextWrapping { max_width: max_text.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = p.layout_job(job);
    p.galley(pos2(r.left() + 32.0, cy - g.size().y / 2.0), g, pal.text_weak);
    if show_keys {
        p.galley(pos2(r.right() - 12.0 - keys_g.size().x, cy - keys_g.size().y / 2.0), keys_g, pal.text_weak);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Kuerzel und Farbe des Projekts fuer das Kaestchen (Farbe bleibt je Name gleich)
pub fn project_badge(name: &str) -> (String, Color32) {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let initials: String = match words.as_slice() {
        [] => "?".into(),
        [one] => one.chars().take(2).collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    const COLORS: [Color32; 6] = [
        Color32::from_rgb(0x35, 0x74, 0xF0),
        Color32::from_rgb(0x2E, 0x9E, 0x6B),
        Color32::from_rgb(0xC2, 0x5B, 0x2E),
        Color32::from_rgb(0x8E, 0x56, 0xD6),
        Color32::from_rgb(0xC9, 0x3D, 0x6D),
        Color32::from_rgb(0x1F, 0x8F, 0xA8),
    ];
    let h = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    (initials.to_uppercase(), COLORS[h as usize % COLORS.len()])
}

/// Leere Flaeche der Leiste: Ziehen verschiebt das Fenster, Doppelklick maximiert.
/// Muss vor den Knoepfen angelegt werden, damit diese Vorrang haben.
pub fn drag_area(ui: &mut egui::Ui, rect: Rect) {
    let resp = ui.interact(rect, ui.id().with("titlebar-drag"), Sense::click_and_drag());
    let ctx = ui.ctx();
    if resp.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Welche Kante oder Ecke liegt unter `p` (None = keine)?
fn edge_at(screen: Rect, p: Pos2) -> Option<ResizeDirection> {
    if !screen.contains(p) {
        return None;
    }
    let left = p.x - screen.left() < EDGE;
    let right = screen.right() - p.x < EDGE;
    let top = p.y - screen.top() < EDGE;
    let bottom = screen.bottom() - p.y < EDGE;
    let corner = EDGE * 2.5;
    let near_x = p.x - screen.left() < corner || screen.right() - p.x < corner;
    let near_y = p.y - screen.top() < corner || screen.bottom() - p.y < corner;
    Some(match (left, right, top, bottom) {
        (true, _, _, _) if near_y && (p.y - screen.top() < corner) => ResizeDirection::NorthWest,
        (true, _, _, _) if near_y => ResizeDirection::SouthWest,
        (_, true, _, _) if near_y && (p.y - screen.top() < corner) => ResizeDirection::NorthEast,
        (_, true, _, _) if near_y => ResizeDirection::SouthEast,
        (_, _, true, _) if near_x && (p.x - screen.left() < corner) => ResizeDirection::NorthWest,
        (_, _, true, _) if near_x => ResizeDirection::NorthEast,
        (_, _, _, true) if near_x && (p.x - screen.left() < corner) => ResizeDirection::SouthWest,
        (_, _, _, true) if near_x => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::West,
        (_, true, _, _) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

fn cursor_for(d: ResizeDirection) -> egui::CursorIcon {
    match d {
        ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
    }
}

/// Fensterrand: Groesse mit der Maus aendern und duenner Rahmen um das Fenster
pub fn frame_and_resize(ctx: &egui::Context) {
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    let screen = ctx.content_rect();
    if !maximized && !fullscreen {
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("window-border"));
        ctx.layer_painter(layer).rect_stroke(screen, 0.0, Stroke::new(1.0, style::pal().border), egui::StrokeKind::Inside);
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos())
            && let Some(dir) = edge_at(screen, p)
        {
            ctx.set_cursor_icon(cursor_for(dir));
            if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_initials() {
        assert_eq!(project_badge("shop").0, "SH");
        assert_eq!(project_badge("mein-projekt").0, "MP");
        assert_eq!(project_badge("").0, "?");
        assert_eq!(project_badge("Kasse 2").1, project_badge("Kasse 2").1);
    }

    #[test]
    fn edges_and_corners() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        assert_eq!(edge_at(r, pos2(400.0, 300.0)), None);
        assert_eq!(edge_at(r, pos2(2.0, 300.0)), Some(ResizeDirection::West));
        assert_eq!(edge_at(r, pos2(798.0, 300.0)), Some(ResizeDirection::East));
        assert_eq!(edge_at(r, pos2(400.0, 1.0)), Some(ResizeDirection::North));
        assert_eq!(edge_at(r, pos2(400.0, 599.0)), Some(ResizeDirection::South));
        assert_eq!(edge_at(r, pos2(2.0, 2.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(r, pos2(798.0, 2.0)), Some(ResizeDirection::NorthEast));
        assert_eq!(edge_at(r, pos2(2.0, 598.0)), Some(ResizeDirection::SouthWest));
        assert_eq!(edge_at(r, pos2(798.0, 598.0)), Some(ResizeDirection::SouthEast));
        assert_eq!(edge_at(r, pos2(-3.0, 10.0)), None);
    }
}
