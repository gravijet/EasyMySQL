// Eigene lokale KI: llama.cpp (llama-server) mit dem Code-Modell Qwen2.5-Coder 1.5B. EasyMySQL laedt beides
// auf Wunsch (Einstellungen oder Installer, jeweils neueste Fassung mit Pruefsumme) in seinen Datenordner und
// startet den Server nur auf 127.0.0.1. Nichts verlaesst den Rechner, es braucht kein Konto und keine
// Grafikkarte. Verwendet fuer lange Vorschlaege im Editor (Lueckenfuellen, "fill in the middle") und als
// letzte Idee im Auto-Fix (dort prueft ein Probelauf jede Antwort, bevor sie gilt).

use crate::dev::install::{Checksum, Progress};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MODEL_REPO: &str = "ggml-org/Qwen2.5-Coder-1.5B-Q8_0-GGUF";
const MODEL_FILE: &str = "qwen2.5-coder-1.5b-q8_0.gguf";
pub const MODEL_LABEL: &str = "Qwen2.5-Coder 1.5B";
/// Groesse des Downloads (Server und Modell) fuer die Anzeige
pub fn size_gb() -> &'static str {
    crate::i18n::tr("1,7", "1.7")
}
const MAX_MODEL: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    NotInstalled,
    Installing,
    /// Installiert, Server laeuft nicht
    Off,
    /// Server startet bzw. laedt das Modell
    Starting,
    Ready,
    Failed,
}

fn dir() -> PathBuf {
    crate::dev::tool_root().join("ki")
}

fn model_path() -> PathBuf {
    dir().join(MODEL_FILE)
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "llama-server.exe" } else { "llama-server" }
}

/// llama-server im Ordner `ki/llama-b…` (auch eine Ebene tiefer, wie im Linux-Archiv)
fn server_exe() -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir()).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() || !e.file_name().to_string_lossy().starts_with("llama-") {
            continue;
        }
        if p.join(exe_name()).is_file() {
            return Some(p.join(exe_name()));
        }
        if let Ok(sub) = std::fs::read_dir(&p) {
            for s in sub.flatten() {
                if s.path().join(exe_name()).is_file() {
                    return Some(s.path().join(exe_name()));
                }
            }
        }
    }
    None
}

pub fn installed() -> bool {
    model_path().is_file() && server_exe().is_some()
}

/// KI eingeschaltet und installiert
pub fn ready() -> bool {
    super::prefs().ai_complete && installed()
}

// ---------------------------------------------------------------------------
// Installation

struct Install {
    running: bool,
    stage: String,
    error: Option<String>,
}

static INSTALL: Mutex<Install> = Mutex::new(Install { running: false, stage: String::new(), error: None });
static PROGRESS: Progress = Progress { done: std::sync::atomic::AtomicU64::new(0), total: std::sync::atomic::AtomicU64::new(0) };
static CANCEL: AtomicBool = AtomicBool::new(false);

/// (laeuft, Schritt, geladen, gesamt, Fehler)
pub fn install_state() -> (bool, String, u64, u64, Option<String>) {
    let s = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    (s.running, s.stage.clone(), PROGRESS.done.load(Ordering::Relaxed), PROGRESS.total.load(Ordering::Relaxed), s.error.clone())
}

pub fn cancel_install() {
    CANCEL.store(true, Ordering::Relaxed);
}

fn set_stage(s: String) {
    INSTALL.lock().unwrap_or_else(|e| e.into_inner()).stage = s;
}

/// Neuester llama.cpp-Build fuer dieses System: (Version, Paket)
fn latest_server() -> Result<crate::dev::install::Package, String> {
    let suffix = if cfg!(windows) {
        "-bin-win-cpu-x64.zip"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "-bin-macos-arm64.tar.gz" } else { "-bin-macos-x64.tar.gz" }
    } else if cfg!(target_arch = "aarch64") {
        "-bin-ubuntu-arm64.tar.gz"
    } else {
        "-bin-ubuntu-x64.tar.gz"
    };
    // Jeder Build ist ein eigenes Release ("b12478"); "latest" zeigt dort nicht auf einen Build
    let list = crate::dev::install::get_json("https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=10")?;
    for r in list.as_array().into_iter().flatten() {
        let tag = r["tag_name"].as_str().unwrap_or("");
        if !tag.starts_with('b') {
            continue;
        }
        let want = format!("llama-{tag}{suffix}");
        let Some(a) = r["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str() == Some(want.as_str())) else { continue };
        let Some(sha) = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")) else { continue };
        return Ok(crate::dev::install::Package {
            version: tag.to_string(),
            url: a["browser_download_url"].as_str().ok_or("llama.cpp: url")?.to_string(),
            checksum: Checksum::Sha256(sha.to_ascii_lowercase()),
            file: want,
        });
    }
    Err(crate::i18n::tr("Kein passender llama.cpp-Build gefunden.", "No matching llama.cpp build found.").into())
}

