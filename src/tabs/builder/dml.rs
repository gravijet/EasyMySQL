// Daten aendern im Assistenten: INSERT (Werte oder Abfrage), UPDATE und DELETE mit denselben Bedingungen wie bei SELECT.

use super::model::{Query, Select, Source, value_sql};
use super::{Env, cond_group_ui, query_ui, section};
use crate::db::q;
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Select,
    Insert,
    Update,
    Delete,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Select, Mode::Insert, Mode::Update, Mode::Delete];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Select => crate::i18n::text("Abfragen"),
            Mode::Insert => crate::i18n::text("Einfügen"),
            Mode::Update => crate::i18n::text("Ändern"),
            Mode::Delete => crate::i18n::text("Löschen"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct Dml {
    pub table: String,
    /// INSERT: gewaehlte Spalten und je Zeile ein Wert dazu
    #[serde(default)]
    pub cols: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    /// INSERT ... SELECT statt VALUES
    #[serde(default)]
    pub from_query: bool,
    /// UPDATE: Spalte = Wert
    #[serde(default)]
    pub sets: Vec<(String, String)>,
    /// Hilfs-SELECT mit der Zieltabelle als einziger Quelle; seine Bedingungen sind das WHERE
    #[serde(default)]
    pub target: Select,
}

impl Dml {
    /// Zieltabelle wechseln: Spalten und Bedingungen beginnen von vorn
    fn set_table(&mut self, name: &str, env: &Env) {
        self.table = name.to_string();
        self.target = Select { sources: vec![Source::table(name)], ..Select::default() };
        self.sets.clear();
        self.rows.clear();
        self.cols = env
            .schema
            .and_then(|s| s.table(name))
            .map(|t| t.columns.iter().filter(|c| !c.extra.to_ascii_lowercase().contains("auto_increment")).map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        self.rows.push(vec![String::new(); self.cols.len()]);
    }

    fn where_sql(&self) -> String {
        match self.target.filter.sql(0) {
            Some(c) => format!(" WHERE {c}"),
            None => String::new(),
        }
    }

    /// SQL der Anweisung; leer, solange noch etwas fehlt
    pub fn sql(&self, mode: Mode, query: &Query) -> String {
        if self.table.is_empty() {
            return String::new();
        }
        let t = q(&self.table);
        match mode {
            Mode::Select => query.sql(),
            Mode::Delete => format!("DELETE FROM {t}{}", self.where_sql()),
            Mode::Update => {
                let sets: Vec<String> = self.sets.iter().filter(|(c, _)| !c.is_empty()).map(|(c, v)| format!("{} = {}", q(c), value_sql(v))).collect();
                if sets.is_empty() {
                    return String::new();
                }
                format!("UPDATE {t} SET {}{}", sets.join(", "), self.where_sql())
            }
            Mode::Insert => {
                let cols = if self.cols.is_empty() { String::new() } else { format!(" ({})", self.cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ")) };
                if self.from_query {
                    return format!("INSERT INTO {t}{cols} {}", query.sql());
                }
                if self.cols.is_empty() {
                    return format!("INSERT INTO {t} () VALUES ()");
                }
                let rows: Vec<String> = self
                    .rows
                    .iter()
                    .map(|r| format!("({})", (0..self.cols.len()).map(|i| r.get(i).filter(|v| !v.trim().is_empty()).map_or("DEFAULT".to_string(), |v| value_sql(v))).collect::<Vec<_>>().join(", ")))
                    .collect();
                format!("INSERT INTO {t}{cols} VALUES {}", rows.join(", "))
            }
        }
    }

    /// Frage vor dem Ausfuehren
    pub fn question(&self, mode: Mode) -> String {
        match mode {
            Mode::Insert => crate::tr_format!("Zeilen in „{}“ einfügen?", "Insert rows into “{}”?", self.table),
            Mode::Update => crate::tr_format!("Zeilen in „{}“ ändern?", "Update rows in “{}”?", self.table),
            Mode::Delete => crate::tr_format!("Zeilen aus „{}“ löschen?", "Delete rows from “{}”?", self.table),
            Mode::Select => String::new(),
        }
    }
}

fn warn(ui: &mut egui::Ui, text: String) {
    ui.label(RichText::new(text).color(crate::style::pal().error_text));
}

/// Eingabe fuer INSERT, UPDATE und DELETE
pub fn ui(ui: &mut egui::Ui, dml: &mut Dml, mode: Mode, query: &mut Query, env: &Env, id: egui::Id) {
    // Zieltabelle: zunaechst die der Abfrage, sonst die erste
    if dml.table.is_empty() {
        let first = match query.parts.first().and_then(|p| p.select.sources.first()).map(|s| &s.from) {
            Some(super::model::From::Table(t)) if env.tables.contains(t) => Some(t.clone()),
            _ => env.tables.first().cloned(),
        };
        if let Some(t) = first {
            dml.set_table(&t, env);
        }
    }
    ui.horizontal(|ui| {
        super::keyword(ui, match mode {
            Mode::Insert => "INSERT INTO",
            Mode::Update => "UPDATE",
            _ => "DELETE FROM",
        });
        let mut name = dml.table.clone();
        if crate::tabs::str_combo(ui, id.with("table"), &env.tables, &mut name, 220.0) || (dml.table.is_empty() && !name.is_empty()) {
            dml.set_table(&name, env);
        }
    });
    if dml.table.is_empty() {
        return;
    }
    let cols: Vec<(String, String)> = env.schema.and_then(|s| s.table(&dml.table)).map(|t| t.columns.iter().map(|c| (c.name.clone(), c.col_type.clone())).collect()).unwrap_or_default();
    let inner = env.inside(&dml.target);
    match mode {
        Mode::Insert => {
            crate::widgets::check(ui, &mut dml.from_query, crate::i18n::text("Werte aus einer Abfrage holen (INSERT … SELECT)"));
            section(ui, id.with("cols"), "", crate::i18n::text("Spalten"), true, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (name, ty) in &cols {
                        let mut on = dml.cols.contains(name);
                        if crate::widgets::check(ui, &mut on, name).on_hover_text(ty).changed() {
                            if on {
                                // Reihenfolge wie in der Tabelle
                                let mut next: Vec<String> = cols.iter().map(|c| c.0.clone()).filter(|c| dml.cols.contains(c) || c == name).collect();
                                std::mem::swap(&mut dml.cols, &mut next);
                            } else {
                                dml.cols.retain(|c| c != name);
                            }
                            let n = dml.cols.len();
                            for r in dml.rows.iter_mut() {
                                r.resize(n, String::new());
                            }
                        }
                    }
                });
            });
            if dml.from_query {
                ui.label(RichText::new(crate::i18n::text("Die Abfrage unten liefert die Werte – ihre Spalten müssen zu den gewählten passen.")).small().color(crate::style::pal().text_weak));
                query_ui(ui, query, env, id.with("query"));
            } else {
                section(ui, id.with("values"), "VALUES", crate::i18n::text("Werte"), true, |ui| {
                    let n = dml.cols.len();
                    let mut remove = None;
                    egui::Grid::new(id.with("grid")).num_columns(n + 1).spacing([6.0, 4.0]).show(ui, |ui| {
                        for c in &dml.cols {
                            ui.label(RichText::new(c).strong());
                        }
                        ui.label("");
                        ui.end_row();
                        for (ri, row) in dml.rows.iter_mut().enumerate() {
                            row.resize(n, String::new());
                            for (ci, v) in row.iter_mut().enumerate() {
                                ui.add_sized([130.0, 28.0], crate::widgets::edit(v).vertical_align(egui::Align::Center).hint_text(crate::i18n::text("leer = DEFAULT")).id_salt((id, ri, ci)));
                            }
                            if super::remove_button(ui) {
                                remove = Some(ri);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = remove
                        && dml.rows.len() > 1
                    {
                        dml.rows.remove(i);
                    }
                    if ui.button(crate::i18n::text("+ Zeile")).clicked() {
                        dml.rows.push(vec![String::new(); n]);
                    }
                    ui.label(RichText::new(crate::i18n::text("Text wird automatisch in Anführungszeichen gesetzt; Zahlen, NULL und DEFAULT bleiben, wie sie sind.")).small().color(crate::style::pal().text_weak));
                });
            }
        }
        Mode::Update => {
            section(ui, id.with("set"), "SET", crate::i18n::text("Neue Werte"), true, |ui| {
                let mut remove = None;
                for (i, (c, v)) in dml.sets.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let names: Vec<String> = cols.iter().map(|c| c.0.clone()).collect();
                        crate::tabs::str_combo(ui, id.with(("setcol", i)), &names, c, 160.0);
                        ui.label("=");
                        ui.add(crate::widgets::edit(v).desired_width(180.0).hint_text(crate::i18n::text("Wert")).id_salt((id, "setval", i)));
                        if super::remove_button(ui) {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    dml.sets.remove(i);
                }
                if ui.button(crate::i18n::text("+ Zuweisung")).clicked() {
                    dml.sets.push((cols.first().map(|c| c.0.clone()).unwrap_or_default(), String::new()));
                }
            });
            where_section(ui, dml, &inner, id, crate::tr_format!("Ohne Bedingung werden ALLE Zeilen von „{}“ geändert!", "Without a condition ALL rows of “{}” are changed!", dml.table));
        }
        Mode::Delete => {
            where_section(ui, dml, &inner, id, crate::tr_format!("Ohne Bedingung werden ALLE Zeilen von „{}“ gelöscht!", "Without a condition ALL rows of “{}” are deleted!", dml.table));
        }
        Mode::Select => {}
    }
}

fn where_section(ui: &mut egui::Ui, dml: &mut Dml, inner: &Env, id: egui::Id, warning: String) {
    section(ui, id.with("where"), "WHERE", crate::i18n::text("Bedingungen"), true, |ui| {
        cond_group_ui(ui, &mut dml.target.filter, inner, id.with("filter"));
        if dml.target.filter.sql(0).is_none() {
            warn(ui, warning);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::builder::model::{CmpOp, Cond, Expr, Quant};

    fn dml() -> Dml {
        Dml { table: "schueler".into(), cols: vec!["vorname".into(), "klasse_id".into()], rows: vec![vec!["Mia".into(), "3".into()], vec!["O'Neil".into(), String::new()]], target: Select { sources: vec![Source::table("schueler")], ..Select::default() }, ..Dml::default() }
    }

    #[test]
    fn insert_values_and_defaults() {
        let d = dml();
        assert_eq!(d.sql(Mode::Insert, &Query::default()), "INSERT INTO `schueler` (`vorname`, `klasse_id`) VALUES ('Mia', 3), ('O''Neil', DEFAULT)");
    }

    #[test]
    fn insert_from_query() {
        let mut d = dml();
        d.from_query = true;
        let mut qr = Query::default();
        qr.parts[0].select.sources = vec![Source::table("alt")];
        assert_eq!(d.sql(Mode::Insert, &qr), "INSERT INTO `schueler` (`vorname`, `klasse_id`) SELECT * FROM `alt`");
    }

    #[test]
    fn update_and_delete_use_the_condition() {
        let mut d = dml();
        d.sets = vec![("vorname".into(), "Max".into()), ("klasse_id".into(), "NULL".into())];
        assert_eq!(d.sql(Mode::Update, &Query::default()), "UPDATE `schueler` SET `vorname` = 'Max', `klasse_id` = NULL");
        assert_eq!(d.sql(Mode::Delete, &Query::default()), "DELETE FROM `schueler`");
        d.target.filter = Cond::Group { any: false, not: false, items: vec![Cond::Cmp { left: Expr::column("schueler", "id"), op: CmpOp::Eq, right: vec![Expr::Value("5".into())], quant: Quant::None, sub: None }] };
        assert_eq!(d.sql(Mode::Delete, &Query::default()), "DELETE FROM `schueler` WHERE `schueler`.`id` = 5");
        assert!(d.sql(Mode::Update, &Query::default()).ends_with("WHERE `schueler`.`id` = 5"));
        d.sets.clear();
        assert_eq!(d.sql(Mode::Update, &Query::default()), "");
    }
}
