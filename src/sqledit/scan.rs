// Python und JavaScript/TypeScript grob zerlegen: Code, Kommentare, Zeichenketten (mit f-Strings und
// Template-Strings) und regulaere Ausdruecke. Liefert dieselben Bereiche wie der Java-Leser, damit
// Hervorhebung, SQL-Pruefung in Zeichenketten und Vorschlaege fuer alle Sprachen gleich arbeiten.

use crate::dev::java::source::{Literal, Span};

/// Python: (Art, von, bis) in Zeichenindizes
pub fn python_spans(chars: &[char]) -> Vec<(Span, usize, usize)> {
    python_scan(chars).0
}

/// Zeichenketten in Python. `joined`: f-String mit {…}, mit + angehaengt oder mit % bzw. .format() gefuellt.
pub fn python_literals(chars: &[char]) -> Vec<Literal> {
    python_scan(chars).1
}

fn python_scan(chars: &[char]) -> (Vec<(Span, usize, usize)>, Vec<Literal>) {
    let n = chars.len();
    let at = |i: usize| if i < n { chars[i] } else { '\0' };
    let mut spans = Vec::new();
    let mut lits = Vec::new();
    let mut i = 0;
    let mut code_start = 0;
    let push_code = |spans: &mut Vec<(Span, usize, usize)>, from: usize, to: usize| {
        if to > from {
            spans.push((Span::Code, from, to));
        }
    };
    while i < n {
        let c = chars[i];
        if c == '#' {
            push_code(&mut spans, code_start, i);
            let s = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            spans.push((Span::Comment, s, i));
            code_start = i;
            continue;
        }
        // Praefix (r, b, f, u, rb, fr, …) direkt vor dem Anfuehrungszeichen
        let mut q = i;
        let mut prefix = String::new();
        if c.is_ascii_alphabetic() && (i == 0 || !super::highlight::is_word_char(chars[i - 1])) {
            while q < n && q - i < 2 && matches!(chars[q].to_ascii_lowercase(), 'r' | 'b' | 'f' | 'u' | 't') {
                prefix.push(chars[q].to_ascii_lowercase());
                q += 1;
            }
            if !(at(q) == '"' || at(q) == '\'') {
                q = i;
                prefix.clear();
            }
        }
        let quote = at(q);
        if quote == '"' || quote == '\'' {
            push_code(&mut spans, code_start, i);
            let s = i;
            let triple = at(q + 1) == quote && at(q + 2) == quote;
            let qn = if triple { 3 } else { 1 };
            let raw = prefix.contains('r');
            let mut j = q + qn;
            let content_start = j;
            let mut closed = false;
            while j < n {
                if chars[j] == '\\' && !raw {
                    j += 2;
                    continue;
                }
                if chars[j] == '\\' && raw {
                    j += 2;
                    continue;
                }
                if !triple && chars[j] == '\n' {
                    break;
                }
                if chars[j] == quote && (!triple || (at(j + 1) == quote && at(j + 2) == quote)) {
                    closed = true;
                    break;
                }
                j += 1;
            }
            let j = j.min(n);
            let content_end = j;
            let end = if closed { j + qn } else { j };
            spans.push((Span::Str, s, end));
            let content: String = chars[content_start..content_end].iter().collect();
            let fmt = prefix.contains('f') && content.replace("{{", "").contains('{');
            lits.push(Literal { content: (content_start, content_end), block: triple, joined: fmt, open: !closed });
            i = end;
            code_start = i;
            continue;
        }
        i += 1;
    }
    push_code(&mut spans, code_start, n);
    mark_joined(&spans, &mut lits, chars, &['+', '%'], &[".format("]);
    (spans, lits)
}

