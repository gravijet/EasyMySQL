// Reverse Engineering: ER-Diagramm einer Datenbank aus den Metadaten erzeugen.

use super::{Action, Ctx, TabView};
use crate::db::{Schema, TableInfo};
use crate::style;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2,
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
const PK_COLOR: Color32 = Color32::from_rgb(0xB0, 0x80, 0x00);
const FK_COLOR: Color32 = Color32::from_rgb(0x20, 0x60, 0xC0);
const TYPE_COLOR: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);

pub struct ErTab {
    pub db: String,
    schema: Option<Schema>,
    pos: HashMap<String, Pos2>,
    offset: Vec2,
    zoom: f32,
    dragging: Option<String>,
    show_types: bool,
    loaded_for: String,
    context_table: Option<String>,
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
    let w = (chars as f32 * CHAR_W + 16.0).clamp(130.0, 420.0);
    let h = HEADER_H + ROW_H * t.columns.len().max(1) as f32 + 6.0;
    vec2(w, h)
}

impl ErTab {
    pub fn new(db: String) -> Self {
        Self {
            db,
            schema: None,
            pos: HashMap::new(),
            offset: vec2(20.0, 20.0),
            zoom: 1.0,
            dragging: None,
            show_types: true,
            loaded_for: String::new(),
            context_table: None,
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
        if let Ok(text) = std::fs::read_to_string(layout_file(&self.db)) {
            for line in text.lines() {
                let p: Vec<&str> = line.split('\t').collect();
                if p.len() == 3 {
                    if let (Ok(x), Ok(y)) = (p[1].parse(), p[2].parse()) {
                        self.pos.insert(p[0].to_string(), pos2(x, y));
                    }
                }
            }
        }
        let schema = self.schema.clone().unwrap();
        if schema.tables.iter().any(|t| !self.pos.contains_key(&t.name)) {
            self.auto_layout(false);
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
        let _ = std::fs::write(path, s);
    }

    /// Anordnung in Spalten nach Abhaengigkeit (referenzierte Tabellen links).
    fn auto_layout(&mut self, all: bool) {
        let Some(schema) = &self.schema else { return };
        let names: Vec<String> = schema.tables.iter().map(|t| t.name.clone()).collect();
        let mut level: HashMap<String, usize> = HashMap::new();
        fn lvl(s: &Schema, t: &str, level: &mut HashMap<String, usize>, depth: usize) -> usize {
            if let Some(l) = level.get(t) {
                return *l;
            }
            if depth > 30 {
                return 0;
            }
            let mut l = 0;
            for fk in s.fks.iter().filter(|f| f.table == t && f.ref_table != t) {
                l = l.max(lvl(s, &fk.ref_table, level, depth + 1) + 1);
            }
            level.insert(t.to_string(), l);
            l
        }
        for n in &names {
            lvl(schema, n, &mut level, 0);
        }
        let max_level = level.values().copied().max().unwrap_or(0);
        let mut x = 0.0;
        // Zu viele Tabellen in einer Spalte? Dann umbrechen.
        let per_col = ((names.len() as f32).sqrt().ceil() as usize).max(3);
        for l in 0..=max_level {
            let mut in_level: Vec<&TableInfo> = schema
                .tables
                .iter()
                .filter(|t| level.get(&t.name) == Some(&l))
                .collect();
            in_level.sort_by_key(|t| (t.is_view, t.name.clone()));
            for chunk in in_level.chunks(per_col) {
                let mut y = 0.0;
                let mut w: f32 = 0.0;
                for t in chunk {
                    let size = box_size(t, self.show_types);
                    if all || !self.pos.contains_key(&t.name) {
                        self.pos.insert(t.name.clone(), pos2(x, y));
                    }
                    y += size.y + 30.0;
                    w = w.max(size.x);
                }
                x += w + 80.0;
            }
        }
    }

    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + self.offset + p.to_vec2() * self.zoom
    }

    fn table_rect(&self, origin: Pos2, t: &TableInfo) -> Rect {
        let p = self.pos.get(&t.name).copied().unwrap_or_default();
        Rect::from_min_size(self.to_screen(origin, p), box_size(t, self.show_types) * self.zoom)
    }

    /// Anker (y) einer Spalte in Weltkoordinaten relativ zum Kasten.
    fn row_y(t: &TableInfo, col: &str) -> f32 {
        let i = t.columns.iter().position(|c| c.name == col).unwrap_or(0);
        HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0
    }

    /// Liefert die Linienpunkte einer Beziehung (in Weltkoordinaten).
    fn relation_points(&self, src: &TableInfo, col: &str, dst: &TableInfo, dcol: &str) -> (Pos2, f32, Pos2, f32) {
        let sp = self.pos.get(&src.name).copied().unwrap_or_default();
        let dp = self.pos.get(&dst.name).copied().unwrap_or_default();
        let ss = box_size(src, self.show_types);
        let ds = box_size(dst, self.show_types);
        let sy = sp.y + Self::row_y(src, col);
        let dy = dp.y + Self::row_y(dst, dcol);
        if src.name == dst.name {
            return (pos2(sp.x + ss.x, sy), 1.0, pos2(dp.x + ds.x, dy), 1.0);
        }
        let s_center = sp.x + ss.x / 2.0;
        let d_center = dp.x + ds.x / 2.0;
        if sp.x + ss.x + 20.0 < dp.x {
            (pos2(sp.x + ss.x, sy), 1.0, pos2(dp.x, dy), -1.0)
        } else if dp.x + ds.x + 20.0 < sp.x {
            (pos2(sp.x, sy), -1.0, pos2(dp.x + ds.x, dy), 1.0)
        } else if s_center <= d_center {
            (pos2(sp.x, sy), -1.0, pos2(dp.x, dy), -1.0)
        } else {
            (pos2(sp.x + ss.x, sy), 1.0, pos2(dp.x + ds.x, dy), 1.0)
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, style::SHADOW), StrokeKind::Inside);
        let origin = rect.min;
        let Some(schema) = self.schema.clone() else { return };

        // Zoom mit Mausrad
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.25, 3.0);
                if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                    let world = (mouse - origin - self.offset) / old;
                    self.offset = mouse - origin - world * self.zoom;
                }
            }
        }

        // Ziehen: Tabelle verschieben oder Ansicht bewegen
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hit = |this: &Self, p: Pos2| -> Option<String> {
            schema
                .tables
                .iter()
                .rev()
                .find(|t| this.table_rect(origin, t).contains(p))
                .map(|t| t.name.clone())
        };
        if resp.drag_started() {
            self.dragging = pointer.and_then(|p| hit(self, p));
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match &self.dragging {
                Some(name) => {
                    if let Some(p) = self.pos.get_mut(name) {
                        *p += d / self.zoom;
                    }
                }
                None => self.offset += d,
            }
        }
        if resp.drag_stopped() {
            if self.dragging.is_some() {
                self.save_layout();
            }
            self.dragging = None;
        }
        if resp.double_clicked() {
            if let Some(t) = pointer.and_then(|p| hit(self, p)) {
                cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: t });
            }
        }
        if resp.secondary_clicked() {
            self.context_table = pointer.and_then(|p| hit(self, p));
        }
        let ctx_table = self.context_table.clone();
        let db = self.db.clone();
        resp.context_menu(|ui| {
            if let Some(t) = &ctx_table {
                ui.label(egui::RichText::new(t).strong());
                if ui.button("Daten anzeigen").clicked() {
                    cx.actions.push(Action::OpenData { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button("Struktur bearbeiten").clicked() {
                    cx.actions.push(Action::OpenStructure { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button("SELECT-Abfrage erzeugen").clicked() {
                    cx.actions.push(Action::OpenSql {
                        db: Some(db.clone()),
                        sql: format!("SELECT * FROM {} LIMIT 100;", crate::db::q(t)),
                        run: true,
                    });
                    ui.close();
                }
            } else {
                if ui.button("Neu anordnen").clicked() {
                    self.auto_layout(true);
                    self.save_layout();
                    ui.close();
                }
                if ui.button("Neue Tabelle...").clicked() {
                    cx.actions.push(Action::NewTable(db.clone()));
                    ui.close();
                }
            }
        });

        let painter = painter.with_clip_rect(rect);
        let z = self.zoom;

        // Beziehungslinien
        let line = Stroke::new((1.4 * z).max(1.0), LINE);
        for fk in &schema.fks {
            let (Some(src), Some(dst)) = (schema.table(&fk.table), schema.table(&fk.ref_table)) else {
                continue;
            };
            let (a, da, b, db_) = self.relation_points(src, &fk.columns[0], dst, &fk.ref_columns[0]);
            let a = self.to_screen(origin, a);
            let b = self.to_screen(origin, b);
            let a1 = a + vec2(da * 16.0 * z, 0.0);
            let b1 = b + vec2(db_ * 16.0 * z, 0.0);
            let bend = ((b1.x - a1.x).abs() * 0.5).max(40.0 * z);
            painter.line_segment([a, a1], line);
            painter.line_segment([b, b1], line);
            painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
                [a1, a1 + vec2(da * bend, 0.0), b1 + vec2(db_ * bend, 0.0), b1],
                false,
                Color32::TRANSPARENT,
                line,
            )));
            // "viele"-Seite: Kraehenfuss
            let f = a + vec2(da * 11.0 * z, 0.0);
            painter.line_segment([f, a + vec2(0.0, -6.0 * z)], line);
            painter.line_segment([f, a + vec2(0.0, 6.0 * z)], line);
            // "eins"-Seite: zwei Striche
            for k in [6.0, 10.0] {
                let p = b + vec2(db_ * k * z, 0.0);
                painter.line_segment([p + vec2(0.0, -5.0 * z), p + vec2(0.0, 5.0 * z)], line);
            }
        }

        // Tabellen
        let font = FontId::proportional(12.0 * z);
        let bold = FontId::proportional(12.5 * z);
        let small = FontId::proportional(9.5 * z);
        for t in &schema.tables {
            let r = self.table_rect(origin, t);
            if !r.intersects(rect) {
                continue;
            }
            painter.rect_filled(r.translate(vec2(3.0, 3.0) * z), 0.0, Color32::from_black_alpha(25));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            let head = Rect::from_min_size(r.min, vec2(r.width(), HEADER_H * z));
            painter.rect_filled(head, 0.0, if t.is_view { VIEW_FILL } else { HEADER_FILL });
            painter.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, BOX_STROKE));
            let title = if t.is_view { format!("{} (Sicht)", t.name) } else { t.name.clone() };
            painter.text(head.left_center() + vec2(6.0 * z, 0.0), Align2::LEFT_CENTER, title, bold.clone(), Color32::BLACK);
            for (i, c) in t.columns.iter().enumerate() {
                let y = r.top() + (HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0) * z;
                let is_fk = schema.is_fk_column(&t.name, &c.name);
                let mut x = r.left() + 5.0 * z;
                if c.is_pk() {
                    painter.text(pos2(x, y), Align2::LEFT_CENTER, "PK", small.clone(), PK_COLOR);
                } else if is_fk {
                    painter.text(pos2(x, y), Align2::LEFT_CENTER, "FK", small.clone(), FK_COLOR);
                }
                if c.is_pk() && is_fk {
                    painter.text(pos2(x, y + 6.0 * z), Align2::LEFT_CENTER, "FK", FontId::proportional(7.0 * z), FK_COLOR);
                }
                x += 22.0 * z;
                let name_color = if c.is_pk() { Color32::BLACK } else { Color32::from_gray(0x20) };
                let nf = if c.is_pk() { bold.clone() } else { font.clone() };
                let mut label = c.name.clone();
                if !c.nullable && !c.is_pk() {
                    label.push_str(" *");
                }
                painter.text(pos2(x, y), Align2::LEFT_CENTER, label, nf, name_color);
                if self.show_types {
                    painter.text(pos2(r.right() - 6.0 * z, y), Align2::RIGHT_CENTER, &c.col_type, font.clone(), TYPE_COLOR);
                }
            }
            let stroke_col = if self.dragging.as_deref() == Some(&t.name) { style::ACCENT } else { BOX_STROKE };
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, stroke_col), StrokeKind::Inside);
        }

        if schema.tables.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Diese Datenbank enthält noch keine Tabellen.", FontId::proportional(14.0), Color32::GRAY);
        }
    }

    fn to_svg(&self) -> String {
        let Some(schema) = &self.schema else { return String::new() };
        let mut max = pos2(0.0, 0.0);
        for t in &schema.tables {
            let p = self.pos.get(&t.name).copied().unwrap_or_default();
            let s = box_size(t, self.show_types);
            max = max.max(p + s);
        }
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let pad = 20.0;
        let mut o = String::new();
        let _ = writeln!(
            o,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" font-family="Segoe UI, Arial, sans-serif" font-size="12">"#,
            max.x + 2.0 * pad,
            max.y + 2.0 * pad
        );
        let _ = writeln!(o, r#"<rect width="100%" height="100%" fill="white"/><g transform="translate({pad},{pad})">"#);
        for fk in &schema.fks {
            let (Some(src), Some(dst)) = (schema.table(&fk.table), schema.table(&fk.ref_table)) else { continue };
            let (a, da, b, db_) = self.relation_points(src, &fk.columns[0], dst, &fk.ref_columns[0]);
            let a1 = a + vec2(da * 16.0, 0.0);
            let b1 = b + vec2(db_ * 16.0, 0.0);
            let bend = ((b1.x - a1.x).abs() * 0.5).max(40.0);
            let _ = writeln!(
                o,
                r##"<path d="M{} {} L{} {} C{} {} {} {} {} {} L{} {}" fill="none" stroke="#305090" stroke-width="1.4"/>"##,
                a.x, a.y, a1.x, a1.y, a1.x + da * bend, a1.y, b1.x + db_ * bend, b1.y, b1.x, b1.y, b.x, b.y
            );
            let f = a.x + da * 11.0;
            let _ = writeln!(o, r##"<path d="M{f} {} L{} {} M{f} {} L{} {}" stroke="#305090" stroke-width="1.4"/>"##, a.y, a.x, a.y - 6.0, a.y, a.x, a.y + 6.0);
            for k in [6.0, 10.0] {
                let x = b.x + db_ * k;
                let _ = writeln!(o, r##"<line x1="{x}" y1="{}" x2="{x}" y2="{}" stroke="#305090" stroke-width="1.4"/>"##, b.y - 5.0, b.y + 5.0);
            }
        }
        for t in &schema.tables {
            let p = self.pos.get(&t.name).copied().unwrap_or_default();
            let s = box_size(t, self.show_types);
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
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" text-anchor="end" fill="#707070">{}</text>"##, p.x + s.x - 6.0, esc(&c.col_type));
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
            self.load(cx);
            for (k, v) in pos {
                self.pos.insert(k, v);
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
            if ui.button("Neu anordnen").clicked() {
                self.auto_layout(true);
                self.save_layout();
            }
            ui.checkbox(&mut self.show_types, "Datentypen");
            ui.separator();
            if ui.button("−").clicked() {
                self.zoom = (self.zoom / 1.2).max(0.25);
            }
            ui.label(format!("{:.0} %", self.zoom * 100.0));
            if ui.button("+").clicked() {
                self.zoom = (self.zoom * 1.2).min(3.0);
            }
            if ui.button("100 %").clicked() {
                self.zoom = 1.0;
                self.offset = vec2(20.0, 20.0);
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
                egui::RichText::new(format!(
                    "{} Tabellen, {} Beziehungen – Kästen mit der Maus verschieben, Hintergrund ziehen = Ansicht bewegen, Mausrad = Zoom, Doppelklick = Struktur, Rechtsklick = Menü.  PK = Primärschlüssel, FK = Fremdschlüssel, * = Pflichtfeld",
                    s.tables.len(),
                    s.fks.len()
                ))
                .small()
                .color(style::NULL_TEXT),
            );
        }
        if let Some(e) = &self.error {
            ui.label(egui::RichText::new(e).color(style::ERROR_TEXT));
        }
        self.draw(ui, cx);
    }
}
