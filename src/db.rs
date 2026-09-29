// Datenbankzugriff: Verbindung, Abfragen, Metadaten (Reverse Engineering).

use mysql::prelude::*;
use mysql::{Conn, Opts, OptsBuilder, Params, Pool, PoolConstraints, PoolOpts, Value};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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

/// Ergebnis einer einzelnen Anweisung.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub affected: u64,
    pub truncated: bool,
}

impl ResultSet {
    pub fn has_table(&self) -> bool {
        !self.columns.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryOutput {
    pub sets: Vec<ResultSet>,
    pub error: Option<String>,
    pub elapsed: Duration,
    pub database: Option<String>,
    /// Zusatzhinweis (z. B. "vorher gesichert")
    pub note: Option<String>,
}

pub const MAX_ROWS: usize = 50_000;

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

pub fn opt_to_value(v: &Option<String>) -> Value {
    match v {
        None => Value::NULL,
        Some(s) => Value::Bytes(s.clone().into_bytes()),
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

pub fn opt_lit(v: &Option<String>) -> String {
    match v {
        None => "NULL".into(),
        Some(s) => lit(s),
    }
}

/// Fuehrt einen (ggf. mehrteiligen) SQL-Text aus und sammelt alle Ergebnisse.
pub fn run_script<C: Queryable>(conn: &mut C, sql: &str) -> QueryOutput {
    let start = Instant::now();
    let mut out = QueryOutput::default();
    match conn.query_iter(sql) {
        Ok(mut result) => {
            while let Some(set) = result.iter() {
                let mut rs = ResultSet {
                    columns: set
                        .columns()
                        .as_ref()
                        .iter()
                        .map(|c| c.name_str().into_owned())
                        .collect(),
                    ..Default::default()
                };
                rs.affected = set.affected_rows();
                let mut failed = None;
                for row in set {
                    match row {
                        Ok(row) => {
                            if rs.rows.len() >= MAX_ROWS {
                                rs.truncated = true;
                                continue;
                            }
                            let r: Row = row.unwrap().iter().map(value_to_string).collect();
                            rs.rows.push(r);
                        }
                        Err(e) => {
                            failed = Some(e.to_string());
                            break;
                        }
                    }
                }
                out.sets.push(rs);
                if let Some(e) = failed {
                    out.error = Some(e);
                    break;
                }
            }
        }
        Err(e) => out.error = Some(e.to_string()),
    }
    out.elapsed = start.elapsed();
    out.database = conn
        .query_first::<Option<String>, _>("SELECT DATABASE()")
        .ok()
        .flatten()
        .flatten();
    out
}

// ---------------------------------------------------------------------------
// Metadaten

#[derive(Clone, Debug, Default)]
pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub comment: String,
}

impl ColumnInfo {
    pub fn is_pk(&self) -> bool {
        self.key == "PRI"
    }
}

#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub name: String,
    pub is_view: bool,
    pub engine: String,
    pub rows: Option<u64>,
    pub comment: String,
    pub columns: Vec<ColumnInfo>,
}

impl TableInfo {
    pub fn pk_columns(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|c| c.is_pk())
            .map(|c| c.name.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ForeignKey {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Clone, Debug, Default)]
pub struct IndexInfo {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<TableInfo>,
    pub fks: Vec<ForeignKey>,
}

impl Schema {
    pub fn table(&self, name: &str) -> Option<&TableInfo> {
        self.tables.iter().find(|t| t.name == name)
    }

    pub fn is_fk_column(&self, table: &str, column: &str) -> bool {
        self.fks
            .iter()
            .any(|f| f.table == table && f.columns.iter().any(|c| c == column))
    }

    /// Tabellen in Abhaengigkeitsreihenfolge (referenzierte zuerst).
    pub fn ordered_tables(&self) -> Vec<String> {
        let mut done: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        fn visit(s: &Schema, t: &str, done: &mut Vec<String>, seen: &mut HashSet<String>) {
            if !seen.insert(t.to_string()) {
                return;
            }
            for fk in s.fks.iter().filter(|f| f.table == t) {
                if fk.ref_table != t {
                    visit(s, &fk.ref_table, done, seen);
                }
            }
            if s.table(t).is_some() {
                done.push(t.to_string());
            }
        }
        for t in self.tables.iter().filter(|t| !t.is_view) {
            visit(self, &t.name, &mut done, &mut seen);
        }
        for t in self.tables.iter().filter(|t| t.is_view) {
            done.push(t.name.clone());
        }
        done
    }
}

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
        let pool = Pool::new(info.opts()).map_err(|e| e.to_string())?;
        let mut conn = pool.get_conn().map_err(|e| e.to_string())?;
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .map_err(|e| e.to_string())?;
        Ok(Db {
            pool,
            info: info.clone(),
            version: version.unwrap_or_default(),
        })
    }

    /// Eigene, dauerhafte Verbindung (z. B. fuer ein SQL-Fenster).
    pub fn new_conn(&self) -> Result<Conn, String> {
        Conn::new(self.info.opts()).map_err(|e| e.to_string())
    }

    fn conn(&self) -> Result<mysql::PooledConn, String> {
        self.pool.get_conn().map_err(|e| e.to_string())
    }

    pub fn rows(&self, sql: &str, params: impl Into<Params>) -> Result<Vec<Row>, String> {
        let mut c = self.conn()?;
        let res: Vec<mysql::Row> = c.exec(sql, params).map_err(|e| e.to_string())?;
        Ok(res
            .into_iter()
            .map(|r| r.unwrap().iter().map(value_to_string).collect())
            .collect())
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

    /// Abfrage in einer bestimmten Datenbank ausfuehren.
    pub fn query_in(&self, db: &str, sql: &str) -> Result<ResultSet, String> {
        let mut c = self.conn()?;
        c.query_drop(format!("USE {}", q(db)))
            .map_err(|e| e.to_string())?;
        let mut out = run_script(&mut c, sql);
        if let Some(e) = out.error.take() {
            return Err(e);
        }
        Ok(out.sets.into_iter().find(|s| s.has_table()).unwrap_or_default())
    }

    pub fn exec(&self, sql: &str, params: impl Into<Params>) -> Result<u64, String> {
        let mut c = self.conn()?;
        c.exec_drop(sql, params).map_err(|e| e.to_string())?;
        Ok(c.affected_rows())
    }

    pub fn exec_in(&self, db: &str, sql: &str) -> Result<u64, String> {
        let mut c = self.conn()?;
        c.query_drop(format!("USE {}", q(db)))
            .map_err(|e| e.to_string())?;
        c.query_drop(sql).map_err(|e| e.to_string())?;
        Ok(c.affected_rows())
    }

    pub fn databases(&self) -> Result<Vec<String>, String> {
        Ok(self
            .rows("SHOW DATABASES", ())?
            .into_iter()
            .map(|r| s(&r[0]))
            .collect())
    }

    pub fn schema(&self, db: &str) -> Result<Schema, String> {
        let mut schema = Schema {
            name: db.to_string(),
            ..Default::default()
        };
        let tables = self.rows(
            "SELECT TABLE_NAME, TABLE_TYPE, IFNULL(ENGINE,''), TABLE_ROWS, IFNULL(TABLE_COMMENT,'') \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (db,),
        )?;
        let mut idx = HashMap::new();
        for r in tables {
            idx.insert(s(&r[0]), schema.tables.len());
            schema.tables.push(TableInfo {
                name: s(&r[0]),
                is_view: s(&r[1]).contains("VIEW"),
                engine: s(&r[2]),
                rows: r[3].as_ref().and_then(|v| v.parse().ok()),
                comment: s(&r[4]),
                columns: Vec::new(),
            });
        }
        let cols = self.rows(
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, \
             EXTRA, IFNULL(COLUMN_COMMENT,'') FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (db,),
        )?;
        for r in cols {
            if let Some(&i) = idx.get(&s(&r[0])) {
                schema.tables[i].columns.push(ColumnInfo {
                    name: s(&r[1]),
                    col_type: s(&r[2]),
                    nullable: s(&r[3]) == "YES",
                    key: s(&r[4]),
                    // MariaDB liefert "NULL" (ohne Anfuehrungszeichen) fuer "kein Standardwert"
                    default: r[5].clone().filter(|d| d != "NULL"),
                    extra: s(&r[6]),
                    comment: s(&r[7]),
                });
            }
        }
        let fks = self.rows(
            "SELECT k.CONSTRAINT_NAME, k.TABLE_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_NAME, \
             k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE k \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
              AND r.TABLE_NAME = k.TABLE_NAME \
             WHERE k.TABLE_SCHEMA = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            (db,),
        )?;
        for r in fks {
            let name = s(&r[0]);
            let table = s(&r[1]);
            if let Some(fk) = schema
                .fks
                .iter_mut()
                .find(|f| f.name == name && f.table == table)
            {
                fk.columns.push(s(&r[2]));
                fk.ref_columns.push(s(&r[4]));
            } else {
                schema.fks.push(ForeignKey {
                    name,
                    table,
                    columns: vec![s(&r[2])],
                    ref_table: s(&r[3]),
                    ref_columns: vec![s(&r[4])],
                    on_update: s(&r[5]),
                    on_delete: s(&r[6]),
                });
            }
        }
        Ok(schema)
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

    /// Erzeugt ein SQL-Skript der Datenbank (Struktur, optional mit Daten).
    pub fn dump(&self, db: &str, with_data: bool) -> Result<String, String> {
        let schema = self.schema(db)?;
        let mut out = String::new();
        out.push_str(&format!(
            "-- EasyMySQL SQL-Export\n-- Datenbank: {db}\n-- Server: {}\n\n",
            self.version
        ));
        out.push_str("SET FOREIGN_KEY_CHECKS = 0;\n\n");
        out.push_str(&format!(
            "CREATE DATABASE IF NOT EXISTS {};\nUSE {};\n\n",
            q(db),
            q(db)
        ));
        for t in schema.ordered_tables() {
            let info = schema.table(&t).unwrap();
            let create = self.create_statement(db, &t)?;
            out.push_str(&format!("-- {}: {}\n", if info.is_view { "Sicht" } else { "Tabelle" }, t));
            // Nur bei einer vollstaendigen Sicherung vorhandene Tabellen ersetzen
            if with_data {
                if info.is_view {
                    out.push_str(&format!("DROP VIEW IF EXISTS {};\n", q(&t)));
                } else {
                    out.push_str(&format!("DROP TABLE IF EXISTS {};\n", q(&t)));
                }
            }
            out.push_str(&create);
            out.push_str(";\n\n");
            if with_data && !info.is_view {
                let rs = self.query(&format!("SELECT * FROM {}.{}", q(db), q(&t)))?;
                if !rs.rows.is_empty() {
                    let cols: Vec<String> = rs.columns.iter().map(|c| q(c)).collect();
                    for chunk in rs.rows.chunks(200) {
                        out.push_str(&format!(
                            "INSERT INTO {} ({}) VALUES\n",
                            q(&t),
                            cols.join(", ")
                        ));
                        let vals: Vec<String> = chunk
                            .iter()
                            .map(|r| {
                                format!(
                                    "  ({})",
                                    r.iter().map(opt_lit).collect::<Vec<_>>().join(", ")
                                )
                            })
                            .collect();
                        out.push_str(&vals.join(",\n"));
                        out.push_str(";\n");
                    }
                    out.push('\n');
                }
            }
        }
        out.push_str("SET FOREIGN_KEY_CHECKS = 1;\n");
        Ok(out)
    }
}

pub const COMMON_TYPES: &[&str] = &[
    "INT",
    "BIGINT",
    "SMALLINT",
    "TINYINT",
    "DECIMAL(10,2)",
    "FLOAT",
    "DOUBLE",
    "VARCHAR(50)",
    "VARCHAR(255)",
    "CHAR(10)",
    "TEXT",
    "LONGTEXT",
    "DATE",
    "DATETIME",
    "TIMESTAMP",
    "TIME",
    "YEAR",
    "BOOLEAN",
    "BLOB",
    "ENUM('a','b')",
    "JSON",
];

pub const SYSTEM_DATABASES: &[&str] = &["information_schema", "performance_schema", "mysql", "sys"];

/// Spaltendefinition (fuer Tabellen-Designer und ALTER TABLE).
#[derive(Clone, Debug, Default)]
pub struct ColDef {
    pub name: String,
    pub col_type: String,
    pub not_null: bool,
    pub primary: bool,
    pub auto_inc: bool,
    pub unique: bool,
    pub default: String,
    pub comment: String,
    /// Fremdschluessel-Ziel im Format "tabelle.spalte" (leer = keiner)
    pub references: String,
}

impl ColDef {
    pub fn new(name: &str, ty: &str) -> Self {
        Self {
            name: name.into(),
            col_type: ty.into(),
            ..Default::default()
        }
    }

    pub fn from_info(c: &ColumnInfo) -> Self {
        Self {
            name: c.name.clone(),
            col_type: c.col_type.clone(),
            not_null: !c.nullable,
            primary: c.is_pk(),
            auto_inc: c.extra.to_lowercase().contains("auto_increment"),
            unique: c.key == "UNI",
            default: c.default.clone().unwrap_or_default(),
            comment: c.comment.clone(),
            references: String::new(),
        }
    }

    /// Spaltendefinition ohne PRIMARY KEY/UNIQUE (die kommen separat).
    pub fn sql(&self) -> String {
        let mut s = format!("{} {}", q(&self.name), self.col_type.trim());
        if self.not_null || self.primary {
            s.push_str(" NOT NULL");
        } else {
            s.push_str(" NULL");
        }
        let d = self.default.trim();
        if !d.is_empty() && !self.auto_inc {
            let upper = d.to_uppercase();
            let is_expr = upper == "NULL"
                || upper.starts_with("CURRENT_TIMESTAMP")
                || upper == "NOW()"
                || upper == "CURDATE()"
                || (d.starts_with('\'') && d.ends_with('\'') && d.len() >= 2)
                || (d.starts_with('(') && d.ends_with(')'));
            if is_expr {
                s.push_str(&format!(" DEFAULT {d}"));
            } else {
                s.push_str(&format!(" DEFAULT {}", smart_lit(d)));
            }
        }
        if self.auto_inc {
            s.push_str(" AUTO_INCREMENT");
        }
        if !self.comment.trim().is_empty() {
            s.push_str(&format!(" COMMENT {}", lit(self.comment.trim())));
        }
        s
    }

    pub fn fk_target(&self) -> Option<(String, String)> {
        let r = self.references.trim();
        let (t, c) = r.split_once('.')?;
        if t.is_empty() || c.is_empty() {
            return None;
        }
        Some((t.to_string(), c.to_string()))
    }
}

pub fn create_table_sql(db: &str, table: &str, cols: &[ColDef], engine: &str, comment: &str) -> String {
    let mut parts: Vec<String> = cols
        .iter()
        .filter(|c| !c.name.trim().is_empty())
        .map(|c| format!("  {}", c.sql()))
        .collect();
    let pks: Vec<String> = cols
        .iter()
        .filter(|c| c.primary && !c.name.trim().is_empty())
        .map(|c| q(&c.name))
        .collect();
    if !pks.is_empty() {
        parts.push(format!("  PRIMARY KEY ({})", pks.join(", ")));
    }
    for c in cols.iter().filter(|c| c.unique && !c.primary && !c.name.trim().is_empty()) {
        parts.push(format!("  UNIQUE KEY {} ({})", q(&format!("uq_{}", c.name)), q(&c.name)));
    }
    for c in cols.iter().filter(|c| !c.name.trim().is_empty()) {
        if let Some((rt, rc)) = c.fk_target() {
            parts.push(format!(
                "  CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE RESTRICT ON UPDATE CASCADE",
                q(&format!("fk_{}_{}", table, c.name)),
                q(&c.name),
                q(&rt),
                q(&rc)
            ));
        }
    }
    let mut sql = format!(
        "CREATE TABLE {}.{} (\n{}\n) ENGINE={} DEFAULT CHARSET=utf8mb4",
        q(db),
        q(table),
        parts.join(",\n"),
        if engine.is_empty() { "InnoDB" } else { engine }
    );
    if !comment.trim().is_empty() {
        sql.push_str(&format!(" COMMENT={}", lit(comment.trim())));
    }
    sql
}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert_eq!(s.ordered_tables(), vec!["klasse", "schueler", "note"]);
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
