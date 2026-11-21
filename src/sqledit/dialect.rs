// Typische Stolpersteine mit MariaDB/MySQL erkennen und gleich korrigierbar machen:
// Schreibweisen anderer Datenbanken (SQL Server, Oracle, PostgreSQL, SQLite), Text ohne Anfuehrungszeichen
// in VALUES, fehlende Kommas, deutsches Datum, falsche Anzahl Werte, REFERENCES ohne Spalte.
// Positionen sind Zeichenindizes wie in `lint`; jede Korrektur ersetzt genau einen Bereich.

use super::lint::{Fix, Kind, Problem, Severity, Tok, known_word, quote_if_needed};
use crate::db::TableInfo;
use std::collections::HashMap;

pub(super) fn text(chars: &[char], a: usize, b: usize) -> String {
    chars[a.min(chars.len())..b.min(chars.len())].iter().collect()
}

fn problem(start: usize, end: usize, severity: Severity, message: String, fix: Option<(usize, usize, String)>) -> Problem {
    Problem { start, end, severity, message, fix: fix.map(|(start, end, text)| Fix { start, end, text }) }
}

/// SQL-Text in einfachen Anfuehrungszeichen
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Inhalt eines Text-Tokens (ohne Anfuehrungszeichen)
fn str_content(chars: &[char], t: &Tok) -> Option<String> {
    (t.kind == Kind::Str && t.end >= t.start + 2).then(|| text(chars, t.start + 1, t.end - 1))
}

/// "17.11.2026" bzw. "1.2.2026 14:30" -> "2026-11-17" bzw. "2026-02-01 14:30:00"
pub fn german_date(s: &str) -> Option<String> {
    let (date, time) = match s.trim().split_once(' ') {
        Some((d, t)) => (d, Some(t.trim())),
        None => (s.trim(), None),
    };
    // 17.11.2026, 17/11/2026 oder 17-11-2026 (Jahr hinten); bei / auch 11/17/2026, wenn eindeutig
    let sep = ['.', '/', '-'].into_iter().find(|c| date.contains(*c))?;
    let mut it = date.split(sep);
    let (d, m, y) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || d.is_empty() || m.is_empty() || d.len() > 2 || m.len() > 2 || y.len() != 4 {
        return None;
    }
    let (mut d, mut m, y): (u32, u32, u32) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
    if sep == '/' && m > 12 && d <= 12 {
        std::mem::swap(&mut d, &mut m);
    }
    if !(1..=31).contains(&d) || !(1..=12).contains(&m) {
        return None;
    }
    let mut out = format!("{y:04}-{m:02}-{d:02}");
    if let Some(t) = time {
        let parts: Vec<&str> = t.split(':').collect();
        if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty() || p.len() > 2 || !p.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        let n: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
        if n[0] > 23 || n[1] > 59 || n.get(2).is_some_and(|s| *s > 59) {
            return None;
        }
        out.push_str(&format!(" {:02}:{:02}:{:02}", n[0], n[1], n.get(2).copied().unwrap_or(0)));
    }
    Some(out)
}

/// Funktionen anderer Datenbanken -> MariaDB
const FUNCTIONS: &[(&str, &str)] = &[
    ("GETDATE", "NOW"),
    ("SYSDATETIME", "NOW"),
    ("GETUTCDATE", "UTC_TIMESTAMP"),
    ("LEN", "CHAR_LENGTH"),
    ("NVL", "IFNULL"),
    ("DATALENGTH", "LENGTH"),
    ("NEWID", "UUID"),
    ("CHARINDEX", "LOCATE"),
];

/// Datentypen anderer Datenbanken -> MariaDB (ohne Klammer danach)
const TYPES: &[(&str, &str)] = &[
    ("VARCHAR2", "VARCHAR"),
    ("NVARCHAR2", "VARCHAR"),
    ("MONEY", "DECIMAL(19,4)"),
    ("SMALLMONEY", "DECIMAL(10,4)"),
    ("CLOB", "LONGTEXT"),
    ("NCLOB", "LONGTEXT"),
    ("UNIQUEIDENTIFIER", "CHAR(36)"),
    ("BYTEA", "BLOB"),
    ("INT4", "INT"),
    ("INT8", "BIGINT"),
    ("FLOAT8", "DOUBLE"),
    ("TIMESTAMPTZ", "TIMESTAMP"),
    ("STRING", "VARCHAR(255)"),
];

/// Woerter, die allein als Wert in VALUES stehen duerfen
const VALUE_WORDS: &[&str] = &["NULL", "TRUE", "FALSE", "DEFAULT", "CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP", "LOCALTIME", "LOCALTIMESTAMP", "CURRENT_USER", "UNKNOWN"];

