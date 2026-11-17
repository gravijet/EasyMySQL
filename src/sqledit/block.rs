// Block-Vorschlaege: ganze Anweisungen oder Bloecke, die sich aus dem Zusammenhang ergeben. Sie erscheinen grau
// hinter dem Cursor; Tab uebernimmt sie. Nur, was an der Stelle fast immer folgt:
// - nach CREATE TABLE: das INSERT fuer diese Tabelle (ohne AUTO_INCREMENT-Spalte)
// - in einer VALUES-Liste nach "),": die naechste Zeile nach dem Muster der vorigen (ID hochgezaehlt)
// - in leerem CREATE TABLE x ( ... ): Primaerschluessel nach der Namensregel der Datenbank
// - nach JOIN tabelle: die ON-Bedingung ueber den Fremdschluessel
// Die Cursorstelle im Vorschlag ist mit CURSOR markiert.

use super::dialect::{group_end, insert_parts, items, text};
use super::lint::{Env, Kind, Tok, lex, quote_if_needed};
use crate::db::Schema;

pub const CURSOR: char = '\u{1}';

pub fn block(src: &str, cursor: usize, env: &Env) -> Option<String> {
    let chars: Vec<char> = src.chars().collect();
    if cursor > chars.len() || chars.len() > super::lint::MAX_CHARS {
        return None;
    }
    let line_start = chars[..cursor].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
    let line_end = chars[cursor..].iter().position(|&c| c == '\n').map_or(chars.len(), |p| cursor + p);
    let before_on_line: String = chars[line_start..cursor].iter().collect();
    let after_on_line: String = chars[cursor..line_end].iter().collect();
    let toks = lex(&chars[..cursor]).toks;
    // Tokens der Anweisung, in der der Cursor steht
    let stmt_start = toks.iter().rposition(|t| t.kind == Kind::Semi).map_or(0, |p| p + 1);
    let stmt = &toks[stmt_start..];
    let fresh_line = before_on_line.trim().is_empty() && after_on_line.trim().is_empty();
    if fresh_line && stmt.is_empty() && stmt_start > 0 {
        // Neue Anweisung: was folgt auf die vorige?
        let prev_start = toks[..stmt_start - 1].iter().rposition(|t| t.kind == Kind::Semi).map_or(0, |p| p + 1);
        return after_create(&chars, &toks[prev_start..stmt_start - 1]);
    }
    if fresh_line && let Some(row) = next_row(&chars, stmt, env) {
        return Some(row);
    }
    if let Some(b) = create_body(&chars, stmt, cursor, &before_on_line, &after_on_line, env.schema) {
        return Some(b);
    }
    join_on(stmt, &chars, cursor, &after_on_line, env.schema)
}

/// Nach `CREATE TABLE x (...)`: `INSERT INTO x (spalten) VALUES ();`
fn after_create(chars: &[char], prev: &[Tok]) -> Option<String> {
    let (name, cols) = super::script::create_columns(chars, prev)?;
    if cols.is_empty() {
        return None;
    }
    let list: Vec<String> = cols.iter().map(|c| quote_if_needed(c)).collect();
    Some(format!("INSERT INTO {} ({})\nVALUES ({CURSOR});", quote_if_needed(&name), list.join(", ")))
}

/// In `INSERT ... VALUES (...),` auf neuer Zeile: die naechste Zeile nach dem Muster der vorigen
fn next_row(chars: &[char], stmt: &[Tok], env: &Env) -> Option<String> {
    if !stmt.first().is_some_and(|t| t.is_word("INSERT") || t.is_word("REPLACE")) || stmt.last()?.kind != Kind::Comma {
        return None;
    }
    let (ti, vi) = insert_parts(stmt)?;
    // letzte vollstaendige Zeile
    let close = stmt.len() - 2;
    if stmt[close].kind != Kind::Close {
        return None;
    }
    let open = (vi + 1..close).rev().find(|&k| stmt[k].kind == Kind::Open && group_end(stmt, k) == Some(close))?;
    let table = env.schema.and_then(|s| s.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&stmt[ti].text)));
    let cols: Vec<String> = match stmt.get(ti + 1) {
        Some(o) if o.kind == Kind::Open => {
            let c = group_end(stmt, ti + 1)?;
            items(stmt, ti + 1, c).iter().map(|&(a, _)| stmt[a].text.to_lowercase()).collect()
        }
        _ => table.map(|t| t.columns.iter().map(|c| c.name.to_lowercase()).collect()).unwrap_or_default(),
    };
    let mut out = Vec::new();
    let mut cursor_set = false;
    for (k, &(a, b)) in items(stmt, open, close).iter().enumerate() {
        if b <= a {
            return None;
        }
        let one = &stmt[a..b];
        let raw = text(chars, one[0].start, one[b - a - 1].end);
        let col = cols.get(k).map(String::as_str).unwrap_or("");
        let is_key = table.and_then(|t| t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(col))).is_some_and(|c| c.is_pk())
            || (k == 0 && (col.is_empty() || col == "id" || col.ends_with("id") || col.ends_with("nr")));
        let v = match one {
            [t] if t.kind == Kind::Num && is_key => raw.parse::<i64>().map(|n| (n + 1).to_string()).unwrap_or(raw),
            [t] if t.kind == Kind::Str => {
                if cursor_set {
                    "''".into()
                } else {
                    cursor_set = true;
                    format!("'{CURSOR}'")
                }
            }
            _ => raw,
        };
        out.push(v);
    }
    if out.is_empty() {
        return None;
    }
    let row = format!("({})", out.join(", "));
    Some(if cursor_set { row } else { format!("{row}{CURSOR}") })
}

