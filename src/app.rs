// Hauptfenster von EasyMySQL.

mod autoscroll;
mod dialogs;
mod palette;
mod shell;
mod side;
mod terminal_dock;

use autoscroll::Autoscroll;
pub use autoscroll::block_autoscroll;
use dialogs::Dialog;
use side::{ExplorerDlg, SearchState, SideView};
use terminal_dock::TerminalDock;

use crate::db::{self, Db};
use crate::keymap::{self, Cmd};
use crate::platform::{self, TrayCmd};
use crate::server::{Server, State};
use crate::settings::{self, Settings};
use crate::style;
use crate::tabs::{
    Action, Ctx, SchemaCache, TabView, builder::BuilderTab, data::DataTab, designer::DesignerTab, er::ErTab, help::HelpTab,
    log::LogTab, safety::SafetyTab, settings::SettingsTab, sql::SqlTab, structure::StructureTab,
};
use eframe::egui::{self, Rect, ViewportCommand};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// Kontext fuer Registerkarten aus den Feldern der App (getrennte Ausleihe der Felder)
macro_rules! cx {
    ($s:ident, $actions:expr) => {
        Ctx {
            db: $s.db.as_ref(),
            schemas: &mut $s.schemas,
            databases: &$s.databases,
            server: &$s.server,
            project: $s.project.as_deref(),
            settings: &mut $s.settings,
            history: &$s.history,
            actions: $actions,
        }
    };
}


struct Toast {
    text: String,
    error: bool,
    at: Instant,
}

pub struct EasyApp {
    settings: Settings,
    applied_language: crate::i18n::Language,
    updater: crate::updates::Updater,
    server: Server,
    db: Option<Db>,
    auto_connect: bool,
    databases: Vec<String>,
    schemas: SchemaCache,
    tabs: Vec<Box<dyn TabView>>,
    active: usize,
    /// Sitzungseintraege geschlossener Registerkarten (zum Wiederoeffnen)
    closed: Vec<String>,
    current_db: String,
    toasts: Vec<Toast>,
    terminal: TerminalDock,
    dialogs: Vec<Dialog>,
    hwnd: Option<isize>,
    tray: Option<(platform::Tray, Receiver<TrayCmd>)>,
    quitting: bool,
    last_state: State,
    backup_job: Option<Receiver<Result<String, String>>>,
    export_job: Option<Receiver<Result<PathBuf, String>>>,
    backup_last_check: Option<Instant>,
    check_job: Option<Receiver<Result<crate::repair::CheckResult, String>>>,
    myisam_hint_done: bool,
    /// Geoeffneter Projektordner (None = keiner)
    project: Option<PathBuf>,
    tree: Option<std::sync::Arc<crate::workspace::Node>>,
    saved_session: Option<crate::workspace::Session>,
    tree_scan: Option<Instant>,
    tree_job: Option<Receiver<(PathBuf, crate::workspace::Node)>>,
    side_view: SideView,
    sidebar: bool,
    explorer_dlg: Option<ExplorerDlg>,
    session_saved: Instant,
    history: Vec<crate::qhistory::Entry>,
    history_filter: String,
    search: SearchState,
    palette: Option<palette::Palette>,
    autoscroll: Autoscroll,
    /// Zuletzt an den Server uebertragener Zustand der Anweisungs-Mitschrift
    query_log_applied: Option<bool>,
}

pub const COLLATIONS: &[&str] = &[
    "utf8mb4_unicode_ci",
    "utf8mb4_general_ci",
    "utf8mb4_german2_ci",
    "utf8mb4_bin",
    "latin1_german1_ci",
    "latin1_swedish_ci",
];

