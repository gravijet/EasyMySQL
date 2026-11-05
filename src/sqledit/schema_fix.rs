// Strukturvorschlaege aus statischem JDBC-SQL. Unklare Namen oder Typen ergeben keinen Fix.
use super::lint::{Kind, Tok, lex};
use crate::dev::java::source::{self, Span};
use std::collections::BTreeMap;

type Struktur = (Option<String>, String, Vec<(String, String)>);

fn name(t: &Tok) -> Option<String> {
    (t.is_name() && !t.text.is_empty()).then(|| t.text.clone())
}

fn jdbc_typ(methode: &str) -> Option<&'static str> {
    Some(match methode {
        "Int" | "Short" | "Byte" => "INT",
        "Long" => "BIGINT",
        "Boolean" => "BOOLEAN",
        "Double" | "Float" => "DOUBLE",
        "BigDecimal" => "DECIMAL(38, 10)",
        "String" => "TEXT",
        "Date" => "DATE",
        "Time" => "TIME",
        "Timestamp" => "DATETIME(6)",
        "Bytes" => "LONGBLOB",
        _ => return None,
    })
}

/// Nur geschlossene Literale, auch konstante Verkettungen und Textbloecke.
fn abfragen(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let lits = source::literals(&chars);
    let mut out = Vec::new();
    let mut i = 0;
    while i < lits.len() {
        let l = &lits[i];
        let mut sql: String = chars[l.content.0..l.content.1].iter().collect();
        let mut ende = l.content.1 + if l.block { 3 } else { 1 };
        let anfang = l.content.0 - if l.block { 3 } else { 1 };
        let mut sicher =
            !l.open && chars[..anfang].iter().rev().find(|c| !c.is_whitespace()) != Some(&'+');
        while let Some(n) = lits.get(i + 1) {
            let start = n.content.0 - if n.block { 3 } else { 1 };
            if chars[ende..start].iter().collect::<String>().trim() != "+" {
                break;
            }
            sql.extend(chars[n.content.0..n.content.1].iter());
            ende = n.content.1 + if n.block { 3 } else { 1 };
            sicher &= !n.open;
            i += 1;
        }
        sicher &= chars[ende.min(chars.len())..]
            .iter()
            .find(|c| !c.is_whitespace())
            != Some(&'+');
        // shortcut: Java-Escapes werden nicht interpretiert; solche Abfragen brauchen manuelle Pruefung.
        if sicher && !sql.contains('\\') && source::looks_like_sql(&sql) {
            out.push(sql);
        }
        i += 1;
    }
    out
}