/// Namensregel der Primaerschluessel in der Datenbank: id, <tabelle>id, <tabelle>_id, <tabelle>ID ...
fn pk_name(table: &str, schema: Option<&Schema>) -> String {
    let mut votes: Vec<(String, usize)> = Vec::new();
    for t in schema.map(|s| s.tables.as_slice()).unwrap_or_default() {
        let pk = t.pk_columns();
        let [pk] = pk.as_slice() else { continue };
        let rule = if pk.eq_ignore_ascii_case("id") {
            pk.clone()
        } else if pk.len() > t.name.len() && pk[..t.name.len()].eq_ignore_ascii_case(&t.name) {
            format!("*{}", &pk[t.name.len()..])
        } else {
            continue;
        };
        match votes.iter_mut().find(|(r, _)| *r == rule) {
            Some((_, n)) => *n += 1,
            None => votes.push((rule, 1)),
        }
    }
    match votes.into_iter().max_by_key(|(_, n)| *n).map(|(r, _)| r) {
        Some(r) if r.starts_with('*') => format!("{table}{}", &r[1..]),
        Some(r) => r,
        None => "id".into(),
    }
}

/// Leeres `CREATE TABLE x (` …`)`: Primaerschluessel vorschlagen
fn create_body(chars: &[char], stmt: &[Tok], cursor: usize, before: &str, after: &str, schema: Option<&Schema>) -> Option<String> {
    let tk = stmt.iter().take(4).position(|t| t.is_word("TABLE"))?;
    if !stmt.first()?.is_word("CREATE") {
        return None;
    }
    let mut k = tk + 1;
    if stmt.get(k).is_some_and(|t| t.is_word("IF")) {
        k += 3;
    }
    let name = stmt.get(k).filter(|t| t.is_name())?;
    let open = stmt.get(k + 1).filter(|t| t.kind == Kind::Open)?;
    if stmt.len() != k + 2 || chars[open.end..cursor].iter().any(|c| !c.is_whitespace()) {
        return None;
    }
    if !after.trim().is_empty() && after.trim() != ")" && after.trim() != ");" {
        return None;
    }
    let pk = quote_if_needed(&pk_name(&name.text, schema));
    let def = format!("{pk} INT AUTO_INCREMENT PRIMARY KEY,");
    if chars[open.end..cursor].contains(&'\n') {
        // schon auf eigener, eingerueckter Zeile
        let indent: String = before.chars().take_while(|c| c.is_whitespace()).collect();
        let indent = if indent.is_empty() { "    ".to_string() } else { indent };
        Some(format!("{def}\n{indent}{CURSOR}"))
    } else {
        let close = if after.trim().is_empty() { ");" } else { "" };
        Some(format!("\n    {def}\n    {CURSOR}\n{close}"))
    }
}

