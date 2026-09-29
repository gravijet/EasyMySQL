// Hauptfenster von EasyMySQL.

use crate::db::{self, ConnInfo, Db, q};
use crate::platform::{self, TrayCmd};
use crate::server::{Server, State};
use crate::settings::Settings;
use crate::style;
use crate::tabs::{
    Action, Ctx, SchemaCache, TabView, builder::BuilderTab, data::DataTab, designer::DesignerTab,
    er::ErTab, sql::SqlTab, structure::StructureTab,
};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, ViewportCommand};
use std::sync::mpsc::Receiver;

enum Dialog {
    Message { title: String, text: String, error: bool },
    Confirm { text: String, db: Option<String>, sql: String },
    NewDatabase { name: String, collation: String },
    Connection { info: ConnInfo, save_pw: bool, error: Option<String> },
    Export { db: String, with_data: bool },
    About,
    Help,
}

pub struct EasyApp {
    settings: Settings,
    server: Server,
    db: Option<Db>,
    auto_connect: bool,
    databases: Vec<String>,
    schemas: SchemaCache,
    tabs: Vec<Box<dyn TabView>>,
    active: usize,
    current_db: String,
    status: String,
    dialogs: Vec<Dialog>,
    show_log: bool,
    hwnd: Option<isize>,
    tray: Option<(platform::Tray, Receiver<TrayCmd>)>,
    quitting: bool,
    last_state: State,
    vscode_job: Option<Receiver<Result<String, String>>>,
}

const COLLATIONS: &[&str] = &[
    "utf8mb4_unicode_ci",
    "utf8mb4_general_ci",
    "utf8mb4_german2_ci",
    "utf8mb4_bin",
    "latin1_german1_ci",
    "latin1_swedish_ci",
];

