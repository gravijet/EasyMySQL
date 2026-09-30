// Automatische Anordnung fuer das ER-Diagramm.
//
// Die Tabellen werden auf ein Raster gesetzt und so lange vertauscht/verschoben (simulierte
// Abkuehlung), bis moeglichst wenige Linien sich kreuzen oder durch fremde Tabellen laufen,
// verbundene Tabellen nah beieinander liegen und das Ganze etwa Bildschirmformat hat.
// Tabellen ohne Beziehungen kommen gesammelt darunter.

use eframe::egui::{Pos2, Vec2, pos2};
use std::collections::HashMap;

/// Abstand zwischen Spalten bzw. Zeilen des Rasters (Platz fuer Linien und Beschriftungen)
const GAP_X: f32 = 110.0;
const GAP_Y: f32 = 56.0;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Strecken p1-p2 und p3-p4 schneiden sich echt (gemeinsame Endpunkte zaehlen nicht)
fn crosses(p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), p4: (f32, f32)) -> bool {
    let d = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let d1 = d(p3, p4, p1);
    let d2 = d(p3, p4, p2);
    let d3 = d(p1, p2, p3);
    let d4 = d(p1, p2, p4);
    ((d1 > 1e-6 && d2 < -1e-6) || (d1 < -1e-6 && d2 > 1e-6)) && ((d3 > 1e-6 && d4 < -1e-6) || (d3 < -1e-6 && d4 > 1e-6))
}

