// Behebungen fuer Fehler, die der Server beim Ausfuehren meldet. Jede Behebung ist die korrigierte
// Anweisung; der SQL-Reiter setzt sie an die Stelle der alten und fuehrt sie erneut aus.
// Bewusst nur eindeutige Korrekturen: nichts wird geraten, was Daten veraendern koennte.

use super::dialect::{group_end, insert_parts, items, text};
use super::lint::{Env, Kind, Tok, apply_fixes, lex, lint, nearest, quote_if_needed};
use crate::db::{Schema, TableInfo};

#[derive(Clone, Debug, PartialEq)]
pub struct ErrorFix {
    /// Beschriftung des Knopfes
    pub label: String,
    /// Die korrigierte Anweisung (kann mehrere Anweisungen enthalten, z. B. ALTER davor)
    pub sql: String,
}

fn fix(label: String, sql: String) -> ErrorFix {
    ErrorFix { label, sql }
}

/// Fehlernummer aus "ERROR 1146 (42S02): ..."
pub fn code(err: &str) -> Option<u32> {
    let rest = &err[err.find("ERROR ")? + 6..];
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Text zwischen `start'` und dem naechsten `'`
fn quoted_after<'a>(err: &'a str, start: &str) -> Option<&'a str> {
    let i = err.find(start)? + start.len();
    let r = &err[i..];
    r.find('\'').map(|e| &r[..e])
}

/// Bereiche (Zeichen) ersetzen; Bereiche duerfen sich nicht ueberschneiden
fn edit(chars: &[char], mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out: Vec<char> = chars.to_vec();
    for (a, b, t) in edits {
        out.splice(a..b, t.chars());
    }
    out.into_iter().collect()
}

/// Nach dem Wort `after` (erstes Vorkommen am Anfang der Anweisung) `insert` einfuegen
fn insert_after_word(chars: &[char], toks: &[Tok], after: &[&str], insert: &str) -> Option<String> {
    let k = toks.iter().take(8).position(|t| after.iter().any(|w| t.is_word(w)))?;
    let at = toks[k].end;
    Some(edit(chars, vec![(at, at, format!(" {insert}"))]))
}

fn has_words(toks: &[Tok], words: &[&str]) -> bool {
    toks.windows(words.len()).any(|w| w.iter().zip(words).all(|(t, x)| t.is_word(x)))
}

/// Tabellen, die in der Anweisung vorkommen (bekannt aus dem Schema)
fn used_tables<'a>(toks: &[Tok], schema: &'a Schema) -> Vec<&'a TableInfo> {
    let mut out: Vec<&TableInfo> = Vec::new();
    for t in toks.iter().filter(|t| t.is_name()) {
        if let Some(info) = schema.tables.iter().find(|x| x.name.eq_ignore_ascii_case(&t.text))
            && !out.iter().any(|o| o.name == info.name)
        {
            out.push(info);
        }
    }
    out
}

