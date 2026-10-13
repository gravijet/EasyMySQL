// Autoscroll mit der mittleren Maustaste.

use crate::style;
use eframe::egui::{self, Pos2, Rect};
use std::sync::Mutex;
use std::time::Instant;

/// Mittlere Maustaste gedrueckt halten und ziehen = schnell scrollen
#[derive(Default)]
pub(super) struct Autoscroll {
    anchor: Option<Pos2>,
    pos: Pos2,
    active: bool,
    last: Option<Instant>,
}

/// Bereiche, in denen die mittlere Maustaste eine eigene Bedeutung hat (Diagramm, Registerkarten)
static NO_AUTOSCROLL: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

pub fn block_autoscroll(r: Rect) {
    NO_AUTOSCROLL.lock().unwrap().push(r);
}

/// Vor jedem Bild: die gesperrten Bereiche des letzten Bildes verwerfen.
pub(super) fn begin_frame() {
    NO_AUTOSCROLL.lock().unwrap().clear();
}

impl Autoscroll {
    pub(super) fn hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let mut consumed = false;
        let blocked_rects = NO_AUTOSCROLL.lock().unwrap().clone();
        raw.events.retain(|e| match e {
            egui::Event::PointerMoved(p) => {
                self.pos = *p;
                true
            }
            egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, .. } => {
                if *pressed {
                    if blocked_rects.iter().any(|r| r.contains(*pos)) {
                        return true;
                    }
                    self.anchor = Some(*pos);
                    self.pos = *pos;
                    self.active = false;
                    consumed = true;
                    false
                } else if self.anchor.take().is_some() {
                    self.active = false;
                    consumed = true;
                    false
                } else {
                    true
                }
            }
            egui::Event::PointerGone | egui::Event::WindowFocused(false) => {
                self.anchor = None;
                self.active = false;
                true
            }
            _ => true,
        });
        if consumed {
            ctx.request_repaint();
        }
        let now = Instant::now();
        let dt = self.last.map(|l| now.duration_since(l).as_secs_f32()).unwrap_or(0.0).min(0.05);
        self.last = Some(now);
        let Some(a) = self.anchor else { return };
        let d = self.pos - a;
        if d.length() > 8.0 {
            self.active = true;
        }
        if self.active {
            // Geschwindigkeit waechst mit dem Abstand zum Ausgangspunkt
            let speed = |v: f32| v.signum() * ((v.abs() - 10.0).max(0.0) * 9.0).min(8000.0);
            let delta = egui::vec2(-speed(d.x), -speed(d.y)) * dt;
            if delta != egui::Vec2::ZERO {
                raw.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            ctx.request_repaint();
        }
    }

    pub(super) fn paint(&self, ctx: &egui::Context) {
        let (Some(a), true) = (self.anchor, self.active) else { return };
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("autoscroll")));
        let pal = style::pal();
        p.circle(a, 11.0, pal.face.gamma_multiply(0.9), egui::Stroke::new(1.0, pal.text_weak));
        p.circle_filled(a, 2.0, pal.text);
        for dir in [egui::vec2(0.0, -1.0), egui::vec2(0.0, 1.0), egui::vec2(-1.0, 0.0), egui::vec2(1.0, 0.0)] {
            let tip = a + dir * 8.0;
            let side = egui::vec2(dir.y, dir.x) * 3.0;
            p.add(egui::Shape::convex_polygon(vec![tip, tip - dir * 3.0 + side, tip - dir * 3.0 - side], pal.text, egui::Stroke::NONE));
        }
    }
}
