// CSV-Dateien lesen: Trennzeichen und Kodierung werden erkannt, die erste Zeile ist die Kopfzeile.

use super::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Windows-1252 fuer Dateien, die kein UTF-8 sind (Excel)
fn cp1252(b: &[u8]) -> String {
    const HIGH: [char; 32] = ['€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ'];
    b.iter().map(|&c| if (0x80..0xA0).contains(&c) { HIGH[(c - 0x80) as usize] } else { c as char }).collect()
}

fn read_text(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).and_then(|mut f| f.read_to_end(&mut bytes)).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => cp1252(bytes),
    })
}

/// Trennzeichen der ersten Zeile (ausserhalb von Anfuehrungszeichen)
pub fn detect_delimiter(text: &str) -> char {
    let mut counts = [(';', 0), (',', 0), ('\t', 0), ('|', 0)];
    let mut quoted = false;
    for c in text.chars().take(8000) {
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => break,
            c if !quoted => {
                if let Some(e) = counts.iter_mut().find(|e| e.0 == c) {
                    e.1 += 1;
                }
            }
            _ => {}
        }
    }
    counts.iter().max_by_key(|e| e.1).filter(|e| e.1 > 0).map(|e| e.0).unwrap_or(';')
}

/// Zerlegt Text in Datensaetze (Anfuehrungszeichen, doppelte Anfuehrungszeichen, Zeilenumbrueche in Feldern)
pub fn records(text: &str, delim: char, mut f: impl FnMut(Vec<String>) -> bool) {
    let mut row: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut had = false;
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if quoted {
            if c == '"' {
                if it.peek() == Some(&'"') {
                    it.next();
                    cur.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' if cur.is_empty() => {
                quoted = true;
                had = true;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut cur));
                had = true;
            }
            '\r' => {}
            '\n' => {
                if had || !cur.is_empty() {
                    row.push(std::mem::take(&mut cur));
                    if !f(std::mem::take(&mut row)) {
                        return;
                    }
                }
                had = false;
            }
            c => {
                cur.push(c);
                had = true;
            }
        }
    }
    if had || !cur.is_empty() {
        row.push(cur);
        f(row);
    }
}

fn decimal_comma(s: &str, delim: char) -> Option<f64> {
    if delim == ',' {
        return None;
    }
    let (a, b) = s.split_once(',')?;
    let a = a.strip_prefix('-').unwrap_or(a);
    (!a.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit())).then(|| s.replace(',', ".").parse().ok())?
}

/// Textfeld als Wert: leer = NULL, Zahlen und Wahrheitswerte werden erkannt
pub fn field(s: &str, delim: char) -> Val {
    let t = s.trim();
    if t.is_empty() {
        return Val::Null;
    }
    let plain_int = |x: &str| {
        let d = x.strip_prefix('-').unwrap_or(x);
        !d.is_empty() && d.len() < 19 && d.bytes().all(|c| c.is_ascii_digit()) && (d == "0" || !d.starts_with('0'))
    };
    if plain_int(t) {
        return t.parse().map(Val::Int).unwrap_or_else(|_| Val::Text(s.to_string()));
    }
    let plain_real = t.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'e' | b'E' | b'+')) && t.bytes().any(|c| c == b'.') && !t.starts_with('+') && t.parse::<f64>().is_ok_and(f64::is_finite);
    if plain_real && !t.starts_with('.') && !t.ends_with('.') {
        return Val::Real(t.parse().unwrap());
    }
    if let Some(f) = decimal_comma(t, delim) {
        return Val::Real(f);
    }
    Val::Text(s.to_string())
}

struct Parsed {
    header: Vec<String>,
    delim: char,
}

fn parse_head(text: &str) -> Parsed {
    let delim = detect_delimiter(text);
    let mut header = Vec::new();
    records(text, delim, |r| {
        header = r;
        false
    });
    Parsed { header, delim }
}

pub fn scan(files: &[PathBuf], cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let mut out = Vec::new();
    for path in files {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let text = read_text(path)?;
        let head = parse_head(&text);
        if head.header.is_empty() {
            continue;
        }
        let names = unique_names(head.header.clone());
        let mut infer = vec![Infer::default(); names.len()];
        let mut rows = 0u64;
        let mut first = true;
        records(&text, head.delim, |r| {
            if std::mem::take(&mut first) {
                return true;
            }
            for (i, v) in r.iter().enumerate().take(infer.len()) {
                infer[i].add(&field(v, head.delim));
            }
            rows += 1;
            true
        });
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "import".into());
        out.push(TableScan {
            name: stem,
            origin: path.to_string_lossy().into_owned(),
            columns: names.into_iter().zip(&infer).map(|(name, inf)| Col { name, ty: column_type(inf), decl: None }).collect(),
            rows,
            pk: vec![],
            auto_inc: false,
            fks: vec![],
            ddl: None,
        });
    }
    Ok(out)
}

pub fn read(t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let text = read_text(Path::new(&t.origin))?;
    let delim = detect_delimiter(&text);
    let n = t.columns.len();
    let mut first = true;
    let mut err = None;
    records(&text, delim, |r| {
        if std::mem::take(&mut first) {
            return true;
        }
        let mut row: Vec<Val> = r.iter().take(n).map(|v| field(v, delim)).collect();
        row.resize(n, Val::Null);
        match f(row) {
            Ok(go) => go,
            Err(e) => {
                err = Some(e);
                false
            }
        }
    });
    err.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, d: char) -> Vec<Vec<String>> {
        let mut v = Vec::new();
        records(text, d, |r| {
            v.push(r);
            true
        });
        v
    }

    #[test]
    fn csv_records() {
        assert_eq!(detect_delimiter("a;b;c\n1,2,3"), ';');
        assert_eq!(detect_delimiter("a,b,c\n"), ',');
        assert_eq!(detect_delimiter("\"a;b\",c\n"), ',');
        let r = parse("id;name\r\n1;\"M;\"\"x\"\"\nzwei\"\n\n2;\n", ';');
        assert_eq!(r, vec![vec!["id", "name"], vec!["1", "M;\"x\"\nzwei"], vec!["2", ""]]);
        assert_eq!(field("12", ';'), Val::Int(12));
        assert_eq!(field("007", ';'), Val::Text("007".into()));
        assert_eq!(field("9,50", ';'), Val::Real(9.5));
        assert_eq!(field("9,50", ','), Val::Text("9,50".into()));
        assert_eq!(field("1.5", ';'), Val::Real(1.5));
        assert_eq!(field(" ", ';'), Val::Null);
        assert_eq!(cp1252(&[0x4D, 0xFC, 0x80]), "Mü€");
    }
}