/// Alle Behebungen fuer den Fehler `err` der Anweisung `sql`
pub fn for_error(err: &str, sql: &str, schema: Option<&Schema>) -> Vec<ErrorFix> {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let mut out: Vec<ErrorFix> = Vec::new();
    let tr = crate::i18n::tr;
    match code(err) {
        // Sicht, Prozedur, Funktion oder Trigger gibt es schon: ersetzen (beim erneuten Ausfuehren gewollt)
        Some(1050 | 1304 | 1359)
            if toks.first().is_some_and(|t| t.is_word("CREATE"))
                && !has_words(&toks, &["OR", "REPLACE"])
                && toks.iter().take(12).any(|t| ["VIEW", "PROCEDURE", "FUNCTION", "TRIGGER"].iter().any(|w| t.is_word(w))) =>
        {
            let at = toks[0].end;
            out.push(fix(tr("Vorhandene ersetzen (OR REPLACE)", "Replace the existing one (OR REPLACE)").into(), edit(&chars, vec![(at, at, " OR REPLACE".into())])));
        }
        // Gibt es schon / gibt es nicht
        Some(1050) if !has_words(&toks, &["IF", "NOT", "EXISTS"]) => {
            if let Some(s) = insert_after_word(&chars, &toks, &["TABLE"], "IF NOT EXISTS") {
                out.push(fix(tr("Nur anlegen, wenn sie fehlt (IF NOT EXISTS)", "Create only if missing (IF NOT EXISTS)").into(), s));
            }
            if let Some(name) = quoted_after(err, "Table '") {
                out.push(fix(
                    crate::tr_format!("Alte Tabelle „{name}“ ersetzen", "Replace the old table “{name}”"),
                    format!("DROP TABLE IF EXISTS {};\n{}", crate::db::q(name), sql.trim()),
                ));
            }
        }
        Some(1007) if !has_words(&toks, &["IF", "NOT", "EXISTS"]) => {
            if let Some(s) = insert_after_word(&chars, &toks, &["DATABASE", "SCHEMA"], "IF NOT EXISTS") {
                out.push(fix(tr("Nur anlegen, wenn sie fehlt (IF NOT EXISTS)", "Create only if missing (IF NOT EXISTS)").into(), s));
            }
        }
        Some(1051 | 1008) if !has_words(&toks, &["IF", "EXISTS"]) => {
            if let Some(s) = insert_after_word(&chars, &toks, &["TABLE", "TABLES", "DATABASE", "SCHEMA", "VIEW"], "IF EXISTS") {
                out.push(fix(tr("Nur löschen, wenn vorhanden (IF EXISTS)", "Drop only if it exists (IF EXISTS)").into(), s));
            }
        }
        Some(1091) => {
            // ALTER TABLE t DROP [COLUMN|INDEX|KEY|FOREIGN KEY|CONSTRAINT] x -> ... IF EXISTS x
            let mut edits = Vec::new();
            for (k, t) in toks.iter().enumerate() {
                if !t.is_word("DROP") || k < 2 {
                    continue;
                }
                let mut j = k + 1;
                if toks.get(j).is_some_and(|x| x.is_word("FOREIGN")) {
                    j += 1;
                }
                if toks.get(j).is_some_and(|x| ["COLUMN", "INDEX", "KEY", "CONSTRAINT"].iter().any(|w| x.is_word(w))) {
                    j += 1;
                }
                if toks.get(j).is_some_and(|x| x.is_word("IF") || x.is_word("PRIMARY")) {
                    continue;
                }
                if let Some(name) = toks.get(j).filter(|x| x.is_name()) {
                    edits.push((name.start, name.start, "IF EXISTS ".to_string()));
                }
            }
            if !edits.is_empty() {
                out.push(fix(tr("Nur löschen, wenn vorhanden (IF EXISTS)", "Drop only if it exists (IF EXISTS)").into(), edit(&chars, edits)));
            }
        }
        Some(1060) if !has_words(&toks, &["IF", "NOT", "EXISTS"]) => {
            // ADD [COLUMN] x -> ADD [COLUMN] IF NOT EXISTS x
            let mut edits = Vec::new();
            for (k, t) in toks.iter().enumerate() {
                if t.is_word("ADD") {
                    let j = if toks.get(k + 1).is_some_and(|x| x.is_word("COLUMN")) { k + 2 } else { k + 1 };
                    if let Some(name) = toks.get(j).filter(|x| x.is_name() && !super::lint::known_word(&x.upper())) {
                        edits.push((name.start, name.start, "IF NOT EXISTS ".to_string()));
                    }
                }
            }
            if !edits.is_empty() {
                out.push(fix(tr("Nur ergänzen, wenn sie fehlt (IF NOT EXISTS)", "Add only if missing (IF NOT EXISTS)").into(), edit(&chars, edits)));
            }
        }
        // Tabelle unbekannt: aehnlicher Name im Schema
        Some(1146) => {
            if let (Some(full), Some(s)) = (quoted_after(err, "Table '"), schema) {
                let missing = full.rsplit('.').next().unwrap_or(full);
                let names: Vec<&str> = s.tables.iter().map(|t| t.name.as_str()).collect();
                let best = names.iter().find(|n| n.eq_ignore_ascii_case(missing) && **n != missing).copied().or_else(|| nearest(missing, names.iter().copied()));
                if let Some(b) = best {
                    let edits: Vec<_> = toks.iter().filter(|t| t.is_name() && t.text == missing).map(|t| (t.start, t.end, quote_if_needed(b))).collect();
                    if !edits.is_empty() {
                        out.push(fix(crate::tr_format!("„{b}“ verwenden", "Use “{b}”"), edit(&chars, edits)));
                    }
                }
            }
        }
        // Spalte unbekannt: aehnlicher Name in den Tabellen der Anweisung
        Some(1054) => {
            // In VALUES ist ein "unbekannter Name" fast immer Text ohne Anfuehrungszeichen (siehe unten)
            let in_values = insert_parts(&toks).is_some_and(|(_, vi)| quoted_after(err, "Unknown column '").is_some_and(|c| toks[vi..].iter().any(|t| t.is_name() && t.text == c)));
            if !in_values && let (Some(full), Some(s)) = (quoted_after(err, "Unknown column '"), schema) {
                let (qual, col) = match full.rsplit_once('.') {
                    Some((q, c)) => (Some(q), c),
                    None => (None, full),
                };
                let tables = used_tables(&toks, s);
                let tables = if tables.is_empty() { s.tables.iter().collect() } else { tables };
                let mut cols: Vec<&str> = Vec::new();
                for t in &tables {
                    for c in &t.columns {
                        if !cols.contains(&c.name.as_str()) {
                            cols.push(&c.name);
                        }
                    }
                }
                // tabelle.spalte, obwohl die Tabelle einen Alias hat: mit dem Alias schreiben
                if let Some(q) = qual
                    && let Some(alias) = alias_of(&toks, q)
                {
                    let edits: Vec<_> = toks
                        .iter()
                        .enumerate()
                        .filter(|(k, t)| t.is_name() && t.text.eq_ignore_ascii_case(q) && toks.get(k + 1).is_some_and(|d| d.kind == Kind::Dot) && (*k == 0 || toks[k - 1].kind != Kind::Dot))
                        .map(|(_, t)| (t.start, t.end, alias.clone()))
                        .collect();
                    out.push(fix(crate::tr_format!("Den Alias {alias} statt {q} verwenden", "Use the alias {alias} instead of {q}"), edit(&chars, edits)));
                }
                let best = cols.iter().find(|n| n.eq_ignore_ascii_case(col) && **n != col).copied().or_else(|| nearest(col, cols.iter().copied()));
                // Text ohne Anfuehrungszeichen nach einem Vergleich: name = Ada -> name = 'Ada'
                let compared: Vec<_> = toks
                    .iter()
                    .enumerate()
                    .filter(|(k, t)| {
                        qual.is_none() && t.kind == Kind::Word && t.text == col && *k >= 1
                            && (toks[k - 1].kind == Kind::Op && ["=", "<>", "!=", "<", ">", "<=", ">="].contains(&toks[k - 1].text.as_str()) || toks[k - 1].is_word("LIKE"))
                    })
                    .map(|(_, t)| (t.start, t.end, format!("'{}'", t.text)))
                    .collect();
                if best.is_none() && !compared.is_empty() {
                    out.push(fix(crate::tr_format!("„{col}“ ist Text: in Anführungszeichen setzen", "“{col}” is text: put it in quotes"), edit(&chars, compared.clone())));
                }
                if let Some(b) = best {
                    let edits: Vec<_> = toks
                        .iter()
                        .enumerate()
                        .filter(|(k, t)| {
                            t.is_name() && t.text == col && match qual {
                                Some(q) => *k >= 2 && toks[k - 1].kind == Kind::Dot && toks[k - 2].text == q,
                                None => true,
                            }
                        })
                        .map(|(_, t)| (t.start, t.end, quote_if_needed(b)))
                        .collect();
                    if !edits.is_empty() {
                        out.push(fix(crate::tr_format!("„{b}“ verwenden", "Use “{b}”"), edit(&chars, edits)));
                    }
                }
                if best.is_some() && !compared.is_empty() {
                    out.push(fix(crate::tr_format!("„{col}“ ist Text: in Anführungszeichen setzen", "“{col}” is text: put it in quotes"), edit(&chars, compared)));
                }
            }
        }
        // Aggregat in WHERE: Bedingung nach GROUP BY als HAVING
        Some(1111) => {
            if let Some(f) = where_to_having(&chars, &toks) {
                out.push(f);
            }
        }
        // Mehrdeutige Spalte: mit der ersten passenden Tabelle bzw. ihrem Alias qualifizieren
        Some(1052) => {
            if let (Some(col), Some(s)) = (quoted_after(err, "Column '"), schema)
                && let Some(alias) = owner_of(&toks, s, col)
            {
                let edits: Vec<_> = toks
                    .iter()
                    .enumerate()
                    .filter(|(k, t)| {
                        t.is_name()
                            && t.text.eq_ignore_ascii_case(col)
                            && (*k == 0 || toks[k - 1].kind != Kind::Dot && !toks[k - 1].is_word("AS"))
                            && toks.get(k + 1).is_none_or(|n| n.kind != Kind::Dot && n.kind != Kind::Open)
                    })
                    .map(|(_, t)| (t.start, t.start, format!("{alias}.")))
                    .collect();
                if !edits.is_empty() {
                    out.push(fix(crate::tr_format!("Als {alias}.{col} schreiben", "Write it as {alias}.{col}"), edit(&chars, edits)));
                }
            }
        }
        // Unterabfrage liefert mehrere Zeilen: = (SELECT ...) -> IN (SELECT ...)
        Some(1242) => {
            let mut edits = Vec::new();
            for (k, t) in toks.iter().enumerate() {
                if t.kind == Kind::Op && matches!(t.text.as_str(), "=" | "<>" | "!=") && toks.get(k + 1).is_some_and(|o| o.kind == Kind::Open) && toks.get(k + 2).is_some_and(|w| w.is_word("SELECT")) {
                    edits.push((t.start, t.end, if t.text == "=" { "IN" } else { "NOT IN" }.to_string()));
                }
            }
            if !edits.is_empty() {
                out.push(fix(tr("Mehrere Treffer erlauben (IN statt =)", "Allow several matches (IN instead of =)").into(), edit(&chars, edits)));
            }
        }
        // Doppelter Schluessel
        Some(1062) if toks.first().is_some_and(|t| t.is_word("INSERT")) && !toks.get(1).is_some_and(|t| t.is_word("IGNORE")) => {
            if let Some(s) = schema
                && let Some(without) = without_auto_id(&chars, &toks, s)
            {
                out.push(fix(tr("Ohne ID einfügen (MariaDB vergibt sie)", "Insert without the ID (MariaDB assigns it)").into(), without));
            }
            let at = toks[0].end;
            out.push(fix(tr("Vorhandene überspringen (INSERT IGNORE)", "Skip existing rows (INSERT IGNORE)").into(), edit(&chars, vec![(at, at, " IGNORE".into())])));
        }
        // Text zu lang: Spalte verlaengern
        Some(1406) => {
            if let (Some(col), Some(s)) = (quoted_after(err, "for column '").or_else(|| quoted_after(err, "for column `")), schema)
                && let Some(alter) = widen_column(&chars, &toks, s, col.trim_matches('`'))
            {
                out.push(fix(alter.0, format!("{};\n{}", alter.1, sql.trim())));
            }
        }
        // AUTO_INCREMENT braucht einen Schluessel
        Some(1075) => {
            if let Some(k) = toks.iter().position(|t| t.is_word("AUTO_INCREMENT")) {
                let (word, label) = if toks.iter().any(|t| t.is_word("PRIMARY")) {
                    (" UNIQUE", tr("Automatisch gezählte Spalte eindeutig machen (UNIQUE)", "Make the auto-numbered column unique (UNIQUE)"))
                } else {
                    (" PRIMARY KEY", tr("Automatisch gezählte Spalte als Primärschlüssel", "Use the auto-numbered column as primary key"))
                };
                let at = toks[k].end;
                out.push(fix(label.into(), edit(&chars, vec![(at, at, word.into())])));
            }
        }
        // Zwei Primaerschluessel: zu einem gemeinsamen zusammenfassen
        Some(1068) => {
            if let Some(f) = one_primary_key(&chars, &toks) {
                out.push(f);
            }
        }
        // TEXT/BLOB in einem Index: VARCHAR(255) statt TEXT
        Some(1170) => {
            if let Some(col) = quoted_after(err, "column '")
                && let Some(f) = text_key(&chars, &toks, schema, col, sql)
            {
                out.push(f);
            }
        }
        // Index gibt es schon
        Some(1061) if !has_words(&toks, &["IF", "NOT", "EXISTS"]) => {
            if let Some(s) = insert_after_word(&chars, &toks, &["INDEX", "KEY"], "IF NOT EXISTS") {
                out.push(fix(tr("Nur anlegen, wenn er fehlt (IF NOT EXISTS)", "Create only if missing (IF NOT EXISTS)").into(), s));
            }
        }
        // Zieltabelle in der Unterabfrage (MySQL): Unterabfrage in eine abgeleitete Tabelle packen
        Some(1093) => {
            if let Some(t) = quoted_after(err, "target table '").or_else(|| quoted_after(err, "Table '"))
                && let Some(f) = wrap_self_subqueries(&chars, &toks, t)
            {
                out.push(f);
            }
        }
        // Abgeleitete Tabelle ohne Namen
        Some(1248) => {
            if let Some(f) = alias_derived(&chars, &toks) {
                out.push(f);
            }
        }
        // Nicht gruppierte Spalte neben einer Zusammenfassung (MySQL mit ONLY_FULL_GROUP_BY)
        Some(1055 | 1140) => {
            if let Some(f) = group_by_fix(&chars, &toks) {
                out.push(f);
            }
        }
        // UNION mit unterschiedlich vielen Spalten: die kuerzeren mit NULL auffuellen
        Some(1222) => {
            if let Some(f) = union_pad(&chars, &toks) {
                out.push(f);
            }
        }
        // Fremdschluessel nicht moeglich: Datentyp an die Zielspalte anpassen
        Some(1005 | 1215 | 150) => {
            if let Some(s) = schema
                && let Some(f) = fk_type_fix(&chars, &toks, s)
            {
                out.push(f);
            }
        }
        _ => {}
    }
    // Syntax, Datum, Zahl, Anzahl der Werte, Text ohne Anfuehrungszeichen: die Pruefung des Editors weiss es
    if out.is_empty() && matches!(code(err), Some(1064 | 1292 | 1366 | 1136 | 1054 | 1265 | 1305 | 1630)) {
        let env = Env { schema, databases: &[] };
        let problems = lint(sql, &env);
        let (fixed, n) = apply_fixes(sql, &problems);
        if n > 0 && fixed != sql {
            let first = problems.iter().find(|p| p.fix.is_some()).map(|p| p.message.clone()).unwrap_or_default();
            out.push(fix(if n == 1 { first } else { crate::tr_format!("{n} Stellen korrigieren", "Correct {n} places") }, fixed));
        }
    }
    out
}