/// Aus einfachen INSERT/SELECT/UPDATE/DELETE-Anweisungen benoetigte Spalten und JDBC-Typen sammeln.
/// Joins, Unterabfragen, Ausdruecke und unbekannte Typen bleiben beim Designer.
fn struktur(sql: &str, java: &str) -> Option<Struktur> {
    let chars: Vec<char> = sql.chars().collect();
    let l = lex(&chars);
    if !l.problems.is_empty() {
        return None;
    }
    let mut ts = l.toks;
    if ts.last().is_some_and(|t| t.kind == Kind::Semi) {
        ts.pop();
    }
    if ts
        .iter()
        .any(|t| t.kind == Kind::Semi || t.is_word("JOIN") || t.is_word("UNION"))
    {
        return None;
    }
    let insert = ts.first()?.is_word("INSERT");
    let update = ts.first()?.is_word("UPDATE");
    let select = ts.first()?.is_word("SELECT");
    let delete = ts.first()?.is_word("DELETE");
    if !insert && !update && !select && !delete {
        return None;
    }
    if ts.iter().filter(|t| t.is_word("SELECT")).count() > usize::from(select) {
        return None;
    }
    let at = if update {
        1
    } else {
        ts.iter()
            .position(|t| t.is_word(if insert { "INTO" } else { "FROM" }))?
            + 1
    };
    let first = name(ts.get(at)?)?;
    let (db, table, end) = if ts.get(at + 1).is_some_and(|t| t.kind == Kind::Dot) {
        (Some(first), name(ts.get(at + 2)?)?, at + 3)
    } else {
        (None, first, at + 1)
    };
    let mut cols: BTreeMap<String, Option<String>> = BTreeMap::new();
    if insert {
        if ts.get(end)?.kind != Kind::Open {
            return None;
        }
        let close = end + 1 + ts[end + 1..].iter().position(|t| t.kind == Kind::Close)?;
        let names: Vec<String> = ts[end + 1..close]
            .split(|t| t.kind == Kind::Comma)
            .map(|s| if s.len() == 1 { name(&s[0]) } else { None })
            .collect::<Option<_>>()?;
        if names.is_empty()
            || !ts.get(close + 1)?.is_word("VALUES")
            || ts.get(close + 2)?.kind != Kind::Open
        {
            return None;
        }
        let values = &ts[close + 3..];
        if values.last()?.kind != Kind::Close {
            return None;
        }
        let vals: Vec<_> = values[..values.len() - 1]
            .split(|t| t.kind == Kind::Comma)
            .collect();
        if names.len() != vals.len() {
            return None;
        }
        let mut parameter = 0;
        for (col, val) in names.into_iter().zip(vals) {
            let typ = if val.len() == 1 {
                let t = &val[0];
                match t.kind {
                    Kind::Str => Some("TEXT".to_string()),
                    Kind::Num => {
                        let n: String = chars[t.start..t.end].iter().collect();
                        if n.parse::<i64>().is_ok() {
                            Some("BIGINT".into())
                        } else if n.parse::<f64>().is_ok() {
                            Some("DOUBLE".into())
                        } else {
                            None
                        }
                    }
                    Kind::Op if t.text == "?" => {
                        parameter += 1;
                        // Nur ein PreparedStatement: damit gehoeren die Setter eindeutig zur Abfrage.
                        if java.matches("prepareStatement").count() == 1 {
                            setter_typ(java, parameter)
                        } else {
                            None
                        }
                    }
                    _ if t.is_word("TRUE") || t.is_word("FALSE") => Some("BOOLEAN".into()),
                    _ => None,
                }
            } else if val.len() == 2 && val[0].text == "-" && val[1].kind == Kind::Num {
                let n: String = chars[val[0].start..val[1].end].iter().collect();
                if n.parse::<i64>().is_ok() { Some("BIGINT".into()) }
                else if n.parse::<f64>().is_ok() { Some("DOUBLE".into()) } else { None }
            } else {
                None
            };
            let typ = typ.or_else(|| getter_typ(java, &col));
            if cols.insert(col, typ).is_some() {
                return None;
            }
        }
    } else {
        // shortcut: nur unqualifizierte Spalten ohne Alias/Ausdruck; komplexes SQL bleibt im Designer.
        let ranges = if select {
            vec![&ts[1..at - 1], &ts[end..]]
        } else {
            vec![&ts[end..]]
        };
        for range in ranges {
            for (i, t) in range.iter().enumerate() {
                if t.kind == Kind::Dot
                    || t.kind == Kind::Open
                    || t.kind == Kind::Close
                    || t.text == "*"
                {
                    return None;
                }
                if t.is_name() && (t.kind == Kind::Quoted || !super::lint::known_word(&t.upper())) {
                    let typ = getter_typ(java, &t.text);
                    cols.entry(t.text.clone()).or_insert(typ);
                } else if t.kind == Kind::Comma && range.get(i + 1).is_none() {
                    return None;
                }
            }
        }
    }
    if cols.is_empty() {
        return None;
    }
    let cols = cols
        .into_iter()
        .map(|(c, t)| (c, t.unwrap_or_default()))
        .collect();
    Some((db, table, cols))
}

fn setter_typ(java: &str, index: usize) -> Option<String> {
    let mut found = None;
    for (i, _) in java.match_indices(".set") {
        let rest = &java[i + 4..];
        let (method, args) = rest.split_once('(')?;
        let Some(typ) = jdbc_typ(method.trim()) else {
            continue;
        };
        if args.split(',').next()?.trim().parse::<usize>().ok() == Some(index) {
            if found.as_deref().is_some_and(|old| old != typ) {
                return None;
            }
            found = Some(typ.into());
        }
    }
    found
}

