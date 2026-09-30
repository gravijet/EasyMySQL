// SQL-Editor-Registerkarte: eine Datei (oder unbenannte Abfrage), automatisch gespeichert.

use super::{Action, Ctx, TabView};
use crate::db::{self, Stmt, StmtResult};
use crate::grid::{self, GridState};
use crate::icons::{self, Icon};
use crate::keymap::Cmd;
use crate::sqledit::{self, SqlEditor, Words};
use crate::style;
use crate::workspace;
use eframe::egui::{self, RichText};
use mysql::Conn;
use mysql::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

type JobResult = (Option<Conn>, Vec<StmtResult>, Option<String>, Option<String>);

pub struct SqlTab {
    /// Datei (None = unbenannte Abfrage, gesichert im Speicherordner)
    pub file: Option<PathBuf>,
    scratch_id: usize,
    /// Projektordner, in dem die Datei liegt
    project: Option<PathBuf>,
    pub database: String,
    pub text: String,
    saved_text: String,
    last_edit: Instant,
    file_mtime: Option<std::time::SystemTime>,
    conn: Option<Conn>,
    job: Option<Receiver<JobResult>>,
    /// Anweisungen des laufenden Auftrags (fuer den Verlauf)
    running: Vec<Stmt>,
    results: Vec<StmtResult>,
    note: Option<String>,
    res_tab: usize,
    grid: GridState,
    editor: SqlEditor,
    selection: Option<String>,
    words: Words,
    words_for: String,
    save_as: Option<String>,
    history: Option<Vec<(String, PathBuf)>>,
    save_error: Option<String>,
    /// Beim ersten Anzeigen zu dieser Zeile springen
    pending_line: Option<usize>,
}

fn next_scratch_id() -> usize {
    let used: Vec<usize> = std::fs::read_dir(workspace::scratch_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.strip_prefix("unbenannt-")?.strip_suffix(".sql")?.parse().ok()
                })
                .collect()
        })
        .unwrap_or_default();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let mut id = NEXT.load(std::sync::atomic::Ordering::Relaxed).max(used.iter().max().map(|m| m + 1).unwrap_or(1));
    while used.contains(&id) {
        id += 1;
    }
    NEXT.store(id + 1, std::sync::atomic::Ordering::Relaxed);
    id
}

fn file_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

impl SqlTab {
    fn empty() -> Self {
        Self {
            file: None,
            scratch_id: 0,
            project: None,
            database: String::new(),
            text: String::new(),
            saved_text: String::new(),
            last_edit: Instant::now(),
            file_mtime: None,
            conn: None,
            job: None,
            running: Vec::new(),
            results: Vec::new(),
            note: None,
            res_tab: 0,
            grid: GridState::default(),
            editor: SqlEditor::new(egui::Id::new(("sql-editor", next_scratch_id() + 100000))),
            selection: None,
            words: Words::default(),
            words_for: "\u{0}".into(),
            save_as: None,
            history: None,
            save_error: None,
            pending_line: None,
        }
    }

    /// Neue unbenannte Abfrage (wird trotzdem laufend gesichert).
    pub fn scratch(database: Option<String>, text: String) -> Self {
        let mut t = Self::empty();
        t.scratch_id = next_scratch_id();
        t.editor = SqlEditor::new(egui::Id::new(("sql-scratch", t.scratch_id)));
        t.database = database.unwrap_or_default();
        t.text = text;
        t.save();
        t
    }

    /// Unbenannte Abfrage aus der letzten Sitzung wiederherstellen.
    pub fn restore_scratch(id: usize, database: String) -> Option<Self> {
        let text = std::fs::read_to_string(workspace::scratch_path(id)).ok()?;
        let mut t = Self::empty();
        t.scratch_id = id;
        t.editor = SqlEditor::new(egui::Id::new(("sql-scratch", id)));
        t.database = database;
        t.saved_text = text.clone();
        t.text = text;
        Some(t)
    }

