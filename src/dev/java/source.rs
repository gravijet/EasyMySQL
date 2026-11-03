// Java-Quelltext lesen, ohne ihn zu uebersetzen: Paket, main-Methode und Zeichenketten (fuer SQL in
// Java-Code). Alle Positionen sind Zeichenindizes wie im Editor.

/// Zeichenkette im Quelltext
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Literal {
    /// Inhalt ohne Anfuehrungszeichen: `content.0..content.1`
    pub content: (usize, usize),
    /// Textblock (""" … """)
    pub block: bool,
    /// Mit `+` an andere Teile gehaengt: dann ist sie nur ein Stueck einer Anweisung
    pub joined: bool,
    /// Noch nicht geschlossen (wird gerade getippt)
    pub open: bool,
}

/// Bereiche im Quelltext
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Code,
    Comment,
    Str,
    Char,
}

/// Zerlegt Java grob in Code, Kommentare, Zeichenketten und Zeichen: (Art, von, bis).
pub fn spans(chars: &[char]) -> Vec<(Span, usize, usize)> {
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut code_start = 0;
    let at = |i: usize| if i < n { chars[i] } else { '\0' };
    while i < n {
        let c = chars[i];
        let kind_end = if c == '/' && at(i + 1) == '/' {
            let s = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            Some((Span::Comment, s, i))
        } else if c == '/' && at(i + 1) == '*' {
            let s = i;
            i += 2;
            while i < n && !(chars[i] == '*' && at(i + 1) == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            Some((Span::Comment, s, i))
        } else if c == '"' && at(i + 1) == '"' && at(i + 2) == '"' {
            let s = i;
            i += 3;
            while i < n && !(chars[i] == '"' && at(i + 1) == '"' && at(i + 2) == '"' && chars[i - 1] != '\\') {
                i += 1;
            }
            i = (i + 3).min(n);
            Some((Span::Str, s, i))
        } else if c == '"' || c == '\'' {
            let s = i;
            i += 1;
            while i < n && chars[i] != c && chars[i] != '\n' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < n && chars[i] == c {
                i += 1;
            }
            Some((if c == '"' { Span::Str } else { Span::Char }, s, i.min(n)))
        } else {
            None
        };
        match kind_end {
            Some((k, s, e)) => {
                if s > code_start {
                    out.push((Span::Code, code_start, s));
                }
                out.push((k, s, e));
                code_start = e;
            }
            None => i += 1,
        }
    }
    if n > code_start {
        out.push((Span::Code, code_start, n));
    }
    out
}

/// Alle Zeichenketten des Quelltexts
pub fn literals(chars: &[char]) -> Vec<Literal> {
    let sp = spans(chars);
    let mut out = Vec::new();
    for (k, &(kind, s, e)) in sp.iter().enumerate() {
        if kind != Span::Str {
            continue;
        }
        // Ein normales "" endet am zweiten Anfuehrungszeichen, drei am Anfang gibt es nur bei Textbloecken
        let block = e - s >= 3 && chars[s + 1] == '"' && chars[s + 2] == '"';
        let (q, open) = if block {
            (3, !(e - s >= 6 && chars[e - 1] == '"' && chars[e - 2] == '"' && chars[e - 3] == '"'))
        } else {
            (1, !(e - s >= 2 && chars[e - 1] == '"'))
        };
        let content = (s + q, if open { e } else { e - q });
        // Steht ein + direkt davor oder dahinter (nur Leerraum dazwischen)?
        let plus = |idx: Option<usize>, forward: bool| -> bool {
            let Some(j) = idx else { return false };
            let (kind, a, b) = sp[j];
            if kind != Span::Code {
                return false;
            }
            let text: String = chars[a..b].iter().collect();
            let t = if forward { text.trim_start() } else { text.trim_end() };
            if forward { t.starts_with('+') } else { t.ends_with('+') }
        };
        let joined = plus(k.checked_sub(1), false) || plus(Some(k + 1).filter(|j| *j < sp.len()), true);
        out.push(Literal { content: (content.0.min(content.1), content.1), block, joined, open });
    }
    out
}

/// Zeichenkette, in der der Cursor steht
pub fn literal_at(chars: &[char], cursor: usize) -> Option<Literal> {
    literals(chars).into_iter().find(|l| cursor >= l.content.0 && cursor <= l.content.1)
}

const SQL_START: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "ALTER", "DROP", "TRUNCATE", "WITH", "SHOW", "DESCRIBE", "EXPLAIN", "CALL", "USE", "SET",
    "GRANT", "REVOKE", "RENAME",
];

