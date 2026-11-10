// Sprachen ausser SQL im Editor: Java (mit SQL in Zeichenketten),
// Gradle-Skripte, XML, JSON, .properties. Hervorhebung, Kommentarzeichen, Vorschlaege und die SQL-Pruefung
// in Zeichenketten.

use super::complete::{self, Item, Suggestion};
use super::highlight::is_word_char;
use super::lint::{Env, Problem, lint};
use crate::dev::java::source::{self, Literal, Span};
use eframe::egui::{Color32, FontId, TextFormat, text::LayoutJob};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Lang {
    #[default]
    Sql,
    Java,
    /// build.gradle(.kts) und andere Kotlin/Groovy-Dateien
    Gradle,
    Xml,
    Json,
    Properties,
    Plain,
}

impl Lang {
    pub fn from_path(path: &std::path::Path) -> Lang {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "sql" => Lang::Sql,
            "java" => Lang::Java,
            "gradle" | "kts" | "kt" | "groovy" => Lang::Gradle,
            "xml" | "pom" | "html" | "htm" | "xsd" | "svg" => Lang::Xml,
            "json" => Lang::Json,
            "properties" | "ini" | "cfg" | "conf" | "env" | "toml" | "yml" | "yaml" | "gitignore" => Lang::Properties,
            _ if name == ".gitignore" => Lang::Properties,
            _ => Lang::Plain,
        }
    }

    /// Zeichen fuer Zeilenkommentare (Strg+#)
    pub fn comment(self) -> Option<&'static str> {
        match self {
            Lang::Sql => Some("--"),
            Lang::Java | Lang::Gradle => Some("//"),
            Lang::Properties => Some("#"),
            _ => None,
        }
    }

    /// Einrueckung je Ebene
    pub fn indent(self) -> &'static str {
        match self {
            Lang::Java | Lang::Gradle => "    ",
            _ => "  ",
        }
    }

    /// Code mit geschweiften Klammern (Einruecken nach `{`)
    pub fn braces(self) -> bool {
        matches!(self, Lang::Java | Lang::Gradle | Lang::Json)
    }

    /// Sprache mit Ausfuehren, Vorlagen und SQL in Zeichenketten
    pub fn runnable(self) -> bool {
        self == Lang::Java
    }

    /// Name fuer Sprachserver (LSP languageId)
    pub fn lsp_id(self) -> &'static str {
        match self {
            Lang::Java => "java",
            _ => "plaintext",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Lang::Sql => "SQL",
            Lang::Java => "Java",
            Lang::Gradle => "Gradle",
            Lang::Xml => "XML",
            Lang::Json => "JSON",
            Lang::Properties => "Properties",
            Lang::Plain => "Text",
        }
    }
}

// ---------------------------------------------------------------------------
// Hervorhebung

fn push(job: &mut LayoutJob, text: &str, font: &FontId, color: Color32) {
    if !text.is_empty() {
        job.append(text, 0.0, TextFormat::simple(font.clone(), color));
    }
}

const GRADLE_WORDS: &[&str] = &[
    "plugins", "dependencies", "repositories", "implementation", "testImplementation", "runtimeOnly", "compileOnly", "api", "application", "tasks", "java",
    "val", "var", "fun", "def", "it", "id", "version", "apply", "mavenCentral", "google", "register", "named", "withType", "configureEach",
];


/// Bereiche (Code, Kommentar, Zeichenkette) und Zeichenketten einer Sprache
pub fn spans_and_literals(chars: &[char], lang: Lang) -> (Vec<(Span, usize, usize)>, Vec<Literal>) {
    let _ = lang;
    (source::spans(chars), source::literals(chars))
}

