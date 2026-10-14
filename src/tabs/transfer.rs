// Import und Export in einer Registerkarte: Daten aus anderen Datenbanken (SQLite, MongoDB, CSV, Redis,
// MariaDB/MySQL, PostgreSQL) uebernehmen und in diese Formate bzw. auf andere Server ausgeben.
// Jede Quelle und jedes Ziel gibt es als Datei und, wo es passt, ueber eine Verbindung zu einem anderen
// Rechner (Web-Adresse, SSH, Server mit TLS und SSH-Tunnel).

use super::{Action, Ctx, TabView};
use crate::icons::{self, Icon};
use crate::ssh::SshConfig;
use crate::style;
use crate::transfer::export::{self, Format};
use crate::transfer::import::write::{self, Options, Progress};
use crate::transfer::import::{self, RedisConf, ServerKind, Source, TableScan};
use crate::transfer::remote::{self, Fetched, Location};
use crate::transfer::server::{Link, ServerConf, Tls};
use crate::widgets::{self, chips, number_field, password_field, row, section, text_field};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

fn t<'a>(de: &'a str, en: &'a str) -> &'a str {
    crate::i18n::tr(de, en)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Import,
    Export,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Sqlite,
    Mongo,
    Csv,
    Redis,
    Mysql,
    Postgres,
}

impl Kind {
    const ALL: [Kind; 6] = [Kind::Sqlite, Kind::Mongo, Kind::Csv, Kind::Redis, Kind::Mysql, Kind::Postgres];

