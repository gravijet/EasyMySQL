// Pruefen und Reparieren von Tabellen.

use crate::db::{ConnInfo, q};
use mysql::prelude::*;

#[derive(Default, Debug, Clone)]
pub struct CheckResult {
    pub checked: usize,
    /// (Datenbank.Tabelle, Meldung)
    pub problems: Vec<(String, String)>,
    pub repaired: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Alle Tabellen aller Datenbanken pruefen (und optional reparieren).
pub fn check_all(conn: &ConnInfo, repair: bool, log: &dyn Fn(String)) -> Result<CheckResult, String> {
    let mut c = mysql::Conn::new(conn.opts()).map_err(|e| e.to_string())?;
    let tables: Vec<(String, String, Option<String>, String)> = c
        .query(
            "SELECT TABLE_SCHEMA, TABLE_NAME, ENGINE, TABLE_TYPE FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA NOT IN ('information_schema','performance_schema','sys') \
             ORDER BY TABLE_SCHEMA, TABLE_NAME",
        )
        .map_err(|e| e.to_string())?;
    let mut res = CheckResult::default();
    for (db, t, engine, ttype) in tables {
        let full = format!("{}.{}", q(&db), q(&t));
        let name = format!("{db}.{t}");
        res.checked += 1;
        let rows: Result<Vec<(String, String, String, String)>, _> = c.query(format!("CHECK TABLE {full}"));
        let problem = match rows {
            Ok(rows) => {
                let bad: Vec<String> = rows
                    .iter()
                    .filter(|r| {
                        let (typ, msg) = (r.2.to_lowercase(), r.3.to_lowercase());
                        typ == "error" || (typ == "status" && msg != "ok" && !msg.contains("up to date"))
                    })
                    .map(|r| r.3.clone())
                    .collect();
                if bad.is_empty() { None } else { Some(bad.join("; ")) }
            }
            Err(e) => Some(e.to_string()),
        };
        let Some(msg) = problem else { continue };
        log(format!("Problem: {name}: {msg}"));
        res.problems.push((name.clone(), msg.clone()));
        if !repair {
            continue;
        }
        if ttype.contains("VIEW") {
            res.failed.push((name, crate::i18n::text("Sicht verweist auf fehlende Tabellen/Spalten – bitte Sicht neu anlegen").into()));
            continue;
        }
        let engine = engine.unwrap_or_default().to_uppercase();
        let sql = if engine == "INNODB" {
            // InnoDB: Tabelle neu aufbauen
            format!("ALTER TABLE {full} ENGINE=InnoDB")
        } else {
            format!("REPAIR TABLE {full} EXTENDED")
        };
        log(crate::tr_format!("Repariere {name} ...", "Repairing {name} ..."));
        let ok = match c.query::<mysql::Row, _>(&sql) {
            Ok(rows) => !rows.iter().any(|r| {
                let typ: String = r.get::<Option<String>, _>(2).flatten().unwrap_or_default().to_lowercase();
                typ == "error"
            }),
            Err(e) => {
                log(crate::tr_format!("  Fehler: {e}", "  Error: {e}"));
                false
            }
        };
        if ok {
            log(crate::tr_format!("  {name} repariert.", "  {name} repaired."));
            res.repaired.push(name);
        } else {
            res.failed.push((name, crate::i18n::text("konnte nicht repariert werden – bitte aus einer Sicherung wiederherstellen").into()));
        }
    }
    Ok(res)
}

/// Tabellen, die nicht absturzsicher sind (MyISAM).
pub fn myisam_tables(conn: &ConnInfo) -> Result<Vec<(String, String)>, String> {
    let mut c = mysql::Conn::new(conn.opts()).map_err(|e| e.to_string())?;
    c.query(
        "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.TABLES WHERE ENGINE = 'MyISAM' \
         AND TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys')",
    )
    .map_err(|e| e.to_string())
}

pub fn convert_to_innodb(conn: &ConnInfo, tables: &[(String, String)], log: &dyn Fn(String)) -> Result<usize, String> {
    let mut c = mysql::Conn::new(conn.opts()).map_err(|e| e.to_string())?;
    let mut n = 0;
    for (db, t) in tables {
        log(crate::tr_format!("Wandle {db}.{t} in InnoDB um ...", "Converting {db}.{t} to InnoDB ..."));
        match c.query_drop(format!("ALTER TABLE {}.{} ENGINE=InnoDB", q(db), q(t))) {
            Ok(_) => n += 1,
            Err(e) => log(crate::tr_format!("  Fehler: {e}", "  Error: {e}")),
        }
    }
    Ok(n)
}
