// Werkzeuge herunterladen: immer die neueste stabile Version, mit Pruefsumme, im Hintergrund.
// Quellen: Adoptium (Temurin-JDK), Maven Central (Treiber, Maven, JUnit), services.gradle.org (Gradle),
// python-build-standalone (Python), nodejs.org (Node.js), download.eclipse.org (JDT LS), GitHub-Releases
// (MinGit, GitHub CLI); Python- und Node-Pakete ueber pip bzw. npm.

use super::java::{Driver, java_dir, lib_dir};
use super::{Tool, is_stable, newest_stable, tool_root, tools};
use eframe::egui;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2";

/// Hoechstgroesse eines Downloads (das JDK hat etwa 200 MB)
const MAX_DOWNLOAD: u64 = 600 * 1024 * 1024;

pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .user_agent(concat!("EasyMySQL/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

pub fn get_text(url: &str) -> Result<String, String> {
    let mut r = agent(Duration::from_secs(30)).get(url).call().map_err(|e| format!("{url}: {e}"))?;
    r.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| format!("{url}: {e}"))
}

pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&get_text(url)?).map_err(|e| format!("{url}: {e}"))
}

// ---------------------------------------------------------------------------
// Versionen und Adressen

/// Neueste stabile Version eines Artefakts auf Maven Central. Das Feld `<release>` der Metadaten
/// zeigt auch auf Vorabversionen (z. B. Maven 4.0.0-rc), deshalb wird die Liste selbst ausgewertet.
pub fn latest_maven(group: &str, artifact: &str) -> Result<String, String> {
    let url = format!("{MAVEN_CENTRAL}/{}/{artifact}/maven-metadata.xml", group.replace('.', "/"));
    let xml = get_text(&url)?;
    let versions: Vec<&str> = xml.split("<version>").skip(1).filter_map(|s| s.split("</version>").next()).map(str::trim).collect();
    newest_stable(versions.iter().copied()).map(str::to_string).ok_or_else(|| format!("{artifact}: {}", crate::i18n::tr("keine stabile Version gefunden", "no stable version found")))
}

/// Datei auf Maven Central
fn central_url(group: &str, artifact: &str, version: &str, file: &str) -> String {
    format!("{MAVEN_CENTRAL}/{}/{artifact}/{version}/{file}", group.replace('.', "/"))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Checksum {
    Sha1(String),
    Sha256(String),
    Sha512(String),
}

/// Pruefsumme einer Datei auf Maven Central: die staerkste, die es gibt
pub fn central_checksum(url: &str) -> Result<Checksum, String> {
    for (ext, make) in [("sha512", Checksum::Sha512 as fn(String) -> Checksum), ("sha256", Checksum::Sha256), ("sha1", Checksum::Sha1)] {
        if let Ok(text) = get_text(&format!("{url}.{ext}")) {
            let hex = text.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(make(hex));
            }
        }
    }
    Err(crate::tr_format!("Keine Prüfsumme für {url}", "No checksum for {url}"))
}

/// Was zu laden ist
#[derive(Clone, Debug)]
pub struct Package {
    pub version: String,
    pub url: String,
    pub checksum: Checksum,
    /// Dateiname (bestimmt die Art des Archivs)
    pub file: String,
}

fn adoptium_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

fn adoptium_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" }
}

/// Neuestes Temurin-JDK der neuesten LTS-Version
pub fn latest_jdk() -> Result<Package, String> {
    let info = get_json("https://api.adoptium.net/v3/info/available_releases")?;
    let lts = info["most_recent_lts"].as_u64().ok_or("Adoptium: most_recent_lts")?;
    let url = format!("https://api.adoptium.net/v3/assets/latest/{lts}/hotspot?architecture={}&image_type=jdk&os={}&vendor=eclipse", adoptium_arch(), adoptium_os());
    let list = get_json(&url)?;
    let first = list.as_array().and_then(|a| a.first()).ok_or("Adoptium: kein JDK für dieses System")?;
    let pkg = &first["binary"]["package"];
    let link = pkg["link"].as_str().ok_or("Adoptium: link")?;
    let sha = pkg["checksum"].as_str().ok_or("Adoptium: checksum")?;
    let name = pkg["name"].as_str().unwrap_or("jdk.zip");
    // "25.0.5+8" -> Version fuer den Vergleich
    let version = first["version"]["openjdk_version"].as_str().or(first["version"]["semver"].as_str()).unwrap_or("").split('-').next().unwrap_or("").to_string();
    if !link.starts_with("https://") {
        return Err("Adoptium: unsichere Adresse".into());
    }
    Ok(Package { version, url: link.into(), checksum: Checksum::Sha256(sha.to_ascii_lowercase()), file: name.into() })
}