/// Pruefsumme und Groesse des Modells (Hugging Face)
fn model_checksum() -> Result<Checksum, String> {
    let list = crate::dev::install::get_json(&format!("https://huggingface.co/api/models/{MODEL_REPO}/tree/main"))?;
    let f = list.as_array().into_iter().flatten().find(|f| f["path"].as_str() == Some(MODEL_FILE)).ok_or("model: file")?;
    let sha = f["lfs"]["oid"].as_str().ok_or("model: sha256")?;
    Ok(Checksum::Sha256(sha.to_ascii_lowercase()))
}

/// Server und Modell laden (blockiert). `stage`: Fortschrittsmeldungen
pub fn install_blocking(stage: &dyn Fn(String), progress: &Progress, cancel: &AtomicBool) -> Result<(), String> {
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    stage(crate::i18n::tr("Neueste Version suchen …", "Looking for the newest version …").into());
    let pkg = latest_server()?;
    let target = d.join(format!("llama-{}", pkg.version));
    if !target.join(exe_name()).is_file() && server_exe().is_none_or(|e| !e.starts_with(&target)) {
        stage(crate::tr_format!("KI-Server (llama.cpp {}) wird geladen …", "Downloading the AI server (llama.cpp {}) …", pkg.version));
        let tmp = d.join(".laden");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let archive = tmp.join(&pkg.file);
        let r = (|| {
            crate::dev::install::download(&pkg.url, &archive, &pkg.checksum, progress, cancel)?;
            let content = crate::dev::install::extract(&archive, &tmp.join("inhalt"))?;
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&content, &target).map_err(|e| e.to_string())
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        r?;
        // Aeltere Builds entfernen
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            if e.path() != target && e.path().is_dir() && e.file_name().to_string_lossy().starts_with("llama-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    if !model_path().is_file() {
        stage(crate::tr_format!("Modell {} wird geladen (etwa {} GB) …", "Downloading the model {} (about {} GB) …", MODEL_LABEL, size_gb()));
        let sum = model_checksum()?;
        let url = format!("https://huggingface.co/{MODEL_REPO}/resolve/main/{MODEL_FILE}");
        crate::dev::install::download_max(&url, &model_path(), &sum, progress, cancel, MAX_MODEL)?;
    }
    Ok(())
}

/// Im Hintergrund installieren (Einstellungen, Installer-Wunsch beim Start)
pub fn install(ctx: Option<eframe::egui::Context>) {
    {
        let mut s = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return;
        }
        s.running = true;
        s.error = None;
    }
    CANCEL.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let r = install_blocking(&set_stage, &PROGRESS, &CANCEL);
        let mut s = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
        s.running = false;
        s.error = r.err();
        drop(s);
        if let Some(c) = ctx {
            c.request_repaint();
        }
    });
}

/// KI wieder entfernen (Speicherplatz freigeben)
pub fn uninstall() -> Result<(), String> {
    shutdown();
    std::fs::remove_dir_all(dir()).map_err(|e| e.to_string())
}

/// Merkzettel des Installers: beim naechsten Start im Hintergrund installieren
fn marker() -> PathBuf {
    crate::dev::tool_root().join("ki-installieren")
}

pub fn request_install_later() -> Result<(), String> {
    std::fs::create_dir_all(crate::dev::tool_root()).map_err(|e| e.to_string())?;
    std::fs::write(marker(), b"1").map_err(|e| e.to_string())
}

/// Beim Start: hat der Installer die KI gewuenscht?
pub fn install_if_requested(ctx: &eframe::egui::Context) -> bool {
    if marker().is_file() {
        let _ = std::fs::remove_file(marker());
        if !installed() {
            install(Some(ctx.clone()));
            return true;
        }
    }
    false
}

