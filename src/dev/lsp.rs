// Sprachserver (Language Server Protocol): Fehler schon beim Tippen, Gehe zu Definition, Umbenennen ueber
// das ganze Projekt und Hinweise beim Zeigen. Java: Eclipse JDT LS, Python: Pyright,
// JavaScript/TypeScript: typescript-language-server. Je Sprache und Projektordner laeuft ein Server.

use super::Tools;
use crate::sqledit::Lang;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Meldung zu einer Stelle: (von, bis) als (Zeile, Spalte in UTF-16), Text, Fehler?
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub message: String,
    pub error: bool,
}

/// Textaenderung: (von, bis, neuer Text), Positionen als (Zeile, Spalte in UTF-16)
pub type TextEdit = ((usize, usize), (usize, usize), String);

/// Antwort auf eine Anfrage an den Server
pub type Reply = Receiver<Result<Value, String>>;

type DiagMap = Arc<Mutex<HashMap<String, (u64, Vec<Diagnostic>)>>>;

/// Aenderung an einer Datei: (von, bis, neuer Text) mit Positionen wie oben
#[derive(Clone, Debug, PartialEq)]
pub struct FileEdit {
    pub path: PathBuf,
    pub edits: Vec<TextEdit>,
}

/// Ergebnis eines Umbenennens: Textaenderungen und umbenannte Dateien (Java: Klasse -> Datei)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceEdit {
    pub files: Vec<FileEdit>,
    pub renames: Vec<(PathBuf, PathBuf)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Starting,
    Ready,
    Failed(String),
}

pub struct Server {
    pub lang: Lang,
    pub root: PathBuf,
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Result<Value, String>>>>,
    /// Meldungen je Datei-URI mit Zaehler (aendert sich bei jeder neuen Meldung)
    diags: DiagMap,
    counter: AtomicI64,
    versions: Mutex<HashMap<String, i64>>,
    pub phase: Mutex<Phase>,
    /// Letzte Fortschrittsmeldung des Servers ("Importing Maven project …")
    pub progress: Mutex<String>,
    settings: Value,
    /// Der Server liefert Meldungen nur auf Anfrage (textDocument/diagnostic, z. B. TypeScript 7)
    pull: std::sync::atomic::AtomicBool,
    ctx: eframe::egui::Context,
}

static SERVERS: Mutex<Vec<Arc<Server>>> = Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// Adressen und Positionen

/// Datei -> file://-URI (mit Prozentkodierung)
pub fn uri(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let p = if p.starts_with('/') { p } else { format!("/{p}") };
    let mut out = String::from("file://");
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    // Windows: "file:///C:/…" mit kleinem Laufwerksbuchstaben wie VS Code waere auch erlaubt
    out
}

/// file://-URI -> Datei (None bei jdt:// u. ae.)
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    // "/C:/x" -> "C:/x"
    let s = if s.len() > 3 && s.as_bytes()[0] == b'/' && s.as_bytes()[2] == b':' { s[1..].to_string() } else { s };
    Some(PathBuf::from(s))
}

/// Zeichenindex -> (Zeile, Spalte in UTF-16-Einheiten)
pub fn to_pos(text: &str, index: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i >= index {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += c.len_utf16();
        }
    }
    (line, col)
}

/// (Zeile, Spalte in UTF-16) -> Zeichenindex
pub fn from_pos(text: &str, pos: (usize, usize)) -> usize {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if line == pos.0 && (col >= pos.1 || c == '\n') {
            return i;
        }
        if c == '\n' {
            if line == pos.0 {
                return i;
            }
            line += 1;
            col = 0;
        } else if line == pos.0 {
            col += c.len_utf16();
        }
    }
    text.chars().count()
}

fn lsp_pos(text: &str, index: usize) -> Value {
    let (l, c) = to_pos(text, index);
    json!({ "line": l, "character": c })
}

fn read_pos(v: &Value) -> (usize, usize) {
    (v["line"].as_u64().unwrap_or(0) as usize, v["character"].as_u64().unwrap_or(0) as usize)
}

// ---------------------------------------------------------------------------
// Starten

