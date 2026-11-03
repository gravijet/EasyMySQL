// Debugger fuer Java: Das Programm laeuft im Terminal (mit JDWP, wartet beim Start), jdb verbindet sich im
// Hintergrund. Haltepunkte, Schritt fuer Schritt, Variablen, Aufrufliste und Ausdruecke auswerten.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Haltepunkte je Datei (Zeilen, 0-basiert); gelten fuer alle Registerkarten und den naechsten Start
static BREAKPOINTS: Mutex<Option<HashMap<PathBuf, Vec<usize>>>> = Mutex::new(None);

pub fn breakpoints(file: &Path) -> Vec<usize> {
    BREAKPOINTS.lock().unwrap().as_ref().and_then(|m| m.get(file).cloned()).unwrap_or_default()
}

/// Haltepunkt an- bzw. ausschalten; laeuft eine Sitzung, gilt er sofort
pub fn toggle_breakpoint(file: &Path, line: usize, class: Option<&str>) {
    let on = {
        let mut g = BREAKPOINTS.lock().unwrap();
        let list = g.get_or_insert_with(HashMap::new).entry(file.to_path_buf()).or_default();
        if let Some(i) = list.iter().position(|l| *l == line) {
            list.remove(i);
            false
        } else {
            list.push(line);
            list.sort_unstable();
            true
        }
    };
    if let (Some(s), Some(class)) = (session(), class) {
        s.send(&format!("{} {class}:{}", if on { "stop at" } else { "clear" }, line + 1));
    }
}

/// Alle Haltepunkte: (Datei, Zeile)
fn all_breakpoints() -> Vec<(PathBuf, usize)> {
    BREAKPOINTS.lock().unwrap().as_ref().map(|m| m.iter().flat_map(|(f, ls)| ls.iter().map(move |l| (f.clone(), *l))).collect()).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// z. B. "app.Main.main"
    pub method: String,
    pub file: Option<PathBuf>,
    /// 0-basiert
    pub line: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Wartet auf die JVM bzw. verbindet sich
    Starting,
    Running,
    /// Angehalten in Datei und Zeile (0-basiert)
    Paused { method: String, file: Option<PathBuf>, line: usize },
    Ended,
}

#[derive(Clone, Debug)]
pub struct State {
    pub status: Status,
    /// (Name, Wert); Argumente zuerst
    pub locals: Vec<(String, String)>,
    pub stack: Vec<Frame>,
    /// Ausgabe von jdb (fuer die Konsole des Debuggers)
    pub console: Vec<String>,
    pub error: Option<String>,
}

pub struct Session {
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pub state: Mutex<State>,
    /// Quellordner (Wurzeln der Pakete), um Klassen zu Dateien zu finden
    roots: Vec<PathBuf>,
    started: Instant,
}

static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);

pub fn session() -> Option<Arc<Session>> {
    SESSION.lock().unwrap().clone()
}

/// Laeuft gerade eine Sitzung (nicht beendet)?
pub fn active() -> bool {
    session().is_some_and(|s| s.state.lock().unwrap().status != Status::Ended)
}

/// Klassenname fuer eine Datei: Paket + Dateiname
pub fn class_for(file: &Path, text: &str) -> Option<String> {
    let stem = file.file_stem()?.to_string_lossy().into_owned();
    Some(match super::java::source::package(text) {
        Some(p) => format!("{p}.{stem}"),
        None => stem,
    })
}

/// Freien Port fuer JDWP suchen
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(5005)
}

