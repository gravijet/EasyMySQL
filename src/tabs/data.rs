// Tabelleninhalt anzeigen, Werte einfuegen, aendern und loeschen.

use super::{Action, Ctx, TabView};
use crate::db::{self, Row, TableInfo, q};
use crate::grid::{self, GridEvent, GridState};
use crate::style;
use eframe::egui::{self, RichText};
use mysql::{Params, Value};

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
    info: Option<TableInfo>,
    loaded: bool,
    error: Option<String>,
    grid: GridState,
    insert: Option<Vec<(String, String, bool)>>,
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
            info: None,
            loaded: false,
            error: None,
            grid: GridState::default(),
            insert: None,
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

    pub fn reload(&mut self, cx: &mut Ctx) {
        self.loaded = true;
        self.error = None;
        self.grid.reset();
        let Some(dbc) = cx.db else {
            self.error = Some("Keine Verbindung.".into());
            return;
        };
        self.info = cx
            .schema(&self.db)
            .and_then(|s| s.table(&self.table).cloned());
        let mut sql = format!("SELECT * FROM {}{}", self.full_name(), self.where_clause());
        if !self.order.is_empty() {
            sql.push_str(&format!(" ORDER BY {}{}", q(&self.order), if self.desc { " DESC" } else { "" }));
        }
        sql.push_str(&format!(" LIMIT {}, {}", self.offset, self.limit));
        match dbc.query(&sql) {
            Ok(rs) => {
                self.columns = rs.columns;
                self.rows = rs.rows;
            }
            Err(e) => {
                self.error = Some(e);
                self.rows.clear();
            }
        }
        self.total = dbc
            .rows(&format!("SELECT COUNT(*) FROM {}{}", self.full_name(), self.where_clause()), ())
            .ok()
            .and_then(|r| r.first().and_then(|r| r[0].clone()))
            .and_then(|v| v.parse().ok());
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
                    cx.status("Keine Zeile geändert.");
                } else {
                    cx.status(format!("{}.{}: 1 Wert geändert.", self.table, self.columns[c]));
                }
                self.rows[r][c] = v;
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
        match dbc.exec(&sql, Params::Positional(params)) {
            Ok(_) => {
                cx.status("Zeile gelöscht.");
                self.reload(cx);
            }
            Err(e) => cx.error(e),
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
        egui::Window::new(format!("Neue Zeile in {}", self.table))
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
                            ui.add_enabled_ui(!*def, |ui| ui.add_sized([240.0, 20.0], egui::TextEdit::singleline(val).hint_text(ty)));
                            ui.checkbox(def, "Standard");
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Einfügen").clicked() {
                        save = true;
                    }
                    if ui.button("Abbrechen").clicked() {
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
                        cx.status(format!("Zeile in {} eingefügt.", self.table));
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
    fn title(&self) -> String {
        format!("{} (Daten)", self.table)
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
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}.{}", self.db, self.table)).strong());
            ui.separator();
            ui.label("WHERE:");
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(220.0)
                    .hint_text("Bedingung"),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.offset = 0;
                self.reload(cx);
            }
            ui.label("Sortieren:");
            let mut cols = vec![String::new()];
            cols.extend(self.columns.iter().cloned());
            if super::str_combo(ui, ("order", &self.table), &cols, &mut self.order, 120.0) {
                self.reload(cx);
            }
            if ui.checkbox(&mut self.desc, "absteigend").changed() {
                self.reload(cx);
            }
            if ui.button("Aktualisieren").clicked() {
                self.reload(cx);
            }
        });
        ui.horizontal(|ui| {
            let is_view = self.info.as_ref().map(|i| i.is_view).unwrap_or(false);
            if ui.add_enabled(!is_view, egui::Button::new("+ Neue Zeile")).clicked() {
                self.open_insert();
            }
            let sel = self.grid.selected;
            if ui
                .add_enabled(sel.is_some() && !is_view, egui::Button::new("Zeile löschen"))
                .clicked()
            {
                if let Some(r) = sel {
                    self.delete_row(cx, r);
                }
            }
            if ui.button("Struktur").clicked() {
                cx.actions.push(Action::OpenStructure {
                    db: self.db.clone(),
                    table: self.table.clone(),
                });
            }
            grid::export_menu(ui, &self.table, &self.columns, &self.rows);
            ui.separator();
            let total = self.total.unwrap_or(self.rows.len() as u64);
            if ui.add_enabled(self.offset > 0, egui::Button::new("<")).clicked() {
                self.offset = self.offset.saturating_sub(self.limit);
                self.reload(cx);
            }
            let to = self.offset + self.rows.len();
            ui.label(format!(
                "Zeilen {}–{} von {}",
                if self.rows.is_empty() { 0 } else { self.offset + 1 },
                to,
                total
            ));
            if ui
                .add_enabled((to as u64) < total, egui::Button::new(">"))
                .clicked()
            {
                self.offset += self.limit;
                self.reload(cx);
            }
            if !self.has_pk() && !is_view && self.info.is_some() {
                ui.label(RichText::new("kein Primärschlüssel").color(style::pal().error_text))
                    .on_hover_text("Änderungen werden über alle Spaltenwerte zugeordnet.");
            }
        });
        if let Some(e) = &self.error {
            ui.label(RichText::new(format!("Fehler: {e}")).color(style::pal().error_text));
        }
        let editable = !self.info.as_ref().map(|i| i.is_view).unwrap_or(false);
        let events = grid::show(ui, ("datagrid", &self.db, &self.table), &self.columns, &self.rows, editable, &mut self.grid);
        for ev in events {
            match ev {
                GridEvent::Edit(r, c, v) => self.update_cell(cx, r, c, v),
                GridEvent::DeleteRow(r) => self.delete_row(cx, r),
                GridEvent::Activate => {}
            }
        }
        self.insert_ui(ui, cx);
    }
}