/// Laeuft die Strecke a-b durch das Quadrat um c (Halbbreite h)?
fn through(a: (f32, f32), b: (f32, f32), c: (f32, f32), h: f32) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-6 {
        return false;
    }
    let t = (((c.0 - a.0) * dx + (c.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    let (px, py) = (a.0 + dx * t, a.1 + dy * t);
    (px - c.0).abs() < h && (py - c.1).abs() < h && t > 0.02 && t < 0.98
}

struct Grid<'a> {
    cols: usize,
    rows: usize,
    cell: Vec<(usize, usize)>,
    occ: HashMap<(usize, usize), usize>,
    edges: &'a [(usize, usize)],
    adj: Vec<Vec<usize>>,
    /// Hoehe einer Rasterzeile relativ zur Spaltenbreite (Tabellen sind oft hoeher als breit)
    ry: f32,
}

impl Grid<'_> {
    fn pt(&self, n: usize) -> (f32, f32) {
        let (c, r) = self.cell[n];
        (c as f32, r as f32 * self.ry)
    }

    /// Kosten einer Kante (ohne Kreuzungen)
    fn edge_cost(&self, e: usize) -> f32 {
        let (a, b) = self.edges[e];
        let (pa, pb) = (self.pt(a), self.pt(b));
        let len = ((pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2)).sqrt();
        let mut c = len * 2.0 + (len - 1.5).max(0.0).powi(2) * 3.0;
        // untereinander: Linie muss aussen herum laufen
        if self.cell[a].0 == self.cell[b].0 {
            c += 1.5;
        }
        c
    }

    /// Alle Kostenanteile, an denen die Knoten in `set` beteiligt sind.
    fn local(&self, set: &[usize]) -> f32 {
        let touches = |e: usize| set.contains(&self.edges[e].0) || set.contains(&self.edges[e].1);
        let mut cost = 0.0;
        let mut mine = Vec::new();
        for (e, _) in self.edges.iter().enumerate() {
            if touches(e) {
                mine.push(e);
                cost += self.edge_cost(e);
            }
        }
        // Kreuzungen: Paare mit mindestens einer betroffenen Kante (jedes Paar einmal)
        for (k, &e) in mine.iter().enumerate() {
            let (a, b) = self.edges[e];
            let (pa, pb) = (self.pt(a), self.pt(b));
            for (f, &(c, d)) in self.edges.iter().enumerate() {
                if f == e || (touches(f) && mine[..k].contains(&f)) {
                    continue;
                }
                if a == c || a == d || b == c || b == d {
                    continue;
                }
                if crosses(pa, pb, self.pt(c), self.pt(d)) {
                    cost += 30.0;
                }
            }
            // durch fremde Tabellen
            for n in 0..self.cell.len() {
                if n != a && n != b && through(pa, pb, self.pt(n), 0.38) {
                    cost += 20.0;
                }
            }
        }
        // fremde Kanten durch die betroffenen Tabellen
        for &n in set {
            for (e, &(a, b)) in self.edges.iter().enumerate() {
                if !touches(e) && through(self.pt(a), self.pt(b), self.pt(n), 0.38) {
                    cost += 20.0;
                }
            }
        }
        cost
    }

    /// Flaeche und Seitenverhaeltnis (Ziel etwa 16:10)
    fn shape_cost(&self) -> f32 {
        let (mut c0, mut c1, mut r0, mut r1) = (usize::MAX, 0, usize::MAX, 0);
        for &(c, r) in &self.cell {
            c0 = c0.min(c);
            c1 = c1.max(c);
            r0 = r0.min(r);
            r1 = r1.max(r);
        }
        let w = (c1 - c0 + 1) as f32;
        let h = (r1 - r0 + 1) as f32 * self.ry;
        let ratio = w / h.max(0.1);
        w * h * 0.6 + (ratio.ln() - 1.6f32.ln()).powi(2) * 6.0 * self.cell.len() as f32
    }

    fn total(&self) -> f32 {
        let all: Vec<usize> = (0..self.cell.len()).collect();
        self.local(&all) + self.shape_cost()
    }

    fn place(&mut self, n: usize, cell: (usize, usize)) {
        self.occ.remove(&self.cell[n]);
        self.cell[n] = cell;
        self.occ.insert(cell, n);
    }

    /// Anfangsbelegung: Breitensuche ab der Tabelle mit den meisten Beziehungen, jede Tabelle
    /// auf den freien Platz, der ihren schon gesetzten Nachbarn am naechsten liegt.
    fn initial(&mut self, rng: &mut Rng) {
        let n = self.cell.len();
        let mut order: Vec<usize> = Vec::new();
        let mut seen = vec![false; n];
        let mut starts: Vec<usize> = (0..n).collect();
        starts.sort_by_key(|&i| std::cmp::Reverse(self.adj[i].len() * 10 + rng.below(3)));
        for s in starts {
            if seen[s] {
                continue;
            }
            seen[s] = true;
            let mut queue = std::collections::VecDeque::from([s]);
            while let Some(v) = queue.pop_front() {
                order.push(v);
                let mut nb = self.adj[v].clone();
                nb.sort_by_key(|&x| std::cmp::Reverse(self.adj[x].len()));
                for x in nb {
                    if !seen[x] {
                        seen[x] = true;
                        queue.push_back(x);
                    }
                }
            }
        }
        self.occ.clear();
        let center = (self.cols as f32 / 2.0, self.rows as f32 / 2.0);
        let mut placed = vec![false; n];
        for v in order {
            let mut best = (f32::MAX, (0, 0));
            for c in 0..self.cols {
                for r in 0..self.rows {
                    if self.occ.contains_key(&(c, r)) {
                        continue;
                    }
                    let mut s = 0.0;
                    let mut k = 0;
                    for &x in &self.adj[v] {
                        if placed[x] {
                            let (xc, xr) = self.cell[x];
                            s += (xc as f32 - c as f32).abs() + (xr as f32 - r as f32).abs() * 1.3;
                            k += 1;
                        }
                    }
                    if k == 0 {
                        s = (c as f32 - center.0).abs() + (r as f32 - center.1).abs();
                    }
                    if s < best.0 {
                        best = (s, (c, r));
                    }
                }
            }
            self.cell[v] = best.1;
            self.occ.insert(best.1, v);
            placed[v] = true;
        }
    }

    /// Simulierte Abkuehlung: Tabelle auf anderes Feld setzen (ggf. tauschen)
    fn anneal(&mut self, rng: &mut Rng, steps: usize) {
        let n = self.cell.len();
        let mut temp = 6.0f32;
        let cool = (0.02f32 / temp).powf(1.0 / steps.max(1) as f32);
        let mut shape = self.shape_cost();
        for _ in 0..steps {
            temp *= cool;
            let v = rng.below(n);
            // meist in die Naehe, manchmal irgendwohin
            let target = if rng.below(4) == 0 {
                (rng.below(self.cols), rng.below(self.rows))
            } else {
                let (c, r) = self.cell[v];
                let dc = rng.below(5) as isize - 2;
                let dr = rng.below(5) as isize - 2;
                ((c as isize + dc).clamp(0, self.cols as isize - 1) as usize, (r as isize + dr).clamp(0, self.rows as isize - 1) as usize)
            };
            if target == self.cell[v] {
                continue;
            }
            let other = self.occ.get(&target).copied();
            let set: Vec<usize> = match other {
                Some(o) => vec![v, o],
                None => vec![v],
            };
            let before = self.local(&set) + shape;
            let old = self.cell[v];
            match other {
                Some(o) => {
                    self.cell[v] = target;
                    self.cell[o] = old;
                    self.occ.insert(target, v);
                    self.occ.insert(old, o);
                }
                None => self.place(v, target),
            }
            let new_shape = self.shape_cost();
            let after = self.local(&set) + new_shape;
            let delta = after - before;
            if delta <= 0.0 || rng.unit() < (-delta / temp).exp() {
                shape = new_shape;
            } else {
                // rueckgaengig
                match other {
                    Some(o) => {
                        self.cell[v] = old;
                        self.cell[o] = target;
                        self.occ.insert(old, v);
                        self.occ.insert(target, o);
                    }
                    None => self.place(v, old),
                }
            }
        }
    }
}