/// jdb mit der wartenden JVM verbinden. `roots`: Quellordner (fuer -sourcepath und Dateinamen).
pub fn start(jdb: &Path, port: u16, roots: Vec<PathBuf>, ctx: &eframe::egui::Context) {
    stop();
    let session = Arc::new(Session {
        stdin: Mutex::new(None),
        child: Mutex::new(None),
        state: Mutex::new(State { status: Status::Starting, locals: Vec::new(), stack: Vec::new(), console: Vec::new(), error: None }),
        roots,
        started: Instant::now(),
    });
    *SESSION.lock().unwrap() = Some(Arc::clone(&session));
    let jdb = jdb.to_path_buf();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        // Verbinden, sobald die JVM im Terminal lauscht. Den Port vorher nicht anfassen: die JVM nimmt nur
        // eine Verbindung an und wuerde eine Probe fuer den Debugger halten.
        let sourcepath = std::env::join_paths(&session.roots).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if session.state.lock().unwrap().status == Status::Ended {
                return;
            }
            if Instant::now() > deadline {
                session.fail(crate::i18n::tr("Das Programm hat den Debugger nicht erreicht.", "The program did not reach the debugger."));
                ctx.request_repaint();
                return;
            }
            let mut cmd = Command::new(&jdb);
            cmd.arg("-attach").arg(format!("127.0.0.1:{port}"));
            if !sourcepath.is_empty() {
                cmd.arg("-sourcepath").arg(&sourcepath);
            }
            crate::server::hidden(&mut cmd);
            let mut child = match cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
                Ok(c) => c,
                Err(e) => {
                    session.fail(format!("jdb: {e}"));
                    ctx.request_repaint();
                    return;
                }
            };
            // jdb meldet sich mit "Initializing jdb ..." bzw. bricht mit "Unable to attach" ab
            let mut out = child.stdout.take();
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            let reader = out.take().map(|mut o| {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = o.read(&mut buf) {
                        if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                })
            });
            let mut seen = String::new();
            let attached = loop {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(chunk) => {
                        seen.push_str(&String::from_utf8_lossy(&chunk));
                        if seen.contains("Initializing jdb") || seen.contains("VM Started") || seen.contains("Set uncaught") {
                            break true;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if child.try_wait().ok().flatten().is_some() || Instant::now() > deadline {
                            break false;
                        }
                    }
                    Err(_) => break false,
                }
            };
            if !attached {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(r) = reader {
                    let _ = r.join();
                }
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            let err = child.stderr.take();
            *session.stdin.lock().unwrap() = child.stdin.take();
            *session.child.lock().unwrap() = Some(child);
            // Haltepunkte setzen, dann starten
            for (file, line) in all_breakpoints() {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                if let Some(class) = class_for(&file, &text) {
                    session.send(&format!("stop at {class}:{}", line + 1));
                }
            }
            session.send("run");
            session.state.lock().unwrap().status = Status::Running;
            // Weiter lesen: was schon kam und alles Folgende
            {
                let s = Arc::clone(&session);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let first = std::io::Cursor::new(seen.into_bytes());
                    let rest = ChannelReader { rx, buf: Vec::new(), pos: 0 };
                    s.read_loop(Box::new(first.chain(rest)), &ctx);
                });
            }
            if let Some(e) = err {
                let s = Arc::clone(&session);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    // Fehlerausgabe nur in die Konsole (das Ende meldet stdout)
                    let mut r = std::io::BufReader::new(e);
                    let mut line = String::new();
                    while std::io::BufRead::read_line(&mut r, &mut line).is_ok_and(|n| n > 0) {
                        s.state.lock().unwrap().console.push(line.trim_end().to_string());
                        line.clear();
                        ctx.request_repaint();
                    }
                });
            }
            return;
        }
    });
}

/// Liest, was ein anderer Thread aus jdb gelesen hat
struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Sitzung beenden (jdb und das Programm)
pub fn stop() {
    if let Some(s) = SESSION.lock().unwrap().take() {
        s.send("exit");
        std::thread::sleep(Duration::from_millis(50));
        if let Some(mut c) = s.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        s.state.lock().unwrap().status = Status::Ended;
    }
}

/// Aus jdb-Zeilen die Eingabeaufforderung entfernen ("main[1] ", "> ")
fn strip_prompt(line: &str) -> &str {
    let mut l = line;
    loop {
        let t = l.trim_start_matches("> ");
        let t = match t.find("] ") {
            Some(i) if t[..i].contains('[') && !t[..i].contains(' ') && t[..i].split('[').nth(1).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())) => &t[i + 2..],
            _ => t,
        };
        if t == l {
            return t;
        }
        l = t;
    }
}

/// `Breakpoint hit: "thread=main", app.Main.main(), line=8 bci=12` -> ("app.Main.main", 8)
fn parse_stop(line: &str) -> Option<(String, usize)> {
    let rest = line.strip_prefix("Breakpoint hit: ").or_else(|| line.strip_prefix("Step completed: "))?;
    let mut parts = rest.split(", ");
    let _thread = parts.next()?;
    let method = parts.next()?.trim_end_matches("()").to_string();
    let l = parts.next()?.strip_prefix("line=")?.split_whitespace().next()?.replace(['.', ','], "");
    Some((method, l.parse().ok()?))
}

