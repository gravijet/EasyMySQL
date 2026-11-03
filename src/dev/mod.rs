// Entwicklungswerkzeuge fuer Java, Python und JavaScript: finden oder selbst herunterladen (immer die
// neueste stabile Version) und Programme mit allem Noetigen starten.
//
// Eigene Werkzeuge liegen im Programmdatenordner:
//   java/          jdk-25.0.5+8/, apache-maven-3.10.0/, gradle-9.8.2/, lib/ (JDBC-Treiber, JUnit)
//   python/        python-3.14.0/ (python-build-standalone), site/ (PyMySQL, mysql-connector-python)
//   node/          node-v24.11.0-linux-x64/, lib/node_modules/ (mysql2, mariadb)
//   sprachserver/  jdtls-1.52.0/ (Eclipse JDT LS), npm/node_modules/ (pyright, typescript-language-server)
//   git/           git-2.51.0/ (MinGit, nur Windows), gh_2.83.0_…/ (GitHub CLI)

pub mod debug;
#[cfg(test)]
mod e2e;
pub mod install;
pub mod java;
pub mod libs;
pub mod lsp;
pub mod names;
pub mod node;
pub mod process;
pub mod python;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[cfg(windows)]
pub const EXE: &str = ".exe";
#[cfg(not(windows))]
pub const EXE: &str = "";

/// Was EasyMySQL selbst herunterladen kann
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tool {
    Jdk,
    /// JDBC-Treiber (MariaDB und MySQL Connector/J)
    JavaDrivers,
    Maven,
    Gradle,
    /// JUnit (Konsolenstarter) fuer Tests in einfachen Projekten
    JUnit,
    Python,
    /// Gewaehlter Python-Treiber (PyMySQL bzw. mysql-connector-python)
    PythonDrivers,
    Node,
    /// Gewaehlter Node-Treiber (mysql2 bzw. mariadb)
    NodeDrivers,
    /// Eclipse JDT Language Server
    JavaLsp,
    /// Pyright
    PythonLsp,
    /// typescript-language-server mit TypeScript
    JsLsp,
    /// Git (nur unter Windows ladbar: MinGit)
    Git,
    /// GitHub CLI (Anmeldung im Browser)
    GhCli,
}

impl Tool {
    pub const ALL: [Tool; 14] = [
        Tool::Jdk,
        Tool::JavaDrivers,
        Tool::Maven,
        Tool::Gradle,
        Tool::JUnit,
        Tool::Python,
        Tool::PythonDrivers,
        Tool::Node,
        Tool::NodeDrivers,
        Tool::JavaLsp,
        Tool::PythonLsp,
        Tool::JsLsp,
        Tool::Git,
        Tool::GhCli,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Jdk => "JDK",
            Tool::JavaDrivers => crate::i18n::tr("JDBC-Treiber", "JDBC drivers"),
            Tool::Maven => "Maven",
            Tool::Gradle => "Gradle",
            Tool::JUnit => "JUnit",
            Tool::Python => "Python",
            Tool::PythonDrivers => crate::i18n::tr("Python-Treiber", "Python driver"),
            Tool::Node => "Node.js",
            Tool::NodeDrivers => crate::i18n::tr("Node-Treiber", "Node driver"),
            Tool::JavaLsp => crate::i18n::tr("Java-Sprachserver", "Java language server"),
            Tool::PythonLsp => crate::i18n::tr("Python-Sprachserver", "Python language server"),
            Tool::JsLsp => crate::i18n::tr("JavaScript-Sprachserver", "JavaScript language server"),
            Tool::Git => "Git",
            Tool::GhCli => "GitHub CLI",
        }
    }

    /// Werkzeuge, die vorher da sein muessen (Treiber fuer Python brauchen pip, npm-Pakete brauchen Node)
    pub fn requires(self) -> &'static [Tool] {
        match self {
            Tool::PythonDrivers => &[Tool::Python],
            Tool::NodeDrivers | Tool::PythonLsp | Tool::JsLsp => &[Tool::Node],
            Tool::JavaLsp => &[Tool::Jdk],
            _ => &[],
        }
    }
}

// ---------------------------------------------------------------------------
// Einstellungen (aus settings.rs uebernommen, im Hintergrund lesbar)

