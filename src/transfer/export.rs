// Export einer Datenbank oder einzelner Tabellen in SQL, JSON, CSV, XML oder eine SQLite-Datei.
// Die Zeilen werden gelesen und sofort geschrieben, auch riesige Tabellen belegen kaum Speicher.

use super::export_targets as targets;
use super::import::RedisConf;
use super::import::write::Progress;
use super::server::Link;
use crate::db::{Db, TableInfo, err_text, q};
use mysql::consts::ColumnType as T;
use mysql::prelude::*;
use mysql::{Column, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Sql,
    Json,
    Csv,
    Xml,
    Sqlite,
    /// SQL-Skript fuer PostgreSQL
    PgSql,
    /// JSON-Zeilen fuer MongoDB (mongoimport)
    Mongo,
    /// Redis-Datei (dump.rdb)
    RedisRdb,
    /// Redis-Befehle (HSET ...), ladbar mit redis-cli
    RedisCmds,
}

/// Anderer Server als Ziel
#[derive(Clone, Debug)]
pub enum Server {
    Mysql(Link),
    Postgres(Link),
    Redis(RedisConf),
}

impl Format {
    pub const ALL: [Format; 9] = [Format::Sql, Format::Json, Format::Csv, Format::Xml, Format::Sqlite, Format::PgSql, Format::Mongo, Format::RedisRdb, Format::RedisCmds];

    pub fn label(self) -> &'static str {
        match self {
            Format::Sql => "SQL",
            Format::Json => "JSON",
            Format::Csv => "CSV",
            Format::Xml => "XML",
            Format::Sqlite => "SQLite",
            Format::PgSql => "PostgreSQL",
            Format::Mongo => "MongoDB",
            Format::RedisRdb => "Redis (RDB)",
            Format::RedisCmds => "Redis (redis-cli)",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Sql => "sql",
            Format::Json => "json",
            Format::Csv => "csv",
            Format::Xml => "xml",
            Format::Sqlite => "sqlite",
            Format::PgSql => "sql",
            Format::Mongo => "jsonl",
            Format::RedisRdb => "rdb",
            Format::RedisCmds => "redis",
        }
    }

    /// Eine Datei je Tabelle (Ziel ist dann ein Ordner)
    pub fn folder(self, tables: usize) -> bool {
        matches!(self, Format::Csv | Format::Mongo) && tables != 1
    }

}

