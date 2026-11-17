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
    "IF", "IFNULL", "INSTR", "ISNULL", "JSON_ARRAY", "JSON_EXTRACT", "JSON_OBJECT", "JSON_VALUE", "LAG",
    "LAST_DAY", "LAST_INSERT_ID", "LAST_VALUE", "LEAD", "LEAST", "LEFT", "LENGTH", "LN", "LOCATE",
    "LOG", "LOWER", "LPAD", "LTRIM", "MAX", "MD5", "MIN", "MINUTE", "MOD", "MONTH", "MONTHNAME",
    "NOW", "NTH_VALUE", "NTILE", "NULLIF", "PERCENT_RANK", "PI", "POSITION", "POW", "POWER",
    "QUARTER", "RAND", "RANK", "REGEXP_REPLACE", "REPEAT", "REPLACE", "REVERSE", "RIGHT", "ROUND",
    "ROW_NUMBER", "RPAD", "RTRIM", "SECOND", "SHA2", "SIGN", "SQRT", "STD", "STDDEV", "STR_TO_DATE",
    "SUBSTRING", "SUBSTRING_INDEX", "SUM", "TIME", "TIMEDIFF", "TIMESTAMPDIFF", "TIME_FORMAT",
    "TRIM", "TRUNCATE", "UNIX_TIMESTAMP", "UPPER", "USER", "UUID", "VARIANCE", "VERSION", "WEEK",
    "WEEKDAY", "YEAR", "YEARWEEK",
];

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

pub fn is_keyword(w: &str) -> bool {
    is_keyword_upper(&w.to_ascii_uppercase())
}

/// Wie `is_keyword` fuer ein schon gross geschriebenes Wort
pub fn is_keyword_upper(u: &str) -> bool {
    KEYWORDS.binary_search(&u).is_ok() || matches!(u, "SCHEMA" | "PROCEDURE" | "FUNCTION" | "RETURNS" | "DECLARE" | "WHILE" | "LOOP" | "DO")
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
    highlight_cached(text, font, &mut LineCache::default())
}

/// Zustand am Zeilenanfang: mitten in einem Blockkommentar, einer Zeichenkette oder einem `Bezeichner`
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum HlState {
    Normal,
    Block,
    Str(char),
    Tick,
}

/// Farbige Bereiche einer Zeile (Bytes relativ zum Zeilenanfang) und Zustand am Zeilenende
type LineSpans = (Vec<(u32, u32, Color32)>, HlState);

/// Faerbung je Zeile, damit beim Tippen nur geaenderte Zeilen neu bestimmt werden
#[derive(Default)]
pub(super) struct LineCache {
    map: std::collections::HashMap<(u64, HlState), LineSpans>,
    dark: bool,
}

/// Wie `highlight`, aber Zeilen, die sich seit dem letzten Aufruf nicht geaendert haben, kommen aus `cache`.
/// Vorher wurde bei jedem Tastendruck der ganze Text neu eingefaerbt (bei 5000 Zeilen rund 17 ms).
pub(super) fn highlight_cached(text: &str, font: FontId, cache: &mut LineCache) -> LayoutJob {
    use std::hash::{Hash, Hasher};
    let pal = crate::style::pal();
    if cache.dark != pal.dark {
        cache.map.clear();
        cache.dark = pal.dark;
    }
    let mut old = std::mem::take(&mut cache.map);
    let mut job = LayoutJob { text: text.to_owned(), ..Default::default() };
    let mut st = HlState::Normal;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        line.hash(&mut h);
        let key = (h.finish(), st);
        let entry = match old.remove(&key).or_else(|| cache.map.get(&key).cloned()) {
            Some(e) => e,
            None => highlight_line(line, st, pal),
        };
        for &(a, b, color) in &entry.0 {
            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(offset + a as usize)..egui::text::ByteIndex(offset + b as usize),
                format: TextFormat::simple(font.clone(), color),
            });
        }
        st = entry.1;
        offset += line.len();
        cache.map.insert(key, entry);
    }
    job
}

