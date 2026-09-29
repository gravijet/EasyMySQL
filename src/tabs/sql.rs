// SQL-Editor: Abfragen eingeben und ausfuehren.

use super::{Action, Ctx, TabView};
use crate::db::{self, QueryOutput};
use crate::grid::{self, GridState};
use crate::style;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
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
}

impl SqlTab {
    pub fn new(database: Option<String>, text: String) -> Self {
        Self {
            number: COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            database: database.unwrap_or_default(),
            text,
            conn: None,
            job: None,
            output: None,
            shown_set: 0,
            grid: GridState::default(),
            file: None,
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

    fn selected_or_all(&self, ui: &egui::Ui, id: egui::Id) -> String {
        if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
            if let Some(range) = state.cursor.char_range() {
                let s = range.slice_str(&self.text);
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
        }
        self.text.clone()
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
        let editor_id = ui.make_persistent_id(("sql-editor", self.number));

        // Werkzeugleiste
        ui.horizontal(|ui| {
            ui.label("Datenbank:");
            let mut dbs = vec![String::new()];
            dbs.extend(cx.databases.iter().cloned());
            super::str_combo(ui, ("sqldb", self.number), &dbs, &mut self.database, 170.0);
            ui.separator();
            let run = ui
                .add_enabled(self.job.is_none(), egui::Button::new("▶ Ausführen (F5)"))
                .on_hover_text("Führt alles oder nur den markierten Text aus. Auch mit Strg+Enter.");
            if run.clicked() {
                let sql = self.selected_or_all(ui, editor_id);
                self.run(cx, sql);
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
            if self.job.is_some() {
                ui.spinner();
                ui.label("Abfrage läuft ...");
            }
        });
        ui.add_space(2.0);

        let ctrl_enter = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        let avail = ui.available_height();
        let editor_h = (avail * 0.42).max(80.0);

        // Editor
        style::sunken_frame().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("sqlscroll", self.number))
                .max_height(editor_h)
                .min_scrolled_height(editor_h)
                .show(ui, |ui| {
                    let resp = ui.add_sized(
                        [ui.available_width(), editor_h],
                        egui::TextEdit::multiline(&mut self.text)
                            .id(editor_id)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .hint_text("SQL hier eingeben, z. B.  SELECT * FROM tabelle;")
                            .lock_focus(true)
                            .desired_width(f32::INFINITY),
                    );
                    if resp.has_focus() && ui.input_mut(|i| i.consume_shortcut(&ctrl_enter)) {
                        let sql = self.selected_or_all(ui, editor_id);
                        self.run(cx, sql);
                    }
                });
        });
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
