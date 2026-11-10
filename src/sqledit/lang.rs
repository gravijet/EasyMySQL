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
        "Map" | "HashMap" | "TreeMap" | "LinkedHashMap" => &[
            ("put(key, value)", "put(\u{1})", "V"),
            ("get(key)", "get(\u{1})", "V"),
            ("getOrDefault(key, default)", "getOrDefault(\u{1})", "V"),
            ("containsKey(key)", "containsKey(\u{1})", "boolean"),
            ("remove(key)", "remove(\u{1})", "V"),
            ("keySet()", "keySet()\u{1}", "Set<K>"),
            ("values()", "values()\u{1}", "Collection<V>"),
            ("entrySet()", "entrySet()\u{1}", "Set<Map.Entry<K, V>>"),
            ("size()", "size()\u{1}", "int"),
            ("isEmpty()", "isEmpty()\u{1}", "boolean"),
            ("forEach(…)", "forEach((k, v) -> \u{1})", "void"),
            ("merge(key, value, fn)", "merge(\u{1})", "V"),
            ("putIfAbsent(key, value)", "putIfAbsent(\u{1})", "V"),
            ("clear()", "clear()\u{1}", "void"),
        ],
        "Set" | "HashSet" | "TreeSet" => &[
            ("add(element)", "add(\u{1})", "boolean"),
            ("contains(element)", "contains(\u{1})", "boolean"),
            ("remove(element)", "remove(\u{1})", "boolean"),
            ("size()", "size()\u{1}", "int"),
            ("isEmpty()", "isEmpty()\u{1}", "boolean"),
            ("stream()", "stream()\u{1}", "Stream<E>"),
            ("clear()", "clear()\u{1}", "void"),
        ],
        "StringBuilder" => &[
            ("append(x)", "append(\u{1})", "StringBuilder"),
            ("insert(i, x)", "insert(\u{1})", "StringBuilder"),
            ("reverse()", "reverse()\u{1}", "StringBuilder"),
            ("length()", "length()\u{1}", "int"),
            ("setLength(n)", "setLength(\u{1})", "void"),
            ("deleteCharAt(i)", "deleteCharAt(\u{1})", "StringBuilder"),
            ("toString()", "toString()\u{1}", "String"),
        ],
        "Optional" => &[
            ("isPresent()", "isPresent()\u{1}", "boolean"),
            ("isEmpty()", "isEmpty()\u{1}", "boolean"),
            ("get()", "get()\u{1}", "T"),
            ("orElse(other)", "orElse(\u{1})", "T"),
            ("orElseThrow()", "orElseThrow()\u{1}", "T"),
            ("ifPresent(…)", "ifPresent(x -> \u{1})", "void"),
            ("map(…)", "map(x -> \u{1})", "Optional<U>"),
        ],
        "Stream" => &[
            ("filter(…)", "filter(x -> \u{1})", "Stream<T>"),
            ("map(…)", "map(x -> \u{1})", "Stream<R>"),
            ("forEach(…)", "forEach(x -> \u{1})", "void"),
            ("toList()", "toList()\u{1}", "List<T>"),
            ("sorted()", "sorted()\u{1}", "Stream<T>"),
            ("count()", "count()\u{1}", "long"),
            ("anyMatch(…)", "anyMatch(x -> \u{1})", "boolean"),
            ("findFirst()", "findFirst()\u{1}", "Optional<T>"),
            ("collect(…)", "collect(\u{1})", "R"),
            ("mapToInt(…)", "mapToInt(x -> \u{1})", "IntStream"),
            ("limit(n)", "limit(\u{1})", "Stream<T>"),
            ("distinct()", "distinct()\u{1}", "Stream<T>"),
        ],
        "LocalDate" | "LocalDateTime" => &[
            ("getYear()", "getYear()\u{1}", "int"),
            ("getMonthValue()", "getMonthValue()\u{1}", "int"),
            ("getDayOfMonth()", "getDayOfMonth()\u{1}", "int"),
            ("plusDays(n)", "plusDays(\u{1})", "LocalDate"),
            ("minusDays(n)", "minusDays(\u{1})", "LocalDate"),
            ("isBefore(other)", "isBefore(\u{1})", "boolean"),
            ("isAfter(other)", "isAfter(\u{1})", "boolean"),
            ("format(formatter)", "format(\u{1})", "String"),
        ],
        // Statische Methoden: nach dem Klassennamen
        "Math" => &[
            ("abs(x)", "abs(\u{1})", "double"),
            ("max(a, b)", "max(\u{1})", "double"),
            ("min(a, b)", "min(\u{1})", "double"),
            ("pow(a, b)", "pow(\u{1})", "double"),
            ("sqrt(x)", "sqrt(\u{1})", "double"),
            ("cbrt(x)", "cbrt(\u{1})", "double"),
            ("round(x)", "round(\u{1})", "long"),
            ("floor(x)", "floor(\u{1})", "double"),
            ("ceil(x)", "ceil(\u{1})", "double"),
            ("random()", "random()\u{1}", "double"),
            ("floorDiv(a, b)", "floorDiv(\u{1})", "int"),
            ("floorMod(a, b)", "floorMod(\u{1})", "int"),
            ("hypot(x, y)", "hypot(\u{1})", "double"),
            ("sin(a)", "sin(\u{1})", "double"),
            ("cos(a)", "cos(\u{1})", "double"),
            ("tan(a)", "tan(\u{1})", "double"),
            ("log(x)", "log(\u{1})", "double"),
            ("log10(x)", "log10(\u{1})", "double"),
            ("exp(x)", "exp(\u{1})", "double"),
            ("signum(x)", "signum(\u{1})", "double"),
            ("toRadians(deg)", "toRadians(\u{1})", "double"),
            ("toDegrees(rad)", "toDegrees(\u{1})", "double"),
            ("clamp(v, min, max)", "clamp(\u{1})", "int"),
            ("PI", "PI\u{1}", "double"),
            ("E", "E\u{1}", "double"),
        ],
        "Integer" | "Long" | "Short" | "Byte" => &[
            ("parseInt(text)", "parseInt(\u{1})", "int"),
            ("valueOf(x)", "valueOf(\u{1})", "Integer"),
            ("toString(x)", "toString(\u{1})", "String"),
            ("compare(a, b)", "compare(\u{1})", "int"),
            ("max(a, b)", "max(\u{1})", "int"),
            ("min(a, b)", "min(\u{1})", "int"),
            ("sum(a, b)", "sum(\u{1})", "int"),
            ("toBinaryString(x)", "toBinaryString(\u{1})", "String"),
            ("toHexString(x)", "toHexString(\u{1})", "String"),
            ("MAX_VALUE", "MAX_VALUE\u{1}", "int"),
            ("MIN_VALUE", "MIN_VALUE\u{1}", "int"),
        ],
        "Double" | "Float" => &[
            ("parseDouble(text)", "parseDouble(\u{1})", "double"),
            ("valueOf(x)", "valueOf(\u{1})", "Double"),
            ("compare(a, b)", "compare(\u{1})", "int"),
            ("isNaN(x)", "isNaN(\u{1})", "boolean"),
            ("toString(x)", "toString(\u{1})", "String"),
            ("MAX_VALUE", "MAX_VALUE\u{1}", "double"),
            ("MIN_VALUE", "MIN_VALUE\u{1}", "double"),
        ],
        "Boolean" => &[("parseBoolean(text)", "parseBoolean(\u{1})", "boolean"), ("valueOf(x)", "valueOf(\u{1})", "Boolean"), ("TRUE", "TRUE\u{1}", "Boolean"), ("FALSE", "FALSE\u{1}", "Boolean")],
        "Character" => &[("isDigit(c)", "isDigit(\u{1})", "boolean"), ("isLetter(c)", "isLetter(\u{1})", "boolean"), ("isUpperCase(c)", "isUpperCase(\u{1})", "boolean"), ("toUpperCase(c)", "toUpperCase(\u{1})", "char"), ("isWhitespace(c)", "isWhitespace(\u{1})", "boolean")],
        "String." => &[("valueOf(x)", "valueOf(\u{1})", "String"), ("format(format, args)", "format(\"\u{1}\")", "String"), ("join(sep, parts)", "join(\u{1})", "String")],
        "Arrays" => &[
            ("toString(array)", "toString(\u{1})", "String"),
            ("sort(array)", "sort(\u{1})", "void"),
            ("asList(…)", "asList(\u{1})", "List<T>"),
            ("stream(array)", "stream(\u{1})", "Stream<T>"),
            ("fill(array, value)", "fill(\u{1})", "void"),
            ("copyOf(array, length)", "copyOf(\u{1})", "T[]"),
            ("equals(a, b)", "equals(\u{1})", "boolean"),
            ("binarySearch(array, key)", "binarySearch(\u{1})", "int"),
        ],
        "Collections" => &[
            ("sort(list)", "sort(\u{1})", "void"),
            ("reverse(list)", "reverse(\u{1})", "void"),
            ("shuffle(list)", "shuffle(\u{1})", "void"),
            ("max(coll)", "max(\u{1})", "T"),
            ("min(coll)", "min(\u{1})", "T"),
            ("unmodifiableList(list)", "unmodifiableList(\u{1})", "List<T>"),
            ("emptyList()", "emptyList()\u{1}", "List<T>"),
        ],
        "List." => &[("of(…)", "of(\u{1})", "List<E>"), ("copyOf(coll)", "copyOf(\u{1})", "List<E>")],
        "Map." => &[("of(…)", "of(\u{1})", "Map<K, V>"), ("entry(k, v)", "entry(\u{1})", "Map.Entry<K, V>"), ("copyOf(map)", "copyOf(\u{1})", "Map<K, V>")],
        "Set." => &[("of(…)", "of(\u{1})", "Set<E>")],
        "Objects" => &[("equals(a, b)", "equals(\u{1})", "boolean"), ("hash(…)", "hash(\u{1})", "int"), ("requireNonNull(x)", "requireNonNull(\u{1})", "T"), ("isNull(x)", "isNull(\u{1})", "boolean"), ("toString(x)", "toString(\u{1})", "String")],
        "LocalDate." | "LocalDateTime." => &[("now()", "now()\u{1}", "LocalDate"), ("of(y, m, d)", "of(\u{1})", "LocalDate"), ("parse(text)", "parse(\u{1})", "LocalDate")],
        "Thread" => &[("sleep(ms)", "sleep(\u{1})", "void"), ("currentThread()", "currentThread()\u{1}", "Thread")],
        "Optional." => &[("of(x)", "of(\u{1})", "Optional<T>"), ("ofNullable(x)", "ofNullable(\u{1})", "Optional<T>"), ("empty()", "empty()\u{1}", "Optional<T>")],
        _ => &[],
    }
}

