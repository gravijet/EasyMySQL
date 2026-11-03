// Python: eigenes Python (python-build-standalone) oder das des Systems, Treiber (PyMySQL bzw.
// mysql-connector-python) in einem eigenen Ordner, Projekte mit requirements.txt/pyproject.toml in einer
// .venv. Vor dem Start werden alle Dateien auf Syntaxfehler geprueft.

pub mod project;

use super::process::{Diag, Failure, Launch, capture, find_up, fingerprint, tail, tool_path};
use super::{Config, EXE, Runtime, Tools, choice, split_args, version_key, versioned_dirs, which};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Mindestens diese Version (f-Strings, dataclasses, Typangaben)
pub const MIN_PYTHON: [u64; 2] = [3, 9];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Driver {
    PyMySql,
    MysqlConnector,
}

impl Driver {
    pub const ALL: [Driver; 2] = [Driver::PyMySql, Driver::MysqlConnector];

    pub fn id(self) -> &'static str {
        match self {
            Driver::PyMySql => "pymysql",
            Driver::MysqlConnector => "mysql-connector",
        }
    }

    pub fn from_id(s: &str) -> Driver {
        if s == "mysql-connector" { Driver::MysqlConnector } else { Driver::PyMySql }
    }

    /// Name auf PyPI
    pub fn package(self) -> &'static str {
        match self {
            Driver::PyMySql => "PyMySQL",
            Driver::MysqlConnector => "mysql-connector-python",
        }
    }

    /// Name fuer `import`
    pub fn module(self) -> &'static str {
        match self {
            Driver::PyMySql => "pymysql",
            Driver::MysqlConnector => "mysql.connector",
        }
    }

    /// Anfang des .dist-info-Ordners nach der Installation
    fn dist_info(self) -> &'static str {
        match self {
            Driver::PyMySql => "pymysql-",
            Driver::MysqlConnector => "mysql_connector_python-",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Driver::PyMySql => "PyMySQL",
            Driver::MysqlConnector => "MySQL Connector/Python",
        }
    }
}

pub fn python_dir() -> PathBuf {
    super::tool_root().join("python")
}

/// Ordner mit den Treibern (steht als PYTHONPATH bereit)
pub fn site_dir() -> PathBuf {
    python_dir().join("site")
}

/// Python-Programm in einem Ordner (eigenes Python bzw. .venv)
pub fn exe_in(home: &Path) -> Option<PathBuf> {
    [home.join("bin").join("python3"), home.join("bin").join("python"), home.join(format!("python{EXE}")), home.join("Scripts").join(format!("python{EXE}"))].into_iter().find(|p| p.is_file())
}

/// Version eines Python-Programms und ob venv und pip dabei sind (bei Debian fehlen sie oft)
fn probe_python(exe: &Path) -> Result<String, String> {
    let out = super::probe(exe, &["-c", "import sys, venv, ensurepip; print('%d.%d.%d' % sys.version_info[:3])"]);
    match out {
        Some(v) => Ok(v.trim().to_string()),
        None => {
            let v = super::probe(exe, &["-c", "import sys; print('%d.%d.%d' % sys.version_info[:3])"]).map(|v| v.trim().to_string()).unwrap_or_default();
            Err(v)
        }
    }
}

