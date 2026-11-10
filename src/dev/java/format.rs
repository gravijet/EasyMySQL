// Java formatieren ohne Sprachserver: Einrueckung nach Klammern (wie IntelliJ), Leerzeichen um Operatoren,
// nach Kommas und Schluesselwoertern, keine Leerzeichen am Zeilenende, hoechstens eine Leerzeile.
// Zeichenketten (auch Textbloecke) und Kommentare bleiben unveraendert.

use super::source::{self, Span};

/// Art jedes Zeichens: Code, Kommentar, Zeichenkette
fn kinds(chars: &[char]) -> Vec<Span> {
    let mut out = vec![Span::Code; chars.len()];
    for (k, a, b) in source::spans(chars) {
        for x in out.iter_mut().take(b.min(chars.len())).skip(a) {
            *x = k;
        }
    }
    out
}

const PAREN_WORDS: &[&str] = &["if", "for", "while", "switch", "catch", "synchronized", "try", "return"];
const BRACE_WORDS: &[&str] = &["else", "try", "finally", "do"];

/// Leerzeichen innerhalb der Zeilen (nur im Code)
fn space_tokens(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let kind = kinds(&chars);
    let code = |i: usize| kind.get(i) == Some(&Span::Code);
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    let last_out = |out: &String| out.chars().last();
    // Offene Generics (<String, List<Integer>>), damit ihre spitzen Klammern keine Leerzeichen bekommen
    let mut generics = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !code(i) {
            out.push(c);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied();
        // Operatoren aus zwei Zeichen und =
        let two: Option<&str> = next.and_then(|n| match (c, n) {
            ('=', '=') => Some("=="),
            ('!', '=') => Some("!="),
            ('<', '=') => Some("<="),
            ('>', '=') => Some(">="),
            ('&', '&') => Some("&&"),
            ('|', '|') => Some("||"),
            ('+', '=') => Some("+="),
            ('-', '=') => Some("-="),
            ('*', '=') => Some("*="),
            ('/', '=') => Some("/="),
            ('%', '=') => Some("%="),
            _ => None,
        });
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        // >>= und <<= sowie Generics wie List<>= nicht anfassen
        let shift_assign = matches!(c, '<' | '>') && (prev == Some(c) || next == Some(c)) && (next == Some('=') || chars.get(i + 2) == Some(&'='));
        if let Some(op) = two.filter(|_| !shift_assign && code(i + 1)) {
            if last_out(&out).is_some_and(|p| !p.is_whitespace() && p != '(') {
                out.push(' ');
            }
            out.push_str(op);
            i += 2;
            if chars.get(i).is_some_and(|n| !n.is_whitespace()) {
                out.push(' ');
            }
            continue;
        }
        if c == '=' && !matches!(prev, Some('=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^')) && next != Some('=') {
            if last_out(&out).is_some_and(|p| !p.is_whitespace()) {
                out.push(' ');
            }
            out.push('=');
            i += 1;
            if chars.get(i).is_some_and(|n| !n.is_whitespace()) {
                out.push(' ');
            }
            continue;
        }
        if (c == '<' || c == '>') && two.is_none() && !shift_assign && prev != Some('-') && (c == '>' && generics > 0 || next != Some(c) && prev != Some(c)) {
            let word: String = out.chars().rev().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect::<Vec<_>>().into_iter().rev().collect();
            let generic = if c == '<' {
                word.starts_with(|f: char| f.is_uppercase()) || next == Some('>') || word.is_empty() && last_out(&out).is_some_and(|p| p == '.' || p == ' ')
            } else {
                generics > 0
            };
            if generic {
                if c == '<' {
                    generics += 1;
                } else {
                    generics -= 1;
                }
                out.push(c);
                i += 1;
                continue;
            }
            if last_out(&out).is_some_and(|p| !p.is_whitespace()) {
                out.push(' ');
            }
            out.push(c);
            i += 1;
            if chars.get(i).is_some_and(|n| !n.is_whitespace()) {
                out.push(' ');
            }
            continue;
        }
        // Binaere Rechenzeichen (nicht ++, --, ->, -1, java.util.*, 1e-5)
        if matches!(c, '+' | '-' | '*' | '/' | '%') && two.is_none() && next != Some(c) && prev != Some(c) && !(c == '-' && next == Some('>')) {
            let before = out.trim_end().chars().last();
            let number_exp = {
                let w: String = out.chars().rev().take_while(|ch| ch.is_alphanumeric() || *ch == '.').collect::<Vec<_>>().into_iter().rev().collect();
                w.starts_with(|f: char| f.is_ascii_digit()) && w.ends_with(['e', 'E'])
            };
            let word: String = out.trim_end().chars().rev().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect::<Vec<_>>().into_iter().rev().collect();
            let keyword = matches!(word.as_str(), "return" | "case" | "yield" | "throw" | "else" | "assert");
            let operand = !keyword && before.is_some_and(|p| p.is_alphanumeric() || matches!(p, '_' | ')' | ']' | '"' | '\''));
            if operand && !number_exp && !(c == '*' && before == Some('.')) {
                if last_out(&out).is_some_and(|p| !p.is_whitespace()) {
                    out.push(' ');
                }
                out.push(c);
                i += 1;
                if chars.get(i).is_some_and(|n| !n.is_whitespace()) {
                    out.push(' ');
                }
                continue;
            }
        }
        if c == ';' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace() && *n != ')' && *n != ';') {
            out.push_str("; ");
            i += 1;
            continue;
        }
        if c == ',' {
            out.push(',');
            i += 1;
            if chars.get(i).is_some_and(|n| !n.is_whitespace()) {
                out.push(' ');
            }
            continue;
        }
        // ") {"  "} else"  "else {"
        if c == '{' && last_out(&out).is_some_and(|p| p == ')' || p.is_alphanumeric()) {
            let tail: String = out.chars().rev().take_while(|ch| ch.is_alphanumeric()).collect::<Vec<_>>().into_iter().rev().collect();
            if last_out(&out) == Some(')') || BRACE_WORDS.contains(&tail.as_str()) || tail.chars().next().is_some_and(|f| f.is_alphanumeric()) {
                out.push(' ');
            }
        }
        if c == '}' {
            out.push('}');
            i += 1;
            let rest: String = chars[i..].iter().take(8).collect();
            if rest.starts_with("else") || rest.starts_with("catch") || rest.starts_with("finally") || rest.starts_with("while") {
                out.push(' ');
            }
            continue;
        }
        if c == '(' {
            let tail: String = out.chars().rev().take_while(|ch| ch.is_alphanumeric() || *ch == '_').collect::<Vec<_>>().into_iter().rev().collect();
            if PAREN_WORDS.contains(&tail.as_str()) {
                out.push(' ');
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Endet die Zeile mitten in einem Ausdruck (`x =`, `a &&`, `s +`)? `i++` und `case 1:` nicht.
fn open_expression(end: &str) -> bool {
    let last = end.chars().last();
    !matches!(end, "++" | "--" | "->") && matches!(last, Some('=' | '+' | '-' | '&' | '|' | '?')) && !end.ends_with("==")
}

/// Einruecken nach Klammern. `step` = Einrueckung je Ebene.
fn indent(text: &str, step: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let kind = kinds(&chars);
    let mut out = String::with_capacity(text.len());
    // Offene Bloecke: ist es ein switch?
    let mut blocks: Vec<bool> = Vec::new();
    let mut parens: i32 = 0;
    let mut blank = 0;
    let mut pos = 0;
    // Ende der vorigen Codezeile (zwei Zeichen): offener Ausdruck -> Fortsetzung einruecken
    let mut prev_code_end = String::new();
    for line in text.split('\n') {
        let start = pos;
        let n = line.chars().count();
        pos += n + 1;
        // Mitten in einem Textblock oder Blockkommentar: Zeile unveraendert lassen (nur Kommentarzeilen ausrichten)
        let lead = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let first = start + lead;
        let inside = |k: Span| start > 0 && kind.get(start - 1) == Some(&k) && kind.get(start) == Some(&k);
        if inside(Span::Str) {
            out.push_str(line.trim_end_matches([' ', '\t', '\r']));
            out.push('\n');
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        let in_comment = inside(Span::Comment) && kind.get(first) == Some(&Span::Comment);
        // Ebene dieser Zeile: schliessende Klammern am Anfang ruecken aus
        let mut level = blocks.len() as i32;
        let closing = trimmed.chars().take_while(|c| *c == '}' || *c == ')').filter(|c| *c == '}').count() as i32;
        level -= closing;
        let in_switch = blocks.len() as i32 > level.max(0) - 1 && level > 0 && blocks.get(level as usize - 1).copied().unwrap_or(false);
        if in_switch && !(trimmed.starts_with("case ") || trimmed.starts_with("default") || trimmed.starts_with('}')) {
            level += 1;
        }
        let mut cont = 0;
        if !in_comment && (parens > 0 || trimmed.starts_with('.') || open_expression(&prev_code_end) && !trimmed.starts_with('{')) {
            cont = 2;
        }
        let mut lineout = String::new();
        for _ in 0..(level.max(0) + cont) {
            lineout.push_str(step);
        }
        if in_comment && trimmed.starts_with('*') {
            lineout.push(' ');
        }
        lineout.push_str(trimmed);
        out.push_str(lineout.trim_end());
        out.push('\n');
        // Klammern dieser Zeile zaehlen (nur Code)
        let code_line: String = line.chars().enumerate().filter(|(i, _)| kind.get(start + i) == Some(&Span::Code)).map(|(_, c)| c).collect();
        for c in code_line.chars() {
            match c {
                '{' => {
                    let before = code_line.split('{').next().unwrap_or("");
                    blocks.push(before.trim_start().starts_with("switch") || before.contains(" switch") || before.contains("= switch"));
                }
                '}' => {
                    blocks.pop();
                }
                '(' => parens += 1,
                ')' => parens = (parens - 1).max(0),
                _ => {}
            }
        }
        let t = code_line.trim_end();
        if !t.is_empty() {
            prev_code_end = t.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
        }
    }
    // Hoechstens ein Zeilenumbruch am Ende wie in der Vorlage
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !text.ends_with('\n') {
        out.pop();
    }
    out
}

/// Ganze Datei formatieren
pub fn format(text: &str, step: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\t', step);
    indent(&space_tokens(&text), step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_braces_and_fixes_spacing() {
        let src = "public class A{\nint x=1;\nvoid m(int a,int b){\nfor(int i=0;i<2;i++){}\nif(a!=b){\nx+=a;\n}else{\nx=b;\n}\n}\n\n\n\n}\n";
        let want = "public class A {\n    int x = 1;\n    void m(int a, int b) {\n        for (int i = 0; i < 2; i++) {}\n        if (a != b) {\n            x += a;\n        } else {\n            x = b;\n        }\n    }\n\n}\n";
        assert_eq!(format(src, "    "), want);
    }

    #[test]
    fn strings_comments_and_text_blocks_stay() {
        let src = "class A {\n  String s=\"a=b,c\";\n  /* x=1,\n   * y */\n  String q = \"\"\"\n      SELECT a,b\n        FROM t\n      \"\"\";\n  // a=b\n}\n";
        let out = format(src, "    ");
        assert!(out.contains("String s = \"a=b,c\";"), "{out}");
        assert!(out.contains("      SELECT a,b\n        FROM t\n"), "{out}");
        assert!(out.contains("    // a=b"), "{out}");
        assert!(out.contains("    /* x=1,\n     * y */"), "{out}");
    }

    #[test]
    fn switch_cases_lambdas_and_generics() {
        let src = "class A {\nvoid m(int k) {\nswitch (k) {\ncase 1:\nfoo();\nbreak;\ndefault:\nbar();\n}\nList<List<String>> l = new ArrayList<>();\nboolean b = x>2 && y<3;\nRunnable r = () -> {\nrun();\n};\nx >>= 2;\n}\n}\n";
        let out = format(src, "    ");
        assert!(out.contains("        switch (k) {\n            case 1:\n                foo();\n                break;\n            default:\n                bar();\n        }\n"), "{out}");
        assert!(out.contains("List<List<String>> l = new ArrayList<>();"), "{out}");
        assert!(out.contains("Runnable r = () -> {\n            run();\n        };"), "{out}");
        assert!(out.contains("x >>= 2;"), "{out}");
        assert!(out.contains("boolean b = x > 2 && y < 3;"), "{out}");
        let ops = format("class A {\nint a=b+c*2-d/e%f;\nint i=-1;\ni++;\nreturn -x;\ndouble z=1e-5;\nimport java.util.*;\nString s=w+\" \"+v;\n}\n", "    ");
        assert!(ops.contains("int a = b + c * 2 - d / e % f;") && ops.contains("int i = -1;") && ops.contains("i++;") && ops.contains("return -x;") && ops.contains("1e-5") && ops.contains("java.util.*;") && ops.contains("String s = w + \" \" + v;"), "{ops}");
    }

    #[test]
    fn already_formatted_code_stays_the_same() {
        let src = "package app;\n\nimport java.util.*;\n\npublic class Main {\n    public static void main(String[] args) {\n        for (int i = 0; i < 3; i++) {\n            System.out.println(i + \" \" + args.length);\n        }\n    }\n}\n";
        assert_eq!(format(src, "    "), src);
    }
}
