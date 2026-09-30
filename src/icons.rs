// Selbst gezeichnete Symbole. Sonderzeichen wie ▾ oder ✖ fehlen in manchen Windows-Schriften
// und erscheinen dann als leeres Viereck.

use crate::style;
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Files,
    Search,
    Database,
    History,
    Diagram,
    Wizard,
    Backup,
    Log,
    Help,
    Close,
    Plus,
    Refresh,
    NewFile,
    NewFolder,
    ChevronDown,
    ChevronRight,
    Play,
    More,
    Trash,
    Up,
    Down,
    Collapse,
}

fn arc(c: Pos2, r: f32, from: f32, to: f32, steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            c + vec2(r * a.cos(), r * a.sin())
        })
        .collect()
}

/// Zeichnet `icon` in `r` (Groesse wird an das Rechteck angepasst, Entwurf fuer 24 px).
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    let k = (r.width().min(r.height()) / 24.0).max(0.4);
    let s = Stroke::new((1.5 * k).max(1.0), color);
    let c = r.center();
    let v = |x: f32, y: f32| c + vec2(x * k, y * k);
    use std::f32::consts::PI;
    match icon {
        Icon::Files => {
            let back = Rect::from_min_max(v(-6.0, -9.0), v(5.0, 5.0));
            p.rect_stroke(back, 1.0, s, egui::StrokeKind::Middle);
            let front = Rect::from_min_max(v(-3.0, -5.0), v(8.0, 9.0));
            p.rect_filled(front, 1.0, style::pal().activity_bg);
            p.rect_stroke(front, 1.0, s, egui::StrokeKind::Middle);
        }
        Icon::Search => {
            p.circle_stroke(v(-2.0, -2.0), 6.0 * k, s);
            p.line_segment([v(2.5, 2.5), v(8.0, 8.0)], Stroke::new(s.width * 1.3, color));
        }
        Icon::Database => {
            let w = 8.0 * k;
            p.add(Shape::ellipse_stroke(v(0.0, -6.5), vec2(w, 2.8 * k), s));
            p.line_segment([v(-8.0, -6.5), v(-8.0, 6.5)], s);
            p.line_segment([v(8.0, -6.5), v(8.0, 6.5)], s);
            for y in [0.0, 6.5] {
                let pts: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let a = PI * i as f32 / 12.0;
                        v(-8.0 * a.cos(), y + 2.8 * a.sin())
                    })
                    .collect();
                p.add(Shape::line(pts, s));
            }
        }
        Icon::History => {
            p.add(Shape::line(arc(c, 8.0 * k, -PI * 0.75, PI * 1.25, 24), s));
            let tip = c + vec2(8.0 * k * (-PI * 0.75).cos(), 8.0 * k * (-PI * 0.75).sin());
            p.line_segment([tip, tip + vec2(0.0, 4.0 * k)], s);
            p.line_segment([tip, tip + vec2(4.0 * k, 0.0)], s);
            p.line_segment([c, v(0.0, -5.0)], s);
            p.line_segment([c, v(3.5, 2.0)], s);
        }
        Icon::Diagram => {
            let a = Rect::from_center_size(v(-5.5, -5.5), vec2(8.0, 6.0) * k);
            let b = Rect::from_center_size(v(5.5, -5.5), vec2(8.0, 6.0) * k);
            let d = Rect::from_center_size(v(0.0, 6.0), vec2(8.0, 6.0) * k);
            for x in [a, b, d] {
                p.rect_stroke(x, 1.0, s, egui::StrokeKind::Middle);
            }
            p.line_segment([a.center_bottom(), d.left_top()], s);
            p.line_segment([b.center_bottom(), d.right_top()], s);
        }
        Icon::Wizard => {
            let pts = vec![v(-8.0, -7.0), v(8.0, -7.0), v(2.0, 0.0), v(2.0, 7.0), v(-2.0, 5.0), v(-2.0, 0.0)];
            p.add(Shape::closed_line(pts, s));
        }
        Icon::Backup => {
            p.add(Shape::line(arc(c, 7.5 * k, PI * 0.35, PI * 1.95, 24), s));
            let tip = c + vec2(7.5 * k * (PI * 0.35).cos(), 7.5 * k * (PI * 0.35).sin());
            p.line_segment([tip, tip + vec2(-4.0 * k, 0.0)], s);
            p.line_segment([tip, tip + vec2(1.0 * k, -4.0 * k)], s);
            p.line_segment([c, v(0.0, -4.5)], s);
            p.line_segment([c, v(3.0, 2.0)], s);
        }
        Icon::Log => {
            let r2 = Rect::from_min_max(v(-7.0, -8.0), v(7.0, 8.0));
            p.rect_stroke(r2, 1.0, s, egui::StrokeKind::Middle);
            for (y, w) in [(-4.0, 8.0), (-1.0, 6.0), (2.0, 8.0), (5.0, 4.0)] {
                p.line_segment([v(-4.0, y), v(-4.0 + w, y)], s);
            }
        }
        Icon::Help => {
            p.circle_stroke(c, 8.5 * k, s);
            p.add(Shape::line(arc(v(0.0, -2.5), 3.0 * k, PI, PI * 2.4, 10), s));
            p.line_segment([v(0.9, 0.3), v(0.0, 2.0)], s);
            p.circle_filled(v(0.0, 5.0), 1.1 * k, color);
        }
        Icon::Close => {
            p.line_segment([v(-4.5, -4.5), v(4.5, 4.5)], s);
            p.line_segment([v(4.5, -4.5), v(-4.5, 4.5)], s);
        }
        Icon::Plus => {
            p.line_segment([v(-6.0, 0.0), v(6.0, 0.0)], s);
            p.line_segment([v(0.0, -6.0), v(0.0, 6.0)], s);
        }
        Icon::Refresh => {
            p.add(Shape::line(arc(c, 6.5 * k, -PI * 0.2, PI * 1.5, 20), s));
            let tip = c + vec2(6.5 * k * (-PI * 0.2).cos(), 6.5 * k * (-PI * 0.2).sin());
            p.line_segment([tip, tip + vec2(-4.0 * k, -0.5 * k)], s);
            p.line_segment([tip, tip + vec2(0.5 * k, -4.0 * k)], s);
        }
        Icon::NewFile => {
            p.add(Shape::closed_line(vec![v(-6.0, -8.0), v(2.0, -8.0), v(6.0, -4.0), v(6.0, 8.0), v(-6.0, 8.0)], s));
            p.line_segment([v(-3.0, 3.0), v(3.0, 3.0)], s);
            p.line_segment([v(0.0, 0.0), v(0.0, 6.0)], s);
        }
        Icon::NewFolder => {
            p.add(Shape::closed_line(vec![v(-8.0, -6.0), v(-2.0, -6.0), v(0.0, -4.0), v(8.0, -4.0), v(8.0, 7.0), v(-8.0, 7.0)], s));
            p.line_segment([v(-3.0, 1.5), v(3.0, 1.5)], s);
            p.line_segment([v(0.0, -1.5), v(0.0, 4.5)], s);
        }
        Icon::ChevronDown => {
            p.add(Shape::line(vec![v(-4.0, -2.0), v(0.0, 2.0), v(4.0, -2.0)], s));
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![v(-2.0, -4.0), v(2.0, 0.0), v(-2.0, 4.0)], s));
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![v(-5.0, -7.0), v(7.0, 0.0), v(-5.0, 7.0)], color, Stroke::NONE));
        }
        Icon::More => {
            for x in [-6.0, 0.0, 6.0] {
                p.circle_filled(v(x, 0.0), 1.5 * k, color);
            }
        }
        Icon::Trash => {
            p.line_segment([v(-7.0, -5.0), v(7.0, -5.0)], s);
            p.add(Shape::line(vec![v(-3.0, -5.0), v(-3.0, -8.0), v(3.0, -8.0), v(3.0, -5.0)], s));
            p.add(Shape::line(vec![v(-5.5, -5.0), v(-4.5, 8.0), v(4.5, 8.0), v(5.5, -5.0)], s));
        }
        Icon::Up => {
            p.add(Shape::line(vec![v(-5.0, 2.5), v(0.0, -2.5), v(5.0, 2.5)], s));
        }
        Icon::Down => {
            p.add(Shape::line(vec![v(-5.0, -2.5), v(0.0, 2.5), v(5.0, -2.5)], s));
        }
        Icon::Collapse => {
            p.rect_stroke(Rect::from_min_max(v(-7.0, -7.0), v(7.0, 7.0)), 1.0, s, egui::StrokeKind::Middle);
            p.line_segment([v(-3.5, 0.0), v(3.5, 0.0)], s);
        }
    }
}