/// Woerter, die als Tabellen- oder Spaltenname Backticks brauchen und oft so benutzt werden
const RESERVED_NAMES: &[&str] = &[
    "ORDER", "GROUP", "KEY", "KEYS", "INDEX", "TABLE", "DESC", "ASC", "CHECK", "CONDITION", "DEFAULT", "RANGE", "READ", "RELEASE", "REPEAT",
    "RETURN", "SCHEMA", "SET", "SHOW", "LIMIT", "LINES", "LOAD", "LOCK", "MATCH", "OPTION", "OUT", "PARTITION", "PROCEDURE", "REFERENCES",
    "RENAME", "REPLACE", "RESTRICT", "SELECT", "FROM", "WHERE", "USAGE", "USE", "VALUES", "INTERVAL", "LEAVE", "LOOP", "MOD", "WITH", "ROWS",
    "FUNCTION", "CHANGE", "COLUMN", "CROSS", "CURRENT_DATE", "DATABASE", "DELETE", "DIV", "ELSE", "EXIT", "FETCH", "FOR", "GRANT", "IN", "INNER",
    "INSERT", "INT", "IS", "JOIN", "LEFT", "LIKE", "NATURAL", "NOT", "NULL", "ON", "OR", "RIGHT", "TO", "TRIGGER", "UNION", "UNIQUE", "UPDATE",
    "USER_RESOURCES", "WHEN", "WHILE", "ADD", "ALL", "ALTER", "AND", "AS", "BETWEEN", "BY", "CASE", "CREATE", "DROP", "EXISTS", "HAVING", "IF",
    "INTO", "PRIMARY", "SCHEMAS", "SQL", "THEN", "OVER", "RANK", "WINDOW", "SYSTEM", "ROW", "ROW_NUMBER", "LAG", "LEAD",
];

/// Schluesselwoerter, die an einer Stelle mit Syntaxfehler gemeint sein koennen
const SYNTAX_WORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP", "ORDER", "BY", "HAVING", "LIMIT", "OFFSET", "JOIN", "INNER", "LEFT", "RIGHT", "OUTER", "ON", "USING",
    "UNION", "DISTINCT", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "ALTER", "DROP", "ADD", "COLUMN", "MODIFY",
    "LIKE", "BETWEEN", "AND", "OR", "NOT", "NULL", "IS", "IN", "EXISTS", "CASE", "WHEN", "THEN", "ELSE", "END", "AS", "ASC", "DESC", "DEFAULT",
    "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "INDEX", "CONSTRAINT", "AUTO_INCREMENT", "DATABASE", "IF", "TRUNCATE", "VIEW",
    "VARCHAR", "INT", "INTEGER", "DECIMAL", "DATE", "DATETIME", "TEXT", "BOOLEAN", "UNSIGNED", "CHARACTER", "COLLATE", "ENGINE", "CASCADE",
];

/// Funktionen, bei denen MariaDB kein Leerzeichen vor der Klammer erlaubt
const NO_SPACE_FUNCS: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT", "BIT_AND", "BIT_OR", "BIT_XOR", "STD", "STDDEV", "VARIANCE", "SUBSTR", "SUBSTRING", "TRIM", "NOW", "CURDATE", "CAST", "EXTRACT", "POSITION"];