/// Woerter, die zeigen, dass ein VALUES-Eintrag ein Ausdruck und kein vergessener Text ist
const EXPRESSION_WORDS: &[&str] = &["NOT", "AND", "OR", "XOR", "IS", "NULL", "TRUE", "FALSE", "DEFAULT", "CASE", "WHEN", "THEN", "ELSE", "END", "INTERVAL", "BINARY", "DISTINCT", "SELECT", "DIV", "MOD", "LIKE", "IN", "BETWEEN"];

fn is_numeric_type(t: &str) -> bool {
    let b = t.to_ascii_lowercase();
    ["int", "tinyint", "smallint", "mediumint", "bigint", "decimal", "numeric", "float", "double", "real", "dec"].iter().any(|k| b.split(['(', ' ']).next() == Some(k))
}

fn is_date_type(t: &str) -> bool {
    let b = t.to_ascii_lowercase();
    b.starts_with("date") || b.starts_with("timestamp")
}

/// Ende der Klammergruppe, die bei `open` beginnt (Index des schliessenden Tokens)
pub(super) fn group_end(stmt: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (k, t) in stmt.iter().enumerate().skip(open) {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Eintraege einer Klammergruppe `(a, b, c)`: Bereiche der Tokens zwischen den Kommas der obersten Ebene
pub(super) fn items(stmt: &[Tok], open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = open + 1;
    for (k, t) in stmt.iter().enumerate().take(close).skip(open + 1) {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            Kind::Comma if depth == 0 => {
                out.push((start, k));
                start = k + 1;
            }
            _ => {}
        }
    }
    if start < close || !out.is_empty() {
        out.push((start, close));
    }
    out
}

pub(super) fn check(chars: &[char], stmt: &[Tok], tables: &HashMap<String, &TableInfo>, out: &mut Vec<Problem>) {
    let Some(first) = stmt.first() else { return };
    let fw = if first.kind == Kind::Word { first.upper() } else { String::new() };
    let ddl = matches!(fw.as_str(), "CREATE" | "ALTER");
    let dml = matches!(fw.as_str(), "SELECT" | "INSERT" | "REPLACE" | "UPDATE" | "DELETE" | "WITH");
    top(chars, stmt, &fw, out);
    for (i, t) in stmt.iter().enumerate() {
        let prev = i.checked_sub(1).map(|k| &stmt[k]);
        let next = stmt.get(i + 1);
        let after_dot = prev.is_some_and(|p| p.kind == Kind::Dot);
        // [Name] (SQL Server) -> `Name`
        if t.kind == Kind::Op && t.text == "["
            && let Some(close) = stmt[i + 1..].iter().position(|x| x.kind == Kind::Op && x.text == "]").map(|k| i + 1 + k)
            && close > i + 1
            && stmt[i + 1..close].iter().all(|x| matches!(x.kind, Kind::Word | Kind::Num))
        {
            let inner = text(chars, t.end, stmt[close].start).trim().replace('`', "");
            out.push(problem(
                t.start,
                stmt[close].end,
                Severity::Error,
                crate::tr_format!("Eckige Klammern um Namen gibt es nur in SQL Server; MariaDB nimmt `{inner}`.", "Square brackets around names are SQL Server syntax; MariaDB uses `{inner}`."),
                Some((t.start, stmt[close].end, format!("`{inner}`"))),
            ));
            continue;
        }
        // Typografische Anfuehrungszeichen (aus Word oder PDF kopiert) -> ' bzw. "
        if t.kind == Kind::Op && matches!(t.text.as_str(), "\u{2018}" | "\u{2019}" | "\u{201A}" | "\u{201C}" | "\u{201D}" | "\u{201E}") {
            let plain = if matches!(t.text.as_str(), "\u{201C}" | "\u{201D}" | "\u{201E}") { "\"" } else { "'" };
            out.push(problem(
                t.start,
                t.end,
                Severity::Error,
                crate::tr_format!("{} ist ein typografisches Anführungszeichen; SQL braucht {plain}.", "{} is a typographic quote; SQL needs {plain}.", t.text),
                Some((t.start, t.end, plain.into())),
            ));
            continue;
        }
        // Datum ohne Anfuehrungszeichen: 2008-03-01 rechnet MariaDB als 2008 - 3 - 1
        if t.kind == Kind::Num
            && t.end - t.start == 4
            && let [m, d2, m2, d] = [stmt.get(i + 1), stmt.get(i + 2), stmt.get(i + 3), stmt.get(i + 4)]
            && let (Some(m), Some(d2), Some(m2), Some(d)) = (m, d2, m2, d)
            && m.text == "-" && m2.text == "-" && d2.kind == Kind::Num && d.kind == Kind::Num
            && m.start == t.end && d2.start == m.end && m2.start == d2.end && d.start == m2.end
            && (1..=2).contains(&(d2.end - d2.start)) && (1..=2).contains(&(d.end - d.start))
        {
            let shown = text(chars, t.start, d.end);
            out.push(problem(
                t.start,
                d.end,
                Severity::Error,
                crate::tr_format!("Ein Datum braucht Anführungszeichen: '{shown}' (ohne rechnet MariaDB eine Subtraktion).", "A date needs quotes: '{shown}' (without them MariaDB subtracts)."),
                Some((t.start, d.end, format!("'{shown}'"))),
            ));
            continue;
        }
        if t.kind != Kind::Word || after_dot || next.is_some_and(|n| n.kind == Kind::Dot) {
            continue;
        }
        let u = t.upper();
        // Funktionen anderer Datenbanken
        if next.is_some_and(|n| n.kind == Kind::Open) && !ddl {
            let close = group_end(stmt, i + 1);
            let args = close.map_or(0, |c| items(stmt, i + 1, c).len());
            let target = FUNCTIONS.iter().find(|(f, _)| *f == u).map(|(_, m)| *m).or_else(|| (u == "ISNULL" && args == 2).then_some("IFNULL"));
            if let Some(m) = target {
                out.push(problem(
                    t.start,
                    t.end,
                    Severity::Error,
                    crate::tr_format!("{} gibt es in MariaDB nicht, dort heißt es {m}().", "{} does not exist in MariaDB; use {m}().", t.text),
                    Some((t.start, t.end, m.into())),
                ));
            }
            continue;
        }
        if u == "SYSDATE" && dml {
            out.push(problem(t.start, t.end, Severity::Error, crate::i18n::tr("SYSDATE braucht in MariaDB Klammern; üblich ist NOW().", "SYSDATE needs parentheses in MariaDB; NOW() is the usual way.").into(), Some((t.start, t.end, "NOW()".into()))));
            continue;
        }
        if !ddl {
            continue;
        }
        // Nur an der Stelle eines Datentyps: "(name TYP", ", name TYP", "ADD [COLUMN] name TYP"
        let type_pos = i >= 2
            && stmt[i - 1].is_name()
            && (matches!(stmt[i - 2].kind, Kind::Open | Kind::Comma) || ["ADD", "COLUMN", "MODIFY"].iter().any(|k| stmt[i - 2].is_word(k)));
        if type_pos {
            let group = next.filter(|n| n.kind == Kind::Open).and_then(|_| group_end(stmt, i + 1));
            let (end, replacement): (usize, Option<String>) = match u.as_str() {
                "NUMBER" => match group {
                    Some(c) => (stmt[c].end, Some(format!("DECIMAL{}", text(chars, stmt[i + 1].start, stmt[c].end)))),
                    None => (t.end, Some("INT".into())),
                },
                "DATETIME2" | "SMALLDATETIME" | "DATETIMEOFFSET" => (group.map_or(t.end, |c| stmt[c].end), Some("DATETIME".into())),
                "VARCHAR" | "NVARCHAR" | "VARBINARY" if group.is_some_and(|c| c == i + 3 && stmt[i + 2].is_word("MAX")) => {
                    (stmt[i + 3].end, Some(if u == "VARBINARY" { "LONGBLOB" } else { "LONGTEXT" }.into()))
                }
                _ => match TYPES.iter().find(|(k, _)| *k == u) {
                    Some((_, m)) if group.is_none() || !m.contains('(') => (t.end, Some((*m).into())),
                    _ => (t.end, None),
                },
            };
            if let Some(r) = replacement {
                let shown = text(chars, t.start, end);
                out.push(problem(
                    t.start,
                    end,
                    Severity::Error,
                    crate::tr_format!("Den Datentyp {shown} kennt MariaDB nicht; passend ist {r}.", "MariaDB does not know the type {shown}; use {r}.", ),
                    Some((t.start, end, r)),
                ));
                continue;
            }
        }
        // IDENTITY(1,1) / AUTOINCREMENT / AUTO INCREMENT -> AUTO_INCREMENT
        let auto_end = match u.as_str() {
            // GENERATED ALWAYS|BY DEFAULT AS IDENTITY (PostgreSQL, Oracle)
            "GENERATED" if stmt[i + 1..].iter().take(4).any(|x| x.is_word("IDENTITY")) => {
                let k = i + 1 + stmt[i + 1..].iter().position(|x| x.is_word("IDENTITY")).unwrap_or(0);
                Some(stmt.get(k + 1).filter(|n| n.kind == Kind::Open).and_then(|_| group_end(stmt, k + 1)).map_or(stmt[k].end, |c| stmt[c].end))
            }
            "IDENTITY" => Some(next.filter(|n| n.kind == Kind::Open).and_then(|_| group_end(stmt, i + 1)).map_or(t.end, |c| stmt[c].end)),
            "AUTOINCREMENT" => Some(t.end),
            "AUTO" if next.is_some_and(|n| n.is_word("INCREMENT")) => next.map(|n| n.end),
            _ => None,
        };
        if let Some(end) = auto_end {
            out.push(problem(
                t.start,
                end,
                Severity::Error,
                crate::tr_format!("In MariaDB heißt das AUTO_INCREMENT (statt {}).", "MariaDB calls this AUTO_INCREMENT (not {}).", text(chars, t.start, end)),
                Some((t.start, end, "AUTO_INCREMENT".into())),
            ));
            continue;
        }
        // REFERENCES tabelle ohne Spalte
        if u == "REFERENCES"
            && let Some(target) = next.filter(|n| n.is_name())
            && stmt.get(i + 2).is_none_or(|x| x.kind != Kind::Open && x.kind != Kind::Dot)
        {
            let cols = references_columns(stmt, i, &target.text, tables);
            if !cols.is_empty() {
                out.push(problem(
                    target.start,
                    target.end,
                    Severity::Error,
                    crate::tr_format!("Nach REFERENCES {} fehlt die Spalte in Klammern, z. B. ({}).", "REFERENCES {} needs the column in parentheses, e.g. ({}).", target.text, cols.join(", ")),
                    Some((target.end, target.end, format!(" ({})", cols.join(", ")))),
                ));
            }
        }
    }
    if dml {
        dates(chars, stmt, &fw, tables, out);
    }
    if matches!(fw.as_str(), "INSERT" | "REPLACE") {
        insert_values(chars, stmt, tables, out);
    }
    if fw == "CREATE" && stmt.iter().take(6).any(|t| t.is_word("TABLE")) {
        create_commas(stmt, out);
    }
}

/// SELECT TOP n ... (SQL Server) -> SELECT ... LIMIT n
fn top(chars: &[char], stmt: &[Tok], fw: &str, out: &mut Vec<Problem>) {
    if fw != "SELECT" {
        return;
    }
    let k = if stmt.get(1).is_some_and(|t| t.is_word("DISTINCT") || t.is_word("ALL")) { 2 } else { 1 };
    let Some(top) = stmt.get(k).filter(|t| t.is_word("TOP")) else { return };
    let (n, after) = match (stmt.get(k + 1), stmt.get(k + 2), stmt.get(k + 3)) {
        (Some(n), _, _) if n.kind == Kind::Num => (text(chars, n.start, n.end), k + 2),
        (Some(o), Some(n), Some(c)) if o.kind == Kind::Open && n.kind == Kind::Num && c.kind == Kind::Close => (text(chars, n.start, n.end), k + 4),
        _ => return,
    };
    let (Some(rest), Some(last)) = (stmt.get(after), stmt.last()) else { return };
    let has_limit = stmt.iter().any(|t| t.is_word("LIMIT"));
    let fix = (!has_limit).then(|| (top.start, last.end, format!("{} LIMIT {n}", text(chars, rest.start, last.end))));
    out.push(problem(
        top.start,
        stmt[after - 1].end,
        Severity::Error,
        crate::tr_format!("TOP gibt es nur in SQL Server; MariaDB schreibt LIMIT {n} ans Ende.", "TOP is SQL Server syntax; MariaDB puts LIMIT {n} at the end."),
        fix,
    ));
}

/// Spalten fuer `REFERENCES tabelle` ohne Klammer: Primaerschluessel der Tabelle, sonst die Spalten des
/// Fremdschluessels (gleichnamig, wie meist ueblich).
fn references_columns(stmt: &[Tok], at: usize, table: &str, tables: &HashMap<String, &TableInfo>) -> Vec<String> {
    if let Some(t) = tables.get(&table.to_lowercase()) {
        let pk = t.pk_columns();
        if !pk.is_empty() {
            return pk;
        }
    }
    // FOREIGN KEY (a, b) REFERENCES t
    if at >= 3 && stmt[at - 1].kind == Kind::Close {
        let mut k = at - 1;
        let mut names = Vec::new();
        while k > 0 {
            k -= 1;
            match stmt[k].kind {
                Kind::Open => break,
                Kind::Comma => {}
                _ if stmt[k].is_name() => names.insert(0, stmt[k].text.clone()),
                _ => return Vec::new(),
            }
        }
        if k > 0 && stmt[k - 1].is_word("KEY") {
            return names;
        }
        return Vec::new();
    }
    // Spalte mit REFERENCES direkt in der Definition: "pid INT REFERENCES personen"
    let mut k = at;
    let mut depth = 0i32;
    while k > 0 {
        k -= 1;
        match stmt[k].kind {
            Kind::Close => depth += 1,
            Kind::Open if depth == 0 => break,
            Kind::Open => depth -= 1,
            Kind::Comma if depth == 0 => break,
            _ => {}
        }
    }
    stmt.get(k + 1).filter(|t| t.is_name() && !known_word(&t.upper())).map(|t| vec![t.text.clone()]).unwrap_or_default()
}

/// Ziel einer INSERT-Anweisung: (Index des Tabellennamens, Index der VALUES-Woerter)
pub(super) fn insert_parts(stmt: &[Tok]) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut values = None;
    for (k, t) in stmt.iter().enumerate() {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ if depth == 0 && (t.is_word("VALUES") || t.is_word("VALUE")) => {
                values = Some(k);
                break;
            }
            _ if depth == 0 && t.is_word("SELECT") => return None,
            _ => {}
        }
    }
    let values = values?;
    let into = stmt[..values].iter().position(|t| t.is_word("INTO")).map_or(1, |k| k + 1);
    let mut table = into;
    // INSERT IGNORE INTO / LOW_PRIORITY ...
    while stmt.get(table).is_some_and(|t| t.kind == Kind::Word && known_word(&t.upper())) && table < values {
        table += 1;
    }
    if stmt.get(table + 1).is_some_and(|t| t.kind == Kind::Dot) {
        table += 2;
    }
    stmt.get(table).filter(|t| t.is_name()).map(|_| (table, values))
}

