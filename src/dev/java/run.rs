// Java-Programme starten, debuggen und testen. Zuerst wird im Hintergrund uebersetzt (Fehler landen als
// Markierung im Editor), dann laeuft das Programm im Terminal, wo es auch Eingaben lesen kann.
//
// - Einfaches Projekt: javac uebersetzt alle Quellen unter dem Quellordner, die Treiber und alle .jar in
//   `lib/` stehen im Klassenpfad. Tests liegen in `test/` neben `src/` und laufen mit JUnit.
// - Maven: `mvn compile dependency:build-classpath`, danach `java` mit diesem Klassenpfad.
// - Gradle: `gradle classes` und ein kleines Init-Skript, das den Klassenpfad aufschreibt.

use super::super::process::{Diag, Failure, Launch, capture, scratch_dir, source_files, tail, tool_path};
use super::super::{Config, Tools, split_args};
use super::{CP_SEP, source};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Build {
    Maven(PathBuf),
    Gradle(PathBuf),
    /// Ohne Build-Werkzeug; der Wert ist der Quellordner (Wurzel der Pakete)
    Plain(PathBuf),
}

impl Build {
    /// Projektordner (bei einfachen Projekten der Ordner ueber `src`)
    pub fn root(&self) -> PathBuf {
        match self {
            Build::Maven(r) | Build::Gradle(r) => r.clone(),
            Build::Plain(src) => {
                if src.file_name().is_some_and(|n| n == "src" || n == "test") {
                    src.parent().map(Path::to_path_buf).unwrap_or_else(|| src.clone())
                } else {
                    src.clone()
                }
            }
        }
    }
}

/// Was mit der Datei geschehen soll
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Run,
    /// Mit Debugger: die JVM wartet auf diesem Port
    Debug(u16),
    Test,
}

/// Quellordner einer Datei: ihr Ordner ohne die Ordner des Pakets (`src/app/Main.java` mit `package app;` -> `src`)
pub fn source_root(file: &Path, package: Option<&str>) -> PathBuf {
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let Some(pkg) = package else { return dir };
    let parts: Vec<&str> = pkg.split('.').collect();
    let mut root = dir.clone();
    for part in parts.iter().rev() {
        if root.file_name().is_some_and(|n| n == *part) {
            root.pop();
        } else {
            return dir;
        }
    }
    root
}

/// Maven- bzw. Gradle-Projekt suchen, zu dem die Datei gehoert (hoechstens bis zum Projektordner).
pub fn detect_build(file: &Path, package: Option<&str>, limit: Option<&Path>) -> Build {
    let mut dir = file.parent();
    let mut depth = 0;
    while let Some(d) = dir {
        if d.join("pom.xml").is_file() {
            return Build::Maven(d.to_path_buf());
        }
        if ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"].iter().any(|f| d.join(f).is_file()) {
            return Build::Gradle(d.to_path_buf());
        }
        if limit.is_some_and(|l| d == l) || depth > 12 {
            break;
        }
        dir = d.parent();
        depth += 1;
    }
    Build::Plain(source_root(file, package))
}

/// Ausgabe von javac (auch ueber Gradle): `Datei.java:12: error: text`, danach Quellzeile und `^`.
pub fn parse_javac(output: &str) -> Vec<Diag> {
    let lines: Vec<&str> = output.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(pos) = l.find(".java:") else { continue };
        let file = &l[..pos + 5];
        let rest = &l[pos + 6..];
        let Some((num, rest)) = rest.split_once(": ") else { continue };
        let Ok(line) = num.trim().parse::<usize>() else { continue };
        let (error, message) = if let Some(m) = rest.strip_prefix("error: ").or_else(|| rest.strip_prefix("Fehler: ")) {
            (true, m)
        } else if let Some(m) = rest.strip_prefix("warning: ").or_else(|| rest.strip_prefix("Warnung: ")) {
            (false, m)
        } else {
            continue;
        };
        // Spalte aus der Zeile mit dem ^ (zwei Zeilen tiefer)
        let col = lines.get(i + 2).filter(|c| c.trim() == "^").and_then(|c| c.find('^'));
        let file = file.trim().trim_start_matches("e: ").trim_start_matches("w: ");
        out.push(Diag { file: PathBuf::from(file), line: line.saturating_sub(1), col, message: message.trim().to_string(), error });
    }
    out
}