/// Vermutungen fuer einen Syntaxfehler (1064) an der Stelle, die MariaDB nennt ("near '…'").
/// Mehrere Kandidaten; der Probelauf behaelt nur den, mit dem das Skript weiter kommt.
pub fn syntax_guesses(err: &str, sql: &str) -> Vec<ErrorFix> {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let mut out: Vec<ErrorFix> = Vec::new();
    let tr = crate::i18n::tr;
    // MariaDB meldet eine abgeleitete Tabelle ohne Alias als Syntaxfehler am Ende
    if let Some(f) = alias_derived(&chars, &toks) {
        out.push(f);
    }
    // Leerzeichen zwischen Funktion und Klammer (ueberall in der Anweisung)
    let spaced: Vec<(usize, usize, String)> = toks
        .windows(2)
        .filter(|w| w[0].kind == Kind::Word && w[1].kind == Kind::Open && w[1].start > w[0].end && NO_SPACE_FUNCS.contains(&w[0].upper().as_str()))
        .map(|w| (w[0].end, w[1].start, String::new()))
        .collect();
    if !spaced.is_empty() {
        out.push(fix(tr("Kein Leerzeichen zwischen Funktion und Klammer", "No space between function and parenthesis").into(), edit(&chars, spaced)));
    }
    // VARCHAR/CHAR ohne Laenge
    let lengths: Vec<(usize, usize, String)> = toks
        .iter()
        .enumerate()
        .filter(|(k, t)| (t.is_word("VARCHAR") || t.is_word("VARBINARY")) && toks.get(k + 1).is_none_or(|n| n.kind != Kind::Open))
        .map(|(_, t)| (t.end, t.end, "(255)".to_string()))
        .collect();
    if !lengths.is_empty() {
        out.push(fix(tr("VARCHAR braucht eine Länge: VARCHAR(255)", "VARCHAR needs a length: VARCHAR(255)").into(), edit(&chars, lengths)));
    }
    // Stelle des Fehlers
    let Some(i) = err.find("near '") else { return out };
    let rest = &err[i + 6..];
    let near = rest.rfind("' at line").map_or(rest, |e| &rest[..e]);
    let probe: String = near.chars().take(30).collect();
    let Some(byte) = (if probe.trim().is_empty() { None } else { sql.find(probe.as_str()) }) else {
        // Am Ende der Anweisung ("near ''"): fehlende Klammer zu
        if near.is_empty() {
            let depth: i32 = toks.iter().map(|t| match t.kind {
                Kind::Open => 1,
                Kind::Close => -1,
                _ => 0,
            }).sum();
            if depth > 0 {
                let end = sql.trim_end().trim_end_matches(';').trim_end().chars().count();
                out.push(fix(tr("Klammer schließen", "Close the parenthesis").into(), edit(&chars, vec![(end, end, ")".repeat(depth as usize))])));
            }
        }
        return out;
    };
    let at = sql[..byte].chars().count();
    let Some(k) = toks.iter().position(|t| t.start >= at) else { return out };
    let t = &toks[k];
    let prev = k.checked_sub(1).map(|j| &toks[j]);
    // Apostroph im Text: 'O'Brien' -> 'O''Brien'
    if t.kind == Kind::Word && prev.is_some_and(|p| p.kind == Kind::Str && p.end == t.start) && chars.get(t.end) == Some(&'\'') {
        let p = prev.map_or(at, |p| p.end - 1);
        out.push(fix(tr("Apostroph im Text verdoppeln ('')", "Double the apostrophe inside the text ('')").into(), edit(&chars, vec![(p, p, "'".into())])));
    }
    // DELETE * FROM -> DELETE FROM
    if t.kind == Kind::Op && t.text == "*" && prev.is_some_and(|p| p.is_word("DELETE")) {
        out.push(fix(tr("DELETE ohne * schreiben", "Write DELETE without *").into(), edit(&chars, vec![(prev.map_or(t.start, |p| p.end), t.end, String::new())])));
    }
    // Reserviertes Wort als Name -> `Name`
    let name_pos = prev.is_some_and(|p| ["TABLE", "INTO", "FROM", "JOIN", "UPDATE", "REFERENCES", "EXISTS"].iter().any(|w| p.is_word(w)) || matches!(p.kind, Kind::Comma | Kind::Open | Kind::Dot));
    if t.kind == Kind::Word && name_pos && RESERVED_NAMES.contains(&t.upper().as_str()) {
        let word = t.upper();
        let edits: Vec<(usize, usize, String)> = toks
            .iter()
            .enumerate()
            .filter(|(j, x)| {
                x.kind == Kind::Word && x.upper() == word && *j > 0
                    && (["TABLE", "INTO", "FROM", "JOIN", "UPDATE", "REFERENCES", "EXISTS"].iter().any(|w| toks[j - 1].is_word(w)) || toks[j - 1].kind == Kind::Dot || *j == k)
            })
            .map(|(_, x)| (x.start, x.end, format!("`{}`", x.text)))
            .collect();
        out.push(fix(crate::tr_format!("{} ist ein reserviertes Wort: als `{}` schreiben", "{} is a reserved word: write it as `{}`", t.text, t.text), edit(&chars, edits)));
    }
    // Vertipptes Schluesselwort an der Stelle
    if t.kind == Kind::Word && !super::lint::known_word(&t.upper())
        && let Some(w) = nearest(&t.upper(), SYNTAX_WORDS.iter().copied())
    {
        out.push(fix(crate::tr_format!("„{}“ statt „{}“", "“{}” instead of “{}”", w, t.text), edit(&chars, vec![(t.start, t.end, w.to_string())])));
    }
    // Fehlendes Komma vor der Stelle (Spalten, Werte, SET-Zuweisungen)
    let ends_value = prev.is_some_and(|p| matches!(p.kind, Kind::Word | Kind::Quoted | Kind::Str | Kind::Num | Kind::Close | Kind::Var) && !(p.kind == Kind::Word && SYNTAX_WORDS.contains(&p.upper().as_str()) && !p.is_word("NULL")));
    let starts_value = matches!(t.kind, Kind::Word | Kind::Quoted | Kind::Str | Kind::Num | Kind::Var) && !(t.kind == Kind::Word && SYNTAX_WORDS.contains(&t.upper().as_str()) && !t.is_word("NULL"));
    if ends_value && starts_value {
        let p = prev.map_or(t.start, |p| p.end);
        out.push(fix(tr("Fehlendes Komma ergänzen", "Add the missing comma").into(), edit(&chars, vec![(p, p, ",".into())])));
    }
    out
}

/// Alias der Tabelle `table` in FROM/JOIN ("FROM schueler s" -> "s")
fn alias_of(toks: &[Tok], table: &str) -> Option<String> {
    for (k, t) in toks.iter().enumerate() {
        let names_follow = t.is_word("FROM") || t.is_word("JOIN") || t.is_word("UPDATE") || t.kind == Kind::Comma;
        if !names_follow || !toks.get(k + 1).is_some_and(|x| x.is_name() && x.text.eq_ignore_ascii_case(table)) {
            continue;
        }
        let mut j = k + 2;
        if toks.get(j).is_some_and(|x| x.is_word("AS")) {
            j += 1;
        }
        return toks.get(j).filter(|x| x.kind == Kind::Word && !super::lint::known_word(&x.upper()) || x.kind == Kind::Quoted).map(|a| a.text.clone());
    }
    None
}

/// "WHERE COUNT(*) > 1 GROUP BY x" -> "GROUP BY x HAVING COUNT(*) > 1" (nur wenn die ganze WHERE-Bedingung ein Aggregat ist)
fn where_to_having(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    const AGG: &[&str] = &["COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT"];
    let depth_at = |k: usize| toks[..k].iter().map(|t| match t.kind { Kind::Open => 1i32, Kind::Close => -1, _ => 0 }).sum::<i32>();
    let w = toks.iter().enumerate().position(|(k, t)| t.is_word("WHERE") && depth_at(k) == 0)?;
    let g = toks.iter().enumerate().position(|(k, t)| t.is_word("GROUP") && toks.get(k + 1).is_some_and(|b| b.is_word("BY")) && k > w && depth_at(k) == 0)?;
    let cond = &toks[w + 1..g];
    if cond.is_empty() || !cond.iter().enumerate().any(|(j, t)| AGG.contains(&t.upper().as_str()) && cond.get(j + 1).is_some_and(|o| o.kind == Kind::Open)) {
        return None;
    }
    // Ende von GROUP BY: HAVING, ORDER, LIMIT oder das Ende
    let end = toks.iter().enumerate().skip(g + 2).find(|(k, t)| depth_at(*k) == 0 && (t.is_word("HAVING") || t.is_word("ORDER") || t.is_word("LIMIT") || t.kind == Kind::Semi)).map(|(k, _)| k);
    let cond_text = text(chars, cond[0].start, cond[cond.len() - 1].end);
    let group_end = end.map_or_else(|| toks.last().map_or(chars.len(), |t| t.end), |k| toks[k - 1].end);
    let mut edits = vec![(toks[w].start, toks[g].start, String::new())];
    match end.map(|k| &toks[k]) {
        Some(h) if h.is_word("HAVING") => edits.push((h.end, h.end, format!(" {cond_text} AND"))),
        _ => edits.push((group_end, group_end, format!(" HAVING {cond_text}"))),
    }
    Some(fix(crate::i18n::tr("Bedingung mit Aggregat gehört nach GROUP BY (HAVING)", "A condition with an aggregate belongs after GROUP BY (HAVING)").into(), edit(chars, edits)))
}

/// Zuletzt per CREATE DATABASE oder USE genannte Datenbank in `script` (fuer "No database selected")
pub fn created_database(script: &str) -> Option<String> {
    let chars: Vec<char> = script.chars().collect();
    let toks = lex(&chars).toks;
    let mut found = None;
    for (k, t) in toks.iter().enumerate() {
        let name_at = if t.is_word("USE") {
            Some(k + 1)
        } else if t.is_word("CREATE") && toks.get(k + 1).is_some_and(|x| x.is_word("DATABASE") || x.is_word("SCHEMA")) {
            let j = k + 2;
            Some(if toks.get(j).is_some_and(|x| x.is_word("IF")) { j + 3 } else { j })
        } else {
            None
        };
        if let Some(n) = name_at.and_then(|j| toks.get(j)).filter(|x| x.is_name()) {
            found = Some(n.text.clone());
        }
    }
    found
}

/// Fehlt eine Tabelle (1146), die weiter oben im Editor angelegt, aber nicht mit ausgefuehrt wurde: Beginn dieses
/// CREATE TABLE in `before` (Zeichenindex). Alles zwischen CREATE und der Fehlerstelle muss gefahrlos wiederholbar sein.
pub fn vorher_angelegt(before: &str, err: &str) -> Option<usize> {
    if code(err) != Some(1146) {
        return None;
    }
    let name = err.split('\'').nth(1)?;
    let name = name.rsplit('.').next().unwrap_or(name).trim_matches('`');
    let stmts = crate::db::split_statements(before);
    let k = stmts.iter().rposition(|st| {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        let mut i = 0;
        if !(toks.first().is_some_and(|t| t.is_word("CREATE")) && toks.get(1).is_some_and(|t| t.is_word("TABLE"))) {
            return false;
        }
        i += 2;
        if toks.get(i).is_some_and(|t| t.is_word("IF")) {
            i += 3;
        }
        let mut n = toks.get(i);
        if toks.get(i + 1).is_some_and(|t| t.kind == Kind::Dot) {
            n = toks.get(i + 2);
        }
        n.is_some_and(|n| n.is_name() && n.text.eq_ignore_ascii_case(name))
    })?;
    let harmlos = |sql: &str| {
        let u = sql.trim_start().to_ascii_uppercase();
        ["SELECT", "SHOW", "USE", "SET", "DESCRIBE", "DESC ", "EXPLAIN"].iter().any(|w| u.starts_with(w)) || (u.starts_with("CREATE") && u.contains("IF NOT EXISTS"))
    };
    stmts[k + 1..].iter().all(|st| harmlos(&st.sql)).then_some(stmts[k].start)
}