/// Faerbt eine Zeile (mit ihrem Zeilenumbruch) ab dem Zustand `st`.
fn highlight_line(line: &str, mut st: HlState, pal: &crate::style::Pal) -> LineSpans {
    let mut out: Vec<(u32, u32, Color32)> = Vec::new();
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let n = chars.len();
    let byte = |i: usize| if i < n { chars[i].0 } else { line.len() };
    let mut push = |a: usize, b: usize, color: Color32| {
        if b > a {
            out.push((byte(a) as u32, byte(b) as u32, color));
        }
    };
    let mut i = 0;
    // Fortsetzung aus der vorigen Zeile
    match st {
        HlState::Normal => {}
        HlState::Block => {
            while i < n && !(chars[i].1 == '*' && i + 1 < n && chars[i + 1].1 == '/') {
                i += 1;
            }
            if i < n {
                i = (i + 2).min(n);
                st = HlState::Normal;
            }
            push(0, i, pal.syn_comment);
        }
        HlState::Str(q) => {
            while i < n && chars[i].1 != q {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < n {
                i += 1;
                st = HlState::Normal;
            }
            i = i.min(n);
            push(0, i, pal.syn_string);
        }
        HlState::Tick => {
            while i < n && chars[i].1 != '`' {
                i += 1;
            }
            if i < n {
                i += 1;
                st = HlState::Normal;
            }
            push(0, i, pal.syn_ident);
        }
    }
    let mut upper = String::new();
    while i < n {
        let c = chars[i].1;
        let next = if i + 1 < n { chars[i + 1].1 } else { '\0' };
        let start = i;
        if (c == '-' && next == '-') || c == '#' {
            while i < n && chars[i].1 != '\n' {
                i += 1;
            }
            push(start, i, pal.syn_comment);
        } else if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i].1 == '*' && i + 1 < n && chars[i + 1].1 == '/') {
                i += 1;
            }
            if i < n {
                i = (i + 2).min(n);
            } else {
                i = n;
                st = HlState::Block;
            }
            push(start, i, pal.syn_comment);
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < n && chars[i].1 != c {
                if chars[i].1 == '\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < n {
                i += 1;
            } else {
                i = n;
                st = HlState::Str(c);
            }
            push(start, i, pal.syn_string);
        } else if c == '`' {
            i += 1;
            while i < n && chars[i].1 != '`' {
                i += 1;
            }
            if i < n {
                i += 1;
            } else {
                st = HlState::Tick;
            }
            push(start, i, pal.syn_ident);
        } else if c.is_ascii_digit() {
            while i < n && (chars[i].1.is_ascii_alphanumeric() || chars[i].1 == '.') {
                i += 1;
            }
            push(start, i, pal.syn_number);
        } else if c == '@' {
            // @Variable, @@system_variable, @`name`
            while i < n && chars[i].1 == '@' {
                i += 1;
            }
            while i < n && (is_word_char(chars[i].1) || chars[i].1 == '.') {
                i += 1;
            }
            push(start, i, pal.syn_variable);
        } else if is_word_char(c) {
            while i < n && is_word_char(chars[i].1) {
                i += 1;
            }
            let word = &line[byte(start)..byte(i)];
            let qualified = start > 0 && chars[start - 1].1 == '.';
            let qualifier = i < n && chars[i].1 == '.';
            let mut j = i;
            while j < n && chars[j].1 == ' ' {
                j += 1;
            }
            let call = j < n && chars[j].1 == '(';
            // Grossschreibung einmal je Wort statt dreimal (Funktion, Typ, Schluesselwort)
            upper.clear();
            upper.extend(word.chars().map(|c| c.to_ascii_uppercase()));
            let u = upper.as_str();
            let color = if qualified {
                pal.syn_text
            } else if qualifier {
                pal.syn_ident
            } else if call && FUNCTIONS.contains(&u) {
                pal.syn_function
            } else if matches!(u, "NULL" | "TRUE" | "FALSE" | "UNKNOWN") {
                pal.syn_const
            } else if crate::sqlfmt::is_type_upper(u) {
                pal.syn_type
            } else if is_keyword_upper(u) {
                pal.syn_keyword
            } else {
                pal.syn_text
            };
            push(start, i, color);
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
            push(start, i, if is_op { pal.syn_op } else { pal.syn_text });
        }
    }
    (out, st)
}

/// Ein Absatz (Zeile ohne `\n`, wie egui Text in Absaetze teilt) mit Faerbungszustand und fertigem Layout
struct Absatz {
    text: String,
    st_in: HlState,
    st_out: HlState,
    galley: Arc<egui::Galley>,
}