/// Kann an dieser Stelle in JavaScript ein regulaerer Ausdruck beginnen? (nach Operator, Klammer, Schluesselwort)
fn regex_allowed(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j > 0 && chars[j - 1].is_whitespace() {
        j -= 1;
    }
    if j == 0 {
        return true;
    }
    let p = chars[j - 1];
    if matches!(p, '(' | ',' | '=' | ':' | '[' | '!' | '&' | '|' | '?' | '{' | '}' | ';' | '+' | '-' | '*' | '%' | '<' | '>' | '~' | '^') {
        return true;
    }
    // return /x/, typeof /x/ …
    let mut k = j;
    while k > 0 && chars[k - 1].is_ascii_alphabetic() {
        k -= 1;
    }
    let word: String = chars[k..j].iter().collect();
    matches!(word.as_str(), "return" | "typeof" | "case" | "do" | "else" | "in" | "of" | "new" | "delete" | "void" | "throw" | "yield" | "await")
}

/// JavaScript/TypeScript: (Art, von, bis)
pub fn js_spans(chars: &[char]) -> Vec<(Span, usize, usize)> {
    js_scan(chars).0
}

/// Zeichenketten in JavaScript. `block`: Template-String (darf Zeilenumbrueche enthalten);
/// `joined`: Template mit ${…} oder mit + angehaengt.
pub fn js_literals(chars: &[char]) -> Vec<Literal> {
    js_scan(chars).1
}

