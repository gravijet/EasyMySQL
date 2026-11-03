// Registerkarte fuer Dateien ausser SQL. Java, Python und JavaScript/TypeScript: Ausfuehren, Tests,
// Debuggen (Java), .jar bauen (Java), Bibliotheken einbinden, Sprachserver (Fehler beim Tippen, Gehe zu
// Definition, Umbenennen), SQL-Pruefung in Zeichenketten und Vorlagen fuer Tabellen.
// Dazu Gradle-Skripte, XML, JSON, .properties und Text.

use super::{Action, Ctx, TabView};
use crate::dev::java::{self, run as jrun, source};
use crate::dev::process::{Failure, Launch};
use crate::dev::{self, Tool, debug, install, libs, lsp, node, python};
use crate::icons::{self, Icon};
use crate::keymap::Cmd;
use crate::sqledit::{self, Lang, SqlEditor};
use crate::style;
use crate::workspace;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Was gestartet werden soll
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Goal {
    Run,
    Test,
    Debug,
    Jar,
}

/// Ergebnis der Vorbereitung im Hintergrund
enum Prepared {
    Launch(Launch),
    /// Debuggen: Start im Terminal, dann jdb auf diesem Port mit diesen Quellordnern
    Debug(Launch, u16, Vec<PathBuf>),
    Jar(PathBuf),
}

enum RunState {
    Idle,
    /// Werkzeuge fehlen und sollen erst nach Rueckfrage geladen werden
    Ask(Vec<Tool>, Goal),
    /// Werkzeuge werden geladen; danach geht es weiter
    WaitTools(Vec<Tool>, Goal),
    /// Uebersetzen / Pakete / Syntaxpruefung im Hintergrund
    Preparing(Receiver<Result<Prepared, Failure>>, Arc<Mutex<String>>, Goal),
}

/// Ergebnis eines Auftrags im Hintergrund
type Job<T> = Receiver<Result<T, String>>;

/// Dialog "Bibliothek einbinden"
struct LibDialog {
    query: String,
    eco: libs::Ecosystem,
    target: libs::Target,
    search: Option<Job<Vec<libs::Hit>>>,
    results: Option<Result<Vec<libs::Hit>, String>>,
    adding: Option<(Job<String>, Arc<Mutex<String>>)>,
    focus: bool,
}

/// Anfrage an den Sprachserver, deren Antwort noch aussteht
enum LspWait {
    Definition(lsp::Reply),
    Rename(lsp::Reply),
    Format(lsp::Reply, String),
}

pub struct CodeTab {
    pub file: PathBuf,
    project: Option<PathBuf>,
    lang: Lang,
    pub database: String,
    pub text: String,
    saved_text: String,
    last_edit: Instant,
    file_mtime: Option<std::time::SystemTime>,
    file_check: Option<Instant>,
    editor: SqlEditor,
    save_error: Option<String>,
    pending_line: Option<usize>,
    /// Cursor nach "Gehe zu Definition" in derselben Datei (Zeichenindex)
    pending_cursor: Option<usize>,
    history: Option<Vec<(String, PathBuf)>>,
    run: RunState,
    /// Nach "Alle speichern" im naechsten Bild starten
    run_next_frame: Option<Goal>,
    failure: Option<Failure>,
    show_output: bool,
    libs: Option<LibDialog>,
    /// Sprachserver: Verbindung, gemeldeter Stand des Textes und zuletzt uebernommene Meldungen
    lsp: Option<Arc<lsp::Server>>,
    lsp_synced: Option<String>,
    lsp_diag_gen: u64,
    lsp_wait: Option<LspWait>,
    /// Hinweis des Sprachservers fuer die Stelle unter dem Mauszeiger: (Stelle, seit, Anfrage, Text)
    hover: Option<(usize, Instant, Option<lsp::Reply>, Option<String>)>,
    rename: Option<(String, usize, bool)>,
    debug_eval: String,
    lsp_note: Option<String>,
}

fn file_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

/// Datenbank aus der naechsten Verbindungsdatei eines EasyMySQL-Projekts (db.properties, db.ini, db.json)
fn database_from_config(file: &Path, project: Option<&Path>) -> Option<String> {
    let mut dir = file.parent();
    while let Some(d) = dir {
        for candidate in [d.join("db.properties"), d.join("src/main/resources/db.properties"), d.join("src/db.properties")] {
            if let Ok(text) = std::fs::read_to_string(&candidate)
                && let Some(url) = text.lines().find_map(|l| l.trim().strip_prefix("url").map(|r| r.trim_start_matches([' ', '=', ':']).trim().to_string()))
            {
                let db = url.split('?').next().unwrap_or("").rsplit_once('/').map(|(_, d)| d.to_string()).unwrap_or_default();
                return Some(db).filter(|d| !d.is_empty());
            }
        }
        if let Ok(text) = std::fs::read_to_string(d.join("db.ini"))
            && let Some(db) = text.lines().find_map(|l| l.trim().strip_prefix("database").and_then(|r| r.trim_start().strip_prefix('=')).map(|v| v.trim().to_string()))
        {
            return Some(db).filter(|d| !d.is_empty());
        }
        if let Ok(text) = std::fs::read_to_string(d.join("db.json"))
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(db) = v["database"].as_str()
        {
            return Some(db.to_string()).filter(|d| !d.is_empty());
        }
        if project.is_some_and(|p| d == p) {
            break;
        }
        dir = d.parent();
    }
    None
}

/// Fehlende Imports nach `package`/vorhandenen Imports einfuegen. Liefert (Text, eingefuegte Zeichen, Einfuegestelle).
fn ensure_imports(text: &str, imports: &[String]) -> (String, usize, usize) {
    let missing: Vec<&String> = imports.iter().filter(|i| !text.contains(&format!("import {i};"))).collect();
    if missing.is_empty() {
        return (text.to_string(), 0, 0);
    }
    let block: String = missing.iter().map(|i| format!("import {i};\n")).collect();
    // Nach dem letzten Import, sonst nach der package-Zeile, sonst am Anfang
    let mut at = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("import ") || t.starts_with("package ") {
            at = offset + line.len();
        }
        offset += line.len();
    }
    let mut out = text.to_string();
    let insert = if at > 0 && text[..at].trim_end().ends_with(';') && !text[..at].contains("import ") { format!("\n{block}") } else { block };
    out.insert_str(at, &insert);
    (out, insert.chars().count(), text[..at].chars().count())
}

/// Fehlende Zeilen (Python-Imports, require) oben einfuegen. Liefert (Text, eingefuegte Zeichen).
fn ensure_header_lines(text: &str, lines: &[String]) -> (String, usize) {
    let missing: Vec<&String> = lines.iter().filter(|l| !text.lines().any(|t| t.trim() == l.trim())).collect();
    if missing.is_empty() {
        return (text.to_string(), 0);
    }
    let block: String = missing.iter().map(|l| format!("{l}\n")).collect();
    // Nach einer Shebang- bzw. Kodierungszeile und Docstrings nicht suchen: einfach ganz oben
    let skip = if text.starts_with("#!") { text.find('\n').map_or(text.len(), |i| i + 1) } else { 0 };
    let mut out = text.to_string();
    out.insert_str(skip, &block);
    (out, block.chars().count())
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || norm(a) == norm(b)
}

