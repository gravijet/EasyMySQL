// SQL-Editor-Registerkarte: eine Datei (oder unbenannte Abfrage), automatisch gespeichert.

use super::{Action, Ctx, TabView};
use crate::db::{self, StmtResult};
use crate::grid::{self, GridState};
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
    /// Datei (None = unbenannte Abfrage, gesichert unter "ungespeichert")
    pub file: Option<PathBuf>,
    scratch_id: usize,
    project: Option<PathBuf>,
    pub database: String,
    pub text: String,
    saved_text: String,
    last_edit: Instant,
    file_mtime: Option<std::time::SystemTime>,
    conn: Option<Conn>,
    job: Option<Receiver<JobResult>>,
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

/// Projektordner, zu dem eine Datei gehoert (falls sie in einem Projekt liegt).
pub fn project_of(path: &Path) -> Option<PathBuf> {
    let root = workspace::projects_root();
    let rel = path.strip_prefix(&root).ok()?;
    let first = rel.components().next()?;
    Some(root.join(first.as_os_str()))
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

    pub fn open_file(path: &Path, default_db: &str) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut t = Self::empty();
        t.editor = SqlEditor::new(egui::Id::new(("sql-file", path.to_string_lossy().to_lowercase())));
        t.text = String::from_utf8_lossy(&bytes).into_owned();
        t.saved_text = t.text.clone();
        t.project = project_of(path);
        t.database = t
            .project
            .as_ref()
            .and_then(|p| workspace::file_db(p, path))
            .unwrap_or_else(|| default_db.to_string());
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
            cx.status("Keine Anweisung zum Ausführen gefunden.");
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
                        Ok(_) => note = Some(format!("Vorher automatisch gesichert: {}", targets.join(", "))),
                        Err(e) => {
                            let _ = tx.send(fail(conn, format!("Nicht ausgeführt: Die automatische Sicherung vor dem Löschen ist fehlgeschlagen ({e})")));
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

    fn run_current(&mut self, cx: &mut Ctx) {
        let stmts = db::split_statements(&self.text);
        match db::statement_at(&stmts, self.editor.cursor) {
            Some(s) => {
                let sql = s.sql.clone();
                let start = s.start;
                self.run(cx, sql, start);
            }
            None => cx.status("Keine Anweisung am Cursor."),
        }
    }

    fn poll(&mut self, cx: &mut Ctx) {
        let Some(rx) = &self.job else { return };
        let Ok((conn, results, current, note)) = rx.try_recv() else { return };
        self.conn = conn;
        self.job = None;
        if let Some(d) = current {
            self.set_database(d);
        }
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
        let ok = results.iter().filter(|r| r.error.is_none()).count();
        cx.status(format!("{ok} von {} Anweisung(en) ausgeführt", results.len()));
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
            let label = if errs > 0 { format!("Meldungen ⚠ {errs}") } else { format!("Meldungen ({})", self.results.len()) };
            let txt = if errs > 0 { RichText::new(label).color(pal.error_text) } else { RichText::new(label) };
            if ui.selectable_label(tab == n_sets, txt).clicked() {
                tab = n_sets;
            }
            if self.job.is_some() {
                ui.spinner();
                ui.label("läuft …");
            }
        });
        if tab != self.res_tab {
            self.grid.reset();
        }
        self.res_tab = tab;
        ui.add_space(2.0);
        if let Some(n) = &self.note {
            ui.label(RichText::new(n).small().color(pal.null_text));
        }
        if tab < n_sets {
            let (_, _, set) = sets[tab];
            let set = set.clone();
            ui.horizontal(|ui| {
                let mut info = format!("{} Zeile(n)", set.rows.len());
                if set.truncated {
                    info.push_str(&format!(" (nur die ersten {} angezeigt)", db::MAX_ROWS));
                }
                ui.label(RichText::new(info).color(pal.ok_text));
                if ui.small_button("In Zwischenablage kopieren").clicked() {
                    ui.ctx().copy_text(grid::to_tsv(&set.columns, &set.rows));
                }
                if ui.small_button("Als CSV speichern...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("ergebnis.csv").save_file() {
                        let _ = std::fs::write(&p, grid::to_csv(&set.columns, &set.rows));
                    }
                }
            });
            grid::show(ui, ("sqlgrid", self.editor.id, tab), &set.columns, &set.rows, false, &mut self.grid);
        } else {
            let mut jump = None;
            egui::ScrollArea::vertical().id_salt(self.editor.id.with("msgs")).auto_shrink([false, false]).show(ui, |ui| {
                if self.results.is_empty() && self.job.is_none() {
                    ui.label(RichText::new("Noch nichts ausgeführt.  Strg+Alt+S = Datei/Auswahl, Strg+Enter = Anweisung am Cursor").color(pal.null_text));
                }
                for r in &self.results {
                    let (icon, color, msg) = match &r.error {
                        Some(e) => ("✖", pal.error_text, e.clone()),
                        None if !r.sets.is_empty() => {
                            ("✔", pal.ok_text, format!("{} Zeile(n) geliefert", r.sets.iter().map(|s| s.rows.len()).sum::<usize>()))
                        }
                        None => ("✔", pal.ok_text, format!("{} Zeile(n) betroffen", r.affected)),
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).color(color));
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
            let mut other = false;
            let mut cancel = false;
            egui::Window::new("Speichern unter")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Im Projekt \"{}\" speichern als:", cx.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Speichern").clicked() {
                            ok = true;
                        }
                        if ui.button("Anderer Ort...").clicked() {
                            other = true;
                        }
                        if ui.button("Abbrechen").clicked() {
                            cancel = true;
                        }
                    });
                });
            let target = if ok {
                match workspace::create_file(cx.project, name, &self.text) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        cx.error(e);
                        None
                    }
                }
            } else if other {
                rfd::FileDialog::new().add_filter("SQL-Datei", &["sql"]).set_file_name(format!("{name}.sql")).save_file()
            } else {
                None
            };
            if let Some(p) = target {
                let old = workspace::scratch_path(self.scratch_id);
                self.file = Some(p.clone());
                self.project = project_of(&p);
                self.editor.id = egui::Id::new(("sql-file", p.to_string_lossy().to_lowercase()));
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
        // Verlauf (fruehere Fassungen)
        if let Some(list) = self.history.clone() {
            let mut close = false;
            egui::Window::new("Frühere Fassungen")
                .collapsible(false)
                .default_size([520.0, 360.0])
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if list.is_empty() {
                        ui.label("Noch keine früheren Fassungen gespeichert (alle 5 Minuten beim Bearbeiten).");
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
                    if ui.button("Schließen").clicked() {
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

    fn schema_changed(&mut self, db: &str, _cx: &mut Ctx) {
        if db == self.database {
            self.words_for = "\u{0}".into();
        }
    }

    fn key(&self) -> Option<String> {
        self.file.as_ref().map(|f| format!("file:{}", f.to_string_lossy().to_lowercase()))
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
                self.project = project_of(&n);
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

    fn save_now(&mut self) {
        self.save();
    }

    fn status(&self) -> Option<String> {
        let (l, c) = self.editor.line_col;
        Some(format!("Zeile {l}, Spalte {c}"))
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        let pal = style::pal();
        if self.job.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        // Autosave 1 s nach der letzten Aenderung
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

        // Kopfzeile: Pfad, Datenbank, Aktionen
        ui.horizontal(|ui| {
            let crumb = match (&self.file, &self.project) {
                (Some(f), Some(p)) => {
                    let proj = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let rel = f.strip_prefix(p).unwrap_or(f).to_string_lossy().replace(['\\', '/'], " › ");
                    format!("{proj} › {rel}")
                }
                (Some(f), None) => f.display().to_string(),
                (None, _) => format!("Unbenannt-{} (nicht in einem Projekt)", self.scratch_id),
            };
            ui.label(RichText::new(crumb).small().color(pal.text_weak));
            ui.separator();
            ui.label(RichText::new("Datenbank:").small());
            let mut dbs = vec![String::new()];
            dbs.extend(cx.databases.iter().cloned());
            let mut d = self.database.clone();
            if super::str_combo(ui, self.editor.id.with("db"), &dbs, &mut d, 150.0) {
                self.set_database(d);
                self.words_for = "\u{0}".into();
            }
            ui.separator();
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("▶ Ausführen"))
                .on_hover_text("Ganze Datei oder markierten Teil ausführen (Strg+Alt+S oder F5)")
                .clicked()
            {
                self.run_selection_or_all(cx);
            }
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("▶ Anweisung"))
                .on_hover_text("Nur die Anweisung unter dem Cursor ausführen (Strg+Enter)")
                .clicked()
            {
                self.run_current(cx);
            }
            if ui.button("Formatieren").on_hover_text("SQL schön formatieren (Strg+Alt+L)").clicked() {
                self.text = sqledit::format_sql(&self.text);
                self.last_edit = Instant::now();
            }
            ui.menu_button("⋯", |ui| {
                if self.file.is_none() && ui.button("Speichern unter...").clicked() {
                    self.save_as = Some(String::from("Neue Abfrage"));
                    ui.close();
                }
                if self.file.is_some() && ui.button("Frühere Fassungen...").clicked() {
                    self.history = Some(workspace::history(self.file.as_ref().unwrap()));
                    ui.close();
                }
                if ui.button("In VS Code öffnen").clicked() {
                    self.save();
                    let res = match &self.file {
                        Some(f) => crate::vscode::open_path(self.project.as_deref(), f, &self.database),
                        None => crate::vscode::open_sql(&format!("unbenannt-{}.sql", self.scratch_id), &self.text, &self.database),
                    };
                    match res {
                        Ok(_) => cx.status("In VS Code geöffnet."),
                        Err(e) => cx.error(e),
                    }
                    ui.close();
                }
                ui.checkbox(&mut self.editor.minimap, "Minimap anzeigen");
            });
            if let Some(e) = &self.save_error {
                ui.label(RichText::new(e).color(pal.error_text));
            }
        });

        let row_h = ui.ctx().fonts_mut(|f| f.row_height(&egui::FontId::monospace(13.0)));
        // Ergebnisbereich unten (Hoehe verstellbar), Editor darueber
        egui::Panel::bottom(self.editor.id.with("results"))
            .resizable(true)
            .default_size(ui.available_height() * 0.38)
            .min_size(80.0)
            .frame(egui::Frame::new().fill(pal.face).inner_margin(egui::Margin::symmetric(4, 4)))
            .show(ui, |ui| {
                self.results_ui(ui, row_h);
            });
        let h = (ui.available_height() - 14.0).max(100.0);
        let out = self.editor.show(ui, &mut self.text, &self.words, h);
        self.selection = out.selection.clone();
        if out.changed {
            self.last_edit = Instant::now();
        }
        if out.format {
            self.text = sqledit::format_sql(&self.text);
            self.last_edit = Instant::now();
        }
        if out.run {
            self.run_selection_or_all(cx);
        }
        if out.run_current {
            self.run_current(cx);
        }
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx, cx);
    }
}
