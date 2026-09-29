// SQL-Code-Editor: Syntax-Hervorhebung, Zeilennummern, Autovervollstaendigung, Formatierung.

use eframe::egui::{
    self, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, TextFormat,
    text::{CCursor, CCursorRange, LayoutJob},
};
use std::sync::Arc;

pub const KEYWORDS: &[&str] = &[
    "ADD", "ALL", "ALTER", "AND", "AS", "ASC", "AUTO_INCREMENT", "BEGIN", "BETWEEN", "BIGINT", "BLOB",
    "BOOLEAN", "BY", "CASCADE", "CASE", "CHANGE", "CHAR", "CHARACTER", "CHECK", "COLLATE", "COLUMN",
    "COMMENT", "COMMIT", "CONSTRAINT", "CREATE", "CROSS", "DATABASE", "DATABASES", "DATE", "DATETIME",
    "DECIMAL", "DEFAULT", "DELETE", "DESC", "DESCRIBE", "DISTINCT", "DOUBLE", "DROP", "ELSE", "END",
    "ENGINE", "ENUM", "EXISTS", "EXPLAIN", "FALSE", "FLOAT", "FOREIGN", "FROM", "FULL", "GRANT",
    "GROUP", "HAVING", "IF", "IGNORE", "IN", "INDEX", "INNER", "INSERT", "INT", "INTEGER", "INTO",
    "IS", "JOIN", "JSON", "KEY", "LEFT", "LIKE", "LIMIT", "LONGTEXT", "MODIFY", "NOT", "NULL",
    "OFFSET", "ON", "OR", "ORDER", "OUTER", "PRIMARY", "PROCEDURE", "REFERENCES", "RENAME",
    "REPLACE", "RESTRICT", "RIGHT", "ROLLBACK", "SELECT", "SET", "SHOW", "SMALLINT", "START",
    "TABLE", "TABLES", "TEXT", "THEN", "TIME", "TIMESTAMP", "TINYINT", "TO", "TRANSACTION",
    "TRIGGER", "TRUE", "TRUNCATE", "UNION", "UNIQUE", "UNSIGNED", "UPDATE", "USE", "USER", "USING",
    "VALUES", "VARCHAR", "VIEW", "WHEN", "WHERE", "WITH", "YEAR",
];

pub const FUNCTIONS: &[&str] = &[
    "ABS", "AVG", "CEIL", "COALESCE", "CONCAT", "CONCAT_WS", "COUNT", "CURDATE", "CURRENT_DATE",
    "CURRENT_TIMESTAMP", "DATE_ADD", "DATE_FORMAT", "DATE_SUB", "DATEDIFF", "DAY", "FLOOR",
    "GROUP_CONCAT", "HOUR", "IFNULL", "LENGTH", "LOWER", "LTRIM", "MAX", "MIN", "MINUTE", "MONTH",
    "NOW", "NULLIF", "RAND", "ROUND", "RTRIM", "SUBSTRING", "SUM", "TRIM", "UPPER", "YEAR",
];


fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_keyword(w: &str) -> bool {
    let u = w.to_ascii_uppercase();
    KEYWORDS.binary_search(&u.as_str()).is_ok()
        || matches!(u.as_str(), "SCHEMA" | "PROCEDURE" | "FUNCTION" | "RETURNS" | "DECLARE" | "WHILE" | "LOOP" | "DO")
}

/// Farbige Darstellung eines SQL-Textes.
pub fn highlight(text: &str, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let byte = |i: usize| if i < n { chars[i].0 } else { text.len() };
    let push = |job: &mut LayoutJob, a: usize, b: usize, color: Color32| {
        if b > a {
            job.append(&text[byte(a)..byte(b)], 0.0, TextFormat::simple(font.clone(), color));
        }
    };
    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        let next = if i + 1 < n { chars[i + 1].1 } else { '\0' };
        let start = i;
        if (c == '-' && next == '-') || c == '#' {
            while i < n && chars[i].1 != '\n' {
                i += 1;
            }
            push(&mut job, start, i, crate::style::pal().syn_comment);
        } else if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i].1 == '*' && i + 1 < n && chars[i + 1].1 == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push(&mut job, start, i, crate::style::pal().syn_comment);
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < n && chars[i].1 != c {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, crate::style::pal().syn_string);
        } else if c == '`' {
            i += 1;
            while i < n && chars[i].1 != '`' {
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, crate::style::pal().syn_ident);
        } else if c.is_ascii_digit() {
            while i < n && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '.') {
                i += 1;
            }
            push(&mut job, start, i, crate::style::pal().syn_number);
        } else if is_word_char(c) {
            while i < n && is_word_char(chars[i].1) {
                i += 1;
            }
            let word = &text[byte(start)..byte(i)];
            let mut j = i;
            while j < n && chars[j].1 == ' ' {
                j += 1;
            }
            let color = if j < n && chars[j].1 == '(' && FUNCTIONS.contains(&word.to_ascii_uppercase().as_str()) {
                crate::style::pal().syn_function
            } else if is_keyword(word) {
                crate::style::pal().syn_keyword
            } else {
                crate::style::pal().syn_text
            };
            push(&mut job, start, i, color);
        } else {
            while i < n {
                let c = chars[i].1;
                let nx = if i + 1 < n { chars[i + 1].1 } else { '\0' };
                if is_word_char(c)
                    || c.is_ascii_digit()
                    || c == '\''
                    || c == '"'
                    || c == '`'
                    || c == '#'
                    || (c == '-' && nx == '-')
                    || (c == '/' && nx == '*')
                {
                    break;
                }
                i += 1;
            }
            if i == start {
                i += 1;
            }
            push(&mut job, start, i, crate::style::pal().syn_text);
        }
    }
    job
}

