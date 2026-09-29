// Reverse Engineering: ER-Diagramm einer Datenbank aus den Metadaten erzeugen.
// - automatische, kreuzungsarme Anordnung (Ebenen-Layout mit Fahrspuren fuer lange Linien)
// - Beziehungen per Maus ziehen anlegen, per Rechtsklick loeschen
// - Export als SVG, SQL-Skript (DDL)

use super::{Action, Ctx, TabView};
use crate::db::{ForeignKey, Schema, TableInfo, q};
use crate::style;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use std::collections::HashMap;
use std::fmt::Write as _;

const HEADER_H: f32 = 22.0;
const ROW_H: f32 = 17.0;
const CHAR_W: f32 = 6.8;

const BOX_STROKE: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const HEADER_FILL: Color32 = Color32::from_rgb(0xDC, 0xE8, 0xF5);
const VIEW_FILL: Color32 = Color32::from_rgb(0xE8, 0xF3, 0xE0);
const LINE: Color32 = Color32::from_rgb(0x30, 0x50, 0x90);
const LINE_DIM: Color32 = Color32::from_rgb(0xB8, 0xC4, 0xD8);
const LINE_HI: Color32 = Color32::from_rgb(0xE0, 0x6C, 0x00);
const PK_COLOR: Color32 = Color32::from_rgb(0xB0, 0x80, 0x00);
const FK_COLOR: Color32 = Color32::from_rgb(0x20, 0x60, 0xC0);
const TYPE_COLOR: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);

const RULES: &[&str] = &["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"];

#[derive(Clone, PartialEq)]
enum Drag {
    None,
    Pan,
    Table(String),
    /// Neue Beziehung ziehen: von (Tabelle, Spalte)
    Link(String, String),
}

#[derive(Clone)]
enum Menu {
    Table(String),
    Fk(usize),
    Background,
}

struct LinkDlg {
    table: String,
    column: String,
    target: String,
    on_delete: String,
    on_update: String,
}

/// Eine Linie als Folge kubischer Bezier-Segmente (Weltkoordinaten).
type Path = Vec<[Pos2; 4]>;

pub struct ErTab {
    pub db: String,
    schema: Option<Schema>,
    pos: HashMap<String, Pos2>,
    /// Zwischenpunkte (Fahrspuren) aus der automatischen Anordnung, je Beziehung
    routes: HashMap<String, Vec<Pos2>>,
    offset: Vec2,
    zoom: f32,
    drag: Drag,
    show_types: bool,
    loaded_for: String,
    menu: Option<Menu>,
    link: Option<LinkDlg>,
    fit_pending: bool,
    error: Option<String>,
}

fn layout_file(db: &str) -> std::path::PathBuf {
    let safe: String = db
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    crate::server::app_data_dir().join("layouts").join(format!("{safe}.txt"))
}

fn box_size(t: &TableInfo, show_types: bool) -> Vec2 {
    let mut chars = t.name.chars().count() + 4;
    for c in &t.columns {
        let mut n = c.name.chars().count() + 5;
        if show_types {
            n += c.col_type.chars().count() + 2;
        }
        chars = chars.max(n);
    }
    let w = (chars as f32 * CHAR_W + 22.0).clamp(130.0, 420.0);
    let h = HEADER_H + ROW_H * t.columns.len().max(1) as f32 + 6.0;
    vec2(w, h)
}

fn fk_key(fk: &ForeignKey) -> String {
    format!("{}|{}", fk.table, fk.name)
}

/// Mitte (y) einer Spaltenzeile relativ zur Kastenoberkante.
fn row_y(t: &TableInfo, col: &str) -> f32 {
    let i = t.columns.iter().position(|c| c.name == col).unwrap_or(0);
    HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0
}

fn bezier_between(a: Pos2, b: Pos2, da: f32, db: f32) -> [Pos2; 4] {
    let bend = ((b.x - a.x).abs() * 0.5).max(30.0);
    [a, a + vec2(da * bend, 0.0), b + vec2(db * bend, 0.0), b]
}

