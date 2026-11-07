// Ganzer Ablauf mit echten Downloads und (optional) einer echten Datenbank:
//   HOME=/tmp/emt cargo test e2e_ -- --ignored --nocapture --test-threads=1
// Mit EASYMYSQL_E2E_PORT=<port> laufen die erzeugten Programme gegen den MariaDB-Server auf diesem Port
// (Benutzer root ohne Passwort, Datenbank e2e mit der Tabelle kunde).

use super::process::Launch;
use super::{Config, Tool, install, lsp, node, python, tools};
use crate::db::{ColumnInfo, TableInfo};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn freier_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

struct TestProzess(std::process::Child);
impl Drop for TestProzess {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

struct TestServer(crate::server::Server, crate::db::ConnInfo);
impl Drop for TestServer {
    fn drop(&mut self) { self.0.stop_blocking(&self.1); }
}

fn kunde() -> TableInfo {
    let col = |name: &str, ty: &str, key: &str, extra: &str| ColumnInfo { name: name.into(), col_type: ty.into(), nullable: key.is_empty(), key: key.into(), extra: extra.into(), ..Default::default() };
    TableInfo { name: "kunde".into(), columns: vec![col("id", "int(11)", "PRI", "auto_increment"), col("name", "varchar(50)", "", ""), col("geboren", "date", "", "")], ..Default::default() }
}

fn port() -> Option<u16> {
    std::env::var("EASYMYSQL_E2E_PORT").ok()?.parse().ok()
}

fn ensure(list: &[Tool]) {
    for t in list {
        if !tools().has(*t) {
            let start = Instant::now();
            let v = install::install_now(*t).unwrap_or_else(|e| panic!("{t:?}: {e}"));
            println!("{t:?} {v} installiert ({:.0?})", start.elapsed());
        }
    }
}

fn run(l: &Launch, env: &[(&str, String)]) -> (bool, String) {
    let mut c = Command::new(&l.program);
    c.args(&l.args).current_dir(&l.cwd);
    for (k, v) in tools().shell_env() {
        c.env(k, v);
    }
    for (k, v) in &l.env {
        c.env(k, v);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    if let Some(p) = super::process::tool_path(&tools()) {
        c.env("PATH", p);
    }
    let out = c.output().unwrap();
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn db_env() -> Vec<(&'static str, String)> {
    let p = port().unwrap_or(1);
    vec![
        ("EASYMYSQL_DB_HOST", "127.0.0.1".into()),
        ("EASYMYSQL_DB_PORT", p.to_string()),
        ("EASYMYSQL_DB_USER", "root".into()),
        ("EASYMYSQL_DB_PASSWORD", String::new()),
        ("EASYMYSQL_DB_NAME", "e2e".into()),
        ("EASYMYSQL_DB_URL", format!("jdbc:mariadb://127.0.0.1:{p}/e2e?sslMode=disable")),
    ]
}

fn fresh(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("easymysql-e2e-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
#[ignore = "laedt Python, Node.js und Treiber"]
fn e2e_python_and_node_projects() {
    let cfg = Config { python: "own".into(), node: "own".into(), ..Default::default() };
    super::set_config(cfg.clone());
    ensure(&[Tool::Python, Tool::PythonDrivers, Tool::Node, Tool::NodeDrivers]);
    let t = tools();
    println!("Python {:?}, Node {:?}, Treiber {:?} {:?}", t.python.as_ref().map(|p| &p.version), t.node.as_ref().map(|n| &n.version), t.python_drivers, t.node_drivers);

    // Python: Projekt mit requirements.txt -> .venv, pip install, Syntaxpruefung, Start
    for driver in python::Driver::ALL {
        let dir = fresh(&format!("py-{}", driver.id()));
        let version = python::latest_on_pypi(driver.package()).unwrap();
        let spec = python::project::Spec { name: "E2E".into(), kind: python::project::Kind::Requirements, database: "e2e".into(), tables: vec![kunde()], host: "127.0.0.1".into(), port: port().unwrap_or(3306), user: "root".into(), password: None, driver, driver_version: version, tests: true };
        let main = python::project::write(&dir, &spec).unwrap();
        let req = python::Request { file: main.clone(), text: std::fs::read_to_string(&main).unwrap(), project: Some(dir.clone()), tools: Arc::clone(&t), cfg: cfg.clone(), live: Arc::default() };
        let l = python::prepare(&req, python::Mode::Run).unwrap_or_else(|f| panic!("{driver:?}: {} {}", f.message, f.output));
        assert!(dir.join(".venv").is_dir());
        if port().is_some() {
            let (ok, out) = run(&l, &db_env());
            assert!(ok && out.contains("Ada"), "Python {driver:?}: {out}");
            let lt = python::prepare(&req, python::Mode::Test).unwrap();
            let (ok, out) = run(&lt, &db_env());
            assert!(ok, "Python-Tests {driver:?}: {out}");
        }
        // Syntaxfehler kommen mit Zeile zurueck
        std::fs::write(dir.join("repos.py"), "def x(:\n").unwrap();
        let f = python::prepare(&req, python::Mode::Run).unwrap_err();
        assert!(f.diags.iter().any(|d| d.file.ends_with("repos.py") && d.line == 0), "{f:?}");
        println!("Python {driver:?}: ok");
    }
    // Lose Datei ohne Projekt: Treiber ueber PYTHONPATH
    let dir = fresh("py-lose");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("skript.py");
    std::fs::write(&f, "import pymysql\nprint('treiber', pymysql.__name__)\n").unwrap();
    let req = python::Request { file: f.clone(), text: std::fs::read_to_string(&f).unwrap(), project: None, tools: Arc::clone(&t), cfg: cfg.clone(), live: Arc::default() };
    let (ok, out) = run(&python::prepare(&req, python::Mode::Run).unwrap(), &[]);
    assert!(ok && out.contains("treiber pymysql"), "{out}");

    // Node: npm install, node --check, Start
    for driver in node::Driver::ALL {
        let dir = fresh(&format!("js-{}", driver.id()));
        let version = node::latest_on_npm(driver.package()).unwrap();
        let spec = node::project::Spec { name: "E2E".into(), database: "e2e".into(), tables: vec![kunde()], host: "127.0.0.1".into(), port: port().unwrap_or(3306), user: "root".into(), password: None, driver, driver_version: version, tests: true };
        let main = node::project::write(&dir, &spec).unwrap();
        let req = node::Request { file: main.clone(), project: Some(dir.clone()), tools: Arc::clone(&t), cfg: cfg.clone(), live: Arc::default() };
        let l = node::prepare(&req, node::Mode::Run).unwrap_or_else(|f| panic!("{driver:?}: {} {}", f.message, f.output));
        assert!(dir.join("node_modules").join(driver.package()).is_dir());
        if port().is_some() {
            let (ok, out) = run(&l, &db_env());
            assert!(ok && out.contains("Ada"), "Node {driver:?}: {out}");
            let (ok, out) = run(&node::prepare(&req, node::Mode::Test).unwrap(), &db_env());
            assert!(ok, "Node-Tests {driver:?}: {out}");
        }
        std::fs::write(&main, "const x = ;\n").unwrap();
        let f = node::prepare(&req, node::Mode::Run).unwrap_err();
        assert_eq!(f.diags.first().map(|d| d.line), Some(0), "{f:?}");
        println!("Node {driver:?}: ok");
    }
    // TypeScript direkt mit Node
    let dir = fresh("ts");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("app.ts");
    std::fs::write(&f, "const n: number = 41;\nconsole.log('ts', n + 1);\n").unwrap();
    let req = node::Request { file: f.clone(), project: None, tools: Arc::clone(&t), cfg: cfg.clone(), live: Arc::default() };
    let (ok, out) = run(&node::prepare(&req, node::Mode::Run).unwrap(), &[]);
    assert!(ok && out.contains("ts 42"), "{out}");
}

/// Sprachserver starten und Fehler, Definition und Umbenennen abfragen
fn lsp_roundtrip(lang: crate::sqledit::Lang, root: &Path, file: &Path, broken: &str, defn_at: usize) {
    let ctx = eframe::egui::Context::default();
    let start = Instant::now();
    let server = lsp::get(lang, root, &ctx).unwrap_or_else(|| panic!("{lang:?}: kein Server"));
    while !server.ready() {
        assert!(start.elapsed() < Duration::from_secs(180), "{lang:?}: {:?}", server.phase.lock().unwrap());
        std::thread::sleep(Duration::from_millis(200));
    }
    server.sync(file, lang, broken);
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Some((_, d)) = server.diagnostics(file)
            && d.iter().any(|x| x.error)
        {
            println!("{lang:?}: {} Meldung(en), z. B. {:?}", d.len(), d[0].message);
            break;
        }
        assert!(Instant::now() < deadline, "{lang:?}: keine Fehlermeldung");
        std::thread::sleep(Duration::from_millis(300));
    }
    if lang == crate::sqledit::Lang::Java {
        let diagnostics = server.diagnostics(file).unwrap().1;
        let index = lsp::from_pos(broken, diagnostics.iter().find(|d| d.error).unwrap().start);
        let actions = server.korrekturen(file, broken, index).recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
        let mut valid = false;
        let folder = fresh("lsp-java-autofix"); std::fs::create_dir_all(&folder).unwrap();
        let target = folder.join("Main.java");
        for action in actions.as_array().unwrap() {
            let action = if action.get("edit").is_none() && action.get("data").is_some() {
                match server.korrektur_auflosen(action.clone()).and_then(|r| r.recv_timeout(Duration::from_secs(60)).ok()).and_then(Result::ok) { Some(v) => v, None => continue }
            } else { action.clone() };
            let Ok(edit) = lsp::korrektur_edit(&action) else { continue };
            let edits = edit.files.iter().filter(|f| f.path == file).flat_map(|f| f.edits.clone()).collect::<Vec<_>>();
            if edits.is_empty() { continue; }
            let fixed = lsp::apply_edits(broken, &edits);
            std::fs::write(&target, fixed).unwrap();
            let out = Command::new(tools().jdk.as_ref().unwrap().javac()).arg("-encoding").arg("UTF-8").arg("-d").arg(&folder).arg(&target).output().unwrap();
            if out.status.success() { valid = true; break; }
        }
        assert!(valid, "Kein Java-CodeAction-Fix kompiliert: {actions}");
    }
    let v = server.definition(file, broken, defn_at).recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
    let loc = lsp::parse_location(&v);
    println!("{lang:?}: Definition {loc:?} ({:.0?})", start.elapsed());
    assert!(loc.is_some(), "{lang:?}: {v}");
    let r = server.rename(file, broken, defn_at, "umbenannt").recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
    let edit = lsp::parse_workspace_edit(&r);
    assert!(!edit.files.is_empty(), "{lang:?}: {r}");
}

#[test]
#[ignore = "laedt die Sprachserver (einige hundert MB)"]
fn e2e_language_servers() {
    super::set_config(Config::default());
    ensure(&[Tool::Jdk, Tool::Node, Tool::JavaLsp, Tool::PythonLsp, Tool::JsLsp]);
    use crate::sqledit::Lang;
    // Python
    let dir = fresh("lsp-py");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("app.py");
    let py = "def rechne(x):\n    return x * 2\n\nprint(rechne(2))\nprint(unbekannt)\n";
    std::fs::write(&f, py).unwrap();
    lsp_roundtrip(Lang::Python, &dir, &f, py, py.find("rechne(2)").unwrap());
    // JavaScript
    let dir = fresh("lsp-js");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("app.ts");
    let js = "function rechne(x: number) {\n  return x * 2;\n}\nconst s: string = rechne(2);\n";
    std::fs::write(&f, js).unwrap();
    lsp_roundtrip(Lang::TypeScript, &dir, &f, js, js.rfind("rechne").unwrap());
    // Java (einfaches Projekt)
    let dir = fresh("lsp-java");
    let src = dir.join("src/app");
    std::fs::create_dir_all(&src).unwrap();
    let f = src.join("Main.java");
    let java = "package app;\n\npublic class Main {\n    static int rechne(int x) { return x * 2; }\n    public static void main(String[] args) {\n        int y = rechne(2);\n        String s = y;\n    }\n}\n";
    std::fs::write(&f, java).unwrap();
    lsp_roundtrip(Lang::Java, &dir, &f, java, java.find("rechne(2)").unwrap());
    lsp::shutdown_all();
}

#[test]
#[ignore = "laedt JDK, Treiber und JUnit"]
fn e2e_java_plain_project_against_the_database() {
    let Some(p) = port() else { return };
    super::set_config(Config::default());
    ensure(&[Tool::Jdk, Tool::JavaDrivers, Tool::JUnit]);
    let t = tools();
    let dir = fresh("java-db");
    let spec = super::java::project::Spec {
        name: "E2E".into(),
        kind: super::java::project::Kind::Plain,
        package: "app".into(),
        database: "e2e".into(),
        tables: vec![kunde()],
        host: "127.0.0.1".into(),
        port: p,
        user: "root".into(),
        password: None,
        driver: super::java::Driver::MariaDb,
        driver_version: install::driver_version(super::java::Driver::MariaDb),
        junit_version: Some(install::junit_version()),
    };
    let main = super::java::project::write(&dir, &spec).unwrap();
    let req = super::java::run::Request { file: main.clone(), text: std::fs::read_to_string(&main).unwrap(), project: Some(dir.clone()), tools: Arc::clone(&t), cfg: Config::default(), live: Arc::default() };
    let (ok, out) = run(&super::java::run::prepare(&req, super::java::run::Mode::Run).unwrap(), &db_env());
    assert!(ok && out.contains("Ada"), "{out}");
    let (ok, out) = run(&super::java::run::prepare(&req, super::java::run::Mode::Test).unwrap(), &db_env());
    assert!(ok && out.contains("tests successful"), "{out}");
    let jar = super::java::jar::build(&req).unwrap();
    let out = Command::new(t.jdk.as_ref().unwrap().java()).arg("-jar").arg(&jar).envs(db_env()).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Ada"), "{}", String::from_utf8_lossy(&out.stderr));
    println!("Java einfach: Start, Tests und .jar ok");
}

#[test]
#[ignore = "laedt Maven und Gradle"]
fn e2e_java_build_tools_run_tests_and_jar_against_the_database() {
    let Some(p) = port() else { return };
    super::set_config(Config::default());
    ensure(&[Tool::Jdk, Tool::JavaDrivers, Tool::Maven, Tool::Gradle]);
    let t = tools();
    for kind in [super::java::project::Kind::Maven, super::java::project::Kind::GradleKotlin] {
        let dir = fresh(&format!("java-{}", kind.id()));
        let spec = super::java::project::Spec {
            name: "E2E".into(),
            kind,
            package: "app".into(),
            database: "e2e".into(),
            tables: vec![kunde()],
            host: "127.0.0.1".into(),
            port: p,
            user: "root".into(),
            password: None,
            driver: super::java::Driver::MariaDb,
            driver_version: install::driver_version(super::java::Driver::MariaDb),
            junit_version: Some(install::junit_version()),
        };
        let main = super::java::project::write(&dir, &spec).unwrap();
        let req = super::java::run::Request { file: main.clone(), text: std::fs::read_to_string(&main).unwrap(), project: Some(dir.clone()), tools: Arc::clone(&t), cfg: Config::default(), live: Arc::default() };
        let (ok, out) = run(&super::java::run::prepare(&req, super::java::run::Mode::Run).unwrap(), &db_env());
        assert!(ok && out.contains("Ada"), "{kind:?}: {out}");
        let (ok, out) = run(&super::java::run::prepare(&req, super::java::run::Mode::Test).unwrap(), &db_env());
        assert!(ok, "{kind:?} Tests: {}", super::process::tail(&out, 40));
        let jar = super::java::jar::build(&req).unwrap();
        let out = Command::new(t.jdk.as_ref().unwrap().java()).arg("-jar").arg(&jar).envs(db_env()).output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("Ada"), "{kind:?}: {}", String::from_utf8_lossy(&out.stderr));
        println!("{kind:?}: Start, Tests und .jar gegen die Datenbank ok");
    }
}

/// Eigener Server mit eigenem Zertifikat und "nur verschluesselt": EasyMySQL und JDBC verbinden sich
/// verschluesselt und pruefen das Zertifikat; ohne TLS wird abgewiesen.
#[test]
#[ignore = "startet einen MariaDB-Server (mariadbd im PATH) und laedt den JDBC-Treiber"]
fn e2e_tls_own_certificate() {
    let Some(bin) = ["/usr/sbin", "/usr/bin"].iter().map(Path::new).find(|b| b.join("mariadbd").is_file()) else { return };
    let port = freier_port();
    let dir = fresh("tls");
    std::fs::create_dir_all(&dir).unwrap();
    let data = dir.join("data");
    let st = Command::new(bin.join("mariadb-install-db").to_string_lossy().replace("/sbin/", "/bin/")).args(["--no-defaults", "--auth-root-authentication-method=normal"]).arg(format!("--datadir={}", data.display())).output();
    assert!(st.is_ok_and(|o| o.status.success()));
    let mut lines = crate::tls::ini_lines(crate::tls::Mode::Required, "").unwrap();
    lines.extend([format!("port={port}"), "bind-address=127.0.0.1".into(), format!("datadir={}", data.display()), format!("socket={}", dir.join("s.sock").display())]);
    let ini = dir.join("my.ini");
    std::fs::write(&ini, format!("[mysqld]\n{}\n", lines.join("\n"))).unwrap();
    let mut cmd = Command::new(bin.join("mariadbd"));
    cmd.arg(format!("--defaults-file={}", ini.display()));
    #[cfg(not(windows))]
    if crate::server::is_root_user() { cmd.arg("--user=root"); }
    let _server = TestProzess(cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap());
    let t0 = Instant::now();
    while !crate::server::port_open(port) && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let info = crate::db::ConnInfo { host: "127.0.0.1".into(), port, user: "root".into(), password: String::new() };
    // Ohne TLS: abgewiesen (require_secure_transport)
    crate::db::set_local_tls(None);
    let plain = crate::db::Db::connect(&info);
    assert!(plain.is_err(), "unverschluesselt darf nicht gehen");
    // Mit der eigenen CA: verschluesselt und geprueft
    crate::db::set_local_tls(Some((port, crate::tls::files().ca)));
    let db = crate::db::Db::connect(&info).unwrap_or_else(|e| panic!("TLS: {e}"));
    // Danach darf der Treiber auf den Unix-Socket wechseln (gilt als sicher); der Handschlag lief ueber TLS
    let rows = db.rows("SHOW GLOBAL STATUS LIKE 'Ssl_accepts'", ()).unwrap();
    let accepts: u64 = rows[0].get(1).cloned().flatten().and_then(|v| v.parse().ok()).unwrap_or(0);
    assert!(accepts > 0, "kein TLS-Handschlag");
    println!("EasyMySQL: TLS-Handschlag mit Pruefung gegen die eigene CA ({accepts})");
    // JDBC mit verify-ca
    super::set_config(Config::default());
    ensure(&[Tool::Jdk, Tool::JavaDrivers]);
    let url = super::java::jdbc_url(super::java::Driver::MariaDb, "127.0.0.1", port, "");
    assert!(url.contains("sslMode=verify-ca"), "{url}");
    let src = dir.join("Tls.java");
    std::fs::write(&src, format!("import java.sql.*;\npublic class Tls {{ public static void main(String[] a) throws Exception {{ try (Connection c = DriverManager.getConnection(\"{url}\", \"root\", \"\"); ResultSet r = c.createStatement().executeQuery(\"SHOW SESSION STATUS LIKE 'Ssl_cipher'\")) {{ r.next(); System.out.println(\"cipher=\" + r.getString(2)); }} }} }}\n")).unwrap();
    let t = tools();
    let cp = t.driver_jars().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(super::java::CP_SEP);
    let out = Command::new(t.jdk.as_ref().unwrap().java()).arg("-cp").arg(cp).arg(&src).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    crate::db::set_local_tls(None);
    assert!(text.contains("cipher=") && !text.contains("cipher=\n"), "JDBC: {text}");
    println!("JDBC: {}", text.trim());
}

/// Nur lesend: Anmeldung ueber die GitHub CLI, Konto, Pull Requests und Actions dieses Repositorys, Klonen
#[test]
#[ignore = "braucht eine angemeldete GitHub CLI"]
fn e2e_github_read_only() {
    use crate::git::{self, github};
    if github::token().is_none() {
        println!("nicht angemeldet – uebersprungen");
        return;
    }
    let u = github::user().unwrap();
    println!("angemeldet als {} <{}>", u.login, u.email);
    let root = std::env::current_dir().unwrap();
    let (o, r) = git::github_repo(&git::remote_url(&root).unwrap()).unwrap();
    let info = github::repo(&o, &r).unwrap();
    let pulls = github::pulls(&o, &r).unwrap();
    let runs = github::runs(&o, &r).unwrap();
    println!("{o}/{r}: Standard-Branch {}, {} offene PRs, {} Actions-Laeufe", info.default_branch, pulls.len(), runs.len());
    let st = git::status(&root).unwrap();
    println!("Branch {} ({} geaenderte Dateien)", st.branch, st.files.len());
    let target = fresh("clone");
    git::clone(&info.clone_url, &target).unwrap();
    assert!(target.join(".git").is_dir() && target.join("Cargo.toml").is_file());
    let _ = std::fs::remove_dir_all(&target);
}

/// MariaDB des Systems (z. B. 10.11) einrichten, dann auf die neueste LTS-Version aktualisieren lassen
#[test]
#[ignore = "laedt MariaDB (mehrere hundert MB) und startet Server"]
fn e2e_mariadb_update_in_place() {
    use crate::mariadb_update as mu;
    let port = freier_port();
    let data = fresh("mariadb-update");
    let cfg = crate::server::Config { port, data_dir: Some(data.join("data")), ..Default::default() };
    let server = crate::server::Server::new(cfg);
    let Some(paths) = server.paths() else { return };
    let before = mu::installed_version(&paths.bin).unwrap();
    let info = crate::db::ConnInfo { host: "127.0.0.1".into(), port, user: "root".into(), password: String::new() };
    let _guard = TestServer(server.clone(), info.clone());
    server.start_blocking(&|| {}).unwrap();
    let db = crate::db::Db::connect(&info).unwrap();
    db.exec("CREATE DATABASE upd", ()).unwrap();
    db.exec("CREATE TABLE upd.t (id INT PRIMARY KEY, txt VARCHAR(20))", ()).unwrap();
    db.exec("INSERT INTO upd.t VALUES (1, 'bleibt')", ()).unwrap();
    drop(db);
    let rel = mu::latest(mu::Track::Lts).unwrap();
    println!("von {before} auf {}", rel.version);
    mu::run(server.clone(), info.clone(), rel.clone(), true, &eframe::egui::Context::default());
    let t0 = Instant::now();
    loop {
        let st = mu::status();
        if !st.busy && (st.done.is_some() || st.error.is_some()) {
            println!("{:?} {:?}", st.done, st.error);
            assert!(st.error.is_none(), "{:?}", st.error);
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(1200));
        std::thread::sleep(Duration::from_millis(500));
    }
    let bin = mu::take_applied().flatten().unwrap();
    assert_eq!(mu::installed_version(&bin).as_deref(), Some(rel.version.as_str()));
    let db = crate::db::Db::connect(&info).unwrap();
    let rows = db.rows("SELECT txt FROM upd.t", ()).unwrap();
    assert_eq!(rows[0][0].as_deref(), Some("bleibt"));
    let v = db.rows("SELECT VERSION()", ()).unwrap();
    println!("laeuft: {:?}", v[0][0]);
    assert!(data.join(format!("data-vor-{}", rel.version)).is_dir(), "Kopie des Datenordners fehlt");
    drop(db);
    server.stop_blocking(&info);
}

#[test]
#[ignore = "laedt Claude Code und Codex mit Pruefsumme, ohne KI-Anfragen"]
fn e2e_ai_plugins() {
    use crate::mcp::setup::Target;
    for ziel in [Target::Claude, Target::Codex] {
        let exe = install::install_ai(ziel, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        let out = Command::new(&exe).arg("--version").output().unwrap();
        assert!(out.status.success(), "{}: {}", exe.display(), String::from_utf8_lossy(&out.stderr));
        println!("{}: {}", ziel.label(), String::from_utf8_lossy(&out.stdout).trim());
        assert_eq!(ziel.program().as_deref(), Some(exe.as_path()));
    }
}
