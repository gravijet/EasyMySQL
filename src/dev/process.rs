// Gemeinsames fuer das Starten von Programmen: was im Terminal laeuft, Fehlermeldungen mit Datei und Zeile,
// Ausgabe von Hilfsprogrammen (Compiler, pip, npm) einsammeln.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Was im Terminal gestartet wird
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub title: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Zusaetzliche Umgebung (ueberschreibt die allgemeine)
    pub env: Vec<(String, String)>,
}

impl Launch {
    pub fn new(title: impl Into<String>, program: impl Into<PathBuf>, args: Vec<String>, cwd: PathBuf) -> Self {
        Launch { title: title.into(), program: program.into(), args, cwd, env: Vec::new() }
    }
}

/// Fehler oder Warnung des Compilers bzw. der Syntaxpruefung
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub file: PathBuf,
    /// 0-basiert
    pub line: usize,
    pub col: Option<usize>,
    pub message: String,
    pub error: bool,
}

/// Fehlschlag beim Vorbereiten: Meldung, Compilerfehler und die letzte Ausgabe
#[derive(Clone, Debug, Default)]
pub struct Failure {
    pub message: String,
    pub diags: Vec<Diag>,
    pub output: String,
}

impl Failure {
    pub fn msg(m: impl Into<String>) -> Self {
        Failure { message: m.into(), ..Default::default() }
    }
}

/// Programm ausfuehren und die Ausgabe sammeln; jede Zeile erscheint sofort in `live`.
pub fn capture(mut cmd: Command, live: &Mutex<String>) -> Result<(bool, String), String> {
    crate::server::hidden(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let program = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    let out = Arc::new(Mutex::new(String::new()));
    let mut readers = Vec::new();
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let out = Arc::clone(&out);
        readers.push(std::thread::spawn(move || {
            for line in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                out.lock().unwrap().push_str(&format!("{line}\n"));
            }
        }));
    }
    // Waehrenddessen die letzte Zeile zeigen
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            for r in readers {
                let _ = r.join();
            }
            let text = out.lock().unwrap().clone();
            return Ok((status.success(), text));
        }
        if let Some(last) = out.lock().unwrap().lines().rev().find(|l| !l.trim().is_empty()) {
            *live.lock().unwrap() = last.chars().take(160).collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Die letzten Zeilen einer Ausgabe
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// PATH mit den eigenen Werkzeugen vorne
pub fn tool_path(tools: &super::Tools) -> Option<std::ffi::OsString> {
    let old = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = tools.path_dirs();
    paths.extend(std::env::split_paths(&old));
    std::env::join_paths(paths).ok()
}

/// Ordner nach oben absuchen, bis eine der Dateien da ist (hoechstens bis `limit`)
pub fn find_up(file: &Path, names: &[&str], limit: Option<&Path>) -> Option<PathBuf> {
    let mut dir = file.parent();
    let mut depth = 0;
    while let Some(d) = dir {
        if names.iter().any(|n| d.join(n).exists()) {
            return Some(d.to_path_buf());
        }
        if limit.is_some_and(|l| d == l) || depth > 12 {
            break;
        }
        dir = d.parent();
        depth += 1;
    }
    None
}

/// Alle Dateien mit einer der Endungen unter `root` (ohne Ausgabe-, Abhaengigkeits- und versteckte Ordner)
pub fn source_files(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 16 || out.len() > 5000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            if p.is_dir() {
                if !name.starts_with('.') && !matches!(name.as_str(), "out" | "bin" | "target" | "build" | "node_modules" | "__pycache__" | "venv" | "dist") {
                    walk(&p, exts, out, depth + 1);
                }
            } else if exts.iter().any(|x| name.ends_with(&format!(".{x}"))) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, exts, &mut out, 0);
    out
}

/// Pruefsumme eines Textes (fuer "hat sich requirements.txt geaendert?")
pub fn fingerprint(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
    }
    format!("{h:016x}")
}

/// Ordner fuer Zwischenergebnisse eines Projekts (ausserhalb des Projekts)
pub fn scratch_dir(kind: &str, root: &Path) -> PathBuf {
    let key = fingerprint(&[root.to_string_lossy().to_lowercase().as_bytes()]);
    crate::workspace::internal_dir().join(kind).join(key)
}
