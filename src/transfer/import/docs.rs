// MongoDB-Exporte lesen: JSON (Array, ein Objekt je Zeile, oder Objekt aus Arrays) und BSON (mongodump).
// Verschachtelte Werte werden als JSON-Spalte uebernommen, Mongo-Typen ($oid, $date, $numberLong, ...) aufgeloest.

use super::*;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

type Doc = Map<String, Value>;

/// Verbindet Datei und optionalen Schluessel in `TableScan::origin`
fn origin(path: &Path, key: &str) -> String {
    format!("{}\u{0}{key}", path.display())
}

fn split_origin(o: &str) -> (PathBuf, &str) {
    let (p, k) = o.split_once('\u{0}').unwrap_or((o, ""));
    (PathBuf::from(p), k)
}

// ---------------------------------------------------------------- BSON

fn cstr(b: &[u8], i: &mut usize) -> Option<String> {
    let end = b.get(*i..)?.iter().position(|&c| c == 0)?;
    let s = String::from_utf8_lossy(&b[*i..*i + end]).into_owned();
    *i += end + 1;
    Some(s)
}

fn le<const N: usize>(b: &[u8], i: &mut usize) -> Option<[u8; N]> {
    let a: [u8; N] = b.get(*i..*i + N)?.try_into().ok()?;
    *i += N;
    Some(a)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(A[(n >> 18) as usize & 63] as char);
        s.push(A[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    s
}

/// Ein BSON-Dokument ab `*i` (Laenge + Elemente) als JSON-Wert im "Extended JSON"-Stil
fn bson_doc(b: &[u8], i: &mut usize, array: bool) -> Option<Value> {
    let start = *i;
    let len = i32::from_le_bytes(le(b, i)?) as usize;
    let end = start.checked_add(len)?;
    if len < 5 || end > b.len() {
        return None;
    }
    let mut map = Doc::new();
    let mut list = Vec::new();
    while *i < end - 1 {
        let ty = *b.get(*i)?;
        *i += 1;
        let key = cstr(b, i)?;
        let v = match ty {
            0x01 => serde_json::Number::from_f64(f64::from_le_bytes(le(b, i)?)).map(Value::Number).unwrap_or(Value::Null),
            0x02 => {
                let n = i32::from_le_bytes(le(b, i)?) as usize;
                let s = b.get(*i..*i + n.checked_sub(1)?)?;
                *i += n;
                Value::String(String::from_utf8_lossy(s).into_owned())
            }
            0x03 => bson_doc(b, i, false)?,
            0x04 => bson_doc(b, i, true)?,
            0x05 => {
                let n = i32::from_le_bytes(le(b, i)?) as usize;
                let sub = *b.get(*i)?;
                let data = b.get(*i + 1..*i + 1 + n)?;
                *i += 1 + n;
                let mut o = Doc::new();
                o.insert("$binary".into(), serde_json::json!({"base64": base64(data), "subType": format!("{sub:02x}")}));
                Value::Object(o)
            }
            0x06 | 0x0A => Value::Null,
            0x07 => {
                let id = b.get(*i..*i + 12)?;
                *i += 12;
                serde_json::json!({"$oid": hex(id)})
            }
            0x08 => {
                let v = *b.get(*i)? != 0;
                *i += 1;
                Value::Bool(v)
            }
            0x09 => serde_json::json!({"$date": {"$numberLong": i64::from_le_bytes(le(b, i)?).to_string()}}),
            0x0B => {
                let pat = cstr(b, i)?;
                let opt = cstr(b, i)?;
                serde_json::json!({"$regularExpression": {"pattern": pat, "options": opt}})
            }
            0x0D | 0x0E => {
                let n = i32::from_le_bytes(le(b, i)?) as usize;
                let s = b.get(*i..*i + n.checked_sub(1)?)?;
                *i += n;
                Value::String(String::from_utf8_lossy(s).into_owned())
            }
            0x10 => Value::from(i32::from_le_bytes(le(b, i)?)),
            0x11 => Value::from(u64::from_le_bytes(le(b, i)?)),
            0x12 => Value::from(i64::from_le_bytes(le(b, i)?)),
            0x13 => {
                let d = b.get(*i..*i + 16)?;
                *i += 16;
                Value::String(hex(d))
            }
            0xFF | 0x7F => Value::Null,
            _ => return None,
        };
        if array {
            list.push(v);
        } else {
            map.insert(key, v);
        }
    }
    *i = end;
    Some(if array { Value::Array(list) } else { Value::Object(map) })
}

fn bson_docs(bytes: &[u8], f: &mut dyn FnMut(Doc) -> Result<bool, String>) -> Result<(), String> {
    let mut i = 0;
    while i + 5 <= bytes.len() {
        match bson_doc(bytes, &mut i, false) {
            Some(Value::Object(d)) => {
                if !f(d)? {
                    break;
                }
            }
            _ => return Err(crate::i18n::text("Die BSON-Datei ist beschädigt.").into()),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- JSON

/// Sammlungen einer JSON-Datei: (Schluessel, Dokumente). Schluessel leer = die Datei selbst.
fn json_collections(text: &str) -> Result<Vec<(String, Vec<Doc>)>, String> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let docs_of = |v: Value| -> Option<Vec<Doc>> {
        match v {
            Value::Array(a) if a.iter().all(Value::is_object) => Some(a.into_iter().filter_map(|x| if let Value::Object(o) = x { Some(o) } else { None }).collect()),
            Value::Object(o) => Some(vec![o]),
            _ => None,
        }
    };
    // Ein Wert (Array oder Objekt) ...
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        if let Value::Object(o) = &v
            && !o.is_empty()
            && o.values().all(|x| matches!(x, Value::Array(a) if a.iter().all(Value::is_object)))
        {
            return Ok(o.iter().map(|(k, v)| (k.clone(), docs_of(v.clone()).unwrap_or_default())).collect());
        }
        return docs_of(v).map(|d| vec![(String::new(), d)]).ok_or_else(|| crate::i18n::text("Keine Objekte in der JSON-Datei.").to_string());
    }
    // ... sonst ein Objekt je Zeile (NDJSON, mongoexport)
    let mut docs = Vec::new();
    for (n, line) in t.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(o)) => docs.push(o),
            Ok(_) => return Err(crate::tr_format!("Zeile {} ist kein Objekt.", "Line {} is not an object.", n + 1)),
            Err(e) => return Err(crate::tr_format!("Zeile {}: {e}", "Line {}: {e}", n + 1)),
        }
    }
    Ok(vec![(String::new(), docs)])
}