pub fn latest_maven_dist() -> Result<Package, String> {
    let version = latest_maven("org.apache.maven", "apache-maven")?;
    let file = format!("apache-maven-{version}-bin.zip");
    let url = central_url("org.apache.maven", "apache-maven", &version, &file);
    let checksum = central_checksum(&url)?;
    Ok(Package { version, url, checksum, file })
}

pub fn latest_gradle() -> Result<Package, String> {
    let v = get_json("https://services.gradle.org/versions/current")?;
    let version = v["version"].as_str().ok_or("Gradle: version")?.to_string();
    let url = v["downloadUrl"].as_str().ok_or("Gradle: downloadUrl")?.to_string();
    let sha = match v["checksum"].as_str() {
        Some(s) => s.to_string(),
        None => get_text(v["checksumUrl"].as_str().ok_or("Gradle: checksumUrl")?)?.trim().to_string(),
    };
    if !url.starts_with("https://") {
        return Err("Gradle: unsichere Adresse".into());
    }
    Ok(Package { file: format!("gradle-{version}-bin.zip"), version, url, checksum: Checksum::Sha256(sha.to_ascii_lowercase()) })
}

pub fn latest_driver(d: Driver) -> Result<Package, String> {
    let version = latest_maven(d.group(), d.artifact())?;
    let file = format!("{}-{version}.jar", d.artifact());
    let url = central_url(d.group(), d.artifact(), &version, &file);
    let checksum = central_checksum(&url)?;
    Ok(Package { version, url, checksum, file })
}

/// Neueste stabile Treiberversion fuer neue Projekte (heruntergeladen, sonst aus dem Netz, sonst fest)
pub fn driver_version(d: Driver) -> String {
    if let Some((_, v, _)) = tools().driver(d) {
        return v.clone();
    }
    latest_maven(d.group(), d.artifact()).unwrap_or_else(|_| d.fallback_version().to_string())
}

/// Neueste JUnit-Version (Jupiter und Konsolenstarter haben seit JUnit 6 dieselbe Nummer)
pub fn junit_version() -> String {
    if let Some((v, _)) = tools().junit.clone() {
        return v;
    }
    latest_maven("org.junit.jupiter", "junit-jupiter").unwrap_or_else(|_| "6.0.1".into())
}

pub fn latest_junit() -> Result<Package, String> {
    let (group, artifact) = ("org.junit.platform", "junit-platform-console-standalone");
    let version = latest_maven(group, artifact)?;
    let file = format!("{artifact}-{version}.jar");
    let url = central_url(group, artifact, &version, &file);
    let checksum = central_checksum(&url)?;
    Ok(Package { version, url, checksum, file })
}

/// Pruefsumme "sha256:…" aus der GitHub-API
fn github_digest(asset: &serde_json::Value) -> Option<Checksum> {
    asset["digest"].as_str()?.strip_prefix("sha256:").map(|h| Checksum::Sha256(h.to_ascii_lowercase()))
}

/// Neueste Version eines GitHub-Projekts: (Tag, Assets)
fn github_latest(repo: &str) -> Result<(String, Vec<serde_json::Value>), String> {
    let v = get_json(&format!("https://api.github.com/repos/{repo}/releases/latest"))?;
    let tag = v["tag_name"].as_str().ok_or_else(|| format!("{repo}: tag_name"))?.to_string();
    Ok((tag, v["assets"].as_array().cloned().unwrap_or_default()))
}

fn python_target() -> &'static str {
    match (cfg!(windows), cfg!(target_os = "macos"), cfg!(target_arch = "aarch64")) {
        (true, _, true) => "aarch64-pc-windows-msvc",
        (true, _, false) => "x86_64-pc-windows-msvc",
        (_, true, true) => "aarch64-apple-darwin",
        (_, true, false) => "x86_64-apple-darwin",
        (_, _, true) => "aarch64-unknown-linux-gnu",
        _ => "x86_64-unknown-linux-gnu",
    }
}