/// SQL schoen formatieren (Schluesselwoerter gross, Einrueckung).
pub fn format_sql(sql: &str) -> String {
    use sqlformat::{FormatOptions, Indent, QueryParams};
    let opts = FormatOptions {
        indent: Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 2,
        max_inline_arguments: Some(60),
        ..Default::default()
    };
    let mut out = sqlformat::format(sql, &QueryParams::None, &opts);
    if sql.trim_end().ends_with(';') && !out.trim_end().ends_with(';') {
        out.push(';');
    }
    out.push('\n');
    out
}

/// Id des SQL-Editors, der gerade den Fokus hat (0 = keiner). Wird vom Eingabefilter der App genutzt,
/// damit Escape den Editor nicht verlaesst (egui wuerde sonst den Fokus entziehen).
pub static FOCUSED_EDITOR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Escape wurde im Editor gedrueckt (vom Eingabefilter gesetzt)
pub static ESCAPE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Aus der App aufrufen (raw_input_hook): Escape fuer den fokussierten Editor abfangen.
pub fn filter_input(ctx: &egui::Context, raw: &mut egui::RawInput) {
    use std::sync::atomic::Ordering;
    let id = FOCUSED_EDITOR.load(Ordering::Relaxed);
    if id == 0 || !ctx.memory(|m| m.focused().is_some_and(|f| f.value() == id)) {
        return;
    }
    let before = raw.events.len();
    raw.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Escape, .. }));
    if raw.events.len() != before {
        ESCAPE.store(true, Ordering::Relaxed);
    }
}

/// Woerter fuer die Autovervollstaendigung.
#[derive(Default, Clone)]
pub struct Words {
    pub tables: Vec<String>,
    /// (Tabelle, Spalte)
    pub columns: Vec<(String, String)>,
}

