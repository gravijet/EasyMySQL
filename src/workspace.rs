// Projekte, Dateien, Dateiverlauf und Sitzung (wie in Visual Studio Code).

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Speicherorte

static ROOT: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// Standard-Speicherordner: Dokumente\EasyMySQL
pub fn default_root() -> PathBuf {
    crate::vscode::workspace_dir()
}

/// Speicherordner aus den Einstellungen setzen (leer = Standard).
pub fn set_storage_root(dir: &str) {
    let p = if dir.trim().is_empty() { None } else { Some(PathBuf::from(dir.trim())) };
    *ROOT.write().unwrap() = p;
}

/// Ordner, in dem EasyMySQL alles speichert: neue Projekte, unbenannte Abfragen, Verlauf, ...
pub fn storage_root() -> PathBuf {
    ROOT.read().unwrap().clone().unwrap_or_else(default_root)
}

/// Interne Dateien (unbenannte Abfragen, fruehere Fassungen, Diagramm-Layouts, Assistent)
pub fn internal_dir() -> PathBuf {
    storage_root().join(".easymysql")
}

/// Aus Version 2 (Programmdatenordner, Dokumente\EasyMySQL\Projekte) uebernehmen.
/// Liefert die gefundenen alten Projektordner.
pub fn migrate_legacy() -> Vec<PathBuf> {
    let old = crate::server::app_data_dir();
    let new = internal_dir();
    for (from, to) in [("ungespeichert", "ungespeichert"), ("verlauf", "fassungen"), ("layouts", "layouts")] {
        let src = old.join(from);
        let dst = new.join(to);
        if src.is_dir() && !dst.exists() {
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::rename(&src, &dst).is_err() && crate::server::copy_dir(&src, &dst).is_ok() {
                let _ = std::fs::remove_dir_all(&src);
            }
        }
    }
    let legacy = default_root().join("Projekte");
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&legacy) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            // Unveraenderte Beispieldatei der Version 2 entfernen
            let w = p.join("Willkommen.sql");
            if std::fs::read_to_string(&w).is_ok_and(|t| t == OLD_WELCOME) {
                let _ = std::fs::remove_file(&w);
            }
            // Nur leere Standardprojekte entfernen (hoechstens EasyMySQL-/VS-Code-Einstellungen darin)
            let empty = std::fs::read_dir(&p)
                .map(|mut r| r.all(|x| x.is_ok_and(|x| matches!(x.file_name().to_str(), Some(".easymysql" | ".vscode")))))
                .unwrap_or(false);
            if empty && e.file_name() == "Meine Abfragen" {
                let _ = std::fs::remove_dir_all(&p);
                continue;
            }
            found.push(p);
        }
    }
    let _ = std::fs::remove_dir(&legacy);
    found
}

/// Beispieldatei der Version 2 (wird entfernt, wenn unveraendert)
const OLD_WELCOME: &str = "-- Willkommen bei EasyMySQL!
-- Diese Datei liegt im Projekt \"Meine Abfragen\" (links im Explorer).
-- Alles wird automatisch gespeichert.
--
-- Tastenkürzel:
--   Strg+Alt+S    ganze Datei ausführen (oder nur den markierten Teil)
--   Strg+Enter    nur die Anweisung unter dem Cursor ausführen
--   Strg+Alt+L    SQL schön formatieren
--   Strg+Leertaste Vorschläge (Tabellen, Spalten, Befehle)
--   Strg+F / Strg+H  Suchen / Ersetzen,  Strg+G  Gehe zu Zeile
--   Strg+#        Zeile(n) aus-/einkommentieren
--   Alt+↑/↓       Zeile verschieben,  Umschalt+Alt+↓  Zeile duplizieren

CREATE DATABASE IF NOT EXISTS beispiel;
USE beispiel;

CREATE TABLE IF NOT EXISTS klasse (
  id INT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(20) NOT NULL
);

CREATE TABLE IF NOT EXISTS schueler (
  id INT AUTO_INCREMENT PRIMARY KEY,
  vorname VARCHAR(50) NOT NULL,
  klasse_id INT,
  FOREIGN KEY (klasse_id) REFERENCES klasse(id)
);

SELECT * FROM schueler;
";