/// Wie oft nach neueren Werkzeugen gesucht wird
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdatePolicy {
    Never,
    Daily,
    Weekly,
    Monthly,
}

impl UpdatePolicy {
    pub const ALL: [UpdatePolicy; 4] = [UpdatePolicy::Never, UpdatePolicy::Daily, UpdatePolicy::Weekly, UpdatePolicy::Monthly];

    pub fn id(self) -> &'static str {
        match self {
            UpdatePolicy::Never => "nie",
            UpdatePolicy::Daily => "taeglich",
            UpdatePolicy::Weekly => "woechentlich",
            UpdatePolicy::Monthly => "monatlich",
        }
    }

    pub fn from_id(s: &str) -> Self {
        Self::ALL.into_iter().find(|p| p.id() == s).unwrap_or(UpdatePolicy::Weekly)
    }

    pub fn label(self) -> &'static str {
        match self {
            UpdatePolicy::Never => crate::i18n::tr("Nie", "Never"),
            UpdatePolicy::Daily => crate::i18n::tr("Täglich", "Daily"),
            UpdatePolicy::Weekly => crate::i18n::tr("Wöchentlich", "Weekly"),
            UpdatePolicy::Monthly => crate::i18n::tr("Monatlich", "Monthly"),
        }
    }

    pub fn interval(self) -> Option<std::time::Duration> {
        let day = 24 * 3600;
        match self {
            UpdatePolicy::Never => None,
            UpdatePolicy::Daily => Some(std::time::Duration::from_secs(day)),
            UpdatePolicy::Weekly => Some(std::time::Duration::from_secs(7 * day)),
            UpdatePolicy::Monthly => Some(std::time::Duration::from_secs(30 * day)),
        }
    }
}

/// Alles, was sich an Java, Python, JavaScript und dem Ausfuehren einstellen laesst.
/// Werkzeugwahl: "" = automatisch (eigenes, sonst vom System), "own", "system" oder ein Pfad.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub java_driver: java::Driver,
    pub jdk: String,
    /// Zusaetzliche Optionen fuer javac (z. B. -Xlint:all)
    pub javac_args: String,
    /// Optionen fuer die JVM beim Starten (z. B. -Xmx512m)
    pub jvm_args: String,
    /// --release fuer javac (0 = Version des JDK)
    pub java_release: u32,
    pub maven_args: String,
    pub gradle_args: String,
    /// Maven/Gradle ohne Netz (-o / --offline)
    pub build_offline: bool,
    /// mvnw/gradlew des Projekts verwenden, wenn vorhanden
    pub use_wrapper: bool,
    /// Projekte von EasyMySQL bekommen JUnit-Tests
    pub junit: bool,

    pub python_driver: python::Driver,
    pub python: String,
    pub python_args: String,
    /// Projekte mit requirements.txt/pyproject.toml in einer eigenen .venv ausfuehren
    pub python_venv: bool,
    pub pip_args: String,

    pub node_driver: node::Driver,
    pub node: String,
    pub node_args: String,
    /// Fehlende Pakete eines Projekts vor dem Start mit npm installieren
    pub npm_auto_install: bool,
    /// npm, pnpm oder yarn
    pub package_manager: String,

    /// Argumente fuer jedes gestartete Programm
    pub program_args: String,
    /// Zusaetzliche Umgebungsvariablen (NAME=Wert je Zeile)
    pub program_env: String,
    /// Vor dem Starten alle Dateien speichern
    pub save_before_run: bool,
    /// Vor dem Starten auf Syntaxfehler pruefen (Python, JavaScript)
    pub check_before_run: bool,
    /// Ein neuer Lauf ersetzt die Registerkarte des vorigen
    pub reuse_terminal: bool,
    /// Arbeitsordner: Projekt (true) oder Ordner der Datei
    pub cwd_project: bool,
    /// Verbindungsdaten der gewaehlten Datenbank als EASYMYSQL_DB_* an Programme geben
    pub pass_connection: bool,

    pub updates: UpdatePolicy,
    /// Fehlende Werkzeuge ohne Nachfrage laden
    pub auto_download: bool,

    pub lsp_java: bool,
    pub lsp_python: bool,
    pub lsp_js: bool,
    /// Arbeitsspeicher fuer den Java-Sprachserver
    pub lsp_java_memory_mb: u32,
    /// Wartezeit nach dem Tippen, bis der Sprachserver den Text bekommt
    pub lsp_delay_ms: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            java_driver: java::Driver::MariaDb,
            jdk: String::new(),
            javac_args: String::new(),
            jvm_args: String::new(),
            java_release: 0,
            maven_args: String::new(),
            gradle_args: String::new(),
            build_offline: false,
            use_wrapper: true,
            junit: true,
            python_driver: python::Driver::PyMySql,
            python: String::new(),
            python_args: String::new(),
            python_venv: true,
            pip_args: String::new(),
            node_driver: node::Driver::Mysql2,
            node: String::new(),
            node_args: String::new(),
            npm_auto_install: true,
            package_manager: "npm".into(),
            program_args: String::new(),
            program_env: String::new(),
            save_before_run: true,
            check_before_run: true,
            reuse_terminal: true,
            cwd_project: true,
            pass_connection: true,
            updates: UpdatePolicy::Weekly,
            auto_download: true,
            lsp_java: true,
            lsp_python: true,
            lsp_js: true,
            lsp_java_memory_mb: 1024,
            lsp_delay_ms: 400,
        }
    }
}

