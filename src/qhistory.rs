// Verlauf aller ausgefuehrten Anweisungen (Seitenleiste "Verlauf").
// Datei: <Speicherordner>/.easymysql/abfrageverlauf.txt, eine Zeile je Anweisung.

use std::io::Write;
use std::path::PathBuf;

const MAX: usize = 3000;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// "JJJJ-MM-TT_hh-mm-ss"
    pub time: String,
    pub db: String,
    pub millis: u64,
    pub error: bool,
    pub sql: String,
}

impl Entry {
    /// Anzeigezeit "TT.MM. hh:mm"
    pub fn short_time(&self) -> String {
        let t = &self.time;
        if t.len() < 16 {
            return t.clone();
        }
        format!("{}.{}. {}:{}", &t[8..10], &t[5..7], &t[11..13], &t[14..16])
    }

    fn to_line(&self) -> String {
        let sql = self.sql.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "").replace('\t', "\\t");
        format!("{}\t{}\t{}\t{}\t{}", self.time, self.db, self.millis, if self.error { 1 } else { 0 }, sql)
    }

    fn from_line(l: &str) -> Option<Entry> {
        let mut it = l.splitn(5, '\t');
        let time = it.next()?.to_string();
        let db = it.next()?.to_string();
        let millis = it.next()?.parse().ok()?;
        let error = it.next()? == "1";
        let raw = it.next()?;
        let mut sql = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => sql.push('\n'),
                    Some('t') => sql.push('\t'),
                    Some(o) => sql.push(o),
                    None => {}
                }
            } else {
                sql.push(c);
            }
        }
        Some(Entry { time, db, millis, error, sql })
    }
}

fn file() -> PathBuf {
    crate::workspace::internal_dir().join("abfrageverlauf.txt")
}

/// Eintraege, neueste zuerst.
pub fn load() -> Vec<Entry> {
    let text = std::fs::read_to_string(file()).unwrap_or_default();
    let mut v: Vec<Entry> = text.lines().filter_map(Entry::from_line).collect();
    v.reverse();
    v.truncate(MAX);
    v
}

/// Eintraege anhaengen; die Datei wird gelegentlich auf die letzten Eintraege gekuerzt.
pub fn append(entries: &[Entry]) {
    if entries.is_empty() {
        return;
    }
    let path = file();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        for e in entries {
            let _ = writeln!(f, "{}", e.to_line());
        }
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 4_000_000) {
        let mut all = load();
        all.truncate(MAX);
        all.reverse();
        let text: String = all.iter().map(|e| e.to_line() + "\n").collect();
        let _ = crate::workspace::atomic_write(&path, &text);
    }
}

pub fn clear() {
    let _ = std::fs::remove_file(file());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_roundtrip() {
        let e = Entry { time: "2026-09-30_10-11-12".into(), db: "shop".into(), millis: 12, error: true, sql: "SELECT 'a\\b'\n\tFROM t".into() };
        assert_eq!(Entry::from_line(&e.to_line()), Some(e.clone()));
        assert_eq!(e.short_time(), "30.09. 10:11");
    }
}