/// `... JOIN tabelle [alias] ` -> `ON alias.fk = andere.pk` ueber den Fremdschluessel
fn join_on(stmt: &[Tok], chars: &[char], cursor: usize, after: &str, schema: Option<&Schema>) -> Option<String> {
    let schema = schema?;
    if !after.trim().is_empty() || !chars.get(cursor.checked_sub(1)?).is_some_and(|c| *c == ' ') {
        return None;
    }
    // Tabellen der Anweisung mit Alias: (Tabelle, Name im SQL)
    let mut refs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < stmt.len() {
        if (stmt[i].is_word("FROM") || stmt[i].is_word("JOIN")) && stmt.get(i + 1).is_some_and(Tok::is_name) {
            let t = stmt[i + 1].text.clone();
            let mut j = i + 2;
            if stmt.get(j).is_some_and(|x| x.is_word("AS")) {
                j += 1;
            }
            let alias = stmt.get(j).filter(|x| x.is_name() && !super::lint::known_word(&x.upper())).map(|x| x.text.clone());
            refs.push((t.clone(), alias.unwrap_or(t)));
            i = j;
        } else {
            i += 1;
        }
    }
    // Der letzte Eintrag muss das gerade getippte JOIN sein (danach nichts mehr)
    let last_join = stmt.iter().rposition(|t| t.is_word("JOIN"))?;
    let tail = &stmt[last_join + 1..];
    if tail.is_empty() || tail.len() > 3 || tail.iter().any(|t| !t.is_name() || t.is_word("ON") || t.is_word("USING")) {
        return None;
    }
    let (new_table, new_name) = refs.last()?.clone();
    for (t, n) in refs[..refs.len() - 1].iter().rev() {
        for f in &schema.fks {
            let (child, child_name, parent, parent_name) = if f.table.eq_ignore_ascii_case(&new_table) && f.ref_table.eq_ignore_ascii_case(t) {
                (f, &new_name, t, n)
            } else if f.table.eq_ignore_ascii_case(t) && f.ref_table.eq_ignore_ascii_case(&new_table) {
                (f, n, &new_table, &new_name)
            } else {
                continue;
            };
            let _ = parent;
            let conds: Vec<String> = child
                .columns
                .iter()
                .zip(&child.ref_columns)
                .map(|(c, r)| format!("{}.{} = {}.{}", quote_if_needed(child_name), quote_if_needed(c), quote_if_needed(parent_name), quote_if_needed(r)))
                .collect();
            if !conds.is_empty() {
                return Some(format!("ON {}{CURSOR}", conds.join(" AND ")));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, ForeignKey, Schema, TableInfo};

    fn schema() -> Schema {
        let col = |n: &str, key: &str| ColumnInfo { name: n.into(), col_type: "int".into(), key: key.into(), ..Default::default() };
        Schema {
            name: "s".into(),
            tables: vec![
                TableInfo { name: "Klasse".into(), columns: vec![col("KlasseID", "PRI"), col("name", "")], ..Default::default() },
                TableInfo { name: "Schueler".into(), columns: vec![col("SchuelerID", "PRI"), col("KlasseID", "")], ..Default::default() },
            ],
            fks: vec![ForeignKey { table: "Schueler".into(), columns: vec!["KlasseID".into()], ref_table: "Klasse".into(), ref_columns: vec!["KlasseID".into()], ..Default::default() }],
            ..Default::default()
        }
    }

    fn at_end(sql: &str) -> Option<String> {
        let s = schema();
        block(sql, sql.chars().count(), &Env { schema: Some(&s), databases: &[] }).map(|b| b.replace(CURSOR, "|"))
    }

    #[test]
    fn insert_follows_create_table() {
        assert_eq!(
            at_end("CREATE TABLE Person (\n    pid INT AUTO_INCREMENT PRIMARY KEY,\n    name VARCHAR(50) NOT NULL,\n    plz CHAR(4)\n);\n\n").as_deref(),
            Some("INSERT INTO Person (name, plz)\nVALUES (|);")
        );
        assert_eq!(at_end("SELECT 1;\n"), None);
    }

    #[test]
    fn next_values_row_follows_the_pattern() {
        assert_eq!(at_end("INSERT INTO t (id, name, ort, alter_j) VALUES\n(1, 'Max', 'Wien', 17),\n").as_deref(), Some("(2, '|', '', 17)"));
        assert_eq!(at_end("INSERT INTO t (a) VALUES (1);\n"), None);
    }

    #[test]
    fn create_table_starts_with_the_key_of_the_house_style() {
        assert_eq!(at_end("CREATE TABLE Lehrer (").as_deref(), Some("\n    LehrerID INT AUTO_INCREMENT PRIMARY KEY,\n    |\n);"));
        assert_eq!(at_end("CREATE TABLE Lehrer (\n    ").as_deref(), Some("LehrerID INT AUTO_INCREMENT PRIMARY KEY,\n    |"));
        let sql = "CREATE TABLE Lehrer (";
        let s = schema();
        let with_close = format!("{sql})");
        assert_eq!(block(&with_close, sql.chars().count(), &Env { schema: Some(&s), databases: &[] }).map(|b| b.replace(CURSOR, "|")).as_deref(), Some("\n    LehrerID INT AUTO_INCREMENT PRIMARY KEY,\n    |\n"));
        assert_eq!(pk_name("x", None), "id");
    }

    #[test]
    fn join_gets_its_condition() {
        assert_eq!(at_end("SELECT * FROM Klasse k JOIN Schueler s ").as_deref(), Some("ON s.KlasseID = k.KlasseID|"));
        assert_eq!(at_end("SELECT * FROM Schueler JOIN Klasse ").as_deref(), Some("ON Schueler.KlasseID = Klasse.KlasseID|"));
        assert_eq!(at_end("SELECT * FROM Klasse k JOIN Schueler s ON "), None);
    }
}
