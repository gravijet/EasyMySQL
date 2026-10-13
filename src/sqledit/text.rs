// Textoperationen des Editors (Positionen, Klammern, Zeilen, Suchen).

/// Steht das Ende von `chars` in einem Kommentar oder Text?
pub(super) fn in_comment_or_string(chars: &[char]) -> bool {
    let (mut i, n) = (0, chars.len());
    while i < n {
        let c = chars[i];
        let nx = chars.get(i + 1).copied().unwrap_or('\0');
        if (c == '-' && nx == '-') || c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            if i >= n {
                return true;
            }
        } else if c == '/' && nx == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 >= n {
                return true;
            }
            i += 1;
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < n && chars[i] != c {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i >= n {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub(super) fn byte_of(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

/// Zeichenindex -> (Zeile, Spalte), 0-basiert
pub fn line_col(text: &str, ci: usize) -> (usize, usize) {
    let mut line = 0;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i >= ci {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Zeichenindex des Zeilenanfangs von `line` (0-basiert)
pub fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut l = 0;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            l += 1;
            if l == line {
                return i + 1;
            }
        }
    }
    text.chars().count()
}

pub(super) fn matching_bracket(chars: &[char], i: usize) -> Option<usize> {
    let c = *chars.get(i)?;
    let (open, close, fwd) = match c {
        '(' => ('(', ')', true),
        '[' => ('[', ']', true),
        '{' => ('{', '}', true),
        ')' => ('(', ')', false),
        ']' => ('[', ']', false),
        '}' => ('{', '}', false),
        _ => return None,
    };
    let mut depth = 0i32;
    if fwd {
        for (j, &ch) in chars.iter().enumerate().skip(i) {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
        }
    } else {
        for j in (0..=i).rev() {
            if chars[j] == close {
                depth += 1;
            } else if chars[j] == open {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
        }
    }
    None
}

/// Zeilen `first..=last` als Block bearbeiten; liefert neuen Text.
pub(super) fn map_lines(text: &str, first: usize, last: usize, f: impl Fn(&str) -> String) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| if i >= first && i <= last { f(l) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Kommentar (-- ) fuer Zeilen umschalten.
pub fn toggle_comment(text: &str, first: usize, last: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let all_commented = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| *i >= first && *i <= last && !l.trim().is_empty())
        .all(|(_, l)| l.trim_start().starts_with("--"));
    map_lines(text, first, last, |l| {
        if l.trim().is_empty() {
            l.to_string()
        } else if all_commented {
            let indent = l.len() - l.trim_start().len();
            let rest = &l[indent..];
            let rest = rest.strip_prefix("-- ").or_else(|| rest.strip_prefix("--")).unwrap_or(rest);
            format!("{}{}", &l[..indent], rest)
        } else {
            let indent = l.len() - l.trim_start().len();
            format!("{}-- {}", &l[..indent], &l[indent..])
        }
    })
}

/// Zeilen verschieben (dir = -1 hoch, +1 runter). Liefert (Text, neue erste Zeile).
pub fn move_lines(text: &str, first: usize, last: usize, dir: i32) -> Option<(String, usize)> {
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    if dir < 0 && first == 0 || dir > 0 && last + 1 >= lines.len() {
        return None;
    }
    if dir < 0 {
        let l = lines.remove(first - 1);
        lines.insert(last, l);
        Some((lines.join("\n"), first - 1))
    } else {
        let l = lines.remove(last + 1);
        lines.insert(first, l);
        Some((lines.join("\n"), first + 1))
    }
}

pub fn duplicate_lines(text: &str, first: usize, last: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        out.push(l);
        if i == last {
            out.extend(lines[first..=last].iter());
        }
    }
    out.join("\n")
}

/// Zeilen `first..=last` entfernen. Liefert (neuer Text, entfernter Text mit Zeilenumbruch,
/// Cursor am Anfang der Zeile, die nachrueckt).
pub fn remove_lines(text: &str, first: usize, last: usize) -> (String, String, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = last.min(lines.len() - 1);
    let removed = lines[first..=last].join("\n") + "\n";
    let mut rest: Vec<&str> = lines[..first].to_vec();
    rest.extend_from_slice(&lines[last + 1..]);
    if rest.is_empty() {
        return (String::new(), removed, 0);
    }
    let t = rest.join("\n");
    let cur = line_start(&t, first.min(rest.len() - 1));
    (t, removed, cur)
}

/// Alle Fundstellen (Zeichenindizes)
pub(super) fn find_all(text: &str, q: &str, case: bool) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return Vec::new();
    }
    let (t, qq) = if case { (text.to_string(), q.to_string()) } else { (text.to_lowercase(), q.to_lowercase()) };
    // Kleinschreibung kann Laengen aendern; bei Abweichung Gross/Klein beachten
    let (t, qq) = if t.chars().count() == text.chars().count() { (t, qq) } else { (text.to_string(), q.to_string()) };
    let tc: Vec<char> = t.chars().collect();
    let qc: Vec<char> = qq.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + qc.len() <= tc.len() && out.len() < 1000 {
        if tc[i..i + qc.len()] == qc[..] {
            out.push((i, i + qc.len()));
            i += qc.len().max(1);
        } else {
            i += 1;
        }
    }
    out
}
