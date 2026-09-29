// Verwaltung des mitgelieferten MariaDB-Servers: einrichten, starten, stoppen.
// Der Server laeuft nur, solange EasyMySQL geoeffnet ist.

use crate::db::ConnInfo;
use mysql::prelude::*;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    NotFound,
    Stopped,
    Initializing,
    Starting,
    Running { own: bool },
    Stopping,
    Failed(String),
}

impl State {
    pub fn text(&self) -> String {
        match self {
            State::NotFound => "MariaDB nicht gefunden".into(),
            State::Stopped => "gestoppt".into(),
            State::Initializing => "wird eingerichtet ...".into(),
            State::Starting => "startet ...".into(),
            State::Running { own: true } => "läuft".into(),
            State::Running { own: false } => "läuft (externer Server)".into(),
            State::Stopping => "wird beendet ...".into(),
            State::Failed(e) => format!("Fehler: {e}"),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, State::Running { .. })
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, State::Initializing | State::Starting | State::Stopping)
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub bin: PathBuf,
    pub base: PathBuf,
    pub data: PathBuf,
    pub ini: PathBuf,
}

impl Paths {
    /// Existiert eine PID-Datei? Dann laeuft (vermutlich) unser eigener Server.
    pub fn has_pid_file(&self) -> bool {
        if self.base.join("mysql.pid").exists() {
            return true;
        }
        std::fs::read_dir(&self.data)
            .map(|rd| {
                rd.flatten()
                    .any(|e| e.path().extension().map(|x| x == "pid").unwrap_or(false))
            })
            .unwrap_or(false)
    }
}

struct Shared {
    state: State,
    child: Option<Child>,
    log: Vec<String>,
}

#[derive(Clone)]
pub struct Server {
    shared: Arc<Mutex<Shared>>,
    pub paths: Option<Paths>,
    pub port: u16,
}

#[cfg(windows)]
const EXE: &str = ".exe";
#[cfg(not(windows))]
const EXE: &str = "";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
}

#[cfg(windows)]
fn writable(dir: &Path) -> bool {
    let test = dir.join(".schreibtest");
    let ok = std::fs::write(&test, b"ok").is_ok();
    let _ = std::fs::remove_file(&test);
    ok
}

/// Ordner fuer Daten und Einstellungen.
/// Windows: bevorzugt C:\ProgramData\EasyMySQL (vom Setup mit Schreibrechten angelegt,
/// kurzer Pfad ohne Umlaute), sonst %LOCALAPPDATA%\EasyMySQL.
pub fn app_data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(find_app_data_dir).clone()
}

fn find_app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("ProgramData") {
            let d = PathBuf::from(p).join("EasyMySQL");
            if d.is_dir() && writable(&d) {
                return d;
            }
        }
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(p).join("EasyMySQL");
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(p) = std::env::var("HOME") {
            return PathBuf::from(p).join(".local/share/EasyMySQL");
        }
    }
    exe_dir().unwrap_or_default().join("userdata")
}

fn find_bin_dir() -> Option<PathBuf> {
    let server = format!("mariadbd{EXE}");
    let mut candidates = Vec::new();
    if let Ok(p) = std::env::var("EASYMYSQL_MARIADB_BIN") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(d) = exe_dir() {
        candidates.push(d.join("mariadb").join("bin"));
        candidates.push(d.join("..").join("mariadb").join("bin"));
    }
    #[cfg(not(windows))]
    {
        candidates.push(PathBuf::from("/usr/sbin"));
        candidates.push(PathBuf::from("/usr/local/sbin"));
        candidates.push(PathBuf::from("/usr/bin"));
    }
    candidates.into_iter().find(|d| d.join(&server).exists())
}

fn find_tool(bin: &Path, name: &str) -> PathBuf {
    let file = format!("{name}{EXE}");
    let p = bin.join(&file);
    if p.exists() {
        return p;
    }
    #[cfg(not(windows))]
    for d in ["/usr/bin", "/usr/local/bin", "/usr/sbin"] {
        let p = Path::new(d).join(&file);
        if p.exists() {
            return p;
        }
    }
    p
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(not(windows))]
fn is_root_user() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
        || std::env::var("HOME").map(|h| h == "/root").unwrap_or(false)
}

fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

impl Server {
    pub fn new(port: u16) -> Self {
        let paths = find_bin_dir().map(|bin| {
            let base = app_data_dir();
            Paths {
                bin,
                data: base.join("data"),
                ini: base.join("my.ini"),
                base,
            }
        });
        let state = if paths.is_some() {
            State::Stopped
        } else {
            State::NotFound
        };
        Server {
            shared: Arc::new(Mutex::new(Shared {
                state,
                child: None,
                log: Vec::new(),
            })),
            paths,
            port,
        }
    }