impl Config {
    /// Zeilen fuer einstellungen.txt (Schluessel mit Praefix dev_)
    pub fn to_lines(&self) -> String {
        let b = |x: bool| if x { "1" } else { "0" };
        let e = crate::settings::escape;
        let mut s = String::new();
        let mut kv = |k: &str, v: String| s.push_str(&format!("dev_{k}={v}\n"));
        kv("java_driver", self.java_driver.id().into());
        kv("jdk", e(&self.jdk));
        kv("javac_args", e(&self.javac_args));
        kv("jvm_args", e(&self.jvm_args));
        kv("java_release", self.java_release.to_string());
        kv("maven_args", e(&self.maven_args));
        kv("gradle_args", e(&self.gradle_args));
        kv("build_offline", b(self.build_offline).into());
        kv("use_wrapper", b(self.use_wrapper).into());
        kv("junit", b(self.junit).into());
        kv("python_driver", self.python_driver.id().into());
        kv("python", e(&self.python));
        kv("python_args", e(&self.python_args));
        kv("python_venv", b(self.python_venv).into());
        kv("pip_args", e(&self.pip_args));
        kv("node_driver", self.node_driver.id().into());
        kv("node", e(&self.node));
        kv("node_args", e(&self.node_args));
        kv("npm_auto_install", b(self.npm_auto_install).into());
        kv("package_manager", e(&self.package_manager));
        kv("program_args", e(&self.program_args));
        kv("program_env", e(&self.program_env));
        kv("save_before_run", b(self.save_before_run).into());
        kv("check_before_run", b(self.check_before_run).into());
        kv("reuse_terminal", b(self.reuse_terminal).into());
        kv("cwd_project", b(self.cwd_project).into());
        kv("pass_connection", b(self.pass_connection).into());
        kv("updates", self.updates.id().into());
        kv("auto_download", b(self.auto_download).into());
        kv("lsp_java", b(self.lsp_java).into());
        kv("lsp_python", b(self.lsp_python).into());
        kv("lsp_js", b(self.lsp_js).into());
        kv("lsp_java_memory_mb", self.lsp_java_memory_mb.to_string());
        kv("lsp_delay_ms", self.lsp_delay_ms.to_string());
        s
    }