/// Tabellennamen nach FROM, JOIN, INTO, UPDATE, TABLE
pub fn table_names(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let mut out: Vec<String> = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        if ["FROM", "JOIN", "INTO", "UPDATE", "TABLE"].iter().any(|w| t.is_word(w))
            && let Some(n) = toks.get(k + 1).filter(|x| x.is_name() && !super::lint::known_word(&x.upper()))
            && toks.get(k + 2).is_none_or(|x| x.kind != Kind::Dot)
            && !out.contains(&n.text)
        {
            out.push(n.text.clone());
        }
    }
    out
}

/// Tabelle bzw. Alias in FROM/JOIN, deren Tabelle die Spalte `col` hat
fn owner_of(toks: &[Tok], schema: &Schema, col: &str) -> Option<String> {
    for (k, t) in toks.iter().enumerate() {
        if !(t.is_word("FROM") || t.is_word("JOIN") || t.is_word("UPDATE")) {
            continue;
        }
        let Some(name) = toks.get(k + 1).filter(|x| x.is_name()) else { continue };
        let Some(info) = schema.tables.iter().find(|x| x.name.eq_ignore_ascii_case(&name.text)) else { continue };
        if !info.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
            continue;
        }
        let mut j = k + 2;
        if toks.get(j).is_some_and(|x| x.is_word("AS")) {
            j += 1;
        }
        let alias = toks.get(j).filter(|x| x.kind == Kind::Word && !super::lint::known_word(&x.upper()) || x.kind == Kind::Quoted);
        return Some(alias.map_or_else(|| name.text.clone(), |a| a.text.clone()));
    }
    None
}

/// INSERT mit Wert fuer die automatisch vergebene ID: ohne diese Spalte und ihre Werte
pub(super) fn without_auto_id(chars: &[char], toks: &[Tok], schema: &Schema) -> Option<String> {
    let (ti, vi) = insert_parts(toks)?;
    let table = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&toks[ti].text))?;
    let auto = table.columns.iter().find(|c| c.extra.to_ascii_lowercase().contains("auto_increment"))?;
    // Spaltenliste vorhanden: Position der ID darin; sonst ist es die Position in der Tabelle
    let list = (toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open)).then_some(ti + 1);
    let cols: Vec<String> = match list {
        Some(o) => {
            let c = group_end(toks, o)?;
            items(toks, o, c).iter().map(|(a, _)| toks[*a].text.clone()).collect()
        }
        None => table.columns.iter().filter(|c| !c.is_generated()).map(|c| c.name.clone()).collect(),
    };
    let pos = cols.iter().position(|c| c.eq_ignore_ascii_case(&auto.name))?;
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    // Ein Eintrag samt Komma davor (bzw. danach beim ersten)
    let remove = |edits: &mut Vec<(usize, usize, String)>, open: usize, close: usize| -> Option<()> {
        let its = items(toks, open, close);
        if its.len() < 2 {
            return None;
        }
        let (a, b) = *its.get(pos)?;
        let (s, e) = if pos == 0 { (toks[a].start, toks[its[1].0].start) } else { (toks[its[pos - 1].1 - 1].end, toks[b - 1].end) };
        edits.push((s, e, String::new()));
        Some(())
    };
    match list {
        Some(o) => remove(&mut edits, o, group_end(toks, o)?)?,
        None => {
            let names: Vec<String> = cols.iter().filter(|c| !c.eq_ignore_ascii_case(&auto.name)).map(|c| quote_if_needed(c)).collect();
            edits.push((toks[ti].end, toks[ti].end, format!(" ({})", names.join(", "))));
        }
    }
    let mut k = vi + 1;
    while let Some(t) = toks.get(k) {
        match t.kind {
            Kind::Open => {
                let c = group_end(toks, k)?;
                remove(&mut edits, k, c)?;
                k = c + 1;
            }
            Kind::Comma => k += 1,
            _ => break,
        }
    }
    Some(edit(chars, edits))
}

/// "Data too long for column": ALTER TABLE ... MODIFY mit genug Platz fuer den laengsten Text der Anweisung
fn widen_column(chars: &[char], toks: &[Tok], schema: &Schema, col: &str) -> Option<(String, String)> {
    let table = used_tables(toks, schema).into_iter().find(|t| t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)))?;
    let info = table.columns.iter().find(|c| c.name.eq_ignore_ascii_case(col))?;
    let t = info.col_type.to_ascii_lowercase();
    let base = if t.starts_with("varchar") { "VARCHAR" } else if t.starts_with("char") { "CHAR" } else { return None };
    let current: usize = t.split_once('(').and_then(|(_, r)| r.trim_end_matches(')').parse().ok())?;
    let longest = toks.iter().filter(|x| x.kind == Kind::Str).map(|x| x.end - x.start - 2).max().unwrap_or(0);
    let need = longest.max(current + 1);
    let size = [50, 100, 255, 500, 1000, 2000, 4000].into_iter().find(|s| *s >= need && *s > current);
    let new_type = match size {
        Some(s) => format!("{base}({s})"),
        None => "TEXT".into(),
    };
    let mut def = format!("ALTER TABLE {} MODIFY {} {new_type}", quote_if_needed(&table.name), quote_if_needed(&info.name));
    if !info.nullable {
        def.push_str(" NOT NULL");
    }
    // MariaDB liefert den Standardwert schon als SQL-Ausdruck ('text', 0, current_timestamp())
    if let Some(d) = &info.default {
        def.push_str(&format!(" DEFAULT {d}"));
    }
    let _ = chars;
    Some((crate::tr_format!("Spalte {} auf {new_type} vergrößern", "Widen column {} to {new_type}", info.name), def))
}

/// Mehrere Primaerschluessel in CREATE TABLE (Spalte PRIMARY KEY und/oder PRIMARY KEY (…)) zu einem vereinen
fn one_primary_key(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    if !toks.first()?.is_word("CREATE") {
        return None;
    }
    let open = toks.iter().position(|t| t.kind == Kind::Open)?;
    let close = group_end(toks, open)?;
    let mut cols: Vec<String> = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (a, b) in items(toks, open, close) {
        let Some(p) = (a..b.saturating_sub(1)).find(|&k| toks[k].is_word("PRIMARY") && toks[k + 1].is_word("KEY")) else { continue };
        let table_level = toks[a].is_word("PRIMARY") || toks[a].is_word("CONSTRAINT");
        if table_level {
            let g = (p + 2..b).find(|&k| toks[k].kind == Kind::Open)?;
            let ge = group_end(toks, g)?;
            for t in &toks[g + 1..ge] {
                if t.is_name() && !cols.iter().any(|c| c.eq_ignore_ascii_case(&t.text)) {
                    cols.push(t.text.clone());
                }
            }
            // Eintrag samt Komma davor entfernen
            let from = if toks[a - 1].kind == Kind::Comma { toks[a - 1].start } else { toks[a].start };
            edits.push((from, toks[b - 1].end, String::new()));
        } else {
            if !cols.iter().any(|c| c.eq_ignore_ascii_case(&toks[a].text)) {
                cols.push(toks[a].text.clone());
            }
            edits.push((toks[p - 1].end, toks[p + 1].end, String::new()));
        }
    }
    if edits.len() < 2 || cols.is_empty() {
        return None;
    }
    let list: Vec<String> = cols.iter().map(|c| quote_if_needed(c)).collect();
    let at = toks[close - 1].end;
    edits.push((at, at, format!(",\n  PRIMARY KEY ({})", list.join(", "))));
    Some(fix(crate::tr_format!("Einen gemeinsamen Primärschlüssel ({}) bilden", "Use one combined primary key ({})", list.join(", ")), edit(chars, edits)))
}

