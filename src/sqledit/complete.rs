// Autovervollstaendigung: Vorschlaege passend zur Stelle in der Anweisung (Tabellen nach FROM/JOIN, Spalten
// nach SELECT/WHERE, Spalten eines Alias nach "a.", Join-Bedingungen aus Fremdschluesseln, Datentypen in
// CREATE TABLE, Vorlagen am Anfang). Namen behalten die Schreibweise der Datenbank, Schluesselwoerter
// folgen der Schreibweise, in der getippt wird.

use super::highlight::{FUNCTIONS, KEYWORDS, is_word_char};
use super::lint::{Env, Kind, Tok, known_word, lex};
use super::text::in_comment_or_string;
use crate::db::TableInfo;

/// Ein Eintrag im Vorschlagsfenster
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// Text, der eingefuegt wird
    pub insert: String,
    pub kind: &'static str,
    pub detail: String,
    /// Um so viele Zeichen springt der Cursor nach dem Einfuegen zurueck (z. B. in die Klammer)
    pub back: usize,
}

pub struct Suggestion {
    pub items: Vec<Item>,
    /// Zeichenindex, ab dem das getippte Wort ersetzt wird
    pub word_start: usize,
}

/// Marke fuer die Cursorstelle in Vorlagen
const CURSOR: char = '\u{1}';

const STATEMENTS: &[&str] = &[
    "SELECT", "INSERT INTO", "UPDATE", "DELETE FROM", "CREATE TABLE", "CREATE DATABASE", "ALTER TABLE", "DROP TABLE", "DROP DATABASE", "TRUNCATE TABLE", "USE",
    "SHOW TABLES", "SHOW DATABASES", "DESCRIBE", "EXPLAIN", "WITH", "SET", "BEGIN", "COMMIT", "ROLLBACK",
];

/// Vorlagen (Markierung \u{1} = Cursor)
const SNIPPETS: &[(&str, &str)] = &[
    ("SELECT * FROM", "SELECT * FROM \u{1}"),
    ("SELECT COUNT(*) FROM", "SELECT COUNT(*) FROM \u{1}"),
    ("SELECT … WHERE", "SELECT * FROM \u{1}\nWHERE "),
    ("INSERT INTO … VALUES", "INSERT INTO \u{1} () VALUES ()"),
    ("UPDATE … SET … WHERE", "UPDATE \u{1}\nSET \nWHERE "),
    ("DELETE FROM … WHERE", "DELETE FROM \u{1}\nWHERE "),
    ("CREATE TABLE …", "CREATE TABLE \u{1} (\n  id INT AUTO_INCREMENT PRIMARY KEY,\n  name VARCHAR(100) NOT NULL\n)"),
    ("ALTER TABLE … ADD COLUMN", "ALTER TABLE \u{1} ADD COLUMN "),
];

const AFTER_TABLE: &[&str] = &[
    "WHERE", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "CROSS JOIN", "JOIN", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "UNION", "UNION ALL", "AS", "SET", "VALUES",
];

const AFTER_SELECT_ITEM: &[&str] = &["FROM", "AS", "INTO", "ASC", "DESC"];
const AFTER_CONDITION: &[&str] = &[
    "AND", "OR", "NOT", "IN", "LIKE", "BETWEEN", "IS NULL", "IS NOT NULL", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "UNION", "INNER JOIN", "LEFT JOIN", "JOIN",
];
const AFTER_ORDER: &[&str] = &["ASC", "DESC", "LIMIT", "HAVING", "ORDER BY", "UNION"];
const EXPRESSION_STARTS: &[&str] = &["DISTINCT", "ALL", "NOT", "EXISTS", "CASE", "NULL", "TRUE", "FALSE", "INTERVAL", "SELECT"];

const TYPES: &[&str] = &[
    "INT", "BIGINT", "SMALLINT", "TINYINT", "VARCHAR(255)", "CHAR(1)", "TEXT", "LONGTEXT", "DATE", "DATETIME", "TIMESTAMP", "TIME", "DECIMAL(10,2)", "FLOAT",
    "DOUBLE", "BOOLEAN", "JSON", "BLOB", "ENUM('')", "YEAR",
];
const CONSTRAINTS: &[&str] = &["NOT NULL", "NULL", "DEFAULT", "AUTO_INCREMENT", "PRIMARY KEY", "UNIQUE", "REFERENCES", "COMMENT", "UNSIGNED", "ON DELETE CASCADE", "ON UPDATE CASCADE"];