pub struct Job {
    pub db: String,
    /// Ausgewaehlte Tabellen (leer = alle)
    pub tables: Vec<String>,
    pub with_data: bool,
    pub format: Format,
    pub target: PathBuf,
    /// Vorhandene Tabellen im Zielserver ersetzen
    pub replace: bool,
    /// Statt in eine Datei auf einen anderen Server uebertragen
    pub server: Option<Server>,
    pub progress: std::sync::Arc<Progress>,
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

type RowFn<'a> = &'a mut dyn FnMut(&[Column], &[Value]) -> Result<(), String>;

/// Zeilen einer Tabelle nacheinander lesen; zaehlt den Fortschritt und bricht bei Abbruch ab.
/// `f` bekommt die Spalten und die Werte jeder Zeile.
pub(super) fn rows_of(dbc: &Db, db: &str, table: &str, p: &Progress, f: RowFn) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    stream(dbc, db, table, &mut |cols, vals| {
        if let Some(v) = vals {
            if p.cancel.load(Ordering::Relaxed) {
                return Err(crate::i18n::text("Abgebrochen.").into());
            }
            f(cols, v)?;
            p.done.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    })
}

pub fn run(dbc: &Db, job: &Job) -> Result<String, String> {
    use std::sync::atomic::Ordering;
    let schema = dbc.schema(&job.db)?;
    let only = (!job.tables.is_empty()).then_some(job.tables.as_slice());
    let done = |p: &Path| Ok(p.display().to_string());
    if job.server.is_none() && job.format == Format::Sql {
        let f = std::fs::File::create(&job.target).map_err(io)?;
        let mut w = std::io::BufWriter::new(f);
        dbc.dump_to(&job.db, only, job.with_data, &mut w)?;
        w.flush().map_err(io)?;
        return done(&job.target);
    }
    // Alle anderen Ziele enthalten nur Tabellen (keine Sichten)
    let tables: Vec<&TableInfo> = schema.ordered_tables().iter().filter_map(|n| schema.table(n)).filter(|t| !t.is_view && only.is_none_or(|o| o.contains(&t.name))).collect();
    if tables.is_empty() {
        return Err(crate::i18n::text("Keine Tabellen zum Exportieren.").into());
    }
    // Gesamtzahl fuer die Fortschrittsanzeige
    let mut total = 0u64;
    if job.with_data || job.server.is_none() {
        for t in &tables {
            if let Ok(rows) = dbc.rows(&format!("SELECT COUNT(*) FROM {}.{}", q(&job.db), q(&t.name)), ()) {
                total += rows.first().and_then(|r| r[0].as_deref()).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
            }
        }
    }
    job.progress.total.store(total, Ordering::Relaxed);
    job.progress.done.store(0, Ordering::Relaxed);
    if let Some(server) = &job.server {
        return match server {
            Server::Mysql(l) => targets::mysql_server(dbc, job, &tables, l).map(|_| l.conf.host.clone()),
            Server::Postgres(l) => targets::pg_server(dbc, job, &tables, &schema, l).map(|_| l.conf.host.clone()),
            Server::Redis(c) => targets::redis_server(dbc, job, &tables, c).map(|_| c.host.clone()),
        };
    }
    match job.format {
        Format::Json => write_file(&job.target, |w| json(dbc, job, &tables, w)),
        Format::Xml => write_file(&job.target, |w| xml(dbc, &job.db, &tables, w)),
        Format::Csv if job.format.folder(tables.len()) => {
            std::fs::create_dir_all(&job.target).map_err(io)?;
            for t in &tables {
                write_file(&job.target.join(format!("{}.csv", file_safe(&t.name))), |w| csv(dbc, job, t, w))?;
            }
            Ok(())
        }
        Format::Csv => write_file(&job.target, |w| csv(dbc, job, tables[0], w)),
        Format::Sqlite => sqlite(dbc, job, &tables, &job.target),
        Format::PgSql => write_file(&job.target, |w| targets::pg_script(dbc, job, &tables, &schema, w)),
        Format::Mongo if job.format.folder(tables.len()) => {
            std::fs::create_dir_all(&job.target).map_err(io)?;
            for t in &tables {
                write_file(&job.target.join(format!("{}.jsonl", file_safe(&t.name))), |w| targets::mongo_lines(dbc, job, t, w))?;
            }
            Ok(())
        }
        Format::Mongo => write_file(&job.target, |w| targets::mongo_lines(dbc, job, tables[0], w)),
        Format::RedisRdb => targets::redis_rdb(dbc, job, &tables, &job.target),
        Format::RedisCmds => write_file(&job.target, |w| targets::redis_commands(dbc, job, &tables, w)),
        Format::Sql => unreachable!(),
    }?;
    done(&job.target)
}

fn write_file(path: &Path, f: impl FnOnce(&mut dyn Write) -> Result<(), String>) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(io)?;
    let mut w = std::io::BufWriter::new(file);
    f(&mut w)?;
    w.flush().map_err(io)
}

fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') { c } else { '_' }).collect()
}

/// Liest eine Tabelle Zeile fuer Zeile. `f` bekommt einmal die Spalten (ohne Zeile) und dann jede Zeile.
/// Wird einmal mit den Spalten (ohne Zeile) und dann je Zeile aufgerufen
type RowSink<'a> = &'a mut dyn FnMut(&[Column], Option<&[Value]>) -> Result<(), String>;

fn stream(dbc: &Db, db: &str, table: &str, f: RowSink) -> Result<(), String> {
    let mut conn = dbc.pool.get_conn().map_err(err_text)?;
    let res = conn.query_iter(format!("SELECT * FROM {}.{}", q(db), q(table))).map_err(err_text)?;
    let cols: Vec<Column> = res.columns().as_ref().to_vec();
    f(&cols, None)?;
    for r in res {
        let r = r.map_err(err_text)?;
        f(&cols, Some(&r.unwrap()))?;
    }
    Ok(())
}

