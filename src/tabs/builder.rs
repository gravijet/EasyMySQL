// Abfrage-Assistent: SELECT-Abfragen grafisch zusammenklicken.

use super::{Action, Ctx, TabView};
use crate::db::{self, ResultSet, Schema, lit, q, smart_lit};
use crate::grid::{self, GridState};
use crate::style;
use eframe::egui::{self, RichText};

struct Join {
    table: String,
    kind: String,
    on: String,
}

struct Cond {
    conj: String,
    col: String,
    op: String,
    value: String,
}

const OPS: &[&str] = &["=", "<>", "<", ">", "<=", ">=", "LIKE", "NOT LIKE", "IN", "IS NULL", "IS NOT NULL"];
const AGGS: &[&str] = &["", "COUNT", "SUM", "AVG", "MIN", "MAX"];

pub struct BuilderTab {
    pub db: String,
    main: String,
    joins: Vec<Join>,
    /// Ausgewaehlte Spalten: (tabelle, spalte, aggregat)
    cols: Vec<(String, String, String)>,
    conds: Vec<Cond>,
    order_col: String,
    order_desc: bool,
    distinct: bool,
    limit: String,
    result: Option<ResultSet>,
    error: Option<String>,
    grid: GridState,
    schema: Option<Schema>,
    loaded_for: String,
}

impl BuilderTab {
    pub fn new(db: Option<String>) -> Self {
        Self {
            db: db.unwrap_or_default(),
            main: String::new(),
            joins: Vec::new(),
            cols: Vec::new(),
            conds: Vec::new(),
            order_col: String::new(),
            order_desc: false,
            distinct: false,
            limit: "1000".into(),
            result: None,
            error: None,
            grid: GridState::default(),
            schema: None,
            loaded_for: String::new(),
        }
    }

    fn tables_in_query(&self) -> Vec<String> {
        let mut v = Vec::new();
        if !self.main.is_empty() {
            v.push(self.main.clone());
        }
        v.extend(self.joins.iter().map(|j| j.table.clone()));
        v
    }

    fn all_columns(&self) -> Vec<String> {
        let Some(s) = &self.schema else { return Vec::new() };
        let mut out = Vec::new();
        for t in self.tables_in_query() {
            if let Some(ti) = s.table(&t) {
                for c in &ti.columns {
                    out.push(format!("{}.{}", t, c.name));
                }
            }
        }
        out
    }

    fn qualified(col: &str) -> String {
        match col.split_once('.') {
            Some((t, c)) => format!("{}.{}", q(t), q(c)),
            None => q(col),
        }
    }

    /// Moegliche Verknuepfungen ueber Fremdschluessel.
    fn join_candidates(&self) -> Vec<(String, String)> {
        let Some(s) = &self.schema else { return Vec::new() };
        let inc = self.tables_in_query();
        let mut out = Vec::new();
        for fk in &s.fks {
            let cond = fk
                .columns
                .iter()
                .zip(&fk.ref_columns)
                .map(|(a, b)| format!("{}.{} = {}.{}", q(&fk.table), q(a), q(&fk.ref_table), q(b)))
                .collect::<Vec<_>>()
                .join(" AND ");
            if inc.contains(&fk.table) && !inc.contains(&fk.ref_table) {
                out.push((fk.ref_table.clone(), cond));
            } else if inc.contains(&fk.ref_table) && !inc.contains(&fk.table) {
                out.push((fk.table.clone(), cond));
            }
        }
        out
    }