impl EasyApp {
    pub fn new(cc: &eframe::CreationContext, startup_files: Vec<PathBuf>) -> Self {
        *CTX.lock().unwrap() = Some(cc.egui_ctx.clone());
        let mut settings = Settings::load();
        crate::workspace::set_storage_root(&settings.storage_dir);
        for p in crate::workspace::migrate_legacy() {
            if !settings.recent.contains(&p) {
                settings.recent.push(p);
            }
        }
        keymap::install(keymap::Keymap::load());
        crate::i18n::install(settings.language);
        style::apply(&cc.egui_ctx, settings.dark);
        // Strg + / Strg - aendern nur die Schrift im Editor (egui wuerde sonst die ganze Oberflaeche skalieren)
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        apply_editor_prefs(&settings);
        let server = Server::new(3306);
        let hwnd = platform::window_handle(cc);
        let tray = platform::create_tray(&cc.egui_ctx, hwnd);
        {
            let srv = server.clone();
            let conn = settings.conn.clone();
            platform::install_shutdown_hook(
                hwnd,
                Box::new(move || {
                    srv.stop_blocking(&conn);
                }),
            );
        }
        let ctx = cc.egui_ctx.clone();
        server.start(move || ctx.request_repaint());
        let session = crate::workspace::Session::load();
        let project = session
            .as_ref()
            .map(|s| {
                let p = PathBuf::from(&s.project);
                // Version 2 speicherte nur den Namen unter Dokumente\EasyMySQL\Projekte
                if p.is_absolute() { p } else { crate::workspace::default_root().join("Projekte").join(p) }
            })
            .filter(|p| !s_empty(p) && p.is_dir());
        let history = crate::qhistory::load();
        let mut app = EasyApp {
            applied_language: settings.language,
            settings,
            updater: crate::updates::Updater::default(),
            server,
            db: None,
            auto_connect: true,
            databases: Vec::new(),
            schemas: {
                let mut c = SchemaCache::default();
                c.set_repaint(cc.egui_ctx.clone());
                c
            },
            tabs: Vec::new(),
            active: 0,
            closed: Vec::new(),
            current_db: String::new(),
            toasts: Vec::new(),
            terminal: TerminalDock::default(),
            dialogs: Vec::new(),
            hwnd,
            tray,
            quitting: false,
            last_state: State::Stopped,
            backup_job: None,
            export_job: None,
            backup_last_check: None,
            check_job: None,
            myisam_hint_done: false,
            project,
            tree: None,
            saved_session: None,
            tree_scan: None,
            tree_job: None,
            side_view: SideView::Explorer,
            sidebar: true,
            explorer_dlg: None,
            session_saved: Instant::now(),
            history,
            history_filter: String::new(),
            search: SearchState::default(),
            palette: None,
            autoscroll: Autoscroll::default(),
            query_log_applied: None,
        };
        if let Some(p) = app.project.clone() {
            app.settings.add_recent(&p);
        }
        app.settings.save();
        app.restore_session(session);
        // Dateien, mit denen EasyMySQL gestartet wurde, oeffnen
        for f in startup_files {
            app.open_file(&f);
        }
        let ctx = cc.egui_ctx.clone();
        platform::install_open_hook(Box::new(move || ctx.request_repaint()));
        app
    }

    // ------------------------------------------------------------------
    // Meldungen

