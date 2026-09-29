// SQL-Editor: Abfragen eingeben und ausfuehren.

use super::{Action, Ctx, TabView};
use crate::db::{self, QueryOutput};
use crate::grid::{self, GridState};
use crate::style;
use crate::sqledit::{self, SqlEditor, Words};
use eframe::egui::{self, RichText};
use mysql::Conn;
use mysql::prelude::*;
use std::sync::mpsc::{Receiver, channel};

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

pub struct SqlTab {
    number: usize,
    pub database: String,
    pub text: String,
    conn: Option<Conn>,
    job: Option<Receiver<(Option<Conn>, QueryOutput)>>,
    output: Option<QueryOutput>,
    shown_set: usize,
    grid: GridState,
    pub file: Option<std::path::PathBuf>,
    file_mtime: Option<std::time::SystemTime>,
    editor: SqlEditor,
    selection: Option<String>,
    words: Words,
    words_for: String,
}

impl SqlTab {
    pub fn new(database: Option<String>, text: String) -> Self {
        let number = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            number,
            database: database.unwrap_or_default(),
            text,
            conn: None,
            job: None,
            output: None,
            shown_set: 0,
            grid: GridState::default(),
            file: None,
            file_mtime: None,
            editor: SqlEditor::new(egui::Id::new(("sql-editor", number))),
            selection: None,
            words: Words::default(),
            words_for: "\u{0}".into(),
        }
    }

    fn run(&mut self, cx: &mut Ctx, sql: String) {
        if self.job.is_some() {
            return;
        }
        let Some(db) = cx.db else {
            cx.error("Keine Verbindung zum Datenbankserver.");
            return;
        };
        let conn = match self.conn.take() {
            Some(c) => c,
            None => match db.new_conn() {
                Ok(c) => c,
                Err(e) => {
                    cx.error(e);
                    return;
                }
            },
        };
        let database = self.database.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut conn = conn;
            let mut alive = true;
            if !database.is_empty() {
                let cur: Option<String> = conn
                    .query_first::<Option<String>, _>("SELECT DATABASE()")
                    .ok()
                    .flatten()
                    .flatten();
                if cur.as_deref() != Some(database.as_str()) {
                    if let Err(e) = conn.query_drop(format!("USE {}", db::q(&database))) {
                        let out = QueryOutput {
                            error: Some(e.to_string()),
                            ..Default::default()
                        };
                        let _ = tx.send((Some(conn), out));
                        return;
                    }
                }
            }
            let out = db::run_script(&mut conn, &sql);
            if let Some(e) = &out.error {
                // Verbindung verloren? Dann neu aufbauen lassen.
                if e.contains("IoError") || e.contains("broken pipe") || e.contains("gone away") {
                    alive = false;
                }
            }
            let _ = tx.send((alive.then_some(conn), out));
        });
        self.job = Some(rx);
        self.output = None;
        self.grid.reset();
    }

    fn poll(&mut self, cx: &mut Ctx) {
        if let Some(rx) = &self.job {
            if let Ok((conn, out)) = rx.try_recv() {
                self.conn = conn;
                self.job = None;
                if let Some(d) = &out.database {
                    self.database = d.clone();
                }
                self.shown_set = out
                    .sets
                    .iter()
                    .rposition(|s| s.has_table())
                    .unwrap_or(0);
                // Struktur geaendert? Baum aktualisieren.
                let upper = self.text.to_uppercase();
                if ["CREATE", "DROP", "ALTER", "RENAME", "TRUNCATE"]
                    .iter()
                    .any(|k| upper.contains(k))
                {
                    cx.actions.push(Action::RefreshAll);
                }
                self.output = Some(out);
            }
        }
    }
}

