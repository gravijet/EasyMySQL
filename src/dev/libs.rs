// Bibliotheken einbinden, ohne pom.xml, build.gradle, requirements.txt oder package.json von Hand zu
// bearbeiten: suchen (Maven Central, PyPI, npm), die neueste stabile Version eintragen bzw. laden.

use super::install::{central_checksum, download, get_json, latest_maven, Progress};
use super::process::capture;
use crate::sqledit::Lang;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ecosystem {
    Maven,
    Pypi,
    Npm,
}

impl Ecosystem {
    pub fn for_lang(lang: Lang) -> Option<Ecosystem> {
        match lang {
            Lang::Java | Lang::Gradle => Some(Ecosystem::Maven),
            Lang::Python => Some(Ecosystem::Pypi),
            Lang::JavaScript | Lang::TypeScript => Some(Ecosystem::Npm),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ecosystem::Maven => "Maven Central",
            Ecosystem::Pypi => "PyPI",
            Ecosystem::Npm => "npm",
        }
    }
}

/// Suchtreffer: "gruppe:artefakt" bzw. Paketname
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub version: String,
    pub description: String,
}

fn encode(q: &str) -> String {
    q.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Bibliotheken suchen
pub fn search(eco: Ecosystem, query: &str) -> Result<Vec<Hit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    match eco {
        Ecosystem::Maven => {
            // "gruppe:artefakt" direkt, sonst Volltextsuche
            let url = match q.split_once(':') {
                Some((g, a)) => format!("https://search.maven.org/solrsearch/select?q=g:{}+AND+a:{}&rows=20&wt=json", encode(g), encode(a)),
                None => format!("https://search.maven.org/solrsearch/select?q={}&rows=25&wt=json", encode(q)),
            };
            let v = get_json(&url)?;
            Ok(v["response"]["docs"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|d| d["p"].as_str().is_none_or(|p| p == "jar" || p == "bundle"))
                        .filter_map(|d| {
                            Some(Hit {
                                id: format!("{}:{}", d["g"].as_str()?, d["a"].as_str()?),
                                version: d["latestVersion"].as_str().unwrap_or("").to_string(),
                                description: crate::tr_format!("{} Versionen", "{} versions", d["versionCount"].as_u64().unwrap_or(0)),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
        Ecosystem::Npm => {
            let v = get_json(&format!("https://registry.npmjs.org/-/v1/search?text={}&size=25", encode(q)))?;
            Ok(v["objects"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| {
                            let p = &o["package"];
                            Some(Hit { id: p["name"].as_str()?.to_string(), version: p["version"].as_str().unwrap_or("").into(), description: p["description"].as_str().unwrap_or("").chars().take(140).collect() })
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
        Ecosystem::Pypi => {
            // PyPI hat keine Such-API: den Namen direkt nachschlagen (auch mit - statt _ und umgekehrt)
            let mut out = Vec::new();
            for name in [q.to_string(), q.replace('_', "-"), q.replace('-', "_")] {
                if out.iter().any(|h: &Hit| h.id.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                if let Ok(v) = get_json(&format!("https://pypi.org/pypi/{}/json", encode(&name))) {
                    let info = &v["info"];
                    out.push(Hit { id: info["name"].as_str().unwrap_or(&name).to_string(), version: info["version"].as_str().unwrap_or("").into(), description: info["summary"].as_str().unwrap_or("").chars().take(140).collect() });
                    break;
                }
            }
            if out.is_empty() {
                return Err(crate::tr_format!("Kein Paket „{q}“ auf PyPI. Bitte den genauen Namen eingeben (z. B. requests).", "No package “{q}” on PyPI. Please enter the exact name (e.g. requests)."));
            }
            Ok(out)
        }
    }
}

/// Wohin die Bibliothek kommt
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Pom(PathBuf),
    Gradle(PathBuf),
    /// Einfaches Java-Projekt: .jar nach lib/
    JavaLib(PathBuf),
    Requirements(PathBuf),
    Pyproject(PathBuf),
    /// Ohne Projektdatei: in den eigenen Ordner von EasyMySQL (fuer alle Skripte)
    PythonShared,
    Package(PathBuf),
    NodeShared,
}

impl Target {
    pub fn describe(&self) -> String {
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match self {
            Target::Pom(p) | Target::Gradle(p) | Target::Requirements(p) | Target::Pyproject(p) => crate::tr_format!("wird in {} eingetragen", "is added to {}", name(p)),
            Target::JavaLib(d) => crate::tr_format!("wird nach {}/ geladen", "is downloaded to {}/", name(d)),
            Target::Package(_) => crate::i18n::tr("wird mit npm install in package.json eingetragen", "is added to package.json with npm install").into(),
            Target::PythonShared | Target::NodeShared => crate::i18n::tr("wird für alle Skripte ohne Projektdatei installiert", "is installed for all scripts without a project file").into(),
        }
    }
}

pub fn target_for(lang: Lang, file: &Path, project: Option<&Path>) -> Option<Target> {
    match lang {
        Lang::Java | Lang::Gradle => {
            let text = std::fs::read_to_string(file).unwrap_or_default();
            Some(match super::java::run::detect_build(file, super::java::source::package(&text).as_deref(), project) {
                super::java::run::Build::Maven(r) => Target::Pom(r.join("pom.xml")),
                super::java::run::Build::Gradle(r) => Target::Gradle(if r.join("build.gradle.kts").is_file() { r.join("build.gradle.kts") } else { r.join("build.gradle") }),
                b @ super::java::run::Build::Plain(_) => Target::JavaLib(b.root().join("lib")),
            })
        }
        Lang::Python => Some(match super::python::project_root(file, project) {
            Some(r) if r.join("requirements.txt").is_file() => Target::Requirements(r.join("requirements.txt")),
            Some(r) if r.join("pyproject.toml").is_file() => Target::Pyproject(r.join("pyproject.toml")),
            _ => Target::PythonShared,
        }),
        Lang::JavaScript | Lang::TypeScript => Some(match super::node::project_root(file, project) {
            Some(r) => Target::Package(r),
            None => Target::NodeShared,
        }),
        _ => None,
    }
}

/// `<dependency>` in die pom.xml (in den Block direkt unter <project>, nicht in Plugins oder dependencyManagement)
pub fn add_to_pom(pom: &str, group: &str, artifact: &str, version: &str) -> Result<String, String> {
    if pom.contains(&format!("<artifactId>{artifact}</artifactId>")) && pom.contains(&format!("<groupId>{group}</groupId>")) {
        return Err(crate::i18n::tr("Die Bibliothek ist schon eingetragen.", "The library is already listed.").into());
    }
    let dep = format!("        <dependency>\n            <groupId>{group}</groupId>\n            <artifactId>{artifact}</artifactId>\n            <version>{version}</version>\n        </dependency>\n");
    let mut search = 0;
    while let Some(i) = pom[search..].find("</dependencies>").map(|i| i + search) {
        let before = &pom[..i];
        let open = |tag: &str| before.matches(&format!("<{tag}>")).count() + before.matches(&format!("<{tag} ")).count();
        let close = |tag: &str| before.matches(&format!("</{tag}>")).count();
        if open("dependencyManagement") == close("dependencyManagement") && open("plugin") == close("plugin") && open("profile") == close("profile") {
            let line_start = before.rfind('\n').map_or(0, |p| p + 1);
            let mut out = pom.to_string();
            out.insert_str(line_start, &dep);
            return Ok(out);
        }
        search = i + 1;
    }
    let at = pom.rfind("</project>").ok_or_else(|| crate::i18n::tr("pom.xml ohne </project>.", "pom.xml without </project>.").to_string())?;
    let mut out = pom.to_string();
    out.insert_str(at, &format!("    <dependencies>\n{dep}    </dependencies>\n"));
    Ok(out)
}

/// Eintrag in den obersten dependencies-Block von build.gradle(.kts)
pub fn add_to_gradle(script: &str, kts: bool, coord: &str) -> Result<String, String> {
    if script.contains(coord.rsplit_once(':').map(|(ga, _)| ga).unwrap_or(coord)) {
        return Err(crate::i18n::tr("Die Bibliothek ist schon eingetragen.", "The library is already listed.").into());
    }
    let line = if kts { format!("    implementation(\"{coord}\")\n") } else { format!("    implementation '{coord}'\n") };
    // "dependencies {" auf oberster Ebene suchen
    let mut depth = 0i32;
    let bytes: Vec<char> = script.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && script.chars().skip(i).take(12).collect::<String>() == "dependencies" {
            // Bis zur oeffnenden Klammer, dann die passende schliessende suchen
            let mut j = i + 12;
            while j < bytes.len() && bytes[j].is_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == '{' {
                let mut d = 0;
                let mut k = j;
                while k < bytes.len() {
                    match bytes[k] {
                        '{' => d += 1,
                        '}' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                // Vor der schliessenden Klammer (am Anfang ihrer Zeile) einfuegen
                let close: String = bytes[..k].iter().collect();
                let at = close.rfind('\n').map_or(close.len(), |p| p + 1);
                let mut out: String = bytes[..at].iter().collect();
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&line);
                out.extend(bytes[at..].iter());
                return Ok(out);
            }
        }
        i += 1;
    }
    let mut out = script.trim_end().to_string();
    out.push_str(&format!("\n\ndependencies {{\n{line}}}\n"));
    Ok(out)
}

/// Paket in requirements.txt eintragen
pub fn add_to_requirements(text: &str, name: &str, version: &str) -> Result<String, String> {
    let norm = |s: &str| s.to_lowercase().replace('_', "-");
    if text.lines().any(|l| norm(l.split(['=', '>', '<', '~', '!', ' ', '[', ';']).next().unwrap_or("").trim()) == norm(name)) {
        return Err(crate::i18n::tr("Das Paket ist schon eingetragen.", "The package is already listed.").into());
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{name}>={version}\n"));
    Ok(out)
}

/// Paket in pyproject.toml ([project] dependencies = [...]) eintragen
pub fn add_to_pyproject(text: &str, name: &str, version: &str) -> Result<String, String> {
    let entry = format!("\"{name}>={version}\"");
    if text.to_lowercase().contains(&format!("\"{}", name.to_lowercase())) {
        return Err(crate::i18n::tr("Das Paket ist schon eingetragen.", "The package is already listed.").into());
    }
    if let Some(i) = text.find("dependencies = [") {
        let start = i + "dependencies = [".len();
        let end = text[start..].find(']').map(|e| e + start).ok_or("pyproject.toml: ]")?;
        let inner = text[start..end].trim_end();
        let mut out = text[..start].to_string();
        if inner.trim().is_empty() {
            out.push_str(&format!("\n    {entry},\n"));
        } else {
            out.push_str(inner);
            if !inner.ends_with(',') {
                out.push(',');
            }
            out.push_str(&format!("\n    {entry},\n"));
        }
        out.push_str(&text[end..]);
        return Ok(out);
    }
    match text.find("[project]") {
        Some(i) => {
            let line_end = text[i..].find('\n').map_or(text.len(), |e| i + e + 1);
            let mut out = text[..line_end].to_string();
            out.push_str(&format!("dependencies = [\n    {entry},\n]\n"));
            out.push_str(&text[line_end..]);
            Ok(out)
        }
        None => Err(crate::i18n::tr("pyproject.toml ohne [project].", "pyproject.toml without [project].").into()),
    }
}

/// Abhaengigkeiten aus einer POM (nur compile/runtime mit fester Version)
fn pom_deps(pom: &str, own_version: &str) -> Vec<(String, String, String)> {
    let body = pom.split("<dependencyManagement>").next().unwrap_or(pom);
    let Some(start) = body.find("<dependencies>") else { return Vec::new() };
    let end = body[start..].find("</dependencies>").map_or(body.len(), |e| start + e);
    let tag = |s: &str, t: &str| s.split(&format!("<{t}>")).nth(1).and_then(|x| x.split(&format!("</{t}>")).next()).map(|x| x.trim().to_string());
    body[start..end]
        .split("<dependency>")
        .skip(1)
        .filter_map(|d| {
            let scope = tag(d, "scope").unwrap_or_else(|| "compile".into());
            if !(scope == "compile" || scope == "runtime") || tag(d, "optional").as_deref() == Some("true") {
                return None;
            }
            let v = tag(d, "version")?;
            let v = if v == "${project.version}" { own_version.to_string() } else { v };
            if v.contains('$') || v.contains('[') || v.contains('(') {
                return None;
            }
            Some((tag(d, "groupId")?, tag(d, "artifactId")?, v))
        })
        .collect()
}

/// .jar samt direkten Abhaengigkeiten (zwei Ebenen) nach `lib` laden. Liefert die Dateinamen.
fn download_jars(lib: &Path, group: &str, artifact: &str, version: &str, live: &Mutex<String>) -> Result<Vec<String>, String> {
    let mut todo = vec![(group.to_string(), artifact.to_string(), version.to_string(), 0)];
    let mut done: Vec<String> = Vec::new();
    std::fs::create_dir_all(lib).map_err(|e| e.to_string())?;
    while let Some((g, a, v, depth)) = todo.pop() {
        let key = format!("{g}:{a}");
        if done.iter().any(|d| d.starts_with(&format!("{a}-"))) || done.contains(&key) {
            continue;
        }
        let base = format!("https://repo1.maven.org/maven2/{}/{a}/{v}/{a}-{v}", g.replace('.', "/"));
        let file = format!("{a}-{v}.jar");
        *live.lock().unwrap() = crate::tr_format!("{file} wird geladen …", "Downloading {file} …");
        let target = lib.join(&file);
        if !target.is_file() {
            let url = format!("{base}.jar");
            let sum = central_checksum(&url)?;
            download(&url, &target, &sum, &Progress::default(), &AtomicBool::new(false))?;
        }
        done.push(file);
        if depth < 2
            && let Ok(pom) = super::install::get_text(&format!("{base}.pom"))
        {
            for (dg, da, dv) in pom_deps(&pom, &v) {
                todo.push((dg, da, dv, depth + 1));
            }
        }
    }
    Ok(done)
}

/// Bibliothek eintragen bzw. laden. Liefert eine Meldung fuer die Statuszeile.
pub fn add(target: &Target, hit: &Hit, live: &Mutex<String>) -> Result<String, String> {
    let tools = super::tools();
    match target {
        Target::Pom(p) | Target::Gradle(p) | Target::JavaLib(p) => {
            let (g, a) = hit.id.split_once(':').ok_or("group:artifact")?;
            let version = latest_maven(g, a).unwrap_or_else(|_| hit.version.clone());
            match target {
                Target::Pom(_) => {
                    let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
                    crate::workspace::atomic_write(p, &add_to_pom(&text, g, a, &version)?).map_err(|e| e.to_string())?;
                }
                Target::Gradle(_) => {
                    let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
                    let kts = p.extension().is_some_and(|e| e == "kts");
                    crate::workspace::atomic_write(p, &add_to_gradle(&text, kts, &format!("{g}:{a}:{version}"))?).map_err(|e| e.to_string())?;
                }
                _ => {
                    let files = download_jars(p, g, a, &version, live)?;
                    return Ok(crate::tr_format!("{} geladen: {}", "Downloaded {}: {}", a, files.join(", ")));
                }
            }
            Ok(crate::tr_format!("{a} {version} eingetragen. Maven/Gradle lädt es beim nächsten Start.", "Added {a} {version}. Maven/Gradle downloads it on the next run."))
        }
        Target::Requirements(p) => {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            crate::workspace::atomic_write(p, &add_to_requirements(&text, &hit.id, &hit.version)?).map_err(|e| e.to_string())?;
            Ok(crate::tr_format!("{} {} eingetragen; die .venv bekommt es beim nächsten Start.", "Added {} {}; the .venv gets it on the next run.", hit.id, hit.version))
        }
        Target::Pyproject(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
            crate::workspace::atomic_write(p, &add_to_pyproject(&text, &hit.id, &hit.version)?).map_err(|e| e.to_string())?;
            Ok(crate::tr_format!("{} {} eingetragen; die .venv bekommt es beim nächsten Start.", "Added {} {}; the .venv gets it on the next run.", hit.id, hit.version))
        }
        Target::PythonShared => {
            let py = tools.python.as_ref().ok_or_else(|| crate::i18n::tr("Kein Python gefunden.", "No Python found.").to_string())?;
            let site = super::python::site_dir();
            let mut cmd = std::process::Command::new(&py.exe);
            cmd.args(["-m", "pip", "install", "--upgrade", "--no-warn-script-location", "--target"]).arg(&site).arg(&hit.id);
            let (ok, out) = capture(cmd, live)?;
            if !ok {
                return Err(super::process::tail(&out, 8));
            }
            super::refresh();
            Ok(crate::tr_format!("{} installiert.", "Installed {}.", hit.id))
        }
        Target::Package(root) => {
            let npm = tools.npm.clone().ok_or_else(|| crate::i18n::tr("npm fehlt.", "npm is missing.").to_string())?;
            let mut cmd = std::process::Command::new(npm);
            if let Some(p) = super::process::tool_path(&tools) {
                cmd.env("PATH", p);
            }
            cmd.current_dir(root).args(["install", "--no-audit", "--no-fund"]).arg(format!("{}@latest", hit.id));
            let (ok, out) = capture(cmd, live)?;
            if !ok {
                return Err(super::process::tail(&out, 8));
            }
            Ok(crate::tr_format!("{} installiert und in package.json eingetragen.", "Installed {} and added it to package.json.", hit.id))
        }
        Target::NodeShared => {
            super::node::npm_install(&tools, &super::node::node_dir().join("lib"), &[&hit.id], live)?;
            Ok(crate::tr_format!("{} installiert.", "Installed {}.", hit.id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pom_gets_the_dependency_in_the_right_block() {
        let pom = "<project>\n  <dependencyManagement>\n    <dependencies>\n    </dependencies>\n  </dependencyManagement>\n    <dependencies>\n        <dependency>\n            <groupId>x</groupId>\n        </dependency>\n    </dependencies>\n  <build><plugins><plugin><dependencies></dependencies></plugin></plugins></build>\n</project>\n";
        let out = add_to_pom(pom, "com.google.code.gson", "gson", "2.14.0").unwrap();
        let i = out.find("<artifactId>gson</artifactId>").unwrap();
        assert!(i > out.find("</dependencyManagement>").unwrap() && i < out.find("<build>").unwrap(), "{out}");
        assert!(add_to_pom(&out, "com.google.code.gson", "gson", "2.14.0").is_err());
        let bare = add_to_pom("<project>\n</project>\n", "a", "b", "1").unwrap();
        assert!(bare.contains("    <dependencies>\n        <dependency>"));
    }

    #[test]
    fn gradle_scripts_get_the_dependency() {
        let kts = "plugins {\n    application\n}\n\ndependencies {\n    implementation(\"a:b:1\")\n}\n\ntasks.test {\n    dependencies {}\n}\n";
        let out = add_to_gradle(kts, true, "com.google.code.gson:gson:2.14.0").unwrap();
        assert!(out.contains("    implementation(\"a:b:1\")\n    implementation(\"com.google.code.gson:gson:2.14.0\")\n}\n"), "{out}");
        let groovy = add_to_gradle("plugins { id 'java' }\n", false, "x:y:1").unwrap();
        assert!(groovy.ends_with("dependencies {\n    implementation 'x:y:1'\n}\n"));
    }

    #[test]
    fn python_files_get_the_package() {
        assert_eq!(add_to_requirements("PyMySQL>=1.1\n", "requests", "2.32.0").unwrap(), "PyMySQL>=1.1\nrequests>=2.32.0\n");
        assert!(add_to_requirements("Requests==2\n", "requests", "3").is_err());
        let py = "[project]\nname = \"x\"\ndependencies = [\n    \"PyMySQL>=1.1\",\n]\n";
        assert_eq!(add_to_pyproject(py, "requests", "2").unwrap(), "[project]\nname = \"x\"\ndependencies = [\n    \"PyMySQL>=1.1\",\n    \"requests>=2\",\n]\n");
        assert!(add_to_pyproject("[project]\nname = \"x\"\n", "a", "1").unwrap().contains("dependencies = [\n    \"a>=1\",\n]"));
    }

    #[test]
    fn pom_dependencies_for_plain_projects() {
        let pom = "<project><dependencies><dependency><groupId>g</groupId><artifactId>a</artifactId><version>1.0</version></dependency><dependency><groupId>t</groupId><artifactId>junit</artifactId><version>4</version><scope>test</scope></dependency><dependency><groupId>p</groupId><artifactId>q</artifactId><version>${project.version}</version></dependency></dependencies></project>";
        assert_eq!(pom_deps(pom, "2.0"), vec![("g".into(), "a".into(), "1.0".into()), ("p".into(), "q".into(), "2.0".into())]);
    }
}