/// Dokumente einer Quelle (Datei und Schluessel) nacheinander
fn each(o: &str, f: &mut dyn FnMut(Doc) -> Result<bool, String>) -> Result<(), String> {
    let (path, key) = split_origin(o);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bson")) {
        return bson_docs(&bytes, f);
    }
    let text = String::from_utf8_lossy(&bytes);
    for (k, docs) in json_collections(&text)? {
        if k == key {
            for d in docs {
                if !f(d)? {
                    break;
                }
            }
            return Ok(());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- Zellen

fn iso_from_millis(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Tage seit 1970-01-01 in Datum (Algorithmus von Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{:03}", rem / 3600, rem % 3600 / 60, rem % 60, ms.rem_euclid(1000))
}

fn number_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn cell(v: &Value) -> Val {
    match v {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => n.as_i64().map(Val::Int).or_else(|| n.as_f64().filter(|_| !n.is_u64()).map(Val::Real)).unwrap_or_else(|| Val::Text(n.to_string())),
        Value::String(s) => Val::Text(s.clone()),
        Value::Array(_) => Val::Json(v.to_string()),
        Value::Object(o) => {
            if o.len() == 1 {
                let (k, x) = o.iter().next().unwrap();
                match k.as_str() {
                    "$oid" => return number_str(x).map(Val::Text).unwrap_or(Val::Null),
                    "$numberInt" | "$numberLong" => return number_str(x).and_then(|s| s.parse().ok()).map(Val::Int).unwrap_or(Val::Null),
                    "$numberDouble" => return number_str(x).and_then(|s| s.parse().ok()).map(Val::Real).unwrap_or(Val::Null),
                    "$numberDecimal" => return number_str(x).map(Val::Text).unwrap_or(Val::Null),
                    "$date" => {
                        return match x {
                            Value::String(s) => Val::Text(normalize_date(s)),
                            Value::Number(n) => n.as_i64().map(|ms| Val::Text(iso_from_millis(ms))).unwrap_or(Val::Null),
                            Value::Object(o) => o.get("$numberLong").and_then(number_str).and_then(|s| s.parse().ok()).map(|ms| Val::Text(iso_from_millis(ms))).unwrap_or(Val::Null),
                            _ => Val::Null,
                        };
                    }
                    "$uuid" => return number_str(x).map(Val::Text).unwrap_or(Val::Null),
                    _ => {}
                }
            }
            Val::Json(v.to_string())
        }
    }
}

// ---------------------------------------------------------------- Oeffentlich

pub fn scan(files: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    // Zuerst nur die Namen der Sammlungen, dann je Sammlung ein Durchlauf
    let mut parts: Vec<(String, String)> = Vec::new();
    for path in files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "import".into());
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bson")) {
            parts.push((stem, origin(path, "")));
            continue;
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        for (key, _) in json_collections(&text).map_err(|e| format!("{}: {e}", path.display()))? {
            parts.push((if key.is_empty() { stem.clone() } else { key.clone() }, origin(path, &key)));
        }
    }
    let mut out = Vec::new();
    for (name, o) in parts {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let mut names: Vec<String> = Vec::new();
        let mut index = std::collections::HashMap::<String, usize>::new();
        let mut infer: Vec<Infer> = Vec::new();
        let mut rows = 0u64;
        let mut ids = std::collections::HashSet::new();
        let mut id_unique = true;
        each(&o, &mut |d| {
            if let Some(id) = d.get("_id") {
                id_unique &= ids.insert(id.to_string());
            }
            for (k, v) in &d {
                let i = *index.entry(k.clone()).or_insert_with(|| {
                    names.push(k.clone());
                    infer.push(Infer::default());
                    names.len() - 1
                });
                infer[i].add(&cell(v));
            }
            rows += 1;
            Ok(!rows.is_multiple_of(50_000) || !cancel.load(Ordering::Relaxed))
        })?;
        // `_id` zuerst
        let mut order: Vec<usize> = (0..names.len()).collect();
        order.sort_by_key(|&i| names[i] != "_id");
        let cols = unique_names(order.iter().map(|&i| names[i].clone()).collect());
        out.push(TableScan {
            name,
            origin: o,
            // Die Spaltennamen der Quelle bleiben in `origin`-Reihenfolge erhalten: siehe `read`
            columns: order.iter().zip(cols).map(|(&i, name)| Col { name, ty: column_type(&infer[i]) }).collect(),
            rows,
            pk: if id_unique && !ids.is_empty() && ids.len() as u64 == rows { vec!["_id".into()] } else { vec![] },
            auto_inc: false,
            fks: vec![],
        });
        // Die Schluessel in Quellreihenfolge fuer `read` merken
        let last = out.last_mut().unwrap();
        last.origin = format!("{}\u{1}{}", last.origin, order.iter().map(|&i| names[i].replace('\u{1}', " ")).collect::<Vec<_>>().join("\u{1}"));
        // `_id` ist nur Primaerschluessel, wenn der Typ es erlaubt (und alle Werte vorhanden und eindeutig sind)
        if last.columns.iter().find(|c| c.name == "_id").is_some_and(|c| !c.ty.indexable()) {
            last.pk.clear();
        }
    }
    Ok(out)
}

pub fn read(t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let (o, keys) = t.origin.split_once('\u{1}').unwrap_or((&t.origin, ""));
    let keys: Vec<&str> = keys.split('\u{1}').collect();
    each(o, &mut |d| {
        let row = keys.iter().map(|k| d.get(*k).map(cell).unwrap_or(Val::Null)).collect();
        f(row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_json_cells() {
        assert_eq!(cell(&serde_json::json!({"$oid": "507f1f77bcf86cd799439011"})), Val::Text("507f1f77bcf86cd799439011".into()));
        assert_eq!(cell(&serde_json::json!({"$numberLong": "5000000000"})), Val::Int(5_000_000_000));
        assert_eq!(cell(&serde_json::json!({"$date": "2024-05-17T10:20:30.123Z"})), Val::Text("2024-05-17 10:20:30.123".into()));
        assert_eq!(cell(&serde_json::json!({"$date": {"$numberLong": "1715941230123"}})), Val::Text("2024-05-17 10:20:30.123".into()));
        assert_eq!(cell(&serde_json::json!({"a": [1, 2]})), Val::Json("{\"a\":[1,2]}".into()));
        assert_eq!(cell(&serde_json::json!(1.5)), Val::Real(1.5));
        assert_eq!(iso_from_millis(0), "1970-01-01 00:00:00.000");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
    }

    #[test]
    fn json_shapes() {
        let c = json_collections("[{\"a\":1},{\"a\":2}]").unwrap();
        assert_eq!((c.len(), c[0].1.len()), (1, 2));
        let c = json_collections("{\"a\":1}\n{\"a\":2}\n\n{\"a\":3}").unwrap();
        assert_eq!(c[0].1.len(), 3);
        let c = json_collections("{\"kunden\":[{\"id\":1}],\"orte\":[{\"x\":1},{\"x\":2}]}").unwrap();
        assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["kunden", "orte"]);
        assert!(json_collections("{\"a\":1}\n[1]").is_err());
    }

    #[test]
    fn bson_documents() {
        // {"_id": ObjectId, "n": int32 7, "s": "hi", "d": date, "arr": [1], "nested": {"x": true}}
        let mut body: Vec<u8> = Vec::new();
        body.extend([0x07]);
        body.extend(b"_id\0");
        body.extend([1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        body.extend([0x10]);
        body.extend(b"n\0");
        body.extend(7i32.to_le_bytes());
        body.extend([0x02]);
        body.extend(b"s\0");
        body.extend(3i32.to_le_bytes());
        body.extend(b"hi\0");
        body.extend([0x09]);
        body.extend(b"d\0");
        body.extend(1715941230123i64.to_le_bytes());
        body.extend([0x04]);
        body.extend(b"arr\0");
        let mut arr: Vec<u8> = vec![0x10];
        arr.extend(b"0\0");
        arr.extend(1i32.to_le_bytes());
        arr.push(0);
        body.extend(((arr.len() + 4) as i32).to_le_bytes());
        body.extend(&arr);
        body.extend([0x03]);
        body.extend(b"nested\0");
        let mut nested: Vec<u8> = vec![0x08];
        nested.extend(b"x\0");
        nested.push(1);
        nested.push(0);
        body.extend(((nested.len() + 4) as i32).to_le_bytes());
        body.extend(&nested);
        body.push(0);
        let mut doc = ((body.len() + 4) as i32).to_le_bytes().to_vec();
        doc.extend(&body);
        let mut two = doc.clone();
        two.extend(&doc);
        let mut n = 0;
        bson_docs(&two, &mut |d| {
            assert_eq!(cell(&d["_id"]), Val::Text("0102030405060708090a0b0c".into()));
            assert_eq!(cell(&d["n"]), Val::Int(7));
            assert_eq!(cell(&d["s"]), Val::Text("hi".into()));
            assert_eq!(cell(&d["d"]), Val::Text("2024-05-17 10:20:30.123".into()));
            assert_eq!(cell(&d["arr"]), Val::Json("[1]".into()));
            assert_eq!(cell(&d["nested"]), Val::Json("{\"x\":true}".into()));
            n += 1;
            Ok(true)
        })
        .unwrap();
        assert_eq!(n, 2);
        assert!(bson_docs(&doc[..doc.len() - 3], &mut |_| Ok(true)).is_err());
    }
}
