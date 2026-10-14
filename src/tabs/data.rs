// Tabelleninhalt anzeigen, Werte einfuegen, aendern und loeschen.

use super::{Action, Ctx, TabView};
use crate::db::{self, Row, TableInfo, q};
use crate::db::ColDef;
use crate::grid::{self, ColumnAction, Edit, GridEvent, GridState};
use crate::style;
use eframe::egui::{self, RichText};
use mysql::{Params, Value};

/// Laufende Seitenabfrage mit eigener Verbindung, damit sie sich bei Bedarf abbrechen laesst.
struct PageJob {
    rx: std::sync::mpsc::Receiver<Result<db::ResultSet, String>>,
    conn_id: std::sync::Arc<std::sync::atomic::AtomicU32>,
    info: db::ConnInfo,
}

impl Drop for PageJob {
    /// Schliessen oder Neuladen bricht eine noch laufende Abfrage auf dem Server ab.
    fn drop(&mut self) {
        let id = self.conn_id.load(std::sync::atomic::Ordering::Relaxed);
        if id != 0 && matches!(self.rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)) {
            let info = self.info.clone();
            std::thread::spawn(move || db::kill_query(&info, id));
        }
    }
}

/// Eine rueckgaengig machbare Aenderung
enum Undo {
    Cell { cond: String, params: Vec<Value>, column: String, old: Option<String> },
    DeleteRow { columns: Vec<String>, row: Vec<Value> },
}

/// Dialog zum Aendern oder Hinzufuegen einer Spalte
struct ColDlg {
    /// Name der bestehenden Spalte (None = neue Spalte)
    orig: Option<String>,
    def: ColDef,
    /// Neue Spalte hinter dieser einfuegen (None = am Ende)
    after: Option<String>,
}

/// Kennzahlen und haeufigste Werte einer Spalte
struct Stats {
    rows: u64,
    filled: u64,
    distinct: u64,
    min: Option<String>,
    max: Option<String>,
    avg: Option<String>,
    top: Vec<(Option<String>, u64)>,
}

struct Analysis {
    column: String,
    rx: Option<std::sync::mpsc::Receiver<Result<Stats, String>>>,
    result: Option<Result<Stats, String>>,
}

pub struct DataTab {
    pub db: String,
    pub table: String,
    filter: String,
    order: String,
    desc: bool,
    limit: usize,
    offset: usize,
    columns: Vec<String>,
    rows: Vec<Row>,
    total: Option<u64>,
    /// Zaehlen laeuft im Hintergrund: COUNT(*) auf einer riesigen Tabelle dauert Sekunden
    count_job: Option<std::sync::mpsc::Receiver<Option<u64>>>,
    /// Die Zeilen werden im Hintergrund gelesen (Sortieren einer riesigen Tabelle dauert)
    page_job: Option<PageJob>,
    info: Option<TableInfo>,
    loaded: bool,
    info_wait: bool,
    error: Option<String>,
    grid: GridState,
    insert: Option<Vec<(String, String, bool)>>,
    undo: Vec<Undo>,
    col_dlg: Option<ColDlg>,
    analysis: Option<Analysis>,
    /// Nach dem Einfuegen zur letzten Zeile springen
    go_last: bool,
    select_last: bool,
    /// Filter wurde gesetzt: im naechsten Bild neu laden
    pending_reload: bool,
}

impl DataTab {
    pub fn new(db: String, table: String) -> Self {
        Self {
            db,
            table,
            filter: String::new(),
            order: String::new(),
            desc: false,
            limit: 500,
            offset: 0,
            columns: Vec::new(),
            rows: Vec::new(),
            total: None,
            count_job: None,
            page_job: None,
            info: None,
            loaded: false,
            info_wait: false,
            error: None,
            grid: GridState::default(),
            insert: None,
            undo: Vec::new(),
            col_dlg: None,
            analysis: None,
            go_last: false,
            select_last: false,
            pending_reload: false,
        }
    }

    fn full_name(&self) -> String {
        format!("{}.{}", q(&self.db), q(&self.table))
    }