pub(super) fn is_number(t: T) -> bool {
    matches!(t, T::MYSQL_TYPE_TINY | T::MYSQL_TYPE_SHORT | T::MYSQL_TYPE_LONG | T::MYSQL_TYPE_LONGLONG | T::MYSQL_TYPE_INT24 | T::MYSQL_TYPE_FLOAT | T::MYSQL_TYPE_DOUBLE | T::MYSQL_TYPE_DECIMAL | T::MYSQL_TYPE_NEWDECIMAL | T::MYSQL_TYPE_YEAR)
}

/// Text eines Werts; Binaerdaten ohne gueltiges UTF-8 als 0x…-Hexwert.
pub(super) fn text(v: &Value) -> Option<String> {
    match v {
        Value::NULL => None,
        Value::Bytes(b) if std::str::from_utf8(b).is_err() => Some(format!("0x{}", hex(b))),
        _ => crate::db::value_to_string(v),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

pub(super) fn valid_number(s: &str) -> bool {
    !s.is_empty() && s.parse::<f64>().is_ok_and(|f| f.is_finite()) && !s.starts_with('+') && !s.starts_with('.') && !s.ends_with('.') && !s.contains(['e', 'E', 'n', 'N', 'i', 'I', 'x'])
}

fn json_value(t: T, v: &Value) -> String {
    let Some(s) = text(v) else { return "null".into() };
    if is_number(t) && valid_number(&s) {
        return s;
    }
    if t == T::MYSQL_TYPE_JSON && serde_json::from_str::<serde_json::Value>(&s).is_ok() {
        return s;
    }
    serde_json::Value::String(s).to_string()
}

fn json(dbc: &Db, job: &Job, tables: &[&TableInfo], w: &mut dyn Write) -> Result<(), String> {
    w.write_all(b"{\n").map_err(io)?;
    for (i, t) in tables.iter().enumerate() {
        let name = serde_json::Value::String(t.name.clone());
        write!(w, "  {name}: [").map_err(io)?;
        let mut n = 0usize;
        rows_of(dbc, &job.db, &t.name, &job.progress, &mut |cols, vals| {
            let fields: Vec<String> = cols.iter().zip(vals).map(|(c, v)| format!("{}: {}", serde_json::Value::String(c.name_str().into_owned()), json_value(c.column_type(), v))).collect();
            write!(w, "{}\n    {{{}}}", if n == 0 { "" } else { "," }, fields.join(", ")).map_err(io)?;
            n += 1;
            Ok(())
        })?;
        writeln!(w, "{}]{}", if n == 0 { "" } else { "\n  " }, if i + 1 < tables.len() { "," } else { "" }).map_err(io)?;
    }
    w.write_all(b"}\n").map_err(io)
}

fn csv_cell(v: &str) -> String {
    if v.contains([';', '"', '\n', '\r']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v.to_string() }
}

fn csv(dbc: &Db, job: &Job, t: &TableInfo, w: &mut dyn Write) -> Result<(), String> {
    // UTF-8-Kennung, damit Excel Umlaute richtig liest
    w.write_all(b"\xEF\xBB\xBF").map_err(io)?;
    let mut header = false;
    rows_of(dbc, &job.db, &t.name, &job.progress, &mut |cols, v| {
        if !header {
            header = true;
            write!(w, "{}\r\n", cols.iter().map(|c| csv_cell(&c.name_str())).collect::<Vec<_>>().join(";")).map_err(io)?;
        }
        let line: Vec<String> = v.iter().map(|v| text(v).map(|s| csv_cell(&s)).unwrap_or_default()).collect();
        write!(w, "{}\r\n", line.join(";")).map_err(io)
    })
}

/// Gueltiger XML-Elementname
fn xml_name(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
    if !out.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
        out.insert(0, '_');
    }
    out
}

fn xml_esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\t' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

fn xml(dbc: &Db, db: &str, tables: &[&TableInfo], w: &mut dyn Write) -> Result<(), String> {
    write!(w, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<database name=\"{}\">\n", xml_esc(db)).map_err(io)?;
    for t in tables {
        writeln!(w, "  <table name=\"{}\">", xml_esc(&t.name)).map_err(io)?;
        let mut conn = dbc.pool.get_conn().map_err(err_text)?;
        let res = conn.query_iter(format!("SELECT * FROM {}.{}", q(db), q(&t.name))).map_err(err_text)?;
        let names: Vec<String> = res.columns().as_ref().iter().map(|c| xml_name(&c.name_str())).collect();
        for r in res {
            let r = r.map_err(err_text)?;
            w.write_all(b"    <row>").map_err(io)?;
            for (n, v) in names.iter().zip(r.unwrap().iter()) {
                match text(v) {
                    Some(s) => write!(w, "<{n}>{}</{n}>", xml_esc(&s)).map_err(io)?,
                    None => write!(w, "<{n} null=\"true\"/>").map_err(io)?,
                }
            }
            w.write_all(b"</row>\n").map_err(io)?;
        }
        w.write_all(b"  </table>\n").map_err(io)?;
    }
    w.write_all(b"</database>\n").map_err(io)
}

/// SQLite-Spaltenart zu einem MariaDB-Typ
fn sqlite_type(col_type: &str) -> &'static str {
    let t = col_type.to_lowercase();
    let base = t.split(['(', ' ']).next().unwrap_or("");
    match base {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "bit" | "bool" | "boolean" | "year" => "INTEGER",
        "float" | "double" | "real" => "REAL",
        "decimal" | "numeric" => "NUMERIC",
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => "BLOB",
        _ => "TEXT",
    }
}

