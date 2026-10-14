// Dateien von einem anderen Rechner holen: aus dem Netz (URL) oder ueber SSH vom Server.
// Die Dateien landen in einem eigenen temporaeren Ordner, der beim Loslassen von `Fetched` geloescht wird.

use crate::ssh::{self, SshConfig};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Location {
    /// Adresse im Netz (http oder https)
    Url(String),
    /// Pfade (einer je Zeile) auf einem Server, erreichbar ueber SSH
    Ssh { cfg: SshConfig, paths: String },
}

/// Heruntergeladene Dateien; der Ordner verschwindet mit diesem Wert
pub struct Fetched {
    pub files: Vec<PathBuf>,
    dir: PathBuf,
}

impl Drop for Fetched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const MAX_BYTES: u64 = 32 * 1024 * 1024 * 1024;

fn temp_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "EasyMySQL-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
    ));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Dateiname aus einer Adresse oder einem Pfad (ohne Ordner, Parameter und Sonderzeichen)
pub fn file_name(from: &str, fallback: &str) -> String {
    let base = from.split(['?', '#']).next().unwrap_or("");
    let last = base.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    if clean.is_empty() || clean.starts_with('.') && clean.len() < 3 { fallback.to_string() } else { clean }
}

fn download(url: &str, dest: &std::path::Path, cancel: &AtomicBool) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(crate::i18n::text("Die Adresse muss mit http:// oder https:// beginnen.").into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60 * 60))).user_agent(concat!("EasyMySQL/", env!("CARGO_PKG_VERSION"))).build().into();
    let mut resp = agent.get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(c) => crate::tr_format!("Der Server antwortet mit Fehler {c}.", "The server answered with error {c}."),
        other => other.to_string(),
    })?;
    let mut reader = resp.body_mut().as_reader().take(MAX_BYTES);
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::i18n::text("Abgebrochen.").into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
}

/// Dateien holen. `ext` ist die Endung, die ein Name ohne Endung bekommt (z. B. "sqlite").
pub fn fetch(loc: &Location, ext: &str, cancel: &AtomicBool) -> Result<Fetched, String> {
    let dir = temp_dir()?;
    let mut files = Vec::new();
    let result = (|| match loc {
        Location::Url(url) => {
            let url = url.trim();
            let mut name = file_name(url, &format!("download.{ext}"));
            if !name.contains('.') {
                name.push('.');
                name.push_str(ext);
            }
            let dest = dir.join(name);
            download(url, &dest, cancel)?;
            files.push(dest);
            Ok(())
        }
        Location::Ssh { cfg, paths } => {
            let list: Vec<&str> = paths.lines().map(str::trim).filter(|p| !p.is_empty()).collect();
            if list.is_empty() {
                return Err(crate::i18n::text("Kein Pfad auf dem Server angegeben.").into());
            }
            for (i, p) in list.iter().enumerate() {
                let mut name = file_name(p, &format!("datei{}.{ext}", i + 1));
                if !name.contains('.') {
                    name.push('.');
                    name.push_str(ext);
                }
                // Gleiche Namen aus verschiedenen Ordnern nicht ueberschreiben
                let mut dest = dir.join(&name);
                let mut k = 2;
                while dest.exists() {
                    dest = dir.join(format!("{k}-{name}"));
                    k += 1;
                }
                ssh::fetch_file(cfg, p, &dest, cancel)?;
                files.push(dest);
            }
            Ok(())
        }
    })();
    match result {
        Ok(()) => Ok(Fetched { files, dir }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_addresses_and_paths() {
        assert_eq!(file_name("https://x.org/data/shop.sqlite?token=1#a", "f.db"), "shop.sqlite");
        assert_eq!(file_name("/var/lib/app/dump.rdb", "f"), "dump.rdb");
        assert_eq!(file_name("C:\\temp\\a b.csv", "f"), "ab.csv");
        assert_eq!(file_name("https://x.org/", "fallback.db"), "fallback.db");
        assert_eq!(file_name("..", "fallback"), "fallback");
    }

    #[test]
    fn rejects_unusable_input() {
        let cancel = AtomicBool::new(false);
        assert!(fetch(&Location::Url("ftp://x/y".into()), "db", &cancel).is_err());
        assert!(fetch(&Location::Ssh { cfg: SshConfig::default(), paths: "   ".into() }, "db", &cancel).is_err());
    }

    /// Braucht sshd auf 127.0.0.1:2222 (Schluessel /tmp/sshd/id) und die Datei /tmp/sshd/remote.txt.
    #[test]
    #[ignore]
    fn fetch_over_ssh() {
        let cfg = SshConfig { enabled: true, host: "127.0.0.1".into(), port: 2222, user: std::env::var("USER").unwrap_or_default(), key: "/tmp/sshd/id".into(), password: String::new() };
        let f = fetch(&Location::Ssh { cfg: cfg.clone(), paths: "/tmp/sshd/remote.txt".into() }, "csv", &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read_to_string(&f.files[0]).unwrap().trim(), "hello");
        let dir = f.files[0].parent().unwrap().to_path_buf();
        drop(f);
        assert!(!dir.exists());
        let e = fetch(&Location::Ssh { cfg, paths: "/tmp/sshd/missing.csv".into() }, "csv", &AtomicBool::new(false)).err().unwrap();
        assert!(e.contains("No such file") || e.contains("missing"), "{e}");
    }
}