fn getter_typ(java: &str, col: &str) -> Option<String> {
    let mut found = None;
    for (i, _) in java.match_indices(".get") {
        let rest = &java[i + 4..];
        let Some((method, args)) = rest.split_once('(') else {
            continue;
        };
        let Some(typ) = jdbc_typ(method.trim()) else {
            continue;
        };
        if args.split(')').next()?.trim() == format!("\"{col}\"") {
            if found.as_deref().is_some_and(|old| old != typ) {
                return None;
            }
            found = Some(typ.into());
        }
    }
    found
}

/// Vorschlag nur bei eindeutiger Struktur aus einer Datei; nie DDL aus dem Projekt ausfuehren.
pub(crate) fn java_struktur(
    text: &str,
    db: Option<&str>,
    table: Option<&str>,
    column: Option<&str>,
) -> Option<Struktur> {
    if text.chars().count() > super::MAX_CHARS { return None; }
    let chars: Vec<char> = text.chars().collect();
    // Kommentare ausblenden, damit Beispiele keine Typinformationen liefern.
    let java: String = source::spans(&chars)
        .into_iter()
        .flat_map(|(kind, a, b)| {
            chars[a..b]
                .iter()
                .map(move |c| if kind == Span::Comment { ' ' } else { *c })
        })
        .collect();
    let sqls = abfragen(&java);
    let mut result: Option<Struktur> = None;
    for sql in &sqls {
        let Some((database, tab, cols)) = struktur(sql, &java) else {
            continue;
        };
        if table.is_some_and(|t| t != tab)
            || db.zip(database.as_deref()).is_some_and(|(a, b)| a != b)
        {
            continue;
        }
        if column.is_some_and(|c| !cols.iter().any(|(n, _)| n == c)) {
            continue;
        }
        if let Some((old_db, old_tab, old_cols)) = &mut result {
            if *old_tab != tab || *old_db != database {
                return None;
            }
            for (n, t) in cols {
                if let Some((_, old)) = old_cols.iter_mut().find(|(c, _)| *c == n) {
                    if old.is_empty() {
                        *old = t;
                    } else if !t.is_empty() && *old != t {
                        return None;
                    }
                } else {
                    old_cols.push((n, t));
                }
            }
        } else {
            result = Some((database, tab, cols));
        }
    }
    result.and_then(|(database, tab, mut cols)| {
        if let Some(c) = column {
            cols.retain(|(n, _)| n == c);
        }
        if cols.iter().any(|(_, t)| t.is_empty()) {
            return None;
        }
        Some((db.map(str::to_string).or(database), tab, cols))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_parameter_und_konstante_verkettung() {
        let java = r#"var p = c.prepareStatement("INSERT INTO `schule`.`kunde` (id, name) " + "VALUES (?, ?)");
            p.setLong(1, id); p.setString(2, name);"#;
        let (db, t, cols) = java_struktur(java, Some("schule"), Some("kunde"), None).unwrap();
        assert_eq!(db.as_deref(), Some("schule"));
        assert_eq!(t, "kunde");
        assert_eq!(
            cols,
            vec![
                ("id".into(), "BIGINT".into()),
                ("name".into(), "TEXT".into())
            ]
        );
        assert!(java_struktur(java, Some("andere"), Some("kunde"), None).is_none());
    }
    #[test]
    fn textblock_und_getter() {
        let java = "var q = \"\"\"\n SELECT id, name FROM kunde WHERE id = ?\n\"\"\"; r.getInt(\"id\"); r.getString(\"name\");";
        let (_, _, cols) = java_struktur(java, None, Some("kunde"), None).unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(
            java_struktur(java, None, None, Some("name")).unwrap().2,
            vec![("name".into(), "TEXT".into())]
        );
    }
    #[test]
    fn unsichere_strukturen_ablehnen() {
        for java in [
            r#"q = "INSERT INTO t (id) VALUES (?)";"#,
            r#"q = "SELECT * FROM t";"#,
            r#"q = "INSERT INTO t (id) VALUES (" + eingabe + ")";"#,
            r#"q = "INSERT INTO t (id) VALUES (1); DROP TABLE x";"#,
            r#"// q = "INSERT INTO t (id) VALUES (1)";"#,
            r#"q = "SELECT id FROM t JOIN x ON t.id=x.id"; r.getInt("id");"#,
        ] {
            assert!(
                java_struktur(java, None, Some("t"), None).is_none(),
                "{java}"
            );
        }
    }
}
