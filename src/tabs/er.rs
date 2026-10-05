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

const LINE_HI: Color32 = Color32::from_rgb(0xE0, 0x6C, 0x00);
const PK_COLOR: Color32 = Color32::from_rgb(0xB0, 0x80, 0x00);
const FK_COLOR: Color32 = Color32::from_rgb(0x20, 0x60, 0xC0);

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

#[derive(Clone, Copy, PartialEq)]
enum LinkKind {
    OneToMany,
    OneToOne,
    ManyToMany,
}

struct LinkDlg {
    kind: LinkKind,
    table: String,
    column: String,
    target: String,
    on_delete: String,
    on_update: String,
    /// n:m: zweite Tabelle und Name der Zwischentabelle (leer = automatisch)
    other: String,
    junction: String,
}

/// Eine Linie als Folge kubischer Bezier-Segmente (Weltkoordinaten).
type Path = Vec<[Pos2; 4]>;

/// Darstellung der Kardinalitaeten
#[derive(Clone, Copy, PartialEq, Debug)]
enum Notation {
    /// Kraehenfuss (Information Engineering), Standard
    Crow,
    /// Chen: 1 / n / m an den Linienenden
    Chen,
    /// (min,max)-Notation
    MinMax,
}

impl Notation {
    const ALL: [Notation; 3] = [Notation::Crow, Notation::Chen, Notation::MinMax];

    fn label(self) -> &'static str {
        match self {
            Notation::Crow => "Krähenfuß",
            Notation::Chen => "Chen (1, n, m)",
            Notation::MinMax => "(min,max)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Notation::Crow => "crow",
            Notation::Chen => "chen",
            Notation::MinMax => "minmax",
        }
    }
}

/// Art einer gezeichneten Beziehung. Linie laeuft immer vom Kind (Anfang) zum Elternteil (Ende),
/// bei n:m von der ersten zur zweiten Tabelle.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Rel {
    /// 1:n (sonst 1:1)
    many: bool,
    /// n:m ueber eine (ausgeblendete) Zwischentabelle
    nm: bool,
    /// Fremdschluessel darf NULL sein (Kind muss keinen Elternteil haben)
    optional: bool,
}

impl Rel {
    fn label(self) -> &'static str {
        if self.nm {
            "n:m"
        } else if self.many {
            "1:n"
        } else {
            "1:1"
        }
    }
}

/// Zeichenelement einer Beziehungsmarkierung (Weltkoordinaten)
#[derive(Clone, Debug, PartialEq)]
enum Prim {
    Line(Pos2, Pos2),
    Circle(Pos2, f32),
    /// kleine Beschriftung am Linienende, oberhalb der Linie
    Text(Pos2, String, Align2),
    /// umrahmte Beschriftung in der Linienmitte
    Tag(Pos2, String),
    /// Raute (Chen) in der Linienmitte
    Diamond(Pos2, f32),
}

/// Mittelpunkt eines Linienverlaufs
fn path_mid(path: &Path) -> Pos2 {
    if path.len() % 2 == 1 {
        sample(&path[path.len() / 2], 0.5)
    } else {
        path[path.len() / 2][0]
    }
}

/// Markierungen fuer eine Beziehung: Anfang = Kind (bzw. erste n:m-Tabelle), Ende = Elternteil.
fn markers(path: &Path, rel: Rel, notation: Notation, labels: bool) -> Vec<Prim> {
    let first = path[0];
    let last = path[path.len() - 1];
    let a = first[0];
    let da = (first[1].x - first[0].x).signum();
    let b = last[3];
    let db = (last[2].x - last[3].x).signum();
    let at = |p: Pos2, d: f32, k: f32| p + vec2(d * k, 0.0);
    let bar = |p: Pos2, d: f32, k: f32| {
        let c = at(p, d, k);
        Prim::Line(c + vec2(0.0, -5.0), c + vec2(0.0, 5.0))
    };
    let foot = |p: Pos2, d: f32| {
        let f = at(p, d, 11.0);
        vec![Prim::Line(f, p + vec2(0.0, -6.0)), Prim::Line(f, p + vec2(0.0, 6.0))]
    };
    let text = |p: Pos2, d: f32, s: &str| {
        let align = if d >= 0.0 { Align2::LEFT_BOTTOM } else { Align2::RIGHT_BOTTOM };
        Prim::Text(at(p, d, 6.0) + vec2(0.0, -3.0), s.to_string(), align)
    };
    let mut out = Vec::new();
    match notation {
        Notation::Crow => {
            // Anfang (Kind): viele = Kraehenfuss, eins = Strich
            if rel.many || rel.nm {
                out.extend(foot(a, da));
            } else {
                out.push(bar(a, da, 8.0));
            }
            // Ende (Elternteil): genau eins = zwei Striche, keins oder eins = Strich + Kreis
            if rel.nm {
                out.extend(foot(b, db));
            } else if rel.optional {
                out.push(bar(b, db, 6.0));
                out.push(Prim::Circle(at(b, db, 14.0), 3.5));
            } else {
                out.push(bar(b, db, 6.0));
                out.push(bar(b, db, 10.0));
            }
            if labels {
                out.push(Prim::Tag(path_mid(path), rel.label().into()));
            }
        }
        Notation::Chen => {
            let (sa, sb) = if rel.nm { ("n", "m") } else if rel.many { ("n", "1") } else { ("1", "1") };
            out.push(text(a, da, sa));
            out.push(text(b, db, sb));
            out.push(Prim::Diamond(path_mid(path), 7.0));
        }
        Notation::MinMax => {
            // (min,max): an wie vielen Beziehungen nimmt ein Datensatz dieser Tabelle teil?
            let (sa, sb) = if rel.nm {
                ("(0,n)", "(0,n)")
            } else {
                (if rel.optional { "(0,1)" } else { "(1,1)" }, if rel.many { "(0,n)" } else { "(0,1)" })
            };
            out.push(text(a, da, sa));
            out.push(text(b, db, sb));
        }
    }
    out
}

/// Beschriftungen an Linienenden ohne Ueberlappung setzen (Weltkoordinaten): gleiche Texte
/// am selben Ort werden zusammengefasst, verschiedene nach aussen weggeschoben.
fn place_texts<T: Clone>(items: Vec<(Pos2, String, Align2, T)>) -> Vec<(Pos2, String, Align2, T)> {
    let mut placed: Vec<(Rect, String)> = Vec::new();
    let mut out = Vec::new();
    'items: for (mut p, s, align, extra) in items {
        let w = s.chars().count() as f32 * 6.0 + 2.0;
        let left = align == Align2::LEFT_BOTTOM;
        let rect_at = |p: Pos2| {
            if left {
                Rect::from_min_max(pos2(p.x, p.y - 11.0), pos2(p.x + w, p.y))
            } else {
                Rect::from_min_max(pos2(p.x - w, p.y - 11.0), pos2(p.x, p.y))
            }
        };
        for _ in 0..8 {
            let r = rect_at(p);
            let Some((o, os)) = placed.iter().find(|(o, _)| o.intersects(r)) else { break };
            if *os == s {
                continue 'items;
            }
            p.x += if left { o.max.x + 3.0 - r.min.x } else { o.min.x - 3.0 - r.max.x };
        }
        placed.push((rect_at(p), s.clone()));
        out.push((p, s, align, extra));
    }
    out
}

