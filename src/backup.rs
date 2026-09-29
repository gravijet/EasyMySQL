// Automatische Sicherungen (Backups) aller Datenbanken und Wiederherstellung.
//
// Aufbau:  <Backup-Ordner>/<JJJJ-MM-TT_hh-mm-ss>_<anlass>/<datenbank>.sql.gz  + info.txt
// Erstellt mit mariadb-dump --single-transaction (konsistent, ohne die Arbeit zu blockieren).

use crate::db::{ConnInfo, SYSTEM_DATABASES, q};
use crate::server::{Paths, timestamp};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use mysql::prelude::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// Alles, was zum Sichern noetig ist (von der App gesetzt, sobald verbunden).
#[derive(Clone)]
pub struct Env {
    pub paths: Paths,
    pub conn: ConnInfo,
    pub dir: PathBuf,
    /// Bei Lesefehlern weitermachen (Server-Rettung)
    pub force: bool,
}

static ENV: OnceLock<Mutex<Option<Env>>> = OnceLock::new();

pub fn set_env(env: Option<Env>) {
    *ENV.get_or_init(|| Mutex::new(None)).lock().unwrap() = env;
}

pub fn env() -> Option<Env> {
    ENV.get_or_init(|| Mutex::new(None)).lock().unwrap().clone()
}

pub fn default_dir() -> PathBuf {
    crate::server::app_data_dir().join("backups")
}

/// Einstellungen fuer automatische Sicherungen.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub auto: bool,
    /// leer = Standardordner
    pub dir: String,
    pub interval_hours: u64,
    pub keep_count: usize,
    pub keep_days: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self { auto: true, dir: String::new(), interval_hours: 2, keep_count: 10, keep_days: 14 }
    }
}

fn config_path() -> PathBuf {
    crate::server::app_data_dir().join("sicherung.txt")
}

impl Config {
    pub fn load() -> Self {
        let mut c = Config::default();
        if let Ok(t) = std::fs::read_to_string(config_path()) {
            for line in t.lines() {
                let Some((k, v)) = line.split_once('=') else { continue };
                let v = v.trim();
                match k.trim() {
                    "auto" => c.auto = v == "1",
                    "dir" => c.dir = v.to_string(),
                    "interval_hours" => c.interval_hours = v.parse().unwrap_or(2).max(1),
                    "keep_count" => c.keep_count = v.parse().unwrap_or(10).max(1),
                    "keep_days" => c.keep_days = v.parse().unwrap_or(14),
                    _ => {}
                }
            }
        }
        c
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::server::app_data_dir());
        let _ = std::fs::write(
            config_path(),
            format!(
                "auto={}\ndir={}\ninterval_hours={}\nkeep_count={}\nkeep_days={}\n",
                if self.auto { 1 } else { 0 },
                self.dir,
                self.interval_hours,
                self.keep_count,
                self.keep_days
            ),
        );
    }

    pub fn dir(&self) -> PathBuf {
        if self.dir.trim().is_empty() { default_dir() } else { PathBuf::from(self.dir.trim()) }
    }
}

#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub name: String,
    pub path: PathBuf,
    pub time: String,
    pub reason: String,
    /// (Datenbank, Groesse in Bytes)
    pub dbs: Vec<(String, u64)>,
    pub modified: SystemTime,
}

impl BackupInfo {
    pub fn total(&self) -> u64 {
        self.dbs.iter().map(|d| d.1).sum()
    }
}

pub fn reason_text(r: &str) -> &str {
    match r {
        "auto" => "automatisch",
        "manuell" => "manuell",
        "vor-loeschen" => "vor dem Löschen",
        "vor-wiederherstellen" => "vor dem Wiederherstellen",
        "absturz" => "nach Absturz",
        "rettung" => "Server-Rettung",
        "vor-reparatur" => "vor der Reparatur",
        other => other,
    }
}

pub fn human_size(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.1} MB", b as f64 / 1024.0 / 1024.0)
    }
}