/// Layout je Zeile: Beim Tippen werden nur geaenderte Zeilen neu gesetzt und die uebrigen mit
/// `Galley::concat` angehaengt. Vorher hat egui bei jedem Tastendruck den ganzen Text samt aller Abschnitte
/// gehasht und in Absaetze zerlegt (5000 Zeilen: rund 11 ms fuer Faerbung und Layout).
#[derive(Default)]
struct Absaetze {
    liste: Vec<Absatz>,
    key: Option<(FontId, bool, u32)>,
    /// Fuellstand des Schrift-Atlas: sinkt er, hat egui die Schriften neu angelegt und alte Layouts sind ungueltig
    atlas: f32,
}

/// Text so in Absaetze teilen wie egui (`layout_each_paragraph_individually`): ohne `\n`, ein `\n` am Textende
/// gehoert zum letzten Absatz.
fn absatz_grenzen(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        if end == text.len() - 1 && text.ends_with('\n') {
            end += 1;
        }
        out.push((start, end));
        start = end + 1;
    }
    out
}

impl Absaetze {
    fn layout(&mut self, ui: &egui::Ui, text: &str, font: &FontId, dark: bool, scale: f32) -> Arc<egui::Galley> {
        let pal = crate::style::pal();
        let atlas = ui.fonts_mut(|f| f.font_atlas_fill_ratio());
        let key = (font.clone(), dark, scale.to_bits());
        if self.key.as_ref() != Some(&key) || atlas + 1e-6 < self.atlas {
            self.liste.clear();
            self.key = Some(key);
        }
        self.atlas = atlas;
        let grenzen = absatz_grenzen(text);
        let teile: Vec<&str> = grenzen.iter().map(|&(a, b)| &text[a..b]).collect();
        let alt = std::mem::take(&mut self.liste);
        // gleicher Anfang und gleiches Ende
        let mut p = 0;
        while p < alt.len() && p < teile.len() && alt[p].text == teile[p] {
            p += 1;
        }
        let mut e = 0;
        while e < alt.len() - p && e < teile.len() - p && alt[alt.len() - 1 - e].text == teile[teile.len() - 1 - e] {
            e += 1;
        }
        let n_alt = alt.len();
        let mut alt: Vec<Option<Absatz>> = alt.into_iter().map(Some).collect();
        let mut neu: Vec<Absatz> = Vec::with_capacity(teile.len());
        let mut st = HlState::Normal;
        let setzen = |ui: &egui::Ui, teil: &str, st: HlState, letzter: bool| -> Absatz {
            // Faerben mit Zeilenumbruch (fuer den Zustand danach), Layout ohne ihn (wie egui)
            let spans = if letzter { highlight_line(teil, st, pal) } else { highlight_line(&format!("{teil}\n"), st, pal) };
            let mut job = LayoutJob { text: teil.to_owned(), ..Default::default() };
            job.wrap.max_width = f32::INFINITY;
            let len = teil.len() as u32;
            for &(a, b, color) in &spans.0 {
                if a <= len {
                    job.sections.push(egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(a as usize)..egui::text::ByteIndex(b.min(len) as usize),
                        format: TextFormat::simple(font.clone(), color),
                    });
                }
            }
            if job.sections.is_empty() {
                job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(0), format: TextFormat::simple(font.clone(), pal.syn_text) });
            }
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            Absatz { text: teil.to_owned(), st_in: st, st_out: spans.1, galley }
        };
        let letzter = teile.len().saturating_sub(1);
        for (i, slot) in alt.iter_mut().enumerate().take(p) {
            let a = slot.take().expect("Absatz");
            st = a.st_out;
            neu.push(a);
            let _ = i;
        }
        for (i, teil) in teile.iter().enumerate().take(teile.len() - e).skip(p) {
            let a = setzen(ui, teil, st, i == letzter);
            st = a.st_out;
            neu.push(a);
        }
        for k in 0..e {
            let i = teile.len() - e + k;
            let a = alt[n_alt - e + k].take().expect("Absatz");
            // Aendert sich der Zustand davor (z. B. Kommentar geoeffnet), neu faerben, bis er wieder passt
            let a = if a.st_in == st { a } else { setzen(ui, teile[i], st, i == letzter) };
            st = a.st_out;
            neu.push(a);
        }
        let galleys: Vec<Arc<egui::Galley>> = neu.iter().map(|a| Arc::clone(&a.galley)).collect();
        self.liste = neu;
        let mut job = LayoutJob { text: text.to_owned(), ..Default::default() };
        job.wrap.max_width = f32::INFINITY;
        job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()), format: TextFormat::simple(font.clone(), pal.syn_text) });
        Arc::new(egui::Galley::concat(Arc::new(job), &galleys, scale))
    }
}