    fn label(self) -> &'static str {
        match self {
            Kind::Sqlite => "SQLite",
            Kind::Mongo => "MongoDB",
            Kind::Csv => "CSV",
            Kind::Redis => "Redis",
            Kind::Mysql => "MariaDB / MySQL",
            Kind::Postgres => "PostgreSQL",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Kind::Sqlite => &["sqlite", "sqlite3", "db", "db3"],
            Kind::Mongo => &["json", "jsonl", "ndjson", "bson"],
            Kind::Csv => &["csv", "tsv", "txt"],
            Kind::Redis => &["rdb"],
            _ => &[],
        }
    }

    /// Endung fuer heruntergeladene Dateien ohne Endung
    fn default_ext(self) -> &'static str {
        match self {
            Kind::Sqlite => "sqlite",
            Kind::Mongo => "json",
            Kind::Csv => "csv",
            _ => "rdb",
        }
    }

    fn wheres(self) -> &'static [Where] {
        match self {
            Kind::Sqlite | Kind::Csv => &[Where::Local, Where::Web, Where::Ssh],
            Kind::Mongo => &[Where::Local, Where::Web, Where::Ssh, Where::Server],
            Kind::Redis => &[Where::Local, Where::Web, Where::Ssh, Where::Server],
            Kind::Mysql | Kind::Postgres => &[Where::Server],
        }
    }

    fn from_path(p: &std::path::Path) -> Option<Kind> {
        let ext = p.extension()?.to_string_lossy().to_lowercase();
        [Kind::Sqlite, Kind::Mongo, Kind::Csv, Kind::Redis].into_iter().find(|k| k.extensions().contains(&ext.as_str()))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Where {
    Local,
    Web,
    Ssh,
    Server,
}

impl Where {
    fn label(self, kind: Kind) -> String {
        match self {
            Where::Local if kind == Kind::Redis => t("Redis-Datei (RDB)", "Redis file (RDB)").into(),
            Where::Local => t("Datei auf diesem Rechner", "File on this computer").into(),
            Where::Web => t("Datei aus dem Netz (URL)", "File from the web (URL)").into(),
            Where::Ssh => t("Datei auf einem Server (SSH)", "File on a server (SSH)").into(),
            Where::Server => t("Server", "Server").into(),
        }
    }
}

/// Ziel des Exports
#[derive(Clone, Copy, PartialEq)]
enum Target {
    File(Format),
    Mysql,
    Postgres,
    Redis,
}

struct Entry {
    scan: TableScan,
    on: bool,
}

struct Running {
    rx: Receiver<Result<u64, String>>,
    progress: Arc<Progress>,
}

/// Ergebnis des Vorbereitens einer Quelle
struct Prepared {
    source: Option<(Source, Vec<TableScan>)>,
    fetched: Option<Fetched>,
    link: Option<Link>,
    databases: Option<Vec<String>>,
}

struct Prepare {
    rx: Receiver<Result<Prepared, String>>,
    cancel: Arc<AtomicBool>,
}

struct Exporting {
    rx: Receiver<Result<String, String>>,
    progress: Arc<Progress>,
}

pub struct TransferTab {
    mode: Mode,
    // --- Import
    kind: Kind,
    place: Where,
    files: Vec<PathBuf>,
    url: String,
    ssh: SshConfig,
    ssh_paths: String,
    mysql: ServerConf,
    postgres: ServerConf,
    redis: ServerConf,
    mongo: ServerConf,
    redis_db: u32,
    databases: Vec<String>,
    link: Option<Link>,
    fetched: Option<Fetched>,
    preparing: Option<Prepare>,
    source: Option<Source>,
    entries: Vec<Entry>,
    error: Option<String>,
    database: String,
    new_db: bool,
    new_name: String,
    replace: bool,
    job: Option<Running>,
    log: Vec<(bool, String)>,
    done: Option<u64>,
    // --- Export
    exp_db: String,
    exp_only: Option<Vec<String>>,
    exp_target: Target,
    exp_data: bool,
    exp_replace: bool,
    exp_mysql: ServerConf,
    exp_postgres: ServerConf,
    exp_redis: ServerConf,
    exp_redis_db: u32,
    exporting: Option<Exporting>,
    exp_result: Option<Result<String, String>>,
}

impl TransferTab {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            kind: Kind::Sqlite,
            place: Where::Local,
            files: Vec::new(),
            url: String::new(),
            ssh: SshConfig { enabled: true, ..SshConfig::default() },
            ssh_paths: String::new(),
            mysql: ServerConf::new(3306),
            postgres: ServerConf::new(5432),
            redis: ServerConf::new(6379),
            mongo: ServerConf::new(27017),
            redis_db: 0,
            databases: Vec::new(),
            link: None,
            fetched: None,
            preparing: None,
            source: None,
            entries: Vec::new(),
            error: None,
            database: String::new(),
            new_db: true,
            new_name: String::new(),
            replace: false,
            job: None,
            log: Vec::new(),
            done: None,
            exp_db: String::new(),
            exp_only: None,
            exp_target: Target::File(Format::Sql),
            exp_data: true,
            exp_replace: false,
            exp_mysql: ServerConf::new(3306),
            exp_postgres: ServerConf::new(5432),
            exp_redis: ServerConf::new(6379),
            exp_redis_db: 0,
            exporting: None,
            exp_result: None,
        }
    }

    /// Export einer Datenbank (optional nur einzelne Tabellen) vorbereiten
    pub fn export(db: String, only: Option<Vec<String>>, format: Format) -> Self {
        let mut tab = Self::new(Mode::Export);
        tab.exp_db = db;
        tab.exp_only = only;
        tab.exp_target = Target::File(format);
        tab
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    fn reset_result(&mut self) {
        self.source = None;
        self.entries.clear();
        self.error = None;
        self.log.clear();
        self.done = None;
    }

    fn conf_mut(&mut self) -> &mut ServerConf {
        match self.kind {
            Kind::Postgres => &mut self.postgres,
            Kind::Redis => &mut self.redis,
            Kind::Mongo => &mut self.mongo,
            _ => &mut self.mysql,
        }
    }

    // ------------------------------------------------------------------
    // Import: Quelle vorbereiten (holen, verbinden, lesen)

    fn start_prepare(&mut self, ctx: &egui::Context, use_link: bool) {
        self.reset_result();
        if let Some(p) = &self.preparing {
            p.cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (kind, place) = (self.kind, self.place);
        let (files, url, ssh, paths) = (self.files.clone(), self.url.clone(), self.ssh.clone(), self.ssh_paths.clone());
        let conf = self.conf_mut().clone();
        let (redis_db, link) = (self.redis_db, if use_link { self.link.clone() } else { None });
        let (c, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(kind, place, files, url, ssh, paths, conf, redis_db, link, &c))).unwrap_or_else(|_| Err(crate::i18n::text("Die Quelle konnte nicht gelesen werden.").into()));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.preparing = Some(Prepare { rx, cancel });
    }

    fn set_files(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        self.new_name = files[0].file_stem().map(|s| sanitize(&s.to_string_lossy())).unwrap_or_default();
        self.files = files;
        self.place = Where::Local;
        self.start_prepare(ctx, false);
    }

    fn choose_files(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new();
        dlg = dlg.add_filter(self.kind.label(), self.kind.extensions()).add_filter(t("Alle Dateien", "All files"), &["*"]);
        let picked = if matches!(self.kind, Kind::Sqlite | Kind::Redis) { dlg.pick_file().map(|f| vec![f]) } else { dlg.pick_files() };
        if let Some(files) = picked {
            self.set_files(ctx, files);
        }
    }

    fn poll(&mut self, cx: &mut Ctx) {
        if let Some(p) = &self.preparing
            && let Ok(r) = p.rx.try_recv()
        {
            self.preparing = None;
            match r {
                Ok(pre) => {
                    if pre.fetched.is_some() {
                        self.fetched = pre.fetched;
                    }
                    if pre.link.is_some() {
                        self.link = pre.link;
                    }
                    if let Some(dbs) = pre.databases {
                        if !dbs.contains(&self.conf_mut().database) {
                            self.conf_mut().database.clear();
                        }
                        self.databases = dbs;
                    }
                    if let Some((src, tables)) = pre.source {
                        if tables.is_empty() {
                            self.error = Some(crate::i18n::text("Keine Tabellen gefunden.").into());
                        }
                        if self.new_name.is_empty() {
                            self.new_name = match self.kind {
                                Kind::Redis => "redis".into(),
                                Kind::Mysql | Kind::Postgres | Kind::Mongo => sanitize(&self.conf_mut().database),
                                _ => String::new(),
                            };
                        }
                        self.entries = tables.into_iter().map(|scan| Entry { scan, on: true }).collect();
                        self.source = Some(src);
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(job) = &self.job
            && let Ok(r) = job.rx.try_recv()
        {
            self.log = job.progress.log.lock().unwrap().clone();
            self.job = None;
            match r {
                Ok(n) => {
                    self.done = Some(n);
                    cx.status(crate::tr_format!("{n} Zeile(n) importiert.", "{n} row(s) imported."));
                    cx.actions.push(Action::RefreshAll);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(job) = &self.exporting
            && let Ok(r) = job.rx.try_recv()
        {
            self.log = job.progress.log.lock().unwrap().clone();
            self.exporting = None;
            if let Ok(msg) = &r {
                cx.status(crate::tr_format!("Exportiert: {msg}", "Exported: {msg}"));
            }
            self.exp_result = Some(r);
        }
    }

    fn start_import(&mut self, cx: &mut Ctx) {
        let (Some(dbc), Some(source)) = (cx.db, self.source.clone()) else { return };
        let tables: Vec<TableScan> = self.entries.iter().filter(|e| e.on).map(|e| e.scan.clone()).collect();
        let mut names: Vec<String> = tables.iter().map(|t| t.name.trim().to_lowercase()).collect();
        names.sort();
        if tables.iter().any(|t| t.name.trim().is_empty()) || names.windows(2).any(|w| w[0] == w[1]) {
            self.error = Some(crate::i18n::text("Jede Tabelle braucht einen eigenen Namen.").into());
            return;
        }
        let database = if self.new_db { self.new_name.trim().to_string() } else { self.database.clone() };
        if database.is_empty() {
            self.error = Some(crate::i18n::text("Datenbank wählen oder Namen für eine neue eingeben.").into());
            return;
        }
        self.error = None;
        self.log.clear();
        self.done = None;
        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let (dbc, p, create_db, replace) = (dbc.clone(), progress.clone(), self.new_db, self.replace);
        std::thread::spawn(move || {
            let opts = Options { database, create_db, replace };
            let r = write::run(&dbc, &source, &tables, &opts, &p);
            let _ = tx.send(r);
        });
        self.job = Some(Running { rx, progress });
    }

    // ------------------------------------------------------------------
    // Export

    fn start_export(&mut self, cx: &mut Ctx) {
        let Some(dbc) = cx.db else { return };
        let dbc = dbc.clone();
        let schema = cx.schema(&self.exp_db);
        let tables = self.exp_only.clone().unwrap_or_default();
        let count = if tables.is_empty() { schema.as_ref().map(|s| s.tables.iter().filter(|t| !t.is_view).count()).unwrap_or(0) } else { tables.len() };
        let name = if let [one] = tables.as_slice() { one.clone() } else { self.exp_db.clone() };
        let progress = Arc::new(Progress::default());
        let mut job = export::Job { db: self.exp_db.clone(), tables, with_data: self.exp_data, format: Format::Sql, target: PathBuf::new(), replace: self.exp_replace, server: None, progress: progress.clone() };
        // Ziel waehlen: Datei oder Server
        let server_conf: Option<(Target, ServerConf, u32)> = match self.exp_target {
            Target::File(f) => {
                job.format = f;
                let target = if f.folder(count) {
                    rfd::FileDialog::new().set_title(t("Ordner wählen", "Choose a folder")).pick_folder().map(|d| d.join(&name))
                } else {
                    rfd::FileDialog::new().add_filter(f.label(), &[f.ext()]).set_file_name(format!("{name}.{}", f.ext())).save_file()
                };
                let Some(target) = target else { return };
                job.target = target;
                None
            }
            Target::Mysql => Some((Target::Mysql, self.exp_mysql.clone(), 0)),
            Target::Postgres => Some((Target::Postgres, self.exp_postgres.clone(), 0)),
            Target::Redis => Some((Target::Redis, self.exp_redis.clone(), self.exp_redis_db)),
        };
        self.exp_result = None;
        self.log.clear();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = (|| -> Result<String, String> {
                if let Some((target, conf, redis_db)) = server_conf {
                    let link = conf.open()?;
                    job.server = Some(match target {
                        Target::Mysql => export::Server::Mysql(link),
                        Target::Postgres => export::Server::Postgres(link),
                        _ => export::Server::Redis(RedisConf { host: conf.host.clone(), port: conf.port, user: conf.user.clone(), password: conf.password.clone(), db: redis_db, tls: conf.tls, link: Some(link) }),
                    });
                }
                export::run(&dbc, &job)
            })();
            let _ = tx.send(r);
        });
        self.exporting = Some(Exporting { rx, progress });
    }
}

/// Quelle holen und lesen (laeuft im Hintergrund)
#[allow(clippy::too_many_arguments)]
fn prepare(kind: Kind, place: Where, files: Vec<PathBuf>, url: String, ssh: SshConfig, paths: String, conf: ServerConf, redis_db: u32, link: Option<Link>, cancel: &AtomicBool) -> Result<Prepared, String> {
    let mut out = Prepared { source: None, fetched: None, link: None, databases: None };
    let source_of = |files: &[PathBuf]| -> Result<Source, String> {
        let first = files.first().ok_or_else(|| t("Keine Datei gewählt.", "No file chosen.").to_string())?.clone();
        Ok(match kind {
            Kind::Sqlite => Source::Sqlite(first),
            Kind::Mongo => Source::Docs(files.to_vec()),
            Kind::Csv => Source::Csv(files.to_vec()),
            _ => Source::RedisFile(first, redis_db),
        })
    };
    let source = match place {
        Where::Local => source_of(&files)?,
        Where::Web | Where::Ssh => {
            let loc = if place == Where::Web { Location::Url(url) } else { Location::Ssh { cfg: ssh, paths } };
            let f = remote::fetch(&loc, kind.default_ext(), cancel)?;
            let s = source_of(&f.files)?;
            out.fetched = Some(f);
            s
        }
        Where::Server => {
            let link = match link {
                Some(l) => l,
                None => conf.open()?,
            };
            out.link = Some(link.clone());
            match kind {
                Kind::Redis => Source::Redis(RedisConf { host: conf.host.clone(), port: conf.port, user: conf.user.clone(), password: conf.password.clone(), db: redis_db, tls: conf.tls, link: Some(link) }),
                Kind::Mysql | Kind::Postgres | Kind::Mongo => {
                    let sk = match kind {
                        Kind::Mysql => ServerKind::Mysql,
                        Kind::Postgres => ServerKind::Postgres,
                        _ => ServerKind::Mongo,
                    };
                    if conf.database.is_empty() {
                        out.databases = Some(import::databases(sk, &link)?);
                        return Ok(out);
                    }
                    let mut l = link;
                    l.conf.database = conf.database.clone();
                    match kind {
                        Kind::Mysql => Source::Mysql(l),
                        Kind::Postgres => Source::Postgres(l),
                        _ => Source::Mongo(l),
                    }
                }
                _ => return Err(t("Nicht unterstützt.", "Not supported.").into()),
            }
        }
    };
    let tables = import::scan(&source, cancel)?;
    out.source = Some((source, tables));
    Ok(out)
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

// ---------------------------------------------------------------------------
// Formularteile

fn tls_chips(ui: &mut egui::Ui, tls: &mut Tls) {
    let items = [(Tls::Off, t("Aus", "Off").to_string()), (Tls::On, t("Ein", "On").to_string()), (Tls::NoVerify, t("Ein, Zertifikat nicht prüfen", "On, skip certificate check").to_string())];
    chips(ui, tls, &items);
}

/// Anmeldung an einem Server samt SSH-Tunnel
fn server_form(ui: &mut egui::Ui, c: &mut ServerConf, user_hint: &str) {
    row(ui, t("Server und Port", "Server and port"), |ui| {
        let w = (ui.available_width() - 96.0).max(80.0);
        text_field(ui, &mut c.host, "db.example.org", Some(w));
        number_field(ui, &mut c.port, 1..=65535);
    });
    row(ui, t("Benutzer", "User"), |ui| {
        text_field(ui, &mut c.user, user_hint, None);
    });
    row(ui, t("Passwort", "Password"), |ui| {
        password_field(ui, &mut c.password, "", None);
    });
    row(ui, t("Verschlüsselung (TLS)", "Encryption (TLS)"), |ui| tls_chips(ui, &mut c.tls));
    row(ui, t("SSH-Tunnel", "SSH tunnel"), |ui| {
        widgets::switch(ui, &mut c.ssh.enabled, t("Über einen SSH-Server verbinden", "Connect through an SSH server"))
            .on_hover_text(t("Server und Port oben gelten dann aus Sicht des SSH-Servers (meist 127.0.0.1).", "Server and port above then apply as seen from the SSH server (usually 127.0.0.1)."));
    });
    if c.ssh.enabled {
        ssh_form(ui, &mut c.ssh);
    }
}

fn ssh_form(ui: &mut egui::Ui, s: &mut SshConfig) {
    row(ui, t("SSH-Server und Port", "SSH server and port"), |ui| {
        let w = (ui.available_width() - 96.0).max(80.0);
        text_field(ui, &mut s.host, "ssh.example.org", Some(w));
        number_field(ui, &mut s.port, 1..=65535);
    });
    row(ui, t("SSH-Benutzer", "SSH user"), |ui| {
        text_field(ui, &mut s.user, "", None);
    });
    row(ui, t("Schlüsseldatei", "Key file"), |ui| {
        let w = (ui.available_width() - 40.0).max(80.0);
        text_field(ui, &mut s.key, t("leer = ssh-Agent", "empty = ssh agent"), Some(w));
        if icons::button_sized(ui, Icon::File, t("Datei wählen …", "Choose file …"), widgets::CONTROL_H - 4.0).clicked()
            && let Some(p) = rfd::FileDialog::new().pick_file()
        {
            s.key = p.to_string_lossy().into_owned();
        }
    });
    row(ui, t("SSH-Passwort", "SSH password"), |ui| {
        password_field(ui, &mut s.password, t("nur ohne Schlüssel (oder dessen Passphrase)", "only without a key (or its passphrase)"), None);
    });
}

impl TabView for TransferTab {
    fn icon(&self) -> Option<Icon> {
        Some(if self.mode == Mode::Import { Icon::Upload } else { Icon::Download })
    }

    fn title(&self) -> String {
        match self.mode {
            Mode::Import => crate::i18n::text("Importieren").into(),
            Mode::Export => crate::i18n::text("Exportieren").into(),
        }
    }

    fn key(&self) -> Option<String> {
        Some("transfer".into())
    }

    fn session(&self) -> Option<String> {
        Some(if self.mode == Mode::Import { "import" } else { "export" }.into())
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.exporting.is_some()
    }

    fn takes_drop(&self, path: &std::path::Path) -> bool {
        Kind::from_path(path).is_some()
    }

    fn show_section(&mut self, id: &str) {
        self.mode = if id == "export" { Mode::Export } else { Mode::Import };
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        if self.preparing.is_some() || self.job.is_some() || self.exporting.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Dateien auf die Registerkarte ziehen
        let dropped: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if self.mode == Mode::Import
            && let Some(k) = dropped.first().and_then(|p| Kind::from_path(p))
        {
            let same: Vec<PathBuf> = dropped.into_iter().filter(|p| Kind::from_path(p) == Some(k)).collect();
            self.kind = k;
            let files = if matches!(k, Kind::Sqlite | Kind::Redis) { same.into_iter().take(1).collect() } else { same };
            let ctx = ui.ctx().clone();
            self.set_files(&ctx, files);
        }
        if self.exp_db.is_empty() {
            self.exp_db = cx.databases.iter().find(|d| !super::is_system_db(d)).cloned().unwrap_or_default();
        }
        super::page_keys(ui);
        egui::ScrollArea::vertical().id_salt("transferscroll").auto_shrink([false, false]).show(ui, |ui| {
            let narrow = ui.available_width() < 640.0;
            ui.set_max_width(ui.available_width().min(860.0));
            ui.horizontal_wrapped(|ui| {
                ui.heading(t("Import und Export", "Import and export"));
                if !narrow {
                    ui.add_space(12.0);
                }
                widgets::segmented(ui, &mut self.mode, &[(Mode::Import, t("Importieren", "Import")), (Mode::Export, t("Exportieren", "Export"))]);
            });
            ui.add_space(4.0);
            match self.mode {
                Mode::Import => self.import_ui(ui, cx),
                Mode::Export => self.export_ui(ui, cx),
            }
            ui.add_space(24.0);
        });
    }
}

impl TransferTab {
    fn import_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let busy = self.job.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            let items: Vec<(Kind, String)> = Kind::ALL.iter().map(|k| (*k, k.label().to_string())).collect();
            section(ui, t("Woher kommen die Daten?", "Where does the data come from?"), |ui| {
                ui.label(RichText::new(t("Art der Quelle", "Type of source")).small().color(pal.text_weak));
                if chips(ui, &mut self.kind, &items) {
                    self.place = self.kind.wheres()[0];
                    self.files.clear();
                    self.databases.clear();
                    self.link = None;
                    self.new_name.clear();
                    self.reset_result();
                }
                let places: Vec<(Where, String)> = self.kind.wheres().iter().map(|w| (*w, w.label(self.kind))).collect();
                if places.len() > 1 {
                    ui.label(RichText::new(t("Wo liegt sie?", "Where is it?")).small().color(pal.text_weak));
                }
                if places.len() > 1 && chips(ui, &mut self.place, &places) {
                    self.reset_result();
                }
                let ctx = ui.ctx().clone();
                self.source_ui(ui, &ctx);
            });
        });
        if self.preparing.is_some() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t("Wird geladen und gelesen …", "Loading and reading …"));
                if ui.button(t("Abbrechen", "Cancel")).clicked()
                    && let Some(p) = &self.preparing
                {
                    p.cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        if let Some(e) = &self.error {
            ui.add_space(8.0);
            ui.label(RichText::new(e).color(pal.error_text));
        }
        if self.source.is_some() && !self.entries.is_empty() {
            ui.add_enabled_ui(!busy, |ui| {
                section(ui, t("Tabellen", "Tables"), |ui| {
                    for e in self.entries.iter_mut() {
                        ui.horizontal_wrapped(|ui| {
                            widgets::check(ui, &mut e.on, "");
                            text_field(ui, &mut e.scan.name, "", Some(220.0));
                            let cols: Vec<String> = e.scan.columns.iter().map(|c| format!("{}  {}", c.name, c.decl.clone().unwrap_or_else(|| c.ty.sql()))).collect();
                            ui.label(RichText::new(crate::tr_format!("{} Zeilen, {} Spalten", "{} rows, {} columns", e.scan.rows, e.scan.columns.len())).color(pal.text_weak)).on_hover_text(cols.join("\n"));
                        });
                    }
                });
                let mut start = false;
                section(ui, t("Ziel in EasyMySQL", "Destination in EasyMySQL"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.radio_value(&mut self.new_db, true, t("Neue Datenbank", "New database"));
                        ui.add_enabled_ui(self.new_db, |ui| text_field(ui, &mut self.new_name, "", Some(200.0)));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.radio_value(&mut self.new_db, false, t("Vorhandene Datenbank", "Existing database"));
                        ui.add_enabled_ui(!self.new_db, |ui| super::db_combo(ui, "imp-db", cx.databases, &mut self.database));
                    });
                    widgets::check(ui, &mut self.replace, t("Vorhandene Tabellen gleichen Namens ersetzen", "Replace existing tables with the same name"));
                    let any = self.entries.iter().any(|e| e.on);
                    ui.add_enabled_ui(any && cx.db.is_some(), |ui| {
                        if icons::primary_button(ui, Icon::Upload, crate::i18n::text("Importieren")).clicked() {
                            start = true;
                        }
                    });
                });
                if start {
                    self.start_import(cx);
                }
            });
        }
        if let Some(job) = &self.job {
            let total = job.progress.total.load(Ordering::Relaxed).max(1);
            let done = job.progress.done.load(Ordering::Relaxed);
            let table = job.progress.table.lock().unwrap().clone();
            progress_ui(ui, &table, done, total, &job.progress);
            self.log = job.progress.log.lock().unwrap().clone();
        }
        if let Some(n) = self.done {
            ui.add_space(8.0);
            ui.label(RichText::new(crate::tr_format!("Fertig: {n} Zeile(n) importiert.", "Done: {n} row(s) imported.")).color(pal.ok_text));
        }
        log_ui(ui, &self.log);
    }

    fn source_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pal = style::pal();
        match self.place {
            Where::Local => {
                ui.horizontal_wrapped(|ui| {
                    if icons::text_button(ui, Icon::File, t("Datei wählen …", "Choose file …")).clicked() {
                        self.choose_files(ctx);
                    }
                    let names: Vec<String> = self.files.iter().filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
                    if names.is_empty() {
                        ui.label(RichText::new(t("oder Datei hierher ziehen", "or drop a file here")).color(pal.text_weak));
                    } else {
                        ui.label(names.join(", "));
                    }
                });
            }
            Where::Web => {
                row(ui, t("Adresse", "Address"), |ui| {
                    text_field(ui, &mut self.url, "https://example.org/daten.sqlite", None);
                });
                if icons::text_button(ui, Icon::Download, t("Laden und lesen", "Load and read")).clicked() && !self.url.trim().is_empty() {
                    self.new_name = remote::file_name(&self.url, "import").split('.').next().map(sanitize).unwrap_or_default();
                    self.start_prepare(ctx, false);
                }
            }
            Where::Ssh => {
                ssh_form(ui, &mut self.ssh);
                row(ui, t("Pfad auf dem Server", "Path on the server"), |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.ssh_paths).hint_text(t("/home/anna/daten.sqlite  (eine Datei je Zeile)", "/home/anna/data.sqlite  (one file per line)")).desired_rows(2).desired_width(f32::INFINITY));
                });
                if icons::text_button(ui, Icon::Download, t("Holen und lesen", "Fetch and read")).clicked() && !self.ssh_paths.trim().is_empty() {
                    self.new_name = remote::file_name(self.ssh_paths.lines().next().unwrap_or(""), "import").split('.').next().map(sanitize).unwrap_or_default();
                    self.start_prepare(ctx, false);
                }
            }
            Where::Server => {
                let kind = self.kind;
                let hint = match kind {
                    Kind::Postgres => "postgres",
                    Kind::Mongo => t("leer = ohne Anmeldung", "empty = no login"),
                    Kind::Redis => t("leer = ohne Benutzer", "empty = no user"),
                    _ => "root",
                };
                server_form(ui, self.conf_mut(), hint);
                if kind == Kind::Redis {
                    row(ui, t("Datenbank-Nummer", "Database number"), |ui| {
                        number_field(ui, &mut self.redis_db, 0..=63);
                    });
                }
                let connect_label = if matches!(kind, Kind::Redis) { t("Lesen", "Read") } else { t("Verbinden", "Connect") };
                ui.horizontal_wrapped(|ui| {
                    if icons::text_button(ui, Icon::Refresh, connect_label).clicked() {
                        self.databases.clear();
                        self.link = None;
                        if kind != Kind::Redis {
                            self.conf_mut().database.clear();
                        }
                        self.start_prepare(ctx, false);
                    }
                    if !self.databases.is_empty() {
                        ui.label(RichText::new(t("Datenbank:", "Database:")).color(pal.text_weak));
                        let mut db = self.conf_mut().database.clone();
                        let before = db.clone();
                        super::str_combo(ui, "src-db", &self.databases, &mut db, 200.0);
                        if db != before {
                            self.conf_mut().database = db;
                            self.start_prepare(ctx, true);
                        }
                    }
                });
            }
        }
    }

    fn export_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let busy = self.exporting.is_some();
        let schema = cx.schema(&self.exp_db);
        ui.add_enabled_ui(!busy, |ui| {
            section(ui, t("Was soll exportiert werden?", "What should be exported?"), |ui| {
                row(ui, t("Datenbank", "Database"), |ui| {
                    if super::db_combo(ui, "expdb", cx.databases, &mut self.exp_db) {
                        self.exp_only = None;
                        self.exp_result = None;
                    }
                });
                row(ui, t("Tabellen", "Tables"), |ui| {
                    let mut all = self.exp_only.is_none();
                    if widgets::segmented(ui, &mut all, &[(true, t("Alle", "All")), (false, t("Auswahl", "Selection"))]) {
                        self.exp_only = if all { None } else { Some(Vec::new()) };
                    }
                });
                if let (Some(sel), Some(schema)) = (self.exp_only.as_mut(), &schema) {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for tb in schema.tables.iter().filter(|tb| (matches!(self.exp_target, Target::File(Format::Sql))) || !tb.is_view) {
                            let mut on = sel.contains(&tb.name);
                            if widgets::check(ui, &mut on, &tb.name).changed() {
                                if on {
                                    sel.push(tb.name.clone());
                                } else {
                                    sel.retain(|n| n != &tb.name);
                                }
                            }
                        }
                    });
                }
            });
            let file_items: Vec<(Target, String)> = Format::ALL.iter().map(|f| (Target::File(*f), f.label().to_string())).collect();
            section(ui, t("Wohin?", "Where to?"), |ui| {
                ui.label(RichText::new(t("In eine Datei", "To a file")).small().color(pal.text_weak));
                chips(ui, &mut self.exp_target, &file_items);
                ui.label(RichText::new(t("Auf einen anderen Server", "To another server")).small().color(pal.text_weak));
                let server_items = vec![(Target::Mysql, "MariaDB / MySQL".to_string()), (Target::Postgres, "PostgreSQL".to_string()), (Target::Redis, "Redis".to_string())];
                chips(ui, &mut self.exp_target, &server_items);
                ui.separator();
                match self.exp_target {
                    Target::File(f) => {
                        ui.label(RichText::new(file_hint(f)).color(pal.text_weak));
                        if f == Format::Sql {
                            widgets::check(ui, &mut self.exp_data, t("Mit Daten", "With data"));
                        }
                    }
                    Target::Mysql => {
                        server_form(ui, &mut self.exp_mysql, "root");
                        row(ui, t("Zieldatenbank", "Target database"), |ui| {
                            text_field(ui, &mut self.exp_mysql.database, &self.exp_db, None);
                        });
                        widgets::check(ui, &mut self.exp_data, t("Mit Daten", "With data"));
                        widgets::check(ui, &mut self.exp_replace, t("Vorhandene Tabellen gleichen Namens ersetzen", "Replace existing tables with the same name"));
                    }
                    Target::Postgres => {
                        server_form(ui, &mut self.exp_postgres, "postgres");
                        row(ui, t("Zieldatenbank", "Target database"), |ui| {
                            text_field(ui, &mut self.exp_postgres.database, "postgres", None);
                        });
                        widgets::check(ui, &mut self.exp_data, t("Mit Daten", "With data"));
                        widgets::check(ui, &mut self.exp_replace, t("Vorhandene Tabellen gleichen Namens ersetzen", "Replace existing tables with the same name"));
                    }
                    Target::Redis => {
                        ui.label(RichText::new(file_hint(Format::RedisRdb)).color(pal.text_weak));
                        server_form(ui, &mut self.exp_redis, t("leer = ohne Benutzer", "empty = no user"));
                        row(ui, t("Datenbank-Nummer", "Database number"), |ui| {
                            number_field(ui, &mut self.exp_redis_db, 0..=63);
                        });
                        widgets::check(ui, &mut self.exp_replace, t("Vorhandene Schlüssel ersetzen", "Replace existing keys"));
                    }
                }
                let none_chosen = self.exp_only.as_ref().is_some_and(|s| s.is_empty());
                let ready = !self.exp_db.is_empty() && !none_chosen && cx.db.is_some();
                let label = if matches!(self.exp_target, Target::File(_)) { t("Speichern unter …", "Save as …") } else { t("Übertragen", "Transfer") };
                ui.add_enabled_ui(ready, |ui| {
                    if icons::primary_button(ui, Icon::Download, label).clicked() {
                        self.start_export(cx);
                    }
                });
            });
        });
        if let Some(job) = &self.exporting {
            let total = job.progress.total.load(Ordering::Relaxed).max(1);
            let done = job.progress.done.load(Ordering::Relaxed);
            let table = job.progress.table.lock().unwrap().clone();
            progress_ui(ui, &table, done, total, &job.progress);
        }
        match &self.exp_result {
            Some(Ok(msg)) => {
                ui.add_space(8.0);
                ui.label(RichText::new(crate::tr_format!("Fertig: {msg}", "Done: {msg}")).color(pal.ok_text));
                if matches!(self.exp_target, Target::File(_))
                    && icons::text_button(ui, Icon::FolderOpen, t("Ordner zeigen", "Show folder")).clicked()
                    && let Some(d) = std::path::Path::new(msg).parent()
                {
                    crate::app::open_path(d);
                }
            }
            Some(Err(e)) => {
                ui.add_space(8.0);
                ui.label(RichText::new(e).color(pal.error_text));
            }
            None => {}
        }
        log_ui(ui, &self.log);
    }
}