/// Projektordner, fuer den der Server laeuft
pub fn root_for(lang: Lang, file: &Path, project: Option<&Path>) -> PathBuf {
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let found = match lang {
        Lang::Java => {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            Some(super::java::run::detect_build(file, super::java::source::package(&text).as_deref(), project).root())
        }
        Lang::Python => super::python::project_root(file, project),
        Lang::JavaScript | Lang::TypeScript => super::node::project_root(file, project),
        _ => None,
    };
    found.or_else(|| project.filter(|p| file.starts_with(p)).map(Path::to_path_buf)).unwrap_or(dir)
}

/// Welches Werkzeug braucht der Server dieser Sprache?
pub fn tool_for(lang: Lang) -> Option<super::Tool> {
    match lang {
        Lang::Java => Some(super::Tool::JavaLsp),
        Lang::Python => Some(super::Tool::PythonLsp),
        Lang::JavaScript | Lang::TypeScript => Some(super::Tool::JsLsp),
        _ => None,
    }
}


/// Ist der Server fuer diese Sprache eingeschaltet?
pub fn enabled(lang: Lang) -> bool {
    let c = super::config();
    match lang {
        Lang::Java => c.lsp_java,
        Lang::Python => c.lsp_python,
        Lang::JavaScript | Lang::TypeScript => c.lsp_js,
        _ => false,
    }
}

fn os_config() -> &'static str {
    if cfg!(windows) {
        "config_win"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { "config_mac_arm" } else { "config_mac" }
    } else if cfg!(target_arch = "aarch64") {
        "config_linux_arm"
    } else {
        "config_linux"
    }
}

/// Befehl und Einstellungen fuer den Server
fn command(lang: Lang, root: &Path, tools: &Tools) -> Result<(Command, Value), String> {
    let cfg = super::config();
    match lang {
        Lang::Java => {
            let (_, home) = tools.jdtls.clone().ok_or_else(|| crate::i18n::tr("Java-Sprachserver fehlt.", "Java language server is missing.").to_string())?;
            let jdk = tools.jdk.as_ref().ok_or_else(|| crate::i18n::tr("Kein JDK gefunden.", "No JDK found.").to_string())?;
            if jdk.major < 21 {
                return Err(crate::tr_format!("Der Java-Sprachserver braucht Java 21 oder neuer (gefunden: {}).", "The Java language server needs Java 21 or newer (found: {}).", jdk.version));
            }
            let launcher = std::fs::read_dir(home.join("plugins"))
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("org.eclipse.equinox.launcher_")))
                .ok_or("JDT LS: launcher")?;
            // Arbeitsordner je Projekt (Index, Einstellungen) ausserhalb des Projekts
            let data = super::process::scratch_dir("jdtls", root);
            let _ = std::fs::create_dir_all(&data);
            let mut cmd = Command::new(jdk.java());
            cmd.args([
                "-Declipse.application=org.eclipse.jdt.ls.core.id1",
                "-Dosgi.bundles.defaultStartLevel=4",
                "-Declipse.product=org.eclipse.jdt.ls.core.product",
                "-Dlog.level=WARNING",
                &format!("-Xmx{}m", cfg.lsp_java_memory_mb),
                "--add-modules=ALL-SYSTEM",
                "--add-opens",
                "java.base/java.util=ALL-UNNAMED",
                "--add-opens",
                "java.base/java.lang=ALL-UNNAMED",
                &format!("-Duser.language={}", if crate::i18n::is_english() { "en" } else { "de" }),
                "-jar",
            ])
            .arg(launcher)
            .arg("-configuration")
            .arg(home.join(os_config()))
            .arg("-data")
            .arg(data);
            // Treiber und lib/*.jar im Klassenpfad einfacher Projekte
            let mut libs: Vec<String> = tools.driver_jars().iter().map(|p| p.display().to_string()).collect();
            libs.push("lib/**/*.jar".into());
            if let Some((_, junit)) = &tools.junit {
                libs.push(junit.display().to_string());
            }
            let mut java = json!({
                "project": { "referencedLibraries": libs, "sourcePaths": ["src", "test"] },
                "configuration": { "updateBuildConfiguration": "automatic" },
                "import": { "gradle": { "enabled": true }, "maven": { "enabled": true } },
                "errors": { "incompleteClasspath": { "severity": "warning" } },
            });
            if let Some(g) = tools.gradle.as_ref().filter(|g| g.own) {
                java["import"]["gradle"]["home"] = json!(g.home.display().to_string());
            }
            if let Some(m) = tools.maven.as_ref().filter(|m| m.own) {
                java["configuration"]["maven"] = json!({ "globalSettings": m.home.join("conf/settings.xml").display().to_string() });
            }
            Ok((cmd, json!({ "settings": { "java": java }, "extendedClientCapabilities": { "classFileContentsSupport": false } })))
        }
        Lang::Python => {
            let (_, script) = tools.pyright.clone().ok_or_else(|| crate::i18n::tr("Python-Sprachserver fehlt.", "Python language server is missing.").to_string())?;
            let node = tools.node.as_ref().ok_or("Node.js")?;
            let mut cmd = Command::new(&node.exe);
            cmd.arg(script).arg("--stdio");
            Ok((cmd, Value::Null))
        }
        _ => {
            let (_, script) = tools.tsls.clone().ok_or_else(|| crate::i18n::tr("JavaScript-Sprachserver fehlt.", "JavaScript language server is missing.").to_string())?;
            // Nativer Server von TypeScript 7
            if script.file_stem().is_some_and(|n| n == "tsc") {
                let mut cmd = Command::new(&script);
                cmd.args(["--lsp", "--stdio"]);
                return Ok((cmd, Value::Null));
            }
            let node = tools.node.as_ref().ok_or("Node.js")?;
            let tsserver = script.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map(|nm| nm.join("typescript").join("lib").join("tsserver.js"));
            let mut cmd = Command::new(&node.exe);
            cmd.arg(script).arg("--stdio");
            let opts = match tsserver.filter(|p| p.is_file()) {
                Some(p) => json!({ "tsserver": { "path": p.display().to_string() }, "preferences": { "includeCompletionsForModuleExports": true } }),
                None => Value::Null,
            };
            Ok((cmd, opts))
        }
    }
}

