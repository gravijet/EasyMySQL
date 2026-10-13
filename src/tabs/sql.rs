// SQL-Editor-Registerkarte: eine Datei (oder unbenannte Abfrage), auf Wunsch automatisch gespeichert.

use super::{Action, Ctx, TabView};
use crate::db::{self, Stmt, StmtResult};
use crate::grid::{self, GridState};
use crate::icons::{self, Icon};
use crate::keymap::Cmd;
use crate::sqledit::{self, SqlEditor};
use crate::style;
use crate::workspace;
use eframe::egui::{self, RichText};
use mysql::Conn;
use mysql::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Hoehe des Ergebnisfensters, wenn nur die Kopfzeile sichtbar ist
const RESULTS_HEADER: f32 = 34.0;

/// Dateien kurz nach der letzten Aenderung speichern (Einstellungen)
pub static AUTOSAVE: AtomicBool = AtomicBool::new(true);

/// Was im Ergebnisbereich gerade gezeigt wird
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Set(usize),
    Messages,
    History,
}

/// Wunsch aus dem Verlauf-Reiter
enum HistoryAct {
    Insert(String),
    Run(String),
}

type JobResult = (Option<Conn>, Vec<StmtResult>, Option<String>, Option<String>);

pub struct SqlTab {
    /// Datei (None = unbenannte Abfrage, gesichert im Speicherordner)
    pub file: Option<PathBuf>,
    scratch_id: usize,
    /// Ergebnisfenster eingeklappt (nur Kopfzeile)
    results_collapsed: bool,
    /// Projektordner, in dem die Datei liegt
    project: Option<PathBuf>,
    pub database: String,
    pub text: String,
    saved_text: String,
    last_edit: Instant,
    file_mtime: Option<std::time::SystemTime>,
    file_check: Option<Instant>,
    conn: Option<Conn>,
    job: Option<Receiver<JobResult>>,
    /// Anweisungen des laufenden Auftrags (fuer den Verlauf)
    running: Vec<Stmt>,
    results: Vec<StmtResult>,
    note: Option<String>,
    pane: Pane,
    /// Meldungen und Verlauf wurden weggeklickt
    hide_messages: bool,
    show_history: bool,
    history_filter: String,
    grid: GridState,
    editor: SqlEditor,
    selection: Option<String>,
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
            results_collapsed: false,
            saved_text: String::new(),
            last_edit: Instant::now(),
            file_mtime: None,
            file_check: None,
            conn: None,
            job: None,
            running: Vec::new(),
            results: Vec::new(),
            note: None,
            pane: Pane::Messages,
            hide_messages: false,
            show_history: false,
            history_filter: String::new(),
            grid: GridState::default(),
            editor: SqlEditor::new(egui::Id::new(("sql-editor", next_scratch_id() + 100000))),
            selection: None,
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
            Err(e) => self.save_error = Some(crate::tr_format!("Speichern fehlgeschlagen: {e}", "Save failed: {e}")),
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
            cx.error(crate::i18n::text("Keine Verbindung zum Datenbankserver."));
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
        let info = dbc.info.clone();
        let (tx, rx) = channel();
        self.running = stmts.clone();
        std::thread::spawn(move || {
            let mut conn = conn;
            let fail = |conn: Conn, msg: String| (Some(conn), vec![StmtResult { error: Some(msg), ..Default::default() }], None, None);
            if !database.is_empty() {
                let cur: Option<String> = conn.query_first::<Option<String>, _>("SELECT DATABASE()").ok().flatten().flatten();
                if cur.as_deref() != Some(database.as_str())
                    && let Err(e) = conn.query_drop(format!("USE {}", db::q(&database))) {
                        let _ = tx.send(fail(conn, db::err_text(e)));
                        return;
                    }
            }
            // Vor DROP/TRUNCATE/DELETE ohne WHERE automatisch sichern
            let targets = crate::backup::destructive_targets(&sql, Some(&database));
            let mut note = None;
            if !targets.is_empty()
                && let Some(env) = crate::backup::env() {
                    match crate::backup::create(&env, Some(targets.clone()), "vor-loeschen", &|_| {}) {
                        Ok(_) => note = Some(crate::tr_format!("Vorher gesichert: {}", "Backup created: {}", targets.join(", "))),
                        Err(e) => {
                            let _ = tx.send(fail(conn, crate::tr_format!("Nicht ausgeführt: Die Sicherung vor dem Löschen ist fehlgeschlagen ({e})", "Not executed: The backup before deletion failed ({e})")));
                            return;
                        }
                    }
                }
            // Liefert eine Leseabfrage mehr Zeilen als gespeichert werden, wird sie auf dem Server beendet
            let (info, id) = (info, conn.connection_id());
            let results = db::run_statements(&mut conn, &stmts, Some(&|| db::kill_query(&info, id)));
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

    /// Markierung bzw. Anweisung am Cursor als fertige Java-Klasse (JDBC) in die Zwischenablage
    fn copy_java(&mut self, cx: &mut Ctx) {
        let sql = match self.selection.clone() {
            Some(s) => s,
            None => match self.current_statement() {
                Some(s) => s.sql,
                None => return,
            },
        };
        let target = crate::javagen::Target::new(&cx.settings.conn, &self.database);
        cx.actions.push(Action::CopyText(crate::javagen::query_class(&sql, &target)));
        cx.status(crate::i18n::text("Java-Code kopiert."));
    }

    /// Alle Fehler mit Korrekturvorschlag selbst ausbessern
    fn fix_all(&mut self, cx: &mut Ctx) {
        let n = self.editor.fix_all(&mut self.text);
        if n > 0 {
            self.last_edit = Instant::now();
            cx.status(crate::tr_format!("{n} Stelle(n) korrigiert.", "{n} place(s) corrected."));
        } else {
            cx.status(crate::i18n::text("Nichts zu korrigieren."));
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
        self.hide_messages = false;
        self.pane = if n_sets > 0 && !results.iter().any(|r| r.error.is_some()) { Pane::Set(n_sets - 1) } else { Pane::Messages };
        self.results = results;
        self.note = note;
    }

    fn results_ui(&mut self, ui: &mut egui::Ui, row_h: f32, history: &[crate::qhistory::Entry]) -> Option<HistoryAct> {
        let collapsed = self.results_collapsed;
        let mut toggle = false;
        let mut act = None;
        let pal = style::pal();
        let sets: Vec<(usize, usize, usize)> = self
            .results
            .iter()
            .enumerate()
            .flat_map(|(ri, r)| (0..r.sets.len()).map(move |si| (ri, si, r.sets[si].rows.len())))
            .collect();
        let n_sets = sets.len();
        let mut pane = match self.pane {
            Pane::Set(i) if i >= n_sets => Pane::Messages,
            Pane::Messages if self.hide_messages => if self.show_history { Pane::History } else { Pane::Set(n_sets.saturating_sub(1)) },
            p => p,
        };
        let mut close_set = None;
        // Reiter
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, &(ri, _, rows)) in sets.iter().enumerate() {
                let label = crate::tr_format!("Ergebnis {} · Z. {} ({})", "Result {} · Ln {} ({})", i + 1, self.results[ri].line + 1, rows);
                let c = super::chip(ui, &label, None, pane == Pane::Set(i));
                if c.clicked {
                    pane = Pane::Set(i);
                }
                if c.closed {
                    close_set = Some(i);
                }
            }
            if !self.hide_messages {
                let errs = self.results.iter().filter(|r| r.error.is_some()).count();
                let label = if errs > 0 { crate::tr_format!("Meldungen ({errs} Fehler)", "Messages ({errs} errors)") } else { crate::tr_format!("Meldungen ({})", "Messages ({})", self.results.len()) };
                let c = super::chip(ui, &label, (errs > 0).then_some(pal.error_text), pane == Pane::Messages);
                if c.clicked {
                    pane = Pane::Messages;
                }
                if c.closed && (n_sets > 0 || self.show_history) {
                    self.hide_messages = true;
                }
            }
            if self.show_history {
                let c = super::chip(ui, &crate::i18n::text("Verlauf"), None, pane == Pane::History);
                if c.clicked {
                    pane = Pane::History;
                }
                if c.closed {
                    self.show_history = false;
                    pane = if n_sets > 0 && self.hide_messages { Pane::Set(n_sets - 1) } else { Pane::Messages };
                    if n_sets == 0 {
                        self.hide_messages = false;
                    }
                }
            }
            if self.job.is_some() {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, tip) = if collapsed { (Icon::Up, crate::i18n::text("Ergebnisse einblenden")) } else { (Icon::Down, crate::i18n::text("Ergebnisse ausblenden")) };
                if icons::button(ui, icon, tip).clicked() {
                    toggle = true;
                }
                if icons::button(ui, Icon::History, crate::i18n::text("Verlauf")).clicked() {
                    self.show_history = true;
                    pane = Pane::History;
                    self.results_collapsed = false;
                }
            });
        });
        if let Some(i) = close_set {
            let (ri, si, _) = sets[i];
            self.results[ri].sets.remove(si);
            self.grid.reset();
            pane = match pane {
                Pane::Set(p) if p > i => Pane::Set(p - 1),
                Pane::Set(p) if p == i && n_sets > 1 => Pane::Set(i.min(n_sets - 2)),
                Pane::Set(p) if p == i => Pane::Messages,
                other => other,
            };
            if n_sets == 1 && !self.show_history {
                self.hide_messages = false;
            }
        }
        if toggle {
            self.results_collapsed = !self.results_collapsed;
        }
        if pane != self.pane {
            self.grid.reset();
        }
        self.pane = pane;
        // Nur noch die Kopfzeile sichtbar: der Rest hat keinen Platz
        if collapsed || ui.available_height() < 30.0 {
            return None;
        }
        match pane {
            Pane::Set(_) if n_sets == 0 => {}
            Pane::Set(tab) => {
                let (ri, si, _) = sets[tab.min(n_sets - 1)];
                let set = &self.results[ri].sets[si];
                ui.horizontal(|ui| {
                    let mut info = crate::tr_format!("{} Zeile(n)", "{} row(s)", set.rows.len());
                    if set.truncated {
                        info.push_str(&crate::tr_format!(" (die ersten {})", " (first {})", db::MAX_ROWS));
                    }
                    ui.label(RichText::new(info).color(pal.ok_text));
                    grid::export_menu(ui, "ergebnis", &set.columns, &set.rows);
                });
                grid::show(ui, ("sqlgrid", self.editor.id, tab), &set.columns, &set.rows, false, &mut self.grid);
            }
            Pane::Messages => {
                if let Some(n) = &self.note {
                    ui.label(RichText::new(n).small().color(pal.text_weak));
                }
                let mut jump = None;
                egui::ScrollArea::vertical().id_salt(self.editor.id.with("msgs")).auto_shrink([false, false]).show(ui, |ui| {
                    for r in &self.results {
                        let (ok, msg) = match &r.error {
                            Some(e) => (false, e.clone()),
                            None if r.returned.is_some() => (true, crate::tr_format!("{} Zeile(n) geliefert", "{} row(s) returned", r.returned.unwrap_or(0))),
                            None => (true, crate::tr_format!("{} Zeile(n) betroffen", "{} row(s) affected", r.affected)),
                        };
                        let color = if ok { pal.ok_text } else { pal.error_text };
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 3.5, color);
                            if ui.link(crate::tr_format!("Zeile {}", "Line {}", r.line + 1)).clicked() {
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
            Pane::History => {
                ui.add(egui::TextEdit::singleline(&mut self.history_filter).hint_text(crate::i18n::text("Filtern")).desired_width(240.0));
                let f = self.history_filter.to_lowercase();
                let items: Vec<&crate::qhistory::Entry> = history.iter().filter(|e| f.is_empty() || e.sql.to_lowercase().contains(&f)).collect();
                let h = 20.0;
                egui::ScrollArea::vertical().id_salt(self.editor.id.with("hist")).auto_shrink([false, false]).show_rows(ui, h, items.len(), |ui, range| {
                    for e in &items[range] {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 2.0, pal.hover);
                        }
                        let p = ui.painter_at(r);
                        let meta = format!("{}  {} ms", e.short_time(), e.millis);
                        p.text(egui::pos2(r.left() + 6.0, r.center().y), egui::Align2::LEFT_CENTER, meta, egui::FontId::proportional(11.5), pal.text_weak);
                        let sql: String = e.sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
                        p.text(egui::pos2(r.left() + 150.0, r.center().y), egui::Align2::LEFT_CENTER, sql, egui::FontId::monospace(12.0), if e.error { pal.error_text } else { pal.text });
                        let resp = resp.on_hover_text(e.sql.chars().take(1500).collect::<String>());
                        if resp.double_clicked() {
                            act = Some(HistoryAct::Insert(e.sql.clone()));
                        }
                        resp.context_menu(|ui| {
                            if ui.button(crate::i18n::text("In Editor einfügen")).clicked() {
                                act = Some(HistoryAct::Insert(e.sql.clone()));
                                ui.close();
                            }
                            if ui.button(crate::i18n::text("Erneut ausführen")).clicked() {
                                act = Some(HistoryAct::Run(e.sql.clone()));
                                ui.close();
                            }
                            if ui.button(crate::i18n::text("Kopieren")).clicked() {
                                ui.ctx().copy_text(e.sql.clone());
                                ui.close();
                            }
                        });
                    }
                });
            }
        }
        act
    }

    fn dialogs(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        // Speichern unter (fuer unbenannte Abfragen)
        if let Some(name) = self.save_as.as_mut() {
            let mut ok = false;
            let mut other = cx.project.is_none();
            let mut cancel = false;
            if let Some(project) = cx.project {
                egui::Window::new(crate::i18n::text("Speichern unter"))
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(crate::tr_format!("Im Projekt „{}“:", "In project “{}”:", workspace::dir_name(project)));
                        let r = super::name_field(ui, egui::Id::new("save-as-name"), name, 260.0, true);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::text("Speichern")).clicked() {
                                ok = true;
                            }
                            if ui.button(crate::i18n::text("Anderer Ort …")).clicked() {
                                other = true;
                            }
                            if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
                let p = rfd::FileDialog::new().add_filter(crate::i18n::text("SQL-Datei"), &["sql"]).set_file_name(format!("{name}.sql")).save_file();
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
                cx.status(crate::tr_format!("Gespeichert: {}", "Saved: {}", p.display()));
            } else if cancel {
                self.save_as = None;
            }
        }
        // Fruehere Fassungen
        if let Some(list) = self.history.clone() {
            let mut close = false;
            egui::Window::new(crate::i18n::text("Frühere Fassungen"))
                .collapsible(false)
                .default_size([520.0, 360.0])
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if list.is_empty() {
                        ui.label(crate::i18n::text("Keine."));
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
                                if ui.small_button(crate::i18n::text("Wiederherstellen")).clicked()
                                    && let Ok(t) = std::fs::read_to_string(path) {
                                        self.text = t;
                                        self.last_edit = Instant::now();
                                        close = true;
                                    }
                            });
                        }
                    });
                    if ui.button(crate::i18n::text("Schließen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
    fn icon(&self) -> Option<Icon> {
        Some(if self.file.is_some() { Icon::FileCode } else { Icon::NewFile })
    }

    fn modified(&self) -> bool {
        self.file.is_some() && self.dirty()
    }

    fn title(&self) -> String {
        
        match &self.file {
            Some(f) => f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => crate::tr_format!("Unbenannt-{}", "Untitled-{}", self.scratch_id),
        }
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
            Cmd::FixAll => self.fix_all(cx),
            Cmd::CopyJava => self.copy_java(cx),
            Cmd::Find | Cmd::Replace | Cmd::GotoLine => self.editor.pending = Some(cmd),
            _ => return false,
        }
        true
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
        if let Some(f) = &self.file
            && (f == old || f.starts_with(old)) {
                let rel = f.strip_prefix(old).unwrap_or(Path::new(""));
                let n = if rel.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rel) };
                self.file = Some(n.clone());
                if let Some(p) = &self.project
                    && !n.starts_with(p) {
                        self.project = None;
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

    fn unsaved(&self) -> Option<String> {
        let f = self.file.as_ref()?;
        (self.dirty() && !AUTOSAVE.load(Ordering::Relaxed)).then(|| workspace::dir_name(f))
    }

    fn discard_changes(&mut self) {
        self.text = self.saved_text.clone();
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
            self.save_as = Some(crate::i18n::text("Neue Abfrage").into());
        }
        self.save();
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        let pal = style::pal();
        if self.job.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        // Automatisch speichern 1 s nach der letzten Aenderung. Unbenannte Abfragen werden immer gesichert,
        // Dateien nur mit der Einstellung "Automatisch speichern" (sonst Strg+S).
        if self.dirty() && (self.file.is_none() || AUTOSAVE.load(Ordering::Relaxed)) {
            if self.last_edit.elapsed() > Duration::from_millis(1000) {
                self.save();
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        }
        // Aenderungen von aussen (anderer Editor) uebernehmen
        if let (Some(f), false) = (&self.file, self.dirty()) {
            if self.file_check.is_none_or(|last| last.elapsed() >= Duration::from_secs(2)) {
                self.file_check = Some(Instant::now());
                if let Ok(m) = std::fs::metadata(f).and_then(|m| m.modified())
                    && self.file_mtime.is_some_and(|old| old != m) {
                        if let Ok(b) = std::fs::read(f) {
                            self.text = String::from_utf8_lossy(&b).into_owned();
                            self.saved_text = self.text.clone();
                        }
                        self.file_mtime = Some(m);
                    }
            }
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
        // Struktur fuer die Pruefung der Tabellen- und Spaltennamen
        self.editor.schema = cx.schema(&self.database);
        if self.editor.databases.len() != cx.databases.len() {
            self.editor.databases = cx.databases.to_vec();
        }

        // Kopfzeile: Datenbank, Aktionen, Position
        ui.horizontal(|ui| {
            let mut dbs = vec![String::new()];
            dbs.extend(cx.databases.iter().cloned());
            let mut d = self.database.clone();
            if super::str_combo(ui, self.editor.id.with("db"), &dbs, &mut d, 150.0) {
                self.set_database(d);
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                if icons::primary_button(ui, Icon::Play, crate::i18n::text("Ausführen"))
                    .on_hover_text(crate::tr_format!("Datei oder Markierung ausführen ({})", "Run file or selection ({})", crate::keymap::text(Cmd::RunAll)))
                    .clicked()
                {
                    self.run_selection_or_all(cx);
                }
            });
            if ui.button(crate::i18n::text("Formatieren")).on_hover_text(crate::keymap::text(Cmd::Format)).clicked() {
                self.format();
            }
            if self.file.is_some() {
                let tip = format!("{} ({})", crate::i18n::text("Speichern"), crate::keymap::text(Cmd::Save));
                if ui.add_enabled_ui(self.dirty(), |ui| icons::button(ui, Icon::Save, &tip)).inner.clicked() {
                    self.save();
                }
            }
            let (errors, warnings, fixable) = self.editor.lint_summary();
            if errors + warnings > 0 {
                let color = if errors > 0 { pal.error_text } else { pal.warn_text };
                let summary = match (errors, warnings) {
                    (e, 0) => crate::tr_format!("{e} Fehler", "{e} error(s)"),
                    (0, w) => crate::tr_format!("{w} Hinweis(e)", "{w} warning(s)"),
                    (e, w) => crate::tr_format!("{e} Fehler, {w} Hinweis(e)", "{e} error(s), {w} warning(s)"),
                };
                ui.label(RichText::new(summary).small().color(color));
                if fixable > 0
                    && icons::text_button(ui, Icon::Sparkles, crate::i18n::text("Korrigieren"))
                        .on_hover_text(crate::tr_format!("Alle Fehler mit Vorschlag selbst ausbessern ({})", "Fix every problem that has a suggestion ({})", crate::keymap::text(Cmd::FixAll)))
                        .clicked()
                {
                    self.fix_all(cx);
                }
            }
            let more = icons::button(ui, Icon::More, crate::i18n::text("Weitere"));
            egui::Popup::menu(&more).show(|ui| {
                if ui.button(crate::i18n::text("Speichern unter …")).clicked() {
                    self.save_as = Some(self.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or(crate::i18n::text("Neue Abfrage").into()));
                    ui.close();
                }
                if let Some(f) = self.file.clone()
                    && ui.button(crate::i18n::text("Frühere Fassungen …")).clicked() {
                        self.history = Some(workspace::history(&f));
                        ui.close();
                    }
            });
            if let Some(e) = &self.save_error {
                ui.label(RichText::new(e).color(pal.error_text));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (l, c) = self.editor.line_col;
                ui.label(RichText::new(crate::tr_format!("Z. {l}, Sp. {c}", "Ln {l}, Col {c}")).small().color(pal.text_weak));
            });
        });

        let font_row = ui.ctx().fonts_mut(|f| f.row_height(&egui::FontId::monospace(sqledit::font_size())));
        if let Some(l) = self.pending_line.take() {
            let ctx = ui.ctx().clone();
            self.editor.goto_line(&ctx, &self.text, l, font_row);
        }
        // Ergebnisbereich unten (Hoehe verstellbar), Editor darueber
        // Das Ergebnisfenster laesst sich bis auf die Kopfzeile herunterziehen oder mit dem Pfeil einklappen
        let total = ui.available_height();
        let panel = if self.results_collapsed {
            egui::Panel::bottom(self.editor.id.with("results-collapsed")).resizable(false).exact_size(RESULTS_HEADER)
        } else {
            egui::Panel::bottom(self.editor.id.with("results"))
                .resizable(true)
                .default_size(total * 0.38)
                .min_size(RESULTS_HEADER)
                .max_size((total - 90.0).max(RESULTS_HEADER))
        };
        let mut hist_act = None;
        panel.frame(egui::Frame::new().fill(pal.face).inner_margin(egui::Margin::symmetric(4, 4))).show(ui, |ui| {
            hist_act = self.results_ui(ui, font_row, cx.history);
        });
        match hist_act {
            Some(HistoryAct::Insert(sql)) => {
                if !self.text.is_empty() && !self.text.ends_with('\n') {
                    self.text.push('\n');
                }
                self.text.push_str(sql.trim_end_matches(';'));
                self.text.push_str(";\n");
                self.last_edit = Instant::now();
            }
            Some(HistoryAct::Run(sql)) => self.run(cx, sql, 0),
            None => {}
        }
        let h = (ui.available_height() - 4.0).max(100.0);
        let out = self.editor.show(ui, &mut self.text, h);
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

#[cfg(test)]
mod performance_tests {
    use super::*;

    /// Run with `cargo test performance_render -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual render benchmark"]
    fn performance_render_large_result() {
        let ctx = egui::Context::default();
        let mut tab = SqlTab::empty();
        tab.results = vec![StmtResult {
            line: 0, preview: "SELECT".into(), affected: 0, error: None,
            error_line: None, elapsed: Duration::ZERO, returned: None,
            sets: vec![db::ResultSet {
                columns: (0..12).map(|i| format!("column_{i}")).collect(),
                rows: (0..50_000).map(|r| (0..12).map(|c| Some(format!("value_{r}_{c}"))).collect()).collect(),
                affected: 0, truncated: false,
            }],
        }];
        let mut render = || {
            let _ = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 760.0))),
                ..Default::default()
            }, |ui| { tab.results_ui(ui, 18.0, &[]); });
        };
        for _ in 0..5 { render(); }
        let start = Instant::now();
        for _ in 0..30 { render(); }
        println!("50,000 rows × 12 columns: {:.3} ms/frame", start.elapsed().as_secs_f64() * 1000.0 / 30.0);
    }
}

#[cfg(test)]
mod autosave_tests {
    use super::*;

    #[test]
    fn unsaved_changes_are_reported_without_autosave_and_can_be_discarded() {
        let dir = std::env::temp_dir().join(format!("easymysql-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.sql");
        std::fs::write(&file, "SELECT 1;").unwrap();
        let mut tab = SqlTab::open_file(&file, "", None).unwrap();
        tab.text = "SELECT 2;".into();
        AUTOSAVE.store(false, Ordering::Relaxed);
        assert_eq!(tab.unsaved().as_deref(), Some("a.sql"));
        tab.discard_changes();
        assert_eq!(tab.unsaved(), None);
        assert_eq!(tab.text, "SELECT 1;");
        // Mit Autosave gibt es nichts nachzufragen
        tab.text = "SELECT 3;".into();
        AUTOSAVE.store(true, Ordering::Relaxed);
        assert_eq!(tab.unsaved(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
