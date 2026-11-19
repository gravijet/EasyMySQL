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
    type_field_w(ui, id, value, 160.0);
}

/// Datentyp: Textfeld und Pfeilknopf zusammen genau `w` breit (der Typ wird nie abgeschnitten)
pub fn type_field_w(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut String, w: f32) {
    let h = crate::widgets::CONTROL_H;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_sized([w - h - 2.0, h], crate::widgets::edit(value).vertical_align(egui::Align::Center));
        let knopf = crate::icons::tool_button(ui, crate::icons::Icon::ChevronDown, crate::i18n::tr("Gängige Datentypen", "Common data types"));
        egui::Popup::menu(&knopf).id(egui::Id::new(id)).show(|ui| {
            crate::style::menu_item_look(ui);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for t in db::COMMON_TYPES {
                    if ui.selectable_label(value == t, *t).clicked() {
                        *value = t.to_string();
                        ui.close();
                    }
                }
            });
        });
    });
}

/// Auswahl eines Fremdschluessel-Ziels ("tabelle.spalte").
pub fn ref_field(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, targets: &[String], value: &mut String) {
    ref_field_w(ui, id, targets, value, 150.0);
}

pub fn ref_field_w(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, targets: &[String], value: &mut String, w: f32) {
    crate::widgets::combo(id)
        .selected_text(if value.is_empty() { egui::RichText::new(crate::i18n::tr("Kein Verweis", "No reference")).color(crate::style::pal().text_weak) } else { egui::RichText::new(value.as_str()) })
        .width(w)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), crate::i18n::tr("Kein Verweis", "No reference")).clicked() {
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
    fn database(&self) -> Option<String> {
        Some(self.db.clone()).filter(|d| !d.is_empty())
    }

    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::NewFile)
    }

    fn title(&self) -> String {
        if self.name.is_empty() {
            crate::i18n::text("Neue Tabelle").into()
        } else {
            crate::tr_format!("Neue Tabelle: {}", "New table: {}", self.name)
        }
    }

    fn execute(&mut self, cx: &mut Ctx) {
        if self.name.trim().is_empty() {
            cx.error(crate::i18n::text("Bitte einen Tabellennamen eingeben."));
            return;
        }
        if !self.cols.iter().any(|c| !c.name.trim().is_empty()) {
            cx.error(crate::i18n::text("Die Tabelle braucht mindestens eine Spalte."));
            return;
        }
        let Some(dbc) = cx.db else { return };
        match dbc.exec(&self.sql(), ()) {
            Ok(_) => {
                cx.status(crate::tr_format!("Tabelle {} erstellt.", "Table {} created.", self.name));
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

        // Kopf: dieselben Formularzeilen wie in den Einstellungen
        const FELD: f32 = 280.0;
        crate::widgets::row(ui, crate::i18n::tr("Datenbank", "Database"), |ui| {
            let show_system = super::SHOW_SYSTEM_DBS.load(std::sync::atomic::Ordering::Relaxed);
            let dbs: Vec<String> = cx.databases.iter().filter(|d| show_system || !super::is_system_db(d) || **d == self.db).cloned().collect();
            super::db_select(ui, "designer-db", &dbs, &mut self.db, FELD);
        });
        crate::widgets::row(ui, crate::i18n::tr("Tabellenname", "Table name"), |ui| {
            let r = ui.add(crate::widgets::edit(&mut self.name).hint_text(crate::i18n::tr("z. B. kunde", "e.g. customer")).desired_width(FELD));
            crate::widgets::focus_when_shown(ui, &r);
        });
        crate::widgets::row(ui, crate::i18n::tr("Speicher-Engine", "Storage engine"), |ui| {
            super::str_combo(ui, "designer-engine", &["InnoDB".into(), "Aria".into(), "MEMORY".into()], &mut self.engine, FELD);
        });
        crate::widgets::row(ui, crate::i18n::tr("Kommentar", "Comment"), |ui| {
            ui.add(crate::widgets::edit(&mut self.comment).hint_text(crate::i18n::tr("optional", "optional")).desired_width(FELD));
        });
        ui.add_space(12.0);
        let pal = style::pal();
        // Spaltentabelle mit festen Breiten: Kopf und Zeilen stehen genau untereinander
        const W: [f32; 10] = [150.0, 170.0, 44.0, 76.0, 76.0, 76.0, 120.0, 180.0, 160.0, 62.0];
        let h = crate::widgets::CONTROL_H;
        let mitte = |ui: &mut egui::Ui, w: f32, an: &mut bool, tip: &str| {
            crate::widgets::fixed(ui, w, h, |ui| {
                ui.add_space((w - 16.0) / 2.0);
                crate::widgets::check(ui, an, "").on_hover_text(tip);
            });
        };
        style::group_frame().show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt("designer-cols")
                .max_height((ui.available_height() - 170.0).max(120.0))
                .auto_shrink([true, true])
                .show(ui, |ui| {
                    let mut remove = None;
                    let mut move_up = None;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let kopf = ["Name", crate::i18n::text("Datentyp"), "PK", crate::i18n::text("Nicht NULL"), crate::i18n::text("Auto-Inkr."), crate::i18n::text("Eindeutig"), crate::i18n::text("Standardwert"), crate::i18n::text("Verweist auf (FK)"), crate::i18n::text("Kommentar"), ""];
                        for (k, (t, w)) in kopf.iter().zip(W).enumerate() {
                            let mittig = (2..=5).contains(&k);
                            crate::widgets::fixed(ui, w, 20.0, |ui| {
                                let g = ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(12.0), pal.text_weak);
                                if mittig {
                                    ui.add_space(((w - g.size().x) / 2.0).max(0.0));
                                }
                                ui.label(RichText::new(*t).size(12.0).color(pal.text_weak));
                            });
                        }
                    });
                    ui.add_space(2.0);
                    for (i, c) in self.cols.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.add_sized([W[0], h], crate::widgets::edit(&mut c.name).hint_text(crate::i18n::tr("spalte", "column")).vertical_align(egui::Align::Center));
                            type_field_w(ui, ("dtype", i), &mut c.col_type, W[1]);
                            mitte(ui, W[2], &mut c.primary, crate::i18n::tr("Primärschlüssel", "Primary key"));
                            mitte(ui, W[3], &mut c.not_null, crate::i18n::text("Nicht NULL"));
                            mitte(ui, W[4], &mut c.auto_inc, "AUTO_INCREMENT");
                            mitte(ui, W[5], &mut c.unique, "UNIQUE");
                            ui.add_sized([W[6], h], crate::widgets::edit(&mut c.default).vertical_align(egui::Align::Center));
                            ref_field_w(ui, ("dref", i), &targets, &mut c.references, W[7]);
                            ui.add_sized([W[8], h], crate::widgets::edit(&mut c.comment).vertical_align(egui::Align::Center));
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if ui.add_enabled_ui(i > 0, |ui| crate::icons::button_sized(ui, crate::icons::Icon::ArrowUp, crate::i18n::text("Nach oben"), 26.0)).inner.clicked() {
                                move_up = Some(i);
                            }
                            if crate::icons::button_sized(ui, crate::icons::Icon::Trash, crate::i18n::text("Spalte entfernen"), 26.0).clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        self.cols.remove(i);
                    }
                    if let Some(i) = move_up {
                        self.cols.swap(i, i - 1);
                    }
                });
            ui.add_space(4.0);
            if crate::widgets::add_button(ui, crate::i18n::text("+ Spalte hinzufügen")).clicked() {
                self.cols.push(ColDef::new("", "VARCHAR(255)"));
            }
        });
        ui.add_space(10.0);
        let fehlt = if self.name.trim().is_empty() {
            Some(crate::i18n::tr("Bitte einen Tabellennamen eingeben.", "Please enter a table name."))
        } else if !self.cols.iter().any(|c| !c.name.trim().is_empty()) {
            Some(crate::i18n::tr("Die Tabelle braucht mindestens eine Spalte.", "The table needs at least one column."))
        } else {
            None
        };
        ui.horizontal(|ui| {
            if ui.add_enabled_ui(fehlt.is_none(), |ui| crate::icons::primary_button(ui, crate::icons::Icon::Check, crate::i18n::text("Tabelle erstellen"))).inner.clicked() {
                self.execute(cx);
            }
            if ui.add_enabled_ui(fehlt.is_none(), |ui| crate::icons::text_button(ui, crate::icons::Icon::Code, crate::i18n::text("Im SQL-Editor öffnen"))).inner.clicked() {
                cx.actions.push(Action::OpenSql {
                    db: Some(self.db.clone()),
                    sql: format!("{};\n", self.sql()),
                    run: false,
                });
            }
            if let Some(f) = fehlt {
                ui.label(RichText::new(f).color(pal.text_weak));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::widgets::switch(ui, &mut self.show_sql, crate::i18n::text("SQL anzeigen"));
            });
        });
        if self.show_sql && fehlt.is_none() {
            ui.add_space(6.0);
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