/// `  [1] app.Main.main (Main.java:8)` -> ("app.Main.main", "Main.java", 8)
fn parse_frame(line: &str) -> Option<(String, String, Option<usize>)> {
    let t = line.trim_start();
    let rest = t.strip_prefix('[')?;
    let (_, rest) = rest.split_once("] ")?;
    let (method, loc) = rest.rsplit_once(" (")?;
    let loc = loc.trim_end_matches(')');
    let (file, line) = match loc.rsplit_once(':') {
        Some((f, l)) => (f.to_string(), l.replace(['.', ','], "").parse().ok()),
        None => (loc.to_string(), None),
    };
    Some((method.trim().to_string(), file, line))
}

impl Session {
    pub fn send(&self, cmd: &str) {
        if let Some(stdin) = self.stdin.lock().unwrap().as_mut() {
            let _ = stdin.write_all(format!("{cmd}\n").as_bytes());
            let _ = stdin.flush();
        }
        self.state.lock().unwrap().console.push(format!("> {cmd}"));
    }

    fn fail(&self, msg: impl Into<String>) {
        let mut st = self.state.lock().unwrap();
        st.error = Some(msg.into());
        st.status = Status::Ended;
    }

    /// Datei zu Klasse und Dateiname suchen (in den Quellordnern)
    fn file_for(&self, method: &str, file_name: Option<&str>) -> Option<PathBuf> {
        // app.Main.main -> app/Main.java (innere Klassen: app.Main$Inner.run -> app/Main.java)
        let class = method.rsplit_once('.').map(|(c, _)| c).unwrap_or(method);
        let class = class.split('$').next().unwrap_or(class);
        let pkg_path: PathBuf = class.rsplit_once('.').map(|(p, _)| p.split('.').collect()).unwrap_or_default();
        let name = file_name.map(str::to_string).unwrap_or_else(|| format!("{}.java", class.rsplit('.').next().unwrap_or(class)));
        self.roots.iter().map(|r| r.join(&pkg_path).join(&name)).find(|p| p.is_file())
    }

    pub fn resume(&self) {
        self.running();
        self.send("cont");
    }
    pub fn step_over(&self) {
        self.running();
        self.send("next");
    }
    pub fn step_into(&self) {
        self.running();
        self.send("step");
    }
    pub fn step_out(&self) {
        self.running();
        self.send("step up");
    }
    /// Ausdruck auswerten (Ergebnis erscheint in der Konsole)
    pub fn eval(&self, expr: &str) {
        if !expr.trim().is_empty() {
            self.send(&format!("print {}", expr.trim()));
        }
    }
    /// Felder eines Objekts zeigen
    pub fn dump(&self, name: &str) {
        self.send(&format!("dump {name}"));
    }

    fn running(&self) {
        let mut st = self.state.lock().unwrap();
        if matches!(st.status, Status::Paused { .. }) {
            st.status = Status::Running;
        }
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    fn read_loop(&self, mut stream: Box<dyn Read + Send>, ctx: &eframe::egui::Context) {
        let mut buf = [0u8; 4096];
        let mut pending = String::new();
        // Abschnitt der Ausgabe: Variablen oder Aufrufliste
        let mut section = 0u8;
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            pending.push_str(&String::from_utf8_lossy(&buf[..n]));
            while let Some(i) = pending.find('\n') {
                let raw: String = pending.drain(..=i).collect();
                let line = strip_prompt(raw.trim_end_matches(['\n', '\r'])).to_string();
                self.handle_line(&line, &mut section);
            }
            ctx.request_repaint();
        }
        let mut st = self.state.lock().unwrap();
        st.status = Status::Ended;
        ctx.request_repaint();
    }