    /// Datei oeffnen. `project`: geoeffneter Projektordner (falls die Datei darin liegt).
    pub fn open_file(path: &Path, default_db: &str, project: Option<&Path>) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut t = Self::empty();
        t.editor = SqlEditor::new(egui::Id::new(("sql-file", file_key(path))));
        t.text = String::from_utf8_lossy(&bytes).into_owned();
        t.saved_text = t.text.clone();
        t.project = project.filter(|p| path.starts_with(p)).map(|p| p.to_path_buf());
        t.database = t.project.as_ref().and_then(|p| workspace::file_db(p, path)).unwrap_or_else(|| default_db.to_string());
        t.file_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        t.file = Some(path.to_path_buf());
        Ok(t)
    }

    fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Speichern (Datei oder Sicherung der unbenannten Abfrage); sicher gegen Abstuerze.
    pub fn save(&mut self) {
        let target = match &self.file {
            Some(f) => f.clone(),
            None => workspace::scratch_path(self.scratch_id),
        };
        if self.file.is_some() && self.saved_text != self.text {
            workspace::snapshot(&target, &self.saved_text);
        }
        match workspace::atomic_write(&target, &self.text) {
            Ok(_) => {
                self.saved_text = self.text.clone();
                self.save_error = None;
                self.file_mtime = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
            }
            Err(e) => self.save_error = Some(format!("Speichern fehlgeschlagen: {e}")),
        }
    }

    fn set_database(&mut self, db: String) {
        if db != self.database {
            self.database = db;
            if let (Some(p), Some(f)) = (&self.project, &self.file) {
                workspace::set_file_db(p, f, &self.database);
            }
        }
    }

    /// Fuehrt `sql` (ein oder mehrere Anweisungen) aus. `offset`: Zeichenposition im Text.
    fn run(&mut self, cx: &mut Ctx, sql: String, offset: usize) {
        if self.job.is_some() {
            return;
        }
        let Some(dbc) = cx.db else {
            cx.error("Keine Verbindung zum Datenbankserver.");
            return;
        };
        let mut stmts = db::split_statements(&sql);
        if stmts.is_empty() {
            return;
        }
        let base_line = sqledit::line_col(&self.text, offset).0;
        for s in &mut stmts {
            s.line += base_line;
        }
        let conn = match self.conn.take() {
            Some(c) => c,
            None => match dbc.new_conn() {
                Ok(c) => c,
                Err(e) => {
                    cx.error(e);
                    return;
                }
            },
        };
        let database = self.database.clone();
        let (tx, rx) = channel();
        self.running = stmts.clone();
        std::thread::spawn(move || {
            let mut conn = conn;
            let fail = |conn: Conn, msg: String| (Some(conn), vec![StmtResult { error: Some(msg), ..Default::default() }], None, None);
            if !database.is_empty() {
                let cur: Option<String> = conn.query_first::<Option<String>, _>("SELECT DATABASE()").ok().flatten().flatten();
                if cur.as_deref() != Some(database.as_str()) {
                    if let Err(e) = conn.query_drop(format!("USE {}", db::q(&database))) {
                        let _ = tx.send(fail(conn, db::err_text(e)));
                        return;
                    }
                }
            }
            // Vor DROP/TRUNCATE/DELETE ohne WHERE automatisch sichern
            let targets = crate::backup::destructive_targets(&sql, Some(&database));
            let mut note = None;
            if !targets.is_empty() {
                if let Some(env) = crate::backup::env() {
                    match crate::backup::create(&env, Some(targets.clone()), "vor-loeschen", &|_| {}) {
                        Ok(_) => note = Some(format!("Vorher gesichert: {}", targets.join(", "))),
                        Err(e) => {
                            let _ = tx.send(fail(conn, format!("Nicht ausgeführt: Die Sicherung vor dem Löschen ist fehlgeschlagen ({e})")));
                            return;
                        }
                    }
                }
            }
            let results = db::run_statements(&mut conn, &stmts);
            let lost = results.iter().any(|r| {
                r.error.as_ref().is_some_and(|e| e.contains("IoError") || e.contains("broken pipe") || e.contains("gone away"))
            });
            let current: Option<String> = conn.query_first::<Option<String>, _>("SELECT DATABASE()").ok().flatten().flatten();
            let _ = tx.send(((!lost).then_some(conn), results, current, note));
        });
        self.job = Some(rx);
        self.results.clear();
        self.note = None;
        self.editor.error_line = None;
        self.grid.reset();
    }

    fn run_selection_or_all(&mut self, cx: &mut Ctx) {
        match self.selection.clone() {
            Some(s) => {
                let off = self.text.find(&s).map(|b| self.text[..b].chars().count()).unwrap_or(0);
                self.run(cx, s, off);
            }
            None => self.run(cx, self.text.clone(), 0),
        }
    }

    fn current_statement(&self) -> Option<Stmt> {
        let stmts = db::split_statements(&self.text);
        db::statement_at(&stmts, self.editor.cursor).cloned()
    }

    fn run_current(&mut self, cx: &mut Ctx) {
        if let Some(s) = self.current_statement() {
            self.run(cx, s.sql, s.start);
        }
    }

    /// Ausfuehrungsplan der Anweisung am Cursor
    fn explain(&mut self, cx: &mut Ctx) {
        let sql = match self.selection.clone() {
            Some(s) => s,
            None => match self.current_statement() {
                Some(s) => s.sql,
                None => return,
            },
        };
        let first = sql.split_whitespace().next().unwrap_or("").to_uppercase();
        if first == "EXPLAIN" || first == "ANALYZE" {
            self.run(cx, sql, 0);
        } else {
            self.run(cx, format!("EXPLAIN {sql}"), 0);
        }
    }

    fn format(&mut self) {
        self.text = sqledit::format_sql(&self.text);
        self.last_edit = Instant::now();
    }

    fn poll(&mut self, cx: &mut Ctx) {
        let Some(rx) = &self.job else { return };
        let Ok((conn, results, current, note)) = rx.try_recv() else { return };
        self.conn = conn;
        self.job = None;
        let db_before = self.database.clone();
        if let Some(d) = current {
            self.set_database(d);
        }
        // Verlauf
        let now = crate::server::timestamp();
        let entries: Vec<crate::qhistory::Entry> = results
            .iter()
            .zip(self.running.iter())
            .map(|(r, s)| crate::qhistory::Entry {
                time: now.clone(),
                db: db_before.clone(),
                millis: r.elapsed.as_millis() as u64,
                error: r.error.is_some(),
                sql: s.sql.clone(),
            })
            .collect();
        cx.actions.push(Action::History(entries));
        self.running.clear();
        let changed_structure = results.iter().any(|r| {
            let u = r.preview.to_uppercase();
            ["CREATE", "DROP", "ALTER", "RENAME", "TRUNCATE"].iter().any(|k| u.starts_with(k))
        });
        if changed_structure {
            cx.actions.push(Action::RefreshAll);
        }
        if let Some(r) = results.iter().find(|r| r.error.is_some()) {
            self.editor.error_line = r.error_line;
        }
        let n_sets: usize = results.iter().map(|r| r.sets.len()).sum();
        // Ergebnis zeigen, sonst Meldungen
        self.res_tab = if n_sets > 0 && !results.iter().any(|r| r.error.is_some()) { n_sets - 1 } else { n_sets };
        self.results = results;
        self.note = note;
    }

    fn results_ui(&mut self, ui: &mut egui::Ui, row_h: f32) {
        let pal = style::pal();
        let sets: Vec<(usize, usize, &db::ResultSet)> = self
            .results
            .iter()
            .enumerate()
            .flat_map(|(ri, r)| r.sets.iter().enumerate().map(move |(si, s)| (ri, si, s)))
            .collect();
        let n_sets = sets.len();
        let mut tab = self.res_tab.min(n_sets);
        // Reiter
        ui.horizontal(|ui| {
            for (i, (ri, _, s)) in sets.iter().enumerate() {
                let r = &self.results[*ri];
                let label = format!("Ergebnis {} · Z. {} ({})", i + 1, r.line + 1, s.rows.len());
                if ui.selectable_label(tab == i, label).clicked() {
                    tab = i;
                }
            }
            let errs = self.results.iter().filter(|r| r.error.is_some()).count();
            let label = if errs > 0 { format!("Meldungen ({errs} Fehler)") } else { format!("Meldungen ({})", self.results.len()) };
            let txt = if errs > 0 { RichText::new(label).color(pal.error_text) } else { RichText::new(label) };
            if ui.selectable_label(tab == n_sets, txt).clicked() {
                tab = n_sets;
            }
            if self.job.is_some() {
                ui.spinner();
            }
        });
        if tab != self.res_tab {
            self.grid.reset();
        }
        self.res_tab = tab;
        if let Some(n) = &self.note {
            ui.label(RichText::new(n).small().color(pal.text_weak));
        }
        if tab < n_sets {
            let (_, _, set) = sets[tab];
            let set = set.clone();
            ui.horizontal(|ui| {
                let mut info = format!("{} Zeile(n)", set.rows.len());
                if set.truncated {
                    info.push_str(&format!(" (die ersten {})", db::MAX_ROWS));
                }
                ui.label(RichText::new(info).color(pal.ok_text));
                grid::export_menu(ui, "ergebnis", &set.columns, &set.rows);
            });
            grid::show(ui, ("sqlgrid", self.editor.id, tab), &set.columns, &set.rows, false, &mut self.grid);
        } else {
            let mut jump = None;
            egui::ScrollArea::vertical().id_salt(self.editor.id.with("msgs")).auto_shrink([false, false]).show(ui, |ui| {
                for r in &self.results {
                    let (ok, msg) = match &r.error {
                        Some(e) => (false, e.clone()),
                        None if !r.sets.is_empty() => (true, format!("{} Zeile(n) geliefert", r.sets.iter().map(|s| s.rows.len()).sum::<usize>())),
                        None => (true, format!("{} Zeile(n) betroffen", r.affected)),
                    };
                    let color = if ok { pal.ok_text } else { pal.error_text };
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 3.5, color);
                        if ui.link(format!("Zeile {}", r.line + 1)).clicked() {
                            jump = Some(r.error_line.unwrap_or(r.line));
                        }
                        ui.label(RichText::new(format!("{:.3} s", r.elapsed.as_secs_f64())).small().color(pal.text_weak));
                        ui.label(RichText::new(&r.preview).monospace().small());
                    });
                    ui.label(RichText::new(format!("    {msg}")).color(color));
                }
            });
            if let Some(l) = jump {
                let ctx = ui.ctx().clone();
                self.editor.goto_line(&ctx, &self.text, l, row_h);
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        // Speichern unter (fuer unbenannte Abfragen)
        if let Some(name) = self.save_as.as_mut() {
            let mut ok = false;
            let mut other = cx.project.is_none();
            let mut cancel = false;
            if let Some(project) = cx.project {
                egui::Window::new("Speichern unter")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Im Projekt „{}“:", workspace::dir_name(project)));
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Speichern").clicked() {
                                ok = true;
                            }
                            if ui.button("Anderer Ort …").clicked() {
                                other = true;
                            }
                            if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                cancel = true;
                            }
                        });
                    });
            }
            let target = if ok {
                match workspace::create_file(cx.project.unwrap_or(Path::new(".")), name, &self.text) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        cx.error(e);
                        None
                    }
                }
            } else if other {
                let p = rfd::FileDialog::new().add_filter("SQL-Datei", &["sql"]).set_file_name(format!("{name}.sql")).save_file();
                if p.is_none() {
                    cancel = true;
                }
                p
            } else {
                None
            };
            if let Some(p) = target {
                let old = workspace::scratch_path(self.scratch_id);
                self.file = Some(p.clone());
                self.project = cx.project.filter(|pr| p.starts_with(pr)).map(|pr| pr.to_path_buf());
                self.editor.id = egui::Id::new(("sql-file", file_key(&p)));
                self.saved_text.clear();
                self.save();
                let _ = std::fs::remove_file(old);
                if let Some(pr) = &self.project {
                    workspace::set_file_db(pr, &p, &self.database);
                }
                self.save_as = None;
                cx.actions.push(Action::FilesChanged);
                cx.status(format!("Gespeichert: {}", p.display()));
            } else if cancel {
                self.save_as = None;
            }
        }
        // Fruehere Fassungen
        if let Some(list) = self.history.clone() {
            let mut close = false;
            egui::Window::new("Frühere Fassungen")
                .collapsible(false)
                .default_size([520.0, 360.0])
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if list.is_empty() {
                        ui.label("Keine.");
                    }
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (time, path) in &list {
                            ui.horizontal(|ui| {
                                ui.label(time);
                                let preview: String = std::fs::read_to_string(path)
                                    .unwrap_or_default()
                                    .lines()
                                    .find(|l| !l.trim().is_empty())
                                    .unwrap_or("")
                                    .chars()
                                    .take(50)
                                    .collect();
                                ui.label(RichText::new(preview).monospace().small().color(style::pal().text_weak));
                                if ui.small_button("Wiederherstellen").clicked() {
                                    if let Ok(t) = std::fs::read_to_string(path) {
                                        self.text = t;
                                        self.last_edit = Instant::now();
                                        close = true;
                                    }
                                }
                            });
                        }
                    });
                    if ui.button("Schließen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            if close {
                self.history = None;
            }
        }
    }
}

