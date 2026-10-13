// SQL-Pruefung waehrend des Tippens: Tippfehler in Schluesselwoertern und Namen, unbekannte Tabellen
// und Spalten (sobald eine Datenbank gewaehlt ist), fehlende Klammern, Kommas und Semikolons.
// Jedes Problem kann einen Korrekturvorschlag mitbringen; "Alle korrigieren" wendet sie gemeinsam an.
// Alle Positionen sind Zeichenindizes (wie der Cursor im Editor).

use super::highlight::{FUNCTIONS, KEYWORDS};
use crate::db::{Schema, TableInfo};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

/// Ersetzt die Zeichen `start..end` durch `text` (Einfuegen: start == end, Loeschen: leerer Text)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
    pub fix: Option<Fix>,
}

/// Texte ab dieser Groesse werden nicht geprueft (Sicherungsdateien u. ae.)
pub const MAX_CHARS: usize = 300_000;

// ---------------------------------------------------------------------------
// Zerlegung in Zeichen

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    /// `Name` in Backticks
    Quoted,
    Str,
    Num,
    Var,
    Op,
    Open,
    Close,
    Comma,
    Semi,
    Dot,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: Kind,
    start: usize,
    end: usize,
    /// Wort bzw. Name ohne Backticks, Operator; sonst leer
    text: String,
}

impl Tok {
    fn upper(&self) -> String {
        self.text.to_ascii_uppercase()
    }

    fn is_word(&self, w: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(w)
    }