/// Farbe eines Wortes im Code
fn word_color(word: &str, lang: Lang, call: bool, pal: &crate::style::Pal) -> Color32 {
    let keyword = match lang {
        Lang::Gradle => GRADLE_WORDS.contains(&word) || source::KEYWORDS.binary_search(&word).is_ok(),
        _ => source::KEYWORDS.binary_search(&word).is_ok() || matches!(word, "var" | "record" | "yield" | "sealed" | "permits"),
    };
    let constant = matches!(word, "true" | "false" | "null");
    if constant {
        pal.syn_const
    } else if keyword {
        pal.syn_keyword
    } else if word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        pal.syn_const
    } else if call {
        pal.syn_function
    } else if word.starts_with(|c: char| c.is_uppercase()) {
        pal.syn_type
    } else {
        pal.syn_text
    }
}

/// Java und Gradle farbig. Zeichenketten, die nach SQL aussehen, werden als SQL hervorgehoben.
pub fn highlight_code(text: &str, font: FontId, lang: Lang) -> LayoutJob {
    let pal = crate::style::pal();
    let mut job = LayoutJob::default();
    let chars: Vec<char> = text.chars().collect();
    let byte_at: Vec<usize> = text.char_indices().map(|(b, _)| b).chain(std::iter::once(text.len())).collect();
    let slice = |a: usize, b: usize| &text[byte_at[a]..byte_at[b]];
    let (spans, lits) = spans_and_literals(&chars, lang);
    for (kind, a, b) in spans {
        match kind {
            Span::Comment => push(&mut job, slice(a, b), &font, pal.syn_comment),
            Span::Char => push(&mut job, slice(a, b), &font, pal.syn_string),
            Span::Str => {
                let lit = lits.iter().find(|l| l.content.0 > a && l.content.1 <= b);
                let (ca, cb) = lit.map(|l| l.content).unwrap_or((b, b));
                let content = slice(ca.min(cb), cb);
                if lit.is_some() && source::looks_like_sql(content) {
                    push(&mut job, slice(a, ca.min(cb)), &font, pal.syn_string);
                    let sub = super::highlight(content, font.clone());
                    for s in &sub.sections {
                        push(&mut job, &sub.text[s.byte_range.start.0..s.byte_range.end.0], &font, s.format.color);
                    }
                    push(&mut job, slice(cb, b), &font, pal.syn_string);
                } else {
                    push(&mut job, slice(a, b), &font, pal.syn_string);
                }
            }
            Span::Code => {
                let mut i = a;
                while i < b {
                    let c = chars[i];
                    let start = i;
                    if c == '@' && i + 1 < b && chars[i + 1].is_alphabetic() {
                        i += 1;
                        while i < b && (is_word_char(chars[i]) || chars[i] == '.') {
                            i += 1;
                        }
                        push(&mut job, slice(start, i), &font, pal.syn_variable);
                    } else if c.is_ascii_digit() {
                        while i < b && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                            i += 1;
                        }
                        push(&mut job, slice(start, i), &font, pal.syn_number);
                    } else if is_word_char(c) {
                        while i < b && is_word_char(chars[i]) {
                            i += 1;
                        }
                        let word = slice(start, i);
                        let mut j = i;
                        while j < b && chars[j] == ' ' {
                            j += 1;
                        }
                        let call = j < b && chars[j] == '(';
                        push(&mut job, word, &font, word_color(word, lang, call, pal));
                    } else {
                        while i < b && !is_word_char(chars[i]) && !chars[i].is_ascii_digit() && chars[i] != '@' {
                            i += 1;
                        }
                        if i == start {
                            i += 1;
                        }
                        push(&mut job, slice(start, i), &font, pal.syn_op);
                    }
                }
            }
        }
    }
    job
}

