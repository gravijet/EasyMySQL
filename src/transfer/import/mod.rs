// Import aus SQLite-Dateien, MongoDB-Exporten (JSON, NDJSON, BSON), CSV-Dateien und Redis.
// Jede Quelle liefert Tabellen; Spaltentypen werden aus den Werten bestimmt (`Infer`).
// Gelesen wird zweimal: erst zum Bestimmen der Typen, dann zum Einfuegen. So bleibt der Speicher klein.

mod csv;
mod docs;
mod redis;
mod sqlite;
pub mod write;

pub use redis::RedisConf;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Source {
    Sqlite(PathBuf),
    /// MongoDB-Exporte: `.json` (Array, NDJSON oder Objekt aus Arrays), `.bson` (mongodump)
    Docs(Vec<PathBuf>),
    Csv(Vec<PathBuf>),
    Redis(RedisConf),
}

/// Ein Wert aus einer Quelle
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    /// Verschachteltes Objekt oder Array als JSON-Text
    Json(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColType {
    Bool,
    Int,
    BigInt,
    Double,
    Date,
    DateTime,
    Varchar(u32),
    Text,
    LongText,
    Json,
    Blob,
}

impl ColType {
    pub fn sql(self) -> String {
        match self {
            ColType::Bool => "TINYINT(1)".into(),
            ColType::Int => "INT".into(),
            ColType::BigInt => "BIGINT".into(),
            ColType::Double => "DOUBLE".into(),
            ColType::Date => "DATE".into(),
            ColType::DateTime => "DATETIME(6)".into(),
            ColType::Varchar(n) => format!("VARCHAR({n})"),
            ColType::Text => "TEXT".into(),
            ColType::LongText => "LONGTEXT".into(),
            ColType::Json => "JSON".into(),
            ColType::Blob => "LONGBLOB".into(),
        }
    }

    /// Als Primaerschluessel oder in einem Index brauchbar
    pub fn indexable(self) -> bool {
        !matches!(self, ColType::Text | ColType::LongText | ColType::Json | ColType::Blob)
    }
}

#[derive(Clone, Debug)]
pub struct Col {
    pub name: String,
    pub ty: ColType,
}

#[derive(Clone, Debug)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct TableScan {
    pub name: String,
    /// Wo die Zeilen herkommen (je Quelle eigene Bedeutung)
    pub origin: String,
    pub columns: Vec<Col>,
    pub rows: u64,
    pub pk: Vec<String>,
    pub auto_inc: bool,
    pub fks: Vec<ForeignKey>,
}

/// Sammelt Eigenschaften einer Spalte und bestimmt daraus den Typ.
#[derive(Clone, Debug)]
pub struct Infer {
    seen: u64,
    ints: u64,
    bools: u64,
    reals: u64,
    texts: u64,
    jsons: u64,
    bytes: u64,
    dates: u64,
    datetimes: u64,
    big: bool,
    max_len: usize,
}

impl Default for Infer {
    fn default() -> Self {
        Infer { seen: 0, ints: 0, bools: 0, reals: 0, texts: 0, jsons: 0, bytes: 0, dates: 0, datetimes: 0, big: false, max_len: 0 }
    }
}

/// "2024-05-17" -> Date, "2024-05-17 12:30:00(.123)" oder mit T und Z -> DateTime
pub fn date_kind(s: &str) -> Option<ColType> {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    if b.len() < 10 || !(digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10)) {
        return None;
    }
    let (mo, d) = (s[5..7].parse::<u32>().ok()?, s[8..10].parse::<u32>().ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    if b.len() == 10 {
        return Some(ColType::Date);
    }
    if !(b[10] == b' ' || b[10] == b'T') || b.len() < 19 || !(digits(11..13) && b[13] == b':' && digits(14..16) && b[16] == b':' && digits(17..19)) {
        return None;
    }
    let rest = &s[19..];
    let rest = rest.strip_suffix('Z').unwrap_or(rest);
    let ok = rest.is_empty() || (rest.starts_with('.') && rest.len() > 1 && rest[1..].bytes().all(|c| c.is_ascii_digit()));
    ok.then_some(ColType::DateTime)
}

/// Datum fuer MariaDB: "T" -> Leerzeichen, "Z" weg, Sekundenbruchteile auf 6 Stellen
pub fn normalize_date(s: &str) -> String {
    let mut t = s.replacen('T', " ", 1);
    if t.ends_with('Z') {
        t.pop();
    }
    if let Some(dot) = t.find('.') {
        t.truncate((dot + 7).min(t.len()));
    }
    t
}