    fn where_clause(&self) -> String {
        if self.filter.trim().is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.filter.trim())
        }
    }

    /// Tabelleninfo aus der Struktur holen, sobald sie gelesen ist.
    fn update_info(&mut self, cx: &mut Ctx) {
        if let Some(s) = cx.schema(&self.db) {
            self.info = s.table(&self.table).cloned();
            self.info_wait = false;
        } else if !cx.schema_loading(&self.db) {
            self.info_wait = false;
        }
    }

    pub fn reload(&mut self, cx: &mut Ctx) {
        self.loaded = true;
        self.error = None;
        self.grid.reset();
        let Some(dbc) = cx.db else {
            self.error = Some(crate::i18n::text("Keine Verbindung.").into());
            return;
        };
        self.info = None;
        self.info_wait = true;
        self.update_info(cx);
        let mut sql = format!("SELECT * FROM {}{}", self.full_name(), self.where_clause());
        if !self.order.is_empty() {
            sql.push_str(&format!(" ORDER BY {}{}", q(&self.order), if self.desc { " DESC" } else { "" }));
        }
        sql.push_str(&format!(" LIMIT {}, {}", self.offset, self.limit));
        let (tx, rx) = std::sync::mpsc::channel();
        let conn_id = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (db, id) = (dbc.clone(), conn_id.clone());
        std::thread::spawn(move || {
            let r = db.new_conn().and_then(|mut conn| {
                id.store(conn.connection_id(), std::sync::atomic::Ordering::Relaxed);
                let mut out = db::run_script(&mut conn, &sql);
                match out.error.take() {
                    Some(e) => Err(e),
                    None => Ok(out.sets.into_iter().next().unwrap_or_default()),
                }
            });
            let _ = tx.send(r);
        });
        self.page_job = Some(PageJob { rx, conn_id, info: dbc.info.clone() });
        self.total = None;
        let pool = dbc.pool.clone();
        let sql = format!("SELECT COUNT(*) FROM {}{}", self.full_name(), self.where_clause());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use mysql::prelude::Queryable;
            let n = pool.get_conn().ok().and_then(|mut c| c.query_first::<u64, _>(sql).ok().flatten());
            let _ = tx.send(n);
        });
        self.count_job = Some(rx);
    }

    /// WHERE-Bedingung, die genau eine Zeile trifft.
    fn row_key(&self, r: usize) -> (String, Vec<Value>) {
        let row = &self.rows[r];
        let pk: Vec<String> = self.info.as_ref().map(|i| i.pk_columns()).unwrap_or_default();
        let key_cols: Vec<usize> = if !pk.is_empty() && pk.iter().all(|p| self.columns.contains(p)) {
            pk.iter()
                .map(|p| self.columns.iter().position(|c| c == p).unwrap())
                .collect()
        } else {
            (0..self.columns.len()).collect()
        };
        let mut conds = Vec::new();
        let mut params = Vec::new();
        for i in key_cols {
            conds.push(format!("{} <=> ?", q(&self.columns[i])));
            params.push(db::opt_to_value(&row[i]));
        }
        (conds.join(" AND "), params)
    }

    fn has_pk(&self) -> bool {
        self.info.as_ref().map(|i| !i.pk_columns().is_empty()).unwrap_or(false)
    }

    fn update_cell(&mut self, cx: &mut Ctx, r: usize, c: usize, v: Option<String>) {
        let Some(dbc) = cx.db else { return };
        let (cond, mut params) = self.row_key(r);
        let sql = format!(
            "UPDATE {} SET {} = ? WHERE {} LIMIT 1",
            self.full_name(),
            q(&self.columns[c]),
            cond
        );
        params.insert(0, db::opt_to_value(&v));
        match dbc.exec(&sql, Params::Positional(params)) {
            Ok(n) => {
                if n == 0 {
                    cx.status(crate::i18n::text("Keine Zeile geändert."));
                } else {
                    cx.status(crate::tr_format!("{}.{}: 1 Wert geändert.", "{}.{}: 1 value changed.", self.table, self.columns[c]));
                }
                let old = self.rows[r][c].clone();
                self.rows[r][c] = v;
                if n > 0 {
                    // Rueckgaengig: die Zeile wird nach der Aenderung gefunden (bei geaendertem Schluessel mit dem neuen Wert)
                    let (cond, params) = self.row_key(r);
                    self.undo.push(Undo::Cell { cond, params, column: self.columns[c].clone(), old });
                }
                if !self.has_pk() {
                    self.reload(cx);
                }
            }
            Err(e) => cx.error(e),
        }
    }

    fn delete_row(&mut self, cx: &mut Ctx, r: usize) {
        let Some(dbc) = cx.db else { return };
        let (cond, params) = self.row_key(r);
        let sql = format!("DELETE FROM {} WHERE {} LIMIT 1", self.full_name(), cond);
        let saved = (self.columns.clone(), self.rows[r].iter().map(db::opt_to_value).collect::<Vec<_>>());
        match dbc.exec(&sql, Params::Positional(params)) {
            Ok(_) => {
                cx.status(crate::i18n::text("Zeile gelöscht."));
                // Zeilen mit Binaerdaten lassen sich als Text nicht verlustfrei wiederherstellen
                let binary = self.info.as_ref().is_some_and(|i| i.columns.iter().any(|c| {
                    let t = c.col_type.to_lowercase();
                    t.contains("blob") || t.contains("binary")
                }));
                if !binary {
                    self.undo.push(Undo::DeleteRow { columns: saved.0, row: saved.1 });
                }
                self.reload(cx);
            }
            Err(e) => cx.error(e),
        }
    }

    /// Letzte Aenderung rueckgaengig machen
    fn undo(&mut self, cx: &mut Ctx) {
        let (Some(dbc), Some(op)) = (cx.db, self.undo.pop()) else { return };
        let res = match op {
            Undo::Cell { cond, mut params, column, old } => {
                params.insert(0, db::opt_to_value(&old));
                dbc.exec(&format!("UPDATE {} SET {} = ? WHERE {} LIMIT 1", self.full_name(), q(&column), cond), Params::Positional(params))
            }
            Undo::DeleteRow { columns, row } => {
                let marks = vec!["?"; columns.len()].join(", ");
                let cols: Vec<String> = columns.iter().map(|c| q(c)).collect();
                dbc.exec(&format!("INSERT INTO {} ({}) VALUES ({marks})", self.full_name(), cols.join(", ")), Params::Positional(row))
            }
        };
        match res {
            Ok(_) => {
                cx.status(crate::i18n::tr("Rückgängig gemacht.", "Undone."));
                self.reload(cx);
            }
            Err(e) => cx.error(e),
        }
    }

    /// ALTER TABLE ausfuehren; danach laden alle Registerkarten der Datenbank die Struktur neu
    fn alter(&mut self, cx: &mut Ctx, sql: &[String]) -> bool {
        let Some(dbc) = cx.db else { return false };
        for s in sql {
            if let Err(e) = dbc.exec(s, ()) {
                cx.error(format!("{e}\n\nSQL: {s}"));
                return false;
            }
        }
        cx.status(crate::i18n::text("Struktur geändert."));
        cx.actions.push(Action::SchemaChanged(self.db.clone()));
        true
    }

    fn column_action(&mut self, cx: &mut Ctx, ci: usize, action: ColumnAction) {
        let Some(name) = self.columns.get(ci).cloned() else { return };
        let Some(info) = self.info.clone() else { return };
        let Some(col) = info.columns.iter().find(|c| c.name == name).cloned() else { return };
        let full = self.full_name();
        match action {
            ColumnAction::Analyze => self.start_analysis(cx, name, &col),
            ColumnAction::Sort(desc) => {
                self.order = name;
                self.desc = desc;
                self.reload(cx);
            }
            _ if info.is_view => cx.status(crate::i18n::tr("Sichten lassen sich hier nicht ändern.", "Views cannot be changed here.")),
            ColumnAction::Edit | ColumnAction::Move(_) | ColumnAction::Duplicate if col.is_generated() => cx.status(crate::i18n::tr("Berechnete Spalten lassen sich hier nicht ändern.", "Generated columns cannot be changed here.")),
            ColumnAction::Edit => self.col_dlg = Some(ColDlg { orig: Some(name), def: ColDef::from_info(&col), after: None }),
            ColumnAction::AddAfter => self.col_dlg = Some(ColDlg { orig: None, def: ColDef::new("", "VARCHAR(255)"), after: Some(name) }),
            ColumnAction::Move(m) => {
                if let Some(sql) = db::move_column(&full, &info, &name, m) {
                    self.alter(cx, &[sql]);
                }
            }
            ColumnAction::Duplicate => {
                if let Some(sql) = db::duplicate_column(&full, &info, &name) {
                    self.alter(cx, &sql);
                }
            }
            ColumnAction::Delete => cx.actions.push(Action::Confirm {
                text: crate::tr_format!("Spalte \"{}\" wirklich löschen? Alle Werte darin gehen verloren.", "Really delete column \"{}\"? All its values will be lost.", name),
                db: None,
                sql: format!("ALTER TABLE {full} DROP COLUMN {}", q(&name)),
            }),
        }
    }

    /// Kennzahlen im Hintergrund berechnen (bei grossen Tabellen dauert das)
    fn start_analysis(&mut self, cx: &mut Ctx, name: String, col: &db::ColumnInfo) {
        let Some(dbc) = cx.db else { return };
        let (dbc, full, c) = (dbc.clone(), self.full_name(), q(&name));
        let numeric = {
            let t = col.col_type.to_lowercase();
            ["int", "decimal", "numeric", "float", "double", "real", "bit"].iter().any(|k| t.contains(k))
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let run = || -> Result<Stats, String> {
                let avg = if numeric { format!("CAST(AVG({c}) AS CHAR)") } else { "NULL".into() };
                let r = dbc.rows(&format!("SELECT COUNT(*), COUNT({c}), COUNT(DISTINCT {c}), CAST(MIN({c}) AS CHAR), CAST(MAX({c}) AS CHAR), {avg} FROM {full}"), ())?;
                let r = r.into_iter().next().ok_or("keine Daten")?;
                let n = |i: usize| r[i].as_deref().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                let top = dbc.rows(&format!("SELECT CAST({c} AS CHAR), COUNT(*) AS n FROM {full} GROUP BY {c} ORDER BY n DESC LIMIT 8"), ())?;
                Ok(Stats {
                    rows: n(0),
                    filled: n(1),
                    distinct: n(2),
                    min: r[3].clone(),
                    max: r[4].clone(),
                    avg: r[5].clone(),
                    top: top.into_iter().map(|t| (t[0].clone(), t[1].as_deref().and_then(|v| v.parse().ok()).unwrap_or(0))).collect(),
                })
            };
            let _ = tx.send(run());
        });
        self.analysis = Some(Analysis { column: name, rx: Some(rx), result: None });
    }

    fn analysis_ui(&mut self, ctx: &egui::Context) {
        let Some(a) = self.analysis.as_mut() else { return };
        if let Some(rx) = &a.rx {
            match rx.try_recv() {
                Ok(r) => {
                    a.result = Some(r);
                    a.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(80)),
                Err(_) => a.rx = None,
            }
        }
        let pal = style::pal();
        let t = crate::i18n::tr;
        let mut open = true;
        let mut filter: Option<String> = None;
        egui::Window::new(crate::tr_format!("Spalte {}", "Column {}", a.column)).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            match &a.result {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(crate::i18n::text("Wird gelesen …"));
                    });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(pal.error_text));
                }
                Some(Ok(s)) => {
                    let pct = |n: u64| if s.rows == 0 { 0.0 } else { n as f64 * 100.0 / s.rows as f64 };
                    egui::Grid::new("stats").num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
                        let mut row = |k: &str, v: String| {
                            ui.label(RichText::new(k).color(pal.text_weak));
                            ui.label(v);
                            ui.end_row();
                        };
                        row(t("Zeilen", "Rows"), s.rows.to_string());
                        row(t("Leer (NULL)", "Empty (NULL)"), format!("{} ({:.1} %)", s.rows - s.filled, pct(s.rows - s.filled)));
                        row(t("Verschiedene Werte", "Distinct values"), s.distinct.to_string());
                        row(t("Doppelt vorkommend", "Repeated"), (s.filled - s.distinct).to_string());
                        if let Some(v) = &s.min {
                            row(t("Kleinster Wert", "Smallest"), v.chars().take(60).collect());
                        }
                        if let Some(v) = &s.max {
                            row(t("Größter Wert", "Largest"), v.chars().take(60).collect());
                        }
                        if let Some(v) = &s.avg {
                            row(t("Durchschnitt", "Average"), v.chars().take(20).collect());
                        }
                    });
                    if !s.top.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new(t("Häufigste Werte", "Most frequent values")).strong());
                        let most = s.top.first().map(|x| x.1).unwrap_or(1).max(1);
                        for (v, n) in &s.top {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(380.0, 22.0), egui::Sense::click());
                            let w = (r.width() * *n as f32 / most as f32).max(2.0);
                            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(w, r.height())), 3.0, pal.accent.gamma_multiply(if resp.hovered() { 0.55 } else { 0.35 }));
                            let label = v.clone().map(|x| x.chars().take(40).collect::<String>()).unwrap_or_else(|| "NULL".into());
                            ui.painter().text(egui::pos2(r.left() + 6.0, r.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), pal.text);
                            ui.painter().text(egui::pos2(r.right() - 6.0, r.center().y), egui::Align2::RIGHT_CENTER, n.to_string(), egui::FontId::proportional(12.0), pal.text);
                            if resp.on_hover_text(t("Klick zeigt diese Zeilen", "Click to show these rows")).clicked() {
                                filter = Some(match v {
                                    Some(x) => format!("{} = {}", q(&a.column), db::lit(x)),
                                    None => format!("{} IS NULL", q(&a.column)),
                                });
                            }
                        }
                    }
                }
            }
        });
        if let Some(f) = filter {
            self.filter = f;
            self.offset = 0;
            self.analysis = None;
            self.pending_reload = true;
        } else if !open {
            self.analysis = None;
        }
    }

    fn col_dlg_ui(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(d) = self.col_dlg.as_mut() else { return };
        let (mut ok, mut cancel) = (false, false);
        let title = match &d.orig {
            Some(o) => crate::tr_format!("Spalte {o}", "Column {o}"),
            None => crate::i18n::tr("Neue Spalte", "New column").to_string(),
        };
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("coldlg").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Name");
                let r = ui.add_sized([220.0, 28.0], crate::widgets::edit(&mut d.def.name).vertical_align(egui::Align::Center));
                if d.orig.is_none() && !r.has_focus() && d.def.name.is_empty() {
                    r.request_focus();
                }
                ui.end_row();
                ui.label(crate::i18n::text("Datentyp:"));
                super::designer::type_field(ui, "coldlg-type", &mut d.def.col_type);
                ui.end_row();
                ui.label("");
                crate::widgets::check(ui, &mut d.def.not_null, crate::i18n::text("Nicht NULL"));
                ui.end_row();
                ui.label(crate::i18n::text("Standardwert:"));
                ui.add_sized([220.0, 28.0], crate::widgets::edit(&mut d.def.default).vertical_align(egui::Align::Center));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.add_enabled(!d.def.name.trim().is_empty() && !d.def.col_type.trim().is_empty(), egui::Button::new("OK")).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !d.def.name.trim().is_empty());
                cancel = ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if ok {
            let d = self.col_dlg.take().unwrap();
            let full = self.full_name();
            let sql = match &d.orig {
                Some(o) => db::change_column(&full, o, &d.def),
                None => {
                    let pos = d.after.as_ref().map(|a| format!(" AFTER {}", q(a))).unwrap_or_default();
                    format!("ALTER TABLE {full} ADD COLUMN {}{pos}", d.def.sql())
                }
            };
            if !self.alter(cx, &[sql]) {
                self.col_dlg = Some(d);
            }
        } else if cancel {
            self.col_dlg = None;
        }
    }

    /// Neue Zeile sofort mit Standardwerten anlegen; geht das nicht (Pflichtfelder), fragt der Dialog die Werte ab.
    fn quick_insert(&mut self, cx: &mut Ctx) {
        let Some(dbc) = cx.db else { return };
        match dbc.exec(&format!("INSERT INTO {} () VALUES ()", self.full_name()), ()) {
            Ok(_) => {
                self.go_last = true;
                self.reload(cx);
            }
            Err(_) => self.open_insert(),
        }
    }

    fn open_insert(&mut self) {
        let fields = match &self.info {
            Some(info) => info
                .columns
                .iter()
                .map(|c| {
                    let auto = c.extra.to_lowercase().contains("auto_increment")
                        || c.default.is_some()
                        || c.nullable
                        || c.extra.to_lowercase().contains("generated");
                    (c.name.clone(), String::new(), auto)
                })
                .collect(),
            None => self.columns.iter().map(|c| (c.clone(), String::new(), false)).collect(),
        };
        self.insert = Some(fields);
    }

    fn insert_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let Some(fields) = self.insert.as_mut() else { return };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(crate::tr_format!("Neue Zeile in {}", "New row in {}", self.table))
            .id(egui::Id::new(("insert", &self.db, &self.table)))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ui.ctx().content_rect().center())
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    egui::Grid::new("insertgrid").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                        let info = self.info.as_ref();
                        for (name, val, def) in fields.iter_mut() {
                            let ty = info
                                .and_then(|i| i.columns.iter().find(|c| &c.name == name))
                                .map(|c| c.col_type.clone())
                                .unwrap_or_default();
                            ui.label(name.as_str()).on_hover_text(&ty);
                            ui.add_enabled_ui(!*def, |ui| ui.add_sized([240.0, 28.0], crate::widgets::edit(val).vertical_align(egui::Align::Center).hint_text(ty)));
                            crate::widgets::check(ui, def, crate::i18n::text("Standard"));
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::text("Einfügen")).clicked() {
                        save = true;
                    }
                    if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            let fields = self.insert.clone().unwrap();
            let used: Vec<&(String, String, bool)> = fields.iter().filter(|f| !f.2).collect();
            let sql = if used.is_empty() {
                format!("INSERT INTO {} () VALUES ()", self.full_name())
            } else {
                format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    self.full_name(),
                    used.iter().map(|f| q(&f.0)).collect::<Vec<_>>().join(", "),
                    used.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
                )
            };
            let params: Vec<Value> = used
                .iter()
                .map(|f| {
                    if f.1.eq_ignore_ascii_case("null") {
                        Value::NULL
                    } else {
                        Value::Bytes(f.1.clone().into_bytes())
                    }
                })
                .collect();
            if let Some(dbc) = cx.db {
                match dbc.exec(&sql, if params.is_empty() { Params::Empty } else { Params::Positional(params) }) {
                    Ok(_) => {
                        cx.status(crate::tr_format!("Zeile in {} eingefügt.", "Row inserted into {}.", self.table));
                        self.insert = None;
                        self.reload(cx);
                    }
                    Err(e) => cx.error(e),
                }
            }
        } else if cancel || !open {
            self.insert = None;
        }
    }
}

