// Integriertes Terminal: Jede Sitzung ist eine Shell in einer Pseudokonsole (ConPTY unter Windows). Die Ausgabe
// wird von vt100 zu einem Bildschirm zusammengesetzt, den `view` zeichnet.

mod view;

pub use view::show;

use eframe::egui;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Hat gerade ein Terminal den Tastaturfokus? Dann geben die globalen Kuerzel der App den meisten Tasten frei.
static FOCUSED: AtomicBool = AtomicBool::new(false);

pub fn focused() -> bool {
    FOCUSED.load(Ordering::Relaxed)
}

pub(crate) fn set_focused(on: bool) {
    FOCUSED.store(on, Ordering::Relaxed);
}

/// Einstellungen des Terminals
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// Schriftgroesse (0 = wie im Editor)
    pub font_size: u32,
    /// Zeilen, die man zurueckscrollen kann
    pub scrollback: usize,
    /// Name der Standard-Shell (leer = erste der Liste)
    pub default_shell: String,
    /// Eigenes Programm als Shell (z. B. C:\Program Files\Git\bin\bash.exe); leer = keins
    pub custom_shell: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { font_size: 0, scrollback: 5000, default_shell: String::new(), custom_shell: String::new() }
    }
}

static PREFS: std::sync::RwLock<Option<Prefs>> = std::sync::RwLock::new(None);

pub fn prefs() -> Prefs {
    PREFS.read().unwrap().clone().unwrap_or_default()
}

pub fn set_prefs(p: Prefs) {
    *PREFS.write().unwrap() = Some(p);
}

/// Schriftgroesse im Terminal
pub fn font_size() -> f32 {
    match prefs().font_size {
        0 => crate::sqledit::font_size(),
        n => n.clamp(6, 48) as f32,
    }
}

/// Eine auswaehlbare Shell
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    /// Zusaetzliche Umgebungsvariablen
    pub env: Vec<(String, String)>,
}

/// Verfuegbare Profile; das erste ist die Standard-Shell. `bin` = MariaDB-Programmordner, `conn` = Verbindung.
pub fn profiles(bin: Option<&Path>, conn: &crate::db::ConnInfo) -> Vec<Profile> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        out.push(Profile { name: "PowerShell".into(), program: "powershell.exe".into(), args: vec!["-NoLogo".into()], env: Vec::new() });
        out.push(Profile { name: crate::i18n::text("Eingabeaufforderung").into(), program: "cmd.exe".into(), args: Vec::new(), env: Vec::new() });
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        let name = Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Shell".into());
        out.push(Profile { name, program: shell, args: Vec::new(), env: Vec::new() });
    }
    let client = match bin {
        Some(b) => crate::server::find_tool(b, "mysql").display().to_string(),
        None => "mysql".to_string(),
    };
    let mut args = vec!["-u".to_string(), conn.user.clone(), "-h".to_string(), conn.host.clone(), "-P".to_string(), conn.port.to_string()];
    if !conn.password.is_empty() {
        args.push("-p".into());
    }
    out.push(Profile { name: crate::i18n::text("MariaDB-Konsole").into(), program: client, args, env: Vec::new() });
    // Python- und Node-Konsole, wenn vorhanden
    let tools = crate::dev::tools();
    if let Some(py) = &tools.python {
        out.push(Profile { name: format!("Python {}", py.version), program: py.exe.display().to_string(), args: vec!["-q".into()], env: Vec::new() });
    }
    if let Some(n) = &tools.node {
        out.push(Profile { name: format!("Node.js {}", n.version), program: n.exe.display().to_string(), args: Vec::new(), env: Vec::new() });
    }
    if let Some(j) = &tools.jdk
        && j.bin().join(format!("jshell{}", crate::dev::EXE)).is_file()
    {
        out.push(Profile { name: format!("JShell {}", j.version), program: j.bin().join(format!("jshell{}", crate::dev::EXE)).display().to_string(), args: Vec::new(), env: Vec::new() });
    }
    let p = prefs();
    if !p.custom_shell.trim().is_empty() {
        let mut parts = crate::dev::split_args(&p.custom_shell);
        if !parts.is_empty() {
            let program = parts.remove(0);
            let name = Path::new(&program).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Shell".into());
            out.insert(0, Profile { name, program, args: parts, env: Vec::new() });
        }
    }
    // Gewaehlte Standard-Shell nach vorne
    if let Some(i) = out.iter().position(|x| !p.default_shell.is_empty() && x.name == p.default_shell) {
        let d = out.remove(i);
        out.insert(0, d);
    }
    out
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Eine laufende (oder beendete) Shell
pub struct Session {
    pub profile: Profile,
    pub(crate) parser: Arc<Mutex<vt100::Parser>>,
    writer: Option<Writer>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    exited: Arc<AtomicBool>,
    pub error: Option<String>,
    pub(crate) size: (u16, u16),
    /// Auswahl mit der Maus: (Zeile, Spalte) von/bis
    pub(crate) selection: Option<((u16, u16), (u16, u16))>,
    cwd: PathBuf,
    /// Ordner, die vorne in PATH stehen (MariaDB-Programme, JDK, Maven, Gradle)
    path_prefix: Vec<PathBuf>,
    /// Bisherige Ausgabe als Text (fuer die Fehlererkennung), hoechstens LOG_MAX Bytes
    pub log: Arc<Mutex<String>>,
}