impl Infer {
    pub fn add(&mut self, v: &Val) {
        match v {
            Val::Null => return,
            Val::Bool(_) => self.bools += 1,
            Val::Int(i) => {
                self.ints += 1;
                if i32::try_from(*i).is_err() {
                    self.big = true;
                }
            }
            Val::Real(_) => self.reals += 1,
            Val::Text(s) => {
                match date_kind(s) {
                    Some(ColType::Date) => self.dates += 1,
                    Some(_) => self.datetimes += 1,
                    None => self.texts += 1,
                }
                self.max_len = self.max_len.max(s.chars().count());
            }
            Val::Json(s) => {
                self.jsons += 1;
                self.max_len = self.max_len.max(s.chars().count());
            }
            Val::Bytes(_) => self.bytes += 1,
        }
        self.seen += 1;
    }

    pub fn finish(&self) -> ColType {
        let n = self.seen;
        let text_len = |max: usize| match max {
            0..=255 => ColType::Varchar(255),
            256..=65_000 => ColType::Text,
            _ => ColType::LongText,
        };
        if n == 0 {
            ColType::Varchar(255)
        } else if self.bytes == n {
            ColType::Blob
        } else if self.jsons == n {
            ColType::Json
        } else if self.bools == n {
            ColType::Bool
        } else if self.ints + self.bools == n {
            if self.big { ColType::BigInt } else { ColType::Int }
        } else if self.ints + self.reals + self.bools == n {
            ColType::Double
        } else if self.dates == n {
            ColType::Date
        } else if self.dates + self.datetimes == n {
            ColType::DateTime
        } else {
            text_len(self.max_len)
        }
    }

    pub fn max_len(&self) -> usize {
        self.max_len
    }
}

/// Typ aus den gesammelten Werten; kurze Texte bekommen eine Laenge, die zum laengsten Wert passt.
pub fn column_type(inf: &Infer) -> ColType {
    match inf.finish() {
        ColType::Varchar(_) if inf.seen == 0 => ColType::Varchar(255),
        ColType::Varchar(_) => ColType::Varchar(((inf.max_len().max(1) as u32).div_ceil(64) * 64).clamp(64, 255)),
        t => t,
    }
}

/// Tabellen der Quelle mit Spalten, Typen und Zeilenzahl
pub fn scan(src: &Source, cancel: &std::sync::atomic::AtomicBool) -> Result<Vec<TableScan>, String> {
    match src {
        Source::Sqlite(p) => sqlite::scan(p, cancel),
        Source::Docs(files) => docs::scan(files, cancel),
        Source::Csv(files) => csv::scan(files, cancel),
        Source::Redis(c) => redis::scan(c, cancel),
    }
}

/// Zeilen einer Tabelle (Spalten wie in `TableScan`) nacheinander an `f` geben; `f` liefert false zum Abbrechen
pub fn read(src: &Source, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    match src {
        Source::Sqlite(p) => sqlite::read(p, t, f),
        Source::Docs(_) => docs::read(t, f),
        Source::Csv(_) => csv::read(t, f),
        Source::Redis(c) => redis::read(c, t, f),
    }
}

/// Eindeutiger, gueltiger Spaltenname (leer, doppelt)
pub fn unique_names(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.into_iter()
        .enumerate()
        .map(|(i, n)| {
            let base = if n.trim().is_empty() { format!("spalte{}", i + 1) } else { n.trim().chars().take(64).collect() };
            let mut name = base.clone();
            let mut k = 2;
            while !seen.insert(name.to_lowercase()) {
                name = format!("{base}_{k}");
                k += 1;
            }
            name
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infer(vals: &[Val]) -> ColType {
        let mut i = Infer::default();
        vals.iter().for_each(|v| i.add(v));
        column_type(&i)
    }

    #[test]
    fn inference() {
        assert_eq!(infer(&[Val::Int(1), Val::Null, Val::Int(2)]), ColType::Int);
        assert_eq!(infer(&[Val::Int(1), Val::Int(5_000_000_000)]), ColType::BigInt);
        assert_eq!(infer(&[Val::Int(1), Val::Real(2.5)]), ColType::Double);
        assert_eq!(infer(&[Val::Bool(true), Val::Bool(false)]), ColType::Bool);
        assert_eq!(infer(&[Val::Text("2024-01-02".into())]), ColType::Date);
        assert_eq!(infer(&[Val::Text("2024-01-02".into()), Val::Text("2024-01-02T10:20:30.5Z".into())]), ColType::DateTime);
        assert_eq!(infer(&[Val::Text("2024-13-02".into())]), ColType::Varchar(64));
        assert_eq!(infer(&[Val::Text("a".into()), Val::Int(3)]), ColType::Varchar(64));
        assert_eq!(infer(&[Val::Json("{}".into()), Val::Json("[1]".into())]), ColType::Json);
        assert_eq!(infer(&[Val::Text("x".repeat(300))]), ColType::Text);
        assert_eq!(infer(&[Val::Null]), ColType::Varchar(255));
        assert_eq!(normalize_date("2024-01-02T10:20:30.123456789Z"), "2024-01-02 10:20:30.123456");
        assert_eq!(unique_names(vec!["a".into(), "A".into(), "".into()]), vec!["a", "A_2", "spalte3"]);
    }
}