struct Popup {
    items: Vec<(String, &'static str)>,
    selected: usize,
    word_start: usize,
    cursor: usize,
}

#[derive(Default)]
struct Find {
    query: String,
    replace: String,
    show_replace: bool,
    case: bool,
    focus: bool,
    current: usize,
}

pub struct SqlEditor {
    pub id: egui::Id,
    popup: Option<Popup>,
    popup_rect: Option<egui::Rect>,
    prev_text: String,
    find: Option<Find>,
    goto: Option<(String, bool)>,
    scroll_to: Option<f32>,
    pub minimap: bool,
    /// Zeile (0-basiert) mit Fehler -> rot unterstrichen
    pub error_line: Option<usize>,
    /// (Zeile, Spalte), 1-basiert
    pub line_col: (usize, usize),
    /// Cursor (Zeichenindex)
    pub cursor: usize,
}

#[derive(Default)]
pub struct EditorOutput {
    /// Strg+Alt+S: Datei oder Auswahl ausfuehren
    pub run: bool,
    /// Strg+Enter: Anweisung unter dem Cursor ausfuehren
    pub run_current: bool,
    pub format: bool,
    pub changed: bool,
    pub selection: Option<String>,
}

/// Steht das Ende von `chars` in einem Kommentar oder Text?
fn in_comment_or_string(chars: &[char]) -> bool {
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

fn byte_of(s: &str, ci: usize) -> usize {
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

fn matching_bracket(chars: &[char], i: usize) -> Option<usize> {
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
fn map_lines(text: &str, first: usize, last: usize, f: impl Fn(&str) -> String) -> String {
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

/// Alle Fundstellen (Zeichenindizes)
fn find_all(text: &str, q: &str, case: bool) -> Vec<(usize, usize)> {
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

impl SqlEditor {
    pub fn new(id: egui::Id) -> Self {
        Self {
            id,
            popup: None,
            popup_rect: None,
            prev_text: String::new(),
            find: None,
            goto: None,
            scroll_to: None,
            minimap: true,
            error_line: None,
            line_col: (1, 1),
            cursor: 0,
        }
    }

    fn set_selection(&self, ctx: &egui::Context, a: usize, b: usize) {
        let mut st = egui::TextEdit::load_state(ctx, self.id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
        st.store(ctx, self.id);
    }

    fn set_cursor(&self, ctx: &egui::Context, pos: usize) {
        self.set_selection(ctx, pos, pos);
    }

    /// Aktuelle Auswahl (sortiert, Zeichenindizes)
    fn selection(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let st = egui::TextEdit::load_state(ctx, self.id)?;
        let r = st.cursor.char_range()?;
        let (a, b) = (r.primary.index.0, r.secondary.index.0);
        Some((a.min(b), a.max(b)))
    }

    /// Cursor auf Zeile setzen (0-basiert) und dorthin scrollen.
    pub fn goto_line(&mut self, ctx: &egui::Context, text: &str, line: usize, row_h: f32) {
        let pos = line_start(text, line);
        self.set_cursor(ctx, pos);
        self.scroll_to = Some((line as f32 * row_h - 80.0).max(0.0));
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    fn accept(&mut self, ctx: &egui::Context, text: &mut String) {
        let Some(p) = self.popup.take() else { return };
        let Some((item, _)) = p.items.get(p.selected) else { return };
        let a = byte_of(text, p.word_start);
        let b = byte_of(text, p.cursor);
        text.replace_range(a..b, item);
        self.set_cursor(ctx, p.word_start + item.chars().count());
        self.prev_text = text.clone();
    }

    fn suggestions(text: &str, cursor: usize, words: &Words, force: bool) -> Option<Popup> {
        let chars: Vec<char> = text.chars().collect();
        if cursor > chars.len() || in_comment_or_string(&chars[..cursor]) {
            return None;
        }
        let mut s = cursor;
        while s > 0 && is_word_char(chars[s - 1]) {
            s -= 1;
        }
        let word: String = chars[s..cursor].iter().collect();
        // "tabelle." -> Spalten dieser Tabelle
        let mut table_prefix: Option<String> = None;
        if s > 0 && chars[s - 1] == '.' {
            let mut t = s - 1;
            while t > 0 && (is_word_char(chars[t - 1]) || chars[t - 1] == '`') {
                t -= 1;
            }
            let name: String = chars[t..s - 1].iter().filter(|c| **c != '`').collect();
            if !name.is_empty() {
                table_prefix = Some(name);
            }
        }
        if word.chars().count() < 2 && table_prefix.is_none() && !force {
            return None;
        }
        let lw = word.to_lowercase();
        let mut items: Vec<(String, &'static str)> = Vec::new();
        let add = |w: &str, kind: &'static str, items: &mut Vec<(String, &'static str)>| {
            if w.to_lowercase().starts_with(&lw) && !items.iter().any(|(x, _)| x == w) {
                items.push((w.to_string(), kind));
            }
        };
        match &table_prefix {
            Some(t) => {
                // Alias? Dann alle Spalten anbieten, deren Tabelle passt, sonst alle.
                let known = words.tables.iter().any(|x| x.eq_ignore_ascii_case(t));
                for (tab, c) in &words.columns {
                    if !known || tab.eq_ignore_ascii_case(t) {
                        add(c, "Spalte", &mut items);
                    }
                }
            }
            None => {
                for t in &words.tables {
                    add(t, "Tabelle", &mut items);
                }
                for (_, c) in &words.columns {
                    add(c, "Spalte", &mut items);
                }
                for f in FUNCTIONS {
                    add(f, "Funktion", &mut items);
                }
                for k in KEYWORDS {
                    add(k, "SQL", &mut items);
                }
            }
        }
        if items.is_empty() || (items.len() == 1 && items[0].0 == word && !force) {
            return None;
        }
        items.truncate(60);
        Some(Popup { items, selected: 0, word_start: s, cursor })
    }

    /// Tastenkuerzel, die den Text veraendern (vor dem Zeichnen des Editors).
    fn handle_keys(&mut self, ui: &mut egui::Ui, text: &mut String, out: &mut EditorOutput) {
        let ctx = ui.ctx().clone();
        let cmd = Modifiers::COMMAND;
        let pressed = |ui: &mut egui::Ui, m: Modifiers, k: Key| ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        if pressed(ui, cmd | Modifiers::ALT, Key::S) {
            out.run = true;
        }
        if pressed(ui, cmd | Modifiers::ALT, Key::L) || pressed(ui, Modifiers::SHIFT | Modifiers::ALT, Key::F) || pressed(ui, cmd | Modifiers::SHIFT, Key::F) {
            out.format = true;
        }
        if self.popup.is_none() && !ui.input(|i| i.modifiers.alt) && pressed(ui, cmd, Key::Enter) {
            out.run_current = true;
        }
        if !ui.input(|i| i.modifiers.alt) && pressed(ui, cmd, Key::F) {
            let sel = self.selection(&ctx).filter(|(a, b)| a != b).map(|(a, b)| text.chars().skip(a).take(b - a).collect::<String>());
            let f = self.find.get_or_insert_with(Find::default);
            if let Some(s) = sel.filter(|s| !s.contains('\n')) {
                f.query = s;
            }
            f.focus = true;
        }
        if !ui.input(|i| i.modifiers.alt) && pressed(ui, cmd, Key::H) {
            let f = self.find.get_or_insert_with(Find::default);
            f.show_replace = true;
            f.focus = true;
        }
        if !ui.input(|i| i.modifiers.alt) && pressed(ui, cmd, Key::G) {
            self.goto = Some((String::new(), true));
        }

        let Some((a, b)) = self.selection(&ctx) else { return };
        let (first, _) = line_col(text, a);
        let (mut last, last_col) = line_col(text, b);
        if b > a && last_col == 0 && last > first {
            last -= 1; // Auswahl endet am Zeilenanfang
        }

        // Strg+# (deutsche Tastatur) oder Strg+/
        let comment = ui.input_mut(|i| {
            let mut hit = false;
            i.events.retain(|e| {
                if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = e {
                    if modifiers.command && !modifiers.alt && (*key == Key::Slash || *physical_key == Some(Key::Backslash)) {
                        hit = true;
                        return false;
                    }
                }
                true
            });
            hit
        });
        if comment {
            *text = toggle_comment(text, first, last);
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(if last + 1 < text.split('\n').count() { 1 } else { 0 }));
            out.changed = true;
            self.prev_text = text.clone();
        }

        // Alt+Pfeil: Zeilen verschieben, Umschalt+Alt+Pfeil: duplizieren
        for (key, dir) in [(Key::ArrowUp, -1i32), (Key::ArrowDown, 1)] {
            if pressed(ui, Modifiers::ALT, key) {
                if let Some((t, nf)) = move_lines(text, first, last, dir) {
                    *text = t;
                    let span = last - first;
                    self.set_selection(&ctx, line_start(text, nf), line_start(text, nf + span + 1).saturating_sub(1).max(line_start(text, nf)));
                    out.changed = true;
                    self.prev_text = text.clone();
                }
            }
            if pressed(ui, Modifiers::ALT | Modifiers::SHIFT, key) {
                *text = duplicate_lines(text, first, last);
                if dir > 0 {
                    let nf = last + 1;
                    self.set_cursor(&ctx, line_start(text, nf));
                }
                out.changed = true;
                self.prev_text = text.clone();
            }
        }

        // Tab / Umschalt+Tab bei mehrzeiliger Auswahl: Block einruecken
        if self.popup.is_none() && last > first {
            if pressed(ui, Modifiers::NONE, Key::Tab) {
                *text = map_lines(text, first, last, |l| format!("  {l}"));
                self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1));
                out.changed = true;
                self.prev_text = text.clone();
            }
        }
        if pressed(ui, Modifiers::SHIFT, Key::Tab) {
            *text = map_lines(text, first, last, |l| {
                let n = l.chars().take(2).take_while(|c| *c == ' ').count();
                if l.starts_with('\t') { l[1..].to_string() } else { l[n..].to_string() }
            });
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1).max(line_start(text, first)));
            out.changed = true;
            self.prev_text = text.clone();
        }
    }

    /// Automatisches Schliessen von Klammern/Anfuehrungszeichen nach einer Eingabe.
    fn auto_close(&mut self, ctx: &egui::Context, text: &mut String, cursor: usize) {
        let old_n = self.prev_text.chars().count();
        let new_n = text.chars().count();
        let chars: Vec<char> = text.chars().collect();
        if new_n == old_n + 1 && cursor > 0 && cursor <= chars.len() {
            let c = chars[cursor - 1];
            let next = chars.get(cursor).copied();
            let prev = if cursor >= 2 { chars.get(cursor - 2).copied() } else { None };
            let close = match c {
                '(' => Some(')'),
                '[' => Some(']'),
                '{' => Some('}'),
                '\'' | '"' | '`' => {
                    let word_around = prev.is_some_and(is_word_char) || next.is_some_and(is_word_char);
                    if word_around { None } else { Some(c) }
                }
                _ => None,
            };
            // Ueberschreiben einer bereits vorhandenen schliessenden Klammer
            if matches!(c, ')' | ']' | '}' | '\'' | '"' | '`') && next == Some(c) {
                let old: Vec<char> = self.prev_text.chars().collect();
                if old.get(cursor - 1) == Some(&c) {
                    let bi = byte_of(text, cursor);
                    text.remove(bi);
                    return;
                }
            }
            if let Some(cl) = close {
                if next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';')) {
                    let bi = byte_of(text, cursor);
                    text.insert(bi, cl);
                    self.set_cursor(ctx, cursor);
                }
            }
        } else if new_n + 1 == old_n {
            // Rueckschritt zwischen leerem Paar "()" -> beide loeschen
            let old: Vec<char> = self.prev_text.chars().collect();
            if let (Some(&o), Some(&cl)) = (old.get(cursor), old.get(cursor + 1)) {
                let pair = matches!((o, cl), ('(', ')') | ('[', ']') | ('{', '}') | ('\'', '\'') | ('"', '"'));
                if pair && chars.get(cursor) == Some(&cl) {
                    let bi = byte_of(text, cursor);
                    text.remove(bi);
                }
            }
        }
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, text: &mut String, row_h: f32) {
        let ctx = ui.ctx().clone();
        let Some(f) = self.find.as_mut() else { return };
        let matches = find_all(text, &f.query, f.case);
        let mut close = false;
        let mut jump: Option<usize> = None;
        let mut replace_one = false;
        let mut replace_all = false;
        egui::Frame::new().fill(crate::style::pal().face_light).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Suchen:");
                let find_id = self.id.with("find");
                let r = ui.add(egui::TextEdit::singleline(&mut f.query).desired_width(200.0).id(find_id));
                if f.focus {
                    r.request_focus();
                    // Vorhandenen Suchtext markieren, damit Tippen ihn ersetzt
                    let mut st = egui::TextEdit::load_state(ui.ctx(), find_id).unwrap_or_default();
                    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(f.query.chars().count()))));
                    st.store(ui.ctx(), find_id);
                    f.focus = false;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.toggle_value(&mut f.case, "Aa").on_hover_text("Groß-/Kleinschreibung beachten");
                let info = if f.query.is_empty() {
                    String::new()
                } else if matches.is_empty() {
                    "keine Treffer".into()
                } else {
                    format!("{} von {}", (f.current % matches.len()) + 1, matches.len())
                };
                ui.label(RichText::new(info).small());
                if ui.small_button("↑").on_hover_text("Vorheriger (Umschalt+Enter)").clicked() && !matches.is_empty() {
                    f.current = (f.current + matches.len() - 1) % matches.len();
                    jump = Some(f.current);
                }
                if (ui.small_button("↓").on_hover_text("Nächster (Enter)").clicked() || enter) && !matches.is_empty() {
                    if enter {
                        f.current = (f.current + 1) % matches.len();
                        r.request_focus();
                    } else {
                        f.current = (f.current + 1) % matches.len();
                    }
                    jump = Some(f.current);
                }
                ui.toggle_value(&mut f.show_replace, "Ersetzen");
                if ui.small_button("✖").on_hover_text("Schließen (Esc)").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    close = true;
                }
            });
            if f.show_replace {
                ui.horizontal(|ui| {
                    ui.label("Ersetzen:");
                    ui.add(egui::TextEdit::singleline(&mut f.replace).desired_width(200.0));
                    if ui.small_button("Ersetzen").clicked() {
                        replace_one = true;
                    }
                    if ui.small_button("Alle ersetzen").clicked() {
                        replace_all = true;
                    }
                });
            }
        });
        if replace_all && !matches.is_empty() {
            let chars: Vec<char> = text.chars().collect();
            let mut out = String::new();
            let mut last = 0;
            for (a, b) in &matches {
                out.extend(&chars[last..*a]);
                out.push_str(&f.replace);
                last = *b;
            }
            out.extend(&chars[last..]);
            *text = out;
        } else if replace_one && !matches.is_empty() {
            let (a, b) = matches[f.current % matches.len()];
            let (ba, bb) = (byte_of(text, a), byte_of(text, b));
            text.replace_range(ba..bb, &f.replace);
            jump = Some(f.current);
        }
        if let Some(i) = jump {
            let m = find_all(text, &f.query, f.case);
            if let Some(&(a, b)) = m.get(i % m.len().max(1)) {
                let (line, _) = line_col(text, a);
                self.set_selection(&ctx, a, b);
                self.scroll_to = Some((line as f32 * row_h - 80.0).max(0.0));
            }
        }
        if close {
            self.find = None;
            ctx.memory_mut(|m| m.request_focus(self.id));
        }
    }

    /// Zeigt den Editor. `height` = gewuenschte Hoehe.
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String, words: &Words, height: f32) -> EditorOutput {
        let mut out = EditorOutput::default();
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(self.id));
        let pal = crate::style::pal();
        let font = FontId::monospace(13.0);
        let row_h = ctx.fonts_mut(|f| f.row_height(&font));
        if self.prev_text.is_empty() && !text.is_empty() {
            self.prev_text = text.clone();
        }

        // Tasten fuer das Vorschlagsfenster abfangen, bevor der Editor sie bekommt
        if focused && self.popup.is_some() {
            let (down, up, enter, tab, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                    i.consume_key(Modifiers::NONE, Key::Tab),
                    i.consume_key(Modifiers::NONE, Key::Escape),
                )
            });
            if let Some(p) = self.popup.as_mut() {
                if down {
                    p.selected = (p.selected + 1) % p.items.len();
                }
                if up {
                    p.selected = (p.selected + p.items.len() - 1) % p.items.len();
                }
            }
            if enter || tab {
                // Ist das Wort schon vollstaendig getippt, bleibt Enter ein Zeilenumbruch
                let typed_complete = self.popup.as_ref().is_some_and(|p| {
                    let a = byte_of(text, p.word_start);
                    let b = byte_of(text, p.cursor);
                    p.items.get(p.selected).is_some_and(|(w, _)| text.get(a..b).is_some_and(|t| t.eq_ignore_ascii_case(w)))
                });
                if enter && typed_complete {
                    self.popup = None;
                    ui.input_mut(|i| {
                        i.events.push(egui::Event::Key {
                            key: Key::Enter,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: Modifiers::NONE,
                        })
                    });
                } else {
                    self.accept(&ctx, text);
                    out.changed = true;
                }
            }
            if esc {
                self.popup = None;
            }
        }
        // Escape (vom Eingabefilter abgefangen): Vorschlaege bzw. Leisten schliessen, Fokus behalten
        if focused && ESCAPE.swap(false, std::sync::atomic::Ordering::Relaxed) {
            if self.popup.is_some() {
                self.popup = None;
            } else {
                self.find = None;
                self.goto = None;
            }
        }
        let force = focused && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Space)));
        if focused {
            self.handle_keys(ui, text, &mut out);
        }

        // Such-/Ersetzen-Leiste und "Gehe zu Zeile"
        let top0 = ui.cursor().top();
        if self.find.is_some() {
            self.find_bar(ui, text, row_h);
        }
        if let Some((line, first)) = self.goto.as_mut() {
            let mut go = None;
            let mut close = false;
            egui::Frame::new().fill(pal.face_light).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Gehe zu Zeile (1–{}):", text.split('\n').count()));
                    let r = ui.add(egui::TextEdit::singleline(line).desired_width(80.0));
                    if *first {
                        r.request_focus();
                        *first = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = line.trim().parse::<usize>().ok();
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) || ui.small_button("✖").clicked() {
                        close = true;
                    }
                });
            });
            if let Some(n) = go {
                self.goto = None;
                self.goto_line(&ctx, text, n.saturating_sub(1), row_h);
            } else if close {
                self.goto = None;
            }
        }

        // Hoehe der Leisten vom Editor abziehen
        let height = (height - (ui.cursor().top() - top0)).max(60.0);
        let lines = text.split('\n').count().max(1);
        let digits = lines.to_string().len().max(2);
        let gutter_w = digits as f32 * 8.0 + 18.0;

        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            let mut job = highlight(buf.as_str(), font.clone());
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let minimap_w = if self.minimap { 70.0 } else { 0.0 };
        let frame = egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border));
        let mut result = None;
        let mut scroll_state: Option<(f32, f32, egui::Rect)> = None;
        let find_q = self.find.as_ref().map(|f| (f.query.clone(), f.case));
        frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut sa = egui::ScrollArea::both()
                    .id_salt(self.id.with("scroll"))
                    .max_height(height)
                    .min_scrolled_height(height)
                    .max_width(ui.available_width() - minimap_w)
                    .auto_shrink([false, false]);
                if let Some(y) = self.scroll_to.take() {
                    sa = sa.vertical_scroll_offset(y);
                }
                let so = sa.show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (gutter, _) = ui.allocate_exact_size(
                            egui::vec2(gutter_w, (lines as f32 * row_h + 8.0).max(height - 4.0)),
                            egui::Sense::hover(),
                        );
                        // Platzhalter fuer Hintergrund (aktuelle Zeile, Klammern, Treffer)
                        let bg_idx = ui.painter().add(egui::Shape::Noop);
                        let te = egui::TextEdit::multiline(text)
                            .id(self.id)
                            .font(font.clone())
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .margin(egui::vec2(6.0, 4.0))
                            .desired_width(f32::INFINITY)
                            .desired_rows(((height - 8.0) / row_h).max(3.0) as usize)
                            .lock_focus(true)
                            .hint_text("SQL hier eingeben …   Strg+Leertaste = Vorschläge, Strg+Alt+S = ausführen")
                            .layouter(&mut layouter);
                        let o = te.show(ui);
                        let gp = o.galley_pos;
                        let text_rect = o.response.response.rect;
                        let mut bg = Vec::new();
                        let cur = o.cursor_range.map(|r| r.primary.index.0);
                        let pos_of = |i: usize| o.galley.pos_from_cursor(CCursor::new(i)).translate(gp.to_vec2());
                        if let (Some(c), true) = (cur, o.response.response.has_focus()) {
                            let r = pos_of(c);
                            bg.push(egui::Shape::rect_filled(
                                egui::Rect::from_x_y_ranges(text_rect.x_range(), r.y_range()),
                                0.0,
                                pal.current_line,
                            ));
                            // Klammerpaar
                            let chars: Vec<char> = text.chars().collect();
                            let at = if c > 0 && matching_bracket(&chars, c - 1).is_some() { Some(c - 1) } else if matching_bracket(&chars, c).is_some() { Some(c) } else { None };
                            if let Some(i) = at {
                                if let Some(j) = matching_bracket(&chars, i) {
                                    for k in [i, j] {
                                        let a = pos_of(k);
                                        let b = pos_of(k + 1);
                                        bg.push(egui::Shape::rect_filled(egui::Rect::from_min_max(a.left_top(), egui::pos2(b.left(), a.bottom())), 2.0, pal.bracket));
                                    }
                                }
                            }
                        }
                        if let Some((q, case)) = &find_q {
                            for (a, b) in find_all(text, q, *case) {
                                let ra = pos_of(a);
                                let rb = pos_of(b);
                                if (ra.top() - rb.top()).abs() < 1.0 {
                                    bg.push(egui::Shape::rect_filled(egui::Rect::from_min_max(ra.left_top(), egui::pos2(rb.left(), ra.bottom())), 2.0, pal.find_match));
                                }
                            }
                        }
                        ui.painter().set(bg_idx, egui::Shape::Vec(bg));
                        // Fehlerzeile rot unterkringeln
                        if let Some(el) = self.error_line {
                            let s = line_start(text, el);
                            let e = line_start(text, el + 1).saturating_sub(1).max(s + 1);
                            let ra = pos_of(s);
                            let rb = pos_of(e);
                            let y = ra.bottom() - 1.0;
                            let mut pts = Vec::new();
                            let mut x = ra.left();
                            let mut up = true;
                            while x < rb.left().max(ra.left() + 30.0) {
                                pts.push(egui::pos2(x, if up { y - 2.0 } else { y }));
                                x += 3.0;
                                up = !up;
                            }
                            ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.0, pal.error_text)));
                        }
                        // Zeilennummern
                        let p = ui.painter();
                        p.rect_filled(gutter, 0.0, pal.bg);
                        let cur_line = cur.map(|c| line_col(text, c).0);
                        let mut n = 1;
                        let mut new_line = true;
                        for row in &o.galley.rows {
                            if new_line {
                                let active = cur_line == Some(n - 1);
                                let err = self.error_line == Some(n - 1);
                                p.text(
                                    egui::pos2(gutter.right() - 8.0, gp.y + row.pos.y),
                                    egui::Align2::RIGHT_TOP,
                                    n.to_string(),
                                    font.clone(),
                                    if err { pal.error_text } else if active { pal.text } else { pal.syn_gutter },
                                );
                                n += 1;
                            }
                            new_line = row.ends_with_newline;
                        }
                        result = Some(o);
                    });
                });
                scroll_state = Some((so.state.offset.y, so.content_size.y, so.inner_rect));

                // Minimap
                if self.minimap {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(minimap_w, height), egui::Sense::click_and_drag());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 0.0, pal.bg);
                    p.vline(rect.left(), rect.y_range(), egui::Stroke::new(1.0, pal.grid_line));
                    let scale = (rect.height() / (lines as f32 * row_h).max(1.0)).min(2.0 / row_h);
                    let lh = row_h * scale;
                    for (i, l) in text.split('\n').enumerate() {
                        let y = rect.top() + i as f32 * lh;
                        if y > rect.bottom() {
                            break;
                        }
                        let indent = l.len() - l.trim_start().len();
                        let len = l.trim_end().chars().count();
                        if len > indent {
                            let x0 = rect.left() + 4.0 + indent as f32 * 0.8;
                            let x1 = (rect.left() + 4.0 + len as f32 * 0.8).min(rect.right() - 2.0);
                            let color = if l.trim_start().starts_with("--") { pal.syn_comment } else { pal.syn_gutter };
                            p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(lh.clamp(1.0, 2.0), color.gamma_multiply(0.8)));
                        }
                    }
                    if let Some((off, content, inner)) = scroll_state {
                        let top = rect.top() + off * scale;
                        let vh = (inner.height() * scale).max(8.0);
                        p.rect_filled(egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(minimap_w, vh)), 0.0, pal.text.gamma_multiply(0.12));
                        if let Some(pt) = resp.interact_pointer_pos() {
                            if resp.clicked() || resp.dragged() {
                                let y = ((pt.y - rect.top()) / scale - inner.height() / 2.0).clamp(0.0, (content - inner.height()).max(0.0));
                                self.scroll_to = Some(y);
                            }
                        }
                    }
                }
            });
        });
        let Some(o) = result else { return out };
        {
            use std::sync::atomic::Ordering;
            if o.response.response.has_focus() {
                FOCUSED_EDITOR.store(self.id.value(), Ordering::Relaxed);
            } else if FOCUSED_EDITOR.load(Ordering::Relaxed) == self.id.value() {
                FOCUSED_EDITOR.store(0, Ordering::Relaxed);
            }
        }

        let cursor = o.cursor_range.map(|r| r.primary.index.0);
        if let Some(r) = o.cursor_range {
            let s = r.slice_str(text);
            if !s.trim().is_empty() {
                out.selection = Some(s.to_string());
            }
        }
        if let Some(c) = cursor {
            self.cursor = c;
            let (l, col) = line_col(text, c);
            self.line_col = (l + 1, col + 1);
        }

        if o.response.response.changed() {
            out.changed = true;
            if let Some(c) = cursor {
                self.auto_close(&ctx, text, c);
            }
            self.error_line = None;
        }
        if out.changed {
            self.prev_text = text.clone();
        }

        // Vorschlaege aktualisieren
        if o.response.response.changed() || force {
            self.popup = cursor.and_then(|c| Self::suggestions(text, c, words, force));
        } else if let (Some(p), Some(c)) = (&self.popup, cursor) {
            if p.cursor != c {
                self.popup = None;
            }
        }
        if !ctx.memory(|m| m.has_focus(self.id)) && !force {
            // Fokus verloren (ausser durch Klick ins Vorschlagsfenster)
            let pressed_outside = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .map(|p| !self.popup_rect.is_some_and(|r| r.contains(p)))
                        .unwrap_or(true)
            });
            if pressed_outside {
                self.popup = None;
            }
        }

        // Vorschlagsfenster zeichnen
        let mut clicked = None;
        if let (Some(p), Some(c)) = (&self.popup, cursor) {
            let caret = o.galley.pos_from_cursor(CCursor::new(c));
            let pos = o.galley_pos + caret.left_bottom().to_vec2() + egui::vec2(0.0, 2.0);
            let area = egui::Area::new(self.id.with("popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(&ctx, |ui| {
                    egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
                        ui.set_min_width(240.0);
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for (i, (w, kind)) in p.items.iter().enumerate() {
                                let sel = i == p.selected;
                                let r = ui.add(
                                    egui::Button::selectable(sel, RichText::new(w.as_str()).monospace())
                                        .right_text(RichText::new(*kind).small().color(pal.text_weak))
                                        .min_size(egui::vec2(236.0, 0.0)),
                                );
                                if sel {
                                    r.scroll_to_me(None);
                                }
                                if r.clicked() {
                                    clicked = Some(i);
                                }
                            }
                        });
                    });
                });
            self.popup_rect = Some(area.response.rect);
        } else {
            self.popup_rect = None;
        }
        if let Some(i) = clicked {
            if let Some(p) = self.popup.as_mut() {
                p.selected = i;
            }
            self.accept(&ctx, text);
            out.changed = true;
            ctx.memory_mut(|m| m.request_focus(self.id));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_sorted() {
        let mut k = KEYWORDS.to_vec();
        k.sort();
        assert_eq!(k, KEYWORDS.to_vec(), "KEYWORDS muss sortiert sein (binary_search)");
    }

    #[test]
    fn highlight_covers_text() {
        let t = "SELECT `a`, 'x''y' -- k\nFROM t WHERE n = 1.5 /* c */ AND COUNT(*)";
        let job = highlight(t, FontId::monospace(13.0));
        assert_eq!(job.text, t);
    }

    #[test]
    fn format_basic() {
        let f = format_sql("select a,b from t where x=1;");
        assert!(f.starts_with("SELECT\n  a, b\nFROM\n  t\nWHERE\n  x = 1;"), "{f}");
    }

    #[test]
    fn completion() {
        let w = Words {
            tables: vec!["schueler".into(), "klasse".into()],
            columns: vec![("schueler".into(), "vorname".into()), ("klasse".into(), "name".into())],
        };
        let p = SqlEditor::suggestions("SELECT * FROM sch", 17, &w, false).unwrap();
        assert_eq!(p.items[0].0, "schueler");
        let p = SqlEditor::suggestions("SELECT klasse.", 14, &w, false).unwrap();
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].0, "name");
        let p = SqlEditor::suggestions("sel", 3, &w, false).unwrap();
        assert_eq!(p.items[0].0, "SELECT");
        assert!(SqlEditor::suggestions("-- sel", 6, &w, false).is_none());
        assert!(SqlEditor::suggestions("SELECT 'sch", 11, &w, false).is_none());
        assert!(SqlEditor::suggestions("SELECT 'a' FROM sch", 19, &w, false).is_some());
        assert!(SqlEditor::suggestions("s", 1, &w, false).is_none());
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn comment_toggle() {
        let t = "SELECT 1;\n  SELECT 2;\nSELECT 3;";
        let c = toggle_comment(t, 0, 1);
        assert_eq!(c, "-- SELECT 1;\n  -- SELECT 2;\nSELECT 3;");
        assert_eq!(toggle_comment(&c, 0, 1), t);
    }

    #[test]
    fn move_and_duplicate() {
        let t = "a\nb\nc";
        assert_eq!(move_lines(t, 1, 1, -1).unwrap().0, "b\na\nc");
        assert_eq!(move_lines(t, 1, 1, 1).unwrap().0, "a\nc\nb");
        assert!(move_lines(t, 0, 0, -1).is_none());
        assert_eq!(duplicate_lines(t, 1, 1), "a\nb\nb\nc");
    }

    #[test]
    fn positions_and_brackets() {
        let t = "ab\ncd(e)";
        assert_eq!(line_col(t, 4), (1, 1));
        assert_eq!(line_start(t, 1), 3);
        let c: Vec<char> = t.chars().collect();
        assert_eq!(matching_bracket(&c, 5), Some(7));
        assert_eq!(matching_bracket(&c, 7), Some(5));
        assert_eq!(find_all("Select SELECT sel", "select", false).len(), 2);
    }
}