/// XML farbig: Tags, Attribute, Werte, Kommentare
pub fn highlight_xml(text: &str, font: FontId) -> LayoutJob {
    let pal = crate::style::pal();
    let mut job = LayoutJob::default();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("<!--") {
            let end = r.find("-->").map(|e| e + 7).unwrap_or(rest.len());
            push(&mut job, &rest[..end], &font, pal.syn_comment);
            rest = &rest[end..];
        } else if rest.starts_with('<') {
            let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            let tag = &rest[..end];
            // <name attr="wert">
            let name_end = tag.find(|c: char| c.is_whitespace() || c == '>' || (c == '/' && tag.len() > 2)).filter(|&i| i > 1).unwrap_or(tag.len());
            push(&mut job, &tag[..name_end], &font, pal.syn_keyword);
            let mut t = &tag[name_end..];
            while !t.is_empty() {
                if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let e = t[1..].find(q).map(|e| e + 2).unwrap_or(t.len());
                    push(&mut job, &t[..e], &font, pal.syn_string);
                    t = &t[e..];
                } else {
                    let e = t.find(['"', '\'']).unwrap_or(t.len());
                    let part = &t[..e];
                    // Attributnamen vor "=", Rest (>, /) als Zeichen
                    let mut p = part;
                    while !p.is_empty() {
                        let w = p.find(|c: char| !(c.is_alphanumeric() || c == ':' || c == '-' || c == '_' || c == '.')).unwrap_or(p.len());
                        if w > 0 {
                            push(&mut job, &p[..w], &font, pal.syn_ident);
                            p = &p[w..];
                        } else {
                            let ch = p.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                            push(&mut job, &p[..ch], &font, pal.syn_op);
                            p = &p[ch..];
                        }
                    }
                    t = &t[e..];
                }
            }
            rest = &rest[end..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            push(&mut job, &rest[..end], &font, pal.syn_text);
            rest = &rest[end..];
        }
    }
    job
}

/// .properties/.yml/.ini: Kommentare, Schluessel, Werte
pub fn highlight_properties(text: &str, font: FontId) -> LayoutJob {
    let pal = crate::style::pal();
    let mut job = LayoutJob::default();
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with('#') || t.starts_with('!') || t.starts_with(';') {
            push(&mut job, line, &font, pal.syn_comment);
        } else if t.starts_with('[') {
            push(&mut job, line, &font, pal.syn_keyword);
        } else if let Some(i) = line.find(['=', ':']) {
            push(&mut job, &line[..i], &font, pal.syn_ident);
            push(&mut job, &line[i..i + 1], &font, pal.syn_op);
            push(&mut job, &line[i + 1..], &font, pal.syn_string);
        } else {
            push(&mut job, line, &font, pal.syn_text);
        }
    }
    job
}

/// JSON: Schluessel, Zeichenketten, Zahlen, true/false/null
pub fn highlight_json(text: &str, font: FontId) -> LayoutJob {
    let pal = crate::style::pal();
    let mut job = LayoutJob::default();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let byte = |i: usize| if i < n { chars[i].0 } else { text.len() };
    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        let start = i;
        if c == '"' {
            i += 1;
            while i < n && chars[i].1 != '"' {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            // Schluessel: danach folgt ein Doppelpunkt
            let mut j = i;
            while j < n && chars[j].1.is_whitespace() {
                j += 1;
            }
            let key = j < n && chars[j].1 == ':';
            push(&mut job, &text[byte(start)..byte(i)], &font, if key { pal.syn_ident } else { pal.syn_string });
        } else if c == '-' || c.is_ascii_digit() {
            i += 1;
            while i < n && (chars[i].1.is_ascii_digit() || matches!(chars[i].1, '.' | 'e' | 'E' | '+' | '-')) {
                i += 1;
            }
            push(&mut job, &text[byte(start)..byte(i)], &font, pal.syn_number);
        } else if c.is_alphabetic() {
            while i < n && chars[i].1.is_alphabetic() {
                i += 1;
            }
            push(&mut job, &text[byte(start)..byte(i)], &font, pal.syn_const);
        } else {
            i += 1;
            push(&mut job, &text[byte(start)..byte(i)], &font, if c.is_whitespace() { pal.syn_text } else { pal.syn_op });
        }
    }
    job
}

