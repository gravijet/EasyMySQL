// Datenbankzugriff: Verbindung, Abfragen, Metadaten (Reverse Engineering).

use mysql::prelude::*;
use mysql::{Conn, Opts, OptsBuilder, Params, Pool, PoolConstraints, PoolOpts, Value};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

mod ddl;
mod schema;
mod script;

pub use ddl::*;
pub use schema::*;
pub use script::*;

pub type Row = Vec<Option<String>>;

#[derive(Clone, Debug, PartialEq)]
pub struct ConnInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

impl Default for ConnInfo {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: String::new(),
        }
    }
}

/// Beendet die laufende Abfrage einer Verbindung auf dem Server (`KILL QUERY`) ueber eine
/// zweite, kurzlebige Verbindung.
pub fn kill_query(info: &ConnInfo, connection_id: u32) {
    if let Ok(mut c) = Conn::new(info.opts()) {
        let _ = c.query_drop(format!("KILL QUERY {connection_id}"));
    }
}

impl ConnInfo {
    pub fn opts(&self) -> Opts {
        OptsBuilder::new()
            .ip_or_hostname(Some(self.host.clone()))
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(if self.password.is_empty() {
                None
            } else {
                Some(self.password.clone())
            })
            .tcp_connect_timeout(Some(Duration::from_secs(3)))
            .pool_opts(PoolOpts::default().with_constraints(PoolConstraints::new(1, 8).unwrap()))
            .into()
    }

    pub fn label(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }
}

pub fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::NULL => None,
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        Value::Int(i) => Some(i.to_string()),
        Value::UInt(u) => Some(u.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Double(d) => Some(d.to_string()),
        Value::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                Some(format!("{y:04}-{mo:02}-{d:02}"))
            } else if *us == 0 {
                Some(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"))
            } else {
                Some(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}"))
            }
        }
        Value::Time(neg, d, h, mi, s, us) => {
            let sign = if *neg { "-" } else { "" };
            let hours = *d * 24 + *h as u32;
            if *us == 0 {
                Some(format!("{sign}{hours:02}:{mi:02}:{s:02}"))
            } else {
                Some(format!("{sign}{hours:02}:{mi:02}:{s:02}.{us:06}"))
            }
        }
    }
}

/// Wie `value_to_string`, verbraucht den Wert aber und uebernimmt Byte-Puffer ohne Kopie.
pub fn value_into_string(v: Value) -> Option<String> {
    match v {
        Value::Bytes(b) => Some(String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())),
        other => value_to_string(&other),
    }
}

pub fn opt_to_value(v: &Option<String>) -> Value {
    match v {
        None => Value::NULL,
        Some(s) => Value::Bytes(s.clone().into_bytes()),
    }
}

/// Fehlermeldung lesbar machen: "MySqlError { ERROR 1146 (42S02): ... }" -> "ERROR 1146 (42S02): ..."
/// und fuer haeufige Fehler einen deutschen Hinweis anhaengen.
pub fn err_text(e: impl ToString) -> String {
    let s = e.to_string();
    let t = s.strip_prefix("MySqlError { ").and_then(|x| x.strip_suffix(" }")).unwrap_or(&s).to_string();
    let code: Option<u32> = t
        .strip_prefix("ERROR ")
        .and_then(|x| x.split_whitespace().next())
        .and_then(|c| c.parse().ok());
    let hint = match code {
        Some(1064) => Some(crate::i18n::text("Syntaxfehler – Schreibweise prüfen (fehlendes Komma, Klammer oder Anführungszeichen?).")),
        Some(1146) => Some(crate::i18n::text("Diese Tabelle gibt es (in der gewählten Datenbank) nicht.")),
        Some(1054) => Some(crate::i18n::text("Diese Spalte gibt es nicht – Schreibweise oder Tabelle prüfen.")),
        Some(1049) => Some(crate::i18n::text("Diese Datenbank gibt es nicht.")),
        Some(1046) => Some(crate::i18n::text("Keine Datenbank ausgewählt – oben eine Datenbank wählen oder USE datenbank; ausführen.")),
        Some(1050) => Some(crate::i18n::text("Die Tabelle existiert bereits (tipp: CREATE TABLE IF NOT EXISTS).")),
        Some(1007) => Some(crate::i18n::text("Die Datenbank existiert bereits (tipp: CREATE DATABASE IF NOT EXISTS).")),
        Some(1062) => Some(crate::i18n::text("Doppelter Wert in einer eindeutigen Spalte (z. B. Primärschlüssel).")),
        Some(1048) => Some(crate::i18n::text("Diese Spalte darf nicht leer (NULL) sein.")),
        Some(1364) => Some(crate::i18n::text("Für eine Pflichtspalte ohne Standardwert wurde kein Wert angegeben.")),
        Some(1451) => Some(crate::i18n::text("Der Datensatz wird noch von einer anderen Tabelle referenziert (Fremdschlüssel) – erst die abhängigen Datensätze löschen.")),
        Some(1452) => Some(crate::i18n::text("Der Fremdschlüssel verweist auf einen Datensatz, den es nicht gibt.")),
        Some(1045) => Some(crate::i18n::text("Anmeldung fehlgeschlagen – Benutzer oder Passwort falsch.")),
        Some(1136) => Some(crate::i18n::text("Anzahl der Werte passt nicht zur Anzahl der Spalten.")),
        Some(1265) | Some(1366) => Some(crate::i18n::text("Der Wert passt nicht zum Datentyp der Spalte.")),
        Some(1406) => Some(crate::i18n::text("Der Text ist zu lang für diese Spalte.")),
        Some(1215) | Some(1005) => Some(crate::i18n::text("Fremdschlüssel nicht möglich – gleicher Datentyp und ein Index/Primärschlüssel in der Zieltabelle nötig.")),
        _ => None,
    };
    match hint {
        Some(h) => crate::tr_format!("{t}\nHinweis: {h}", "{t}\nHint: {h}"),
        None => t,
    }
}