/// Neuestes stabiles CPython aus python-build-standalone (fertig entpackbar, mit pip und venv)
pub fn latest_python() -> Result<Package, String> {
    let (_, assets) = github_latest("astral-sh/python-build-standalone")?;
    let target = python_target();
    let mut best: Option<(String, &serde_json::Value)> = None;
    for a in &assets {
        let name = a["name"].as_str().unwrap_or("");
        let Some(rest) = name.strip_prefix("cpython-") else { continue };
        if !name.ends_with(&format!("{target}-install_only_stripped.tar.gz")) {
            continue;
        }
        let version = rest.split('+').next().unwrap_or("");
        if !is_stable(version) {
            continue;
        }
        if best.as_ref().is_none_or(|(v, _)| super::is_newer(version, v)) {
            best = Some((version.to_string(), a));
        }
    }
    let (version, asset) = best.ok_or_else(|| crate::i18n::tr("Kein Python für dieses System gefunden.", "No Python found for this system.").to_string())?;
    let url = asset["browser_download_url"].as_str().ok_or("python: url")?.to_string();
    let checksum = github_digest(asset).ok_or("python: digest")?;
    Ok(Package { file: asset["name"].as_str().unwrap_or("python.tar.gz").to_string(), version, url, checksum })
}

/// Neueste LTS-Version von Node.js
pub fn latest_node() -> Result<Package, String> {
    let list = get_json("https://nodejs.org/dist/index.json")?;
    let rel = list.as_array().and_then(|a| a.iter().find(|r| !r["lts"].is_boolean() || r["lts"].as_bool() == Some(true))).ok_or("Node.js: lts")?;
    let tag = rel["version"].as_str().ok_or("Node.js: version")?.to_string();
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let file = if cfg!(windows) {
        format!("node-{tag}-win-{arch}.zip")
    } else if cfg!(target_os = "macos") {
        format!("node-{tag}-darwin-{arch}.tar.gz")
    } else {
        format!("node-{tag}-linux-{arch}.tar.gz")
    };
    let sums = get_text(&format!("https://nodejs.org/dist/{tag}/SHASUMS256.txt"))?;
    let sha = sums.lines().find_map(|l| l.split_once("  ").filter(|(_, n)| n.trim() == file).map(|(h, _)| h.trim().to_ascii_lowercase())).ok_or("Node.js: SHASUMS256")?;
    Ok(Package { version: tag.trim_start_matches('v').to_string(), url: format!("https://nodejs.org/dist/{tag}/{file}"), checksum: Checksum::Sha256(sha), file })
}

/// Neuester Meilenstein des Eclipse JDT Language Server
pub fn latest_jdtls() -> Result<Package, String> {
    let base = "https://download.eclipse.org/jdtls/milestones";
    let html = get_text(&format!("{base}/"))?;
    let versions: Vec<&str> = html.split("milestones/").skip(1).filter_map(|s| s.split(['/', '"', '\'', '<']).next()).collect();
    let version = newest_stable(versions.iter().copied()).ok_or("JDT LS: version")?.to_string();
    let file = get_text(&format!("{base}/{version}/latest.txt"))?.trim().to_string();
    if !file.starts_with("jdt-language-server-") || file.contains('/') {
        return Err("JDT LS: latest.txt".into());
    }
    let url = format!("{base}/{version}/{file}");
    let sha = get_text(&format!("{url}.sha256"))?.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    Ok(Package { version, url, checksum: Checksum::Sha256(sha), file })
}

/// MinGit fuer Windows (Git ohne Installation)
pub fn latest_mingit() -> Result<Package, String> {
    let (tag, assets) = github_latest("git-for-windows/git")?;
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "64-bit" };
    let asset = assets
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("MinGit-") && n.ends_with(&format!("-{arch}.zip")) && !n.contains("busybox")))
        .ok_or("MinGit: asset")?;
    let version = tag.trim_start_matches('v').split(".windows").next().unwrap_or("").to_string();
    Ok(Package { version, url: asset["browser_download_url"].as_str().ok_or("MinGit: url")?.into(), checksum: github_digest(asset).ok_or("MinGit: digest")?, file: asset["name"].as_str().unwrap_or("mingit.zip").into() })
}