    pub fn sql(&self) -> String {
        if self.main.is_empty() {
            return String::new();
        }
        let mut s = String::from("SELECT ");
        if self.distinct {
            s.push_str("DISTINCT ");
        }
        let has_agg = self.cols.iter().any(|c| !c.2.is_empty());
        if self.cols.is_empty() {
            s.push('*');
        } else {
            let parts: Vec<String> = self
                .cols
                .iter()
                .map(|(t, c, a)| {
                    let e = format!("{}.{}", q(t), q(c));
                    if a.is_empty() {
                        e
                    } else {
                        format!("{a}({e}) AS {}", q(&format!("{}_{}", a.to_lowercase(), c)))
                    }
                })
                .collect();
            s.push_str(&parts.join(", "));
        }
        s.push_str(&format!("\nFROM {}", q(&self.main)));
        for j in &self.joins {
            s.push_str(&format!("\n  {} {} ON {}", j.kind, q(&j.table), j.on));
        }
        let mut first = true;
        for c in &self.conds {
            if c.col.is_empty() {
                continue;
            }
            s.push_str(if first { "\nWHERE " } else { "" });
            if !first {
                s.push_str(&format!("\n  {} ", c.conj));
            }
            first = false;
            let col = Self::qualified(&c.col);
            let expr = match c.op.as_str() {
                "IS NULL" | "IS NOT NULL" => format!("{col} {}", c.op),
                "IN" => {
                    let vals: Vec<String> = c.value.split(',').map(|v| smart_lit(v.trim())).collect();
                    format!("{col} IN ({})", vals.join(", "))
                }
                "LIKE" | "NOT LIKE" => format!("{col} {} {}", c.op, lit(&c.value)),
                op => format!("{col} {op} {}", smart_lit(&c.value)),
            };
            s.push_str(&expr);
        }
        if has_agg {
            let group: Vec<String> = self
                .cols
                .iter()
                .filter(|c| c.2.is_empty())
                .map(|(t, c, _)| format!("{}.{}", q(t), q(c)))
                .collect();
            if !group.is_empty() {
                s.push_str(&format!("\nGROUP BY {}", group.join(", ")));
            }
        }
        if !self.order_col.is_empty() {
            s.push_str(&format!(
                "\nORDER BY {}{}",
                Self::qualified(&self.order_col),
                if self.order_desc { " DESC" } else { "" }
            ));
        }
        if let Ok(n) = self.limit.trim().parse::<u64>() {
            if n > 0 {
                s.push_str(&format!("\nLIMIT {n}"));
            }
        }
        s
    }

    fn run(&mut self, cx: &mut Ctx) {
        let sql = self.sql();
        if sql.is_empty() {
            return;
        }
        let Some(dbc) = cx.db else { return };
        self.grid.reset();
        match dbc.query_in(&self.db, &sql) {
            Ok(rs) => {
                cx.status(format!("Abfrage-Assistent: {} Zeile(n)", rs.rows.len()));
                self.result = Some(rs);
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.result = None;
            }
        }
    }
}

impl TabView for BuilderTab {
    fn title(&self) -> String {
        "Abfrage-Assistent".into()
    }