    fn is_name(&self) -> bool {
        matches!(self.kind, Kind::Word | Kind::Quoted)
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

struct Lexed {
    toks: Vec<Tok>,
    problems: Vec<Problem>,
}

fn lex(chars: &[char]) -> Lexed {
    let n = chars.len();
    let mut toks = Vec::new();
    let mut problems = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        let start = i;
        let tok = |kind: Kind, end: usize, text: String| Tok { kind, start, end, text };
        if c.is_whitespace() {
            i += 1;
        } else if (c == '-' && next == '-' && chars.get(i + 2).is_none_or(|c| c.is_whitespace())) || c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            if i >= n {
                problems.push(Problem {
                    start,
                    end: (start + 2).min(n),
                    severity: Severity::Warning,
                    message: crate::tr_format!("Der Kommentar wird nie mit */ beendet.", "The comment is never closed with */."),
                    fix: Some(Fix { start: n, end: n, text: " */".into() }),
                });
            }
            i = (i + 2).min(n);
        } else if c == '\'' || c == '"' {
            i += 1;
            let mut closed = false;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == c {
                    if chars.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    closed = true;
                    break;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            if !closed {
                // Das Textende liegt meist am Ende der Zeile, in der der Text begann
                let line_end = (start..n).find(|&k| chars[k] == '\n').unwrap_or(n);
                let fix_at = chars[start..line_end].iter().rposition(|c| !c.is_whitespace() && *c != ';').map_or(line_end, |p| start + p + 1);
                problems.push(Problem {
                    start,
                    end: (start + 1).min(n),
                    severity: Severity::Error,
                    message: crate::tr_format!("Der Text wird nicht beendet: Das schließende Anführungszeichen fehlt.", "The text is not closed: the closing quote is missing."),
                    fix: Some(Fix { start: fix_at, end: fix_at, text: c.to_string() }),
                });
                // Der Rest der Zeile gehoert zum Text; ab der naechsten Zeile geht es normal weiter
                i = line_end.max(start + 1);
            }
            toks.push(tok(Kind::Str, i, String::new()));
        } else if c == '`' {
            i += 1;
            let name_start = i;
            while i < n && chars[i] != '`' {
                i += 1;
            }
            let name: String = chars[name_start..i].iter().collect();
            i = (i + 1).min(n);
            toks.push(tok(Kind::Quoted, i, name));
        } else if c.is_ascii_digit() || (c == '.' && next.is_ascii_digit() && !toks.last().is_some_and(|t| t.is_name() || t.kind == Kind::Close)) {
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            toks.push(tok(Kind::Num, i, String::new()));
        } else if c == '@' {
            while i < n && (chars[i] == '@' || is_word_char(chars[i]) || chars[i] == '.') {
                i += 1;
            }
            toks.push(tok(Kind::Var, i, String::new()));
        } else if is_word_char(c) {
            while i < n && is_word_char(chars[i]) {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            toks.push(tok(Kind::Word, i, text));
        } else {
            i += 1;
            let kind = match c {
                '(' => Kind::Open,
                ')' => Kind::Close,
                ',' => Kind::Comma,
                ';' => Kind::Semi,
                '.' => Kind::Dot,
                _ => Kind::Op,
            };
            let mut text = c.to_string();
            if kind == Kind::Op {
                let two: String = [c, next].iter().collect();
                if ["<=", ">=", "<>", "!=", "||", "&&", ":=", "<<", ">>", "->", "=="].contains(&two.as_str()) {
                    i += 1;
                    text = two;
                    if text == "<=" && chars.get(i) == Some(&'>') {
                        i += 1;
                        text = "<=>".into();
                    } else if text == "->" && chars.get(i) == Some(&'>') {
                        i += 1;
                        text = "->>".into();
                    }
                }
            }
            toks.push(tok(kind, i, text));
        }
    }
    Lexed { toks, problems }
}

// ---------------------------------------------------------------------------
// Woerter

/// Weitere Woerter der SQL-Sprache, die keine Namen sind
const EXTRA: &[&str] = &[
    "AGAINST", "BOTH", "CHARSET", "CROSS", "CURRENT_DATE", "CURRENT_TIME", "CURRENT_USER", "DAY", "DAYS", "DELAYED", "DISTINCTROW", "DUAL",
    "EXPANSION", "FETCH", "FORCE", "GLOBAL", "HIGH_PRIORITY", "HOUR", "LATERAL", "LEADING", "LOCAL", "LOCALTIME", "LOCALTIMESTAMP", "LOCKED",
    "LOW_PRIORITY", "MATCH", "MICROSECOND", "MINUTE", "MODE", "MONTH", "NAMES", "NEXT", "NOWAIT", "OF", "ONLY", "ORDINALITY", "QUARTER",
    "QUERY", "QUICK", "SEPARATOR", "SESSION", "SHARE", "SIGNED", "SKIP", "SOUNDS", "SQL_CALC_FOUND_ROWS", "SQL_NO_CACHE", "STATUS", "TIES",
    "TRAILING", "UNKNOWN", "VARIABLES", "WAIT", "WEEK", "YEAR_MONTH", "YEARS", "MONTHS", "WEEKS", "HOURS", "MINUTES", "SECONDS", "ACTION",
    "ENGINES", "INDEXES", "FIELDS", "FULLTEXT", "SPATIAL", "BTREE", "HASH", "VISIBLE", "INVISIBLE", "WORK", "ISOLATION", "LEVEL", "READ",
    "WRITE", "UNCOMMITTED", "COMMITTED", "REPEATABLE", "SERIALIZABLE", "SNAPSHOT", "TEXT", "STORED", "VIRTUAL", "GENERATED", "ALWAYS",
    "EVENT", "EVENTS", "WARNINGS", "ERRORS", "PROCESSLIST", "PRIVILEGES", "PASSWORD", "IDENTIFIED", "USER", "ROLE", "TRIGGERS", "FUNCTION",
    "PROCEDURE", "STATUS", "ENGINE", "OPTIONS", "DATA", "INFILE", "OUTFILE", "TERMINATED", "ENCLOSED", "LINES", "STARTING", "OPTIONALLY",
    "INPLACE", "COPY", "INSTANT", "NOCOPY", "LOCK", "NONE", "SHARED", "EXCLUSIVE", "DEFAULT", "REPLACE", "SYSTEM", "TIME", "VERSIONING",
    "PERIOD", "WITHOUT", "OVERLAPS", "FOR", "DO", "NOWAIT", "NULLS", "FIRST", "LAST", "BEGIN", "END", "WHILE", "LOOP", "DECLARE", "RETURNS",
    "RETURN", "ELSEIF", "ITERATE", "LEAVE", "REPEAT", "UNTIL", "CURSOR", "HANDLER", "OPEN", "CLOSE", "SIGNAL", "RESIGNAL", "CHARACTER",
    "COLLATION", "ASCII", "UNICODE", "UTF8", "UTF8MB4", "LATIN1", "BINARY",
];

/// Anweisungen, mit denen ein Befehl beginnen darf
const STATEMENTS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "DROP", "ALTER", "TRUNCATE", "USE", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "SET",
    "BEGIN", "START", "COMMIT", "ROLLBACK", "GRANT", "REVOKE", "CALL", "WITH", "LOAD", "LOCK", "UNLOCK", "ANALYZE", "OPTIMIZE", "REPAIR", "CHECK",
    "FLUSH", "KILL", "PREPARE", "EXECUTE", "DEALLOCATE", "DO", "HANDLER", "RENAME", "SAVEPOINT", "RELEASE", "DELIMITER", "SOURCE", "TABLE", "VALUES",
    "HELP", "BACKUP", "RESET", "PURGE", "XA", "SIGNAL", "DECLARE", "IF", "WHILE", "LOOP", "END", "ELSE", "ELSEIF",
];

/// Schluesselwoerter, bei denen ein Tippfehler erkannt wird (ab vier Buchstaben, sonst gibt es zu viele Fehlalarme)
const TYPO_TARGETS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "JOIN", "INNER", "LEFT", "RIGHT", "OUTER", "UNION", "DISTINCT", "INSERT", "INTO",
    "VALUES", "UPDATE", "DELETE", "CREATE", "TABLE", "ALTER", "DROP", "LIKE", "BETWEEN", "EXISTS", "CASE", "WHEN", "THEN", "ELSE", "NULL", "OFFSET",
    "DEFAULT", "PRIMARY", "FOREIGN", "REFERENCES", "TRUNCATE", "DATABASE", "INDEX", "UNIQUE", "CONSTRAINT", "AUTO_INCREMENT", "VARCHAR", "INTEGER",
];

fn known_word(upper: &str) -> bool {
    KEYWORDS.binary_search(&upper).is_ok()
        || EXTRA.contains(&upper)
        || FUNCTIONS.binary_search(&upper).is_ok()
        || crate::sqlfmt::is_type(upper)
        || STATEMENTS.contains(&upper)
}

/// Editierabstand mit Vertauschung benachbarter Buchstaben (ohne Beachtung der Gross-/Kleinschreibung)
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().flat_map(|c| c.to_lowercase()).collect();
    let b: Vec<char> = b.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// Aehnlichster Name aus `names` (hoechstens ein Fuenftel der Laenge, mindestens 1 Abweichung erlaubt)