    /// Einen Schluessel (ohne dev_) uebernehmen; false, wenn unbekannt
    pub fn set(&mut self, key: &str, v: &str) -> bool {
        let on = v == "1";
        let text = crate::settings::unescape(v);
        match key {
            "java_driver" => self.java_driver = java::Driver::from_id(v),
            "jdk" => self.jdk = text,
            "javac_args" => self.javac_args = text,
            "jvm_args" => self.jvm_args = text,
            "java_release" => self.java_release = v.parse().unwrap_or(0),
            "maven_args" => self.maven_args = text,
            "gradle_args" => self.gradle_args = text,
            "build_offline" => self.build_offline = on,
            "use_wrapper" => self.use_wrapper = on,
            "junit" => self.junit = on,
            "python_driver" => self.python_driver = python::Driver::from_id(v),
            "python" => self.python = text,
            "python_args" => self.python_args = text,
            "python_venv" => self.python_venv = on,
            "pip_args" => self.pip_args = text,
            "node_driver" => self.node_driver = node::Driver::from_id(v),
            "node" => self.node = text,
            "node_args" => self.node_args = text,
            "npm_auto_install" => self.npm_auto_install = on,
            "package_manager" if !v.is_empty() => self.package_manager = text,
            "program_args" => self.program_args = text,
            "program_env" => self.program_env = text,
            "save_before_run" => self.save_before_run = on,
            "check_before_run" => self.check_before_run = on,
            "reuse_terminal" => self.reuse_terminal = on,
            "cwd_project" => self.cwd_project = on,
            "pass_connection" => self.pass_connection = on,
            "updates" => self.updates = UpdatePolicy::from_id(v),
            "auto_download" => self.auto_download = on,
            "lsp_java" => self.lsp_java = on,
            "lsp_python" => self.lsp_python = on,
            "lsp_js" => self.lsp_js = on,
            "lsp_java_memory_mb" => self.lsp_java_memory_mb = v.parse().unwrap_or(1024).clamp(256, 16384),
            "lsp_delay_ms" => self.lsp_delay_ms = v.parse().unwrap_or(400).clamp(50, 5000),
            _ => return false,
        }
        true
    }

    /// Zusaetzliche Umgebungsvariablen aus `program_env`
    pub fn env(&self) -> Vec<(String, String)> {
        self.program_env
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.is_empty() || l.starts_with('#') {
                    return None;
                }
                let (k, v) = l.split_once('=')?;
                let k = k.trim().trim_start_matches("export ").trim();
                (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
            })
            .collect()
    }
}

static CONFIG: RwLock<Option<Config>> = RwLock::new(None);

pub fn config() -> Config {
    CONFIG.read().unwrap().clone().unwrap_or_default()
}

/// Neue Einstellungen; bei anderer Werkzeugwahl wird neu gesucht
pub fn set_config(c: Config) {
    let old = config();
    let changed_tools = old.jdk != c.jdk || old.python != c.python || old.node != c.node || old.python_driver != c.python_driver || old.node_driver != c.node_driver;
    *CONFIG.write().unwrap() = Some(c);
    if changed_tools {
        refresh();
    }
}

/// Argumente aus einer Zeile wie in einer Shell: Leerzeichen trennen, "…" und '…' halten zusammen.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                any = true;
            }
            None if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            None => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Gefundene Werkzeuge

/// Maven, Gradle, Git, … (ein Programm in einem Ordner)
#[derive(Clone, Debug, PartialEq)]
pub struct Dist {
    pub home: PathBuf,
    /// Leer, wenn unbekannt (Installation aus dem System)
    pub version: String,
    pub own: bool,
    /// Startdatei (bin/mvn, bin/gradle bzw. .cmd/.bat unter Windows)
    pub launcher: PathBuf,
}

