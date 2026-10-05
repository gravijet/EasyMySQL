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
    Repairing,
    Failed(String),
}

impl State {
    pub fn text(&self) -> String {
        match self {
            State::NotFound => crate::i18n::text("MariaDB nicht gefunden").into(),
            State::Stopped => crate::i18n::text("gestoppt").into(),
            State::Initializing => crate::i18n::text("wird eingerichtet ...").into(),
            State::Starting => crate::i18n::text("startet ...").into(),
            State::Running { own: true } => crate::i18n::text("läuft").into(),
            State::Running { own: false } => crate::i18n::text("läuft (externer Server)").into(),
            State::Stopping => crate::i18n::text("wird beendet ...").into(),
            State::Repairing => crate::i18n::text("wird repariert ...").into(),
            State::Failed(e) => crate::tr_format!("Fehler: {e}", "Error: {e}"),
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, State::Running { .. })
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, State::Initializing | State::Starting | State::Stopping | State::Repairing)
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub bin: PathBuf,
    pub base: PathBuf,
    pub data: PathBuf,
    pub ini: PathBuf,
}

/// Einstellungen, die Datenverlust bei Absturz/Stromausfall verhindern.
pub const SAFETY_OPTIONS: &str = "\
# --- Datensicherheit (absturz- und stromausfallsicher) ---
default-storage-engine=InnoDB
innodb_flush_log_at_trx_commit=1
innodb_doublewrite=ON
innodb_file_per_table=ON
aria_recover_options=BACKUP,QUICK
myisam_recover_options=BACKUP,FORCE
";

/// Versatz der Ortszeit zu UTC in Sekunden (Windows: inkl. Sommerzeit).
pub fn local_offset_secs() -> i64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
        let mut l: SYSTEMTIME = std::mem::zeroed();
        let mut u: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut l);
        GetSystemTime(&mut u);
        let m = |t: &SYSTEMTIME| t.wDay as i64 * 1440 + t.wHour as i64 * 60 + t.wMinute as i64;
        let mut diff = m(&l) - m(&u);
        // Monatswechsel abfangen
        if diff > 720 {
            diff -= 1440 * ((diff + 720) / 1440);
        } else if diff < -720 {
            diff += 1440 * ((-diff + 720) / 1440);
        }
        diff * 60
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Zeitstempel in Ortszeit, z. B. "2026-09-29_21-30-05" (sortierbar).
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_ts((secs + local_offset_secs()).max(0) as u64)
}

/// Sekunden seit 1970 -> "JJJJ-MM-TT_hh-mm-ss"
pub fn format_ts(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Algorithmus nach H. Hinnant (civil_from_days)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Ordner rekursiv kopieren.
pub fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}

/// Server sauber beenden lassen, ohne Anmeldung.
pub fn signal_shutdown(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
        // mariadbd wartet unter Windows auf das Ereignis "MySQLShutdown<PID>"
        let name: Vec<u16> = format!("MySQLShutdown{pid}").encode_utf16().chain(std::iter::once(0)).collect();
        let h = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if !h.is_null() {
            SetEvent(h);
            CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }
}

impl Paths {
    /// PID des laufenden Servers aus der PID-Datei
    pub fn read_pid(&self) -> Option<u32> {
        let mut files = vec![self.base.join("mysql.pid")];
        if let Ok(rd) = std::fs::read_dir(&self.data) {
            files.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pid")));
        }
        files.iter().find_map(|f| std::fs::read_to_string(f).ok()?.trim().parse().ok())
    }

