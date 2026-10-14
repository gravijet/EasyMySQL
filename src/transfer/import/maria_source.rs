// Tabellen eines anderen MariaDB- oder MySQL-Servers lesen (auch ueber SSH-Tunnel und TLS).
// Die Tabellen werden mit ihrer eigenen CREATE-TABLE-Anweisung angelegt (Spalten, Schluessel, Fremdschluessel).

use super::*;
use crate::db::{err_text, q};
use crate::transfer::server::{Link, Tls};
use mysql::prelude::*;
use mysql::{OptsBuilder, SslOpts, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SYSTEM: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

pub fn connect(link: &Link, database: Option<&str>) -> Result<mysql::Conn, String> {
    let c = &link.conf;
    let mut o = OptsBuilder::new()
        .ip_or_hostname(Some(link.host.clone()))
        .tcp_port(link.port)
        .user(Some(c.user.clone()))
        .pass(if c.password.is_empty() { None } else { Some(c.password.clone()) })
        .db_name(database.filter(|d| !d.is_empty()).map(str::to_string))
        .tcp_connect_timeout(Some(Duration::from_secs(10)));
    if c.tls.on() {
        let insecure = c.tls == Tls::NoVerify;
        o = o.ssl_opts(Some(SslOpts::default().with_danger_accept_invalid_certs(insecure).with_danger_skip_domain_validation(insecure)));
    }
    mysql::Conn::new(o).map_err(err_text)
}

/// Datenbanken des Servers ohne die Systemdatenbanken
pub fn databases(link: &Link) -> Result<Vec<String>, String> {
    let mut c = connect(link, None)?;
    let dbs: Vec<String> = c.query("SHOW DATABASES").map_err(err_text)?;
    Ok(dbs.into_iter().filter(|d| !SYSTEM.contains(&d.to_lowercase().as_str())).collect())
}

fn col_type(data_type: &str) -> ColType {
    match data_type.to_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "year" => ColType::Int,
        "bigint" => ColType::BigInt,
        "float" | "double" | "decimal" | "numeric" => ColType::Double,
        "date" => ColType::Date,
        "datetime" | "timestamp" => ColType::DateTime,
        "json" => ColType::Json,
        "tinyblob" | "blob" | "mediumblob" | "longblob" | "binary" | "varbinary" | "bit" | "geometry" | "point" | "linestring" | "polygon" => ColType::Blob,
        _ => ColType::Text,
    }
}

/// Anweisung eines MySQL-Servers fuer MariaDB lesbar machen (Sortierungen von MySQL 8)
pub fn mariadb_ddl(ddl: &str) -> String {
    ddl.replace("utf8mb4_0900_ai_ci", "utf8mb4_unicode_ci")
        .replace("utf8mb4_0900_as_cs", "utf8mb4_bin")
        .replace("utf8mb4_0900_as_ci", "utf8mb4_unicode_ci")
        .replace("utf8mb4_0900_bin", "utf8mb4_bin")
}

pub fn scan(link: &Link, cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let db = link.conf.database.as_str();
    if db.is_empty() {
        return Err(crate::i18n::text("Bitte eine Datenbank wählen.").into());
    }
    let mut c = connect(link, Some(db))?;
    let tables: Vec<(String, Option<u64>)> = c
        .exec_map("SELECT TABLE_NAME, TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME", (db,), |(n, r): (String, Option<u64>)| (n, r))
        .map_err(err_text)?;
    let mut out = Vec::new();
    for (name, estimate) in tables {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let cols: Vec<(String, String, String)> = c
            .exec_map("SELECT COLUMN_NAME, DATA_TYPE, COLUMN_KEY FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION", (db, &name), |r: (String, String, String)| r)
            .map_err(err_text)?;
        let ddl: Option<(String, String)> = c.query_first(format!("SHOW CREATE TABLE {}", q(&name))).map_err(err_text)?;
        // Kleine Tabellen genau zaehlen, grosse nur schaetzen (COUNT(*) kann dort sehr lange dauern)
        let rows = if estimate.unwrap_or(0) < 200_000 {
            c.query_first::<u64, _>(format!("SELECT COUNT(*) FROM {}", q(&name))).map_err(err_text)?.unwrap_or(0)
        } else {
            estimate.unwrap_or(0)
        };
        out.push(TableScan {
            name: name.clone(),
            origin: name,
            columns: cols.iter().map(|(n, t, _)| Col { name: n.clone(), ty: col_type(t), decl: None }).collect(),
            rows,
            pk: cols.iter().filter(|c| c.2 == "PRI").map(|c| c.0.clone()).collect(),
            auto_inc: false,
            fks: vec![],
            ddl: ddl.map(|(_, d)| mariadb_ddl(&d)),
        });
    }
    Ok(out)
}

fn val(v: Value) -> Val {
    match v {
        Value::NULL => Val::Null,
        Value::Int(i) => Val::Int(i),
        Value::UInt(u) => i64::try_from(u).map(Val::Int).unwrap_or_else(|_| Val::Text(u.to_string())),
        Value::Float(f) => Val::Real(f as f64),
        Value::Double(d) => Val::Real(d),
        Value::Bytes(b) => match String::from_utf8(b) {
            Ok(s) => Val::Text(s),
            Err(e) => Val::Bytes(e.into_bytes()),
        },
        other => crate::db::value_to_string(&other).map(Val::Text).unwrap_or(Val::Null),
    }
}

pub fn read(link: &Link, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let db = link.conf.database.as_str();
    let mut c = connect(link, Some(db))?;
    let res = c.query_iter(format!("SELECT * FROM {}.{}", q(db), q(&t.origin))).map_err(err_text)?;
    for row in res {
        let row = row.map_err(err_text)?;
        if !f(row.unwrap().into_iter().map(val).collect())? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_and_ddl() {
        assert_eq!(col_type("INT"), ColType::Int);
        assert_eq!(col_type("longblob"), ColType::Blob);
        assert_eq!(col_type("varchar"), ColType::Text);
        assert_eq!(col_type("decimal"), ColType::Double);
        assert_eq!(mariadb_ddl("a utf8mb4_0900_ai_ci b"), "a utf8mb4_unicode_ci b");
        assert_eq!(val(Value::UInt(u64::MAX)), Val::Text(u64::MAX.to_string()));
        assert_eq!(val(Value::Bytes(vec![0xFF])), Val::Bytes(vec![0xFF]));
        assert_eq!(val(Value::Bytes(b"x".to_vec())), Val::Text("x".into()));
    }
}