fn view_file() -> std::path::PathBuf {
    crate::workspace::internal_dir().join("layouts").join("ansicht.txt")
}

pub struct ErTab {
    pub db: String,
    /// Struktur wie in der Datenbank
    raw: Option<Schema>,
    /// Angezeigte Struktur (ggf. ohne Zwischentabellen, dafuer mit n:m-Linien)
    schema: Option<Schema>,
    /// n:m-Linien der Ansicht: Schluessel -> Zwischentabelle
    nm: HashMap<String, String>,
    notation: Notation,
    collapse_nm: bool,
    rel_labels: bool,
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
    /// Ausgewaehlte Beziehung (fk_key), Entf loescht sie
    selected: Option<String>,
    /// Verbinden: Ziehen von einer beliebigen Spalte legt eine Beziehung an
    connect_mode: bool,
}

fn layout_file(db: &str) -> std::path::PathBuf {
    let safe: String = db
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    crate::workspace::internal_dir().join("layouts").join(format!("{safe}.txt"))
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

/// SQL fuer eine neue Beziehung. Err("") = noch unvollstaendig, Err(text) = Problem.
fn link_sql(db: &str, schema: &Schema, d: &LinkDlg) -> Result<String, String> {
    if d.kind == LinkKind::ManyToMany {
        if d.table.is_empty() || d.other.is_empty() {
            return Err(String::new());
        }
        let key = |name: &str| -> Result<(String, String), String> {
            let t = schema.table(name).ok_or_else(String::new)?;
            let pk = t.pk_columns();
            if pk.len() != 1 {
                return Err(format!("„{name}“ braucht einen Primärschlüssel aus genau einer Spalte (z. B. id)."));
            }
            let ty = t.columns.iter().find(|c| c.name == pk[0]).map(|c| c.col_type.clone()).unwrap_or_else(|| "INT".into());
            Ok((pk[0].clone(), ty))
        };
        let (pa, ta) = key(&d.table)?;
        let (pb, tb) = key(&d.other)?;
        let junction = if d.junction.trim().is_empty() { format!("{}_{}", d.table, d.other) } else { d.junction.trim().to_string() };
        if schema.table(&junction).is_some() {
            return Err(format!("Eine Tabelle „{junction}“ gibt es schon – bitte einen anderen Namen wählen."));
        }
        let ca = format!("{}_{}", d.table, pa);
        let mut cb = format!("{}_{}", d.other, pb);
        if cb == ca {
            cb.push('2');
        }
        return Ok(format!(
            "CREATE TABLE {db}.{j} (\n  {ca} {ta} NOT NULL,\n  {cb} {tb} NOT NULL,\n  PRIMARY KEY ({ca}, {cb}),\n  \
             CONSTRAINT {fa} FOREIGN KEY ({ca}) REFERENCES {db}.{a} ({pa}) ON DELETE CASCADE ON UPDATE CASCADE,\n  \
             CONSTRAINT {fb} FOREIGN KEY ({cb}) REFERENCES {db}.{b} ({pb}) ON DELETE CASCADE ON UPDATE CASCADE\n) ENGINE=InnoDB",
            db = q(db),
            j = q(&junction),
            ca = q(&ca),
            cb = q(&cb),
            fa = q(&format!("fk_{junction}_{ca}")),
            fb = q(&format!("fk_{junction}_{cb}")),
            a = q(&d.table),
            b = q(&d.other),
            pa = q(&pa),
            pb = q(&pb),
        ));
    }
    let Some((rt, rc)) = d.target.split_once('.') else { return Err(String::new()) };
    if d.table.is_empty() || d.column.is_empty() {
        return Err(String::new());
    }
    let already_unique = schema.unique_keys.iter().any(|(t, k)| *t == d.table && k.len() == 1 && k[0] == d.column);
    let unique = if d.kind == LinkKind::OneToOne && !already_unique {
        format!("ADD UNIQUE KEY {} ({}), ", q(&format!("uq_{}_{}", d.table, d.column)), q(&d.column))
    } else {
        String::new()
    };
    Ok(format!(
        "ALTER TABLE {}.{} {unique}ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}.{} ({}) ON DELETE {} ON UPDATE {}",
        q(db),
        q(&d.table),
        q(&format!("fk_{}_{}", d.table, d.column)),
        q(&d.column),
        q(db),
        q(rt),
        q(rc),
        d.on_delete,
        d.on_update
    ))
}

impl ErTab {
    pub fn new(db: String) -> Self {
        let (mut notation, mut collapse_nm, mut rel_labels) = (Notation::Crow, false, true);
        if let Ok(text) = std::fs::read_to_string(view_file()) {
            for line in text.lines() {
                match line.split_once('=') {
                    Some(("notation", v)) => notation = Notation::ALL.into_iter().find(|n| n.key() == v).unwrap_or(Notation::Crow),
                    Some(("nm", v)) => collapse_nm = v == "1",
                    Some(("beschriftung", v)) => rel_labels = v != "0",
                    _ => {}
                }
            }
        }
        Self {
            db,
            raw: None,
            schema: None,
            nm: HashMap::new(),
            notation,
            collapse_nm,
            rel_labels,
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
            selected: None,
            connect_mode: false,
        }
    }

    fn save_view(&self) {
        let path = view_file();
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let s = format!(
            "notation={}\nnm={}\nbeschriftung={}\n",
            self.notation.key(),
            self.collapse_nm as u8,
            self.rel_labels as u8
        );
        let _ = crate::workspace::atomic_write(&path, &s);
    }

    fn set_schema(&mut self, s: Option<Schema>) {
        self.raw = s;
        self.rebuild_view();
    }

    /// Angezeigte Struktur aufbauen: bei "n:m zusammenfassen" Zwischentabellen ausblenden
    /// und durch eine direkte n:m-Linie zwischen den beiden Tabellen ersetzen.
    fn rebuild_view(&mut self) {
        self.nm.clear();
        let Some(raw) = &self.raw else {
            self.schema = None;
            return;
        };
        let mut view = raw.clone();
        if self.collapse_nm {
            let js = raw.junctions();
            let hidden: Vec<&str> = js.iter().map(|(j, _, _)| j.as_str()).collect();
            view.tables.retain(|t| !hidden.contains(&t.name.as_str()));
            view.fks.retain(|f| !hidden.contains(&f.table.as_str()));
            for (j, a, b) in &js {
                let (fa, fb) = (&raw.fks[*a], &raw.fks[*b]);
                let syn = ForeignKey {
                    name: j.clone(),
                    table: fa.ref_table.clone(),
                    columns: fa.ref_columns.clone(),
                    ref_table: fb.ref_table.clone(),
                    ref_columns: fb.ref_columns.clone(),
                    ..Default::default()
                };
                self.nm.insert(fk_key(&syn), j.clone());
                view.fks.push(syn);
            }
        }
        self.schema = Some(view);
    }

    /// Art der (angezeigten) Beziehung
    fn rel(&self, schema: &Schema, fk: &ForeignKey) -> Rel {
        if self.nm.contains_key(&fk_key(fk)) {
            return Rel { many: true, nm: true, optional: false };
        }
        Rel {
            many: schema.rel_kind(fk) == crate::db::RelKind::OneToMany,
            nm: false,
            optional: schema.rel_optional(fk),
        }
    }

    /// Beschreibung fuer den Tooltip
    fn rel_text(&self, schema: &Schema, fk: &ForeignKey) -> String {
        if let Some(j) = self.nm.get(&fk_key(fk)) {
            let mut s = format!("n:m-Beziehung über die Zwischentabelle „{j}“\n{}  ↔  {}", fk.table, fk.ref_table);
            let raw = self.raw.as_ref().unwrap_or(schema);
            if let Some(t) = raw.table(j) {
                let extra: Vec<&str> = t
                    .columns
                    .iter()
                    .filter(|c| !raw.is_fk_column(j, &c.name))
                    .map(|c| c.name.as_str())
                    .collect();
                if !extra.is_empty() {
                    let _ = write!(s, "\nweitere Spalten: {}", extra.join(", "));
                }
            }
            s.push_str(&format!(
                "\nJede Zeile in {} kann zu vielen in {} gehören und umgekehrt.\n(Rechtsklick für mehr)",
                fk.table, fk.ref_table
            ));
            return s;
        }
        let rel = self.rel(schema, fk);
        let what = if rel.many {
            format!("1:n – ein Datensatz in {} gehört zu vielen in {}", fk.ref_table, fk.table)
        } else {
            format!("1:1 – ein Datensatz in {} gehört zu höchstens einem in {}", fk.ref_table, fk.table)
        };
        let opt = if rel.optional {
            format!("optional: {}.{} darf leer (NULL) sein", fk.table, fk.columns.join(","))
        } else {
            format!("Pflicht: jede Zeile in {} braucht einen Eintrag in {}", fk.table, fk.ref_table)
        };
        format!(
            "{what}\n{opt}\n\n{}\n{}.{}  →  {}.{}\nON DELETE {} / ON UPDATE {}\n(Rechtsklick zum Löschen)",
            fk.name,
            fk.table,
            fk.columns.join(","),
            fk.ref_table,
            fk.ref_columns.join(","),
            fk.on_delete,
            fk.on_update
        )
    }

    fn load(&mut self, cx: &mut Ctx) {
        self.loaded_for = self.db.clone();
        self.error = None;
        cx.schemas.invalidate(&self.db);
        let s = cx.schema(&self.db);
        self.set_schema(s);
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
        // Wieder eingeblendete Zwischentabellen zwischen ihre beiden Tabellen setzen
        for (j, a, b) in schema.junctions() {
            if self.pos.contains_key(&j) {
                continue;
            }
            let (ta, tb) = (&schema.fks[a].ref_table, &schema.fks[b].ref_table);
            if let (Some(pa), Some(pb), Some(sa), Some(sb), Some(sj)) = (
                self.pos.get(ta),
                self.pos.get(tb),
                schema.table(ta).map(|t| box_size(t, self.show_types)),
                schema.table(tb).map(|t| box_size(t, self.show_types)),
                schema.table(&j).map(|t| box_size(t, self.show_types)),
            ) {
                let mid = (Rect::from_min_size(*pa, sa).center().to_vec2() + Rect::from_min_size(*pb, sb).center().to_vec2()) / 2.0;
                let mut r = Rect::from_min_size((mid - sj / 2.0).to_pos2(), sj);
                // nicht auf andere Tabellen legen: nach unten ausweichen
                let others: Vec<Rect> = schema
                    .tables
                    .iter()
                    .filter_map(|t| self.pos.get(&t.name).map(|p| Rect::from_min_size(*p, box_size(t, self.show_types)).expand(10.0)))
                    .collect();
                for _ in 0..200 {
                    match others.iter().find(|o| o.intersects(r)) {
                        Some(o) => r = r.translate(vec2(0.0, o.bottom() - r.top() + 1.0)),
                        None => break,
                    }
                }
                self.pos.insert(j.clone(), r.min);
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

    /// Automatische Anordnung (siehe er_layout): wenig Kreuzungen, kompakt, etwa Bildschirmformat.
    fn auto_layout(&mut self) {
        let Some(schema) = self.schema.clone() else { return };
        let sizes: Vec<(String, Vec2)> = schema.tables.iter().map(|t| (t.name.clone(), box_size(t, self.show_types))).collect();
        let edges: Vec<(String, String)> = schema.fks.iter().map(|f| (f.table.clone(), f.ref_table.clone())).collect();
        self.pos = super::er_layout::layout(&sizes, &edges);
        self.routes.clear();
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

    /// Endpunkte einer Beziehung: (Anfang am Kind, Richtung, Ende am Elternteil, Richtung, Fahrspuren).
    /// Richtung = +1 nach rechts aus der Tabelle heraus, -1 nach links.
    fn endpoints(&self, schema: &Schema, fk: &ForeignKey) -> Option<(Pos2, f32, Pos2, f32, Option<&Vec<Pos2>>)> {
        let src = schema.table(&fk.table)?;
        let dst = schema.table(&fk.ref_table)?;
        let sr = self.table_rect_world(src);
        let dr = self.table_rect_world(dst);
        let sy = sr.top() + row_y(src, fk.columns.first()?);
        let dy = dr.top() + row_y(dst, fk.ref_columns.first()?);
        if src.name == dst.name {
            return Some((pos2(sr.right(), sy), 1.0, pos2(dr.right(), dy), 1.0, None));
        }
        // Mit Fahrspuren, wenn die Anordnung noch passt (Kind rechts vom Elternteil)
        if let Some(lanes) = self.routes.get(&fk_key(fk)) {
            if !lanes.is_empty() && dr.right() < lanes.last()?.x + 1.0 && sr.left() > lanes[0].x - 1.0 {
                return Some((pos2(sr.left(), sy), -1.0, pos2(dr.right(), dy), 1.0, Some(lanes)));
            }
        }
        // Direkt: passende Seiten waehlen
        Some(if sr.left() > dr.right() + 20.0 {
            (pos2(sr.left(), sy), -1.0, pos2(dr.right(), dy), 1.0, None)
        } else if dr.left() > sr.right() + 20.0 {
            (pos2(sr.right(), sy), 1.0, pos2(dr.left(), dy), -1.0, None)
        } else if sr.center().x <= dr.center().x {
            (pos2(sr.left(), sy), -1.0, pos2(dr.left(), dy), -1.0, None)
        } else {
            (pos2(sr.right(), sy), 1.0, pos2(dr.right(), dy), 1.0, None)
        })
    }

    /// Enden mehrere Linien am selben Punkt (z. B. alle Verweise auf kunde.id), werden sie
    /// leicht aufgefaechert – sortiert nach der Hoehe des anderen Endes, damit sie sich nicht kreuzen.
    fn port_offset(&self, schema: &Schema, fk: &ForeignKey, p: Pos2, d: f32, at_start: bool) -> f32 {
        let me = (fk_key(fk), at_start);
        let mut group: Vec<(f32, (String, bool))> = Vec::new();
        for g in &schema.fks {
            let Some((a, da, b, db, lanes)) = self.endpoints(schema, g) else { continue };
            let (na, nb) = match lanes {
                Some(l) => (l[0], l[l.len() - 1]),
                None => (b, a),
            };
            if a.distance(p) < 0.5 && da == d {
                group.push((na.y, (fk_key(g), true)));
            }
            if b.distance(p) < 0.5 && db == d {
                group.push((nb.y, (fk_key(g), false)));
            }
        }
        let n = group.len();
        if n <= 1 {
            return 0.0;
        }
        group.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| x.1.cmp(&y.1)));
        let idx = group.iter().position(|(_, k)| *k == me).unwrap_or(0);
        let step = (12.0 / (n - 1) as f32).min(5.0);
        (idx as f32 - (n - 1) as f32 / 2.0) * step
    }

    /// Linienverlauf einer Beziehung (Weltkoordinaten).
    fn edge_path(&self, schema: &Schema, fk: &ForeignKey) -> Option<Path> {
        let (mut a, da, mut b, db, lanes) = self.endpoints(schema, fk)?;
        a.y += self.port_offset(schema, fk, a, da, true);
        b.y += self.port_offset(schema, fk, b, db, false);
        if fk.table == fk.ref_table {
            let bend = 40.0 + (a.y - b.y).abs() * 0.2;
            return Some(vec![[a, a + vec2(bend, 0.0), b + vec2(bend, 0.0), b]]);
        }
        if let Some(lanes) = lanes {
            let mut pts = vec![a];
            pts.extend(lanes.iter().copied());
            pts.push(b);
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
        // Ist die Spalte schon eindeutig, ist 1:1 die naheliegende Art
        let unique = self.raw.as_ref().is_some_and(|s| {
            s.unique_keys.iter().any(|(t, k)| *t == table && k.len() == 1 && k[0] == column)
        });
        self.link = Some(LinkDlg {
            kind: if unique { LinkKind::OneToOne } else { LinkKind::OneToMany },
            table,
            column,
            target,
            on_delete: "RESTRICT".into(),
            on_update: "CASCADE".into(),
            other: String::new(),
            junction: String::new(),
        });
    }

    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, style::pal().er_canvas);
        let origin = rect.min;
        let Some(schema) = self.schema.clone() else { return };
        if self.fit_pending && rect.width() > 50.0 {
            self.fit_pending = false;
            self.fit(rect);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));

        crate::app::block_autoscroll(rect);
        // Zoom mit Mausrad (unabhaengig von der eingestellten Scrollgeschwindigkeit)
        if resp.hovered() {
            let speed = cx.settings.scroll_speed.max(0.25);
            let scroll = ui.input(|i| i.smooth_scroll_delta.y / speed + i.zoom_delta().ln() * 200.0);
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
            let middle = ui.input(|i| i.pointer.middle_down());
            self.drag = match origin_press {
                Some(_) if middle => Drag::Pan,
                Some(p) => {
                    let column = if self.connect_mode {
                        self.column_at(&schema, self.to_world(origin, p)).and_then(|(t, c)| Some((t, c?)))
                    } else {
                        None
                    };
                    if let Some((t, c)) = handle_at(self, p).or(column) {
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
        if resp.clicked() {
            self.selected = pointer.filter(|p| hit_table(self, *p).is_none()).and_then(hit_fk).map(|i| fk_key(&schema.fks[i]));
        }
        // Entf: ausgewaehlte Beziehung loeschen
        let editing_text = ui.ctx().egui_wants_keyboard_input();
        if !editing_text && self.link.is_none() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            if let Some(fk) = self.selected.as_ref().and_then(|k| schema.fks.iter().find(|f| fk_key(f) == *k)) {
                cx.actions.push(self.delete_action(fk));
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
            self.connect_mode = false;
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
        let raw = self.raw.clone().unwrap_or_else(|| schema.clone());
        let mut tags: Vec<(Pos2, String, Color32)> = Vec::new();
        let mut texts: Vec<(Pos2, String, Align2, Color32)> = Vec::new();
        for (i, fk) in schema.fks.iter().enumerate() {
            let Some(path) = &paths[i] else { continue };
            let related = focus_table.as_ref().map(|t| *t == fk.table || *t == fk.ref_table);
            let is_hover = hover_fk == Some(i) || matches!(self.menu, Some(Menu::Fk(j)) if j == i);
            let is_sel = self.selected.as_deref() == Some(fk_key(fk).as_str());
            let (color, width) = if is_sel {
                (LINE_HI, 3.2)
            } else if is_hover || related == Some(true) {
                (LINE_HI, 2.2)
            } else if related == Some(false) {
                (style::pal().er_line_dim, 1.2)
            } else {
                (style::pal().er_line, 1.4)
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
            // Kardinalitaeten (1:1, 1:n, n:m, optional) in der gewaehlten Notation
            let rel = self.rel(&schema, fk);
            for prim in markers(path, rel, self.notation, self.rel_labels) {
                match prim {
                    Prim::Line(p, q) => {
                        painter.line_segment([screen(p), screen(q)], stroke);
                    }
                    Prim::Circle(c, r) => {
                        painter.circle(screen(c), r * z, style::pal().er_canvas, stroke);
                    }
                    Prim::Text(p, s, align) => texts.push((p, s, align, color)),
                    Prim::Tag(p, s) => tags.push((screen(p), s, color)),
                    Prim::Diamond(c, r) => {
                        let c = screen(c);
                        let (w, h) = (r * 1.4 * z, r * z);
                        let pts = vec![c + vec2(-w, 0.0), c + vec2(0.0, -h), c + vec2(w, 0.0), c + vec2(0.0, h)];
                        painter.add(Shape::convex_polygon(pts, style::pal().er_canvas, stroke));
                    }
                }
            }
        }
        // Beschriftungen ueber alle Linien
        let small = FontId::proportional((10.5 * z).max(6.0));
        for (p, s, align, color) in place_texts(texts) {
            painter.text(screen(p), align, s, small.clone(), color);
        }
        if z > 0.35 {
            let font = FontId::proportional(10.5 * z);
            for (p, s, color) in tags {
                let galley = painter.layout_no_wrap(s, font.clone(), color);
                let r = Rect::from_center_size(p, galley.size() + vec2(8.0, 2.0) * z);
                painter.rect(r, 3.0 * z, style::pal().er_canvas, Stroke::new(1.0, color), StrokeKind::Inside);
                painter.galley(r.center() - galley.size() / 2.0, galley, color);
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
            painter.rect_filled(r, 0.0, style::pal().er_box);
            let head = Rect::from_min_size(r.min, vec2(r.width(), HEADER_H * z));
            painter.rect_filled(head, 0.0, if t.is_view { style::pal().er_view_header } else { style::pal().er_header });
            painter.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, style::pal().border));
            let title = if t.is_view { format!("{} (Sicht)", t.name) } else { t.name.clone() };
            painter.text(head.left_center() + vec2(6.0 * z, 0.0), Align2::LEFT_CENTER, title, bold.clone(), style::pal().text);
            for (i, c) in t.columns.iter().enumerate() {
                let y = r.top() + (HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0) * z;
                let is_fk = raw.is_fk_column(&t.name, &c.name);
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
                painter.text(pos2(x0 + 22.0 * z, y), Align2::LEFT_CENTER, label, nf, style::pal().text);
                if self.show_types {
                    painter.text(pos2(r.right() - 16.0 * z, y), Align2::RIGHT_CENTER, &c.col_type, font.clone(), style::pal().er_type);
                }
                // Anfasser zum Verknuepfen (nur bei Maus ueber der Tabelle)
                if (focused || self.connect_mode) && !t.is_view && matches!(self.drag, Drag::None) {
                    painter.circle(pos2(r.right() - 7.0 * z, y), 3.5 * z, style::pal().er_box, Stroke::new(1.2, LINE_HI));
                }
            }
            let stroke = if focused { Stroke::new(1.8, LINE_HI) } else { Stroke::new(1.0, style::pal().border) };
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
            resp.clone().on_hover_text(self.rel_text(&schema, &schema.fks[i]));
        }

        if schema.tables.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Diese Datenbank enthält noch keine Tabellen.", FontId::proportional(14.0), style::pal().text_weak);
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, style::pal().border), StrokeKind::Inside);

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
                if ui.button("Beziehung von hier anlegen …").clicked() {
                    let col = schema.table(t).and_then(|ti| ti.columns.iter().find(|c| !c.is_pk()).or(ti.columns.first())).map(|c| c.name.clone()).unwrap_or_default();
                    self.open_link(t.clone(), col, String::new());
                    ui.close();
                }
            }
            Some(Menu::Fk(i)) if schema.fks.get(*i).is_some_and(|fk| self.nm.contains_key(&fk_key(fk))) => {
                let fk = &schema.fks[*i];
                let j = self.nm[&fk_key(fk)].clone();
                ui.label(RichText::new(format!("{} ↔ {}  (n:m über {j})", fk.table, fk.ref_table)).strong());
                if ui.button("Daten der Zwischentabelle").clicked() {
                    cx.actions.push(Action::OpenData { db: db.clone(), table: j.clone() });
                    ui.close();
                }
                if ui.button("Struktur der Zwischentabelle").clicked() {
                    cx.actions.push(Action::OpenStructure { db: db.clone(), table: j.clone() });
                    ui.close();
                }
                if ui.button("Beziehung löschen …").clicked() {
                    cx.actions.push(self.delete_action(fk));
                    close_menu = true;
                    ui.close();
                }
            }
            Some(Menu::Fk(i)) => {
                if let Some(fk) = schema.fks.get(*i) {
                    ui.label(RichText::new(format!("{}.{} → {}.{}", fk.table, fk.columns.join(","), fk.ref_table, fk.ref_columns.join(","))).strong());
                    if ui.button("Beziehung löschen …").clicked() {
                        cx.actions.push(self.delete_action(fk));
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
                if ui.button("Neue Tabelle …").clicked() {
                    cx.actions.push(Action::NewTable(db.clone()));
                    ui.close();
                }
                if ui.button("Neue Beziehung …").clicked() {
                    self.open_link(String::new(), String::new(), String::new());
                    ui.close();
                }
            }
        });
        if close_menu {
            self.menu = None;
        }
    }

    /// Rueckfrage zum Loeschen einer Beziehung (bei n:m die Zwischentabelle)
    fn delete_action(&self, fk: &ForeignKey) -> Action {
        match self.nm.get(&fk_key(fk)) {
            Some(j) => Action::Confirm {
                text: format!("n:m-Beziehung {} – {} löschen? Die Zwischentabelle „{j}“ wird mit allen Zuordnungen gelöscht.", fk.table, fk.ref_table),
                db: Some(self.db.clone()),
                sql: format!("DROP TABLE {}", q(j)),
            },
            None => Action::Confirm {
                text: format!("Beziehung {}.{} → {}.{} löschen?", fk.table, fk.columns.join(","), fk.ref_table, fk.ref_columns.join(",")),
                db: Some(self.db.clone()),
                sql: format!("ALTER TABLE {} DROP FOREIGN KEY {}", q(&fk.table), q(&fk.name)),
            },
        }
    }

    fn link_dialog(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(schema) = self.raw.clone() else { return };
        let db = self.db.clone();
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
        egui::Window::new("Beziehung anlegen")
            .id(egui::Id::new(("erlink", &self.db)))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Art:");
                    let before = dlg.kind;
                    ui.radio_value(&mut dlg.kind, LinkKind::OneToMany, "1:n")
                        .on_hover_text("Ein Datensatz gehört zu vielen (z. B. eine Klasse – viele Schüler)");
                    ui.radio_value(&mut dlg.kind, LinkKind::OneToOne, "1:1")
                        .on_hover_text("Ein Datensatz gehört zu höchstens einem (z. B. Schüler – Ausweis)");
                    ui.radio_value(&mut dlg.kind, LinkKind::ManyToMany, "n:m")
                        .on_hover_text("Viele zu vielen über eine Zwischentabelle (z. B. Schüler – Kurse)");
                    // Beim Wechsel zu n:m die Zieltabelle als zweite Tabelle uebernehmen
                    if dlg.kind == LinkKind::ManyToMany && before != LinkKind::ManyToMany && dlg.other.is_empty() {
                        if let Some((rt, _)) = dlg.target.split_once('.') {
                            dlg.other = rt.to_string();
                        }
                    }
                });
                ui.add_space(4.0);
                egui::Grid::new("erlinkgrid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    if dlg.kind == LinkKind::ManyToMany {
                        ui.label("Tabelle A:");
                        super::str_combo(ui, "erl_t", &tables, &mut dlg.table, 180.0);
                        ui.end_row();
                        ui.label("Tabelle B:");
                        super::str_combo(ui, "erl_o", &tables, &mut dlg.other, 180.0);
                        ui.end_row();
                        ui.label("Zwischentabelle:");
                        let auto = format!("{}_{}", dlg.table, dlg.other);
                        ui.add(egui::TextEdit::singleline(&mut dlg.junction).hint_text(auto).desired_width(180.0));
                        ui.end_row();
                    } else {
                        ui.label(if dlg.kind == LinkKind::OneToOne { "Tabelle (abhängig):" } else { "Tabelle (viele):" });
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
                    }
                });
                if dlg.kind != LinkKind::ManyToMany {
                    // Typen vergleichen
                    let ty = |t: &str, c: &str| {
                        schema.table(t).and_then(|ti| ti.columns.iter().find(|x| x.name == c)).map(|x| x.col_type.clone())
                    };
                    if let (Some(a), Some((rt, rc))) = (ty(&dlg.table, &dlg.column), dlg.target.split_once('.')) {
                        if let Some(b) = ty(rt, rc) {
                            if a != b {
                                ui.label(RichText::new(format!("Hinweis: Datentypen unterscheiden sich ({a} / {b}).")).color(style::pal().error_text).small());
                            }
                        }
                    }
                }
                let sql = link_sql(&db, &schema, dlg);
                ui.separator();
                match &sql {
                    Ok(sql) => {
                        egui::CollapsingHeader::new(RichText::new("SQL anzeigen").small()).id_salt("erlinksql").show(ui, |ui| {
                            ui.label(RichText::new(sql).monospace().small());
                        });
                    }
                    Err(e) if !e.is_empty() => {
                        ui.label(RichText::new(e).small().color(style::pal().error_text));
                    }
                    Err(_) => {}
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(sql.is_ok(), egui::Button::new("Anlegen")).clicked() {
                        ok = true;
                    }
                    if ui.button("Abbrechen").clicked() {
                        cancel = true;
                    }
                });
            });
        if ok {
            let d = self.link.take().unwrap();
            let Ok(sql) = link_sql(&self.db, &schema, &d) else { return };
            if let Some(dbc) = cx.db {
                match dbc.exec(&sql, ()) {
                    Ok(_) => {
                        let what = match d.kind {
                            LinkKind::ManyToMany => format!("n:m-Beziehung {} ↔ {} angelegt.", d.table, d.other),
                            LinkKind::OneToOne => format!("1:1-Beziehung {}.{} → {} angelegt.", d.table, d.column, d.target),
                            LinkKind::OneToMany => format!("1:n-Beziehung {}.{} → {} angelegt.", d.table, d.column, d.target),
                        };
                        cx.status(what);
                        cx.actions.push(Action::SchemaChanged(self.db.clone()));
                    }
                    Err(e) => {
                        let tip = match d.kind {
                            LinkKind::ManyToMany => "Tipp: Gibt es die Zwischentabelle schon? Dann einen anderen Namen wählen.",
                            LinkKind::OneToOne => "Tipp: Für 1:1 darf jeder Wert in der Spalte nur einmal vorkommen. Beide Spalten brauchen den gleichen Datentyp.",
                            LinkKind::OneToMany => "Tipp: Beide Spalten brauchen den gleichen Datentyp, und vorhandene Werte müssen in der Zieltabelle existieren.",
                        };
                        cx.error(format!("{e}\n\n{tip}\n\nSQL: {sql}"));
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
        let raw = self.raw.as_ref().unwrap_or(schema);
        let mut tags: Vec<(Pos2, String)> = Vec::new();
        let mut texts: Vec<(Pos2, String, Align2, ())> = Vec::new();
        for fk in &schema.fks {
            let Some(path) = self.edge_path(schema, fk) else { continue };
            let mut d = format!("M{} {}", path[0][0].x, path[0][0].y);
            for s in &path {
                let _ = write!(d, " C{} {} {} {} {} {}", s[1].x, s[1].y, s[2].x, s[2].y, s[3].x, s[3].y);
            }
            let _ = writeln!(o, r##"<path d="{d}" fill="none" stroke="#305090" stroke-width="1.4"/>"##);
            let rel = self.rel(schema, fk);
            for prim in markers(&path, rel, self.notation, self.rel_labels) {
                match prim {
                    Prim::Line(p, q) => {
                        let _ = writeln!(o, r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#305090" stroke-width="1.4"/>"##, p.x, p.y, q.x, q.y);
                    }
                    Prim::Circle(c, r) => {
                        let _ = writeln!(o, r##"<circle cx="{}" cy="{}" r="{r}" fill="white" stroke="#305090" stroke-width="1.4"/>"##, c.x, c.y);
                    }
                    Prim::Text(p, t, align) => texts.push((p, t, align, ())),
                    Prim::Tag(p, t) => tags.push((p, t)),
                    Prim::Diamond(c, r) => {
                        let w = r * 1.4;
                        let _ = writeln!(
                            o,
                            r##"<path d="M{} {} L{} {} L{} {} L{} {} Z" fill="white" stroke="#305090" stroke-width="1.4"/>"##,
                            c.x - w, c.y, c.x, c.y - r, c.x + w, c.y, c.x, c.y + r
                        );
                    }
                }
            }
        }
        for (p, t, align, _) in place_texts(texts) {
            let anchor = if align == Align2::LEFT_BOTTOM { "start" } else { "end" };
            let _ = writeln!(o, r##"<text x="{}" y="{}" font-size="10.5" text-anchor="{anchor}" fill="#305090">{}</text>"##, p.x, p.y - 2.0, esc(&t));
        }
        for (p, t) in tags {
            let w = t.chars().count() as f32 * 6.5 + 8.0;
            let _ = writeln!(
                o,
                r##"<rect x="{}" y="{}" width="{w}" height="15" rx="3" fill="white" stroke="#305090"/><text x="{}" y="{}" font-size="10.5" text-anchor="middle" fill="#305090">{}</text>"##,
                p.x - w / 2.0,
                p.y - 7.5,
                p.x,
                p.y + 4.0,
                esc(&t)
            );
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
                } else if raw.is_fk_column(&t.name, &c.name) {
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

    fn session(&self) -> Option<String> {
        Some(format!("er\t{}", self.db))
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.load(cx);
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db && (self.pos.is_empty() || self.raw.is_none()) {
            // noch nie (erfolgreich) geladen
            self.loaded_for.clear();
        } else if db == self.db {
            let pos = self.pos.clone();
            let routes = self.routes.clone();
            cx.schemas.invalidate(&self.db);
            let s = cx.schema(&self.db);
            self.set_schema(s);
            self.pos = pos;
            self.routes = routes;
            if let (Some(raw), Some(s)) = (self.raw.clone(), self.schema.clone()) {
                self.pos.retain(|k, _| raw.table(k).is_some());
                self.place_new_tables(&s);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if self.loaded_for != self.db && cx.db.is_some() {
            self.load(cx);
        }
        ui.horizontal_wrapped(|ui| {
            let mut d = self.db.clone();
            if super::db_combo(ui, "erdb", cx.databases, &mut d) {
                self.db = d;
            }
            if crate::icons::button(ui, crate::icons::Icon::Refresh, "Neu einlesen").clicked() {
                self.schema_changed(&self.db.clone(), cx);
            }
            ui.separator();
            if ui.button("Anordnen").on_hover_text("Automatisch anordnen: wenig Kreuzungen, kompakt").clicked() {
                self.auto_layout();
                self.fit_pending = true;
                self.save_layout();
            }
            ui.separator();
            let r = ui.add(egui::Button::new("Verbinden").selected(self.connect_mode));
            if r.clicked() {
                self.connect_mode = !self.connect_mode;
            }
            r
                .on_hover_text("Von einer Spalte auf die Zielspalte ziehen legt eine Beziehung an (Esc beendet)");
            if ui.button("+ Beziehung").clicked() {
                self.open_link(String::new(), String::new(), String::new());
            }
            let sel = self.selected.clone().and_then(|k| self.schema.as_ref()?.fks.iter().find(|f| fk_key(f) == k).cloned());
            if let Some(fk) = &sel {
                if ui.button("Beziehung löschen").on_hover_text("Entf").clicked() {
                    cx.actions.push(self.delete_action(fk));
                }
            }
            if ui.button("+ Tabelle").clicked() {
                cx.actions.push(Action::NewTable(self.db.clone()));
            }
            ui.separator();
            let before = (self.notation, self.collapse_nm, self.rel_labels);
            ui.menu_button("Ansicht", |ui| {
                ui.label(RichText::new("Notation").small().color(style::pal().text_weak));
                for n in Notation::ALL {
                    ui.radio_value(&mut self.notation, n, n.label());
                }
                ui.separator();
                ui.checkbox(&mut self.show_types, "Datentypen");
                if self.notation == Notation::Crow {
                    ui.checkbox(&mut self.rel_labels, "1:n / 1:1 / n:m an den Linien");
                }
                ui.checkbox(&mut self.collapse_nm, "Zwischentabellen als n:m-Linie");
            });
            if before != (self.notation, self.collapse_nm, self.rel_labels) {
                if before.1 != self.collapse_nm {
                    self.rebuild_view();
                    if let Some(s) = self.schema.clone() {
                        self.place_new_tables(&s);
                    }
                }
                self.save_view();
            }
            ui.menu_button("Export", |ui| {
                if ui.button("Als SVG speichern …").clicked() {
                    ui.close();
                    if let Some(p) = rfd::FileDialog::new().add_filter("SVG-Grafik", &["svg"]).set_file_name(format!("{}-er-diagramm.svg", self.db)).save_file() {
                        if let Err(e) = std::fs::write(&p, self.to_svg()) {
                            cx.error(e.to_string());
                        }
                    }
                }
                if ui.button("SQL-Skript (CREATE TABLE)").clicked() {
                    ui.close();
                    if let Some(dbc) = cx.db {
                        match dbc.dump(&self.db, false) {
                            Ok(sql) => cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql, run: false }),
                            Err(e) => cx.error(e),
                        }
                    }
                }
            });
            ui.separator();
            if ui.button(format!("{:.0}\u{a0}%", self.zoom * 100.0)).on_hover_text("Alles zeigen (Mausrad = Zoom)").clicked() {
                self.fit_pending = true;
            }
        });
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(style::pal().error_text));
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
    fn layout_no_overlap_and_clear_lines() {
        let schema = Schema {
            name: "x".into(),
            tables: vec![
                table("klasse", &["id", "name"]),
                table("lehrer", &["id", "name"]),
                table("schueler", &["id", "klasse_id"]),
                table("note", &["id", "schueler_id", "lehrer_id"]),
                table("einsam", &["id"]),
            ],
            unique_keys: vec![],
            fks: vec![fk("schueler", "klasse_id", "klasse"), fk("note", "schueler_id", "schueler"), fk("note", "lehrer_id", "lehrer")],
        };
        let mut er = ErTab::new("x".into());
        er.collapse_nm = false;
        er.set_schema(Some(schema.clone()));
        er.auto_layout();
        // keine Ueberlappungen
        let rects: Vec<Rect> = schema.tables.iter().map(|t| er.table_rect_world(t)).collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(!rects[i].intersects(rects[j]), "{} / {}", schema.tables[i].name, schema.tables[j].name);
            }
        }
        // keine Linie laeuft durch eine fremde Tabelle
        for f in &schema.fks {
            let path = er.edge_path(&schema, f).unwrap();
            for t in schema.tables.iter().filter(|t| t.name != f.table && t.name != f.ref_table) {
                let r = er.table_rect_world(t);
                for seg in &path {
                    for i in 0..=20 {
                        assert!(!r.contains(sample(seg, i as f32 / 20.0)), "{} -> {} durch {}", f.table, f.ref_table, t.name);
                    }
                }
            }
        }
        // Tabelle ohne Beziehung unterhalb
        let max_connected = ["klasse", "lehrer", "schueler", "note"].iter().map(|t| er.table_rect_world(schema.table(t).unwrap()).bottom()).fold(0.0, f32::max);
        assert!(er.pos["einsam"].y > max_connected);
    }

    fn shop() -> Schema {
        let mut pos = table("position", &["bestellung_id", "artikel_id", "menge"]);
        for c in pos.columns.iter_mut().take(2) {
            c.key = "PRI".into();
        }
        let mut kunde = table("bestellung", &["id", "kunde_id"]);
        kunde.columns[1].nullable = true;
        Schema {
            name: "shop".into(),
            tables: vec![table("kunde", &["id"]), kunde, table("artikel", &["id", "name"]), pos, table("ausweis", &["id", "kunde_id"])],
            fks: vec![
                fk("bestellung", "kunde_id", "kunde"),
                fk("position", "bestellung_id", "bestellung"),
                fk("position", "artikel_id", "artikel"),
                fk("ausweis", "kunde_id", "kunde"),
            ],
            unique_keys: vec![
                ("kunde".into(), vec!["id".into()]),
                ("bestellung".into(), vec!["id".into()]),
                ("artikel".into(), vec!["id".into()]),
                ("position".into(), vec!["bestellung_id".into(), "artikel_id".into()]),
                ("ausweis".into(), vec!["kunde_id".into()]),
            ],
        }
    }

    #[test]
    fn collapse_many_to_many() {
        let mut er = ErTab::new("shop".into());
        er.collapse_nm = true;
        er.set_schema(Some(shop()));
        let v = er.schema.clone().unwrap();
        assert!(v.table("position").is_none(), "Zwischentabelle ausgeblendet");
        assert_eq!(v.fks.len(), 3);
        let nm = v.fks.iter().find(|f| f.name == "position").unwrap();
        assert_eq!((nm.table.as_str(), nm.ref_table.as_str()), ("bestellung", "artikel"));
        assert_eq!(er.rel(&v, nm).label(), "n:m");
        let b = v.fks.iter().find(|f| f.table == "bestellung").unwrap();
        assert_eq!(er.rel(&v, b), Rel { many: true, nm: false, optional: true });
        let a = v.fks.iter().find(|f| f.table == "ausweis").unwrap();
        assert_eq!(er.rel(&v, a).label(), "1:1");
        // Layout mit n:m-Linie und wieder einblenden: Zwischentabelle bekommt einen Platz
        er.auto_layout();
        assert!(er.edge_path(&v, nm).is_some());
        er.notation = Notation::Crow;
        er.rel_labels = true;
        let svg = er.to_svg();
        assert!(svg.contains(">n:m</text>") && svg.contains(">1:n</text>") && svg.contains(">1:1</text>"));
        assert!(svg.contains("<circle"), "optionale Beziehung mit Kreis");
        assert!(!svg.contains(">position<"), "Zwischentabelle ausgeblendet");
        er.collapse_nm = false;
        er.rebuild_view();
        let s = er.schema.clone().unwrap();
        er.place_new_tables(&s);
        assert!(er.pos.contains_key("position"));
        assert_eq!(s.fks.len(), 4);
        let j = er.table_rect_world(s.table("position").unwrap());
        for t in s.tables.iter().filter(|t| t.name != "position") {
            assert!(!er.table_rect_world(t).intersects(j), "position liegt auf {}", t.name);
        }
    }

    #[test]
    fn marker_shapes() {
        let path: Path = vec![bezier_between(pos2(200.0, 50.0), pos2(0.0, 50.0), -1.0, 1.0)];
        let lines = |v: &[Prim]| v.iter().filter(|p| matches!(p, Prim::Line(..))).count();
        let circles = |v: &[Prim]| v.iter().filter(|p| matches!(p, Prim::Circle(..))).count();
        let texts = |v: &[Prim]| {
            v.iter()
                .filter_map(|p| match p {
                    Prim::Text(_, s, _) | Prim::Tag(_, s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let one_n = Rel { many: true, nm: false, optional: false };
        let m = markers(&path, one_n, Notation::Crow, true);
        assert_eq!((lines(&m), circles(&m)), (4, 0)); // Kraehenfuss (2) + zwei Striche
        assert_eq!(texts(&m), vec!["1:n"]);
        let m = markers(&path, Rel { optional: true, ..one_n }, Notation::Crow, false);
        assert_eq!((lines(&m), circles(&m)), (3, 1));
        assert!(texts(&m).is_empty());
        let m = markers(&path, Rel { many: false, nm: false, optional: false }, Notation::Crow, true);
        assert_eq!(lines(&m), 3);
        assert_eq!(texts(&m), vec!["1:1"]);
        let nm = Rel { many: true, nm: true, optional: false };
        assert_eq!(lines(&markers(&path, nm, Notation::Crow, false)), 4);
        assert_eq!(texts(&markers(&path, one_n, Notation::Chen, true)), vec!["n", "1"]);
        assert_eq!(texts(&markers(&path, nm, Notation::Chen, true)), vec!["n", "m"]);
        assert_eq!(texts(&markers(&path, Rel { optional: true, ..one_n }, Notation::MinMax, true)), vec!["(0,1)", "(0,n)"]);
        assert_eq!(texts(&markers(&path, one_n, Notation::MinMax, true)), vec!["(1,1)", "(0,n)"]);
        // Kind-Ende zeigt nach rechts: Beschriftung links ausgerichtet von der Linie weg
        match &markers(&path, one_n, Notation::Chen, true)[0] {
            Prim::Text(p, _, align) => {
                assert_eq!(*align, Align2::RIGHT_BOTTOM);
                assert!(p.x < 200.0);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn link_sql_kinds() {
        let s = shop();
        let mut d = LinkDlg {
            kind: LinkKind::OneToMany,
            table: "bestellung".into(),
            column: "kunde_id".into(),
            target: "kunde.id".into(),
            on_delete: "RESTRICT".into(),
            on_update: "CASCADE".into(),
            other: String::new(),
            junction: String::new(),
        };
        let sql = link_sql("shop", &s, &d).unwrap();
        assert!(sql.starts_with("ALTER TABLE `shop`.`bestellung` ADD CONSTRAINT `fk_bestellung_kunde_id` FOREIGN KEY"));
        d.kind = LinkKind::OneToOne;
        assert!(link_sql("shop", &s, &d).unwrap().contains("ADD UNIQUE KEY `uq_bestellung_kunde_id` (`kunde_id`), ADD CONSTRAINT"));
        // schon eindeutig: kein zweiter UNIQUE-Index
        d.table = "ausweis".into();
        assert!(!link_sql("shop", &s, &d).unwrap().contains("UNIQUE"));
        d.kind = LinkKind::ManyToMany;
        d.table = "kunde".into();
        assert_eq!(link_sql("shop", &s, &d), Err(String::new()));
        d.other = "artikel".into();
        let sql = link_sql("shop", &s, &d).unwrap();
        assert!(sql.starts_with("CREATE TABLE `shop`.`kunde_artikel` ("));
        assert!(sql.contains("`kunde_id` int(11) NOT NULL"));
        assert!(sql.contains("PRIMARY KEY (`kunde_id`, `artikel_id`)"));
        assert!(sql.contains("REFERENCES `shop`.`artikel` (`id`) ON DELETE CASCADE"));
        d.junction = "position".into();
        assert!(link_sql("shop", &s, &d).unwrap_err().contains("gibt es schon"));
        d.junction.clear();
        d.table = "position".into();
        assert!(link_sql("shop", &s, &d).unwrap_err().contains("genau einer Spalte"));
    }

    #[test]
    fn end_labels_do_not_overlap() {
        let items = vec![
            (pos2(100.0, 50.0), "1".to_string(), Align2::LEFT_BOTTOM, ()),
            (pos2(100.0, 53.0), "1".to_string(), Align2::LEFT_BOTTOM, ()),
            (pos2(100.0, 56.0), "m".to_string(), Align2::LEFT_BOTTOM, ()),
            (pos2(300.0, 50.0), "n".to_string(), Align2::RIGHT_BOTTOM, ()),
        ];
        let out = place_texts(items);
        assert_eq!(out.len(), 3, "gleiche Beschriftung zusammengefasst");
        assert!(out[1].0.x > 100.0 + 6.0, "andere Beschriftung nach aussen geschoben");
        assert_eq!(out[2].0, pos2(300.0, 50.0));
    }
}