/// INSERT ... VALUES: Text ohne Anfuehrungszeichen, fehlendes Komma zwischen Zeilen, Anzahl der Werte,
/// Dezimalkomma in Zahlen.
fn insert_values(chars: &[char], stmt: &[Tok], tables: &HashMap<String, &TableInfo>, out: &mut Vec<Problem>) {
    let Some((ti, vi)) = insert_parts(stmt) else { return };
    let table_tok = &stmt[ti];
    let table = tables.get(&table_tok.text.to_lowercase()).copied();
    // Spaltenliste
    let explicit: Option<Vec<String>> = (stmt.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) && ti + 1 < vi)
        .then(|| stmt[ti + 2..vi].iter().filter(|t| t.is_name()).map(|t| t.text.clone()).collect());
    let columns: Option<Vec<&crate::db::ColumnInfo>> = table.map(|t| match &explicit {
        Some(names) => names.iter().filter_map(|n| t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(n))).collect(),
        None => t.columns.iter().filter(|c| !c.is_generated()).collect(),
    });
    let expected = explicit.as_ref().map(Vec::len).or(columns.as_ref().map(Vec::len));
    // Zeilen (Klammergruppen) einsammeln
    let mut tuples: Vec<(usize, usize)> = Vec::new();
    let mut k = vi + 1;
    while let Some(t) = stmt.get(k) {
        match t.kind {
            Kind::Open => {
                let Some(c) = group_end(stmt, k) else { break };
                if let Some(&(_, prev_close)) = tuples.last()
                    && prev_close + 1 == k
                {
                    let p = &stmt[prev_close];
                    out.push(problem(p.start, t.end, Severity::Error, crate::i18n::tr("Zwischen zwei Zeilen fehlt ein Komma.", "A comma is missing between two rows.").into(), Some((p.end, p.end, ",".into()))));
                }
                tuples.push((k, c));
                k = c + 1;
            }
            Kind::Comma => k += 1,
            _ => break,
        }
    }
    let mut counts = Vec::new();
    for &(open, close) in &tuples {
        let its = items(stmt, open, close);
        counts.push(its.len());
        for (n, &(a, b)) in its.iter().enumerate() {
            let toks = &stmt[a..b];
            let col = columns.as_ref().and_then(|c| c.get(n)).copied();
            // Text ohne Anfuehrungszeichen: nur Woerter, kein Ausdruck, keine Spalte der Tabelle
            let bare = !toks.is_empty()
                && toks.iter().all(|t| t.kind == Kind::Word && !EXPRESSION_WORDS.contains(&t.upper().as_str()))
                && !(toks.len() == 1 && (VALUE_WORDS.contains(&toks[0].upper().as_str()) || table.is_some_and(|t| t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&toks[0].text)))));
            if bare {
                let (s, e) = (toks[0].start, toks[toks.len() - 1].end);
                let raw = text(chars, s, e);
                out.push(problem(
                    s,
                    e,
                    Severity::Error,
                    crate::tr_format!("Text steht in Anführungszeichen: {}", "Text goes in quotes: {}", quoted(&raw)),
                    Some((s, e, quoted(&raw))),
                ));
                continue;
            }
            // '3,5' in einer Zahlenspalte
            if let ([t], Some(c)) = (toks, col)
                && is_numeric_type(&c.col_type)
                && let Some(v) = str_content(chars, t)
                && let Some((a, b)) = v.trim().split_once(',')
                && !a.is_empty()
                && a.trim_start_matches('-').bytes().all(|x| x.is_ascii_digit())
                && !b.is_empty()
                && b.bytes().all(|x| x.is_ascii_digit())
            {
                let num = format!("{a}.{b}");
                out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Kommazahlen schreibt SQL mit Punkt: {num}", "SQL writes decimals with a dot: {num}"), Some((t.start, t.end, num))));
            }
        }
    }
    let (Some(expected), Some(&(first_open, _))) = (expected, tuples.first()) else { return };
    if counts.iter().all(|c| *c == expected) {
        return;
    }
    // Ohne Spaltenliste und genau die automatisch vergebene ID weggelassen: Spaltenliste ergaenzen
    if explicit.is_none()
        && let Some(t) = table
        && counts.iter().all(|c| *c + 1 == expected)
        && let Some(auto) = t.columns.iter().find(|c| c.extra.to_ascii_lowercase().contains("auto_increment"))
    {
        let list: Vec<String> = t.columns.iter().filter(|c| !c.is_generated() && c.name != auto.name).map(|c| quote_if_needed(&c.name)).collect();
        out.push(problem(
            table_tok.start,
            table_tok.end,
            Severity::Error,
            crate::tr_format!(
                "{} hat {expected} Spalten, hier stehen {} Werte. {} vergibt MariaDB selbst – Spaltenliste ergänzen.",
                "{} has {expected} columns but there are {} values. MariaDB fills in {} itself – add the column list.",
                t.name, expected - 1, auto.name
            ),
            Some((table_tok.end, table_tok.end, format!(" ({})", list.join(", ")))),
        ));
        return;
    }
    for (&(open, close), &c) in tuples.iter().zip(&counts) {
        let its = items(stmt, open, close);
        // (9,50) bei einer Spalte zu wenig: Dezimalkomma in einer Kommazahl-Spalte
        if c == expected + 1
            && let Some(cols) = &columns
            && let Some(n) = (0..c - 1).find(|&n| {
                let (a, b) = its[n];
                let (a2, b2) = its[n + 1];
                b == a + 1 && b2 == a2 + 1 && stmt[a].kind == Kind::Num && stmt[a2].kind == Kind::Num && stmt[a2].start == stmt[a].end + 1
                    && (1..=2).contains(&(stmt[a2].end - stmt[a2].start))
                    && cols.get(n).is_some_and(|col| ["decimal", "numeric", "float", "double", "real"].iter().any(|k| col.col_type.to_ascii_lowercase().starts_with(k)))
            })
        {
            let (a, a2) = (its[n].0, its[n + 1].0);
            let num = format!("{}.{}", text(chars, stmt[a].start, stmt[a].end), text(chars, stmt[a2].start, stmt[a2].end));
            out.push(problem(stmt[a].start, stmt[a2].end, Severity::Error, crate::tr_format!("Kommazahlen schreibt SQL mit Punkt: {num}", "SQL writes decimals with a dot: {num}"), Some((stmt[a].start, stmt[a2].end, num))));
            continue;
        }
        // Zu wenige Werte bei einer Spaltenliste: der Rest bekommt den Standardwert
        // (nicht, wenn eher ein Komma fehlt: zwei Werte direkt hintereinander)
        let adjacent_values = its.iter().any(|&(a, b)| {
            stmt[a..b].windows(2).any(|w| matches!(w[0].kind, Kind::Str | Kind::Num | Kind::Quoted | Kind::Close | Kind::Var) && matches!(w[1].kind, Kind::Str | Kind::Num | Kind::Quoted | Kind::Var | Kind::Word))
        });
        if c < expected && c > 0 && explicit.is_some() && !adjacent_values {
            let missing = expected - c;
            let at = stmt[close].start;
            out.push(problem(
                stmt[open].start,
                stmt[close].end,
                Severity::Error,
                crate::tr_format!("{c} Wert(e), aber {expected} Spalte(n): der Rest bekommt DEFAULT.", "{c} value(s) but {expected} column(s): the rest gets DEFAULT."),
                Some((at, at, ", DEFAULT".repeat(missing))),
            ));
            continue;
        }
        if c != expected {
            out.push(problem(
                stmt[open].start,
                stmt[close].end,
                Severity::Error,
                crate::tr_format!("{c} Wert(e), aber {expected} Spalte(n).", "{c} value(s) but {expected} column(s)."),
                None,
            ));
        }
    }
    let _ = first_open;
}

