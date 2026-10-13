// Kleine Dialoge (Meldung, Bestaetigung, neue Datenbank, Export, ...).

use super::*;
use crate::db::q;
use crate::transfer::export::{self, Format};
use eframe::egui::{Key, RichText};

pub enum Dialog {
    Message { title: String, text: String, error: bool },
    Confirm { text: String, db: Option<String>, sql: String },
    NewDatabase { name: String, collation: String },
    /// `only`: ausgewaehlte Tabellen (None = alle)
    Export { db: String, only: Option<Vec<String>>, with_data: bool, format: Format },
    /// Tabelle umbenennen oder kopieren
    TableOp { db: String, table: String, copy: bool, with_data: bool, name: String },
    /// Zwei Tabellen zu einer neuen zusammenfuegen
    Merge { db: String, a: String, b: String, mode: MergeKind, left_col: String, right_col: String, keep_all_a: bool, name: String },
    About,
    Myisam(Vec<(String, String)>),
    /// Verbindung fehlgeschlagen
    ConnectFailed(String),
    /// Ungespeicherte Dateien (Autosave aus) vor dem Schliessen, Beenden oder Projektwechsel
    Unsaved { files: Vec<String>, then: Then },
}

#[derive(Clone, Copy, PartialEq)]
pub enum MergeKind {
    Rows,
    Columns,
}

/// Was nach der Rueckfrage zu ungespeicherten Dateien geschieht
#[derive(Clone)]
pub enum Then {
    CloseTabs(Vec<usize>),
    Quit,
    Project(Option<PathBuf>),
}

