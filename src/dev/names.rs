// Namen aus Tabellen und Spalten fuer erzeugten Code: snake_case (Python), camelCase/PascalCase (JavaScript),
// ohne Schluesselwoerter und ungueltige Zeichen.

/// Woerter einer Bezeichnung: "KundenNr", "kunden_nr", "Kunden-Nr" -> ["kunden", "nr"]
fn words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == 'ß' {
            cur.push_str("ss");
            continue;
        }
        let c = match c {
            'ä' | 'Ä' => 'a',
            'ö' | 'Ö' => 'o',
            'ü' | 'Ü' => 'u',
            c => c,
        };
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        // Neues Wort bei einem Grossbuchstaben nach einem Kleinbuchstaben ("kundenNr")
        let prev_lower = i > 0 && chars[i - 1].is_lowercase();
        if c.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn fix_start(s: String, fallback: &str) -> String {
    if s.is_empty() {
        fallback.to_string()
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{s}")
    } else {
        s
    }
}

pub fn snake(name: &str) -> String {
    fix_start(words(name).join("_"), "value")
}

pub fn pascal(name: &str) -> String {
    let s: String = words(name)
        .iter()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect();
    fix_start(s, "Row")
}

pub fn camel(name: &str) -> String {
    let p = pascal(name);
    let mut c = p.chars();
    let s = c.next().map(|f| f.to_ascii_lowercase().to_string() + c.as_str()).unwrap_or_default();
    fix_start(s, "value")
}

/// Name in SQL mit Backticks
pub fn sql_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
    "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match", "case", "type",
];

const JS_RESERVED: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for", "function", "if",
    "import", "in", "instanceof", "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with", "yield", "let", "static",
    "await", "implements", "interface", "package", "private", "protected", "public",
];

/// Python-Bezeichner fuer eine Spalte
pub fn py_ident(name: &str) -> String {
    let s = snake(name);
    if PY_KEYWORDS.contains(&s.as_str()) { format!("{s}_") } else { s }
}

/// JavaScript-Bezeichner fuer eine Spalte
pub fn js_ident(name: &str) -> String {
    let s = camel(name);
    if JS_RESERVED.contains(&s.as_str()) { format!("{s}_") } else { s }
}

/// Zeichenkette fuer Python/JavaScript-Quelltext mit doppelten Anfuehrungszeichen
pub fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_generated_code() {
        assert_eq!(snake("KundenNr"), "kunden_nr");
        assert_eq!(snake("Kunden-Nr 2"), "kunden_nr_2");
        assert_eq!(pascal("bestell_position"), "BestellPosition");
        assert_eq!(camel("Geburts Datum"), "geburtsDatum");
        assert_eq!(py_ident("class"), "class_");
        assert_eq!(js_ident("delete"), "delete_");
        assert_eq!(snake("1a"), "_1a");
        assert_eq!(pascal("größe"), "Grosse");
        assert_eq!(sql_name("a`b"), "`a``b`");
        assert_eq!(quoted("a\"b"), "\"a\\\"b\"");
    }
}