/// Schluesselwoerter, nach denen ein neuer Teil der Anweisung beginnt
const CLAUSE: &[&str] = &[
    "SELECT", "FROM", "JOIN", "INTO", "UPDATE", "TABLE", "WHERE", "ON", "AND", "OR", "SET", "BY", "HAVING", "USING", "VALUES", "USE", "DESCRIBE", "DESC", "TRUNCATE",
    "WHEN", "THEN", "ELSE", "CASE", "LIMIT", "OFFSET", "UNION",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    Statement,
    Table,
    AfterTable,
    Column,
    Database,
    Type,
}

/// Schreibweise fuer Schluesselwoerter: klein, wenn so getippt wird
fn lower_style(word: &str, toks: &[Tok]) -> bool {
    if !word.is_empty() {
        return word.chars().all(|c| !c.is_uppercase());
    }
    toks.iter().find(|t| t.kind == Kind::Word && known_word(&t.upper())).is_some_and(|t| t.text.chars().all(|c| !c.is_uppercase()))
}

fn styled(s: &str, lower: bool) -> String {
    if lower { s.to_lowercase() } else { s.to_string() }
}

/// Rang eines Namens zum getippten Wort: 0 gleiche Schreibweise am Anfang, 1 Anfang ohne Beachtung der Schreibweise,
/// 2 enthalten, None passt nicht. Ohne Wort: 1.
fn rank(label: &str, word: &str) -> Option<u8> {
    if word.is_empty() {
        return Some(1);
    }
    if label.starts_with(word) {
        return Some(0);
    }
    let (l, w) = (label.to_lowercase(), word.to_lowercase());
    if l.starts_with(&w) {
        Some(1)
    } else if l.contains(&w) {
        Some(2)
    } else {
        None
    }
}

/// Unscharf: die Buchstaben kommen in dieser Reihenfolge vor und der erste passt (`kid` -> `kunde_id`,
/// `bpos` -> `bestell_position`). Nur fuer Namen, nicht fuer Schluesselwoerter.
fn fuzzy(label: &str, word: &str) -> bool {
    let l: Vec<char> = label.to_lowercase().chars().filter(|c| *c != '`').collect();
    let w: Vec<char> = word.to_lowercase().chars().collect();
    if w.len() < 2 || l.first() != w.first() {
        return false;
    }
    let mut i = 0;
    for c in &l {
        if i < w.len() && *c == w[i] {
            i += 1;
        }
    }
    i == w.len()
}

struct Scope<'a> {
    /// (Anzeigename = Alias oder Tabellenname, Tabelle)
    entries: Vec<(String, &'a TableInfo)>,
}