/// Reuse tokenization and glyph layout until text, font, theme or display scale changes.
#[derive(Default)]
pub struct SqlLayoutCache {
    entry: Option<SqlLayout>,
    /// Faerbung je Zeile (nur SQL)
    lines: LineCache,
    /// Layout je Zeile (nur SQL, mehrzeilig)
    absaetze: Absaetze,
    #[cfg(test)]
    pub(super) builds: usize,
}

pub(super) struct SqlLayout {
    pub(super) lang: super::Lang,
    pub(super) text: String,
    pub(super) font: FontId,
    pub(super) dark: bool,
    pub(super) scale: f32,
    pub(super) galley: Arc<egui::Galley>,
}

impl SqlLayoutCache {
    pub fn layout(&mut self, ui: &egui::Ui, text: &str, font: &FontId) -> Arc<egui::Galley> {
        self.layout_lang(ui, text, font, super::Lang::Sql)
    }

    pub fn layout_lang(&mut self, ui: &egui::Ui, text: &str, font: &FontId, lang: super::Lang) -> Arc<egui::Galley> {
        let dark = crate::style::pal().dark;
        let scale = ui.ctx().pixels_per_point();
        if let Some(entry) = &self.entry
            && entry.lang == lang && entry.text == text && entry.font == *font && entry.dark == dark && entry.scale == scale {
                return Arc::clone(&entry.galley);
            }
        let galley = if lang == super::Lang::Sql && text.contains('\n') {
            self.absaetze.layout(ui, text, font, dark, scale)
        } else {
            let mut job = if lang == super::Lang::Sql { highlight_cached(text, font.clone(), &mut self.lines) } else { super::lang::highlight_lang(text, font.clone(), lang) };
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        self.entry = Some(SqlLayout { lang, text: text.to_owned(), font: font.clone(), dark, scale, galley: Arc::clone(&galley) });
        #[cfg(test)] { self.builds += 1; }
        galley
    }
}

/// SQL formatieren (siehe sqlfmt)
pub fn format_sql(sql: &str) -> String {
    crate::sqlfmt::format_sql(sql)
}

#[cfg(test)]
pub(super) fn highlight_reference(text: &str, font: FontId) -> LayoutJob {
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


#[cfg(test)]
mod line_cache_tests {
    use super::*;

    /// Farbe je Byte, damit unterschiedliche Aufteilung in Abschnitte keine Rolle spielt
    fn colors(job: &LayoutJob) -> Vec<Color32> {
        let mut out = vec![Color32::TRANSPARENT; job.text.len()];
        for s in &job.sections {
            out[s.byte_range.start.0..s.byte_range.end.0].fill(s.format.color);
        }
        out
    }

    #[test]
    fn zeilenweise_faerbung_gleicht_der_bisherigen() {
        let font = FontId::monospace(13.0);
        let mut cases: Vec<String> = vec![
            "SELECT a, b.c FROM t WHERE x = 'it''s' AND y > 3.5e2 -- Ende\nORDER BY COUNT(*) DESC;".into(),
            "/* mehr-\nzeiliger\nKommentar */ SELECT 1;\n# Hash\nSELECT `a\nb` FROM t;".into(),
            "SELECT 'Zeile 1\nZeile 2 \\' noch' , \"x\\\ny\" FROM café;\n@var := @@global.x;".into(),
            "SELECT 1 /* offen".into(),
            "SELECT 'offen\n\nbis zum Ende".into(),
            "a.\nb (\n)  != <> <=>\n\n\n".into(),
        ];
        // Zufaellige Texte aus den heiklen Zeichen
        let alphabet: Vec<char> = "ab9_$é '\"`\\/*-#@.()=<>\n \nSELECTnull".chars().collect();
        let mut seed = 12345u64;
        for _ in 0..3000 {
            let mut t = String::new();
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
            let len = (seed % 120) as usize;
            for _ in 0..len {
                seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
                t.push(alphabet[(seed % alphabet.len() as u64) as usize]);
            }
            cases.push(t);
        }
        let mut cache = LineCache::default();
        for t in &cases {
            assert_eq!(colors(&highlight(t, font.clone())), colors(&highlight_reference(t, font.clone())), "{t:?}");
            // Mit gefuelltem Zwischenspeicher aus anderen Texten kommt dasselbe heraus
            assert_eq!(colors(&highlight_cached(t, font.clone(), &mut cache)), colors(&highlight_reference(t, font.clone())), "{t:?}");
        }
        // Tippen: ein Zeichen nach dem anderen in einen langen Text
        let mut t = "SELECT 1;\n".repeat(50);
        for (k, c) in "/* x */ 'a\nb' -- c\n".chars().enumerate() {
            t.insert(200 + k, c);
            assert_eq!(colors(&highlight_cached(&t, font.clone(), &mut cache)), colors(&highlight_reference(&t, font.clone())), "{t:?}");
        }
    }
}

#[cfg(test)]
mod probe_timing {
    #[test]
    #[ignore = "probe"]
    fn probe_highlight_vs_layout() {
        let ctx = eframe::egui::Context::default();
        let mut text = "SELECT b.id, b.name FROM perf.big b WHERE b.id > 100 AND b.name LIKE '%x%';\n".repeat(5000);
        let font = eframe::egui::FontId::monospace(13.0);
        let _ = ctx.run_ui(Default::default(), |ui| {
            for i in 0..5 {
                text.insert(text.len() / 2, 'x');
                let t = std::time::Instant::now();
                let mut job = super::super::lang::highlight_lang(&text, font.clone(), super::super::Lang::Sql);
                let h = t.elapsed();
                job.wrap.max_width = f32::INFINITY;
                let t = std::time::Instant::now();
                let _g = ui.fonts_mut(|f| f.layout_job(job));
                println!("round {i}: highlight {:?}, layout {:?}", h, t.elapsed());
            }
        });
    }
}

#[cfg(test)]
mod absatz_tests {
    use super::*;

    /// Das zeilenweise Layout muss genau dem von egui fuer den ganzen Text entsprechen (Zeilen, Zeichen, Lage),
    /// auch nach Einfuegen, Loeschen, Blockkommentaren und Zeilenumbruechen am Ende.
    #[test]
    fn zeilenweises_layout_gleicht_dem_ganzen() {
        let ctx = egui::Context::default();
        let font = FontId::monospace(13.0);
        let mut cache = SqlLayoutCache::default();
        let mut texte: Vec<String> = vec![
            "SELECT 1;\nSELECT 'a';\n".into(),
            "SELECT 1;\n/* offen\nSELECT 'a';\n".into(),
            "SELECT 1;\n/* offen */\nSELECT 'a';\n\n".into(),
            "\n\nx".into(),
            "SELECT `ä`, \"ü\"\n-- Kommentar\nFROM t\n".into(),
            "a\n".into(),
            "\n".into(),
        ];
        let mut lang = "SELECT id, name FROM kunde WHERE id > 100;\n".repeat(50);
        texte.push(lang.clone());
        lang.insert_str(400, "/* x\n");
        texte.push(lang.clone());
        lang.insert_str(800, "\n*/");
        texte.push(lang);
        for t in &texte {
            let _ = ctx.run_ui(Default::default(), |ui| {
                let mine = cache.layout(ui, t, &font);
                let mut job = highlight(t, font.clone());
                job.wrap.max_width = f32::INFINITY;
                let theirs = ui.fonts_mut(|f| f.layout_job(job));
                assert_eq!(mine.rows.len(), theirs.rows.len(), "{t:?}");
                assert_eq!(mine.job.text, *t);
                for (a, b) in mine.rows.iter().zip(theirs.rows.iter()) {
                    assert_eq!(a.pos, b.pos, "{t:?}");
                    assert_eq!(a.ends_with_newline, b.ends_with_newline, "{t:?}");
                    assert_eq!(a.char_count_excluding_newline(), b.char_count_excluding_newline(), "{t:?}");
                    let fa: Vec<_> = a.glyphs.iter().map(|g| (g.chr, g.pos)).collect();
                    let fb: Vec<_> = b.glyphs.iter().map(|g| (g.chr, g.pos)).collect();
                    assert_eq!(fa, fb, "{t:?}");
                }
                // Farben gleich
                for (a, b) in mine.rows.iter().zip(theirs.rows.iter()) {
                    let fa: Vec<Color32> = a.visuals.mesh.vertices.iter().map(|v| v.color).collect();
                    let fb: Vec<Color32> = b.visuals.mesh.vertices.iter().map(|v| v.color).collect();
                    assert_eq!(fa, fb, "{t:?}");
                }
                assert_eq!(mine.rect, theirs.rect, "{t:?}");
                for i in [0, t.chars().count() / 2, t.chars().count()] {
                    let c = egui::text::CCursor::new(i);
                    assert_eq!(mine.pos_from_cursor(c), theirs.pos_from_cursor(c), "{t:?} @ {i}");
                }
            });
        }
    }
}