impl EasyApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        style::apply(&cc.egui_ctx);
        let settings = Settings::load();
        let server = Server::new(3306);
        let hwnd = platform::window_handle(cc);
        let tray = platform::create_tray(&cc.egui_ctx, hwnd);
        let ctx = cc.egui_ctx.clone();
        server.start(move || ctx.request_repaint());
        EasyApp {
            settings,
            server,
            db: None,
            auto_connect: true,
            databases: Vec::new(),
            schemas: SchemaCache::default(),
            tabs: Vec::new(),
            active: 0,
            current_db: String::new(),
            status: "Willkommen bei EasyMySQL.".into(),
            dialogs: Vec::new(),
            show_log: false,
            hwnd,
            tray,
            quitting: false,
            last_state: State::Stopped,
            vscode_job: None,
        }
    }

    // ------------------------------------------------------------------
    // Verbindung

    fn connect(&mut self, show_dialog_on_error: bool) {
        match Db::connect(&self.settings.conn) {
            Ok(db) => {
                self.status = format!("Verbunden mit {} (MariaDB {}).", db.info.label(), db.version);
                self.db = Some(db);
                self.refresh_all();
            }
            Err(e) => {
                self.db = None;
                self.status = format!("Verbindung fehlgeschlagen: {e}");
                if show_dialog_on_error {
                    self.dialogs.push(Dialog::Connection {
                        info: self.settings.conn.clone(),
                        save_pw: self.settings.save_password,
                        error: Some(e),
                    });
                }
            }
        }
    }

    fn refresh_all(&mut self) {
        self.schemas.clear();
        if let Some(db) = &self.db {
            match db.databases() {
                Ok(d) => self.databases = d,
                Err(e) => self.status = format!("Fehler: {e}"),
            }
        }
        if self.current_db.is_empty() || !self.databases.contains(&self.current_db) {
            self.current_db = self
                .user_databases()
                .first()
                .cloned()
                .unwrap_or_default();
        }
        let dbs: Vec<String> = self.databases.clone();
        let mut actions = Vec::new();
        for d in &dbs {
            let mut cx = Ctx {
                db: self.db.as_ref(),
                schemas: &mut self.schemas,
                databases: &self.databases,
                actions: &mut actions,
            };
            for t in self.tabs.iter_mut() {
                t.schema_changed(d, &mut cx);
            }
        }
        self.handle_actions(actions);
    }

    fn user_databases(&self) -> Vec<String> {
        self.databases
            .iter()
            .filter(|d| !db::SYSTEM_DATABASES.contains(&d.as_str()))
            .cloned()
            .collect()
    }

    // ------------------------------------------------------------------
    // Registerkarten

    fn open_tab(&mut self, tab: Box<dyn TabView>) {
        if let Some(k) = tab.key() {
            if let Some(i) = self.tabs.iter().position(|t| t.key().as_deref() == Some(&k)) {
                self.active = i;
                return;
            }
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    fn close_tab(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.tabs.remove(i);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            } else if self.active > i {
                self.active -= 1;
            }
        }
    }

    fn need_db(&mut self) -> Option<String> {
        if self.db.is_none() {
            self.error("Keine Verbindung zum Datenbankserver.");
            return None;
        }
        if self.current_db.is_empty() {
            self.dialogs.push(Dialog::Message {
                title: "Keine Datenbank".into(),
                text: "Es gibt noch keine Datenbank. Bitte zuerst eine neue Datenbank anlegen (Datenbank → Neue Datenbank).".into(),
                error: false,
            });
            return None;
        }
        Some(self.current_db.clone())
    }

    fn error(&mut self, e: impl Into<String>) {
        let e = e.into();
        self.status = format!("Fehler: {}", e.lines().next().unwrap_or(""));
        self.dialogs.push(Dialog::Message { title: "Fehler".into(), text: e, error: true });
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
                    let mut tab = SqlTab::new(db, sql);
                    if run {
                        let mut acts = Vec::new();
                        let mut cx = Ctx {
                            db: self.db.as_ref(),
                            schemas: &mut self.schemas,
                            databases: &self.databases,
                            actions: &mut acts,
                        };
                        tab.execute(&mut cx);
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
                    let mut cx = Ctx {
                        db: self.db.as_ref(),
                        schemas: &mut self.schemas,
                        databases: &self.databases,
                        actions: &mut acts,
                    };
                    for t in self.tabs.iter_mut() {
                        t.schema_changed(&d, &mut cx);
                    }
                    self.handle_actions(acts);
                }
                Action::Status(s) => self.status = s,
                Action::Error(e) => self.error(e),
                Action::Confirm { text, db, sql } => self.dialogs.push(Dialog::Confirm { text, db, sql }),
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
            if !st.is_running() && self.last_state.is_running() && self.db.is_some() {
                // Nur trennen, wenn wir mit dem lokalen Server verbunden waren
                let local = matches!(self.settings.conn.host.as_str(), "127.0.0.1" | "localhost")
                    && self.settings.conn.port == self.server.port;
                if local {
                    self.db = None;
                    self.databases.clear();
                    self.schemas.clear();
                }
            }
            if let State::Failed(e) = &st {
                self.status = format!("Server-Fehler: {e}");
                self.show_log = true;
            }
            self.last_state = st;
            ctx.request_repaint();
        }
        if self.last_state.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
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
        if let Some(h) = self.hwnd {
            platform::hide_window(h);
        }
        self.db = None;
        self.server.stop_blocking(&self.settings.conn);
        self.settings.save();
        std::process::exit(0);
    }

    // ------------------------------------------------------------------
    // Oberflaeche

    fn menu_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Datei", |ui| {
                if ui.button("Neue SQL-Abfrage          Strg+N").clicked() {
                    actions.push(Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false });
                    ui.close();
                }
                if ui.button("SQL-Datei öffnen...").clicked() {
                    ui.close();
                    self.open_sql_file(false);
                }
                ui.separator();
                if self.tray.is_some() && ui.button("Fenster ausblenden (läuft weiter)").clicked() {
                    ui.close();
                    if let Some(h) = self.hwnd {
                        platform::hide_window(h);
                    }
                }
                if ui.button("Beenden (Server stoppen)").clicked() {
                    ui.close();
                    self.quit();
                }
            });
            ui.menu_button("Datenbank", |ui| {
                let connected = self.db.is_some();
                if ui.add_enabled(connected, egui::Button::new("Neue Datenbank...")).clicked() {
                    self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                    ui.close();
                }
                if ui.add_enabled(connected, egui::Button::new("Neue Tabelle...")).clicked() {
                    if let Some(d) = self.need_db() {
                        actions.push(Action::NewTable(d));
                    }
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled(connected, egui::Button::new("SQL-Export (Sicherung)...")).clicked() {
                    if let Some(d) = self.need_db() {
                        self.dialogs.push(Dialog::Export { db: d, with_data: true });
                    }
                    ui.close();
                }
                if ui.add_enabled(connected, egui::Button::new("SQL-Datei importieren / ausführen...")).clicked() {
                    ui.close();
                    self.open_sql_file(true);
                }
                ui.separator();
                if ui.add_enabled(connected, egui::Button::new("Aktualisieren")).clicked() {
                    actions.push(Action::RefreshAll);
                    ui.close();
                }
            });
            ui.menu_button("Werkzeuge", |ui| {
                let connected = self.db.is_some();
                if ui.add_enabled(connected, egui::Button::new("ER-Diagramm (Reverse Engineering)")).clicked() {
                    if let Some(d) = self.need_db() {
                        actions.push(Action::OpenEr(d));
                    }
                    ui.close();
                }
                if ui.add_enabled(connected, egui::Button::new("Abfrage-Assistent")).clicked() {
                    actions.push(Action::OpenBuilder(Some(self.current_db.clone())));
                    ui.close();
                }
                ui.separator();
                if ui.button("In VS Code öffnen").clicked() {
                    ui.close();
                    match crate::vscode::open_workspace(&self.current_db) {
                        Ok(d) => self.status = format!("VS Code geöffnet: {}", d.display()),
                        Err(e) => self.error(e),
                    }
                }
                if ui
                    .add_enabled(self.vscode_job.is_none(), egui::Button::new("VS Code einrichten (SQLTools + GitHub Copilot)"))
                    .clicked()
                {
                    ui.close();
                    self.setup_vscode(&ctx);
                }
                ui.separator();
                if ui.button("Verbindung...").clicked() {
                    self.dialogs.push(Dialog::Connection {
                        info: self.settings.conn.clone(),
                        save_pw: self.settings.save_password,
                        error: None,
                    });
                    ui.close();
                }
                if ui.button("Server-Log").clicked() {
                    self.show_log = true;
                    ui.close();
                }
            });
            ui.menu_button("Server", |ui| {
                let st = self.server.state();
                if ui.add_enabled(!st.is_running() && !st.is_busy(), egui::Button::new("Starten")).clicked() {
                    self.start_server(&ctx);
                    ui.close();
                }
                if ui.add_enabled(st == State::Running { own: true }, egui::Button::new("Stoppen")).clicked() {
                    self.stop_server(&ctx);
                    ui.close();
                }
                if ui.add_enabled(!st.is_busy(), egui::Button::new("Neu starten")).clicked() {
                    self.restart_server(&ctx);
                    ui.close();
                }
                ui.separator();
                if let Some(p) = &self.server.paths {
                    if ui.button("Datenordner öffnen").clicked() {
                        open_folder(&p.base);
                        ui.close();
                    }
                }
                if ui.button("Server-Log anzeigen").clicked() {
                    self.show_log = true;
                    ui.close();
                }
            });
            ui.menu_button("Hilfe", |ui| {
                if ui.button("Kurzanleitung").clicked() {
                    self.dialogs.push(Dialog::Help);
                    ui.close();
                }
                if ui.button("Über EasyMySQL").clicked() {
                    self.dialogs.push(Dialog::About);
                    ui.close();
                }
            });
        });
    }

    fn setup_vscode(&mut self, ctx: &egui::Context) {
        if crate::vscode::find_vscode().is_none() {
            self.dialogs.push(Dialog::Message {
                title: "Visual Studio Code".into(),
                text: crate::vscode::NOT_FOUND.into(),
                error: false,
            });
            let _ = open_url("https://code.visualstudio.com/download");
            return;
        }
        let _ = crate::vscode::prepare_workspace(&self.current_db);
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::vscode::install_extensions());
            ctx.request_repaint();
        });
        self.vscode_job = Some(rx);
        self.status = "VS-Code-Erweiterungen werden installiert ...".into();
    }

    fn poll_vscode(&mut self) {
        let Some(rx) = &self.vscode_job else { return };
        let Ok(res) = rx.try_recv() else { return };
        self.vscode_job = None;
        match res {
            Ok(report) => {
                self.status = "VS Code ist eingerichtet.".into();
                self.dialogs.push(Dialog::Message {
                    title: "VS Code eingerichtet".into(),
                    text: format!(
                        "{report}\nWichtig: Beim ersten Öffnen fragt VS Code, ob Sie dem Ordner vertrauen. \
                         Bitte \"Ja, ich vertraue den Autoren\" (bzw. \"Trust\") wählen, sonst sind die Erweiterungen aus.\n\n\
                         In VS Code gibt es dann links das Datenbank-Symbol (SQLTools) mit der \
                         Verbindung \"EasyMySQL\". In .sql-Dateien führt Strg+E Strg+E die Abfrage aus.\n\n\
                         GitHub Copilot: in VS Code unten rechts auf das Copilot-Symbol klicken und mit dem \
                         GitHub-Konto anmelden. Danach schlägt Copilot beim Tippen SQL vor.\n\n\
                         Ordner für Abfragen: {}",
                        crate::vscode::workspace_dir().display()
                    ),
                    error: false,
                });
                let _ = crate::vscode::open_workspace(&self.current_db);
            }
            Err(e) => self.error(e),
        }
    }

    fn open_sql_file(&mut self, run: bool) {
        if let Some(p) = rfd::FileDialog::new().add_filter("SQL-Dateien", &["sql", "txt"]).pick_file() {
            match std::fs::read(&p) {
                Ok(b) => {
                    let mut tab = SqlTab::new(
                        if self.current_db.is_empty() { None } else { Some(self.current_db.clone()) },
                        String::from_utf8_lossy(&b).into_owned(),
                    );
                    tab.file = Some(p);
                    if run {
                        let mut acts = Vec::new();
                        let mut cx = Ctx {
                            db: self.db.as_ref(),
                            schemas: &mut self.schemas,
                            databases: &self.databases,
                            actions: &mut acts,
                        };
                        tab.execute(&mut cx);
                    }
                    self.open_tab(Box::new(tab));
                }
                Err(e) => self.error(e.to_string()),
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal_wrapped(|ui| {
            let connected = self.db.is_some();
            if ui.button("Neue Abfrage").clicked() {
                actions.push(Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false });
            }
            let can_run = self.tabs.get(self.active).map(|t| !t.busy()).unwrap_or(false);
            if ui.add_enabled(can_run, egui::Button::new("▶ Ausführen (F5)")).clicked() {
                self.execute_active(actions);
            }
            ui.separator();
            ui.add_enabled_ui(connected, |ui| {
                if ui.button("Neue Datenbank").clicked() {
                    self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                }
                if ui.button("Neue Tabelle").clicked() {
                    if let Some(d) = self.need_db() {
                        actions.push(Action::NewTable(d));
                    }
                }
                ui.separator();
                if ui.button("ER-Diagramm").on_hover_text("Reverse Engineering der aktuellen Datenbank").clicked() {
                    if let Some(d) = self.need_db() {
                        actions.push(Action::OpenEr(d));
                    }
                }
                if ui.button("Abfrage-Assistent").on_hover_text("Abfragen grafisch erstellen").clicked() {
                    actions.push(Action::OpenBuilder(Some(self.current_db.clone())));
                }
                ui.separator();
                if ui.button("Aktualisieren").clicked() {
                    actions.push(Action::RefreshAll);
                }
            });
            ui.separator();
            ui.label("Aktuelle Datenbank:");
            let user_dbs = self.user_databases();
            let mut cur = self.current_db.clone();
            crate::tabs::db_combo(ui, "curdb", &user_dbs, &mut cur);
            self.current_db = cur;
        });
    }

    fn execute_active(&mut self, actions: &mut Vec<Action>) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            let mut cx = Ctx {
                db: self.db.as_ref(),
                schemas: &mut self.schemas,
                databases: &self.databases,
                actions,
            };
            t.execute(&mut cx);
        }
    }

    fn tree(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Objekt-Explorer").strong());
        });
        ui.separator();
        let Some(dbc) = &self.db else {
            ui.label("Nicht verbunden.");
            if ui.button("Verbinden...").clicked() {
                self.dialogs.push(Dialog::Connection {
                    info: self.settings.conn.clone(),
                    save_pw: self.settings.save_password,
                    error: None,
                });
            }
            return;
        };
        ui.label(RichText::new(dbc.info.label()).small());
        let show_sys = self.settings.show_system_dbs;
        let mut new_db = false;
        let mut dialogs = Vec::new();
        egui::ScrollArea::both().id_salt("tree").auto_shrink([false, false]).max_height(ui.available_height() - 40.0).show(ui, |ui| {
            for d in &self.databases {
                let sys = db::SYSTEM_DATABASES.contains(&d.as_str());
                if sys && !show_sys {
                    continue;
                }
                let title = if *d == self.current_db { RichText::new(d).strong() } else { RichText::new(d) };
                let resp = egui::CollapsingHeader::new(title).id_salt(("db", d)).show(ui, |ui| {
                    match self.schemas.get(Some(dbc), d) {
                        Some(schema) => {
                            if schema.tables.is_empty() {
                                ui.label(RichText::new("(keine Tabellen)").color(style::NULL_TEXT));
                            }
                            for t in &schema.tables {
                                let label = if t.is_view { format!("{} (Sicht)", t.name) } else { t.name.clone() };
                                let h = egui::CollapsingHeader::new(label).id_salt(("t", d, &t.name)).show(ui, |ui| {
                                    for c in &t.columns {
                                        let mut txt = format!("{}  {}", c.name, c.col_type);
                                        if c.is_pk() {
                                            txt = format!("{txt}  [PK]");
                                        } else if schema.is_fk_column(&t.name, &c.name) {
                                            txt = format!("{txt}  [FK]");
                                        }
                                        ui.label(RichText::new(txt).small());
                                    }
                                });
                                let hr = h.header_response;
                                if hr.clicked() {
                                    self.current_db = d.clone();
                                }
                                if hr.double_clicked() {
                                    actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                                }
                                hr.context_menu(|ui| {
                                    if ui.button("Daten anzeigen / bearbeiten").clicked() {
                                        actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                                        ui.close();
                                    }
                                    if ui.button("Struktur").clicked() {
                                        actions.push(Action::OpenStructure { db: d.clone(), table: t.name.clone() });
                                        ui.close();
                                    }
                                    if ui.button("SELECT-Abfrage").clicked() {
                                        actions.push(Action::OpenSql {
                                            db: Some(d.clone()),
                                            sql: format!("SELECT * FROM {} LIMIT 100;", q(&t.name)),
                                            run: true,
                                        });
                                        ui.close();
                                    }
                                    ui.separator();
                                    if !t.is_view && ui.button("Tabelle leeren...").clicked() {
                                        actions.push(Action::Confirm {
                                            text: format!("Alle Zeilen der Tabelle \"{}\" löschen?", t.name),
                                            db: Some(d.clone()),
                                            sql: format!("DELETE FROM {}", q(&t.name)),
                                        });
                                        ui.close();
                                    }
                                    if ui.button(if t.is_view { "Sicht löschen..." } else { "Tabelle löschen..." }).clicked() {
                                        actions.push(Action::Confirm {
                                            text: format!("\"{}\" mit allen Daten endgültig löschen?", t.name),
                                            db: Some(d.clone()),
                                            sql: format!("DROP {} {}", if t.is_view { "VIEW" } else { "TABLE" }, q(&t.name)),
                                        });
                                        ui.close();
                                    }
                                });
                            }
                        }
                        None => {
                            ui.label(RichText::new("(kein Zugriff)").color(style::ERROR_TEXT));
                        }
                    }
                });
                let hr = resp.header_response;
                if hr.clicked() {
                    self.current_db = d.clone();
                }
                hr.context_menu(|ui| {
                    if ui.button("Neue Tabelle...").clicked() {
                        actions.push(Action::NewTable(d.clone()));
                        ui.close();
                    }
                    if ui.button("ER-Diagramm").clicked() {
                        actions.push(Action::OpenEr(d.clone()));
                        ui.close();
                    }
                    if ui.button("Abfrage-Assistent").clicked() {
                        actions.push(Action::OpenBuilder(Some(d.clone())));
                        ui.close();
                    }
                    if ui.button("Neue SQL-Abfrage").clicked() {
                        actions.push(Action::OpenSql { db: Some(d.clone()), sql: String::new(), run: false });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("SQL-Export...").clicked() {
                        dialogs.push(Dialog::Export { db: d.clone(), with_data: true });
                        ui.close();
                    }
                    if !sys && ui.button("Datenbank löschen...").clicked() {
                        actions.push(Action::Confirm {
                            text: format!("Datenbank \"{d}\" mit ALLEN Tabellen und Daten endgültig löschen?"),
                            db: None,
                            sql: format!("DROP DATABASE {}", q(d)),
                        });
                        ui.close();
                    }
                });
            }
            ui.add_space(6.0);
            if ui.button("+ Neue Datenbank").clicked() {
                new_db = true;
            }
        });
        self.dialogs.extend(dialogs);
        if new_db {
            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
        }
        ui.separator();
        if ui.checkbox(&mut self.settings.show_system_dbs, "Systemdatenbanken zeigen").changed() {
            self.settings.save();
        }
    }

    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        egui::ScrollArea::horizontal().id_salt("tabstrip").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, t) in self.tabs.iter().enumerate() {
                    let sel = i == self.active;
                    let frame = egui::Frame::new()
                        .fill(if sel { egui::Color32::WHITE } else { style::FACE })
                        .stroke(egui::Stroke::new(1.0, style::SHADOW))
                        .inner_margin(egui::Margin::symmetric(6, 2));
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut title = t.title();
                            if t.busy() {
                                title.push_str(" …");
                            }
                            let txt = if sel { RichText::new(title).strong() } else { RichText::new(title) };
                            let r = ui.add(egui::Label::new(txt).sense(egui::Sense::click()));
                            if r.clicked() {
                                self.active = i;
                            }
                            if r.middle_clicked() {
                                close = Some(i);
                            }
                            if ui.add(egui::Button::new("×").small().frame(false)).on_hover_text("Schließen (Strg+W)").clicked() {
                                close = Some(i);
                            }
                        });
                    });
                }
            });
        });
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    fn start_page(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space(10.0);
        ui.heading("EasyMySQL");
        ui.label("MariaDB-Datenbankserver und grafische Verwaltung in einem Programm.");
        ui.add_space(10.0);
        style::group_frame().show(ui, |ui| {
            ui.set_width(560.0);
            egui::Grid::new("startgrid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Server:");
                let st = self.server.state();
                let color = if st.is_running() { style::OK_TEXT } else if matches!(st, State::Failed(_) | State::NotFound) { style::ERROR_TEXT } else { egui::Color32::BLACK };
                ui.label(RichText::new(format!("{} (Port {})", st.text(), self.server.port)).color(color));
                ui.end_row();
                ui.label("Verbindung:");
                match &self.db {
                    Some(d) => ui.label(format!("{} – MariaDB {}", d.info.label(), d.version)),
                    None => ui.label("nicht verbunden"),
                };
                ui.end_row();
                if let Some(p) = &self.server.paths {
                    ui.label("Datenordner:");
                    ui.label(p.data.display().to_string());
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let st = self.server.state();
                if !st.is_running() && !st.is_busy() && ui.button("Server starten").clicked() {
                    self.start_server(&ctx);
                }
                if self.db.is_none() && st.is_running() && ui.button("Verbinden").clicked() {
                    self.connect(true);
                }
                if ui.button("Server-Log").clicked() {
                    self.show_log = true;
                }
            });
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Kommandozeile").strong());
        ui.label("Solange EasyMySQL geöffnet ist (auch im Hintergrund), kann in der Eingabeaufforderung (cmd) gearbeitet werden:");
        style::sunken_frame().show(ui, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("mysql -u root").monospace());
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Erste Schritte").strong());
        ui.label("1. Links im Objekt-Explorer eine Datenbank aufklappen oder oben \"Neue Datenbank\" wählen.");
        ui.label("2. Mit \"Neue Tabelle\" Tabellen grafisch anlegen, per Doppelklick Daten ansehen und bearbeiten.");
        ui.label("3. \"ER-Diagramm\" zeigt Tabellen und Beziehungen (Reverse Engineering).");
        ui.label("4. \"Neue Abfrage\" für SQL oder \"Abfrage-Assistent\" für Abfragen ohne SQL.");
    }

    fn dialogs_ui(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.dialogs.last_mut() else { return };
        let mut close = false;
        let mut run_sql: Option<(Option<String>, String)> = None;
        let mut do_connect: Option<(ConnInfo, bool)> = None;
        let mut export: Option<(String, bool)> = None;
        let mut create_db: Option<(String, String)> = None;

        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(14));
        let modal = egui::Modal::new(egui::Id::new("dialog")).frame(frame).show(ctx, |ui| {
            ui.set_max_width(520.0);
            match dlg {
                Dialog::Message { title, text, error } => {
                    ui.label(RichText::new(title.as_str()).strong());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        let t = RichText::new(text.as_str());
                        ui.label(if *error { t.color(style::ERROR_TEXT) } else { t });
                    });
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        close = true;
                    }
                }
                Dialog::Confirm { text, db, sql } => {
                    ui.label(RichText::new("Bitte bestätigen").strong());
                    ui.add_space(4.0);
                    ui.label(text.as_str());
                    ui.add_space(4.0);
                    ui.label(RichText::new(sql.as_str()).monospace().small());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Ja").clicked() {
                            run_sql = Some((db.clone(), sql.clone()));
                            close = true;
                        }
                        if ui.button("Nein").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::NewDatabase { name, collation } => {
                    ui.label(RichText::new("Neue Datenbank").strong());
                    ui.add_space(4.0);
                    egui::Grid::new("newdb").num_columns(2).show(ui, |ui| {
                        ui.label("Name:");
                        let r = ui.text_edit_singleline(name);
                        r.request_focus();
                        ui.end_row();
                        ui.label("Sortierung:");
                        let items: Vec<String> = COLLATIONS.iter().map(|s| s.to_string()).collect();
                        crate::tabs::str_combo(ui, "coll", &items, collation, 180.0);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Erstellen").clicked() || (ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty()) {
                            create_db = Some((name.trim().to_string(), collation.clone()));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Connection { info, save_pw, error } => {
                    ui.label(RichText::new("Verbindung zum Datenbankserver").strong());
                    if let Some(e) = error {
                        ui.label(RichText::new(e.as_str()).color(style::ERROR_TEXT));
                    }
                    ui.add_space(4.0);
                    egui::Grid::new("conn").num_columns(2).show(ui, |ui| {
                        ui.label("Server:");
                        ui.text_edit_singleline(&mut info.host);
                        ui.end_row();
                        ui.label("Port:");
                        let mut p = info.port.to_string();
                        if ui.text_edit_singleline(&mut p).changed() {
                            info.port = p.parse().unwrap_or(info.port);
                        }
                        ui.end_row();
                        ui.label("Benutzer:");
                        ui.text_edit_singleline(&mut info.user);
                        ui.end_row();
                        ui.label("Passwort:");
                        ui.add(egui::TextEdit::singleline(&mut info.password).password(true));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(save_pw, "Passwort speichern");
                        ui.end_row();
                    });
                    ui.label(RichText::new("Standard: 127.0.0.1, Port 3306, Benutzer root, kein Passwort").small().color(style::NULL_TEXT));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Verbinden").clicked() {
                            do_connect = Some((info.clone(), *save_pw));
                        }
                        if ui.button("Standard").clicked() {
                            *info = ConnInfo::default();
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Export { db, with_data } => {
                    ui.label(RichText::new("SQL-Export").strong());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Datenbank:");
                        crate::tabs::db_combo(ui, "expdb", &self.databases, db);
                    });
                    ui.checkbox(with_data, "Mit Daten (INSERT-Anweisungen)");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Speichern unter...").clicked() {
                            export = Some((db.clone(), *with_data));
                        }
                        if ui.button("Abbrechen").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::About => {
                    ui.label(RichText::new(format!("EasyMySQL {}", env!("CARGO_PKG_VERSION"))).strong());
                    ui.add_space(4.0);
                    ui.label("MariaDB-Server und Datenbankverwaltung für Windows.");
                    ui.label("Geschrieben in Rust (egui). Enthält MariaDB (GPL v2).");
                    if let Some(d) = &self.db {
                        ui.label(format!("Server-Version: {}", d.version));
                    }
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
                Dialog::Help => {
                    ui.label(RichText::new("Kurzanleitung").strong());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        ui.label(HELP_TEXT);
                    });
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
            }
        });
        if modal.should_close() && !matches!(self.dialogs.last(), Some(Dialog::Confirm { .. })) {
            close = true;
        }

        if let Some((db, sql)) = run_sql {
            if let Some(dbc) = &self.db {
                let res = match &db {
                    Some(d) => dbc.exec_in(d, &sql),
                    None => dbc.exec(&sql, ()),
                };
                match res {
                    Ok(n) => {
                        self.status = format!("Ausgeführt ({n} Zeile(n) betroffen).");
                        self.dialogs.pop();
                        close = false;
                        match db {
                            Some(d) => self.handle_actions(vec![Action::SchemaChanged(d)]),
                            None => self.handle_actions(vec![Action::RefreshAll]),
                        }
                    }
                    Err(e) => {
                        self.dialogs.pop();
                        close = false;
                        self.error(e);
                    }
                }
            }
        }
        if let Some((name, coll)) = create_db {
            if let Some(dbc) = &self.db {
                let charset = coll.split('_').next().unwrap_or("utf8mb4").to_string();
                let sql = format!("CREATE DATABASE {} CHARACTER SET {} COLLATE {}", q(&name), charset, coll);
                match dbc.exec(&sql, ()) {
                    Ok(_) => {
                        self.dialogs.pop();
                        self.status = format!("Datenbank {name} erstellt.");
                        self.refresh_all();
                        self.current_db = name;
                    }
                    Err(e) => self.error(e),
                }
            }
        }
        if let Some((info, save_pw)) = do_connect {
            self.settings.conn = info;
            self.settings.save_password = save_pw;
            self.settings.save();
            self.dialogs.pop();
            self.connect(true);
        }
        if let Some((d, with_data)) = export {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("SQL-Datei", &["sql"])
                .set_file_name(format!("{d}.sql"))
                .save_file()
            {
                let res = self.db.as_ref().map(|dbc| dbc.dump(&d, with_data));
                match res {
                    Some(Ok(text)) => match std::fs::write(&p, text) {
                        Ok(_) => {
                            self.status = format!("Export gespeichert: {}", p.display());
                            self.dialogs.pop();
                        }
                        Err(e) => self.error(e.to_string()),
                    },
                    Some(Err(e)) => self.error(e),
                    None => {}
                }
            }
        }
        if close {
            self.dialogs.pop();
        }
    }

    fn log_window(&mut self, ctx: &egui::Context) {
        if !self.show_log {
            return;
        }
        let mut open = true;
        let lines = self.server.log_lines();
        egui::Window::new("Server-Log")
            .open(&mut open)
            .default_size([640.0, 360.0])
            .resizable(true)
            .show(ctx, |ui| {
                if let Some(p) = &self.server.paths {
                    ui.label(RichText::new(format!("Programme: {}", p.bin.display())).small());
                    ui.label(RichText::new(format!("Daten: {}", p.data.display())).small());
                } else {
                    ui.label(RichText::new("MariaDB wurde nicht gefunden. Bitte EasyMySQL mit dem Setup installieren.").color(style::ERROR_TEXT));
                }
                ui.horizontal(|ui| {
                    if ui.button("Kopieren").clicked() {
                        ui.ctx().copy_text(lines.join("\n"));
                    }
                });
                style::sunken_frame().show(ui, |ui| {
                    egui::ScrollArea::both().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                        for l in &lines {
                            ui.label(RichText::new(l).monospace().small());
                        }
                    });
                });
            });
        self.show_log = open;
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let st = self.server.state();
            let color = if st.is_running() {
                style::OK_TEXT
            } else if matches!(st, State::Failed(_) | State::NotFound) {
                style::ERROR_TEXT
            } else {
                egui::Color32::BLACK
            };
            if st.is_busy() {
                ui.spinner();
            }
            ui.label(RichText::new(format!("Server: {}", st.text())).color(color));
            ui.separator();
            match &self.db {
                Some(d) => ui.label(format!("{} (MariaDB {})", d.info.label(), d.version)),
                None => ui.label("nicht verbunden"),
            };
            ui.separator();
            ui.add(egui::Label::new(self.status.as_str()).truncate());
        });
    }
}

