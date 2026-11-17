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

/// Zeilen trennen wie in IntelliJ: eine Anweisung je Zeile, nach "{" und vor "}" eines Blocks, nach
/// "case x:", nach Annotationen am Zeilenanfang; "{" allein in einer Zeile kommt ans Ende der vorigen.
/// Array-Initialisierer ({1, 2}) und for-Koepfe bleiben in einer Zeile.
fn split_lines(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let kind = kinds(&chars);
    let code = |i: usize| kind.get(i) == Some(&Span::Code);
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    // Offene geschweifte Klammern: (Block?, Klammertiefe davor); Klammertiefe zaehlt je Block neu
    let mut braces: Vec<(bool, i32)> = Vec::new();
    let mut parens = 0i32;
    // Rest der Zeile ab i: naechstes Zeichen (Code) und ob nur noch ein Kommentar oder nichts folgt
    let rest = |i: usize| -> Option<char> {
        let mut k = i;
        while k < chars.len() && chars[k] != '\n' {
            if !chars[k].is_whitespace() {
                return if code(k) { Some(chars[k]) } else { None };
            }
            k += 1;
        }
        None
    };
    let word_at = |i: usize| -> String { chars[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect() };
    let newline = |out: &mut String| {
        while out.ends_with(' ') || out.ends_with('\t') {
            out.pop();
        }
        out.push('\n');
    };
    let line_start = |out: &String| out.rsplit('\n').next().unwrap_or("").trim().is_empty();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !code(i) {
            out.push(c);
            i += 1;
            continue;
        }
        match c {
            '(' => parens += 1,
            ')' => parens = (parens - 1).max(0),
            '{' => {
                let prev = out.trim_end().chars().last();
                let parent_inline = braces.last().is_some_and(|b| !b.0);
                let inline = parent_inline || matches!(prev, Some('=' | ']' | ',' | '('));
                // "{" allein am Zeilenanfang: an die vorige Zeile haengen (nicht nach Kommentaren)
                if !inline && line_start(&out) {
                    let trimmed = out.trim_end().to_string();
                    let last = trimmed.rsplit('\n').next().unwrap_or("");
                    if last.ends_with(|ch: char| ch == ')' || ch.is_alphanumeric()) && !last.contains("//") && !last.trim_start().starts_with('*') {
                        out = trimmed;
                        out.push(' ');
                    }
                }
                braces.push((!inline, parens));
                parens = 0;
                out.push('{');
                i += 1;
                if !inline && rest(i).is_some_and(|n| n != '}') {
                    newline(&mut out);
                }
                continue;
            }
            '}' => {
                let (block, saved) = braces.pop().unwrap_or((true, 0));
                parens = saved;
                if block && !line_start(&out) && !out.trim_end().ends_with('{') {
                    newline(&mut out);
                }
                out.push('}');
                i += 1;
                if block && let Some(n) = rest(i) {
                    let w = word_at(chars[i..].iter().position(|c| !c.is_whitespace()).map_or(i, |p| i + p));
                    if !matches!(n, ')' | ';' | ',' | '.') && !matches!(w.as_str(), "else" | "catch" | "finally" | "while") {
                        newline(&mut out);
                    }
                }
                continue;
            }
            ';' if parens == 0 => {
                out.push(';');
                i += 1;
                if rest(i).is_some() {
                    newline(&mut out);
                }
                continue;
            }
            ':' if parens == 0 && chars.get(i + 1) != Some(&':') && i > 0 && chars[i - 1] != ':' => {
                let line = out.rsplit('\n').next().unwrap_or("").trim_start();
                if line.starts_with("case ") || line.starts_with("default") {
                    out.push(':');
                    i += 1;
                    if rest(i).is_some() {
                        newline(&mut out);
                    }
                    continue;
                }
            }
            '@' if line_start(&out) && !chars[i + 1..].starts_with(&['i', 'n', 't', 'e', 'r', 'f', 'a', 'c', 'e']) => {
                // Annotation am Zeilenanfang (samt Argumenten) in eine eigene Zeile
                let mut k = i + 1;
                while k < chars.len() && (chars[k].is_alphanumeric() || chars[k] == '_' || chars[k] == '.') {
                    k += 1;
                }
                if chars.get(k) == Some(&'(') {
                    let mut depth = 0;
                    while k < chars.len() {
                        if code(k) {
                            match chars[k] {
                                '(' => depth += 1,
                                ')' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        k += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        k += 1;
                    }
                }
                out.extend(&chars[i..k]);
                i = k;
                if rest(i).is_some_and(|n| n.is_alphabetic() || n == '@') {
                    newline(&mut out);
                    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                        i += 1;
                    }
                }
                continue;
            }
            _ => {}
        }
        // Nach einem eingefuegten Zeilenumbruch keine fuehrenden Leerzeichen (das Einruecken macht `indent`)
        if (c == ' ' || c == '\t') && out.ends_with('\n') {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Leerzeilen wie in IntelliJ: nach package, nach den Imports und zwischen Methoden einer Klasse
fn blank_lines(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    for (k, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(prev) = out.last().cloned() {
            let p = prev.trim();
            let need = !t.is_empty()
                && !p.is_empty()
                && (p.starts_with("package ") && p.ends_with(';')
                    || p.starts_with("import ") && !t.starts_with("import ")
                    // "}" eines Members der Klasse, danach der naechste Member auf gleicher Ebene
                    || p == "}" && indent_of(&prev) > 0 && indent_of(&prev) == indent_of(line) && !t.starts_with('}') && !t.starts_with(')') && k > 0 && is_member_level(&out, indent_of(line)));
            if need {
                out.push(String::new());
            }
        }
        out.push((*line).to_string());
    }
    out.join("\n")
}

/// Liegt die Ebene `indent` direkt in einer Klasse (nicht in einer Methode)? Die naechste oeffnende Zeile
/// weiter aussen ist dann eine Klassen-, Interface-, Enum- oder Record-Deklaration.
fn is_member_level(out: &[String], indent: usize) -> bool {
    for l in out.iter().rev() {
        let t = l.trim();
        if t.is_empty() || l.len() - l.trim_start().len() >= indent {
            continue;
        }
        return t.ends_with('{') && [" class ", " interface ", " enum ", " record "].iter().any(|k| format!(" {t}").contains(k));
    }
    false
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
    // (switch-Block?, Klammertiefe davor): in einem Block (z. B. Lambda im Aufruf) zaehlen Klammern neu
    let mut blocks: Vec<(bool, i32)> = Vec::new();
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
        let in_switch = blocks.len() as i32 > level.max(0) - 1 && level > 0 && blocks.get(level as usize - 1).is_some_and(|b| b.0);
        if in_switch && !(trimmed.starts_with("case ") || trimmed.starts_with("default") || trimmed.starts_with('}')) {
            level += 1;
        }
        let mut cont = 0;
        let clause = ["throws ", "extends ", "implements "].iter().any(|k| trimmed.starts_with(k));
        // ")" am Zeilenanfang schliesst die Argumentliste: wieder auf Hoehe der Anweisung
        let closers = trimmed.chars().take_while(|c| *c == ')').count() as i32;
        if !in_comment && (parens - closers > 0 || clause || trimmed.starts_with('.') || open_expression(&prev_code_end) && !trimmed.starts_with('{')) {
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
                    blocks.push((before.trim_start().starts_with("switch") || before.contains(" switch") || before.contains("= switch"), parens));
                    parens = 0;
                }
                '}' => {
                    if let Some((_, p)) = blocks.pop() {
                        parens = p;
                    }
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
    blank_lines(&indent(&split_lines(&space_tokens(&text)), step))
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
    fn crammed_code_is_split_into_lines() {
        let src = "package model;\nimport java.util.*;\npublic class Foo{\nprivate int a;private String b;\n@Override public String toString(){return \"x\";}\npublic Foo(int a,String b){this.a=a;this.b=b;}\npublic void loop()\n{\nfor(int i=0;i<10;i++){if(i%2==0){System.out.println(i);}else{System.out.println(-i);}}\nint[] z={1,2,3};\nswitch(a){case 1: b=\"eins\";break;default: b=\"?\";}\n}\npublic void x() throws Exception {}\n}\n";
        let want = "package model;\n\nimport java.util.*;\n\npublic class Foo {\n    private int a;\n    private String b;\n    @Override\n    public String toString() {\n        return \"x\";\n    }\n\n    public Foo(int a, String b) {\n        this.a = a;\n        this.b = b;\n    }\n\n    public void loop() {\n        for (int i = 0; i < 10; i++) {\n            if (i % 2 == 0) {\n                System.out.println(i);\n            } else {\n                System.out.println(-i);\n            }\n        }\n        int[] z = {1, 2, 3};\n        switch (a) {\n            case 1:\n                b = \"eins\";\n                break;\n            default:\n                b = \"?\";\n        }\n    }\n\n    public void x() throws Exception {}\n}\n";
        assert_eq!(format(src, "    "), want);
        assert_eq!(format(want, "    "), want);
        // throws in der naechsten Zeile wird doppelt eingerueckt, Lambda-Bloecke in Aufrufen bleiben richtig
        let t = format("class A {\nvoid m()\nthrows X {\nlist.forEach(x -> {a(x);b(x);});\n}\n}\n", "    ");
        assert!(t.contains("    void m()\n            throws X {\n"), "{t}");
        assert!(t.contains("list.forEach(x -> {\n            a(x);\n            b(x);\n        });"), "{t}");
        let c = format("class A {\nvoid m() {\nx = f(\n1,\n2\n);\n}\n}\n", "    ");
        assert!(c.contains("        x = f(\n                1,\n                2\n        );"), "{c}");
    }

    #[test]
    fn already_formatted_code_stays_the_same() {
        let src = "package app;\n\nimport java.util.*;\n\npublic class Main {\n    public static void main(String[] args) {\n        for (int i = 0; i < 3; i++) {\n            System.out.println(i + \" \" + args.length);\n        }\n    }\n}\n";
        assert_eq!(format(src, "    "), src);
    }
}