    fn handle_line(&self, line: &str, section: &mut u8) {
        let mut st = self.state.lock().unwrap();
        if !line.trim().is_empty() {
            st.console.push(line.to_string());
            let n = st.console.len();
            if n > 2000 {
                st.console.drain(..n - 2000);
            }
        }
        if let Some((method, l)) = parse_stop(line) {
            let file = self.file_for(&method, None);
            st.status = Status::Paused { method, file, line: l.saturating_sub(1) };
            st.locals.clear();
            st.stack.clear();
            drop(st);
            // Variablen und Aufrufliste gleich nachfragen
            self.send("locals");
            self.send("where");
            return;
        }
        if line.starts_with("The application exited") || line.starts_with("The application has been disconnected") {
            st.status = Status::Ended;
            return;
        }
        match line.trim() {
            "Method arguments:" | "Local variables:" => {
                *section = 1;
                return;
            }
            _ => {}
        }
        if let Some((method, file, l)) = parse_frame(line) {
            *section = 2;
            let path = self.file_for(&method, Some(&file));
            st.stack.push(Frame { method, file: path, line: l.map(|x| x.saturating_sub(1)) });
            return;
        }
        if *section == 1
            && let Some((name, value)) = line.split_once(" = ")
            && !name.trim().is_empty()
            && !name.contains(' ')
        {
            let name = name.trim().to_string();
            if let Some(v) = st.locals.iter_mut().find(|(n, _)| *n == name) {
                v.1 = value.to_string();
            } else {
                st.locals.push((name, value.to_string()));
            }
            return;
        }
        if line.contains("Local variable information not available") {
            st.locals.push((crate::i18n::tr("(keine Angaben)", "(not available)").into(), crate::i18n::tr("ohne -g übersetzt", "compiled without -g").into()));
        }
        *section = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jdb_lines_are_understood() {
        assert_eq!(strip_prompt("main[1] Method arguments:"), "Method arguments:");
        assert_eq!(strip_prompt("main[1] > Set deferred breakpoint app.Main:14"), "Set deferred breakpoint app.Main:14");
        assert_eq!(strip_prompt("> "), "");
        assert_eq!(strip_prompt("x = a[1] b"), "x = a[1] b");
        assert_eq!(parse_stop("Breakpoint hit: \"thread=main\", app.Main.main(), line=8 bci=12"), Some(("app.Main.main".into(), 8)));
        assert_eq!(parse_stop("Step completed: \"thread=main\", app.Main.helper(), line=1.014 bci=0"), Some(("app.Main.helper".into(), 1014)));
        assert_eq!(parse_frame("  [1] app.Main.main (Main.java:8)"), Some(("app.Main.main".into(), "Main.java".into(), Some(8))));
        assert_eq!(parse_frame("  [2] jdk.internal.reflect.X.invoke (native method)"), Some(("jdk.internal.reflect.X.invoke".into(), "native method".into(), None)));
    }

    /// Mit einem echten JDK: anhalten, Variablen lesen, weiterlaufen
    #[test]
    fn debugging_a_program_stops_at_breakpoints() {
        let cfg = super::super::Config { jdk: "system".into(), ..Default::default() };
        let tools = super::super::detect(&std::env::temp_dir().join("easymysql-kein-werkzeug-ordner"), &cfg);
        let Some(jdk) = tools.jdk else { return };
        if !jdk.jdb().is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("easymysql-debug-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src/app");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("Main.java");
        std::fs::write(&file, "package app;\n\npublic class Main {\n    public static void main(String[] args) {\n        int x = 41;\n        x++;\n        System.out.println(x);\n    }\n}\n").unwrap();
        let out = dir.join("out");
        assert!(Command::new(jdk.javac()).arg("-g").arg("-d").arg(&out).arg(&file).status().unwrap().success());
        let port = free_port();
        let mut prog = Command::new(jdk.java())
            .arg(format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"))
            .arg("-cp")
            .arg(&out)
            .arg("app.Main")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        toggle_breakpoint(&file, 5, None);
        start(&jdk.jdb(), port, vec![dir.join("src")], &eframe::egui::Context::default());
        let wait = |f: &dyn Fn(&State) -> bool| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(30) {
                if let Some(s) = session()
                    && f(&s.state.lock().unwrap())
                {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            false
        };
        assert!(wait(&|s| matches!(&s.status, Status::Paused { line: 5, file: Some(f), .. } if f.ends_with("Main.java"))), "{:?}", session().map(|s| s.state.lock().unwrap().clone()));
        assert!(wait(&|s| s.locals.iter().any(|(n, v)| n == "x" && v == "41") && !s.stack.is_empty()));
        session().unwrap().resume();
        assert!(wait(&|s| s.status == Status::Ended));
        stop();
        let _ = prog.wait();
        toggle_breakpoint(&file, 5, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
