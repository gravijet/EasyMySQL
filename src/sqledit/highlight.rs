// Syntaxfaerbung und Zwischenspeicher fuer den Textsatz.

use super::*;

pub const KEYWORDS: &[&str] = &[
    "ADD", "ALL", "ALTER", "AND", "ANY", "AS", "ASC", "AUTO_INCREMENT", "BEGIN", "BETWEEN",
    "BIGINT", "BINARY", "BLOB", "BOOLEAN", "BY", "CALL", "CASCADE", "CASE", "CHANGE", "CHAR",
    "CHARACTER", "CHECK", "COLLATE", "COLUMN", "COLUMNS", "COMMENT", "COMMIT", "CONSTRAINT",
    "CREATE", "CROSS", "CURRENT_TIMESTAMP", "DATABASE", "DATABASES", "DATE", "DATETIME", "DECIMAL",
    "DECLARE", "DEFAULT", "DELETE", "DELIMITER", "DESC", "DESCRIBE", "DISTINCT", "DIV", "DO",
    "DOUBLE", "DROP", "DUPLICATE", "ELSE", "END", "ENGINE", "ENUM", "ESCAPE", "EXCEPT", "EXISTS",
    "EXPLAIN", "FALSE", "FIRST", "FLOAT", "FOLLOWING", "FOREIGN", "FROM", "FULL", "FUNCTION",
    "GRANT", "GROUP", "HAVING", "IF", "IGNORE", "IN", "INDEX", "INNER", "INSERT", "INT", "INTEGER",
    "INTERSECT", "INTERVAL", "INTO", "IS", "JOIN", "JSON", "KEY", "LAST", "LEFT", "LIKE", "LIMIT",
    "LOCK", "LONGTEXT", "LOOP", "MODIFY", "NATURAL", "NOT", "NULL", "NULLS", "OFFSET", "ON", "OR",
    "ORDER", "OUTER", "OVER", "PARTITION", "PRECEDING", "PRIMARY", "PROCEDURE", "RANGE",
    "RECURSIVE", "REFERENCES", "REGEXP", "RENAME", "REPLACE", "RESTRICT", "RETURNING", "RETURNS",
    "RIGHT", "ROLLBACK", "ROLLUP", "ROW", "ROWS", "SCHEMA", "SELECT", "SET", "SHOW", "SMALLINT",
    "SOME", "START", "STRAIGHT_JOIN", "TABLE", "TABLES", "TEMPORARY", "TEXT", "THEN", "TIME",
    "TIMESTAMP", "TINYINT", "TO", "TRANSACTION", "TRIGGER", "TRUE", "TRUNCATE", "UNBOUNDED",
    "UNION", "UNIQUE", "UNLOCK", "UNSIGNED", "UPDATE", "USE", "USER", "USING", "VALUE", "VALUES",
    "VARCHAR", "VIEW", "WHEN", "WHERE", "WHILE", "WINDOW", "WITH", "XOR", "YEAR", "ZEROFILL",
];

pub const FUNCTIONS: &[&str] = &[
    "ABS", "ASCII", "AVG", "CAST", "CEIL", "CEILING", "CHAR", "CHAR_LENGTH", "COALESCE", "CONCAT",
    "CONCAT_WS", "CONVERT", "COUNT", "CUME_DIST", "CURDATE", "CURRENT_DATE", "CURRENT_TIMESTAMP",
    "CURTIME", "DATABASE", "DATE", "DATEDIFF", "DATE_ADD", "DATE_FORMAT", "DATE_SUB", "DAY",
    "DAYNAME", "DAYOFWEEK", "DAYOFYEAR", "DENSE_RANK", "EXP", "EXTRACT", "FIELD", "FIND_IN_SET",
    "FIRST_VALUE", "FLOOR", "FORMAT", "FROM_UNIXTIME", "GREATEST", "GROUP_CONCAT", "HEX", "HOUR",
    "IF", "IFNULL", "INSTR", "JSON_ARRAY", "JSON_EXTRACT", "JSON_OBJECT", "JSON_VALUE", "LAG",
    "LAST_DAY", "LAST_INSERT_ID", "LAST_VALUE", "LEAD", "LEAST", "LEFT", "LENGTH", "LN", "LOCATE",
    "LOG", "LOWER", "LPAD", "LTRIM", "MAX", "MD5", "MIN", "MINUTE", "MOD", "MONTH", "MONTHNAME",
    "NOW", "NTH_VALUE", "NTILE", "NULLIF", "PERCENT_RANK", "PI", "POSITION", "POW", "POWER",
    "QUARTER", "RAND", "RANK", "REGEXP_REPLACE", "REPEAT", "REPLACE", "REVERSE", "RIGHT", "ROUND",
    "ROW_NUMBER", "RPAD", "RTRIM", "SECOND", "SHA2", "SIGN", "SQRT", "STD", "STDDEV", "STR_TO_DATE",
    "SUBSTRING", "SUBSTRING_INDEX", "SUM", "TIME", "TIMEDIFF", "TIMESTAMPDIFF", "TIME_FORMAT",
    "TRIM", "TRUNCATE", "UNIX_TIMESTAMP", "UPPER", "USER", "UUID", "VARIANCE", "VERSION", "WEEK",
    "WEEKDAY", "YEAR", "YEARWEEK",
];

pub(super) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

