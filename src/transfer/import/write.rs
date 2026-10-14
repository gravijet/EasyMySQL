// Importierte Tabellen in MariaDB anlegen und fuellen.

use super::*;
use crate::db::{Db, err_text, q};
use mysql::prelude::*;
use mysql::{Params, Value};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct Options {
    pub database: String,
    /// Datenbank anlegen, falls es sie nicht gibt
    pub create_db: bool,
    /// Vorhandene Tabellen gleichen Namens loeschen und neu anlegen
    pub replace: bool,
}

/// Fortschritt, vom Hintergrundthread geschrieben und von der Oberflaeche gelesen
#[derive(Default)]
pub struct Progress {
    pub table: Mutex<String>,
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Meldungen: (Fehler?, Text)
    pub log: Mutex<Vec<(bool, String)>>,
}

impl Progress {
    fn note(&self, error: bool, text: String) {
        self.log.lock().unwrap().push((error, text));
    }
}

pub fn create_sql(t: &TableScan) -> String {
    // Fertige Anweisung der Quelle (anderer MySQL-Server): nur den Namen ersetzen, falls er geaendert wurde
    if let Some(ddl) = &t.ddl {
        if t.name == t.origin {
            return ddl.clone();
        }
        let old = format!("CREATE TABLE {}", q(&t.origin));
        if let Some(rest) = ddl.strip_prefix(&old) {
            return format!("CREATE TABLE {}{rest}", q(&t.name));
        }
    }
    let mut defs: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            let auto = t.auto_inc && t.pk.len() == 1 && t.pk[0] == c.name && matches!(c.ty, ColType::Int | ColType::BigInt);
            let ty = c.decl.clone().unwrap_or_else(|| c.ty.sql());
            format!("{} {}{}", q(&c.name), ty, if auto { " NOT NULL AUTO_INCREMENT" } else if t.pk.contains(&c.name) { " NOT NULL" } else { " NULL" })
        })
        .collect();
    if !t.pk.is_empty() {
        defs.push(format!("PRIMARY KEY ({})", t.pk.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ")));
    }
    format!("CREATE TABLE {} ({}) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4", q(&t.name), defs.join(", "))
}

fn to_value(v: Val, ty: ColType) -> Value {
    let text = matches!(ty, ColType::Varchar(_) | ColType::Text | ColType::LongText | ColType::Json);
    match v {
        Val::Null => Value::NULL,
        Val::Bool(b) if text => Value::Bytes(b.to_string().into_bytes()),
        Val::Bool(b) => Value::Int(b as i64),
        Val::Int(i) if text => Value::Bytes(i.to_string().into_bytes()),
        Val::Int(i) if ty == ColType::Double => Value::Double(i as f64),
        Val::Int(i) => Value::Int(i),
        Val::Real(r) if text => Value::Bytes(r.to_string().into_bytes()),
        Val::Real(r) => Value::Double(r),
        Val::Text(s) if matches!(ty, ColType::Date | ColType::DateTime) => Value::Bytes(normalize_date(&s).into_bytes()),
        Val::Text(s) | Val::Json(s) => Value::Bytes(s.into_bytes()),
        Val::Bytes(b) if text => Value::Bytes(String::from_utf8_lossy(&b).into_owned().into_bytes()),
        Val::Bytes(b) => Value::Bytes(b),
    }
}

fn flush(conn: &mut mysql::Conn, t: &TableScan, batch: &mut Vec<Value>, rows: usize) -> Result<(), String> {
    if rows == 0 {
        return Ok(());
    }
    let one = format!("({})", vec!["?"; t.columns.len()].join(","));
    let sql = format!("INSERT INTO {} ({}) VALUES {}", q(&t.name), t.columns.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(","), vec![one; rows].join(","));
    conn.exec_drop(sql, Params::Positional(std::mem::take(batch))).map_err(err_text)
}

fn import_table(conn: &mut mysql::Conn, src: &Source, t: &TableScan, p: &Progress) -> Result<u64, String> {
    *p.table.lock().unwrap() = t.name.clone();
    let cols = t.columns.len().max(1);
    let per_stmt = (60_000 / cols).clamp(1, 1000);
    let types: Vec<ColType> = t.columns.iter().map(|c| c.ty).collect();
    let mut batch: Vec<Value> = Vec::with_capacity(per_stmt * cols);
    let (mut in_batch, mut total, mut since_commit) = (0usize, 0u64, 0usize);
    conn.query_drop("START TRANSACTION").map_err(err_text)?;
    let mut failed: Option<String> = None;
    let r = read(src, t, &mut |row| {
        if p.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        for (v, ty) in row.into_iter().zip(&types) {
            batch.push(to_value(v, *ty));
        }
        in_batch += 1;
        total += 1;
        since_commit += 1;
        if in_batch == per_stmt {
            if let Err(e) = flush(conn, t, &mut batch, in_batch) {
                failed = Some(e);
                return Ok(false);
            }
            p.done.fetch_add(in_batch as u64, Ordering::Relaxed);
            in_batch = 0;
            if since_commit >= 50_000 {
                conn.query_drop("COMMIT").map_err(err_text)?;
                conn.query_drop("START TRANSACTION").map_err(err_text)?;
                since_commit = 0;
            }
        }
        Ok(true)
    });
    if r.is_ok() && failed.is_none() {
        failed = flush(conn, t, &mut batch, in_batch).err();
        p.done.fetch_add(in_batch as u64, Ordering::Relaxed);
    }
    let end = if failed.is_some() || r.is_err() { "ROLLBACK" } else { "COMMIT" };
    conn.query_drop(end).map_err(err_text)?;
    r?;
    match failed {
        Some(e) => Err(e),
        None => Ok(total),
    }
}

/// Importiert die gewaehlten Tabellen. Gibt die Zahl der importierten Zeilen zurueck.
pub fn run(dbc: &Db, src: &Source, tables: &[TableScan], opts: &Options, p: &Progress) -> Result<u64, String> {
    let mut conn = dbc.new_conn()?;
    if opts.create_db {
        conn.query_drop(format!("CREATE DATABASE IF NOT EXISTS {} CHARACTER SET utf8mb4", q(&opts.database))).map_err(err_text)?;
    }
    conn.query_drop(format!("USE {}", q(&opts.database))).map_err(err_text)?;
    conn.query_drop("SET SESSION foreign_key_checks = 0, unique_checks = 0").map_err(err_text)?;
    p.total.store(tables.iter().map(|t| t.rows).sum(), Ordering::Relaxed);
    let existing: Vec<String> = conn.query::<String, _>("SHOW TABLES").map_err(err_text)?.into_iter().map(|t| t.to_lowercase()).collect();
    let mut imported: Vec<&TableScan> = Vec::new();
    let mut all = 0u64;
    for t in tables {
        if p.cancel.load(Ordering::Relaxed) {
            p.note(true, crate::i18n::text("Abgebrochen.").into());
            break;
        }
        if existing.contains(&t.name.to_lowercase()) {
            if !opts.replace {
                p.note(true, crate::tr_format!("{}: gibt es schon – übersprungen.", "{}: already exists — skipped.", t.name));
                continue;
            }
            conn.query_drop(format!("DROP TABLE {}", q(&t.name))).map_err(err_text)?;
        }
        if let Err(e) = conn.query_drop(create_sql(t)) {
            p.note(true, format!("{}: {}", t.name, err_text(e)));
            continue;
        }
        match import_table(&mut conn, src, t, p) {
            Ok(n) => {
                all += n;
                imported.push(t);
                p.note(false, crate::tr_format!("{}: {} Zeile(n)", "{}: {} row(s)", t.name, n));
            }
            Err(e) => {
                let _ = conn.query_drop(format!("DROP TABLE IF EXISTS {}", q(&t.name)));
                p.note(true, format!("{}: {e}", t.name));
            }
        }
    }
    // Beziehungen (SQLite): nur zwischen importierten Tabellen, Fehler sind nicht schlimm
    for t in &imported {
        for fk in &t.fks {
            let Some(target) = imported.iter().find(|x| x.name == fk.ref_table) else { continue };
            let refs = if fk.ref_columns.is_empty() { target.pk.clone() } else { fk.ref_columns.clone() };
            if refs.len() != fk.columns.len() {
                continue;
            }
            let sql = format!(
                "ALTER TABLE {} ADD FOREIGN KEY ({}) REFERENCES {} ({})",
                q(&t.name),
                fk.columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(", "),
                q(&target.name),
                refs.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ")
            );
            if let Err(e) = conn.query_drop(sql) {
                p.note(true, crate::tr_format!("Beziehung {} → {} nicht übernommen: {}", "Relationship {} → {} not applied: {}", t.name, fk.ref_table, err_text(e)));
            }
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ConnInfo;

    fn go(dbc: &Db, src: &Source, db: &str) -> (Vec<TableScan>, Progress, u64) {
        let never = AtomicBool::new(false);
        let tables = scan(src, &never).unwrap();
        let p = Progress::default();
        let n = run(dbc, src, &tables, &Options { database: db.into(), create_db: true, replace: true }, &p).unwrap();
        (tables, p, n)
    }

    /// Braucht MariaDB auf 127.0.0.1:3306 (root ohne Passwort) und Redis auf 127.0.0.1:6379.
    #[test]
    #[ignore]
    fn all_sources() {
        let dbc = Db::connect(&ConnInfo::default()).unwrap();
        let mut c = dbc.new_conn().unwrap();
        let dir = std::env::temp_dir().join(format!("import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for d in ["imp_sqlite", "imp_json", "imp_csv", "imp_redis"] {
            c.query_drop(format!("DROP DATABASE IF EXISTS {d}")).unwrap();
        }

        // SQLite
        let path = dir.join("shop.sqlite");
        let sq = rusqlite::Connection::open(&path).unwrap();
        sq.execute_batch(
            "CREATE TABLE kunde (id INTEGER PRIMARY KEY, name TEXT NOT NULL, ok BOOLEAN, preis REAL, seit DATETIME, bild BLOB);
             CREATE TABLE bestellung (nr INTEGER PRIMARY KEY, kunde_id INTEGER REFERENCES kunde(id), menge INT);
             INSERT INTO kunde VALUES (1,'Müller',1,9.5,'2024-01-02 03:04:05',x'FF00'),(2,'Meier',0,NULL,NULL,NULL);
             INSERT INTO bestellung VALUES (10,1,3),(11,2,5);",
        )
        .unwrap();
        drop(sq);
        let (tables, p, n) = go(&dbc, &Source::Sqlite(path), "imp_sqlite");
        assert_eq!(n, 4, "{:?}", p.log.lock().unwrap());
        assert_eq!(tables.len(), 2);
        let rows = dbc.rows("SELECT name, ok, preis, seit, HEX(bild) FROM imp_sqlite.kunde ORDER BY id", ()).unwrap();
        assert_eq!(rows[0], vec![Some("Müller".into()), Some("1".into()), Some("9.5".into()), Some("2024-01-02 03:04:05".into()), Some("FF00".into())]);
        assert_eq!(rows[1][2], None);
        let schema = dbc.schema("imp_sqlite").unwrap();
        assert_eq!(schema.fks.len(), 1, "{:?}", p.log.lock().unwrap());
        assert_eq!(schema.table("kunde").unwrap().pk_columns(), vec!["id"]);
        assert!(schema.table("kunde").unwrap().columns[0].extra.contains("auto_increment"));

        // MongoDB-Export (NDJSON) und Objekt aus Arrays
        let nd = dir.join("kunden.json");
        std::fs::write(&nd, "{\"_id\":{\"$oid\":\"507f1f77bcf86cd799439011\"},\"name\":\"A\",\"alter\":30,\"adresse\":{\"ort\":\"Wien\"},\"seit\":{\"$date\":\"2024-05-17T10:20:30Z\"}}\n{\"_id\":{\"$oid\":\"507f1f77bcf86cd799439012\"},\"name\":\"B\",\"tags\":[\"x\",\"y\"]}\n").unwrap();
        let multi = dir.join("alles.json");
        std::fs::write(&multi, "{\"a\":[{\"x\":1}],\"b\":[{\"y\":\"t\"},{\"y\":\"u\"}]}").unwrap();
        let (tables, p, n) = go(&dbc, &Source::Docs(vec![nd, multi]), "imp_json");
        assert_eq!(n, 5, "{:?}", p.log.lock().unwrap());
        assert_eq!(tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["kunden", "a", "b"]);
        let rows = dbc.rows("SELECT `_id`, name, `alter`, adresse, tags, seit FROM imp_json.kunden ORDER BY name", ()).unwrap();
        assert_eq!(rows[0][0].as_deref(), Some("507f1f77bcf86cd799439011"));
        assert_eq!(rows[0][2].as_deref(), Some("30"));
        assert_eq!(rows[0][3].as_deref(), Some("{\"ort\":\"Wien\"}"));
        assert_eq!(rows[1][2], None);
        assert_eq!(rows[1][4].as_deref(), Some("[\"x\",\"y\"]"));
        assert_eq!(rows[0][5].as_deref(), Some("2024-05-17 10:20:30"));

        // CSV (Semikolon, deutsche Zahlen, Windows-1252)
        let csv = dir.join("preise.csv");
        let mut bytes = b"artikel;preis;menge;datum\r\nSt".to_vec();
        bytes.extend([0xFC]);
        bytes.extend(b"hle;19,90;3;2024-01-02\r\n\"Tisch; gro\xDF\";120,5;;2024-02-03\r\n");
        std::fs::write(&csv, bytes).unwrap();
        let (_, p, n) = go(&dbc, &Source::Csv(vec![csv]), "imp_csv");
        assert_eq!(n, 2, "{:?}", p.log.lock().unwrap());
        let rows = dbc.rows("SELECT artikel, preis, menge, datum FROM imp_csv.preise ORDER BY preis", ()).unwrap();
        assert_eq!(rows[0], vec![Some("Stühle".into()), Some("19.9".into()), Some("3".into()), Some("2024-01-02".into())]);
        assert_eq!(rows[1][0].as_deref(), Some("Tisch; groß"));
        assert_eq!(rows[1][2], None);

        // Redis
        let conf = RedisConf::default();
        let r = |a: &[&str]| redis::exec(&conf, a);
        r(&["FLUSHDB"]);
        r(&["SET", "name", "Müller"]);
        r(&["SET", "tmp", "1", "EX", "1000"]);
        r(&["HSET", "user:1", "a", "1", "b", "2"]);
        r(&["RPUSH", "q", "x", "y", "z"]);
        r(&["SADD", "s", "m1", "m2"]);
        r(&["ZADD", "z", "1.5", "p", "2", "r"]);
        let (tables, p, n) = go(&dbc, &Source::Redis(conf), "imp_redis");
        assert_eq!(tables.len(), 5);
        assert_eq!(n, 2 + 2 + 3 + 2 + 2, "{:?}", p.log.lock().unwrap());
        let rows = dbc.rows("SELECT `key`, value, ttl IS NOT NULL FROM imp_redis.redis_strings ORDER BY `key`", ()).unwrap();
        assert_eq!(rows[0], vec![Some("name".into()), Some("Müller".into()), Some("0".into())]);
        assert_eq!(rows[1][2].as_deref(), Some("1"));
        let rows = dbc.rows("SELECT member, score FROM imp_redis.redis_zsets ORDER BY score", ()).unwrap();
        assert_eq!(rows[0], vec![Some("p".into()), Some("1.5".into())]);

        for d in ["imp_sqlite", "imp_json", "imp_csv", "imp_redis"] {
            c.query_drop(format!("DROP DATABASE {d}")).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
