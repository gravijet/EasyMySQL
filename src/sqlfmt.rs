// SQL formatieren: Schluesselwoerter gross, jede Klausel (SELECT, FROM, WHERE, ...) auf eine
// eigene Zeile. Listen und Bedingungen werden nur umbrochen, wenn die Zeile zu lang wird.
// Kommentare, DELIMITER und Prozeduren (BEGIN ... END) bleiben unveraendert.

const WIDTH: usize = 100;

/// Woerter, die immer gross geschrieben werden
const KEYWORDS: &[&str] = &[
    "ADD", "AFTER", "ALGORITHM", "ALL", "ALTER", "ANALYZE", "AND", "ANY", "AS", "ASC", "AUTO_INCREMENT", "BETWEEN", "BINARY", "BY",
    "CASCADE", "CASE", "CHANGE", "CHARACTER", "CHARSET", "CHECK", "COLLATE", "COLUMN", "COLUMNS", "COMMIT", "CONSTRAINT", "CREATE", "CROSS",
    "CURRENT", "CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP", "DATABASE", "DATABASES", "DEFAULT", "DELETE", "DESC", "DESCRIBE",
    "DISTINCT", "DIV", "DROP", "DUPLICATE", "ELSE", "END", "ENGINE", "ESCAPE", "EXCEPT", "EXISTS", "EXPLAIN", "FALSE", "FIRST",
    "FOLLOWING", "FOR", "FOREIGN", "FROM", "FULL", "FULLTEXT", "GRANT", "GROUP", "HAVING", "IF", "IGNORE", "IN", "INDEX", "INNER",
    "INSERT", "INTERSECT", "INTERVAL", "INTO", "IS", "JOIN", "KEY", "KEYS", "LAST", "LEFT", "LIKE", "LIMIT", "LOCK", "MODIFY", "NATURAL",
    "NO", "NOT", "NULL", "NULLS", "OFFSET", "ON", "OR", "ORDER", "OUTER", "OVER", "PARTITION", "PRECEDING", "PRIMARY", "PROCEDURE",
    "RANGE", "RECURSIVE", "REFERENCES", "REGEXP", "RENAME", "REPLACE", "RESTRICT", "RETURNING", "REVOKE", "RIGHT", "RLIKE", "ROLLBACK",
    "ROLLUP", "ROW", "ROWS", "SAVEPOINT", "SCHEMA", "SELECT", "SET", "SHOW", "SOME", "START", "STRAIGHT_JOIN", "TABLE", "TABLES",
    "TEMPORARY", "THEN", "TO", "TRANSACTION", "TRIGGER", "TRUE", "TRUNCATE", "UNBOUNDED", "UNION", "UNIQUE", "UNSIGNED", "UPDATE",
    "USE", "USING", "VALUE", "VALUES", "VIEW", "WHEN", "WHERE", "WINDOW", "WITH", "XOR", "ZEROFILL",
];