/// Einstellungen fuer workspace/configuration (Pyright fragt nach dem Python und den Suchpfaden)
fn configuration(lang: Lang, root: &Path, tools: &Tools, section: &str) -> Value {
    if lang != Lang::Python {
        return Value::Null;
    }
    let venv = super::python::exe_in(&root.join(".venv"));
    let python = venv.or_else(|| tools.python.as_ref().map(|p| p.exe.clone()));
    let extra: Vec<String> = if root.join(".venv").is_dir() { vec![] } else { vec![super::python::site_dir().display().to_string()] };
    match section {
        "python" => json!({ "pythonPath": python.map(|p| p.display().to_string()), "analysis": { "extraPaths": extra, "typeCheckingMode": "basic" } }),
        "python.analysis" => json!({ "extraPaths": extra, "typeCheckingMode": "basic", "diagnosticMode": "openFilesOnly" }),
        _ => Value::Null,
    }
}

/// Server fuer diese Sprache und diesen Ordner (startet ihn bei Bedarf). None, wenn ausgeschaltet oder das
/// Werkzeug fehlt.
pub fn get(lang: Lang, root: &Path, ctx: &eframe::egui::Context) -> Option<Arc<Server>> {
    let lang = if lang == Lang::TypeScript { Lang::JavaScript } else { lang };
    if !enabled(lang) {
        return None;
    }
    {
        let mut list = SERVERS.lock().unwrap();
        // Beendete Server entfernen
        list.retain(|s| !s.exited());
        if let Some(s) = list.iter().find(|s| s.lang == lang && s.root == root) {
            return Some(Arc::clone(s));
        }
    }
    let tools = super::tools();
    if !tools.has(tool_for(lang)?) {
        return None;
    }
    let (mut cmd, init_options) = command(lang, root, &tools).ok()?;
    if let Some(p) = super::process::tool_path(&tools) {
        cmd.env("PATH", p);
    }
    crate::server::hidden(&mut cmd);
    let mut child = cmd.current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take();
    let server = Arc::new(Server {
        lang,
        root: root.to_path_buf(),
        stdin: Mutex::new(stdin),
        child: Mutex::new(child),
        next_id: AtomicI64::new(1),
        pending: Mutex::new(HashMap::new()),
        diags: Arc::new(Mutex::new(HashMap::new())),
        counter: AtomicI64::new(0),
        versions: Mutex::new(HashMap::new()),
        phase: Mutex::new(Phase::Starting),
        progress: Mutex::new(String::new()),
        settings: init_options.get("settings").cloned().unwrap_or(Value::Null),
        pull: std::sync::atomic::AtomicBool::new(false),
        ctx: ctx.clone(),
    });
    {
        let s = Arc::clone(&server);
        let tools = Arc::clone(&tools);
        std::thread::spawn(move || s.read_loop(stdout, &tools));
    }
    if let Some(err) = stderr {
        std::thread::spawn(move || {
            for _ in BufReader::new(err).lines().map_while(Result::ok) {}
        });
    }
    // Initialisieren im Hintergrund (Java braucht dafuer einige Sekunden)
    {
        let s = Arc::clone(&server);
        std::thread::spawn(move || {
            let params = json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "EasyMySQL", "version": env!("CARGO_PKG_VERSION") },
                "rootUri": uri(&s.root),
                "workspaceFolders": [{ "uri": uri(&s.root), "name": s.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }],
                "initializationOptions": init_options,
                "capabilities": {
                    "workspace": { "configuration": true, "workspaceEdit": { "documentChanges": true, "resourceOperations": ["rename"] }, "applyEdit": true, "workspaceFolders": true },
                    "textDocument": {
                        "synchronization": { "didSave": true },
                        "publishDiagnostics": { "relatedInformation": false },
                        "diagnostic": { "dynamicRegistration": false },
                        "definition": { "linkSupport": false },
                        "hover": { "contentFormat": ["plaintext", "markdown"] },
                        "rename": { "prepareSupport": false },
                    },
                    "window": { "workDoneProgress": true },
                },
            });
            match s.request_blocking("initialize", params, std::time::Duration::from_secs(120)) {
                Ok(result) => {
                    s.pull.store(result["capabilities"].get("diagnosticProvider").is_some_and(|d| !d.is_null()), Ordering::Relaxed);
                    s.notify("initialized", json!({}));
                    if !s.settings.is_null() {
                        s.notify("workspace/didChangeConfiguration", json!({ "settings": s.settings }));
                    }
                    *s.phase.lock().unwrap() = Phase::Ready;
                }
                Err(e) => *s.phase.lock().unwrap() = Phase::Failed(e),
            }
            s.ctx.request_repaint();
        });
    }
    SERVERS.lock().unwrap().push(Arc::clone(&server));
    Some(server)
}