    /// Kurze Meldung unten rechts (verschwindet nach ein paar Sekunden)
    fn toast(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast { text, error: false, at: Instant::now() });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), error: true, at: Instant::now() });
    }

    fn error(&mut self, e: impl Into<String>) {
        self.dialogs.push(Dialog::Message { title: crate::i18n::text("Fehler").into(), text: e.into(), error: true });
    }

    // ------------------------------------------------------------------
    // Verbindung

    fn connect(&mut self, show_dialog_on_error: bool) {
        match Db::connect(&self.settings.conn) {
            Ok(db) => {
                let info = db.info.clone();
                self.db = Some(db);
                self.query_log_applied = None;
                self.update_backup_env();
                self.refresh_all();
                // Nach einem Absturz: alle Tabellen im Hintergrund pruefen
                if self.server.take_unclean() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let conn = info.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::repair::check_all(&conn, false, &|_| {}));
                    });
                    self.check_job = Some(rx);
                    self.toast(crate::i18n::text("Letztes Mal nicht sauber beendet – alle Tabellen werden geprüft …"));
                }
                if !self.myisam_hint_done {
                    self.myisam_hint_done = true;
                    if let Ok(list) = crate::repair::myisam_tables(&info)
                        && !list.is_empty() {
                            self.dialogs.push(Dialog::Myisam(list));
                        }
                }
            }
            Err(e) => {
                self.db = None;
                if show_dialog_on_error {
                    self.dialogs.push(Dialog::ConnectFailed(e));
                } else {
                    self.toast_error(crate::tr_format!("Verbindung fehlgeschlagen: {e}", "Connection failed: {e}"));
                }
            }
        }
    }

    fn update_backup_env(&mut self) {
        let env = match (&self.db, &self.server.paths) {
            (Some(db), Some(paths)) => Some(crate::backup::Env {
                paths: paths.clone(),
                conn: db.info.clone(),
                dir: crate::backup::Config::load().dir(),
                force: false,
            }),
            _ => None,
        };
        crate::backup::set_env(env);
    }

    /// Mitschrift aller Anweisungen (general log) am eigenen Server ein-/ausschalten.
    fn apply_query_log(&mut self) {
        let want = self.settings.query_log;
        if self.query_log_applied == Some(want) || !matches!(self.server.state(), State::Running { own: true }) {
            return;
        }
        let Some(dbc) = &self.db else { return };
        let path = crate::server::query_log_file().to_string_lossy().replace('\\', "/");
        let sql = if want {
            format!("SET GLOBAL general_log_file = {}; SET GLOBAL log_output = 'FILE'; SET GLOBAL general_log = ON", db::lit(&path))
        } else {
            "SET GLOBAL general_log = OFF".to_string()
        };
        let first = self.query_log_applied.is_none();
        let res = dbc.query(&sql);
        self.query_log_applied = Some(want);
        match res {
            Ok(_) if want => self.server.log(crate::i18n::text("Mitschrift aller Anweisungen eingeschaltet.")),
            Ok(_) if !first => self.server.log(crate::i18n::text("Mitschrift aller Anweisungen ausgeschaltet.")),
            Ok(_) => {}
            Err(e) => self.toast_error(crate::tr_format!("Mitschrift: {e}", "Query logging: {e}")),
        }
    }

    /// Automatische Sicherung (alle x Stunden) und Pruefergebnisse abholen.
    fn background_tasks(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.backup_job
            && let Ok(r) = rx.try_recv() {
                self.backup_job = None;
                match r {
                    Ok(m) => self.server.log(m),
                    Err(e) => self.toast_error(crate::tr_format!("Automatische Sicherung fehlgeschlagen: {e}", "Automatic backup failed: {e}")),
                }
            }
        if let Some(rx) = &self.export_job {
            ctx.request_repaint_after(Duration::from_millis(300));
            if let Ok(r) = rx.try_recv() {
                self.export_job = None;
                match r {
                    Ok(p) => self.toast(crate::tr_format!("Gespeichert: {}", "Saved: {}", p.display())),
                    Err(e) => self.toast_error(e),
                }
            }
        }
        if let Some(rx) = &self.check_job
            && let Ok(r) = rx.try_recv() {
                self.check_job = None;
                match r {
                    Ok(res) if res.problems.is_empty() => {
                        self.toast(crate::tr_format!("{} Tabellen geprüft – alles in Ordnung.", "{} tables checked — everything is OK.", res.checked));
                    }
                    Ok(res) => {
                        let list: Vec<String> = res.problems.iter().map(|(t, m)| format!("• {t}: {m}")).collect();
                        self.dialogs.push(Dialog::Message {
                            title: crate::i18n::text("Beschädigte Tabellen gefunden").into(),
                            text: crate::tr_format!("{}\n\nUnter „Sicherungen & Reparatur“ auf „Prüfen und reparieren“ klicken.", "{}\n\nOpen “Backups & repair” and click “Check and repair”.", list.join("\n")),
                            error: true,
                        });
                        self.open_tab(Box::new(SafetyTab::new()));
                    }
                    Err(e) => self.toast_error(crate::tr_format!("Prüfung fehlgeschlagen: {e}", "Check failed: {e}")),
                }
            }
        // Regelmaessige Sicherung
        let now = Instant::now();
        let check = self.backup_last_check.is_none_or(|t| now.duration_since(t).as_secs() >= 60);
        if check && self.backup_job.is_none() && self.db.is_some() && self.server.state().is_running() {
            self.backup_last_check = Some(now);
            let cfg = crate::backup::Config::load();
            let dir = cfg.dir();
            if cfg.auto && crate::backup::due(&dir, Duration::from_secs(cfg.interval_hours * 3600)) {
                self.update_backup_env();
                if let Some(env) = crate::backup::env() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let r = crate::backup::create(&env, None, "auto", &|_| {}).map(|d| {
                            crate::backup::rotate(&cfg.dir(), cfg.keep_count, cfg.keep_days);
                            crate::tr_format!("Automatische Sicherung erstellt ({})", "Automatic backup created ({})", d.file_name().unwrap_or_default().to_string_lossy())
                        });
                        let _ = tx.send(r);
                        ctx.request_repaint();
                    });
                    self.backup_job = Some(rx);
                }
            }
        }
        self.apply_query_log();
    }

    fn refresh_all(&mut self) {
        self.schemas.clear();
        if let Some(db) = &self.db {
            match db.databases() {
                Ok(d) => self.databases = d,
                Err(e) => self.toast_error(e),
            }
        }
        if self.current_db.is_empty() || !self.databases.contains(&self.current_db) {
            self.current_db = self.user_databases().first().cloned().unwrap_or_default();
        }
        let dbs: Vec<String> = self.databases.clone();
        let mut actions = Vec::new();
        for d in &dbs {
            let mut cx = cx!(self, &mut actions);
            for t in self.tabs.iter_mut() {
                t.schema_changed(d, &mut cx);
            }
        }
        self.handle_actions(actions);
    }

    fn user_databases(&self) -> Vec<String> {
        self.databases.iter().filter(|d| !db::SYSTEM_DATABASES.contains(&d.as_str())).cloned().collect()
    }

    // ------------------------------------------------------------------
    // Registerkarten

    fn open_tab(&mut self, tab: Box<dyn TabView>) {
        if let Some(k) = tab.key()
            && let Some(i) = self.tabs.iter().position(|t| t.key().as_deref() == Some(&k)) {
                self.active = i;
                return;
            }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Registerkarte schliessen; hat sie ungespeicherte Aenderungen (Autosave aus), wird vorher gefragt.
    fn close_tab(&mut self, i: usize) {
        self.request_close(vec![i]);
    }

    fn request_close(&mut self, mut idx: Vec<usize>) {
        idx.sort_unstable();
        idx.dedup();
        idx.retain(|&i| i < self.tabs.len());
        let files: Vec<String> = idx.iter().filter_map(|&i| self.tabs[i].unsaved()).collect();
        if files.is_empty() {
            self.close_now(idx);
        } else {
            self.dialogs.push(Dialog::Unsaved { files, then: dialogs::Then::CloseTabs(idx) });
        }
    }

    fn close_now(&mut self, idx: Vec<usize>) {
        for i in idx.into_iter().rev() {
            self.close_tab_now(i);
        }
    }

    fn close_tab_now(&mut self, i: usize) {
        if i >= self.tabs.len() || !self.tabs[i].on_close() {
            return;
        }
        let t = self.tabs.remove(i);
        if let Some(s) = t.session() {
            // Leere unbenannte Abfragen werden beim Schliessen geloescht
            if !s.starts_with("scratch") || workspace_scratch_exists(&s) {
                self.closed.push(s);
            }
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if self.active > i {
            self.active -= 1;
        }
    }

    fn close_tabs_where(&mut self, keep: impl Fn(usize) -> bool) {
        let idx: Vec<usize> = (0..self.tabs.len()).filter(|&i| !keep(i)).collect();
        self.request_close(idx);
    }

    /// Alle Tabs mit ungespeicherten Dateien (nur bei ausgeschaltetem Autosave)
    fn unsaved_files(&self) -> Vec<String> {
        self.tabs.iter().filter_map(|t| t.unsaved()).collect()
    }

    /// Beenden; vorher nach ungespeicherten Aenderungen fragen.
    pub(crate) fn request_quit(&mut self, ctx: &egui::Context) {
        let files = self.unsaved_files();
        if files.is_empty() {
            self.quit();
        } else {
            // Das Fenster kann im Infobereich versteckt sein
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.dialogs.push(Dialog::Unsaved { files, then: dialogs::Then::Quit });
        }
    }

    fn need_db(&mut self) -> Option<String> {
        if self.db.is_none() {
            self.error(crate::i18n::text("Keine Verbindung zum Datenbankserver."));
            return None;
        }
        if self.current_db.is_empty() {
            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
            return None;
        }
        Some(self.current_db.clone())
    }

    fn handle_actions(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::OpenData { db, table } => {
                    self.current_db = db.clone();
                    self.open_tab(Box::new(DataTab::new(db, table)));
                }
                Action::OpenStructure { db, table } => {
                    self.current_db = db.clone();
                    self.open_tab(Box::new(StructureTab::new(db, table)));
                }
                Action::OpenSql { db, sql, run } => {
                    let db = db.filter(|d| !d.is_empty()).or_else(|| Some(self.current_db.clone()));
                    let mut tab = SqlTab::scratch(db, sql);
                    if run {
                        let mut acts = Vec::new();
                        tab.execute(&mut cx!(self, &mut acts));
                        self.handle_actions(acts);
                    }
                    self.open_tab(Box::new(tab));
                }
                Action::OpenEr(db) => {
                    self.current_db = db.clone();
                    self.open_tab(Box::new(ErTab::new(db)));
                }
                Action::OpenBuilder(db) => self.open_tab(Box::new(BuilderTab::new(db))),
                Action::NewTable(db) => {
                    self.current_db = db.clone();
                    self.open_tab(Box::new(DesignerTab::new(db)));
                }
                Action::RefreshAll => self.refresh_all(),
                Action::SchemaChanged(d) => {
                    self.schemas.invalidate(&d);
                    let mut acts = Vec::new();
                    let mut cx = cx!(self, &mut acts);
                    for t in self.tabs.iter_mut() {
                        t.schema_changed(&d, &mut cx);
                    }
                    self.handle_actions(acts);
                }
                Action::Status(s) => self.toast(s),
                Action::Error(e) => self.error(e),
                Action::Confirm { text, db, sql } => self.dialogs.push(Dialog::Confirm { text, db, sql }),
                Action::Reconnect | Action::Connect => {
                    self.auto_connect = true;
                    self.db = None;
                    if self.server.state().is_running() || !self.is_local() {
                        self.connect(true);
                    }
                }
                Action::Disconnect => {
                    self.db = None;
                    self.databases.clear();
                    self.schemas.clear();
                    crate::backup::set_env(None);
                }
                Action::OpenSafety => self.open_tab(Box::new(SafetyTab::new())),
                Action::FilesChanged => self.tree_scan = None,
                Action::OpenFileAt { path, line } => {
                    self.open_file(&path);
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.goto_line(line);
                    }
                }
                Action::OpenHelp(section) => {
                    self.open_tab(Box::new(HelpTab::new()));
                    if let (Some(s), Some(t)) = (section, self.tabs.get_mut(self.active)) {
                        t.show_section(&s);
                    }
                }
                Action::OpenSettings(section) => {
                    self.open_tab(Box::new(SettingsTab::new()));
                    if let (Some(s), Some(t)) = (section, self.tabs.get_mut(self.active)) {
                        t.show_section(&s);
                    }
                }
                Action::OpenLog => self.open_tab(Box::new(LogTab::new())),
                Action::CopyText(text) => {
                    if let Some(ctx) = CTX.lock().unwrap().as_ref() {
                        ctx.copy_text(text);
                    }
                }
                Action::Run(cmd) => {
                    let ctx = CTX.lock().unwrap().clone();
                    if let Some(ctx) = ctx {
                        self.run_cmd(&ctx, cmd);
                    }
                }
                Action::History(entries) => {
                    crate::qhistory::append(&entries);
                    for e in entries {
                        self.history.insert(0, e);
                    }
                    self.history.truncate(3000);
                }
                Action::SettingsChanged => self.apply_settings(),
                Action::FilesMoved { from, to } => {
                    for t in self.tabs.iter_mut() {
                        t.file_renamed(&from, &to);
                    }
                    let remap = |p: &PathBuf| p.strip_prefix(&from).map(|r| to.join(r)).unwrap_or_else(|_| p.clone());
                    self.settings.recent = self.settings.recent.iter().map(remap).collect();
                    self.project = self.project.as_ref().map(remap);
                    self.tree_scan = None;
                    self.settings.save();
                }
            }
        }
    }

    fn is_local(&self) -> bool {
        matches!(self.settings.conn.host.as_str(), "127.0.0.1" | "localhost") && self.settings.conn.port == self.server.port
    }

    /// Geaenderte Einstellungen anwenden und speichern
    fn apply_settings(&mut self) {
        crate::i18n::install(self.settings.language);
        if let Some(ctx) = CTX.lock().unwrap().as_ref() {
            style::set_theme(ctx, self.settings.dark);
        }
        if self.applied_language != self.settings.language {
            if let Some(ctx) = CTX.lock().unwrap().as_ref() {
                self.tray = platform::create_tray(ctx, self.hwnd);
            }
            self.applied_language = self.settings.language;
        }
        apply_editor_prefs(&self.settings);
        let root_before = crate::workspace::storage_root();
        crate::workspace::set_storage_root(&self.settings.storage_dir);
        if crate::workspace::storage_root() != root_before {
            self.save_all();
        }
        self.settings.save();
    }

    // ------------------------------------------------------------------
    // Befehle (Tastenkuerzel, Menue, Befehlsliste)

    fn run_cmd(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::RunAll | Cmd::RunStatement | Cmd::Explain | Cmd::Format | Cmd::FixAll | Cmd::CopyJava | Cmd::Find | Cmd::Replace | Cmd::GotoLine => {
                let mut acts = Vec::new();
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.command(cmd, &mut cx!(self, &mut acts));
                }
                self.handle_actions(acts);
            }
            Cmd::NewQuery => self.handle_actions(vec![Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false }]),
            Cmd::NewFile => match self.project.clone() {
                Some(p) => self.explorer_dlg = Some(ExplorerDlg::NewFile { name: side::new_name(&p), dir: p }),
                None => self.handle_actions(vec![Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false }]),
            },
            Cmd::OpenFile => self.open_sql_file(false),
            Cmd::OpenFolder => self.pick_project(),
            Cmd::Save => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.save_now();
                }
            }
            Cmd::SaveAll => {
                self.save_all();
                self.toast(crate::i18n::text("Alles gespeichert."));
            }
            Cmd::CloseTab => {
                let a = self.active;
                self.close_tab(a);
            }
            Cmd::CloseAllTabs => {
                self.close_tabs_where(|_| false);
                self.save_session();
            }
            Cmd::ReopenTab => {
                if let Some(s) = self.closed.pop()
                    && let Some(t) = self.tab_from_session(&s) {
                        self.open_tab(t);
                    }
            }
            Cmd::NextTab if !self.tabs.is_empty() => self.active = (self.active + 1) % self.tabs.len(),
            Cmd::PrevTab if !self.tabs.is_empty() => self.active = (self.active + self.tabs.len() - 1) % self.tabs.len(),
            Cmd::CommandPalette => self.palette = Some(palette::Palette::new(true)),
            Cmd::QuickOpen => self.palette = Some(palette::Palette::new(false)),
            Cmd::ToggleSidebar => self.sidebar = !self.sidebar,
            Cmd::ToggleTerminal => self.toggle_terminal(ctx),
            Cmd::NewTerminal => self.new_terminal(ctx, None),
            Cmd::FontLarger => self.set_editor_font(self.settings.editor_font.saturating_add(1)),
            Cmd::FontSmaller => self.set_editor_font(self.settings.editor_font.saturating_sub(1)),
            Cmd::FontReset => self.set_editor_font(settings::DEFAULT_FONT),
            Cmd::ShowExplorer => self.show_side(SideView::Explorer),
            Cmd::ShowSearch => {
                self.show_side(SideView::Search);
                self.search.focus = true;
            }
            Cmd::ShowDatabases => self.show_side(SideView::Databases),
            Cmd::ShowHistory => self.show_side(SideView::History),
            Cmd::Settings => self.handle_actions(vec![Action::OpenSettings(None)]),
            Cmd::Help => self.handle_actions(vec![Action::OpenHelp(None)]),
            Cmd::CheckUpdates => self.updater.check_now(ctx, true),
            Cmd::ServerLog => self.handle_actions(vec![Action::OpenLog]),
            Cmd::ErDiagram => {
                if let Some(d) = self.need_db() {
                    self.handle_actions(vec![Action::OpenEr(d)]);
                }
            }
            Cmd::QueryBuilder => self.handle_actions(vec![Action::OpenBuilder(Some(self.current_db.clone()))]),
            Cmd::Backups => self.handle_actions(vec![Action::OpenSafety]),
            _ => {}
        }
        let _ = ctx;
    }

    /// Schriftgroesse im Editor aendern und merken; die uebrige Oberflaeche bleibt unveraendert.
    fn set_editor_font(&mut self, px: u32) {
        let px = px.clamp(settings::MIN_FONT, settings::MAX_FONT);
        if px != self.settings.editor_font {
            self.settings.editor_font = px;
            apply_editor_prefs(&self.settings);
            self.settings.save();
        }
        self.toast(crate::tr_format!("Editor-Schrift: {px} px", "Editor font size: {px} px"));
    }

    fn show_side(&mut self, v: SideView) {
        self.side_view = v;
        self.sidebar = true;
    }

    /// Tastenkuerzel, die unabhaengig vom Fokus gelten
    fn global_keys(&mut self, ctx: &egui::Context) {
        if !self.dialogs.is_empty() || self.explorer_dlg.is_some() {
            return;
        }
        const GLOBAL: &[Cmd] = &[
            Cmd::RunAll,
            Cmd::RunStatement,
            Cmd::Explain,
            Cmd::Format,
            Cmd::FixAll,
            Cmd::Find,
            Cmd::Replace,
            Cmd::GotoLine,
            Cmd::NewQuery,
            Cmd::NewFile,
            Cmd::OpenFile,
            Cmd::OpenFolder,
            Cmd::Save,
            Cmd::SaveAll,
            Cmd::CloseTab,
            Cmd::CloseAllTabs,
            Cmd::ReopenTab,
            Cmd::NextTab,
            Cmd::PrevTab,
            Cmd::CommandPalette,
            Cmd::QuickOpen,
            Cmd::ToggleSidebar,
            Cmd::ToggleTerminal,
            Cmd::NewTerminal,
            Cmd::FontLarger,
            Cmd::FontSmaller,
            Cmd::FontReset,
            Cmd::ShowExplorer,
            Cmd::ShowSearch,
            Cmd::ShowDatabases,
            Cmd::ShowHistory,
            Cmd::Settings,
            Cmd::Help,
            Cmd::CheckUpdates,
            Cmd::ServerLog,
            Cmd::ErDiagram,
            Cmd::QueryBuilder,
            Cmd::Backups,
        ];
        // Waehrend ein Kuerzel in den Einstellungen aufgezeichnet wird, nichts ausloesen
        if crate::tabs::settings::recording() {
            return;
        }
        // Im Terminal gehoeren fast alle Tasten der Shell
        let in_terminal = crate::terminal::focused();
        for c in GLOBAL {
            if in_terminal && !matches!(c, Cmd::ToggleTerminal | Cmd::NewTerminal | Cmd::CommandPalette) {
                continue;
            }
            if keymap::take_ctx(ctx, *c) {
                self.run_cmd(ctx, *c);
            }
        }
    }

    // ------------------------------------------------------------------
    // Server und Beenden

    fn on_change_cb(ctx: &egui::Context) -> impl Fn() + Send + 'static {
        let ctx = ctx.clone();
        move || ctx.request_repaint()
    }

    fn check_server(&mut self, ctx: &egui::Context) {
        let st = self.server.state();
        if st != self.last_state {
            if st.is_running() && self.db.is_none() && self.auto_connect {
                self.auto_connect = false;
                self.connect(true);
            }
            if !st.is_running() && self.last_state.is_running() && self.db.is_some() && self.is_local() {
                self.db = None;
                self.databases.clear();
                self.schemas.clear();
            }
            if let State::Failed(e) = &st {
                self.toast_error(format!("Server: {e}"));
            }
            self.last_state = st;
            ctx.request_repaint();
        }
        if self.last_state.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    fn start_server(&mut self, ctx: &egui::Context) {
        self.auto_connect = true;
        self.server.start(Self::on_change_cb(ctx));
    }

    fn stop_server(&mut self, ctx: &egui::Context) {
        self.db = None;
        self.databases.clear();
        self.schemas.clear();
        self.server.stop(self.settings.conn.clone(), Self::on_change_cb(ctx));
    }

    fn restart_server(&mut self, ctx: &egui::Context) {
        self.db = None;
        self.databases.clear();
        self.schemas.clear();
        self.auto_connect = true;
        self.server.restart(self.settings.conn.clone(), Self::on_change_cb(ctx));
    }

    fn quit(&mut self) {
        self.quitting = true;
        self.terminal.close_all();
        self.save_all();
        if let Some(h) = self.hwnd {
            platform::hide_window(h);
        }
        self.db = None;
        self.server.stop_blocking(&self.settings.conn);
        self.settings.save();
        std::process::exit(0);
    }

    // ------------------------------------------------------------------
    // Dateien und Projekte

    fn open_sql_file(&mut self, run: bool) {
        let mut dlg = rfd::FileDialog::new().add_filter(crate::i18n::text("SQL-Dateien"), &["sql", "txt"]);
        if let Some(p) = &self.project {
            dlg = dlg.set_directory(p);
        }
        if let Some(p) = dlg.pick_file() {
            self.open_file(&p);
            if run {
                let mut acts = Vec::new();
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.execute(&mut cx!(self, &mut acts));
                }
                self.handle_actions(acts);
            }
        }
    }

    /// Datei im Editor oeffnen (oder vorhandene Registerkarte aktivieren).
    pub(crate) fn open_file(&mut self, p: &Path) {
        let key = format!("file:{}", p.to_string_lossy().to_lowercase());
        if let Some(i) = self.tabs.iter().position(|t| t.key().as_deref() == Some(&key)) {
            self.active = i;
            return;
        }
        // Bilder, PDF-Dokumente und andere Binaerdateien bekommen eine Vorschau statt des Texteditors
        let mut head = [0u8; 8192];
        let n = std::fs::File::open(p).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        let kind = crate::tabs::preview::detect(p, &head[..n]);
        if kind != crate::tabs::preview::Kind::Text {
            match crate::tabs::preview::PreviewTab::open(&CTX.lock().unwrap().clone().unwrap_or_default(), p, kind) {
                Ok(tab) => self.open_tab(Box::new(tab)),
                Err(e) => self.error(e),
            }
            return;
        }
        match SqlTab::open_file(p, &self.current_db, self.project.as_deref()) {
            Ok(tab) => self.open_tab(Box::new(tab)),
            Err(e) => self.error(e),
        }
    }

    fn pick_project(&mut self) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(p) = self.project.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(p);
        } else {
            let root = crate::workspace::storage_root();
            let _ = std::fs::create_dir_all(&root);
            dlg = dlg.set_directory(root);
        }
        if let Some(p) = dlg.pick_folder() {
            self.switch_project(Some(p));
        }
    }

    pub(crate) fn switch_project(&mut self, dir: Option<PathBuf>) {
        let files = self.unsaved_files();
        if files.is_empty() {
            self.switch_project_now(dir);
        } else {
            self.dialogs.push(Dialog::Unsaved { files, then: dialogs::Then::Project(dir) });
        }
    }

    fn switch_project_now(&mut self, dir: Option<PathBuf>) {
        self.save_all();
        if let Some(d) = &dir {
            self.settings.add_recent(d);
            self.settings.save();
        }
        self.project = dir;
        self.tree = None;
        self.tree_scan = None;
        self.tree_job = None;
        self.search = SearchState::default();
        self.show_side(SideView::Explorer);
    }

}