fn nearest<'a>(word: &str, names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    if word.chars().count() < 3 {
        return None;
    }
    let max = (word.chars().count() / 4).max(1);
    let mut best: Option<(usize, &str)> = None;
    for n in names {
        if n.eq_ignore_ascii_case(word) {
            continue;
        }
        let d = distance(word, n);
        if d <= max && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, n));
        }
    }
    best.map(|(_, n)| n)
}

/// Schluesselwort, das `word` vermutlich meint
fn keyword_typo(word: &str, strict_names_known: bool) -> Option<&'static str> {
    if word.chars().count() < 3 || !word.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let mut best: Option<(usize, &'static str)> = None;
    for k in TYPO_TARGETS {
        let d = distance(word, k);
        let len = k.len();
        // Bei kurzen Woertern nur sicher erkennbar, wenn bekannt ist, dass es kein Name ist
        let ok = d == 1 && (len >= 5 || strict_names_known) || d == 2 && len >= 7 && strict_names_known;
        if ok && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

fn quote_if_needed(name: &str) -> String {
    let simple = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit());
    if simple { name.to_string() } else { format!("`{name}`") }
}

// ---------------------------------------------------------------------------
// Pruefung

/// Was die Pruefung ueber die Datenbank weiss
pub struct Env<'a> {
    pub schema: Option<&'a Schema>,
    pub databases: &'a [String],
}

struct Scope<'a> {
    /// Alias bzw. Tabellenname (klein) -> Tabelle (None: abgeleitete Tabelle o. ae., Spalten unbekannt)
    entries: Vec<(String, Option<&'a TableInfo>)>,
    /// Fremde oder abgeleitete Quellen vorhanden: Spalten koennen nicht sicher geprueft werden
    lenient: bool,
}

pub fn lint(text: &str, env: &Env) -> Vec<Problem> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_CHARS {
        return Vec::new();
    }
    let Lexed { toks, mut problems } = lex(&chars);
    let tables: HashMap<String, &TableInfo> = env.schema.map(|s| s.tables.iter().map(|t| (t.name.to_lowercase(), t)).collect()).unwrap_or_default();
    // Anweisungen trennen; Prozeduren (BEGIN ... END) und DELIMITER werden nicht geprueft
    let mut start = 0;
    let mut stmts: Vec<&[Tok]> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind == Kind::Semi {
            stmts.push(&toks[start..i]);
            start = i + 1;
        }
    }
    stmts.push(&toks[start..]);
    for stmt in stmts {
        let Some(first) = stmt.first() else { continue };
        let fw = if first.kind == Kind::Word { first.upper() } else { String::new() };
        let routine = fw == "CREATE" && stmt.iter().take(8).any(|t| ["PROCEDURE", "FUNCTION", "TRIGGER", "EVENT"].iter().any(|k| t.is_word(k)));
        if fw == "DELIMITER" || routine || stmt.iter().any(|t| t.is_word("BEGIN")) && fw != "START" {
            break;
        }
        check_statement(stmt, env, &tables, &fw, &mut problems);
    }
    problems.sort_by_key(|p| (p.start, p.end));
    problems
}

fn problem(t: &Tok, severity: Severity, message: String, fix: Option<Fix>) -> Problem {
    Problem { start: t.start, end: t.end, severity, message, fix }
}

fn replace(t: &Tok, with: &str) -> Fix {
    Fix { start: t.start, end: t.end, text: with.to_string() }
}

fn check_statement<'a>(stmt: &[Tok], env: &Env, tables: &HashMap<String, &'a TableInfo>, fw: &str, out: &mut Vec<Problem>) {
    let schema_known = env.schema.is_some();
    // Erstes Wort muss ein Befehl sein
    if let Some(first) = stmt.first()
        && first.kind == Kind::Word
        && !STATEMENTS.contains(&fw)
        && let Some(k) = STATEMENTS.iter().filter(|k| k.len() >= 4).map(|k| (distance(&first.text, k), *k)).filter(|(d, _)| *d <= 2 && *d * 3 <= first.text.len()).min().map(|(_, k)| k)
    {
        out.push(problem(
            first,
            Severity::Error,
            crate::tr_format!("Unbekannter Befehl „{}“. Meinten Sie {k}?", "Unknown command “{}”. Did you mean {k}?", first.text),
            Some(replace(first, k)),
        ));
    }
    check_syntax(stmt, fw, out);
    if !schema_known {
        check_words(stmt, None, &HashSet::new(), false, out);
        return;
    }
    let dml = matches!(fw, "SELECT" | "INSERT" | "REPLACE" | "UPDATE" | "DELETE" | "WITH");
    let (scope, consumed, aliases) = resolve_tables(stmt, env, tables, fw, out);
    check_words(stmt, Some((&scope, tables, env)), &aliases, dml, out);
    let _ = consumed;
}

