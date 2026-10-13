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

/// Eine auswaehlbare Shell
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

/// Verfuegbare Profile; das erste ist die Standard-Shell. `bin` = MariaDB-Programmordner, `conn` = Verbindung.
pub fn profiles(bin: Option<&Path>, conn: &crate::db::ConnInfo) -> Vec<Profile> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        out.push(Profile { name: "PowerShell".into(), program: "powershell.exe".into(), args: vec!["-NoLogo".into()] });
        out.push(Profile { name: crate::i18n::text("Eingabeaufforderung").into(), program: "cmd.exe".into(), args: Vec::new() });
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        let name = Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Shell".into());
        out.push(Profile { name, program: shell, args: Vec::new() });
    }
    let client = match bin {
        Some(b) => crate::server::find_tool(b, "mysql").display().to_string(),
        None => "mysql".to_string(),
    };
    let mut args = vec!["-u".to_string(), conn.user.clone(), "-h".to_string(), conn.host.clone(), "-P".to_string(), conn.port.to_string()];
    if !conn.password.is_empty() {
        args.push("-p".into());
    }
    out.push(Profile { name: crate::i18n::text("MariaDB-Konsole").into(), program: client, args });
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
    path_prefix: Option<PathBuf>,
}

impl Session {
    pub fn spawn(ctx: &egui::Context, profile: Profile, cwd: PathBuf, path_prefix: Option<PathBuf>) -> Session {
        let mut s = Session {
            profile,
            parser: Arc::new(Mutex::new(vt100::Parser::new(24, 80, 5000))),
            writer: None,
            master: None,
            child: None,
            exited: Arc::new(AtomicBool::new(false)),
            error: None,
            size: (24, 80),
            selection: None,
            cwd,
            path_prefix,
        };
        s.start(ctx);
        s
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.error = None;
        self.exited.store(false, Ordering::Relaxed);
        self.selection = None;
        let (rows, cols) = self.size;
        *self.parser.lock().unwrap() = vt100::Parser::new(rows, cols, 5000);
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
        if let Some(prefix) = &self.path_prefix {
            let old = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![prefix.clone()];
            paths.extend(std::env::split_paths(&old));
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("{}: {e}", self.profile.program))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let parser = Arc::clone(&self.parser);
        let exited = Arc::clone(&self.exited);
        let reply = Arc::clone(&writer);
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
        let last = p.last().unwrap();
        assert!(last.args.contains(&"-u".to_string()) && last.args.contains(&"root".to_string()));
    }

    /// Eine echte Shell starten, etwas ausfuehren lassen und die Ausgabe lesen (nur Linux).
    #[cfg(not(windows))]
    #[test]
    fn shell_runs_a_command_and_the_screen_shows_the_output() {
        let ctx = egui::Context::default();
        let profile = Profile { name: "sh".into(), program: "/bin/sh".into(), args: Vec::new() };
        let mut s = Session::spawn(&ctx, profile, std::env::temp_dir(), None);
        assert!(s.error.is_none(), "{:?}", s.error);
        s.write(b"echo EASY$((20+22))\r");
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