fn s_empty(p: &Path) -> bool {
    p.as_os_str().is_empty()
}

fn workspace_scratch_exists(session: &str) -> bool {
    session
        .split('\t')
        .nth(1)
        .and_then(|id| id.parse::<usize>().ok())
        .is_some_and(|id| crate::workspace::scratch_path(id).exists())
}

fn apply_editor_prefs(s: &Settings) {
    use std::sync::atomic::Ordering;
    crate::sqledit::set_font_size(s.editor_font);
    crate::sqledit::AUTO_SUGGEST.store(s.auto_suggest, Ordering::Relaxed);
    crate::sqledit::AUTO_CLOSE.store(s.auto_close, Ordering::Relaxed);
    crate::tabs::sql::AUTOSAVE.store(s.autosave, Ordering::Relaxed);
    crate::sqledit::LINT.store(s.lint, Ordering::Relaxed);
}

/// egui-Kontext fuer Aktionen, die ihn brauchen (einmal je Bild gesetzt)
static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Ordner im Datei-Explorer bzw. Datei mit dem zugehoerigen Programm oeffnen
pub fn open_path(p: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

impl eframe::App for EasyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        *CTX.lock().unwrap() = Some(ctx.clone());
        // Befehle aus dem Infobereich
        let cmds: Vec<TrayCmd> = self.tray.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        for c in cmds {
            match c {
                TrayCmd::Show => ctx.request_repaint(),
                TrayCmd::RestartServer => self.restart_server(ctx),
                TrayCmd::Quit => self.request_quit(ctx),
            }
        }
        // Dateien von einer zweiten Instanz (Doppelklick im Explorer)
        for f in platform::take_opened_files() {
            self.open_file(&f);
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        self.check_server(ctx);
        self.updater.poll(ctx, self.settings.auto_update);
        self.background_tasks(ctx);

        // Fenster schliessen = im Hintergrund weiterlaufen
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting
            && let (Some(h), Some(_)) = (self.hwnd, &self.tray) {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                platform::hide_window(h);
            }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        autoscroll::begin_frame();
        let mut actions: Vec<Action> = Vec::new();
        if self.palette.is_none() {
            self.global_keys(&ctx);
        }

        crate::terminal::set_focused(false);
        egui::Panel::top("menu")
            .frame(egui::Frame::new().fill(style::pal().face).inner_margin(egui::Margin::symmetric(4, 2)))
            .show(ui, |ui| {
                self.menu_bar(ui, &mut actions);
            });
        egui::Panel::left("activity")
            .exact_size(44.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(style::pal().activity_bg))
            .show(ui, |ui| {
                self.activity_bar(ui);
            });
        if self.sidebar {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(260.0)
                .min_size(170.0)
                .frame(egui::Frame::new().fill(style::pal().sidebar_bg))
                .show(ui, |ui| {
                    self.side_panel(ui, &mut actions);
                });
        }
        if self.terminal.open {
            self.terminal_panel(ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(style::pal().bg)).show(ui, |ui| {
            if !self.tabs.is_empty() {
                self.tab_bar(ui);
            }
            // Nach dem Schliessen der letzten Registerkarte bleibt die Flaeche einfach leer
            if self.tabs.is_empty() {
                self.empty_area(ui);
            } else {
                let active = self.active.min(self.tabs.len() - 1);
                self.active = active;
                let tab = &mut self.tabs[active];
                let mut cx = cx!(self, &mut actions);
                egui::Frame::new().fill(style::pal().bg).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    tab.ui(ui, &mut cx);
                });
            }
        });

        // Sitzung regelmaessig sichern (offene Dateien usw.)
        if self.session_saved.elapsed().as_secs() >= 5 {
            self.save_session();
        }
        self.explorer_dialogs(&ctx);
        self.handle_actions(actions);
        self.palette_ui(&ctx);
        self.dialogs_ui(&ctx);
        if self.updater.ui(&ctx) {
            self.save_all();
            self.db = None;
            self.server.stop_blocking(&self.settings.conn);
            match self.updater.launch() {
                Ok(()) => self.quit(),
                Err(e) => {
                    self.error(e);
                    self.restart_server(&ctx);
                }
            }
        }
        self.toasts_ui(&ctx);
        self.autoscroll.paint(&ctx);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        crate::sqledit::filter_input(ctx, raw_input);
        let f = self.settings.scroll_speed;
        if (f - 1.0).abs() > 0.01 {
            for e in raw_input.events.iter_mut() {
                if let egui::Event::MouseWheel { delta, modifiers, .. } = e
                    && !modifiers.ctrl && !modifiers.command {
                        *delta *= f;
                    }
            }
        }
        if self.settings.autoscroll {
            self.autoscroll.hook(ctx, raw_input);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.terminal.close_all();
        self.save_all();
        self.db = None;
        self.server.stop_blocking(&self.settings.conn);
        self.settings.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        style::pal().face.to_normalized_gamma_f32()
    }
}
