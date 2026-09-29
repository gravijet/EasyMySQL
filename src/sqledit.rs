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

const C_KEYWORD: Color32 = Color32::from_rgb(0x00, 0x00, 0xE0);
const C_FUNCTION: Color32 = Color32::from_rgb(0x79, 0x5E, 0x26);
const C_STRING: Color32 = Color32::from_rgb(0xA3, 0x15, 0x15);
const C_NUMBER: Color32 = Color32::from_rgb(0x09, 0x86, 0x58);
const C_COMMENT: Color32 = Color32::from_rgb(0x00, 0x80, 0x00);
const C_IDENT: Color32 = Color32::from_rgb(0x26, 0x7F, 0x99);
const C_TEXT: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
const C_GUTTER: Color32 = Color32::from_rgb(0x23, 0x78, 0x93);

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
            push(&mut job, start, i, C_COMMENT);
        } else if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i].1 == '*' && i + 1 < n && chars[i + 1].1 == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push(&mut job, start, i, C_COMMENT);
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < n && chars[i].1 != c {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, C_STRING);
        } else if c == '`' {
            i += 1;
            while i < n && chars[i].1 != '`' {
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, C_IDENT);
        } else if c.is_ascii_digit() {
            while i < n && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '.') {
                i += 1;
            }
            push(&mut job, start, i, C_NUMBER);
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
                C_FUNCTION
            } else if is_keyword(word) {
                C_KEYWORD
            } else {
                C_TEXT
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
            push(&mut job, start, i, C_TEXT);
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

pub struct SqlEditor {
    pub id: egui::Id,
    popup: Option<Popup>,
    popup_rect: Option<egui::Rect>,
}

#[derive(Default)]
pub struct EditorOutput {
    pub run: bool,
    pub format: bool,
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

impl SqlEditor {
    pub fn new(id: egui::Id) -> Self {
        Self { id, popup: None, popup_rect: None }
    }

    fn set_cursor(&self, ctx: &egui::Context, pos: usize) {
        if let Some(mut st) = egui::TextEdit::load_state(ctx, self.id) {
            st.cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(pos))));
            st.store(ctx, self.id);
        }
    }

    fn accept(&mut self, ctx: &egui::Context, text: &mut String) {
        let Some(p) = self.popup.take() else { return };
        let Some((item, _)) = p.items.get(p.selected) else { return };
        let a = byte_of(text, p.word_start);
        let b = byte_of(text, p.cursor);
        text.replace_range(a..b, item);
        self.set_cursor(ctx, p.word_start + item.chars().count());
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

    /// Zeigt den Editor. `height` = gewuenschte Hoehe.
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String, words: &Words, height: f32) -> EditorOutput {
        let mut out = EditorOutput::default();
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(self.id));

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
                }
            }
            if esc {
                self.popup = None;
            }
        }
        let force = focused
            && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Space)));
        if focused && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter))) {
            out.run = true;
        }
        if focused
            && ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F)))
        {
            out.format = true;
        }

        let font = FontId::monospace(13.0);
        let row_h = ctx.fonts_mut(|f| f.row_height(&font));
        let lines = text.split('\n').count().max(1);
        let digits = lines.to_string().len().max(2);
        let gutter_w = digits as f32 * 8.0 + 14.0;

        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            let mut job = highlight(buf.as_str(), font.clone());
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let frame = egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0, crate::style::SHADOW));
        let mut result = None;
        frame.show(ui, |ui| {
            ui.set_min_height(height);
            egui::ScrollArea::both()
                .id_salt(self.id.with("scroll"))
                .max_height(height)
                .min_scrolled_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (gutter, _) = ui.allocate_exact_size(
                            egui::vec2(gutter_w, (lines as f32 * row_h + 8.0).max(height - 4.0)),
                            egui::Sense::hover(),
                        );
                        let te = egui::TextEdit::multiline(text)
                            .id(self.id)
                            .font(font.clone())
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .margin(egui::vec2(6.0, 4.0))
                            .desired_width(f32::INFINITY)
                            .desired_rows(((height - 8.0) / row_h).max(3.0) as usize)
                            .lock_focus(true)
                            .hint_text("SQL hier eingeben, z. B.  SELECT * FROM tabelle;   (Strg+Leertaste = Vorschläge)")
                            .layouter(&mut layouter);
                        let o = te.show(ui);
                        // Zeilennummern
                        let p = ui.painter();
                        p.rect_filled(gutter, 0.0, Color32::from_rgb(0xF3, 0xF3, 0xF3));
                        p.vline(gutter.right() - 0.5, gutter.y_range(), egui::Stroke::new(1.0, Color32::from_gray(0xDD)));
                        // Nummer an jede Zeile, die eine neue Textzeile beginnt
                        let mut n = 1;
                        let mut new_line = true;
                        for row in &o.galley.rows {
                            if new_line {
                                p.text(
                                    egui::pos2(gutter.right() - 6.0, o.galley_pos.y + row.pos.y),
                                    egui::Align2::RIGHT_TOP,
                                    n.to_string(),
                                    font.clone(),
                                    C_GUTTER,
                                );
                                n += 1;
                            }
                            new_line = row.ends_with_newline;
                        }
                        result = Some(o);
                    });
                });
        });
        let Some(o) = result else { return out };

        let cursor = o.cursor_range.map(|r| r.primary.index.0);
        if let Some(r) = o.cursor_range {
            let s = r.slice_str(text);
            if !s.trim().is_empty() {
                out.selection = Some(s.to_string());
            }
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
                        ui.set_min_width(220.0);
                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                            for (i, (w, kind)) in p.items.iter().enumerate() {
                                let sel = i == p.selected;
                                let r = ui.add(
                                    egui::Button::selectable(
                                        sel,
                                        RichText::new(format!("{w}")).monospace(),
                                    )
                                    .right_text(RichText::new(*kind).small().color(Color32::GRAY))
                                    .min_size(egui::vec2(216.0, 0.0)),
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
