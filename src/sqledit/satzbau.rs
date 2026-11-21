// Satzbau einer Anweisung: Teile in falscher Reihenfolge (ORDER BY vor WHERE), vergessene Woerter
// (SET, VALUES, BY, FROM-Klammer), doppelte Schluesselwoerter, Schreibweisen anderer Datenbanken, die
// den Aufbau betreffen (x::TEXT, DATEDIFF(day, …), || zum Verketten), Spalten ohne Datentyp.
// Wie `dialect`: Positionen sind Zeichenindizes, jede Korrektur ersetzt genau einen Bereich.

use super::dialect::{group_end, insert_parts, items, text};
use super::lint::{Fix, Kind, Problem, Severity, Tok, known_word};

fn problem(start: usize, end: usize, severity: Severity, message: String, fix: (usize, usize, String)) -> Problem {
    Problem { start, end, severity, message, fix: Some(Fix { start: fix.0, end: fix.1, text: fix.2 }) }
}

/// Klammertiefe vor jedem Token
fn depths(stmt: &[Tok]) -> Vec<i32> {
    let mut d = 0;
    stmt.iter()
        .map(|t| {
            let before = d;
            match t.kind {
                Kind::Open => d += 1,
                Kind::Close => d -= 1,
                _ => {}
            }
            before
        })
        .collect()
}

/// Ein einzelner Wert (Zahl, Text, Variable, NULL, Name)
fn is_value(t: &Tok) -> bool {
    matches!(t.kind, Kind::Num | Kind::Str | Kind::Var | Kind::Quoted) || t.kind == Kind::Word && (t.is_word("NULL") || !known_word(&t.upper()))
}

/// Woerter, die zusammengeschrieben wurden
const JOINED: &[(&str, &str)] = &[
    ("NOTNULL", "NOT NULL"),
    ("PRIMARYKEY", "PRIMARY KEY"),
    ("FOREIGNKEY", "FOREIGN KEY"),
    ("ORDERBY", "ORDER BY"),
    ("GROUPBY", "GROUP BY"),
    ("INSERTINTO", "INSERT INTO"),
    ("DELETEFROM", "DELETE FROM"),
    ("LEFTJOIN", "LEFT JOIN"),
    ("INNERJOIN", "INNER JOIN"),
    ("ISNOTNULL", "IS NOT NULL"),
];

/// Schluesselwoerter, die nie zweimal hintereinander stehen
const NEVER_TWICE: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INTO", "VALUES", "AND", "OR", "BY", "SET", "JOIN", "ON", "AS", "ORDER", "GROUP", "INSERT", "UPDATE",
    "DELETE", "DISTINCT", "HAVING", "LIMIT", "CREATE", "DROP",
];

/// Einheiten von SQL Server bei DATEDIFF/DATEADD -> MariaDB
fn unit(word: &str) -> Option<&'static str> {
    Some(match word.to_ascii_uppercase().as_str() {
        "DAY" | "DD" | "D" | "DAYS" => "DAY",
        "MONTH" | "MM" | "M" | "MONTHS" => "MONTH",
        "YEAR" | "YY" | "YYYY" | "YEARS" => "YEAR",
        "HOUR" | "HH" | "HOURS" => "HOUR",
        "MINUTE" | "MI" | "N" | "MINUTES" => "MINUTE",
        "SECOND" | "SS" | "S" | "SECONDS" => "SECOND",
        "WEEK" | "WK" | "WW" | "WEEKS" => "WEEK",
        "QUARTER" | "QQ" | "Q" => "QUARTER",
        _ => return None,
    })
}

/// Datentyp fuer eine Spalte, bei der er vergessen wurde (nach dem Namen geraten)
pub(super) fn guess_type(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if n == "id" || n.ends_with("_id") || n.ends_with("id") && n.len() > 2 && n[..n.len() - 2].ends_with('_') || has(&["anzahl", "menge", "stueck", "stück", "alter", "punkte", "count", "jahr"]) {
        "INT"
    } else if has(&["preis", "betrag", "gehalt", "lohn", "summe", "kosten", "price", "amount", "total"]) {
        "DECIMAL(10,2)"
    } else if has(&["uhrzeit"]) {
        "TIME"
    } else if has(&["zeitpunkt", "erstellt", "geaendert", "created", "updated", "timestamp"]) {
        "DATETIME"
    } else if has(&["datum", "date", "geburt", "tag"]) {
        "DATE"
    } else {
        "VARCHAR(255)"
    }
}