/// Sieht der Inhalt einer Zeichenkette nach SQL aus? In Grossbuchstaben geschriebene Befehle zaehlen
/// immer, kleingeschriebene nur in eindeutiger Form ("select … from"), damit Texte wie "Select a file"
/// nicht als SQL gelten.
pub fn looks_like_sql(content: &str) -> bool {
    let t = content.trim_start();
    let first: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let upper = first.to_ascii_uppercase();
    if !SQL_START.contains(&upper.as_str()) {
        return false;
    }
    if first == upper && first.len() > 1 {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    match upper.as_str() {
        "SELECT" => words.contains(&"from"),
        "INSERT" | "REPLACE" => words.get(1) == Some(&"into"),
        "DELETE" => words.get(1) == Some(&"from"),
        "UPDATE" => words.get(2) == Some(&"set"),
        "CREATE" | "DROP" | "ALTER" | "TRUNCATE" => words.get(1).is_some_and(|w| matches!(*w, "table" | "database" | "view" | "index")),
        _ => false,
    }
}

/// Paketname aus `package a.b.c;`
pub fn package(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for (kind, a, b) in spans(&chars) {
        if kind != Span::Code {
            continue;
        }
        let code: String = chars[a..b].iter().collect();
        let t = code.trim_start();
        if t.is_empty() {
            continue;
        }
        let rest = t.strip_prefix("package")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$').collect();
        return (!name.is_empty()).then_some(name);
    }
    None
}

/// Hat die Datei eine main-Methode? (auch die kurze Form `void main()` aus Java 25)
pub fn has_main(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let code: String = spans(&chars).into_iter().map(|(k, a, b)| if k == Span::Code { chars[a..b].iter().collect::<String>() } else { " ".to_string() }).collect();
    let squeezed: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed.contains("void main(") || squeezed.contains("void main (")
}

/// Ist `name` ein gueltiger Java-Bezeichner (und kein Schluesselwort)?
pub fn is_identifier(name: &str) -> bool {
    let mut it = name.chars();
    let Some(first) = it.next() else { return false };
    (first.is_alphabetic() || first == '_' || first == '$') && it.all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !KEYWORDS.contains(&name) && name != "_"
}

/// Java-Schluesselwoerter (sortiert)
pub const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue", "default", "do", "double", "else", "enum",
    "extends", "false", "final", "finally", "float", "for", "goto", "if", "implements", "import", "instanceof", "int", "interface", "long", "native", "new",
    "null", "package", "private", "protected", "public", "return", "short", "static", "strictfp", "super", "switch", "synchronized", "this", "throw",
    "throws", "transient", "true", "try", "void", "volatile", "while",
];

/// Aus einem frei getippten Namen einen Klassennamen machen: "neue klasse" -> "NeueKlasse"
pub fn class_name_from(input: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in input.chars() {
        if c.is_alphanumeric() || c == '_' {
            if up {
                out.extend(c.to_uppercase());
            } else {
                out.push(c);
            }
            up = false;
        } else {
            up = true;
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lits(s: &str) -> Vec<(String, bool, bool)> {
        let c: Vec<char> = s.chars().collect();
        literals(&c).into_iter().map(|l| (c[l.content.0..l.content.1].iter().collect(), l.block, l.joined)).collect()
    }

    #[test]
    fn strings_and_text_blocks_are_found_and_comments_skipped() {
        let src = "// \"kein\"\nString a = \"SELECT 1\"; /* \"auch nicht\" */ char q = '\"';\nString b = \"\"\"\n    SELECT *\n    FROM t\"\"\";\nString c = \"x\" + y;";
        let l = lits(src);
        assert_eq!(l.len(), 3, "{l:?}");
        assert_eq!(l[0], ("SELECT 1".into(), false, false));
        assert_eq!(l[1], ("\n    SELECT *\n    FROM t".into(), true, false));
        assert_eq!(l[2], ("x".into(), false, true));
    }

    #[test]
    fn escaped_quotes_stay_inside() {
        assert_eq!(lits(r#"s = "a \"b\" c";"#)[0].0, r#"a \"b\" c"#);
    }

    #[test]
    fn open_literal_while_typing() {
        let c: Vec<char> = "q = \"SELECT * FR".chars().collect();
        let l = literal_at(&c, c.len()).unwrap();
        assert!(l.open);
        assert_eq!(c[l.content.0..l.content.1].iter().collect::<String>(), "SELECT * FR");
    }

    #[test]
    fn sql_is_recognised_without_false_alarms() {
        assert!(looks_like_sql("SELECT * FROM kunde"));
        assert!(looks_like_sql("\n  UPDATE t SET a = ?"));
        assert!(looks_like_sql("select id from t"));
        assert!(looks_like_sql("insert into t values (1)"));
        assert!(!looks_like_sql("Select a file"));
        assert!(!looks_like_sql("Delete failed"));
        assert!(!looks_like_sql("Hallo"));
        assert!(!looks_like_sql("set the value"));
    }

    #[test]
    fn package_main_and_names() {
        let src = "/* Kopf */\n// x\npackage app.dao;\n\nimport java.sql.*;\npublic class A { public static void main(String[] args) {} }";
        assert_eq!(package(src).as_deref(), Some("app.dao"));
        assert_eq!(package("public class A {}"), None);
        assert!(has_main(src));
        assert!(has_main("void main() {\n  IO.println(1);\n}"));
        assert!(!has_main("class A { String s = \"void main(\"; }"));
        assert!(is_identifier("Kunde") && !is_identifier("class") && !is_identifier("1a") && !is_identifier("a b"));
        assert_eq!(class_name_from("neue klasse"), "NeueKlasse");
        assert_eq!(class_name_from("kunden-dao"), "KundenDao");
    }
}
