// JavaScript/TypeScript mit Node.js: eigenes Node (neueste LTS) oder das des Systems, Treiber (mysql2 bzw.
// mariadb) in einem eigenen Ordner (NODE_PATH), Projekte mit package.json und automatischem `npm install`.
// Vor dem Start prueft `node --check` die Syntax.

pub mod project;

use super::process::{Diag, Failure, Launch, capture, find_up, fingerprint, tail, tool_path};
use super::{Config, EXE, Runtime, Tools, choice, split_args, version_key, versioned_dirs, which};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Mindestens diese Version (fetch, node:test, top-level await in Modulen)
pub const MIN_NODE: u64 = 18;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Driver {
    Mysql2,
    Mariadb,
}

impl Driver {
    pub const ALL: [Driver; 2] = [Driver::Mysql2, Driver::Mariadb];

    pub fn id(self) -> &'static str {
        match self {
            Driver::Mysql2 => "mysql2",
            Driver::Mariadb => "mariadb",
        }
    }

    pub fn from_id(s: &str) -> Driver {
        if s == "mariadb" { Driver::Mariadb } else { Driver::Mysql2 }
    }

    /// Name auf npm
    pub fn package(self) -> &'static str {
        self.id()
    }

    pub fn label(self) -> &'static str {
        match self {
            Driver::Mysql2 => "mysql2",
            Driver::Mariadb => "MariaDB Connector/Node.js",
        }
    }
}

pub fn node_dir() -> PathBuf {
    super::tool_root().join("node")
}

/// Ordner mit den Treibern (NODE_PATH)
pub fn modules_dir() -> PathBuf {
    node_dir().join("lib").join("node_modules")
}

/// Version aus package.json eines Pakets
pub fn package_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["version"].as_str().map(str::to_string)
}

fn exe_in(home: &Path) -> Option<PathBuf> {
    [home.join("bin").join("node"), home.join(format!("node{EXE}"))].into_iter().find(|p| p.is_file())
}

fn npm_in(home: &Path) -> Option<PathBuf> {
    [home.join("bin").join("npm"), home.join("npm.cmd")].into_iter().find(|p| p.is_file() || p.is_symlink())
}

pub(super) fn detect(dir: &Path, cfg: &Config, t: &mut Tools) {
    let (own_ok, system_ok, fixed) = choice(&cfg.node);
    let accept = |exe: PathBuf, own: bool, t: &mut Tools| -> bool {
        let home = if own { exe.parent().and_then(|b| if b.ends_with("bin") { b.parent() } else { Some(b) }).map(Path::to_path_buf).unwrap_or_default() } else { exe.parent().map(Path::to_path_buf).unwrap_or_default() };
        let version = if own {
            home.file_name().map(|n| n.to_string_lossy().trim_start_matches("node-v").split('-').next().unwrap_or("").to_string()).unwrap_or_default()
        } else {
            super::probe(&exe, &["--version"]).map(|v| v.trim().trim_start_matches('v').to_string()).unwrap_or_default()
        };
        let rt = Runtime { home: home.clone(), exe: exe.clone(), version: version.clone(), own };
        if version_key(&version).first().copied().unwrap_or(0) < MIN_NODE {
            t.bad_node = Some((rt, crate::tr_format!("Node.js {version} ist zu alt (mindestens {MIN_NODE})", "Node.js {version} is too old (needs {MIN_NODE} or newer)")));
            return false;
        }
        t.npm = if own { npm_in(&home) } else { exe.parent().and_then(npm_in_dir).or_else(|| which("npm")) };
        t.node = Some(rt);
        true
    };
    let mut found = false;
    if let Some(p) = fixed {
        let exe = if p.is_dir() { exe_in(&p).unwrap_or(p.clone()) } else { p.clone() };
        found = accept(exe, false, t);
    }
    if !found && own_ok {
        for (_, home) in versioned_dirs(dir, "node-v") {
            if let Some(exe) = exe_in(&home) {
                found = accept(exe, true, t);
                break;
            }
        }
    }
    if !found && system_ok && let Some(exe) = which("node") {
        accept(exe, false, t);
    }
    for d in Driver::ALL {
        if let Some(v) = package_version(&dir.join("lib").join("node_modules").join(d.package())) {
            t.node_drivers.push((d, v));
        }
    }
}

fn npm_in_dir(bin: &Path) -> Option<PathBuf> {
    [bin.join("npm"), bin.join("npm.cmd")].into_iter().find(|p| p.is_file())
}

// ---------------------------------------------------------------------------
// Ausfuehren

pub struct Request {
    pub file: PathBuf,
    pub project: Option<PathBuf>,
    pub tools: Arc<Tools>,
    pub cfg: Config,
    pub live: Arc<Mutex<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Run,
    Test,
}

/// Projektordner mit package.json
pub fn project_root(file: &Path, limit: Option<&Path>) -> Option<PathBuf> {
    find_up(file, &["package.json"], limit)
}

