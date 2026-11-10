// Umbauten im Editor ohne Sprachserver (wie Alt+Einfg und Strg+Alt+T in IntelliJ): Getter/Setter,
// Konstruktor, toString, equals/hashCode, Code mit try/catch, if, for umgeben, Variable einfuehren,
// in der Datei umbenennen, Gross/klein, Zeilen verbinden und sortieren. Jede Funktion liefert den neuen
// Text und die Cursorstelle (Zeichenindex).

use crate::dev::java::outline::{self, Kind, Member};
use crate::dev::java::source::{self, Span};

/// Was "Generieren" erzeugen kann
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generate {
    GettersSetters,
    Getters,
    Setters,
    Constructor,
    ToString,
    EqualsHashCode,
}

impl Generate {
    pub const ALL: [Generate; 6] = [Generate::GettersSetters, Generate::Getters, Generate::Setters, Generate::Constructor, Generate::ToString, Generate::EqualsHashCode];

    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Generate::GettersSetters => tr("Getter und Setter", "Getters and setters"),
            Generate::Getters => "Getter",
            Generate::Setters => "Setter",
            Generate::Constructor => tr("Konstruktor mit allen Feldern", "Constructor with all fields"),
            Generate::ToString => "toString()",
            Generate::EqualsHashCode => "equals() und hashCode()",
        }
    }
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn byte_of(text: &str, ci: usize) -> usize {
    text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
}

/// Einrueckung der Zeile, in der `pos` liegt
fn indent_at(text: &str, pos: usize) -> String {
    let c = chars(text);
    let start = c[..pos.min(c.len())].iter().rposition(|x| *x == '\n').map_or(0, |i| i + 1);
    c[start..].iter().take_while(|x| **x == ' ' || **x == '\t').collect()
}