/// Bezeichner in Backticks setzen.
pub fn q(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// String-Literal fuer SQL erzeugen.
pub fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Wert als Literal: Zahlen bleiben Zahlen, alles andere wird gequotet.
pub fn smart_lit(s: &str) -> String {
    let t = s.trim();
    let numeric = !t.is_empty()
        && t.parse::<f64>().is_ok()
        && !t.starts_with('+')
        && !(t.len() > 1 && t.starts_with('0') && !t.starts_with("0."));
    if numeric {
        t.to_string()
    } else if t.eq_ignore_ascii_case("null") {
        "NULL".into()
    } else {
        lit(s)
    }
}

#[derive(Clone)]
pub struct Db {
    pub pool: Pool,
    pub info: ConnInfo,
    pub version: String,
}

fn s(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

impl Db {
    pub fn connect(info: &ConnInfo) -> Result<Db, String> {
        let pool = Pool::new(info.opts()).map_err(err_text)?;
        let mut conn = pool.get_conn().map_err(err_text)?;
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .map_err(err_text)?;
        Ok(Db {
            pool,
            info: info.clone(),
            version: version.unwrap_or_default(),
        })
    }

    /// Eigene, dauerhafte Verbindung (z. B. fuer ein SQL-Fenster).
    pub fn new_conn(&self) -> Result<Conn, String> {
        Conn::new(self.info.opts()).map_err(err_text)
    }

    fn conn(&self) -> Result<mysql::PooledConn, String> {
        self.pool.get_conn().map_err(err_text)
    }

    pub fn rows(&self, sql: &str, params: impl Into<Params>) -> Result<Vec<Row>, String> {
        let mut c = self.conn()?;
        c.exec_map(sql, params, |r: mysql::Row| r.unwrap().into_iter().map(value_into_string).collect())
            .map_err(err_text)
    }

    /// Text-Protokoll-Abfrage mit Spaltennamen.
    pub fn query(&self, sql: &str) -> Result<ResultSet, String> {
        let mut c = self.conn()?;
        let mut out = run_script(&mut c, sql);
        if let Some(e) = out.error.take() {
            return Err(e);
        }
        Ok(out.sets.into_iter().next().unwrap_or_default())
    }

    pub fn exec(&self, sql: &str, params: impl Into<Params>) -> Result<u64, String> {
        let mut c = self.conn()?;
        c.exec_drop(sql, params).map_err(err_text)?;
        Ok(c.affected_rows())
    }

    pub fn exec_in(&self, db: &str, sql: &str) -> Result<u64, String> {
        let mut c = self.conn()?;
        c.query_drop(format!("USE {}", q(db)))
            .map_err(err_text)?;
        c.query_drop(sql).map_err(err_text)?;
        Ok(c.affected_rows())
    }

    pub fn databases(&self) -> Result<Vec<String>, String> {
        Ok(self
            .rows("SHOW DATABASES", ())?
            .into_iter()
            .map(|r| s(&r[0]))
            .collect())
    }

    pub fn indexes(&self, db: &str, table: &str) -> Result<Vec<IndexInfo>, String> {
        let rows = self.rows(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (db, table),
        )?;
        let mut out: Vec<IndexInfo> = Vec::new();
        for r in rows {
            let name = s(&r[0]);
            if let Some(ix) = out.iter_mut().find(|i| i.name == name) {
                ix.columns.push(s(&r[2]));
            } else {
                out.push(IndexInfo {
                    name,
                    unique: s(&r[1]) == "0",
                    columns: vec![s(&r[2])],
                });
            }
        }
        Ok(out)
    }

    pub fn create_statement(&self, db: &str, table: &str) -> Result<String, String> {
        let rs = self.query(&format!("SHOW CREATE TABLE {}.{}", q(db), q(table)))?;
        Ok(rs
            .rows
            .first()
            .and_then(|r| r.get(1).cloned().flatten())
            .unwrap_or_default())
    }

    /// SQL-Export als Text (nur sinnvoll ohne Daten, z. B. fuer CREATE TABLE).
    pub fn dump(&self, db: &str, with_data: bool) -> Result<String, String> {
        let mut out = Vec::new();
        self.dump_to(db, with_data, &mut out)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// SQL-Export Zeile fuer Zeile in `out` schreiben: auch riesige Tabellen belegen nur den
    /// Speicher eines Einfuege-Blocks (200 Zeilen).
    pub fn dump_to(&self, db: &str, with_data: bool, out: &mut dyn std::io::Write) -> Result<(), String> {
        const BATCH: usize = 200;
        let w = |e: std::io::Error| e.to_string();
        let schema = self.schema(db)?;
        write!(out, "-- EasyMySQL SQL-Export\n-- Datenbank: {db}\n-- Server: {}\n\n", self.version).map_err(w)?;
        write!(out, "SET FOREIGN_KEY_CHECKS = 0;\n\nCREATE DATABASE IF NOT EXISTS {};\nUSE {};\n\n", q(db), q(db)).map_err(w)?;
        for t in schema.ordered_tables() {
            let info = schema.table(&t).unwrap();
            let create = self.create_statement(db, &t)?;
            let kind = if info.is_view { "VIEW" } else { "TABLE" };
            writeln!(out, "-- {}: {t}", if info.is_view { crate::i18n::text("Sicht") } else { crate::i18n::text("Tabelle") }).map_err(w)?;
            // Nur bei einer vollstaendigen Sicherung vorhandene Tabellen ersetzen
            if with_data {
                writeln!(out, "DROP {kind} IF EXISTS {};", q(&t)).map_err(w)?;
            }
            write!(out, "{create};\n\n").map_err(w)?;
            if !with_data || info.is_view {
                continue;
            }
            let mut conn = self.conn()?;
            let res = conn.query_iter(format!("SELECT * FROM {}.{}", q(db), q(&t))).map_err(err_text)?;
            let cols: Vec<String> = res.columns().as_ref().iter().map(|c| q(&c.name_str())).collect();
            let mut batch = String::new();
            let mut n = 0usize;
            for row in res {
                let row = row.map_err(err_text)?;
                batch.push_str(if n == 0 { "" } else { ",\n" });
                if n == 0 {
                    batch.push_str(&format!("INSERT INTO {} ({}) VALUES\n", q(&t), cols.join(", ")));
                }
                let vals: Vec<String> = row.unwrap().iter().map(dump_literal).collect();
                batch.push_str(&format!("  ({})", vals.join(", ")));
                n += 1;
                if n == BATCH {
                    batch.push_str(";\n");
                    out.write_all(batch.as_bytes()).map_err(w)?;
                    batch.clear();
                    n = 0;
                }
            }
            if n > 0 {
                batch.push_str(";\n");
                out.write_all(batch.as_bytes()).map_err(w)?;
            }
            out.write_all(b"\n").map_err(w)?;
        }
        out.write_all(b"SET FOREIGN_KEY_CHECKS = 1;\n").map_err(w)?;
        Ok(())
    }
}

/// SQL-Wert fuer den Export; Binaerdaten (kein gueltiges UTF-8) als Hex, damit nichts verfaelscht wird.
fn dump_literal(v: &Value) -> String {
    match v {
        Value::NULL => "NULL".into(),
        Value::Bytes(b) if std::str::from_utf8(b).is_err() => {
            let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
            format!("X'{hex}'")
        }
        _ => lit(&value_to_string(v).unwrap_or_default()),
    }
}

pub const SYSTEM_DATABASES: &[&str] = &["information_schema", "performance_schema", "mysql", "sys"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text() {
        let e = err_text("MySqlError { ERROR 1146 (42S02): Table 'a.b' doesn't exist }");
        assert!(e.starts_with("ERROR 1146 (42S02): Table 'a.b' doesn't exist\nHint: "), "{e}");
        assert_eq!(err_text("anderer Fehler"), "anderer Fehler");
    }

    #[test]
    fn dump_literals() {
        assert_eq!(dump_literal(&Value::NULL), "NULL");
        assert_eq!(dump_literal(&Value::Bytes(b"O'Neil".to_vec())), "'O''Neil'");
        assert_eq!(dump_literal(&Value::Bytes(vec![0xFF, 0x00, 0x1A])), "X'FF001A'");
        assert_eq!(dump_literal(&Value::Int(-5)), "'-5'");
    }

    #[test]
    fn quoting() {
        assert_eq!(q("a`b"), "`a``b`");
        assert_eq!(lit("O'Neil"), "'O''Neil'");
        assert_eq!(lit("a\\b"), "'a\\\\b'");
        assert_eq!(smart_lit("42"), "42");
        assert_eq!(smart_lit("3.5"), "3.5");
        assert_eq!(smart_lit("007"), "'007'");
        assert_eq!(smart_lit("10a"), "'10a'");
        assert_eq!(smart_lit("null"), "NULL");
    }

    #[test]
    fn column_sql() {
        let mut c = ColDef::new("preis", "DECIMAL(10,2)");
        c.not_null = true;
        c.default = "0".into();
        assert_eq!(c.sql(), "`preis` DECIMAL(10,2) NOT NULL DEFAULT 0");
        let mut d = ColDef::new("erstellt", "DATETIME");
        d.default = "CURRENT_TIMESTAMP".into();
        assert_eq!(d.sql(), "`erstellt` DATETIME NULL DEFAULT CURRENT_TIMESTAMP");
        let mut e = ColDef::new("name", "VARCHAR(20)");
        e.default = "'abc'".into();
        assert_eq!(e.sql(), "`name` VARCHAR(20) NULL DEFAULT 'abc'");
    }

    #[test]
    fn create_table() {
        let mut id = ColDef::new("id", "INT");
        id.primary = true;
        id.auto_inc = true;
        let mut k = ColDef::new("klasse_id", "INT");
        k.references = "klasse.id".into();
        let sql = create_table_sql("schule", "schueler", &[id, k, ColDef::new("", "INT")], "InnoDB", "");
        assert!(sql.starts_with("CREATE TABLE `schule`.`schueler` ("));
        assert!(sql.contains("`id` INT NOT NULL AUTO_INCREMENT"));
        assert!(sql.contains("PRIMARY KEY (`id`)"));
        assert!(sql.contains("FOREIGN KEY (`klasse_id`) REFERENCES `klasse` (`id`)"));
        assert_eq!(sql.matches("  `").count(), 2);
    }

    #[test]
    fn dependency_order() {
        let t = |n: &str| TableInfo { name: n.into(), ..Default::default() };
        let s = Schema {
            name: "x".into(),
            tables: vec![t("note"), t("schueler"), t("klasse")],
            fks: vec![
                ForeignKey { table: "note".into(), ref_table: "schueler".into(), ..Default::default() },
                ForeignKey { table: "schueler".into(), ref_table: "klasse".into(), ..Default::default() },
            ],
            unique_keys: vec![],
            ..Default::default()
        };
        assert_eq!(s.ordered_tables(), vec!["klasse", "schueler", "note"]);
    }

    #[test]
    fn relationship_kinds() {
        let col = |n: &str, null: bool| ColumnInfo { name: n.into(), nullable: null, ..Default::default() };
        let t = |n: &str, cols: Vec<ColumnInfo>| TableInfo { name: n.into(), columns: cols, ..Default::default() };
        let fk = |t: &str, c: &str, rt: &str| ForeignKey {
            name: format!("fk_{c}"),
            table: t.into(),
            columns: vec![c.into()],
            ref_table: rt.into(),
            ref_columns: vec!["id".into()],
            ..Default::default()
        };
        let uk = |t: &str, c: &[&str]| (t.to_string(), c.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let s = Schema {
            name: "x".into(),
            tables: vec![
                t("klasse", vec![col("id", false)]),
                t("schueler", vec![col("id", false), col("klasse_id", true)]),
                t("ausweis", vec![col("id", false), col("schueler_id", false)]),
                t("kurs", vec![col("id", false)]),
                t("belegung", vec![col("schueler_id", false), col("kurs_id", false)]),
            ],
            fks: vec![
                fk("schueler", "klasse_id", "klasse"),
                fk("ausweis", "schueler_id", "schueler"),
                fk("belegung", "schueler_id", "schueler"),
                fk("belegung", "kurs_id", "kurs"),
            ],
            unique_keys: vec![
                uk("klasse", &["id"]),
                uk("schueler", &["id"]),
                uk("ausweis", &["id"]),
                uk("ausweis", &["schueler_id"]),
                uk("kurs", &["id"]),
                uk("belegung", &["kurs_id", "schueler_id"]),
            ],
            ..Default::default()
        };
        assert_eq!(s.rel_kind(&s.fks[0]), RelKind::OneToMany);
        assert!(s.rel_optional(&s.fks[0]));
        assert_eq!(s.rel_kind(&s.fks[1]), RelKind::OneToOne);
        assert!(!s.rel_optional(&s.fks[1]));
        assert_eq!(s.junctions(), vec![("belegung".to_string(), 2, 3)]);
        // Wird die Zwischentabelle selbst referenziert, ist sie keine reine Zwischentabelle.
        let mut s2 = s.clone();
        s2.fks.push(fk("klasse", "belegung_id", "belegung"));
        assert!(s2.junctions().is_empty());
    }

    /// Braucht einen laufenden Server auf 127.0.0.1:3306 mit Datenbank "schule".
    #[test]
    #[ignore]
    fn dump_roundtrip() {
        let db = Db::connect(&ConnInfo::default()).unwrap();
        let dump = db.dump("schule", true).unwrap();
        let copy = dump.replace("`schule`", "`schule_kopie`");
        let mut c = db.new_conn().unwrap();
        c.query_drop("DROP DATABASE IF EXISTS schule_kopie").unwrap();
        let out = run_script(&mut c, &copy);
        assert!(out.error.is_none(), "{:?}", out.error);
        for t in db.schema("schule").unwrap().tables {
            let n1 = db.rows(&format!("SELECT COUNT(*) FROM schule.{}", q(&t.name)), ()).unwrap();
            let n2 = db.rows(&format!("SELECT COUNT(*) FROM schule_kopie.{}", q(&t.name)), ()).unwrap();
            assert_eq!(n1, n2, "{}", t.name);
        }
        assert_eq!(db.schema("schule_kopie").unwrap().fks.len(), db.schema("schule").unwrap().fks.len());
        c.query_drop("DROP DATABASE schule_kopie").unwrap();
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn split_basic() {
        let t = "-- Kommentar\nSELECT 1;\nSELECT 'a;b';  -- x\n\nUPDATE t SET a = \"x;\" WHERE `c;` = 1;\nSELECT 3";
        let s = split_statements(t);
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].sql, "SELECT 1");
        assert_eq!(s[0].line, 1);
        assert_eq!(s[1].sql, "SELECT 'a;b'");
        assert_eq!(s[2].line, 4);
        assert_eq!(s[3].sql, "SELECT 3");
    }

    #[test]
    fn split_delimiter() {
        let t = "DELIMITER //\nCREATE PROCEDURE p()\nBEGIN\n  SELECT 1;\n  SELECT 2;\nEND //\nDELIMITER ;\nCALL p();";
        let s = split_statements(t);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s[0].sql.starts_with("CREATE PROCEDURE") && s[0].sql.ends_with("END"));
        assert_eq!(s[1].sql, "CALL p()");
    }

    #[test]
    fn cursor_statement() {
        let t = "SELECT 1;\nSELECT 2;";
        let s = split_statements(t);
        assert_eq!(statement_at(&s, 3).unwrap().sql, "SELECT 1");
        assert_eq!(statement_at(&s, 12).unwrap().sql, "SELECT 2");
        assert_eq!(statement_at(&s, 9).unwrap().sql, "SELECT 1");
    }

    #[test]
    fn only_comments() {
        assert!(split_statements("-- nur\n# Kommentar\n").is_empty());
    }
}