fn sqlite_value(t: T, v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as S;
    match v {
        Value::NULL => S::Null,
        Value::Int(i) => S::Integer(*i),
        Value::UInt(u) => i64::try_from(*u).map(S::Integer).unwrap_or_else(|_| S::Text(u.to_string())),
        Value::Float(f) => S::Real(*f as f64),
        Value::Double(d) => S::Real(*d),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            // Zahlen kommen im Textprotokoll als Text; Dezimalzahlen bleiben Text (keine Rundung)
            Ok(s) if is_number(t) && !matches!(t, T::MYSQL_TYPE_DECIMAL | T::MYSQL_TYPE_NEWDECIMAL) => s.parse::<i64>().map(S::Integer).or_else(|_| s.parse::<f64>().map(S::Real)).unwrap_or_else(|_| S::Text(s.into())),
            Ok(s) => S::Text(s.into()),
            Err(_) => S::Blob(b.clone()),
        },
        other => text(other).map(S::Text).unwrap_or(S::Null),
    }
}

fn sqlite(dbc: &Db, job: &Job, tables: &[&TableInfo], target: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(target);
    let mut conn = rusqlite::Connection::open(target).map_err(|e| e.to_string())?;
    let sq = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    for t in tables {
        let cols: Vec<String> = t.columns.iter().map(|c| format!("{} {}", sq(&c.name), sqlite_type(&c.col_type))).collect();
        let pk = t.pk_columns();
        let mut ddl = format!("CREATE TABLE {} ({}", sq(&t.name), cols.join(", "));
        if !pk.is_empty() {
            ddl.push_str(&format!(", PRIMARY KEY ({})", pk.iter().map(|c| sq(c)).collect::<Vec<_>>().join(", ")));
        }
        ddl.push(')');
        conn.execute_batch(&ddl).map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let marks = vec!["?"; t.columns.len()].join(", ");
            let mut stmt = tx.prepare(&format!("INSERT INTO {} VALUES ({marks})", sq(&t.name))).map_err(|e| e.to_string())?;
            rows_of(dbc, &job.db, &t.name, &job.progress, &mut |cols, vals| {
                let params: Vec<rusqlite::types::Value> = cols.iter().zip(vals).map(|(c, v)| sqlite_value(c.column_type(), v)).collect();
                stmt.execute(rusqlite::params_from_iter(params)).map(|_| ()).map_err(|e| e.to_string())
            })?;
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_names() {
        assert!(valid_number("12") && valid_number("-3.5") && valid_number("0.25"));
        assert!(!valid_number("") && !valid_number("1e5") && !valid_number("NaN") && !valid_number("0x10") && !valid_number("+1"));
        assert_eq!(xml_name("1 spalte"), "_1_spalte");
        assert_eq!(xml_esc("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(sqlite_type("int(11) unsigned"), "INTEGER");
        assert_eq!(sqlite_type("varchar(20)"), "TEXT");
        assert_eq!(sqlite_type("decimal(10,2)"), "NUMERIC");
        assert_eq!(json_value(T::MYSQL_TYPE_LONG, &Value::Bytes(b"42".to_vec())), "42");
        assert_eq!(json_value(T::MYSQL_TYPE_VAR_STRING, &Value::Bytes(b"a\"b".to_vec())), "\"a\\\"b\"");
        assert_eq!(json_value(T::MYSQL_TYPE_LONG, &Value::NULL), "null");
    }

    /// Braucht einen Server auf 127.0.0.1:3306 (root ohne Passwort).
    #[test]
    #[ignore]
    fn all_formats() {
        use crate::db::{ConnInfo, Db};
        let dbc = Db::connect(&ConnInfo::default()).unwrap();
        let mut c = dbc.new_conn().unwrap();
        let setup = "DROP DATABASE IF EXISTS exp_test; CREATE DATABASE exp_test; USE exp_test;
            CREATE TABLE Kunde (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(40), preis DECIMAL(8,2), notiz TEXT, bild BLOB, flag TINYINT(1), ts DATETIME);
            INSERT INTO Kunde VALUES (1,'Müller; \"Q\"',9.50,'a\nb',X'FF00',1,'2024-01-02 03:04:05'),(2,NULL,NULL,NULL,NULL,0,NULL);
            CREATE TABLE zwei (a INT, b DOUBLE); INSERT INTO zwei VALUES (1, 2.5);";
        let out = crate::db::run_script(&mut c, setup);
        assert!(out.error.is_none(), "{:?}", out.error);
        let dir = std::env::temp_dir().join(format!("export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let job = |format: Format, tables: &[&str], file: &str| Job { db: "exp_test".into(), tables: tables.iter().map(|t| t.to_string()).collect(), with_data: true, format, target: dir.join(file), replace: false, server: None, progress: Default::default() };

        let p = run(&dbc, &job(Format::Json, &[], "a.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!(v["Kunde"][0]["id"], 1);
        assert_eq!(v["Kunde"][0]["preis"], 9.5);
        assert_eq!(v["Kunde"][0]["name"], "Müller; \"Q\"");
        assert!(v["Kunde"][1]["name"].is_null());
        assert_eq!(v["zwei"][0]["b"], 2.5);

        let p = run(&dbc, &job(Format::Xml, &["Kunde"], "a.xml")).unwrap();
        let x = std::fs::read_to_string(p).unwrap();
        assert!(x.contains("<name>Müller; &quot;Q&quot;</name>") && x.contains("<name null=\"true\"/>") && !x.contains("zwei"), "{x}");

        let p = run(&dbc, &job(Format::Csv, &["Kunde"], "k.csv")).unwrap();
        let csv = std::fs::read_to_string(p).unwrap();
        assert!(csv.starts_with("\u{feff}id;name;preis"), "{csv}");
        assert!(csv.contains("\"Müller; \"\"Q\"\"\""));
        run(&dbc, &job(Format::Csv, &[], "alle")).unwrap();
        assert!(dir.join("alle/zwei.csv").exists());

        let p = run(&dbc, &job(Format::Sqlite, &[], "a.sqlite")).unwrap();
        let sq = rusqlite::Connection::open(p).unwrap();
        let n: i64 = sq.query_row("SELECT COUNT(*) FROM Kunde", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let (name, preis, flag): (String, String, i64) = sq.query_row("SELECT name, preis, flag FROM Kunde WHERE id = 1", [], |r| Ok((r.get(0)?, r.get::<_, rusqlite::types::Value>(1).map(|v| format!("{v:?}"))?, r.get(2)?))).unwrap();
        assert_eq!(name, "Müller; \"Q\"");
        assert!(preis.contains("9.5"), "{preis}");
        assert_eq!(flag, 1);
        let blob: Vec<u8> = sq.query_row("SELECT bild FROM Kunde WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(blob, vec![0xFF, 0]);

        let p = run(&dbc, &job(Format::Sql, &["zwei"], "a.sql")).unwrap();
        let sql = std::fs::read_to_string(p).unwrap();
        assert!(sql.contains("CREATE TABLE `zwei`") && !sql.contains("Kunde"));
        let _ = c.query_drop("DROP DATABASE exp_test");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