/// TEXT/BLOB-Spalte in einem Index: im CREATE TABLE VARCHAR(255) verwenden, sonst die Spalte vorher aendern
fn text_key(chars: &[char], toks: &[Tok], schema: Option<&Schema>, col: &str, sql: &str) -> Option<ErrorFix> {
    let label = crate::tr_format!("{col} als VARCHAR(255) statt TEXT (Text im Index braucht eine Länge)", "{col} as VARCHAR(255) instead of TEXT (text in an index needs a length)");
    if toks.first()?.is_word("CREATE")
        && let Some(open) = toks.iter().position(|t| t.kind == Kind::Open)
        && let Some(close) = group_end(toks, open)
    {
        for (a, _) in items(toks, open, close) {
            if toks[a].is_name() && toks[a].text.eq_ignore_ascii_case(col) && let Some(t) = toks.get(a + 1) {
                let new = match t.upper().as_str() {
                    "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" => "VARCHAR(255)",
                    "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => "VARBINARY(255)",
                    _ => return None,
                };
                return Some(fix(label, edit(chars, vec![(t.start, t.end, new.into())])));
            }
        }
    }
    let table = used_tables(toks, schema?).into_iter().find(|t| t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)))?;
    let info = table.columns.iter().find(|c| c.name.eq_ignore_ascii_case(col))?;
    if !info.col_type.to_ascii_lowercase().contains("text") {
        return None;
    }
    let null = if info.nullable { "" } else { " NOT NULL" };
    Some(fix(label, format!("ALTER TABLE {} MODIFY {} VARCHAR(255){null};\n{}", quote_if_needed(&table.name), quote_if_needed(&info.name), sql.trim())))
}

/// Unterabfragen, die die Zieltabelle lesen: (SELECT …) -> (SELECT * FROM (SELECT …) AS _t1)
fn wrap_self_subqueries(chars: &[char], toks: &[Tok], target: &str) -> Option<ErrorFix> {
    let target = target.rsplit('.').next().unwrap_or(target);
    let mut edits = Vec::new();
    let mut k = 0;
    let mut n = 0;
    while k + 1 < toks.len() {
        if toks[k].kind == Kind::Open && toks[k + 1].is_word("SELECT") {
            let end = group_end(toks, k)?;
            let reads = toks[k..end].windows(2).any(|w| (w[0].is_word("FROM") || w[0].is_word("JOIN")) && w[1].is_name() && w[1].text.eq_ignore_ascii_case(target));
            if reads {
                n += 1;
                edits.push((toks[k].end, toks[k].end, "SELECT * FROM (".to_string()));
                edits.push((toks[end].start, toks[end].start, format!(") AS _t{n}")));
                k = end + 1;
                continue;
            }
        }
        k += 1;
    }
    if edits.is_empty() {
        return None;
    }
    Some(fix(crate::i18n::tr("Unterabfrage zuerst auswerten (abgeleitete Tabelle)", "Evaluate the subquery first (derived table)").into(), edit(chars, edits)))
}

/// Abgeleitete Tabellen ohne Alias: FROM (SELECT …) -> FROM (SELECT …) AS t1
fn alias_derived(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    let mut edits = Vec::new();
    let mut n = 0;
    for k in 1..toks.len() {
        let after_from = toks[k - 1].is_word("FROM") || toks[k - 1].is_word("JOIN") || toks[k - 1].kind == Kind::Comma;
        if toks[k].kind != Kind::Open || !after_from || !toks.get(k + 1).is_some_and(|t| t.is_word("SELECT")) {
            continue;
        }
        let end = group_end(toks, k)?;
        let named = toks.get(end + 1).is_some_and(|t| t.is_word("AS") || (t.is_name() && !super::lint::known_word(&t.upper())));
        if !named {
            n += 1;
            edits.push((toks[end].end, toks[end].end, format!(" AS t{n}")));
        }
    }
    if edits.is_empty() {
        return None;
    }
    Some(fix(crate::i18n::tr("Unterabfrage benennen (AS t1)", "Name the subquery (AS t1)").into(), edit(chars, edits)))
}

/// Bereich der SELECT-Liste ab `from_k`: (Index von SELECT, Ende der Liste, Eintraege)
type SelectList = (usize, usize, Vec<(usize, usize)>);

fn select_list(toks: &[Tok], from_k: usize, to: usize) -> Option<SelectList> {
    let s = (from_k..to).find(|&k| toks[k].is_word("SELECT"))?;
    let mut depth = 0i32;
    let mut end = to;
    for (k, t) in toks.iter().enumerate().take(to).skip(s + 1) {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ if depth == 0 && (t.is_word("FROM") || t.is_word("UNION") || t.is_word("EXCEPT") || t.is_word("INTERSECT") || t.kind == Kind::Semi) => {
                end = k;
                break;
            }
            _ => {}
        }
    }
    let mut a = s + 1;
    while toks.get(a).is_some_and(|t| t.is_word("DISTINCT") || t.is_word("ALL") || t.is_word("DISTINCTROW")) {
        a += 1;
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = a;
    for (k, t) in toks.iter().enumerate().take(end).skip(a) {
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
    if start < end {
        out.push((start, end));
    }
    Some((s, end, out))
}

/// Nicht zusammengefasste Spalten der SELECT-Liste ins GROUP BY aufnehmen (bzw. GROUP BY anlegen)
fn group_by_fix(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    const AGG: [&str; 9] = ["COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT", "STD", "STDDEV", "VARIANCE"];
    let (_, from, list) = select_list(toks, 0, toks.len())?;
    let text = |a: usize, b: usize| chars[toks[a].start..toks[b - 1].end].iter().collect::<String>();
    let norm = |x: &str| x.chars().filter(|c| !c.is_whitespace() && *c != '`').collect::<String>().to_lowercase();
    let mut wanted: Vec<String> = Vec::new();
    for (a, b) in list {
        let item = &toks[a..b];
        if item.windows(2).any(|w| w[1].kind == Kind::Open && AGG.iter().any(|g| w[0].is_word(g))) {
            continue;
        }
        if item.iter().all(|t| matches!(t.kind, Kind::Str | Kind::Num) || t.kind == Kind::Op) {
            continue;
        }
        // Ausdruck ohne Alias (AS x bzw. nachgestellter Name)
        let mut e = b;
        if let Some(p) = (a..b).rev().find(|&k| toks[k].is_word("AS")) {
            e = p;
        } else if b - a >= 2 && toks[b - 1].is_name() && !toks[b - 2].is_word("AS") && toks[b - 2].kind != Kind::Dot && toks[b - 2].kind != Kind::Op {
            e = b - 1;
        }
        if e > a {
            wanted.push(text(a, e));
        }
    }
    // Vorhandenes GROUP BY und wo die Liste endet
    let mut depth = 0i32;
    let mut group: Option<usize> = None;
    let mut tail: Option<usize> = None;
    for k in from..toks.len() {
        match toks[k].kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ if depth == 0 && toks[k].is_word("GROUP") && toks.get(k + 1).is_some_and(|t| t.is_word("BY")) => group = Some(k + 2),
            _ if depth == 0 && (["HAVING", "ORDER", "LIMIT", "WINDOW", "UNION"].iter().any(|w| toks[k].is_word(w)) || toks[k].kind == Kind::Semi) => {
                tail = Some(k);
                break;
            }
            _ => {}
        }
    }
    let tail = tail.unwrap_or(toks.len());
    let have: Vec<String> = match group {
        Some(g) => {
            let mut v = Vec::new();
            let mut st = g;
            for (k, t) in toks.iter().enumerate().take(tail).skip(g) {
                if t.kind == Kind::Comma {
                    if st < k {
                        v.push(norm(&text(st, k)));
                    }
                    st = k + 1;
                }
            }
            if st < tail {
                v.push(norm(&text(st, tail)));
            }
            v
        }
        None => Vec::new(),
    };
    let missing: Vec<String> = wanted.into_iter().filter(|w| !have.contains(&norm(w))).collect();
    if missing.is_empty() {
        return None;
    }
    let at = toks[tail - 1].end;
    let ins = if group.is_some() { format!(", {}", missing.join(", ")) } else { format!(" GROUP BY {}", missing.join(", ")) };
    Some(fix(crate::tr_format!("Nach {} gruppieren", "Group by {}", missing.join(", ")), edit(chars, vec![(at, at, ins)])))
}

