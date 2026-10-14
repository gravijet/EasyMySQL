// Automatische Anordnung fuer das ER-Diagramm (Ebenenlayout nach Sugiyama).
//
// Eltern stehen links, Kinder rechts: Jede Beziehung laeuft von links nach rechts, die Tabellen
// einer Ebene stehen untereinander. Lange Beziehungen bekommen in jeder uebersprungenen Ebene
// eine eigene Fahrspur (Zwischenpunkt), damit sie nicht durch fremde Tabellen laufen.
// Ablauf je zusammenhaengender Gruppe:
//   1. Zyklen aufbrechen, Ebenen bestimmen (laengster Weg, Nachschlagetabellen nah an ihre Kinder)
//   2. Reihenfolge in den Ebenen: Schwerpunkt-Verfahren und Nachbarn tauschen (wenig Kreuzungen)
//   3. Hoehe: jede Tabelle moeglichst auf gleicher Hoehe wie die Spalten, mit denen sie verbunden ist
// Die Gruppen werden zu einem Rechteck etwa im Bildschirmformat gepackt, Tabellen ohne
// Beziehungen kommen gesammelt darunter.

use super::ortho;
use eframe::egui::{Pos2, Vec2, pos2, vec2};

/// Abstand zwischen Tabellen einer Ebene
const GAP_Y: f32 = 36.0;
/// Hoehe einer Fahrspur und Abstand zu ihren Nachbarn
const LANE_H: f32 = 8.0;
const LANE_GAP: f32 = 6.0;
/// Abstand zwischen Gruppen
const COMP_GAP: f32 = 120.0;

/// Eine Beziehung: Kind (hat den Fremdschluessel) -> Elternteil. `*_y` = Hoehe der Spalte in der
/// Tabelle (ab Oberkante), an der die Linie ansetzt.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub child: usize,
    pub parent: usize,
    pub child_y: f32,
    pub parent_y: f32,
}

pub struct Layout {
    /// Linke obere Ecke jeder Tabelle
    pub pos: Vec<Pos2>,
    /// Fahrspuren je Beziehung (vom Kind zum Elternteil), None = direkte Linie
    pub lanes: Vec<Option<Vec<Pos2>>>,
    /// Kreuzungen zwischen benachbarten Ebenen (Mass fuer die Guete)
    #[cfg_attr(not(test), allow(dead_code))]
    pub crossings: usize,
}

struct Pair {
    child: usize,
    parent: usize,
    child_y: f32,
    parent_y: f32,
}

struct Comp {
    nodes: Vec<(usize, Pos2)>,
    lanes: Vec<(usize, Vec<Pos2>)>,
    size: Vec2,
    crossings: usize,
}

/// Nachbar in der Nachbarebene: (Knoten, Ansatzhoehe hier, Ansatzhoehe dort, Gewicht)
type Nb = (usize, f32, f32, f32);

struct Lg {
    layers: Vec<Vec<usize>>,
    layer: Vec<usize>,
    pos: Vec<usize>,
    left: Vec<Vec<Nb>>,
    right: Vec<Vec<Nb>>,
    h: Vec<f32>,
    w: Vec<f32>,
    real: usize,
}

impl Lg {
    /// Kreuzungen zwischen Ebene `l` und `l + 1`: Paare von Kanten, die sich in der Reihenfolge umkehren
    fn crossings_between(&self, l: usize) -> usize {
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for &v in &self.layers[l] {
            for &(nb, ..) in &self.right[v] {
                edges.push((self.pos[v], self.pos[nb]));
            }
        }
        edges.sort_unstable();
        let width = self.layers[l + 1].len();
        let mut tree = vec![0u32; width + 1];
        let mut count = 0usize;
        for (seen, &(_, low)) in edges.iter().enumerate() {
            // schon eingefuegte Kanten mit groesserer unterer Position kreuzen diese Kante
            let mut i = low + 1;
            let mut le = 0usize;
            while i > 0 {
                le += tree[i] as usize;
                i -= i.isolate_lowest_one();
            }
            count += seen - le;
            let mut i = low + 1;
            while i <= width {
                tree[i] += 1;
                i += i.isolate_lowest_one();
            }
        }
        count
    }

    fn total_crossings(&self) -> usize {
        (0..self.layers.len().saturating_sub(1)).map(|l| self.crossings_between(l)).sum()
    }

    fn renumber(&mut self, l: usize) {
        for (i, &v) in self.layers[l].iter().enumerate() {
            self.pos[v] = i;
        }
    }

