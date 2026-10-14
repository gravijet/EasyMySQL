// Tabellen eines PostgreSQL-Servers lesen (auch ueber SSH-Tunnel und TLS).
// Jede Spalte wird als Text gelesen und anhand ihres Typs umgewandelt; die Typen werden nach MariaDB uebersetzt.

use super::*;
use crate::transfer::server::{Link, Tls};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub fn connect(link: &Link, database: Option<&str>) -> Result<::postgres::Client, String> {
    let c = &link.conf;
    let mut cfg = ::postgres::Config::new();
    cfg.host(&link.host).port(link.port).user(if c.user.is_empty() { "postgres" } else { &c.user }).dbname(database.filter(|d| !d.is_empty()).unwrap_or("postgres")).connect_timeout(Duration::from_secs(10));
    if !c.password.is_empty() {
        cfg.password(&c.password);
    }
    let r = if c.tls.on() {
        cfg.connect(tokio_postgres_rustls::MakeRustlsConnect::new((*crate::transfer::server::tls_config(c.tls)).clone()))
    } else {
        cfg.connect(::postgres::NoTls)
    };
    r.map_err(|e| {
        let mut text = e.to_string();
        if let Some(src) = std::error::Error::source(&e) {
            text = format!("{text}: {src}");
        }
        if c.tls == Tls::Off && text.contains("SSL") {
            text.push_str(&format!("\n{}", crate::i18n::text("Der Server verlangt eine verschlüsselte Verbindung (TLS einschalten).")));
        }
        text
    })
}

/// Datenbanken des Servers ohne Vorlagen
pub fn databases(link: &Link) -> Result<Vec<String>, String> {
    let mut c = connect(link, None)?;
    let rows = c.query("SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY 1", &[]).map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
}

/// Spalte: Name, Typ in PostgreSQL, ColType, SQL-Typ fuer MariaDB
struct PgCol {
    name: String,
    ty: ColType,
    decl: String,
    pg: String,
}

fn map_type(data_type: &str, udt: &str, len: Option<i32>, prec: Option<i32>, scale: Option<i32>) -> (ColType, String) {
    match data_type {
        "smallint" => (ColType::Int, "SMALLINT".into()),
        "integer" => (ColType::Int, "INT".into()),
        "bigint" => (ColType::BigInt, "BIGINT".into()),
        "numeric" | "decimal" => {
            let p = prec.unwrap_or(38).clamp(1, 65);
            let s = scale.unwrap_or(10).clamp(0, p.min(30));
            (ColType::Double, format!("DECIMAL({p},{s})"))
        }
        "real" => (ColType::Double, "FLOAT".into()),
        "double precision" => (ColType::Double, "DOUBLE".into()),
        "boolean" => (ColType::Bool, "TINYINT(1)".into()),
        "character varying" => match len {
            Some(n) if n <= 16000 => (ColType::Varchar(n as u32), format!("VARCHAR({n})")),
            _ => (ColType::LongText, "LONGTEXT".into()),
        },
        "character" => (ColType::Varchar(len.unwrap_or(1) as u32), format!("CHAR({})", len.unwrap_or(1).clamp(1, 255))),
        "text" => (ColType::LongText, "LONGTEXT".into()),
        "date" => (ColType::Date, "DATE".into()),
        "timestamp without time zone" | "timestamp with time zone" => (ColType::DateTime, "DATETIME(6)".into()),
        "time without time zone" | "time with time zone" => (ColType::Text, "TIME(6)".into()),
        "uuid" => (ColType::Varchar(36), "CHAR(36)".into()),
        "json" | "jsonb" => (ColType::Json, "JSON".into()),
        "bytea" => (ColType::Blob, "LONGBLOB".into()),
        "interval" => (ColType::Text, "VARCHAR(64)".into()),
        _ => {
            let _ = udt;
            (ColType::LongText, "LONGTEXT".into())
        }
    }
}

/// Ausdruck, der die Spalte als Text liefert
fn select_expr(c: &PgCol) -> String {
    let n = format!("\"{}\"", c.name.replace('"', "\"\""));
    match c.pg.as_str() {
        "bytea" => format!("encode({n}, 'hex')"),
        "timestamp with time zone" => format!("({n} AT TIME ZONE 'UTC')::text"),
        "time with time zone" => format!("{n}::time::text"),
        _ => format!("{n}::text"),
    }
}