/// Python oder Node.js
#[derive(Clone, Debug, PartialEq)]
pub struct Runtime {
    pub home: PathBuf,
    pub exe: PathBuf,
    pub version: String,
    pub own: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tools {
    /// Brauchbares JDK (mindestens `java::MIN_JDK`)
    pub jdk: Option<java::Jdk>,
    /// Gefunden, aber zu alt
    pub old_jdk: Option<java::Jdk>,
    /// Heruntergeladene Treiber: (Treiber, Version, Datei)
    pub drivers: Vec<(java::Driver, String, PathBuf)>,
    pub maven: Option<Dist>,
    pub gradle: Option<Dist>,
    /// junit-platform-console-standalone: (Version, Datei)
    pub junit: Option<(String, PathBuf)>,
    pub python: Option<Runtime>,
    /// Gefunden, aber zu alt oder ohne venv/pip
    pub bad_python: Option<(Runtime, String)>,
    /// Installierte Python-Treiber: (Treiber, Version)
    pub python_drivers: Vec<(python::Driver, String)>,
    pub node: Option<Runtime>,
    pub bad_node: Option<(Runtime, String)>,
    /// npm (bzw. npm.cmd) passend zu `node`
    pub npm: Option<PathBuf>,
    pub node_drivers: Vec<(node::Driver, String)>,
    /// Sprachserver: (Version, Ordner bzw. Startdatei)
    pub jdtls: Option<(String, PathBuf)>,
    pub pyright: Option<(String, PathBuf)>,
    pub tsls: Option<(String, PathBuf)>,
    pub git: Option<Dist>,
    pub gh: Option<Dist>,
}

impl Tools {
    pub fn has(&self, t: Tool) -> bool {
        let cfg = config();
        match t {
            Tool::Jdk => self.jdk.is_some(),
            Tool::JavaDrivers => java::Driver::ALL.iter().all(|d| self.driver(*d).is_some()),
            Tool::Maven => self.maven.is_some(),
            Tool::Gradle => self.gradle.is_some(),
            Tool::JUnit => self.junit.is_some(),
            Tool::Python => self.python.is_some(),
            Tool::PythonDrivers => self.python_drivers.iter().any(|d| d.0 == cfg.python_driver),
            Tool::Node => self.node.is_some() && self.npm.is_some(),
            Tool::NodeDrivers => self.node_drivers.iter().any(|d| d.0 == cfg.node_driver),
            Tool::JavaLsp => self.jdtls.is_some(),
            Tool::PythonLsp => self.pyright.is_some(),
            Tool::JsLsp => self.tsls.is_some(),
            Tool::Git => self.git.is_some(),
            Tool::GhCli => self.gh.is_some(),
        }
    }

    pub fn driver(&self, d: java::Driver) -> Option<&(java::Driver, String, PathBuf)> {
        self.drivers.iter().find(|x| x.0 == d)
    }

    /// Alle Treiber-Dateien (fuer den Klassenpfad)
    pub fn driver_jars(&self) -> Vec<PathBuf> {
        self.drivers.iter().map(|d| d.2.clone()).collect()
    }

    /// Installierte Version eines eigenen Werkzeugs (fuer die Suche nach Updates)
    pub fn own_version(&self, t: Tool) -> Option<String> {
        let own = |d: &Option<Dist>| d.as_ref().filter(|m| m.own).map(|m| m.version.clone());
        let own_rt = |r: &Option<Runtime>| r.as_ref().filter(|m| m.own).map(|m| m.version.clone());
        match t {
            Tool::Jdk => self.jdk.as_ref().filter(|j| j.own).map(|j| j.version.clone()),
            Tool::JavaDrivers => self.driver(java::Driver::MariaDb).map(|d| d.1.clone()),
            Tool::Maven => own(&self.maven),
            Tool::Gradle => own(&self.gradle),
            Tool::JUnit => self.junit.as_ref().map(|j| j.0.clone()),
            Tool::Python => own_rt(&self.python),
            Tool::PythonDrivers => self.python_drivers.first().map(|d| d.1.clone()),
            Tool::Node => own_rt(&self.node),
            Tool::NodeDrivers => self.node_drivers.first().map(|d| d.1.clone()),
            Tool::JavaLsp => self.jdtls.as_ref().map(|j| j.0.clone()),
            Tool::PythonLsp => self.pyright.as_ref().map(|j| j.0.clone()),
            Tool::JsLsp => self.tsls.as_ref().map(|j| j.0.clone()),
            Tool::Git => own(&self.git),
            Tool::GhCli => own(&self.gh),
        }
    }

    /// Ordner, die beim Starten von Programmen und in Terminals vorne in PATH stehen
    pub fn path_dirs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(j) = &self.jdk {
            out.push(j.bin());
        }
        for d in [&self.maven, &self.gradle, &self.git, &self.gh].into_iter().flatten() {
            if let Some(bin) = d.launcher.parent() {
                out.push(bin.to_path_buf());
            }
        }
        for r in [&self.python, &self.node].into_iter().flatten() {
            if let Some(bin) = r.exe.parent() {
                out.push(bin.to_path_buf());
            }
        }
        // Python unter Windows: Scripts\ (pip.exe)
        if let Some(p) = self.python.as_ref().filter(|p| p.own)
            && p.home.join("Scripts").is_dir()
        {
            out.push(p.home.join("Scripts"));
        }
        out.dedup();
        out
    }