impl TabView for DataTab {
    fn database(&self) -> Option<String> {
        Some(self.db.clone()).filter(|d| !d.is_empty())
    }

    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Table)
    }

    fn title(&self) -> String {
        crate::tr_format!("{} (Daten)", "{} (Data)", self.table)
    }

    fn key(&self) -> Option<String> {
        Some(format!("data:{}.{}", self.db, self.table))
    }

    fn session(&self) -> Option<String> {
        Some(format!("data\t{}\t{}", self.db, self.table))
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.reload(cx);
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db {
            self.reload(cx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if !self.loaded {
            self.reload(cx);
        } else if self.info_wait {
            self.update_info(cx);
        }
        if let Some(job) = &self.page_job {
            match job.rx.try_recv() {
                Ok(r) => {
                    match r {
                        Ok(rs) => {
                            self.columns = rs.columns;
                            self.rows = rs.rows;
                        }
                        Err(e) => {
                            self.error = Some(e);
                            self.rows.clear();
                        }
                    }
                    self.page_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(50)),
                Err(_) => self.page_job = None,
            }
        }
        if let Some(rx) = &self.count_job {
            match rx.try_recv() {
                Ok(n) => {
                    self.total = n;
                    self.count_job = None;
                    // Nach dem Einfuegen die letzte Seite zeigen
                    if std::mem::take(&mut self.go_last) {
                        let last = (n.unwrap_or(0).saturating_sub(1) as usize / self.limit) * self.limit;
                        self.select_last = true;
                        if last != self.offset {
                            self.offset = last;
                            self.reload(cx);
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
                Err(_) => self.count_job = None,
            }
        }
        if self.select_last && self.page_job.is_none() && self.count_job.is_none() && !self.rows.is_empty() {
            self.select_last = false;
            self.grid.selected = Some(self.rows.len() - 1);
            self.grid.scroll_to = Some(self.rows.len() - 1);
        }
        let pal = style::pal();
        let is_view = self.info.as_ref().map(|i| i.is_view).unwrap_or(false);
        // Strg+Z macht die letzte Aenderung rueckgaengig (nicht beim Tippen in einem Textfeld)
        if !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.undo(cx);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}.{}", self.db, self.table)).strong());
            ui.add_space(6.0);
            let w = (ui.available_width() - 46.0 - if self.order.is_empty() { 0.0 } else { 150.0 }).clamp(160.0, 560.0);
            let r = ui.add_sized([w, 28.0], crate::widgets::edit(&mut self.filter).vertical_align(egui::Align::Center).hint_text(crate::i18n::tr("WHERE-Bedingung", "WHERE condition")));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.offset = 0;
                self.reload(cx);
            }
            if crate::icons::button(ui, crate::icons::Icon::Refresh, crate::i18n::text("Aktualisieren")).clicked() {
                self.reload(cx);
            }
            if !self.order.is_empty() {
                let tip = crate::i18n::tr("Sortierung aufheben", "Clear sorting");
                if crate::icons::text_button(ui, crate::icons::Icon::Close, &format!("{} {}", self.order, if self.desc { "DESC" } else { "ASC" })).on_hover_text(tip).clicked() {
                    self.order.clear();
                    self.reload(cx);
                }
            }
        });
        ui.horizontal(|ui| {
            use crate::icons::{self, Icon};
            ui.add_enabled_ui(!is_view, |ui| {
                if icons::text_button(ui, Icon::Plus, crate::i18n::tr("Zeile", "Row")).on_hover_text(crate::i18n::tr("Neue Zeile", "New row")).clicked() {
                    self.quick_insert(cx);
                }
                if icons::text_button(ui, Icon::Plus, crate::i18n::tr("Spalte", "Column")).on_hover_text(crate::i18n::tr("Neue Spalte am Ende", "New column at the end")).clicked() {
                    self.col_dlg = Some(ColDlg { orig: None, def: ColDef::new("", "VARCHAR(255)"), after: self.columns.last().cloned() });
                }
                let sel = self.grid.selected;
                if ui.add_enabled_ui(sel.is_some(), |ui| icons::button(ui, Icon::Trash, crate::i18n::text("Zeile löschen"))).inner.clicked()
                    && let Some(r) = sel
                {
                    self.delete_row(cx, r);
                }
                let tip = format!("{} (Ctrl+Z)", crate::i18n::tr("Rückgängig", "Undo"));
                if ui.add_enabled_ui(!self.undo.is_empty(), |ui| icons::button(ui, Icon::Undo, &tip)).inner.clicked() {
                    self.undo(cx);
                }
            });
            ui.separator();
            if icons::button(ui, Icon::Columns, crate::i18n::tr("Struktur, Indizes und Beziehungen", "Structure, indexes and relationships")).clicked() {
                cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: self.table.clone() });
            }
            grid::export_menu(ui, &self.table, &self.columns, &self.rows);
            if !self.has_pk() && !is_view && self.info.is_some() {
                ui.label(RichText::new(crate::i18n::text("kein Primärschlüssel")).color(pal.error_text));
            }
            // Seitenwahl rechts
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let total = self.total.unwrap_or(self.rows.len() as u64);
                let to = self.offset + self.rows.len();
                if ui.add_enabled_ui((to as u64) < total, |ui| icons::button(ui, Icon::ChevronRight, crate::i18n::text("Nächste Seite"))).inner.clicked() {
                    self.offset += self.limit;
                    self.reload(cx);
                }
                ui.label(RichText::new(crate::tr_format!("Zeilen {}–{} von {}", "Rows {}–{} of {}", if self.rows.is_empty() { 0 } else { self.offset + 1 }, to, total)).color(pal.text_weak));
                if ui.add_enabled_ui(self.offset > 0, |ui| icons::button(ui, Icon::ChevronLeft, crate::i18n::text("Vorherige Seite"))).inner.clicked() {
                    self.offset = self.offset.saturating_sub(self.limit);
                    self.reload(cx);
                }
            });
        });
        if let Some(e) = &self.error {
            ui.label(RichText::new(crate::tr_format!("Fehler: {e}", "Error: {e}")).color(pal.error_text));
        }
        if self.page_job.is_some() {
            ui.label(RichText::new(crate::i18n::text("Wird gelesen …")).color(pal.text_weak));
            return;
        }
        let edit = if is_view { Edit::View } else { Edit::CellsAndColumns };
        let events = grid::show(ui, ("datagrid", &self.db, &self.table), &self.columns, &self.rows, edit, &mut self.grid);
        for ev in events {
            match ev {
                GridEvent::Edit(r, c, v) => self.update_cell(cx, r, c, v),
                GridEvent::DeleteRow(r) => self.delete_row(cx, r),
                GridEvent::Column(c, a) => self.column_action(cx, c, a),
                GridEvent::Activate => {}
            }
        }
        let ctx = ui.ctx().clone();
        self.analysis_ui(&ctx);
        if std::mem::take(&mut self.pending_reload) {
            self.reload(cx);
        }
        self.col_dlg_ui(&ctx, cx);
        self.insert_ui(ui, cx);
    }
}