fn parse_hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

fn value(c: &PgCol, text: Option<String>) -> Val {
    let Some(s) = text else { return Val::Null };
    match c.pg.as_str() {
        "boolean" => Val::Bool(s == "t" || s == "true"),
        "smallint" | "integer" | "bigint" => s.parse().map(Val::Int).unwrap_or(Val::Text(s)),
        "real" | "double precision" => s.parse().map(Val::Real).unwrap_or(Val::Text(s)),
        "bytea" => Val::Bytes(parse_hex(&s)),
        "json" | "jsonb" => Val::Json(s),
        _ => Val::Text(s),
    }
}

/// Name der Tabelle in MariaDB: Tabellen im Schema "public" behalten ihren Namen
fn local_name(schema: &str, table: &str) -> String {
    if schema == "public" { table.to_string() } else { format!("{schema}_{table}") }
}

pub fn scan(link: &Link, cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let db = link.conf.database.as_str();
    if db.is_empty() {
        return Err(crate::i18n::text("Bitte eine Datenbank wählen.").into());
    }
    let mut c = connect(link, Some(db))?;
    let tables = c
        .query(
            "SELECT table_schema, table_name FROM information_schema.tables WHERE table_type = 'BASE TABLE' AND table_schema NOT IN ('pg_catalog', 'information_schema') AND table_schema NOT LIKE 'pg\\_%' ORDER BY 1, 2",
            &[],
        )
        .map_err(|e| e.to_string())?;
    let fk_rows = c
        .query(
            "SELECT ns.nspname, cl.relname, rns.nspname, rcl.relname,
                    (SELECT array_agg(a.attname::text ORDER BY k.ord) FROM unnest(c.conkey) WITH ORDINALITY k(attnum, ord) JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum),
                    (SELECT array_agg(a.attname::text ORDER BY k.ord) FROM unnest(c.confkey) WITH ORDINALITY k(attnum, ord) JOIN pg_attribute a ON a.attrelid = c.confrelid AND a.attnum = k.attnum)
             FROM pg_constraint c
             JOIN pg_class cl ON cl.oid = c.conrelid JOIN pg_namespace ns ON ns.oid = cl.relnamespace
             JOIN pg_class rcl ON rcl.oid = c.confrelid JOIN pg_namespace rns ON rns.oid = rcl.relnamespace
             WHERE c.contype = 'f'",
            &[],
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for t in tables {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let (schema, table): (String, String) = (t.get(0), t.get(1));
        let cols = c
            .query(
                "SELECT column_name, data_type, udt_name, character_maximum_length, numeric_precision, numeric_scale, column_default
                 FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .map_err(|e| e.to_string())?;
        let pk_rows = c
            .query(
                "SELECT kcu.column_name FROM information_schema.table_constraints tc
                 JOIN information_schema.key_column_usage kcu ON kcu.constraint_name = tc.constraint_name AND kcu.table_schema = tc.table_schema AND kcu.table_name = tc.table_name
                 WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = $1 AND tc.table_name = $2 ORDER BY kcu.ordinal_position",
                &[&schema, &table],
            )
            .map_err(|e| e.to_string())?;
        let pk: Vec<String> = pk_rows.iter().map(|r| r.get(0)).collect();
        let mut pg_cols: Vec<PgCol> = Vec::new();
        let mut auto_inc = false;
        for r in &cols {
            let name: String = r.get(0);
            let data_type: String = r.get(1);
            let udt: String = r.get(2);
            let (len, prec, scale): (Option<i32>, Option<i32>, Option<i32>) = (r.get(3), r.get(4), r.get(5));
            let default: Option<String> = r.get(6);
            let (mut ty, mut decl) = map_type(&data_type, &udt, len, prec, scale);
            // Textspalten im Primaerschluessel muessen eine Laenge haben
            if pk.contains(&name) && !ty.indexable() {
                ty = ColType::Varchar(255);
                decl = "VARCHAR(255)".into();
            }
            if pk.len() == 1 && pk[0] == name && default.as_deref().is_some_and(|d| d.starts_with("nextval(")) && matches!(ty, ColType::Int | ColType::BigInt) {
                auto_inc = true;
            }
            pg_cols.push(PgCol { name, ty, decl, pg: data_type });
        }
        let qualified = format!("\"{}\".\"{}\"", schema.replace('"', "\"\""), table.replace('"', "\"\""));
        let rows: i64 = c.query_one(&format!("SELECT count(*) FROM {qualified}"), &[]).map_err(|e| e.to_string())?.get(0);
        let local = local_name(&schema, &table);
        let fks: Vec<ForeignKey> = fk_rows
            .iter()
            .filter(|r| r.get::<_, String>(0) == schema && r.get::<_, String>(1) == table)
            .map(|r| ForeignKey {
                columns: r.get::<_, Vec<String>>(4),
                ref_table: local_name(&r.get::<_, String>(2), &r.get::<_, String>(3)),
                ref_columns: r.get::<_, Vec<String>>(5),
            })
            .collect();
        out.push(TableScan {
            name: local,
            // Schema und Tabelle fuer `read`
            origin: format!("{schema}\u{1}{table}"),
            columns: pg_cols.iter().map(|p| Col { name: p.name.clone(), ty: p.ty, decl: Some(p.decl.clone()) }).collect(),
            rows: rows.max(0) as u64,
            pk,
            auto_inc,
            fks,
            ddl: None,
        });
        // Postgres-Typen fuer `read` merken
        let last = out.last_mut().unwrap();
        last.origin = format!("{}\u{1}{}", last.origin, pg_cols.iter().map(|p| p.pg.replace('\u{1}', " ")).collect::<Vec<_>>().join("\u{2}"));
    }
    Ok(out)
}

pub fn read(link: &Link, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let mut parts = t.origin.splitn(3, '\u{1}');
    let (schema, table, types) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let pg_types: Vec<&str> = types.split('\u{2}').collect();
    let cols: Vec<PgCol> = t
        .columns
        .iter()
        .zip(&pg_types)
        .map(|(c, pg)| PgCol { name: c.name.clone(), ty: c.ty, decl: c.decl.clone().unwrap_or_default(), pg: (*pg).to_string() })
        .collect();
    let mut c = connect(link, Some(&link.conf.database))?;
    let exprs: Vec<String> = cols.iter().map(select_expr).collect();
    let sql = format!("SELECT {} FROM \"{}\".\"{}\"", exprs.join(", "), schema.replace('"', "\"\""), table.replace('"', "\"\""));
    let params: Vec<&(dyn ::postgres::types::ToSql + Sync)> = Vec::new();
    let mut tx = c.transaction().map_err(|e| e.to_string())?;
    let iter = tx.query_raw(&sql, params).map_err(|e| e.to_string())?;
    let mut iter = iter;
    use ::postgres::fallible_iterator::FallibleIterator;
    while let Some(row) = iter.next().map_err(|e| e.to_string())? {
        let vals: Vec<Val> = cols.iter().enumerate().map(|(i, c)| value(c, row.get::<_, Option<String>>(i))).collect();
        if !f(vals)? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_mapping_and_values() {
        assert_eq!(map_type("integer", "int4", None, None, None).1, "INT");
        assert_eq!(map_type("numeric", "numeric", None, Some(10), Some(2)), (ColType::Double, "DECIMAL(10,2)".into()));
        assert_eq!(map_type("character varying", "varchar", Some(40), None, None).1, "VARCHAR(40)");
        assert_eq!(map_type("character varying", "varchar", None, None, None).1, "LONGTEXT");
        assert_eq!(map_type("timestamp with time zone", "timestamptz", None, None, None).1, "DATETIME(6)");
        assert_eq!(map_type("ARRAY", "_int4", None, None, None).1, "LONGTEXT");
        let col = |pg: &str, ty| PgCol { name: "c".into(), ty, decl: String::new(), pg: pg.into() };
        assert_eq!(value(&col("boolean", ColType::Bool), Some("t".into())), Val::Bool(true));
        assert_eq!(value(&col("integer", ColType::Int), Some("42".into())), Val::Int(42));
        assert_eq!(value(&col("bytea", ColType::Blob), Some("ff00".into())), Val::Bytes(vec![255, 0]));
        assert_eq!(value(&col("text", ColType::Text), None), Val::Null);
        assert_eq!(select_expr(&col("bytea", ColType::Blob)), "encode(\"c\", 'hex')");
        assert_eq!(local_name("public", "a"), "a");
        assert_eq!(local_name("app", "a"), "app_a");
    }
}