fn js_scan(chars: &[char]) -> (Vec<(Span, usize, usize)>, Vec<Literal>) {
    let n = chars.len();
    let at = |i: usize| if i < n { chars[i] } else { '\0' };
    let mut spans = Vec::new();
    let mut lits = Vec::new();
    let mut i = 0;
    let mut code_start = 0;
    let flush = |spans: &mut Vec<(Span, usize, usize)>, from: usize, to: usize| {
        if to > from {
            spans.push((Span::Code, from, to));
        }
    };
    while i < n {
        let c = chars[i];
        if c == '/' && at(i + 1) == '/' {
            flush(&mut spans, code_start, i);
            let s = i;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            spans.push((Span::Comment, s, i));
            code_start = i;
        } else if c == '/' && at(i + 1) == '*' {
            flush(&mut spans, code_start, i);
            let s = i;
            i += 2;
            while i < n && !(chars[i] == '*' && at(i + 1) == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            spans.push((Span::Comment, s, i));
            code_start = i;
        } else if c == '"' || c == '\'' {
            flush(&mut spans, code_start, i);
            let s = i;
            let mut j = i + 1;
            while j < n && chars[j] != c && chars[j] != '\n' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let j = j.min(n);
            let closed = j < n && chars[j] == c;
            let end = if closed { j + 1 } else { j };
            spans.push((Span::Str, s, end));
            lits.push(Literal { content: (s + 1, j), block: false, joined: false, open: !closed });
            i = end;
            code_start = i;
        } else if c == '`' {
            flush(&mut spans, code_start, i);
            let s = i;
            let mut j = i + 1;
            let mut interp = false;
            let mut depth = 0usize;
            while j < n {
                let d = chars[j];
                if d == '\\' {
                    j += 2;
                    continue;
                }
                if depth == 0 && d == '`' {
                    break;
                }
                if d == '$' && at(j + 1) == '{' {
                    interp = true;
                    depth += 1;
                    j += 2;
                    continue;
                }
                if depth > 0 && d == '}' {
                    depth -= 1;
                } else if depth > 0 && d == '{' {
                    depth += 1;
                }
                j += 1;
            }
            let j = j.min(n);
            let closed = j < n;
            let end = if closed { j + 1 } else { j };
            spans.push((Span::Str, s, end));
            lits.push(Literal { content: (s + 1, j), block: true, joined: interp, open: !closed });
            i = end;
            code_start = i;
        } else if c == '/' && regex_allowed(chars, i) && at(i + 1) != '/' && at(i + 1) != '*' {
            // Regulaerer Ausdruck bis zum naechsten / (ausserhalb von [...]) in derselben Zeile
            let mut j = i + 1;
            let mut class = false;
            let mut ok = false;
            while j < n && chars[j] != '\n' {
                match chars[j] {
                    '\\' => j += 1,
                    '[' => class = true,
                    ']' => class = false,
                    '/' if !class => {
                        ok = true;
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            if ok {
                flush(&mut spans, code_start, i);
                j += 1;
                while j < n && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                spans.push((Span::Char, i, j));
                i = j;
                code_start = i;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    flush(&mut spans, code_start, n);
    mark_joined(&spans, &mut lits, chars, &['+'], &[]);
    (spans, lits)
}

/// Zeichenketten, an denen direkt ein Operator haengt (`"…" + x`, `"…" % x`, `"…".format(`), als
/// zusammengesetzt markieren: sie sind dann nur ein Teil einer Anweisung.
fn mark_joined(spans: &[(Span, usize, usize)], lits: &mut [Literal], chars: &[char], ops: &[char], calls: &[&str]) {
    for l in lits.iter_mut() {
        let Some(k) = spans.iter().position(|(kind, a, b)| *kind == Span::Str && *a < l.content.0 && *b >= l.content.1) else { continue };
        let code = |idx: usize| -> Option<String> {
            let (kind, a, b) = *spans.get(idx)?;
            (kind == Span::Code).then(|| chars[a..b].iter().collect())
        };
        let before = k.checked_sub(1).and_then(code).is_some_and(|t| t.trim_end().ends_with(|c: char| ops.contains(&c)));
        let after = code(k + 1).is_some_and(|t| {
            let t = t.trim_start();
            t.starts_with(|c: char| ops.contains(&c)) || calls.iter().any(|c| t.starts_with(c))
        });
        l.joined |= before || after;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn text(c: &[char], r: (usize, usize)) -> String {
        c[r.0..r.1].iter().collect()
    }

    #[test]
    fn python_strings_and_comments() {
        let src = "x = 'a#b'  # Kommentar\nq = \"\"\"SELECT *\nFROM t\"\"\"\nf = f\"{x} y\"\ng = rb'\\d'\nh = \"SELECT %s\" % (1,)\n";
        let c = chars(src);
        let sp = python_spans(&c);
        assert!(sp.iter().any(|(k, a, b)| *k == Span::Comment && text(&c, (*a, *b)) == "# Kommentar"));
        let l = python_literals(&c);
        assert_eq!(l.len(), 5);
        assert_eq!(text(&c, l[0].content), "a#b");
        assert!(l[1].block && text(&c, l[1].content) == "SELECT *\nFROM t");
        assert!(l[2].joined, "f-String mit Platzhalter");
        assert_eq!(text(&c, l[3].content), "\\d");
        assert!(l[4].joined, "mit % gefuellt");
        let open = python_literals(&chars("q = \"SELECT * FR"));
        assert!(open[0].open);
    }

    #[test]
    fn javascript_strings_templates_and_regex() {
        let src = "const a = 'x' + b; // k\nconst t = `SELECT ${c}\nFROM t`;\nconst r = /\"[a-z]\"/g.test(s);\nconst q = \"SELECT 1\";\nconst d = x / 2 / y;";
        let c = chars(src);
        let l = js_literals(&c);
        assert_eq!(l.len(), 3, "{l:?}");
        assert!(l[0].joined);
        assert!(l[1].block && l[1].joined);
        assert_eq!(text(&c, l[2].content), "SELECT 1");
        assert!(!l[2].joined);
        let sp = js_spans(&c);
        assert!(sp.iter().any(|(k, a, b)| *k == Span::Char && text(&c, (*a, *b)) == "/\"[a-z]\"/g"));
        // Geteilt durch ist kein regulaerer Ausdruck
        assert!(!sp.iter().any(|(k, a, _)| *k == Span::Char && c[*a] == '/' && *a > src.find("x / 2").unwrap()));
    }
}