/// GitHub CLI
pub fn latest_gh() -> Result<Package, String> {
    let (tag, assets) = github_latest("cli/cli")?;
    let version = tag.trim_start_matches('v').to_string();
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    let name = if cfg!(windows) {
        format!("gh_{version}_windows_{arch}.zip")
    } else if cfg!(target_os = "macos") {
        format!("gh_{version}_macOS_{arch}.zip")
    } else {
        format!("gh_{version}_linux_{arch}.tar.gz")
    };
    let asset = assets.iter().find(|a| a["name"].as_str() == Some(&name)).ok_or("gh: asset")?;
    let checksum = match github_digest(asset) {
        Some(c) => c,
        None => {
            let sums = assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with("_checksums.txt"))).and_then(|a| a["browser_download_url"].as_str()).ok_or("gh: checksums")?;
            let text = get_text(sums)?;
            Checksum::Sha256(text.lines().find_map(|l| l.split_once("  ").filter(|(_, n)| n.trim() == name).map(|(h, _)| h.to_ascii_lowercase())).ok_or("gh: checksum")?)
        }
    };
    Ok(Package { version, url: asset["browser_download_url"].as_str().ok_or("gh: url")?.into(), checksum, file: name })
}

/// Neueste stabile Version eines Werkzeugs (fuer Updates und die Anzeige)
pub fn latest_version(tool: Tool) -> Result<String, String> {
    let cfg = super::config();
    match tool {
        Tool::Jdk => latest_jdk().map(|p| p.version),
        Tool::JavaDrivers => latest_maven(Driver::MariaDb.group(), Driver::MariaDb.artifact()),
        Tool::Maven => latest_maven_dist().map(|p| p.version),
        Tool::Gradle => latest_gradle().map(|p| p.version),
        Tool::JUnit => latest_junit().map(|p| p.version),
        Tool::Python => latest_python().map(|p| p.version),
        Tool::PythonDrivers => super::python::latest_on_pypi(cfg.python_driver.package()),
        Tool::Node => latest_node().map(|p| p.version),
        Tool::NodeDrivers => super::node::latest_on_npm(cfg.node_driver.package()),
        Tool::JavaLsp => latest_jdtls().map(|p| p.version),
        Tool::PythonLsp => super::node::latest_on_npm("pyright"),
        Tool::JsLsp => super::node::latest_on_npm("typescript"),
        Tool::Git => latest_mingit().map(|p| p.version),
        Tool::GhCli => latest_gh().map(|p| p.version),
    }
}

// ---------------------------------------------------------------------------
// Laden, pruefen, entpacken

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Laedt `url` nach `target`; erst nach bestandener Pruefung liegt die Datei dort.
pub fn download(url: &str, target: &Path, checksum: &Checksum, progress: &Progress, cancel: &AtomicBool) -> Result<(), String> {
    let part = target.with_extension("part");
    let result = (|| {
        let mut r = agent(Duration::from_secs(60 * 60)).get(url).call().map_err(|e| format!("{url}: {e}"))?;
        let total = r.body().content_length().unwrap_or(0);
        if total > MAX_DOWNLOAD {
            return Err(crate::i18n::tr("Der Download ist zu groß.", "The download is too large.").to_string());
        }
        progress.total.store(total, Ordering::Relaxed);
        progress.done.store(0, Ordering::Relaxed);
        let mut reader = r.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
        if let Some(dir) = part.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let (mut s1, mut s256, mut s512) = (Sha1::new(), Sha256::new(), Sha512::new());
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::i18n::tr("Abgebrochen.", "Cancelled.").to_string());
            }
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            match checksum {
                Checksum::Sha1(_) => s1.update(&buf[..n]),
                Checksum::Sha256(_) => s256.update(&buf[..n]),
                Checksum::Sha512(_) => s512.update(&buf[..n]),
            }
            done += n as u64;
            progress.done.store(done, Ordering::Relaxed);
        }
        file.sync_all().map_err(|e| e.to_string())?;
        let (got, want) = match checksum {
            Checksum::Sha1(w) => (hex(&s1.finalize()), w),
            Checksum::Sha256(w) => (hex(&s256.finalize()), w),
            Checksum::Sha512(w) => (hex(&s512.finalize()), w),
        };
        if !got.eq_ignore_ascii_case(want) {
            return Err(crate::tr_format!("Die Prüfsumme von {} stimmt nicht – die Datei wird nicht verwendet.", "The checksum of {} does not match – the file is not used.", url.rsplit('/').next().unwrap_or(url)));
        }
        drop(file);
        std::fs::rename(&part, target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Archiv (zip oder tar.gz) in einen neuen Ordner `into` entpacken und den Ordner des Werkzeugs liefern
/// (den einzigen Ordner darin, z. B. "jdk-25.0.5+8", sonst `into`).
pub fn extract(archive: &Path, into: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
        ar.set_preserve_permissions(true);
        // unpack() verhindert Pfade ausserhalb des Zielordners
        ar.unpack(into).map_err(|e| e.to_string())?;
    } else {
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            // Eintraege mit ".." oder absoluten Pfaden auslassen
            let Some(rel) = entry.enclosed_name() else { continue };
            let out = into.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut w = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut w).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777));
            }
        }
    }
    // Ein einzelner Ordner im Archiv ist das Werkzeug; sonst (JDT LS, MinGit) der Zielordner selbst
    let entries: Vec<PathBuf> = std::fs::read_dir(into).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    match entries.as_slice() {
        [one] if one.is_dir() => Ok(one.clone()),
        [] => Err(crate::i18n::tr("Das Archiv ist leer.", "The archive is empty.").into()),
        _ => Ok(into.to_path_buf()),
    }
}