/// Klammern, Kommas, Gleichheitszeichen und fehlende Semikolons
fn check_syntax(stmt: &[Tok], fw: &str, out: &mut Vec<Problem>) {
    // Klammern
    let mut stack: Vec<&Tok> = Vec::new();
    for t in stmt {
        match t.kind {
            Kind::Open => stack.push(t),
            Kind::Close => {
                if stack.pop().is_none() {
                    out.push(problem(
                        t,
                        Severity::Error,
                        crate::tr_format!("Diese schließende Klammer hat keine öffnende Klammer.", "This closing parenthesis has no matching opening parenthesis."),
                        Some(Fix { start: t.start, end: t.end, text: String::new() }),
                    ));
                }
            }
            _ => {}
        }
    }
    if let (Some(open), Some(last)) = (stack.last(), stmt.last()) {
        out.push(problem(
            open,
            Severity::Error,
            crate::tr_format!("Diese Klammer wird nicht geschlossen.", "This parenthesis is never closed."),
            Some(Fix { start: last.end, end: last.end, text: ")".repeat(stack.len()) }),
        ));
    }
    // Einzelne Muster
    for (i, t) in stmt.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|k| stmt.get(k));
        let next = stmt.get(i + 1);
        match t.kind {
            Kind::Op if t.text == "==" => out.push(problem(
                t,
                Severity::Error,
                crate::tr_format!("In SQL vergleicht man mit einem einzelnen =.", "In SQL, comparison uses a single =."),
                Some(replace(t, "=")),
            )),
            Kind::Op if matches!(t.text.as_str(), "=" | "!=" | "<>") && next.is_some_and(|n| n.is_word("NULL")) => {
                let n = next.unwrap();
                let (txt, with) = if t.text == "=" { ("IS NULL", "IS NULL") } else { ("IS NOT NULL", "IS NOT NULL") };
                out.push(Problem {
                    start: t.start,
                    end: n.end,
                    severity: Severity::Warning,
                    message: crate::tr_format!("Mit NULL vergleicht man mit {txt}, ein {} NULL ist nie wahr.", "NULL is compared with {txt}; {} NULL is never true.", t.text),
                    fix: Some(Fix { start: t.start, end: n.end, text: with.into() }),
                });
            }
            Kind::Comma => {
                let closes = next.is_none()
                    || next.is_some_and(|n| n.kind == Kind::Close || ["FROM", "WHERE", "GROUP", "ORDER", "HAVING", "LIMIT", "SET", "VALUES", "ON", "JOIN", "UNION"].iter().any(|k| n.is_word(k)));
                if closes && prev.is_some_and(|p| p.kind != Kind::Open) {
                    out.push(problem(
                        t,
                        Severity::Error,
                        crate::tr_format!("Dieses Komma ist überflüssig: Danach kommt nichts mehr in der Liste.", "This comma is superfluous: nothing follows it in the list."),
                        Some(Fix { start: t.start, end: t.end, text: String::new() }),
                    ));
                }
            }
            Kind::Word if i > 0 && matches!(fw, "SELECT" | "WITH" | "UPDATE" | "DELETE" | "DROP" | "TRUNCATE" | "USE" | "SHOW") => {
                let w = t.upper();
                let starts = matches!(w.as_str(), "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "DROP" | "TRUNCATE" | "USE" | "SHOW");
                let depth = stmt[..i].iter().fold(0i32, |d, x| d + i32::from(x.kind == Kind::Open) - i32::from(x.kind == Kind::Close));
                let ends_operand = prev.is_some_and(|p| match p.kind {
                    Kind::Close | Kind::Num | Kind::Str | Kind::Quoted => true,
                    Kind::Word => !known_word(&p.upper()),
                    _ => false,
                });
                // "WITH x AS (...) SELECT" und "(...) UNION SELECT" gehoeren zusammen
                let joined = prev.is_some_and(|p| p.kind == Kind::Close) && fw == "WITH";
                if starts && depth == 0 && ends_operand && !joined && !(w == "SELECT" && prev.is_some_and(|p| p.kind == Kind::Close) && stmt[..i].iter().any(|x| x.is_word("INSERT"))) {
                    let p = prev.unwrap();
                    out.push(problem(
                        t,
                        Severity::Error,
                        crate::tr_format!("Hier fehlt vermutlich ein Semikolon.", "A semicolon is probably missing here."),
                        Some(Fix { start: p.end, end: p.end, text: ";".into() }),
                    ));
                }
            }
            _ => {}
        }
    }
}