const HELP_TEXT: &str = "\
EasyMySQL startet beim Öffnen automatisch den MariaDB-Server (Port 3306).
Solange das Programm läuft – auch im Hintergrund – kann man in der
Eingabeaufforderung (cmd) mit  mysql -u root  arbeiten.

Fenster schließen (X): EasyMySQL läuft im Infobereich (neben der Uhr) weiter.
Zum richtigen Beenden: Rechtsklick auf das Symbol → Beenden, oder Datei → Beenden.
Dabei wird auch der Datenbankserver gestoppt.

Objekt-Explorer (links):
  • Datenbank aufklappen zeigt Tabellen, Tabelle aufklappen zeigt Spalten.
  • Doppelklick auf eine Tabelle öffnet die Daten.
  • Rechtsklick öffnet ein Menü (Struktur, löschen, Export, ...).

Daten bearbeiten:
  • Doppelklick auf eine Zelle, Wert eingeben, Enter speichert sofort.
  • \"+ Neue Zeile\" fügt Datensätze ein.

SQL-Abfrage:
  • SQL eingeben, F5 oder Strg+Enter führt aus.
  • Beim Tippen erscheinen Vorschläge (Tabellen, Spalten, Befehle),
    Strg+Leertaste öffnet sie jederzeit, Enter/Tab übernimmt.
  • \"Formatieren\" (Strg+Umschalt+F) macht das SQL übersichtlich.
  • \"In VS Code öffnen\" bearbeitet die Abfrage in Visual Studio Code
    (mit SQLTools und GitHub Copilot, siehe Werkzeuge → VS Code einrichten).
  • Ist Text markiert, wird nur dieser ausgeführt.
  • Mehrere Anweisungen mit ; trennen.