/// Inhalt des Speicherordners in einen anderen Ordner verschieben (Projekte, interne Dateien).
/// Liefert die Anzahl der verschobenen Eintraege.
pub fn move_storage(from: &Path, to: &Path) -> Result<usize, String> {
    if from == to {
        return Ok(0);
    }
    if to.starts_with(from) || from.starts_with(to) {
        return Err("Der neue Ordner darf nicht im alten liegen (und umgekehrt).".into());
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(from) else { return Ok(0) };
    for e in rd.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        if dst.exists() {
            return Err(format!("„{}“ gibt es im neuen Ordner schon.", e.file_name().to_string_lossy()));
        }
        if std::fs::rename(&src, &dst).is_err() {
            // anderes Laufwerk: kopieren, dann loeschen
            let r = if src.is_dir() { crate::server::copy_dir(&src, &dst) } else { std::fs::copy(&src, &dst).map(|_| ()) };
            r.map_err(|err| format!("{}: {err}", src.display()))?;
            let _ = if src.is_dir() { std::fs::remove_dir_all(&src) } else { std::fs::remove_file(&src) };
        }
        n += 1;
    }
    Ok(n)
}

/// Name eines Ordners fuer die Anzeige
pub fn dir_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && !n.starts_with('.') && !n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

// ---------------------------------------------------------------------------
// Dateibaum

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

pub fn scan(dir: &Path) -> Node {
    fn walk(dir: &Path, depth: usize) -> Vec<Node> {
        let mut out: Vec<Node> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if name.starts_with('.') || name.ends_with(".tmp") {
                            return None;
                        }
                        let path = e.path();
                        let is_dir = path.is_dir();
                        let children = if is_dir && depth < 12 { walk(&path, depth + 1) } else { Vec::new() };
                        Some(Node { name, path, is_dir, children })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        out
    }
    Node {
        name: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: dir.to_path_buf(),
        is_dir: true,
        children: walk(dir, 0),
    }
}

/// Neue, noch nicht vorhandene Datei "Neue Abfrage.sql", "Neue Abfrage 2.sql", ...
pub fn unique_file(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{base}.{ext}"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{base} {i}.{ext}"));
        i += 1;
    }
    p
}