/// Tabellen der Anweisung ermitteln und unbekannte melden.
fn resolve_tables<'a>(
    stmt: &[Tok],
    env: &Env,
    tables: &HashMap<String, &'a TableInfo>,
    fw: &str,
    out: &mut Vec<Problem>,
) -> (Scope<'a>, HashSet<usize>, HashSet<String>) {
    let mut scope = Scope { entries: Vec::new(), lenient: false };
    let mut consumed: HashSet<usize> = HashSet::new();
    let mut aliases: HashSet<String> = HashSet::new();
    // Benannte Hilfsabfragen (WITH name AS (...)) und Spaltenalias nach AS
    let mut ctes: HashSet<String> = HashSet::new();
    for (i, t) in stmt.iter().enumerate() {
        if t.is_word("AS") {
            if let Some(n) = stmt.get(i + 1)
                && n.is_name()
            {
                aliases.insert(n.text.to_lowercase());
            }
            if let (Some(p), Some(o)) = (i.checked_sub(1).and_then(|k| stmt.get(k)), stmt.get(i + 1))
                && p.is_name()
                && o.kind == Kind::Open
            {
                ctes.insert(p.text.to_lowercase());
            }
        }
    }
    let db_name = env.schema.map(|s| s.name.to_lowercase()).unwrap_or_default();
    // Klammern, die zu Funktionen wie EXTRACT(... FROM ...) gehoeren
    let mut paren_func: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < stmt.len() {
        let t = &stmt[i];
        match t.kind {
            Kind::Open => {
                let func = i.checked_sub(1).and_then(|k| stmt.get(k)).is_some_and(|p| p.kind == Kind::Word && matches!(p.upper().as_str(), "EXTRACT" | "TRIM" | "SUBSTRING" | "SUBSTR" | "POSITION" | "OVERLAY"));
                paren_func.push(func);
            }
            Kind::Close => {
                paren_func.pop();
            }
            _ => {}
        }
        let in_func = paren_func.last().copied().unwrap_or(false);
        let w = if t.kind == Kind::Word { t.upper() } else { String::new() };
        let table_context = match w.as_str() {
            "FROM" | "JOIN" if !in_func => true,
            "UPDATE" if i == 0 => true,
            "INTO" if i > 0 && matches!(stmt[0].upper().as_str(), "INSERT" | "REPLACE") => true,
            "TABLE" if matches!(fw, "ALTER" | "TRUNCATE") => true,
            "TABLE" if fw == "DROP" && !stmt.iter().any(|x| x.is_word("EXISTS")) => true,
            "TRUNCATE" if i == 0 => false,
            "DESCRIBE" | "DESC" if i == 0 => true,
            _ => false,
        };
        if table_context {
            let mut j = i + 1;
            loop {
                j = table_ref(stmt, j, env, tables, &db_name, &ctes, &mut scope, &mut consumed, out);
                // Mehrere Tabellen mit Komma (FROM a, b; UPDATE a, b SET ...)
                if matches!(w.as_str(), "FROM" | "UPDATE") && stmt.get(j).is_some_and(|c| c.kind == Kind::Comma) {
                    j += 1;
                } else {
                    break;
                }
            }
            // INSERT INTO t (spalten): Spaltennamen gegen die Tabelle pruefen
            if w == "INTO"
                && let Some(o) = stmt.get(j)
                && o.kind == Kind::Open
                && let Some((_, Some(table))) = scope.entries.last()
            {
                let cols: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
                let mut k = j + 1;
                while k < stmt.len() && stmt[k].kind != Kind::Close {
                    let c = &stmt[k];
                    if c.is_name() {
                        consumed.insert(k);
                        if !cols.iter().any(|x| x.eq_ignore_ascii_case(&c.text)) {
                            out.push(unknown_column(c, table, &cols));
                        }
                    }
                    k += 1;
                }
            }
            i = i.max(j.saturating_sub(1));
        }
        i += 1;
    }
    (scope, consumed, aliases)
}

fn unknown_column(c: &Tok, table: &TableInfo, cols: &[&str]) -> Problem {
    match nearest(&c.text, cols.iter().copied()) {
        Some(s) => problem(
            c,
            Severity::Error,
            crate::tr_format!("Die Tabelle „{}“ hat keine Spalte „{}“. Meinten Sie „{s}“?", "Table “{}” has no column “{}”. Did you mean “{s}”?", table.name, c.text),
            Some(replace(c, &quote_if_needed(s))),
        ),
        None => problem(c, Severity::Error, crate::tr_format!("Die Tabelle „{}“ hat keine Spalte „{}“.", "Table “{}” has no column “{}”.", table.name, c.text), None),
    }
}

/// Eine Tabelle ab `j` lesen (`[db.]name [[AS] alias]` oder `(Unterabfrage) alias`); liefert die Position danach.
#[allow(clippy::too_many_arguments)]
fn table_ref<'a>(
    stmt: &[Tok],
    mut j: usize,
    env: &Env,
    tables: &HashMap<String, &'a TableInfo>,
    db_name: &str,
    ctes: &HashSet<String>,
    scope: &mut Scope<'a>,
    consumed: &mut HashSet<usize>,
    out: &mut Vec<Problem>,
) -> usize {
    let Some(t) = stmt.get(j) else { return j };
    // Unterabfrage
    if t.kind == Kind::Open {
        let mut depth = 0;
        while j < stmt.len() {
            match stmt[j].kind {
                Kind::Open => depth += 1,
                Kind::Close => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        j += 1;
        scope.lenient = true;
        return alias_after(stmt, j, None, scope, consumed);
    }
    if !t.is_name() {
        return j;
    }
    let mut name_tok = t;
    let mut db: Option<&Tok> = None;
    if stmt.get(j + 1).is_some_and(|d| d.kind == Kind::Dot)
        && let Some(n) = stmt.get(j + 2)
        && n.is_name()
    {
        db = Some(t);
        name_tok = n;
        consumed.insert(j);
        j += 2;
    }
    consumed.insert(j);
    let lower = name_tok.text.to_lowercase();
    let foreign_db = db.is_some_and(|d| !d.text.eq_ignore_ascii_case(db_name));
    let table = if foreign_db { None } else { tables.get(&lower).copied() };
    if foreign_db || ctes.contains(&lower) || lower == "dual" {
        scope.lenient = true;
    } else if table.is_none() {
        let names: Vec<&str> = tables.values().map(|t| t.name.as_str()).collect();
        let dbn = env.schema.map_or("", |s| s.name.as_str());
        out.push(match nearest(&name_tok.text, names.iter().copied()) {
            Some(s) => problem(
                name_tok,
                Severity::Error,
                crate::tr_format!("Die Tabelle „{}“ gibt es in „{dbn}“ nicht. Meinten Sie „{s}“?", "Table “{}” does not exist in “{dbn}”. Did you mean “{s}”?", name_tok.text),
                Some(replace(name_tok, &quote_if_needed(s))),
            ),
            None => problem(
                name_tok,
                Severity::Error,
                crate::tr_format!("Die Tabelle „{}“ gibt es in „{dbn}“ nicht.", "Table “{}” does not exist in “{dbn}”.", name_tok.text),
                None,
            ),
        });
        scope.lenient = true;
    }
    alias_after(stmt, j + 1, Some((&lower, table)), scope, consumed)
}

/// Optionalen Alias lesen und die Tabelle im Geltungsbereich eintragen.
fn alias_after<'a>(stmt: &[Tok], mut j: usize, table: Option<(&str, Option<&'a TableInfo>)>, scope: &mut Scope<'a>, consumed: &mut HashSet<usize>) -> usize {
    let mut alias: Option<String> = None;
    if let Some(a) = stmt.get(j) {
        let explicit = a.is_word("AS");
        let cand = if explicit { stmt.get(j + 1) } else { Some(a) };
        if let Some(c) = cand
            && c.is_name()
            && (explicit || c.kind == Kind::Quoted || !known_word(&c.upper()) && keyword_typo(&c.text, true).is_none())
        {
            alias = Some(c.text.to_lowercase());
            let at = if explicit { j + 1 } else { j };
            consumed.insert(at);
            j = at + 1;
        }
    }
    match (table, alias) {
        (Some((name, t)), Some(a)) => {
            scope.entries.push((a, t));
            // Mit Alias ist die Tabelle meist nur noch unter dem Alias erreichbar; der Originalname bleibt
            // der Nachsicht halber auch erlaubt.
            scope.entries.push((name.to_string(), t));
        }
        (Some((name, t)), None) => scope.entries.push((name.to_string(), t)),
        (None, Some(a)) => scope.entries.push((a, None)),
        (None, None) => {}
    }
    j
}