pub(super) fn check(chars: &[char], stmt: &[Tok], out: &mut Vec<Problem>) {
    let Some(first) = stmt.first() else { return };
    let fw = if first.kind == Kind::Word { first.upper() } else { String::new() };
    let depth = depths(stmt);
    let tr = crate::i18n::tr;
    let dml = matches!(fw.as_str(), "SELECT" | "WITH" | "UPDATE" | "DELETE" | "INSERT" | "REPLACE");
    // Platzhalter-Korrekturen nur beim Lesen: in DELETE/UPDATE wuerden sie mehr Zeilen treffen
    let reading = matches!(fw.as_str(), "SELECT" | "WITH");
    for (i, t) in stmt.iter().enumerate() {
        let prev = i.checked_sub(1).map(|k| &stmt[k]);
        let next = stmt.get(i + 1);
        let after_dot = prev.is_some_and(|p| p.kind == Kind::Dot);
        if t.kind == Kind::Word && !after_dot && next.is_none_or(|n| n.kind != Kind::Dot && n.kind != Kind::Open) {
            let u = t.upper();
            // NOTNULL -> NOT NULL
            if let Some((_, w)) = JOINED.iter().find(|(k, _)| *k == u) {
                out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Hier fehlt ein Leerzeichen: {w}", "A space is missing here: {w}"), (t.start, t.end, (*w).into())));
                continue;
            }
            // FROM FROM
            if let Some(n) = next.filter(|n| n.kind == Kind::Word && n.upper() == u && NEVER_TWICE.contains(&u.as_str())) {
                out.push(problem(n.start, n.end, Severity::Error, crate::tr_format!("{u} steht doppelt.", "{u} appears twice."), (t.end, n.end, String::new())));
                continue;
            }
            // GROUP klasse -> GROUP BY klasse
            if (u == "GROUP" || u == "ORDER")
                && matches!(fw.as_str(), "SELECT" | "WITH" | "UPDATE" | "DELETE")
                && prev.is_none_or(|p| !["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "AS"].iter().any(|w| p.is_word(w)) && p.kind != Kind::Comma)
                && next.is_some_and(|n| matches!(n.kind, Kind::Word | Kind::Quoted | Kind::Num) && !n.is_word("BY") && (n.kind != Kind::Word || !known_word(&n.upper()) || n.is_word("CASE")))
            {
                out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Nach {u} fehlt BY.", "BY is missing after {u}."), (t.end, t.end, " BY".into())));
                continue;
            }
        }
        // => statt >=, =< statt <=
        if t.kind == Kind::Op && t.text == "=" && let Some(n) = next.filter(|n| n.kind == Kind::Op && (n.text == ">" || n.text == "<") && n.start == t.end) {
            let right = format!("{}=", n.text);
            out.push(problem(t.start, n.end, Severity::Error, crate::tr_format!("Der Vergleich heißt {right}.", "The comparison is written {right}."), (t.start, n.end, right.clone())));
            continue;
        }
        // x::TEXT (PostgreSQL) -> CAST(x AS CHAR)
        if t.kind == Kind::Op && t.text == ":" && i > 0 && let Some(cast) = pg_cast(chars, stmt, i) {
            out.push(cast);
            continue;
        }
        // Funktionen von SQL Server mit Einheit vorne
        if dml && t.kind == Kind::Word && next.is_some_and(|n| n.kind == Kind::Open) && (t.is_word("DATEDIFF") || t.is_word("DATEADD")) && let Some(p) = date_unit_call(chars, stmt, i) {
            out.push(p);
            continue;
        }
        // "schueler" bzw. 'schueler' als Tabellenname -> `schueler`
        if t.kind == Kind::Str
            && let Some(p) = prev
            && (["FROM", "JOIN", "INTO", "UPDATE", "REFERENCES"].iter().any(|w| p.is_word(w)) || p.is_word("TABLE") && i >= 2 && ["CREATE", "DROP", "ALTER", "TRUNCATE"].iter().any(|w| stmt[i - 2].is_word(w)))
            && !inside_function(stmt, i)
            && let Some(name) = identifier_in(chars, t)
        {
            out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Namen stehen in `Backticks`, nicht in Anführungszeichen: `{name}`", "Names go in `backticks`, not quotes: `{name}`"), (t.start, t.end, format!("`{name}`"))));
            continue;
        }
        // IN 1, 2 -> IN (1, 2)
        if t.is_word("IN") && !after_dot && next.is_some_and(|n| matches!(n.kind, Kind::Num | Kind::Str | Kind::Var)) && !inside_function(stmt, i) {
            let mut k = i + 1;
            while stmt.get(k + 1).is_some_and(|c| c.kind == Kind::Comma) && stmt.get(k + 2).is_some_and(|v| matches!(v.kind, Kind::Num | Kind::Str | Kind::Var)) {
                k += 2;
            }
            let (a, b) = (stmt[i + 1].start, stmt[k].end);
            out.push(problem(t.start, b, Severity::Error, tr("Die Liste nach IN steht in Klammern.", "The list after IN goes in parentheses.").into(), (a, b, format!("({})", text(chars, a, b)))));
            continue;
        }
        // BETWEEN 1, 5 -> BETWEEN 1 AND 5
        if t.is_word("BETWEEN") && stmt.get(i + 1).is_some_and(is_value) && stmt.get(i + 2).is_some_and(|c| c.kind == Kind::Comma) && stmt.get(i + 3).is_some_and(is_value) {
            let c = &stmt[i + 2];
            out.push(problem(c.start, c.end, Severity::Error, tr("BETWEEN verbindet die Grenzen mit AND.", "BETWEEN joins its limits with AND.").into(), (c.start, c.end, " AND".into())));
            continue;
        }
        // LEFT JOIN t WHERE a.x = t.y -> LEFT JOIN t ON a.x = t.y
        if t.is_word("JOIN") && depth[i] == 0 && let Some(p) = join_without_on(chars, stmt, &depth, i) {
            out.push(p);
            continue;
        }
        // WHERE name = 'A%' -> LIKE
        if t.kind == Kind::Op && matches!(t.text.as_str(), "=" | "!=" | "<>") && reading && let Some(p) = percent_compare(chars, stmt, &depth, i) {
            out.push(p);
        }
        // LIKE 'A*' (Access, Windows) -> LIKE 'A%'
        if t.is_word("LIKE") && reading && let Some(s) = next.filter(|n| n.kind == Kind::Str && n.end >= n.start + 3) {
            let inner = text(chars, s.start + 1, s.end - 1);
            if inner.contains('*') && !inner.contains('%') {
                let new = inner.replace('*', "%").replace('?', "_");
                out.push(Problem {
                    start: s.start,
                    end: s.end,
                    severity: Severity::Warning,
                    message: crate::tr_format!("Bei LIKE steht % für beliebig viele Zeichen (nicht *): '{new}'", "LIKE uses % for any number of characters (not *): '{new}'"),
                    fix: Some(Fix { start: s.start + 1, end: s.end - 1, text: new }),
                });
            }
        }
        // ! =, < >, > =, < = -> !=, <>, >=, <=
        if t.kind == Kind::Op
            && matches!(t.text.as_str(), "!" | "<" | ">")
            && let Some(n) = next.filter(|n| n.kind == Kind::Op && n.start > t.end && (n.text == "=" || t.text == "<" && n.text == ">"))
            && chars[t.end..n.start].iter().all(|c| *c == ' ')
        {
            let joined = format!("{}{}", t.text, n.text);
            out.push(problem(t.start, n.end, Severity::Error, crate::tr_format!("Ohne Leerzeichen: {joined}", "Without a space: {joined}"), (t.start, n.end, joined.clone())));
            continue;
        }
        // WHERE id := 1 -> WHERE id = 1 (:= weist nur Variablen zu)
        if t.kind == Kind::Op && t.text == ":=" && prev.is_some_and(|p| p.kind != Kind::Var) && dml {
            let d = depth[i];
            let ctx = (0..i).rev().filter(|&k| depth[k] == d).map(|k| &stmt[k]).find(|x| ["WHERE", "HAVING", "ON", "WHEN", "SET", "SELECT"].iter().any(|w| x.is_word(w)));
            if ctx.is_some_and(|c| ["WHERE", "HAVING", "ON", "WHEN"].iter().any(|w| c.is_word(w))) {
                out.push(problem(t.start, t.end, Severity::Error, crate::i18n::tr("Verglichen wird mit =; := weist nur Variablen zu.", "Comparison uses =; := only assigns variables.").into(), (t.start, t.end, "=".into())));
                continue;
            }
        }
        // COUNT * -> COUNT(*)
        if t.is_word("COUNT") && next.is_some_and(|n| n.kind == Kind::Op && n.text == "*") {
            let n = &stmt[i + 1];
            out.push(problem(t.start, n.end, Severity::Error, crate::i18n::tr("COUNT braucht Klammern: COUNT(*)", "COUNT needs parentheses: COUNT(*)").into(), (t.start, n.end, "COUNT(*)".into())));
            continue;
        }
        // DATE_FORMAT(d, 'dd.MM.yyyy') (Java/Excel) -> '%d.%m.%Y'
        if t.is_word("DATE_FORMAT") && let Some(p) = java_date_pattern(chars, stmt, i) {
            out.push(p);
        }
    }
    match fw.as_str() {
        "SELECT" => {
            alias_strings(chars, stmt, &depth, out);
            clause_order(chars, stmt, &depth, out);
            distinct_in_list(chars, stmt, &depth, out);
            pipes(chars, stmt, &depth, out);
        }
        "UPDATE" => {
            update_set(chars, stmt, &depth, out);
            update_from(stmt, &depth, out);
        }
        "SHOW" => {
            // SHOW TABLE -> SHOW TABLES
            if let Some(w) = stmt.get(1).filter(|w| ["TABLE", "DATABASE", "COLUMN", "SCHEMA"].iter().any(|k| w.is_word(k))) && stmt.get(2).is_none_or(|n| n.is_word("FROM") || n.is_word("IN") || n.is_word("LIKE") || n.is_word("WHERE")) {
                let plural = format!("{}S", w.upper());
                out.push(problem(w.start, w.end, Severity::Error, crate::tr_format!("Gemeint ist SHOW {plural}.", "You mean SHOW {plural}."), (w.start, w.end, plural.clone())));
            }
        }
        "INSERT" | "REPLACE" => insert_shape(chars, stmt, &depth, out),
        "CREATE" | "ALTER" => ddl_shape(chars, stmt, &fw, out),
        _ => {}
    }
}

/// Liegt das Token in den Klammern einer Funktion, in der FROM o. ae. erlaubt ist (EXTRACT, TRIM …)?
fn inside_function(stmt: &[Tok], i: usize) -> bool {
    let mut d = 0;
    for k in (0..i).rev() {
        match stmt[k].kind {
            Kind::Close => d += 1,
            Kind::Open if d == 0 => {
                return k > 0 && ["EXTRACT", "TRIM", "SUBSTRING", "SUBSTR", "POSITION", "CAST", "CONVERT", "OVERLAY"].iter().any(|w| stmt[k - 1].is_word(w));
            }
            Kind::Open => d -= 1,
            _ => {}
        }
    }
    false
}

/// Inhalt eines Texts, wenn er wie ein Name aussieht
fn identifier_in(chars: &[char], t: &Tok) -> Option<String> {
    if t.end < t.start + 3 {
        return None;
    }
    let inner = text(chars, t.start + 1, t.end - 1);
    let ok = inner.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !inner.chars().next().is_some_and(|c| c.is_ascii_digit());
    ok.then_some(inner)
}

fn pg_cast(chars: &[char], stmt: &[Tok], i: usize) -> Option<Problem> {
    stmt.get(i + 1).filter(|n| n.kind == Kind::Op && n.text == ":" && n.start == stmt[i].end)?;
    let ty = stmt.get(i + 2).filter(|n| n.kind == Kind::Word)?;
    let group = stmt.get(i + 3).filter(|n| n.kind == Kind::Open).and_then(|_| group_end(stmt, i + 3));
    let args = group.map(|c| text(chars, stmt[i + 3].start, stmt[c].end)).unwrap_or_default();
    let target = match ty.upper().as_str() {
        "TEXT" | "VARCHAR" | "CHAR" | "CHARACTER" | "BPCHAR" | "NAME" | "CITEXT" => format!("CHAR{args}"),
        "INT" | "INTEGER" | "INT2" | "INT4" | "INT8" | "BIGINT" | "SMALLINT" => "SIGNED".into(),
        "NUMERIC" | "DECIMAL" => format!("DECIMAL{args}"),
        "REAL" | "FLOAT" | "FLOAT4" | "FLOAT8" | "DOUBLE" => "DOUBLE".into(),
        "DATE" => "DATE".into(),
        "TIMESTAMP" | "TIMESTAMPTZ" => "DATETIME".into(),
        "TIME" => "TIME".into(),
        "JSON" | "JSONB" => "JSON".into(),
        _ => return None,
    };
    // Operand davor: Klammer (mit Funktionsname), Name mit Punkten oder ein Wert
    let j = i - 1;
    let start = if stmt[j].kind == Kind::Close {
        let mut d = 0;
        let mut o = None;
        for k in (0..=j).rev() {
            match stmt[k].kind {
                Kind::Close => d += 1,
                Kind::Open => {
                    d -= 1;
                    if d == 0 {
                        o = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let o = o?;
        let func = o > 0 && stmt[o - 1].kind == Kind::Word && (!known_word(&stmt[o - 1].upper()) || super::highlight::FUNCTIONS.binary_search(&stmt[o - 1].upper().as_str()).is_ok());
        if func { o - 1 } else { o }
    } else if matches!(stmt[j].kind, Kind::Word | Kind::Quoted | Kind::Str | Kind::Num | Kind::Var) {
        let mut s = j;
        while s >= 2 && stmt[s - 1].kind == Kind::Dot && stmt[s - 2].is_name() {
            s -= 2;
        }
        s
    } else {
        return None;
    };
    let end = group.map_or(ty.end, |c| stmt[c].end);
    let operand = text(chars, stmt[start].start, stmt[j].end);
    let new = format!("CAST({operand} AS {target})");
    Some(problem(stmt[i].start, end, Severity::Error, crate::tr_format!(":: gibt es nur in PostgreSQL; MariaDB schreibt {new}.", ":: is PostgreSQL syntax; MariaDB writes {new}."), (stmt[start].start, end, new.clone())))
}

/// DATEDIFF(day, a, b) -> TIMESTAMPDIFF(DAY, a, b); DATEADD(day, n, d) -> DATE_ADD(d, INTERVAL n DAY)
fn date_unit_call(chars: &[char], stmt: &[Tok], i: usize) -> Option<Problem> {
    let close = group_end(stmt, i + 1)?;
    let its = items(stmt, i + 1, close);
    let [(a0, b0), (a1, b1), (a2, b2)] = its.as_slice() else { return None };
    if b0 - a0 != 1 || stmt[*a0].kind != Kind::Word {
        return None;
    }
    let u = unit(&stmt[*a0].text)?;
    let arg = |a: usize, b: usize| text(chars, stmt[a].start, stmt[b - 1].end);
    let new = if stmt[i].is_word("DATEDIFF") {
        format!("TIMESTAMPDIFF({u}, {}, {})", arg(*a1, *b1), arg(*a2, *b2))
    } else {
        format!("DATE_ADD({}, INTERVAL {} {u})", arg(*a2, *b2), arg(*a1, *b1))
    };
    let (s, e) = (stmt[i].start, stmt[close].end);
    Some(problem(s, e, Severity::Error, crate::tr_format!("So schreibt das SQL Server; in MariaDB: {new}", "That is SQL Server syntax; in MariaDB: {new}"), (s, e, new.clone())))
}

/// Woerter, mit denen der naechste Teil einer Abfrage beginnt
fn clause_word(t: &Tok) -> bool {
    ["WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "UNION", "WINDOW", "JOIN", "LEFT", "RIGHT", "INNER", "CROSS", "NATURAL", "FOR", "INTO"].iter().any(|w| t.is_word(w))
}

fn join_without_on(chars: &[char], stmt: &[Tok], depth: &[i32], i: usize) -> Option<Problem> {
    let mut b = i.checked_sub(1)?;
    if stmt[b].is_word("OUTER") {
        b = b.checked_sub(1)?;
    }
    if !(stmt[b].is_word("LEFT") || stmt[b].is_word("RIGHT")) {
        return None;
    }
    // Tabelle [AS] [Alias]
    let mut k = i + 1;
    let table = stmt.get(k).filter(|t| t.is_name())?;
    let mut name = table.text.clone();
    k += 1;
    if stmt.get(k).is_some_and(|t| t.kind == Kind::Dot) && stmt.get(k + 1).is_some_and(Tok::is_name) {
        name = stmt[k + 1].text.clone();
        k += 2;
    }
    if stmt.get(k).is_some_and(|t| t.is_word("AS")) {
        k += 1;
    }
    if let Some(a) = stmt.get(k).filter(|t| t.kind == Kind::Quoted || t.kind == Kind::Word && !known_word(&t.upper())) {
        name = a.text.clone();
        k += 1;
    }
    let w = stmt.get(k).filter(|t| t.is_word("WHERE"))?;
    let _ = w;
    // Bedingung bis zum naechsten Teil
    let mut end = k + 1;
    while end < stmt.len() && !(depth[end] == 0 && clause_word(&stmt[end])) {
        end += 1;
    }
    if end == k + 1 || stmt[k + 1..end].iter().zip(&depth[k + 1..end]).any(|(t, d)| *d == 0 && (t.is_word("OR") || t.is_word("XOR"))) {
        return None;
    }
    // In Teile an AND zerlegen (das AND von BETWEEN gehoert dazu)
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut s = k + 1;
    let mut between = false;
    for m in k + 1..end {
        if depth[m] != 0 {
            continue;
        }
        if stmt[m].is_word("BETWEEN") {
            between = true;
        } else if stmt[m].is_word("AND") {
            if between {
                between = false;
            } else {
                parts.push((s, m));
                s = m + 1;
            }
        }
    }
    parts.push((s, end));
    let refers = |(a, b): (usize, usize)| {
        let toks = &stmt[a..b];
        let joined = toks.windows(2).any(|w| w[0].is_name() && w[0].text.eq_ignore_ascii_case(&name) && w[1].kind == Kind::Dot);
        let other = toks.windows(2).any(|w| w[0].is_name() && !w[0].text.eq_ignore_ascii_case(&name) && w[1].kind == Kind::Dot);
        joined && other && toks.iter().any(|t| t.kind == Kind::Op && t.text == "=")
    };
    let part_text = |(a, b): (usize, usize)| text(chars, stmt[a].start, stmt[b - 1].end);
    type Parts = Vec<(usize, usize)>;
    let (on, rest): (Parts, Parts) = parts.iter().copied().filter(|(a, b)| a < b).partition(|p| refers(*p));
    let (on, rest) = if on.is_empty() && rest.len() == 1 { (rest, Vec::new()) } else { (on, rest) };
    if on.is_empty() {
        return None;
    }
    let mut new = format!("ON {}", on.into_iter().map(part_text).collect::<Vec<_>>().join(" AND "));
    if !rest.is_empty() {
        new.push_str(&format!(" WHERE {}", rest.into_iter().map(part_text).collect::<Vec<_>>().join(" AND ")));
    }
    let (s, e) = (stmt[k].start, stmt[end - 1].end);
    Some(problem(
        stmt[b].start,
        stmt[i].end,
        Severity::Error,
        crate::tr_format!("{} JOIN braucht die Verknüpfung mit ON.", "{} JOIN needs its join condition after ON.", stmt[b].upper()),
        (s, e, new),
    ))
}

fn percent_compare(chars: &[char], stmt: &[Tok], depth: &[i32], i: usize) -> Option<Problem> {
    let s = stmt.get(i + 1).filter(|n| n.kind == Kind::Str && n.end >= n.start + 3)?;
    let inner = text(chars, s.start + 1, s.end - 1);
    let core = inner.trim_matches('%');
    if !(inner.starts_with('%') || inner.ends_with('%')) || core.is_empty() || core.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' ' | '-')) {
        return None;
    }
    // Nur in Bedingungen, nicht bei Zuweisungen (SET a = '10%')
    let d = depth[i];
    let ctx = (0..i).rev().filter(|&k| depth[k] == d).map(|k| &stmt[k]).find(|t| ["WHERE", "HAVING", "ON", "WHEN", "SET", "VALUES", "SELECT", "BY", "LIMIT"].iter().any(|w| t.is_word(w)))?;
    if !["WHERE", "HAVING", "ON", "WHEN"].iter().any(|w| ctx.is_word(w)) {
        return None;
    }
    let t = &stmt[i];
    let with = if t.text == "=" { "LIKE" } else { "NOT LIKE" };
    Some(Problem {
        start: t.start,
        end: s.end,
        severity: Severity::Warning,
        message: crate::tr_format!("% ist nur mit {with} ein Platzhalter; mit {} wird nach dem Zeichen % gesucht.", "% is only a wildcard with {with}; with {} it looks for the character %.", t.text),
        fix: Some(Fix { start: t.start, end: t.end, text: with.into() }),
    })
}

/// SELECT … FROM … WHERE … GROUP BY … HAVING … ORDER BY … LIMIT in die richtige Reihenfolge bringen,
/// doppelte WHERE/HAVING mit AND verbinden
fn clause_order(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let mut starts: Vec<(usize, u8)> = vec![(0, 0)];
    for (k, t) in stmt.iter().enumerate().skip(1) {
        if depth[k] != 0 || t.kind != Kind::Word {
            continue;
        }
        let by = stmt.get(k + 1).is_some_and(|n| n.is_word("BY"));
        let rank = match t.upper().as_str() {
            "FROM" => 1,
            "WHERE" => 2,
            "GROUP" if by => 3,
            "HAVING" => 4,
            "ORDER" if by => 5,
            "LIMIT" => 6,
            "UNION" | "INTO" | "FOR" | "LOCK" | "WINDOW" | "PROCEDURE" | "FETCH" | "EXCEPT" | "INTERSECT" | "SELECT" => return,
            _ => continue,
        };
        starts.push((k, rank));
    }
    let sorted = starts.windows(2).all(|w| w[0].1 < w[1].1);
    if sorted {
        return;
    }
    let mut segs: Vec<(u8, usize, usize)> = starts.iter().enumerate().map(|(n, &(a, r))| (r, a, starts.get(n + 1).map_or(stmt.len(), |s| s.0))).collect();
    for r in [1u8, 3, 5, 6] {
        if segs.iter().filter(|s| s.0 == r).count() > 1 {
            return;
        }
    }
    segs.sort_by_key(|s| s.0);
    let seg_text = |a: usize, b: usize| text(chars, stmt[a].start, stmt[b - 1].end);
    let mut parts: Vec<(u8, String)> = Vec::new();
    for (r, a, b) in segs {
        if let Some(last) = parts.last_mut().filter(|l| l.0 == r) {
            // zweites WHERE/HAVING: Bedingung anhaengen
            let body = seg_text(a + 1, b);
            let has_or = stmt[a + 1..b].iter().zip(&depth[a + 1..b]).any(|(t, d)| *d == 0 && t.is_word("OR"));
            last.1.push_str(&format!(" AND {}", if has_or { format!("({body})") } else { body }));
        } else {
            parts.push((r, seg_text(a, b)));
        }
    }
    let gap_newline = starts.iter().skip(1).any(|&(k, _)| chars[stmt[k - 1].end..stmt[k].start].contains(&'\n'));
    let new = parts.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join(if gap_newline { "\n" } else { " " });
    let (s, e) = (stmt[0].start, stmt[stmt.len() - 1].end);
    out.push(problem(
        s,
        stmt[0].end,
        Severity::Error,
        crate::tr_format!(
            "Die Teile der Abfrage stehen falsch: Reihenfolge SELECT, FROM, WHERE, GROUP BY, HAVING, ORDER BY, LIMIT.",
            "The parts of the query are out of order: SELECT, FROM, WHERE, GROUP BY, HAVING, ORDER BY, LIMIT."
        ),
        (s, e, new),
    ));
}

/// SELECT a, DISTINCT b -> SELECT DISTINCT a, b
fn distinct_in_list(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    if stmt.get(1).is_none_or(|t| t.is_word("DISTINCT") || t.is_word("ALL")) {
        return;
    }
    let Some(k) = (2..stmt.len()).find(|&k| depth[k] == 0 && stmt[k].is_word("DISTINCT") && stmt[k - 1].kind == Kind::Comma) else { return };
    let Some(n) = stmt.get(k + 1) else { return };
    let (s, e) = (stmt[1].start, n.start);
    out.push(problem(
        stmt[k].start,
        stmt[k].end,
        Severity::Error,
        crate::i18n::tr("DISTINCT gilt für die ganze Zeile und steht direkt nach SELECT.", "DISTINCT applies to the whole row and goes right after SELECT.").into(),
        (s, e, format!("DISTINCT {}", text(chars, s, stmt[k].start))),
    ));
}

/// SELECT name || ' ' || ort -> SELECT CONCAT(name, ' ', ort) (|| ist in MariaDB ein ODER)
fn pipes(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let end = (1..stmt.len()).find(|&k| depth[k] == 0 && stmt[k].is_word("FROM")).unwrap_or(stmt.len());
    let mut a = 1;
    while stmt.get(a).is_some_and(|t| t.is_word("DISTINCT") || t.is_word("ALL")) {
        a += 1;
    }
    let mut item_start = a;
    for k in a..=end {
        if k < end && !(depth[k] == 0 && stmt[k].kind == Kind::Comma) {
            continue;
        }
        let item_end = (item_start..k).find(|&m| depth[m] == 0 && stmt[m].is_word("AS")).unwrap_or(k);
        let ops: Vec<usize> = (item_start..item_end).filter(|&m| depth[m] == 0 && stmt[m].kind == Kind::Op && stmt[m].text == "||").collect();
        if !ops.is_empty() {
            let mut bounds = vec![item_start];
            for &o in &ops {
                bounds.push(o);
                bounds.push(o + 1);
            }
            bounds.push(item_end);
            let operands: Vec<(usize, usize)> = bounds.chunks(2).map(|c| (c[0], c[1])).collect();
            if operands.iter().all(|(x, y)| x < y) && operands.iter().any(|(x, y)| y - x == 1 && stmt[*x].kind == Kind::Str) {
                let list: Vec<String> = operands.iter().map(|(x, y)| text(chars, stmt[*x].start, stmt[y - 1].end)).collect();
                let (s, e) = (stmt[item_start].start, stmt[item_end - 1].end);
                out.push(Problem {
                    start: stmt[ops[0]].start,
                    end: stmt[ops[0]].end,
                    severity: Severity::Warning,
                    message: crate::i18n::tr("|| ist in MariaDB ein logisches ODER; Texte verbindet CONCAT(…).", "|| is a logical OR in MariaDB; CONCAT(…) joins text.").into(),
                    fix: Some(Fix { start: s, end: e, text: format!("CONCAT({})", list.join(", ")) }),
                });
            }
        }
        item_start = k + 1;
    }
}

fn update_set(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let set = (1..stmt.len()).find(|&k| depth[k] == 0 && stmt[k].is_word("SET"));
    let Some(set) = set else {
        // UPDATE t name = 'x' -> UPDATE t SET name = 'x'
        if let Some(e) = (2..stmt.len()).find(|&k| depth[k] == 0 && stmt[k].kind == Kind::Op && stmt[k].text == "=") {
            let mut n = e - 1;
            if n >= 2 && stmt[n - 1].kind == Kind::Dot {
                n -= 2;
            }
            if n >= 2 && stmt[n].is_name() && !(1..n).any(|k| stmt[k].is_word("WHERE")) {
                let t = &stmt[n];
                out.push(problem(t.start, t.end, Severity::Error, crate::i18n::tr("Vor den neuen Werten fehlt SET.", "SET is missing before the new values.").into(), (t.start, t.start, "SET ".into())));
            }
        }
        return;
    };
    // SET a = 1 AND b = 2 -> SET a = 1, b = 2
    for k in set + 1..stmt.len() {
        if depth[k] != 0 {
            continue;
        }
        let t = &stmt[k];
        if ["WHERE", "ORDER", "LIMIT"].iter().any(|w| t.is_word(w)) {
            break;
        }
        let assign = |n: usize| stmt.get(n).is_some_and(Tok::is_name) && stmt.get(n + 1).is_some_and(|o| o.kind == Kind::Op && o.text == "=");
        if t.is_word("AND") && (assign(k + 1) || stmt.get(k + 2).is_some_and(|d| d.kind == Kind::Dot) && assign(k + 3)) {
            let p = &stmt[k - 1];
            out.push(problem(t.start, t.end, Severity::Error, crate::i18n::tr("Mehrere Zuweisungen in SET trennt ein Komma, nicht AND.", "Several assignments in SET are separated by commas, not AND.").into(), (p.end, t.end, ",".into())));
        }
    }
    let _ = chars;
}

fn insert_shape(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let tr = crate::i18n::tr;
    // Tabelle und Spaltenliste
    let into = stmt.iter().position(|t| t.is_word("INTO")).map_or(1, |k| k + 1);
    let mut ti = into;
    while stmt.get(ti).is_some_and(|t| t.kind == Kind::Word && known_word(&t.upper())) && ti < into + 3 {
        ti += 1;
    }
    if stmt.get(ti + 1).is_some_and(|t| t.kind == Kind::Dot) {
        ti += 2;
    }
    if stmt.get(ti).is_some_and(Tok::is_name) && stmt.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) && let Some(c) = group_end(stmt, ti + 1) {
        // 'name' in der Spaltenliste -> `name`
        if stmt.get(c + 1).is_some_and(|n| n.is_word("VALUES") || n.is_word("VALUE") || n.is_word("SELECT") || n.kind == Kind::Open) {
            for (a, b) in items(stmt, ti + 1, c) {
                if b == a + 1 && stmt[a].kind == Kind::Str && let Some(name) = identifier_in(chars, &stmt[a]) {
                    let t = &stmt[a];
                    let name = if known_word(&name.to_ascii_uppercase()) { format!("`{name}`") } else { name };
                    out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Spaltennamen stehen ohne Anführungszeichen (oder in `Backticks`): {name}", "Column names go without quotes (or in `backticks`): {name}"), (t.start, t.end, name)));
                }
            }
        }
        // INSERT INTO t (a) ('x') -> VALUES fehlt
        if stmt.get(c + 1).is_some_and(|n| n.kind == Kind::Open) && !stmt.get(c + 2).is_some_and(|n| n.is_word("SELECT")) {
            let n = &stmt[c + 1];
            out.push(problem(n.start, n.end, Severity::Error, tr("Vor den Werten fehlt VALUES.", "VALUES is missing before the values.").into(), (n.start, n.start, "VALUES ".into())));
            return;
        }
    }
    let Some((_, vi)) = insert_parts(stmt) else { return };
    // VALUES (SELECT …) -> INSERT … SELECT …
    if stmt.get(vi + 1).is_some_and(|t| t.kind == Kind::Open)
        && stmt.get(vi + 2).is_some_and(|t| t.is_word("SELECT"))
        && let Some(c) = group_end(stmt, vi + 1)
        && stmt.get(c + 1).is_none_or(|t| t.is_word("ON"))
    {
        let (s, e) = (stmt[vi].start, stmt[c].end);
        out.push(problem(
            stmt[vi].start,
            stmt[vi].end,
            Severity::Error,
            tr("Werte aus einer Abfrage kommen ohne VALUES: INSERT … SELECT …", "Values from a query come without VALUES: INSERT … SELECT …").into(),
            (s, e, text(chars, stmt[vi + 2].start, stmt[c].start).trim_end().to_string()),
        ));
        return;
    }
    // VALUES ('a'), 'b' -> VALUES ('a'), ('b')
    let mut k = vi + 1;
    while k < stmt.len() {
        let t = &stmt[k];
        if t.kind == Kind::Open {
            let Some(c) = group_end(stmt, k) else { return };
            k = c + 1;
            if stmt.get(k).is_some_and(|x| x.kind == Kind::Comma) {
                k += 1;
            }
            continue;
        }
        if t.is_word("ON") || depth[k] != 0 || !(is_value(t) || t.kind == Kind::Op && t.text == "-") {
            return;
        }
        let mut e = k;
        while e + 1 < stmt.len() && !(stmt[e + 1].kind == Kind::Comma && stmt.get(e + 2).is_some_and(|x| x.kind == Kind::Open)) && !stmt[e + 1].is_word("ON") {
            e += 1;
        }
        let (s, end) = (t.start, stmt[e].end);
        out.push(problem(s, end, Severity::Error, tr("Jede Zeile in VALUES steht in Klammern.", "Each row in VALUES goes in parentheses.").into(), (s, end, format!("({})", text(chars, s, end)))));
        return;
    }
}

fn ddl_shape(chars: &[char], stmt: &[Tok], fw: &str, out: &mut Vec<Problem>) {
    let tr = crate::i18n::tr;
    let is_ty = |x: &Tok| x.kind == Kind::Word && crate::sqlfmt::is_type(&x.text);
    for (k, t) in stmt.iter().enumerate() {
        let next = stmt.get(k + 1);
        // FOREIGN KEY spalte REFERENCES / , PRIMARY KEY spalte) / CREATE INDEX i ON t spalte -> (spalte)
        let wrap = if t.is_word("KEY") && k >= 1 && (stmt[k - 1].is_word("FOREIGN") && stmt.get(k + 2).is_some_and(|r| r.is_word("REFERENCES")) || stmt[k - 1].is_word("PRIMARY") && k >= 2 && stmt[k - 2].kind == Kind::Comma && stmt.get(k + 2).is_none_or(|r| matches!(r.kind, Kind::Comma | Kind::Close))) {
            next.filter(|n| n.is_name())
        } else if fw == "CREATE" && t.is_word("ON") && stmt.iter().take(4).any(|x| x.is_word("INDEX")) && stmt.get(k + 1).is_some_and(Tok::is_name) && stmt.len() == k + 3 {
            stmt.get(k + 2).filter(|n| n.is_name())
        } else {
            None
        };
        if let Some(n) = wrap {
            out.push(problem(n.start, n.end, Severity::Error, crate::tr_format!("Die Spalte steht in Klammern: ({})", "The column goes in parentheses: ({})", n.text), (n.start, n.end, format!("({})", text(chars, n.start, n.end)))));
            continue;
        }
        // RENAME name TO neu -> RENAME COLUMN name TO neu
        if fw == "ALTER" && t.is_word("RENAME") && next.is_some_and(|n| n.is_name() && !n.is_word("TO") && !n.is_word("AS") && !n.is_word("COLUMN") && !n.is_word("INDEX") && !n.is_word("KEY")) && stmt.get(k + 2).is_some_and(|x| x.is_word("TO")) {
            out.push(problem(t.start, t.end, Severity::Error, tr("Eine Spalte benennt RENAME COLUMN … TO … um.", "RENAME COLUMN … TO … renames a column.").into(), (t.end, t.end, " COLUMN".into())));
            continue;
        }
        // DEFAULT ohne Wert
        if t.is_word("DEFAULT") && k >= 1 && (is_ty(&stmt[k - 1]) || stmt[k - 1].kind == Kind::Close || stmt[k - 1].is_word("NULL")) && next.is_none_or(|n| matches!(n.kind, Kind::Comma | Kind::Close)) {
            out.push(problem(t.start, t.end, Severity::Error, tr("Nach DEFAULT fehlt der Wert – weggelassen.", "DEFAULT has no value – removed.").into(), (stmt[k - 1].end, t.end, String::new())));
            continue;
        }
        // VARCHAR 20 -> VARCHAR(20)
        if ["VARCHAR", "CHAR", "VARBINARY", "BINARY"].iter().any(|w| t.is_word(w)) && let Some(n) = next.filter(|n| n.kind == Kind::Num) {
            out.push(problem(t.start, n.end, Severity::Error, crate::tr_format!("Die Länge steht in Klammern: {}({})", "The length goes in parentheses: {}({})", t.upper(), text(chars, n.start, n.end)), (t.end, n.end, format!("({})", text(chars, n.start, n.end)))));
            continue;
        }
        // DECIMAL(8.2) -> DECIMAL(8,2) (MariaDB nimmt sonst stillschweigend 8 Stellen ohne Komma)
        if ["DECIMAL", "NUMERIC", "DEC"].iter().any(|w| t.is_word(w)) && next.is_some_and(|n| n.kind == Kind::Open) && let Some(n) = stmt.get(k + 2).filter(|n| n.kind == Kind::Num && text(chars, n.start, n.end).contains('.')) {
            let fixed = text(chars, n.start, n.end).replacen('.', ",", 1);
            out.push(problem(n.start, n.end, Severity::Error, crate::tr_format!("Stellen und Nachkommastellen trennt ein Komma: {}({fixed})", "Digits and decimals are separated by a comma: {}({fixed})", t.upper()), (n.start, n.end, fixed.clone())));
            continue;
        }
        // ENUM(rot, gelb) -> ENUM('rot', 'gelb')
        if (t.is_word("ENUM") || t.is_word("SET") && k >= 1 && stmt[k - 1].is_name() && fw != "UPDATE") && next.is_some_and(|n| n.kind == Kind::Open) && let Some(c) = group_end(stmt, k + 1) {
            let its = items(stmt, k + 1, c);
            if !its.is_empty() && its.iter().all(|&(a, b)| b == a + 1 && matches!(stmt[a].kind, Kind::Word | Kind::Str | Kind::Num)) && its.iter().any(|&(a, _)| stmt[a].kind == Kind::Word) {
                let list: Vec<String> = its.iter().map(|&(a, _)| if stmt[a].kind == Kind::Str { text(chars, stmt[a].start, stmt[a].end) } else { format!("'{}'", text(chars, stmt[a].start, stmt[a].end)) }).collect();
                let (s, e) = (stmt[k + 1].start, stmt[c].end);
                out.push(problem(s, e, Severity::Error, crate::tr_format!("Die erlaubten Werte stehen in Anführungszeichen: ({})", "The allowed values go in quotes: ({})", list.join(", ")), (s, e, format!("({})", list.join(", ")))));
            }
        }
    }
    if fw == "CREATE" && let Some(tk) = stmt.iter().take(4).position(|t| t.is_word("TABLE") || t.is_word("DATABASE") || t.is_word("SCHEMA")) {
        let mut n = tk + 1;
        if stmt.get(n).is_some_and(|t| t.is_word("IF")) {
            n += 3;
        }
        let table = stmt[tk].is_word("TABLE");
        // CREATE TABLE lieblings fach ( / klasse-neu ( -> lieblings_fach
        if let (Some(a), Some(b)) = (stmt.get(n).filter(|t| t.kind == Kind::Word && !known_word(&t.upper())), stmt.get(n + 1)) {
            let follows = |k: usize| if table { stmt.get(k).is_some_and(|t| t.kind == Kind::Open) } else { stmt.get(k).is_none_or(|t| t.is_word("CHARACTER") || t.is_word("DEFAULT") || t.is_word("COLLATE")) };
            let joined = if b.kind == Kind::Word && !known_word(&b.upper()) && follows(n + 2) {
                Some((b.end, format!("{}_{}", a.text, b.text)))
            } else if b.kind == Kind::Op && b.text == "-" && b.start == a.end && stmt.get(n + 2).is_some_and(|c| c.kind == Kind::Word && c.start == b.end) && follows(n + 3) {
                Some((stmt[n + 2].end, format!("{}_{}", a.text, stmt[n + 2].text)))
            } else {
                None
            };
            if let Some((end, name)) = joined {
                out.push(problem(a.start, end, Severity::Error, crate::tr_format!("Ein Name ohne Backticks hat keine Leer- oder Bindestriche: {name}", "A name without backticks has no spaces or hyphens: {name}"), (a.start, end, name)));
                return;
            }
        }
        // Spalte ohne Datentyp
        if table && let Some(open) = stmt.iter().position(|t| t.kind == Kind::Open) && let Some(close) = group_end(stmt, open) {
            for (a, b) in items(stmt, open, close) {
                if b == a + 1 && stmt[a].is_name() && (stmt[a].kind == Kind::Quoted || !known_word(&stmt[a].upper())) {
                    let t = &stmt[a];
                    let ty = guess_type(&t.text);
                    out.push(problem(t.start, t.end, Severity::Error, crate::tr_format!("Die Spalte {} braucht einen Datentyp, z. B. {ty}.", "The column {} needs a data type, e.g. {ty}.", t.text), (t.end, t.end, format!(" {ty}"))));
                }
            }
        }
        return;
    }
    if fw != "ALTER" {
        return;
    }
    for (k, t) in stmt.iter().enumerate() {
        // ADD [COLUMN] email -> ADD COLUMN email VARCHAR(255)
        if t.is_word("ADD") {
            let j = if stmt.get(k + 1).is_some_and(|x| x.is_word("COLUMN")) { k + 2 } else { k + 1 };
            if let Some(name) = stmt.get(j).filter(|x| x.is_name() && (x.kind == Kind::Quoted || !known_word(&x.upper())))
                && stmt.get(j + 1).is_none_or(|x| x.kind == Kind::Comma)
            {
                let ty = guess_type(&name.text);
                out.push(problem(name.start, name.end, Severity::Error, crate::tr_format!("Die Spalte {} braucht einen Datentyp, z. B. {ty}.", "The column {} needs a data type, e.g. {ty}.", name.text), (name.end, name.end, format!(" {ty}"))));
            }
        }
        // CHANGE name VARCHAR(80) -> MODIFY name VARCHAR(80) (CHANGE braucht alten und neuen Namen)
        if t.is_word("CHANGE") {
            let j = if stmt.get(k + 1).is_some_and(|x| x.is_word("COLUMN")) { k + 2 } else { k + 1 };
            let is_ty = |x: &Tok| x.kind == Kind::Word && crate::sqlfmt::is_type(&x.text);
            if stmt.get(j).is_some_and(Tok::is_name) && let Some(ty) = stmt.get(j + 1).filter(|x| is_ty(x)) {
                let after = if stmt.get(j + 2).is_some_and(|x| x.kind == Kind::Open) { group_end(stmt, j + 2).map(|c| c + 1) } else { Some(j + 2) };
                if after.is_some_and(|a| stmt.get(a).is_none_or(|x| !is_ty(x))) {
                    let _ = ty;
                    out.push(problem(t.start, t.end, Severity::Error, tr("CHANGE braucht alten und neuen Namen; nur den Typ ändert MODIFY.", "CHANGE needs the old and new name; MODIFY changes only the type.").into(), (t.start, t.end, "MODIFY".into())));
                }
            }
        }
    }
    let _ = chars;
}

/// UPDATE t SET … FROM t WHERE … (SQL Server): FROM mit derselben Tabelle weglassen
fn update_from(stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let Some(table) = stmt.get(1).filter(|t| t.is_name()) else { return };
    let Some(f) = (2..stmt.len()).find(|&k| depth[k] == 0 && stmt[k].is_word("FROM")) else { return };
    if stmt.get(f + 1).is_some_and(|t| t.text.eq_ignore_ascii_case(&table.text)) && stmt.get(f + 2).is_none_or(|t| t.is_word("WHERE")) {
        let (s, e) = (stmt[f - 1].end, stmt[f + 1].end);
        out.push(problem(stmt[f].start, e, Severity::Error, crate::i18n::tr("UPDATE braucht kein FROM für die eigene Tabelle.", "UPDATE needs no FROM for its own table.").into(), (s, e, String::new())));
    }
}

/// ORDER BY 'Vorname' sortiert nach einem festen Text; gemeint ist die Spalte `Vorname`
fn alias_strings(chars: &[char], stmt: &[Tok], depth: &[i32], out: &mut Vec<Problem>) {
    let aliases: Vec<String> = stmt
        .windows(2)
        .filter(|w| w[0].is_word("AS"))
        .filter_map(|w| match w[1].kind {
            Kind::Str => identifier_like(chars, &w[1]),
            Kind::Word | Kind::Quoted => Some(w[1].text.clone()),
            _ => None,
        })
        .collect();
    for (k, t) in stmt.iter().enumerate() {
        if t.kind != Kind::Str || depth[k] != 0 || k == 0 {
            continue;
        }
        let in_order = (0..k).rev().filter(|&j| depth[j] == 0).map(|j| &stmt[j]).find(|x| x.kind == Kind::Word && ["BY", "WHERE", "SELECT", "FROM", "HAVING", "LIMIT"].iter().any(|w| x.is_word(w)));
        if !in_order.is_some_and(|x| x.is_word("BY")) || !matches!(stmt[k - 1].kind, Kind::Comma) && !stmt[k - 1].is_word("BY") {
            continue;
        }
        let Some(name) = identifier_like(chars, t).filter(|n| aliases.iter().any(|a| a == n)) else { continue };
        out.push(Problem {
            start: t.start,
            end: t.end,
            severity: Severity::Warning,
            message: crate::tr_format!("'{name}' ist ein fester Text; die Spalte heißt `{name}`.", "'{name}' is fixed text; the column is `{name}`."),
            fix: Some(Fix { start: t.start, end: t.end, text: format!("`{name}`") }),
        });
    }
}

/// Inhalt eines Texts als Spaltenname (ohne Backticks und Zeilenumbrueche)
fn identifier_like(chars: &[char], t: &Tok) -> Option<String> {
    let inner = text(chars, t.start + 1, t.end.saturating_sub(1));
    (!inner.is_empty() && !inner.contains(['`', '\n', '\''])).then_some(inner)
}

/// DATE_FORMAT(d, 'dd.MM.yyyy') -> DATE_FORMAT(d, '%d.%m.%Y')
fn java_date_pattern(chars: &[char], stmt: &[Tok], i: usize) -> Option<Problem> {
    let close = group_end(stmt, i + 1)?;
    let its = items(stmt, i + 1, close);
    let &(a, b) = its.get(1)?;
    let s = stmt.get(a).filter(|s| s.kind == Kind::Str && b == a + 1)?;
    let inner = text(chars, s.start + 1, s.end - 1);
    if inner.contains('%') || !["yy", "dd", "MM", "HH"].iter().any(|p| inner.contains(p)) {
        return None;
    }
    let mut new = inner.clone();
    for (from, to) in [("yyyy", "%Y"), ("YYYY", "%Y"), ("yy", "%y"), ("MM", "%m"), ("dd", "%d"), ("DD", "%d"), ("HH", "%H"), ("mm", "%i"), ("ss", "%s")] {
        new = new.replace(from, to);
    }
    Some(Problem {
        start: s.start,
        end: s.end,
        severity: Severity::Warning,
        message: crate::tr_format!("DATE_FORMAT schreibt Platzhalter mit %: '{new}'", "DATE_FORMAT writes placeholders with %: '{new}'"),
        fix: Some(Fix { start: s.start + 1, end: s.end - 1, text: new }),
    })
}

/// Ueber Anweisungsgrenzen: ein Semikolon zu frueh ("SELECT … ;\nWHERE …")
pub(super) fn split_too_early(stmts: &[super::script::Stmt], out: &mut Vec<Problem>) {
    for w in stmts.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let Some(first) = b.toks.first() else { continue };
        let continues = ["WHERE", "AND", "OR", "ORDER", "GROUP", "HAVING", "LIMIT", "FROM", "JOIN", "LEFT", "INNER", "OFFSET"].iter().any(|k| first.is_word(k));
        let a_kind = a.toks.first().map(Tok::upper).unwrap_or_default();
        if continues && a.semi && matches!(a_kind.as_str(), "SELECT" | "UPDATE" | "DELETE" | "WITH") {
            out.push(problem(
                a.end - 1,
                a.end,
                Severity::Error,
                crate::tr_format!("Dieses Semikolon beendet die Anweisung zu früh: {} gehört noch dazu.", "This semicolon ends the statement too early: {} still belongs to it.", first.upper()),
                (a.end - 1, a.end, String::new()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lint::{Env, apply_fixes, lint};

    fn fixed(sql: &str) -> String {
        let env = Env { schema: None, databases: &[] };
        let mut s = sql.to_string();
        for _ in 0..5 {
            let (n, _) = apply_fixes(&s, &lint(&s, &env));
            if n == s {
                break;
            }
            s = n;
        }
        s
    }

    #[test]
    fn structure_is_repaired() {
        let cases = [
            ("SELECT * FROM t ORDER BY a WHERE b = 1;", "SELECT * FROM t WHERE b = 1 ORDER BY a;"),
            ("SELECT * FROM t LIMIT 1 ORDER BY a;", "SELECT * FROM t ORDER BY a LIMIT 1;"),
            ("SELECT * FROM t WHERE a > 0 WHERE b = 'x';", "SELECT * FROM t WHERE a > 0 AND b = 'x';"),
            ("SELECT a, COUNT(*) FROM t GROUP a;", "SELECT a, COUNT(*) FROM t GROUP BY a;"),
            ("SELECT a, DISTINCT b FROM t;", "SELECT DISTINCT a, b FROM t;"),
            ("SELECT * FROM FROM t;", "SELECT * FROM t;"),
            ("SELECT * FROM t WHERE a => 1 AND b =< 2;", "SELECT * FROM t WHERE a >= 1 AND b <= 2;"),
            ("SELECT * FROM t WHERE id IN 1, 2;", "SELECT * FROM t WHERE id IN (1, 2);"),
            ("SELECT * FROM t WHERE id BETWEEN 1, 2;", "SELECT * FROM t WHERE id BETWEEN 1 AND 2;"),
            ("SELECT a.x FROM a LEFT JOIN b WHERE a.id = b.a_id AND b.y = 1;", "SELECT a.x FROM a LEFT JOIN b ON a.id = b.a_id WHERE b.y = 1;"),
            ("SELECT name, k::TEXT FROM t;", "SELECT name, CAST(k AS CHAR) FROM t;"),
            ("SELECT DATEDIFF(day, a, NOW()) FROM t;", "SELECT TIMESTAMPDIFF(DAY, a, NOW()) FROM t;"),
            ("SELECT DATEADD(month, 2, a) FROM t;", "SELECT DATE_ADD(a, INTERVAL 2 MONTH) FROM t;"),
            ("SELECT * FROM \"t\";", "SELECT * FROM `t`;"),
            ("SELECT a || ' ' || b AS c FROM t;", "SELECT CONCAT(a, ' ', b) AS c FROM t;"),
            ("SELECT * FROM t WHERE a = 'X%';", "SELECT * FROM t WHERE a LIKE 'X%';"),
            ("UPDATE t a = 1 WHERE id = 2;", "UPDATE t SET a = 1 WHERE id = 2;"),
            ("UPDATE t SET a = 'x' AND b = 2 WHERE id = 1;", "UPDATE t SET a = 'x', b = 2 WHERE id = 1;"),
            ("INSERT INTO t (a) ('x');", "INSERT INTO t (a) VALUES ('x');"),
            ("INSERT INTO t ('a', 'b') VALUES (1, 2);", "INSERT INTO t (a, b) VALUES (1, 2);"),
            ("INSERT INTO t (a) VALUES (SELECT a FROM u);", "INSERT INTO t (a) SELECT a FROM u;"),
            ("INSERT INTO t (a) VALUES ('x'), 'y';", "INSERT INTO t (a) VALUES ('x'), ('y');"),
            ("CREATE TABLE t (id INT NOTNULL);", "CREATE TABLE t (id INT NOT NULL);"),
            ("CREATE TABLE lieblings fach (id INT);", "CREATE TABLE lieblings_fach (id INT);"),
            ("CREATE TABLE klasse-neu (id INT);", "CREATE TABLE klasse_neu (id INT);"),
            ("CREATE TABLE t (id INT, nummer, schule_id);", "CREATE TABLE t (id INT, nummer VARCHAR(255), schule_id INT);"),
            ("ALTER TABLE t ADD COLUMN geburtsdatum;", "ALTER TABLE t ADD COLUMN geburtsdatum DATE;"),
            ("ALTER TABLE t CHANGE name VARCHAR(80);", "ALTER TABLE t MODIFY name VARCHAR(80);"),
            ("SELECT * FROM t;\nWHERE id = 1;", "SELECT * FROM t\nWHERE id = 1;"),
            ("SELECT COUNT(* FROM t;", "SELECT COUNT(*) FROM t;"),
            ("CREATE TABLE t (id INT, a VARCHAR(5, b INT);", "CREATE TABLE t (id INT, a VARCHAR(5), b INT);"),
            ("SELECT * FROM t; // alle", "SELECT * FROM t; -- alle"),
            ("SELECT * FROM t WHERE a LIKE 'A*';", "SELECT * FROM t WHERE a LIKE 'A%';"),
            ("SELECT * FROM t WHERE a ! = 1 AND b < > 2;", "SELECT * FROM t WHERE a != 1 AND b <> 2;"),
            ("SELECT * FROM t WHERE a := 1;", "SELECT * FROM t WHERE a = 1;"),
            ("SELECT COUNT * FROM t;", "SELECT COUNT(*) FROM t;"),
            ("SELECT DATE_FORMAT(d, 'dd.MM.yyyy HH:mm') FROM t;", "SELECT DATE_FORMAT(d, '%d.%m.%Y %H:%i') FROM t;"),
            ("SHOW TABLE;", "SHOW TABLES;"),
            ("UPDATE t SET a = 1 FROM t WHERE id = 2;", "UPDATE t SET a = 1 WHERE id = 2;"),
            ("SELECT a AS 'Vorname' FROM t ORDER BY 'Vorname';", "SELECT a AS 'Vorname' FROM t ORDER BY `Vorname`;"),
            ("CREATE TABLE n (id INT, s INT, FOREIGN KEY s REFERENCES u(id));", "CREATE TABLE n (id INT, s INT, FOREIGN KEY (s) REFERENCES u(id));"),
            ("CREATE TABLE n (id INT, PRIMARY KEY id);", "CREATE TABLE n (id INT, PRIMARY KEY (id));"),
            ("CREATE INDEX ix ON t name;", "CREATE INDEX ix ON t (name);"),
            ("ALTER TABLE t RENAME name TO vorname;", "ALTER TABLE t RENAME COLUMN name TO vorname;"),
            ("CREATE TABLE r (id INT, p INT DEFAULT);", "CREATE TABLE r (id INT, p INT);"),
            ("CREATE TABLE r (id INT, n VARCHAR 20);", "CREATE TABLE r (id INT, n VARCHAR(20));"),
            ("CREATE TABLE r (id INT, b DECIMAL(8.2));", "CREATE TABLE r (id INT, b DECIMAL(8,2));"),
            ("CREATE TABLE r (id INT, f ENUM(rot, gelb));", "CREATE TABLE r (id INT, f ENUM('rot', 'gelb'));"),
        ];
        for (sql, want) in cases {
            assert_eq!(fixed(sql), want, "{sql}");
        }
    }

    #[test]
    fn valid_sql_stays() {
        for sql in [
            "SELECT EXTRACT(YEAR FROM '2020-01-01'), TRIM(LEADING 'x' FROM 'xxa') FROM t;",
            "UPDATE t SET rabatt = '10%' WHERE id = 1;",
            "SELECT * FROM t WHERE a = '10%';",
            "SELECT a.x FROM a JOIN b WHERE a.id = b.id;",
            "SELECT a FROM t WHERE x = 1 OR y = 2 ORDER BY a LIMIT 3;",
            "ALTER TABLE t CHANGE datum date DATE;",
            "ALTER TABLE t ADD INDEX (a), ADD PRIMARY KEY (id);",
            "INSERT INTO t (a) (SELECT a FROM u);",
            "INSERT INTO t (a) VALUES (1), (2) ON DUPLICATE KEY UPDATE a = VALUES(a);",
            "SELECT GROUP_CONCAT(a ORDER BY a) FROM t GROUP BY b;",
            "CREATE TABLE t LIKE u;",
            "SELECT a FROM t WHERE b IN (SELECT b FROM u);",
            "SELECT COUNT(*) FROM t WHERE a BETWEEN 1 AND 2;",
            "SET @x := 1;",
            "SELECT @x := 1;",
            "SELECT * FROM t WHERE a LIKE '10%*';",
            "SELECT DATE_FORMAT(d, '%d.%m.%Y') FROM t;",
            "ALTER TABLE t RENAME TO u;",
            "CREATE TABLE r (id INT, PRIMARY KEY (id), KEY ix (id));",
            "CREATE TABLE r (id INT DEFAULT 0, b DECIMAL(8,2));",
            "SELECT a AS x FROM t ORDER BY x;",
            "UPDATE t JOIN u ON t.id = u.id SET t.a = u.a;",
            "DELETE FROM t WHERE code = 'A%';",
            "UPDATE t SET a = 1 WHERE code LIKE 'A*';",
            "WITH x AS (SELECT a FROM t ORDER BY a LIMIT 3) SELECT * FROM x WHERE a > 1;",
            "SELECT a, ROW_NUMBER() OVER (PARTITION BY b ORDER BY c) AS r FROM t;",
            "SELECT a FROM t UNION SELECT a FROM u ORDER BY a LIMIT 5;",
            "SELECT * FROM (SELECT a FROM t ORDER BY a LIMIT 2) AS x WHERE a > 0;",
            "SELECT a, SUM(b) FROM t GROUP BY a WITH ROLLUP;",
            "SELECT * FROM t WHERE id = 1 FOR UPDATE;",
            "ALTER TABLE t ADD COLUMN c INT, ADD d VARCHAR(5);",
            "CREATE TABLE t (s SET('a','b'), e ENUM('x','y'), KEY (s));",
            "UPDATE t SET a = a + 1 ORDER BY id LIMIT 1;",
            "SELECT CASE WHEN a = 1 THEN 'x' ELSE 'y' END FROM t;",
            "INSERT INTO t (a) SELECT a FROM u ORDER BY a;",
            "SELECT CAST(a AS CHAR(5)), CONVERT(b, SIGNED) FROM t;",
            "SELECT * FROM t WHERE a IN (1, 2) AND b NOT IN ('x');",
        ] {
            assert_eq!(fixed(sql), sql);
        }
    }
}