/// Alte Fassungen eines Werkzeugs entfernen (laeuft gerade ein Programm daraus, bleibt sie bis zum
/// naechsten Mal liegen).
fn remove_others(dir: &Path, prefix: &str, keep: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p != keep && p.is_dir() && e.file_name().to_string_lossy().starts_with(prefix) {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
}

/// Einzelne .jar nach lib/ laden und aeltere Fassungen entfernen
fn install_jar(pkg: &Package, prefix: &str, progress: &Progress, cancel: &AtomicBool) -> Result<(), String> {
    let target = lib_dir().join(&pkg.file);
    if !target.is_file() {
        download(&pkg.url, &target, &pkg.checksum, progress, cancel)?;
    }
    if let Ok(rd) = std::fs::read_dir(lib_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(prefix) && e.path() != target {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(())
}

/// Archiv laden, entpacken und als `<dir>/<name>` ablegen; `prefix` erkennt alte Fassungen.
/// Hat das Archiv keinen einzelnen Ordner (JDT LS, MinGit) oder einen ohne Version ("python"), heisst der
/// Ordner `<prefix><version>`.
fn install_archive(tool: Tool, pkg: &Package, dir: &Path, prefix: &str, progress: &Progress, cancel: &AtomicBool, stage: &dyn Fn(String)) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Eigener Ordner je Aufruf: Hintergrundauftrag und ein gleichzeitiges `install_now` stoeren sich nicht
    static N: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(".laden-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::create_dir_all(&tmp);
    let result = (|| {
        let archive = tmp.join(&pkg.file);
        stage(crate::tr_format!("{} {} wird geladen …", "Downloading {} {} …", tool.label(), pkg.version));
        download(&pkg.url, &archive, &pkg.checksum, progress, cancel)?;
        stage(crate::tr_format!("{} {} wird entpackt …", "Unpacking {} {} …", tool.label(), pkg.version));
        let content = tmp.join("inhalt");
        let top = extract(&archive, &content)?;
        let top_name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (source, name) = if top != content && top_name.starts_with(prefix) && top_name[prefix.len()..].starts_with(|c: char| c.is_ascii_digit()) {
            (top, top_name)
        } else {
            (top, format!("{prefix}{}", pkg.version))
        };
        let target = dir.join(&name);
        if target.exists() {
            let _ = std::fs::remove_dir_all(&target);
        }
        std::fs::rename(&source, &target).map_err(|e| e.to_string())?;
        remove_others(dir, prefix, &target);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Ein Werkzeug in der neuesten Version installieren. Liefert die installierte Version.
fn install_tool(tool: Tool, progress: &Progress, cancel: &AtomicBool, stage: &dyn Fn(String)) -> Result<String, String> {
    let cfg = super::config();
    let live = Mutex::new(String::new());
    match tool {
        Tool::JavaDrivers => {
            std::fs::create_dir_all(lib_dir()).map_err(|e| e.to_string())?;
            let mut versions = Vec::new();
            for d in Driver::ALL {
                stage(format!("{} …", d.label()));
                let pkg = latest_driver(d)?;
                install_jar(&pkg, &format!("{}-", d.artifact()), progress, cancel)?;
                versions.push(format!("{} {}", d.label(), pkg.version));
            }
            return Ok(versions.join(", "));
        }
        Tool::JUnit => {
            std::fs::create_dir_all(lib_dir()).map_err(|e| e.to_string())?;
            stage(crate::i18n::tr("Neueste Version suchen …", "Looking for the newest version …").to_string());
            let pkg = latest_junit()?;
            stage(crate::tr_format!("{} {} wird geladen …", "Downloading {} {} …", tool.label(), pkg.version));
            install_jar(&pkg, "junit-platform-console-standalone-", progress, cancel)?;
            return Ok(pkg.version);
        }
        Tool::PythonDrivers => {
            let d = cfg.python_driver;
            stage(crate::tr_format!("{} wird mit pip installiert …", "Installing {} with pip …", d.label()));
            return super::python::install_driver(&tools(), d, &live);
        }
        Tool::NodeDrivers => {
            let d = cfg.node_driver;
            stage(crate::tr_format!("{} wird mit npm installiert …", "Installing {} with npm …", d.label()));
            super::node::npm_install(&tools(), &super::node::node_dir().join("lib"), &[d.package()], &live)?;
            return super::node::package_version(&super::node::modules_dir().join(d.package())).ok_or_else(|| "npm".to_string());
        }
        Tool::PythonLsp | Tool::JsLsp => {
            let pkgs: &[&str] = if tool == Tool::PythonLsp { &["pyright"] } else { &["typescript", "typescript-language-server"] };
            stage(crate::tr_format!("{} wird mit npm installiert …", "Installing {} with npm …", tool.label()));
            let prefix = tool_root().join("sprachserver").join("npm");
            super::node::npm_install(&tools(), &prefix, pkgs, &live)?;
            return super::node::package_version(&prefix.join("node_modules").join(pkgs[0])).ok_or_else(|| "npm".to_string());
        }
        Tool::Git if !cfg!(windows) => {
            return Err(crate::i18n::tr("Git bitte über die Paketverwaltung des Systems installieren (z. B. sudo apt install git).", "Please install Git with the system package manager (e.g. sudo apt install git).").into());
        }
        _ => {}
    }
    stage(crate::i18n::tr("Neueste Version suchen …", "Looking for the newest version …").to_string());
    let (pkg, dir, prefix) = match tool {
        Tool::Jdk => (latest_jdk()?, java_dir(), "jdk-"),
        Tool::Maven => (latest_maven_dist()?, java_dir(), "apache-maven-"),
        Tool::Gradle => (latest_gradle()?, java_dir(), "gradle-"),
        Tool::Python => (latest_python()?, super::python::python_dir(), "python-"),
        Tool::Node => (latest_node()?, super::node::node_dir(), "node-v"),
        Tool::JavaLsp => (latest_jdtls()?, tool_root().join("sprachserver"), "jdtls-"),
        Tool::Git => (latest_mingit()?, tool_root().join("git"), "git-"),
        Tool::GhCli => (latest_gh()?, tool_root().join("git"), "gh_"),
        _ => unreachable!(),
    };
    // Schon die neueste Version: nichts laden
    if tools().own_version(tool).is_some_and(|v| !super::is_newer(&pkg.version, &v)) {
        return Ok(pkg.version);
    }
    install_archive(tool, &pkg, &dir, prefix, progress, cancel, stage)?;
    Ok(pkg.version)
}

/// Ein Werkzeug sofort installieren (im aufrufenden Thread, ohne Anzeige)
pub fn install_now(tool: Tool) -> Result<String, String> {
    for dep in tool.requires() {
        if !tools().has(*dep) {
            install_tool(*dep, &Progress::default(), &AtomicBool::new(false), &|_| {})?;
            super::refresh();
        }
    }
    let r = install_tool(tool, &Progress::default(), &AtomicBool::new(false), &|_| {});
    super::refresh();
    r
}

// ---------------------------------------------------------------------------
// Hintergrundauftrag: eine Warteschlange von Werkzeugen

#[derive(Default)]
struct State {
    queue: VecDeque<Tool>,
    current: Option<Tool>,
    stage: String,
    progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    /// Letzter Fehler (bleibt stehen, bis neu gestartet wird)
    error: Option<String>,
    running: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

/// Momentaufnahme fuer die Anzeige
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub busy: bool,
    pub stage: String,
    /// 0..1, `None` wenn die Groesse unbekannt ist
    pub fraction: Option<f32>,
    pub done_bytes: u64,
    pub error: Option<String>,
}

pub fn status() -> Status {
    with_state(|s| {
        let total = s.progress.total.load(Ordering::Relaxed);
        let done = s.progress.done.load(Ordering::Relaxed);
        Status {
            busy: s.running,
            stage: s.stage.clone(),
            fraction: (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0)),
            done_bytes: done,
            error: s.error.clone(),
        }
    })
}

/// Text fuer die Anzeige, z. B. "JDK 25.0.5 wird geladen … 42 %"
pub fn status_line(st: &Status) -> String {
    match st.fraction {
        Some(f) if st.busy => format!("{}  {:.0} %", st.stage, f * 100.0),
        None if st.busy && st.done_bytes > 0 => format!("{}  {:.1} MB", st.stage, st.done_bytes as f64 / 1_048_576.0),
        _ => st.stage.clone(),
    }
}

/// Werkzeuge installieren bzw. aktualisieren (im Hintergrund; bereits wartende werden nicht doppelt
/// eingereiht).
pub fn install(list: &[Tool], ctx: &egui::Context) {
    // Voraussetzungen (Node fuer npm-Pakete, Python fuer pip) zuerst
    let have = tools();
    let mut wanted: Vec<Tool> = Vec::new();
    for t in list {
        for dep in t.requires() {
            if !have.has(*dep) && !wanted.contains(dep) {
                wanted.push(*dep);
            }
        }
        if !wanted.contains(t) {
            wanted.push(*t);
        }
    }
    let start = with_state(|s| {
        for t in &wanted {
            if s.current != Some(*t) && !s.queue.contains(t) {
                s.queue.push_back(*t);
            }
        }
        s.error = None;
        if s.running || s.queue.is_empty() {
            return false;
        }
        s.running = true;
        s.cancel = Arc::new(AtomicBool::new(false));
        true
    });
    if !start {
        return;
    }
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("werkzeuge-laden".into())
        .spawn(move || {
            loop {
                let next = with_state(|s| {
                    let t = s.queue.pop_front();
                    s.current = t;
                    s.progress = Arc::new(Progress::default());
                    // Ohne weiteren Auftrag bleibt die letzte Meldung ("… ist bereit.") stehen
                    if let Some(t) = t {
                        s.stage = crate::tr_format!("{} wird vorbereitet …", "Preparing {} …", t.label());
                    }
                    // Leer und "fertig" im selben Schritt: ein gleichzeitiges install() startet sonst keinen neuen Lauf
                    // und sein Auftrag bliebe liegen
                    if t.is_none() {
                        s.running = false;
                    }
                    t.map(|t| (t, Arc::clone(&s.progress), Arc::clone(&s.cancel)))
                });
                let Some((tool, progress, cancel)) = next else { break };
                let repaint = ctx.clone();
                // Fortschritt regelmaessig neu zeichnen lassen
                let ticking = Arc::new(AtomicBool::new(true));
                {
                    let (ticking, repaint) = (Arc::clone(&ticking), repaint.clone());
                    std::thread::spawn(move || {
                        while ticking.load(Ordering::Relaxed) {
                            repaint.request_repaint();
                            std::thread::sleep(Duration::from_millis(250));
                        }
                    });
                }
                let stage = |text: String| with_state(|s| s.stage = text);
                let result = install_tool(tool, &progress, &cancel, &stage);
                ticking.store(false, Ordering::Relaxed);
                super::refresh();
                with_state(|s| {
                    s.current = None;
                    match result {
                        Ok(v) => {
                            s.stage = crate::tr_format!("{} {} ist bereit.", "{} {} is ready.", tool.label(), v);
                        }
                        Err(e) => {
                            s.stage.clear();
                            s.error = Some(format!("{}: {e}", tool.label()));
                            // Nach einem Fehler nicht stur weiterladen
                            s.queue.clear();
                        }
                    }
                });
                repaint.request_repaint();
            }
            ctx.request_repaint();
        })
        .ok();
}

pub fn cancel() {
    with_state(|s| {
        s.cancel.store(true, Ordering::Relaxed);
        s.queue.clear();
    });
}

/// Fehlende Werkzeuge nachladen (nur auf Wunsch aufrufen, nicht in jedem Bild). Liefert true, wenn alles da ist.
pub fn ensure(needed: &[Tool], ctx: &egui::Context) -> bool {
    let t = tools();
    let missing: Vec<Tool> = needed.iter().copied().filter(|n| !t.has(*n)).collect();
    if missing.is_empty() {
        return true;
    }
    install(&missing, ctx);
    false
}

// ---------------------------------------------------------------------------
// Updates: einmal pro Woche (wenn Java benutzt wird) nach neueren Versionen der eigenen Werkzeuge sehen

fn check_file() -> PathBuf {
    tool_root().join("werkzeuge-letzte-pruefung.txt")
}

static CHECKING: AtomicBool = AtomicBool::new(false);

/// Liefert die eigenen Werkzeuge, fuer die es eine neuere stabile Version gibt.
pub fn find_updates() -> Vec<(Tool, String)> {
    let t = tools();
    let mut out = Vec::new();
    for tool in Tool::ALL {
        if tool == Tool::JavaDrivers {
            let newer = Driver::ALL.iter().find_map(|d| {
                let have = t.driver(*d)?.1.clone();
                let latest = latest_maven(d.group(), d.artifact()).ok()?;
                super::is_newer(&latest, &have).then(|| format!("{} {latest}", d.label()))
            });
            if let Some(v) = newer {
                out.push((tool, v));
            }
            continue;
        }
        let Some(have) = t.own_version(tool) else { continue };
        if let Ok(latest) = latest_version(tool)
            && super::is_newer(&latest, &have)
        {
            out.push((tool, latest));
        }
    }
    out
}

/// Je nach Einstellung (taeglich, woechentlich, …) nach Updates sehen und neuere Versionen gleich installieren.
pub fn weekly_update(ctx: &egui::Context) {
    let Some(interval) = super::config().updates.interval() else { return };
    let due = std::fs::metadata(check_file()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age > interval);
    if !due || !["java", "python", "node", "sprachserver", "git"].iter().any(|d| tool_root().join(d).is_dir()) || CHECKING.swap(true, Ordering::Relaxed) {
        return;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let updates = find_updates();
        let _ = crate::workspace::atomic_write(&check_file(), &crate::server::timestamp());
        let list: Vec<Tool> = updates.into_iter().map(|(t, _)| t).collect();
        if !list.is_empty() {
            install(&list, &ctx);
        }
        CHECKING.store(false, Ordering::Relaxed);
    });
}

/// Jetzt nach neueren Versionen sehen und sie installieren (Knopf in den Einstellungen)
pub fn check_updates_now(ctx: &egui::Context) {
    if CHECKING.swap(true, Ordering::Relaxed) {
        return;
    }
    with_state(|s| {
        s.error = None;
        s.stage = crate::i18n::tr("Suche nach neueren Versionen …", "Looking for newer versions …").into();
    });
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let updates = find_updates();
        let _ = crate::workspace::atomic_write(&check_file(), &crate::server::timestamp());
        if updates.is_empty() {
            with_state(|s| s.stage = crate::i18n::tr("Alles ist auf dem neuesten Stand.", "Everything is up to date.").into());
        } else {
            install(&updates.into_iter().map(|(t, _)| t).collect::<Vec<_>>(), &ctx);
        }
        CHECKING.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

pub fn checking() -> bool {
    CHECKING.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_is_extracted_into_its_single_folder_without_escaping() {
        let dir = std::env::temp_dir().join(format!("easymysql-extract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("gradle-9.0-bin.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            z.start_file("gradle-9.0/bin/gradle", opts).unwrap();
            z.write_all(b"#!/bin/sh\n").unwrap();
            z.start_file("../boese.txt", opts).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let top = extract(&zip_path, &dir.join("out")).unwrap();
        assert!(top.ends_with("gradle-9.0"));
        assert!(top.join("bin/gradle").is_file());
        assert!(!dir.join("boese.txt").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(top.join("bin/gradle")).unwrap().permissions().mode() & 0o111, 0o111);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Echte Downloads (Netz noetig): `cargo test java_downloads -- --ignored`
    #[test]
    #[ignore = "braucht Internet"]
    fn java_downloads_find_stable_versions_and_verify_them() {
        let m = latest_maven_dist().unwrap();
        assert!(super::super::is_stable(&m.version), "{m:?}");
        let g = latest_gradle().unwrap();
        assert!(super::super::is_stable(&g.version));
        let j = latest_jdk().unwrap();
        assert!(j.url.starts_with("https://"));
        let d = latest_driver(Driver::MariaDb).unwrap();
        let dir = std::env::temp_dir().join(format!("easymysql-dl-{}", std::process::id()));
        let target = dir.join(&d.file);
        download(&d.url, &target, &d.checksum, &Progress::default(), &AtomicBool::new(false)).unwrap();
        assert!(target.is_file());
        let bad = Checksum::Sha1("0".repeat(40));
        assert!(download(&d.url, &dir.join("x.jar"), &bad, &Progress::default(), &AtomicBool::new(false)).is_err());
        assert!(!dir.join("x.jar").exists() && !dir.join("x.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