/// Paketverwaltung fuer ein Projekt: Einstellung, sonst npm
fn package_manager(tools: &Tools, cfg: &Config, root: &Path) -> Option<PathBuf> {
    let wanted = if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else {
        cfg.package_manager.as_str()
    };
    match wanted {
        "pnpm" | "yarn" => which(wanted).or_else(|| tools.npm.clone()),
        _ => tools.npm.clone(),
    }
}

fn base_cmd(exe: &Path, tools: &Tools) -> Command {
    let mut cmd = Command::new(exe);
    if let Some(p) = tool_path(tools) {
        cmd.env("PATH", p);
    }
    cmd.env("NO_UPDATE_NOTIFIER", "1").env("npm_config_fund", "false").env("npm_config_audit", "false");
    cmd
}

/// `npm install`, wenn node_modules fehlt oder sich package.json geaendert hat
fn ensure_packages(req: &Request, root: &Path) -> Result<(), Failure> {
    let pkg = std::fs::read(root.join("package.json")).unwrap_or_default();
    let lock = std::fs::read(root.join("package-lock.json")).unwrap_or_default();
    let stamp = root.join("node_modules").join(".easymysql-pakete");
    let print = fingerprint(&[&pkg, &lock]);
    let has_deps = serde_json::from_slice::<serde_json::Value>(&pkg).ok().is_some_and(|v| ["dependencies", "devDependencies"].iter().any(|k| v[k].as_object().is_some_and(|o| !o.is_empty())));
    if !has_deps || std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == print) {
        return Ok(());
    }
    let pm = package_manager(&req.tools, &req.cfg, root).ok_or_else(|| Failure::msg(crate::i18n::tr("npm fehlt.", "npm is missing.")))?;
    *req.live.lock().unwrap() = crate::i18n::tr("Pakete werden installiert (npm install) …", "Installing packages (npm install) …").into();
    let mut cmd = base_cmd(&pm, &req.tools);
    cmd.current_dir(root).arg("install");
    let (ok, out) = capture(cmd, &req.live).map_err(Failure::msg)?;
    if !ok {
        return Err(Failure { message: crate::i18n::tr("npm install ist fehlgeschlagen.", "npm install failed.").into(), diags: vec![], output: tail(&out, 40) });
    }
    let _ = std::fs::write(&stamp, print);
    Ok(())
}

/// Ausgabe von `node --check`:
/// ```text
/// /p/app.js:3
///   let x = ;
///           ^
///
/// SyntaxError: Unexpected token ';'
/// ```
pub fn parse_check(output: &str, file: &Path) -> Vec<Diag> {
    let lines: Vec<&str> = output.lines().collect();
    let Some(head) = lines.iter().position(|l| l.rsplit_once(':').is_some_and(|(p, n)| !p.is_empty() && n.trim().parse::<usize>().is_ok())) else { return Vec::new() };
    let (path, num) = lines[head].rsplit_once(':').unwrap();
    let line: usize = num.trim().parse().unwrap_or(1);
    let col = lines.get(head + 2).filter(|l| l.trim() == "^" || l.trim().chars().all(|c| c == '^')).and_then(|l| l.find('^'));
    let message = lines.iter().find(|l| l.contains("Error:") && !l.starts_with(' ')).map(|l| l.trim().to_string()).unwrap_or_else(|| "SyntaxError".into());
    let p = PathBuf::from(path.trim());
    vec![Diag { file: if p.as_os_str().is_empty() { file.to_path_buf() } else { p }, line: line.saturating_sub(1), col, message, error: true }]
}

fn is_typescript(file: &Path) -> bool {
    file.extension().is_some_and(|e| matches!(e.to_string_lossy().as_ref(), "ts" | "mts" | "cts"))
}

/// Optionen fuer TypeScript: ab Node 23.6 laeuft .ts ohne weitere Angaben, davor mit dem Schalter
fn ts_flags(version: &str) -> Vec<String> {
    let k = version_key(version);
    let (major, minor) = (k.first().copied().unwrap_or(0), k.get(1).copied().unwrap_or(0));
    if major > 23 || (major == 23 && minor >= 6) {
        Vec::new()
    } else if major > 22 || (major == 22 && minor >= 6) {
        vec!["--experimental-strip-types".into()]
    } else {
        Vec::new()
    }
}