pub fn highlight_lang(text: &str, font: FontId, lang: Lang) -> LayoutJob {
    match lang {
        Lang::Sql => super::highlight(text, font),
        Lang::Java | Lang::Gradle => highlight_code(text, font, lang),
        Lang::Xml => highlight_xml(text, font),
        Lang::Json => highlight_json(text, font),
        Lang::Properties => highlight_properties(text, font),
        Lang::Plain => {
            let mut job = LayoutJob::default();
            push(&mut job, text, &font, crate::style::pal().syn_text);
            job
        }
    }
}

// ---------------------------------------------------------------------------
// SQL in Zeichenketten (Java)

/// Inhalt einer Zeichenkette fuer die SQL-Pruefung: Escapes (\n, \", …) werden zu gleich langen, harmlosen
/// Zeichen, damit die Positionen stimmen.
fn sql_view(content: &[char]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < content.len() {
        if content[i] == '\\' && i + 1 < content.len() {
            let next = content[i + 1];
            out.push(' ');
            out.push(match next {
                '"' => '"',
                '\'' => '\'',
                _ => ' ',
            });
            i += 2;
        } else {
            out.push(content[i]);
            i += 1;
        }
    }
    out
}

/// SQL-Zeichenketten, die vollstaendig sind (nicht zusammengesetzt) und geprueft werden koennen
fn checkable(chars: &[char], lang: Lang) -> Vec<Literal> {
    spans_and_literals(chars, lang)
        .1
        .into_iter()
        .filter(|l| !l.joined && !l.open)
        .filter(|l| source::looks_like_sql(&chars[l.content.0..l.content.1].iter().collect::<String>()))
        .collect()
}

/// Probleme im SQL der Zeichenketten (Positionen im ganzen Text)
pub fn code_sql_problems(text: &str, env: &Env, lang: Lang) -> Vec<Problem> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for l in checkable(&chars, lang) {
        let sql = sql_view(&chars[l.content.0..l.content.1]);
        let off = l.content.0;
        for mut p in lint(&sql, env) {
            // Ein fehlendes Semikolon am Ende ist in Programmcode normal
            if p.message.contains(';') && p.start >= sql.trim_end().chars().count().saturating_sub(1) {
                continue;
            }
            // Korrekturen mit Zeilenumbruch passen nicht in eine normale Zeichenkette
            if !l.block && p.fix.as_ref().is_some_and(|f| f.text.contains('\n')) {
                p.fix = None;
            }
            p.start += off;
            p.end += off;
            if let Some(f) = p.fix.as_mut() {
                f.start += off;
                f.end += off;
            }
            out.push(p);
        }
    }
    out
}

/// Probleme im SQL der Java-Zeichenketten
#[cfg(test)]
pub fn java_sql_problems(text: &str, env: &Env) -> Vec<Problem> {
    code_sql_problems(text, env, Lang::Java)
}

// ---------------------------------------------------------------------------
// Vorschlaege in Java

/// Vorlage: \u{1} markiert die Cursorstelle
fn snippet(label: &str, detail: &str, text: &str) -> Item {
    let back = text.chars().skip_while(|c| *c != '\u{1}').count().saturating_sub(1);
    Item { label: label.into(), insert: text.replace('\u{1}', ""), kind: crate::i18n::tr("Vorlage", "Snippet"), detail: detail.into(), back }
}

const TYPES: &[&str] = &[
    "String", "Integer", "Long", "Double", "Boolean", "Character", "Object", "StringBuilder", "Math", "System", "Scanner", "List", "ArrayList", "Map", "HashMap",
    "Set", "HashSet", "Optional", "Arrays", "Collections", "Objects", "LocalDate", "LocalDateTime", "LocalTime", "BigDecimal", "BigInteger", "Exception",
    "RuntimeException", "IllegalArgumentException", "IllegalStateException", "Connection", "DriverManager", "PreparedStatement", "Statement", "ResultSet",
    "ResultSetMetaData", "DatabaseMetaData", "SQLException", "Timestamp", "Types",
];