fn file_hint(f: Format) -> String {
    match f {
        Format::Sql => t("SQL-Skript für MariaDB/MySQL, mit Sichten und Beziehungen.", "SQL script for MariaDB/MySQL, with views and relationships.").into(),
        Format::Json => t("Eine JSON-Datei mit allen Tabellen.", "One JSON file with all tables.").into(),
        Format::Csv => t("Eine CSV-Datei je Tabelle (Semikolon, UTF-8).", "One CSV file per table (semicolon, UTF-8).").into(),
        Format::Xml => t("Eine XML-Datei mit allen Tabellen.", "One XML file with all tables.").into(),
        Format::Sqlite => t("Eine SQLite-Datenbank (.sqlite).", "A SQLite database (.sqlite).").into(),
        Format::PgSql => t("SQL-Skript für PostgreSQL (psql -f datei.sql).", "SQL script for PostgreSQL (psql -f file.sql).").into(),
        Format::Mongo => t("JSON-Zeilen je Tabelle, ladbar mit mongoimport.", "JSON lines per table, loadable with mongoimport.").into(),
        Format::RedisRdb => t("Jede Zeile wird ein Redis-Hash „tabelle:schlüssel“.", "Every row becomes a Redis hash “table:key”.").into(),
        Format::RedisCmds => t("HSET-Befehle, ladbar mit redis-cli < datei.", "HSET commands, loadable with redis-cli < file.").into(),
    }
}

fn progress_ui(ui: &mut egui::Ui, table: &str, done: u64, total: u64, progress: &Progress) {
    ui.add_space(10.0);
    ui.add(egui::ProgressBar::new((done as f32 / total as f32).min(1.0)).text(format!("{table} · {done} / {total}")));
    if icons::text_button(ui, Icon::Close, crate::i18n::text("Abbrechen")).clicked() {
        progress.cancel.store(true, Ordering::Relaxed);
    }
}

fn log_ui(ui: &mut egui::Ui, log: &[(bool, String)]) {
    let pal = style::pal();
    for (err, line) in log {
        ui.label(RichText::new(line).small().color(if *err { pal.error_text } else { pal.text_weak }));
    }
}
