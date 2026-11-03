// Fertige Programmdatei: eine .jar mit allen eigenen Klassen, Ressourcen und Abhaengigkeiten (auch dem
// JDBC-Treiber), die mit `java -jar Name.jar` ueberall laeuft, wo Java installiert ist.

use super::super::process::Failure;
use super::run::{Request, compile};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// Signaturen der Abhaengigkeiten passen nicht mehr zur zusammengefuegten Datei
fn skip_entry(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "META-INF/MANIFEST.MF"
        || upper == "META-INF/INDEX.LIST"
        || (upper.starts_with("META-INF/") && !upper[9..].contains('/') && [".SF", ".RSA", ".DSA", ".EC"].iter().any(|e| upper.ends_with(e)))
        || name == "module-info.class"
        || name.ends_with("/module-info.class") && name.starts_with("META-INF/versions/")
}

struct Writer {
    zip: zip::ZipWriter<std::fs::File>,
    seen: HashSet<String>,
    /// META-INF/services/…: Inhalte aller Abhaengigkeiten zusammen (sonst findet DriverManager den Treiber nicht)
    services: BTreeMap<String, Vec<String>>,
    multi_release: bool,
}

impl Writer {
    fn add(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        if name.starts_with("META-INF/services/") && !name.ends_with('/') {
            let list = self.services.entry(name.to_string()).or_default();
            for l in String::from_utf8_lossy(data).lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty()) {
                if !list.iter().any(|x| x == l) {
                    list.push(l.to_string());
                }
            }
            return Ok(());
        }
        if skip_entry(name) || !self.seen.insert(name.to_string()) {
            return Ok(());
        }
        self.zip.start_file(name, SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        self.zip.write_all(data).map_err(|e| e.to_string())
    }

    fn add_dir(&mut self, dir: &Path) -> Result<(), String> {
        fn walk(w: &mut Writer, base: &Path, dir: &Path, depth: usize) -> Result<(), String> {
            if depth > 32 {
                return Ok(());
            }
            let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
            let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(w, base, &p, depth + 1)?;
                } else if let Ok(rel) = p.strip_prefix(base) {
                    let name = rel.to_string_lossy().replace('\\', "/");
                    let data = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    w.add(&name, &data)?;
                }
            }
            Ok(())
        }
        walk(self, dir, dir, 0)
    }

    fn add_jar(&mut self, jar: &Path) -> Result<(), String> {
        let f = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", jar.display()))?;
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| e.to_string())?;
            if e.is_dir() {
                continue;
            }
            let name = e.name().map_err(|e| e.to_string())?.to_string();
            let mut data = Vec::with_capacity(e.size() as usize);
            e.read_to_end(&mut data).map_err(|e| e.to_string())?;
            if name.eq_ignore_ascii_case("META-INF/MANIFEST.MF") && String::from_utf8_lossy(&data).to_ascii_lowercase().contains("multi-release: true") {
                self.multi_release = true;
            }
            self.add(&name, &data)?;
        }
        Ok(())
    }
}

/// Dateiname der .jar: Name des Projektordners
fn jar_name(root: &Path, class: &str) -> String {
    let base = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let clean: String = base.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    let clean = clean.trim_matches('-').to_string();
    if clean.is_empty() { class.rsplit('.').next().unwrap_or("app").to_string() } else { clean }
}

/// Uebersetzen und `dist/<Projekt>.jar` schreiben. Liefert die Datei.
pub fn build(req: &Request) -> Result<PathBuf, Failure> {
    let c = compile(req)?;
    let root = c.build.root();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| Failure::msg(e.to_string()))?;
    let target = dist.join(format!("{}.jar", jar_name(&root, &c.class)));
    let part = target.with_extension("jar.part");
    let result = (|| -> Result<(), String> {
        let file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut w = Writer { zip: zip::ZipWriter::new(file), seen: HashSet::new(), services: BTreeMap::new(), multi_release: false };
        // Platz fuer das Manifest vorne: erst die Inhalte sammeln, dann schreiben
        let mut parts: Vec<PathBuf> = vec![c.classes.clone()];
        parts.extend(c.classpath.iter().filter(|p| **p != c.classes).cloned());
        // Manifest zuerst (manche Werkzeuge erwarten es am Anfang)
        let mut probe = false;
        for p in &parts {
            if p.is_file()
                && let Ok(f) = std::fs::File::open(p)
                && let Ok(mut z) = zip::ZipArchive::new(std::io::BufReader::new(f))
                && let Ok(mut m) = z.by_name("META-INF/MANIFEST.MF")
            {
                let mut s = String::new();
                let _ = m.read_to_string(&mut s);
                probe |= s.to_ascii_lowercase().contains("multi-release: true");
            }
        }
        let mut manifest = format!("Manifest-Version: 1.0\r\nMain-Class: {}\r\nCreated-By: EasyMySQL {}\r\n", c.class, env!("CARGO_PKG_VERSION"));
        if probe {
            manifest.push_str("Multi-Release: true\r\n");
        }
        manifest.push_str("\r\n");
        w.zip.start_file("META-INF/MANIFEST.MF", SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        w.zip.write_all(manifest.as_bytes()).map_err(|e| e.to_string())?;
        w.seen.insert("META-INF/MANIFEST.MF".into());
        for p in &parts {
            if p.is_dir() {
                w.add_dir(p)?;
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jar")) && p.is_file() {
                w.add_jar(p)?;
            }
        }
        let services = std::mem::take(&mut w.services);
        for (name, lines) in services {
            w.zip.start_file(name.as_str(), SimpleFileOptions::default()).map_err(|e| e.to_string())?;
            w.zip.write_all((lines.join("\n") + "\n").as_bytes()).map_err(|e| e.to_string())?;
        }
        w.zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&part, &target).map_err(|e| Failure::msg(e.to_string()))?;
            Ok(target)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(Failure::msg(crate::tr_format!("Die .jar konnte nicht geschrieben werden: {e}", "Could not write the .jar: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_and_module_info_are_left_out() {
        assert!(skip_entry("META-INF/MANIFEST.MF") && skip_entry("META-INF/BC.SF") && skip_entry("META-INF/BC.RSA") && skip_entry("module-info.class"));
        assert!(!skip_entry("META-INF/services/java.sql.Driver") && !skip_entry("org/mariadb/jdbc/Driver.class") && !skip_entry("META-INF/versions/11/x/Y.class"));
        assert_eq!(jar_name(Path::new("/p/Mein Projekt"), "app.Main"), "Mein-Projekt");
    }
}
