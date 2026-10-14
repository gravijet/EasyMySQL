// Rechtwinklige Linienfuehrung fuer das ER-Diagramm.
//
// Jede Beziehung besteht aus waagerechten Staeben an den Tabellen und senkrechten Teilen in den
// freien Streifen zwischen den Tabellenspalten ("Fahrspuren"). Beziehungen vom selben Anschluss
// (z. B. alle Verweise auf kunde.id) teilen sich einen Strang wie eine Sammelschiene. Innerhalb
// eines Streifens bekommt jeder Strang eine eigene Spur; die Reihenfolge der Spuren wird so
// gewaehlt, dass sich moeglichst wenige Linien kreuzen.

use eframe::egui::{Pos2, Rect, pos2};
use std::collections::HashMap;

/// Abstand zweier Spuren
pub const TRACK: f32 = 8.0;
/// Freier Rand neben den Tabellen, in dem keine Spur liegt
pub const MARGIN: f32 = 14.0;

/// Ein Strang in einem Streifen: die Hoehen, an denen waagerechte Linien links bzw. rechts
/// anschliessen. Die senkrechte Linie reicht vom kleinsten bis zum groessten Wert.
#[derive(Clone, Debug, Default)]
pub struct Net {
    pub lefts: Vec<f32>,
    pub rights: Vec<f32>,
}

impl Net {
    fn span(&self) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &y in self.lefts.iter().chain(&self.rights) {
            lo = lo.min(y);
            hi = hi.max(y);
        }
        (lo, hi)
    }
}

fn inside(y: f32, span: (f32, f32)) -> bool {
    y > span.0 + 0.01 && y < span.1 - 0.01
}

/// Kreuzungen, wenn `a` links von `b` liegt: rechte Anschluesse von a laufen ueber b, linke von b ueber a
fn cost_left_of(a: &Net, sa: (f32, f32), b: &Net, sb: (f32, f32)) -> usize {
    a.rights.iter().filter(|&&y| inside(y, sb)).count() + b.lefts.iter().filter(|&&y| inside(y, sa)).count()
}