    fn session(&self) -> Option<String> {
        Some(format!("builder\t{}", self.db))
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.run(cx);
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db {
            self.schema = cx.schema(db);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if self.db.is_empty() {
            if let Some(d) = cx.databases.iter().find(|d| !db::SYSTEM_DATABASES.contains(&d.as_str())) {
                self.db = d.clone();
            }
        }
        if self.loaded_for != self.db {
            self.loaded_for = self.db.clone();
            self.schema = cx.schema(&self.db);
            self.main.clear();
            self.joins.clear();
            self.cols.clear();
            self.conds.clear();
            self.order_col.clear();
            self.result = None;
        }
        let tables: Vec<String> = self
            .schema
            .as_ref()
            .map(|s| s.tables.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default();

        ui.horizontal(|ui| {
            ui.label("Datenbank:");
            super::db_combo(ui, "bdb", cx.databases, &mut self.db);
            ui.label("Haupttabelle:");
            let old = self.main.clone();
            super::str_combo(ui, "bmain", &tables, &mut self.main, 170.0);
            if old != self.main {
                self.joins.clear();
                self.cols.clear();
                self.conds.clear();
                self.order_col.clear();
            }
            let cands = self.join_candidates();
            ui.add_enabled_ui(!cands.is_empty(), |ui| {
                ui.menu_button("+ Tabelle verknüpfen", |ui| {
                    ui.label(RichText::new("Über vorhandene Beziehungen (Fremdschlüssel):").small());
                    for (t, on) in &cands {
                        if ui.button(format!("{t}   ({on})")).clicked() {
                            self.joins.push(Join { table: t.clone(), kind: "INNER JOIN".into(), on: on.clone() });
                            ui.close();
                        }
                    }
                });
            });
        });
        if self.main.is_empty() {
            ui.add_space(8.0);
            ui.label("Bitte eine Haupttabelle wählen. Danach Spalten anhaken, Bedingungen hinzufügen und auf \"Ausführen\" klicken.");
            return;
        }

        let top_h = (ui.available_height() * 0.5).max(200.0);
        ui.allocate_ui(egui::vec2(ui.available_width(), top_h), |ui| {
            ui.horizontal_top(|ui| {
                // Tabellen und Spalten
                egui::ScrollArea::horizontal().id_salt("btables").max_width(ui.available_width() * 0.55).show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let mut remove_join = None;
                        for (ti, t) in self.tables_in_query().iter().enumerate() {
                            style::group_frame().show(ui, |ui| {
                              ui.vertical(|ui| {
                                ui.set_width(190.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(t).strong());
                                    if ti > 0 && ui.small_button("✖").on_hover_text("Verknüpfung entfernen").clicked() {
                                        remove_join = Some(ti - 1);
                                    }
                                });
                                if ti > 0 {
                                    let j = &mut self.joins[ti - 1];
                                    super::str_combo(ui, ("jk", ti), &["INNER JOIN".into(), "LEFT JOIN".into(), "RIGHT JOIN".into()], &mut j.kind, 120.0);
                                }
                                egui::ScrollArea::vertical().id_salt(("bcols", ti)).max_height(top_h - 70.0).show(ui, |ui| {
                                    if let Some(info) = self.schema.as_ref().and_then(|s| s.table(t)) {
                                        for c in &info.columns {
                                            let idx = self.cols.iter().position(|x| &x.0 == t && x.1 == c.name);
                                            let mut on = idx.is_some();
                                            ui.horizontal(|ui| {
                                                if ui.checkbox(&mut on, &c.name).on_hover_text(&c.col_type).changed() {
                                                    if on {
                                                        self.cols.push((t.clone(), c.name.clone(), String::new()));
                                                    } else if let Some(i) = idx {
                                                        self.cols.remove(i);
                                                    }
                                                }
                                                if let Some(i) = idx {
                                                    let agg = &mut self.cols[i].2;
                                                    egui::ComboBox::from_id_salt(("agg", t, &c.name))
                                                        .selected_text(if agg.is_empty() { "–" } else { agg.as_str() })
                                                        .width(60.0)
                                                        .show_ui(ui, |ui| {
                                                            for a in AGGS {
                                                                ui.selectable_value(agg, a.to_string(), if a.is_empty() { "–" } else { a });
                                                            }
                                                        });
                                                }
                                            });
                                        }
                                    }
                                });
                              });
                            });
                        }
                        if let Some(i) = remove_join {
                            let t = self.joins.remove(i).table;
                            self.cols.retain(|c| c.0 != t);
                        }
                    });
                });

                // Bedingungen, Sortierung, Optionen
                ui.vertical(|ui| {
                    let all = self.all_columns();
                    ui.label(RichText::new("Bedingungen (WHERE)").strong());
                    let mut remove = None;
                    egui::ScrollArea::vertical().id_salt("bconds").max_height(top_h - 120.0).show(ui, |ui| {
                        for (i, c) in self.conds.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if i > 0 {
                                    super::str_combo(ui, ("conj", i), &["AND".into(), "OR".into()], &mut c.conj, 50.0);
                                } else {
                                    ui.add_sized([62.0, 18.0], egui::Label::new(""));
                                }
                                super::str_combo(ui, ("ccol", i), &all, &mut c.col, 150.0);
                                let ops: Vec<String> = OPS.iter().map(|s| s.to_string()).collect();
                                super::str_combo(ui, ("cop", i), &ops, &mut c.op, 80.0);
                                if !c.op.starts_with("IS") {
                                    let hint = if c.op == "IN" { "1, 2, 3" } else if c.op.contains("LIKE") { "%text%" } else { "Wert" };
                                    ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(120.0).hint_text(hint));
                                }
                                if ui.small_button("✖").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                    });
                    if let Some(i) = remove {
                        self.conds.remove(i);
                    }
                    if ui.button("+ Bedingung").clicked() {
                        self.conds.push(Cond {
                            conj: "AND".into(),
                            col: all.first().cloned().unwrap_or_default(),
                            op: "=".into(),
                            value: String::new(),
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Sortieren nach:");
                        let mut items = vec![String::new()];
                        items.extend(all.iter().cloned());
                        super::str_combo(ui, "border", &items, &mut self.order_col, 150.0);
                        ui.checkbox(&mut self.order_desc, "absteigend");
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.distinct, "Keine doppelten Zeilen (DISTINCT)");
                        ui.label("Max. Zeilen:");
                        ui.add(egui::TextEdit::singleline(&mut self.limit).desired_width(60.0));
                    });
                });
            });
        });

        ui.separator();
        let sql = self.sql();
        ui.horizontal(|ui| {
            if ui.button("▶ Ausführen (F5)").clicked() {
                self.run(cx);
            }
            if ui.button("Im SQL-Editor öffnen").clicked() {
                cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql: format!("{sql};\n"), run: false });
            }
            if ui.button("SQL kopieren").clicked() {
                ui.ctx().copy_text(sql.clone());
            }
        });
        style::sunken_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(&sql).monospace());
        });
        if let Some(e) = &self.error {
            ui.label(RichText::new(format!("Fehler: {e}")).color(style::pal().error_text));
        }
        if let Some(rs) = &self.result {
            ui.label(RichText::new(format!("{} Zeile(n)", rs.rows.len())).color(style::pal().ok_text));
            grid::show(ui, "bgrid", &rs.columns, &rs.rows, false, &mut self.grid);
        }
    }
}