    /// Umgebung fuer Terminals und Programme: JAVA_HOME, Klassenpfad mit den Treibern, PYTHONPATH und
    /// NODE_PATH mit den Treibern – so funktionieren `java Main.java`, `python app.py` und `node app.js`
    /// ohne weitere Angaben.
    pub fn shell_env(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(j) = &self.jdk {
            out.push(("JAVA_HOME".into(), j.home.display().to_string()));
        }
        let jars = self.driver_jars();
        if !jars.is_empty() {
            let mut cp = vec![".".to_string()];
            cp.extend(jars.iter().map(|p| p.display().to_string()));
            out.push(("CLASSPATH".into(), cp.join(java::CP_SEP)));
        }
        if !self.python_drivers.is_empty() {
            out.push(("PYTHONPATH".into(), python::site_dir().display().to_string()));
        }
        out.push(("PYTHONIOENCODING".into(), "utf-8".into()));
        if !self.node_drivers.is_empty() {
            out.push(("NODE_PATH".into(), node::modules_dir().display().to_string()));
        }
        out
    }
}

/// Ordner fuer die eigenen Werkzeuge einer Art
pub fn tool_root() -> PathBuf {
    crate::server::app_data_dir()
}

static TOOLS: RwLock<Option<Arc<Tools>>> = RwLock::new(None);

/// Gefundene Werkzeuge (einmal gesucht, nach Installationen und neuen Einstellungen neu)
pub fn tools() -> Arc<Tools> {
    if let Some(t) = TOOLS.read().unwrap().as_ref() {
        return Arc::clone(t);
    }
    let t = Arc::new(detect(&tool_root(), &config()));
    *TOOLS.write().unwrap() = Some(Arc::clone(&t));
    t
}

/// Nach einer Installation neu suchen
pub fn refresh() {
    *TOOLS.write().unwrap() = None;
}

// ---------------------------------------------------------------------------
// Versionen

/// Zahlen einer Version zum Vergleichen: "25.0.5+8" -> [25, 0, 5, 8]
pub fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Stabile Version? Vorabversionen ("4.0.0-rc-7", "9.0-beta1", "3.0.0-M5", "3.15.0a1") nicht.
pub fn is_stable(v: &str) -> bool {
    !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Neueste stabile Version aus einer Liste
pub fn newest_stable<'a>(versions: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    versions.into_iter().filter(|v| is_stable(v)).max_by(|a, b| version_key(a).cmp(&version_key(b)))
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    version_key(candidate) > version_key(installed)
}

// ---------------------------------------------------------------------------
// Suche

/// Programm im PATH suchen (unter Windows ohne die Platzhalter des Microsoft Store)
pub fn which(name: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat")] } else { vec![name.to_string()] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        for n in &names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Unterordner von `dir`, deren Name mit `prefix` beginnt, neueste Version zuerst
pub fn versioned_dirs(dir: &Path, prefix: &str) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let version = name.strip_prefix(prefix)?.to_string();
                    version.starts_with(|c: char| c.is_ascii_digit()).then(|| (version, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|x| std::cmp::Reverse(version_key(&x.0)));
    v
}

/// Startdatei `name` in `home/bin` (unter Windows .cmd/.bat/.exe)
pub fn launcher(home: &Path, name: &str) -> Option<PathBuf> {
    let bin = home.join("bin");
    let candidates: Vec<String> = if cfg!(windows) { vec![format!("{name}.cmd"), format!("{name}.bat"), format!("{name}.exe")] } else { vec![name.to_string()] };
    candidates.into_iter().map(|n| bin.join(n)).find(|p| p.is_file())
}

/// Ausgabe eines kurzen Aufrufs (stdout und stderr), None bei Fehlschlag
pub fn probe(exe: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).stdin(std::process::Stdio::null());
    crate::server::hidden(&mut cmd);
    let out = cmd.output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    out.status.success().then_some(text)
}

