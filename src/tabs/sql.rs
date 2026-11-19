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
const RESULTS_HEADER: f32 = 50.0;

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
    /// Erkannten Fehler beheben
    Fix(crate::dev::autofix::Fix),
    /// Korrigierte Anweisung einsetzen und erneut ausfuehren (Index in `Failed::fixes`)
    ErrorFix(usize),
    /// In dieser Datenbank erneut ausfuehren
    UseDatabase(String),
}

/// Zuletzt fehlgeschlagene Anweisung samt Behebungen
struct Failed {
    /// Zeichenbereich der Anweisung im Text und ihr SQL
    start: usize,
    end: usize,
    sql: String,
    /// Ende des ausgefuehrten Bereichs (danach wird weiter ausgefuehrt)
    run_end: usize,
    fixes: Vec<crate::sqledit::errfix::ErrorFix>,
    /// Datenbanken, in denen die Tabellen der Anweisung liegen (bei "No database selected")
    databases: Vec<String>,
}

type JobResult = (Option<Conn>, Vec<StmtResult>, Option<String>, Option<String>);

/// Auto-Fix mit Probelauf (laeuft im Hintergrund, Ergebnis als Leiste ueber dem Editor)
struct AutoFix {
    rx: Option<Receiver<sqledit::probe::Report>>,
    status: std::sync::Arc<std::sync::Mutex<String>>,
    cancel: std::sync::Arc<AtomicBool>,
    /// Text beim Start (zum Rueckgaengigmachen und um spaetere Aenderungen zu erkennen)
    before: String,
    report: Option<sqledit::probe::Report>,
    /// Text nach dem Auto-Fix (Rueckgaengig nur, solange er unveraendert ist)
    after: String,
    /// Nach einem Fehler beim Ausfuehren: repariert wird nur der Rest ab der fehlgeschlagenen Anweisung
    heal: Option<Heal>,
    /// Bereich, den "Ausfuehren" startet (Zeichen im Text nach dem Auto-Fix); None = alles
    region: Option<(usize, usize)>,
    /// Die Korrektur wurde schon von selbst weiter ausgefuehrt
    ran: bool,
}

/// Rest eines fehlgeschlagenen Laufs (Zeichenbereich im Text beim Start der Reparatur)
struct Heal {
    start: usize,
    end: usize,
}

/// Wunsch aus der Auto-Fix-Leiste
enum FixAct {
    Run,
    Undo,
    Close,
    Cancel,
    Goto(usize),
}

pub struct SqlTab {
    /// Datei (None = unbenannte Abfrage, gesichert im Speicherordner)
    pub file: Option<PathBuf>,
    scratch_id: usize,
    /// Ergebnisfenster eingeklappt (nur Kopfzeile)
    results_collapsed: bool,
    /// Projektordner, in dem die Datei liegt
    project: Option<PathBuf>,
    pub database: String,
    /// Gespeicherter Server dieser Datei; `None` = der globale Server
    server: Option<String>,
    pub text: String,
    saved_text: String,
    last_edit: Instant,
    file_mtime: Option<std::time::SystemTime>,
    file_check: Option<Instant>,
    conn: Option<Conn>,
    job: Option<Receiver<JobResult>>,
    /// Anweisungen des laufenden Auftrags (Zeichenbereiche im Text) und Ende des ausgefuehrten Bereichs
    running: Vec<Stmt>,
    run_end: usize,
    failed: Option<Failed>,
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
    /// Beim ersten Anzeigen den Editor fokussieren (neue Abfrage: sofort lostippen)
    focus_editor: bool,
    autofix: Option<AutoFix>,
    /// Automatische Reparaturen in Folge seit dem letzten Ausfuehren durch den Nutzer
    heal_round: u8,
    /// Der naechste Lauf setzt eine automatische Reparatur fort (zaehlt nicht als neuer Start)
    heal_run: bool,
    /// Ausfuehren ohne Verbindung: laeuft, sobald der Server bereit ist (SQL, Position, seit wann)
    wait_run: Option<(String, usize, Instant)>,
}