fn capitalized(name: &str) -> String {
    let mut c = name.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Felder (ohne static) der Klasse am Cursor
fn instance_fields(class: &outline::Class) -> Vec<&Member> {
    class.members.iter().filter(|m| m.kind == Kind::Field && !m.is_static).collect()
}

fn has_method(class: &outline::Class, name: &str) -> bool {
    class.members.iter().any(|m| m.kind == Kind::Method && m.name == name)
}

/// Code fuer die Klasse am Cursor erzeugen und vor ihrer schliessenden Klammer einfuegen.
/// Liefert None, wenn es nichts zu erzeugen gibt (keine Felder, alles schon da).
pub fn generate(text: &str, cursor: usize, what: Generate, step: &str) -> Option<(String, usize)> {
    let classes = outline::outline(text);
    let class = outline::class_at(&classes, cursor)?;
    let fields = instance_fields(class);
    let base = indent_at(text, class.body.0);
    let ind = format!("{base}{step}");
    let body = format!("{ind}{step}");
    let mut code = String::new();
    let mut push = |s: String| {
        if !code.is_empty() {
            code.push('\n');
        }
        code.push_str(&s);
    };
    let n = &class.name;
    match what {
        Generate::GettersSetters | Generate::Getters | Generate::Setters => {
            for f in &fields {
                let cap = capitalized(&f.name);
                let getter = if f.ty == "boolean" { format!("is{cap}") } else { format!("get{cap}") };
                if what != Generate::Setters && !has_method(class, &getter) {
                    push(format!("{ind}public {} {getter}() {{\n{body}return {};\n{ind}}}\n", f.ty, f.name));
                }
                let setter = format!("set{cap}");
                if what != Generate::Getters && !f.is_final && !has_method(class, &setter) {
                    push(format!("{ind}public void {setter}({} {}) {{\n{body}this.{} = {};\n{ind}}}\n", f.ty, f.name, f.name, f.name));
                }
            }
        }
        Generate::Constructor => {
            if fields.is_empty() {
                return None;
            }
            let params: Vec<String> = fields.iter().map(|f| format!("{} {}", f.ty, f.name)).collect();
            let sets: String = fields.iter().map(|f| format!("{body}this.{} = {};\n", f.name, f.name)).collect();
            push(format!("{ind}public {n}({}) {{\n{sets}{ind}}}\n", params.join(", ")));
        }
        Generate::ToString => {
            if has_method(class, "toString") {
                return None;
            }
            let parts: Vec<String> = fields.iter().enumerate().map(|(i, f)| {
                let sep = if i == 0 { "" } else { ", " };
                if f.ty == "String" { format!("\"{sep}{}='\" + {} + '\\''", f.name, f.name) } else { format!("\"{sep}{}=\" + {}", f.name, f.name) }
            }).collect();
            let inner = if parts.is_empty() { String::new() } else { format!(" +\n{body}        {}", parts.join(" +\n                ")) };
            push(format!("{ind}@Override\n{ind}public String toString() {{\n{body}return \"{n}{{\"{inner} +\n{body}        '}}';\n{ind}}}\n"));
        }
        Generate::EqualsHashCode => {
            if has_method(class, "equals") || fields.is_empty() {
                return None;
            }
            let cmp: Vec<String> = fields.iter().map(|f| {
                if matches!(f.ty.as_str(), "int" | "long" | "short" | "byte" | "char" | "boolean" | "double" | "float") {
                    format!("{} == other.{}", f.name, f.name)
                } else {
                    format!("java.util.Objects.equals({}, other.{})", f.name, f.name)
                }
            }).collect();
            let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
            push(format!("{ind}@Override\n{ind}public boolean equals(Object o) {{\n{body}if (this == o) return true;\n{body}if (!(o instanceof {n} other)) return false;\n{body}return {};\n{ind}}}\n", cmp.join("\n{body}        && ").replace("{body}", &body)));
            push(format!("{ind}@Override\n{ind}public int hashCode() {{\n{body}return java.util.Objects.hash({});\n{ind}}}\n", names.join(", ")));
        }
    }
    if code.is_empty() {
        return None;
    }
    // Vor der schliessenden Klammer der Klasse, mit einer Leerzeile Abstand
    let close = class.body.1.saturating_sub(1);
    let c = chars(text);
    let mut at = close;
    while at > 0 && (c[at - 1] == ' ' || c[at - 1] == '\t') {
        at -= 1;
    }
    let before: String = c[..at].iter().collect();
    let gap = if before.trim_end().ends_with('{') { "" } else { "\n" };
    let insert = format!("{gap}{code}");
    let b = byte_of(text, at);
    let mut out = text.to_string();
    out.insert_str(b, &insert);
    let cursor = at + gap.chars().count() + ind.chars().count();
    Some((out, cursor))
}

/// Womit sich Code umgeben laesst
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surround {
    TryCatch,
    TryFinally,
    If,
    IfElse,
    For,
    ForEach,
    While,
    Block,
}

impl Surround {
    pub const ALL: [Surround; 8] = [Surround::TryCatch, Surround::TryFinally, Surround::If, Surround::IfElse, Surround::For, Surround::ForEach, Surround::While, Surround::Block];

    pub fn label(self) -> &'static str {
        match self {
            Surround::TryCatch => "try / catch",
            Surround::TryFinally => "try / finally",
            Surround::If => "if",
            Surround::IfElse => "if / else",
            Surround::For => "for (int i …)",
            Surround::ForEach => "for (… : …)",
            Surround::While => "while",
            Surround::Block => "{ }",
        }
    }
}