/// '17.11.2026' -> '2026-11-17' (nicht in Textspalten, wenn die Spalte bekannt ist)
fn dates(chars: &[char], stmt: &[Tok], fw: &str, tables: &HashMap<String, &TableInfo>, out: &mut Vec<Problem>) {
    // In INSERT nur Werte, deren Spalte ein Datum ist (sofern bekannt)
    let text_cols: Vec<(usize, usize)> = if matches!(fw, "INSERT" | "REPLACE")
        && let Some((ti, vi)) = insert_parts(stmt)
        && let Some(t) = tables.get(&stmt[ti].text.to_lowercase())
    {
        let names: Option<Vec<String>> = (stmt.get(ti + 1).is_some_and(|x| x.kind == Kind::Open)).then(|| stmt[ti + 2..vi].iter().filter(|x| x.is_name()).map(|x| x.text.clone()).collect());
        let cols: Vec<&crate::db::ColumnInfo> = match &names {
            Some(n) => n.iter().filter_map(|n| t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(n))).collect(),
            None => t.columns.iter().filter(|c| !c.is_generated()).collect(),
        };
        let mut skip = Vec::new();
        let mut k = vi + 1;
        while let Some(o) = stmt.get(k).filter(|x| x.kind == Kind::Open) {
            let Some(c) = group_end(stmt, k) else { break };
            for (n, (a, b)) in items(stmt, k, c).into_iter().enumerate() {
                if cols.get(n).is_some_and(|col| !is_date_type(&col.col_type)) {
                    skip.push((stmt[a].start, stmt[b - 1].end));
                }
            }
            let _ = o;
            k = c + 1;
            if stmt.get(k).is_some_and(|x| x.kind == Kind::Comma) {
                k += 1;
            }
        }
        skip
    } else {
        Vec::new()
    };
    for t in stmt {
        let Some(v) = str_content(chars, t) else { continue };
        if text_cols.iter().any(|(a, b)| t.start >= *a && t.end <= *b) {
            continue;
        }
        if let Some(iso) = german_date(&v) {
            let q = chars[t.start];
            out.push(problem(
                t.start,
                t.end,
                Severity::Error,
                crate::tr_format!("MariaDB erwartet Datumsangaben als JJJJ-MM-TT: {q}{iso}{q}", "MariaDB expects dates as YYYY-MM-DD: {q}{iso}{q}"),
                Some((t.start, t.end, format!("{q}{iso}{q}"))),
            ));
        }
    }
}