ER-Diagramm (Reverse Engineering):
  • Liest alle Tabellen, Spalten und Fremdschlüssel und zeichnet sie.
  • Linien zeigen Beziehungen (Krähenfuß = \"viele\"-Seite).
  • Als SVG speichern oder als SQL-Skript (CREATE TABLE ...) ausgeben.

Abfrage-Assistent:
  • Haupttabelle wählen, Spalten anhaken, verknüpfte Tabellen hinzufügen,
    Bedingungen und Sortierung einstellen – das SQL wird automatisch erzeugt.

Tastenkürzel: F5 Ausführen, Strg+N neue Abfrage, Strg+W Registerkarte schließen.";

fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd").args(["/C", "start", "", url]).creation_flags(0x0800_0000).spawn()
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()
    }
}

fn open_folder(p: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

impl eframe::App for EasyApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Befehle aus dem Infobereich
        let cmds: Vec<TrayCmd> = self
            .tray
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for c in cmds {
            match c {
                TrayCmd::Show => ctx.request_repaint(),
                TrayCmd::RestartServer => self.restart_server(ctx),
                TrayCmd::Quit => self.quit(),
            }
        }
        self.check_server(ctx);
        self.poll_vscode();

        // Fenster schliessen = im Hintergrund weiterlaufen
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if let (Some(h), Some(_)) = (self.hwnd, &self.tray) {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                platform::hide_window(h);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut actions: Vec<Action> = Vec::new();

        // Tastenkuerzel
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5)) {
            self.execute_active(&mut actions);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::N))) {
            actions.push(Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false });
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::W))) {
            let a = self.active;
            self.close_tab(a);
        }

        egui::Panel::top("menu").show(ui, |ui| {
            self.menu_bar(ui, &mut actions);
        });
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.toolbar(ui, &mut actions);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            self.status_bar(ui);
        });
        egui::Panel::left("tree")
            .resizable(true)
            .default_size(240.0)
            .min_size(150.0)
            .show(ui, |ui| {
                self.tree(ui, &mut actions);
            });
        egui::CentralPanel::default().show(ui, |ui| {
            if self.tabs.is_empty() {
                self.start_page(ui);
            } else {
                self.tab_strip(ui);
                ui.add_space(2.0);
                let active = self.active.min(self.tabs.len() - 1);
                let tab = &mut self.tabs[active];
                let mut cx = Ctx {
                    db: self.db.as_ref(),
                    schemas: &mut self.schemas,
                    databases: &self.databases,
                    actions: &mut actions,
                };
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(0xFA, 0xFA, 0xFA))
                    .stroke(egui::Stroke::new(1.0, style::SHADOW))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        tab.ui(ui, &mut cx);
                    });
            }
        });

        self.handle_actions(actions);
        self.log_window(&ctx);
        self.dialogs_ui(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.db = None;
        self.server.stop_blocking(&self.settings.conn);
        self.settings.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        style::FACE.to_normalized_gamma_f32()
    }
}