/// Ausgabe von Maven: `[ERROR] /pfad/Datei.java:[12,5] text`
pub fn parse_maven(output: &str) -> Vec<Diag> {
    let mut out = Vec::new();
    for l in output.lines() {
        let (error, rest) = if let Some(r) = l.strip_prefix("[ERROR] ") {
            (true, r)
        } else if let Some(r) = l.strip_prefix("[WARNING] ") {
            (false, r)
        } else {
            continue;
        };
        let Some(pos) = rest.find(".java:[") else { continue };
        let mut file = rest[..pos + 5].to_string();
        // Windows: "/C:/Users/…" -> "C:/Users/…"
        if file.len() > 3 && file.starts_with('/') && file.as_bytes()[2] == b':' {
            file.remove(0);
        }
        let after = &rest[pos + 7..];
        let Some((nums, message)) = after.split_once(']') else { continue };
        let mut it = nums.split(',').map(|n| n.trim().parse::<usize>().ok());
        let (Some(Some(line)), col) = (it.next(), it.next().flatten()) else { continue };
        let d = Diag { file: PathBuf::from(file), line: line.saturating_sub(1), col: col.map(|c| c.saturating_sub(1)), message: message.trim().to_string(), error };
        // Maven nennt jeden Fehler zweimal (Liste und Zusammenfassung)
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

pub struct Request {
    pub file: PathBuf,
    pub text: String,
    pub project: Option<PathBuf>,
    pub tools: Arc<Tools>,
    pub cfg: Config,
    /// Letzte Ausgabezeile waehrend des Uebersetzens (fuer die Anzeige)
    pub live: Arc<Mutex<String>>,
}

/// Sprache der Compilermeldungen passend zur Oberflaeche
fn lang_flag() -> String {
    format!("-Duser.language={}", if crate::i18n::is_english() { "en" } else { "de" })
}

/// Klasse mit main suchen: die geoeffnete Datei, sonst die einzige (oder die namens Main) unter `root`.
fn main_class(req: &Request, root: &Path) -> Result<String, Failure> {
    let fqcn = |package: Option<String>, file: &Path| -> String {
        let class = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match package {
            Some(p) => format!("{p}.{class}"),
            None => class,
        }
    };
    if source::has_main(&req.text) {
        return Ok(fqcn(source::package(&req.text), &req.file));
    }
    let mut found: Vec<(String, PathBuf)> = java_files(root)
        .into_iter()
        .filter_map(|p| {
            let t = std::fs::read_to_string(&p).ok()?;
            source::has_main(&t).then(|| (fqcn(source::package(&t), &p), p))
        })
        .collect();
    found.sort();
    match found.as_slice() {
        [] => Err(Failure::msg(crate::i18n::tr(
            "Keine Klasse mit main-Methode gefunden. Schreib zum Beispiel: public static void main(String[] args) { … }",
            "No class with a main method found. Add for example: public static void main(String[] args) { … }",
        ))),
        [(c, _)] => Ok(c.clone()),
        many => match many.iter().find(|(c, _)| c == "Main" || c.ends_with(".Main")) {
            Some((c, _)) => Ok(c.clone()),
            None => Err(Failure::msg(crate::tr_format!(
                "Diese Datei hat keine main-Methode, und es gibt mehrere andere ({}). Öffne die, die starten soll.",
                "This file has no main method and there are several others ({}). Open the one that should run.",
                many.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        },
    }
}

/// Alle .java-Dateien unter `root` (ohne Ausgabe- und versteckte Ordner)
pub fn java_files(root: &Path) -> Vec<PathBuf> {
    source_files(root, &["java"])
}

/// Alle .jar in `lib`-Ordnern
fn lib_jars(dirs: &[&Path]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs {
        let lib = d.join("lib");
        if let Ok(rd) = std::fs::read_dir(&lib) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jar")) && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Zeile einer javac-Argumentdatei (Anfuehrungszeichen, Backslashes maskiert)
fn argfile_line(p: &Path) -> String {
    format!("\"{}\"", p.display().to_string().replace('\\', "\\\\").replace('"', "\\\""))
}

fn java_env(cmd: &mut Command, tools: &Tools) {
    if let Some(j) = &tools.jdk {
        cmd.env("JAVA_HOME", &j.home);
    }
    if let Some(p) = tool_path(tools) {
        cmd.env("PATH", p);
    }
}

/// Ergebnis des Uebersetzens
#[derive(Clone, Debug)]
pub struct Compiled {
    pub build: Build,
    /// Klasse mit main
    pub class: String,
    /// Laufzeit-Klassenpfad: zuerst die eigenen Klassen, dann die Abhaengigkeiten
    pub classpath: Vec<PathBuf>,
    /// Ordner mit den eigenen Klassen (und Ressourcen)
    pub classes: PathBuf,
    pub cwd: PathBuf,
}

/// javac fuer `sources` nach `out` (einfaches Projekt)
fn javac(req: &Request, sources: &[PathBuf], out: &Path, cp: &[PathBuf], cwd: &Path) -> Result<(), Failure> {
    let jdk = req.tools.jdk.as_ref().ok_or_else(|| Failure::msg(crate::i18n::tr("Kein JDK gefunden.", "No JDK found.")))?;
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).map_err(|e| Failure::msg(e.to_string()))?;
    let argfile = out.with_extension("quellen.txt");
    let list: String = sources.iter().map(|p| argfile_line(p) + "\n").collect();
    std::fs::write(&argfile, list).map_err(|e| Failure::msg(e.to_string()))?;
    let cp_text = cp.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(CP_SEP);
    let mut cmd = Command::new(jdk.javac());
    cmd.current_dir(cwd).arg(format!("-J{}", lang_flag())).args(["-encoding", "UTF-8", "-g", "-Xlint:-options", "-Xmaxerrs", "200", "-d"]).arg(out);
    if req.cfg.java_release > 0 {
        cmd.arg("--release").arg(req.cfg.java_release.to_string());
    }
    cmd.args(split_args(&req.cfg.javac_args));
    if !cp_text.is_empty() {
        cmd.arg("-cp").arg(&cp_text);
    }
    cmd.arg(format!("@{}", argfile.display()));
    let (ok, output) = capture(cmd, &req.live).map_err(Failure::msg)?;
    if !ok {
        return Err(Failure { message: crate::i18n::tr("Übersetzen fehlgeschlagen.", "Compilation failed.").into(), diags: parse_javac(&output), output: tail(&output, 40) });
    }
    Ok(())
}

/// Ressourcen (db.properties u. ae.) aus dem Quellordner neben die Klassen kopieren
fn copy_resources(src: &Path, out: &Path) {
    fn walk(dir: &Path, base: &Path, out: &Path, depth: usize) {
        if depth > 16 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                walk(&p, base, out, depth + 1);
            } else if !name.ends_with(".java")
                && let Ok(rel) = p.strip_prefix(base)
            {
                let target = out.join(rel);
                if let Some(parent) = target.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(&p, target);
            }
        }
    }
    walk(src, src, out, 0);
}

/// Uebersetzen (laeuft im Hintergrund). Liefert die Klasse mit main und den Klassenpfad.
pub fn compile(req: &Request) -> Result<Compiled, Failure> {
    let tools = &req.tools;
    let package = source::package(&req.text);
    let build = detect_build(&req.file, package.as_deref(), req.project.as_deref());
    match &build {
        Build::Plain(root) => {
            // Datei in test/: der Quellordner ist dann src/ daneben
            let src = if root.file_name().is_some_and(|n| n == "test") && root.with_file_name("src").is_dir() { root.with_file_name("src") } else { root.clone() };
            let class = main_class(req, &src)?;
            let out = scratch_dir("java-klassen", &src);
            let mut cp: Vec<PathBuf> = tools.driver_jars();
            let project_root = build.root();
            let mut dirs = vec![src.as_path(), project_root.as_path()];
            if let Some(p) = req.project.as_deref() {
                dirs.push(p);
            }
            cp.extend(lib_jars(&dirs));
            javac(req, &java_files(&src), &out, &cp, &src)?;
            copy_resources(&src, &out);
            let mut classpath = vec![out.clone()];
            classpath.extend(cp);
            let cwd = if req.cfg.cwd_project { req.project.clone().filter(|p| src.starts_with(p)).unwrap_or(project_root) } else { req.file.parent().map(Path::to_path_buf).unwrap_or(src.clone()) };
            Ok(Compiled { build, class, classpath, classes: out, cwd })
        }
        Build::Maven(root) => {
            let class = main_class(req, &root.join("src/main/java"))?;
            let launcher = maven(root, tools, &req.cfg)?;
            let cp_file = scratch_dir("java-klassen", root).with_extension("maven-cp.txt");
            if let Some(p) = cp_file.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::remove_file(&cp_file);
            let mut cmd = Command::new(&launcher);
            java_env(&mut cmd, tools);
            cmd.env("MAVEN_OPTS", lang_flag());
            cmd.current_dir(root).args(["-B", "-q"]);
            if req.cfg.build_offline {
                cmd.arg("-o");
            }
            cmd.args(split_args(&req.cfg.maven_args));
            cmd.args(["compile", "dependency:build-classpath", "-Dmdep.includeScope=runtime"]).arg(format!("-Dmdep.outputFile={}", cp_file.display()));
            let (ok, output) = capture(cmd, &req.live).map_err(Failure::msg)?;
            if !ok {
                let mut diags = parse_maven(&output);
                if diags.is_empty() {
                    diags = parse_javac(&output);
                }
                return Err(Failure { message: crate::i18n::tr("Maven konnte das Projekt nicht bauen.", "Maven could not build the project.").into(), diags, output: tail(&output, 40) });
            }
            let deps = std::fs::read_to_string(&cp_file).unwrap_or_default();
            let classes = root.join("target").join("classes");
            let mut classpath = vec![classes.clone()];
            classpath.extend(deps.trim().split(CP_SEP).filter(|s| !s.is_empty()).map(PathBuf::from));
            Ok(Compiled { build: build.clone(), class, classpath, classes, cwd: root.clone() })
        }
        Build::Gradle(root) => {
            let class = main_class(req, &root.join("src/main/java"))?;
            let launcher = gradle(root, tools, &req.cfg)?;
            let base = scratch_dir("java-klassen", root);
            if let Some(p) = base.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let init = base.with_extension("init.gradle");
            let cp_file = base.with_extension("gradle-cp.txt");
            let _ = std::fs::remove_file(&cp_file);
            std::fs::write(&init, GRADLE_INIT).map_err(|e| Failure::msg(e.to_string()))?;
            let mut cmd = Command::new(&launcher);
            java_env(&mut cmd, tools);
            cmd.current_dir(root).args(["-q", "--console=plain"]);
            if req.cfg.build_offline {
                cmd.arg("--offline");
            }
            cmd.args(split_args(&req.cfg.gradle_args));
            cmd.args(["easymysqlClasspath", "-I"]).arg(&init).arg(format!("-PeasymysqlCp={}", cp_file.display()));
            let (ok, output) = capture(cmd, &req.live).map_err(Failure::msg)?;
            if !ok {
                return Err(Failure { message: crate::i18n::tr("Gradle konnte das Projekt nicht bauen.", "Gradle could not build the project.").into(), diags: parse_javac(&output), output: tail(&output, 40) });
            }
            let cp = std::fs::read_to_string(&cp_file).map_err(|_| {
                Failure::msg(crate::i18n::tr(
                    "Gradle hat keinen Klassenpfad geliefert (nur Projekte mit dem Java-Plugin im Hauptprojekt werden unterstützt). Im Terminal: gradle run",
                    "Gradle did not report a class path (only builds with the Java plugin in the root project are supported). In the terminal: gradle run",
                ))
            })?;
            let classpath: Vec<PathBuf> = cp.trim().split(CP_SEP).filter(|s| !s.is_empty()).map(PathBuf::from).collect();
            let classes = classpath.first().cloned().unwrap_or_else(|| root.join("build/classes/java/main"));
            Ok(Compiled { build: build.clone(), class, classpath, classes, cwd: root.clone() })
        }
    }
}

fn join_cp(cp: &[PathBuf]) -> String {
    cp.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(CP_SEP)
}

/// Uebersetzen und den Start vorbereiten (laeuft im Hintergrund).
pub fn prepare(req: &Request, mode: Mode) -> Result<Launch, Failure> {
    let jdk = req.tools.jdk.clone().ok_or_else(|| Failure::msg(crate::i18n::tr("Kein JDK gefunden.", "No JDK found.")))?;
    if mode == Mode::Test {
        return prepare_tests(req, &jdk);
    }
    let c = compile(req)?;
    let mut args = split_args(&req.cfg.jvm_args);
    if let Mode::Debug(port) = mode {
        args.push(format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"));
    }
    args.extend(["-cp".to_string(), join_cp(&c.classpath), c.class.clone()]);
    args.extend(split_args(&req.cfg.program_args));
    Ok(Launch::new(short(&c.class), jdk.java(), args, c.cwd))
}

/// Tests starten: Maven/Gradle im Terminal, einfache Projekte mit dem JUnit-Konsolenstarter
fn prepare_tests(req: &Request, jdk: &super::Jdk) -> Result<Launch, Failure> {
    let package = source::package(&req.text);
    let build = detect_build(&req.file, package.as_deref(), req.project.as_deref());
    let title = crate::i18n::tr("Tests", "Tests").to_string();
    match &build {
        Build::Maven(root) => {
            let mut args = vec!["-B".to_string()];
            if req.cfg.build_offline {
                args.push("-o".into());
            }
            args.extend(split_args(&req.cfg.maven_args));
            args.push("test".into());
            Ok(Launch::new(title, maven(root, &req.tools, &req.cfg)?, args, root.clone()))
        }
        Build::Gradle(root) => {
            let mut args = vec!["--console=rich".to_string()];
            if req.cfg.build_offline {
                args.push("--offline".into());
            }
            args.extend(split_args(&req.cfg.gradle_args));
            args.push("test".into());
            Ok(Launch::new(title, gradle(root, &req.tools, &req.cfg)?, args, root.clone()))
        }
        Build::Plain(_) => {
            let root = build.root();
            let src = root.join("src");
            let test = root.join("test");
            if !test.is_dir() {
                return Err(Failure::msg(crate::i18n::tr("Kein Ordner test/ neben src/. Lege mit „Test anlegen“ einen Test an.", "No test/ folder next to src/. Create a test with “Create test”.")));
            }
            let junit = req.tools.junit.clone().ok_or_else(|| Failure::msg(crate::i18n::tr("JUnit fehlt.", "JUnit is missing.")))?.1;
            let mut cp: Vec<PathBuf> = req.tools.driver_jars();
            cp.extend(lib_jars(&[root.as_path()]));
            let main_out = scratch_dir("java-klassen", &src);
            if src.is_dir() {
                javac(req, &java_files(&src), &main_out, &cp, &src)?;
                copy_resources(&src, &main_out);
            }
            let test_out = scratch_dir("java-tests", &test);
            let mut test_cp = vec![main_out.clone(), junit.clone()];
            test_cp.extend(cp.iter().cloned());
            javac(req, &java_files(&test), &test_out, &test_cp, &test)?;
            copy_resources(&test, &test_out);
            let mut run_cp = vec![test_out, main_out];
            run_cp.extend(cp);
            let mut args = split_args(&req.cfg.jvm_args);
            args.extend(["-jar".into(), junit.display().to_string(), "execute".into(), "--class-path".into(), join_cp(&run_cp), "--scan-class-path".into(), "--details=tree".into()]);
            Ok(Launch::new(title, jdk.java(), args, root))
        }
    }
}

/// Maven des Projekts (mvnw) bzw. das eigene
fn maven(root: &Path, tools: &Tools, cfg: &Config) -> Result<PathBuf, Failure> {
    cfg.use_wrapper
        .then(|| wrapper(root, "mvnw"))
        .flatten()
        .or_else(|| tools.maven.as_ref().map(|m| m.launcher.clone()))
        .ok_or_else(|| Failure::msg(crate::i18n::tr("Maven fehlt.", "Maven is missing.")))
}

fn gradle(root: &Path, tools: &Tools, cfg: &Config) -> Result<PathBuf, Failure> {
    cfg.use_wrapper
        .then(|| wrapper(root, "gradlew"))
        .flatten()
        .or_else(|| tools.gradle.as_ref().map(|g| g.launcher.clone()))
        .ok_or_else(|| Failure::msg(crate::i18n::tr("Gradle fehlt.", "Gradle is missing.")))
}

/// Init-Skript fuer Gradle: schreibt den Laufzeit-Klassenpfad des Hauptprojekts in eine Datei.
const GRADLE_INIT: &str = r#"// Von EasyMySQL: schreibt den Klassenpfad, mit dem das Programm gestartet wird
rootProject {
    pluginManager.withPlugin('java') {
        def target = file(providers.gradleProperty('easymysqlCp').get())
        def cp = sourceSets.main.runtimeClasspath
        tasks.register('easymysqlClasspath') {
            dependsOn 'classes'
            outputs.upToDateWhen { false }
            doLast { target.text = cp.files.join(File.pathSeparator) }
        }
    }
}
"#;

/// Wrapper des Projekts (mvnw, gradlew), falls vorhanden
fn wrapper(root: &Path, name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { if name == "mvnw" { format!("{name}.cmd") } else { format!("{name}.bat") } } else { name.to_string() };
    let p = root.join(file);
    p.is_file().then_some(p)
}

/// Klassenname ohne Paket (fuer den Titel des Terminals)
fn short(fqcn: &str) -> String {
    fqcn.rsplit('.').next().unwrap_or(fqcn).to_string()
}

/// Welche Werkzeuge braucht das Starten (bzw. Testen) dieser Datei?
pub fn needed_tools(file: &Path, text: &str, project: Option<&Path>, mode: Mode) -> Vec<super::super::Tool> {
    use super::super::Tool;
    let cfg = super::super::config();
    let mut v = vec![Tool::Jdk];
    match detect_build(file, source::package(text).as_deref(), project) {
        Build::Plain(_) => {
            v.push(Tool::JavaDrivers);
            if mode == Mode::Test {
                v.push(Tool::JUnit);
            }
        }
        Build::Maven(root) if !(cfg.use_wrapper && wrapper(&root, "mvnw").is_some()) => v.push(Tool::Maven),
        Build::Gradle(root) if !(cfg.use_wrapper && wrapper(&root, "gradlew").is_some()) => v.push(Tool::Gradle),
        _ => {}
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_root_strips_the_package_folders() {
        assert_eq!(source_root(Path::new("/p/src/app/dao/A.java"), Some("app.dao")), PathBuf::from("/p/src"));
        assert_eq!(source_root(Path::new("/p/src/A.java"), None), PathBuf::from("/p/src"));
        // Paket passt nicht zum Ordner: dann gilt der Ordner der Datei
        assert_eq!(source_root(Path::new("/p/x/A.java"), Some("app")), PathBuf::from("/p/x"));
        assert_eq!(Build::Plain(PathBuf::from("/p/src")).root(), PathBuf::from("/p"));
    }

    #[test]
    fn javac_output_is_parsed_with_column() {
        let out = "/p/src/app/Main.java:5: error: ';' expected\n        int x = 1\n                 ^\n/p/src/app/Main.java:9: Warnung: [unchecked] unchecked call\n1 error\n";
        let d = parse_javac(out);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0], Diag { file: "/p/src/app/Main.java".into(), line: 4, col: Some(17), message: "';' expected".into(), error: true });
        assert!(!d[1].error && d[1].line == 8);
    }

    #[test]
    fn maven_output_is_parsed() {
        let out = "[INFO] x\n[ERROR] /C:/Users/a b/p/src/main/java/app/Main.java:[12,5] cannot find symbol\n[ERROR] -> [Help 1]\n[ERROR] /C:/Users/a b/p/src/main/java/app/Main.java:[12,5] cannot find symbol\n";
        let d = parse_maven(out);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].file, PathBuf::from("C:/Users/a b/p/src/main/java/app/Main.java"));
        assert_eq!((d[0].line, d[0].col), (11, Some(4)));
    }

    #[test]
    fn build_tools_are_detected_upwards() {
        let dir = std::env::temp_dir().join(format!("easymysql-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("src/main/java/app");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(dir.join("pom.xml"), "<project/>").unwrap();
        assert_eq!(detect_build(&src.join("Main.java"), Some("app"), Some(&dir)), Build::Maven(dir.clone()));
        std::fs::remove_file(dir.join("pom.xml")).unwrap();
        std::fs::write(dir.join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build(&src.join("Main.java"), Some("app"), Some(&dir)), Build::Gradle(dir.clone()));
        std::fs::remove_file(dir.join("build.gradle.kts")).unwrap();
        assert_eq!(detect_build(&src.join("Main.java"), Some("app"), Some(&dir)), Build::Plain(dir.join("src/main/java")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn system_tools() -> Option<Tools> {
        let cfg = Config { jdk: "system".into(), ..Default::default() };
        let tools = super::super::super::detect(&std::env::temp_dir().join("easymysql-kein-werkzeug-ordner"), &cfg);
        tools.jdk.is_some().then_some(tools)
    }

    /// Ganzer Ablauf mit einem echten JDK (wird uebersprungen, wenn keins da ist)
    #[test]
    fn plain_project_compiles_runs_and_reports_errors() {
        let Some(tools) = system_tools() else { return };
        let dir = std::env::temp_dir().join(format!("easymysql-plain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pkg = dir.join("src/app");
        std::fs::create_dir_all(&pkg).unwrap();
        let main = "package app;\n\npublic class Main {\n    public static void main(String[] args) {\n        System.out.println(Helfer.text() + args.length);\n    }\n}\n";
        std::fs::write(pkg.join("Main.java"), main).unwrap();
        std::fs::write(pkg.join("Helfer.java"), "package app;\n\nclass Helfer {\n    static String text() { return \"hallo\"; }\n}\n").unwrap();
        let cfg = Config { program_args: "a \"b c\"".into(), jvm_args: "-ea".into(), ..Default::default() };
        let req = |file: &Path, text: &str| Request { file: file.to_path_buf(), text: text.into(), project: Some(dir.clone()), tools: Arc::new(tools.clone()), cfg: cfg.clone(), live: Arc::default() };
        // Aus einer Datei ohne main heraus startet die einzige Klasse mit main
        let helper = std::fs::read_to_string(pkg.join("Helfer.java")).unwrap();
        let launch = prepare(&req(&pkg.join("Helfer.java"), &helper), Mode::Run).unwrap();
        assert_eq!(launch.args[0], "-ea");
        assert_eq!(&launch.args[launch.args.len() - 3..], ["app.Main", "a", "b c"]);
        let out = Command::new(&launch.program).args(&launch.args).current_dir(&launch.cwd).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hallo2");
        // Fehler kommen mit Datei und Zeile zurueck
        let broken = main.replace("args.length);", "args.length)");
        std::fs::write(pkg.join("Main.java"), &broken).unwrap();
        let fail = prepare(&req(&pkg.join("Main.java"), &broken), Mode::Run).unwrap_err();
        assert_eq!(fail.diags.len(), 1, "{fail:?}");
        assert!(fail.diags[0].file.ends_with("Main.java") && fail.diags[0].line == 4 && fail.diags[0].error);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Maven- und Gradle-Projekt aus dem Projektdialog bauen, starten und testen (laedt Maven/Gradle und die
    /// Abhaengigkeiten): `cargo test build_tool_projects -- --ignored --nocapture`
    #[test]
    #[ignore = "braucht Internet und ein JDK"]
    fn build_tool_projects_compile_and_report_errors() {
        use super::super::super::{Tool, install};
        use super::super::project;
        for tool in [Tool::JavaDrivers, Tool::Maven, Tool::Gradle, Tool::JUnit] {
            if !super::super::super::tools().has(tool) {
                install::install_now(tool).unwrap();
            }
        }
        let tools = super::super::super::tools();
        assert!(tools.jdk.is_some(), "JDK fehlt");
        for kind in [project::Kind::Plain, project::Kind::Maven, project::Kind::GradleKotlin, project::Kind::GradleGroovy] {
            let dir = std::env::temp_dir().join(format!("easymysql-{}-{}", kind.id(), std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let spec = project::Spec {
                name: "Test".into(),
                kind,
                package: "app".into(),
                database: "test".into(),
                tables: vec![],
                host: "127.0.0.1".into(),
                port: 3306,
                user: "root".into(),
                password: None,
                driver: super::super::Driver::MariaDb,
                driver_version: install::driver_version(super::super::Driver::MariaDb),
                junit_version: Some(install::junit_version()),
            };
            let main = project::write(&dir, &spec).unwrap();
            let text = std::fs::read_to_string(&main).unwrap();
            let req = Request { file: main.clone(), text: text.clone(), project: Some(dir.clone()), tools: Arc::clone(&tools), cfg: Config::default(), live: Arc::default() };
            let launch = prepare(&req, Mode::Run).unwrap_or_else(|f| panic!("{kind:?}: {}\n{}", f.message, f.output));
            assert_eq!(launch.args.last().unwrap(), "app.Main");
            assert!(launch.args.iter().any(|a| a.contains("mariadb-java-client")), "{kind:?}: Treiber fehlt im Klassenpfad: {:?}", launch.args);
            // Die Klassen sind uebersetzt: das Programm startet (und scheitert erst an der Verbindung)
            let out = Command::new(&launch.program).args(&launch.args).current_dir(&launch.cwd).env("EASYMYSQL_DB_URL", "jdbc:mariadb://127.0.0.1:1/x?sslMode=disable").output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(!err.contains("ClassNotFoundException") && !err.contains("No suitable driver"), "{kind:?}: {err}");
            // Tests lassen sich vorbereiten
            let t = prepare(&req, Mode::Test).unwrap_or_else(|f| panic!("{kind:?} Test: {}\n{}", f.message, f.output));
            assert!(!t.args.is_empty());
            // .jar bauen
            let jar = super::super::jar::build(&req).unwrap_or_else(|f| panic!("{kind:?} jar: {}", f.message));
            assert!(jar.is_file());
            let out = Command::new(tools.jdk.as_ref().unwrap().java()).arg("-jar").arg(&jar).env("EASYMYSQL_DB_URL", "jdbc:mariadb://127.0.0.1:1/x?sslMode=disable").output().unwrap();
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(!err.contains("ClassNotFoundException") && !err.contains("No suitable driver") && !err.contains("no main manifest"), "{kind:?} jar: {err}");
            // Fehler im Code kommen mit Zeile zurueck
            let broken = text.replace("throws SQLException {", "throws SQLException {\n        int x = ;");
            std::fs::write(&main, &broken).unwrap();
            let fail = prepare(&Request { text: broken, ..req }, Mode::Run).unwrap_err();
            assert!(fail.diags.iter().any(|d| d.error && d.line == 6 && d.file.ends_with("Main.java")), "{kind:?}: {fail:?}");
            assert!(fail.diags.iter().enumerate().all(|(i, d)| !fail.diags[..i].contains(d)), "{kind:?}: doppelte Meldungen: {:?}", fail.diags);
            println!("{kind:?}: ok");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
