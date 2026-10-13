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
use std::sync::Arc;

mod draw;
mod layout;
mod link;
mod notation;
mod routes;
mod svg;

use link::*;
use notation::*;
use routes::*;

const HEADER_H: f32 = 22.0;
const ROW_H: f32 = 17.0;
const CHAR_W: f32 = 6.8;
const LINE_HI: Color32 = Color32::from_rgb(0xE0, 0x6C, 0x00);
const PK_COLOR: Color32 = Color32::from_rgb(0xB0, 0x80, 0x00);
const FK_COLOR: Color32 = Color32::from_rgb(0x20, 0x60, 0xC0);

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

fn view_file() -> std::path::PathBuf {
    crate::workspace::internal_dir().join("layouts").join("ansicht.txt")
}

pub struct ErTab {
    pub db: String,
    /// Struktur wie in der Datenbank
    raw: Option<Arc<Schema>>,
    /// Angezeigte Struktur (ggf. ohne Zwischentabellen, dafuer mit n:m-Linien)
    schema: Option<Arc<Schema>>,
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
    /// Struktur wird gelesen; `Some(pos, routes)` = Aktualisierung mit bisheriger Anordnung
    waiting: Option<Option<(HashMap<String, Pos2>, HashMap<String, Vec<Pos2>>)>>,
    menu: Option<Menu>,
    link: Option<LinkDlg>,
    fit_pending: bool,
    error: Option<String>,
    /// Ausgewaehlte Beziehung (fk_key), Entf loescht sie
    selected: Option<String>,
    route_cache: Option<RouteCache>,
    box_cache: Option<BoxCache>,
}