/// Statische Mitglieder einer Klasse des JDK (`Math.`, `List.`), sonst die Methoden ihrer Objekte
fn static_members(class: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    let key = format!("{class}.");
    match members(&key) {
        [] => members(class),
        m => m,
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
        let classes = crate::dev::java::outline::outline(text);
        let own = |name: &str| classes.iter().find(|c| c.name == name);
        // this. / Klassenname. / Variable. aus dieser Datei
        let (target, statics) = if qualifier == "this" {
            (crate::dev::java::outline::class_at(&classes, cursor).map(|c| c.name.clone()), false)
        } else if own(&qualifier).is_some() {
            (Some(qualifier.clone()), true)
        } else {
            let ty = crate::dev::java::outline::locals(text, q)
                .into_iter()
                .rev()
                .find(|(n, _, _)| *n == qualifier)
                .map(|(_, t, _)| t)
                .or_else(|| crate::dev::java::outline::class_at(&classes, cursor).and_then(|c| c.members.iter().find(|m| m.name == qualifier && m.kind == crate::dev::java::outline::Kind::Field)).map(|m| m.ty.clone()))
                .map(|t| t.split('<').next().unwrap_or(&t).trim_end_matches("[]").to_string());
            (ty.filter(|t| own(t).is_some()), false)
        };
        if let Some(c) = target.as_deref().and_then(own) {
            use crate::dev::java::outline::Kind;
            for m in &c.members {
                let fits = if statics { m.is_static || m.kind == Kind::Constant } else { m.kind != Kind::Constructor };
                if !fits || !matches(&m.name) {
                    continue;
                }
                let (insert, back, kind) = match m.kind {
                    Kind::Method => (format!("{}()", m.name), usize::from(!m.params.is_empty()), crate::i18n::tr("Methode", "Method")),
                    Kind::Constant => (m.name.clone(), 0, crate::i18n::tr("Konstante", "Constant")),
                    _ => (m.name.clone(), 0, crate::i18n::tr("Feld", "Field")),
                };
                let label = if m.kind == Kind::Method { format!("{}({})", m.name, m.params) } else { m.name.clone() };
                items.push(Item { label, insert, kind, detail: m.ty.clone(), back });
            }
            if statics && c.kind == "enum" {
                for (l, ins, b) in [("values()", "values()", 0), ("valueOf(name)", "valueOf()", 1)] {
                    if matches(l) {
                        items.push(Item { label: l.into(), insert: ins.into(), kind: crate::i18n::tr("Methode", "Method"), detail: format!("{}[]", c.name), back: b });
                    }
                }
            }
            return (!items.is_empty()).then_some(Suggestion { items, word_start: s });
        }
        let ty = if qualifier == "System.out" || qualifier == "System.err" {
            Some("System.out".to_string())
        } else if qualifier.starts_with(|c: char| c.is_uppercase()) && !static_members(&qualifier).is_empty() {
            // Math., Integer., List. …: statische Mitglieder
            Some(format!("{qualifier}."))
        } else {
            crate::dev::java::outline::locals(text, q).into_iter().rev().find(|(n, _, _)| *n == qualifier).map(|(_, t, _)| t.split('<').next().unwrap_or(&t).to_string()).or_else(|| declared_type(&before, &qualifier))
        };
        let ty = ty?;
        let list = if let Some(class) = ty.strip_suffix('.') { static_members(class) } else { members(&ty) };
        for (label, insert, ret) in list {
            if matches(label) {
                let back = insert.chars().skip_while(|c| *c != '\u{1}').count().saturating_sub(1);
                let kind = if label.contains('(') { crate::i18n::tr("Methode", "Method") } else { crate::i18n::tr("Konstante", "Constant") };
                items.push(Item { label: (*label).into(), insert: insert.replace('\u{1}', ""), kind, detail: (*ret).into(), back });
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
    // Variablen, Felder, Methoden und Klassen dieser Datei mit Art und Typ
    let mut seen: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    {
        use crate::dev::java::outline::{self, Kind};
        let classes = outline::outline(text);
        let mut own: Vec<Item> = Vec::new();
        for (n, t, _) in outline::locals(text, s).into_iter().rev() {
            if n != word && matches(&n) && !own.iter().any(|i| i.insert == n) {
                own.push(Item { label: n.clone(), insert: n, kind: crate::i18n::tr("Variable", "Variable"), detail: t, back: 0 });
            }
        }
        if let Some(c) = outline::class_at(&classes, cursor) {
            for m in &c.members {
                if m.name == word || !matches(&m.name) || m.kind == Kind::Constructor {
                    continue;
                }
                let (label, insert, back, kind) = match m.kind {
                    Kind::Method => (format!("{}({})", m.name, m.params), format!("{}()", m.name), usize::from(!m.params.is_empty()), crate::i18n::tr("Methode", "Method")),
                    _ => (m.name.clone(), m.name.clone(), 0, crate::i18n::tr("Feld", "Field")),
                };
                if !own.iter().any(|i| i.insert == insert) {
                    own.push(Item { label, insert, kind, detail: m.ty.clone(), back });
                }
            }
        }
        for c in &classes {
            if c.name != word && matches(&c.name) {
                own.push(Item { label: c.name.clone(), insert: c.name.clone(), kind: crate::i18n::tr("Klasse", "Class"), detail: c.kind.clone(), back: 0 });
            }
        }
        seen.extend(own.iter().map(|i| i.insert.clone()));
        // Genau passende Anfaenge zuerst, sonst Reihenfolge: Variablen, Mitglieder, Klassen
        own.sort_by_key(|i| !i.label.starts_with(&word));
        items.splice(0..0, own);
    }
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

    #[test]
    fn dot_completion_for_jdk_and_own_classes() {
        let env = Env { schema: None, databases: &[] };
        let labels = |src: &str| suggest_java(src, src.chars().count(), &env, false, false).map(|s| s.items.into_iter().map(|i| i.label).collect::<Vec<_>>()).unwrap_or_default();
        let head = "import java.util.*;\nclass A {\n    enum Farbe { ROT, GRUEN }\n    private int alter;\n    static int summe(int a, int b) { return a + b; }\n    void m() {\n        List<String> liste = new ArrayList<>();\n        ";
        assert!(labels(&format!("{head}Math.")).contains(&"sqrt(x)".to_string()));
        assert!(labels(&format!("{head}Math.sq")).iter().all(|l| l.starts_with("sq")));
        assert_eq!(labels(&format!("{head}Farbe.")).iter().take(2).cloned().collect::<Vec<_>>(), vec!["ROT", "GRUEN"]);
        let this = labels(&format!("{head}this."));
        assert!(this.contains(&"alter".to_string()) && this.contains(&"summe(int a, int b)".to_string()), "{this:?}");
        assert!(labels(&format!("{head}liste.")).contains(&"add(element)".to_string()));
        assert!(labels(&format!("{head}Integer.")).contains(&"parseInt(text)".to_string()));
        let plain = labels(&format!("{head}li"));
        assert_eq!(plain.first().map(String::as_str), Some("liste"), "{plain:?}");
    }
}