/// So oft repariert ein Lauf sich hoechstens selbst, bevor der Nutzer wieder entscheidet
const MAX_HEALS: u8 = 3;

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
            server: None,
            text: String::new(),
            results_collapsed: false,
            saved_text: String::new(),
            last_edit: Instant::now(),
            file_mtime: None,
            file_check: None,
            conn: None,
            job: None,
            running: Vec::new(),
            run_end: 0,
            failed: None,
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
            focus_editor: false,
            autofix: None,
            heal_round: 0,
            heal_run: false,
            wait_run: None,
        }
    }

    /// Neue unbenannte Abfrage (wird trotzdem laufend gesichert).
    pub fn scratch(database: Option<String>, text: String) -> Self {
        let mut t = Self::empty();
        t.scratch_id = next_scratch_id();
        t.editor = SqlEditor::new(egui::Id::new(("sql-scratch", t.scratch_id)));
        t.database = database.unwrap_or_default();
        t.text = text;
        t.focus_editor = true;
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
        t.server = workspace::file_server(&workspace::scratch_path(id)).filter(|n| crate::profiles::find(n).is_some());
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
        t.server = workspace::file_server(path).filter(|n| crate::profiles::find(n).is_some());
        // Auf einem anderen Server passt die Standarddatenbank des globalen nicht
        // Reihenfolge: fuer die Datei gemerkt, USE am Anfang der Datei, haeufigste Datenbank des Projekts, zuletzt benutzte
        t.database = workspace::file_db_any(t.project.as_deref(), path)
            .or_else(|| workspace::sql_use_db(&t.text))
            .or_else(|| t.project.as_deref().and_then(workspace::project_db))
            .unwrap_or_else(|| if t.server.is_some() { String::new() } else { default_db.to_string() });
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
        // Von aussen geaendert, waehrend hier ungespeicherte Aenderungen lagen: fremde Fassung nicht verlieren
        let external = self.file.is_some() && workspace::keep_external_version(&target, self.file_mtime, &self.text);
        if self.file.is_some() && self.saved_text != self.text {
            workspace::snapshot(&target, &self.saved_text);
        }
        match workspace::atomic_write(&target, &self.text) {
            Ok(_) => {
                self.saved_text = self.text.clone();
                self.save_error = external.then(|| {
                    crate::i18n::tr(
                        "Die Datei wurde außerhalb geändert und jetzt überschrieben. Die fremde Fassung liegt unter „Frühere Fassungen“.",
                        "The file was changed elsewhere and has now been overwritten. That version is kept under “Previous versions”.",
                    )
                    .to_string()
                });
                self.file_mtime = std::fs::metadata(&target).and_then(|m| m.modified()).ok();
            }
            Err(e) => self.save_error = Some(crate::tr_format!("Speichern fehlgeschlagen: {e}", "Save failed: {e}")),
        }
    }

    fn set_database(&mut self, db: String) {
        if db != self.database {
            self.database = db;
            if let Some(f) = &self.file {
                workspace::set_file_db_any(self.project.as_deref(), f, &self.database);
            }
        }
    }

    /// Schluessel fuer Dinge, die zur Datei gemerkt werden (Datei oder gesicherte unbenannte Abfrage)
    fn memory_key(&self) -> PathBuf {
        self.file.clone().unwrap_or_else(|| workspace::scratch_path(self.scratch_id))
    }

    fn set_server(&mut self, server: Option<String>) {
        if server != self.server {
            self.server = server;
            self.database.clear();
            // Die Verbindung gehoert zum alten Server
            self.conn = None;
            self.results.clear();
            workspace::set_file_server(&self.memory_key(), self.server.as_deref());
            workspace::set_file_db_any(self.project.as_deref(), &self.memory_key(), "");
        }
    }

    /// Auswahl des Servers dieser Datei: der globale (Standard) oder ein gespeicherter
    fn server_combo(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let default = crate::i18n::tr("Standard-Server", "Default server");
        let shown = self.server.clone().unwrap_or_else(|| default.to_string());
        let mut pick: Option<Option<String>> = None;
        crate::widgets::combo(self.editor.id.with("server")).selected_text(shown).width(140.0).show_ui(ui, |ui| {
            if ui.selectable_label(self.server.is_none(), default).on_hover_text(cx.settings.conn.label()).clicked() {
                pick = Some(None);
            }
            for p in crate::profiles::all() {
                if ui.selectable_label(self.server.as_deref() == Some(p.name.as_str()), &p.name).on_hover_text(p.label()).clicked() {
                    pick = Some(Some(p.name));
                }
            }
            ui.separator();
            if ui.button(crate::i18n::tr("Server verwalten …", "Manage servers …")).clicked() {
                cx.actions.push(Action::OpenSettings(Some("verbindung".into())));
                ui.close();
            }
        })
        .response
        .on_hover_text(crate::i18n::tr("Server, auf dem diese Datei ausgeführt wird", "Server this file runs on"));
        if let Some(p) = pick {
            self.set_server(p);
        }
    }

    /// Fuehrt `sql` (ein oder mehrere Anweisungen) aus. `offset`: Zeichenposition im Text.
    fn run(&mut self, cx: &mut Ctx, sql: String, offset: usize) {
        if self.job.is_some() {
            return;
        }
        let Some(dbc) = cx.db else {
            // Ohne Verbindung: Server starten bzw. verbinden, danach laeuft die Abfrage von selbst
            if self.server.is_none() && self.wait_run.is_none() && !sql.trim().is_empty() {
                self.wait_run = Some((sql, offset, Instant::now()));
                cx.actions.push(Action::EnsureServer);
                cx.status(crate::i18n::tr("Verbindung wird hergestellt – die Abfrage läuft danach von selbst.", "Connecting – the query runs by itself afterwards."));
            } else if self.wait_run.is_none() {
                cx.error(crate::i18n::text("Keine Verbindung zum Datenbankserver."));
            }
            return;
        };
        let mut stmts = db::split_statements(&sql);
        if stmts.is_empty() {
            return;
        }
        let base_line = sqledit::line_col(&self.text, offset).0;
        for s in &mut stmts {
            s.line += base_line;
            s.start += offset;
            s.end += offset;
        }
        self.run_end = offset + sql.chars().count();
        self.failed = None;
        if !std::mem::take(&mut self.heal_run) {
            self.heal_round = 0;
            // Ein neuer Lauf ersetzt die Leiste einer frueheren Reparatur (eine laufende Reparatur bleibt)
            if self.autofix.as_ref().is_some_and(|a| a.rx.is_none() && a.heal.is_some()) {
                self.autofix = None;
            }
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
            let mut gone_note = None;
            if !database.is_empty() {
                let cur: Option<String> = conn.query_first::<Option<String>, _>("SELECT DATABASE()").ok().flatten().flatten();
                if cur.as_deref() != Some(database.as_str())
                    && let Err(e) = conn.query_drop(format!("USE {}", db::q(&database))) {
                        let e = db::err_text(e);
                        // Gemerkte Datenbank gibt es nicht mehr: ohne sie laufen (das Skript legt sie evtl. selbst an)
                        if crate::sqledit::errfix::code(&e) == Some(1049) {
                            gone_note = Some(crate::tr_format!("Die Datenbank {database} gibt es nicht mehr – ausgeführt ohne gewählte Datenbank.", "The database {database} no longer exists – run without a selected database."));
                        } else {
                            let _ = tx.send(fail(conn, e));
                            return;
                        }
                    }
            }
            // Vor DROP/TRUNCATE/DELETE ohne WHERE automatisch sichern
            let targets = if crate::backup::BEFORE_DELETE.load(std::sync::atomic::Ordering::Relaxed) { crate::backup::destructive_targets(&sql, Some(&database)) } else { Vec::new() };
            let mut note = gone_note;
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

    /// Korrigierte Anweisung an die Stelle der fehlgeschlagenen setzen und ab dort weiter ausfuehren
    fn apply_error_fix(&mut self, cx: &mut Ctx, k: usize) {
        let Some(f) = self.failed.take() else { return };
        let Some(new) = f.fixes.get(k).map(|x| x.sql.clone()) else { return };
        let chars: Vec<char> = self.text.chars().collect();
        let seg: String = chars[f.start.min(chars.len())..f.end.min(chars.len())].iter().collect();
        // Die Anweisung muss noch dort stehen (der Text kann seitdem geaendert worden sein)
        let Some(pos) = seg.find(&f.sql) else {
            cx.status(crate::i18n::tr("Die Anweisung wurde inzwischen geändert – bitte erneut ausführen.", "The statement has changed since – please run it again."));
            return;
        };
        let a = f.start + seg[..pos].chars().count();
        let b = a + f.sql.chars().count();
        let mut out: Vec<char> = chars.clone();
        out.splice(a..b, new.chars());
        self.text = out.iter().collect();
        self.last_edit = Instant::now();
        let end = (f.run_end + new.chars().count()).saturating_sub(f.sql.chars().count()).min(out.len());
        let sql: String = out[a..end.max(a)].iter().collect();
        self.run(cx, sql, a);
    }

    /// Auto-Fix: alle Fehler beheben; mit Server zusaetzlich Probelaeufe in Wegwerf-Kopien, bis das Skript laeuft
    fn fix_all(&mut self, cx: &mut Ctx) {
        if self.autofix.as_ref().is_some_and(|a| a.rx.is_some()) || self.text.trim().is_empty() {
            return;
        }
        let Some(dbc) = cx.db else {
            let n = self.editor.fix_all(&mut self.text);
            if n > 0 {
                self.last_edit = Instant::now();
                cx.status(crate::tr_format!("{n} Stelle(n) korrigiert.", "{n} place(s) corrected."));
            } else {
                cx.status(crate::i18n::text("Nichts zu korrigieren."));
            }
            return;
        };
        let first = crate::i18n::tr("Auto-Fix startet …", "Auto-fix is starting …").to_string();
        self.start_probe(dbc, cx.databases.to_vec(), self.text.clone(), None, first);
    }

    /// Fehlgeschlagenen Lauf im Hintergrund reparieren: der Rest ab der fehlgeschlagenen Anweisung laeuft
    /// in Wegwerf-Kopien (die Anweisungen davor sind schon auf dem Server), bis er fehlerfrei durchgeht
    fn heal(&mut self, cx: &mut Ctx, start: usize, end: usize, line: usize) {
        let Some(dbc) = cx.db else { return };
        let rest: String = self.text.chars().skip(start).take(end.saturating_sub(start)).collect();
        if rest.trim().is_empty() {
            return;
        }
        self.heal_round += 1;
        let first = crate::tr_format!("Zeile {}: Fehler wird auf Kopien repariert …", "Line {}: repairing the error on copies …", line + 1);
        self.start_probe(dbc, cx.databases.to_vec(), rest, Some(Heal { start, end }), first);
    }

    /// Auto-Fix mit Probelauf im Hintergrund starten (`text`: ganzer Text oder der Rest eines Laufs)
    fn start_probe(&mut self, dbc: &crate::db::Db, databases: Vec<String>, text: String, heal: Option<Heal>, first: String) {
        let setup = sqledit::probe::Setup {
            database: Some(self.database.clone()).filter(|d| !d.is_empty()),
            databases,
            file_stem: self.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()),
        };
        let pool = crate::db::Db { pool: dbc.pool.clone(), info: dbc.info.clone(), version: dbc.version.clone() };
        let status = std::sync::Arc::new(std::sync::Mutex::new(first));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (st, cn) = (status.clone(), cancel.clone());
        let ai_ready = sqledit::ai::ready();
        std::thread::spawn(move || {
            // Frische Liste: das Skript kann eben erst Datenbanken angelegt haben
            let mut setup = setup;
            if let Ok(list) = pool.databases() {
                setup.databases = list;
            }
            let say = |m: String| {
                if let Ok(mut g) = st.lock() {
                    *g = m;
                }
            };
            let ask = |sql: &str, err: &str, schema: Option<&crate::db::Schema>| sqledit::ai::fix_statement(sql, err, schema);
            let ai: Option<sqledit::probe::Ai> = if ai_ready { Some(&ask) } else { None };
            let report = match pool.new_conn() {
                Ok(conn) => {
                    let mut live = sqledit::probe::Live { conn, db: pool };
                    sqledit::probe::repair(&text, &setup, Some(&mut live), &say, &cn, ai)
                }
                Err(e) => {
                    let mut r = sqledit::probe::repair(&text, &setup, None, &say, &cn, None);
                    r.end = sqledit::probe::End::Failed(e);
                    r
                }
            };
            let _ = tx.send(report);
        });
        self.autofix = Some(AutoFix { rx: Some(rx), status, cancel, before: self.text.clone(), report: None, after: String::new(), heal, region: None, ran: false });
    }

    /// Ergebnis des Auto-Fix uebernehmen
    fn poll_autofix(&mut self, cx: &mut Ctx) {
        let Some(a) = &mut self.autofix else { return };
        let Some(rx) = &a.rx else { return };
        let Ok(report) = rx.try_recv() else { return };
        a.rx = None;
        if self.text != a.before {
            self.autofix = None;
            cx.status(crate::i18n::tr("Auto-Fix verworfen: Der Text wurde inzwischen geändert.", "Auto-fix discarded: the text was changed in the meantime."));
            return;
        }
        if let Some(h) = &a.heal {
            let (start, end) = (h.start, h.end);
            let chars: Vec<char> = self.text.chars().collect();
            let rest: String = chars[start.min(chars.len())..end.min(chars.len())].iter().collect();
            if report.end != sqledit::probe::End::Clean || report.text == rest {
                // Keine sichere Korrektur: Fehlermeldung und Ein-Klick-Hilfen unten bleiben
                self.autofix = None;
                if report.end != sqledit::probe::End::Cancelled {
                    self.note = Some(crate::i18n::tr("Automatische Reparatur: keine sichere Korrektur gefunden.", "Automatic repair: no safe fix found.").into());
                }
                return;
            }
            let safe = !report.changes.iter().any(|c| c.check) && !sqledit::probe::adds_destruction(&rest, &report.text);
            let base = sqledit::line_col(&self.text, start).0;
            let mut report = report;
            for c in &mut report.changes {
                c.line = c.line.map(|l| l + base);
            }
            let fixed = report.text.clone();
            self.text = chars[..start.min(chars.len())].iter().chain(fixed.chars().collect::<Vec<_>>().iter()).chain(chars[end.min(chars.len())..].iter()).collect();
            self.last_edit = Instant::now();
            a.after = self.text.clone();
            a.region = Some((start, start + fixed.chars().count()));
            a.report = Some(report);
            a.ran = safe;
            if safe {
                // Auf Kopien geprueft, loescht nichts zusaetzlich: gleich weiter ausfuehren
                self.heal_run = true;
                self.run(cx, fixed, start);
            }
            return;
        }
        if report.text != self.text {
            self.text = report.text.clone();
            self.last_edit = Instant::now();
        }
        a.after = self.text.clone();
        a.report = Some(report);
    }

    /// Leiste mit dem Ergebnis des Auto-Fix
    fn autofix_ui(&mut self, ui: &mut egui::Ui) -> Option<FixAct> {
        use sqledit::probe::End;
        let a = self.autofix.as_ref()?;
        let pal = style::pal();
        let tr = crate::i18n::tr;
        let mut act = None;
        egui::Frame::new()
            .fill(pal.face_light)
            .stroke(egui::Stroke::new(1.0, pal.border))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let Some(r) = &a.report else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        let s = a.status.lock().map(|s| s.clone()).unwrap_or_default();
                        ui.label(s);
                        if ui.button(tr("Abbrechen", "Cancel")).clicked() {
                            act = Some(FixAct::Cancel);
                        }
                    });
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                    return;
                };
                let n = r.changes.len();
                let (icon, color, head) = match &r.end {
                    End::Clean if a.ran => (Icon::Check, pal.ok_text, crate::tr_format!("Fehler automatisch behoben ({n} Änderung(en), auf Kopien geprüft) und weiter ausgeführt.", "Error fixed automatically ({n} change(s), checked on copies) and execution continued.")),
                    End::Clean if a.heal.is_some() => (Icon::Alert, pal.warn_text, crate::tr_format!("Korrektur gefunden ({n} Änderung(en)). Sie löscht Daten oder enthält Platzhalter – bitte ansehen, dann weiter ausführen.", "Fix found ({n} change(s)). It deletes data or contains placeholders – please review, then continue.")),
                    End::Clean if n == 0 => (Icon::Check, pal.ok_text, tr("Alles in Ordnung: Das Skript läuft im Probelauf ohne Fehler.", "All good: the script runs without errors in a trial run.").to_string()),
                    End::Clean => (Icon::Check, pal.ok_text, crate::tr_format!("{n} Änderung(en) – das Skript läuft jetzt im Probelauf ohne Fehler.", "{n} change(s) – the script now runs without errors in a trial run.")),
                    End::Offline => (Icon::Sparkles, pal.warn_text, crate::tr_format!("{n} Stelle(n) korrigiert (ohne Server kein Probelauf).", "{n} place(s) corrected (no trial run without a server).")),
                    End::Stuck { line, error } => (Icon::Alert, pal.error_text, crate::tr_format!("Zeile {}: {} – dafür gibt es keine sichere Korrektur.", "Line {}: {} – there is no safe fix for this.", line + 1, error.lines().next().unwrap_or(""))),
                    End::Cancelled => (Icon::Alert, pal.warn_text, tr("Abgebrochen.", "Cancelled.").to_string()),
                    End::Failed(e) => (Icon::Alert, pal.error_text, crate::tr_format!("Probelauf nicht möglich: {e}", "Trial run not possible: {e}")),
                };
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(ui.painter(), rect, icon, color);
                    ui.add(egui::Label::new(RichText::new(head).color(color)).wrap());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, tr("Schließen", "Close")).clicked() {
                            act = Some(FixAct::Close);
                        }
                        if n > 0 && !a.ran && self.text == a.after && icons::text_button(ui, Icon::Undo, tr("Rückgängig", "Undo")).clicked() {
                            act = Some(FixAct::Undo);
                        }
                        let label = if a.heal.is_some() { tr("Weiter ausführen", "Continue") } else { tr("Ausführen", "Run") };
                        if r.end == End::Clean && !a.ran && self.text == a.after && icons::run_button(ui, label).clicked() {
                            act = Some(FixAct::Run);
                        }
                    });
                });
                // Lange Listen scrollen, damit der Editor sichtbar bleibt
                egui::ScrollArea::vertical().max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.spacing_mut().interact_size.y = 14.0;
                    for c in &r.changes {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            // Zeilenangaben in einer festen Spalte, damit die Texte buendig stehen
                            crate::widgets::fixed(ui, 46.0, 16.0, |ui| {
                                if let Some(l) = c.line
                                    && ui.link(RichText::new(crate::tr_format!("Z. {}", "Ln {}", l + 1)).small()).on_hover_text(crate::i18n::tr("Zur Zeile springen", "Go to the line")).clicked()
                                {
                                    act = Some(FixAct::Goto(l));
                                }
                            });
                            let text = RichText::new(&c.label).small();
                            ui.add(egui::Label::new(if c.check { text.color(pal.warn_text) } else { text }).wrap());
                        });
                    }
                });
                let mut notes: Vec<String> = Vec::new();
                if r.skipped > 0 {
                    notes.push(crate::tr_format!(
                        "{} Anweisung(en) liefen im Probelauf bewusst nicht (Benutzer, Rechte, Dateien, globale Einstellungen).",
                        "{} statement(s) were deliberately not run in the trial (users, privileges, files, global settings).",
                        r.skipped
                    ));
                }
                if r.without_data {
                    notes.push(tr("Die Datenbank ist zu groß zum Kopieren: doppelte Schlüssel und fehlende Elternzeilen wurden nicht geprüft.", "The database is too large to copy: duplicate keys and missing parent rows were not checked.").into());
                }
                if r.runs > 0 {
                    notes.push(crate::tr_format!("Geprüft in {} Probeläufen auf Kopien – deine Daten wurden nicht verändert.", "Checked in {} trial runs on copies – your data was not changed.", r.runs));
                }
                for note in notes {
                    ui.label(RichText::new(note).small().color(pal.text_weak));
                }
            });
        ui.add_space(4.0);
        act
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
        match current {
            Some(d) => self.set_database(d),
            // Gemerkte Datenbank gibt es nicht mehr: nicht weiter anzeigen
            None if !self.database.is_empty() && !cx.databases.iter().any(|d| d == &self.database) => self.set_database(String::new()),
            None => {}
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
        // Fehler: passende Behebungen vorbereiten (korrigierte Anweisung oder andere Datenbank)
        let mut heal_from: Option<(usize, usize)> = None;
        if let Some((i, err)) = results.iter().enumerate().find_map(|(i, r)| r.error.clone().map(|e| (i, e)))
            && let Some(st) = self.running.get(i).cloned()
        {
            // Nur wenn die Anweisung so im Text steht (nicht bei EXPLAIN o. ae.) und die Verbindung steht
            let seg: String = self.text.chars().skip(st.start).take(self.run_end.saturating_sub(st.start)).collect();
            if self.conn.is_some() && seg.trim_start().starts_with(st.sql.trim_start()) {
                heal_from = Some((st.start, st.line));
            }
            let schema = if db_before.is_empty() { None } else { cx.schema(&db_before) };
            let mut fixes = crate::sqledit::errfix::for_error(&err, &st.sql, schema.as_deref());
            let mut databases = Vec::new();
            if crate::sqledit::errfix::code(&err) == Some(1046) {
                // Eine vorher im Skript angelegte Datenbank, sonst alle Datenbanken mit diesen Tabellen
                let before: String = self.text.chars().take(st.start).collect();
                if let Some(d) = crate::sqledit::errfix::created_database(&before) {
                    fixes.push(crate::sqledit::errfix::ErrorFix { label: crate::tr_format!("Zuerst USE {}", "USE {} first", d), sql: format!("USE {};\n{}", db::q(&d), st.sql) });
                }
                let names = crate::sqledit::errfix::table_names(&st.sql);
                let candidates: Vec<String> = cx.databases.iter().filter(|d| !crate::db::SYSTEM_DATABASES.contains(&d.as_str())).take(40).cloned().collect();
                for d in candidates {
                    if let Some(s) = cx.schema(&d)
                        && !names.is_empty()
                        && names.iter().all(|n| s.tables.iter().any(|t| t.name.eq_ignore_ascii_case(n)))
                    {
                        databases.push(d);
                    }
                }
            }
            if !fixes.is_empty() || !databases.is_empty() {
                self.failed = Some(Failed { start: st.start, end: st.end, sql: st.sql.clone(), run_end: self.run_end, fixes, databases });
            }
        }
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
        // Fehler: im Hintergrund auf Kopien reparieren und sicher Korrigiertes weiter ausfuehren
        if let Some((start, line)) = heal_from
            && sqledit::prefs().auto_fix_run
            && self.heal_round < MAX_HEALS
            && self.autofix.as_ref().is_none_or(|a| a.rx.is_none())
        {
            self.heal(cx, start, self.run_end, line);
        }
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
                let c = super::chip(ui, crate::i18n::text("Verlauf"), None, pane == Pane::History);
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
                        info.push_str(&crate::tr_format!(" (die ersten {})", " (first {})", db::max_rows()));
                    }
                    ui.label(RichText::new(info).color(pal.text_weak));
                    grid::export_menu(ui, "ergebnis", &set.columns, &set.rows);
                });
                grid::show(ui, ("sqlgrid", self.editor.id, tab), &set.columns, &set.rows, grid::Edit::View, &mut self.grid);
            }
            Pane::Messages => {
                if let Some(n) = &self.note {
                    ui.label(RichText::new(n).small().color(pal.text_weak));
                }
                let mut jump = None;
                let mut fixes = Vec::new();
                let mut fix_pick: Option<HistoryAct> = None;
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
                        egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 0, top: 0, bottom: 6 }).show(ui, |ui| {
                            if ok {
                                ui.label(RichText::new(msg).color(color));
                            } else {
                                crate::widgets::error_label(ui, &msg);
                                // Eindeutige Korrekturen der Anweisung: ein Klick setzt sie ein und fuehrt weiter aus
                                if let Some(f) = &self.failed {
                                    ui.horizontal_wrapped(|ui| {
                                        for (k, x) in f.fixes.iter().enumerate() {
                                            if crate::icons::text_button(ui, crate::icons::Icon::Wizard, &x.label).on_hover_text(crate::tr_format!("Wird zu:\n{}", "Becomes:\n{}", x.sql)).clicked() {
                                                fix_pick = Some(HistoryAct::ErrorFix(k));
                                            }
                                        }
                                        for d in &f.databases {
                                            if crate::icons::text_button(ui, crate::icons::Icon::Database, &crate::tr_format!("In {} ausführen", "Run in {}", d)).clicked() {
                                                fix_pick = Some(HistoryAct::UseDatabase(d.clone()));
                                            }
                                        }
                                    });
                                }
                                // Erkannte Fehler: Erklaerung und Behebung (Datenbank anlegen, Tabelle anlegen …)
                                for sug in crate::dev::autofix::analyze_output(&msg, None, None, false) {
                                    ui.horizontal(|ui| {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                        crate::icons::paint(ui.painter(), r, crate::icons::Icon::Info, pal.text_weak);
                                        ui.label(RichText::new(&sug.problem).small().color(pal.text_weak));
                                        if let Some(f) = &sug.fix
                                            && crate::icons::text_button(ui, crate::icons::Icon::Wizard, &sug.label).clicked()
                                        {
                                            fixes.push(f.clone());
                                        }
                                    });
                                }
                            }
                        });
                    }
                });
                if let Some(l) = jump {
                    let ctx = ui.ctx().clone();
                    self.editor.goto_line(&ctx, &self.text, l, row_h);
                }
                if let Some(f) = fixes.into_iter().next() {
                    act = Some(HistoryAct::Fix(f));
                }
                if fix_pick.is_some() {
                    act = fix_pick;
                }
            }
            Pane::History => {
                ui.add(crate::widgets::edit(&mut self.history_filter).hint_text(crate::i18n::text("Filtern")).desired_width(240.0));
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
                        p.text(egui::pos2(r.left() + 6.0, r.center().y), egui::Align2::LEFT_CENTER, meta, egui::FontId::proportional(12.5), pal.text_weak);
                        let sql: String = e.sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
                        p.text(egui::pos2(r.left() + 150.0, r.center().y), egui::Align2::LEFT_CENTER, sql, egui::FontId::monospace(12.0), if e.error { pal.error_text } else { pal.text });
                        let resp = resp.on_hover_text(e.sql.chars().take(1500).collect::<String>());
                        if resp.double_clicked() {
                            act = Some(HistoryAct::Insert(e.sql.clone()));
                        }
                        resp.context_menu(|ui| {
                            crate::style::menu_item_look(ui);
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
        let pal_weak = || style::pal().text_weak;
        // Speichern unter (fuer unbenannte Abfragen)
        if let Some(name) = self.save_as.as_mut() {
            let mut ok = false;
            let mut other = false;
            let mut cancel = false;
            match cx.project {
                // Gespeichert wird immer in einem Projekt oder Ordner: erst einen waehlen, dann geht es hier weiter
                None => {
                    egui::Window::new(crate::i18n::text("Speichern unter"))
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.set_max_width(360.0);
                            ui.label(RichText::new(crate::i18n::tr("Kein Projekt geöffnet", "No project open")).strong());
                            ui.label(RichText::new(crate::i18n::tr("Gespeichert wird immer in einem Projekt oder Ordner. Wähle zuerst einen aus – die Abfrage bleibt dabei offen.", "Files are always saved in a project or folder. Choose one first – the query stays open.")).color(pal_weak()));
                            crate::widgets::dialog_actions(ui, |ui| {
                                if icons::primary_button(ui, Icon::Folder, crate::i18n::text("Ordner öffnen …")).clicked() {
                                    cx.actions.push(Action::Run(Cmd::OpenFolder));
                                }
                                if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    cancel = true;
                                }
                            });
                        });
                }
                Some(project) => {
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
                            crate::widgets::dialog_actions(ui, |ui| {
                                if crate::widgets::primary(ui, crate::i18n::text("Speichern")).clicked() {
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
            }
            let target = if ok {
                match cx.project.ok_or_else(|| crate::i18n::tr("Kein Projekt geöffnet", "No project open").to_string()).and_then(|p| workspace::create_file(p, name, &self.text)) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        cx.error(e);
                        None
                    }
                }
            } else if other {
                let mut dlg = rfd::FileDialog::new().add_filter(crate::i18n::text("SQL-Datei"), &["sql"]).set_file_name(format!("{name}.sql"));
                if let Some(p) = cx.project {
                    dlg = dlg.set_directory(p);
                }
                let p = dlg.save_file();
                if p.is_none() {
                    cancel = true;
                }
                p
            } else {
                None
            };
            if let Some(p) = target {
                let old = workspace::scratch_path(self.scratch_id);
                let old_key = old.clone();
                self.file = Some(p.clone());
                self.project = cx.project.filter(|pr| p.starts_with(pr)).map(|pr| pr.to_path_buf());
                self.editor.id = egui::Id::new(("sql-file", file_key(&p)));
                self.saved_text.clear();
                self.save();
                let _ = std::fs::remove_file(old);
                workspace::set_file_db_any(self.project.as_deref(), &p, &self.database);
                workspace::set_file_server(&p, self.server.as_deref());
                workspace::set_file_server(&old_key, None);
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
                                if ui.button(crate::i18n::text("Wiederherstellen")).clicked()
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
    fn server(&self) -> Option<String> {
        self.server.clone()
    }

    fn use_server(&mut self, server: Option<String>) {
        self.set_server(server);
    }

    fn adopt_database(&mut self, db: &str) {
        if self.database.is_empty() && self.job.is_none() {
            self.set_database(db.to_string());
        }
    }

    fn database(&self) -> Option<String> {
        Some(self.database.clone()).filter(|d| !d.is_empty())
    }

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

    fn runnable(&self) -> bool {
        true
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
            Cmd::RenameSymbol => self.editor.start_local_rename(&self.text),
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
            workspace::set_file_server(&workspace::scratch_path(self.scratch_id), None);
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

    fn cursor(&self) -> Option<usize> {
        Some(self.editor.cursor)
    }

    fn restore_cursor(&mut self, pos: usize) {
        self.editor.restore = Some(pos);
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
        self.poll_autofix(cx);
        if let Some((_, _, since)) = &self.wait_run {
            if cx.db.is_some() {
                if let Some((sql, offset, _)) = self.wait_run.take() {
                    self.run(cx, sql, offset);
                }
            } else if since.elapsed() > Duration::from_secs(60) {
                self.wait_run = None;
                cx.error(crate::i18n::text("Keine Verbindung zum Datenbankserver."));
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }
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
        // Ohne gewaehlte Datenbank gibt es nichts zu pruefen
        self.editor.schema = if self.database.is_empty() { None } else { cx.schema(&self.database) };
        if self.editor.databases.len() != cx.databases.len() {
            self.editor.databases = cx.databases.to_vec();
        }

        // Kopfzeile: Datenbank, Aktionen, Position
        ui.horizontal(|ui| {
            let mut dbs = vec![String::new()];
            let show_system = super::SHOW_SYSTEM_DBS.load(std::sync::atomic::Ordering::Relaxed);
            dbs.extend(cx.databases.iter().filter(|d| show_system || !super::is_system_db(d) || **d == self.database).cloned());
            self.server_combo(ui, cx);
            let mut d = self.database.clone();
            if super::db_select(ui, self.editor.id.with("db"), &dbs, &mut d, 150.0) {
                self.set_database(d);
            }
            if self.wait_run.is_some() {
                ui.spinner().on_hover_text(crate::i18n::tr("Wartet auf den Datenbankserver …", "Waiting for the database server …"));
            }
            ui.add_enabled_ui(self.job.is_none() && self.wait_run.is_none(), |ui| {
                if icons::run_button(ui, crate::i18n::text("Ausführen"))
                    .on_hover_text(crate::tr_format!("Datei oder Markierung ausführen ({})", "Run file or selection ({})", crate::keymap::text(Cmd::RunAll)))
                    .clicked()
                {
                    self.run_selection_or_all(cx);
                }
            });
            if icons::text_button(ui, Icon::Code, crate::i18n::text("Formatieren")).on_hover_text(crate::keymap::text(Cmd::Format)).clicked() {
                self.format();
            }
            if self.file.is_some() {
                let tip = format!("{} ({})", crate::i18n::text("Speichern"), crate::keymap::text(Cmd::Save));
                if ui.add_enabled_ui(self.dirty(), |ui| icons::tool_button(ui, Icon::Save, &tip)).inner.clicked() {
                    self.save();
                }
            }
            let (errors, warnings, _) = self.editor.lint_summary();
            let busy = self.autofix.as_ref().is_some_and(|a| a.rx.is_some());
            if ui
                .add_enabled_ui(!busy && self.job.is_none() && !self.text.trim().is_empty(), |ui| icons::text_button(ui, Icon::Sparkles, "Auto-Fix"))
                .inner
                .on_hover_text(crate::tr_format!(
                    "Alle Fehler beheben und das Skript in einer Kopie der Datenbank probelaufen lassen, bis es ohne Fehler läuft ({})",
                    "Fix every error and trial-run the script on a copy of the database until it runs without errors ({})",
                    crate::keymap::text(Cmd::FixAll)
                ))
                .clicked()
            {
                self.fix_all(cx);
            }
            if errors + warnings > 0 {
                let color = if errors > 0 { pal.error_text } else { pal.warn_text };
                let summary = match (errors, warnings) {
                    (e, 0) => crate::tr_format!("{e} Fehler", "{e} error(s)"),
                    (0, w) => crate::tr_format!("{w} Hinweis(e)", "{w} warning(s)"),
                    (e, w) => crate::tr_format!("{e} Fehler, {w} Hinweis(e)", "{e} error(s), {w} warning(s)"),
                };
                ui.label(RichText::new(summary).small().color(color));
            }
            if icons::tool_button(ui, Icon::History, crate::i18n::text("Verlauf")).clicked() {
                self.show_history = !self.show_history || self.pane != Pane::History;
                self.pane = if self.show_history { Pane::History } else { Pane::Messages };
                self.results_collapsed = false;
            }
            let more = icons::tool_button(ui, Icon::More, crate::i18n::text("Weitere"));
            egui::Popup::menu(&more).show(|ui| {
                crate::style::menu_item_look(ui);
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

        ui.add_space(4.0);
        let font_row = ui.ctx().fonts_mut(|f| f.row_height(&egui::FontId::monospace(sqledit::font_size())));
        match self.autofix_ui(ui) {
            Some(FixAct::Run) => {
                let region = self.autofix.take().and_then(|a| a.region);
                match region {
                    Some((a, b)) => {
                        let sql: String = self.text.chars().skip(a).take(b - a).collect();
                        self.run(cx, sql, a);
                    }
                    None => self.run(cx, self.text.clone(), 0),
                }
            }
            Some(FixAct::Undo) => {
                if let Some(a) = self.autofix.take() {
                    self.text = a.before;
                    self.last_edit = Instant::now();
                }
            }
            Some(FixAct::Close) => self.autofix = None,
            Some(FixAct::Cancel) => {
                if let Some(a) = &self.autofix {
                    a.cancel.store(true, Ordering::Relaxed);
                }
            }
            Some(FixAct::Goto(l)) => self.pending_line = Some(l),
            None => {}
        }
        if let Some(l) = self.pending_line.take() {
            let ctx = ui.ctx().clone();
            self.editor.goto_line(&ctx, &self.text, l, font_row);
        }
        // Ergebnisbereich unten (Hoehe verstellbar), Editor darueber
        // Das Ergebnisfenster laesst sich bis auf die Kopfzeile herunterziehen oder mit dem Pfeil einklappen
        // Unten nur dann ein Ergebnisbereich, wenn es etwas zu zeigen gibt (sonst gehoert der Platz dem Editor)
        let has_output = !self.results.is_empty() || self.show_history || self.job.is_some() || self.note.is_some();
        let mut hist_act = None;
        if has_output {
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
            // Trennlinie zwischen Editor und Ausgabe ist zugleich der Griff zum Verstellen
            panel.show_separator_line(true).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 })).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                hist_act = self.results_ui(ui, font_row, cx.history);
            });
        }
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
            Some(HistoryAct::Fix(f)) => cx.actions.push(Action::AutoFix(f)),
            Some(HistoryAct::ErrorFix(k)) => self.apply_error_fix(cx, k),
            Some(HistoryAct::UseDatabase(d)) => {
                if let Some(f) = self.failed.take() {
                    self.set_database(d);
                    let sql: String = self.text.chars().skip(f.start).take(f.run_end.saturating_sub(f.start)).collect();
                    self.run(cx, sql, f.start);
                }
            }
            None => {}
        }
        let h = (ui.available_height() - 8.0).max(100.0);
        // Erst fokussieren, wenn der Klick, der den Tab geoeffnet hat, vorbei ist: sonst wertet das Textfeld
        // ihn als Klick daneben und gibt den Fokus sofort wieder ab
        if self.focus_editor {
            if ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_down()) {
                ui.ctx().request_repaint();
            } else {
                self.focus_editor = false;
                ui.memory_mut(|m| m.request_focus(self.editor.id));
            }
        }
        let out = self.editor.show(ui, &mut self.text, h);
        self.selection = out.selection.clone();
        if out.changed {
            self.last_edit = Instant::now();
        }
        if out.format {
            self.format();
        }
        if let Some(n) = &out.notice {
            cx.status(n.clone());
        }
        match out.command {
            Some(Cmd::RenameSymbol) => self.editor.start_local_rename(&self.text),
            Some(cmd) => {
                TabView::command(self, cmd, cx);
            }
            None => {}
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
        // Leere SQL-Tabs zeigen zuerst Meldungen; hier muss das Raster sichtbar sein.
        tab.pane = Pane::Set(0);
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
            let ausgabe = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 760.0))),
                ..Default::default()
            }, |ui| { tab.results_ui(ui, 18.0, &[]); });
            assert!(ausgabe.shapes.len() > 100, "Benchmark muss sichtbare Tabellenzellen zeichnen");
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
