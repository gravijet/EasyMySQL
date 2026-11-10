// Java: JDK, JDBC-Treiber, Maven, Gradle und JUnit; Quelltext lesen, uebersetzen, starten, testen, .jar bauen.

pub mod format;
pub mod jar;
pub mod project;
pub mod run;
pub mod source;

use super::{Config, Dist, EXE, Tools, choice, is_stable, launcher, version_key, versioned_dirs, which};
use std::path::{Path, PathBuf};

/// Aeltere JDKs reichen nicht: der erzeugte Code nutzt Records und Textbloecke.
pub const MIN_JDK: u32 = 17;

#[cfg(windows)]
pub const CP_SEP: &str = ";";
#[cfg(not(windows))]
pub const CP_SEP: &str = ":";

/// JDBC-Treiber, der im erzeugten Code und in Maven/Gradle-Projekten verwendet wird
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Driver {
    MariaDb,
    MySql,
}

impl Driver {
    pub const ALL: [Driver; 2] = [Driver::MariaDb, Driver::MySql];

    pub fn id(self) -> &'static str {
        match self {
            Driver::MariaDb => "mariadb",
            Driver::MySql => "mysql",
        }
    }

    pub fn from_id(s: &str) -> Driver {
        if s == "mysql" { Driver::MySql } else { Driver::MariaDb }
    }

    /// Teil der JDBC-Adresse: `jdbc:<scheme>://`
    pub fn scheme(self) -> &'static str {
        self.id()
    }

    pub fn group(self) -> &'static str {
        match self {
            Driver::MariaDb => "org.mariadb.jdbc",
            Driver::MySql => "com.mysql",
        }
    }

    pub fn artifact(self) -> &'static str {
        match self {
            Driver::MariaDb => "mariadb-java-client",
            Driver::MySql => "mysql-connector-j",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Driver::MariaDb => "MariaDB Connector/J",
            Driver::MySql => "MySQL Connector/J",
        }
    }

    /// Version, die in neue Maven/Gradle-Projekte geschrieben wird, wenn nichts heruntergeladen werden kann
    pub fn fallback_version(self) -> &'static str {
        match self {
            Driver::MariaDb => "4.0.0",
            Driver::MySql => "26.10.0",
        }
    }
}

/// Gewaehlter Treiber (Einstellung)
pub fn driver() -> Driver {
    super::config().java_driver
}

#[derive(Clone, Debug, PartialEq)]
pub struct Jdk {
    pub home: PathBuf,
    /// z. B. "25.0.5"
    pub version: String,
    pub major: u32,
    /// Von EasyMySQL heruntergeladen
    pub own: bool,
}

impl Jdk {
    pub fn bin(&self) -> PathBuf {
        self.home.join("bin")
    }
    pub fn java(&self) -> PathBuf {
        self.bin().join(format!("java{EXE}"))
    }
    pub fn javac(&self) -> PathBuf {
        self.bin().join(format!("javac{EXE}"))
    }
    pub fn jdb(&self) -> PathBuf {
        self.bin().join(format!("jdb{EXE}"))
    }
}

/// Ordner fuer die eigenen Java-Werkzeuge
pub fn java_dir() -> PathBuf {
    super::tool_root().join("java")
}

pub fn lib_dir() -> PathBuf {
    java_dir().join("lib")
}

/// Hauptversion aus "1.8.0_402" (= 8) bzw. "21.0.2" (= 21)
fn major_of(version: &str) -> u32 {
    let k = version_key(version);
    match k.as_slice() {
        [1, m, ..] => *m as u32,
        [m, ..] => *m as u32,
        [] => 0,
    }
}

/// JDK in `home` (mit javac und der Datei `release`)
pub fn jdk_at(home: &Path, own: bool) -> Option<Jdk> {
    // macOS-Archive haben das JDK unter Contents/Home
    let home = if home.join("Contents/Home/bin").is_dir() { home.join("Contents/Home") } else { home.to_path_buf() };
    if !home.join("bin").join(format!("javac{EXE}")).is_file() {
        return None;
    }
    let release = std::fs::read_to_string(home.join("release")).unwrap_or_default();
    let version = release
        .lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let major = major_of(&version);
    Some(Jdk { home, version, major, own })
}

/// Neueste Datei `lib/<prefix><version>.jar` mit stabiler Version
fn newest_jar(lib: &Path, prefix: &str) -> Option<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(lib)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let v = name.strip_prefix(prefix)?.strip_suffix(".jar")?.to_string();
                    is_stable(&v).then(|| (v, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|x| std::cmp::Reverse(version_key(&x.0)));
    found.into_iter().next()
}