impl TabView for SqlTab {
    fn title(&self) -> String {
        match &self.file {
            Some(f) => f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => format!("SQL-Abfrage {}", self.number),
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn schema_changed(&mut self, db: &str, _cx: &mut Ctx) {
        if db == self.database {
            self.words_for = "\u{0}".into();
        }
    }

    fn execute(&mut self, cx: &mut Ctx) {
        let sql = self.text.clone();
        if !sql.trim().is_empty() {
            self.run(cx, sql);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        if self.job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        // Woerter fuer die Autovervollstaendigung (Tabellen/Spalten der Datenbank)
        if self.words_for != self.database {
            self.words_for = self.database.clone();
            self.words = Words::default();
            if let Some(s) = cx.schema(&self.database) {
                for t in &s.tables {
                    self.words.tables.push(t.name.clone());
                    for c in &t.columns {
                        self.words.columns.push((t.name.clone(), c.name.clone()));
                    }
                }
            }
        }

        // Werkzeugleiste
        ui.horizontal_wrapped(|ui| {
            ui.label("Datenbank:");
            let mut dbs = vec![String::new()];
            dbs.extend(cx.databases.iter().cloned());
            super::str_combo(ui, ("sqldb", self.number), &dbs, &mut self.database, 170.0);
            ui.separator();
            let run = ui
                .add_enabled(self.job.is_none(), egui::Button::new("▶ Ausführen (F5)"))
                .on_hover_text("Führt alles oder nur den markierten Text aus. Auch mit Strg+Enter.");
            if run.clicked() {
                let sql = self.selection.clone().unwrap_or_else(|| self.text.clone());
                self.run(cx, sql);
            }
            if ui
                .button("Formatieren")
                .on_hover_text("SQL schön formatieren (Strg+Umschalt+F)")
                .clicked()
            {
                self.text = sqledit::format_sql(&self.text);
            }
            if ui.button("Leeren").clicked() {
                self.text.clear();
            }
            if ui.button("Öffnen...").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("SQL-Dateien", &["sql", "txt"])
                    .pick_file()
                {
                    match std::fs::read(&p) {
                        Ok(b) => {
                            self.text = String::from_utf8_lossy(&b).into_owned();
                            self.file = Some(p);
                        }
                        Err(e) => cx.error(e.to_string()),
                    }
                }
            }
            if ui.button("Speichern...").clicked() {
                let mut dlg = rfd::FileDialog::new().add_filter("SQL-Datei", &["sql"]);
                dlg = dlg.set_file_name(self.file.as_ref().and_then(|f| f.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or("abfrage.sql".into()));
                if let Some(p) = dlg.save_file() {
                    match std::fs::write(&p, &self.text) {
                        Ok(_) => {
                            cx.status(format!("Gespeichert: {}", p.display()));
                            self.file = Some(p);
                        }
                        Err(e) => cx.error(e.to_string()),
                    }
                }
            }
            if ui
                .button("In VS Code öffnen")
                .on_hover_text("Abfrage in Visual Studio Code bearbeiten (mit SQLTools und GitHub Copilot)")
                .clicked()
            {
                let name = self
                    .file
                    .as_ref()
                    .and_then(|f| f.file_name())
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or(format!("abfrage-{}.sql", self.number));
                match crate::vscode::open_sql(&name, &self.text, &self.database) {
                    Ok(p) => {
                        cx.status(format!("In VS Code geöffnet: {}", p.display()));
                        self.file = Some(p);
                    }
                    Err(e) => cx.error(e),
                }
            }
            if self.job.is_some() {
                ui.spinner();
                ui.label("Abfrage läuft ...");
            }
        });
        ui.add_space(2.0);

        // Aenderungen aus VS Code uebernehmen (gleiche Datei)
        if let Some(f) = &self.file {
            if let Ok(meta) = std::fs::metadata(f) {
                let mtime = meta.modified().ok();
                if self.file_mtime.is_some() && mtime != self.file_mtime {
                    if let Ok(b) = std::fs::read(f) {
                        self.text = String::from_utf8_lossy(&b).into_owned();
                    }
                }
                self.file_mtime = mtime;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }

        let avail = ui.available_height();
        let editor_h = (avail * 0.45).max(100.0);

        // Editor
        let out = self.editor.show(ui, &mut self.text, &self.words, editor_h);
        self.selection = out.selection;
        if out.format {
            self.text = sqledit::format_sql(&self.text);
        }
        if out.run {
            let sql = self.selection.clone().unwrap_or_else(|| self.text.clone());
            self.run(cx, sql);
        }
        ui.add_space(4.0);

        // Ergebnis
        let Some(out) = &self.output else {
            if self.job.is_none() {
                ui.label(RichText::new("Noch keine Abfrage ausgeführt.").color(style::NULL_TEXT));
            }
            return;
        };
        ui.horizontal(|ui| {
            if let Some(e) = &out.error {
                ui.label(RichText::new(format!("Fehler: {e}")).color(style::ERROR_TEXT));
            } else {
                let n_tables = out.sets.iter().filter(|s| s.has_table()).count();
                let affected: u64 = out.sets.iter().filter(|s| !s.has_table()).map(|s| s.affected).sum();
                let mut msg = format!("OK – {} Anweisung(en) in {:.3} s", out.sets.len().max(1), out.elapsed.as_secs_f64());
                if let Some(s) = out.sets.get(self.shown_set).filter(|s| s.has_table()) {
                    msg.push_str(&format!(", {} Zeile(n)", s.rows.len()));
                    if s.truncated {
                        msg.push_str(&format!(" (nur die ersten {} angezeigt)", db::MAX_ROWS));
                    }
                } else if n_tables == 0 {
                    msg.push_str(&format!(", {affected} Zeile(n) betroffen"));
                }
                ui.label(RichText::new(msg).color(style::OK_TEXT));
            }
        });
        let tables: Vec<usize> = out
            .sets
            .iter()
            .enumerate()
            .filter(|(_, s)| s.has_table())
            .map(|(i, _)| i)
            .collect();
        if tables.len() > 1 {
            ui.horizontal(|ui| {
                ui.label("Ergebnis:");
                for (n, i) in tables.iter().enumerate() {
                    if ui.selectable_label(self.shown_set == *i, format!("{}", n + 1)).clicked() {
                        self.shown_set = *i;
                        self.grid.reset();
                    }
                }
            });
        }
        if let Some(set) = out.sets.get(self.shown_set).filter(|s| s.has_table()) {
            ui.horizontal(|ui| {
                if ui.small_button("In Zwischenablage kopieren").clicked() {
                    ui.ctx().copy_text(grid::to_tsv(&set.columns, &set.rows));
                }
                if ui.small_button("Als CSV speichern...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .set_file_name("ergebnis.csv")
                        .save_file()
                    {
                        if let Err(e) = std::fs::write(&p, grid::to_csv(&set.columns, &set.rows)) {
                            cx.error(e.to_string());
                        }
                    }
                }
            });
            let set = set.clone();
            grid::show(ui, ("sqlgrid", self.number, self.shown_set), &set.columns, &set.rows, false, &mut self.grid);
        }
    }
}