fn layout_file(db: &str) -> std::path::PathBuf {
    let safe: String = db
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    crate::workspace::internal_dir().join("layouts").join(format!("{safe}.txt"))
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
            waiting: None,
            menu: None,
            link: None,
            fit_pending: false,
            error: None,
            selected: None,
            route_cache: None,
            box_cache: None,
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

    fn set_schema(&mut self, s: Option<Arc<Schema>>) {
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
        if !self.collapse_nm {
            self.schema = Some(Arc::clone(raw));
            return;
        }
        let mut view = raw.as_ref().clone();
        {
            let js = raw.junctions();
            let hidden: std::collections::HashSet<&str> = js.iter().map(|(j, _, _)| j.as_str()).collect();
            view.tables.retain(|t| !hidden.contains(t.name.as_str()));
            view.fks.retain(|f| !hidden.contains(f.table.as_str()));
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
        view.invalidate_index();
        self.schema = Some(Arc::new(view));
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
            let mut s = crate::tr_format!("n:m-Beziehung über die Zwischentabelle „{j}“\n{}  ↔  {}", "n:m relationship through junction table “{j}”\n{}  ↔  {}", fk.table, fk.ref_table);
            let raw = self.raw.as_deref().unwrap_or(schema);
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
            s.push_str(&crate::tr_format!("\nJede Zeile in {} kann zu vielen in {} gehören und umgekehrt.\n(Rechtsklick für mehr)", "\nEach row in {} can relate to many rows in {}, and vice versa.\n(Right-click for more)",
                fk.table, fk.ref_table
            ));
            return s;
        }
        let rel = self.rel(schema, fk);
        let what = if rel.many {
            crate::tr_format!("1:n – ein Datensatz in {} gehört zu vielen in {}", "1:n — one record in {} relates to many in {}", fk.ref_table, fk.table)
        } else {
            crate::tr_format!("1:1 – ein Datensatz in {} gehört zu höchstens einem in {}", "1:1 — one record in {} relates to at most one in {}", fk.ref_table, fk.table)
        };
        let opt = if rel.optional {
            crate::tr_format!("optional: {}.{} darf leer (NULL) sein", "optional: {}.{} can be NULL", fk.table, fk.columns.join(","))
        } else {
            crate::tr_format!("Pflicht: jede Zeile in {} braucht einen Eintrag in {}", "required: each row in {} needs a record in {}", fk.table, fk.ref_table)
        };
        crate::tr_format!("{what}\n{opt}\n\n{}\n{}.{}  →  {}.{}\nON DELETE {} / ON UPDATE {}\n(Rechtsklick zum Löschen)", "{what}\n{opt}\n\n{}\n{}.{}  →  {}.{}\nON DELETE {} / ON UPDATE {}\n(Right-click to delete)",
            fk.name,
            fk.table,
            fk.columns.join(","),
            fk.ref_table,
            fk.ref_columns.join(","),
            fk.on_delete,
            fk.on_update
        )
    }

    /// Struktur neu anfordern; `finish_load` uebernimmt sie, sobald sie da ist.
    fn load(&mut self, cx: &mut Ctx) {
        self.loaded_for = self.db.clone();
        self.error = None;
        cx.schemas.invalidate(&self.db);
        self.set_schema(None);
        self.waiting = Some(None);
        self.finish_load(cx);
    }

    fn finish_load(&mut self, cx: &mut Ctx) {
        let Some(keep) = self.waiting.take() else { return };
        let Some(s) = cx.schema(&self.db) else {
            if cx.schema_loading(&self.db) {
                self.waiting = Some(keep);
            } else {
                self.error = Some(crate::i18n::text("Struktur konnte nicht gelesen werden.").into());
            }
            return;
        };
        if let Some((pos, routes)) = keep {
            // Aktualisierung: Anordnung behalten, nur neue Tabellen platzieren
            self.set_schema(Some(s.clone()));
            self.pos = pos;
            self.routes = routes;
            if let Some(raw) = self.raw.clone() {
                self.pos.retain(|k, _| raw.table(k).is_some());
            }
            self.place_new_tables(&s);
            return;
        }
        self.set_schema(Some(s));
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
        self.ensure_boxes();
        let mut max_x: f32 = 0.0;
        for t in &schema.tables {
            if let Some(p) = self.pos.get(&t.name) {
                max_x = max_x.max(p.x + self.box_of(t).x);
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
                schema.table(ta).map(|t| self.box_of(t)),
                schema.table(tb).map(|t| self.box_of(t)),
                schema.table(&j).map(|t| self.box_of(t)),
            ) {
                let mid = (Rect::from_min_size(*pa, sa).center().to_vec2() + Rect::from_min_size(*pb, sb).center().to_vec2()) / 2.0;
                let mut r = Rect::from_min_size((mid - sj / 2.0).to_pos2(), sj);
                // nicht auf andere Tabellen legen: nach unten ausweichen
                let others: Vec<Rect> = schema
                    .tables
                    .iter()
                    .filter_map(|t| self.pos.get(&t.name).map(|p| Rect::from_min_size(*p, self.box_of(t)).expand(10.0)))
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
                y += self.box_of(t).y + 30.0;
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

    /// Automatische Anordnung (siehe layout): wenig Kreuzungen, kompakt, etwa Bildschirmformat.
    fn auto_layout(&mut self) {
        let Some(schema) = self.schema.clone() else { return };
        self.ensure_boxes();
        let sizes: Vec<(String, Vec2)> = schema.tables.iter().map(|t| (t.name.clone(), self.box_of(t))).collect();
        let edges: Vec<(String, String)> = schema.fks.iter().map(|f| (f.table.clone(), f.ref_table.clone())).collect();
        self.pos = layout::layout(&sizes, &edges);
        self.routes.clear();
    }

    fn bounds(&self) -> Option<Rect> {
        let schema = self.schema.as_ref()?;
        let mut r: Option<Rect> = None;
        for t in &schema.tables {
            let p = self.pos.get(&t.name).copied().unwrap_or_default();
            let b = Rect::from_min_size(p, self.box_of(t));
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

    fn remove_routes_of(&mut self, table: &str) {
        if self.routes.is_empty() {
            return;
        }
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
}

impl TabView for ErTab {
    fn title(&self) -> String {
        crate::tr_format!("ER-Diagramm: {}", "ER diagram: {}", self.db)
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
        if db != self.db {
            return;
        }
        if self.pos.is_empty() || self.raw.is_none() {
            // noch nie (erfolgreich) geladen
            self.loaded_for.clear();
        } else {
            cx.schemas.invalidate(db);
            self.waiting = Some(Some((std::mem::take(&mut self.pos), std::mem::take(&mut self.routes))));
            self.finish_load(cx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if self.loaded_for != self.db && cx.db.is_some() {
            self.load(cx);
        } else if self.waiting.is_some() {
            self.finish_load(cx);
        }
        ui.horizontal_wrapped(|ui| {
            let mut d = self.db.clone();
            if super::db_combo(ui, "erdb", cx.databases, &mut d) {
                self.db = d;
            }
            if crate::icons::button(ui, crate::icons::Icon::Refresh, crate::i18n::text("Neu einlesen")).clicked() {
                self.schema_changed(&self.db.clone(), cx);
            }
            ui.separator();
            if ui.button(crate::i18n::text("Anordnen")).clicked() {
                self.auto_layout();
                self.fit_pending = true;
                self.save_layout();
            }
            ui.separator();
            if ui.button(crate::i18n::text("+ Beziehung")).clicked() {
                self.open_link(String::new(), String::new(), String::new());
            }
            ui.separator();
            let before = (self.notation, self.collapse_nm, self.rel_labels);
            ui.menu_button(crate::i18n::text("Ansicht"), |ui| {
                ui.label(RichText::new("Notation").small().color(style::pal().text_weak));
                for n in Notation::ALL {
                    ui.radio_value(&mut self.notation, n, n.label());
                }
                ui.separator();
                ui.checkbox(&mut self.show_types, crate::i18n::text("Datentypen"));
                if self.notation == Notation::Crow {
                    ui.checkbox(&mut self.rel_labels, crate::i18n::text("1:n / 1:1 / n:m an den Linien"));
                }
                ui.checkbox(&mut self.collapse_nm, crate::i18n::text("Zwischentabellen als n:m-Linie"));
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
                if ui.button(crate::i18n::text("Als SVG speichern …")).clicked() {
                    ui.close();
                    if let Some(p) = rfd::FileDialog::new().add_filter(crate::i18n::text("SVG-Grafik"), &["svg"]).set_file_name(format!("{}-er-diagramm.svg", self.db)).save_file() {
                        if let Err(e) = std::fs::write(&p, self.to_svg()) {
                            cx.error(e.to_string());
                        }
                    }
                }
                if ui.button(crate::i18n::text("SQL-Skript (CREATE TABLE)")).clicked() {
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
            if ui.button(format!("{:.0}\u{a0}%", self.zoom * 100.0)).clicked() {
                self.fit_pending = true;
            }
        });
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(style::pal().error_text));
        }
        if self.waiting.is_some() && self.schema.is_none() {
            ui.label(RichText::new(crate::i18n::text("Wird gelesen …")).color(style::pal().text_weak));
            return;
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
    fn routes_reuse_geometry_for_pan_zoom_and_update_after_drag() {
        let mut diagram = ErTab::new("shop".into());
        diagram.set_schema(Some(Arc::new(shop())));
        diagram.auto_layout();
        let schema = diagram.schema.clone().unwrap();
        let original = diagram.geometry(&schema);
        diagram.offset += vec2(40.0, 80.0);
        diagram.zoom = 1.5;
        assert!(Arc::ptr_eq(&original, &diagram.geometry(&schema)));
        let table = schema.tables[0].name.clone();
        *diagram.pos.get_mut(&table).unwrap() += vec2(120.0, 100.0);
        let moved = diagram.geometry(&schema);
        assert!(!Arc::ptr_eq(&original, &moved));
        assert_ne!(original.paths, moved.paths);
        diagram.show_types = !diagram.show_types;
        assert!(!Arc::ptr_eq(&moved, &diagram.geometry(&schema)));
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
            ..Default::default()
        };
        let mut er = ErTab::new("x".into());
        er.collapse_nm = false;
        er.set_schema(Some(Arc::new(schema.clone())));
        er.auto_layout();
        // keine Ueberlappungen
        let rects: Vec<Rect> = schema.tables.iter().map(|t| er.table_rect_world(t)).collect();
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(!rects[i].intersects(rects[j]), "{} / {}", schema.tables[i].name, schema.tables[j].name);
            }
        }
        // keine Linie laeuft durch eine fremde Tabelle
        for (f, path) in schema.fks.iter().zip(er.all_paths(&schema)) {
            let path = path.unwrap();
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
            ..Default::default()
        }
    }

    #[test]
    fn collapse_many_to_many() {
        let mut er = ErTab::new("shop".into());
        er.collapse_nm = true;
        er.set_schema(Some(Arc::new(shop())));
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
        let nm_index = v.fks.iter().position(|f| std::ptr::eq(f, nm)).unwrap();
        assert!(er.all_paths(&v)[nm_index].is_some());
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
        assert!(link_sql("shop", &s, &d).unwrap_err().contains("already exists"));
        d.junction.clear();
        d.table = "position".into();
        assert!(link_sql("shop", &s, &d).unwrap_err().contains("exactly one column"));
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

    /// Handmessung mit einer riesigen Datenbank: `cargo test --release er_geometry_timing -- --ignored --nocapture`
    #[test]
    #[ignore = "manual benchmark"]
    fn er_geometry_timing() {
        let n = 3000;
        let tables: Vec<TableInfo> = (0..n).map(|i| table(&format!("t{i}"), &["id", "a_id", "b_id", "name", "created"])).collect();
        let mut fks = Vec::new();
        for i in 1..n {
            if i % 10 != 0 {
                fks.push(fk(&format!("t{i}"), "a_id", &format!("t{}", i / 2)));
                fks.push(fk(&format!("t{i}"), "b_id", &format!("t{}", i / 3)));
            }
        }
        let schema = Arc::new(Schema { name: "gross".into(), tables, fks, ..Default::default() });
        let mut er = ErTab::new("gross".into());
        er.set_schema(Some(schema.clone()));
        let t = std::time::Instant::now();
        er.auto_layout();
        println!("Anordnen: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let geometry = er.geometry(&schema);
        println!("Geometrie ({} Linien): {:?}", geometry.paths.len(), t.elapsed());
        let t = std::time::Instant::now();
        let _ = er.geometry(&schema);
        println!("Geometrie aus dem Zwischenspeicher: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let svg = er.to_svg();
        println!("SVG ({} KB): {:?}", svg.len() / 1024, t.elapsed());
    }
}
