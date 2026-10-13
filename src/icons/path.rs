// Minimaler SVG-Pfadleser: wandelt "d"-Angaben in Linienzuege um (Kurven werden in Strecken zerlegt).

use eframe::egui::{Pos2, pos2};
use std::f32::consts::PI;

pub struct Poly {
    pub pts: Vec<Pos2>,
    pub closed: bool,
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        c.is_ascii_alphabetic().then(|| {
            self.i += 1;
            c
        })
    }

    fn has_number(&mut self) -> bool {
        self.skip();
        self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    /// Zahl; "1.5.5" sind zwei Zahlen (1.5 und .5), "-" beginnt eine neue Zahl.
    fn number(&mut self) -> f32 {
        self.skip();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        let mut dot = false;
        while let Some(&c) = self.s.get(self.i) {
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else if matches!(c, b'e' | b'E') {
                self.i += 1;
                if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
    }

    /// Bogen-Flag: genau ein Zeichen "0" oder "1" (kann direkt vor der naechsten Zahl stehen)
    fn flag(&mut self) -> bool {
        self.skip();
        let on = self.s.get(self.i) == Some(&b'1');
        self.i += 1;
        on
    }
}

fn cubic(out: &mut Vec<Pos2>, p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2) {
    const N: usize = 10;
    for k in 1..=N {
        let t = k as f32 / N as f32;
        let u = 1.0 - t;
        out.push(pos2(
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        ));
    }
}

fn quad(out: &mut Vec<Pos2>, p0: Pos2, p1: Pos2, p2: Pos2) {
    const N: usize = 8;
    for k in 1..=N {
        let t = k as f32 / N as f32;
        let u = 1.0 - t;
        out.push(pos2(u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x, u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y));
    }
}

/// Elliptischer Bogen nach SVG (Endpunkt-Darstellung, Anhang F.6)
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Vec<Pos2>, from: Pos2, mut rx: f32, mut ry: f32, rot_deg: f32, large: bool, sweep: bool, to: Pos2) {
    if from == to {
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-6 || ry < 1e-6 {
        out.push(to);
        return;
    }
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    let dx = (from.x - to.x) / 2.0;
    let dy = (from.y - to.y) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den == 0.0 { 0.0 } else { (num / den).max(0.0).sqrt() };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (from.x + to.x) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from.y + to.y) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let start = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }
    let steps = ((delta.abs() / (PI / 12.0)).ceil() as usize).max(2);
    for k in 1..=steps {
        let a = start + delta * k as f32 / steps as f32;
        let (sa, ca) = a.sin_cos();
        out.push(pos2(cos * rx * ca - sin * ry * sa + cx, sin * rx * ca + cos * ry * sa + cy));
    }
    // Endpunkt exakt treffen
    if let Some(last) = out.last_mut() {
        *last = to;
    }
}

pub fn parse(d: &str) -> Vec<Poly> {
    let mut r = Reader { s: d.as_bytes(), i: 0 };
    let mut polys: Vec<Poly> = Vec::new();
    let mut cur = pos2(0.0, 0.0);
    let mut start = cur;
    let mut ctrl: Option<Pos2> = None;
    let mut cmd = b' ';
    loop {
        if let Some(c) = r.command() {
            cmd = c;
            if matches!(cmd, b'Z' | b'z') {
                if let Some(p) = polys.last_mut() {
                    p.closed = true;
                }
                cur = start;
                ctrl = None;
                continue;
            }
        } else if !r.has_number() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { pos2(0.0, 0.0) };
        let upper = cmd.to_ascii_uppercase();
        let mut next_ctrl = None;
        match upper {
            b'M' => {
                let p = pos2(base.x + r.number(), base.y + r.number());
                polys.push(Poly { pts: vec![p], closed: false });
                cur = p;
                start = p;
                // weitere Zahlenpaare gelten als Linien
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' | b'H' | b'V' | b'C' | b'S' | b'Q' | b'T' | b'A' => {
                if polys.last().is_none_or(|p| p.closed) {
                    polys.push(Poly { pts: vec![cur], closed: false });
                }
                let poly = polys.last_mut().unwrap();
                match upper {
                    b'L' => {
                        cur = pos2(base.x + r.number(), base.y + r.number());
                        poly.pts.push(cur);
                    }
                    b'H' => {
                        cur = pos2(base.x + r.number(), cur.y);
                        poly.pts.push(cur);
                    }
                    b'V' => {
                        cur = pos2(cur.x, base.y + r.number());
                        poly.pts.push(cur);
                    }
                    b'C' => {
                        let p1 = pos2(base.x + r.number(), base.y + r.number());
                        let p2 = pos2(base.x + r.number(), base.y + r.number());
                        let p3 = pos2(base.x + r.number(), base.y + r.number());
                        cubic(&mut poly.pts, cur, p1, p2, p3);
                        next_ctrl = Some(p2);
                        cur = p3;
                    }
                    b'S' => {
                        let p1 = ctrl.map_or(cur, |c| pos2(2.0 * cur.x - c.x, 2.0 * cur.y - c.y));
                        let p2 = pos2(base.x + r.number(), base.y + r.number());
                        let p3 = pos2(base.x + r.number(), base.y + r.number());
                        cubic(&mut poly.pts, cur, p1, p2, p3);
                        next_ctrl = Some(p2);
                        cur = p3;
                    }
                    b'Q' => {
                        let p1 = pos2(base.x + r.number(), base.y + r.number());
                        let p2 = pos2(base.x + r.number(), base.y + r.number());
                        quad(&mut poly.pts, cur, p1, p2);
                        next_ctrl = Some(p1);
                        cur = p2;
                    }
                    b'T' => {
                        let p1 = ctrl.map_or(cur, |c| pos2(2.0 * cur.x - c.x, 2.0 * cur.y - c.y));
                        let p2 = pos2(base.x + r.number(), base.y + r.number());
                        quad(&mut poly.pts, cur, p1, p2);
                        next_ctrl = Some(p1);
                        cur = p2;
                    }
                    _ => {
                        let rx = r.number();
                        let ry = r.number();
                        let rot = r.number();
                        let large = r.flag();
                        let sweep = r.flag();
                        let to = pos2(base.x + r.number(), base.y + r.number());
                        arc(&mut poly.pts, cur, rx, ry, rot, large, sweep, to);
                        cur = to;
                    }
                }
            }
            _ => break,
        }
        ctrl = next_ctrl;
    }
    polys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_curve_and_arc() {
        let p = parse("M3 5V19A9 3 0 0 0 21 19V5");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].pts.first(), Some(&pos2(3.0, 5.0)));
        assert_eq!(p[0].pts.last(), Some(&pos2(21.0, 5.0)));
        // Der Bogen laeuft unten herum (y groesser als 19)
        assert!(p[0].pts.iter().any(|q| q.y > 21.0));
    }

    #[test]
    fn compact_numbers_and_flags() {
        // "0022" = Flags 0 und 0, dann 22; "1.704.706" = 1.704 und .706
        let p = parse("M20.001 19A2 2 0 0022 17M1.704.706l1 1");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].pts.last(), Some(&pos2(22.0, 17.0)));
        assert_eq!(p[1].pts[0], pos2(1.704, 0.706));
        assert_eq!(p[1].pts[1], pos2(2.704, 1.706));
    }

    #[test]
    fn closed_and_implicit_lines() {
        let p = parse("M0 0 10 0 10 10z");
        assert_eq!(p.len(), 1);
        assert!(p[0].closed);
        assert_eq!(p[0].pts.len(), 3);
    }
}