pub fn create_file(dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("Ungültiger Dateiname.".into());
    }
    let mut n = name.trim().to_string();
    if !n.contains('.') {
        n.push_str(".sql");
    }
    let p = dir.join(n);
    if p.exists() {
        return Err(format!("{} existiert bereits.", p.display()));
    }
    atomic_write(&p, content).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("Ungültiger Ordnername.".into());
    }
    let p = dir.join(name.trim());
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    if !valid_name(new_name) {
        return Err("Ungültiger Name.".into());
    }
    let target = path.with_file_name(new_name.trim());
    if target.exists() {
        return Err(format!("{} existiert bereits.", target.display()));
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// "Loeschen" = in den Papierkorb des Projekts verschieben (wiederherstellbar).
pub fn trash(project: &Path, path: &Path) -> Result<PathBuf, String> {
    let bin = project.join(".papierkorb");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let target = bin.join(format!("{}_{}", crate::server::timestamp(), name));
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Sicheres Speichern: erst Zwischendatei schreiben und auf die Platte bringen, dann umbenennen.
/// So bleibt bei einem Absturz immer entweder die alte oder die neue Fassung vollstaendig erhalten.
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Dateiverlauf (frühere Versionen)

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn history_dir(path: &Path) -> PathBuf {
    let key = path.to_string_lossy().to_lowercase();
    internal_dir().join("fassungen").join(format!("{:016x}", fnv(&key)))
}

/// Alte Fassung sichern (hoechstens alle 5 Minuten eine, max. 50 je Datei).
pub fn snapshot(path: &Path, old_content: &str) {
    if old_content.trim().is_empty() {
        return;
    }
    let dir = history_dir(path);
    let mut list = history(path);
    if let Some((_, newest)) = list.first() {
        let age = newest.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if age.is_some_and(|a| a.as_secs() < 300) {
            return;
        }
        if std::fs::read_to_string(newest).is_ok_and(|c| c == old_content) {
            return;
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("pfad.txt"), path.to_string_lossy().as_bytes());
    let _ = atomic_write(&dir.join(format!("{}.sql", crate::server::timestamp())), old_content);
    list = history(path);
    for (_, p) in list.iter().skip(50) {
        let _ = std::fs::remove_file(p);
    }
}

/// Fruehere Fassungen, neueste zuerst: (Anzeige-Zeit, Datei)
pub fn history(path: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(history_dir(path))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "sql"))
                .filter_map(|p| {
                    let n = p.file_stem()?.to_string_lossy().into_owned();
                    if n.len() < 19 {
                        return None;
                    }
                    let t = format!("{}.{}.{} {}:{}:{}", &n[8..10], &n[5..7], &n[0..4], &n[11..13], &n[14..16], &n[17..19]);
                    Some((t, p))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

// ---------------------------------------------------------------------------
// Datenbank je Datei

fn db_map_file(project: &Path) -> PathBuf {
    project.join(".easymysql").join("datenbanken.txt")
}

fn rel(project: &Path, file: &Path) -> String {
    file.strip_prefix(project).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

pub fn file_db(project: &Path, file: &Path) -> Option<String> {
    let key = rel(project, file);
    std::fs::read_to_string(db_map_file(project))
        .ok()?
        .lines()
        .find_map(|l| l.split_once('\t').filter(|(k, _)| *k == key).map(|(_, v)| v.to_string()))
        .filter(|v| !v.is_empty())
}

pub fn set_file_db(project: &Path, file: &Path, db: &str) {
    if !file.starts_with(project) {
        return;
    }
    let key = rel(project, file);
    let path = db_map_file(project);
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with(&format!("{key}\t")))
        .map(|l| l.to_string())
        .collect();
    if file_db(project, file).as_deref() == Some(db) {
        return;
    }
    lines.push(format!("{key}\t{db}"));
    let _ = atomic_write(&path, &(lines.join("\n") + "\n"));
}

// ---------------------------------------------------------------------------
// Ungespeicherte Abfragen ("Unbenannt"), bleiben ueber Neustarts erhalten

pub fn scratch_dir() -> PathBuf {
    internal_dir().join("ungespeichert")
}

pub fn scratch_path(id: usize) -> PathBuf {
    scratch_dir().join(format!("unbenannt-{id}.sql"))
}

// ---------------------------------------------------------------------------
// Sitzung: offene Registerkarten, Projekt, Seitenleiste

#[derive(Clone, Debug, Default)]
pub struct Session {
    pub project: String,
    pub tabs: Vec<String>,
    pub active: usize,
    pub side_view: String,
    pub sidebar: bool,
}

fn session_file() -> PathBuf {
    crate::server::app_data_dir().join("sitzung.txt")
}

impl Session {
    pub fn load() -> Option<Session> {
        let text = std::fs::read_to_string(session_file()).ok()?;
        let mut s = Session { sidebar: true, ..Default::default() };
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("projekt=") {
                s.project = v.to_string();
            } else if let Some(v) = line.strip_prefix("aktiv=") {
                s.active = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("ansicht=") {
                s.side_view = v.to_string();
            } else if let Some(v) = line.strip_prefix("seitenleiste=") {
                s.sidebar = v == "1";
            } else if let Some(v) = line.strip_prefix("tab=") {
                s.tabs.push(v.to_string());
            }
        }
        Some(s)
    }

    pub fn save(&self) {
        let mut t = format!(
            "projekt={}\naktiv={}\nansicht={}\nseitenleiste={}\n",
            self.project,
            self.active,
            self.side_view,
            if self.sidebar { 1 } else { 0 }
        );
        for tab in &self.tabs {
            t.push_str(&format!("tab={tab}\n"));
        }
        let _ = atomic_write(&session_file(), &t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_history() {
        let dir = std::env::temp_dir().join(format!("easymysql-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = create_file(&dir, "test", "SELECT 1;").unwrap();
        assert!(f.ends_with("test.sql"));
        assert!(create_file(&dir, "test", "").is_err());
        assert!(create_file(&dir, "a/b", "").is_err());
        let u = unique_file(&dir, "test", "sql");
        assert!(u.ends_with("test 2.sql"));
        atomic_write(&f, "SELECT 2;").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "SELECT 2;");
        let sub = create_folder(&dir, "Ordner").unwrap();
        create_file(&sub, "x.sql", "").unwrap();
        let tree = scan(&dir);
        assert!(tree.children[0].is_dir, "Ordner zuerst");
        let r = rename(&f, "neu.sql").unwrap();
        assert!(r.exists() && !f.exists());
        let t = trash(&dir, &r).unwrap();
        assert!(t.exists() && !r.exists());
        assert!(!scan(&dir).children.iter().any(|n| n.name == ".papierkorb"));
        set_file_db(&dir, &sub.join("x.sql"), "shop");
        assert_eq!(file_db(&dir, &sub.join("x.sql")).as_deref(), Some("shop"));
        set_file_db(&dir, &sub.join("x.sql"), "schule");
        assert_eq!(file_db(&dir, &sub.join("x.sql")).as_deref(), Some("schule"));
        let target = std::env::temp_dir().join(format!("easymysql-ws2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&target);
        assert!(move_storage(&dir, &dir.join("x")).is_err());
        assert!(move_storage(&dir, &target).unwrap() >= 2);
        assert!(target.join("Ordner").join("x.sql").exists());
        assert!(!sub.exists());
        let _ = std::fs::remove_dir_all(&target);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
