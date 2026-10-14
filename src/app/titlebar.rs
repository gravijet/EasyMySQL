// Eigene Titelleiste: Minimieren, Maximieren und Schliessen sitzen direkt in der Menueleiste
// (statt in einer zusaetzlichen Zeile des Betriebssystems). Dazu gehoeren Ziehen, Doppelklick
// und die Griffe an den Fensterkanten.

use crate::style;
use eframe::egui::{self, Color32, Pos2, Rect, ResizeDirection, Sense, Stroke, ViewportCommand, pos2, vec2};

/// Hoehe der Titelleiste
pub const HEIGHT: f32 = 44.0;
/// Breite eines Fensterknopfs
const BUTTON_W: f32 = 48.0;
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
