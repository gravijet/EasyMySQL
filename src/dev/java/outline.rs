// Aufbau einer Java-Datei ohne Compiler: Klassen (auch enum, record, interface), Felder, Methoden,
// Konstruktoren, Enum-Werte mit Javadoc und lokale Variablen. Grundlage fuer Vervollstaendigung nach dem
// Punkt, Hinweise beim Zeigen, Getter/Setter erzeugen und Umbenennen, solange der Sprachserver nicht hilft.
// Alle Positionen sind Zeichenindizes.

use super::source::{self, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tok {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// Woerter und Zeichen des Codes (ohne Kommentare und Zeichenketten; diese werden zu einem Platzhalter "\"")
pub fn tokens(chars: &[char]) -> Vec<Tok> {
    let mut out = Vec::new();
    for (kind, a, b) in source::spans(chars) {
        if kind != Span::Code {
            if matches!(kind, Span::Str | Span::Char) {
                out.push(Tok { text: "\"".into(), start: a, end: b });
            }
            continue;
        }
        let mut i = a;
        while i < b {
            let c = chars[i];
            if c.is_whitespace() {
                i += 1;
            } else if c.is_alphabetic() || c == '_' || c == '$' {
                let s = i;
                while i < b && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                    i += 1;
                }
                out.push(Tok { text: chars[s..i].iter().collect(), start: s, end: i });
            } else if c.is_ascii_digit() {
                let s = i;
                while i < b && (chars[i].is_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok { text: chars[s..i].iter().collect(), start: s, end: i });
            } else {
                out.push(Tok { text: c.to_string(), start: i, end: i + 1 });
                i += 1;
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Field,
    Method,
    Constructor,
    Constant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Typ des Feldes bzw. Rueckgabetyp der Methode
    pub ty: String,
    pub kind: Kind,
    /// Parameter wie geschrieben: "int a, String b"
    pub params: String,
    pub is_static: bool,
    pub is_final: bool,
    /// Lage des Namens
    pub at: usize,
    pub doc: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    pub name: String,
    /// "class", "enum", "record" oder "interface"
    pub kind: String,
    pub at: usize,
    /// Geschweifte Klammern des Rumpfes
    pub body: (usize, usize),
    pub members: Vec<Member>,
    pub doc: Option<String>,
}

const MODIFIERS: &[&str] = &["public", "private", "protected", "static", "final", "abstract", "synchronized", "native", "transient", "volatile", "default", "strictfp", "sealed", "non-sealed"];

/// Javadoc direkt vor `pos` (nur Leerraum und Anmerkungen dazwischen), ohne * und Einrueckung
fn doc_before(chars: &[char], spans: &[(Span, usize, usize)], pos: usize) -> Option<String> {
    let (_, a, b) = spans.iter().rev().find(|(k, _, b)| *k == Span::Comment && *b <= pos)?;
    let between: String = chars[*b..pos].iter().collect();
    // Dazwischen nur Leerraum, Anmerkungen und Modifikatoren
    if between.contains(';') || between.contains('{') || between.contains('}') {
        return None;
    }
    let text: String = chars[*a..*b].iter().collect();
    let body = text.strip_prefix("/**")?.strip_suffix("*/")?;
    let lines: Vec<String> = body.lines().map(|l| l.trim().trim_start_matches('*').trim().to_string()).filter(|l| !l.is_empty()).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Typ aus Token zusammensetzen (List<String>, int[], Map.Entry<K, V>)
fn type_text(toks: &[Tok]) -> String {
    let mut s = String::new();
    for t in toks {
        if t.text == "," {
            s.push_str(", ");
        } else {
            s.push_str(&t.text);
        }
    }
    s
}

/// Ende einer Typangabe ab `i` (inklusive Generics und []), sonst None
fn skip_type(toks: &[Tok], mut i: usize) -> Option<usize> {
    let first = toks.get(i)?;
    if !first.text.starts_with(|c: char| c.is_alphabetic() || c == '_') {
        return None;
    }
    i += 1;
    loop {
        match toks.get(i).map(|t| t.text.as_str()) {
            Some(".") if toks.get(i + 1).is_some_and(|t| t.text.starts_with(|c: char| c.is_alphabetic())) => i += 2,
            Some("<") => {
                let mut depth = 0;
                while let Some(t) = toks.get(i) {
                    match t.text.as_str() {
                        "<" => depth += 1,
                        ">" => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        ";" | "{" | "}" | "=" => return None,
                        _ => {}
                    }
                    i += 1;
                }
            }
            Some("[") if toks.get(i + 1).is_some_and(|t| t.text == "]") => i += 2,
            Some("...") => i += 1,
            _ => return Some(i),
        }
    }
}

/// Alle Klassen der Datei mit ihren Mitgliedern
pub fn outline(text: &str) -> Vec<Class> {
    let chars: Vec<char> = text.chars().collect();
    let spans = source::spans(&chars);
    let toks = tokens(&chars);
    let mut classes: Vec<Class> = Vec::new();
    // Offene Klassen: (Index in `classes`, Tiefe der geschweiften Klammer des Rumpfes)
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0usize;
    // Beginn der aktuellen Anweisung auf Klassenebene
    let mut stmt = 0usize;
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        match t.text.as_str() {
            "class" | "enum" | "record" | "interface" if toks.get(i + 1).is_some_and(|n| source::is_identifier(&n.text)) && !(i > 0 && toks[i - 1].text == ".") => {
                let name = toks[i + 1].text.clone();
                let kind = t.text.clone();
                let at = toks[i + 1].start;
                // Rumpf suchen; bei record die Komponenten als Felder
                let mut j = i + 2;
                let mut components = Vec::new();
                if kind == "record" && toks.get(j).is_some_and(|t| t.text == "<") {
                    j = skip_type(&toks, i + 1).unwrap_or(j);
                }
                if kind == "record" && toks.get(j).is_some_and(|t| t.text == "(") {
                    let mut k = j + 1;
                    let mut start = k;
                    let mut pd = 0;
                    while k < toks.len() {
                        match toks[k].text.as_str() {
                            "(" | "<" => pd += 1,
                            ")" if pd == 0 => break,
                            ")" | ">" => pd -= 1,
                            "," if pd == 0 => {
                                components.push(start..k);
                                start = k + 1;
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    components.push(start..k);
                    j = k;
                }
                while j < toks.len() && toks[j].text != "{" && toks[j].text != ";" {
                    j += 1;
                }
                if j >= toks.len() || toks[j].text != "{" {
                    i = j;
                    continue;
                }
                let doc = doc_before(&chars, &spans, toks[stmt.min(i)].start);
                let mut members = Vec::new();
                for r in components {
                    let part = &toks[r];
                    if part.len() >= 2 {
                        let name_tok = &part[part.len() - 1];
                        members.push(Member { name: name_tok.text.clone(), ty: type_text(&part[..part.len() - 1]), kind: Kind::Field, params: String::new(), is_static: false, is_final: true, at: name_tok.start, doc: None });
                    }
                }
                classes.push(Class { name, kind, at, body: (toks[j].start, chars.len()), members, doc });
                depth += 1;
                open.push((classes.len() - 1, depth));
                i = j + 1;
                stmt = i;
                // Enum-Werte bis zum ersten ; auf Rumpfebene
                if classes.last().is_some_and(|c| c.kind == "enum") {
                    let ci = classes.len() - 1;
                    let mut pd = 0;
                    let mut expect = true;
                    while i < toks.len() {
                        let x = &toks[i];
                        match x.text.as_str() {
                            "(" | "{" => pd += 1,
                            ")" | "}" if pd > 0 => pd -= 1,
                            "}" => break,
                            ";" if pd == 0 => {
                                i += 1;
                                break;
                            }
                            "," if pd == 0 => expect = true,
                            "@" => {
                                i += 1;
                            }
                            w if expect && pd == 0 && source::is_identifier(w) => {
                                let doc = doc_before(&chars, &spans, x.start);
                                let ty = classes[ci].name.clone();
                                classes[ci].members.push(Member { name: w.into(), ty, kind: Kind::Constant, params: String::new(), is_static: true, is_final: true, at: x.start, doc });
                                expect = false;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    stmt = i;
                }
                continue;
            }
            "{" => {
                // Methodenrumpf oder Initialisierer: ueberspringen bis zur passenden Klammer
                let in_class = open.last().is_some_and(|(_, d)| *d == depth);
                if in_class {
                    let (ci, _) = *open.last().unwrap();
                    if let Some(m) = method_at(&toks[stmt..i], &classes[ci].name, &chars, &spans) {
                        classes[ci].members.push(m);
                    }
                }
                depth += 1;
                // Bis zum Ende dieses Blocks springen, ausser es folgen innere Klassen (dann normal weiter)
                if in_class {
                    let mut d = 1;
                    let mut k = i + 1;
                    let mut inner = false;
                    while k < toks.len() {
                        match toks[k].text.as_str() {
                            "{" => d += 1,
                            "}" => {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                            "class" | "enum" | "record" | "interface" if d == 1 && toks.get(k + 1).is_some_and(|n| source::is_identifier(&n.text)) && toks[k - 1].text != "." => inner = true,
                            _ => {}
                        }
                        k += 1;
                    }
                    if !inner {
                        depth -= 1;
                        i = k + 1;
                        stmt = i;
                        continue;
                    }
                }
                i += 1;
                stmt = i;
                continue;
            }
            "}" => {
                if open.last().is_some_and(|(_, d)| *d == depth) {
                    let (ci, _) = open.pop().unwrap();
                    classes[ci].body.1 = t.start + 1;
                }
                depth = depth.saturating_sub(1);
                i += 1;
                stmt = i;
                continue;
            }
            ";" => {
                if let Some(&(ci, d)) = open.last()
                    && d == depth
                {
                    let part = &toks[stmt..i];
                    if let Some(m) = method_at(part, &classes[ci].name, &chars, &spans) {
                        classes[ci].members.push(m);
                    } else {
                        for f in fields_of(part, &chars, &spans) {
                            classes[ci].members.push(f);
                        }
                    }
                }
                i += 1;
                stmt = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    classes
}

/// Modifikatoren und Anmerkungen am Anfang einer Anweisung ueberspringen
fn skip_modifiers(part: &[Tok]) -> (usize, bool, bool) {
    let mut k = 0;
    let (mut st, mut fin) = (false, false);
    while k < part.len() {
        let w = part[k].text.as_str();
        if w == "@" {
            // @Override, @Column(name = "x")
            k += 2;
            if part.get(k).is_some_and(|t| t.text == "(") {
                let mut d = 0;
                while k < part.len() {
                    match part[k].text.as_str() {
                        "(" => d += 1,
                        ")" => {
                            d -= 1;
                            if d == 0 {
                                k += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
            }
            continue;
        }
        if MODIFIERS.contains(&w) {
            st |= w == "static";
            fin |= w == "final";
            k += 1;
            continue;
        }
        break;
    }
    (k, st, fin)
}

/// Methode oder Konstruktor, wenn die Anweisung so aussieht
fn method_at(part: &[Tok], class: &str, chars: &[char], spans: &[(Span, usize, usize)]) -> Option<Member> {
    let (mut k, st, fin) = skip_modifiers(part);
    // Typparameter <T>
    if part.get(k).is_some_and(|t| t.text == "<") {
        while k < part.len() && part[k].text != ">" {
            k += 1;
        }
        k += 1;
    }
    let first = part.get(k)?;
    let doc = doc_before(chars, spans, part.first()?.start);
    // Konstruktor: Klassenname direkt vor (
    if first.text == class && part.get(k + 1).is_some_and(|t| t.text == "(") {
        let params = params_text(chars, &part[k + 1..])?;
        return Some(Member { name: class.into(), ty: class.into(), kind: Kind::Constructor, params, is_static: false, is_final: false, at: first.start, doc });
    }
    let end = skip_type(part, k)?;
    let ty = type_text(&part[k..end]);
    let name = part.get(end)?;
    if !source::is_identifier(&name.text) || !part.get(end + 1).is_some_and(|t| t.text == "(") || ty == "new" || ty == "return" {
        return None;
    }
    let params = params_text(chars, &part[end + 1..])?;
    Some(Member { name: name.text.clone(), ty, kind: Kind::Method, params, is_static: st, is_final: fin, at: name.start, doc })
}

/// Text zwischen ( und der passenden )
fn params_text(chars: &[char], from_paren: &[Tok]) -> Option<String> {
    let open = from_paren.first()?;
    let mut d = 0;
    for t in from_paren {
        match t.text.as_str() {
            "(" => d += 1,
            ")" => {
                d -= 1;
                if d == 0 {
                    let s: String = chars[open.end..t.start].iter().collect();
                    return Some(s.split_whitespace().collect::<Vec<_>>().join(" "));
                }
            }
            _ => {}
        }
    }
    None
}

/// Felder einer Anweisung `private int a = 1, b;`
fn fields_of(part: &[Tok], chars: &[char], spans: &[(Span, usize, usize)]) -> Vec<Member> {
    let (k, st, fin) = skip_modifiers(part);
    let Some(end) = skip_type(part, k) else { return Vec::new() };
    let ty = type_text(&part[k..end]);
    if matches!(ty.as_str(), "return" | "package" | "import" | "throw" | "new") {
        return Vec::new();
    }
    let doc = part.first().and_then(|f| doc_before(chars, spans, f.start));
    let mut out = Vec::new();
    let mut j = end;
    let mut depth = 0;
    let mut expect = true;
    while j < part.len() {
        let w = part[j].text.as_str();
        match w {
            "(" | "{" | "[" | "<" => depth += 1,
            ")" | "}" | "]" | ">" => depth -= 1,
            "," if depth == 0 => expect = true,
            "=" if depth == 0 => expect = false,
            _ if expect && depth == 0 && source::is_identifier(w) => {
                out.push(Member { name: w.into(), ty: ty.clone(), kind: Kind::Field, params: String::new(), is_static: st, is_final: fin, at: part[j].start, doc: doc.clone() });
                expect = false;
            }
            _ => {}
        }
        j += 1;
    }
    out
}

/// Innerste Klasse an `pos`
pub fn class_at(classes: &[Class], pos: usize) -> Option<&Class> {
    classes.iter().filter(|c| c.body.0 <= pos && pos <= c.body.1).min_by_key(|c| c.body.1 - c.body.0)
}

/// Lokale Variablen und Parameter vor `pos` (Name, Typ, Lage): `int x = 1`, `for (String s : l)`, `catch (Exception e)`
pub fn locals(text: &str, pos: usize) -> Vec<(String, String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let toks = tokens(&chars);
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() && toks[k].start < pos {
        if let Some(end) = skip_type(&toks, k)
            && end > k
            && let Some(name) = toks.get(end)
            && source::is_identifier(&name.text)
            && toks.get(end + 1).is_some_and(|t| matches!(t.text.as_str(), "=" | ";" | ":" | "," | ")"))
            && name.start < pos
        {
            let ty = type_text(&toks[k..end]);
            let before = if k > 0 { toks[k - 1].text.as_str() } else { "" };
            // Kein Ausdruck wie `a = b;` oder `return x;`
            if !matches!(ty.as_str(), "return" | "new" | "throw" | "else" | "case" | "package" | "import" | "yield")
                && !matches!(before, "." | "=" | "+" | "-" | "*" | "/" | "return" | "new" | "<" | "?" | "!")
            {
                out.push((name.text.clone(), ty, name.start));
            }
            k = end + 1;
            continue;
        }
        k += 1;
    }
    out
}

/// Text fuer eine Methode: "int summe(int a, int b)"
pub fn signature(m: &Member) -> String {
    match m.kind {
        Kind::Method => format!("{} {}({})", m.ty, m.name, m.params),
        Kind::Constructor => format!("{}({})", m.name, m.params),
        Kind::Field | Kind::Constant => format!("{} {}", m.ty, m.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "package app;\n\nimport java.util.*;\n\n/** Ein Kunde. */\npublic class Kunde {\n    /** Die Farben. */\n    enum Farbe { ROT, GRUEN(2), BLAU; int x; }\n\n    private final int id;\n    /** Vor- und Nachname */\n    private String name = \"a;b\", ort;\n    static List<Map<String, Integer>> liste = new ArrayList<>();\n\n    public Kunde(int id, String name) {\n        this.id = id;\n        int lokal = 3;\n        for (String s : List.of(\"x\")) {}\n    }\n\n    /** Summe zweier Zahlen. */\n    @Override\n    public static int summe(int a, int b) { return a + b; }\n\n    abstract void leer();\n\n    record Punkt(int x, int y) {}\n}\n";

    #[test]
    fn classes_fields_methods_and_enums() {
        let cs = outline(SRC);
        let k = cs.iter().find(|c| c.name == "Kunde").unwrap();
        assert_eq!(k.doc.as_deref(), Some("Ein Kunde."));
        let names: Vec<(&str, Kind)> = k.members.iter().map(|m| (m.name.as_str(), m.kind)).collect();
        assert!(names.contains(&("id", Kind::Field)) && names.contains(&("name", Kind::Field)) && names.contains(&("ort", Kind::Field)) && names.contains(&("liste", Kind::Field)), "{names:?}");
        assert!(names.contains(&("Kunde", Kind::Constructor)) && names.contains(&("summe", Kind::Method)) && names.contains(&("leer", Kind::Method)), "{names:?}");
        assert!(!names.iter().any(|(n, _)| *n == "lokal" || *n == "s"), "{names:?}");
        let summe = k.members.iter().find(|m| m.name == "summe").unwrap();
        assert_eq!((summe.ty.as_str(), summe.params.as_str(), summe.is_static), ("int", "int a, int b", true));
        assert_eq!(summe.doc.as_deref(), Some("Summe zweier Zahlen."));
        assert_eq!(k.members.iter().find(|m| m.name == "name").unwrap().doc.as_deref(), Some("Vor- und Nachname"));
        assert_eq!(k.members.iter().find(|m| m.name == "liste").unwrap().ty, "List<Map<String, Integer>>");
        let f = cs.iter().find(|c| c.name == "Farbe").unwrap();
        let consts: Vec<&str> = f.members.iter().filter(|m| m.kind == Kind::Constant).map(|m| m.name.as_str()).collect();
        assert_eq!(consts, vec!["ROT", "GRUEN", "BLAU"]);
        assert!(f.members.iter().any(|m| m.name == "x" && m.kind == Kind::Field));
        let p = cs.iter().find(|c| c.name == "Punkt").unwrap();
        assert_eq!(p.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), vec!["x", "y"]);
        let pos = SRC.find("this.id").unwrap();
        assert_eq!(class_at(&cs, SRC[..pos].chars().count()).map(|c| c.name.as_str()), Some("Kunde"));
    }

    #[test]
    fn local_variables_before_the_cursor() {
        let at = SRC[..SRC.find("for (").unwrap()].chars().count() + 30;
        let l = locals(SRC, at);
        assert!(l.iter().any(|(n, t, _)| n == "lokal" && t == "int"), "{l:?}");
        assert!(l.iter().any(|(n, t, _)| n == "s" && t == "String"), "{l:?}");
        assert!(l.iter().any(|(n, t, _)| n == "name" && t == "String"), "{l:?}");
        assert!(!l.iter().any(|(n, _, _)| n == "id" && false));
    }
}