/// Zeilen `first..=last` (0-basiert) mit einem Block umgeben. Der Cursor landet in der Bedingung bzw. im catch.
pub fn surround(text: &str, first: usize, last: usize, with: Surround, step: &str) -> (String, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = last.min(lines.len().saturating_sub(1));
    let ind: String = lines.get(first).map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect()).unwrap_or_default();
    let inner: Vec<String> = lines[first..=last].iter().map(|l| if l.trim().is_empty() { String::new() } else { format!("{step}{l}") }).collect();
    let inner = inner.join("\n");
    let (head, tail, mark) = match with {
        Surround::TryCatch => ("try {".to_string(), format!("}} catch (Exception e) {{\n{ind}{step}e.printStackTrace();\n{ind}}}"), "Exception"),
        Surround::TryFinally => ("try {".to_string(), format!("}} finally {{\n{ind}{step}\n{ind}}}"), "finally"),
        Surround::If => ("if (true) {".to_string(), "}".to_string(), "true"),
        Surround::IfElse => ("if (true) {".to_string(), format!("}} else {{\n{ind}{step}\n{ind}}}"), "true"),
        Surround::For => ("for (int i = 0; i < 10; i++) {".to_string(), "}".to_string(), "10"),
        Surround::ForEach => ("for (var item : liste) {".to_string(), "}".to_string(), "liste"),
        Surround::While => ("while (true) {".to_string(), "}".to_string(), "true"),
        Surround::Block => ("{".to_string(), "}".to_string(), "{"),
    };
    let block = format!("{ind}{head}\n{inner}\n{ind}{tail}");
    let mut out: Vec<String> = lines[..first].iter().map(|s| s.to_string()).collect();
    let start: usize = out.iter().map(|l| l.chars().count() + 1).sum();
    out.push(block.clone());
    out.extend(lines[last + 1..].iter().map(|s| s.to_string()));
    // Cursor: auf die Stelle, die man meist anpasst (Bedingung, Ausnahme)
    let pos = match with {
        Surround::TryCatch => block.rfind(mark).map(|b| block[..b].chars().count()),
        Surround::Block => None,
        _ => block.find(mark).map(|b| block[..b].chars().count()),
    };
    (out.join("\n"), start + pos.unwrap_or(ind.chars().count() + head.chars().count()))
}

/// Markierten Ausdruck in eine Variable auslagern: `var wert = <Ausdruck>;` in der Zeile davor
pub fn introduce_variable(text: &str, a: usize, b: usize, name: &str) -> Option<(String, usize, usize)> {
    let c = chars(text);
    let expr: String = c.get(a..b)?.iter().collect();
    let expr = expr.trim();
    if expr.is_empty() || expr.contains('\n') || expr.ends_with(';') {
        return None;
    }
    let line_start = c[..a].iter().rposition(|x| *x == '\n').map_or(0, |i| i + 1);
    let ind = indent_at(text, a);
    let decl = format!("{ind}var {name} = {expr};\n");
    let mut out = String::new();
    out.extend(&c[..line_start]);
    out.push_str(&decl);
    out.extend(&c[line_start..a]);
    out.push_str(name);
    out.extend(&c[b..]);
    // Name markiert lassen, damit man ihn gleich ueberschreiben kann
    let s = line_start + ind.chars().count() + 4;
    Some((out, s, s + name.chars().count()))
}

/// Alle Vorkommen des Namens im Code (nicht in Zeichenketten und Kommentaren), auch `this.name`
pub fn occurrences(text: &str, name: &str) -> Vec<(usize, usize)> {
    let c = chars(text);
    let spans = source::spans(&c);
    let mut out = Vec::new();
    for t in outline::tokens(&c) {
        if t.text == name && spans.iter().any(|(k, a, b)| *k == Span::Code && *a <= t.start && t.end <= *b) {
            out.push((t.start, t.end));
        }
    }
    out
}