/// Anordnung berechnen. `sizes`: Name und Groesse jeder Tabelle, `edges`: Beziehungen (Namen).
/// Liefert die linke obere Ecke jeder Tabelle.
pub fn layout(sizes: &[(String, Vec2)], edges: &[(String, String)]) -> HashMap<String, Pos2> {
    let idx: HashMap<&str, usize> = sizes.iter().enumerate().map(|(i, (n, _))| (n.as_str(), i)).collect();
    let mut e_all: Vec<(usize, usize)> = edges
        .iter()
        .filter_map(|(a, b)| Some((*idx.get(a.as_str())?, *idx.get(b.as_str())?)))
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect();
    e_all.sort();
    e_all.dedup();
    let connected: Vec<usize> = (0..sizes.len()).filter(|i| e_all.iter().any(|(a, b)| a == i || b == i)).collect();
    let local_of: HashMap<usize, usize> = connected.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let edges_l: Vec<(usize, usize)> = e_all.iter().map(|(a, b)| (local_of[a], local_of[b])).collect();
    let n = connected.len();
    let mut out = HashMap::new();
    let mut bottom = 0.0f32;
    let mut right = 0.0f32;

    if n > 0 {
        // Rasterzeilen im Verhaeltnis zur typischen Tabellenhoehe
        let avg_w = connected.iter().map(|&i| sizes[i].1.x).sum::<f32>() / n as f32 + GAP_X;
        let avg_h = connected.iter().map(|&i| sizes[i].1.y).sum::<f32>() / n as f32 + GAP_Y;
        let ry = (avg_h / avg_w).clamp(0.5, 2.5);
        let cols = ((n as f32 * 1.6 / ry).sqrt().ceil() as usize + 1).max(2);
        let rows = (n.div_ceil(cols) + 2).max(2);
        let mut adj = vec![Vec::new(); n];
        for &(a, b) in &edges_l {
            adj[a].push(b);
            adj[b].push(a);
        }
        let steps = (n * 900).clamp(2000, 60_000);
        let mut best: Option<(f32, Vec<(usize, usize)>)> = None;
        for seed in 1..=if n > 40 { 1u64 } else { 3 } {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
            let mut g = Grid { cols, rows, cell: vec![(0, 0); n], occ: HashMap::new(), edges: &edges_l, adj: adj.clone(), ry };
            g.initial(&mut rng);
            g.anneal(&mut rng, steps);
            let cost = g.total();
            if best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, g.cell.clone()));
            }
        }
        let cells = best.map(|b| b.1).unwrap_or_default();
        // leere Zeilen/Spalten entfernen
        let mut used_c: Vec<usize> = cells.iter().map(|c| c.0).collect();
        let mut used_r: Vec<usize> = cells.iter().map(|c| c.1).collect();
        used_c.sort();
        used_c.dedup();
        used_r.sort();
        used_r.dedup();
        let col_w: Vec<f32> = used_c
            .iter()
            .map(|&c| cells.iter().enumerate().filter(|(_, x)| x.0 == c).map(|(k, _)| sizes[connected[k]].1.x).fold(0.0, f32::max))
            .collect();
        let row_h: Vec<f32> = used_r
            .iter()
            .map(|&r| cells.iter().enumerate().filter(|(_, x)| x.1 == r).map(|(k, _)| sizes[connected[k]].1.y).fold(0.0, f32::max))
            .collect();
        let col_x: Vec<f32> = col_w.iter().scan(0.0, |x, w| {
            let v = *x;
            *x += w + GAP_X;
            Some(v)
        }).collect();
        let row_y: Vec<f32> = row_h.iter().scan(0.0, |y, h| {
            let v = *y;
            *y += h + GAP_Y;
            Some(v)
        }).collect();
        for (k, &(c, r)) in cells.iter().enumerate() {
            let ci = used_c.binary_search(&c).unwrap_or(0);
            let ri = used_r.binary_search(&r).unwrap_or(0);
            let s = sizes[connected[k]].1;
            // in der Zelle waagerecht mittig, senkrecht oben
            let x = col_x[ci] + (col_w[ci] - s.x) / 2.0;
            let y = row_y[ri] + ((row_h[ri] - s.y) / 2.0).min(20.0);
            out.insert(sizes[connected[k]].0.clone(), pos2(x, y));
            bottom = bottom.max(y + s.y);
            right = right.max(x + s.x);
        }
    }

    // Tabellen ohne Beziehungen: Raster darunter, alphabetisch
    let mut rest: Vec<usize> = (0..sizes.len()).filter(|i| !local_of.contains_key(i)).collect();
    rest.sort_by_key(|&i| sizes[i].0.to_lowercase());
    if !rest.is_empty() {
        let width = if n > 0 { right.max(700.0) } else { (rest.len() as f32).sqrt().ceil() * 260.0 };
        let (mut x, mut y, mut line_h) = (0.0f32, if n > 0 { bottom + GAP_Y * 1.6 } else { 0.0 }, 0.0f32);
        for i in rest {
            let s = sizes[i].1;
            if x > 0.0 && x + s.x > width {
                x = 0.0;
                y += line_h + GAP_Y * 0.8;
                line_h = 0.0;
            }
            out.insert(sizes[i].0.clone(), pos2(x, y));
            x += s.x + 50.0;
            line_h = line_h.max(s.y);
        }
    }
    out
}