/// Werkzeugwahl auswerten: (eigene erlaubt, System erlaubt, fester Pfad)
fn choice(s: &str) -> (bool, bool, Option<PathBuf>) {
    match s.trim() {
        "" => (true, true, None),
        "own" => (true, false, None),
        "system" => (false, true, None),
        p => (false, false, Some(PathBuf::from(p))),
    }
}

/// Alle Werkzeuge suchen: zuerst die eigenen, dann die des Systems (je nach Einstellung).
pub fn detect(root: &Path, cfg: &Config) -> Tools {
    let mut t = Tools::default();
    java::detect(&root.join("java"), cfg, &mut t);
    python::detect(&root.join("python"), cfg, &mut t);
    node::detect(&root.join("node"), cfg, &mut t);
    let lsp = root.join("sprachserver");
    if let Some((v, home)) = versioned_dirs(&lsp, "jdtls-").into_iter().next() {
        t.jdtls = Some((v, home));
    }
    let npm_pkg = |name: &str, bin: &str| -> Option<(String, PathBuf)> {
        let dir = lsp.join("npm").join("node_modules").join(name);
        let v = node::package_version(&dir)?;
        Some((v, dir.join(bin)))
    };
    t.pyright = npm_pkg("pyright", "langserver.index.js");
    // TypeScript ab 7 hat einen eigenen, nativen Sprachserver (tsc --lsp); sonst typescript-language-server
    let native = std::fs::read_dir(lsp.join("npm/node_modules/@typescript")).ok().and_then(|rd| {
        rd.flatten().map(|e| e.path().join("lib").join(format!("tsc{EXE}"))).find(|p| p.is_file())
    });
    t.tsls = match (native, node::package_version(&lsp.join("npm/node_modules/typescript"))) {
        (Some(exe), Some(v)) => Some((v, exe)),
        _ => npm_pkg("typescript-language-server", "lib/cli.mjs"),
    };

    // Git: eigenes MinGit (Windows), sonst aus dem System
    let git_dir = root.join("git");
    t.git = versioned_dirs(&git_dir, "git-")
        .into_iter()
        .find_map(|(version, home)| {
            let launcher = home.join("cmd").join(format!("git{EXE}"));
            launcher.is_file().then_some(Dist { home, version, own: true, launcher })
        })
        .or_else(|| {
            let found = which("git")?;
            Some(Dist { home: found.parent().map(Path::to_path_buf).unwrap_or_default(), version: String::new(), own: false, launcher: found })
        });
    t.gh = versioned_dirs(&git_dir, "gh_")
        .into_iter()
        .find_map(|(version, home)| {
            let launcher = home.join("bin").join(format!("gh{EXE}"));
            let version = version.split('_').next().unwrap_or("").to_string();
            launcher.is_file().then_some(Dist { home, version, own: true, launcher })
        })
        .or_else(|| {
            let found = which("gh")?;
            Some(Dist { home: found.parent().map(Path::to_path_buf).unwrap_or_default(), version: String::new(), own: false, launcher: found })
        });
    t
}

/// Umgebung fuer ein Programm, das aus EasyMySQL gestartet wird: die Verbindung zur Datenbank. Der erzeugte
/// Code liest diese Werte zuerst, so stehen keine Passwoerter in den Dateien.
pub fn db_env(conn: &crate::db::ConnInfo, database: &str) -> Vec<(String, String)> {
    vec![
        ("EASYMYSQL_DB_URL".into(), java::jdbc_url(java::driver(), &conn.host, conn.port, database)),
        ("EASYMYSQL_DB_HOST".into(), conn.host.clone()),
        ("EASYMYSQL_DB_PORT".into(), conn.port.to_string()),
        ("EASYMYSQL_DB_NAME".into(), database.to_string()),
        ("EASYMYSQL_DB_USER".into(), conn.user.clone()),
        ("EASYMYSQL_DB_PASSWORD".into(), conn.password.clone()),
        ("EASYMYSQL_DB_SSL_CA".into(), crate::db::local_tls_ca(&conn.host, conn.port).map(|p| p.display().to_string()).unwrap_or_default()),
    ]
}