/// In der ganzen Datei umbenennen (Code, nicht Texte)
pub fn rename_in_file(text: &str, old: &str, new: &str) -> String {
    let c = chars(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (a, b) in occurrences(text, old) {
        out.extend(&c[last..a]);
        out.push_str(new);
        last = b;
    }
    out.extend(&c[last..]);
    out
}

/// Gross-/Kleinschreibung umschalten: klein -> GROSS, sonst -> klein
pub fn toggle_case(s: &str) -> String {
    if s.chars().any(|c| c.is_lowercase()) { s.to_uppercase() } else { s.to_lowercase() }
}

/// Zeilen `first..=last` zu einer verbinden (wie Strg+Umschalt+J in IntelliJ)
pub fn join_lines(text: &str, first: usize, last: usize) -> (String, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = if last == first { (first + 1).min(lines.len() - 1) } else { last };
    let mut joined = lines[first].trim_end().to_string();
    for l in &lines[first + 1..=last] {
        let t = l.trim();
        if !t.is_empty() {
            joined.push(' ');
            joined.push_str(t);
        }
    }
    let start: usize = lines[..first].iter().map(|l| l.chars().count() + 1).sum();
    let cursor = start + lines[first].trim_end().chars().count();
    let mut out: Vec<String> = lines[..first].iter().map(|s| s.to_string()).collect();
    out.push(joined);
    out.extend(lines[last + 1..].iter().map(|s| s.to_string()));
    (out.join("\n"), cursor)
}

/// Zeilen `first..=last` sortieren
pub fn sort_lines(text: &str, first: usize, last: usize) -> String {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let last = last.min(lines.len() - 1);
    lines[first..=last].sort_by_key(|l| l.trim().to_lowercase());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "public class Kunde {\n    private int id;\n    private String name;\n    private boolean aktiv;\n    private final String ort = \"x\";\n    static int zaehler;\n}\n";

    #[test]
    fn getters_and_setters_for_all_fields() {
        let (out, cur) = generate(SRC, 30, Generate::GettersSetters, "    ").unwrap();
        assert!(out.contains("    public int getId() {\n        return id;\n    }\n"), "{out}");
        assert!(out.contains("    public void setName(String name) {\n        this.name = name;\n    }\n"), "{out}");
        assert!(out.contains("public boolean isAktiv()"));
        assert!(out.contains("public String getOrt()") && !out.contains("setOrt"), "final hat keinen Setter");
        assert!(!out.contains("getZaehler"), "static nicht");
        assert!(out.ends_with("    }\n}\n"), "{out}");
        assert_eq!(out.chars().nth(cur), Some('p'));
        // Zweimal erzeugen: nichts Doppeltes
        assert!(generate(&out, 30, Generate::GettersSetters, "    ").is_none());
    }

    #[test]
    fn constructor_to_string_and_equals() {
        let (out, _) = generate(SRC, 30, Generate::Constructor, "    ").unwrap();
        assert!(out.contains("    public Kunde(int id, String name, boolean aktiv, String ort) {\n        this.id = id;\n"), "{out}");
        let (out, _) = generate(SRC, 30, Generate::ToString, "    ").unwrap();
        assert!(out.contains("@Override\n    public String toString() {\n        return \"Kunde{\" +\n"), "{out}");
        assert!(out.contains("\"id=\" + id") && out.contains("\", name='\" + name + '\\''"), "{out}");
        let (out, _) = generate(SRC, 30, Generate::EqualsHashCode, "    ").unwrap();
        assert!(out.contains("if (!(o instanceof Kunde other)) return false;") && out.contains("id == other.id") && out.contains("java.util.Objects.hash(id, name, aktiv, ort)"), "{out}");
    }

    #[test]
    fn surround_and_introduce_variable() {
        let src = "void m() {\n    a();\n    b();\n}";
        let (out, cur) = surround(src, 1, 2, Surround::TryCatch, "    ");
        assert_eq!(out, "void m() {\n    try {\n        a();\n        b();\n    } catch (Exception e) {\n        e.printStackTrace();\n    }\n}");
        assert!(out[out.char_indices().nth(cur).unwrap().0..].starts_with("Exception e"));
        let (out, cur) = surround(src, 1, 1, Surround::If, "    ");
        assert!(out.starts_with("void m() {\n    if (true) {\n        a();\n    }\n    b();"), "{out}");
        assert!(out[cur..].starts_with("true"));
        let src = "    int x = a * b + 1;";
        let a = src.find("a * b").unwrap();
        let (out, s, e) = introduce_variable(src, a, a + 5, "wert").unwrap();
        assert_eq!(out, "    var wert = a * b;\n    int x = wert + 1;");
        assert_eq!(&out[s..e], "wert");
    }

    #[test]
    fn rename_case_join_and_sort() {
        let src = "int zahl = 1; // zahl\nString s = \"zahl\"; this.zahl = zahl + zahlen;";
        assert_eq!(rename_in_file(src, "zahl", "wert"), "int wert = 1; // zahl\nString s = \"zahl\"; this.wert = wert + zahlen;");
        assert_eq!(toggle_case("select"), "SELECT");
        assert_eq!(toggle_case("SELECT"), "select");
        assert_eq!(join_lines("a(\n    b,\n    c);\nd", 0, 2).0, "a( b, c);\nd");
        assert_eq!(sort_lines("c\nB\na\nz", 0, 2), "a\nB\nc\nz");
    }
}