/// Woerter pruefen: Tippfehler, unbekannte Spalten, qualifizierte Namen.
fn check_words<'a>(stmt: &[Tok], names: Option<(&Scope<'a>, &HashMap<String, &'a TableInfo>, &Env)>, aliases: &HashSet<String>, dml: bool, out: &mut Vec<Problem>) {
    let consumed_unused = ();
    let _ = consumed_unused;
    // Namen, die in der Anweisung bekannt sind und nie als Tippfehler gelten
    let mut known: HashSet<String> = aliases.clone();
    if let Some((scope, tables, _)) = &names {
        known.extend(tables.keys().cloned());
        for (a, t) in &scope.entries {
            known.insert(a.clone());
            if let Some(t) = t {
                known.extend(t.columns.iter().map(|c| c.name.to_lowercase()));
            }
        }
    }
    // Spalten aller Tabellen im Geltungsbereich
    let strict = names.as_ref().is_some_and(|(s, _, _)| !s.lenient && !s.entries.is_empty() && s.entries.iter().all(|(_, t)| t.is_some()));
    let mut all_cols: Vec<&str> = Vec::new();
    if let Some((scope, _, _)) = &names {
        for (_, t) in &scope.entries {
            if let Some(t) = t {
                for c in &t.columns {
                    if !all_cols.contains(&c.name.as_str()) {
                        all_cols.push(c.name.as_str());
                    }
                }
            }
        }
    }
    let schema_known = names.is_some();
    for (i, t) in stmt.iter().enumerate() {
        if t.kind != Kind::Word && t.kind != Kind::Quoted {
            continue;
        }
        let prev = i.checked_sub(1).and_then(|k| stmt.get(k));
        let next = stmt.get(i + 1);
        let after_dot = prev.is_some_and(|p| p.kind == Kind::Dot);
        let lower = t.text.to_lowercase();
        let upper = t.upper();
        // table.column
        if next.is_some_and(|n| n.kind == Kind::Dot) && !after_dot {
            if let Some((scope, tables, env)) = &names {
                let col = stmt.get(i + 2);
                if let Some((_, entry)) = scope.entries.iter().find(|(a, _)| *a == lower) {
                    if let (Some(table), Some(c)) = (entry, col)
                        && c.is_name()
                        && !table.columns.iter().any(|x| x.name.eq_ignore_ascii_case(&c.text))
                    {
                        let cols: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
                        out.push(unknown_column(c, table, &cols));
                    }
                } else if !scope.lenient
                    && dml
                    && !tables.contains_key(&lower)
                    && !env.databases.iter().any(|d| d.eq_ignore_ascii_case(&t.text))
                    && !aliases.contains(&lower)
                    && !matches!(lower.as_str(), "information_schema" | "mysql" | "performance_schema" | "sys")
                {
                    let cand: Vec<&str> = scope.entries.iter().map(|(a, _)| a.as_str()).collect();
                    out.push(match nearest(&t.text, cand.iter().copied()) {
                        Some(s) => problem(
                            t,
                            Severity::Error,
                            crate::tr_format!("„{}“ ist hier weder eine Tabelle noch ein Alias. Meinten Sie „{s}“?", "“{}” is neither a table nor an alias here. Did you mean “{s}”?", t.text),
                            Some(replace(t, s)),
                        ),
                        None => problem(
                            t,
                            Severity::Error,
                            crate::tr_format!("„{}“ ist hier weder eine Tabelle noch ein Alias.", "“{}” is neither a table nor an alias here.", t.text),
                            None,
                        ),
                    });
                }
            }
            continue;
        }
        if after_dot || t.kind == Kind::Quoted {
            // Backtick-Namen: nur als Spalte pruefen
            if t.kind == Kind::Quoted && !after_dot && dml && strict && !known.contains(&lower) && next.is_none_or(|n| n.kind != Kind::Open) {
                out.push(problem(
                    t,
                    Severity::Warning,
                    crate::tr_format!("Eine Spalte „{}“ gibt es hier nicht.", "There is no column “{}” here.", t.text),
                    nearest(&t.text, all_cols.iter().copied()).map(|s| replace(t, &quote_if_needed(s))),
                ));
            }
            continue;
        }
        if next.is_some_and(|n| n.kind == Kind::Open) {
            // Funktion: Tippfehler im Namen
            if !known_word(&upper) && !known.contains(&lower) && upper.len() >= 4 {
                let fnames = FUNCTIONS.iter().copied();
                if let Some(f) = fnames.into_iter().find(|f| f.len() >= 4 && distance(&upper, f) == 1 && f.chars().next() == upper.chars().next()) {
                    out.push(problem(
                        t,
                        Severity::Warning,
                        crate::tr_format!("Die Funktion „{}“ gibt es nicht. Meinten Sie {f}?", "There is no function “{}”. Did you mean {f}?", t.text),
                        Some(replace(t, f)),
                    ));
                }
            }
            continue;
        }
        if known_word(&upper) || known.contains(&lower) {
            continue;
        }
        // Tippfehler in Schluesselwoertern
        if let Some(k) = keyword_typo(&t.text, schema_known) {
            out.push(problem(
                t,
                Severity::Error,
                crate::tr_format!("„{}“ ist unbekannt. Meinten Sie {k}?", "“{}” is unknown. Did you mean {k}?", t.text),
                Some(replace(t, k)),
            ));
            continue;
        }
        // Unbekannte Spalte
        let implicit_alias = prev.is_some_and(|p| match p.kind {
            Kind::Close | Kind::Num | Kind::Str | Kind::Quoted => true,
            Kind::Word => !known_word(&p.upper()),
            _ => false,
        });
        if dml && strict && !implicit_alias {
            let tbl_names: Vec<&str> = names.as_ref().map(|(s, _, _)| s.entries.iter().map(|(a, _)| a.as_str()).collect()).unwrap_or_default();
            let fix = nearest(&t.text, all_cols.iter().copied()).map(|s| replace(t, &quote_if_needed(s)));
            let msg = match &fix {
                Some(f) => crate::tr_format!("Eine Spalte „{}“ gibt es hier nicht. Meinten Sie „{}“?", "There is no column “{}” here. Did you mean “{}”?", t.text, f.text.trim_matches('`')),
                None => crate::tr_format!("Eine Spalte „{}“ gibt es in {} nicht.", "There is no column “{}” in {}.", t.text, tbl_names.join(", ")),
            };
            out.push(problem(t, Severity::Warning, msg, fix));
        }
    }
}

