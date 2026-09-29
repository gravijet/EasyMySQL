// Tabellen-Designer: neue Tabelle grafisch anlegen.

use super::{Action, Ctx, TabView};
use crate::db::{self, ColDef};
use crate::style;
use eframe::egui::{self, RichText};

pub struct DesignerTab {
    pub db: String,
    name: String,
    engine: String,
    comment: String,
    cols: Vec<ColDef>,
    show_sql: bool,
}

/// Eingabefeld fuer den Datentyp mit Auswahlliste gaengiger Typen.
pub fn type_field(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut String) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add(egui::TextEdit::singleline(value).desired_width(110.0));
        egui::ComboBox::from_id_salt(id)
            .selected_text("")
            .width(18.0)
            .show_ui(ui, |ui| {
                for t in db::COMMON_TYPES {
                    if ui.selectable_label(value == t, *t).clicked() {
                        *value = t.to_string();
                    }
                }
            });
    });
}

/// Auswahl eines Fremdschluessel-Ziels ("tabelle.spalte").
pub fn ref_field(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, targets: &[String], value: &mut String) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(if value.is_empty() { "–" } else { value.as_str() })
        .width(150.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), "– (kein Verweis)").clicked() {
                value.clear();
            }
            for t in targets {
                if ui.selectable_label(value == t, t).clicked() {
                    *value = t.clone();
                }
            }
        });
}

impl DesignerTab {
    pub fn new(db: String) -> Self {
        let mut id = ColDef::new("id", "INT");
        id.primary = true;
        id.auto_inc = true;
        id.not_null = true;
        Self {
            db,
            name: String::new(),
            engine: "InnoDB".into(),
            comment: String::new(),
            cols: vec![id, ColDef::new("", "VARCHAR(255)")],
            show_sql: true,
        }
    }

    fn sql(&self) -> String {
        db::create_table_sql(&self.db, self.name.trim(), &self.cols, &self.engine, &self.comment)
    }
}

impl TabView for DesignerTab {
    fn title(&self) -> String {
        if self.name.is_empty() {
            "Neue Tabelle".into()
        } else {
            format!("Neue Tabelle: {}", self.name)
        }
    }

    fn execute(&mut self, cx: &mut Ctx) {
        if self.name.trim().is_empty() {
            cx.error("Bitte einen Tabellennamen eingeben.");
            return;
        }
        if !self.cols.iter().any(|c| !c.name.trim().is_empty()) {
            cx.error("Die Tabelle braucht mindestens eine Spalte.");
            return;
        }
        let Some(dbc) = cx.db else { return };
        match dbc.exec(&self.sql(), ()) {
            Ok(_) => {
                cx.status(format!("Tabelle {} erstellt.", self.name));
                cx.actions.push(Action::SchemaChanged(self.db.clone()));
                cx.actions.push(Action::OpenStructure {
                    db: self.db.clone(),
                    table: self.name.trim().to_string(),
                });
                // Designer fuer die naechste Tabelle leeren
                *self = DesignerTab::new(self.db.clone());
            }
            Err(e) => cx.error(e),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let targets: Vec<String> = cx
            .schema(&self.db)
            .map(|s| {
                s.tables
                    .iter()
                    .filter(|t| !t.is_view)
                    .flat_map(|t| {
                        t.columns
                            .iter()
                            .filter(|c| c.is_pk() || c.key == "UNI")
                            .map(move |c| format!("{}.{}", t.name, c.name))
                    })
                    .collect()
            })
            .unwrap_or_default();

        egui::Grid::new("designer-head").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Datenbank:");
            super::db_combo(ui, "designer-db", cx.databases, &mut self.db);
            ui.end_row();
            ui.label("Tabellenname:");
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(250.0));
            ui.end_row();
            ui.label("Speicher-Engine:");
            super::str_combo(
                ui,
                "designer-engine",
                &["InnoDB".into(), "MyISAM".into(), "Aria".into(), "MEMORY".into()],
                &mut self.engine,
                120.0,
            );
            ui.end_row();
            ui.label("Kommentar:");
            ui.add(egui::TextEdit::singleline(&mut self.comment).desired_width(250.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Spalten").strong());
        style::group_frame().show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt("designer-cols")
                .max_height((ui.available_height() - 150.0).max(120.0))
                .show(ui, |ui| {
                    let mut remove = None;
                    let mut move_up = None;
                    egui::Grid::new("designer-grid")
                        .num_columns(11)
                        .striped(true)
                        .spacing([6.0, 3.0])
                        .show(ui, |ui| {
                            for h in ["Name", "Datentyp", "PK", "Nicht NULL", "Auto-Inkr.", "Eindeutig", "Standardwert", "Verweist auf (FK)", "Kommentar", "", ""] {
                                ui.label(RichText::new(h).strong());
                            }
                            ui.end_row();
                            for (i, c) in self.cols.iter_mut().enumerate() {
                                ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut c.name));
                                type_field(ui, ("dtype", i), &mut c.col_type);
                                ui.checkbox(&mut c.primary, "");
                                ui.checkbox(&mut c.not_null, "");
                                ui.checkbox(&mut c.auto_inc, "");
                                ui.checkbox(&mut c.unique, "");
                                ui.add_sized([90.0, 20.0], egui::TextEdit::singleline(&mut c.default));
                                ref_field(ui, ("dref", i), &targets, &mut c.references);
                                ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(&mut c.comment));
                                if ui.small_button("▲").on_hover_text("nach oben").clicked() && i > 0 {
                                    move_up = Some(i);
                                }
                                if ui.small_button("✖").on_hover_text("Spalte entfernen").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                    if let Some(i) = remove {
                        self.cols.remove(i);
                    }
                    if let Some(i) = move_up {
                        self.cols.swap(i, i - 1);
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("+ Spalte hinzufügen").clicked() {
                    self.cols.push(ColDef::new("", "VARCHAR(255)"));
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Tabelle erstellen").clicked() {
                self.execute(cx);
            }
            ui.checkbox(&mut self.show_sql, "SQL anzeigen");
            if ui.button("Im SQL-Editor öffnen").clicked() {
                cx.actions.push(Action::OpenSql {
                    db: Some(self.db.clone()),
                    sql: format!("{};\n", self.sql()),
                    run: false,
                });
            }
        });
        if self.show_sql {
            let mut sql = self.sql();
            style::sunken_frame().show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("designer-sql").show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut sql)
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .interactive(true)
                            .desired_width(f32::INFINITY),
                    );
                });
            });
        }
    }
}