/// Datentypen: gross nur in CREATE/ALTER, nach AS (CAST) oder vor "(" – sonst koennten Spalten
/// wie "date" oder "text" umgeschrieben werden.
const TYPES: &[&str] = &[
    "BIGINT", "BIT", "BLOB", "BOOL", "BOOLEAN", "CHAR", "COMMENT", "DATE", "DATETIME", "DEC", "DECIMAL", "DOUBLE", "ENUM", "FLOAT", "INT",
    "INTEGER", "JSON", "LONGBLOB", "LONGTEXT", "MEDIUMBLOB", "MEDIUMINT", "MEDIUMTEXT", "NUMERIC", "REAL", "SIGNED", "SMALLINT", "TEXT",
    "TIME", "TIMESTAMP", "TINYBLOB", "TINYINT", "TINYTEXT", "VARBINARY", "VARCHAR", "YEAR",
];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Word(String),
    /// Text, `Bezeichner`, Zahl, @Variable – unveraendert
    Lit(String),
    Op(String),
    Comma,
    Dot,
    Open,
    Close,
    LineComment(String),
    BlockComment(String),
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn tokenize(s: &str) -> Vec<Tok> {
    let ch: Vec<char> = s.chars().collect();
    let n = ch.len();
    let mut out = Vec::new();
    let mut i = 0;
    let take = |a: usize, b: usize| ch[a..b].iter().collect::<String>();
    while i < n {
        let c = ch[i];
        let nx = ch.get(i + 1).copied().unwrap_or('\0');
        let start = i;
        if c.is_whitespace() {
            i += 1;
        } else if (c == '-' && nx == '-' && ch.get(i + 2).is_none_or(|c| c.is_whitespace())) || c == '#' {
            while i < n && ch[i] != '\n' {
                i += 1;
            }
            out.push(Tok::LineComment(take(start, i).trim_end().to_string()));
        } else if c == '/' && nx == '*' {
            i += 2;
            while i < n && !(ch[i] == '*' && ch.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(n);
            out.push(Tok::BlockComment(take(start, i)));
        } else if c == '\'' || c == '"' || c == '`' {
            i += 1;
            while i < n {
                if ch[i] == '\\' && c != '`' {
                    i += 2;
                    continue;
                }
                if ch[i] == c {
                    // verdoppeltes Anfuehrungszeichen = Zeichen im Text
                    if ch.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            out.push(Tok::Lit(take(start, i)));
        } else if c.is_ascii_digit() || (c == '.' && nx.is_ascii_digit()) {
            while i < n && (ch[i].is_ascii_alphanumeric() || ch[i] == '.' || ((ch[i] == '+' || ch[i] == '-') && matches!(ch[i - 1], 'e' | 'E'))) {
                i += 1;
            }
            out.push(Tok::Lit(take(start, i)));
        } else if c == '@' {
            i += 1;
            while i < n && (is_word_char(ch[i]) || ch[i] == '@' || ch[i] == '.') {
                i += 1;
            }
            out.push(Tok::Lit(take(start, i)));
        } else if is_word_char(c) {
            while i < n && is_word_char(ch[i]) {
                i += 1;
            }
            out.push(Tok::Word(take(start, i)));
        } else {
            i += 1;
            out.push(match c {
                ',' => Tok::Comma,
                '.' => Tok::Dot,
                '(' => Tok::Open,
                ')' => Tok::Close,
                _ => {
                    let two: String = [c, nx].iter().collect();
                    if ["<=", ">=", "<>", "!=", "||", "&&", ":=", "<<", ">>", "->"].contains(&two.as_str()) {
                        i += 1;
                        if two == "<=" && ch.get(i) == Some(&'>') {
                            i += 1;
                            Tok::Op("<=>".into())
                        } else if two == "->" && ch.get(i) == Some(&'>') {
                            i += 1;
                            Tok::Op("->>".into())
                        } else {
                            Tok::Op(two)
                        }
                    } else {
                        Tok::Op(c.to_string())
                    }
                }
            });
        }
    }
    out
}

/// Baum aus Klammergruppen
#[derive(Clone, Debug)]
enum Node {
    T(Tok),
    G(Vec<Node>),
}

fn tree(toks: &[Tok]) -> Vec<Node> {
    fn build(toks: &[Tok], i: &mut usize) -> Vec<Node> {
        let mut v = Vec::new();
        while *i < toks.len() {
            let t = &toks[*i];
            *i += 1;
            match t {
                Tok::Open => v.push(Node::G(build(toks, i))),
                Tok::Close => return v,
                t => v.push(Node::T(t.clone())),
            }
        }
        v
    }
    let mut i = 0;
    let mut out = Vec::new();
    while i < toks.len() {
        out.extend(build(toks, &mut i));
    }
    out
}

fn upper(w: &str) -> String {
    w.to_ascii_uppercase()
}

fn word_of(n: &Node) -> Option<String> {
    match n {
        Node::T(Tok::Word(w)) => Some(upper(w)),
        _ => None,
    }
}

fn is_subquery(g: &[Node]) -> bool {
    matches!(g.first().and_then(word_of).as_deref(), Some("SELECT" | "WITH"))
}

struct Fmt {
    /// CREATE/ALTER: Datentypen gross schreiben, Spaltenliste je Zeile
    ddl: bool,
}

impl Fmt {
    fn word(&self, w: &str, prev: Option<&Node>, next: Option<&Node>) -> String {
        let u = upper(w);
        let before_paren = matches!(next, Some(Node::G(_)));
        let after_dot = matches!(prev, Some(Node::T(Tok::Dot)));
        let next_dot = matches!(next, Some(Node::T(Tok::Dot)));
        if after_dot || next_dot {
            return w.to_string();
        }
        if KEYWORDS.binary_search(&u.as_str()).is_ok() {
            return u;
        }
        let after_as = prev.and_then(word_of).as_deref() == Some("AS");
        if TYPES.binary_search(&u.as_str()).is_ok() && (self.ddl || after_as || before_paren) {
            return u;
        }
        if before_paren && crate::sqledit::FUNCTIONS.contains(&u.as_str()) {
            return u;
        }
        w.to_string()
    }

    fn tok_text(&self, t: &Tok, prev: Option<&Node>, next: Option<&Node>) -> String {
        match t {
            Tok::Word(w) => self.word(w, prev, next),
            Tok::Lit(s) | Tok::Op(s) | Tok::LineComment(s) | Tok::BlockComment(s) => s.clone(),
            Tok::Comma => ",".into(),
            Tok::Dot => ".".into(),
            Tok::Open => "(".into(),
            Tok::Close => ")".into(),
        }
    }

    /// Alles in einer Zeile (Zeilenkommentare erzwingen einen Umbruch mit `indent`).
    fn inline(&self, nodes: &[Node], indent: usize) -> String {
        let mut out = String::new();
        let mut prev: Option<&Node> = None;
        let mut prev2: Option<&Node> = None;
        let mut newline = false;
        for (i, n) in nodes.iter().enumerate() {
            let next = nodes.get(i + 1);
            let text = match n {
                Node::T(t) => self.tok_text(t, prev, next),
                Node::G(g) => format!("({})", self.inline(g, indent + 2)),
            };
            if !out.is_empty() && !newline && self.space_between(prev2, prev, n) {
                out.push(' ');
            }
            if newline {
                out.push('\n');
                out.push_str(&" ".repeat(indent));
                newline = false;
            }
            out.push_str(&text);
            if matches!(n, Node::T(Tok::LineComment(_))) {
                newline = true;
            }
            prev2 = prev;
            prev = Some(n);
        }
        if newline && !out.is_empty() {
            // Kommentar am Ende: Zeilenende sicherstellen
            out.push('\n');
            out.push_str(&" ".repeat(indent));
        }
        out
    }

    fn space_between(&self, prev2: Option<&Node>, prev: Option<&Node>, cur: &Node) -> bool {
        let Some(prev) = prev else { return false };
        match (prev, cur) {
            (_, Node::T(Tok::Comma | Tok::Dot)) => false,
            (Node::T(Tok::Dot), _) => false,
            (Node::T(Tok::Op(o)), _) if o == "-" || o == "+" || o == "~" || o == "!" => {
                // Vorzeichen: kein Leerzeichen danach
                !is_unary(prev2)
            }
            (Node::T(Tok::Op(o)), Node::T(Tok::Word(_) | Tok::Lit(_))) if o == "@" => false,
            (Node::T(Tok::Word(w)), Node::G(_)) => {
                let u = upper(w);
                if KEYWORDS.binary_search(&u.as_str()).is_ok() && !matches!(u.as_str(), "IF" | "REPLACE" | "LEFT" | "RIGHT" | "VALUES" | "INSERT") {
                    return true;
                }
                if matches!(u.as_str(), "VALUES") {
                    return true;
                }
                // Tabellenname vor Spaltenliste: INSERT INTO t (a, b), CREATE TABLE t (...)
                matches!(prev2.and_then(word_of).as_deref(), Some("INTO" | "TABLE" | "EXISTS" | "REFERENCES" | "VIEW" | "INDEX" | "KEY"))
                    || self.ddl && matches!(prev2.and_then(word_of).as_deref(), Some("ON"))
            }
            (Node::T(Tok::Lit(l)), Node::G(_)) if l.starts_with('`') => {
                matches!(prev2.and_then(word_of).as_deref(), Some("INTO" | "TABLE" | "EXISTS" | "REFERENCES" | "VIEW" | "INDEX" | "KEY" | "ON"))
            }
            (Node::T(Tok::Op(o)), Node::G(_)) if o == "(" => false,
            _ => true,
        }
    }

    /// Anweisung bzw. Unterabfrage in Klauseln zerlegen und mehrzeilig ausgeben.
    fn block(&self, nodes: &[Node], indent: usize) -> String {
        let clauses = split_clauses(nodes);
        let pad = " ".repeat(indent);
        let mut lines: Vec<String> = Vec::new();
        for (kw, body) in clauses {
            lines.push(self.clause(&kw, body, indent));
        }
        lines.iter().map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n")
    }

    /// Eine Klausel, z. B. "WHERE a = 1 AND b = 2". Rueckgabe ohne fuehrende Einrueckung.
    fn clause(&self, kw: &str, body: &[Node], indent: usize) -> String {
        let head = if kw.is_empty() { String::new() } else { format!("{kw} ") };
        let one = format!("{head}{}", self.inline(body, indent + 2)).trim_end().to_string();
        if indent + one.chars().count() <= WIDTH && !one.contains('\n') {
            return one;
        }
        let pad = " ".repeat(indent);
        let cont = format!("\n{pad}  ");
        match kw {
            "WHERE" | "HAVING" | "ON" => {
                let parts = split_logic(body);
                if parts.len() > 1 {
                    let items: Vec<String> = parts
                        .iter()
                        .enumerate()
                        .map(|(i, (op, p))| {
                            let t = self.expand(p, indent + 2);
                            if i == 0 { t } else { format!("{op} {t}") }
                        })
                        .collect();
                    return format!("{head}{}", items.join(&cont));
                }
            }
            "VALUES" | "VALUE" => {
                let items: Vec<String> = split_commas(body).iter().map(|p| self.expand(p, indent + 2)).collect();
                return format!("{head}{}", items.join(&format!(",{cont}")));
            }
            _ => {}
        }
        // Join-Klausel: ON-Bedingung auf eigene Zeile
        if kw.ends_with("JOIN") {
            if let Some(pos) = body.iter().position(|n| word_of(n).as_deref() == Some("ON")) {
                let table = self.inline(&body[..pos], indent + 2);
                let on = self.clause("ON", &body[pos + 1..], indent + 2);
                return format!("{head}{table}{cont}{on}");
            }
        }
        // Kommalisten fortlaufend fuellen
        let items = split_commas(body);
        if items.len() > 1 {
            let mut out = head.clone();
            let mut col = indent + out.chars().count();
            for (i, it) in items.iter().enumerate() {
                let mut t = self.expand(it, indent + 2);
                if i + 1 < items.len() {
                    t.push(',');
                }
                let w = t.lines().next().unwrap_or("").chars().count();
                if i > 0 && (col + 1 + w > WIDTH || t.contains('\n')) {
                    out.push_str(&cont);
                    col = indent + 2;
                } else if i > 0 {
                    out.push(' ');
                    col += 1;
                }
                col = if t.contains('\n') { t.lines().last().unwrap_or("").chars().count() } else { col + w };
                out.push_str(&t);
            }
            return out;
        }
        format!("{head}{}", self.expand(body, indent + 2))
    }

    /// Ausdruck; Unterabfragen und lange Spaltendefinitionen werden mehrzeilig.
    fn expand(&self, nodes: &[Node], indent: usize) -> String {
        let one = self.inline(nodes, indent);
        if indent + one.chars().count() <= WIDTH && !one.contains('\n') {
            return one;
        }
        let pad = " ".repeat(indent.saturating_sub(2));
        let mut out = String::new();
        let mut prev: Option<&Node> = None;
        let mut prev2: Option<&Node> = None;
        for (i, n) in nodes.iter().enumerate() {
            if !out.is_empty() && self.space_between(prev2, prev, n) && !out.ends_with('\n') {
                out.push(' ');
            }
            match n {
                Node::G(g) if is_subquery(g) => {
                    let inner = self.inline(g, indent);
                    if indent + inner.chars().count() + 2 <= WIDTH && !inner.contains('\n') {
                        out.push_str(&format!("({inner})"));
                    } else {
                        out.push_str(&format!("(\n{}\n{pad})", self.block(g, indent)));
                    }
                }
                Node::G(g) if self.ddl && split_commas(g).len() > 1 => {
                    let items: Vec<String> = split_commas(g).iter().map(|p| format!("{pad}  {}", self.inline(p, indent + 2))).collect();
                    out.push_str(&format!("(\n{}\n{pad})", items.join(",\n")));
                }
                Node::G(g) => out.push_str(&format!("({})", self.inline(g, indent + 2))),
                Node::T(t) => {
                    out.push_str(&self.tok_text(t, prev, nodes.get(i + 1)));
                    if matches!(t, Tok::LineComment(_)) {
                        out.push('\n');
                        out.push_str(&" ".repeat(indent));
                    }
                }
            }
            prev2 = prev;
            prev = Some(n);
        }
        out.trim_end().to_string()
    }
}

fn is_unary(before: Option<&Node>) -> bool {
    match before {
        None => true,
        Some(Node::T(Tok::Op(_) | Tok::Comma)) => true,
        Some(Node::T(Tok::Word(w))) => KEYWORDS.binary_search(&upper(w).as_str()).is_ok(),
        _ => false,
    }
}

/// An Kommas der obersten Ebene trennen
fn split_commas(nodes: &[Node]) -> Vec<&[Node]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, n) in nodes.iter().enumerate() {
        if matches!(n, Node::T(Tok::Comma)) {
            out.push(&nodes[start..i]);
            start = i + 1;
        }
    }
    out.push(&nodes[start..]);
    out
}

/// An AND/OR der obersten Ebene trennen (nicht das AND von BETWEEN)
fn split_logic(nodes: &[Node]) -> Vec<(String, &[Node])> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut op = String::new();
    let mut between = false;
    let mut case_depth = 0;
    for (i, n) in nodes.iter().enumerate() {
        match word_of(n).as_deref() {
            Some("BETWEEN") => between = true,
            Some("CASE") => case_depth += 1,
            Some("END") if case_depth > 0 => case_depth -= 1,
            Some(w @ ("AND" | "OR" | "XOR")) if case_depth == 0 => {
                if w == "AND" && between {
                    between = false;
                    continue;
                }
                out.push((op.clone(), &nodes[start..i]));
                op = w.to_string();
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((op, &nodes[start..]));
    out
}

/// Mehrwort-Schluesselwoerter am Anfang einer Klausel
const CLAUSES: &[&[&str]] = &[
    &["SELECT"],
    &["FROM"],
    &["WHERE"],
    &["GROUP", "BY"],
    &["HAVING"],
    &["WINDOW"],
    &["ORDER", "BY"],
    &["LIMIT"],
    &["UNION", "ALL"],
    &["UNION", "DISTINCT"],
    &["UNION"],
    &["EXCEPT", "ALL"],
    &["EXCEPT"],
    &["INTERSECT", "ALL"],
    &["INTERSECT"],
    &["ON", "DUPLICATE", "KEY", "UPDATE"],
    &["RETURNING"],
    &["NATURAL", "LEFT", "OUTER", "JOIN"],
    &["NATURAL", "RIGHT", "OUTER", "JOIN"],
    &["NATURAL", "LEFT", "JOIN"],
    &["NATURAL", "RIGHT", "JOIN"],
    &["NATURAL", "JOIN"],
    &["LEFT", "OUTER", "JOIN"],
    &["RIGHT", "OUTER", "JOIN"],
    &["FULL", "OUTER", "JOIN"],
    &["LEFT", "JOIN"],
    &["RIGHT", "JOIN"],
    &["INNER", "JOIN"],
    &["CROSS", "JOIN"],
    &["STRAIGHT_JOIN"],
    &["JOIN"],
];

/// Klauseln der obersten Ebene: (Schluesselwort, Inhalt)
fn split_clauses(nodes: &[Node]) -> Vec<(String, &[Node])> {
    let first = nodes.first().and_then(word_of).unwrap_or_default();
    let dml = matches!(first.as_str(), "SELECT" | "WITH" | "INSERT" | "REPLACE" | "UPDATE" | "DELETE");
    let is_update = first == "UPDATE";
    let is_insert = matches!(first.as_str(), "INSERT" | "REPLACE");
    // CREATE VIEW ... AS SELECT: ab SELECT wie eine Abfrage
    let view_select = !dml && first == "CREATE" && nodes.iter().any(|n| word_of(n).as_deref() == Some("VIEW"));
    let mut out: Vec<(String, &[Node])> = Vec::new();
    let mut kw = String::new();
    let mut start = 0;
    let mut i = 0;
    let mut seen_select = false;
    while i < nodes.len() {
        let mut hit: Option<(String, usize)> = None;
        if dml || view_select {
            for c in CLAUSES {
                if c.iter().enumerate().all(|(k, w)| nodes.get(i + k).and_then(word_of).as_deref() == Some(*w)) {
                    hit = Some((c.join(" "), c.len()));
                    break;
                }
            }
            if !dml && !seen_select && hit.as_ref().is_some_and(|h| h.0 != "SELECT") {
                hit = None;
            }
            let w = nodes.get(i).and_then(word_of);
            if hit.is_none() && i > 0 {
                // SET nur bei UPDATE/INSERT, VALUES nur bei INSERT
                if (is_update || is_insert) && w.as_deref() == Some("SET") && !matches!(nodes.get(i - 1).and_then(word_of).as_deref(), Some("CHARACTER")) {
                    hit = Some(("SET".into(), 1));
                } else if is_insert && matches!(w.as_deref(), Some("VALUES" | "VALUE")) {
                    hit = Some((w.clone().unwrap_or_default(), 1));
                } else if first == "WITH" && matches!(nodes.get(i - 1), Some(Node::G(_))) && matches!(w.as_deref(), Some("UPDATE" | "DELETE" | "INSERT")) {
                    hit = Some((w.clone().unwrap_or_default(), 1));
                }
            }
        }
        if let Some((k, len)) = hit {
            if k == "SELECT" {
                seen_select = true;
            }
            if i > start || !kw.is_empty() {
                out.push((kw.clone(), &nodes[start..i]));
            }
            kw = k;
            i += len;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push((kw, &nodes[start..]));
    out.retain(|(k, b)| !k.is_empty() || !b.is_empty());
    out
}

fn format_statement(sql: &str) -> String {
    let toks = tokenize(sql);
    let words: Vec<String> = toks
        .iter()
        .filter_map(|t| match t {
            Tok::Word(w) => Some(upper(w)),
            _ => None,
        })
        .collect();
    let first = words.first().cloned().unwrap_or_default();
    // Prozeduren, Funktionen, Trigger, Ereignisse: nicht anfassen
    let routine = first == "CREATE" && words.iter().take(8).any(|w| matches!(w.as_str(), "PROCEDURE" | "FUNCTION" | "TRIGGER" | "EVENT"));
    if routine || words.iter().any(|w| w == "BEGIN") || first == "DELIMITER" {
        return sql.trim().to_string();
    }
    let f = Fmt { ddl: matches!(first.as_str(), "CREATE" | "ALTER") };
    let nodes = tree(&toks);
    // Eine Klammer mehr geschlossen als geoeffnet o. ae.: lieber unveraendert lassen
    let opens = toks.iter().filter(|t| **t == Tok::Open).count();
    let closes = toks.iter().filter(|t| **t == Tok::Close).count();
    if opens != closes {
        return sql.trim().to_string();
    }
    f.block(&nodes, 0).trim_end().to_string()
}

/// Ganzes Skript formatieren.
pub fn format_sql(text: &str) -> String {
    let stmts = crate::db::split_statements(text);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut pos = 0;
    let push_gap = |out: &mut String, gap: &str, leading: bool| {
        // Zwischenraum: Trennzeichen, Kommentare, DELIMITER; hoechstens eine Leerzeile
        let mut blank = 0;
        let segs: Vec<&str> = gap.split('\n').collect();
        let last = segs.len() - 1;
        for (i, line) in segs.iter().enumerate() {
            let l = line.trim_end();
            if i == 0 && !leading {
                // Trennzeichen direkt anhaengen, "END //" behaelt sein Leerzeichen
                let t = l.trim_start();
                if !t.is_empty() && !t.starts_with(';') {
                    out.push(' ');
                }
                out.push_str(t);
                continue;
            }
            if l.trim().is_empty() {
                if i == last {
                    // Einrueckung vor der naechsten Anweisung, keine Leerzeile
                    continue;
                }
                blank += 1;
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
                if blank > 0 {
                    out.push('\n');
                }
            }
            blank = 0;
            out.push_str(l);
        }
        blank
    };
    for (k, s) in stmts.iter().enumerate() {
        let gap: String = chars[pos..s.start].iter().collect();
        let blank = push_gap(&mut out, &gap, k == 0);
        if !out.is_empty() {
            out.push('\n');
            if blank > 0 && k > 0 {
                out.push('\n');
            }
        }
        out.push_str(&format_statement(&s.sql));
        pos = s.end;
    }
    let tail: String = chars[pos.min(chars.len())..].iter().collect();
    push_gap(&mut out, &tail, stmts.is_empty());
    let mut out = out.trim_end().to_string();
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_sorted() {
        let mut k = KEYWORDS.to_vec();
        k.sort();
        assert_eq!(k, KEYWORDS);
        let mut t = TYPES.to_vec();
        t.sort();
        assert_eq!(t, TYPES);
    }

    #[test]
    fn short_query_stays_compact() {
        assert_eq!(format_sql("select a,b from t where x=1;"), "SELECT a, b\nFROM t\nWHERE x = 1;\n");
        assert_eq!(format_sql("select count(*) as n, max(p.preis) from produkt p"), "SELECT COUNT(*) AS n, MAX(p.preis)\nFROM produkt p\n");
    }

    #[test]
    fn joins_group_order() {
        let f = format_sql(
            "select k.name, sum(b.betrag) summe from kunde k left join bestellung b on b.kunde_id=k.id group by k.name having sum(b.betrag)>100 order by summe desc limit 10",
        );
        assert_eq!(
            f,
            "SELECT k.name, SUM(b.betrag) summe\nFROM kunde k\nLEFT JOIN bestellung b ON b.kunde_id = k.id\nGROUP BY k.name\nHAVING SUM(b.betrag) > 100\nORDER BY summe DESC\nLIMIT 10\n"
        );
    }

    #[test]
    fn long_lists_are_filled_not_one_per_line() {
        let cols: Vec<String> = (1..=30).map(|i| format!("spalte_{i}")).collect();
        let f = format_sql(&format!("select {} from t", cols.join(",")));
        let lines: Vec<&str> = f.lines().collect();
        assert!(lines.len() < 8, "{f}");
        assert!(lines.iter().all(|l| l.chars().count() <= WIDTH), "{f}");
        assert!(lines[0].starts_with("SELECT spalte_1, spalte_2,"));
    }

    #[test]
    fn long_where_breaks_at_and() {
        let f = format_sql(
            "select * from bestellung where datum between '2024-01-01' and '2024-12-31' and status = 'offen' and kunde_id in (1,2,3) and betrag > 1000",
        );
        assert!(f.contains("WHERE datum BETWEEN '2024-01-01' AND '2024-12-31'\n  AND status = 'offen'\n  AND kunde_id IN (1, 2, 3)"), "{f}");
    }

    #[test]
    fn subqueries_union_and_comments() {
        let f = format_sql("-- Umsatz\nselect id from a union all select id from b;\n\n\n# zweite\nselect * from t where id in (select x from y);");
        assert_eq!(f, "-- Umsatz\nSELECT id\nFROM a\nUNION ALL\nSELECT id\nFROM b;\n\n# zweite\nSELECT *\nFROM t\nWHERE id IN (SELECT x FROM y);\n");
    }

    #[test]
    fn create_table_one_column_per_line() {
        let f = format_sql("create table if not exists kunde (id int auto_increment primary key, name varchar(50) not null, ort varchar(80), erstellt datetime default current_timestamp) engine=InnoDB;");
        assert_eq!(
            f,
            "CREATE TABLE IF NOT EXISTS kunde (\n  id INT AUTO_INCREMENT PRIMARY KEY,\n  name VARCHAR(50) NOT NULL,\n  ort VARCHAR(80),\n  erstellt DATETIME DEFAULT CURRENT_TIMESTAMP\n) ENGINE = InnoDB;\n"
        );
    }

    #[test]
    fn column_names_keep_case() {
        assert_eq!(format_sql("select date, text, year(date) from t"), "SELECT date, text, YEAR(date)\nFROM t\n");
    }

    #[test]
    fn insert_update_delete() {
        assert_eq!(format_sql("insert into t (a,b) values (1,'x'),(2,'y')"), "INSERT INTO t (a, b)\nVALUES (1, 'x'), (2, 'y')\n");
        assert_eq!(format_sql("update t set a=1, b='x' where id=5"), "UPDATE t\nSET a = 1, b = 'x'\nWHERE id = 5\n");
        assert_eq!(format_sql("delete from t where id=-1"), "DELETE\nFROM t\nWHERE id = -1\n");
    }

    #[test]
    fn procedures_untouched() {
        let t = "DELIMITER //\nCREATE PROCEDURE p()\nBEGIN\n  select 1;\nEND //\nDELIMITER ;\ncall p();";
        let f = format_sql(t);
        assert!(f.contains("CREATE PROCEDURE p()\nBEGIN\n  select 1;\nEND //"), "{f}");
        assert!(f.starts_with("DELIMITER //\n"));
        assert!(f.contains("DELIMITER ;\n"), "{f}");
    }

    #[test]
    fn idempotent() {
        let t = "select k.name, (select count(*) from bestellung b where b.kunde_id = k.id) as anzahl from kunde k where k.ort like 'B%' order by 2 desc;";
        let once = format_sql(t);
        assert_eq!(format_sql(&once), once);
    }

    #[test]
    fn strings_untouched() {
        assert_eq!(format_sql("select 'a,b  from' , \"x\"\"y\" from t"), "SELECT 'a,b  from', \"x\"\"y\"\nFROM t\n");
    }
}