/// CREATE TABLE: fehlendes Komma zwischen zwei Spalten ("id INT name VARCHAR(20)")
fn create_commas(stmt: &[Tok], out: &mut Vec<Problem>) {
    let Some(open) = stmt.iter().position(|t| t.kind == Kind::Open) else { return };
    let Some(close) = group_end(stmt, open) else { return };
    let mut depth = 0i32;
    for i in open + 1..close {
        let t = &stmt[i];
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ => {}
        }
        if depth != 0 || i < open + 2 {
            continue;
        }
        let prev = &stmt[i - 1];
        let next = &stmt[i + 1];
        let name = (t.kind == Kind::Word && !known_word(&t.upper())) || t.kind == Kind::Quoted;
        let next_type = next.kind == Kind::Word && (crate::sqlfmt::is_type(&next.text) || TYPES.iter().any(|(k, _)| next.is_word(k)));
        let ends_def = match prev.kind {
            Kind::Close | Kind::Num | Kind::Str => true,
            Kind::Word => {
                let p = prev.upper();
                crate::sqlfmt::is_type_upper(&p) || ["NULL", "KEY", "AUTO_INCREMENT", "UNIQUE", "UNSIGNED", "ZEROFILL", "CURRENT_TIMESTAMP", "TRUE", "FALSE"].contains(&p.as_str())
            }
            _ => false,
        };
        if name && next_type && ends_def {
            out.push(problem(t.start, t.end, Severity::Error, crate::i18n::tr("Vor dieser Spalte fehlt ein Komma.", "A comma is missing before this column.").into(), Some((prev.end, prev.end, ",".into()))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lint::{Env, apply_fixes, lint};
    use super::*;
    use crate::db::{ColumnInfo, Schema};

    fn col(name: &str, t: &str, extra: &str, key: &str) -> ColumnInfo {
        ColumnInfo { name: name.into(), col_type: t.into(), extra: extra.into(), key: key.into(), ..Default::default() }
    }

    fn schema() -> Schema {
        Schema {
            name: "person".into(),
            tables: vec![
                TableInfo {
                    name: "Personen".into(),
                    columns: vec![col("pid", "int(11)", "auto_increment", "PRI"), col("vorname", "varchar(100)", "", ""), col("nachname", "varchar(100)", "", ""), col("geboren", "date", "", "")],
                    ..Default::default()
                },
                TableInfo { name: "Produkt".into(), columns: vec![col("id", "int(11)", "", "PRI"), col("preis", "decimal(8,2)", "", "")], ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn fixed(sql: &str) -> String {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let mut t = sql.to_string();
        for _ in 0..3 {
            let (n, k) = apply_fixes(&t, &lint(&t, &env));
            t = n;
            if k == 0 {
                break;
            }
        }
        t
    }

    fn fixed_no_schema(sql: &str) -> String {
        apply_fixes(sql, &lint(sql, &Env { schema: None, databases: &[] })).0
    }

    #[test]
    fn german_dates() {
        assert_eq!(german_date("17.11.2026").as_deref(), Some("2026-11-17"));
        assert_eq!(german_date("1.2.2026 9:05").as_deref(), Some("2026-02-01 09:05:00"));
        assert_eq!(german_date("32.1.2026"), None);
        assert_eq!(german_date("24/12/2008").as_deref(), Some("2008-12-24"));
        assert_eq!(german_date("12/24/2008").as_deref(), Some("2008-12-24"));
        assert_eq!(german_date("24-12-2008").as_deref(), Some("2008-12-24"));
        assert_eq!(german_date("2008-12-24"), None);
        assert_eq!(german_date("1.2.3"), None);
        assert_eq!(german_date("v1.2.2026x"), None);
    }

    #[test]
    fn other_dialects_become_mariadb() {
        assert_eq!(fixed_no_schema("SELECT TOP 5 name FROM t ORDER BY name"), "SELECT name FROM t ORDER BY name LIMIT 5");
        assert_eq!(fixed_no_schema("SELECT GETDATE(), LEN(name), ISNULL(a, 0) FROM t"), "SELECT NOW(), CHAR_LENGTH(name), IFNULL(a, 0) FROM t");
        assert_eq!(fixed_no_schema("SELECT ISNULL(a) FROM t"), "SELECT ISNULL(a) FROM t");
        assert_eq!(fixed_no_schema("SELECT [Vor name] FROM [Kunde]"), "SELECT `Vor name` FROM `Kunde`");
        assert_eq!(
            fixed_no_schema("CREATE TABLE k (id INT IDENTITY(1,1) PRIMARY KEY, name VARCHAR2(50), betrag NUMBER(8,2), n NUMBER, t NVARCHAR(MAX), x INTEGER AUTOINCREMENT)"),
            "CREATE TABLE k (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50), betrag DECIMAL(8,2), n INT, t LONGTEXT, x INTEGER AUTO_INCREMENT)"
        );
        // Spalten, die so heissen, bleiben
        assert_eq!(fixed_no_schema("CREATE TABLE k (number INT, money VARCHAR(5))"), "CREATE TABLE k (number INT, money VARCHAR(5))");
    }

    #[test]
    fn dash_comment_needs_a_space() {
        assert_eq!(fixed_no_schema("--distinct heisst\nSELECT 1;\n  --x\n"), "-- distinct heisst\nSELECT 1;\n  -- x\n");
        assert_eq!(fixed_no_schema("SELECT 5--3"), "SELECT 5--3");
    }

    #[test]
    fn insert_values_are_repaired() {
        assert_eq!(fixed("INSERT INTO Personen (vorname, nachname) VALUES (Max, Muster)"), "INSERT INTO Personen (vorname, nachname) VALUES ('Max', 'Muster')");
        assert_eq!(fixed("INSERT INTO Personen (vorname, nachname) VALUES ('A', 'B') ('C', 'D')"), "INSERT INTO Personen (vorname, nachname) VALUES ('A', 'B'), ('C', 'D')");
        assert_eq!(
            fixed("INSERT INTO Personen VALUES ('Anna', 'Huber', '17.11.2008')"),
            "INSERT INTO Personen (vorname, nachname, geboren) VALUES ('Anna', 'Huber', '2008-11-17')"
        );
        assert_eq!(fixed("INSERT INTO Produkt (id, preis) VALUES (1, '3,50')"), "INSERT INTO Produkt (id, preis) VALUES (1, 3.50)");
        // Textspalte: Datum bleibt Text, NULL und DEFAULT sind Werte
        assert_eq!(fixed("INSERT INTO Personen (vorname, nachname, geboren) VALUES ('1.2.2000', NULL, DEFAULT)"), "INSERT INTO Personen (vorname, nachname, geboren) VALUES ('1.2.2000', NULL, DEFAULT)");
        // Zu wenige Werte: der Rest bekommt den Standardwert; zu viele bleiben eine Meldung ohne Korrektur
        assert_eq!(fixed("INSERT INTO Personen (vorname, nachname) VALUES ('A')"), "INSERT INTO Personen (vorname, nachname) VALUES ('A', DEFAULT)");
        let s = schema();
        let p = lint("INSERT INTO Personen (vorname) VALUES ('A', 'B', 'C')", &Env { schema: Some(&s), databases: &[] });
        assert!(p.iter().any(|p| p.message.contains('3') && p.fix.is_none()), "{p:?}");
    }

    #[test]
    fn create_table_slips() {
        assert_eq!(fixed_no_schema("CREATE TABLE t (id INT PRIMARY KEY name VARCHAR(20) NOT NULL preis DECIMAL(5,2))"), "CREATE TABLE t (id INT PRIMARY KEY, name VARCHAR(20) NOT NULL, preis DECIMAL(5,2))");
        assert_eq!(fixed("CREATE TABLE Email (pid INT, email VARCHAR(200), FOREIGN KEY (pid) REFERENCES Personen)"), "CREATE TABLE Email (pid INT, email VARCHAR(200), FOREIGN KEY (pid) REFERENCES Personen (pid))");
        assert_eq!(fixed_no_schema("CREATE TABLE Tel (nr INT REFERENCES Kunde)"), "CREATE TABLE Tel (nr INT REFERENCES Kunde (nr))");
        assert_eq!(fixed_no_schema("CREATE TABLE ok (id INT, CONSTRAINT fk FOREIGN KEY (id) REFERENCES a (id))"), "CREATE TABLE ok (id INT, CONSTRAINT fk FOREIGN KEY (id) REFERENCES a (id))");
    }
}