pub(super) fn detect(dir: &Path, cfg: &Config, t: &mut Tools) {
    let (own_ok, system_ok, fixed) = choice(&cfg.python);
    let check = |exe: PathBuf, home: PathBuf, own: bool, t: &mut Tools| -> bool {
        if own {
            let version = home.file_name().map(|n| n.to_string_lossy().trim_start_matches("python-").to_string()).unwrap_or_default();
            t.python = Some(Runtime { home, exe, version, own });
            return true;
        }
        match probe_python(&exe) {
            Ok(v) if version_key(&v) >= MIN_PYTHON.to_vec() => {
                t.python = Some(Runtime { home, exe, version: v, own });
                true
            }
            Ok(v) => {
                t.bad_python = Some((Runtime { home, exe, version: v.clone(), own }, crate::tr_format!("Python {v} ist zu alt (mindestens 3.9)", "Python {v} is too old (needs 3.9 or newer)")));
                false
            }
            Err(v) => {
                t.bad_python = Some((Runtime { home, exe, version: v.clone(), own }, crate::tr_format!("Python {v} ohne venv/pip", "Python {v} without venv/pip")));
                false
            }
        }
    };
    let mut found = false;
    if let Some(p) = fixed {
        let exe = if p.is_dir() { exe_in(&p).unwrap_or(p.clone()) } else { p.clone() };
        let home = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        found = check(exe, home, false, t);
    }
    if !found && own_ok {
        for (_, home) in versioned_dirs(dir, "python-") {
            if let Some(exe) = exe_in(&home) {
                found = check(exe, home, true, t);
                break;
            }
        }
    }
    if !found && system_ok {
        for name in ["python3", "python"] {
            if let Some(exe) = which(name) {
                let home = exe.parent().map(Path::to_path_buf).unwrap_or_default();
                if check(exe, home, false, t) {
                    break;
                }
            }
        }
    }
    // Treiber im eigenen Ordner
    let site = dir.join("site");
    if let Ok(rd) = std::fs::read_dir(&site) {
        let names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        for d in Driver::ALL {
            if let Some(v) = names.iter().filter_map(|n| n.strip_prefix(d.dist_info())?.strip_suffix(".dist-info")).max_by_key(|v| version_key(v)) {
                t.python_drivers.push((d, v.to_string()));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ausfuehren

pub struct Request {
    pub file: PathBuf,
    pub text: String,
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

/// Projektordner: mit requirements.txt, pyproject.toml oder .venv
pub fn project_root(file: &Path, limit: Option<&Path>) -> Option<PathBuf> {
    find_up(file, &["requirements.txt", "pyproject.toml", "setup.py", ".venv"], limit)
}

fn venv_python(root: &Path) -> Option<PathBuf> {
    exe_in(&root.join(".venv"))
}

fn base_cmd(exe: &Path, tools: &Tools) -> Command {
    let mut cmd = Command::new(exe);
    if let Some(p) = tool_path(tools) {
        cmd.env("PATH", p);
    }
    cmd.env("PYTHONIOENCODING", "utf-8").env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
    cmd
}

/// .venv anlegen bzw. die Pakete nachinstallieren, wenn sich requirements.txt/pyproject.toml geaendert hat.
/// Liefert das Python der .venv.
fn ensure_venv(req: &Request, root: &Path, python: &Path) -> Result<PathBuf, Failure> {
    let venv = root.join(".venv");
    if venv_python(root).is_none() {
        *req.live.lock().unwrap() = crate::i18n::tr("Virtuelle Umgebung (.venv) wird angelegt …", "Creating the virtual environment (.venv) …").into();
        let mut cmd = base_cmd(python, &req.tools);
        cmd.current_dir(root).args(["-m", "venv", ".venv"]);
        let (ok, out) = capture(cmd, &req.live).map_err(Failure::msg)?;
        if !ok {
            return Err(Failure { message: crate::i18n::tr("Die .venv konnte nicht angelegt werden.", "Could not create the .venv.").into(), diags: vec![], output: tail(&out, 30) });
        }
    }
    let py = venv_python(root).ok_or_else(|| Failure::msg(crate::i18n::tr("In der .venv fehlt Python.", "Python is missing in the .venv.")))?;
    let req_txt = std::fs::read(root.join("requirements.txt")).unwrap_or_default();
    let pyproject = std::fs::read(root.join("pyproject.toml")).unwrap_or_default();
    if req_txt.is_empty() && pyproject.is_empty() {
        return Ok(py);
    }
    let stamp = venv.join(".easymysql-pakete");
    let print = fingerprint(&[&req_txt, &pyproject, req.cfg.pip_args.as_bytes()]);
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == print) {
        return Ok(py);
    }
    *req.live.lock().unwrap() = crate::i18n::tr("Pakete werden installiert (pip) …", "Installing packages (pip) …").into();
    let mut cmd = base_cmd(&py, &req.tools);
    cmd.current_dir(root).args(["-m", "pip", "install"]);
    if !req_txt.is_empty() {
        cmd.args(["-r", "requirements.txt"]);
    } else {
        cmd.args(["-e", "."]);
    }
    cmd.args(split_args(&req.cfg.pip_args));
    let (ok, out) = capture(cmd, &req.live).map_err(Failure::msg)?;
    if !ok {
        return Err(Failure { message: crate::i18n::tr("pip konnte die Pakete nicht installieren.", "pip could not install the packages.").into(), diags: vec![], output: tail(&out, 40) });
    }
    let _ = std::fs::write(&stamp, print);
    Ok(py)
}

/// Prueft alle .py-Dateien unter `root` auf Syntaxfehler (ohne sie auszufuehren)
const CHECK_SCRIPT: &str = r#"
import os, sys
root = sys.argv[1]
skip = {'.venv', 'venv', '__pycache__', 'node_modules', 'build', 'dist', '.git'}
n = 0
for d, dirs, files in os.walk(root):
    dirs[:] = [x for x in dirs if x not in skip and not x.startswith('.')]
    for f in files:
        if not f.endswith('.py'):
            continue
        n += 1
        if n > 3000:
            sys.exit(0)
        p = os.path.join(d, f)
        try:
            with open(p, 'rb') as h:
                compile(h.read(), p, 'exec', dont_inherit=True)
        except SyntaxError as e:
            msg = (e.msg or 'SyntaxError').replace('\t', ' ').replace('\n', ' ')
            print('%s\t%d\t%d\t%s\t%s' % (p, e.lineno or 1, e.offset or 0, type(e).__name__, msg))
        except (ValueError, UnicodeDecodeError) as e:
            print('%s\t1\t0\tError\t%s' % (p, str(e).replace('\t', ' ')))
"#;

/// Zeilen der Syntaxpruefung: Datei, Zeile, Spalte, Art, Meldung
pub fn parse_check(output: &str) -> Vec<Diag> {
    output
        .lines()
        .filter_map(|l| {
            let mut it = l.splitn(5, '\t');
            let file = it.next()?;
            let line: usize = it.next()?.parse().ok()?;
            let col: usize = it.next()?.parse().ok()?;
            let kind = it.next()?;
            let msg = it.next()?;
            let message = if kind == "SyntaxError" { msg.to_string() } else { format!("{kind}: {msg}") };
            Some(Diag { file: PathBuf::from(file), line: line.saturating_sub(1), col: (col > 0).then(|| col - 1), message, error: true })
        })
        .collect()
}

fn syntax_check(req: &Request, py: &Path, root: &Path) -> Result<(), Failure> {
    *req.live.lock().unwrap() = crate::i18n::tr("Syntax wird geprüft …", "Checking syntax …").into();
    let mut cmd = base_cmd(py, &req.tools);
    cmd.arg("-c").arg(CHECK_SCRIPT).arg(root);
    let (_, out) = capture(cmd, &req.live).map_err(Failure::msg)?;
    let diags = parse_check(&out);
    if diags.is_empty() {
        return Ok(());
    }
    Err(Failure { message: crate::i18n::tr("Syntaxfehler – das Programm wurde nicht gestartet.", "Syntax error – the program was not started.").into(), diags, output: String::new() })
}

/// Datei, die gestartet wird: die geoeffnete, wenn sie `if __name__ == "__main__"` hat oder kein main.py
/// daneben liegt, sonst main.py/app.py des Projekts
fn entry(req: &Request, root: &Path) -> PathBuf {
    if req.text.contains("__name__") {
        return req.file.clone();
    }
    for name in ["main.py", "app.py", "__main__.py"] {
        let p = root.join(name);
        if p.is_file() && p != req.file {
            return p;
        }
    }
    req.file.clone()
}

/// Start vorbereiten: .venv/Pakete, Syntaxpruefung, dann das Programm (oder die Tests) im Terminal
pub fn prepare(req: &Request, mode: Mode) -> Result<Launch, Failure> {
    let base = req.tools.python.as_ref().map(|p| p.exe.clone()).ok_or_else(|| Failure::msg(crate::i18n::tr("Kein Python gefunden.", "No Python found.")))?;
    let root = project_root(&req.file, req.project.as_deref());
    let dir = req.file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut env = Vec::new();
    let py = match &root {
        Some(r) if req.cfg.python_venv && (r.join("requirements.txt").is_file() || r.join("pyproject.toml").is_file() || r.join(".venv").is_dir()) => {
            // Treiber der .venv statt der allgemeinen (sonst hat PYTHONPATH Vorrang)
            env.push(("PYTHONPATH".into(), String::new()));
            env.push(("VIRTUAL_ENV".into(), r.join(".venv").display().to_string()));
            ensure_venv(req, r, &base)?
        }
        _ => base,
    };
    let root = root.unwrap_or_else(|| req.project.clone().filter(|p| dir.starts_with(p)).unwrap_or(dir.clone()));
    if req.cfg.check_before_run {
        syntax_check(req, &py, &root)?;
    }
    let cwd = if req.cfg.cwd_project { root.clone() } else { dir };
    let mut args = split_args(&req.cfg.python_args);
    let title;
    match mode {
        Mode::Run => {
            let file = entry(req, &root);
            title = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            args.push(file.display().to_string());
            args.extend(split_args(&req.cfg.program_args));
        }
        Mode::Test => {
            title = crate::i18n::tr("Tests", "Tests").into();
            // pytest, wenn es in der Umgebung installiert ist, sonst unittest aus der Standardbibliothek
            if super::probe(&py, &["-c", "import pytest"]).is_some() {
                args.extend(["-m".into(), "pytest".into(), "-v".into()]);
            } else {
                let start = if root.join("tests").is_dir() { "tests" } else { "." };
                args.extend(["-m".into(), "unittest".into(), "discover".into(), "-v".into(), "-s".into(), start.into(), "-t".into(), ".".into()]);
            }
        }
    }
    let mut l = Launch::new(title, py, args, cwd);
    l.env = env;
    Ok(l)
}

/// Welche Werkzeuge braucht das Starten dieser Datei?
pub fn needed_tools(file: &Path, project: Option<&Path>) -> Vec<super::Tool> {
    use super::Tool;
    let cfg = super::config();
    let root = project_root(file, project);
    let own_env = cfg.python_venv && root.is_some_and(|r| r.join("requirements.txt").is_file() || r.join("pyproject.toml").is_file());
    if own_env { vec![Tool::Python] } else { vec![Tool::Python, Tool::PythonDrivers] }
}

/// Treiber mit pip in den eigenen Ordner installieren
pub fn install_driver(tools: &Tools, d: Driver, live: &Mutex<String>) -> Result<String, String> {
    let py = tools.python.as_ref().ok_or_else(|| crate::i18n::tr("Kein Python gefunden.", "No Python found.").to_string())?;
    let site = site_dir();
    std::fs::create_dir_all(&site).map_err(|e| e.to_string())?;
    let mut cmd = base_cmd(&py.exe, tools);
    cmd.args(["-m", "pip", "install", "--upgrade", "--no-warn-script-location", "--target"]).arg(&site).arg(d.package());
    cmd.args(split_args(&super::config().pip_args));
    let (ok, out) = capture(cmd, live)?;
    if !ok {
        return Err(tail(&out, 12));
    }
    // Alte .dist-info-Ordner (pip --target laesst sie bei Upgrades liegen) aufraeumen
    let mut versions: Vec<(String, PathBuf)> = std::fs::read_dir(&site)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    let v = n.strip_prefix(d.dist_info())?.strip_suffix(".dist-info")?.to_string();
                    Some((v, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    versions.sort_by_key(|x| std::cmp::Reverse(version_key(&x.0)));
    for (_, p) in versions.iter().skip(1) {
        let _ = std::fs::remove_dir_all(p);
    }
    versions.first().map(|v| v.0.clone()).ok_or_else(|| crate::i18n::tr("pip hat nichts installiert.", "pip installed nothing.").into())
}

/// Neueste Version eines Pakets auf PyPI
pub fn latest_on_pypi(package: &str) -> Result<String, String> {
    let v = super::install::get_json(&format!("https://pypi.org/pypi/{package}/json"))?;
    v["info"]["version"].as_str().map(str::to_string).ok_or_else(|| format!("PyPI: {package}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_check_lines_are_parsed() {
        let d = parse_check("/p/app.py\t3\t9\tSyntaxError\tinvalid syntax\n/p/x.py\t1\t0\tIndentationError\tunexpected indent\nrauschen\n");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0], Diag { file: "/p/app.py".into(), line: 2, col: Some(8), message: "invalid syntax".into(), error: true });
        assert_eq!(d[1].message, "IndentationError: unexpected indent");
    }

    /// Mit dem Python des Systems: Syntaxfehler werden gefunden, sonst startet das Programm
    #[test]
    fn python_files_are_checked_and_started() {
        let cfg = Config { python: "system".into(), python_venv: false, ..Default::default() };
        let tools = super::super::detect(&std::env::temp_dir().join("easymysql-kein-werkzeug-ordner"), &cfg);
        if tools.python.is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("easymysql-py-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("app.py");
        let req = |text: &str| {
            std::fs::write(&file, text).unwrap();
            Request { file: file.clone(), text: text.into(), project: Some(dir.clone()), tools: Arc::new(tools.clone()), cfg: cfg.clone(), live: Arc::default() }
        };
        let fail = prepare(&req("print('a')\nif True\n    pass\n"), Mode::Run).unwrap_err();
        assert_eq!(fail.diags.len(), 1, "{fail:?}");
        assert_eq!(fail.diags[0].line, 1);
        let l = prepare(&req("import sys\nprint('ok', len(sys.argv))\n"), Mode::Run).unwrap();
        let out = Command::new(&l.program).args(&l.args).current_dir(&l.cwd).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok 1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