/// UNION/EXCEPT/INTERSECT: kuerzere SELECT-Listen mit NULL auf dieselbe Spaltenzahl bringen
fn union_pad(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0i32;
    let mut st = 0;
    for (k, t) in toks.iter().enumerate() {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ if depth == 0 && (t.is_word("UNION") || t.is_word("EXCEPT") || t.is_word("INTERSECT")) => {
                parts.push((st, k));
                st = k + 1;
            }
            _ => {}
        }
    }
    parts.push((st, toks.len()));
    if parts.len() < 2 {
        return None;
    }
    let lists: Vec<(usize, usize, usize)> = parts.iter().filter_map(|&(a, b)| select_list(toks, a, b).map(|(_, end, l)| (end, l.len(), l.last().map_or(0, |x| x.1)))).collect();
    let max = lists.iter().map(|l| l.1).max()?;
    let mut edits = Vec::new();
    for (_, n, last) in &lists {
        if *n < max && *last > 0 {
            let at = toks[last - 1].end;
            edits.push((at, at, ", NULL".repeat(max - n)));
        }
    }
    if edits.is_empty() {
        return None;
    }
    Some(fix(crate::i18n::tr("Fehlende Spalten mit NULL auffüllen", "Fill the missing columns with NULL").into(), edit(chars, edits)))
}

/// Typ einer Spalte ohne Anzeigebreite: "int(11) unsigned" -> "INT UNSIGNED"
pub(super) fn norm_type(t: &str) -> String {
    let t = t.to_ascii_uppercase();
    let (base, rest) = match t.split_once('(') {
        Some((b, r)) => (b.trim().to_string(), r.split_once(')').map(|(args, tail)| (args.to_string(), tail.trim().to_string()))),
        None => (t.split_whitespace().next().unwrap_or("").to_string(), Some((String::new(), t.split_whitespace().skip(1).collect::<Vec<_>>().join(" ")))),
    };
    let (args, tail) = rest.unwrap_or_default();
    let int = matches!(base.as_str(), "INT" | "INTEGER" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "BIGINT");
    let base = if base == "INTEGER" { "INT".to_string() } else { base };
    let mut out = if int || args.is_empty() { base } else { format!("{base}({args})") };
    if tail.contains("UNSIGNED") {
        out.push_str(" UNSIGNED");
    }
    out
}