/// Alle Server beenden (beim Schliessen oder nach neuen Einstellungen)
pub fn shutdown_all() {
    let list: Vec<Arc<Server>> = std::mem::take(&mut *SERVERS.lock().unwrap());
    for s in list {
        s.shutdown();
    }
}

/// Server einer Sprache beenden (z. B. nach dem Ausschalten)
pub fn shutdown_lang(lang: Lang) {
    let mut list = SERVERS.lock().unwrap();
    let (stop, keep): (Vec<_>, Vec<_>) = list.drain(..).partition(|s| s.lang == lang);
    *list = keep;
    drop(list);
    for s in stop {
        s.shutdown();
    }
}

/// Anzahl der laufenden Server (fuer die Einstellungen)
pub fn running() -> Vec<(Lang, PathBuf, Phase)> {
    SERVERS.lock().unwrap().iter().map(|s| (s.lang, s.root.clone(), s.phase.lock().unwrap().clone())).collect()
}

impl Server {
    fn write(&self, msg: &Value) {
        let body = msg.to_string();
        let mut w = self.stdin.lock().unwrap();
        let _ = write!(w, "Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = w.flush();
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    pub fn request(&self, method: &str, params: Value) -> Reply {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        rx
    }

    fn request_blocking(&self, method: &str, params: Value, timeout: std::time::Duration) -> Result<Value, String> {
        self.request(method, params).recv_timeout(timeout).map_err(|_| crate::tr_format!("{method}: keine Antwort", "{method}: no answer"))?
    }

    pub fn ready(&self) -> bool {
        *self.phase.lock().unwrap() == Phase::Ready
    }

    pub fn exited(&self) -> bool {
        self.child.lock().unwrap().try_wait().ok().flatten().is_some()
    }

    fn shutdown(&self) {
        if self.ready() {
            let _ = self.request("shutdown", Value::Null).recv_timeout(std::time::Duration::from_secs(2));
            self.notify("exit", Value::Null);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = self.child.lock().unwrap().kill();
    }

    /// Datei oeffnen bzw. neuen Text melden (der ganze Text; Server mit Teilaenderungen nehmen das auch)
    pub fn sync(&self, path: &Path, lang: Lang, text: &str) {
        let u = uri(path);
        let mut versions = self.versions.lock().unwrap();
        match versions.get_mut(&u) {
            Some(v) => {
                *v += 1;
                self.notify("textDocument/didChange", json!({ "textDocument": { "uri": u, "version": *v }, "contentChanges": [{ "text": text }] }));
            }
            None => {
                versions.insert(u.clone(), 1);
                self.notify("textDocument/didOpen", json!({ "textDocument": { "uri": u, "languageId": lang.lsp_id(), "version": 1, "text": text } }));
            }
        }
        drop(versions);
        if self.pull.load(Ordering::Relaxed) {
            self.pull_diagnostics(&u);
        }
    }

    /// Meldungen anfordern (Server mit Pull-Diagnosen) und wie gepushte ablegen
    fn pull_diagnostics(&self, u: &str) {
        let rx = self.request("textDocument/diagnostic", json!({ "textDocument": { "uri": u } }));
        let u = u.to_string();
        let counter = self.counter.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        let store = self.diags_handle();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            if let Ok(Ok(v)) = rx.recv_timeout(std::time::Duration::from_secs(60))
                && v["kind"].as_str() != Some("unchanged")
            {
                let list = parse_diagnostics(&v["items"]);
                store.lock().unwrap().insert(u, (counter, list));
                ctx.request_repaint();
            }
        });
    }

    fn diags_handle(&self) -> DiagMap {
        Arc::clone(&self.diags)
    }

    pub fn saved(&self, path: &Path) {
        if self.versions.lock().unwrap().contains_key(&uri(path)) {
            self.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri(path) } }));
        }
    }

    pub fn close(&self, path: &Path) {
        if self.versions.lock().unwrap().remove(&uri(path)).is_some() {
            self.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri(path) } }));
        }
    }

    /// Meldungen zur Datei mit Zaehler (aendert sich bei jeder neuen Meldung)
    pub fn diagnostics(&self, path: &Path) -> Option<(u64, Vec<Diagnostic>)> {
        self.diags.lock().unwrap().get(&uri(path)).cloned()
    }

    fn doc_pos(path: &Path, text: &str, index: usize) -> Value {
        json!({ "textDocument": { "uri": uri(path) }, "position": lsp_pos(text, index) })
    }

    pub fn definition(&self, path: &Path, text: &str, index: usize) -> Reply {
        self.request("textDocument/definition", Self::doc_pos(path, text, index))
    }

    pub fn hover(&self, path: &Path, text: &str, index: usize) -> Reply {
        self.request("textDocument/hover", Self::doc_pos(path, text, index))
    }

    pub fn rename(&self, path: &Path, text: &str, index: usize, new_name: &str) -> Reply {
        let mut p = Self::doc_pos(path, text, index);
        p["newName"] = json!(new_name);
        self.request("textDocument/rename", p)
    }

    pub fn format(&self, path: &Path, indent: usize) -> Reply {
        self.request("textDocument/formatting", json!({ "textDocument": { "uri": uri(path) }, "options": { "tabSize": indent, "insertSpaces": true } }))
    }

    fn read_loop(&self, stdout: impl Read, tools: &Tools) {
        let mut r = BufReader::new(stdout);
        loop {
            // Kopfzeilen bis zur Leerzeile
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                match r.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        self.ended();
                        return;
                    }
                    Ok(_) => {}
                }
                let t = line.trim();
                if t.is_empty() {
                    break;
                }
                if let Some(v) = t.strip_prefix("Content-Length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            if r.read_exact(&mut body).is_err() {
                self.ended();
                return;
            }
            let Ok(msg) = serde_json::from_slice::<Value>(&body) else { continue };
            self.handle(msg, tools);
        }
    }

    fn ended(&self) {
        let mut ph = self.phase.lock().unwrap();
        if *ph != Phase::Ready {
            *ph = Phase::Failed(crate::i18n::tr("Der Sprachserver wurde beendet.", "The language server stopped.").into());
        }
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(crate::i18n::tr("Der Sprachserver wurde beendet.", "The language server stopped.").into()));
        }
        self.ctx.request_repaint();
    }

    fn handle(&self, msg: Value, tools: &Tools) {
        let method = msg["method"].as_str().map(str::to_string);
        match (msg.get("id").cloned(), method) {
            // Antwort auf eine Anfrage
            (Some(id), None) => {
                if let Some(id) = id.as_i64()
                    && let Some(tx) = self.pending.lock().unwrap().remove(&id)
                {
                    let r = match msg.get("error") {
                        Some(e) => Err(e["message"].as_str().unwrap_or("Fehler").to_string()),
                        None => Ok(msg["result"].clone()),
                    };
                    let _ = tx.send(r);
                }
                self.ctx.request_repaint();
            }
            // Anfrage des Servers
            (Some(id), Some(m)) => {
                let result = match m.as_str() {
                    "workspace/configuration" => {
                        let items = msg["params"]["items"].as_array().cloned().unwrap_or_default();
                        Value::Array(
                            items
                                .iter()
                                .map(|i| {
                                    let section = i["section"].as_str().unwrap_or("");
                                    match configuration(self.lang, &self.root, tools, section) {
                                        Value::Null => self.settings.pointer(&format!("/{}", section.replace('.', "/"))).cloned().unwrap_or(Value::Null),
                                        v => v,
                                    }
                                })
                                .collect(),
                        )
                    }
                    "workspace/workspaceFolders" => json!([{ "uri": uri(&self.root), "name": "root" }]),
                    // Aenderungen, die der Server selbst anwenden will (z. B. Imports organisieren), lehnen wir ab
                    "workspace/applyEdit" => json!({ "applied": false }),
                    _ => Value::Null,
                };
                self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }
            (None, Some(m)) => match m.as_str() {
                "textDocument/publishDiagnostics" => {
                    let p = &msg["params"];
                    let u = p["uri"].as_str().unwrap_or("").to_string();
                    let list = parse_diagnostics(&p["diagnostics"]);
                    let n = self.counter.fetch_add(1, Ordering::Relaxed) as u64 + 1;
                    self.diags.lock().unwrap().insert(u, (n, list));
                    self.ctx.request_repaint();
                }
                "$/progress" => {
                    let v = &msg["params"]["value"];
                    let text = match v["kind"].as_str() {
                        Some("end") => String::new(),
                        _ => [v["title"].as_str(), v["message"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(" "),
                    };
                    *self.progress.lock().unwrap() = text;
                    self.ctx.request_repaint();
                }
                "language/status" => {
                    let t = msg["params"]["message"].as_str().unwrap_or("").to_string();
                    *self.progress.lock().unwrap() = if msg["params"]["type"].as_str() == Some("ServiceReady") { String::new() } else { t };
                    self.ctx.request_repaint();
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// Liste von Meldungen (Fehler und Warnungen; Hinweise werden ausgelassen)
fn parse_diagnostics(v: &Value) -> Vec<Diagnostic> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|d| d["severity"].as_u64().unwrap_or(1) <= 2)
                .map(|d| Diagnostic {
                    start: read_pos(&d["range"]["start"]),
                    end: read_pos(&d["range"]["end"]),
                    message: d["message"].as_str().unwrap_or("").to_string(),
                    error: d["severity"].as_u64().unwrap_or(1) == 1,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Ziel einer Definition: (Datei, Zeile, Spalte in UTF-16)
pub fn parse_location(v: &Value) -> Option<(PathBuf, (usize, usize))> {
    let first = if v.is_array() { v.get(0)? } else { v };
    let u = first["uri"].as_str().or(first["targetUri"].as_str())?;
    let range = if first["range"].is_object() { &first["range"] } else { &first["targetSelectionRange"] };
    Some((path_of(u)?, read_pos(&range["start"])))
}

/// Text eines Hover-Ergebnisses (Markdown wird als Text gezeigt, Codebloecke ohne Zaeune)
pub fn hover_text(v: &Value) -> Option<String> {
    let c = &v["contents"];
    let parts: Vec<String> = match c {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string).or_else(|| x["value"].as_str().map(str::to_string))).collect(),
        Value::Object(_) => c["value"].as_str().map(|s| vec![s.to_string()]).unwrap_or_default(),
        _ => vec![],
    };
    let text = parts.join("\n\n").lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n");
    let t = text.trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn text_edits(list: &Value) -> Vec<TextEdit> {
    list.as_array()
        .map(|a| a.iter().map(|e| (read_pos(&e["range"]["start"]), read_pos(&e["range"]["end"]), e["newText"].as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default()
}

/// Ergebnis von textDocument/rename lesen (changes oder documentChanges mit Umbenennungen von Dateien)
pub fn parse_workspace_edit(v: &Value) -> WorkspaceEdit {
    let mut out = WorkspaceEdit::default();
    if let Some(changes) = v["changes"].as_object() {
        for (u, edits) in changes {
            if let Some(path) = path_of(u) {
                out.files.push(FileEdit { path, edits: text_edits(edits) });
            }
        }
    }
    if let Some(list) = v["documentChanges"].as_array() {
        for c in list {
            if c["kind"].as_str() == Some("rename") {
                if let (Some(a), Some(b)) = (c["oldUri"].as_str().and_then(path_of), c["newUri"].as_str().and_then(path_of)) {
                    out.renames.push((a, b));
                }
            } else if let Some(path) = c["textDocument"]["uri"].as_str().and_then(path_of) {
                out.files.push(FileEdit { path, edits: text_edits(&c["edits"]) });
            }
        }
    }
    out
}

/// Aenderungen auf einen Text anwenden (von hinten, damit die Positionen stimmen)
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut ranges: Vec<(usize, usize, &str)> = edits.iter().map(|(a, b, t)| (from_pos(text, *a), from_pos(text, *b), t.as_str())).collect();
    ranges.sort_by(|x, y| y.0.cmp(&x.0).then(y.1.cmp(&x.1)));
    let mut chars: Vec<char> = text.chars().collect();
    for (a, b, t) in ranges {
        let (a, b) = (a.min(chars.len()), b.min(chars.len()).max(a.min(chars.len())));
        chars.splice(a..b, t.chars());
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_positions() {
        let p = Path::new("/home/a b/Übung.java");
        let u = uri(p);
        assert_eq!(u, "file:///home/a%20b/%C3%9Cbung.java");
        assert_eq!(path_of(&u).unwrap(), p);
        assert_eq!(path_of("file:///C:/x/y.java").unwrap(), PathBuf::from("C:/x/y.java"));
        assert!(path_of("jdt://contents/x").is_none());
        let text = "ab\n😀c\nd";
        assert_eq!(to_pos(text, 4), (1, 2));
        assert_eq!(from_pos(text, (1, 2)), 4);
        assert_eq!(from_pos(text, (2, 0)), 6);
        assert_eq!(from_pos(text, (1, 99)), 5, "hinter dem Zeilenende: Zeilenende");
    }

    #[test]
    fn workspace_edits_are_read_and_applied() {
        let v = json!({ "documentChanges": [
            { "textDocument": { "uri": "file:///p/A.java", "version": 1 }, "edits": [
                { "range": { "start": { "line": 0, "character": 13 }, "end": { "line": 0, "character": 14 } }, "newText": "Kunde" },
                { "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 5 } }, "newText": "Kunde" }
            ] },
            { "kind": "rename", "oldUri": "file:///p/A.java", "newUri": "file:///p/Kunde.java" }
        ] });
        let w = parse_workspace_edit(&v);
        assert_eq!(w.renames, vec![(PathBuf::from("/p/A.java"), PathBuf::from("/p/Kunde.java"))]);
        assert_eq!(apply_edits("public class A {\n    A() {}\n}", &w.files[0].edits), "public class Kunde {\n    Kunde() {}\n}");
        let loc = json!([{ "uri": "file:///p/B.java", "range": { "start": { "line": 3, "character": 2 }, "end": { "line": 3, "character": 5 } } }]);
        assert_eq!(parse_location(&loc), Some((PathBuf::from("/p/B.java"), (3, 2))));
        assert_eq!(hover_text(&json!({ "contents": { "kind": "markdown", "value": "```java\nint x\n```\nZahl" } })).as_deref(), Some("int x\nZahl"));
    }
}