    /// Existiert, solange der eigene Server laeuft. Bleibt nach einem Absturz liegen.
    pub fn session_flag(&self) -> PathBuf {
        self.base.join("server-laeuft.flag")
    }

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

/// Herkunft einer Log-Zeile
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LogKind {
    /// Meldung von EasyMySQL
    App,
    /// Ausgabe von MariaDB
    Server,
    /// Mitgeschriebene Anweisung (general log)
    Query,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    /// "JJJJ-MM-TT hh:mm:ss"
    pub time: String,
    pub kind: LogKind,
    pub text: String,
    /// Aus einer frueheren Sitzung (aus der Datei geladen)
    pub old: bool,
}

const LOG_MAX: usize = 5000;

struct Shared {
    state: State,
    child: Option<Child>,
    log: Vec<LogLine>,
    /// Anzahl aller jemals angefuegten Zeilen (fuer "neue Zeilen seit ...")
    log_total: usize,
    unclean: bool,
}

/// Datei, in die das Server-Log dauerhaft geschrieben wird
pub fn log_file() -> PathBuf {
    app_data_dir().join("server.log")
}

/// Datei fuer die Mitschrift aller Anweisungen (MariaDB general log)
pub fn query_log_file() -> PathBuf {
    app_data_dir().join("abfragen.log")
}

fn now_text() -> String {
    timestamp().replacen('_', " ", 1).replace('-', ":").replacen(':', "-", 2)
}

/// Letzte Zeilen der Log-Datei (fruehere Sitzungen)
fn load_old_log() -> Vec<LogLine> {
    let path = log_file();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(1000)..]
        .iter()
        .filter_map(|l| {
            let (time, rest) = l.split_once('\t')?;
            let (kind, text) = rest.split_once('\t')?;
            let kind = match kind {
                "S" => LogKind::Server,
                "Q" => LogKind::Query,
                _ => LogKind::App,
            };
            Some(LogLine { time: time.to_string(), kind, text: text.to_string(), old: true })
        })
        .collect()
}

/// Eine Zeile des general log lesbar machen: "241001 12:00:00\t    5 Query\tSELECT 1" -> "SELECT 1"
fn parse_query_log(line: &str) -> Option<String> {
    let t = line.trim_end();
    if t.is_empty() || t.contains("Tcp port:") || t.starts_with("Time") || t.ends_with("started with:") {
        return None;
    }
    let mut parts: Vec<&str> = t.split('\t').collect();
    // Zeitstempel am Anfang entfernen
    if parts.len() > 1 && parts[0].chars().next().is_some_and(|c| c.is_ascii_digit()) {
        parts.remove(0);
    }
    while parts.len() > 1 && parts[0].trim().is_empty() {
        parts.remove(0);
    }
    match parts.as_slice() {
        [head, arg, ..] => {
            let head = head.trim();
            let (id, cmd) = head.split_once(' ').unwrap_or(("", head));
            let cmd = cmd.trim();
            if cmd == "Query" || cmd == "Execute" {
                Some(format!("[{id}] {arg}"))
            } else {
                Some(format!("[{id}] {cmd} {arg}").trim_end().to_string())
            }
        }
        [one] if !one.trim().is_empty() => {
            // "    11 Quit" (Befehl ohne Argument) oder Fortsetzung einer mehrzeiligen Anweisung
            let t = one.trim();
            match t.split_once(' ') {
                Some((id, cmd)) if id.chars().all(|c| c.is_ascii_digit()) && !cmd.contains(' ') && one.starts_with(['\t', ' ']) => {
                    Some(format!("[{id}] {cmd}"))
                }
                _ => Some(format!("    {t}")),
            }
        }
        _ => None,
    }
}

/// Liest neue Zeilen des general log mit (laeuft die ganze Zeit im Hintergrund).
fn follow_query_log(shared: Arc<Mutex<Shared>>) {
    let path = query_log_file();
    let mut offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(700));
        let Ok(meta) = std::fs::metadata(&path) else {
            offset = 0;
            continue;
        };
        let len = meta.len();
        if len < offset {
            offset = 0;
        }
        if len == offset {
            continue;
        }
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        if f.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::new();
        let _ = f.take(4_000_000).read_to_end(&mut buf);
        // nur vollstaendige Zeilen
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else { continue };
        offset += end as u64 + 1;
        let text = String::from_utf8_lossy(&buf[..end]);
        let time = now_text();
        let mut sh = shared.lock().unwrap();
        for l in text.lines() {
            if let Some(t) = parse_query_log(l) {
                sh.log.push(LogLine { time: time.clone(), kind: LogKind::Query, text: t, old: false });
                sh.log_total += 1;
            }
        }
        let n = sh.log.len();
        if n > LOG_MAX {
            sh.log.drain(0..n - LOG_MAX);
        }
    }
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