pub fn prepare(req: &Request, mode: Mode) -> Result<Launch, Failure> {
    let rt = req.tools.node.clone().ok_or_else(|| Failure::msg(crate::i18n::tr("Kein Node.js gefunden.", "No Node.js found.")))?;
    let root = project_root(&req.file, req.project.as_deref());
    let dir = req.file.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Some(r) = &root
        && req.cfg.npm_auto_install
    {
        ensure_packages(req, r)?;
    }
    let ts = is_typescript(&req.file);
    // `node --check` kennt keine Typangaben: TypeScript meldet Fehler erst beim Start
    if req.cfg.check_before_run && mode == Mode::Run && !ts {
        *req.live.lock().unwrap() = crate::i18n::tr("Syntax wird geprüft …", "Checking syntax …").into();
        let mut cmd = base_cmd(&rt.exe, &req.tools);
        cmd.arg("--check").arg(&req.file);
        let (ok, out) = capture(cmd, &req.live).map_err(Failure::msg)?;
        if !ok {
            let diags = parse_check(&out, &req.file);
            return Err(Failure { message: crate::i18n::tr("Syntaxfehler – das Programm wurde nicht gestartet.", "Syntax error – the program was not started.").into(), diags, output: tail(&out, 20) });
        }
    }
    let project_dir = root.clone().unwrap_or_else(|| req.project.clone().filter(|p| dir.starts_with(p)).unwrap_or(dir.clone()));
    let cwd = if req.cfg.cwd_project { project_dir.clone() } else { dir };
    match mode {
        Mode::Run => {
            let mut args = split_args(&req.cfg.node_args);
            if ts {
                args.extend(ts_flags(&rt.version));
            }
            args.push(req.file.display().to_string());
            args.extend(split_args(&req.cfg.program_args));
            let title = req.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Ok(Launch::new(title, rt.exe, args, cwd))
        }
        Mode::Test => {
            let title = crate::i18n::tr("Tests", "Tests");
            // Eigenes Testskript des Projekts, sonst der eingebaute Testlaeufer
            let script = root.as_ref().and_then(|r| std::fs::read_to_string(r.join("package.json")).ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v["scripts"]["test"].as_str().map(str::to_string));
            if let (Some(s), Some(r)) = (&script, &root)
                && !s.contains("no test specified")
                && let Some(pm) = package_manager(&req.tools, &req.cfg, r)
            {
                return Ok(Launch::new(title, pm, vec!["test".into()], r.clone()));
            }
            let mut args = split_args(&req.cfg.node_args);
            args.push("--test".into());
            Ok(Launch::new(title, rt.exe, args, project_dir))
        }
    }
}

pub fn needed_tools(file: &Path, project: Option<&Path>) -> Vec<super::Tool> {
    use super::Tool;
    if project_root(file, project).is_some() { vec![Tool::Node] } else { vec![Tool::Node, Tool::NodeDrivers] }
}

/// Pakete mit npm in einen Ordner installieren (Treiber, Sprachserver). Liefert die Ausgabe.
pub fn npm_install(tools: &Tools, prefix: &Path, packages: &[&str], live: &Mutex<String>) -> Result<(), String> {
    let npm = tools.npm.clone().ok_or_else(|| crate::i18n::tr("npm fehlt.", "npm is missing.").to_string())?;
    std::fs::create_dir_all(prefix).map_err(|e| e.to_string())?;
    // Ohne package.json legt npm die Pakete sonst im naechsten Elternordner mit package.json ab
    let pj = prefix.join("package.json");
    if !pj.is_file() {
        std::fs::write(&pj, "{\n  \"name\": \"easymysql-tools\",\n  \"private\": true\n}\n").map_err(|e| e.to_string())?;
    }
    let mut cmd = base_cmd(&npm, tools);
    cmd.current_dir(prefix).args(["install", "--no-audit", "--no-fund", "--save"]);
    for p in packages {
        cmd.arg(format!("{p}@latest"));
    }
    let (ok, out) = capture(cmd, live)?;
    if ok { Ok(()) } else { Err(tail(&out, 12)) }
}

/// Neueste Version eines Pakets auf npm
pub fn latest_on_npm(package: &str) -> Result<String, String> {
    let v = super::install::get_json(&format!("https://registry.npmjs.org/{package}/latest"))?;
    v["version"].as_str().map(str::to_string).ok_or_else(|| format!("npm: {package}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_check_output_is_parsed() {
        let out = "/p/app.js:3\n  let x = ;\n          ^\n\nSyntaxError: Unexpected token ';'\n    at wrapSafe (node:internal/modules/cjs/loader:1469:18)\n\nNode.js v24.11.0\n";
        let d = parse_check(out, Path::new("/p/app.js"));
        assert_eq!(d, vec![Diag { file: "/p/app.js".into(), line: 2, col: Some(10), message: "SyntaxError: Unexpected token ';'".into(), error: true }]);
        let win = "C:\\p\\app.js:1\n{\n^\n\nSyntaxError: Unexpected end of input\n";
        assert_eq!(parse_check(win, Path::new("C:\\p\\app.js"))[0].file, PathBuf::from("C:\\p\\app.js"));
        assert_eq!(ts_flags("22.11.0"), vec!["--experimental-strip-types".to_string()]);
        assert!(ts_flags("24.11.0").is_empty());
    }
}