    pub fn state(&self) -> State {
        let mut sh = self.shared.lock().unwrap();
        // Unerwartet beendet?
        if matches!(sh.state, State::Running { own: true }) {
            if let Some(child) = sh.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    sh.child = None;
                    sh.state = State::Failed(format!("Server unerwartet beendet ({status})"));
                }
            }
        }
        sh.state.clone()
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.shared.lock().unwrap().log.clone()
    }

    pub fn log(&self, line: impl Into<String>) {
        push_log(&self.shared, line.into());
    }

    fn set_state(&self, st: State) {
        self.shared.lock().unwrap().state = st;
    }

    /// Startet den Server im Hintergrund (nicht blockierend).
    pub fn start(&self, on_change: impl Fn() + Send + 'static) {
        let st = self.state();
        if st.is_running() || st.is_busy() {
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let result = this.start_blocking(&on_change);
            if let Err(e) = result {
                this.log(format!("FEHLER: {e}"));
                this.set_state(State::Failed(e));
            }
            on_change();
        });
    }

    fn start_blocking(&self, on_change: &dyn Fn()) -> Result<(), String> {
        if port_open(self.port) {
            if self.paths.as_ref().map(|p| p.has_pid_file()).unwrap_or(false) {
                // Eigener Server aus einer frueheren (abgebrochenen) Sitzung
                self.log(format!(
                    "Der EasyMySQL-Server läuft noch aus einer früheren Sitzung (Port {}) – wird übernommen.",
                    self.port
                ));
                self.set_state(State::Running { own: true });
            } else {
                self.log(format!(
                    "Auf Port {} läuft bereits ein anderer Datenbankserver – dieser wird verwendet.",
                    self.port
                ));
                self.set_state(State::Running { own: false });
            }
            return Ok(());
        }
        let Some(paths) = self.paths.clone() else {
            self.set_state(State::NotFound);
            return Err("MariaDB wurde nicht gefunden. Bitte EasyMySQL neu installieren.".into());
        };
        std::fs::create_dir_all(&paths.base)
            .map_err(|e| format!("Ordner {} kann nicht angelegt werden: {e}", paths.base.display()))?;

        // 1. Datenverzeichnis einrichten (nur beim allerersten Start)
        if !paths.data.join("mysql").exists() {
            self.set_state(State::Initializing);
            on_change();
            if paths.data.exists() {
                let backup = paths.base.join(format!(
                    "data-defekt-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                ));
                let _ = std::fs::rename(&paths.data, &backup);
            }
            self.log(format!("Richte Datenverzeichnis ein: {}", paths.data.display()));
            let tool = find_tool(&paths.bin, "mariadb-install-db");
            let mut cmd = Command::new(&tool);
            cmd.arg(format!("--datadir={}", paths.data.display()));
            #[cfg(windows)]
            {
                cmd.arg(format!("--port={}", self.port));
            }
            #[cfg(not(windows))]
            {
                cmd.arg("--auth-root-authentication-method=normal");
                cmd.arg("--skip-test-db");
                if is_root_user() {
                    cmd.arg("--user=root");
                }
            }
            let out = hidden(&mut cmd)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| format!("{} konnte nicht gestartet werden: {e}", tool.display()))?;
            for l in String::from_utf8_lossy(&out.stdout)
                .lines()
                .chain(String::from_utf8_lossy(&out.stderr).lines())
            {
                self.log(l.to_string());
            }
            if !out.status.success() || !paths.data.join("mysql").exists() {
                return Err("Einrichtung des Datenverzeichnisses fehlgeschlagen (siehe Server-Log).".into());
            }
        }

        // 2. Konfiguration schreiben
        let mut ini = String::new();
        ini.push_str("# Von EasyMySQL erzeugt\n[mysqld]\n");
        ini.push_str(&format!("datadir={}\n", slash(&paths.data)));
        ini.push_str(&format!("port={}\n", self.port));
        ini.push_str("bind-address=127.0.0.1\n");
        ini.push_str("character-set-server=utf8mb4\n");
        ini.push_str("collation-server=utf8mb4_unicode_ci\n");
        ini.push_str("innodb_buffer_pool_size=128M\n");
        ini.push_str("max_allowed_packet=64M\n");
        #[cfg(not(windows))]
        {
            ini.push_str(&format!("socket={}\n", slash(&paths.base.join("mysql.sock"))));
            ini.push_str(&format!("pid-file={}\n", slash(&paths.base.join("mysql.pid"))));
        }
        ini.push_str(&format!(
            "\n[client]\nport={}\n",
            self.port
        ));
        std::fs::write(&paths.ini, ini).map_err(|e| format!("my.ini: {e}"))?;

        // 3. Server starten
        self.set_state(State::Starting);
        on_change();
        let server = find_tool(&paths.bin, "mariadbd");
        self.log(format!("Starte {}", server.display()));
        let mut cmd = Command::new(&server);
        cmd.arg(format!("--defaults-file={}", paths.ini.display()));
        #[cfg(windows)]
        cmd.arg("--console");
        #[cfg(not(windows))]
        if is_root_user() {
            cmd.arg("--user=root");
        }
        let mut child = hidden(&mut cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Server konnte nicht gestartet werden: {e}"))?;
        for stream in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let shared = self.shared.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    push_log(&shared, line);
                }
            });
        }
        self.shared.lock().unwrap().child = Some(child);

        // 4. Warten, bis der Port erreichbar ist
        let start = Instant::now();
        loop {
            {
                let mut sh = self.shared.lock().unwrap();
                if let Some(c) = sh.child.as_mut() {
                    if let Ok(Some(status)) = c.try_wait() {
                        sh.child = None;
                        drop(sh);
                        return Err(format!(
                            "Server hat sich sofort beendet ({status}). Ist Port {} belegt? Siehe Server-Log.",
                            self.port
                        ));
                    }
                }
            }
            if port_open(self.port) {
                self.log(format!("Server bereit auf Port {}.", self.port));
                self.upgrade_if_needed(&paths);
                self.set_state(State::Running { own: true });
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(90) {
                return Err("Server antwortet nicht (Zeitüberschreitung).".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Nach einem Update auf eine neuere MariaDB-Version die Systemtabellen
    /// anpassen. mariadb-upgrade erkennt selbst, ob das noetig ist.
    fn upgrade_if_needed(&self, paths: &Paths) {
        let tool = find_tool(&paths.bin, "mariadb-upgrade");
        if !tool.exists() {
            return;
        }
        let mut cmd = Command::new(&tool);
        cmd.args([
            "--user=root",
            "--host=127.0.0.1",
            "--protocol=tcp",
            &format!("--port={}", self.port),
            "--silent",
        ]);
        match hidden(&mut cmd).stdin(Stdio::null()).output() {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_string()
                    + &String::from_utf8_lossy(&out.stderr);
                if out.status.success() {
                    self.log("Datenbank-Systemtabellen sind aktuell.");
                } else {
                    self.log("Hinweis: mariadb-upgrade konnte nicht ausgeführt werden (evtl. root-Passwort gesetzt).");
                }
                for l in text.lines().filter(|l| !l.trim().is_empty()).take(20) {
                    self.log(l.to_string());
                }
            }
            Err(e) => self.log(format!("Hinweis: mariadb-upgrade: {e}")),
        }
    }

    /// Beendet den Server (blockierend, max. ca. 20 Sekunden).
    pub fn stop_blocking(&self, conn: &ConnInfo) {
        let st = self.state();
        if !matches!(st, State::Running { own: true } | State::Starting | State::Failed(_)) {
            if matches!(st, State::Running { own: false }) {
                self.set_state(State::Stopped);
            }
            return;
        }
        let orphan = self.shared.lock().unwrap().child.is_none();
        if orphan && !matches!(st, State::Running { own: true }) {
            self.set_state(State::Stopped);
            return;
        }
        self.set_state(State::Stopping);
        self.log("Server wird beendet ...");
        // Sauber herunterfahren
        let mut info = conn.clone();
        info.host = "127.0.0.1".into();
        info.port = self.port;
        if let Ok(mut c) = mysql::Conn::new(info.opts()) {
            let _ = c.query_drop("SHUTDOWN");
        }
        let start = Instant::now();
        loop {
            let mut sh = self.shared.lock().unwrap();
            let done = match sh.child.as_mut() {
                Some(c) => matches!(c.try_wait(), Ok(Some(_))),
                None => !orphan || !port_open(self.port),
            };
            if done {
                sh.child = None;
                break;
            }
            if start.elapsed() > Duration::from_secs(20) {
                if let Some(c) = sh.child.as_mut() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                sh.child = None;
                drop(sh);
                self.log("Server musste hart beendet werden.");
                break;
            }
            drop(sh);
            std::thread::sleep(Duration::from_millis(100));
        }
        self.log("Server beendet.");
        self.set_state(State::Stopped);
    }

    pub fn restart(&self, conn: ConnInfo, on_change: impl Fn() + Send + 'static) {
        let this = self.clone();
        std::thread::spawn(move || {
            this.stop_blocking(&conn);
            on_change();
            if let Err(e) = this.start_blocking(&on_change) {
                this.log(format!("FEHLER: {e}"));
                this.set_state(State::Failed(e));
            }
            on_change();
        });
    }

    pub fn stop(&self, conn: ConnInfo, on_change: impl Fn() + Send + 'static) {
        let this = self.clone();
        std::thread::spawn(move || {
            this.stop_blocking(&conn);
            on_change();
        });
    }
}

fn push_log(shared: &Arc<Mutex<Shared>>, line: String) {
    let mut sh = shared.lock().unwrap();
    sh.log.push(line);
    let n = sh.log.len();
    if n > 3000 {
        sh.log.drain(0..n - 3000);
    }
}