// ---------------------------------------------------------------------------
// Korrektur

/// Wendet die Korrekturen an (ueberlappende werden uebersprungen). Liefert den neuen Text und die Anzahl.
pub fn apply_fixes(text: &str, problems: &[Problem]) -> (String, usize) {
    let mut fixes: Vec<&Fix> = problems.iter().filter_map(|p| p.fix.as_ref()).collect();
    fixes.sort_by_key(|f| (f.start, f.end));
    let mut chosen: Vec<&Fix> = Vec::new();
    for f in fixes {
        if chosen.last().is_none_or(|l| f.start >= l.end && !(f.start == l.start && f.end == l.end)) {
            chosen.push(f);
        }
    }
    let bytes: Vec<usize> = text.char_indices().map(|(b, _)| b).chain(std::iter::once(text.len())).collect();
    let at = |c: usize| bytes[c.min(bytes.len() - 1)];
    let mut out = text.to_string();
    for f in chosen.iter().rev() {
        out.replace_range(at(f.start)..at(f.end), &f.text);
    }
    (out, chosen.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, TableInfo};

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
            tables: vec![table("schueler", &["id", "vorname", "klasse_id", "geboren"]), table("klasse", &["id", "name"])],
            ..Default::default()
        }
    }

    fn check(sql: &str) -> Vec<Problem> {
        let s = schema();
        lint(sql, &Env { schema: Some(&s), databases: &["schule".to_string()] })
    }

    fn messages(sql: &str) -> Vec<String> {
        check(sql).into_iter().map(|p| p.message).collect()
    }

    fn fixed(sql: &str) -> String {
        apply_fixes(sql, &check(sql)).0
    }

    #[test]
    fn valid_queries_have_no_problems() {
        for q in [
            "SELECT * FROM schueler",
            "SELECT s.vorname, k.name FROM schueler s JOIN klasse k ON k.id = s.klasse_id WHERE s.id > 3 ORDER BY k.name DESC LIMIT 5;",
            "SELECT COUNT(*) AS anzahl FROM schueler GROUP BY klasse_id HAVING anzahl > 2 ORDER BY anzahl",
            "SELECT vorname, (SELECT MAX(id) FROM klasse) m FROM schueler WHERE NOT EXISTS (SELECT 1 FROM klasse WHERE klasse.id = schueler.klasse_id)",
            "INSERT INTO schueler (vorname, klasse_id) VALUES ('Mia', 3);",
            "UPDATE schueler SET vorname = 'X' WHERE id = 1",
            "DELETE FROM schueler WHERE geboren < '2000-01-01'",
            "SELECT EXTRACT(YEAR FROM geboren) FROM schueler",
            "WITH x AS (SELECT id FROM klasse) SELECT * FROM x",
            "SELECT * FROM (SELECT id FROM klasse) t WHERE t.id = 1",
            "SELECT DATE_ADD(geboren, INTERVAL 1 DAY) FROM schueler",
            "CREATE TABLE neu (id INT PRIMARY KEY, name VARCHAR(20));",
            "SELECT CASE WHEN id > 1 THEN 'a' ELSE 'b' END AS c FROM schueler",
            "SELECT 1; -- Kommentar\nSELECT 2;",
            "SHOW TABLES",
            "USE schule;",
            "SET @x = 5; SELECT @x;",
            "SELECT vorname FROM schueler ORDER BY 1",
            "SELECT s.* FROM schueler AS s",
            "SELECT * FROM schule.schueler",
            "SELECT * FROM `schueler` WHERE `vorname` = \"Mia\"",
        ] {
            assert!(check(q).is_empty(), "{q}: {:?}", messages(q));
        }
    }

    #[test]
    fn typos_in_keywords_are_found_and_fixed() {
        assert_eq!(fixed("SELCT * FROM schueler"), "SELECT * FROM schueler");
        assert_eq!(fixed("SELECT * FORM schueler"), "SELECT * FROM schueler");
        assert_eq!(fixed("SELECT * FROM schueler WEHRE id = 1"), "SELECT * FROM schueler WHERE id = 1");
        assert_eq!(fixed("SELECT * FROM schueler ODER BY id"), "SELECT * FROM schueler ORDER BY id");
        assert_eq!(fixed("SELECT * FROM schueler GRUOP BY id"), "SELECT * FROM schueler GROUP BY id");
        assert_eq!(fixed("SELECT * FROM schueler ORDR BY id"), "SELECT * FROM schueler ORDER BY id");
    }

    #[test]
    fn unknown_tables_and_columns() {
        assert_eq!(fixed("SELECT * FROM schueller"), "SELECT * FROM schueler");
        assert_eq!(fixed("SELECT vornam FROM schueler"), "SELECT vorname FROM schueler");
        assert_eq!(fixed("SELECT s.vornme FROM schueler s"), "SELECT s.vorname FROM schueler s");
        assert_eq!(fixed("SELECT x.id FROM schueler s"), "SELECT x.id FROM schueler s");
        assert!(messages("SELECT x.id FROM schueler s").iter().any(|m| m.contains("weder eine Tabelle noch ein Alias") || m.contains("neither")));
        assert_eq!(fixed("INSERT INTO schueler (vornam) VALUES ('a')"), "INSERT INTO schueler (vorname) VALUES ('a')");
        assert_eq!(check("SELECT * FROM gibtesnicht").len(), 1);
        // Keine Spalte, nichts Aehnliches: Hinweis ohne Korrektur
        let p = check("SELECT zzzzzz FROM schueler");
        assert_eq!(p.len(), 1);
        assert!(p[0].fix.is_none());
        assert_eq!(p[0].severity, Severity::Warning);
    }

    #[test]
    fn syntax_slips() {
        assert_eq!(fixed("SELECT * FROM schueler WHERE id == 1"), "SELECT * FROM schueler WHERE id = 1");
        assert_eq!(fixed("SELECT * FROM schueler WHERE klasse_id = NULL"), "SELECT * FROM schueler WHERE klasse_id IS NULL");
        assert_eq!(fixed("SELECT * FROM schueler WHERE klasse_id <> NULL"), "SELECT * FROM schueler WHERE klasse_id IS NOT NULL");
        assert_eq!(fixed("SELECT id, FROM schueler"), "SELECT id FROM schueler");
        assert_eq!(fixed("SELECT COUNT(id FROM schueler"), "SELECT COUNT(id FROM schueler)");
        assert_eq!(fixed("SELECT id FROM schueler)"), "SELECT id FROM schueler");
        assert_eq!(fixed("SELECT * FROM schueler WHERE vorname = 'Mia"), "SELECT * FROM schueler WHERE vorname = 'Mia'");
        assert_eq!(fixed("SELECT * FROM schueler\nSELECT * FROM klasse"), "SELECT * FROM schueler;\nSELECT * FROM klasse");
        assert_eq!(fixed("SELECT 1 /* offen"), "SELECT 1 /* offen */");
    }

    #[test]
    fn without_a_schema_only_clear_slips_are_reported() {
        let none = |q: &str| lint(q, &Env { schema: None, databases: &[] });
        assert!(none("SELECT * FROM whatever WHERE a = 1").is_empty());
        assert_eq!(apply_fixes("SELCT 1", &none("SELCT 1")).0, "SELECT 1");
        // FORM koennte ein Spaltenname sein, ohne Datenbank wird nicht korrigiert
        assert!(none("SELECT form FROM t").is_empty());
        assert!(none("SELECT ort FROM t WHERE ort = 'x'").is_empty());
    }

    #[test]
    fn column_named_like_a_keyword_typo_is_fine_when_it_exists() {
        let s = Schema { name: "d".into(), tables: vec![table("t", &["form", "ort"])], ..Default::default() };
        let env = Env { schema: Some(&s), databases: &[] };
        assert!(lint("SELECT form, ort FROM t WHERE form = 'x'", &env).is_empty());
    }

    #[test]
    fn procedures_and_delimiter_are_skipped() {
        assert!(check("DELIMITER //\nCREATE PROCEDURE p() BEGIN SELCT 1; END //").is_empty());
    }

    #[test]
    fn fixes_do_not_overlap_and_keep_unicode_positions() {
        let q = "SELECT 'äöü', vornam FROM schueler";
        assert_eq!(fixed(q), "SELECT 'äöü', vorname FROM schueler");
        assert_eq!(apply_fixes("a", &[]).1, 0);
    }
}