impl TabView for SqlTab {
    fn title(&self) -> String {
        let name = match &self.file {
            Some(f) => f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => format!("Unbenannt-{}", self.scratch_id),
        };
        if self.dirty() { format!("{name} ●") } else { name }
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.run_selection_or_all(cx);
    }

    fn command(&mut self, cmd: Cmd, cx: &mut Ctx) -> bool {
        match cmd {
            Cmd::RunAll => self.run_selection_or_all(cx),
            Cmd::RunStatement => self.run_current(cx),
            Cmd::Explain => self.explain(cx),
            Cmd::Format => self.format(),
            Cmd::Find | Cmd::Replace | Cmd::GotoLine => self.editor.pending = Some(cmd),
            _ => return false,
        }
        true
    }

    fn schema_changed(&mut self, db: &str, _cx: &mut Ctx) {
        if db == self.database {
            self.words_for = "\u{0}".into();
        }
    }

    fn key(&self) -> Option<String> {
        self.file.as_ref().map(|f| format!("file:{}", file_key(f)))
    }

    fn session(&self) -> Option<String> {
        Some(match &self.file {
            Some(f) => format!("sql\t{}", f.display()),
            None => format!("scratch\t{}\t{}", self.scratch_id, self.database),
        })
    }

    fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    fn file_renamed(&mut self, old: &Path, new: &Path) {
        if let Some(f) = &self.file {
            if f == old || f.starts_with(old) {
                let rel = f.strip_prefix(old).unwrap_or(Path::new(""));
                let n = if rel.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rel) };
                self.file = Some(n.clone());
                if let Some(p) = &self.project {
                    if !n.starts_with(p) {
                        self.project = None;
                    }
                }
            }
        }
    }

    fn on_close(&mut self) -> bool {
        self.save();
        // Leere unbenannte Abfrage beim Schliessen entfernen
        if self.file.is_none() && self.text.trim().is_empty() {
            let _ = std::fs::remove_file(workspace::scratch_path(self.scratch_id));
        }
        true
    }

    fn save_now_quiet(&mut self) {
        self.save();
    }

    fn goto_line(&mut self, line: usize) {
        self.pending_line = Some(line);
    }

    fn save_now(&mut self) {
        if self.file.is_none() && !self.text.trim().is_empty() && self.save_as.is_none() {
            // Strg+S bei unbenannter Abfrage: Speichern unter
            self.save_as = Some("Neue Abfrage".into());
        }
        self.save();
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        let pal = style::pal();
        self.editor.minimap = cx.settings.minimap;
        if self.job.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        // Automatisch speichern 1 s nach der letzten Aenderung
        if self.dirty() {
            if self.last_edit.elapsed() > Duration::from_millis(1000) {
                self.save();
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        }
        // Aenderungen von aussen (z. B. VS Code) uebernehmen
        if let (Some(f), false) = (&self.file, self.dirty()) {
            if let Ok(m) = std::fs::metadata(f).and_then(|m| m.modified()) {
                if self.file_mtime.is_some_and(|old| old != m) {
                    if let Ok(b) = std::fs::read(f) {
                        self.text = String::from_utf8_lossy(&b).into_owned();
                        self.saved_text = self.text.clone();
                    }
                    self.file_mtime = Some(m);
                }
            }
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
        // Woerter fuer die Autovervollstaendigung
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

        // Kopfzeile: Datenbank, Aktionen, Position
        ui.horizontal(|ui| {
            let mut dbs = vec![String::new()];
            dbs.extend(cx.databases.iter().cloned());
            let mut d = self.database.clone();
            if super::str_combo(ui, self.editor.id.with("db"), &dbs, &mut d, 150.0) {
                self.set_database(d);
                self.words_for = "\u{0}".into();
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                if icons::text_button(ui, Icon::Play, "Ausführen")
                    .on_hover_text(format!("Datei oder Markierung ausführen ({})", crate::keymap::text(Cmd::RunAll)))
                    .clicked()
                {
                    self.run_selection_or_all(cx);
                }
                if ui
                    .button("Anweisung")
                    .on_hover_text(format!("Anweisung am Cursor ausführen ({})", crate::keymap::text(Cmd::RunStatement)))
                    .clicked()
                {
                    self.run_current(cx);
                }
                if ui.button("Plan").on_hover_text(format!("EXPLAIN der Anweisung am Cursor ({})", crate::keymap::text(Cmd::Explain))).clicked() {
                    self.explain(cx);
                }
            });
            if ui.button("Formatieren").on_hover_text(crate::keymap::text(Cmd::Format)).clicked() {
                self.format();
            }
            let more = icons::button(ui, Icon::More, "Weitere");
            egui::Popup::menu(&more).show(|ui| {
                if ui.button("Speichern unter …").clicked() {
                    self.save_as = Some(self.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or("Neue Abfrage".into()));
                    ui.close();
                }
                if let Some(f) = self.file.clone() {
                    if ui.button("Frühere Fassungen …").clicked() {
                        self.history = Some(workspace::history(&f));
                        ui.close();
                    }
                }
                if ui.button("In VS Code öffnen").clicked() {
                    self.save();
                    let res = match &self.file {
                        Some(f) => crate::vscode::open_path(self.project.as_deref(), f, &self.database),
                        None => crate::vscode::open_sql(&format!("unbenannt-{}.sql", self.scratch_id), &self.text, &self.database),
                    };
                    if let Err(e) = res {
                        cx.error(e);
                    }
                    ui.close();
                }
            });
            if let Some(e) = &self.save_error {
                ui.label(RichText::new(e).color(pal.error_text));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (l, c) = self.editor.line_col;
                ui.label(RichText::new(format!("Z. {l}, Sp. {c}")).small().color(pal.text_weak));
            });
        });

        let font_row = ui.ctx().fonts_mut(|f| f.row_height(&egui::FontId::monospace(sqledit::font_size())));
        if let Some(l) = self.pending_line.take() {
            let ctx = ui.ctx().clone();
            self.editor.goto_line(&ctx, &self.text, l, font_row);
        }
        // Ergebnisbereich unten (Hoehe verstellbar), Editor darueber
        egui::Panel::bottom(self.editor.id.with("results"))
            .resizable(true)
            .default_size(ui.available_height() * 0.38)
            .min_size(80.0)
            .frame(egui::Frame::new().fill(pal.face).inner_margin(egui::Margin::symmetric(4, 4)))
            .show(ui, |ui| {
                self.results_ui(ui, font_row);
            });
        let h = (ui.available_height() - 4.0).max(100.0);
        let out = self.editor.show(ui, &mut self.text, &self.words, h);
        self.selection = out.selection.clone();
        if out.changed {
            self.last_edit = Instant::now();
        }
        if out.format {
            self.format();
        }
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx, cx);
    }
}