/// `easymysql install-local-ai [--later]`: fuer die Installer (Konsole mit Fortschritt)
pub fn install_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--later") {
        return match request_install_later() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    if installed() {
        println!("  ✓ {MODEL_LABEL} ({})", dir().display());
        return 0;
    }
    let progress = Arc::new(Progress::default());
    let done = Arc::new(AtomicBool::new(false));
    let (p, dn) = (progress.clone(), done.clone());
    let printer = std::thread::spawn(move || {
        while !dn.load(Ordering::Relaxed) {
            let (a, b) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
            if b > 0 {
                print!("\r    {:>5.0} / {:.0} MB ", a as f64 / 1e6, b as f64 / 1e6);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let r = install_blocking(&|s| println!("\n  {s}"), &progress, &AtomicBool::new(false));
    done.store(true, Ordering::Relaxed);
    let _ = printer.join();
    println!();
    match r {
        Ok(()) => {
            println!("  ✓ {MODEL_LABEL}");
            0
        }
        Err(e) => {
            eprintln!("  {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Server

struct Proc {
    child: std::process::Child,
    port: u16,
    ready: bool,
    failed: bool,
}

static PROC: Mutex<Option<Proc>> = Mutex::new(None);

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

/// Zustand; startet den Server, wenn `start` und die KI installiert ist
pub fn status_and_start(start: bool) -> Status {
    if INSTALL.lock().unwrap_or_else(|e| e.into_inner()).running {
        return Status::Installing;
    }
    let mut p = PROC.lock().unwrap_or_else(|e| e.into_inner());
    // Beendet (abgestuerzt oder nach dem Entfernen)?
    if let Some(pr) = p.as_mut()
        && !matches!(pr.child.try_wait(), Ok(None))
    {
        let failed = !pr.ready || pr.failed;
        *p = None;
        if failed {
            FAILED.store(true, Ordering::Relaxed);
        }
    }
    if let Some(pr) = p.as_ref() {
        return if pr.ready { Status::Ready } else if pr.failed { Status::Failed } else { Status::Starting };
    }
    if !installed() {
        return Status::NotInstalled;
    }
    if FAILED.load(Ordering::Relaxed) && !start {
        return Status::Failed;
    }
    if !start {
        return Status::Off;
    }
    FAILED.store(false, Ordering::Relaxed);
    match spawn() {
        Ok(pr) => {
            let port = pr.port;
            *p = Some(pr);
            std::thread::spawn(move || wait_ready(port));
            Status::Starting
        }
        Err(_) => {
            FAILED.store(true, Ordering::Relaxed);
            Status::Failed
        }
    }
}

static FAILED: AtomicBool = AtomicBool::new(false);

pub fn status() -> Status {
    status_and_start(false)
}

fn spawn() -> Result<Proc, String> {
    let exe = server_exe().ok_or("llama-server fehlt")?;
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())?;
    let log = std::fs::File::create(dir().join("server.log")).map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("-m")
        .arg(model_path())
        .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", "8192", "-np", "1", "--cache-reuse", "256", "--sleep-idle-seconds", "600"])
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    if let Some(d) = exe.parent() {
        cmd.current_dir(d);
        #[cfg(target_os = "linux")]
        cmd.env("LD_LIBRARY_PATH", d);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    Ok(Proc { child, port, ready: false, failed: false })
}

/// Warten, bis das Modell geladen ist (/health antwortet mit 200)
fn wait_ready(port: u16) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(120) {
        if agent(Duration::from_millis(800)).get(&format!("http://127.0.0.1:{port}/health")).call().is_ok() {
            if let Some(p) = PROC.lock().unwrap_or_else(|e| e.into_inner()).as_mut().filter(|p| p.port == port) {
                p.ready = true;
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    if let Some(p) = PROC.lock().unwrap_or_else(|e| e.into_inner()).as_mut().filter(|p| p.port == port) {
        p.failed = true;
    }
}

/// Server beenden (beim Schliessen von EasyMySQL)
pub fn shutdown() {
    if let Some(mut p) = PROC.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = p.child.kill();
        let _ = p.child.wait();
    }
}

fn port() -> Option<u16> {
    PROC.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|p| p.ready).map(|p| p.port)
}

/// Port, notfalls nach dem Start (blockiert hoechstens `wait`)
fn port_waiting(wait: Duration) -> Option<u16> {
    let start = Instant::now();
    loop {
        match status_and_start(true) {
            Status::Ready => return port(),
            Status::Starting if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(200)),
            _ => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// Anfragen

/// Ergebnis einer Anfrage: (Nummer der Anfrage, Cursorstelle, Vorschlag)
pub type Slot = Arc<Mutex<Option<(u64, usize, String)>>>;

/// Vorschlag fuer die Stelle zwischen `prefix` und `suffix` anfordern
pub fn request(ctx: eframe::egui::Context, prefix: String, suffix: String, at: usize, id: u64, slot: Slot) {
    let Some(port) = port() else { return };
    std::thread::spawn(move || {
        let body = serde_json::json!({
            "input_prefix": prefix,
            "input_suffix": suffix,
            "n_predict": 160,
            "temperature": 0.2,
            "stop": ["\n\n\n"],
            "t_max_predict_ms": 3000,
            "cache_prompt": true,
        });
        let Ok(mut r) = agent(Duration::from_secs(20)).post(&format!("http://127.0.0.1:{port}/infill")).send_json(&body) else { return };
        let Ok(v) = r.body_mut().read_json::<serde_json::Value>() else { return };
        if let Some(text) = v["content"].as_str().and_then(|t| clean(t, &suffix)) {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, at, text));
            ctx.request_repaint();
        }
    });
}

/// Auto-Fix: korrigierte Fassung einer Anweisung, die mit `err` scheitert (blockiert; nur ein Vorschlag,
/// der Probelauf entscheidet, ob er gilt)
pub fn fix_statement(sql: &str, err: &str, schema: Option<&crate::db::Schema>) -> Option<String> {
    let port = port_waiting(Duration::from_secs(90))?;
    let err = err.lines().next().unwrap_or(err);
    let wrong: String = sql.lines().map(|l| format!("-- {l}\n")).collect();
    let prompt = format!("{}-- Diese Anweisung scheitert mit: {err}\n{wrong}-- Korrigierte Anweisung fuer MariaDB:\n", schema_hint(schema, "--"));
    let body = serde_json::json!({
        "prompt": prompt,
        "n_predict": 400,
        "temperature": 0.0,
        "stop": [";", "\n\n\n", "\n--"],
        "cache_prompt": true,
    });
    let mut r = agent(Duration::from_secs(60)).post(&format!("http://127.0.0.1:{port}/completion")).send_json(&body).ok()?;
    let v: serde_json::Value = r.body_mut().read_json().ok()?;
    let out = v["content"].as_str()?.trim().trim_end_matches(';').trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Antwort kuerzen: hoechstens 15 Zeilen, nichts, was schon hinter dem Cursor steht
pub fn clean(t: &str, suffix: &str) -> Option<String> {
    let t = t.replace("\r\n", "\n");
    let mut lines: Vec<&str> = t.lines().take(15).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let out = lines.join("\n");
    let rest = suffix.trim_start();
    if out.trim().is_empty() || (!rest.is_empty() && rest.starts_with(out.trim())) {
        return None;
    }
    Some(out)
}

/// Kurze Beschreibung der Datenbank fuer die KI (als SQL-Kommentar vor dem Text)
pub fn schema_hint(schema: Option<&crate::db::Schema>, comment: &str) -> String {
    let Some(s) = schema else { return String::new() };
    let mut out = format!("{comment} MariaDB, Datenbank {}\n", s.name);
    for t in s.tables.iter().take(40) {
        let cols: Vec<String> = t.columns.iter().map(|c| format!("{} {}{}", c.name, c.col_type, if c.is_pk() { " PK" } else { "" })).collect();
        out.push_str(&format!("{comment} {}({})\n", t.name, cols.join(", ")));
    }
    for f in s.fks.iter().take(60) {
        out.push_str(&format!("{comment} {}.{} -> {}.{}\n", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_trimmed() {
        assert_eq!(clean("SELECT *\nFROM t;\n\n", "").as_deref(), Some("SELECT *\nFROM t;"));
        assert_eq!(clean("  \n", ""), None);
        assert_eq!(clean(")", ");"), None);
    }

    /// Laedt llama.cpp und das Modell (etwa 1,7 GB) und fragt die KI: cargo test e2e_local_ai -- --ignored
    #[test]
    #[ignore]
    fn e2e_local_ai() {
        install_blocking(&|s| eprintln!("{s}"), &Progress::default(), &AtomicBool::new(false)).unwrap();
        assert!(installed());
        let fixed = fix_statement("SELEC name FORM kunde", "ERROR 1064 (42000): You have an error in your SQL syntax", None);
        eprintln!("{fixed:?}");
        assert!(fixed.is_some_and(|f| f.to_uppercase().contains("SELECT")));
        let port = port().unwrap();
        let body = serde_json::json!({ "input_prefix": "-- Alle Kunden aus Wien\n", "input_suffix": "", "n_predict": 60 });
        let mut r = agent(Duration::from_secs(60)).post(&format!("http://127.0.0.1:{port}/infill")).send_json(&body).unwrap();
        let v: serde_json::Value = r.body_mut().read_json().unwrap();
        eprintln!("{}", v["content"]);
        assert!(v["content"].as_str().is_some_and(|c| !c.trim().is_empty()));
        shutdown();
    }
}