/// Anzahl der Kreuzungen gerader Verbindungen zwischen den Tabellenmitten (fuer Tests)
#[cfg(test)]
pub fn crossings(pos: &HashMap<String, Pos2>, sizes: &[(String, Vec2)], edges: &[(String, String)]) -> usize {
    let center = |n: &str| {
        let s = sizes.iter().find(|x| x.0 == n).unwrap().1;
        let p = pos[n] + s / 2.0;
        (p.x, p.y)
    };
    let mut k = 0;
    for (i, (a, b)) in edges.iter().enumerate() {
        for (c, d) in &edges[i + 1..] {
            if a == c || a == d || b == c || b == d || a == b || c == d {
                continue;
            }
            if crosses(center(a), center(b), center(c), center(d)) {
                k += 1;
            }
        }
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Rect, vec2};

    fn shop() -> (Vec<(String, Vec2)>, Vec<(String, String)>) {
        let t = |n: &str, cols: usize| (n.to_string(), vec2(170.0, 28.0 + 17.0 * cols as f32));
        let sizes = vec![
            t("kunde", 5),
            t("ort", 3),
            t("kategorie", 3),
            t("artikel", 5),
            t("hersteller", 3),
            t("bestellung", 4),
            t("position", 3),
            t("lieferung", 3),
            t("bewertung", 4),
            t("lager", 2),
            t("bestand", 3),
            t("gutschein", 2),
            t("newsletter", 1),
        ];
        let e = |a: &str, b: &str| (a.to_string(), b.to_string());
        let edges = vec![
            e("kunde", "ort"),
            e("artikel", "kategorie"),
            e("artikel", "hersteller"),
            e("hersteller", "ort"),
            e("bestellung", "kunde"),
            e("position", "bestellung"),
            e("position", "artikel"),
            e("lieferung", "bestellung"),
            e("bewertung", "kunde"),
            e("bewertung", "artikel"),
            e("lager", "ort"),
            e("bestand", "lager"),
            e("bestand", "artikel"),
        ];
        (sizes, edges)
    }

    #[test]
    fn shop_layout_is_clear() {
        let (sizes, edges) = shop();
        let pos = layout(&sizes, &edges);
        assert_eq!(pos.len(), sizes.len());
        let rects: Vec<Rect> = sizes.iter().map(|(n, s)| Rect::from_min_size(pos[n], *s)).collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(!rects[i].expand(10.0).intersects(rects[j]), "{} / {}", sizes[i].0, sizes[j].0);
            }
        }
        assert!(crossings(&pos, &sizes, &edges) <= 1, "Kreuzungen: {}", crossings(&pos, &sizes, &edges));
        // nicht alles in einer Reihe: etwa Bildschirmformat
        let all = rects.iter().fold(rects[0], |a, b| a.union(*b));
        let ratio = all.width() / all.height();
        assert!((0.8..=3.2).contains(&ratio), "Seitenverhaeltnis {ratio}");
        // Tabellen ohne Beziehung liegen unter den anderen
        let connected_bottom = sizes.iter().take(11).map(|(n, s)| pos[n].y + s.y).fold(0.0, f32::max);
        assert!(pos["gutschein"].y > connected_bottom && pos["newsletter"].y > connected_bottom);
    }

    #[test]
    fn deterministic_and_small_cases() {
        let (sizes, edges) = shop();
        assert_eq!(layout(&sizes, &edges), layout(&sizes, &edges));
        let one = vec![("a".to_string(), vec2(100.0, 50.0))];
        assert_eq!(layout(&one, &[]).len(), 1);
        let two = vec![("a".to_string(), vec2(100.0, 50.0)), ("b".to_string(), vec2(100.0, 50.0))];
        let p = layout(&two, &[("a".into(), "b".into()), ("a".into(), "a".into())]);
        assert_eq!(p.len(), 2);
        assert!(layout(&[], &[]).is_empty());
    }

    #[test]
    fn star_is_not_a_single_row() {
        // Eine zentrale Tabelle mit 12 abhaengigen: frueher alles untereinander
        let mut sizes = vec![("mitte".to_string(), vec2(150.0, 80.0))];
        let mut edges = Vec::new();
        for i in 0..12 {
            sizes.push((format!("t{i}"), vec2(150.0, 80.0)));
            edges.push((format!("t{i}"), "mitte".to_string()));
        }
        let pos = layout(&sizes, &edges);
        let xs: std::collections::BTreeSet<i32> = pos.values().map(|p| p.x as i32).collect();
        let ys: std::collections::BTreeSet<i32> = pos.values().map(|p| p.y as i32).collect();
        assert!(xs.len() >= 3 && ys.len() >= 3, "{xs:?} {ys:?}");
        assert_eq!(crossings(&pos, &sizes, &edges), 0);
    }
}