/// Alle Sicherungen, neueste zuerst.
pub fn list(dir: &Path) -> Vec<BackupInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        // Name: JJJJ-MM-TT_hh-mm-ss_anlass
        if name.len() < 19 || !name.as_bytes()[0].is_ascii_digit() {
            continue;
        }
        let (ts, reason) = (&name[..19], name.get(20..).unwrap_or(""));
        let time = format!(
            "{}.{}.{} {}:{}:{}",
            &ts[8..10],
            &ts[5..7],
            &ts[0..4],
            &ts[11..13],
            &ts[14..16],
            &ts[17..19]
        );
        let mut dbs: Vec<(String, u64)> = std::fs::read_dir(&path)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|f| {
                        let n = f.file_name().to_string_lossy().into_owned();
                        let db = n.strip_suffix(".sql.gz").or_else(|| n.strip_suffix(".sql"))?;
                        Some((db.to_string(), f.metadata().map(|m| m.len()).unwrap_or(0)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        dbs.sort();
        let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        out.push(BackupInfo { name: name.clone(), path, time, reason: reason.to_string(), dbs, modified });
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Ist eine automatische Sicherung faellig?
pub fn due(dir: &Path, interval: Duration) -> bool {
    match list(dir).into_iter().find(|b| b.reason == "auto" || b.reason == "manuell") {
        Some(b) => SystemTime::now().duration_since(b.modified).map(|d| d >= interval).unwrap_or(true),
        None => true,
    }
}

fn client_cmd(env: &Env, tool: &str) -> Command {
    let exe = crate::server::find_tool(&env.paths.bin, tool);
    let mut cmd = Command::new(exe);
    cmd.arg(format!("--host={}", env.conn.host))
        .arg("--protocol=tcp")
        .arg(format!("--port={}", env.conn.port))
        .arg(format!("--user={}", env.conn.user))
        .arg("--default-character-set=utf8mb4");
    if !env.conn.password.is_empty() {
        cmd.env("MYSQL_PWD", &env.conn.password);
    }
    crate::server::hidden(&mut cmd);
    cmd
}

/// Benutzer-Datenbanken des Servers.
pub fn user_databases(conn: &ConnInfo) -> Result<Vec<String>, String> {
    let mut c = mysql::Conn::new(conn.opts()).map_err(|e| e.to_string())?;
    let dbs: Vec<String> = c.query("SHOW DATABASES").map_err(|e| e.to_string())?;
    Ok(dbs.into_iter().filter(|d| !SYSTEM_DATABASES.contains(&d.as_str())).collect())
}

/// Eine Datenbank in eine .sql.gz-Datei sichern.
pub fn dump_db(env: &Env, db: &str, file: &Path) -> Result<u64, String> {
    let mut cmd = client_cmd(env, "mariadb-dump");
    if env.force {
        cmd.arg("--force");
    }
    let mut child = cmd
        .args([
            "--single-transaction",
            "--routines",
            "--triggers",
            "--events",
            "--hex-blob",
            "--add-drop-table",
            "--skip-dump-date",
        ])
        .arg(db)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("mariadb-dump konnte nicht gestartet werden: {e}"))?;
    let mut stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let tmp = file.with_extension("gz.tmp");
    let result = (|| -> Result<u64, String> {
        let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut gz = GzEncoder::new(std::io::BufWriter::new(f), Compression::default());
        let n = std::io::copy(child.stdout.as_mut().unwrap(), &mut gz).map_err(|e| e.to_string())?;
        let mut w = gz.finish().map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
        w.get_ref().sync_all().map_err(|e| e.to_string())?;
        Ok(n)
    })();
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = err_thread.join().unwrap_or_default();
    match result {
        Ok(n) if status.success() => {
            std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
            Ok(n)
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("mariadb-dump {db}: {}", err.trim()))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(format!("Sicherung {db}: {e}"))
        }
    }
}

/// Neue Sicherung anlegen. `dbs` = None -> alle Benutzer-Datenbanken.
pub fn create(env: &Env, dbs: Option<Vec<String>>, reason: &str, log: &dyn Fn(String)) -> Result<PathBuf, String> {
    let dbs = match dbs {
        Some(d) => d,
        None => user_databases(&env.conn)?,
    };
    let dir = env.dir.join(format!("{}_{reason}", timestamp()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut errors = Vec::new();
    for db in &dbs {
        log(format!("Sichere {db} ..."));
        if let Err(e) = dump_db(env, db, &dir.join(format!("{db}.sql.gz"))) {
            log(format!("FEHLER: {e}"));
            errors.push(e);
        }
    }
    let _ = std::fs::write(
        dir.join("info.txt"),
        format!(
            "EasyMySQL-Sicherung\r\nZeitpunkt: {}\r\nAnlass: {}\r\nDatenbanken: {}\r\n",
            timestamp(),
            reason_text(reason),
            dbs.join(", ")
        ),
    );
    if dbs.is_empty() {
        log("Keine Benutzer-Datenbanken vorhanden.".into());
    }
    if !errors.is_empty() && errors.len() == dbs.len() {
        return Err(errors.join("\n"));
    }
    Ok(dir)
}

/// Alte Sicherungen aufraeumen: die neuesten `keep_count` bleiben,
/// ausserdem die jeweils erste Sicherung jedes Tages der letzten `keep_days` Tage.
pub fn rotate(dir: &Path, keep_count: usize, keep_days: u64) -> usize {
    let all = list(dir);
    let mut seen_days = std::collections::HashSet::new();
    let mut removed = 0;
    let now = SystemTime::now();
    for (i, b) in all.iter().enumerate() {
        let day = b.name[..10].to_string();
        let age = now.duration_since(b.modified).unwrap_or_default();
        let young = age <= Duration::from_secs(keep_days * 86400);
        // Liste ist neu->alt; pro Tag die aelteste behalten waere auch moeglich,
        // wir behalten die neueste jedes Tages.
        let first_of_day = seen_days.insert(day);
        if i < keep_count || (young && first_of_day) {
            continue;
        }
        if std::fs::remove_dir_all(&b.path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Eine Datenbank aus einer Sicherung in `target` einspielen (wird vorher geleert).
pub fn restore(env: &Env, file: &Path, target: &str, log: &dyn Fn(String)) -> Result<(), String> {
    let mut c = mysql::Conn::new(env.conn.opts()).map_err(|e| e.to_string())?;
    let exists: Option<String> = c
        .exec_first("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?", (target,))
        .map_err(|e| e.to_string())?;
    if exists.is_some() {
        log(format!("Sichere die aktuelle Datenbank {target}, bevor sie ersetzt wird ..."));
        create(env, Some(vec![target.to_string()]), "vor-wiederherstellen", log)?;
        c.query_drop(format!("DROP DATABASE {}", q(target))).map_err(|e| e.to_string())?;
    }
    c.query_drop(format!("CREATE DATABASE {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", q(target)))
        .map_err(|e| e.to_string())?;
    drop(c);
    log(format!("Spiele {} in {target} ein ...", file.display()));
    import_file(env, file, Some(target))
}

/// SQL-Datei (.sql oder .sql.gz) mit dem mariadb-Client einspielen.
pub fn import_file(env: &Env, file: &Path, db: Option<&str>) -> Result<(), String> {
    let mut cmd = client_cmd(env, "mariadb");
    if let Some(d) = db {
        cmd.arg(d);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("mariadb konnte nicht gestartet werden: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let gz = file.extension().is_some_and(|x| x == "gz");
    let writer = std::thread::spawn(move || -> std::io::Result<u64> {
        let mut reader: Box<dyn Read> = if gz { Box::new(GzDecoder::new(f)) } else { Box::new(f) };
        let n = std::io::copy(&mut reader, &mut stdin)?;
        drop(stdin);
        Ok(n)
    });
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("Einspielen fehlgeschlagen: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Soll vor diesem SQL eine Sicherung gemacht werden? Liefert die betroffenen Datenbanken.
/// `current` = aktuell gewaehlte Datenbank.
pub fn destructive_targets(sql: &str, current: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let upper = sql.to_uppercase();
    let words: Vec<&str> = sql.split(|c: char| c.is_whitespace() || c == ';' || c == '(').filter(|w| !w.is_empty()).collect();
    let uw: Vec<String> = words.iter().map(|w| w.to_uppercase()).collect();
    let clean = |w: &str| w.trim_matches('`').to_string();
    for i in 0..uw.len() {
        // DROP DATABASE x / DROP SCHEMA x
        if uw[i] == "DROP" && i + 2 < uw.len() && (uw[i + 1] == "DATABASE" || uw[i + 1] == "SCHEMA") {
            let mut j = i + 2;
            if uw[j] == "IF" {
                j += 2;
            }
            if let Some(w) = words.get(j) {
                out.push(clean(w));
            }
        }
        // db.tabelle in DROP TABLE / TRUNCATE / DELETE / ALTER
        if matches!(uw[i].as_str(), "TABLE" | "TRUNCATE" | "FROM" | "INTO") && i + 1 < words.len() {
            let w = words[i + 1];
            if let Some((d, _)) = w.split_once('.') {
                let d = clean(d);
                if !d.is_empty() {
                    out.push(d);
                }
            }
        }
    }
    // UPDATE/DELETE nur ohne WHERE (dann ist die ganze Tabelle betroffen)
    let no_where = !upper.contains("WHERE");
    let risky = ["DROP TABLE", "DROP VIEW", "TRUNCATE"].iter().any(|k| upper.contains(k))
        || (upper.contains("ALTER TABLE") && upper.contains("DROP"))
        || (no_where && (upper.contains("DELETE ") || upper.contains("UPDATE ")));
    if risky {
        if let Some(c) = current.filter(|c| !c.is_empty()) {
            out.push(c.to_string());
        }
    }
    out.retain(|d| !SYSTEM_DATABASES.contains(&d.to_lowercase().as_str()));
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(destructive_targets("DROP DATABASE `shop`;", None), vec!["shop"]);
        assert_eq!(destructive_targets("drop database if exists alt", None), vec!["alt"]);
        assert_eq!(destructive_targets("DROP TABLE kunde", Some("shop")), vec!["shop"]);
        assert_eq!(destructive_targets("TRUNCATE schule.note", None), vec!["schule"]);
        assert!(destructive_targets("SELECT * FROM kunde", Some("shop")).is_empty());
        assert!(destructive_targets("CREATE TABLE x (id INT)", Some("shop")).is_empty());
        assert!(destructive_targets("DELETE FROM kunde WHERE id=1", Some("shop")).is_empty());
        assert_eq!(destructive_targets("DELETE FROM kunde", Some("shop")), vec!["shop"]);
        assert_eq!(destructive_targets("ALTER TABLE kunde DROP COLUMN email", Some("shop")), vec!["shop"]);
        assert!(destructive_targets("DROP DATABASE mysql", None).is_empty());
    }

    #[test]
    fn rotation() {
        let dir = std::env::temp_dir().join(format!("easymysql-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for i in 0..15 {
            std::fs::create_dir_all(dir.join(format!("2026-09-{:02}_10-00-00_auto", 10 + i))).unwrap();
            std::fs::create_dir_all(dir.join(format!("2026-09-{:02}_12-00-00_auto", 10 + i))).unwrap();
        }
        // alle "jung" (gerade angelegt): 10 neueste + 1 pro Tag
        rotate(&dir, 10, 14);
        let left = list(&dir);
        // 15 Tage: pro Tag die neueste (15) + die 10 neuesten (davon 5 zusaetzlich) = 20
        assert_eq!(left.len(), 20);
        rotate(&dir, 3, 0);
        assert_eq!(list(&dir).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