impl EasyApp {
    pub(super) fn dialogs_ui(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.dialogs.last_mut() else { return };
        let mut close = false;
        let mut run_sql: Option<(Option<String>, String)> = None;
        let mut export: Option<export::Job> = None;
        // Mehrere Anweisungen nacheinander in einer Datenbank ausfuehren
        let mut batch: Option<(String, Vec<String>)> = None;
        let mut open_sql: Option<(String, String)> = None;
        let mut create_db: Option<(String, String)> = None;
        let mut convert: Option<Vec<(String, String)>> = None;
        let mut open_settings = false;
        let mut retry = false;
        let mut unsaved: Option<(bool, Then)> = None;
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
                Dialog::Export { db, only, with_data, format } => {
                    ui.label(RichText::new(crate::i18n::text("Exportieren")).strong());
                    ui.add_space(4.0);
                    let schema = self.db.as_ref().and_then(|d| self.schemas.get_arc(Some(d), db).cloned());
                    egui::Grid::new("exp").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label(crate::i18n::text("Datenbank"));
                        if crate::tabs::db_combo(ui, "expdb", &self.databases, db) {
                            *only = None;
                        }
                        ui.end_row();
                        ui.label(crate::i18n::text("Format"));
                        ui.horizontal(|ui| {
                            for f in Format::ALL {
                                ui.selectable_value(format, f, f.label());
                            }
                        });
                        ui.end_row();
                        ui.label(crate::i18n::text("Tabellen"));
                        ui.horizontal(|ui| {
                            if ui.selectable_label(only.is_none(), crate::i18n::text("Alle")).clicked() {
                                *only = None;
                            }
                            if ui.selectable_label(only.is_some(), crate::i18n::text("Auswahl")).clicked() && only.is_none() {
                                *only = Some(Vec::new());
                            }
                        });
                        ui.end_row();
                    });
                    if let (Some(sel), Some(schema)) = (only.as_mut(), &schema) {
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            for t in schema.tables.iter().filter(|t| *format == Format::Sql || !t.is_view) {
                                let mut on = sel.contains(&t.name);
                                if crate::widgets::check(ui, &mut on, &t.name).changed() {
                                    if on {
                                        sel.push(t.name.clone());
                                    } else {
                                        sel.retain(|n| n != &t.name);
                                    }
                                }
                            }
                        });
                    }
                    if *format == Format::Sql {
                        crate::widgets::check(ui, with_data, crate::i18n::text("Mit Daten"));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let none_chosen = only.as_ref().is_some_and(|s| s.is_empty());
                        if ui.add_enabled(!none_chosen, egui::Button::new(crate::i18n::text("Speichern unter …"))).clicked() {
                            let tables = only.clone().unwrap_or_default();
                            let count = if tables.is_empty() { schema.as_ref().map(|s| s.tables.iter().filter(|t| !t.is_view).count()).unwrap_or(0) } else { tables.len() };
                            let name = if let [one] = tables.as_slice() { one.clone() } else { db.clone() };
                            let target = if format.folder(count) {
                                rfd::FileDialog::new().pick_folder().map(|f| f.join(&name))
                            } else {
                                rfd::FileDialog::new().add_filter(format.label(), &[format.ext()]).set_file_name(format!("{name}.{}", format.ext())).save_file()
                            };
                            if let Some(target) = target {
                                export = Some(export::Job { db: db.clone(), tables, with_data: *with_data || *format != Format::Sql, format: *format, target });
                            }
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::TableOp { db, table, copy, with_data, name } => {
                    let t = crate::i18n::tr;
                    ui.label(RichText::new(if *copy { crate::tr_format!("Tabelle {table} kopieren", "Copy table {table}") } else { crate::tr_format!("Tabelle {table} umbenennen", "Rename table {table}") }).strong());
                    ui.add_space(4.0);
                    let r = crate::tabs::name_field(ui, egui::Id::new("tableop-name"), name, 240.0, false);
                    let valid = !name.trim().is_empty() && name.trim() != table;
                    if *copy {
                        crate::widgets::check(ui, with_data, t("Mit Daten", "With data"));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let go = ui.add_enabled(valid, egui::Button::new("OK")).clicked() || (valid && r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                        if go {
                            let new = name.trim();
                            batch = Some((
                                db.clone(),
                                if *copy { crate::db::duplicate_table(db, table, new, *with_data) } else { vec![format!("RENAME TABLE {}.{} TO {}.{}", q(db), q(table), q(db), q(new))] },
                            ));
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            close = true;
                        }
                    });
                }
                Dialog::Merge { db, a, b, mode, left_col, right_col, keep_all_a, name } => {
                    let t = crate::i18n::tr;
                    let schema = self.db.as_ref().and_then(|d| self.schemas.get_arc(Some(d), db).cloned());
                    ui.label(RichText::new(crate::tr_format!("Tabellen zusammenfügen", "Merge tables")).strong());
                    ui.add_space(6.0);
                    let tables: Vec<String> = schema.as_ref().map(|s| s.tables.iter().filter(|x| !x.is_view).map(|x| x.name.clone()).collect()).unwrap_or_default();
                    let (ta, tb) = schema.as_ref().map(|s| (s.table(a).cloned(), s.table(b).cloned())).unwrap_or((None, None));
                    egui::Grid::new("merge").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label(t("Tabelle A", "Table A"));
                        let mut changed = crate::tabs::str_combo(ui, "merge-a", &tables, a, 200.0);
                        ui.end_row();
                        ui.label(t("Tabelle B", "Table B"));
                        changed |= crate::tabs::str_combo(ui, "merge-b", &tables, b, 200.0);
                        ui.end_row();
                        ui.label(t("Zusammenfügen", "Combine"));
                        ui.vertical(|ui| {
                            ui.radio_value(mode, MergeKind::Rows, t("Zeilen untereinander (gleiche Spaltennamen)", "Rows below each other (same column names)"));
                            ui.radio_value(mode, MergeKind::Columns, t("Spalten nebeneinander (über gleiche Werte)", "Columns side by side (matching values)"));
                        });
                        ui.end_row();
                        if *mode == MergeKind::Columns {
                            let cols = |t: &Option<crate::db::TableInfo>| t.as_ref().map(|t| t.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
                            if changed || left_col.is_empty() {
                                // Beziehung zwischen den Tabellen vorschlagen, sonst gleichnamige Spalte
                                let fk = schema.as_ref().and_then(|s| s.fks.iter().find(|f| (f.table == *a && f.ref_table == *b) || (f.table == *b && f.ref_table == *a)));
                                match fk {
                                    Some(f) if f.table == *a => (*left_col, *right_col) = (f.columns[0].clone(), f.ref_columns[0].clone()),
                                    Some(f) => (*left_col, *right_col) = (f.ref_columns[0].clone(), f.columns[0].clone()),
                                    None => {
                                        let same = cols(&ta).into_iter().find(|c| cols(&tb).contains(c));
                                        *left_col = same.clone().or_else(|| cols(&ta).first().cloned()).unwrap_or_default();
                                        *right_col = same.or_else(|| cols(&tb).first().cloned()).unwrap_or_default();
                                    }
                                }
                            }
                            ui.label(t("Verbinden über", "Match on"));
                            ui.horizontal(|ui| {
                                crate::tabs::str_combo(ui, "merge-lc", &cols(&ta), left_col, 120.0);
                                ui.label("=");
                                crate::tabs::str_combo(ui, "merge-rc", &cols(&tb), right_col, 120.0);
                            });
                            ui.end_row();
                            ui.label("");
                            crate::widgets::check(ui, keep_all_a, t("Alle Zeilen von A behalten", "Keep all rows of A"));
                            ui.end_row();
                        }
                        ui.label(t("Neue Tabelle", "New table"));
                        ui.add_sized([200.0, 22.0], egui::TextEdit::singleline(name).vertical_align(egui::Align::Center));
                        ui.end_row();
                    });
                    let select = match (&ta, &tb) {
                        (Some(x), Some(y)) if a != b => {
                            let m = match mode {
                                MergeKind::Rows => crate::db::MergeMode::Union,
                                MergeKind::Columns => crate::db::MergeMode::Join { left_col: left_col.clone(), right_col: right_col.clone(), left_outer: *keep_all_a },
                            };
                            Some(crate::db::merge_select(db, x, y, &m))
                        }
                        _ => None,
                    };
                    if let Some(sel) = &select {
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| ui.label(RichText::new(sel.as_str()).monospace().small().color(pal.text_weak)));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let ok = select.is_some() && !name.trim().is_empty();
                        if let Some(sel) = select {
                            let sql = format!("CREATE TABLE {}.{} AS\n{sel}", q(db), q(name.trim()));
                            if ui.add_enabled(ok, egui::Button::new(t("Zusammenfügen", "Merge"))).clicked() {
                                batch = Some((db.clone(), vec![sql.clone()]));
                                close = true;
                            }
                            if ui.button(t("In Editor öffnen", "Open in editor")).clicked() {
                                open_sql = Some((db.clone(), format!("{sql};\n")));
                                close = true;
                            }
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
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
                Dialog::Unsaved { files, then } => {
                    ui.label(RichText::new(crate::i18n::text("Ungespeicherte Änderungen")).strong());
                    ui.add_space(4.0);
                    ui.label(crate::i18n::text("Diese Dateien wurden geändert und noch nicht gespeichert:"));
                    for f in files.iter().take(8) {
                        ui.label(RichText::new(f.as_str()).monospace());
                    }
                    if files.len() > 8 {
                        ui.label(RichText::new(format!("… +{}", files.len() - 8)).color(pal.text_weak));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::text("Speichern")).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            unsaved = Some((true, then.clone()));
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Nicht speichern")).clicked() {
                            unsaved = Some((false, then.clone()));
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
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

        if let Some((d, sql)) = open_sql {
            self.handle_actions(vec![Action::OpenSql { db: Some(d), sql, run: false }]);
        }
        if let (Some((d, list)), Some(dbc)) = (batch, self.db.clone()) {
            let mut failed = false;
            for sql in &list {
                if let Err(e) = dbc.exec_in(&d, sql) {
                    self.error(format!("{e}\n\nSQL: {sql}"));
                    failed = true;
                    break;
                }
            }
            if !failed {
                self.handle_actions(vec![Action::SchemaChanged(d)]);
            }
        }
        if let Some((save, then)) = unsaved {
            self.finish_unsaved(save, then);
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
        if let Some((name, coll)) = create_db
            && let Some(dbc) = &self.db {
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
        if let Some(job) = export
            && let Some(dbc) = self.db.clone() {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(export::run(&dbc, &job));
                });
                self.export_job = Some(rx);
                self.dialogs.pop();
                self.toast(crate::i18n::text("Export läuft …"));
            }
    }

    /// Antwort auf die Rueckfrage zu ungespeicherten Dateien: speichern oder verwerfen, dann fortfahren.
    fn finish_unsaved(&mut self, save: bool, then: Then) {
        let idx: Vec<usize> = match &then {
            Then::CloseTabs(v) => v.clone(),
            _ => (0..self.tabs.len()).collect(),
        };
        for i in idx {
            if let Some(t) = self.tabs.get_mut(i) {
                if save {
                    t.save_now_quiet();
                } else {
                    t.discard_changes();
                }
            }
        }
        match then {
            Then::CloseTabs(v) => self.close_now(v),
            Then::Quit => self.quit(),
            Then::Project(dir) => self.switch_project_now(dir),
        }
    }

    /// Bestaetigtes SQL ausfuehren (vorher bei Loeschen automatisch sichern)
    fn run_confirmed(&mut self, db: Option<String>, sql: String) {
        let targets = crate::backup::destructive_targets(&sql, db.as_deref());
        if !targets.is_empty() {
            self.update_backup_env();
            if let Some(env) = crate::backup::env()
                && let Err(e) = crate::backup::create(&env, Some(targets), "vor-loeschen", &|_| {}) {
                    self.error(crate::tr_format!("Die Sicherung vor dem Löschen ist fehlgeschlagen – nichts wurde gelöscht.\n\n{e}", "The backup before deletion failed — nothing was deleted.\n\n{e}"));
                    return;
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