/// Methoden haeufiger JDBC- und Java-Typen (Name, Einfuegen mit Cursor, Rueckgabe)
fn members(ty: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match ty {
        "Connection" => &[
            ("prepareStatement(sql)", "prepareStatement(\u{1})", "PreparedStatement"),
            ("createStatement()", "createStatement()\u{1}", "Statement"),
            ("setAutoCommit(false)", "setAutoCommit(\u{1}false)", "void"),
            ("commit()", "commit()\u{1}", "void"),
            ("rollback()", "rollback()\u{1}", "void"),
            ("getMetaData()", "getMetaData()\u{1}", "DatabaseMetaData"),
            ("getCatalog()", "getCatalog()\u{1}", "String"),
            ("close()", "close()\u{1}", "void"),
        ],
        "PreparedStatement" | "Statement" => &[
            ("executeQuery()", "executeQuery()\u{1}", "ResultSet"),
            ("executeUpdate()", "executeUpdate()\u{1}", "int"),
            ("execute()", "execute()\u{1}", "boolean"),
            ("setString(i, value)", "setString(\u{1})", "void"),
            ("setInt(i, value)", "setInt(\u{1})", "void"),
            ("setLong(i, value)", "setLong(\u{1})", "void"),
            ("setDouble(i, value)", "setDouble(\u{1})", "void"),
            ("setBigDecimal(i, value)", "setBigDecimal(\u{1})", "void"),
            ("setObject(i, value)", "setObject(\u{1})", "void"),
            ("setNull(i, Types.…)", "setNull(\u{1})", "void"),
            ("getGeneratedKeys()", "getGeneratedKeys()\u{1}", "ResultSet"),
            ("addBatch()", "addBatch()\u{1}", "void"),
            ("executeBatch()", "executeBatch()\u{1}", "int[]"),
        ],
        "ResultSet" => &[
            ("next()", "next()\u{1}", "boolean"),
            ("getString(column)", "getString(\"\u{1}\")", "String"),
            ("getInt(column)", "getInt(\"\u{1}\")", "int"),
            ("getLong(column)", "getLong(\"\u{1}\")", "long"),
            ("getDouble(column)", "getDouble(\"\u{1}\")", "double"),
            ("getBigDecimal(column)", "getBigDecimal(\"\u{1}\")", "BigDecimal"),
            ("getBoolean(column)", "getBoolean(\"\u{1}\")", "boolean"),
            ("getDate(column)", "getDate(\"\u{1}\")", "Date"),
            ("getObject(column, Class)", "getObject(\"\u{1}\", .class)", "T"),
            ("wasNull()", "wasNull()\u{1}", "boolean"),
            ("getMetaData()", "getMetaData()\u{1}", "ResultSetMetaData"),
        ],
        "String" => &[
            ("length()", "length()\u{1}", "int"),
            ("isBlank()", "isBlank()\u{1}", "boolean"),
            ("equals(other)", "equals(\u{1})", "boolean"),
            ("equalsIgnoreCase(other)", "equalsIgnoreCase(\u{1})", "boolean"),
            ("trim()", "trim()\u{1}", "String"),
            ("toUpperCase()", "toUpperCase()\u{1}", "String"),
            ("toLowerCase()", "toLowerCase()\u{1}", "String"),
            ("contains(text)", "contains(\u{1})", "boolean"),
            ("split(regex)", "split(\u{1})", "String[]"),
            ("formatted(args)", "formatted(\u{1})", "String"),
        ],
        "Scanner" => &[
            ("nextLine()", "nextLine()\u{1}", "String"),
            ("nextInt()", "nextInt()\u{1}", "int"),
            ("nextDouble()", "nextDouble()\u{1}", "double"),
            ("hasNextLine()", "hasNextLine()\u{1}", "boolean"),
        ],
        "List" | "ArrayList" => &[
            ("add(element)", "add(\u{1})", "boolean"),
            ("get(index)", "get(\u{1})", "E"),
            ("size()", "size()\u{1}", "int"),
            ("isEmpty()", "isEmpty()\u{1}", "boolean"),
            ("remove(index)", "remove(\u{1})", "E"),
            ("forEach(…)", "forEach(\u{1})", "void"),
            ("stream()", "stream()\u{1}", "Stream"),
        ],
        "System.out" => &[("println(x)", "println(\u{1})", "void"), ("print(x)", "print(\u{1})", "void"), ("printf(format, args)", "printf(\"\u{1}\")", "PrintStream")],
        "System" => &[("out", "out\u{1}", "PrintStream"), ("in", "in\u{1}", "InputStream"), ("getenv(name)", "getenv(\"\u{1}\")", "String"), ("currentTimeMillis()", "currentTimeMillis()\u{1}", "long")],
        "DriverManager" => &[("getConnection(url, user, password)", "getConnection(\u{1})", "Connection")],
        _ => &[],
    }
}