    /// Eine Ebene nach dem Schwerpunkt der Nachbarn in Richtung `toward_left` ordnen
    fn reorder(&mut self, l: usize, toward_left: bool) {
        let mut keyed: Vec<(f32, usize, usize)> = self.layers[l]
            .iter()
            .map(|&v| {
                let nbs = if toward_left { &self.left[v] } else { &self.right[v] };
                let key = if nbs.is_empty() {
                    self.pos[v] as f32
                } else {
                    nbs.iter().map(|&(n, ..)| self.pos[n] as f32).sum::<f32>() / nbs.len() as f32
                };
                (key, self.pos[v], v)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.layers[l] = keyed.into_iter().map(|k| k.2).collect();
        self.renumber(l);
    }

    /// Kreuzungen, die entstehen, wenn `u` ueber `v` steht (nur Kanten an u und v)
    fn pair_crossings(&self, u: usize, v: usize) -> usize {
        let mut c = 0;
        for (a, b) in [(&self.left[u], &self.left[v]), (&self.right[u], &self.right[v])] {
            for &(x, ..) in a {
                for &(y, ..) in b {
                    if self.pos[x] > self.pos[y] {
                        c += 1;
                    }
                }
            }
        }
        c
    }

    /// Benachbarte Tabellen vertauschen, wenn das Kreuzungen spart
    fn transpose(&mut self) -> bool {
        let mut any = false;
        for l in 0..self.layers.len() {
            let mut improved = true;
            let mut rounds = 0;
            while improved && rounds < 8 {
                improved = false;
                rounds += 1;
                for i in 0..self.layers[l].len().saturating_sub(1) {
                    let (u, v) = (self.layers[l][i], self.layers[l][i + 1]);
                    if self.pair_crossings(v, u) < self.pair_crossings(u, v) {
                        self.layers[l].swap(i, i + 1);
                        self.pos[u] = i + 1;
                        self.pos[v] = i;
                        improved = true;
                        any = true;
                    }
                }
            }
        }
        any
    }

    fn gap(&self, a: usize, b: usize) -> f32 {
        match (a >= self.real, b >= self.real) {
            (true, true) => LANE_GAP,
            (false, false) => GAP_Y,
            _ => (GAP_Y + LANE_GAP) / 2.0,
        }
    }

    /// Eine Ebene so setzen, dass die Abstaende eingehalten werden und die Summe der
    /// quadratischen Abweichungen zu den Wunschhoehen minimal ist (Zusammenfassen von Bloecken).
    fn place_layer(&self, l: usize, desired: &[f32]) -> Vec<f32> {
        let layer = &self.layers[l];
        let m = layer.len();
        let mut off = vec![0.0f32; m];
        for i in 1..m {
            off[i] = off[i - 1] + self.h[layer[i - 1]] + self.gap(layer[i - 1], layer[i]);
        }
        // Bloecke: (Summe, Anzahl, erster Index)
        let mut blocks: Vec<(f32, usize, usize)> = Vec::with_capacity(m);
        for i in 0..m {
            blocks.push((desired[i] - off[i], 1, i));
            while blocks.len() >= 2 {
                let (s2, c2, _) = blocks[blocks.len() - 1];
                let (s1, c1, f1) = blocks[blocks.len() - 2];
                if s1 / c1 as f32 > s2 / c2 as f32 {
                    blocks.truncate(blocks.len() - 2);
                    blocks.push((s1 + s2, c1 + c2, f1));
                } else {
                    break;
                }
            }
        }
        let mut out = vec![0.0f32; m];
        for (b, &(s, c, first)) in blocks.iter().enumerate() {
            let end = blocks.get(b + 1).map_or(m, |n| n.2);
            let base = s / c as f32;
            for i in first..end {
                out[i] = base + off[i];
            }
        }
        out
    }
}


/// Anfangshoehen: die Tabellen jeder Ebene untereinander, Ebenen mittig zueinander
fn stack_layers(lg: &Lg, y: &mut [f32]) {
    let mut heights = Vec::with_capacity(lg.layers.len());
    for layer_v in &lg.layers {
        let mut at = 0.0;
        for (i, &v) in layer_v.iter().enumerate() {
            if i > 0 {
                at += lg.gap(layer_v[i - 1], v);
            }
            y[v] = at;
            at += lg.h[v];
        }
        heights.push(at);
    }
    let tallest = heights.iter().copied().fold(0.0, f32::max);
    for (l, layer_v) in lg.layers.iter().enumerate() {
        let shift = (tallest - heights[l]) / 2.0;
        for &v in layer_v {
            y[v] += shift;
        }
    }
}

/// Hoehen so setzen, dass jede Tabelle moeglichst auf gleicher Hoehe wie die Spalten liegt, mit denen
/// sie verbunden ist (abwechselnd von links nach rechts und zurueck)
fn assign_heights(lg: &Lg, y: &mut [f32], passes: usize) {
    let n_layers = lg.layers.len();
    let mut desired = Vec::new();
    for pass in 0..passes {
        let order: Vec<usize> = if pass % 2 == 0 { (0..n_layers).collect() } else { (0..n_layers).rev().collect() };
        for l in order {
            desired.clear();
            for &v in &lg.layers[l] {
                let (mut sum, mut wsum) = (0.0f32, 0.0f32);
                for &(nb, mine, theirs, wt) in lg.left[v].iter().chain(lg.right[v].iter()) {
                    sum += (y[nb] + theirs - mine) * wt;
                    wsum += wt;
                }
                desired.push(if wsum > 0.0 { sum / wsum } else { y[v] });
            }
            let placed = lg.place_layer(l, &desired);
            for (&v, p) in lg.layers[l].iter().zip(placed) {
                y[v] = p;
            }
        }
    }
}

/// Benachbarte Tabellen vertauschen, wenn sich dadurch Linien weniger kreuzen – gemessen an den
/// echten Ansatzpunkten (Spaltenzeilen), nicht nur an der Reihenfolge der Tabellen.
fn refine_ports(lg: &mut Lg, y: &[f32]) -> bool {
    // Kreuzungen, wenn `a` ueber `b` steht
    let cost = |lg: &Lg, a: usize, b: usize| -> usize {
        let mut c = 0;
        for (sa, sb) in [(&lg.left[a], &lg.left[b]), (&lg.right[a], &lg.right[b])] {
            for &(x, _, px, _) in sa {
                for &(w, _, pw, _) in sb {
                    if y[x] + px > y[w] + pw + 0.01 {
                        c += 1;
                    }
                }
            }
        }
        c
    };
    let mut changed = false;
    for l in 0..lg.layers.len() {
        for i in 0..lg.layers[l].len().saturating_sub(1) {
            let (u, v) = (lg.layers[l][i], lg.layers[l][i + 1]);
            if cost(lg, v, u) < cost(lg, u, v) {
                lg.layers[l].swap(i, i + 1);
                lg.pos[u] = i + 1;
                lg.pos[v] = i;
                changed = true;
            }
        }
    }
    changed
}

fn layer_component(nodes: &[usize], sizes: &[Vec2], pairs: &[Pair], ids: &[usize]) -> Comp {
    let n = nodes.len();
    let mut local = std::collections::HashMap::with_capacity(n);
    for (k, &g) in nodes.iter().enumerate() {
        local.insert(g, k);
    }

    // --- 1. Zyklen aufbrechen: Kanten Elternteil -> Kind, rueckwaerts zeigende werden umgedreht
    let mut out_adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n]; // (Ziel, Index in ids)
    let mut has_parent = vec![false; n];
    for (k, &id) in ids.iter().enumerate() {
        let (p, c) = (local[&pairs[id].parent], local[&pairs[id].child]);
        out_adj[p].push((c, k));
        has_parent[c] = true;
    }
    let mut reversed = vec![false; ids.len()];
    {
        let mut color = vec![0u8; n];
        let mut starts: Vec<usize> = (0..n).filter(|&v| !has_parent[v]).collect();
        starts.extend((0..n).filter(|&v| has_parent[v]));
        for s in starts {
            if color[s] != 0 {
                continue;
            }
            let mut stack = vec![(s, 0usize)];
            color[s] = 1;
            while let Some(&mut (v, ref mut i)) = stack.last_mut() {
                if *i < out_adj[v].len() {
                    let (w, k) = out_adj[v][*i];
                    *i += 1;
                    match color[w] {
                        0 => {
                            color[w] = 1;
                            stack.push((w, 0));
                        }
                        1 => reversed[k] = true,
                        _ => {}
                    }
                } else {
                    color[v] = 2;
                    stack.pop();
                }
            }
        }
    }
    // Kanten (links, rechts, Index in ids) des azyklischen Graphen
    let dag: Vec<(usize, usize, usize)> = ids
        .iter()
        .enumerate()
        .map(|(k, &id)| {
            let (p, c) = (local[&pairs[id].parent], local[&pairs[id].child]);
            if reversed[k] { (c, p, k) } else { (p, c, k) }
        })
        .collect();

    // --- Ebenen: laengster Weg von links
    let mut indeg = vec![0usize; n];
    let mut dag_out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b, _) in &dag {
        indeg[b] += 1;
        dag_out[a].push(b);
    }
    let mut layer = vec![0usize; n];
    let mut queue: std::collections::VecDeque<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut remaining = indeg.clone();
    let mut topo = Vec::with_capacity(n);
    while let Some(v) = queue.pop_front() {
        topo.push(v);
        for &w in &dag_out[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            remaining[w] -= 1;
            if remaining[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    // Quellen (ohne Eltern) bis direkt vor ihr erstes Kind ruecken: kuerzere Linien
    for v in 0..n {
        if indeg[v] == 0
            && let Some(first) = dag_out[v].iter().map(|&w| layer[w]).min()
        {
            layer[v] = first - 1;
        }
    }
    // Knoten naeher an die Mehrheit ihrer Nachbarn ruecken: kuerzere Linien, weniger Fahrspuren.
    // Wer mehr Eltern als Kinder hat, rueckt an seine Eltern heran, sonst an seine Kinder.
    {
        let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
        for &(a, b, _) in &dag {
            parents[b].push(a);
        }
        let mut lay: Vec<isize> = layer.iter().map(|&l| l as isize).collect();
        for _ in 0..24 {
            let mut moved = false;
            for &v in &topo {
                let (nin, nout) = (parents[v].len(), dag_out[v].len());
                if nin == nout {
                    continue;
                }
                let lo = parents[v].iter().map(|&p| lay[p] + 1).max().unwrap_or(isize::MIN);
                let hi = dag_out[v].iter().map(|&c| lay[c] - 1).min().unwrap_or(isize::MAX);
                if lo > hi {
                    continue;
                }
                let target = if nin > nout { lo } else { hi };
                if target != isize::MIN && target != isize::MAX && target != lay[v] {
                    lay[v] = target;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        // leere Ebenen entfernen
        let mut used: Vec<isize> = lay.clone();
        used.sort_unstable();
        used.dedup();
        for (l, &v) in layer.iter_mut().zip(&lay) {
            *l = used.binary_search(&v).unwrap_or(0);
        }
    }
    let base = layer.iter().copied().min().unwrap_or(0);
    layer.iter_mut().for_each(|l| *l -= base);
    let n_layers = layer.iter().copied().max().unwrap_or(0) + 1;

    // --- Zwischenknoten (Fahrspuren) fuer Kanten ueber mehrere Ebenen
    let mut lg = Lg {
        layers: vec![Vec::new(); n_layers],
        layer: layer.clone(),
        pos: Vec::new(),
        left: vec![Vec::new(); n],
        right: vec![Vec::new(); n],
        h: nodes.iter().map(|&g| sizes[g].y).collect(),
        w: nodes.iter().map(|&g| sizes[g].x).collect(),
        real: n,
    };
    // Kette je Kante: Knoten von links nach rechts
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(dag.len());
    for &(a, b, k) in &dag {
        let pair = &pairs[ids[k]];
        let (pa, pb) = if reversed[k] { (pair.child_y, pair.parent_y) } else { (pair.parent_y, pair.child_y) };
        let mut chain = vec![a];
        for l in layer[a] + 1..layer[b] {
            let d = lg.h.len();
            lg.h.push(LANE_H);
            lg.w.push(0.0);
            lg.layer.push(l);
            lg.left.push(Vec::new());
            lg.right.push(Vec::new());
            chain.push(d);
        }
        chain.push(b);
        let last = chain.len() - 1;
        for i in 0..last {
            let (x, y) = (chain[i], chain[i + 1]);
            let px = if i == 0 { pa } else { LANE_H / 2.0 };
            let py = if i + 1 == last { pb } else { LANE_H / 2.0 };
            // Fahrspuren sollen gerade bleiben: hoeheres Gewicht
            let weight = if last > 1 { 2.5 } else { 1.0 };
            lg.right[x].push((y, px, py, weight));
            lg.left[y].push((x, py, px, weight));
        }
        chains.push(chain);
    }
    let total = lg.h.len();
    lg.pos = vec![0; total];

    // --- 2. Reihenfolge: zuerst nach Tiefensuche, dann Schwerpunkt-Verfahren
    let mut visit = vec![f32::MAX; total];
    {
        let mut sources: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
        sources.sort_by_key(|&v| std::cmp::Reverse(dag_out[v].len()));
        let mut counter = 0.0f32;
        for s in sources.into_iter().chain(topo.iter().copied()) {
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                if visit[v] != f32::MAX {
                    continue;
                }
                visit[v] = counter;
                counter += 1.0;
                for &(w, ..) in lg.right[v].iter().rev() {
                    if visit[w] == f32::MAX {
                        stack.push(w);
                    }
                }
            }
        }
    }
    for v in visit.iter_mut().filter(|v| **v == f32::MAX) {
        *v = 0.0;
    }
    let mut order: Vec<usize> = (0..total).collect();
    order.sort_by(|&a, &b| visit[a].total_cmp(&visit[b]).then(a.cmp(&b)));
    for v in order {
        let l = lg.layer[v];
        lg.layers[l].push(v);
    }
    for l in 0..n_layers {
        lg.renumber(l);
    }
    let mut best = (lg.total_crossings(), lg.layers.clone());
    // Mehr Durchgaenge bei kleinen Diagrammen (Arbeit je Durchgang ~ Zahl der Kanten)
    let layered_edges: usize = lg.right.iter().map(|v| v.len()).sum();
    let sweeps = (24_000_000 / (layered_edges + 1)).clamp(6, 60);
    let mut stale = 0;
    for it in 0..sweeps {
        if it % 2 == 0 {
            for l in 1..n_layers {
                lg.reorder(l, true);
            }
        } else {
            for l in (0..n_layers.saturating_sub(1)).rev() {
                lg.reorder(l, false);
            }
        }
        if total <= 8000 {
            lg.transpose();
        }
        let c = lg.total_crossings();
        if c < best.0 {
            best = (c, lg.layers.clone());
            stale = 0;
        } else {
            stale += 1;
            if stale >= 8 {
                break;
            }
        }
    }
    lg.layers = best.1;
    for l in 0..n_layers {
        lg.renumber(l);
    }
    let crossings = best.0;

    // --- 3. Hoehen; danach Reihenfolge nach den tatsaechlichen Ansatzpunkten der Linien verfeinern
    let mut y = vec![0.0f32; total];
    let passes = if total > 20_000 { 4 } else { 14 };
    stack_layers(&lg, &mut y);
    assign_heights(&lg, &mut y, passes);
    if total <= 12_000 {
        for _ in 0..3 {
            if !refine_ports(&mut lg, &y) {
                break;
            }
            stack_layers(&lg, &mut y);
            assign_heights(&lg, &mut y, passes);
        }
    }
    let min_y = y.iter().copied().fold(f32::MAX, f32::min);
    y.iter_mut().for_each(|v| *v -= min_y);

    // --- Spalten (x): der Abstand zwischen zwei Spalten richtet sich nach der Zahl der Spuren, die ihre Linien brauchen
    let mut width = vec![0.0f32; n_layers];
    for v in 0..n {
        width[lg.layer[v]] = width[lg.layer[v]].max(lg.w[v]);
    }
    let mut gap_segs: Vec<Vec<ortho::Seg>> = vec![Vec::new(); n_layers];
    for (c, chain) in chains.iter().enumerate() {
        let k = dag[c].2;
        let pair = &pairs[ids[k]];
        let (pa, pb) = if reversed[k] { (pair.child_y, pair.parent_y) } else { (pair.parent_y, pair.child_y) };
        let last = chain.len() - 1;
        for i in 0..last {
            let (x, z) = (chain[i], chain[i + 1]);
            let ly = y[x] + if i == 0 { pa } else { lg.h[x] / 2.0 };
            let ry = y[z] + if i + 1 == last { pb } else { lg.h[z] / 2.0 };
            // Linien vom selben Anschluss des Elternteils teilen sich einen Strang
            let key = if i == 0 && !reversed[k] {
                Some(ortho::port_key(nodes[x], ly, true))
            } else if i + 1 == last && reversed[k] {
                Some(ortho::port_key(nodes[z], ry, false))
            } else {
                None
            };
            gap_segs[lg.layer[x]].push(ortho::Seg { ly, ry, key });
        }
    }
    let mut layer_x = vec![0.0f32; n_layers];
    let mut at = 0.0;
    for l in 0..n_layers {
        if width[l] == 0.0 {
            width[l] = 12.0;
        }
        layer_x[l] = at;
        at += width[l] + ortho::gap_width(ortho::tracks_needed(&gap_segs[l]));
    }
    let mut comp = Comp { nodes: Vec::with_capacity(n), lanes: Vec::new(), size: Vec2::ZERO, crossings };
    let (mut max_x, mut max_y) = (0.0f32, 0.0f32);
    for v in 0..n {
        let l = lg.layer[v];
        let p = pos2(layer_x[l] + (width[l] - lg.w[v]) / 2.0, y[v]);
        max_x = max_x.max(p.x + lg.w[v]);
        max_y = max_y.max(p.y + lg.h[v]);
        comp.nodes.push((nodes[v], p));
    }
    for (c, chain) in chains.iter().enumerate() {
        if chain.len() <= 2 {
            continue;
        }
        let k = dag[c].2;
        let mut pts: Vec<Pos2> = chain[1..chain.len() - 1]
            .iter()
            .map(|&d| {
                let l = lg.layer[d];
                pos2(layer_x[l] + width[l] / 2.0, y[d] + lg.h[d] / 2.0)
            })
            .collect();
        // vom Kind zum Elternteil
        if !reversed[k] {
            pts.reverse();
        }
        max_y = max_y.max(pts.iter().map(|p| p.y).fold(0.0, f32::max));
        comp.lanes.push((ids[k], pts));
    }
    comp.size = vec2(max_x, max_y);
    comp
}

/// Anordnung berechnen. `sizes`: Groesse jeder Tabelle, `edges`: Beziehungen.
pub fn layout(sizes: &[Vec2], edges: &[Edge]) -> Layout {
    let n = sizes.len();
    // gleiche Kind/Eltern-Paare zusammenfassen
    let mut pair_of = std::collections::HashMap::new();
    let mut pairs: Vec<Pair> = Vec::new();
    let mut edge_pair: Vec<Option<usize>> = Vec::with_capacity(edges.len());
    for e in edges {
        if e.child >= n || e.parent >= n || e.child == e.parent {
            edge_pair.push(None);
            continue;
        }
        let id = *pair_of.entry((e.child, e.parent)).or_insert_with(|| {
            pairs.push(Pair { child: e.child, parent: e.parent, child_y: e.child_y, parent_y: e.parent_y });
            pairs.len() - 1
        });
        edge_pair.push(Some(id));
    }
    // Gruppen: zusammenhaengende Tabellen
    let mut uf: Vec<usize> = (0..n).collect();
    fn find(uf: &mut [usize], mut x: usize) -> usize {
        while uf[x] != x {
            uf[x] = uf[uf[x]];
            x = uf[x];
        }
        x
    }
    for p in &pairs {
        let (a, b) = (find(&mut uf, p.child), find(&mut uf, p.parent));
        if a != b {
            uf[a] = b;
        }
    }
    let mut members: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for v in 0..n {
        let r = find(&mut uf, v);
        members.entry(r).or_default().push(v);
    }
    let mut comp_pairs: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (id, p) in pairs.iter().enumerate() {
        let r = find(&mut uf, p.child);
        comp_pairs.entry(r).or_default().push(id);
    }

    let mut comps: Vec<Comp> = Vec::new();
    let mut rest: Vec<usize> = Vec::new();
    for (root, nodes) in &members {
        if nodes.len() < 2 {
            rest.extend(nodes);
            continue;
        }
        let ids = comp_pairs.get(root).cloned().unwrap_or_default();
        comps.push(layer_component(nodes, sizes, &pairs, &ids));
    }
    // grosse Gruppen zuerst, bei Gleichstand nach der kleinsten Tabellennummer (stabil)
    comps.sort_by(|a, b| (b.size.x * b.size.y).total_cmp(&(a.size.x * a.size.y)).then(a.nodes[0].0.cmp(&b.nodes[0].0)));

    let mut result = Layout { pos: vec![Pos2::ZERO; n], lanes: vec![None; edges.len()], crossings: comps.iter().map(|c| c.crossings).sum() };
    let mut lane_of_pair: std::collections::HashMap<usize, Vec<Pos2>> = std::collections::HashMap::new();
    let (mut right, mut bottom) = (0.0f32, 0.0f32);
    if !comps.is_empty() {
        let area: f32 = comps.iter().map(|c| (c.size.x + COMP_GAP) * (c.size.y + COMP_GAP)).sum();
        let widest = comps.iter().map(|c| c.size.x).fold(0.0, f32::max);
        let target = widest.max((area * 1.7).sqrt());
        let (mut x, mut y, mut row_h) = (0.0f32, 0.0f32, 0.0f32);
        for c in &comps {
            if x > 0.0 && x + c.size.x > target {
                x = 0.0;
                y += row_h + COMP_GAP;
                row_h = 0.0;
            }
            let off = vec2(x, y);
            for &(g, p) in &c.nodes {
                result.pos[g] = p + off;
            }
            for (id, pts) in &c.lanes {
                lane_of_pair.insert(*id, pts.iter().map(|p| *p + off).collect());
            }
            right = right.max(x + c.size.x);
            bottom = bottom.max(y + c.size.y);
            x += c.size.x + COMP_GAP;
            row_h = row_h.max(c.size.y);
        }
    }
    for (i, p) in edge_pair.iter().enumerate() {
        if let Some(id) = p {
            result.lanes[i] = lane_of_pair.get(id).cloned();
        }
    }

    // Tabellen ohne Beziehungen: Raster darunter
    if !rest.is_empty() {
        rest.sort_unstable();
        let width = if comps.is_empty() { (rest.len() as f32).sqrt().ceil() * 260.0 } else { right.max(700.0) };
        let (mut x, mut y, mut line_h) = (0.0f32, if comps.is_empty() { 0.0 } else { bottom + COMP_GAP }, 0.0f32);
        for i in rest {
            let s = sizes[i];
            if x > 0.0 && x + s.x > width {
                x = 0.0;
                y += line_h + 30.0;
                line_h = 0.0;
            }
            result.pos[i] = pos2(x, y);
            x += s.x + 50.0;
            line_h = line_h.max(s.y);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Rect;

    fn t(cols: usize) -> Vec2 {
        vec2(170.0, 28.0 + 17.0 * cols as f32)
    }

    fn edge(child: usize, parent: usize) -> Edge {
        Edge { child, parent, child_y: 40.0, parent_y: 40.0 }
    }

    fn check_no_overlap(sizes: &[Vec2], lay: &Layout) {
        let mut rects: Vec<(usize, Rect)> = sizes.iter().enumerate().map(|(i, s)| (i, Rect::from_min_size(lay.pos[i], *s))).collect();
        rects.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x));
        for (k, (i, a)) in rects.iter().enumerate() {
            for (j, b) in rects[k + 1..].iter().take_while(|(_, b)| b.min.x < a.max.x) {
                assert!(!a.expand(8.0).intersects(*b), "Tabellen {i} und {j} ueberlappen");
            }
        }
    }

    /// Names: kunde 0, ort 1, kategorie 2, artikel 3, hersteller 4, bestellung 5, position 6,
    /// lieferung 7, bewertung 8, lager 9, bestand 10, gutschein 11, newsletter 12
    fn shop() -> (Vec<Vec2>, Vec<Edge>) {
        let sizes = vec![t(5), t(3), t(3), t(5), t(3), t(4), t(3), t(3), t(4), t(2), t(3), t(2), t(1)];
        let edges = vec![
            edge(0, 1),
            edge(3, 2),
            edge(3, 4),
            edge(4, 1),
            edge(5, 0),
            edge(6, 5),
            edge(6, 3),
            edge(7, 5),
            edge(8, 0),
            edge(8, 3),
            edge(9, 1),
            edge(10, 9),
            edge(10, 3),
        ];
        (sizes, edges)
    }

    #[test]
    fn shop_layout_is_clear() {
        let (sizes, edges) = shop();
        let lay = layout(&sizes, &edges);
        check_no_overlap(&sizes, &lay);
        assert!(lay.crossings <= 3, "Kreuzungen: {}", lay.crossings);
        // Eltern links von ihren Kindern
        for e in &edges {
            assert!(lay.pos[e.parent].x < lay.pos[e.child].x, "{} -> {}", e.child, e.parent);
        }
        // Tabellen ohne Beziehung liegen unter den anderen
        let bottom = (0..11).map(|i| lay.pos[i].y + sizes[i].y).fold(0.0, f32::max);
        assert!(lay.pos[11].y > bottom && lay.pos[12].y > bottom);
        // Fahrspuren nur fuer Linien ueber mehrere Ebenen, vom Kind zum Elternteil (nach links)
        for (e, lanes) in edges.iter().zip(&lay.lanes) {
            if let Some(l) = lanes {
                assert!(l[0].x < lay.pos[e.child].x && l[l.len() - 1].x > lay.pos[e.parent].x + sizes[e.parent].x);
                assert!(l.windows(2).all(|w| w[0].x > w[1].x));
            }
        }
    }

    #[test]
    fn long_edges_get_lanes_that_avoid_boxes() {
        // a <- b <- c <- d und d verweist zusaetzlich auf a: braucht Fahrspuren
        let sizes = vec![t(3), t(3), t(3), t(3)];
        let edges = vec![edge(1, 0), edge(2, 1), edge(3, 2), edge(3, 0)];
        let lay = layout(&sizes, &edges);
        assert!(lay.lanes[0].is_none() && lay.lanes[3].is_some());
        let lane = lay.lanes[3].as_ref().unwrap();
        assert_eq!(lane.len(), 2);
        for p in lane {
            for (i, s) in sizes.iter().enumerate() {
                assert!(!Rect::from_min_size(lay.pos[i], *s).expand(2.0).contains(*p), "Fahrspur in Tabelle {i}");
            }
        }
        check_no_overlap(&sizes, &lay);
    }

    #[test]
    fn cycles_and_self_references_terminate() {
        let sizes = vec![t(2), t(2), t(2), t(2)];
        let edges = vec![edge(0, 1), edge(1, 2), edge(2, 0), edge(3, 3), edge(3, 2), edge(9, 0)];
        let lay = layout(&sizes, &edges);
        assert_eq!(lay.pos.len(), 4);
        check_no_overlap(&sizes, &lay);
    }

    #[test]
    fn deterministic_and_small_cases() {
        let (sizes, edges) = shop();
        let a = layout(&sizes, &edges);
        let b = layout(&sizes, &edges);
        assert_eq!(a.pos, b.pos);
        assert_eq!(layout(&[t(2)], &[]).pos.len(), 1);
        let two = layout(&[t(2), t(2)], &[edge(0, 1), edge(0, 1)]);
        assert!(two.pos[1].x < two.pos[0].x);
        assert!(layout(&[], &[]).pos.is_empty());
    }

    #[test]
    fn star_has_no_crossings() {
        // Eine Tabelle, auf die zwoelf andere verweisen
        let mut sizes = vec![t(4)];
        let mut edges = Vec::new();
        for i in 1..=12 {
            sizes.push(t(3));
            edges.push(edge(i, 0));
        }
        let lay = layout(&sizes, &edges);
        assert_eq!(lay.crossings, 0);
        check_no_overlap(&sizes, &lay);
        let xs: std::collections::BTreeSet<i32> = lay.pos.iter().map(|p| p.x as i32).collect();
        assert_eq!(xs.len(), 2, "zwei Spalten: Elternteil links, Kinder rechts");
    }

    /// Datenbank mit vielen Tabellen: Baum-/Netzstruktur wie in grossen Anwendungen.
    fn big(n: usize) -> (Vec<Vec2>, Vec<Edge>) {
        let sizes: Vec<Vec2> = (0..n).map(|i| t(2 + i % 9)).collect();
        let mut edges = Vec::new();
        let mut seed = 12345u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // etwa jede zehnte Tabelle ohne Beziehung, sonst 1-3 Verweise auf frueher angelegte Tabellen
        for i in 1..n {
            if i % 10 == 0 {
                continue;
            }
            for _ in 0..1 + next() % 3 {
                edges.push(edge(i, (next() % i as u64) as usize));
            }
        }
        (sizes, edges)
    }

    #[test]
    fn many_tables_are_placed_without_overlap() {
        let (sizes, edges) = big(700);
        let lay = layout(&sizes, &edges);
        assert_eq!(lay.pos.len(), sizes.len());
        check_no_overlap(&sizes, &lay);
        // etwa Bildschirmformat
        let all = (0..sizes.len()).fold(Rect::NOTHING, |r, i| r.union(Rect::from_min_size(lay.pos[i], sizes[i])));
        let ratio = all.width() / all.height();
        assert!((0.3..=4.0).contains(&ratio), "Seitenverhaeltnis {ratio}");
    }

    /// Linien eines grossen Diagramms: rechtwinklig, nie durch eine Tabelle, wenige Kreuzungen
    #[test]
    fn routes_of_a_large_layout_are_clean() {
        use super::ortho::{self, EdgeIn};
        for n in [30usize, 68, 150] {
            let (sizes, edges) = big(n);
            let lay = layout(&sizes, &edges);
            let rects: Vec<Rect> = (0..n).map(|i| Rect::from_min_size(lay.pos[i], sizes[i])).collect();
            let ins: Vec<EdgeIn> = edges
                .iter()
                .enumerate()
                .map(|(i, e)| EdgeIn {
                    child: e.child,
                    parent: e.parent,
                    child_y: lay.pos[e.child].y + e.child_y,
                    parent_y: lay.pos[e.parent].y + e.parent_y,
                    lanes: lay.lanes[i].as_ref().map(|l| l.iter().map(|p| p.y).collect()).unwrap_or_default(),
                })
                .collect();
            let routed = ortho::route(&rects, &ins);
            let mut segs: Vec<(usize, Pos2, Pos2)> = Vec::new();
            for (i, r) in routed.iter().enumerate() {
                let p = &r.points;
                assert!(p.len() >= 2, "Kante {i} ohne Linie");
                for w in p.windows(2) {
                    assert!((w[0].x - w[1].x).abs() < 0.01 || (w[0].y - w[1].y).abs() < 0.01, "Kante {i} nicht rechtwinklig: {p:?}");
                    for (k, r) in rects.iter().enumerate() {
                        let seg = Rect::from_two_pos(w[0], w[1]);
                        let thin = seg.width() < 0.5 && seg.height() < 0.5;
                        assert!(thin || !seg.intersects(r.shrink(1.0)), "Kante {i} laeuft durch Tabelle {k}: {w:?} {r:?}");
                    }
                    segs.push((i, w[0], w[1]));
                }
            }
            // Kreuzungen zwischen senkrechten und waagerechten Stuecken verschiedener Kanten
            let mut crossings = 0;
            for (a, &(ia, a0, a1)) in segs.iter().enumerate() {
                if (a0.x - a1.x).abs() >= 0.01 {
                    continue;
                }
                let (lo, hi) = (a0.y.min(a1.y), a0.y.max(a1.y));
                for &(ib, b0, b1) in &segs {
                    if ia == ib || (b0.y - b1.y).abs() >= 0.01 {
                        continue;
                    }
                    let (l, r) = (b0.x.min(b1.x), b0.x.max(b1.x));
                    if a0.x > l + 0.01 && a0.x < r - 0.01 && b0.y > lo + 0.01 && b0.y < hi - 0.01 {
                        crossings += 1;
                    }
                }
                let _ = a;
            }
            println!("{n} Tabellen, {} Beziehungen: {crossings} Kreuzungen im Linienbild", edges.len());
        }
    }

    /// Handmessung: `cargo test --release layout_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "manual benchmark"]
    fn layout_timing() {
        for n in [30, 100, 300, 1000, 3000, 10_000] {
            let (sizes, edges) = big(n);
            let t = std::time::Instant::now();
            let lay = layout(&sizes, &edges);
            println!("{n} Tabellen, {} Beziehungen: {:?}, {} Kreuzungen", edges.len(), t.elapsed(), lay.crossings);
        }
    }
}