/// Adresse auf diesem PC? (dann ist TLS meist nicht noetig bzw. nicht verfuegbar)
pub fn is_local(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_prereleases_are_skipped() {
        assert!(is_newer("3.10.0", "3.9.16"));
        assert!(!is_newer("3.9.16", "3.10.0"));
        assert!(is_newer("25.0.5+8", "25.0.5"));
        assert!(is_stable("26.10.0") && !is_stable("4.0.0-rc-7") && !is_stable("3.0.0-M5") && !is_stable("") && !is_stable("3.15.0a1"));
        let list = ["3.9.16", "4.0.0-rc-7", "3.10.0", "4.0.0-beta-5"];
        assert_eq!(newest_stable(list), Some("3.10.0"));
    }

    #[test]
    fn arguments_are_split_like_a_shell() {
        assert_eq!(split_args(r#"-Xmx1g "-Dname=a b" '' x"#), vec!["-Xmx1g", "-Dname=a b", "", "x"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn config_roundtrip_and_env_lines() {
        let mut c = Config { jvm_args: "-Xmx1g\n-ea".into(), python_venv: false, updates: UpdatePolicy::Monthly, program_env: "# x\nA=1\nexport B = zwei\n=leer\n".into(), ..Default::default() };
        c.node_driver = node::Driver::Mariadb;
        let mut back = Config::default();
        for line in c.to_lines().lines() {
            let (k, v) = line.split_once('=').unwrap();
            assert!(back.set(k.strip_prefix("dev_").unwrap(), v), "{k}");
        }
        assert_eq!(back, c);
        assert_eq!(c.env(), vec![("A".to_string(), "1".to_string()), ("B".to_string(), "zwei".to_string())]);
    }

    #[test]
    fn own_tools_are_found_with_the_newest_version_first() {
        let root = std::env::temp_dir().join(format!("easymysql-dev-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("java");
        for (name, ver) in [("jdk-21.0.2+13", "21.0.2"), ("jdk-25.0.5+8", "25.0.5")] {
            let bin = dir.join(name).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join(format!("javac{EXE}")), "").unwrap();
            std::fs::write(dir.join(name).join("release"), format!("JAVA_VERSION=\"{ver}\"\n")).unwrap();
        }
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        for f in ["mariadb-java-client-3.5.11.jar", "mariadb-java-client-4.0.0.jar", "mariadb-java-client-4.1.0-rc-1.jar", "mysql-connector-j-26.10.0.jar", "junit-platform-console-standalone-6.0.1.jar"] {
            std::fs::write(dir.join("lib").join(f), "").unwrap();
        }
        let mvn = dir.join("apache-maven-3.10.0").join("bin");
        std::fs::create_dir_all(&mvn).unwrap();
        std::fs::write(mvn.join(if cfg!(windows) { "mvn.cmd" } else { "mvn" }), "").unwrap();
        // Python- und Node-Treiber
        std::fs::create_dir_all(root.join("python/site/PyMySQL-1.1.2.dist-info")).unwrap();
        let m2 = root.join("node/lib/node_modules/mysql2");
        std::fs::create_dir_all(&m2).unwrap();
        std::fs::write(m2.join("package.json"), r#"{"name":"mysql2","version":"3.15.3"}"#).unwrap();

        let cfg = Config { jdk: "own".into(), python: "own".into(), node: "own".into(), ..Default::default() };
        let t = detect(&root, &cfg);
        let jdk = t.jdk.clone().unwrap();
        assert!(jdk.own && jdk.major == 25 && jdk.version == "25.0.5", "{jdk:?}");
        assert_eq!(t.driver(java::Driver::MariaDb).unwrap().1, "4.0.0");
        assert_eq!(t.driver(java::Driver::MySql).unwrap().1, "26.10.0");
        assert_eq!(t.junit.as_ref().unwrap().0, "6.0.1");
        let m = t.maven.clone().unwrap();
        assert!(m.own && m.version == "3.10.0");
        assert_eq!(t.python_drivers, vec![(python::Driver::PyMySql, "1.1.2".to_string())]);
        assert_eq!(t.node_drivers, vec![(node::Driver::Mysql2, "3.15.3".to_string())]);
        assert!(t.python.is_none() && t.node.is_none(), "nur eigene erlaubt, keine vorhanden");
        let _ = std::fs::remove_dir_all(&root);
    }
}