/// Spuren verteilen: Straenge mit ueberlappender Hoehe bekommen verschiedene Spuren, die anderen
/// teilen sich eine. Liefert die Spur (0 = ganz links) je Strang und die Zahl der Spuren.
pub fn assign_tracks(nets: &[Net]) -> (Vec<usize>, usize) {
    let n = nets.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    let spans: Vec<(f32, f32)> = nets.iter().map(Net::span).collect();
    let overlap = |i: usize, j: usize| spans[i].0 <= spans[j].1 + 0.01 && spans[j].0 <= spans[i].1 + 0.01;
    let mean = |v: &[f32]| if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
    // Anfangsreihenfolge: nach unten fuehrende Straenge zuerst (der tiefer beginnende links), dann nach oben fuehrende
    let key = |i: usize| -> (u8, f32) {
        let (l, r) = (mean(&nets[i].lefts), mean(&nets[i].rights));
        if r >= l { (0, -l) } else { (1, l) }
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(a.cmp(&b))
    });
    if n <= 160 {
        // Benachbarte Straenge tauschen, solange das Kreuzungen spart (nur bei ueberlappender Hoehe)
        for _ in 0..12 {
            let mut changed = false;
            for p in 0..n - 1 {
                let (a, b) = (order[p], order[p + 1]);
                if overlap(a, b) && cost_left_of(&nets[b], spans[b], &nets[a], spans[a]) < cost_left_of(&nets[a], spans[a], &nets[b], spans[b]) {
                    order.swap(p, p + 1);
                    changed = true;
                }
            }
            // auch uebernaechste Nachbarn, falls dazwischen ein nicht ueberlappender Strang liegt
            for p in 0..n.saturating_sub(2) {
                let (a, b, c) = (order[p], order[p + 1], order[p + 2]);
                if overlap(a, c) && !overlap(a, b) && !overlap(b, c) && cost_left_of(&nets[c], spans[c], &nets[a], spans[a]) < cost_left_of(&nets[a], spans[a], &nets[c], spans[c]) {
                    order.swap(p, p + 2);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    // Spuren: rechts von allen frueheren Straengen mit ueberlappender Hoehe
    let mut track = vec![0usize; n];
    let mut count = 0;
    if n <= 4000 {
        for p in 0..n {
            let i = order[p];
            let mut t = 0;
            for &j in &order[..p] {
                if overlap(i, j) {
                    t = t.max(track[j] + 1);
                }
            }
            track[i] = t;
            count = count.max(t + 1);
        }
    } else {
        // Sehr viele Straenge: einfache Verteilung nach Anfangshoehe (Intervallfaerbung)
        let mut by_start: Vec<usize> = (0..n).collect();
        by_start.sort_by(|&a, &b| spans[a].0.total_cmp(&spans[b].0));
        let mut ends: Vec<f32> = Vec::new();
        for i in by_start {
            let t = ends.iter().position(|&e| e < spans[i].0 - 0.01).unwrap_or_else(|| {
                ends.push(f32::MIN);
                ends.len() - 1
            });
            ends[t] = spans[i].1;
            track[i] = t;
        }
        count = ends.len();
    }
    (track, count)
}

/// Ein Stueck einer Linie in einem Streifen: links in Hoehe `ly` hinein, rechts in Hoehe `ry` hinaus.
/// Gleicher `key` = gleicher Anschluss: die Stuecke teilen sich einen Strang.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub ly: f32,
    pub ry: f32,
    pub key: Option<u64>,
}

/// Straenge aus den Stuecken eines Streifens bilden. Liefert die Straenge und den Strang je Stueck.
pub fn nets_of(segs: &[Seg]) -> (Vec<Net>, Vec<usize>) {
    let mut nets: Vec<Net> = Vec::new();
    let mut by_key: HashMap<u64, usize> = HashMap::new();
    let mut of = Vec::with_capacity(segs.len());
    for s in segs {
        let idx = match s.key {
            Some(k) => *by_key.entry(k).or_insert_with(|| {
                nets.push(Net::default());
                nets.len() - 1
            }),
            None => {
                nets.push(Net::default());
                nets.len() - 1
            }
        };
        let net = &mut nets[idx];
        if !net.lefts.iter().any(|&y| (y - s.ly).abs() < 0.01) {
            net.lefts.push(s.ly);
        }
        if !net.rights.iter().any(|&y| (y - s.ry).abs() < 0.01) {
            net.rights.push(s.ry);
        }
        of.push(idx);
    }
    (nets, of)
}

/// Zahl der Spuren fuer die Stuecke eines Streifens
pub fn tracks_needed(segs: &[Seg]) -> usize {
    let (nets, _) = nets_of(segs);
    assign_tracks(&nets).1
}

/// Breite, die ein Streifen fuer `tracks` Spuren braucht (inklusive Rand und Platz fuer Beschriftungen)
pub fn gap_width(tracks: usize) -> f32 {
    (2.0 * MARGIN + 44.0 + tracks as f32 * TRACK).max(96.0)
}

// ---------------------------------------------------------------------------
// Linienfuehrung

/// Eine Beziehung: Kind (hat den Fremdschluessel) und Elternteil mit der Hoehe ihres Anschlusses
/// und den Hoehen der Fahrspuren (vom Kind zum Elternteil), wenn die Linie Spalten ueberspringt.
#[derive(Clone, Debug)]
pub struct EdgeIn {
    pub child: usize,
    pub parent: usize,
    pub child_y: f32,
    pub parent_y: f32,
    pub lanes: Vec<f32>,
}

/// Verlauf einer Beziehung vom Kind zum Elternteil
#[derive(Clone, Debug, Default)]
pub struct Routed {
    pub points: Vec<Pos2>,
}

#[derive(Clone, Copy)]
struct Strip {
    lo: f32,
    hi: f32,
}

/// Freie Streifen zwischen den (um den Rand erweiterten) Tabellen, von links nach rechts,
/// dazu ein Streifen links und einer rechts von allen.
fn strips_of(rects: &[Rect], members: &[usize]) -> Vec<Strip> {
    let mut iv: Vec<(f32, f32)> = members.iter().map(|&i| (rects[i].left() - MARGIN, rects[i].right() + MARGIN)).collect();
    iv.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (a, b) in iv {
        match merged.last_mut() {
            Some(m) if a <= m.1 => m.1 = m.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut out = Vec::new();
    if let Some(first) = merged.first() {
        out.push(Strip { lo: first.0 - 10.0 * TRACK - 2.0 * MARGIN, hi: first.0 });
    }
    for w in merged.windows(2) {
        out.push(Strip { lo: w[0].1, hi: w[1].0 });
    }
    if let Some(last) = merged.last() {
        out.push(Strip { lo: last.1, hi: last.1 + 10.0 * TRACK + 2.0 * MARGIN });
    }
    out
}

/// Stueck einer Linie: Strang (Streifen und Anschluss), Hoehen links und rechts
struct Piece {
    edge: usize,
    strip: usize,
    seg: Seg,
}

/// Schluessel eines Anschlusses: Tabelle, Hoehe und Seite
pub fn port_key(table: usize, y: f32, right_side: bool) -> u64 {
    ((table as u64) << 40) ^ ((y.to_bits() as u64) << 1) ^ right_side as u64
}

fn dedup(points: &mut Vec<Pos2>) {
    points.dedup_by(|b, a| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    // Punkte auf einer Geraden entfernen
    let mut i = 1;
    while i + 1 < points.len() {
        let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
        let same_x = (a.x - b.x).abs() < 0.01 && (b.x - c.x).abs() < 0.01;
        let same_y = (a.y - b.y).abs() < 0.01 && (b.y - c.y).abs() < 0.01;
        if same_x || same_y {
            points.remove(i);
        } else {
            i += 1;
        }
    }
}

/// Alle Beziehungen rechtwinklig fuehren. `rects`: Tabellen in Weltkoordinaten.
pub fn route(rects: &[Rect], edges: &[EdgeIn]) -> Vec<Routed> {
    let mut out = vec![Routed::default(); edges.len()];
    // Gruppen: zusammenhaengende Tabellen (nur diese bestimmen die freien Streifen)
    let mut uf: Vec<usize> = (0..rects.len()).collect();
    fn find(uf: &mut [usize], mut x: usize) -> usize {
        while uf[x] != x {
            uf[x] = uf[uf[x]];
            x = uf[x];
        }
        x
    }
    for e in edges {
        if e.child < rects.len() && e.parent < rects.len() {
            let (a, b) = (find(&mut uf, e.child), find(&mut uf, e.parent));
            if a != b {
                uf[a] = b;
            }
        }
    }
    let mut comp_edges: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        if e.child < rects.len() && e.parent < rects.len() {
            let r = find(&mut uf, e.child);
            comp_edges.entry(r).or_default().push(i);
        }
    }
    let mut roots: Vec<usize> = comp_edges.keys().copied().collect();
    roots.sort_unstable();
    for root in roots {
        let ids = &comp_edges[&root];
        let mut members: Vec<usize> = ids.iter().flat_map(|&i| [edges[i].child, edges[i].parent]).collect();
        members.sort_unstable();
        members.dedup();
        route_group(rects, edges, ids, &members, &mut out);
    }
    out
}

fn route_group(rects: &[Rect], edges: &[EdgeIn], ids: &[usize], members: &[usize], out: &mut [Routed]) {
    let strips = strips_of(rects, members);
    // Fuer jede Beziehung: Tabelle links (x), Tabelle rechts (y), Hoehen von links nach rechts
    struct Plan {
        left: usize,
        right: usize,
        ys: Vec<f32>,
        strips: Vec<usize>,
        /// Elternteil liegt links (sonst rechts)
        parent_left: bool,
    }
    let mut plans: HashMap<usize, Plan> = HashMap::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut self_loops: Vec<usize> = Vec::new();
    let mut c_edges: Vec<usize> = Vec::new();
    let right_strip = strips.len() - 1;
    for &i in ids {
        let e = &edges[i];
        if e.child == e.parent {
            self_loops.push(i);
            continue;
        }
        let (rc, rp) = (rects[e.child], rects[e.parent]);
        let parent_left = rp.center().x < rc.center().x;
        let (left, right) = if parent_left { (e.parent, e.child) } else { (e.child, e.parent) };
        let (ly, ry) = if parent_left { (e.parent_y, e.child_y) } else { (e.child_y, e.parent_y) };
        let (xl, xr) = (rects[left].right(), rects[right].left());
        // Streifen zwischen den beiden Tabellen
        let between: Vec<usize> = (0..strips.len()).filter(|&s| strips[s].lo >= xl - 0.01 && strips[s].hi <= xr + 0.01).collect();
        if between.is_empty() {
            c_edges.push(i);
            continue;
        }
        // Fahrspuren (Hoehen) von links nach rechts; sie gelten nur, wenn zu jeder ein Streifen passt und
        // die waagerechte Strecke durch die Spalte frei von Tabellen ist
        let mut lane_ys: Vec<f32> = if parent_left { e.lanes.iter().rev().copied().collect() } else { e.lanes.clone() };
        let valid = lane_ys.len() < between.len()
            && lane_ys.iter().enumerate().all(|(k, &y)| {
                let (a, b) = (strips[between[k]].hi, strips[between[k + 1]].lo);
                !members.iter().any(|&m| rects[m].min.x < b && rects[m].max.x > a && rects[m].min.y - 1.0 < y && rects[m].max.y + 1.0 > y)
            });
        if !valid {
            lane_ys.clear();
        }
        let mut ys = vec![ly];
        ys.extend(&lane_ys);
        ys.push(ry);
        // Segment k liegt im k-ten Streifen; ohne Fahrspuren im ersten (am Elternteil, wenn er links liegt)
        let used: Vec<usize> = if lane_ys.is_empty() { vec![if parent_left { between[0] } else { *between.last().unwrap() }] } else { between[..ys.len() - 1].to_vec() };
        for k in 0..ys.len() - 1 {
            let key = if k == 0 && parent_left {
                Some(port_key(left, ys[0], true))
            } else if k == ys.len() - 2 && !parent_left {
                Some(port_key(right, ys[k + 1], false))
            } else {
                None
            };
            pieces.push(Piece { edge: i, strip: used[k], seg: Seg { ly: ys[k], ry: ys[k + 1], key } });
        }
        plans.insert(i, Plan { left, right, ys, strips: used, parent_left });
    }
    // Umwege (Tabellen ohne Streifen dazwischen): ueber den Streifen rechts von allen
    for &i in &c_edges {
        let e = &edges[i];
        pieces.push(Piece { edge: i, strip: right_strip, seg: Seg { ly: e.child_y, ry: e.parent_y, key: None } });
    }
    // Spuren je Streifen
    let mut by_strip: HashMap<usize, Vec<usize>> = HashMap::new();
    for (p, piece) in pieces.iter().enumerate() {
        by_strip.entry(piece.strip).or_default().push(p);
    }
    let mut x_of: Vec<f32> = vec![0.0; pieces.len()];
    for (s, list) in &by_strip {
        let segs: Vec<Seg> = list.iter().map(|&p| pieces[p].seg).collect();
        let (nets, of) = nets_of(&segs);
        let (track, count) = assign_tracks(&nets);
        let strip = strips[*s];
        let width = strip.hi - strip.lo;
        // Platz fuer Marken (Kraehenfuss, Beschriftung) an beiden Enden
        let usable = (width - 2.0 * 8.0).max(0.0);
        let spacing = if count <= 1 { 0.0 } else { (usable / (count - 1) as f32).min(TRACK) };
        let first = (strip.lo + strip.hi) / 2.0 - spacing * (count.saturating_sub(1)) as f32 / 2.0;
        for (k, &p) in list.iter().enumerate() {
            x_of[p] = first + spacing * track[of[k]] as f32;
        }
    }
    // Linien zusammensetzen (vom Kind zum Elternteil)
    let mut piece_of_edge: HashMap<usize, Vec<usize>> = HashMap::new();
    for (p, piece) in pieces.iter().enumerate() {
        piece_of_edge.entry(piece.edge).or_default().push(p);
    }
    for (&i, plan) in &plans {
        let ps = &piece_of_edge[&i];
        let mut pts: Vec<Pos2> = Vec::new();
        pts.push(pos2(rects[plan.left].right(), plan.ys[0]));
        if plan.strips.len() == 1 && plan.ys.len() > 2 {
            // Fahrspuren verworfen: sollte nicht vorkommen
        }
        for (k, &p) in ps.iter().enumerate() {
            let x = x_of[p];
            pts.push(pos2(x, plan.ys[k]));
            pts.push(pos2(x, plan.ys[k + 1]));
        }
        pts.push(pos2(rects[plan.right].left(), *plan.ys.last().unwrap()));
        dedup(&mut pts);
        if !plan.parent_left {
            // Linie laeuft vom Kind (links) zum Elternteil (rechts): schon richtig herum
        } else {
            pts.reverse();
        }
        out[i] = Routed { points: pts };
    }
    for &i in &c_edges {
        let e = &edges[i];
        let p = pieces.iter().position(|pc| pc.edge == i).unwrap();
        let x = x_of[p];
        let (rc, rp) = (rects[e.child], rects[e.parent]);
        let mut pts = vec![pos2(rc.right(), e.child_y), pos2(x, e.child_y), pos2(x, e.parent_y), pos2(rp.right(), e.parent_y)];
        dedup(&mut pts);
        out[i] = Routed { points: pts };
    }
    // Rueckverweise einer Tabelle auf sich selbst: Schleife rechts neben der Tabelle
    let mut loops_of: HashMap<usize, usize> = HashMap::new();
    for &i in &self_loops {
        let e = &edges[i];
        let r = rects[e.child];
        let n = loops_of.entry(e.child).or_insert(0);
        let x = r.right() + 22.0 + 10.0 * *n as f32;
        *n += 1;
        let (y1, y2) = if (e.child_y - e.parent_y).abs() < 1.0 { (e.child_y - 3.0, e.parent_y + 3.0) } else { (e.child_y, e.parent_y) };
        out[i] = Routed { points: vec![pos2(r.right(), y1), pos2(x, y1), pos2(x, y2), pos2(r.right(), y2)] };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    fn net(l: &[f32], r: &[f32]) -> Net {
        Net { lefts: l.to_vec(), rights: r.to_vec() }
    }

    #[test]
    fn disjoint_nets_share_a_track() {
        let (t, n) = assign_tracks(&[net(&[0.0], &[10.0]), net(&[50.0], &[60.0])]);
        assert_eq!(n, 1);
        assert_eq!(t, vec![0, 0]);
        let (_, n) = assign_tracks(&[net(&[0.0], &[100.0]), net(&[50.0], &[60.0])]);
        assert_eq!(n, 2);
        assert_eq!(assign_tracks(&[]).1, 0);
    }

    #[test]
    fn order_avoids_crossings_for_parallel_downward_edges() {
        // A: 0 -> 100, B: 20 -> 120 (beide nach unten, ueberlappend): der tiefer beginnende B muss links liegen
        let nets = [net(&[0.0], &[100.0]), net(&[20.0], &[120.0])];
        let (t, n) = assign_tracks(&nets);
        assert_eq!(n, 2);
        assert!(t[1] < t[0], "{t:?}");
        // gleiches Ergebnis mit anderer Eingabereihenfolge
        let nets = [net(&[20.0], &[120.0]), net(&[0.0], &[100.0])];
        let (t, _) = assign_tracks(&nets);
        assert!(t[0] < t[1], "{t:?}");
    }

    #[test]
    fn shared_port_forms_one_net() {
        let segs = [Seg { ly: 10.0, ry: 40.0, key: Some(1) }, Seg { ly: 10.0, ry: 90.0, key: Some(1) }, Seg { ly: 30.0, ry: 50.0, key: None }];
        let (nets, of) = nets_of(&segs);
        assert_eq!(nets.len(), 2);
        assert_eq!(of, vec![0, 0, 1]);
        assert_eq!(nets[0].lefts, vec![10.0]);
        assert_eq!(nets[0].rights, vec![40.0, 90.0]);
        assert_eq!(tracks_needed(&segs), 2);
        assert!(gap_width(1) >= 96.0 && gap_width(30) > gap_width(5));
    }

    fn boxes() -> Vec<Rect> {
        // parent links, drei Kinder rechts untereinander
        vec![
            Rect::from_min_size(pos2(0.0, 100.0), vec2(120.0, 80.0)),
            Rect::from_min_size(pos2(300.0, 0.0), vec2(120.0, 60.0)),
            Rect::from_min_size(pos2(300.0, 120.0), vec2(120.0, 60.0)),
            Rect::from_min_size(pos2(300.0, 240.0), vec2(120.0, 60.0)),
        ]
    }

    fn edge(child: usize, parent: usize, cy: f32, py: f32) -> EdgeIn {
        EdgeIn { child, parent, child_y: cy, parent_y: py, lanes: vec![] }
    }

    fn orthogonal(pts: &[Pos2]) -> bool {
        pts.windows(2).all(|w| (w[0].x - w[1].x).abs() < 0.01 || (w[0].y - w[1].y).abs() < 0.01)
    }

    #[test]
    fn fan_out_shares_one_trunk_and_is_orthogonal() {
        let rects = boxes();
        let edges = vec![edge(1, 0, 20.0, 130.0), edge(2, 0, 150.0, 130.0), edge(3, 0, 270.0, 130.0)];
        let routes = route(&rects, &edges);
        for (r, e) in routes.iter().zip(&edges) {
            assert!(orthogonal(&r.points), "{:?}", r.points);
            // beginnt am Kind (links der Kinderkante), endet am Elternteil
            assert!((r.points[0].x - rects[e.child].left()).abs() < 0.01);
            assert!((r.points.last().unwrap().x - rects[e.parent].right()).abs() < 0.01);
        }
        // alle drei benutzen dieselbe senkrechte Spur
        let trunk_x: Vec<f32> = routes.iter().map(|r| r.points[r.points.len() - 2].x).collect();
        assert!(trunk_x.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01), "{trunk_x:?}");
        assert!(trunk_x[0] > rects[0].right() && trunk_x[0] < rects[1].left());
    }

    #[test]
    fn self_reference_loops_out_on_the_right() {
        let rects = vec![Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0)), Rect::from_min_size(pos2(300.0, 0.0), vec2(100.0, 80.0))];
        let routes = route(&rects, &[edge(0, 0, 20.0, 40.0), edge(1, 0, 30.0, 40.0)]);
        assert!(routes[0].points.iter().all(|p| p.x >= 100.0 - 0.01));
        assert!(orthogonal(&routes[0].points));
    }

    #[test]
    fn tables_without_a_free_strip_use_the_outer_channel() {
        // Zwei Tabellen uebereinander, ueberlappende Spalten
        let rects = vec![Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 60.0)), Rect::from_min_size(pos2(20.0, 120.0), vec2(100.0, 60.0))];
        let routes = route(&rects, &[edge(1, 0, 140.0, 20.0)]);
        let p = &routes[0].points;
        assert!(orthogonal(p) && p.len() >= 3, "{p:?}");
        assert!(p.iter().all(|q| q.x >= 0.0));
    }

    #[test]
    fn long_edge_uses_lane_height_between_columns() {
        // a (links) -> b (mitte) -> c (rechts) und c -> a ueber die Spalte von b hinweg
        let rects = vec![
            Rect::from_min_size(pos2(0.0, 100.0), vec2(100.0, 60.0)),
            Rect::from_min_size(pos2(250.0, 100.0), vec2(100.0, 60.0)),
            Rect::from_min_size(pos2(500.0, 100.0), vec2(100.0, 60.0)),
        ];
        let mut long = edge(2, 0, 120.0, 120.0);
        long.lanes = vec![60.0];
        let routes = route(&rects, &[edge(1, 0, 120.0, 120.0), edge(2, 1, 120.0, 120.0), long]);
        let p = &routes[2].points;
        assert!(orthogonal(p), "{p:?}");
        // geht oberhalb von b durch: irgendwo waagerecht in Hoehe 60 durch die mittlere Spalte
        assert!(p.windows(2).any(|w| (w[0].y - 60.0).abs() < 0.01 && (w[1].y - 60.0).abs() < 0.01 && w[0].x.min(w[1].x) < 250.0 && w[0].x.max(w[1].x) > 350.0), "{p:?}");
        // keine Strecke durch die Tabelle b
        for w in p.windows(2) {
            let seg = Rect::from_two_pos(w[0], w[1]);
            assert!(!seg.shrink(0.5).intersects(rects[1]) || seg.width() < 0.5 && seg.height() < 0.5, "{w:?}");
        }
    }
}