/// JDK, Treiber, Maven, Gradle und JUnit suchen
pub(super) fn detect(dir: &Path, cfg: &Config, t: &mut Tools) {
    let (own_ok, system_ok, fixed) = choice(&cfg.jdk);
    let mut jdks: Vec<Jdk> = Vec::new();
    if let Some(p) = &fixed {
        jdks.extend(jdk_at(p, false));
    }
    if own_ok {
        jdks.extend(versioned_dirs(dir, "jdk-").into_iter().filter_map(|(_, p)| jdk_at(&p, true)));
    }
    if system_ok {
        if let Some(h) = std::env::var_os("JAVA_HOME") {
            jdks.extend(jdk_at(Path::new(&h), false));
        }
        if let Some(javac) = which("javac").and_then(|p| std::fs::canonicalize(p).ok())
            && let Some(home) = javac.parent().and_then(|b| b.parent())
        {
            jdks.extend(jdk_at(home, false));
        }
        #[cfg(windows)]
        for base in ["Java", "Eclipse Adoptium", "Microsoft", "Zulu", "BellSoft", "Amazon Corretto"] {
            if let Some(pf) = std::env::var_os("ProgramFiles") {
                let root = Path::new(&pf).join(base);
                if let Ok(rd) = std::fs::read_dir(root) {
                    jdks.extend(rd.flatten().filter_map(|e| jdk_at(&e.path(), false)));
                }
            }
        }
        #[cfg(not(windows))]
        if let Ok(rd) = std::fs::read_dir("/usr/lib/jvm") {
            jdks.extend(rd.flatten().filter_map(|e| jdk_at(&e.path(), false)));
        }
    }
    // Ein fest eingestelltes JDK gilt immer; sonst hat das eigene Vorrang, dann das neueste
    if fixed.is_none() {
        jdks.sort_by(|a, b| b.own.cmp(&a.own).then(b.major.cmp(&a.major)));
    }
    t.jdk = jdks.iter().find(|j| j.major >= MIN_JDK).cloned();
    if t.jdk.is_none() {
        t.old_jdk = jdks.first().cloned();
    }

    for d in Driver::ALL {
        if let Some((v, p)) = newest_jar(&dir.join("lib"), &format!("{}-", d.artifact())) {
            t.drivers.push((d, v, p));
        }
    }
    t.junit = newest_jar(&dir.join("lib"), "junit-platform-console-standalone-");

    // Maven und Gradle: eigene, sonst aus dem PATH
    let dist = |prefix: &str, name: &str| -> Option<Dist> {
        if let Some((version, home)) = versioned_dirs(dir, prefix).into_iter().find(|(_, h)| launcher(h, name).is_some()) {
            let launcher = launcher(&home, name)?;
            return Some(Dist { home, version, own: true, launcher });
        }
        let found = which(name)?;
        let real = std::fs::canonicalize(&found).unwrap_or(found.clone());
        let home = real.parent().and_then(|b| b.parent()).map(Path::to_path_buf).unwrap_or_default();
        Some(Dist { home, version: String::new(), own: false, launcher: found })
    };
    t.maven = dist("apache-maven-", "mvn");
    t.gradle = dist("gradle-", "gradle");
}

/// JDBC-Adresse. Der MariaDB-Treiber verlangt ab Version 4 eine verschluesselte Verbindung; der eigene Server
/// auf diesem PC (und ein SSH-Tunnel, der ohnehin verschluesselt) bietet sie oft nicht an. Fuer Adressen auf
/// diesem PC wird TLS deshalb abgeschaltet, entfernte Server bleiben verschluesselt.
pub fn jdbc_url(driver: Driver, host: &str, port: u16, database: &str) -> String {
    let local = super::is_local(host);
    let own_tls = crate::db::local_tls_ca(host, port);
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    let params = match (own_tls, driver) {
        // Eigener Server mit eigenem Zertifikat: verschluesselt und gegen die eigene CA geprueft
        (Some(ca), Driver::MariaDb) => format!("?sslMode=verify-ca&serverSslCert={}", ca.display().to_string().replace('\\', "/")),
        (Some(_), Driver::MySql) => "?sslMode=REQUIRED".to_string(),
        (None, Driver::MariaDb) if local => "?sslMode=disable".to_string(),
        _ => String::new(),
    };
    format!("jdbc:{}://{host}:{port}/{database}{params}", driver.scheme())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_versions() {
        assert_eq!(major_of("1.8.0_402"), 8);
        assert_eq!(major_of("21.0.2"), 21);
    }

    #[test]
    fn jdbc_urls() {
        assert_eq!(jdbc_url(Driver::MariaDb, "127.0.0.1", 3306, "schule"), "jdbc:mariadb://127.0.0.1:3306/schule?sslMode=disable");
        assert_eq!(jdbc_url(Driver::MariaDb, "db.example.org", 3306, "schule"), "jdbc:mariadb://db.example.org:3306/schule");
        assert_eq!(jdbc_url(Driver::MySql, "::1", 3307, ""), "jdbc:mysql://[::1]:3307/");
    }
}