/// Kleiner Knopf nur mit Symbol (ohne Rahmen, Hervorhebung bei Maus darueber).
pub fn button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> Response {
    button_sized(ui, icon, tip, 22.0)
}

pub fn button_sized(ui: &mut egui::Ui, icon: Icon, tip: &str, size: f32) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let pal = style::pal();
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, pal.hover);
    }
    let color = if ui.is_enabled() { if resp.hovered() { pal.text } else { pal.text_weak } } else { pal.text_weak.gamma_multiply(0.5) };
    paint(ui.painter(), r.shrink(size * 0.15), icon, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Knopf mit Symbol und Text
pub fn text_button(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    let pal = style::pal();
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), pal.text);
    let size = vec2(galley.size().x + 30.0, ui.spacing().interact_size.y.max(22.0));
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact(&resp);
    ui.painter().rect(r, 2.0, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
    let color = if ui.is_enabled() { visuals.fg_stroke.color } else { pal.text_weak };
    let ir = Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(14.0, 14.0));
    let icon_color = if icon == Icon::Play && ui.is_enabled() { pal.ok_text } else { color };
    paint(ui.painter(), ir, icon, icon_color);
    ui.painter().galley_with_override_text_color(pos2(r.left() + 24.0, r.center().y - galley.size().y / 2.0), galley, color);
    resp
}
