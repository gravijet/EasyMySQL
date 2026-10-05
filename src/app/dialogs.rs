// Kleine Dialoge (Meldung, Bestaetigung, neue Datenbank, Export, ...).

use super::*;
use crate::db::q;
use eframe::egui::{Key, RichText};

pub enum Dialog {
    Message { title: String, text: String, error: bool },
    Confirm { text: String, db: Option<String>, sql: String },
    NewDatabase { name: String, collation: String },
    Export { db: String, with_data: bool },
    About,
    Myisam(Vec<(String, String)>),
    /// Verbindung fehlgeschlagen
    ConnectFailed(String),
}

impl EasyApp {
    pub(super) fn dialogs_ui(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.dialogs.last_mut() else { return };
        let mut close = false;
        let mut run_sql: Option<(Option<String>, String)> = None;
        let mut export: Option<(String, bool)> = None;
        let mut create_db: Option<(String, String)> = None;
        let mut convert: Option<Vec<(String, String)>> = None;
        let mut open_settings = false;
        let mut retry = false;
        let pal = style::pal();

        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(14));
        let modal = egui::Modal::new(egui::Id::new("dialog")).frame(frame).show(ctx, |ui| {
            ui.set_max_width(520.0);
            match dlg {
                Dialog::Message { title, text, error } => {
                    ui.label(RichText::new(title.as_str()).strong());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        let t = RichText::new(text.as_str());
                        ui.label(if *error { t.color(pal.error_text) } else { t });
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            close = true;
                        }
                        if *error && ui.button(crate::i18n::text("Kopieren")).clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                    });
                }
                Dialog::Confirm { text, db, sql } => {
                    ui.label(RichText::new(text.as_str()).strong());
                    ui.add_space(4.0);
                    ui.label(RichText::new(sql.as_str()).monospace().small().color(pal.text_weak));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("Ja")).clicked() {
                            run_sql = Some((db.clone(), sql.clone()));
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Nein")).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            close = true;
                        }
                    });
                }
                Dialog::NewDatabase { name, collation } => {
                    ui.label(RichText::new(crate::i18n::text("Neue Datenbank")).strong());
                    ui.add_space(4.0);
                    egui::Grid::new("newdb").num_columns(2).show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(name).request_focus();
                        ui.end_row();
                        ui.label(crate::i18n::text("Sortierung"));
                        let items: Vec<String> = COLLATIONS.iter().map(|s| s.to_string()).collect();
                        crate::tabs::str_combo(ui, "coll", &items, collation, 180.0);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("Erstellen")).clicked() || (ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty()) {
                            create_db = Some((name.trim().to_string(), collation.clone()));
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Export { db, with_data } => {
                    ui.label(RichText::new(crate::i18n::text("Exportieren")).strong());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::text("Datenbank"));
                        crate::tabs::db_combo(ui, "expdb", &self.databases, db);
                    });
                    ui.checkbox(with_data, crate::i18n::text("Mit Daten"));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("Speichern unter …")).clicked() {
                            export = Some((db.clone(), *with_data));
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::About => {
                    ui.label(RichText::new(format!("EasyMySQL {}", env!("CARGO_PKG_VERSION"))).strong());
                    ui.add_space(4.0);
                    ui.label(crate::i18n::text("Enthält MariaDB (GPL v2)."));
                    if let Some(d) = &self.db {
                        ui.label(format!("Server: MariaDB {}", d.version));
                    }
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
                Dialog::Myisam(list) => {
                    ui.label(RichText::new(crate::i18n::text("Nicht absturzsichere Tabellen")).strong());
                    ui.add_space(4.0);
                    ui.label(crate::tr_format!("{} Tabelle(n) verwenden MyISAM und können bei einem Absturz beschädigt werden. InnoDB ist absturzsicher.", "{} table(s) use MyISAM and may be damaged by a crash. InnoDB is crash-safe.", list.len()));
                    let names: Vec<String> = list.iter().take(10).map(|(d, t)| format!("{d}.{t}")).collect();
                    ui.label(RichText::new(names.join(", ")).small());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("In InnoDB umwandeln")).clicked() {
                            convert = Some(list.clone());
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Später")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::ConnectFailed(e) => {
                    ui.label(RichText::new(crate::i18n::text("Verbindung fehlgeschlagen")).strong());
                    ui.add_space(4.0);
                    ui.label(RichText::new(e.as_str()).color(pal.error_text));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("Erneut versuchen")).clicked() {
                            retry = true;
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Verbindung einstellen")).clicked() {
                            open_settings = true;
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Schließen")).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if modal.should_close() && !matches!(self.dialogs.last(), Some(Dialog::Confirm { .. })) {
            close = true;
        }
        if close {
            self.dialogs.pop();
        }

        if let Some((db, sql)) = run_sql {
            self.run_confirmed(db, sql);
        }
        if open_settings {
            self.handle_actions(vec![Action::OpenSettings(Some("verbindung".into()))]);
        }
        if retry {
            self.connect(true);
        }
        if let (Some(list), Some(dbc)) = (convert, &self.db) {
            match crate::repair::convert_to_innodb(&dbc.info, &list, &|_| {}) {
                Ok(n) => self.toast(crate::tr_format!("{n} Tabelle(n) in InnoDB umgewandelt.", "{n} table(s) converted to InnoDB.")),
                Err(e) => self.error(e),
            }
        }
        if let Some((name, coll)) = create_db {
            if let Some(dbc) = &self.db {
                let charset = coll.split('_').next().unwrap_or("utf8mb4").to_string();
                let sql = format!("CREATE DATABASE {} CHARACTER SET {} COLLATE {}", q(&name), charset, coll);
                match dbc.exec(&sql, ()) {
                    Ok(_) => {
                        self.dialogs.pop();
                        self.toast(crate::tr_format!("Datenbank {name} erstellt.", "Database {name} created."));
                        self.refresh_all();
                        self.current_db = name;
                    }
                    Err(e) => self.error(e),
                }
            }
        }
        if let Some((d, with_data)) = export {
            if let Some(p) = rfd::FileDialog::new().add_filter(crate::i18n::text("SQL-Datei"), &["sql"]).set_file_name(format!("{d}.sql")).save_file() {
                match self.db.as_ref().map(|dbc| dbc.dump(&d, with_data)) {
                    Some(Ok(text)) => match std::fs::write(&p, text) {
                        Ok(_) => {
                            self.toast(crate::tr_format!("Gespeichert: {}", "Saved: {}", p.display()));
                            self.dialogs.pop();
                        }
                        Err(e) => self.error(e.to_string()),
                    },
                    Some(Err(e)) => self.error(e),
                    None => {}
                }
            }
        }
    }

    /// Bestaetigtes SQL ausfuehren (vorher bei Loeschen automatisch sichern)
    fn run_confirmed(&mut self, db: Option<String>, sql: String) {
        let targets = crate::backup::destructive_targets(&sql, db.as_deref());
        if !targets.is_empty() {
            self.update_backup_env();
            if let Some(env) = crate::backup::env() {
                if let Err(e) = crate::backup::create(&env, Some(targets), "vor-loeschen", &|_| {}) {
                    self.error(crate::tr_format!("Die Sicherung vor dem Löschen ist fehlgeschlagen – nichts wurde gelöscht.\n\n{e}", "The backup before deletion failed — nothing was deleted.\n\n{e}"));
                    return;
                }
            }
        }
        let Some(dbc) = &self.db else { return };
        let res = match &db {
            Some(d) => dbc.exec_in(d, &sql),
            None => dbc.exec(&sql, ()),
        };
        match res {
            Ok(_) => match db {
                Some(d) => self.handle_actions(vec![Action::SchemaChanged(d)]),
                None => self.handle_actions(vec![Action::RefreshAll]),
            },
            Err(e) => self.error(e),
        }
    }
}