pub fn is_keyword(w: &str) -> bool {
    let u = w.to_ascii_uppercase();
    KEYWORDS.binary_search(&u.as_str()).is_ok()
        || matches!(u.as_str(), "SCHEMA" | "PROCEDURE" | "FUNCTION" | "RETURNS" | "DECLARE" | "WHILE" | "LOOP" | "DO")
}

/// Farbige Darstellung eines SQL-Textes.
pub(super) static FONT_SIZE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(13);

/// Schriftgroesse des Editors (Einstellungen)
pub fn set_font_size(px: u32) {
    FONT_SIZE.store(px.clamp(crate::settings::MIN_FONT, crate::settings::MAX_FONT), std::sync::atomic::Ordering::Relaxed);
}

pub fn font_size() -> f32 {
    FONT_SIZE.load(std::sync::atomic::Ordering::Relaxed) as f32
}

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
    let pal = crate::style::pal();
    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        let next = if i + 1 < n { chars[i + 1].1 } else { '\0' };
        let start = i;
        if (c == '-' && next == '-') || c == '#' {
            while i < n && chars[i].1 != '\n' {
                i += 1;
            }
            push(&mut job, start, i, pal.syn_comment);
        } else if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i].1 == '*' && i + 1 < n && chars[i + 1].1 == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push(&mut job, start, i, pal.syn_comment);
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < n && chars[i].1 != c {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, pal.syn_string);
        } else if c == '`' {
            i += 1;
            while i < n && chars[i].1 != '`' {
                i += 1;
            }
            i = (i + 1).min(n);
            push(&mut job, start, i, pal.syn_ident);
        } else if c.is_ascii_digit() {
            while i < n && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '.') {
                i += 1;
            }
            push(&mut job, start, i, pal.syn_number);
        } else if c == '@' {
            // @Variable, @@system_variable, @`name`
            while i < n && chars[i].1 == '@' {
                i += 1;
            }
            while i < n && (is_word_char(chars[i].1) || chars[i].1 == '.') {
                i += 1;
            }
            push(&mut job, start, i, pal.syn_variable);
        } else if is_word_char(c) {
            while i < n && is_word_char(chars[i].1) {
                i += 1;
            }
            let word = &text[byte(start)..byte(i)];
            let qualified = start > 0 && chars[start - 1].1 == '.';
            let qualifier = i < n && chars[i].1 == '.';
            let mut j = i;
            while j < n && chars[j].1 == ' ' {
                j += 1;
            }
            let call = j < n && chars[j].1 == '(';
            let upper = word.to_ascii_uppercase();
            let color = if qualified {
                pal.syn_text
            } else if qualifier {
                pal.syn_ident
            } else if call && FUNCTIONS.contains(&upper.as_str()) {
                pal.syn_function
            } else if matches!(upper.as_str(), "NULL" | "TRUE" | "FALSE" | "UNKNOWN") {
                pal.syn_const
            } else if crate::sqlfmt::is_type(word) {
                pal.syn_type
            } else if is_keyword(word) {
                pal.syn_keyword
            } else {
                pal.syn_text
            };
            push(&mut job, start, i, color);
        } else {
            // Operatoren abgesetzt, Klammern, Kommas und Leerraum in Textfarbe
            let op = |c: char| matches!(c, '=' | '<' | '>' | '!' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '~' | ':');
            let is_op = op(c);
            while i < n {
                let c = chars[i].1;
                let nx = if i + 1 < n { chars[i + 1].1 } else { '\0' };
                if is_word_char(c)
                    || c == '\''
                    || c == '"'
                    || c == '`'
                    || c == '#'
                    || c == '@'
                    || (c == '-' && nx == '-')
                    || (c == '/' && nx == '*')
                    || (i > start && op(c) != is_op)
                {
                    break;
                }
                i += 1;
            }
            if i == start {
                i += 1;
            }
            push(&mut job, start, i, if is_op { pal.syn_op } else { pal.syn_text });
        }
    }
    job
}

/// Reuse tokenization and glyph layout until text, font, theme or display scale changes.
#[derive(Default)]
pub struct SqlLayoutCache {
    entry: Option<SqlLayout>,
    #[cfg(test)]
    pub(super) builds: usize,
}

pub(super) struct SqlLayout {
    pub(super) text: String,
    pub(super) font: FontId,
    pub(super) dark: bool,
    pub(super) scale: f32,
    pub(super) galley: Arc<egui::Galley>,
}

impl SqlLayoutCache {
    pub fn layout(&mut self, ui: &egui::Ui, text: &str, font: &FontId) -> Arc<egui::Galley> {
        let dark = crate::style::pal().dark;
        let scale = ui.ctx().pixels_per_point();
        if let Some(entry) = &self.entry
            && entry.text == text && entry.font == *font && entry.dark == dark && entry.scale == scale {
                return Arc::clone(&entry.galley);
            }
        let mut job = highlight(text, font.clone());
        job.wrap.max_width = f32::INFINITY;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        self.entry = Some(SqlLayout { text: text.to_owned(), font: font.clone(), dark, scale, galley: Arc::clone(&galley) });
        #[cfg(test)] { self.builds += 1; }
        galley
    }
}

/// SQL formatieren (siehe sqlfmt)
pub fn format_sql(sql: &str) -> String {
    crate::sqlfmt::format_sql(sql)
}