impl CodeTab {
    pub fn open_file(path: &Path, default_db: &str, project: Option<&Path>) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let project = project.filter(|p| path.starts_with(p)).map(Path::to_path_buf);
        let lang = Lang::from_path(path);
        let mut editor = SqlEditor::new(egui::Id::new(("code-file", file_key(path))));
        editor.lang = lang;
        editor.breakpoints_enabled = lang == Lang::Java;
        editor.breakpoints = debug::breakpoints(path);
        let database = workspace::file_db_any(project.as_deref(), path)
            .or_else(|| lang.runnable().then(|| database_from_config(path, project.as_deref())).flatten())
            .unwrap_or_else(|| default_db.to_string());
        let mut t = Self {
            file: path.to_path_buf(),
            project,
            lang,
            database,
            saved_text: text.clone(),
            text,
            last_edit: Instant::now(),
            file_mtime: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            file_check: None,
            editor,
            save_error: None,
            pending_line: None,
            pending_cursor: None,
            history: None,
            run: RunState::Idle,
            run_next_frame: None,
            failure: None,
            show_output: false,
            libs: None,
            lsp: None,
            lsp_synced: None,
            lsp_diag_gen: 0,
            lsp_wait: None,
            hover: None,
            rename: None,
            debug_eval: String::new(),
            lsp_note: None,
        };
        t.update_database_module();
        Ok(t)
    }

    fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn save(&mut self) {
        if self.saved_text != self.text {
            workspace::snapshot(&self.file, &self.saved_text);
        }
        match workspace::atomic_write(&self.file, &self.text) {
            Ok(_) => {
                self.saved_text = self.text.clone();
                self.save_error = None;
                self.file_mtime = std::fs::metadata(&self.file).and_then(|m| m.modified()).ok();
                if let Some(s) = &self.lsp {
                    s.saved(&self.file);
                }
            }
            Err(e) => self.save_error = Some(crate::tr_format!("Speichern fehlgeschlagen: {e}", "Save failed: {e}")),
        }
    }

    fn set_database(&mut self, db: String) {
        if db != self.database {
            self.database = db;
            workspace::set_file_db_any(self.project.as_deref(), &self.file, &self.database);
        }
    }

    /// Gibt es neben der Datei ein Verbindungsmodul aus einem EasyMySQL-Projekt (Database.java, db.py, db.js)?
    fn update_database_module(&mut self) {
        let name = match self.lang {
            Lang::Java => "Database.java",
            Lang::Python => "db.py",
            Lang::JavaScript | Lang::TypeScript => "db.js",
            _ => "",
        };
        self.editor.has_database_class = !name.is_empty() && self.file.parent().is_some_and(|d| d.join(name).is_file() && d.join(name) != self.file);
    }

    fn runnable(&self) -> bool {
        self.lang.runnable()
    }

    // ------------------------------------------------------------------
    // Ausfuehren, Tests, Debuggen, .jar

    fn request(&mut self, goal: Goal, cx: &mut Ctx) {
        if !self.runnable() || !matches!(self.run, RunState::Idle) {
            return;
        }
        if matches!(goal, Goal::Debug | Goal::Jar) && self.lang != Lang::Java {
            cx.status(crate::i18n::tr("Debuggen und .jar gibt es für Java-Dateien.", "Debugging and .jar are available for Java files."));
            return;
        }
        if dev::config().save_before_run {
            // Erst alle Dateien speichern, damit Compiler und Interpreter den aktuellen Stand sehen
            self.save();
            cx.actions.push(Action::Run(Cmd::SaveAll));
        }
        self.run_next_frame = Some(goal);
    }

    fn needed(&self, goal: Goal) -> Vec<Tool> {
        let project = self.project.as_deref();
        match self.lang {
            Lang::Java => jrun::needed_tools(&self.file, &self.text, project, if goal == Goal::Test { jrun::Mode::Test } else { jrun::Mode::Run }),
            Lang::Python => python::needed_tools(&self.file, project),
            _ => node::needed_tools(&self.file, project),
        }
    }

    fn start(&mut self, goal: Goal, ctx: &egui::Context) {
        let needed = self.needed(goal);
        let tools = dev::tools();
        let missing: Vec<Tool> = needed.iter().copied().filter(|t| !tools.has(*t)).collect();
        if !missing.is_empty() {
            if dev::config().auto_download {
                install::install(&missing, ctx);
                self.run = RunState::WaitTools(needed, goal);
            } else {
                self.run = RunState::Ask(missing, goal);
            }
            return;
        }
        let live = Arc::new(Mutex::new(String::new()));
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        let (file, text, project, cfg) = (self.file.clone(), self.text.clone(), self.project.clone(), dev::config());
        let lang = self.lang;
        let live2 = Arc::clone(&live);
        std::thread::spawn(move || {
            let r = match lang {
                Lang::Java => {
                    let req = jrun::Request { file, text, project, tools, cfg, live: live2 };
                    match goal {
                        Goal::Run => jrun::prepare(&req, jrun::Mode::Run).map(Prepared::Launch),
                        Goal::Test => jrun::prepare(&req, jrun::Mode::Test).map(Prepared::Launch),
                        Goal::Jar => java::jar::build(&req).map(Prepared::Jar),
                        Goal::Debug => {
                            let port = debug::free_port();
                            jrun::prepare(&req, jrun::Mode::Debug(port)).map(|l| {
                                let build = jrun::detect_build(&req.file, source::package(&req.text).as_deref(), req.project.as_deref());
                                let root = build.root();
                                let roots = match build {
                                    jrun::Build::Plain(src) => vec![src, root.join("test")],
                                    _ => vec![root.join("src/main/java"), root.join("src/test/java")],
                                };
                                Prepared::Debug(l, port, roots)
                            })
                        }
                    }
                }
                Lang::Python => {
                    let req = python::Request { file, text, project, tools, cfg, live: live2 };
                    python::prepare(&req, if goal == Goal::Test { python::Mode::Test } else { python::Mode::Run }).map(Prepared::Launch)
                }
                _ => {
                    let req = node::Request { file, project, tools, cfg, live: live2 };
                    node::prepare(&req, if goal == Goal::Test { node::Mode::Test } else { node::Mode::Run }).map(Prepared::Launch)
                }
            };
            let _ = tx.send(r);
            repaint.request_repaint();
        });
        self.failure = None;
        self.editor.set_diagnostics(&self.text, &[]);
        self.run = RunState::Preparing(rx, live, goal);
    }

    fn env(&self, cx: &Ctx) -> Vec<(String, String)> {
        let cfg = dev::config();
        let mut env = dev::tools().shell_env();
        if cfg.pass_connection {
            let conn = cx.db.map(|d| d.info.clone()).unwrap_or_else(|| cx.settings.conn.clone());
            let database = if self.database.is_empty() { database_from_config(&self.file, self.project.as_deref()).unwrap_or_default() } else { self.database.clone() };
            env.extend(dev::db_env(&conn, &database));
        }
        env.extend(cfg.env());
        env
    }

    fn poll_run(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        if let Some(goal) = self.run_next_frame.take() {
            self.start(goal, ctx);
        }
        match &self.run {
            RunState::WaitTools(needed, goal) => {
                let st = install::status();
                if !st.busy {
                    let tools = dev::tools();
                    let goal = *goal;
                    if needed.iter().all(|t| tools.has(*t)) {
                        self.run = RunState::Idle;
                        self.start(goal, ctx);
                    } else if st.error.is_some() {
                        self.run = RunState::Idle;
                    }
                }
            }
            RunState::Preparing(rx, _, _) => {
                if let Ok(result) = rx.try_recv() {
                    self.run = RunState::Idle;
                    match result {
                        Ok(Prepared::Launch(launch)) => {
                            let env = self.env(cx);
                            cx.actions.push(Action::RunProgram { launch, env });
                        }
                        Ok(Prepared::Debug(launch, port, roots)) => {
                            let env = self.env(cx);
                            cx.actions.push(Action::RunProgram { launch, env });
                            if let Some(jdk) = dev::tools().jdk.clone() {
                                debug::start(&jdk.jdb(), port, roots, ctx);
                            }
                        }
                        Ok(Prepared::Jar(path)) => {
                            cx.actions.push(Action::FilesChanged);
                            cx.status(crate::tr_format!("Fertig: {} – starten mit java -jar {}", "Done: {} – run it with java -jar {}", path.display(), path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                            if let Some(dir) = path.parent() {
                                crate::app::open_path(dir);
                            }
                        }
                        Err(f) => {
                            let mine: Vec<(usize, Option<usize>, String, bool)> = f.diags.iter().filter(|d| same_file(&d.file, &self.file)).map(|d| (d.line, d.col, d.message.clone(), d.error)).collect();
                            self.editor.set_diagnostics(&self.text, &mine);
                            if let Some(first) = f.diags.iter().find(|d| d.error) {
                                if same_file(&first.file, &self.file) {
                                    self.pending_line = Some(first.line);
                                } else {
                                    cx.actions.push(Action::OpenFileAt { path: first.file.clone(), line: first.line });
                                }
                            }
                            self.show_output = true;
                            self.failure = Some(f);
                        }
                    }
                } else {
                    ctx.request_repaint_after(Duration::from_millis(150));
                }
            }
            RunState::Idle | RunState::Ask(..) => {}
        }
    }

    // ------------------------------------------------------------------
    // Sprachserver

    fn lsp_root(&self) -> PathBuf {
        lsp::root_for(self.lang, &self.file, self.project.as_deref())
    }

    fn poll_lsp(&mut self, ctx: &egui::Context) {
        if !self.runnable() || !lsp::enabled(self.lang) {
            if self.lsp.take().is_some() {
                self.editor.set_external(Vec::new());
            }
            return;
        }
        if self.lsp.as_ref().is_some_and(|s| s.exited()) {
            self.lsp = None;
            self.lsp_synced = None;
        }
        if self.lsp.is_none() {
            let Some(tool) = lsp::tool_for(self.lang) else { return };
            let tools = dev::tools();
            if !tools.has(tool) {
                // Einmal pro Programmstart im Hintergrund laden (wenn erlaubt)
                static ASKED: Mutex<Vec<Tool>> = Mutex::new(Vec::new());
                let mut asked = ASKED.lock().unwrap();
                if !asked.contains(&tool) && dev::config().auto_download && !install::status().busy {
                    asked.push(tool);
                    install::install(&[tool], ctx);
                }
                self.lsp_note = Some(crate::tr_format!("{} wird bei Bedarf geladen", "{} is downloaded when needed", tool.label()));
                return;
            }
            self.lsp = lsp::get(self.lang, &self.lsp_root(), ctx);
            self.lsp_synced = None;
            if self.lsp.is_none() {
                return;
            }
        }
        let Some(server) = self.lsp.clone() else { return };
        self.lsp_note = match &*server.phase.lock().unwrap() {
            lsp::Phase::Starting => Some(crate::tr_format!("{} startet …", "{} is starting …", lsp::tool_for(self.lang).map(|t| t.label()).unwrap_or(""))),
            lsp::Phase::Failed(e) => Some(e.clone()),
            lsp::Phase::Ready => {
                let p = server.progress.lock().unwrap().clone();
                (!p.is_empty()).then_some(p)
            }
        };
        if !server.ready() {
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }
        // Neuen Text melden, wenn kurz nicht getippt wurde
        let delay = Duration::from_millis(dev::config().lsp_delay_ms as u64);
        if self.lsp_synced.as_deref() != Some(self.text.as_str()) {
            if self.last_edit.elapsed() >= delay || self.lsp_synced.is_none() {
                server.sync(&self.file, self.lang, &self.text);
                self.lsp_synced = Some(self.text.clone());
            } else {
                ctx.request_repaint_after(delay);
            }
        }
        // Meldungen uebernehmen, solange der Text zum gemeldeten Stand passt
        if let Some((n, list)) = server.diagnostics(&self.file)
            && n != self.lsp_diag_gen
            && self.lsp_synced.as_deref() == Some(self.text.as_str())
        {
            self.lsp_diag_gen = n;
            let marks = list.iter().map(|d| (lsp::from_pos(&self.text, d.start), lsp::from_pos(&self.text, d.end), d.message.clone(), d.error)).collect();
            self.editor.set_external(marks);
        }
    }

    fn goto_definition(&mut self, index: usize, cx: &mut Ctx) {
        match self.lsp.clone().filter(|s| s.ready()) {
            Some(s) => {
                self.flush_lsp();
                self.lsp_wait = Some(LspWait::Definition(s.definition(&self.file, &self.text, index)));
            }
            None => cx.status(self.lsp_missing()),
        }
    }

    fn lsp_missing(&self) -> String {
        if !lsp::enabled(self.lang) {
            crate::i18n::tr("Der Sprachserver ist in den Einstellungen ausgeschaltet.", "The language server is turned off in the settings.").into()
        } else {
            self.lsp_note.clone().unwrap_or_else(|| crate::i18n::tr("Der Sprachserver ist noch nicht bereit.", "The language server is not ready yet.").into())
        }
    }

    /// Aktuellen Text sofort melden (vor Anfragen)
    fn flush_lsp(&mut self) {
        if let Some(s) = &self.lsp
            && self.lsp_synced.as_deref() != Some(self.text.as_str())
        {
            s.sync(&self.file, self.lang, &self.text);
            self.lsp_synced = Some(self.text.clone());
        }
    }

    fn poll_lsp_wait(&mut self, cx: &mut Ctx) {
        let Some(wait) = &self.lsp_wait else { return };
        let rx = match wait {
            LspWait::Definition(rx) | LspWait::Rename(rx) | LspWait::Format(rx, _) => rx,
        };
        let Ok(result) = rx.try_recv() else { return };
        let wait = self.lsp_wait.take().unwrap();
        match (wait, result) {
            (_, Err(e)) => cx.error(e),
            (LspWait::Definition(_), Ok(v)) => match lsp::parse_location(&v) {
                Some((path, pos)) if same_file(&path, &self.file) => self.pending_cursor = Some(lsp::from_pos(&self.text, pos)),
                Some((path, pos)) => cx.actions.push(Action::OpenFileAt { path, line: pos.0 }),
                None if v.is_null() || v.as_array().is_some_and(|a| a.is_empty()) => cx.status(crate::i18n::tr("Keine Definition gefunden.", "No definition found.")),
                None => cx.status(crate::i18n::tr("Die Definition liegt in einer Bibliothek.", "The definition is in a library.")),
            },
            (LspWait::Rename(_), Ok(v)) => {
                let edit = lsp::parse_workspace_edit(&v);
                if edit.files.is_empty() && edit.renames.is_empty() {
                    cx.status(crate::i18n::tr("Nichts umzubenennen.", "Nothing to rename."));
                } else {
                    cx.actions.push(Action::ApplyEdit(edit));
                }
            }
            (LspWait::Format(_, before), Ok(v)) => {
                if self.text == before && let Some(list) = v.as_array() {
                    let edits: Vec<_> = list.iter().map(|e| (pos(&e["range"]["start"]), pos(&e["range"]["end"]), e["newText"].as_str().unwrap_or("").to_string())).collect();
                    if !edits.is_empty() {
                        self.text = lsp::apply_edits(&self.text, &edits);
                        self.last_edit = Instant::now();
                    }
                }
            }
        }
    }

    fn start_rename(&mut self, cx: &mut Ctx) {
        if self.lsp.as_ref().is_none_or(|s| !s.ready()) {
            cx.status(self.lsp_missing());
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut a = self.editor.cursor.min(chars.len());
        let mut b = a;
        while a > 0 && sqledit::is_word_char(chars[a - 1]) {
            a -= 1;
        }
        while b < chars.len() && sqledit::is_word_char(chars[b]) {
            b += 1;
        }
        if a == b {
            cx.status(crate::i18n::tr("Den Cursor auf einen Namen setzen.", "Put the cursor on a name."));
            return;
        }
        self.rename = Some((chars[a..b].iter().collect(), a, true));
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some((name, at, first)) = self.rename.as_mut() else { return };
        let mut go = false;
        let mut cancel = false;
        egui::Modal::new(self.editor.id.with("rename")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(crate::i18n::tr("Umbenennen", "Rename")).strong());
            ui.label(RichText::new(crate::i18n::tr("Ändert den Namen überall im Projekt.", "Changes the name everywhere in the project.")).small().color(style::pal().text_weak));
            ui.add_space(6.0);
            let r = ui.add(crate::widgets::edit(name).desired_width(f32::INFINITY));
            if *first {
                r.request_focus();
                *first = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if crate::widgets::primary(ui, crate::i18n::tr("Umbenennen", "Rename")).clicked() {
                    go = true;
                }
                if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if go {
            let (name, at) = (name.trim().to_string(), *at);
            self.rename = None;
            self.flush_lsp();
            if let Some(s) = &self.lsp
                && !name.is_empty()
            {
                self.lsp_wait = Some(LspWait::Rename(s.rename(&self.file, &self.text, at, &name)));
            }
        } else if cancel {
            self.rename = None;
        }
    }

    /// Hinweis des Sprachservers fuer die Stelle unter dem Mauszeiger
    fn poll_hover(&mut self, ui: &egui::Ui, index: Option<usize>) {
        let Some(server) = self.lsp.clone().filter(|s| s.ready()) else { return };
        match (index, &mut self.hover) {
            (None, _) => self.hover = None,
            (Some(i), Some((at, since, rx, text))) if *at == i => {
                if rx.is_none() && text.is_none() && since.elapsed() > Duration::from_millis(600) && self.lsp_synced.as_deref() == Some(self.text.as_str()) {
                    *rx = Some(server.hover(&self.file, &self.text, i));
                }
                if let Some(r) = rx
                    && let Ok(res) = r.try_recv()
                {
                    *text = Some(res.ok().and_then(|v| lsp::hover_text(&v)).unwrap_or_default());
                    *rx = None;
                }
                if let Some(t) = text.as_ref().filter(|t| !t.is_empty()) {
                    let t: String = t.chars().take(1200).collect();
                    egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), self.editor.id.with("hover"), egui::PopupAnchor::Pointer).show(|ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(t).monospace().small());
                    });
                } else if rx.is_some() || text.is_none() {
                    ui.ctx().request_repaint_after(Duration::from_millis(150));
                }
            }
            (Some(i), _) => self.hover = Some((i, Instant::now(), None, None)),
        }
    }

    // ------------------------------------------------------------------
    // Einfuegen

    /// Text an der Cursorstelle einfuegen (mehrzeilig mit der Einrueckung der Zeile)
    fn insert_at_cursor(&mut self, snippet: &str, imports: &[String]) {
        let mut cursor = self.editor.cursor;
        if self.lang == Lang::Java {
            let (text, added, at) = ensure_imports(&self.text, imports);
            if cursor >= at && added > 0 {
                cursor += added;
            }
            self.text = text;
        } else if !imports.is_empty() {
            let (text, added) = ensure_header_lines(&self.text, imports);
            cursor += added;
            self.text = text;
        }
        let b = self.text.char_indices().nth(cursor).map(|(b, _)| b).unwrap_or(self.text.len());
        let line_start = self.text[..b].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = self.text[line_start..b].chars().take_while(|c| *c == ' ').collect();
        let body = snippet.trim_end().replace('\n', &format!("\n{indent}"));
        self.text.insert_str(b, &body);
        self.last_edit = Instant::now();
    }

    /// Record und DAO-Klasse einer Tabelle neben dieser Datei anlegen
    fn create_table_classes(&mut self, table: &crate::db::TableInfo, cx: &mut Ctx) {
        let Some(dir) = self.file.parent() else { return };
        let package = source::package(&self.text);
        let class = crate::javagen::class_name(&table.name);
        let files = [(format!("{class}.java"), crate::javagen::record_file(table, package.as_deref())), (format!("{class}Dao.java"), crate::javagen::dao_file(table, package.as_deref()))];
        let existing: Vec<&str> = files.iter().filter(|(n, _)| dir.join(n).exists()).map(|(n, _)| n.as_str()).collect();
        if !existing.is_empty() {
            cx.error(crate::tr_format!("Gibt es schon: {}", "Already exists: {}", existing.join(", ")));
            return;
        }
        for (name, content) in &files {
            if let Err(e) = workspace::atomic_write(&dir.join(name), content) {
                cx.error(e.to_string());
                return;
            }
        }
        cx.actions.push(Action::FilesChanged);
        cx.status(crate::tr_format!("{class}.java und {class}Dao.java angelegt.", "Created {class}.java and {class}Dao.java."));
    }

    /// Testdatei fuer diese Datei anlegen (und oeffnen)
    fn create_test(&mut self, cx: &mut Ctx) {
        let stem = self.file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (path, content) = match self.lang {
            Lang::Java => {
                let package = source::package(&self.text);
                let build = jrun::detect_build(&self.file, package.as_deref(), self.project.as_deref());
                let base = match &build {
                    jrun::Build::Plain(_) => build.root().join("test"),
                    _ => build.root().join("src/test/java"),
                };
                let dir = package.as_ref().map(|p| base.join(p.replace('.', "/"))).unwrap_or(base);
                let class = format!("{stem}Test");
                let head = package.map(|p| format!("package {p};\n\n")).unwrap_or_default();
                let body = format!(
                    "{head}import static org.junit.jupiter.api.Assertions.*;\n\nimport org.junit.jupiter.api.Test;\n\nclass {class} {{\n    @Test\n    void {}() {{\n        assertTrue(true);\n    }}\n}}\n",
                    if crate::i18n::is_english() { "works" } else { "funktioniert" }
                );
                if let Err(e) = ensure_junit(&build) {
                    cx.error(e);
                }
                (dir.join(format!("{class}.java")), body)
            }
            Lang::Python => {
                let root = python::project_root(&self.file, self.project.as_deref()).unwrap_or_else(|| self.file.parent().map(Path::to_path_buf).unwrap_or_default());
                let module = stem.clone();
                (
                    root.join("tests").join(format!("test_{module}.py")),
                    format!("import os\nimport sys\nimport unittest\n\nsys.path.insert(0, os.path.join(os.path.dirname(__file__), \"..\"))\n\nimport {module}  # noqa: E402\n\n\nclass {}Test(unittest.TestCase):\n    def test_works(self):\n        self.assertTrue(True)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n", dev::names::pascal(&module)),
                )
            }
            _ => {
                let root = node::project_root(&self.file, self.project.as_deref()).unwrap_or_else(|| self.file.parent().map(Path::to_path_buf).unwrap_or_default());
                let ext = self.file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "js".into());
                let rel = self.file.strip_prefix(&root).map(|r| r.with_extension("")).unwrap_or_else(|_| PathBuf::from(&stem));
                let import = format!("../{}", rel.to_string_lossy().replace('\\', "/"));
                (
                    root.join("test").join(format!("{stem}.test.{ext}")),
                    format!("const test = require('node:test');\nconst assert = require('node:assert/strict');\nconst mod = require('{import}');\n\ntest('{stem}', () => {{\n  assert.ok(mod);\n}});\n"),
                )
            }
        };
        if path.exists() {
            cx.actions.push(Action::OpenFileAt { path, line: 0 });
            return;
        }
        match workspace::atomic_write(&path, &content) {
            Ok(_) => {
                cx.actions.push(Action::FilesChanged);
                cx.actions.push(Action::OpenFileAt { path, line: 0 });
            }
            Err(e) => cx.error(e.to_string()),
        }
    }

    // ------------------------------------------------------------------
    // Oberflaeche

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        ui.horizontal(|ui| {
            if self.runnable() {
                let mut dbs = vec![String::new()];
                let show_system = super::SHOW_SYSTEM_DBS.load(Ordering::Relaxed);
                dbs.extend(cx.databases.iter().filter(|d| show_system || !super::is_system_db(d) || **d == self.database).cloned());
                let mut d = self.database.clone();
                if super::str_combo(ui, self.editor.id.with("db"), &dbs, &mut d, 150.0) {
                    self.set_database(d);
                }
                let busy = !matches!(self.run, RunState::Idle);
                ui.add_enabled_ui(!busy, |ui| {
                    if icons::run_button(ui, crate::i18n::text("Ausführen"))
                        .on_hover_text(crate::tr_format!("Starten im Terminal ({})", "Start in the terminal ({})", crate::keymap::text(Cmd::RunAll)))
                        .clicked()
                    {
                        self.request(Goal::Run, cx);
                    }
                    if self.lang == Lang::Java && icons::button(ui, Icon::Bug, &format!("{} ({})", crate::i18n::tr("Debuggen", "Debug"), crate::keymap::text(Cmd::DebugStart))).clicked() {
                        self.request(Goal::Debug, cx);
                    }
                    if icons::button(ui, Icon::Flask, &format!("{} ({})", crate::i18n::tr("Tests ausführen", "Run tests"), crate::keymap::text(Cmd::RunTests))).clicked() {
                        self.request(Goal::Test, cx);
                    }
                });
                self.insert_menu(ui, cx);
            }
            let tip = format!("{} ({})", crate::i18n::text("Speichern"), crate::keymap::text(Cmd::Save));
            if ui.add_enabled_ui(self.dirty(), |ui| icons::button(ui, Icon::Save, &tip)).inner.clicked() {
                self.save();
            }
            self.status(ui, cx);
            let more = icons::button(ui, Icon::More, crate::i18n::text("Weitere"));
            egui::Popup::menu(&more).show(|ui| {
                crate::style::menu_item_look(ui);
                if self.runnable() {
                    if self.lang == Lang::Java && ui.button(Cmd::BuildJar.label()).on_hover_text(crate::i18n::tr("dist/<Projekt>.jar mit allen Bibliotheken – läuft mit java -jar", "dist/<project>.jar with all libraries – runs with java -jar")).clicked() {
                        self.request(Goal::Jar, cx);
                        ui.close();
                    }
                    if ui.button(Cmd::AddLibrary.label()).clicked() {
                        self.open_libs();
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Test anlegen", "Create test")).clicked() {
                        self.create_test(cx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(format!("{}   {}", Cmd::GotoDefinition.label(), crate::keymap::text(Cmd::GotoDefinition))).clicked() {
                        self.goto_definition(self.editor.cursor, cx);
                        ui.close();
                    }
                    if ui.button(format!("{}   {}", Cmd::RenameSymbol.label(), crate::keymap::text(Cmd::RenameSymbol))).clicked() {
                        self.start_rename(cx);
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Dokument formatieren", "Format document")).clicked() {
                        self.format(cx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(crate::i18n::tr("Einstellungen zum Ausführen …", "Run settings …")).clicked() {
                        cx.actions.push(Action::OpenSettings(Some(match self.lang {
                            Lang::Java => "java".into(),
                            Lang::Python => "python".into(),
                            _ => "javascript".into(),
                        })));
                        ui.close();
                    }
                }
                if ui.button(crate::i18n::text("Frühere Fassungen …")).clicked() {
                    self.history = Some(workspace::history(&self.file));
                    ui.close();
                }
                if ui.button(crate::i18n::text("Pfad kopieren")).clicked() {
                    ui.ctx().copy_text(self.file.display().to_string());
                    ui.close();
                }
            });
            if let Some(e) = &self.save_error {
                ui.label(RichText::new(e).color(pal.error_text));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (l, c) = self.editor.line_col;
                ui.label(RichText::new(crate::tr_format!("Z. {l}, Sp. {c}", "Ln {l}, Col {c}")).small().color(pal.text_weak));
                ui.label(RichText::new(self.lang.label()).small().color(pal.text_weak));
                if let Some(note) = &self.lsp_note {
                    ui.add(egui::Label::new(RichText::new(note).small().color(pal.text_weak)).truncate());
                }
            });
        });
    }

    fn format(&mut self, cx: &mut Ctx) {
        match self.lsp.clone().filter(|s| s.ready()) {
            Some(s) => {
                self.flush_lsp();
                let indent = match sqledit::prefs().indent {
                    0 => self.lang.indent().len(),
                    n => n,
                };
                self.lsp_wait = Some(LspWait::Format(s.format(&self.file, indent), self.text.clone()));
            }
            None => cx.status(self.lsp_missing()),
        }
    }

    /// Anzeige rechts neben den Knoepfen: Laden, Uebersetzen, Fehler, Hinweise zum SQL
    fn status(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        match &self.run {
            RunState::Ask(missing, goal) => {
                let names = missing.iter().map(|t| t.label()).collect::<Vec<_>>().join(", ");
                ui.label(RichText::new(crate::tr_format!("Es fehlt: {names}", "Missing: {names}")).small().color(pal.warn_text));
                let goal = *goal;
                if ui.small_button(crate::i18n::tr("Herunterladen", "Download")).clicked() {
                    install::install(missing, ui.ctx());
                    self.run = RunState::WaitTools(self.needed(goal), goal);
                }
                if ui.small_button(crate::i18n::text("Abbrechen")).clicked() {
                    self.run = RunState::Idle;
                }
                return;
            }
            RunState::WaitTools(..) => {
                let st = install::status();
                ui.spinner();
                ui.label(RichText::new(install::status_line(&st)).small().color(pal.text_weak));
                if let Some(f) = st.fraction {
                    ui.add(egui::ProgressBar::new(f).desired_width(120.0).desired_height(6.0).corner_radius(3));
                }
                if ui.small_button(crate::i18n::text("Abbrechen")).clicked() {
                    install::cancel();
                    self.run = RunState::Idle;
                }
                return;
            }
            RunState::Preparing(_, live, goal) => {
                ui.spinner();
                let line = live.lock().unwrap().clone();
                let text = if !line.is_empty() {
                    line
                } else {
                    match (goal, self.lang) {
                        (Goal::Jar, _) => crate::i18n::tr(".jar wird gebaut …", "Building the .jar …").to_string(),
                        (_, Lang::Java) => crate::i18n::tr("Wird übersetzt …", "Compiling …").to_string(),
                        _ => crate::i18n::tr("Wird vorbereitet …", "Preparing …").to_string(),
                    }
                };
                ui.add(egui::Label::new(RichText::new(text).small().color(pal.text_weak)).truncate());
                return;
            }
            RunState::Idle => {}
        }
        if let Some(e) = install::status().error.filter(|_| self.runnable()) {
            ui.label(RichText::new(crate::tr_format!("Download fehlgeschlagen: {e}", "Download failed: {e}")).small().color(pal.error_text));
            if ui.small_button(crate::i18n::tr("Erneut versuchen", "Try again")).clicked() {
                self.request(Goal::Run, cx);
            }
            return;
        }
        if let Some(f) = &self.failure {
            let errors = f.diags.iter().filter(|d| d.error).count();
            let label = if errors > 0 && self.lang == Lang::Java { crate::tr_format!("{errors} Fehler beim Übersetzen", "{errors} compile error(s)") } else if errors > 0 { crate::tr_format!("{errors} Syntaxfehler", "{errors} syntax error(s)") } else { f.message.clone() };
            if ui.add(egui::Button::new(RichText::new(label).small().color(pal.error_text)).frame(false)).on_hover_text(crate::i18n::tr("Ausgabe zeigen", "Show output")).clicked() {
                self.show_output = !self.show_output;
            }
            return;
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
            if fixable > 0 && icons::text_button(ui, Icon::Sparkles, crate::i18n::text("Korrigieren")).on_hover_text(crate::keymap::text(Cmd::FixAll)).clicked() {
                self.fix_all(cx);
            }
        }
    }

    fn has_db_module(&self) -> bool {
        self.editor.has_database_class
    }

    fn insert_menu(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let schema = if self.database.is_empty() { None } else { cx.schema(&self.database) };
        let resp = ui.button(crate::i18n::tr("Einfügen", "Insert")).on_hover_text(crate::i18n::tr("Code für die Datenbank einfügen", "Insert code for the database"));
        let mut insert: Option<(String, Vec<String>)> = None;
        let mut classes: Option<crate::db::TableInfo> = None;
        let cfg = dev::config();
        let has_db = self.has_db_module();
        let lang = self.lang;
        egui::Popup::menu(&resp).show(|ui| {
            crate::style::menu_item_look(ui);
            let connect: (String, Vec<String>) = match lang {
                Lang::Java if has_db => ("try (Connection con = Database.connect()) {\n    \n}".into(), vec!["java.sql.*".into()]),
                Lang::Java => (
                    "try (Connection con = DriverManager.getConnection(\n        System.getenv().getOrDefault(\"EASYMYSQL_DB_URL\", \"URL\"),\n        System.getenv().getOrDefault(\"EASYMYSQL_DB_USER\", \"root\"),\n        System.getenv().getOrDefault(\"EASYMYSQL_DB_PASSWORD\", \"\"))) {\n    \n}"
                        .replace("\"URL\"", &format!("\"{}\"", java::jdbc_url(java::driver(), &cx.settings.conn.host, cx.settings.conn.port, &self.database))),
                    vec!["java.sql.*".into()],
                ),
                Lang::Python => python::project::snippet_connect(cfg.python_driver, has_db),
                _ if has_db => ("const db = require('./db');".into(), vec![]),
                _ => {
                    let (t, _) = node::project::template("database", cfg.node_driver, false);
                    let pool: String = t.lines().take_while(|l| !l.starts_with("async function")).collect::<Vec<_>>().join("\n");
                    (pool.trim().to_string(), vec![])
                }
            };
            if ui.button(crate::i18n::tr("Verbindung öffnen", "Open connection")).clicked() {
                insert = Some(connect);
                ui.close();
            }
            match &schema {
                Some(s) if !s.tables.is_empty() => {
                    ui.menu_button(crate::i18n::tr("Alle Zeilen lesen", "Read all rows"), |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for t in &s.tables {
                                if ui.button(&t.name).clicked() {
                                    insert = Some(match lang {
                                        Lang::Java => (crate::javagen::select_snippet(t), crate::javagen::select_imports(t)),
                                        Lang::Python => (python::project::snippet_select(t, has_db, cfg.python_driver), vec![]),
                                        _ => (node::project::snippet_select(t, has_db, cfg.node_driver), vec![]),
                                    });
                                    ui.close();
                                }
                            }
                        });
                    });
                    let label = match lang {
                        Lang::Java => crate::i18n::tr("Record und DAO-Klasse anlegen", "Create record and DAO class"),
                        Lang::Python => crate::i18n::tr("dataclass und Repository einfügen", "Insert dataclass and repository"),
                        _ => crate::i18n::tr("Repository-Klasse einfügen", "Insert repository class"),
                    };
                    ui.menu_button(label, |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for t in &s.tables {
                                let b = ui.button(&t.name);
                                let b = if lang == Lang::Java { b.on_hover_text(crate::tr_format!("{0}.java und {0}Dao.java in diesem Ordner", "{0}.java and {0}Dao.java in this folder", crate::javagen::class_name(&t.name))) } else { b };
                                if b.clicked() {
                                    match lang {
                                        Lang::Java => classes = Some(t.clone()),
                                        Lang::Python => {
                                            let code = format!("{}\n\n\n{}", python::project::model(t), python::project::repo(t).replace("db.cursor(self.con)", if has_db { "db.cursor(self.con)" } else { "self.con.cursor()" }));
                                            let mut imports = vec!["import datetime".to_string(), "from dataclasses import dataclass".into(), "from decimal import Decimal".into(), "from typing import Optional".into()];
                                            if has_db {
                                                imports.push("import db".into());
                                            }
                                            insert = Some((code, imports));
                                        }
                                        _ => insert = Some((node::project::repo(t), if has_db { vec!["const db = require('./db');".into()] } else { vec![] })),
                                    }
                                    ui.close();
                                }
                            }
                        });
                    });
                }
                _ => {
                    ui.label(RichText::new(crate::i18n::tr("Oben eine Datenbank wählen, um Code für ihre Tabellen einzufügen.", "Choose a database above to insert code for its tables.")).small().color(style::pal().text_weak));
                }
            }
        });
        if let Some((code, imports)) = insert {
            self.insert_at_cursor(&code, &imports);
        }
        if let Some(t) = classes {
            self.create_table_classes(&t, cx);
        }
    }

    fn fix_all(&mut self, cx: &mut Ctx) {
        let n = self.editor.fix_all(&mut self.text);
        if n > 0 {
            self.last_edit = Instant::now();
            cx.status(crate::tr_format!("{n} Stelle(n) korrigiert.", "{n} place(s) corrected."));
        } else {
            cx.status(crate::i18n::text("Nichts zu korrigieren."));
        }
    }

    // ------------------------------------------------------------------
    // Bibliotheken

    fn open_libs(&mut self) {
        let Some(eco) = libs::Ecosystem::for_lang(self.lang) else { return };
        let Some(target) = libs::target_for(self.lang, &self.file, self.project.as_deref()) else { return };
        self.libs = Some(LibDialog { query: String::new(), eco, target, search: None, results: None, adding: None, focus: true });
    }

    fn libs_dialog(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(d) = self.libs.as_mut() else { return };
        if let Some(rx) = &d.search
            && let Ok(r) = rx.try_recv()
        {
            d.results = Some(r);
            d.search = None;
        }
        let mut done: Option<Result<String, String>> = None;
        if let Some((rx, _)) = &d.adding
            && let Ok(r) = rx.try_recv()
        {
            done = Some(r);
        }
        if let Some(r) = done {
            d.adding = None;
            match r {
                Ok(msg) => {
                    cx.status(msg);
                    cx.actions.push(Action::FilesChanged);
                    dev::refresh();
                    self.libs = None;
                    return;
                }
                Err(e) => d.results = Some(Err(e)),
            }
        }
        if d.search.is_some() || d.adding.is_some() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        let pal = style::pal();
        let mut close = false;
        let mut start_search = false;
        let mut add: Option<libs::Hit> = None;
        egui::Modal::new(self.editor.id.with("libs")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, Icon::Package, pal.accent);
                ui.label(RichText::new(crate::tr_format!("Bibliothek einbinden ({})", "Add library ({})", d.eco.label())).strong().size(15.0));
            });
            ui.label(RichText::new(d.target.describe()).small().color(pal.text_weak));
            ui.add_space(8.0);
            let hint = match d.eco {
                libs::Ecosystem::Maven => crate::i18n::tr("z. B. gson oder com.google.code.gson:gson", "e.g. gson or com.google.code.gson:gson"),
                libs::Ecosystem::Pypi => crate::i18n::tr("genauer Paketname, z. B. requests", "exact package name, e.g. requests"),
                libs::Ecosystem::Npm => crate::i18n::tr("z. B. dayjs", "e.g. dayjs"),
            };
            ui.horizontal(|ui| {
                let r = ui.add(crate::widgets::edit(&mut d.query).hint_text(hint).desired_width(420.0));
                if d.focus {
                    r.request_focus();
                    d.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    start_search = true;
                }
                if ui.add_enabled(d.search.is_none(), egui::Button::new(crate::i18n::tr("Suchen", "Search"))).clicked() {
                    start_search = true;
                }
            });
            ui.add_space(6.0);
            if d.search.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(crate::i18n::tr("Suche …", "Searching …")).small().color(pal.text_weak));
                });
            }
            if let Some((_, live)) = &d.adding {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let t = live.lock().unwrap().clone();
                    ui.add(egui::Label::new(RichText::new(if t.is_empty() { crate::i18n::tr("Wird eingebunden …", "Adding …").to_string() } else { t }).small().color(pal.text_weak)).truncate());
                });
            }
            match &d.results {
                Some(Ok(hits)) if hits.is_empty() => {
                    ui.label(RichText::new(crate::i18n::tr("Nichts gefunden.", "Nothing found.")).color(pal.text_weak));
                }
                Some(Ok(hits)) => {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for h in hits {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&h.id).monospace().strong());
                                    let desc = if h.description.is_empty() { h.version.clone() } else { format!("{} · {}", h.version, h.description) };
                                    ui.add(egui::Label::new(RichText::new(desc).small().color(pal.text_weak)).truncate());
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add_enabled(d.adding.is_none(), egui::Button::new(crate::i18n::tr("Einbinden", "Add"))).clicked() {
                                        add = Some(h.clone());
                                    }
                                });
                            });
                            ui.separator();
                        }
                    });
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(RichText::new(e).small().color(pal.error_text)).wrap());
                }
                None => {}
            }
            ui.add_space(6.0);
            if ui.button(crate::i18n::text("Schließen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
        if start_search && !d.query.trim().is_empty() {
            let (tx, rx) = channel();
            let (eco, q, repaint) = (d.eco, d.query.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(libs::search(eco, &q));
                repaint.request_repaint();
            });
            d.search = Some(rx);
            d.results = None;
        }
        if let Some(hit) = add {
            let (tx, rx) = channel();
            let live = Arc::new(Mutex::new(String::new()));
            let (target, live2, repaint) = (d.target.clone(), Arc::clone(&live), ctx.clone());
            // pip/npm brauchen ihre Laufzeit
            let needs = match &d.target {
                libs::Target::PythonShared => Some(Tool::Python),
                libs::Target::Package(_) | libs::Target::NodeShared => Some(Tool::Node),
                _ => None,
            };
            std::thread::spawn(move || {
                if let Some(t) = needs
                    && !dev::tools().has(t)
                    && let Err(e) = install::install_now(t)
                {
                    let _ = tx.send(Err(e));
                    return;
                }
                let _ = tx.send(libs::add(&target, &hit, &live2));
                repaint.request_repaint();
            });
            d.adding = Some((rx, live));
        }
        if close && d.adding.is_none() {
            self.libs = None;
        }
    }

    // ------------------------------------------------------------------
    // Debugger

    fn debug_command(&mut self, cmd: Cmd, cx: &mut Ctx) -> bool {
        let Some(s) = debug::session().filter(|_| debug::active()) else {
            if cmd == Cmd::DebugStart || cmd == Cmd::DebugContinue {
                self.request(Goal::Debug, cx);
                return true;
            }
            return cmd == Cmd::ToggleBreakpoint && {
                self.toggle_breakpoint(self.editor.line_col.0.saturating_sub(1));
                true
            };
        };
        match cmd {
            Cmd::DebugContinue | Cmd::DebugStart => s.resume(),
            Cmd::StepOver => s.step_over(),
            Cmd::StepInto => s.step_into(),
            Cmd::StepOut => s.step_out(),
            Cmd::DebugStop => debug::stop(),
            Cmd::ToggleBreakpoint => self.toggle_breakpoint(self.editor.line_col.0.saturating_sub(1)),
            _ => return false,
        }
        true
    }

    fn toggle_breakpoint(&mut self, line: usize) {
        if self.lang != Lang::Java {
            return;
        }
        let class = debug::class_for(&self.file, &self.text);
        debug::toggle_breakpoint(&self.file, line, class.as_deref());
        self.editor.breakpoints = debug::breakpoints(&self.file);
    }

    fn debug_panel(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if self.lang != Lang::Java {
            return;
        }
        let Some(session) = debug::session() else {
            self.editor.exec_line = None;
            return;
        };
        let st = session.state.lock().unwrap().clone();
        self.editor.exec_line = match &st.status {
            debug::Status::Paused { file: Some(f), line, .. } if same_file(f, &self.file) => Some(*line),
            _ => None,
        };
        // Beim Anhalten zur Stelle springen (auch in eine andere Datei), aber nur einmal je Stelle
        if let debug::Status::Paused { file: Some(f), line, .. } = &st.status {
            let mut last = LAST_EXEC.lock().unwrap();
            if last.as_ref() != Some(&(f.clone(), *line)) {
                *last = Some((f.clone(), *line));
                if same_file(f, &self.file) {
                    self.pending_line = Some(*line);
                } else {
                    cx.actions.push(Action::OpenFileAt { path: f.clone(), line: *line });
                }
            }
        }
        // Nur in der Datei zeigen, in der angehalten wurde (oder in der geoeffneten, solange es laeuft)
        let paused_elsewhere = matches!(&st.status, debug::Status::Paused { file: Some(f), .. } if !same_file(f, &self.file));
        if paused_elsewhere || (st.status == debug::Status::Ended && st.error.is_none() && session.uptime() > Duration::from_secs(4)) {
            return;
        }
        let pal = style::pal();
        let total = ui.available_height();
        egui::Panel::bottom(self.editor.id.with("debug")).resizable(true).default_size((total * 0.32).max(150.0)).min_size(100.0).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 0 })).show(ui, |ui| {
            egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(style::RADIUS + 1).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    let (label, color) = match &st.status {
                        debug::Status::Starting => (crate::i18n::tr("Debugger verbindet …", "Debugger connecting …").to_string(), pal.text_weak),
                        debug::Status::Running => (crate::i18n::tr("Läuft …", "Running …").to_string(), pal.ok_text),
                        debug::Status::Paused { method, line, .. } => (crate::tr_format!("Angehalten in {method}, Zeile {}", "Paused in {method}, line {}", line + 1), pal.warn_text),
                        debug::Status::Ended => (crate::i18n::tr("Beendet", "Ended").to_string(), pal.text_weak),
                    };
                    ui.label(RichText::new(label).strong().color(color));
                    let paused = matches!(st.status, debug::Status::Paused { .. });
                    let running = st.status != debug::Status::Ended;
                    ui.add_space(8.0);
                    let btn = |ui: &mut egui::Ui, icon: Icon, cmd: Cmd, on: bool| ui.add_enabled_ui(on, |ui| icons::button(ui, icon, &format!("{} ({})", cmd.label(), crate::keymap::text(cmd)))).inner.clicked();
                    if btn(ui, Icon::Play, Cmd::DebugContinue, paused) {
                        session.resume();
                    }
                    if btn(ui, Icon::StepOver, Cmd::StepOver, paused) {
                        session.step_over();
                    }
                    if btn(ui, Icon::StepInto, Cmd::StepInto, paused) {
                        session.step_into();
                    }
                    if btn(ui, Icon::StepOut, Cmd::StepOut, paused) {
                        session.step_out();
                    }
                    if btn(ui, Icon::Stop, Cmd::DebugStop, running) {
                        debug::stop();
                    }
                    if !running && icons::button(ui, Icon::Close, crate::i18n::text("Schließen")).clicked() {
                        debug::stop();
                        *debug_closed() = true;
                    }
                });
                if let Some(e) = &st.error {
                    ui.label(RichText::new(e).color(pal.error_text));
                }
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new(crate::i18n::tr("Variablen", "Variables")).small().strong());
                    egui::ScrollArea::vertical().id_salt(self.editor.id.with("dbg-vars")).max_height(cols[0].available_height() - 30.0).auto_shrink([false, false]).show(&mut cols[0], |ui| {
                        for (name, value) in &st.locals {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).monospace().color(pal.syn_ident));
                                ui.label(RichText::new("=").monospace().color(pal.text_weak));
                                let r = ui.add(egui::Label::new(RichText::new(value).monospace()).truncate().sense(egui::Sense::click()));
                                if value.starts_with("instance of") && r.on_hover_text(crate::i18n::tr("Klicken: Felder zeigen", "Click: show fields")).clicked() {
                                    session.dump(name);
                                }
                            });
                        }
                    });
                    cols[1].label(RichText::new(crate::i18n::tr("Aufrufe", "Call stack")).small().strong());
                    egui::ScrollArea::vertical().id_salt(self.editor.id.with("dbg-stack")).max_height(cols[1].available_height() * 0.45).auto_shrink([false, false]).show(&mut cols[1], |ui| {
                        for f in &st.stack {
                            let label = match f.line {
                                Some(l) => format!("{}  :{}", f.method, l + 1),
                                None => f.method.clone(),
                            };
                            let r = ui.add(egui::Button::new(RichText::new(label).monospace().small()).frame(false));
                            if r.clicked()
                                && let (Some(file), Some(line)) = (&f.file, f.line)
                            {
                                if same_file(file, &self.file) {
                                    self.pending_line = Some(line);
                                } else {
                                    cx.actions.push(Action::OpenFileAt { path: file.clone(), line });
                                }
                            }
                        }
                    });
                    cols[1].horizontal(|ui| {
                        let r = ui.add(crate::widgets::edit(&mut self.debug_eval).hint_text(crate::i18n::tr("Ausdruck, z. B. x * 2", "Expression, e.g. x * 2")).desired_width(ui.available_width() - 70.0));
                        if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.button(crate::i18n::tr("Werten", "Eval")).clicked()) && !self.debug_eval.trim().is_empty() {
                            session.eval(&self.debug_eval);
                        }
                    });
                    egui::ScrollArea::vertical().id_salt(self.editor.id.with("dbg-console")).stick_to_bottom(true).auto_shrink([false, false]).show(&mut cols[1], |ui| {
                        let start = st.console.len().saturating_sub(200);
                        for l in &st.console[start..] {
                            ui.label(RichText::new(l).monospace().small().color(if l.starts_with("> ") { pal.text_weak } else { pal.text }));
                        }
                    });
                });
            });
        });
        if st.status != debug::Status::Ended {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }

    /// Ausgabe nach fehlgeschlagenem Vorbereiten: Fehlerliste zum Anklicken und die letzten Zeilen
    fn output_panel(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let Some(f) = self.failure.clone() else { return };
        if !self.show_output {
            return;
        }
        let pal = style::pal();
        let total = ui.available_height();
        egui::Panel::bottom(self.editor.id.with("output")).resizable(true).default_size((total * 0.3).max(120.0)).min_size(80.0).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 0 })).show(ui, |ui| {
            egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(style::RADIUS + 1).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&f.message).strong().color(pal.error_text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, crate::i18n::text("Schließen")).clicked() {
                            self.show_output = false;
                        }
                    });
                });
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for d in &f.diags {
                        let name = d.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let color = if d.error { pal.error_text } else { pal.warn_text };
                        let text = RichText::new(format!("{name}:{}  {}", d.line + 1, d.message)).monospace().color(color);
                        if ui.add(egui::Button::new(text).frame(false)).on_hover_text(d.file.display().to_string()).clicked() {
                            if same_file(&d.file, &self.file) {
                                self.pending_line = Some(d.line);
                            } else {
                                cx.actions.push(Action::OpenFileAt { path: d.file.clone(), line: d.line });
                            }
                        }
                    }
                    if f.diags.is_empty() && !f.output.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&f.output).monospace().small().color(pal.text_weak)).wrap());
                    } else if !f.output.is_empty() {
                        egui::CollapsingHeader::new(crate::i18n::tr("Vollständige Ausgabe", "Full output")).id_salt(self.editor.id.with("out-raw")).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&f.output).monospace().small().color(pal.text_weak)).wrap());
                        });
                    }
                });
            });
        });
    }

    fn history_dialog(&mut self, ctx: &egui::Context) {
        let Some(list) = self.history.clone() else { return };
        let mut close = false;
        egui::Window::new(crate::i18n::text("Frühere Fassungen")).collapsible(false).default_size([520.0, 360.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            if list.is_empty() {
                ui.label(crate::i18n::text("Keine."));
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (time, path) in &list {
                    ui.horizontal(|ui| {
                        ui.label(time);
                        let preview: String = std::fs::read_to_string(path).unwrap_or_default().lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(50).collect();
                        ui.label(RichText::new(preview).monospace().small().color(style::pal().text_weak));
                        if ui.button(crate::i18n::text("Wiederherstellen")).clicked()
                            && let Ok(t) = std::fs::read_to_string(path)
                        {
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

/// Zuletzt angezeigte Stelle des Debuggers (damit nur beim Wechsel gesprungen wird)
static LAST_EXEC: Mutex<Option<(PathBuf, usize)>> = Mutex::new(None);

/// Der Nutzer hat die Anzeige einer beendeten Sitzung geschlossen
fn debug_closed() -> std::sync::MutexGuard<'static, bool> {
    static CLOSED: Mutex<bool> = Mutex::new(false);
    CLOSED.lock().unwrap()
}

fn pos(v: &serde_json::Value) -> (usize, usize) {
    (v["line"].as_u64().unwrap_or(0) as usize, v["character"].as_u64().unwrap_or(0) as usize)
}

/// JUnit in Maven/Gradle eintragen, wenn es fehlt (fuer "Test anlegen")
fn ensure_junit(build: &jrun::Build) -> Result<(), String> {
    match build {
        jrun::Build::Maven(root) => {
            let p = root.join("pom.xml");
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            if text.contains("junit-jupiter") {
                return Ok(());
            }
            let v = install::junit_version();
            let with = libs::add_to_pom(&text, "org.junit.jupiter", "junit-jupiter", &v)?;
            let with = with.replacen(&format!("<version>{v}</version>\n        </dependency>"), &format!("<version>{v}</version>\n            <scope>test</scope>\n        </dependency>"), 1);
            workspace::atomic_write(&p, &with).map(|_| ()).map_err(|e| e.to_string())
        }
        jrun::Build::Gradle(root) => {
            let kts = root.join("build.gradle.kts").is_file();
            let p = if kts { root.join("build.gradle.kts") } else { root.join("build.gradle") };
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            if text.contains("junit-jupiter") {
                return Ok(());
            }
            let v = install::junit_version();
            let mut out = libs::add_to_gradle(&text, kts, &format!("org.junit:junit-bom:{v}"))?;
            out = if kts {
                out.replace(&format!("    implementation(\"org.junit:junit-bom:{v}\")\n"), &format!("    testImplementation(platform(\"org.junit:junit-bom:{v}\"))\n    testImplementation(\"org.junit.jupiter:junit-jupiter\")\n    testRuntimeOnly(\"org.junit.platform:junit-platform-launcher\")\n"))
                    + "\ntasks.test {\n    useJUnitPlatform()\n}\n"
            } else {
                out.replace(&format!("    implementation 'org.junit:junit-bom:{v}'\n"), &format!("    testImplementation platform('org.junit:junit-bom:{v}')\n    testImplementation 'org.junit.jupiter:junit-jupiter'\n    testRuntimeOnly 'org.junit.platform:junit-platform-launcher'\n"))
                    + "\ntest {\n    useJUnitPlatform()\n}\n"
            };
            workspace::atomic_write(&p, &out).map(|_| ()).map_err(|e| e.to_string())
        }
        jrun::Build::Plain(_) => Ok(()),
    }
}

impl TabView for CodeTab {
    fn title(&self) -> String {
        workspace::dir_name(&self.file)
    }

    fn icon(&self) -> Option<Icon> {
        Some(match self.lang {
            Lang::Java | Lang::Gradle => Icon::Coffee,
            Lang::Python => Icon::Script,
            Lang::JavaScript | Lang::TypeScript => Icon::Braces,
            Lang::Json | Lang::Xml | Lang::Properties => Icon::FileJson,
            _ => Icon::FileText,
        })
    }

    fn modified(&self) -> bool {
        self.dirty()
    }

    fn busy(&self) -> bool {
        matches!(self.run, RunState::Preparing(..))
    }

    fn runnable(&self) -> bool {
        CodeTab::runnable(self)
    }

    fn database(&self) -> Option<String> {
        Some(self.database.clone()).filter(|d| !d.is_empty() && self.runnable())
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.request(Goal::Run, cx);
    }

    fn command(&mut self, cmd: Cmd, cx: &mut Ctx) -> bool {
        match cmd {
            Cmd::RunAll | Cmd::RunStatement => self.request(Goal::Run, cx),
            Cmd::RunTests => self.request(Goal::Test, cx),
            Cmd::BuildJar => self.request(Goal::Jar, cx),
            Cmd::AddLibrary => self.open_libs(),
            Cmd::GotoDefinition => self.goto_definition(self.editor.cursor, cx),
            Cmd::RenameSymbol => self.start_rename(cx),
            Cmd::Format if self.runnable() => self.format(cx),
            Cmd::DebugStart | Cmd::DebugContinue | Cmd::StepOver | Cmd::StepInto | Cmd::StepOut | Cmd::DebugStop | Cmd::ToggleBreakpoint => return self.debug_command(cmd, cx),
            Cmd::FixAll => self.fix_all(cx),
            Cmd::Find | Cmd::Replace | Cmd::GotoLine => self.editor.pending = Some(cmd),
            _ => return false,
        }
        true
    }

    fn key(&self) -> Option<String> {
        Some(format!("file:{}", file_key(&self.file)))
    }

    fn session(&self) -> Option<String> {
        Some(format!("code\t{}", self.file.display()))
    }

    fn file(&self) -> Option<&Path> {
        Some(&self.file)
    }

    fn file_renamed(&mut self, old: &Path, new: &Path) {
        if self.file == old || self.file.starts_with(old) {
            if let Some(s) = &self.lsp {
                s.close(&self.file);
            }
            self.lsp_synced = None;
            let rel = self.file.strip_prefix(old).unwrap_or(Path::new("")).to_path_buf();
            self.file = if rel.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rel) };
            if self.project.as_ref().is_some_and(|p| !self.file.starts_with(p)) {
                self.project = None;
            }
            self.lang = Lang::from_path(&self.file);
            self.editor.lang = self.lang;
        }
    }

    fn apply_edits(&mut self, edits: &[lsp::TextEdit]) -> bool {
        self.text = lsp::apply_edits(&self.text, edits);
        self.last_edit = Instant::now();
        true
    }

    fn on_close(&mut self) -> bool {
        if self.dirty() && sqledit_autosave() {
            self.save();
        }
        if let Some(s) = &self.lsp {
            s.close(&self.file);
        }
        true
    }

    fn unsaved(&self) -> Option<String> {
        (self.dirty() && !sqledit_autosave()).then(|| workspace::dir_name(&self.file))
    }

    fn discard_changes(&mut self) {
        self.text = self.saved_text.clone();
    }

    fn save_now(&mut self) {
        self.save();
    }

    fn goto_line(&mut self, line: usize) {
        self.pending_line = Some(line);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let ctx = ui.ctx().clone();
        self.poll_run(&ctx, cx);
        self.poll_lsp(&ctx);
        self.poll_lsp_wait(cx);
        if self.runnable() {
            install::weekly_update(&ctx);
        }
        // Automatisch speichern kurz nach der letzten Aenderung
        if self.dirty() && sqledit_autosave() {
            let delay = Duration::from_millis(AUTOSAVE_MS.load(Ordering::Relaxed));
            if self.last_edit.elapsed() > delay {
                self.save();
            } else {
                ctx.request_repaint_after(Duration::from_millis(300));
            }
        }
        // Aenderungen von aussen uebernehmen
        if !self.dirty() {
            if self.file_check.is_none_or(|last| last.elapsed() >= Duration::from_secs(2)) {
                self.file_check = Some(Instant::now());
                self.update_database_module();
                if let Ok(m) = std::fs::metadata(&self.file).and_then(|m| m.modified())
                    && self.file_mtime.is_some_and(|old| old != m)
                {
                    if let Ok(b) = std::fs::read(&self.file) {
                        self.text = String::from_utf8_lossy(&b).into_owned();
                        self.saved_text = self.text.clone();
                    }
                    self.file_mtime = Some(m);
                }
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if self.runnable() {
            self.editor.schema = if self.database.is_empty() { None } else { cx.schema(&self.database) };
            if self.editor.databases.len() != cx.databases.len() {
                self.editor.databases = cx.databases.to_vec();
            }
        }
        if self.lang == Lang::Java {
            self.editor.breakpoints = debug::breakpoints(&self.file);
        }
        self.toolbar(ui, cx);
        ui.add_space(4.0);
        let row = ctx.fonts_mut(|f| f.row_height(&egui::FontId::monospace(sqledit::font_size())));
        if let Some(l) = self.pending_line.take() {
            self.editor.goto_line(&ctx, &self.text, l, row);
        }
        if let Some(c) = self.pending_cursor.take() {
            let (line, _) = lsp::to_pos(&self.text, c);
            self.editor.goto_line(&ctx, &self.text, line, row);
        }
        self.output_panel(ui, cx);
        if !*debug_closed() || debug::active() {
            if debug::active() {
                *debug_closed() = false;
            }
            self.debug_panel(ui, cx);
        } else {
            self.editor.exec_line = None;
        }
        let h = (ui.available_height() - 8.0).max(100.0);
        let out = self.editor.show(ui, &mut self.text, h);
        if out.changed {
            self.last_edit = Instant::now();
        }
        if let Some(line) = out.toggle_breakpoint {
            self.toggle_breakpoint(line);
        }
        if let Some(c) = out.ctrl_click {
            self.goto_definition(c, cx);
        }
        if self.runnable() {
            self.poll_hover(ui, out.hover);
        }
        self.history_dialog(&ctx);
        self.libs_dialog(&ctx, cx);
        self.rename_dialog(&ctx);
    }
}

/// Wartezeit fuer das automatische Speichern (Einstellung)
pub static AUTOSAVE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);

fn sqledit_autosave() -> bool {
    super::sql::AUTOSAVE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_are_added_once_after_package_and_imports() {
        let (t, n, at) = ensure_imports("package app;\n\nimport java.util.*;\n\nclass A {}\n", &["java.sql.*".into(), "java.util.*".into()]);
        assert_eq!(t, "package app;\n\nimport java.util.*;\nimport java.sql.*;\n\nclass A {}\n");
        assert_eq!((n, at), ("import java.sql.*;\n".len(), "package app;\n\nimport java.util.*;\n".len()));
        let (t, _, _) = ensure_imports("package app;\n\nclass A {}\n", &["java.sql.*".into()]);
        assert_eq!(t, "package app;\n\nimport java.sql.*;\n\nclass A {}\n");
        let (t, _, _) = ensure_imports("class A {}\n", &["java.sql.*".into()]);
        assert_eq!(t, "import java.sql.*;\nclass A {}\n");
        let (t, n) = ensure_header_lines("#!/usr/bin/env python3\nimport db\nx = 1\n", &["import db".into(), "import os".into()]);
        assert_eq!(t, "#!/usr/bin/env python3\nimport os\nimport db\nx = 1\n");
        assert_eq!(n, "import os\n".len());
    }

    #[test]
    fn database_comes_from_the_project_config() {
        let dir = std::env::temp_dir().join(format!("easymysql-dbprops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/main/resources")).unwrap();
        std::fs::create_dir_all(dir.join("src/main/java/app")).unwrap();
        std::fs::write(dir.join("src/main/resources/db.properties"), "# x\nurl=jdbc:mariadb://127.0.0.1:3306/schule?useSsl=false\nuser=root\n").unwrap();
        assert_eq!(database_from_config(&dir.join("src/main/java/app/Main.java"), Some(&dir)).as_deref(), Some("schule"));
        std::fs::create_dir_all(dir.join("py")).unwrap();
        std::fs::write(dir.join("py/db.ini"), "[database]\nhost = x\ndatabase = shop\n").unwrap();
        assert_eq!(database_from_config(&dir.join("py/main.py"), Some(&dir)).as_deref(), Some("shop"));
        std::fs::create_dir_all(dir.join("js")).unwrap();
        std::fs::write(dir.join("js/db.json"), "{\"database\": \"lager\"}").unwrap();
        assert_eq!(database_from_config(&dir.join("js/index.js"), Some(&dir)).as_deref(), Some("lager"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