const LOG_MAX: usize = 256 * 1024;

impl Session {
    pub fn spawn(ctx: &egui::Context, profile: Profile, cwd: PathBuf, path_prefix: Vec<PathBuf>) -> Session {
        let mut s = Session {
            profile,
            parser: Arc::new(Mutex::new(vt100::Parser::new(24, 80, prefs().scrollback.clamp(100, 200_000)))),
            writer: None,
            master: None,
            child: None,
            exited: Arc::new(AtomicBool::new(false)),
            error: None,
            size: (24, 80),
            selection: None,
            cwd,
            path_prefix,
            log: Arc::new(Mutex::new(String::new())),
        };
        s.start(ctx);
        s
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.error = None;
        self.exited.store(false, Ordering::Relaxed);
        self.selection = None;
        self.log.lock().unwrap().clear();
        let (rows, cols) = self.size;
        *self.parser.lock().unwrap() = vt100::Parser::new(rows, cols, prefs().scrollback.clamp(100, 200_000));
        match self.open(ctx, rows, cols) {
            Ok(()) => {}
            Err(e) => {
                self.error = Some(e);
                self.exited.store(true, Ordering::Relaxed);
            }
        }
    }

    fn open(&mut self, ctx: &egui::Context, rows: u16, cols: u16) -> Result<(), String> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&self.profile.program);
        cmd.args(&self.profile.args);
        cmd.cwd(&self.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if !self.path_prefix.is_empty() {
            let old = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = self.path_prefix.clone();
            paths.extend(std::env::split_paths(&old));
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        for (k, v) in &self.profile.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("{}: {e}", self.profile.program))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let parser = Arc::clone(&self.parser);
        let exited = Arc::clone(&self.exited);
        let reply = Arc::clone(&writer);
        let log = Arc::clone(&self.log);
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("terminal".into())
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let chunk = &buf[..n];
                            if let Ok(mut l) = log.lock() {
                                l.push_str(&String::from_utf8_lossy(chunk));
                                if l.len() > LOG_MAX {
                                    let cut = l.len() - LOG_MAX / 2;
                                    let cut = (cut..l.len()).find(|&i| l.is_char_boundary(i)).unwrap_or(l.len());
                                    l.drain(..cut);
                                }
                            }
                            let answer = {
                                let mut p = parser.lock().unwrap();
                                p.process(chunk);
                                answers(chunk, p.screen().cursor_position())
                            };
                            // ConPTY wartet nach dem Start auf die Antwort auf "Cursorposition abfragen"
                            if !answer.is_empty()
                                && let Ok(mut w) = reply.lock()
                            {
                                let _ = w.write_all(&answer);
                                let _ = w.flush();
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        self.writer = Some(writer);
        self.master = Some(pair.master);
        self.child = Some(child);
        Ok(())
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    /// Eingabe an die Shell schicken
    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(w) = &self.writer
            && let Ok(mut w) = w.lock()
        {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        self.parser.lock().unwrap().screen_mut().set_size(rows, cols);
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// Beendete Shell neu starten
    pub fn restart(&mut self, ctx: &egui::Context) {
        self.stop();
        self.start(ctx);
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
        }
        self.writer = None;
        self.master = None;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Antworten auf Abfragen der Shell: Cursorposition (ESC[6n) und Geraetekennung (ESC[c)
fn answers(chunk: &[u8], cursor: (u16, u16)) -> Vec<u8> {
    let mut out = Vec::new();
    for w in chunk.windows(4) {
        if w == b"\x1b[6n" {
            out.extend_from_slice(format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1).as_bytes());
        }
    }
    for w in chunk.windows(3) {
        if w == b"\x1b[c" {
            out.extend_from_slice(b"\x1b[?1;2c");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_queries_are_answered() {
        assert_eq!(answers(b"abc\x1b[6ndef", (4, 9)), b"\x1b[5;10R");
        assert_eq!(answers(b"\x1b[c", (0, 0)), b"\x1b[?1;2c");
        assert!(answers(b"plain text", (0, 0)).is_empty());
    }

    #[test]
    fn profiles_start_with_a_shell_and_offer_the_client() {
        let p = profiles(None, &crate::db::ConnInfo::default());
        assert!(p.len() >= 2);
        let client = p.iter().find(|x| x.args.contains(&"-u".to_string())).unwrap();
        assert!(client.args.contains(&"root".to_string()));
    }

    /// Eine echte Shell starten, etwas ausfuehren lassen und die Ausgabe lesen (nur Linux).
    #[cfg(not(windows))]
    #[test]
    fn shell_runs_a_command_and_the_screen_shows_the_output() {
        let ctx = egui::Context::default();
        let profile = Profile { name: "sh".into(), program: "/bin/sh".into(), args: Vec::new(), env: vec![("EASY_TEST".into(), "42".into())] };
        let mut s = Session::spawn(&ctx, profile, std::env::temp_dir(), Vec::new());
        assert!(s.error.is_none(), "{:?}", s.error);
        s.write(b"echo EASY$EASY_TEST\r");
        let mut seen = false;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if s.parser.lock().unwrap().screen().contents().contains("EASY42") {
                seen = true;
                break;
            }
        }
        assert!(seen, "{}", s.parser.lock().unwrap().screen().contents());
        s.write(b"exit\r");
        for _ in 0..100 {
            if s.exited() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(s.exited());
    }
}
