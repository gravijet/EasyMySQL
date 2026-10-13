// SQL-Code-Editor: Syntax-Hervorhebung, Zeilennummern, Autovervollstaendigung, Formatierung.

use eframe::egui::{
    self, Color32, FontId, Key, Modifiers, RichText, TextFormat,
    text::{CCursor, CCursorRange, LayoutJob},
};

use std::sync::Arc;
use std::time::{Duration, Instant};

mod highlight;
mod lint;
mod text;

pub use highlight::*;
pub use lint::*;
pub use text::*;

/// Vorschlaege beim Tippen (Einstellung)
pub static AUTO_SUGGEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// SQL-Fehler markieren (Einstellung)
pub static LINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// So lange muss das Tippen pausieren, bevor geprueft wird
const LINT_DELAY: Duration = Duration::from_millis(350);

/// Klammern automatisch schliessen (Einstellung)
pub static AUTO_CLOSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

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

/// Ergebnis der letzten Pruefung samt Zeichenhilfen
#[derive(Default)]
struct LintState {
    problems: Vec<Problem>,
    /// Text und Struktur (Zeiger), fuer die `problems` gelten
    text: String,
    schema: usize,
    computed: bool,
    /// Zuletzt gesehener Text und seit wann (Abwarten nach dem Tippen)
    seen: String,
    seen_at: Option<Instant>,
    /// Je Problem: (Zeile, Ende der Wellenlinie)
    marks: Vec<(usize, usize)>,
    /// Zeilen mit Problemen, aufsteigend
    lines: Vec<(usize, Severity)>,
    /// Gilt `problems` fuer den aktuellen Text? (waehrend des Tippens ausgeblendet)
    shown: bool,
}

pub struct SqlEditor {
    pub id: egui::Id,
    /// Ansicht (Zeilen je Seite, Zeilenhoehe, Scrollstand, Inhalts- und Fensterhoehe) fuer Bild rauf/runter
    view: (usize, f32, f32, f32, f32),
    /// Struktur der gewaehlten Datenbank (fuer die Pruefung) und alle Datenbanknamen
    pub schema: Option<Arc<crate::db::Schema>>,
    pub databases: Vec<String>,
    lint: LintState,
    layout_cache: SqlLayoutCache,
    popup: Option<Popup>,
    popup_rect: Option<egui::Rect>,
    prev_text: String,
    find: Option<Find>,
    goto: Option<(String, bool)>,
    scroll_to: Option<f32>,
    /// Zeile (0-basiert) mit Fehler -> rot unterstrichen
    pub error_line: Option<usize>,
    /// (Zeile, Spalte), 1-basiert
    pub line_col: (usize, usize),
    /// Cursor (Zeichenindex)
    pub cursor: usize,
    /// Suchen/Ersetzen/Gehe zu, von aussen angefordert (Menue, Befehle)
    pub pending: Option<crate::keymap::Cmd>,
}

#[derive(Default)]
pub struct EditorOutput {
    pub format: bool,
    pub changed: bool,
    pub selection: Option<String>,
}