fn sample(seg: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let p = seg[0].to_vec2() * (u * u * u)
        + seg[1].to_vec2() * (3.0 * u * u * t)
        + seg[2].to_vec2() * (3.0 * u * t * t)
        + seg[3].to_vec2() * (t * t * t);
    p.to_pos2()
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

impl ErTab {
    pub fn new(db: String) -> Self {
        Self {
            db,
            schema: None,
            pos: HashMap::new(),
            routes: HashMap::new(),
            offset: vec2(20.0, 20.0),
            zoom: 1.0,
            drag: Drag::None,
            show_types: true,
            loaded_for: String::new(),
            menu: None,
            link: None,
            fit_pending: false,
            error: None,
        }
    }

    fn load(&mut self, cx: &mut Ctx) {
        self.loaded_for = self.db.clone();
        self.error = None;
        cx.schemas.invalidate(&self.db);
        self.schema = cx.schema(&self.db);
        if self.schema.is_none() {
            self.error = Some("Struktur konnte nicht gelesen werden.".into());
            return;
        }
        self.pos.clear();
        self.routes.clear();
        if let Ok(text) = std::fs::read_to_string(layout_file(&self.db)) {
            for line in text.lines() {
                let p: Vec<&str> = line.split('\t').collect();
                match p.as_slice() {
                    ["R", key, pts] => {
                        let v: Vec<Pos2> = pts
                            .split(';')
                            .filter_map(|xy| {
                                let (x, y) = xy.split_once(',')?;
                                Some(pos2(x.parse().ok()?, y.parse().ok()?))
                            })
                            .collect();
                        self.routes.insert(key.to_string(), v);
                    }
                    [name, x, y] => {
                        if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                            self.pos.insert(name.to_string(), pos2(x, y));
                        }
                    }
                    _ => {}
                }
            }
        }
        let schema = self.schema.clone().unwrap();
        if self.pos.is_empty() {
            self.auto_layout();
            self.fit_pending = true;
        } else {
            self.place_new_tables(&schema);
            self.fit_pending = true;
        }
    }

    /// Neue Tabellen (ohne gespeicherte Position) rechts daneben platzieren.
    fn place_new_tables(&mut self, schema: &Schema) {
        let mut max_x: f32 = 0.0;
        for t in &schema.tables {
            if let Some(p) = self.pos.get(&t.name) {
                max_x = max_x.max(p.x + box_size(t, self.show_types).x);
            }
        }
        let mut y = 0.0;
        for t in &schema.tables {
            if !self.pos.contains_key(&t.name) {
                self.pos.insert(t.name.clone(), pos2(max_x + 80.0, y));
                y += box_size(t, self.show_types).y + 30.0;
            }
        }
    }

    fn save_layout(&self) {
        let path = layout_file(&self.db);
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let mut s = String::new();
        for (k, v) in &self.pos {
            let _ = writeln!(s, "{k}\t{}\t{}", v.x, v.y);
        }
        for (k, pts) in &self.routes {
            let p: Vec<String> = pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            let _ = writeln!(s, "R\t{k}\t{}", p.join(";"));
        }
        let _ = std::fs::write(path, s);
    }

    /// Automatische Anordnung: Ebenen nach Abhaengigkeit (referenzierte Tabellen links),
    /// Reihenfolge per Schwerpunkt-Verfahren (wenig Kreuzungen), Fahrspuren fuer lange Linien.
    fn auto_layout(&mut self) {
        let Some(schema) = self.schema.clone() else { return };
        self.pos.clear();
        self.routes.clear();
        let show_types = self.show_types;
        let size: HashMap<String, Vec2> = schema
            .tables
            .iter()
            .map(|t| (t.name.clone(), box_size(t, show_types)))
            .collect();

        // Kanten (Kind -> Eltern), ohne Selbstbezuege
        let edges: Vec<(String, String, String)> = schema
            .fks
            .iter()
            .filter(|f| f.table != f.ref_table && size.contains_key(&f.table) && size.contains_key(&f.ref_table))
            .map(|f| (f.table.clone(), f.ref_table.clone(), fk_key(f)))
            .collect();
        let connected: Vec<String> = schema
            .tables
            .iter()
            .filter(|t| edges.iter().any(|e| e.0 == t.name || e.1 == t.name))
            .map(|t| t.name.clone())
            .collect();

        // 1. Ebenen (laengster Pfad)
        let mut level: HashMap<String, usize> = HashMap::new();
        fn lvl(t: &str, edges: &[(String, String, String)], level: &mut HashMap<String, usize>, depth: usize) -> usize {
            if let Some(l) = level.get(t) {
                return *l;
            }
            if depth > 40 {
                return 0;
            }
            let mut l = 0;
            for e in edges.iter().filter(|e| e.0 == t) {
                l = l.max(lvl(&e.1, edges, level, depth + 1) + 1);
            }
            level.insert(t.to_string(), l);
            l
        }
        for n in &connected {
            lvl(n, &edges, &mut level, 0);
        }
        let n_layers = level.values().copied().max().map(|m| m + 1).unwrap_or(0);

        // 2. Knoten je Ebene inkl. Platzhaltern (Fahrspuren) fuer lange Kanten
        #[derive(Clone)]
        enum Node {
            Table(String),
            Lane(String, usize),
        }
        let mut layers: Vec<Vec<Node>> = vec![Vec::new(); n_layers];
        let mut sorted = connected.clone();
        sorted.sort();
        for n in &sorted {
            layers[level[n]].push(Node::Table(n.clone()));
        }
        // Nachbarschaft zwischen benachbarten Ebenen: (Ebene, Index-Schluessel) -> Nachbarn
        let key = |n: &Node| match n {
            Node::Table(t) => format!("T:{t}"),
            Node::Lane(e, i) => format!("L:{e}:{i}"),
        };
        let mut links: Vec<(String, String)> = Vec::new(); // (linker Knoten, rechter Knoten)
        for (child, parent, ek) in &edges {
            let (lc, lp) = (level[child], level[parent]);
            if lc <= lp {
                continue; // Zyklus: ohne Fahrspuren
            }
            let mut prev = format!("T:{parent}");
            for l in lp + 1..lc {
                let node = Node::Lane(ek.clone(), l);
                let k = key(&node);
                layers[l].push(node);
                links.push((prev, k.clone()));
                prev = k;
            }
            links.push((prev, format!("T:{child}")));
        }

        // 3. Reihenfolge per Schwerpunkt (mehrere Durchlaeufe)
        for _ in 0..12 {
            for dir in [true, false] {
                let order: Vec<usize> = if dir { (1..n_layers).collect() } else { (0..n_layers.saturating_sub(1)).rev().collect() };
                for l in order {
                    let other = if dir { l - 1 } else { l + 1 };
                    let idx: HashMap<String, f32> = layers[other]
                        .iter()
                        .enumerate()
                        .map(|(i, n)| (key(n), i as f32))
                        .collect();
                    let mut scored: Vec<(f32, Node)> = layers[l]
                        .iter()
                        .enumerate()
                        .map(|(i, n)| {
                            let k = key(n);
                            let ns: Vec<f32> = links
                                .iter()
                                .filter_map(|(a, b)| {
                                    if *b == k { idx.get(a).copied() } else if *a == k { idx.get(b).copied() } else { None }
                                })
                                .collect();
                            let bc = if ns.is_empty() { i as f32 } else { ns.iter().sum::<f32>() / ns.len() as f32 };
                            (bc, n.clone())
                        })
                        .collect();
                    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    layers[l] = scored.into_iter().map(|(_, n)| n).collect();
                }
            }
        }

        // 4. Koordinaten
        let node_h = |n: &Node| match n {
            Node::Table(t) => size[t].y,
            Node::Lane(..) => 14.0,
        };
        let gap_y = 34.0;
        let mut layer_x = Vec::new();
        let mut layer_w = Vec::new();
        let mut x = 0.0;
        for (l, nodes) in layers.iter().enumerate() {
            let w = nodes
                .iter()
                .map(|n| match n {
                    Node::Table(t) => size[t].x,
                    Node::Lane(..) => 40.0,
                })
                .fold(40.0, f32::max);
            layer_x.push(x);
            layer_w.push(w);
            // Abstand je nach Anzahl der Linien, die diese Luecke kreuzen
            let crossing = links
                .iter()
                .filter(|(a, _)| layers[l].iter().any(|n| key(n) == *a))
                .count() as f32;
            x += w + (90.0 + crossing * 6.0).min(200.0);
        }
        let mut ys: HashMap<String, f32> = HashMap::new(); // Mitte y
        for nodes in &layers {
            let mut y = 0.0;
            for n in nodes {
                let h = node_h(n);
                ys.insert(key(n), y + h / 2.0);
                y += h + gap_y;
            }
        }
        // Verfeinerung: Knoten Richtung Nachbarn schieben, Reihenfolge und Abstaende bleiben
        for _ in 0..8 {
            for nodes in &layers {
                let desired: Vec<f32> = nodes
                    .iter()
                    .map(|n| {
                        let k = key(n);
                        let ns: Vec<f32> = links
                            .iter()
                            .filter_map(|(a, b)| if *b == k { ys.get(a) } else if *a == k { ys.get(b) } else { None })
                            .copied()
                            .collect();
                        if ns.is_empty() { ys[&k] } else { ns.iter().sum::<f32>() / ns.len() as f32 }
                    })
                    .collect();
                // vorwaerts: Mindestabstand einhalten
                let mut placed: Vec<f32> = Vec::new();
                let mut bottom = f32::NEG_INFINITY;
                for (i, n) in nodes.iter().enumerate() {
                    let h = node_h(n);
                    let top = (desired[i] - h / 2.0).max(bottom + gap_y);
                    placed.push(top + h / 2.0);
                    bottom = top + h;
                }
                // rueckwaerts: nach oben ziehen, wo moeglich
                let mut limit = f32::INFINITY;
                for i in (0..nodes.len()).rev() {
                    let h = node_h(&nodes[i]);
                    let max_center = limit - gap_y - h / 2.0;
                    let want = desired[i].min(max_center);
                    placed[i] = placed[i].min(max_center).max(want.min(placed[i]));
                    limit = placed[i] - h / 2.0;
                }
                for (i, n) in nodes.iter().enumerate() {
                    ys.insert(key(n), placed[i]);
                }
            }
        }
        let min_y = layers
            .iter()
            .flat_map(|ns| ns.iter().map(|n| ys[&key(n)] - node_h(n) / 2.0))
            .fold(f32::INFINITY, f32::min);
        let min_y = if min_y.is_finite() { min_y } else { 0.0 };
        let mut max_bottom: f32 = 0.0;
        for (l, nodes) in layers.iter().enumerate() {
            for n in nodes {
                let cy = ys[&key(n)] - min_y;
                match n {
                    Node::Table(t) => {
                        let s = size[t];
                        // rechtsbuendig zur Ebene? Nein: linksbuendig, Linien kommen von rechts
                        self.pos.insert(t.clone(), pos2(layer_x[l], cy - s.y / 2.0));
                        max_bottom = max_bottom.max(cy + s.y / 2.0);
                    }
                    Node::Lane(ek, _) => {
                        let r = self.routes.entry(ek.clone()).or_default();
                        r.push(pos2(layer_x[l] + layer_w[l] + 10.0, cy));
                        r.push(pos2(layer_x[l] - 10.0, cy));
                    }
                }
            }
        }
        // Fahrspuren von rechts (Kind) nach links (Eltern) sortieren
        for r in self.routes.values_mut() {
            r.sort_by(|a, b| b.x.partial_cmp(&a.x).unwrap_or(std::cmp::Ordering::Equal));
        }

        // 5. Tabellen ohne Beziehungen im Raster darunter
        let mut rest: Vec<&TableInfo> = schema.tables.iter().filter(|t| !connected.contains(&t.name)).collect();
        rest.sort_by_key(|t| (t.is_view, t.name.clone()));
        let start_y = if connected.is_empty() { 0.0 } else { max_bottom + 80.0 };
        let total_w = x.max(900.0);
        let (mut cx, mut cy, mut row_h) = (0.0f32, start_y, 0.0f32);
        for t in rest {
            let s = size[&t.name];
            if cx > 0.0 && cx + s.x > total_w {
                cx = 0.0;
                cy += row_h + 40.0;
                row_h = 0.0;
            }
            self.pos.insert(t.name.clone(), pos2(cx, cy));
            cx += s.x + 50.0;
            row_h = row_h.max(s.y);
        }
    }

    fn bounds(&self) -> Option<Rect> {
        let schema = self.schema.as_ref()?;
        let mut r: Option<Rect> = None;
        for t in &schema.tables {
            let p = self.pos.get(&t.name).copied().unwrap_or_default();
            let b = Rect::from_min_size(p, box_size(t, self.show_types));
            r = Some(r.map_or(b, |r| r.union(b)));
        }
        r
    }

    fn fit(&mut self, canvas: Rect) {
        let Some(b) = self.bounds() else { return };
        let margin = 30.0;
        let zx = (canvas.width() - 2.0 * margin) / b.width().max(1.0);
        let zy = (canvas.height() - 2.0 * margin) / b.height().max(1.0);
        self.zoom = zx.min(zy).clamp(0.25, 1.4);
        let content = b.size() * self.zoom;
        let off = (canvas.size() - content) / 2.0;
        self.offset = off - b.min.to_vec2() * self.zoom;
    }

    fn to_world(&self, origin: Pos2, p: Pos2) -> Pos2 {
        ((p - origin - self.offset) / self.zoom).to_pos2()
    }

    fn table_rect_world(&self, t: &TableInfo) -> Rect {
        let p = self.pos.get(&t.name).copied().unwrap_or_default();
        Rect::from_min_size(p, box_size(t, self.show_types))
    }

    /// Linienverlauf einer Beziehung (Weltkoordinaten).
    fn edge_path(&self, schema: &Schema, fk: &ForeignKey) -> Option<Path> {
        let src = schema.table(&fk.table)?;
        let dst = schema.table(&fk.ref_table)?;
        let sr = self.table_rect_world(src);
        let dr = self.table_rect_world(dst);
        let sy = sr.top() + row_y(src, &fk.columns[0]);
        let dy = dr.top() + row_y(dst, &fk.ref_columns[0]);
        if src.name == dst.name {
            let a = pos2(sr.right(), sy);
            let b = pos2(dr.right(), dy);
            let bend = 40.0 + (sy - dy).abs() * 0.2;
            return Some(vec![[a, a + vec2(bend, 0.0), b + vec2(bend, 0.0), b]]);
        }
        // Mit Fahrspuren, wenn die Anordnung noch passt (Kind rechts vom Elternteil)
        if let Some(lanes) = self.routes.get(&fk_key(fk)) {
            if !lanes.is_empty() && dr.right() < lanes.last()?.x + 1.0 && sr.left() > lanes[0].x - 1.0 {
                let mut pts = vec![pos2(sr.left(), sy)];
                pts.extend(lanes.iter().copied());
                pts.push(pos2(dr.right(), dy));
                let mut path = Vec::new();
                for w in pts.windows(2) {
                    if (w[0].y - w[1].y).abs() < 0.5 {
                        path.push([w[0], w[0].lerp(w[1], 0.33), w[0].lerp(w[1], 0.66), w[1]]);
                    } else {
                        path.push(bezier_between(w[0], w[1], -1.0, 1.0));
                    }
                }
                return Some(path);
            }
        }
        // Direkt: passende Seiten waehlen
        let (a, da, b, db) = if sr.left() > dr.right() + 20.0 {
            (pos2(sr.left(), sy), -1.0, pos2(dr.right(), dy), 1.0)
        } else if dr.left() > sr.right() + 20.0 {
            (pos2(sr.right(), sy), 1.0, pos2(dr.left(), dy), -1.0)
        } else if sr.center().x <= dr.center().x {
            (pos2(sr.left(), sy), -1.0, pos2(dr.left(), dy), -1.0)
        } else {
            (pos2(sr.right(), sy), 1.0, pos2(dr.right(), dy), 1.0)
        };
        Some(vec![bezier_between(a, b, da, db)])
    }

    fn remove_routes_of(&mut self, table: &str) {
        if let Some(s) = &self.schema {
            for fk in s.fks.iter().filter(|f| f.table == table || f.ref_table == table) {
                self.routes.remove(&fk_key(fk));
            }
        }
    }

    /// Welche Spalte liegt unter dem Punkt (Weltkoordinaten)?
    fn column_at(&self, schema: &Schema, p: Pos2) -> Option<(String, Option<String>)> {
        for t in schema.tables.iter().rev() {
            let r = self.table_rect_world(t);
            if r.contains(p) {
                let y = p.y - r.top() - HEADER_H - 3.0;
                if y < 0.0 {
                    return Some((t.name.clone(), None));
                }
                let i = (y / ROW_H) as usize;
                return Some((t.name.clone(), t.columns.get(i).map(|c| c.name.clone())));
            }
        }
        None
    }

    fn open_link(&mut self, table: String, column: String, target: String) {
        self.link = Some(LinkDlg {
            table,
            column,
            target,
            on_delete: "RESTRICT".into(),
            on_update: "CASCADE".into(),
        });
    }

    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        let origin = rect.min;
        let Some(schema) = self.schema.clone() else { return };
        if self.fit_pending && rect.width() > 50.0 {
            self.fit_pending = false;
            self.fit(rect);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));

        // Zoom mit Mausrad
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.2, 3.0);
                if let Some(mouse) = hover {
                    let world = (mouse - origin - self.offset) / old;
                    self.offset = mouse - origin - world * self.zoom;
                }
            }
        }

        // Linienverlaeufe (Bildschirm) fuer Zeichnen und Trefferpruefung
        let paths: Vec<Option<Path>> = schema.fks.iter().map(|fk| self.edge_path(&schema, fk)).collect();
        let (off, zm) = (self.offset, self.zoom);
        let screen = move |p: Pos2| origin + off + p.to_vec2() * zm;
        let polylines: Vec<Vec<Pos2>> = paths
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|segs| {
                        segs.iter()
                            .flat_map(|s| (0..=12).map(move |i| sample(s, i as f32 / 12.0)))
                            .map(screen)
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();

        let hit_table = |this: &Self, p: Pos2| -> Option<String> {
            let w = this.to_world(origin, p);
            schema.tables.iter().rev().find(|t| this.table_rect_world(t).contains(w)).map(|t| t.name.clone())
        };
        let hit_fk = |p: Pos2| -> Option<usize> {
            let mut best = None;
            let mut best_d = 6.0;
            for (i, pl) in polylines.iter().enumerate() {
                for w in pl.windows(2) {
                    let d = dist_to_segment(p, w[0], w[1]);
                    if d < best_d {
                        best_d = d;
                        best = Some(i);
                    }
                }
            }
            best
        };
        let hover_table = hover.and_then(|p| hit_table(self, p));
        let hover_fk = if hover_table.is_none() { hover.and_then(hit_fk) } else { None };
        // Anfasser (Punkt rechts neben der Spalte) unter der Maus?
        let handle_at = |this: &Self, p: Pos2| -> Option<(String, String)> {
            let w = this.to_world(origin, p);
            for t in schema.tables.iter().rev().filter(|t| !t.is_view) {
                let r = this.table_rect_world(t);
                let tol = (9.0 / this.zoom).max(6.0);
                if (w.x - (r.right() - 7.0)).abs() <= tol && w.y > r.top() + HEADER_H && w.y < r.bottom() {
                    let i = ((w.y - r.top() - HEADER_H - 3.0) / ROW_H) as usize;
                    if let Some(c) = t.columns.get(i) {
                        return Some((t.name.clone(), c.name.clone()));
                    }
                }
            }
            None
        };

        // Maus: ziehen
        if resp.drag_started() {
            let origin_press = ui.input(|i| i.pointer.press_origin()).or(pointer);
            self.drag = match origin_press {
                Some(p) => {
                    if let Some((t, c)) = handle_at(self, p) {
                        Drag::Link(t, c)
                    } else if let Some(t) = hit_table(self, p) {
                        Drag::Table(t)
                    } else {
                        Drag::Pan
                    }
                }
                None => Drag::None,
            };
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match self.drag.clone() {
                Drag::Table(name) => {
                    if let Some(p) = self.pos.get_mut(&name) {
                        *p += d / self.zoom;
                    }
                    self.remove_routes_of(&name);
                }
                Drag::Pan => self.offset += d,
                _ => {}
            }
        }
        if resp.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Table(_) => self.save_layout(),
                Drag::Link(t, c) => {
                    if let Some(p) = hover.or(pointer) {
                        let w = self.to_world(origin, p);
                        if let Some((rt, rc)) = self.column_at(&schema, w) {
                            let rc = rc.or_else(|| schema.table(&rt).and_then(|ti| ti.pk_columns().first().cloned()));
                            if let Some(rc) = rc {
                                if !(rt == t && rc == c) {
                                    self.open_link(t, c, format!("{rt}.{rc}"));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if resp.double_clicked() {
            if let Some(t) = pointer.and_then(|p| hit_table(self, p)) {
                cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: t });
            }
        }
        if resp.secondary_clicked() {
            self.menu = Some(match pointer {
                Some(p) => {
                    if let Some(t) = hit_table(self, p) {
                        Menu::Table(t)
                    } else if let Some(i) = hit_fk(p) {
                        Menu::Fk(i)
                    } else {
                        Menu::Background
                    }
                }
                None => Menu::Background,
            });
        }

        let painter = painter.with_clip_rect(rect);
        // Nach den Maus-Aktionen mit aktuellem Zoom/Versatz zeichnen
        let z = self.zoom;
        let (off, zm) = (self.offset, self.zoom);
        let screen = move |p: Pos2| origin + off + p.to_vec2() * zm;
        let paths: Vec<Option<Path>> = schema.fks.iter().map(|fk| self.edge_path(&schema, fk)).collect();

        // Hintergrundraster (dezent)
        let grid = 40.0 * z;
        if grid > 12.0 {
            let dot = Color32::from_gray(0xE4);
            let start = origin + vec2(self.offset.x.rem_euclid(grid), self.offset.y.rem_euclid(grid));
            let mut x = start.x;
            while x < rect.right() {
                let mut y = start.y;
                while y < rect.bottom() {
                    painter.circle_filled(pos2(x, y), 1.0, dot);
                    y += grid;
                }
                x += grid;
            }
        }

        // Beziehungslinien
        let focus_table = match &self.drag {
            Drag::Table(t) => Some(t.clone()),
            _ => hover_table.clone(),
        };
        for (i, fk) in schema.fks.iter().enumerate() {
            let Some(path) = &paths[i] else { continue };
            let related = focus_table.as_ref().map(|t| *t == fk.table || *t == fk.ref_table);
            let is_hover = hover_fk == Some(i) || matches!(self.menu, Some(Menu::Fk(j)) if j == i);
            let (color, width) = if is_hover || related == Some(true) {
                (LINE_HI, 2.2)
            } else if related == Some(false) {
                (LINE_DIM, 1.2)
            } else {
                (LINE, 1.4)
            };
            let stroke = Stroke::new((width * z).max(1.0), color);
            for seg in path {
                let s = seg.map(screen);
                painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
                    s,
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                )));
            }
            // Kraehenfuss ("viele") am Kind, zwei Striche ("genau eins") am Elternteil
            let first = path[0];
            let last = path[path.len() - 1];
            let a = screen(first[0]);
            let da = (first[1].x - first[0].x).signum();
            let b = screen(last[3]);
            let db = (last[2].x - last[3].x).signum();
            let f = a + vec2(da * 11.0 * z, 0.0);
            painter.line_segment([f, a + vec2(0.0, -6.0 * z)], stroke);
            painter.line_segment([f, a + vec2(0.0, 6.0 * z)], stroke);
            painter.line_segment([f, a], stroke);
            for k in [6.0, 10.0] {
                let p = b + vec2(db * k * z, 0.0);
                painter.line_segment([p + vec2(0.0, -5.0 * z), p + vec2(0.0, 5.0 * z)], stroke);
            }
        }

        // Tabellen
        let font = FontId::proportional(12.0 * z);
        let bold = FontId::proportional(12.5 * z);
        let small = FontId::proportional(9.5 * z);
        for t in &schema.tables {
            let r = Rect::from_min_size(screen(self.table_rect_world(t).min), box_size(t, self.show_types) * z);
            if !r.intersects(rect) {
                continue;
            }
            let focused = focus_table.as_deref() == Some(t.name.as_str());
            painter.rect_filled(r.translate(vec2(3.0, 3.0) * z), 0.0, Color32::from_black_alpha(22));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            let head = Rect::from_min_size(r.min, vec2(r.width(), HEADER_H * z));
            painter.rect_filled(head, 0.0, if t.is_view { VIEW_FILL } else { HEADER_FILL });
            painter.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, BOX_STROKE));
            let title = if t.is_view { format!("{} (Sicht)", t.name) } else { t.name.clone() };
            painter.text(head.left_center() + vec2(6.0 * z, 0.0), Align2::LEFT_CENTER, title, bold.clone(), Color32::BLACK);
            for (i, c) in t.columns.iter().enumerate() {
                let y = r.top() + (HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0) * z;
                let is_fk = schema.is_fk_column(&t.name, &c.name);
                let x0 = r.left() + 5.0 * z;
                if c.is_pk() {
                    painter.text(pos2(x0, y), Align2::LEFT_CENTER, "PK", small.clone(), PK_COLOR);
                } else if is_fk {
                    painter.text(pos2(x0, y), Align2::LEFT_CENTER, "FK", small.clone(), FK_COLOR);
                }
                let mut label = c.name.clone();
                if !c.nullable && !c.is_pk() {
                    label.push_str(" *");
                }
                let nf = if c.is_pk() { bold.clone() } else { font.clone() };
                painter.text(pos2(x0 + 22.0 * z, y), Align2::LEFT_CENTER, label, nf, Color32::from_gray(0x20));
                if self.show_types {
                    painter.text(pos2(r.right() - 16.0 * z, y), Align2::RIGHT_CENTER, &c.col_type, font.clone(), TYPE_COLOR);
                }
                // Anfasser zum Verknuepfen (nur bei Maus ueber der Tabelle)
                if focused && !t.is_view && matches!(self.drag, Drag::None) {
                    painter.circle(pos2(r.right() - 7.0 * z, y), 3.5 * z, Color32::WHITE, Stroke::new(1.2, LINE_HI));
                }
            }
            let stroke = if focused { Stroke::new(1.8, LINE_HI) } else { Stroke::new(1.0, BOX_STROKE) };
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
        }

        // Neue Beziehung: gestrichelte Linie zur Maus
        if let Drag::Link(t, c) = &self.drag {
            if let (Some(ti), Some(p)) = (schema.table(t), hover.or(pointer)) {
                let r = self.table_rect_world(ti);
                let a = screen(pos2(r.right() - 7.0, r.top() + row_y(ti, c)));
                painter.add(Shape::dashed_line(&[a, p], Stroke::new(2.0, LINE_HI), 6.0, 4.0));
                painter.circle_filled(a, 4.0, LINE_HI);
                let w = self.to_world(origin, p);
                let tip = match self.column_at(&schema, w) {
                    Some((rt, Some(rc))) => format!("{t}.{c}  →  {rt}.{rc}"),
                    Some((rt, None)) => format!("{t}.{c}  →  {rt} (Primärschlüssel)"),
                    None => "Auf die Zielspalte ziehen (meist id)".into(),
                };
                painter.text(p + vec2(12.0, 12.0), Align2::LEFT_TOP, tip, FontId::proportional(12.0), LINE_HI);
            }
        }

        // Tooltip fuer Linie
        if let (Some(i), Drag::None) = (hover_fk, &self.drag) {
            let fk = &schema.fks[i];
            resp.clone().on_hover_text(format!(
                "{}\n{}.{}  →  {}.{}\nON DELETE {} / ON UPDATE {}\n(Rechtsklick zum Löschen)",
                fk.name,
                fk.table,
                fk.columns.join(","),
                fk.ref_table,
                fk.ref_columns.join(","),
                fk.on_delete,
                fk.on_update
            ));
        }

        if schema.tables.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Diese Datenbank enthält noch keine Tabellen.", FontId::proportional(14.0), Color32::GRAY);
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, style::SHADOW), StrokeKind::Inside);

        // Kontextmenue
        let menu = self.menu.clone();
        let db = self.db.clone();
        let mut close_menu = false;
        resp.context_menu(|ui| match &menu {
            Some(Menu::Table(t)) => {
                ui.label(RichText::new(t).strong());
                if ui.button("Daten anzeigen").clicked() {
                    cx.actions.push(Action::OpenData { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button("Struktur bearbeiten").clicked() {
                    cx.actions.push(Action::OpenStructure { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button("SELECT-Abfrage erzeugen").clicked() {
                    cx.actions.push(Action::OpenSql { db: Some(db.clone()), sql: format!("SELECT * FROM {} LIMIT 100;", q(t)), run: true });
                    ui.close();
                }
                if ui.button("Beziehung von hier anlegen...").clicked() {
                    let col = schema.table(t).and_then(|ti| ti.columns.iter().find(|c| !c.is_pk()).or(ti.columns.first())).map(|c| c.name.clone()).unwrap_or_default();
                    self.open_link(t.clone(), col, String::new());
                    ui.close();
                }
            }
            Some(Menu::Fk(i)) => {
                if let Some(fk) = schema.fks.get(*i) {
                    ui.label(RichText::new(format!("{}.{} → {}.{}", fk.table, fk.columns.join(","), fk.ref_table, fk.ref_columns.join(","))).strong());
                    if ui.button("Beziehung löschen...").clicked() {
                        cx.actions.push(Action::Confirm {
                            text: format!("Beziehung \"{}\" löschen?", fk.name),
                            db: Some(db.clone()),
                            sql: format!("ALTER TABLE {} DROP FOREIGN KEY {}", q(&fk.table), q(&fk.name)),
                        });
                        close_menu = true;
                        ui.close();
                    }
                    if ui.button("Struktur von ".to_string() + &fk.table).clicked() {
                        cx.actions.push(Action::OpenStructure { db: db.clone(), table: fk.table.clone() });
                        ui.close();
                    }
                }
            }
            _ => {
                if ui.button("Automatisch anordnen").clicked() {
                    self.auto_layout();
                    self.fit_pending = true;
                    self.save_layout();
                    ui.close();
                }
                if ui.button("Alles anzeigen").clicked() {
                    self.fit_pending = true;
                    ui.close();
                }
                if ui.button("Neue Tabelle...").clicked() {
                    cx.actions.push(Action::NewTable(db.clone()));
                    ui.close();
                }
                if ui.button("Neue Beziehung...").clicked() {
                    self.open_link(String::new(), String::new(), String::new());
                    ui.close();
                }
            }
        });
        if close_menu {
            self.menu = None;
        }
    }

    fn link_dialog(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(schema) = self.schema.clone() else { return };
        let Some(dlg) = self.link.as_mut() else { return };
        let mut ok = false;
        let mut cancel = false;
        let tables: Vec<String> = schema.tables.iter().filter(|t| !t.is_view).map(|t| t.name.clone()).collect();
        let cols: Vec<String> = schema
            .table(&dlg.table)
            .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let targets: Vec<String> = schema
            .tables
            .iter()
            .filter(|t| !t.is_view)
            .flat_map(|t| t.columns.iter().filter(|c| c.is_pk() || c.key == "UNI").map(move |c| format!("{}.{}", t.name, c.name)))
            .collect();
        egui::Window::new("Beziehung (Fremdschlüssel) anlegen")
            .id(egui::Id::new(("erlink", &self.db)))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("erlinkgrid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("Tabelle (viele):");
                    if super::str_combo(ui, "erl_t", &tables, &mut dlg.table, 180.0) {
                        dlg.column.clear();
                    }
                    ui.end_row();
                    ui.label("Spalte:");
                    super::str_combo(ui, "erl_c", &cols, &mut dlg.column, 180.0);
                    ui.end_row();
                    ui.label("verweist auf (eins):");
                    super::str_combo(ui, "erl_r", &targets, &mut dlg.target, 180.0);
                    ui.end_row();
                    let rules: Vec<String> = RULES.iter().map(|s| s.to_string()).collect();
                    ui.label("Beim Löschen:");
                    super::str_combo(ui, "erl_d", &rules, &mut dlg.on_delete, 120.0);
                    ui.end_row();
                    ui.label("Beim Ändern:");
                    super::str_combo(ui, "erl_u", &rules, &mut dlg.on_update, 120.0);
                    ui.end_row();
                });
                // Typen vergleichen
                let ty = |t: &str, c: &str| {
                    schema.table(t).and_then(|ti| ti.columns.iter().find(|x| x.name == c)).map(|x| x.col_type.clone())
                };
                if let (Some(a), Some((rt, rc))) = (ty(&dlg.table, &dlg.column), dlg.target.split_once('.')) {
                    if let Some(b) = ty(rt, rc) {
                        if a != b {
                            ui.label(RichText::new(format!("Hinweis: Datentypen unterscheiden sich ({a} / {b}).")).color(style::ERROR_TEXT).small());
                        }
                    }
                }
                ui.label(RichText::new("Beispiel: schueler.klasse_id verweist auf klasse.id").small().color(style::NULL_TEXT));
                ui.separator();
                ui.horizontal(|ui| {
                    let valid = !dlg.table.is_empty() && !dlg.column.is_empty() && dlg.target.contains('.');
                    if ui.add_enabled(valid, egui::Button::new("Anlegen")).clicked() {
                        ok = true;
                    }
                    if ui.button("Abbrechen").clicked() {
                        cancel = true;
                    }
                });
            });
        if ok {
            let d = self.link.take().unwrap();
            let (rt, rc) = d.target.split_once('.').unwrap();
            let sql = format!(
                "ALTER TABLE {}.{} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}.{} ({}) ON DELETE {} ON UPDATE {}",
                q(&self.db),
                q(&d.table),
                q(&format!("fk_{}_{}", d.table, d.column)),
                q(&d.column),
                q(&self.db),
                q(rt),
                q(rc),
                d.on_delete,
                d.on_update
            );
            if let Some(dbc) = cx.db {
                match dbc.exec(&sql, ()) {
                    Ok(_) => {
                        cx.status(format!("Beziehung {}.{} → {} angelegt.", d.table, d.column, d.target));
                        cx.actions.push(Action::SchemaChanged(self.db.clone()));
                    }
                    Err(e) => {
                        cx.error(format!("{e}\n\nTipp: Beide Spalten brauchen den gleichen Datentyp, und vorhandene Werte müssen in der Zieltabelle existieren.\n\nSQL: {sql}"));
                        self.link = Some(d);
                    }
                }
            }
        } else if cancel {
            self.link = None;
        }
    }

    fn to_svg(&self) -> String {
        let Some(schema) = &self.schema else { return String::new() };
        let Some(b) = self.bounds() else { return String::new() };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let pad = 30.0;
        let mut o = String::new();
        let _ = writeln!(
            o,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" font-family="Segoe UI, Arial, sans-serif" font-size="12">"#,
            b.width() + 2.0 * pad,
            b.height() + 2.0 * pad
        );
        let _ = writeln!(o, r#"<rect width="100%" height="100%" fill="white"/><g transform="translate({},{})">"#, pad - b.min.x, pad - b.min.y);
        for fk in &schema.fks {
            let Some(path) = self.edge_path(schema, fk) else { continue };
            let mut d = format!("M{} {}", path[0][0].x, path[0][0].y);
            for s in &path {
                let _ = write!(d, " C{} {} {} {} {} {}", s[1].x, s[1].y, s[2].x, s[2].y, s[3].x, s[3].y);
            }
            let _ = writeln!(o, r##"<path d="{d}" fill="none" stroke="#305090" stroke-width="1.4"/>"##);
            let a = path[0][0];
            let da = (path[0][1].x - a.x).signum();
            let last = path[path.len() - 1];
            let bp = last[3];
            let db = (last[2].x - bp.x).signum();
            let f = a.x + da * 11.0;
            let _ = writeln!(o, r##"<path d="M{f} {} L{} {} M{f} {} L{} {} M{f} {} L{} {}" stroke="#305090" stroke-width="1.4"/>"##, a.y, a.x, a.y - 6.0, a.y, a.x, a.y + 6.0, a.y, a.x, a.y);
            for k in [6.0, 10.0] {
                let x = bp.x + db * k;
                let _ = writeln!(o, r##"<line x1="{x}" y1="{}" x2="{x}" y2="{}" stroke="#305090" stroke-width="1.4"/>"##, bp.y - 5.0, bp.y + 5.0);
            }
        }
        for t in &schema.tables {
            let r = self.table_rect_world(t);
            let (p, s) = (r.min, r.size());
            let head = if t.is_view { "#E8F3E0" } else { "#DCE8F5" };
            let _ = writeln!(o, r##"<rect x="{}" y="{}" width="{}" height="{}" fill="white" stroke="#606060"/>"##, p.x, p.y, s.x, s.y);
            let _ = writeln!(o, r##"<rect x="{}" y="{}" width="{}" height="{HEADER_H}" fill="{head}" stroke="#606060"/>"##, p.x, p.y, s.x);
            let _ = writeln!(o, r#"<text x="{}" y="{}" font-weight="bold">{}</text>"#, p.x + 6.0, p.y + 15.0, esc(&t.name));
            for (i, c) in t.columns.iter().enumerate() {
                let y = p.y + HEADER_H + 3.0 + ROW_H * i as f32 + 12.5;
                if c.is_pk() {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" font-size="9" fill="#B08000">PK</text>"##, p.x + 5.0);
                } else if schema.is_fk_column(&t.name, &c.name) {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" font-size="9" fill="#2060C0">FK</text>"##, p.x + 5.0);
                }
                let w = if c.is_pk() { " font-weight=\"bold\"" } else { "" };
                let _ = writeln!(o, r#"<text x="{}" y="{y}"{w}>{}</text>"#, p.x + 27.0, esc(&c.name));
                if self.show_types {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" text-anchor="end" fill="#707070">{}</text>"##, p.x + s.x - 16.0, esc(&c.col_type));
                }
            }
        }
        o.push_str("</g></svg>\n");
        o
    }
}

impl TabView for ErTab {
    fn title(&self) -> String {
        format!("ER-Diagramm: {}", self.db)
    }

    fn key(&self) -> Option<String> {
        Some(format!("er:{}", self.db))
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.load(cx);
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db {
            let pos = self.pos.clone();
            let routes = self.routes.clone();
            cx.schemas.invalidate(&self.db);
            self.schema = cx.schema(&self.db);
            self.pos = pos;
            self.routes = routes;
            if let Some(s) = self.schema.clone() {
                self.pos.retain(|k, _| s.table(k).is_some());
                self.place_new_tables(&s);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if self.loaded_for != self.db {
            self.load(cx);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Datenbank:");
            let mut d = self.db.clone();
            if super::db_combo(ui, "erdb", cx.databases, &mut d) {
                self.db = d;
            }
            if ui.button("Neu einlesen").on_hover_text("Struktur erneut aus der Datenbank lesen (Reverse Engineering)").clicked() {
                self.schema_changed(&self.db.clone(), cx);
            }
            if ui
                .button("Automatisch anordnen")
                .on_hover_text("Tabellen übersichtlich anordnen, möglichst ohne Linienkreuzungen")
                .clicked()
            {
                self.auto_layout();
                self.fit_pending = true;
                self.save_layout();
            }
            if ui.button("Alles anzeigen").on_hover_text("Zoom so wählen, dass das ganze Diagramm sichtbar ist").clicked() {
                self.fit_pending = true;
            }
            if ui.button("+ Beziehung").on_hover_text("Fremdschlüssel zwischen zwei Tabellen anlegen").clicked() {
                self.open_link(String::new(), String::new(), String::new());
            }
            ui.checkbox(&mut self.show_types, "Datentypen");
            ui.separator();
            if ui.button("−").clicked() {
                self.zoom = (self.zoom / 1.2).max(0.2);
            }
            ui.label(format!("{:.0}\u{a0}%", self.zoom * 100.0));
            if ui.button("+").clicked() {
                self.zoom = (self.zoom * 1.2).min(3.0);
            }
            ui.separator();
            if ui.button("Als SVG speichern...").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("SVG-Grafik", &["svg"])
                    .set_file_name(format!("{}-er-diagramm.svg", self.db))
                    .save_file()
                {
                    match std::fs::write(&p, self.to_svg()) {
                        Ok(_) => cx.status(format!("Diagramm gespeichert: {}", p.display())),
                        Err(e) => cx.error(e.to_string()),
                    }
                }
            }
            if ui.button("SQL-Skript erzeugen").on_hover_text("CREATE-Anweisungen aller Tabellen (DDL)").clicked() {
                if let Some(dbc) = cx.db {
                    match dbc.dump(&self.db, false) {
                        Ok(sql) => cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql, run: false }),
                        Err(e) => cx.error(e),
                    }
                }
            }
            if ui.button("Neue Tabelle").clicked() {
                cx.actions.push(Action::NewTable(self.db.clone()));
            }
        });
        if let Some(s) = &self.schema {
            ui.label(
                RichText::new(format!(
                    "{} Tabellen, {} Beziehungen  –  Tabelle ziehen = verschieben · Hintergrund ziehen = Ansicht bewegen · Mausrad = Zoom · \
                     am Punkt ● neben einer Spalte ziehen = Beziehung anlegen · Rechtsklick = Menü (auch auf Linien)",
                    s.tables.len(),
                    s.fks.len()
                ))
                .small()
                .color(style::NULL_TEXT),
            );
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(style::ERROR_TEXT));
        }
        self.draw(ui, cx);
        let ctx = ui.ctx().clone();
        self.link_dialog(&ctx, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ColumnInfo;

    fn table(name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo { name: c.to_string(), col_type: "int(11)".into(), key: if *c == "id" { "PRI".into() } else { String::new() }, ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    fn fk(t: &str, c: &str, rt: &str) -> ForeignKey {
        ForeignKey { name: format!("fk_{t}_{c}"), table: t.into(), columns: vec![c.into()], ref_table: rt.into(), ref_columns: vec!["id".into()], ..Default::default() }
    }

    #[test]
    fn layout_no_overlap_and_lanes() {
        let schema = Schema {
            name: "x".into(),
            tables: vec![
                table("klasse", &["id", "name"]),
                table("lehrer", &["id", "name"]),
                table("schueler", &["id", "klasse_id"]),
                table("note", &["id", "schueler_id", "lehrer_id"]),
                table("einsam", &["id"]),
            ],
            fks: vec![fk("schueler", "klasse_id", "klasse"), fk("note", "schueler_id", "schueler"), fk("note", "lehrer_id", "lehrer")],
        };
        let mut er = ErTab::new("x".into());
        er.schema = Some(schema.clone());
        er.auto_layout();
        // keine Ueberlappungen
        let rects: Vec<Rect> = schema.tables.iter().map(|t| er.table_rect_world(t)).collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(!rects[i].intersects(rects[j]), "{} / {}", schema.tables[i].name, schema.tables[j].name);
            }
        }
        // Eltern links vom Kind
        assert!(er.pos["klasse"].x < er.pos["schueler"].x);
        assert!(er.pos["schueler"].x < er.pos["note"].x);
        // note -> lehrer ueberspringt eine Ebene: Fahrspur vorhanden, Linie trifft keine Tabelle
        let lane_fk = &schema.fks[2];
        assert!(er.routes.contains_key(&fk_key(lane_fk)));
        let path = er.edge_path(&schema, lane_fk).unwrap();
        let schueler = er.table_rect_world(schema.table("schueler").unwrap());
        for seg in &path {
            for i in 0..=20 {
                assert!(!schueler.contains(sample(seg, i as f32 / 20.0)));
            }
        }
        // Tabelle ohne Beziehung unterhalb
        let max_connected = ["klasse", "lehrer", "schueler", "note"].iter().map(|t| er.table_rect_world(schema.table(t).unwrap()).bottom()).fold(0.0, f32::max);
        assert!(er.pos["einsam"].y > max_connected);
    }
}