/// Typ einer Variablen aus ihrer Deklaration davor (`ResultSet rs = …`, `try (Connection con = …)`)
fn declared_type(code: &str, var: &str) -> Option<String> {
    if var.starts_with(|c: char| c.is_uppercase()) {
        return Some(var.to_string());
    }
    let words: Vec<&str> = code.split(|c: char| !(is_word_char(c) || c == '<' || c == '>')).filter(|w| !w.is_empty()).collect();
    words.windows(2).rev().find(|w| w[1] == var && w[0].starts_with(|c: char| c.is_uppercase())).map(|w| w[0].split('<').next().unwrap_or(w[0]).to_string())
}

/// Vorschlaege an der Stelle `cursor`. In SQL-Zeichenketten: SQL mit Tabellen und Spalten der Datenbank.
pub fn suggest_java(text: &str, cursor: usize, env: &Env, force: bool, has_database_class: bool) -> Option<Suggestion> {
    let chars: Vec<char> = text.chars().collect();
    if cursor > chars.len() {
        return None;
    }
    // In einem Kommentar: nichts
    let spans = source::spans(&chars);
    if let Some(&(kind, _, _)) = spans.iter().find(|(_, a, b)| cursor > *a && cursor <= *b) {
        match kind {
            Span::Comment | Span::Char => return None,
            Span::Str => {
                let lit = source::literal_at(&chars, cursor)?;
                let content: String = chars[lit.content.0..lit.content.1].iter().collect();
                // Leere Zeichenkette: nur auf Wunsch (Strg+Leertaste) SQL anbieten
                if !(source::looks_like_sql(&content) || force && content.trim().is_empty()) || cursor < lit.content.0 {
                    return None;
                }
                let sql = sql_view(&chars[lit.content.0..lit.content.1]);
                let mut s = complete::suggest(&sql, cursor - lit.content.0, env, force)?;
                s.word_start += lit.content.0;
                if !lit.block {
                    s.items.retain(|i| !i.insert.contains('\n'));
                }
                return (!s.items.is_empty()).then_some(s);
            }
            Span::Code => {}
        }
    }
    let mut s = cursor;
    while s > 0 && is_word_char(chars[s - 1]) {
        s -= 1;
    }
    let word: String = chars[s..cursor].iter().collect();
    let lower = word.to_lowercase();
    let matches = |label: &str| lower.is_empty() || label.to_lowercase().starts_with(&lower);
    let mut items: Vec<Item> = Vec::new();

    // Nach "variable." die Methoden ihres Typs
    if s > 0 && chars[s - 1] == '.' {
        let mut q = s - 1;
        while q > 0 && (is_word_char(chars[q - 1]) || chars[q - 1] == '.') {
            q -= 1;
        }
        let qualifier: String = chars[q..s - 1].iter().collect();
        let before: String = chars[..q].iter().collect();
        let ty = if qualifier == "System.out" || qualifier == "System.err" { Some("System.out".to_string()) } else { declared_type(&before, &qualifier) };
        let ty = ty?;
        for (label, insert, ret) in members(&ty) {
            if matches(label) {
                let back = insert.chars().skip_while(|c| *c != '\u{1}').count().saturating_sub(1);
                items.push(Item { label: (*label).into(), insert: insert.replace('\u{1}', ""), kind: crate::i18n::tr("Methode", "Method"), detail: (*ret).into(), back });
            }
        }
        return (!items.is_empty()).then_some(Suggestion { items, word_start: s });
    }
    if word.is_empty() && !force {
        return None;
    }
    if !force && word.chars().count() < 2 {
        return None;
    }

    // Vorlagen
    let connect = if has_database_class { "Database.connect()" } else { "DriverManager.getConnection(\"jdbc:mariadb://localhost:3306/\", \"root\", \"\")" };
    let tpl: Vec<Item> = vec![
        snippet("main", "public static void main", "public static void main(String[] args) {\n    \u{1}\n}"),
        snippet("sout", "System.out.println", "System.out.println(\u{1});"),
        snippet("fori", "for (int i …)", "for (int i = 0; i < \u{1}; i++) {\n    \n}"),
        snippet("foreach", "for (… : …)", "for (var \u{1} : ) {\n    \n}"),
        snippet("try", "try … catch", "try {\n    \u{1}\n} catch (Exception e) {\n    e.printStackTrace();\n}"),
        snippet("scanner", "Scanner", "Scanner scanner = new Scanner(System.in);\u{1}"),
        snippet("connection", crate::i18n::tr("Verbindung öffnen", "Open a connection"), &format!("try (Connection con = {connect}) {{\n    \u{1}\n}}")),
        snippet(
            "query",
            crate::i18n::tr("SELECT und Zeilen lesen", "SELECT and read rows"),
            "try (PreparedStatement ps = con.prepareStatement(\"SELECT \u{1}\");\n     ResultSet rs = ps.executeQuery()) {\n    while (rs.next()) {\n        \n    }\n}",
        ),
        snippet("update", crate::i18n::tr("INSERT/UPDATE/DELETE mit Werten", "INSERT/UPDATE/DELETE with values"), "try (PreparedStatement ps = con.prepareStatement(\"UPDATE \u{1}\")) {\n    ps.setObject(1, );\n    int rows = ps.executeUpdate();\n}"),
        snippet("transaction", crate::i18n::tr("Mehrere Änderungen gemeinsam", "Several changes together"), "con.setAutoCommit(false);\ntry {\n    \u{1}\n    con.commit();\n} catch (SQLException e) {\n    con.rollback();\n    throw e;\n}"),
    ];
    items.extend(tpl.into_iter().filter(|i| matches(&i.label)));
    for k in source::KEYWORDS.iter().chain(["var", "record"].iter()) {
        if matches(k) && *k != word {
            items.push(Item { label: (*k).into(), insert: (*k).into(), kind: crate::i18n::tr("Schlüsselwort", "Keyword"), detail: String::new(), back: 0 });
        }
    }
    for t in TYPES {
        if matches(t) && *t != word {
            items.push(Item { label: (*t).into(), insert: (*t).into(), kind: crate::i18n::tr("Typ", "Type"), detail: String::new(), back: 0 });
        }
    }
    // Namen aus der Datei
    let mut seen: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    for (kind, a, b) in &spans {
        if *kind != Span::Code {
            continue;
        }
        let code: String = chars[*a..*b].iter().collect();
        for w in code.split(|c: char| !is_word_char(c)) {
            if w.chars().count() >= 3 && w != word && !w.starts_with(|c: char| c.is_ascii_digit()) && matches(w) && !seen.iter().any(|x| x == w) {
                seen.push(w.to_string());
                items.push(Item { label: w.into(), insert: w.into(), kind: crate::i18n::tr("Name", "Name"), detail: String::new(), back: 0 });
            }
        }
    }
    items.truncate(60);
    (!items.is_empty()).then_some(Suggestion { items, word_start: s })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, Schema, TableInfo};

    fn schema() -> Schema {
        Schema {
            name: "shop".into(),
            tables: vec![TableInfo {
                name: "kunde".into(),
                columns: vec![ColumnInfo { name: "id".into(), col_type: "int".into(), ..Default::default() }, ColumnInfo { name: "name".into(), col_type: "varchar(50)".into(), ..Default::default() }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn languages_by_file_name() {
        use std::path::Path;
        assert_eq!(Lang::from_path(Path::new("A.java")), Lang::Java);
        assert_eq!(Lang::from_path(Path::new("build.gradle.kts")), Lang::Gradle);
        assert_eq!(Lang::from_path(Path::new("pom.xml")), Lang::Xml);
        assert_eq!(Lang::from_path(Path::new("db.properties")), Lang::Properties);
        assert_eq!(Lang::from_path(Path::new("x.sql")), Lang::Sql);
        assert_eq!(Lang::from_path(Path::new("notiz.md")), Lang::Plain);
        assert_eq!(Lang::from_path(Path::new("app.py")), Lang::Plain);
    }

    #[test]
    fn highlighting_keeps_the_whole_text() {
        let f = FontId::monospace(13.0);
        for (t, l) in [
            ("@Override\npublic String s = \"\"\"\n  SELECT * FROM t\n  \"\"\"; // x\nchar c = 'a'; int n = 1_000;", Lang::Java),
            ("<a x=\"1\"><!-- k --><b/>text</a>", Lang::Xml),
            ("# k\nurl=jdbc:x\n", Lang::Properties),
            ("{\"a\": [1, true, \"x\"]}", Lang::Json),
            ("q = \"SELECT * FR", Lang::Java),
        ] {
            assert_eq!(highlight_lang(t, f.clone(), l).text, t, "{l:?}");
        }
    }

    #[test]
    fn sql_inside_java_strings_is_checked_against_the_database() {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let src = "String a = \"SELECT nmae FROM kunde WHERE id = ?\";\nString b = \"SELECT * FROM \" + tabelle;\nString c = \"Select a file\";";
        let probs = java_sql_problems(src, &env);
        assert_eq!(probs.len(), 1, "{probs:?}");
        let chars: Vec<char> = src.chars().collect();
        assert_eq!(chars[probs[0].start..probs[0].end].iter().collect::<String>(), "nmae");
        assert_eq!(probs[0].fix.as_ref().map(|f| f.text.as_str()), Some("name"));
        // Zeilenumbrueche als Escape stoeren nicht
        assert!(java_sql_problems("q = \"SELECT id\\nFROM kunde\";", &env).is_empty());
    }

    #[test]
    fn suggestions_in_sql_strings_and_code() {
        let s = schema();
        let env = Env { schema: Some(&s), databases: &[] };
        let src = "String q = \"SELECT * FROM ku";
        let sg = suggest_java(src, src.chars().count(), &env, false, false).unwrap();
        assert!(sg.items.iter().any(|i| i.insert == "kunde"), "{:?}", sg.items);
        assert_eq!(sg.word_start, src.chars().count() - 2);
        let src = "ResultSet rs = ps.executeQuery();\nrs.";
        let sg = suggest_java(src, src.chars().count(), &env, false, false).unwrap();
        assert!(sg.items.iter().any(|i| i.label == "next()"));
        let src = "void m() {\n  sou";
        let sg = suggest_java(src, src.chars().count(), &env, false, false).unwrap();
        assert_eq!(sg.items[0].insert, "System.out.println();");
        assert_eq!(sg.items[0].back, 2);
        assert!(suggest_java("// sou", 6, &env, true, false).is_none());
    }
}