/// Tabellen der ganzen Anweisung (FROM, JOIN, UPDATE, INSERT INTO) mit ihren Aliasen
fn scope<'a>(toks: &[Tok], env: &Env<'a>) -> Scope<'a> {
    let mut entries: Vec<(String, &'a TableInfo)> = Vec::new();
    let Some(schema) = env.schema else { return Scope { entries } };
    let find = |name: &str| schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name));
    let first = toks.first().map(|t| t.upper()).unwrap_or_default();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let w = if t.kind == Kind::Word { t.upper() } else { String::new() };
        let opens = matches!(w.as_str(), "FROM" | "JOIN" | "UPDATE") || (w == "INTO" && matches!(first.as_str(), "INSERT" | "REPLACE"));
        if opens {
            let mut j = i + 1;
            #[allow(clippy::while_let_loop)]
            loop {
                let Some(n) = toks.get(j).filter(|n| n.is_name()) else { break };
                let mut name = n;
                if toks.get(j + 1).is_some_and(|d| d.kind == Kind::Dot)
                    && let Some(m) = toks.get(j + 2).filter(|m| m.is_name())
                {
                    name = m;
                    j += 2;
                }
                j += 1;
                let mut alias = name.text.clone();
                if let Some(a) = toks.get(j) {
                    let explicit = a.is_word("AS");
                    if let Some(c) = toks.get(j + usize::from(explicit)).filter(|c| c.is_name() && (explicit || c.kind == Kind::Quoted || !known_word(&c.upper()))) {
                        alias = c.text.clone();
                        j += 1 + usize::from(explicit);
                    }
                }
                if let Some(table) = find(&name.text) {
                    entries.push((alias, table));
                }
                if w != "JOIN" && toks.get(j).is_some_and(|c| c.kind == Kind::Comma) {
                    j += 1;
                } else {
                    break;
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    Scope { entries }
}

fn operand_complete(before: &[&Tok]) -> bool {
    let Some(last) = before.last() else { return false };
    match last.kind {
        Kind::Close | Kind::Num | Kind::Str | Kind::Quoted | Kind::Var => true,
        Kind::Word => !known_word(&last.upper()) || matches!(last.upper().as_str(), "NULL" | "TRUE" | "FALSE" | "END"),
        Kind::Op => {
            last.text == "*" && before.len().checked_sub(2).and_then(|k| before.get(k)).is_none_or(|p| p.kind == Kind::Comma || p.kind == Kind::Open || p.is_word("SELECT") || p.is_word("DISTINCT"))
        }
        _ => false,
    }
}

fn unmatched_open(before: &[&Tok]) -> bool {
    let mut depth = 0i32;
    for t in before {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

fn keyword_item(k: &str, kind: &'static str, lower: bool) -> Item {
    let s = styled(k, lower);
    Item { label: s.clone(), insert: s, kind, detail: String::new(), back: 0 }
}

fn snippet_item(label: &str, text: &str, lower: bool) -> Item {
    let text = styled_snippet(text, lower);
    let back = text.chars().skip_while(|c| *c != CURSOR).count().saturating_sub(1);
    Item { label: label.to_string(), insert: text.replace(CURSOR, ""), kind: crate::i18n::text("Vorlage"), detail: String::new(), back }
}

fn styled_snippet(text: &str, lower: bool) -> String {
    if !lower {
        return text.to_string();
    }
    // Nur Woerter in Grossbuchstaben (Schluesselwoerter) werden klein
    text.split_inclusive(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|w| {
            let word: String = w.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
            if word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase() || c == '_') { w.to_lowercase() } else { w.to_string() }
        })
        .collect()
}

fn column_items(table: &TableInfo, qualifier: Option<&str>, out: &mut Vec<(u8, Item)>, prio: u8) {
    for c in &table.columns {
        let name = match qualifier {
            Some(q) => format!("{q}.{}", quote_name(&c.name)),
            None => quote_name(&c.name),
        };
        let key = if c.is_pk() { " · PK" } else if c.key == "UNI" { " · UNI" } else if c.key == "MUL" { " · idx" } else { "" };
        out.push((
            prio,
            Item {
                label: name.clone(),
                insert: name,
                kind: crate::i18n::text("Spalte"),
                detail: if qualifier.is_some() { format!("{}{key}", c.col_type) } else { format!("{}{key}  {}", c.col_type, table.name) },
                back: 0,
            },
        ));
    }
}

/// Kurzer Alias fuer eine Tabelle, der in der Anweisung noch frei ist (kunde -> k, kategorie -> ka)
fn free_alias(table: &str, used: &[String]) -> String {
    let lower = table.to_lowercase();
    let letters: Vec<char> = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let taken = |a: &str| used.iter().any(|u| u.eq_ignore_ascii_case(a)) || KEYWORDS.contains(&a.to_ascii_uppercase().as_str());
    // Anfangsbuchstaben der Wortteile (bestell_position -> bp), dann 1, 2, 3 Buchstaben
    let initials: String = lower.split(['_', '-']).filter_map(|p| p.chars().next()).collect();
    let mut tries: Vec<String> = Vec::new();
    if initials.chars().count() > 1 {
        tries.push(initials);
    }
    for n in 1..=3.min(letters.len()) {
        tries.push(letters[..n].iter().collect());
    }
    tries.into_iter().find(|a| !a.is_empty() && !taken(a)).unwrap_or_else(|| lower.clone())
}

/// Erklaerung zum gewaehlten Vorschlag: (Titel, Text) fuer das Feld unter der Liste
pub fn doc(item: &Item, schema: Option<&crate::db::Schema>) -> Option<(String, String)> {
    use crate::i18n::{text, tr};
    if item.kind == text("Funktion") {
        let name = item.label.trim_end_matches("()").to_uppercase();
        let (sig, de, en) = super::hover::function_doc(&name)?;
        return Some((sig.to_string(), tr(de, en).to_string()));
    }
    if item.kind == "SQL" {
        let (de, en, ex) = super::hover::keyword_doc(&item.label.to_uppercase())?;
        return Some((tr(de, en).to_string(), ex.to_string()));
    }
    let schema = schema?;
    if item.kind == text("Tabelle") {
        let t = schema.tables.iter().find(|t| t.name == item.label)?;
        let cols: Vec<&str> = t.columns.iter().take(12).map(|c| c.name.as_str()).collect();
        let more = if t.columns.len() > 12 { ", …" } else { "" };
        let rows = t.rows.map(|r| crate::tr_format!("  · ≈ {} Zeilen", "  · ≈ {} rows", r)).unwrap_or_default();
        return Some((format!("{} ({}){rows}", t.name, t.columns.len()), format!("{}{more}", cols.join(", "))));
    }
    if item.kind == text("Spalte") {
        let name = item.label.rsplit('.').next().unwrap_or(&item.label).trim_matches('`');
        let table = item.detail.split_whitespace().last().unwrap_or("");
        let (t, c) = schema.tables.iter().filter_map(|t| t.columns.iter().find(|c| c.name == name).map(|c| (t, c)))
            .max_by_key(|(t, _)| t.name == table)?;
        let mut body = Vec::new();
        if c.is_pk() {
            body.push(tr("Primärschlüssel", "Primary key").to_string());
        }
        if let Some(fk) = schema.fks.iter().find(|f| f.table == t.name && f.columns.iter().any(|x| x == name)) {
            body.push(crate::tr_format!("Verweist auf {}.{}", "References {}.{}", fk.ref_table, fk.ref_columns.join(", ")));
        }
        if let Some(d) = &c.default {
            body.push(format!("DEFAULT {d}"));
        }
        if !c.comment.is_empty() {
            body.push(c.comment.clone());
        }
        return Some((format!("{}.{} {}{}", t.name, c.name, c.col_type, if c.nullable { "" } else { " NOT NULL" }), body.join(" · ")));
    }
    if item.kind == text("Join") {
        return Some((item.label.clone(), crate::tr_format!("Verknüpfung über den Fremdschlüssel {}", "Join via the foreign key {}", item.detail)));
    }
    None
}

/// Name, bei Bedarf in Backticks
fn quote_name(name: &str) -> String {
    let simple = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit()) && !KEYWORDS.contains(&name.to_ascii_uppercase().as_str());
    if simple { name.to_string() } else { format!("`{name}`") }
}

pub fn suggest(text: &str, cursor: usize, env: &Env, force: bool) -> Option<Suggestion> {
    let chars: Vec<char> = text.chars().collect();
    if cursor > chars.len() || in_comment_or_string(&chars[..cursor]) {
        return None;
    }
    let mut s = cursor;
    while s > 0 && is_word_char(chars[s - 1]) {
        s -= 1;
    }
    let word: String = chars[s..cursor].iter().collect();
    // "alias." oder "tabelle." davor
    let mut qualifier: Option<String> = None;
    let mut qual_start = s;
    if s > 0 && chars[s - 1] == '.' {
        let mut t = s - 1;
        while t > 0 && (is_word_char(chars[t - 1]) || chars[t - 1] == '`') {
            t -= 1;
        }
        let name: String = chars[t..s - 1].iter().filter(|c| **c != '`').collect();
        if !name.is_empty() {
            qualifier = Some(name);
            qual_start = t;
        }
    }
    // Aktuelle Anweisung
    let st = chars[..cursor].iter().rposition(|c| *c == ';').map_or(0, |p| p + 1);
    let en = chars[cursor..].iter().position(|c| *c == ';').map_or(chars.len(), |p| cursor + p);
    let stmt: Vec<char> = chars[st..en].to_vec();
    let toks = lex(&stmt).toks;
    let ctx_end = qual_start - st;
    let before: Vec<&Tok> = toks.iter().filter(|t| t.end <= ctx_end).collect();
    let scope = scope(&toks, env);
    let lower = lower_style(&word, &toks);
    let schema = env.schema;

    let mut items: Vec<(u8, Item)> = Vec::new();

    if let Some(q) = &qualifier {
        // Spalten des Alias bzw. der Tabelle
        let table = scope
            .entries
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(q))
            .map(|(_, t)| *t)
            .or_else(|| schema.and_then(|s| s.tables.iter().find(|t| t.name.eq_ignore_ascii_case(q))));
        if let Some(t) = table {
            column_items(t, None, &mut items, 0);
            for it in items.iter_mut() {
                it.1.detail = t.columns.iter().find(|c| c.name == it.1.label.trim_matches('`')).map(|c| c.col_type.clone()).unwrap_or_default();
            }
        } else if let Some(sch) = schema.filter(|sch| sch.name.eq_ignore_ascii_case(q)) {
            for t in &sch.tables {
                items.push((0, Item { label: t.name.clone(), insert: quote_name(&t.name), kind: crate::i18n::text("Tabelle"), detail: String::new(), back: 0 }));
            }
        }
    } else {
        let clause = {
            let mut depth = 0i32;
            let mut found: Option<String> = None;
            for t in before.iter().rev() {
                match t.kind {
                    Kind::Close => depth += 1,
                    Kind::Open => depth = (depth - 1).max(0),
                    Kind::Word if depth == 0 => {
                        let u = t.upper();
                        if CLAUSE.contains(&u.as_str()) {
                            found = Some(u);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            found
        };
        let first = toks.first().filter(|t| t.kind == Kind::Word).map(|t| t.upper()).unwrap_or_default();
        let complete = operand_complete(&before);
        // INSERT INTO t (| …): Spalten der Zieltabelle
        let insert_cols = matches!(first.as_str(), "INSERT" | "REPLACE")
            && clause.as_deref() == Some("INTO")
            && unmatched_open(&before)
            && !before.iter().any(|t| t.is_word("VALUES") || t.is_word("SELECT"));
        let want = if insert_cols {
            Want::Column
        } else if before.is_empty() {
            Want::Statement
        } else if matches!(first.as_str(), "CREATE" | "ALTER")
            && unmatched_open(&before)
            && before.last().is_some_and(|l| l.kind == Kind::Word && !known_word(&l.upper()))
            && before.len().checked_sub(2).and_then(|k| before.get(k)).is_some_and(|p| p.kind == Kind::Open || p.kind == Kind::Comma)
        {
            Want::Type
        } else {
            match clause.as_deref() {
                Some("USE") => Want::Database,
                Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "DESCRIBE" | "DESC" | "TRUNCATE") => {
                    let after_name = complete && before.last().is_some_and(|l| l.kind != Kind::Comma);
                    if after_name { Want::AfterTable } else { Want::Table }
                }
                Some(_) => Want::Column,
                None => Want::Statement,
            }
        };
        // Ohne getipptes Wort nur an Stellen, an denen sicher etwas folgt
        let last_is_space = cursor > 0 && chars[cursor - 1].is_whitespace();
        let after_open = cursor > 0 && chars[cursor - 1] == '(';
        if word.is_empty() && !force && !(last_is_space && matches!(want, Want::Table | Want::Database)) && !(insert_cols && after_open) {
            return None;
        }
        if word.chars().count() < 2 && !force && !matches!(want, Want::Table | Want::Column | Want::Database | Want::Type | Want::AfterTable) {
            return None;
        }
        let kw = |list: &[&str], prio: u8, items: &mut Vec<(u8, Item)>| {
            for k in list {
                items.push((prio, keyword_item(k, "SQL", lower)));
            }
        };
        let functions = |prio: u8, items: &mut Vec<(u8, Item)>| {
            for f in FUNCTIONS {
                let name = styled(f, lower);
                items.push((prio, Item { label: format!("{name}()"), insert: format!("{name}()"), kind: crate::i18n::text("Funktion"), detail: String::new(), back: 1 }));
            }
        };
        let tables = |prio: u8, items: &mut Vec<(u8, Item)>| {
            for t in schema.iter().flat_map(|s| &s.tables) {
                let detail = if t.is_view {
                    crate::i18n::text("Sicht").to_string()
                } else {
                    crate::tr_format!("{} Spalten", "{} columns", t.columns.len())
                };
                items.push((prio, Item { label: t.name.clone(), insert: quote_name(&t.name), kind: crate::i18n::text("Tabelle"), detail, back: 0 }));
            }
        };
        match want {
            Want::Statement => {
                kw(STATEMENTS, 0, &mut items);
                for (label, body) in SNIPPETS {
                    items.push((0, snippet_item(label, body, lower)));
                }
                kw(&["IF", "WHILE"], 3, &mut items);
            }
            Want::Table => {
                // Nach JOIN: verwandte Tabellen samt ON-Bedingung aus den Fremdschluesseln zuerst
                if clause.as_deref() == Some("JOIN")
                    && let Some(sch) = schema
                {
                    let used: Vec<String> = scope.entries.iter().map(|(a, _)| a.clone()).collect();
                    for fk in &sch.fks {
                        for (von, nach, eigene, fremde) in [(&fk.table, &fk.ref_table, &fk.columns, &fk.ref_columns), (&fk.ref_table, &fk.table, &fk.ref_columns, &fk.columns)] {
                            // `von` steht schon in der Anweisung, `nach` wird angehaengt
                            let Some((al, _)) = scope.entries.iter().find(|(_, t)| t.name.eq_ignore_ascii_case(von)) else { continue };
                            if scope.entries.iter().filter(|(_, t)| t.name.eq_ignore_ascii_case(nach)).count() > usize::from(nach.eq_ignore_ascii_case(von)) {
                                continue;
                            }
                            let neu = free_alias(nach, &used);
                            let cond = eigene.iter().zip(fremde).map(|(e, f)| format!("{neu}.{} = {al}.{}", quote_name(f), quote_name(e))).collect::<Vec<_>>().join(" AND ");
                            let text = format!("{} {neu} ON {cond}", quote_name(nach));
                            items.push((0, Item { label: text.clone(), insert: text, kind: crate::i18n::text("Join"), detail: fk.name.clone(), back: 0 }));
                        }
                    }
                }
                tables(if items.is_empty() { 0 } else { 1 }, &mut items);
            }
            Want::AfterTable => {
                kw(AFTER_TABLE, 0, &mut items);
                if let Some(sch) = schema {
                    // Alias vorschlagen, wenn noch keiner steht? Nein – nur Fortsetzungen
                    let _ = sch;
                }
            }
            Want::Database => {
                for d in env.databases {
                    items.push((0, Item { label: d.clone(), insert: quote_name(d), kind: crate::i18n::text("Datenbank"), detail: String::new(), back: 0 }));
                }
            }
            Want::Type => {
                kw(TYPES, 0, &mut items);
                kw(CONSTRAINTS, 1, &mut items);
            }
            Want::Column if insert_cols => {
                if let Some((_, t)) = scope.entries.first() {
                    let cols: Vec<String> = t.columns.iter().filter(|c| !c.extra.to_ascii_lowercase().contains("auto_increment") && !c.extra.to_ascii_lowercase().contains("generated")).map(|c| quote_name(&c.name)).collect();
                    if before.last().is_some_and(|l| l.kind == Kind::Open) && cols.len() > 1 {
                        let all = cols.join(", ");
                        items.push((0, Item { label: all.clone(), insert: all, kind: crate::i18n::text("Vorlage"), detail: crate::i18n::tr("alle Spalten", "all columns").into(), back: 0 }));
                    }
                    let mut cs = Vec::new();
                    column_items(t, None, &mut cs, 1);
                    items.extend(cs);
                }
            }
            Want::Column => {
                // Join-Bedingungen aus Fremdschluesseln, wenn nach ON gefragt wird
                if clause.as_deref() == Some("ON")
                    && !complete
                    && let Some(sch) = schema
                {
                    for fk in &sch.fks {
                        let a = scope.entries.iter().find(|(_, t)| t.name.eq_ignore_ascii_case(&fk.table));
                        let b = scope.entries.iter().find(|(al, t)| t.name.eq_ignore_ascii_case(&fk.ref_table) && a.is_none_or(|(a_al, _)| a_al != al));
                        if let (Some((al_a, _)), Some((al_b, _))) = (a, b) {
                            let cond = fk
                                .columns
                                .iter()
                                .zip(&fk.ref_columns)
                                .map(|(c, r)| format!("{al_a}.{} = {al_b}.{}", quote_name(c), quote_name(r)))
                                .collect::<Vec<_>>()
                                .join(" AND ");
                            items.push((0, Item { label: cond.clone(), insert: cond, kind: crate::i18n::text("Join"), detail: fk.name.clone(), back: 0 }));
                        }
                    }
                }
                let continuation: &[&str] = match clause.as_deref() {
                    Some("SELECT") => AFTER_SELECT_ITEM,
                    Some("BY") => AFTER_ORDER,
                    Some("SET") => &["WHERE"],
                    _ => AFTER_CONDITION,
                };
                let (cont_prio, col_prio) = if complete { (1, 2) } else { (3, 1) };
                if complete {
                    kw(continuation, cont_prio, &mut items);
                }
                // Spalten der Tabellen der Anweisung; ohne FROM alle Tabellen
                if scope.entries.is_empty() {
                    for t in schema.iter().flat_map(|s| &s.tables) {
                        column_items(t, None, &mut items, col_prio);
                    }
                } else {
                    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                    for (_, t) in &scope.entries {
                        for c in &t.columns {
                            *counts.entry(c.name.to_lowercase()).or_default() += 1;
                        }
                    }
                    let many = scope.entries.len() > 1;
                    for (alias, t) in &scope.entries {
                        let mut cols = Vec::new();
                        column_items(t, None, &mut cols, col_prio);
                        for (p, mut it) in cols {
                            if many && counts.get(&it.label.trim_matches('`').to_lowercase()).is_some_and(|n| *n > 1) {
                                it.label = format!("{alias}.{}", it.label);
                                it.insert = it.label.clone();
                            }
                            items.push((p, it));
                        }
                        if many {
                            items.push((col_prio + 1, Item { label: alias.clone(), insert: format!("{alias}."), kind: crate::i18n::text("Alias"), detail: t.name.clone(), back: 0 }));
                        }
                    }
                }
                functions(col_prio + 1, &mut items);
                kw(EXPRESSION_STARTS, col_prio + 2, &mut items);
            }
        }
        // Passt nichts zur Stelle, bleibt die allgemeine Liste, damit sich jedes Schluesselwort vervollstaendigen laesst
        let narrowed = rank_items(items.clone(), &word, qualifier.is_some(), force);
        if narrowed.is_none() {
            items.clear();
            tables(8, &mut items);
            for t in schema.iter().flat_map(|s| &s.tables) {
                column_items(t, None, &mut items, 9);
            }
            functions(10, &mut items);
            for k in KEYWORDS {
                items.push((11, keyword_item(k, "SQL", lower)));
            }
        }
    }
    let out = rank_items(items, &word, qualifier.is_some(), force)?;
    Some(Suggestion { items: out, word_start: s })
}

/// Filtern, ordnen, Doppelte entfernen. None, wenn nichts (Sinnvolles) uebrig bleibt.
fn rank_items(items: Vec<(u8, Item)>, word: &str, qualified: bool, force: bool) -> Option<Vec<Item>> {
    let name_like = |k: &str| k == crate::i18n::text("Tabelle") || k == crate::i18n::text("Spalte") || k == crate::i18n::text("Datenbank") || k == crate::i18n::text("Alias");
    let mut ranked: Vec<(u8, u8, u8, String, Item)> = items
        .into_iter()
        .filter_map(|(prio, item)| {
            let r = rank(&item.label, word)
                .or_else(|| (!qualified).then(|| rank(&item.label.replace('`', ""), word)).flatten())
                .or_else(|| (name_like(item.kind) && fuzzy(&item.label, word)).then_some(3))?;
            let order = if name_like(item.kind) { item.label.to_lowercase() } else { String::new() };
            // Genau das getippte Wort (ohne Beachtung der Schreibweise) steht ganz oben
            let exact = u8::from(word.is_empty() || !item.insert.trim_end_matches("()").eq_ignore_ascii_case(word));
            Some((exact, prio, r, order, item))
        })
        .collect();
    // Stabil: Schluesselwoerter und Vorlagen behalten ihre Reihenfolge, Namen sind alphabetisch
    ranked.sort_by(|a, b| (a.0, a.1, a.2, &a.3).cmp(&(b.0, b.1, b.2, &b.3)));
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<Item> = Vec::new();
    for (_, _, _, _, it) in ranked {
        if seen.insert((it.label.clone(), it.kind)) {
            out.push(it);
        }
        if out.len() >= 60 {
            break;
        }
    }
    if out.is_empty() || (out.len() == 1 && out[0].insert == word && !force) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, ForeignKey, Schema, TableInfo};

    fn table(name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            name: name.into(),
            columns: cols.iter().map(|c| ColumnInfo { name: (*c).into(), col_type: "int".into(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    fn schema() -> Schema {
        Schema {
            name: "schule".into(),
            tables: vec![table("Schueler", &["id", "vorname", "klasse_id"]), table("Klasse", &["id", "name"])],
            fks: vec![ForeignKey { name: "fk_k".into(), table: "Schueler".into(), columns: vec!["klasse_id".into()], ref_table: "Klasse".into(), ref_columns: vec!["id".into()], ..Default::default() }],
            ..Default::default()
        }
    }

    fn labels(sql: &str, force: bool) -> Vec<String> {
        let s = schema();
        let dbs = vec!["schule".to_string(), "shop".to_string()];
        let env = Env { schema: Some(&s), databases: &dbs };
        let cursor = sql.chars().count();
        suggest(sql, cursor, &env, force).map(|s| s.items.into_iter().map(|i| i.label).collect()).unwrap_or_default()
    }

    #[test]
    fn tables_after_from_keep_their_case() {
        let l = labels("SELECT * FROM sch", false);
        assert_eq!(l[0], "Schueler");
        let l = labels("SELECT * FROM ", false);
        assert_eq!(l, vec!["Klasse", "Schueler"]);
    }

    #[test]
    fn case_sensitive_prefix_ranks_first() {
        let s = Schema { name: "d".into(), tables: vec![table("spieler", &["a"]), table("Spielleitung", &["b"])], ..Default::default() };
        let env = Env { schema: Some(&s), databases: &[] };
        let l: Vec<String> = suggest("SELECT * FROM Sp", 16, &env, false).unwrap().items.into_iter().map(|i| i.label).collect();
        assert_eq!(l[0], "Spielleitung");
        let l: Vec<String> = suggest("SELECT * FROM sp", 16, &env, false).unwrap().items.into_iter().map(|i| i.label).collect();
        assert_eq!(l[0], "spieler");
    }

    #[test]
    fn keywords_follow_typed_case() {
        assert_eq!(labels("sel", false)[0], "select");
        assert_eq!(labels("SEL", false)[0], "SELECT");
        assert_eq!(labels("select * from Klasse wh", false)[0], "where");
    }

    #[test]
    fn columns_of_alias_and_table() {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let text = "SELECT s. FROM Schueler s";
        let p = suggest(text, 9, &env, false).unwrap();
        assert_eq!(p.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["id", "klasse_id", "vorname"]);
        assert_eq!(p.word_start, 9);
        let p = suggest("SELECT Klasse.", 14, &env, false).unwrap();
        assert_eq!(p.items.len(), 2);
    }

    #[test]
    fn columns_after_select_and_where_use_scope() {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let text = "SELECT vor FROM Schueler";
        let p = suggest(text, 10, &env, false).unwrap();
        assert_eq!(p.items[0].label, "vorname");
        assert_eq!(p.items[0].kind, crate::i18n::text("Spalte"));
        let text = "SELECT * FROM Schueler s JOIN Klasse k ON s.klasse_id = k.id WHERE na";
        let p = suggest(text, text.chars().count(), &env, false).unwrap();
        // "name" gibt es nur in Klasse
        assert!(p.items.iter().any(|i| i.label == "name"));
    }

    #[test]
    fn join_condition_from_foreign_key() {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let text = "SELECT * FROM Schueler s JOIN Klasse k ON ";
        let p = suggest(text, text.chars().count(), &env, true).unwrap();
        assert_eq!(p.items[0].label, "s.klasse_id = k.id");
        assert_eq!(p.items[0].kind, crate::i18n::text("Join"));
    }

    #[test]
    fn databases_after_use_and_types_in_create_table() {
        assert_eq!(labels("USE ", false), vec!["schule", "shop"]);
        let l = labels("CREATE TABLE t (id ", true);
        assert_eq!(l[0], "INT");
        assert!(l.contains(&"VARCHAR(255)".to_string()));
        assert!(l.contains(&"PRIMARY KEY".to_string()));
    }

    #[test]
    fn continuations_and_functions() {
        let l = labels("SELECT * FROM Klasse ", true);
        assert_eq!(l[0], "WHERE");
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let p = suggest("SELECT cou", 10, &env, false).unwrap();
        // Klein getippt: auch die Funktion wird klein eingefuegt
        let f = p.items.iter().find(|i| i.label == "count()").unwrap();
        assert_eq!((f.insert.as_str(), f.back), ("count()", 1));
        let p = suggest("SELECT COU", 10, &env, false).unwrap();
        assert!(p.items.iter().any(|i| i.label == "COUNT()"));
    }

    #[test]
    fn snippets_place_the_cursor() {
        let l = labels("ins", false);
        assert!(l.contains(&"INSERT INTO".to_string()) || l.iter().any(|x| x.starts_with("INSERT")));
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let p = suggest("SEL", 3, &env, false).unwrap();
        let sn = p.items.iter().find(|i| i.label == "SELECT * FROM").unwrap();
        assert_eq!(sn.insert, "SELECT * FROM ");
        assert_eq!(sn.back, 0);
        let p = suggest("sel", 3, &env, false).unwrap();
        assert_eq!(p.items.iter().find(|i| i.label == "SELECT * FROM").unwrap().insert, "select * from ");
        let p = suggest("UPD", 3, &env, false).unwrap();
        let sn = p.items.iter().find(|i| i.label == "UPDATE … SET … WHERE").unwrap();
        assert_eq!(sn.insert, "UPDATE \nSET \nWHERE ");
        assert_eq!(sn.back, "\nSET \nWHERE ".chars().count());
    }

    #[test]
    fn nothing_inside_comments_strings_or_for_one_letter() {
        assert!(labels("-- sel", false).is_empty());
        assert!(labels("SELECT 'sch", false).is_empty());
        assert!(labels("s", false).is_empty());
        assert!(!labels("SELECT 'a' FROM sch", false).is_empty());
    }

    #[test]
    fn unscharf_join_und_insert_spalten() {
        // unscharf: kid -> klasse_id
        let l = labels("SELECT kid FROM Schueler", false);
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let p = suggest("SELECT kid FROM Schueler", 10, &env, false).unwrap();
        assert_eq!(p.items[0].label, "klasse_id", "{l:?}");
        // nach JOIN: verwandte Tabelle mit fertiger Bedingung
        let l = labels("SELECT * FROM Schueler s JOIN ", false);
        assert_eq!(l[0], "Klasse k ON k.id = s.klasse_id", "{l:?}");
        let l = labels("SELECT * FROM Klasse k JOIN ", false);
        assert_eq!(l[0], "Schueler s ON s.klasse_id = k.id", "{l:?}");
        // INSERT: alle Spalten auf einmal, dann einzelne
        let l = labels("INSERT INTO Klasse (", false);
        assert_eq!(l[0], "id, name", "{l:?}");
        let l = labels("INSERT INTO Klasse (id, na", false);
        assert_eq!(l[0], "name", "{l:?}");
        // Erklaerung zum gewaehlten Eintrag
        let f = Item { label: "COUNT()".into(), insert: "COUNT()".into(), kind: crate::i18n::text("Funktion"), detail: String::new(), back: 1 };
        assert_eq!(doc(&f, None).unwrap().0, "COUNT(expr)");
        let k = keyword_item("LEFT JOIN", "SQL", false);
        assert!(doc(&k, None).is_some());
    }

    #[test]
    fn freie_aliasse() {
        assert_eq!(free_alias("kunde", &[]), "k");
        assert_eq!(free_alias("kategorie", &["k".into()]), "ka");
        assert_eq!(free_alias("bestell_position", &[]), "bp");
        // "as" und "on" sind Schluesselwoerter
        assert_eq!(free_alias("ort", &["o".into()]), "ort");
    }
}