impl SqlEditor {
    pub fn new(id: egui::Id) -> Self {
        Self {
            id,
            view: (10, 16.0, 0.0, 0.0, 0.0),
            schema: None,
            databases: Vec::new(),
            lint: LintState::default(),
            layout_cache: SqlLayoutCache::default(),
            popup: None,
            popup_rect: None,
            prev_text: String::new(),
            find: None,
            goto: None,
            scroll_to: None,
            error_line: None,
            line_col: (1, 1),
            cursor: 0,
            pending: None,
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
                        add(c, crate::i18n::text("Spalte"), &mut items);
                    }
                }
            }
            None => {
                for t in &words.tables {
                    add(t, crate::i18n::text("Tabelle"), &mut items);
                }
                for (_, c) in &words.columns {
                    add(c, crate::i18n::text("Spalte"), &mut items);
                }
                for f in FUNCTIONS {
                    add(f, crate::i18n::text("Funktion"), &mut items);
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

    fn lint_env(&self) -> Env<'_> {
        Env { schema: self.schema.as_deref(), databases: &self.databases }
    }

    /// Pruefung auffrischen: erst wenn kurz nicht getippt wurde.
    fn update_lint(&mut self, ctx: &egui::Context, text: &str) {
        if !LINT.load(std::sync::atomic::Ordering::Relaxed) {
            if self.lint.computed {
                self.lint = LintState::default();
            }
            return;
        }
        let schema = self.schema.as_ref().map_or(0, |s| Arc::as_ptr(s) as usize);
        if self.lint.computed && self.lint.schema == schema && self.lint.text == text {
            self.lint.shown = true;
            return;
        }
        if self.lint.seen != text {
            self.lint.seen = text.to_string();
            self.lint.seen_at = Some(Instant::now());
        }
        let waited = self.lint.seen_at.map_or(LINT_DELAY, |t| t.elapsed());
        if self.lint.computed && self.lint.schema == schema && waited < LINT_DELAY {
            self.lint.shown = false;
            ctx.request_repaint_after(LINT_DELAY - waited);
            return;
        }
        let problems = lint(text, &self.lint_env());
        // Zeilenanfaenge einmal bestimmen: Zeile und Ende der Wellenlinie je Problem
        let mut starts = vec![0usize];
        let mut n = 0;
        for (i, c) in text.chars().enumerate() {
            n = i + 1;
            if c == '\n' {
                starts.push(i + 1);
            }
        }
        let mut lines: Vec<(usize, Severity)> = Vec::new();
        let marks = problems
            .iter()
            .map(|p| {
                let line = starts.partition_point(|&s| s <= p.start).saturating_sub(1);
                let line_end = starts.get(line + 1).map_or(n, |s| s - 1);
                match lines.last_mut() {
                    Some((l, sev)) if *l == line => {
                        if p.severity == Severity::Error {
                            *sev = Severity::Error;
                        }
                    }
                    _ => lines.push((line, p.severity)),
                }
                (line, p.end.clamp(p.start + 1, line_end.max(p.start + 1)))
            })
            .collect();
        self.lint = LintState { problems, text: text.to_string(), schema, computed: true, seen: String::new(), seen_at: None, marks, lines, shown: true };
    }

    /// (Fehler, Hinweise, korrigierbar) der aktuell angezeigten Pruefung
    pub fn lint_summary(&self) -> (usize, usize, usize) {
        if !self.lint.shown {
            return (0, 0, 0);
        }
        let errors = self.lint.problems.iter().filter(|p| p.severity == Severity::Error).count();
        let fixable = self.lint.problems.iter().filter(|p| p.fix.is_some()).count();
        (errors, self.lint.problems.len() - errors, fixable)
    }

    /// Alle Fehler, fuer die es einen Vorschlag gibt, selbst ausbessern (bis zu dreimal, weil sich nach einer
    /// Korrektur weitere Fehler zeigen koennen). Liefert die Zahl der Korrekturen.
    pub fn fix_all(&mut self, text: &mut String) -> usize {
        let mut total = 0;
        for _ in 0..3 {
            let problems = lint(text, &self.lint_env());
            let (fixed, n) = apply_fixes(text, &problems);
            if n == 0 {
                break;
            }
            *text = fixed;
            total += n;
        }
        if total > 0 {
            self.prev_text = text.clone();
            self.lint.computed = false;
        }
        total
    }

    /// Oeffnet die Suchleiste (mit markiertem Text als Suchbegriff) bzw. Ersetzen / Gehe zu.
    pub fn open_find(&mut self, ctx: &egui::Context, text: &str, replace: bool) {
        let sel = self.selection(ctx).filter(|(a, b)| a != b).map(|(a, b)| text.chars().skip(a).take(b - a).collect::<String>());
        let f = self.find.get_or_insert_with(Find::default);
        if let Some(s) = sel.filter(|s| !s.contains('\n')) {
            f.query = s;
        }
        if replace {
            f.show_replace = true;
        }
        f.focus = true;
    }

    pub fn open_goto(&mut self) {
        self.goto = Some((String::new(), true));
    }

    /// Cursor um eine Seite nach oben/unten und die Ansicht gleich weit mitnehmen.
    fn page_move(&mut self, ctx: &egui::Context, text: &str, dir: i64, extend: bool) {
        let Some(st) = egui::TextEdit::load_state(ctx, self.id) else { return };
        let Some(range) = st.cursor.char_range() else { return };
        let (primary, secondary) = (range.primary.index.0, range.secondary.index.0);
        let (line, col) = line_col(text, primary);
        let (rows, row_h, offset, content, view) = self.view;
        let step = rows.saturating_sub(1).max(1) as i64;
        let last = text.split('\n').count() as i64 - 1;
        let target = (line as i64 + dir * step).clamp(0, last.max(0)) as usize;
        let len = text.split('\n').nth(target).map_or(0, |l| l.chars().count());
        let pos = line_start(text, target) + col.min(len);
        self.set_selection(ctx, if extend { secondary } else { pos }, pos);
        self.scroll_to = Some((offset + dir as f32 * step as f32 * row_h).clamp(0.0, (content - view).max(0.0)));
    }

    /// Tastenkuerzel, die den Text veraendern (vor dem Zeichnen des Editors).
    fn handle_keys(&mut self, ui: &mut egui::Ui, text: &mut String, out: &mut EditorOutput) {
        use crate::keymap::{Cmd, take};
        let ctx = ui.ctx().clone();
        if take(ui, Cmd::Find) {
            self.open_find(&ctx, text, false);
        }
        if take(ui, Cmd::Replace) {
            self.open_find(&ctx, text, true);
        }
        if take(ui, Cmd::GotoLine) {
            self.open_goto();
        }
        if take(ui, Cmd::Format) {
            out.format = true;
        }

        // Bild rauf/runter: Cursor und Ansicht um eine Seite verschieben (mit Umschalt: Auswahl erweitern)
        for (key, dir) in [(Key::PageUp, -1i64), (Key::PageDown, 1)] {
            for extend in [false, true] {
                let mods = if extend { Modifiers::SHIFT } else { Modifiers::NONE };
                if ui.input_mut(|i| i.consume_key(mods, key)) {
                    self.popup = None;
                    self.page_move(&ctx, text, dir, extend);
                }
            }
        }

        let Some((a, b)) = self.selection(&ctx) else { return };
        let (first, _) = line_col(text, a);
        let (mut last, last_col) = line_col(text, b);
        if b > a && last_col == 0 && last > first {
            last -= 1; // Auswahl endet am Zeilenanfang
        }
        let changed = |this: &mut Self, t: &str, out: &mut EditorOutput| {
            out.changed = true;
            this.prev_text = t.to_string();
        };

        if take(ui, Cmd::ToggleComment) {
            *text = toggle_comment(text, first, last);
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(if last + 1 < text.split('\n').count() { 1 } else { 0 }));
            changed(self, text, out);
        }
        // Strg+X ohne Markierung: ganze Zeile ausschneiden (mit Markierung normales Ausschneiden)
        if a == b && take(ui, Cmd::CutLine) {
            let (t, removed, cur) = remove_lines(text, first, last);
            ctx.copy_text(removed);
            *text = t;
            self.set_cursor(&ctx, cur);
            changed(self, text, out);
        }
        if take(ui, Cmd::DeleteLine) {
            let (t, _, cur) = remove_lines(text, first, last);
            *text = t;
            self.set_cursor(&ctx, cur);
            changed(self, text, out);
        }
        // Strg+D: Zeile(n) darunter duplizieren, Cursor und Markierung bleiben stehen
        if take(ui, Cmd::DuplicateLine) {
            *text = duplicate_lines(text, first, last);
            self.set_selection(&ctx, a, b);
            changed(self, text, out);
        }
        if take(ui, Cmd::SelectLine) {
            let end = line_start(text, last + 1);
            self.set_selection(&ctx, line_start(text, first), end);
        }
        for (up, mv, cp) in [(true, Cmd::MoveLineUp, Cmd::CopyLineUp), (false, Cmd::MoveLineDown, Cmd::CopyLineDown)] {
            let dir = if up { -1 } else { 1 };
            if take(ui, mv)
                && let Some((t, nf)) = move_lines(text, first, last, dir) {
                    *text = t;
                    let span = last - first;
                    self.set_selection(&ctx, line_start(text, nf), line_start(text, nf + span + 1).saturating_sub(1).max(line_start(text, nf)));
                    changed(self, text, out);
                }
            if take(ui, cp) {
                *text = duplicate_lines(text, first, last);
                if !up {
                    let span = last - first;
                    let nf = last + 1;
                    self.set_selection(&ctx, line_start(text, nf), line_start(text, nf + span + 1).saturating_sub(1).max(line_start(text, nf)));
                }
                changed(self, text, out);
            }
        }
        // Tab / Umschalt+Tab bei mehrzeiliger Auswahl: Block einruecken
        if self.popup.is_none() && last > first && take(ui, Cmd::Indent) {
            *text = map_lines(text, first, last, |l| format!("  {l}"));
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1));
            changed(self, text, out);
        }
        if take(ui, Cmd::Outdent) {
            *text = map_lines(text, first, last, |l| {
                let n = l.chars().take(2).take_while(|c| *c == ' ').count();
                l.strip_prefix('\t').unwrap_or(&l[n..]).to_string()
            });
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1).max(line_start(text, first)));
            changed(self, text, out);
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
            if let Some(cl) = close
                && next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';')) {
                    let bi = byte_of(text, cursor);
                    text.insert(bi, cl);
                    self.set_cursor(ctx, cursor);
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
                ui.label(crate::i18n::text("Suchen:"));
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
                ui.toggle_value(&mut f.case, "Aa").on_hover_text(crate::i18n::text("Groß-/Kleinschreibung beachten"));
                let info = if f.query.is_empty() {
                    String::new()
                } else if matches.is_empty() {
                    crate::i18n::text("keine Treffer").into()
                } else {
                    crate::tr_format!("{} von {}", "{} of {}", (f.current % matches.len()) + 1, matches.len())
                };
                ui.label(RichText::new(info).small());
                if crate::icons::button(ui, crate::icons::Icon::Up, crate::i18n::text("Vorheriger")).clicked() && !matches.is_empty() {
                    f.current = (f.current + matches.len() - 1) % matches.len();
                    jump = Some(f.current);
                }
                if (crate::icons::button(ui, crate::icons::Icon::Down, crate::i18n::text("Nächster (Enter)")).clicked() || enter) && !matches.is_empty() {
                    if enter {
                        f.current = (f.current + 1) % matches.len();
                        r.request_focus();
                    } else {
                        f.current = (f.current + 1) % matches.len();
                    }
                    jump = Some(f.current);
                }
                ui.toggle_value(&mut f.show_replace, crate::i18n::text("Ersetzen"));
                if crate::icons::button(ui, crate::icons::Icon::Close, crate::i18n::text("Schließen (Esc)")).clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    close = true;
                }
            });
            if f.show_replace {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::text("Ersetzen:"));
                    ui.add(egui::TextEdit::singleline(&mut f.replace).desired_width(200.0));
                    if ui.small_button(crate::i18n::text("Ersetzen")).clicked() {
                        replace_one = true;
                    }
                    if ui.small_button(crate::i18n::text("Alle ersetzen")).clicked() {
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
        let font = FontId::monospace(font_size());
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
        let force = focused && crate::keymap::take(ui, crate::keymap::Cmd::Suggest);
        match self.pending.take() {
            Some(crate::keymap::Cmd::Find) => self.open_find(&ctx, text, false),
            Some(crate::keymap::Cmd::Replace) => self.open_find(&ctx, text, true),
            Some(crate::keymap::Cmd::GotoLine) => self.open_goto(),
            _ => {}
        }
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
                    ui.label(crate::tr_format!("Gehe zu Zeile (1–{}):", "Go to line (1–{}):", text.split('\n').count()));
                    let r = ui.add(egui::TextEdit::singleline(line).desired_width(80.0));
                    if *first {
                        r.request_focus();
                        *first = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = line.trim().parse::<usize>().ok();
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) || crate::icons::button(ui, crate::icons::Icon::Close, "").clicked() {
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

        self.update_lint(&ctx, text);
        // Hoehe der Leisten vom Editor abziehen
        let height = (height - (ui.cursor().top() - top0)).max(60.0);
        let lines = text.split('\n').count().max(1);
        let digits = lines.to_string().len().max(2);
        let gutter_w = digits as f32 * 8.0 + 18.0;
        // Wie in VS Code laesst sich ueber die letzte Zeile hinaus scrollen: unter dem Text bleibt Platz
        // fuer die Hoehe des Fensters abzueglich einer Zeile.
        let visible_rows = (((height - 8.0) / row_h).floor().max(3.0)) as usize;
        let total_rows = lines + visible_rows - 1;
        self.view.0 = visible_rows;
        self.view.1 = row_h;

        let mut layout_cache = std::mem::take(&mut self.layout_cache);
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            layout_cache.layout(ui, buf.as_str(), &font)
        };

        let frame = egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border));
        let mut result = None;
        let mut scroll_state: Option<(f32, f32, egui::Rect)> = None;
        let mut lint_hits: Vec<(egui::Rect, usize)> = Vec::new();
        let find_q = self.find.as_ref().map(|f| (f.query.clone(), f.case));
        frame.show(ui, |ui| {
            ui.set_min_height(height);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut sa = egui::ScrollArea::both()
                    .id_salt(self.id.with("scroll"))
                    .max_height(height)
                    .min_scrolled_height(height)
                    .auto_shrink([false, false]);
                if let Some(y) = self.scroll_to.take() {
                    sa = sa.vertical_scroll_offset(y);
                }
                let so = sa.show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (gutter, _) = ui.allocate_exact_size(
                            egui::vec2(gutter_w, (total_rows as f32 * row_h + 8.0).max(height - 4.0)),
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
                            .desired_rows(total_rows)
                            .lock_focus(true)
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
                            if let Some(i) = at
                                && let Some(j) = matching_bracket(&chars, i) {
                                    for k in [i, j] {
                                        let a = pos_of(k);
                                        let b = pos_of(k + 1);
                                        bg.push(egui::Shape::rect_filled(egui::Rect::from_min_max(a.left_top(), egui::pos2(b.left(), a.bottom())), 2.0, pal.bracket));
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
                        // Pruefung: Wellenlinien unter den Problemen (nur sichtbare Zeilen werden berechnet)
                        if self.lint.shown {
                            let clip = ui.clip_rect();
                            for (k, prob) in self.lint.problems.iter().enumerate() {
                                let (line, end) = self.lint.marks[k];
                                let y0 = gp.y + line as f32 * row_h;
                                if y0 + row_h < clip.top() || y0 > clip.bottom() {
                                    continue;
                                }
                                let (x0, x1) = (pos_of(prob.start).left(), pos_of(end).left().max(pos_of(prob.start).left() + 6.0));
                                let color = if prob.severity == Severity::Error { pal.error_text } else { pal.warn_text };
                                ui.painter().add(squiggle(x0, x1, y0 + row_h - 1.0, color));
                                lint_hits.push((egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y0 + row_h)), k));
                            }
                        }
                        // Zeilennummern
                        let p = ui.painter();
                        p.rect_filled(gutter, 0.0, pal.bg);
                        let cur_line = cur.map(|c| line_col(text, c).0);
                        // Text never wraps: each galley row is one source line.
                        // Only emit line-number shapes inside the visible viewport.
                        let clip = ui.clip_rect().expand(row_h);
                        let first = o.galley.rows.partition_point(|row| gp.y + row.pos.y + row_h < clip.top());
                        let last = o.galley.rows.partition_point(|row| gp.y + row.pos.y <= clip.bottom());
                        for (index, row) in o.galley.rows[first..last].iter().enumerate() {
                            let n = first + index + 1;
                            {
                                let active = cur_line == Some(n - 1);
                                let err = self.error_line == Some(n - 1);
                                if self.lint.shown
                                    && let Ok(k) = self.lint.lines.binary_search_by_key(&(n - 1), |(l, _)| *l)
                                {
                                    let color = if self.lint.lines[k].1 == Severity::Error { pal.error_text } else { pal.warn_text };
                                    p.circle_filled(egui::pos2(gutter.left() + 6.0, gp.y + row.pos.y + row_h / 2.0), 2.5, color);
                                }
                                p.text(
                                    egui::pos2(gutter.right() - 8.0, gp.y + row.pos.y),
                                    egui::Align2::RIGHT_TOP,
                                    n.to_string(),
                                    font.clone(),
                                    if err { pal.error_text } else if active { pal.text } else { pal.syn_gutter },
                                );
                            }
                        }
                        result = Some(o);
                    });
                });
                scroll_state = Some((so.state.offset.y, so.content_size.y, so.inner_rect));
                self.view.2 = so.state.offset.y;
                self.view.3 = so.content_size.y;
                self.view.4 = so.inner_rect.height();

            });
        });
        self.layout_cache = layout_cache;
        let Some(o) = result else { return out };
        {
            use std::sync::atomic::Ordering;
            if o.response.response.has_focus() {
                FOCUSED_EDITOR.store(self.id.value(), Ordering::Relaxed);
            } else if FOCUSED_EDITOR.load(Ordering::Relaxed) == self.id.value() {
                FOCUSED_EDITOR.store(0, Ordering::Relaxed);
            }
        }

        // Hinweis zum Problem unter dem Mauszeiger
        if let Some(pos) = ctx.pointer_hover_pos()
            && o.response.response.hovered()
            && let Some((_, k)) = lint_hits.iter().find(|(r, _)| r.expand2(egui::vec2(0.0, 1.0)).contains(pos))
        {
            let prob = &self.lint.problems[*k];
            let mut tip = prob.message.clone();
            if let Some(f) = &prob.fix {
                tip.push_str(&format!("\n\n{}  ({})", crate::tr_format!("Korrigieren: {}", "Fix: {}", if f.text.is_empty() { crate::i18n::text("entfernen").to_string() } else { f.text.clone() }), crate::keymap::text(crate::keymap::Cmd::FixAll)));
            }
            o.response.response.clone().on_hover_text_at_pointer(tip);
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
            if let Some(c) = cursor
                && AUTO_CLOSE.load(std::sync::atomic::Ordering::Relaxed) {
                    self.auto_close(&ctx, text, c);
                }
            self.error_line = None;
        }
        if out.changed {
            self.prev_text = text.clone();
        }

        // Vorschlaege aktualisieren
        let auto = AUTO_SUGGEST.load(std::sync::atomic::Ordering::Relaxed);
        if (o.response.response.changed() && auto) || force {
            self.popup = cursor.and_then(|c| Self::suggestions(text, c, words, force));
        } else if let (Some(p), Some(c)) = (&self.popup, cursor)
            && p.cursor != c {
                self.popup = None;
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

/// Wellenlinie unter Text von `x0` bis `x1` auf Hoehe `y`
fn squiggle(x0: f32, x1: f32, y: f32, color: Color32) -> egui::Shape {
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = true;
    while x < x1 {
        pts.push(egui::pos2(x, if up { y - 2.0 } else { y }));
        x += 2.5;
        up = !up;
    }
    pts.push(egui::pos2(x1, y));
    egui::Shape::line(pts, egui::Stroke::new(1.2, color))
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
        assert_eq!(format_sql("select a,b from t where x=1;"), "SELECT a, b\nFROM t\nWHERE x = 1;\n");
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
        assert_eq!(duplicate_lines(t, 0, 1), "a\nb\na\nb\nc");
    }

    #[test]
    fn remove() {
        let t = "a\nb\nc";
        assert_eq!(remove_lines(t, 1, 1), ("a\nc".to_string(), "b\n".to_string(), 2));
        assert_eq!(remove_lines(t, 2, 2), ("a\nb".to_string(), "c\n".to_string(), 2));
        assert_eq!(remove_lines(t, 0, 2), (String::new(), "a\nb\nc\n".to_string(), 0));
        assert_eq!(remove_lines("x", 0, 0).0, "");
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

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    fn layout_cache_reuses_and_invalidates_after_edit_font_and_zoom() {
        let ctx = egui::Context::default();
        let mut cache = SqlLayoutCache::default();
        let mut text = "SELECT 'café', 1;".to_string();
        let mut font = FontId::monospace(13.0);
        let mut previous = None;
        for stage in 0..5 {
            if stage == 2 { text.push_str("\nSELECT 2;"); }
            if stage == 3 { font.size = 18.0; }
            if stage == 4 { ctx.set_pixels_per_point(1.5); }
            let _ = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 480.0))),
                ..Default::default()
            }, |ui| {
                let galley = cache.layout(ui, &text, &font);
                assert_eq!(galley.job.text, text);
                if let Some(old) = &previous {
                    assert_eq!(Arc::ptr_eq(old, &galley), stage == 1);
                }
                previous = Some(galley);
            });
        }
        assert_eq!(cache.builds, 4);
    }

    #[test]
    #[ignore = "manual render benchmark"]
    fn performance_render_large_editor() {
        let ctx = egui::Context::default();
        let mut editor = SqlEditor::new(egui::Id::new("performance-editor"));
        let mut text = "SELECT id, name FROM customers WHERE id > 100;\n".repeat(5000);
        let words = Words::default();
        let mut render = || {
            let _ = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 760.0))),
                ..Default::default()
            }, |ui| { editor.show(ui, &mut text, &words, 700.0); });
        };
        for _ in 0..3 { render(); }
        let start = std::time::Instant::now();
        for _ in 0..10 { render(); }
        println!("5,000 SQL lines: {:.3} ms/frame", start.elapsed().as_secs_f64() * 1000.0 / 10.0);
    }
}