/// CREATE/ALTER TABLE mit Fremdschluessel auf eine Spalte anderen Typs: Typ der eigenen Spalte angleichen
fn fk_type_fix(chars: &[char], toks: &[Tok], schema: &Schema) -> Option<ErrorFix> {
    for (k, t) in toks.iter().enumerate() {
        if !t.is_word("REFERENCES") {
            continue;
        }
        let target = toks.get(k + 1).filter(|x| x.is_name())?;
        let Some(info) = schema.tables.iter().find(|x| x.name.eq_ignore_ascii_case(&target.text)) else { continue };
        // Zielspalte
        let ref_col = match toks.get(k + 2) {
            Some(o) if o.kind == Kind::Open => toks.get(k + 3).map(|x| x.text.clone()),
            _ => info.pk_columns().first().cloned(),
        }?;
        let ref_info = info.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&ref_col))?;
        // Eigene Spalte: FOREIGN KEY (x) davor, sonst die Spalte dieser Definition
        let own = if k >= 3 && toks[k - 1].kind == Kind::Close && toks[k - 3].kind == Kind::Open { toks[k - 2].text.clone() } else { continue };
        // Definition der eigenen Spalte in derselben Anweisung: "(own TYP" bzw. ", own TYP"
        let def = toks.iter().enumerate().position(|(j, x)| j >= 1 && x.text.eq_ignore_ascii_case(&own) && x.is_name() && matches!(toks[j - 1].kind, Kind::Open | Kind::Comma) && toks.get(j + 1).is_some_and(|n| n.kind == Kind::Word && crate::sqlfmt::is_type(&n.text)))?;
        let ty = def + 1;
        let mut end = ty;
        if toks.get(ty + 1).is_some_and(|x| x.kind == Kind::Open) {
            end = group_end(toks, ty + 1)?;
        }
        if toks.get(end + 1).is_some_and(|x| x.is_word("UNSIGNED")) {
            end += 1;
        }
        let have = norm_type(&text(chars, toks[ty].start, toks[end].end));
        let want = norm_type(&ref_info.col_type);
        if have == want {
            continue;
        }
        return Some(fix(
            crate::tr_format!("Typ von {own} an {}.{} anpassen ({want})", "Match the type of {own} to {}.{} ({want})", info.name, ref_info.name),
            edit(chars, vec![(toks[ty].start, toks[end].end, want)]),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn syntax_guesses_at_the_reported_place() {
        let near = |n: &str| format!("ERROR 1064 (42000): You have an error in your SQL syntax; check the manual that corresponds to your MariaDB server version for the right syntax to use near '{n}' at line 1");
        let has = |err: &str, sql: &str, want: &str| syntax_guesses(err, sql).iter().any(|f| f.sql == want);
        assert!(has(&near("COUNT(*) FROM s GROUP BY k"), "SELECT k COUNT(*) FROM s GROUP BY k", "SELECT k, COUNT(*) FROM s GROUP BY k"));
        assert!(has(&near("1)"), "INSERT INTO s (a, b) VALUES ('x' 1)", "INSERT INTO s (a, b) VALUES ('x', 1)"));
        assert!(has(&near("DECS"), "SELECT * FROM s ORDER BY a DECS", "SELECT * FROM s ORDER BY a DESC"));
        assert!(has(&near("* FROM s WHERE id = 2"), "DELETE * FROM s WHERE id = 2", "DELETE FROM s WHERE id = 2"));
        assert!(has(&near("*) FROM s"), "SELECT COUNT (*) FROM s", "SELECT COUNT(*) FROM s"));
        assert!(has(&near("order (id INT)"), "CREATE TABLE order (id INT)", "CREATE TABLE `order` (id INT)"));
        assert!(has(&near(")"), "CREATE TABLE t (id INT, name VARCHAR)", "CREATE TABLE t (id INT, name VARCHAR(255))"));
    }

    #[test]
    fn aggregate_in_where_moves_to_having() {
        let sql = "SELECT k FROM s WHERE COUNT(*) > 1 GROUP BY k ORDER BY k";
        let f = for_error("ERROR 1111 (HY000): Invalid use of group function", sql, None);
        assert_eq!(f[0].sql, "SELECT k FROM s GROUP BY k HAVING COUNT(*) > 1 ORDER BY k");
    }

    use super::*;
    use crate::db::ColumnInfo;

    fn col(name: &str, t: &str, extra: &str, key: &str) -> ColumnInfo {
        ColumnInfo { name: name.into(), col_type: t.into(), extra: extra.into(), key: key.into(), ..Default::default() }
    }

    fn schema() -> Schema {
        Schema {
            name: "person".into(),
            tables: vec![
                TableInfo {
                    name: "Personen".into(),
                    columns: vec![col("pid", "int(10) unsigned", "auto_increment", "PRI"), col("vorname", "varchar(20)", "", ""), col("nachname", "varchar(100)", "", "")],
                    ..Default::default()
                },
                TableInfo { name: "Email".into(), columns: vec![col("pid", "int(11)", "", ""), col("email", "varchar(200)", "", "PRI")], ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn first(err: &str, sql: &str) -> String {
        let s = schema();
        for_error(err, sql, Some(&s)).into_iter().next().map(|f| f.sql).unwrap_or_else(|| panic!("keine Behebung fuer {err}"))
    }

    #[test]
    fn exists_and_missing() {
        assert_eq!(first("ERROR 1050 (42S01): Table 'Personen' already exists", "CREATE TABLE Personen (pid INT)"), "CREATE TABLE IF NOT EXISTS Personen (pid INT)");
        let s = schema();
        let all = for_error("ERROR 1050 (42S01): Table 'Personen' already exists", "CREATE TABLE Personen (pid INT)", Some(&s));
        assert_eq!(all[1].sql, "DROP TABLE IF EXISTS `Personen`;\nCREATE TABLE Personen (pid INT)");
        assert_eq!(first("ERROR 1007 (HY000): Can't create database 'x'; database exists", "create database x"), "create database IF NOT EXISTS x");
        assert_eq!(first("ERROR 1051 (42S02): Unknown table 'person.x'", "DROP TABLE x"), "DROP TABLE IF EXISTS x");
        assert_eq!(first("ERROR 1091 (42000): Can't DROP COLUMN `a`; check that it exists", "ALTER TABLE t DROP COLUMN a"), "ALTER TABLE t DROP COLUMN IF EXISTS a");
        assert_eq!(first("ERROR 1060 (42S21): Duplicate column name 'a'", "ALTER TABLE t ADD a INT"), "ALTER TABLE t ADD IF NOT EXISTS a INT");
    }

    #[test]
    fn grouping_aliases_and_unions() {
        assert_eq!(first("ERROR 1248 (42000): Every derived table must have its own alias", "SELECT * FROM (SELECT id FROM t)"), "SELECT * FROM (SELECT id FROM t) AS t1");
        assert_eq!(
            first("ERROR 1140 (42000): In aggregated query without GROUP BY, expression #1 of SELECT list contains nonaggregated column 'db.t.klasse'", "SELECT klasse, COUNT(*) AS n FROM t ORDER BY n"),
            "SELECT klasse, COUNT(*) AS n FROM t GROUP BY klasse ORDER BY n"
        );
        assert_eq!(
            first("ERROR 1055 (42000): Expression #2 of SELECT list is not in GROUP BY clause", "SELECT k.name, s.ort AS o, COUNT(*) FROM t s JOIN k ON 1 GROUP BY k.name;"),
            "SELECT k.name, s.ort AS o, COUNT(*) FROM t s JOIN k ON 1 GROUP BY k.name, s.ort;"
        );
        assert_eq!(first("ERROR 1222 (21000): The used SELECT statements have a different number of columns", "SELECT a, b FROM x UNION SELECT c FROM y"), "SELECT a, b FROM x UNION SELECT c, NULL FROM y");
    }

    #[test]
    fn table_definitions() {
        assert_eq!(first("ERROR 1075 (42000): Incorrect table definition; there can be only one auto column and it must be defined as a key", "CREATE TABLE t (id INT AUTO_INCREMENT, n INT)"), "CREATE TABLE t (id INT AUTO_INCREMENT PRIMARY KEY, n INT)");
        assert_eq!(first("ERROR 1068 (42000): Multiple primary key defined", "CREATE TABLE t (id INT PRIMARY KEY, n INT, PRIMARY KEY (id))"), "CREATE TABLE t (id INT, n INT,\n  PRIMARY KEY (id))");
        assert_eq!(first("ERROR 1068 (42000): Multiple primary key defined", "CREATE TABLE t (a INT PRIMARY KEY, b INT PRIMARY KEY)"), "CREATE TABLE t (a INT, b INT,\n  PRIMARY KEY (a, b))");
        assert_eq!(first("ERROR 1170 (42000): BLOB/TEXT column 'name' used in key specification without a key length", "CREATE TABLE t (name TEXT PRIMARY KEY)"), "CREATE TABLE t (name VARCHAR(255) PRIMARY KEY)");
        assert_eq!(first("ERROR 1061 (42000): Duplicate key name 'ix'", "CREATE INDEX ix ON t (a)"), "CREATE INDEX IF NOT EXISTS ix ON t (a)");
        assert_eq!(first("ERROR 1050 (42S01): Table 'v' already exists", "CREATE VIEW v AS SELECT 1"), "CREATE OR REPLACE VIEW v AS SELECT 1");
        assert_eq!(
            first("ERROR 1093 (HY000): You can't specify target table 't' for update in FROM clause", "DELETE FROM t WHERE id IN (SELECT id FROM t WHERE n > 1)"),
            "DELETE FROM t WHERE id IN (SELECT * FROM (SELECT id FROM t WHERE n > 1) AS _t1)"
        );
    }

    #[test]
    fn similar_names() {
        assert_eq!(vorher_angelegt("CREATE TABLE lehrer (id INT);\nSELECT 1;\n", "ERROR 1146 (42S02): Table 'heiltest.lehrer' doesn't exist"), Some(0));
        assert_eq!(vorher_angelegt("CREATE TABLE lehrer (id INT);\nINSERT INTO x VALUES (1);\n", "ERROR 1146 (42S02): Table 'heiltest.lehrer' doesn't exist"), None);
        assert_eq!(vorher_angelegt("CREATE TABLE andere (id INT);\n", "ERROR 1146 (42S02): Table 'heiltest.lehrer' doesn't exist"), None);
        assert_eq!(first("ERROR 1146 (42S02): Table 'person.personen' doesn't exist", "SELECT * FROM personen"), "SELECT * FROM Personen");
        assert_eq!(first("ERROR 1146 (42S02): Table 'person.Persone' doesn't exist", "SELECT * FROM Persone p"), "SELECT * FROM Personen p");
        assert_eq!(first("ERROR 1054 (42S22): Unknown column 'vornme' in 'field list'", "SELECT vornme FROM Personen"), "SELECT vorname FROM Personen");
        assert_eq!(first("ERROR 1054 (42S22): Unknown column 'p.nachnam' in 'field list'", "SELECT p.nachnam, nachnam FROM Personen p"), "SELECT p.nachname, nachnam FROM Personen p");
        assert_eq!(
            first("ERROR 1052 (23000): Column 'pid' in field list is ambiguous", "SELECT pid, email FROM Personen p JOIN Email e ON e.pid = p.pid"),
            "SELECT p.pid, email FROM Personen p JOIN Email e ON e.pid = p.pid"
        );
    }

    #[test]
    fn values_and_rows() {
        // MariaDB 11: "in 'VALUES'"; Name aehnlich einer Spalte darf trotzdem nicht ersetzt werden
        assert_eq!(first("ERROR 1054 (42S22): Unknown column 'vornam' in 'VALUES'", "INSERT INTO Personen (vorname) VALUES (vornam)"), "INSERT INTO Personen (vorname) VALUES ('vornam')");
        assert_eq!(first("ERROR 1242 (21000): Subquery returns more than 1 row", "SELECT * FROM Personen WHERE pid = (SELECT pid FROM Email)"), "SELECT * FROM Personen WHERE pid IN (SELECT pid FROM Email)");
        assert_eq!(first("ERROR 1062 (23000): Duplicate entry '1' for key 'PRIMARY'", "INSERT INTO Personen (pid, vorname) VALUES (1, 'A'), (2, 'B')"), "INSERT INTO Personen (vorname) VALUES ('A'), ('B')");
        assert_eq!(first("ERROR 1062 (23000): Duplicate entry 'a' for key 'PRIMARY'", "INSERT INTO Email VALUES (1, 'a')"), "INSERT IGNORE INTO Email VALUES (1, 'a')");
        assert_eq!(
            first("ERROR 1406 (22001): Data too long for column 'vorname' at row 1", "INSERT INTO Personen (vorname) VALUES ('Maximilian-Alexander-Ferdinand')"),
            "ALTER TABLE Personen MODIFY vorname VARCHAR(50) NOT NULL;\nINSERT INTO Personen (vorname) VALUES ('Maximilian-Alexander-Ferdinand')"
        );
        assert_eq!(first("ERROR 1064 (42000): You have an error in your SQL syntax", "INSERT INTO Personen (vorname, nachname) VALUES (Max, Muster)"), "INSERT INTO Personen (vorname, nachname) VALUES ('Max', 'Muster')");
        assert_eq!(first("ERROR 1292 (22007): Incorrect date value: '17.11.2026'", "SELECT * FROM Personen WHERE x > '17.11.2026'"), "SELECT * FROM Personen WHERE x > '2026-11-17'");
    }

    #[test]
    fn foreign_key_type() {
        assert_eq!(
            first("ERROR 1005 (HY000): Can't create table `person`.`Tel` (errno: 150 \"Foreign key constraint is incorrectly formed\")", "CREATE TABLE Tel (pid INT(11) NOT NULL, FOREIGN KEY (pid) REFERENCES Personen (pid))"),
            "CREATE TABLE Tel (pid INT UNSIGNED NOT NULL, FOREIGN KEY (pid) REFERENCES Personen (pid))"
        );
        assert_eq!(norm_type("int(11)"), "INT");
        assert_eq!(norm_type("decimal(8,2) unsigned"), "DECIMAL(8,2) UNSIGNED");
    }

    #[test]
    fn database_hints() {
        assert_eq!(created_database("DROP DATABASE IF EXISTS person;\nCREATE DATABASE IF NOT EXISTS person;\n").as_deref(), Some("person"));
        assert_eq!(created_database("CREATE DATABASE a; USE `b`;").as_deref(), Some("b"));
        assert_eq!(table_names("SELECT * FROM Personen p JOIN Email e ON 1 WHERE x IN (SELECT 1 FROM Telefon)"), vec!["Personen", "Email", "Telefon"]);
    }

    #[test]
    fn code_is_read() {
        assert_eq!(code("ERROR 1146 (42S02): x"), Some(1146));
        assert_eq!(code("nix"), None);
    }
}
