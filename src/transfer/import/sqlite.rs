// SQLite-Datei lesen (Tabellen, Spalten, Primaer- und Fremdschluessel, Zeilen).

use super::*;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

fn open(p: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())
}

fn sq(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn val(v: ValueRef) -> Val {
    match v {
        ValueRef::Null => Val::Null,
        ValueRef::Integer(i) => Val::Int(i),
        ValueRef::Real(f) => Val::Real(f),
        ValueRef::Text(t) => Val::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Val::Bytes(b.to_vec()),
    }
}

pub fn scan(path: &Path, cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let conn = open(path)?;
    let names: Vec<String> = {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name").map_err(|e| e.to_string())?;
        st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    let mut out = Vec::new();
    for name in names {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        // (Name, deklarierter Typ, Primaerschluessel-Position)
        let info: Vec<(String, String, i64)> = {
            let mut st = conn.prepare(&format!("PRAGMA table_info({})", sq(&name))).map_err(|e| e.to_string())?;
            st.query_map([], |r| Ok((r.get(1)?, r.get::<_, String>(2)?, r.get(5)?))).map_err(|e| e.to_string())?.collect::<Result<_, _>>().map_err(|e| e.to_string())?
        };
        if info.is_empty() {
            continue;
        }
        let mut infer = vec![Infer::default(); info.len()];
        let mut rows = 0u64;
        {
            let mut st = conn.prepare(&format!("SELECT * FROM {}", sq(&name))).map_err(|e| e.to_string())?;
            let mut q = st.query([]).map_err(|e| e.to_string())?;
            while let Some(r) = q.next().map_err(|e| e.to_string())? {
                for (i, inf) in infer.iter_mut().enumerate() {
                    inf.add(&val(r.get_ref(i).map_err(|e| e.to_string())?));
                }
                rows += 1;
                if rows.is_multiple_of(50_000) && cancel.load(Ordering::Relaxed) {
                    return Err("abgebrochen".into());
                }
            }
        }
        let names = unique_names(info.iter().map(|c| c.0.clone()).collect());
        let columns: Vec<Col> = info
            .iter()
            .zip(&infer)
            .zip(&names)
            .map(|(((_, decl, _), inf), n)| {
                let mut ty = column_type(inf);
                // BOOLEAN-Spalten mit 0/1 bleiben Wahrheitswerte
                if decl.to_uppercase().contains("BOOL") && ty == ColType::Int {
                    ty = ColType::Bool;
                }
                Col { name: n.clone(), ty, decl: None }
            })
            .collect();
        let mut pk: Vec<(i64, String)> = info.iter().zip(&names).filter(|(c, _)| c.2 > 0).map(|(c, n)| (c.2, n.clone())).collect();
        pk.sort();
        let pk: Vec<String> = pk.into_iter().map(|p| p.1).collect();
        let pk_ok = !pk.is_empty() && pk.iter().all(|p| columns.iter().find(|c| &c.name == p).is_some_and(|c| c.ty.indexable()));
        // "INTEGER PRIMARY KEY" ist in SQLite der Zeilenzaehler
        let auto_inc = pk_ok && pk.len() == 1 && info.iter().any(|c| c.2 > 0 && c.1.eq_ignore_ascii_case("INTEGER"));
        let fks: Vec<ForeignKey> = {
            let mut st = conn.prepare(&format!("PRAGMA foreign_key_list({})", sq(&name))).map_err(|e| e.to_string())?;
            // id, seq, table, from, to
            let list: Vec<(i64, i64, String, String, Option<String>)> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?;
            let mut by_id: Vec<ForeignKey> = Vec::new();
            let mut last = -1;
            for (id, _, table, from, to) in list {
                if id != last {
                    by_id.push(ForeignKey { columns: vec![], ref_table: table, ref_columns: vec![] });
                    last = id;
                }
                let f = by_id.last_mut().unwrap();
                f.columns.push(from);
                if let Some(t) = to {
                    f.ref_columns.push(t);
                }
            }
            by_id.retain(|f| f.columns.len() == f.ref_columns.len());
            by_id
        };
        out.push(TableScan { name: name.clone(), origin: name, columns, rows, pk: if pk_ok { pk } else { vec![] }, auto_inc, fks, ddl: None });
    }
    Ok(out)
}

pub fn read(path: &Path, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let conn = open(path)?;
    let mut st = conn.prepare(&format!("SELECT * FROM {}", sq(&t.origin))).map_err(|e| e.to_string())?;
    let n = t.columns.len();
    let mut q = st.query([]).map_err(|e| e.to_string())?;
    while let Some(r) = q.next().map_err(|e| e.to_string())? {
        let mut row = Vec::with_capacity(n);
        for i in 0..n {
            row.push(val(r.get_ref(i).map_err(|e| e.to_string())?));
        }
        if !f(row)? {
            break;
        }
    }
    Ok(())
}