pub fn find_tool(bin: &Path, name: &str) -> PathBuf {
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

pub fn hidden(cmd: &mut Command) -> &mut Command {
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
        let shared = Arc::new(Mutex::new(Shared { state, child: None, log: load_old_log(), log_total: 0, unclean: false }));
        let sh = shared.clone();
        std::thread::spawn(move || follow_query_log(sh));
        Server { shared, paths, port }
    }

    pub fn state(&self) -> State {
        let mut sh = self.shared.lock().unwrap();
        // Unerwartet beendet?
        if matches!(sh.state, State::Running { own: true }) {
            if let Some(child) = sh.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    sh.child = None;
                    sh.state = State::Failed(crate::tr_format!("Server unerwartet beendet ({status})", "Server stopped unexpectedly ({status})"));
                }
            }
        }
        sh.state.clone()
    }

    pub fn log_lines(&self) -> Vec<LogLine> {
        self.shared.lock().unwrap().log.clone()
    }

    /// Anzahl aller bisher angefuegten Zeilen (aendert sich bei neuen Zeilen)
    pub fn log_count(&self) -> usize {
        self.shared.lock().unwrap().log_total
    }

    pub fn log(&self, line: impl Into<String>) {
        push_log(&self.shared, LogKind::App, line.into());
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
                this.log(crate::tr_format!("FEHLER: {e}", "ERROR: {e}"));
                this.set_state(State::Failed(e));
            }
            on_change();
        });
    }

    pub fn start_blocking(&self, on_change: &dyn Fn()) -> Result<(), String> {
        if port_open(self.port) {
            if self.paths.as_ref().map(|p| p.has_pid_file()).unwrap_or(false) {
                // Eigener Server aus einer frueheren (abgebrochenen) Sitzung
                self.log(crate::tr_format!("Der EasyMySQL-Server läuft noch aus einer früheren Sitzung (Port {}) – wird übernommen.", "The EasyMySQL server is still running from a previous session (port {}) — reconnecting.",
                    self.port
                ));
                self.set_state(State::Running { own: true });
            } else {
                self.log(crate::tr_format!("Auf Port {} läuft bereits ein anderer Datenbankserver – dieser wird verwendet.", "Another database server is already running on port {} — using it.",
                    self.port
                ));
                self.set_state(State::Running { own: false });
            }
            return Ok(());
        }
        let Some(paths) = self.paths.clone() else {
            self.set_state(State::NotFound);
            return Err(crate::i18n::text("MariaDB wurde nicht gefunden. Bitte EasyMySQL neu installieren.").into());
        };
        std::fs::create_dir_all(&paths.base)
            .map_err(|e| crate::tr_format!("Ordner {} kann nicht angelegt werden: {e}", "Could not create folder {}: {e}", paths.base.display()))?;

        // 1. Datenverzeichnis einrichten (nur beim allerersten Start)
        if !paths.data.join("mysql").exists() {
            self.set_state(State::Initializing);
            on_change();
            if paths.data.exists() {
                let backup = paths.base.join(format!("data-unvollstaendig-{}", timestamp()));
                let _ = std::fs::rename(&paths.data, &backup);
            }
            self.init_datadir(&paths, &paths.data, self.port)?;
        }

        // 2. Konfiguration schreiben
        self.write_ini(&paths, &paths.data, self.port, &paths.ini, &[])?;

        // Lief der Server beim letzten Mal noch, als EasyMySQL/der PC beendet wurde?
        if paths.session_flag().exists() {
            self.log(crate::i18n::text("Hinweis: Der Server wurde beim letzten Mal nicht sauber beendet (Absturz, Stromausfall oder hartes Beenden). MariaDB stellt die Daten jetzt automatisch wieder her, danach werden alle Tabellen geprüft."));
            self.shared.lock().unwrap().unclean = true;
        }

        // 3. Server starten
        self.set_state(State::Starting);
        on_change();
        let child = self.spawn_mariadbd(&paths, &paths.ini, &[])?;
        self.shared.lock().unwrap().child = Some(child);

        // 4. Warten, bis der Port erreichbar ist
        self.wait_ready(self.port, Duration::from_secs(600))?;
        self.log(crate::tr_format!("Server bereit auf Port {}.", "Server ready on port {}.", self.port));
        let _ = std::fs::write(paths.session_flag(), timestamp());
        self.upgrade_if_needed(&paths);
        self.set_state(State::Running { own: true });
        Ok(())
    }

    /// Leeres Datenverzeichnis anlegen (mariadb-install-db).
    pub fn init_datadir(&self, paths: &Paths, data: &Path, port: u16) -> Result<(), String> {
        self.log(crate::tr_format!("Richte Datenverzeichnis ein: {}", "Initializing data directory: {}", data.display()));
        let tool = find_tool(&paths.bin, "mariadb-install-db");
        let mut cmd = Command::new(&tool);
        cmd.arg(format!("--datadir={}", data.display()));
        #[cfg(windows)]
        {
            cmd.arg(format!("--port={port}"));
        }
        #[cfg(not(windows))]
        {
            let _ = port;
            cmd.arg("--auth-root-authentication-method=normal");
            cmd.arg("--skip-test-db");
            if is_root_user() {
                cmd.arg("--user=root");
            }
        }
        let out = hidden(&mut cmd)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| crate::tr_format!("{} konnte nicht gestartet werden: {e}", "Could not start {}: {e}", tool.display()))?;
        for l in String::from_utf8_lossy(&out.stdout)
            .lines()
            .chain(String::from_utf8_lossy(&out.stderr).lines())
        {
            self.log(l.to_string());
        }
        if !out.status.success() || !data.join("mysql").exists() {
            return Err(crate::i18n::text("Einrichtung des Datenverzeichnisses fehlgeschlagen (siehe Server-Log).").into());
        }
        Ok(())
    }

    /// Konfigurationsdatei schreiben. `extra` = zusaetzliche Zeilen fuer [mysqld].
    pub fn write_ini(&self, paths: &Paths, data: &Path, port: u16, ini_path: &Path, extra: &[String]) -> Result<(), String> {
        let mut ini = String::new();
        ini.push_str("# Von EasyMySQL erzeugt\n[mysqld]\n");
        ini.push_str(&format!("datadir={}\n", slash(data)));
        ini.push_str(&format!("port={port}\n"));
        ini.push_str("bind-address=127.0.0.1\n");
        ini.push_str("character-set-server=utf8mb4\n");
        ini.push_str("collation-server=utf8mb4_unicode_ci\n");
        ini.push_str("innodb_buffer_pool_size=128M\n");
        ini.push_str("max_allowed_packet=64M\n");
        ini.push_str(SAFETY_OPTIONS);
        #[cfg(not(windows))]
        {
            let sock = if data == paths.data { paths.base.join("mysql.sock") } else { data.with_extension("sock") };
            ini.push_str(&format!("socket={}\n", slash(&sock)));
            if data == paths.data {
                ini.push_str(&format!("pid-file={}\n", slash(&paths.base.join("mysql.pid"))));
            }
        }
        #[cfg(windows)]
        let _ = paths;
        for e in extra {
            ini.push_str(e);
            ini.push('\n');
        }
        ini.push_str(&format!("\n[client]\nport={port}\n"));
        std::fs::write(ini_path, ini).map_err(|e| format!("my.ini: {e}"))
    }

    /// mariadbd starten; Ausgaben landen im Server-Log.
    pub fn spawn_mariadbd(&self, paths: &Paths, ini: &Path, args: &[String]) -> Result<Child, String> {
        let server = find_tool(&paths.bin, "mariadbd");
        self.log(crate::tr_format!("Starte {}", "Starting {}", server.display()));
        let mut cmd = Command::new(&server);
        cmd.arg(format!("--defaults-file={}", ini.display()));
        #[cfg(windows)]
        cmd.arg("--console");
        #[cfg(not(windows))]
        if is_root_user() {
            cmd.arg("--user=root");
        }
        cmd.args(args);
        let mut child = hidden(&mut cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| crate::tr_format!("Server konnte nicht gestartet werden: {e}", "Could not start server: {e}"))?;
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
                    push_log(&shared, LogKind::Server, line);
                }
            });
        }
        Ok(child)
    }

    /// Wartet, bis der eigene Server (self.shared.child) auf `port` antwortet.
    fn wait_ready(&self, port: u16, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            {
                let mut sh = self.shared.lock().unwrap();
                if let Some(c) = sh.child.as_mut() {
                    if let Ok(Some(status)) = c.try_wait() {
                        sh.child = None;
                        drop(sh);
                        return Err(crate::tr_format!("Server hat sich sofort beendet ({status}). Siehe Server-Log. \
                             Falls die Datenbank beschädigt ist: Server → Reparieren.", "Server exited immediately ({status}). See the server log. If the database is damaged, open Server → Repair."
                        ));
                    }
                }
            }
            if port_open(port) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(crate::i18n::text("Server antwortet nicht (Zeitüberschreitung).").into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Wartet, bis ein (Rettungs-)Server auf `port` antwortet.
    fn wait_child_ready(child: &mut Child, port: u16, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(crate::tr_format!("Server beendet ({status})", "Server stopped ({status})"));
            }
            if port_open(port) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                return Err(crate::i18n::text("Zeitüberschreitung").into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Server-Rettung, wenn MariaDB nicht mehr startet oder die Daten stark beschaedigt sind.
    /// Der beschaedigte Datenordner bleibt immer erhalten (data-defekt-<Zeit>).
    pub fn rescue(&self, conn: &ConnInfo, backup_dir: &Path, on_change: &dyn Fn()) -> Result<String, String> {
        let paths = self.paths.clone().ok_or(crate::i18n::text("MariaDB wurde nicht gefunden."))?;
        let log = |m: String| self.log(m);
        let mut report = Vec::new();

        // 0. Laufenden Server beenden
        if self.state().is_running() {
            log(crate::i18n::text("Beende laufenden Server ...").into());
            self.stop_blocking(conn);
        }
        if port_open(self.port) {
            return Err(crate::tr_format!("Port {} ist noch belegt – bitte EasyMySQL neu starten und erneut versuchen.", "Port {} is still in use — please restart EasyMySQL and try again.", self.port));
        }
        self.set_state(State::Repairing);
        on_change();

        // 1. Beschaedigten Ordner beiseitelegen (nichts wird geloescht)
        let ts = timestamp();
        let broken = paths.base.join(format!("data-defekt-{ts}"));
        let have_old = paths.data.exists();
        if have_old {
            std::fs::rename(&paths.data, &broken)
                .map_err(|e| crate::tr_format!("Datenordner kann nicht umbenannt werden (noch in Benutzung?): {e}", "Could not rename the data folder (still in use?): {e}"))?;
            log(crate::tr_format!("Alter Datenordner gesichert als {}", "Old data folder saved as {}", broken.display()));
            report.push(crate::tr_format!("Der alte Datenordner bleibt erhalten: {}", "The old data folder is preserved: {}", broken.display()));
        }

        // 2. Rettungsversuch auf einer Kopie
        let mut rescued: Option<PathBuf> = None;
        if have_old {
            let work = paths.base.join(format!("rettung-{ts}"));
            log(crate::i18n::text("Kopiere Daten für den Rettungsversuch ...").into());
            copy_dir(&broken, &work).map_err(|e| crate::tr_format!("Kopieren fehlgeschlagen: {e}", "Copy failed: {e}"))?;
            let rport: u16 = 3399;
            let ini = paths.base.join("rettung.ini");
            for level in 0..=6u8 {
                log(crate::tr_format!("Rettungsversuch mit innodb_force_recovery={level} ...", "Attempting recovery with innodb_force_recovery={level} ..."));
                let mut extra = vec!["skip-grant-tables".to_string()];
                if level > 0 {
                    extra.push(format!("innodb_force_recovery={level}"));
                }
                self.write_ini(&paths, &work, rport, &ini, &extra)?;
                let mut child = match self.spawn_mariadbd(&paths, &ini, &[]) {
                    Ok(c) => c,
                    Err(e) => {
                        log(e);
                        continue;
                    }
                };
                if Self::wait_child_ready(&mut child, rport, Duration::from_secs(300)).is_err() {
                    let _ = child.kill();
                    let _ = child.wait();
                    continue;
                }
                let env = crate::backup::Env {
                    paths: paths.clone(),
                    conn: ConnInfo { port: rport, ..ConnInfo::default() },
                    dir: backup_dir.to_path_buf(),
                    force: true,
                };
                let res = crate::backup::create(&env, None, "rettung", &log);
                // Rettungs-Server beenden
                if mysql::Conn::new(env.conn.opts()).and_then(|mut c| c.query_drop("SHUTDOWN")).is_err() {
                    let _ = child.kill();
                }
                let _ = child.wait();
                match res {
                    Ok(dir) => {
                        log(crate::tr_format!("Daten gerettet nach {}", "Data recovered to {}", dir.display()));
                        report.push(crate::tr_format!("Daten gerettet (Stufe {level}).", "Data recovered (level {level})."));
                        rescued = Some(dir);
                        break;
                    }
                    Err(e) => log(crate::tr_format!("Sichern fehlgeschlagen: {e}", "Backup failed: {e}")),
                }
            }
            let _ = std::fs::remove_dir_all(&work);
            let _ = std::fs::remove_file(&ini);
        }

        // 3. Frischen Datenordner anlegen und Server starten
        self.set_state(State::Initializing);
        on_change();
        self.init_datadir(&paths, &paths.data, self.port)?;
        let _ = std::fs::remove_file(paths.session_flag());
        self.set_state(State::Stopped);
        self.start_blocking(on_change)?;
        on_change();

        // 4. Daten einspielen: gerettete Daten oder neueste Sicherung je Datenbank
        let env = crate::backup::Env {
            paths: paths.clone(),
            conn: ConnInfo::default(),
            dir: backup_dir.to_path_buf(),
            force: false,
        };
        let backups = crate::backup::list(backup_dir);
        let mut restored = Vec::new();
        // Welche Datenbanken gab es? (gerettete + alle aus Sicherungen)
        let mut all_dbs: Vec<String> = Vec::new();
        for b in &backups {
            for (db, _) in &b.dbs {
                if !all_dbs.contains(db) {
                    all_dbs.push(db.clone());
                }
            }
        }
        for db in all_dbs {
            // 1. Wahl: frisch gerettete Daten, 2. Wahl: neueste Sicherung dieser Datenbank
            let rescued_file = rescued.as_ref().map(|d| d.join(format!("{db}.sql.gz"))).filter(|f| f.exists());
            let source = match rescued_file {
                Some(f) => Some((f, "gerettet".to_string())),
                None => backups
                    .iter()
                    .filter(|b| b.reason != "rettung")
                    .find(|b| b.dbs.iter().any(|(d, _)| *d == db))
                    .map(|b| (b.path.join(format!("{db}.sql.gz")), crate::tr_format!("Sicherung vom {}", "Backup from {}", b.time))),
            };
            let Some((file, what)) = source else { continue };
            match crate::backup::restore(&env, &file, &db, &log) {
                Ok(_) => restored.push(format!("{db} ({what})")),
                Err(e) => log(format!("{db}: {e}")),
            }
        }
        if restored.is_empty() {
            report.push(crate::i18n::text("Es waren keine Datenbanken zum Wiederherstellen vorhanden.").into());
        } else {
            report.push(crate::tr_format!("Wiederhergestellt: {}", "Restored: {}", restored.join(", ")));
        }
        report.push(crate::i18n::text("Benutzerkonten wurden zurückgesetzt: root ohne Passwort.").into());
        log(crate::i18n::text("Server-Rettung abgeschlossen.").into());
        Ok(report.join("\n"))
    }

    /// Wurde der Server beim letzten Mal nicht sauber beendet? (einmalig abfragen)
    pub fn take_unclean(&self) -> bool {
        std::mem::take(&mut self.shared.lock().unwrap().unclean)
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
                    self.log(crate::i18n::text("Datenbank-Systemtabellen sind aktuell."));
                } else {
                    self.log(crate::i18n::text("Hinweis: mariadb-upgrade konnte nicht ausgeführt werden (evtl. root-Passwort gesetzt)."));
                }
                for l in text.lines().filter(|l| !l.trim().is_empty()).take(20) {
                    self.log(l.to_string());
                }
            }
            Err(e) => self.log(crate::tr_format!("Hinweis: mariadb-upgrade: {e}", "Hint: mariadb-upgrade: {e}")),
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
        self.log(crate::i18n::text("Server wird beendet ..."));
        // Sauber herunterfahren
        let mut info = conn.clone();
        info.host = "127.0.0.1".into();
        info.port = self.port;
        let sql_ok = mysql::Conn::new(info.opts())
            .and_then(|mut c| c.query_drop("SHUTDOWN"))
            .is_ok();
        if !sql_ok {
            // Ohne Anmeldung: Signal an den Serverprozess (ebenfalls sauberes Herunterfahren)
            let pid = self.shared.lock().unwrap().child.as_ref().map(|c| c.id()).or_else(|| self.paths.as_ref().and_then(|p| p.read_pid()));
            if let Some(pid) = pid {
                self.log(crate::tr_format!("Sende Beenden-Signal an Prozess {pid} ...", "Sending shutdown signal to process {pid} ..."));
                signal_shutdown(pid);
            }
        }
        let mut killed = false;
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
            if start.elapsed() > Duration::from_secs(60) {
                if let Some(c) = sh.child.as_mut() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                sh.child = None;
                drop(sh);
                killed = true;
                self.log(crate::i18n::text("Server musste hart beendet werden (Daten werden beim nächsten Start automatisch wiederhergestellt)."));
                break;
            }
            drop(sh);
            std::thread::sleep(Duration::from_millis(100));
        }
        self.log(crate::i18n::text("Server beendet."));
        if let (Some(p), false) = (&self.paths, killed) {
            let _ = std::fs::remove_file(p.session_flag());
        }
        self.set_state(State::Stopped);
    }

    pub fn restart(&self, conn: ConnInfo, on_change: impl Fn() + Send + 'static) {
        let this = self.clone();
        std::thread::spawn(move || {
            this.stop_blocking(&conn);
            on_change();
            if let Err(e) = this.start_blocking(&on_change) {
                this.log(crate::tr_format!("FEHLER: {e}", "ERROR: {e}"));
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

/// Zeile anfuegen und in die Log-Datei schreiben (auch wenn das Log gerade nicht angezeigt wird).
fn push_log(shared: &Arc<Mutex<Shared>>, kind: LogKind, text: String) {
    use std::io::Write;
    let time = now_text();
    let tag = match kind {
        LogKind::App => "A",
        LogKind::Server => "S",
        LogKind::Query => "Q",
    };
    let path = log_file();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{time}\t{tag}\t{}", text.replace(['\n', '\r'], " "));
    }
    let mut sh = shared.lock().unwrap();
    sh.log.push(LogLine { time, kind, text, old: false });
    sh.log_total += 1;
    let n = sh.log.len();
    if n > LOG_MAX {
        sh.log.drain(0..n - LOG_MAX);
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn general_log_lines() {
        assert_eq!(parse_query_log("260930 12:00:01\t    12 Query\tSELECT 1").as_deref(), Some("[12] SELECT 1"));
        assert_eq!(parse_query_log("\t\t    12 Query\tSELECT 2").as_deref(), Some("[12] SELECT 2"));
        assert_eq!(parse_query_log("\t\t    7 Connect\troot@localhost on  using TCP/IP").as_deref(), Some("[7] Connect root@localhost on  using TCP/IP"));
        assert_eq!(parse_query_log("FROM t").as_deref(), Some("    FROM t"));
        assert_eq!(parse_query_log("\t\t    11 Quit").as_deref(), Some("[11] Quit"));
        assert!(parse_query_log("Time\t\t    Id Command\tArgument").is_none());
    }

    #[test]
    fn time_text() {
        let t = now_text();
        assert_eq!(t.len(), 19, "{t}");
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[10..11], " ");
        assert_eq!(&t[13..14], ":");
    }
}
