// SQL-Code-Editor: Syntax-Hervorhebung, Zeilennummern, Autovervollstaendigung, Formatierung.

use eframe::egui::{
    self, Color32, FontId, Key, Modifiers, RichText, TextFormat,
    text::{CCursor, CCursorRange, LayoutJob},
};

use std::sync::Arc;
use std::time::{Duration, Instant};

mod complete;
mod highlight;
pub mod hover;
pub mod refactor;
mod menu;
mod lang;
mod lint;
mod schema_fix;
mod schema_decl;
mod schema_jpa;
mod schema_java;
pub(crate) use schema_decl::{TabellenPlan, migrationen, plan_sql, pruefe_daten, bestehender_plan};
pub(crate) use schema_jpa::jpa_modelle;
pub(crate) use schema_fix::java_struktur_projekt;
#[cfg(test)]
pub(crate) use schema_fix::java_struktur;
pub(crate) use schema_java::abfragen as java_abfragen;
mod text;

pub use highlight::*;
pub use lang::Lang;
pub use lint::*;
pub use text::*;

/// Vorschlaege beim Tippen (Einstellung)
pub static AUTO_SUGGEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// SQL-Fehler markieren (Einstellung)
pub static LINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Einstellungen des Editors (fuer alle Editoren gleich)
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// Einrueckung in Leerzeichen (0 = je nach Sprache: Java 4, sonst 2)
    pub indent: usize,
    pub line_numbers: bool,
    /// Zeile mit dem Cursor hinterlegen
    pub highlight_line: bool,
    /// Zusammengehoerige Klammer markieren
    pub match_brackets: bool,
    /// Nach Enter einruecken (und nach `}` ausruecken)
    pub auto_indent: bool,
    /// SQL in Zeichenketten von Java pruefen
    pub sql_in_strings: bool,
    /// So lange muss das Tippen pausieren, bevor geprueft wird
    pub lint_delay_ms: u64,
    /// Ueber die letzte Zeile hinaus scrollen
    pub scroll_past_end: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { indent: 0, line_numbers: true, highlight_line: true, match_brackets: true, auto_indent: true, sql_in_strings: true, lint_delay_ms: 350, scroll_past_end: true }
    }
}

static PREFS: std::sync::RwLock<Option<Prefs>> = std::sync::RwLock::new(None);

pub fn prefs() -> Prefs {
    PREFS.read().unwrap().clone().unwrap_or_default()
}

pub fn set_prefs(p: Prefs) {
    *PREFS.write().unwrap() = Some(p);
}

fn lint_delay() -> Duration {
    Duration::from_millis(prefs().lint_delay_ms.clamp(0, 5000))
}

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

struct Popup {
    items: Vec<complete::Item>,
    selected: usize,
    word_start: usize,
    cursor: usize,
    /// Je Eintrag ein fehlender Import (Vorschlaege des Sprachservers)
    imports: Vec<Option<String>>,
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
    /// Sprache der Datei (SQL, Java, …): Hervorhebung, Pruefung und Vorschlaege richten sich danach
    pub lang: Lang,
    /// Java: Gibt es im Paket eine Klasse `Database` (Vorlage "connection" nutzt sie dann)?
    pub has_database_class: bool,
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
    /// Der Cursor wurde nach der Eingabe versetzt (Einruecken); gilt fuer dieses Bild
    cursor_moved: Option<()>,
    /// Meldungen des Compilers (bis zur naechsten Aenderung am Text)
    diagnostics: Vec<Problem>,
    /// Meldungen des Sprachservers (gelten fuer den Text, zu dem sie kamen)
    external: Vec<Problem>,
    /// Haltepunkte (Zeilen, 0-basiert) und ob man sie per Klick setzen kann
    pub breakpoints: Vec<usize>,
    pub breakpoints_enabled: bool,
    /// Zeile, an der der Debugger gerade steht
    pub exec_line: Option<usize>,
    /// Cursor beim naechsten Zeichnen setzen und dorthin scrollen (Sitzung wiederherstellen)
    pub restore: Option<usize>,
    /// Wort unter dem Mauszeiger (Anfang) und seit wann: Hinweis zu Tabellen, Spalten und Funktionen
    hover_at: Option<(usize, Instant)>,
    /// Auswahlliste Generieren / Umgeben mit
    picker: Option<menu::Picker>,
    /// Fruehere Auswahlen (Auswahl verkleinern)
    sel_stack: Vec<(usize, usize)>,
    /// Umbenennen in dieser Datei
    rename: Option<menu::LocalRename>,
    /// Lage des Cursors auf dem Bildschirm (letztes Bild): dort erscheinen Auswahlliste und Umbenennen
    caret_screen: egui::Pos2,
    /// Beim ersten Zeichnen den Fokus nehmen (neue Abfrage, geoeffnete Datei)
    focus_next: bool,
}

#[derive(Default)]
pub struct EditorOutput {
    pub format: bool,
    pub changed: bool,
    pub selection: Option<String>,
    /// Klick neben eine Zeilennummer: Haltepunkt umschalten
    pub toggle_breakpoint: Option<usize>,
    /// Strg+Klick auf diese Stelle (Zeichenindex)
    pub ctrl_click: Option<usize>,
    /// Zeichen unter dem Mauszeiger
    pub hover: Option<usize>,
    /// Vorschlaege des Sprachservers fuer diese Cursorstelle holen (Java-Code)
    pub complete_at: Option<usize>,
    /// Ein angenommener Vorschlag braucht diesen Import
    pub add_import: Option<String>,
    /// Befehl aus dem Rechtsklickmenue fuer die Registerkarte (Ausfuehren, Formatieren, Umbenennen …)
    pub command: Option<crate::keymap::Cmd>,
    /// Kurze Meldung fuer die Statuszeile
    pub notice: Option<String>,
}

impl SqlEditor {
    pub fn new(id: egui::Id) -> Self {
        Self {
            id,
            lang: Lang::Sql,
            has_database_class: false,
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
            cursor_moved: None,
            diagnostics: Vec::new(),
            external: Vec::new(),
            breakpoints: Vec::new(),
            breakpoints_enabled: false,
            exec_line: None,
            restore: None,
            hover_at: None,
            picker: None,
            sel_stack: Vec::new(),
            rename: None,
            caret_screen: egui::Pos2::ZERO,
            focus_next: true,
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
        // Zeilen weit oben brauchen kein Scrollen (sonst verschwinden bei kurzen Dateien die ersten Zeilen)
        let y = line as f32 * row_h;
        // Vor dem ersten Zeichnen kennt der Editor seine Hoehe noch nicht: dann die des Fensters schaetzen
        let view_h = if self.view.4 > 0.0 { self.view.4 } else { (ctx.content_rect().height() - 220.0).max(200.0) };
        let (top, _) = (self.view.2, ());
        let visible = y >= top && y + row_h <= top + view_h - row_h;
        if !visible {
            self.scroll_to = Some(if y < view_h * 0.75 { 0.0 } else { (y - view_h / 3.0).max(0.0) });
        }
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    fn accept(&mut self, ctx: &egui::Context, text: &mut String) -> Option<String> {
        let p = self.popup.take()?;
        let item = p.items.get(p.selected)?;
        let import = p.imports.get(p.selected).cloned().flatten();
        let a = byte_of(text, p.word_start);
        let b = byte_of(text, p.cursor);
        // Mehrzeilige Vorlagen in Java & Co. bekommen die Einrueckung der aktuellen Zeile
        let indent: String = if self.lang != Lang::Sql {
            let line_start = text[..a].rfind('\n').map_or(0, |i| i + 1);
            text[line_start..a].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
        } else {
            String::new()
        };
        let indented = |s: &str| if indent.is_empty() { s.to_string() } else { s.replace('\n', &format!("\n{indent}")) };
        let insert = indented(&item.insert);
        // `back` zaehlt vom Ende der Vorlage: hinter dem Cursor steht derselbe Rest, ebenfalls eingerueckt
        let rest: String = item.insert.chars().skip(item.insert.chars().count() - item.back).collect();
        text.replace_range(a..b, &insert);
        let end = p.word_start + insert.chars().count() - indented(&rest).chars().count();
        self.set_cursor(ctx, end);
        self.prev_text = text.clone();
        // Nach "alias." gleich die Spalten anbieten
        if item.insert.ends_with('.') {
            self.popup = self.suggest(text, end, true).map(|s| Popup { items: s.items, selected: 0, word_start: s.word_start, cursor: end, imports: Vec::new() });
        }
        import
    }

    /// Cursorstelle laut Editor (auch direkt nach einer Eingabe im selben Bild)
    pub fn caret(&self, ctx: &egui::Context) -> usize {
        self.selection(ctx).map_or(self.cursor, |(_, b)| b)
    }

    /// Cursor setzen (von aussen, z. B. nach einem eingefuegten Import)
    pub fn place_cursor(&mut self, ctx: &egui::Context, pos: usize) {
        self.set_cursor(ctx, pos);
        self.cursor = pos;
    }

    /// Vorschlaege des Sprachservers zeigen, wenn der Cursor noch an derselben Stelle steht. Eigene Vorschlaege
    /// (Vorlagen, Variablen) bleiben dahinter erhalten.
    pub fn offer(&mut self, cursor: usize, word_start: usize, list: Vec<crate::dev::lsp::Completion>) {
        if self.cursor != cursor || list.is_empty() {
            return;
        }
        let mut items: Vec<complete::Item> = Vec::new();
        let mut imports: Vec<Option<String>> = Vec::new();
        for c in list {
            items.push(complete::Item { label: c.label, insert: c.insert, kind: c.kind, detail: c.detail, back: c.back });
            imports.push(c.import);
        }
        let local = self.popup.take().filter(|p| p.cursor == cursor);
        if let Some(p) = local {
            // Vorlagen und Datenbank-Vorschlaege behalten, Doppeltes weglassen
            for it in p.items {
                if !items.iter().any(|x| x.insert == it.insert) && (it.kind == crate::i18n::tr("Vorlage", "Snippet") || items.len() < 8) {
                    items.push(it);
                    imports.push(None);
                }
            }
        }
        self.popup = Some(Popup { items, selected: 0, word_start: word_start.min(cursor), cursor, imports });
    }

    fn lint_env(&self) -> Env<'_> {
        Env { schema: self.schema.as_deref(), databases: &self.databases }
    }

    /// Vorschlaege passend zur Sprache
    fn suggest(&self, text: &str, cursor: usize, force: bool) -> Option<complete::Suggestion> {
        match self.lang {
            Lang::Sql => complete::suggest(text, cursor, &self.lint_env(), force),
            Lang::Java => lang::suggest_java(text, cursor, &self.lint_env(), force, self.has_database_class),
            _ => None,
        }
    }

    /// Probleme passend zur Sprache (Java: das SQL in den Zeichenketten) und Meldungen des Compilers
    fn problems(&self, text: &str) -> Vec<Problem> {
        let mut v = match self.lang {
            Lang::Sql => lint(text, &self.lint_env()),
            Lang::Java if prefs().sql_in_strings => lang::code_sql_problems(text, &self.lint_env(), self.lang),
            _ => Vec::new(),
        };
        if self.lang == Lang::Java {
            v.extend(crate::dev::autofix::java_probleme(text));
        }
        v.extend(self.diagnostics.iter().cloned());
        v.extend(self.external.iter().cloned());
        if self.lang == Lang::Java {
            for p in &mut v {
                if p.fix.is_some() {
                    continue;
                }
                let (line, col) = line_col(text, p.start);
                let diag = crate::dev::process::Diag {
                    file: "Editor.java".into(),
                    line,
                    col: Some(col),
                    message: p.message.clone(),
                    error: p.severity == Severity::Error,
                };
                if let Some(crate::dev::autofix::Fix::Edit(e)) =
                    crate::dev::autofix::fix_for_diag(&diag, text).and_then(|s| s.fix)
                    && e.files.len() == 1
                    && e.files[0].edits.len() == 1
                {
                    let (a, b, replacement) = &e.files[0].edits[0];
                    // Umbenennungen brauchen den echten Dateinamen und bleiben im Ausgabefenster.
                    if replacement != "Editor" {
                        p.fix = Some(Fix {
                            start: crate::dev::lsp::from_pos(text, *a),
                            end: crate::dev::lsp::from_pos(text, *b),
                            text: replacement.clone(),
                        });
                    }
                }
            }
        }
        v.sort_by_key(|p| p.start);
        v
    }

    /// Meldungen des Compilers zeigen: (Zeile, Spalte, beide 0-basiert, Text, Fehler?)
    pub fn set_diagnostics(&mut self, text: &str, list: &[(usize, Option<usize>, String, bool)]) {
        let chars: Vec<char> = text.chars().collect();
        self.diagnostics = list
            .iter()
            .map(|(line, col, message, error)| {
                let ls = line_start(text, *line);
                let le = chars[ls.min(chars.len())..].iter().position(|c| *c == '\n').map_or(chars.len(), |p| ls + p);
                let first = chars[ls.min(le)..le].iter().position(|c| !c.is_whitespace()).map_or(ls, |p| ls + p);
                let start = col.map(|c| (ls + c).min(le)).unwrap_or(first);
                // Bis zum Ende des Wortes, sonst bis zum Zeilenende
                let mut end = start;
                while end < le && is_word_char(chars[end]) {
                    end += 1;
                }
                if end == start {
                    end = if col.is_some() { start + 1 } else { le.max(start + 1) };
                }
                Problem { start, end, severity: if *error { Severity::Error } else { Severity::Warning }, message: message.clone(), fix: None }
            })
            .collect();
        self.lint.computed = false;
        self.lint.seen_at = None;
    }

    /// Meldungen des Sprachservers: (von, bis als Zeichenindex, Text, Fehler?)
    pub fn set_external(&mut self, list: Vec<(usize, usize, String, bool)>) {
        let new: Vec<Problem> = list
            .into_iter()
            .map(|(start, end, message, error)| Problem { start, end: end.max(start + 1), severity: if error { Severity::Error } else { Severity::Warning }, message, fix: None })
            .collect();
        if new != self.external {
            self.external = new;
            self.lint.computed = false;
        }
    }

    /// Einrueckung je Ebene: Einstellung, sonst je nach Sprache
    fn indent_step(&self) -> String {
        match prefs().indent {
            0 => self.lang.indent().to_string(),
            n => " ".repeat(n.min(16)),
        }
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
        let delay = lint_delay();
        let waited = self.lint.seen_at.map_or(delay, |t| t.elapsed());
        if self.lint.computed && self.lint.schema == schema && waited < delay {
            self.lint.shown = false;
            ctx.request_repaint_after(delay - waited);
            return;
        }
        let problems = self.problems(text);
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
            let problems = self.problems(text);
            let (fixed, n) = apply_fixes(text, &problems);
            if n == 0 {
                break;
            }
            *text = fixed;
            // Compiler- und LSP-Positionen beziehen sich auf den alten Text.
            self.diagnostics.clear();
            self.external.clear();
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

        if let Some(prefix) = self.lang.comment()
            && take(ui, Cmd::ToggleComment)
        {
            *text = toggle_comment_with(text, first, last, prefix);
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
        let step = self.indent_step();
        let step = step.as_str();
        // Ruecktaste in der Einrueckung: eine ganze Ebene loeschen statt eines Leerzeichens (wie in IntelliJ)
        if a == b && self.popup.is_none() {
            let ls = line_start(text, first);
            let spaces = text.chars().skip(ls).take(a.saturating_sub(ls)).collect::<String>();
            if !spaces.is_empty()
                && spaces.chars().all(|c| c == ' ')
                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace))
            {
                let n = (spaces.chars().count() - 1) % step.len().max(1) + 1;
                text.replace_range(byte_of(text, a - n)..byte_of(text, a), "");
                self.set_cursor(&ctx, a - n);
                changed(self, text, out);
                return;
            }
        }
        if self.popup.is_none() && last > first && take(ui, Cmd::Indent) {
            *text = map_lines(text, first, last, |l| format!("{step}{l}"));
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1));
            changed(self, text, out);
        }
        if take(ui, Cmd::Outdent) {
            *text = map_lines(text, first, last, |l| {
                let n = l.chars().take(step.len()).take_while(|c| *c == ' ').count();
                l.strip_prefix('\t').unwrap_or(&l[n..]).to_string()
            });
            self.set_selection(&ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1).max(line_start(text, first)));
            changed(self, text, out);
        }
    }

    /// Einruecken in Java & Co.: Enter uebernimmt die Einrueckung (nach `{` eine Ebene mehr, zwischen `{}` steht die
    /// schliessende Klammer danach auf eigener Zeile), Tab fuegt Leerzeichen ein, `}` auf leerer Zeile rueckt aus.
    /// Liefert true, wenn der Text geaendert wurde.
    fn auto_indent(&mut self, ctx: &egui::Context, text: &mut String, cursor: usize) -> bool {
        let chars: Vec<char> = text.chars().collect();
        if !typed_before_cursor(&self.prev_text, &chars, cursor) {
            return false;
        }
        let step = self.indent_step();
        let step = step.as_str();
        let typed_from = cursor - (chars.len() - self.prev_text.chars().count());
        // Enter, auch wenn im selben Bild schon weitergetippt wurde: Einrueckung hinter den letzten Zeilenumbruch
        if let Some(nl) = (typed_from..cursor).rev().find(|&i| chars[i] == '\n') {
            let at = nl + 1;
            let line_start = chars[..nl].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
            let indent: String = chars[line_start..nl].iter().take_while(|c| **c == ' ' || **c == '\t').collect();
            let before = chars[line_start..nl].iter().rev().find(|c| !c.is_whitespace()).copied();
            let after = chars.get(at).copied();
            let opens = self.lang.braces() && matches!(before, Some('{' | '[' | '('));
            let mut insert = indent.clone();
            if opens {
                insert.push_str(step);
            }
            let closing = at == cursor && opens && matches!((before, after), (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')')));
            let caret = cursor + insert.chars().count();
            if closing {
                insert.push('\n');
                insert.push_str(&indent);
            }
            if insert.is_empty() {
                return false;
            }
            text.insert_str(byte_of(text, at), &insert);
            self.set_cursor(ctx, caret);
            self.cursor = caret;
            self.cursor_moved = Some(());
            return true;
        }
        let typed = chars[cursor - 1];
        let line_start = chars[..cursor - 1].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        match typed {
            '\t' => {
                let col = cursor - 1 - line_start;
                let n = step.len() - col % step.len();
                let b = byte_of(text, cursor - 1);
                text.replace_range(b..b + 1, &" ".repeat(n));
                let caret = cursor - 1 + n;
                self.set_cursor(ctx, caret);
                self.cursor = caret;
                self.cursor_moved = Some(());
                true
            }
            '}' | ']' if self.lang.braces() => {
                let before: String = chars[line_start..cursor - 1].iter().collect();
                if before.is_empty() || !before.chars().all(|c| c == ' ') {
                    return false;
                }
                let remove = before.len().min(step.len());
                let b = byte_of(text, line_start);
                text.replace_range(b..b + remove, "");
                let caret = cursor - remove;
                self.set_cursor(ctx, caret);
                self.cursor = caret;
                self.cursor_moved = Some(());
                true
            }
            _ => false,
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
                let r = ui.add(crate::widgets::edit(&mut f.query).desired_width(200.0).id(find_id));
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
                    ui.add(crate::widgets::edit(&mut f.replace).desired_width(200.0));
                    if ui.button(crate::i18n::text("Ersetzen")).clicked() {
                        replace_one = true;
                    }
                    if ui.button(crate::i18n::text("Alle ersetzen")).clicked() {
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
    pub fn show(&mut self, ui: &mut egui::Ui, text: &mut String, height: f32) -> EditorOutput {
        let mut out = EditorOutput::default();
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(self.id));
        let pal = crate::style::pal();
        let font = FontId::monospace(font_size());
        let row_h = ctx.fonts_mut(|f| f.row_height(&font));
        if self.prev_text.is_empty() && !text.is_empty() {
            self.prev_text = text.clone();
        }
        if std::mem::take(&mut self.focus_next) {
            ctx.memory_mut(|m| m.request_focus(self.id));
        }
        if let Some(pos) = self.restore.take() {
            let pos = pos.min(text.chars().count());
            let (line, _) = line_col(text, pos);
            self.goto_line(&ctx, text, line, row_h);
            self.set_cursor(&ctx, pos);
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
                    p.items.get(p.selected).is_some_and(|i| text.get(a..b).is_some_and(|t| t.eq_ignore_ascii_case(&i.insert)))
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
                    out.add_import = self.accept(&ctx, text);
                    out.changed = true;
                }
            }
            if esc {
                self.popup = None;
            }
        }
        // Auswahlliste (Generieren, Umgeben mit) und Umbenennen bekommen die Tasten vor dem Editor
        if self.picker.is_some() {
            let pos = self.caret_screen;
            self.picker_ui(&ctx, text, pos, &mut out);
        }
        if self.rename.is_some() {
            let pos = self.caret_screen;
            self.rename_ui(&ctx, text, pos, &mut out);
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
        if focused && self.picker.is_none() && self.rename.is_none() {
            self.handle_keys(ui, text, &mut out);
            self.handle_refactor_keys(ui, text, &mut out);
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
                    let r = ui.add(crate::widgets::edit(line).desired_width(80.0));
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
        let prefs = prefs();
        let marks = self.breakpoints_enabled || !self.breakpoints.is_empty();
        // Rand: [Haltepunkte] [Zeilennummern] [Fehlerpunkte], jede Spalte mittig zur Zeile
        let digit_w = ctx.fonts_mut(|f| f.glyph_width(&font, '0'));
        let bp_w = if marks { 18.0 } else { 0.0 };
        let num_w = if prefs.line_numbers { digits as f32 * digit_w + 4.0 } else { 0.0 };
        let mark_w = 16.0;
        let gutter_w = 6.0 + bp_w + num_w + mark_w;
        // Wie in VS Code laesst sich ueber die letzte Zeile hinaus scrollen: unter dem Text bleibt Platz
        // fuer die Hoehe des Fensters abzueglich einer Zeile.
        let visible_rows = (((height - 8.0) / row_h).floor().max(3.0)) as usize;
        let total_rows = if prefs.scroll_past_end { lines + visible_rows - 1 } else { lines };
        self.view.0 = visible_rows;
        self.view.1 = row_h;

        let mut layout_cache = std::mem::take(&mut self.layout_cache);
        let lang = self.lang;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            layout_cache.layout_lang(ui, buf.as_str(), &font, lang)
        };

        let frame = egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(crate::style::RADIUS + 1).inner_margin(egui::Margin::same(3));
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
                        let (gutter, gutter_resp) = ui.allocate_exact_size(
                            egui::vec2(gutter_w, (total_rows as f32 * row_h + 8.0).max(height - 4.0)),
                            if self.breakpoints_enabled { egui::Sense::click() } else { egui::Sense::hover() },
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
                        // Zeile, an der der Debugger steht
                        if let Some(l) = self.exec_line {
                            let r = pos_of(line_start(text, l));
                            bg.push(egui::Shape::rect_filled(egui::Rect::from_x_y_ranges(text_rect.x_range(), r.y_range()), 0.0, pal.exec_bg));
                        }
                        if let (Some(c), true) = (cur, o.response.response.has_focus()) {
                            let r = pos_of(c);
                            if prefs.highlight_line {
                                bg.push(egui::Shape::rect_filled(
                                    egui::Rect::from_x_y_ranges(text_rect.x_range(), r.y_range()),
                                    0.0,
                                    pal.current_line,
                                ));
                            }
                            // Klammerpaar
                            let chars: Vec<char> = text.chars().collect();
                            let at = if !prefs.match_brackets {
                                None
                            } else if c > 0 && matching_bracket(&chars, c - 1).is_some() {
                                Some(c - 1)
                            } else if matching_bracket(&chars, c).is_some() {
                                Some(c)
                            } else {
                                None
                            };
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
                                let Some(row) = o.galley.rows.get(line) else { continue };
                                let (y0, rh) = (gp.y + row.min_y(), row.height().max(row_h));
                                if y0 + rh < clip.top() || y0 > clip.bottom() {
                                    continue;
                                }
                                let (x0, x1) = (pos_of(prob.start).left(), pos_of(end).left().max(pos_of(prob.start).left() + 6.0));
                                let color = if prob.severity == Severity::Error { pal.error_text } else { pal.warn_text };
                                ui.painter().add(squiggle(x0, x1, y0 + rh - 1.0, color));
                                lint_hits.push((egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y0 + rh)), k));
                            }
                        }
                        // Zeilennummern
                        let p = ui.painter();
                        p.rect_filled(gutter, 0.0, pal.bg);
                        let cur_line = cur.map(|c| line_col(text, c).0);
                        // Text never wraps: each galley row is one source line.
                        // Only emit line-number shapes inside the visible viewport.
                        let clip = ui.clip_rect().expand(row_h);
                        let first = o.galley.rows.partition_point(|row| gp.y + row.max_y() < clip.top());
                        let last = o.galley.rows.partition_point(|row| gp.y + row.min_y() <= clip.bottom());
                        // Haltepunkt setzen: Klick auf den Rand; die Zeile kommt aus der echten Lage der Zeilen
                        if gutter_resp.clicked()
                            && let Some(pos) = gutter_resp.interact_pointer_pos()
                        {
                            let line = o.galley.rows.partition_point(|row| gp.y + row.max_y() <= pos.y);
                            if line < lines {
                                out.toggle_breakpoint = Some(line);
                            }
                        }
                        if gutter_resp.hovered() && self.breakpoints_enabled {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let dot_x = gutter.left() + 4.0 + bp_w / 2.0;
                        let num_right = gutter.left() + 4.0 + bp_w + num_w;
                        let lint_x = num_right + mark_w / 2.0;
                        for (index, row) in o.galley.rows[first..last].iter().enumerate() {
                            let n = first + index + 1;
                            {
                                let active = cur_line == Some(n - 1);
                                let err = self.error_line == Some(n - 1);
                                // Mitte der Zeile (leere Zeilen haben bei egui keine Hoehe: dann die Zeilenhoehe)
                                let cy = gp.y + row.min_y() + row.height().max(row_h) / 2.0;
                                if self.breakpoints.contains(&(n - 1)) {
                                    p.circle_filled(egui::pos2(dot_x, cy), 5.0, pal.breakpoint_fg);
                                } else if self.breakpoints_enabled && gutter_resp.hover_pos().is_some_and(|h| h.x < num_right && (h.y - cy).abs() < row_h / 2.0) {
                                    // Vorschau beim Zeigen: hier entsteht der Haltepunkt
                                    p.circle_filled(egui::pos2(dot_x, cy), 5.0, pal.breakpoint_fg.gamma_multiply(0.35));
                                }
                                if self.exec_line == Some(n - 1) {
                                    let x = dot_x - 4.0;
                                    p.add(egui::Shape::convex_polygon(vec![egui::pos2(x, cy - 5.0), egui::pos2(x + 8.0, cy), egui::pos2(x, cy + 5.0)], pal.exec_fg, egui::Stroke::NONE));
                                }
                                if self.lint.shown
                                    && let Ok(k) = self.lint.lines.binary_search_by_key(&(n - 1), |(l, _)| *l)
                                {
                                    let color = if self.lint.lines[k].1 == Severity::Error { pal.error_text } else { pal.warn_text };
                                    p.circle_filled(egui::pos2(lint_x, cy), 3.5, color);
                                }
                                if prefs.line_numbers {
                                    p.text(
                                        egui::pos2(num_right, cy),
                                        egui::Align2::RIGHT_CENTER,
                                        n.to_string(),
                                        font.clone(),
                                        if err { pal.error_text } else if active { pal.text } else { pal.syn_gutter },
                                    );
                                }
                            }
                        }
                        // Zeichen unter dem Mauszeiger (Strg+Klick: Gehe zu Definition, Hinweise des Sprachservers)
                        if let Some(pos) = ui.ctx().pointer_hover_pos()
                            && text_rect.contains(pos)
                        {
                            let c = o.galley.cursor_from_pos(pos - gp).index.0;
                            out.hover = Some(c);
                            if o.response.response.clicked() && ui.input(|i| i.modifiers.command) {
                                out.ctrl_click = Some(c);
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

        // Rechtsklick: Cursor an die Stelle (ausser in die Auswahl), dann das Menue
        if o.response.response.secondary_clicked()
            && let Some(p) = o.response.response.interact_pointer_pos()
        {
            let c = o.galley.cursor_from_pos(p - o.galley_pos).index.0;
            let inside = o.cursor_range.is_some_and(|r| {
                let (x, y) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
                x != y && x <= c && c <= y
            });
            if !inside {
                self.set_cursor(&ctx, c);
                self.cursor = c;
            }
        }
        self.context_menu(&o.response.response, text, &mut out);
        if let Some(c) = o.cursor_range.map(|r| r.primary.index.0) {
            self.caret_screen = o.galley_pos + o.galley.pos_from_cursor(CCursor::new(c)).left_bottom().to_vec2() + egui::vec2(0.0, 4.0);
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
        } else if self.lang == Lang::Sql
            && let Some(i) = out.hover
            && o.response.response.hovered()
            && let Some((a, _, _)) = hover::word_at(text, i)
        {
            // SQL: Tabelle, Spalte oder Funktion erklaeren, wenn die Maus kurz ruht
            match self.hover_at {
                Some((at, since)) if at == a => {
                    if since.elapsed() >= Duration::from_millis(350) {
                        if let Some((title, body)) = hover::sql_info(text, i, self.schema.as_deref()) {
                            hover::card(&ctx, ui.layer_id(), self.id.with("sql-hover"), &title, &body, Lang::Sql);
                        }
                    } else {
                        ctx.request_repaint_after(Duration::from_millis(360) - since.elapsed());
                    }
                }
                _ => {
                    self.hover_at = Some((a, Instant::now()));
                    ctx.request_repaint_after(Duration::from_millis(360));
                }
            }
        } else {
            self.hover_at = None;
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
            let indented = self.lang != Lang::Sql && prefs.auto_indent && cursor.is_some_and(|c| self.auto_indent(&ctx, text, c));
            if !indented
                && let Some(c) = cursor
                && AUTO_CLOSE.load(std::sync::atomic::Ordering::Relaxed) {
                    self.auto_close(&ctx, text, c);
                }
            self.error_line = None;
        }
        if out.changed {
            self.prev_text = text.clone();
            // Meldungen des Compilers gelten nur fuer den uebersetzten Stand
            if !self.diagnostics.is_empty() {
                self.diagnostics.clear();
                self.lint.computed = false;
            }
        }

        // Vorschlaege aktualisieren
        let auto = AUTO_SUGGEST.load(std::sync::atomic::Ordering::Relaxed);
        if (o.response.response.changed() && auto) || force {
            // Nach dem Einruecken steht der Cursor woanders als im Ergebnis des Editors
            let c = cursor.map(|c| if self.cursor_moved.take().is_some() { self.cursor } else { c });
            // Java-Code: auch der Sprachserver soll vorschlagen (nach einem Punkt, in einem Wort oder auf Wunsch)
            if self.lang == Lang::Java
                && let Some(c) = c
            {
                let chars: Vec<char> = text.chars().collect();
                let prev = c.checked_sub(1).and_then(|i| chars.get(i)).copied();
                let in_code = crate::dev::java::source::spans(&chars).iter().any(|(k, a, b)| *k == crate::dev::java::source::Span::Code && *a < c && c <= *b);
                if in_code && (force || prev.is_some_and(|p| p == '.' || is_word_char(p))) {
                    out.complete_at = Some(c);
                    self.cursor = c;
                }
            }
            self.popup = c.and_then(|c| self.suggest(text, c, force).map(|s| Popup { items: s.items, selected: 0, word_start: s.word_start, cursor: c, imports: Vec::new() }));
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

        // Vorschlagsfenster zeichnen: kompakte Zeilen, bis zu 16 ohne Scrollen; reicht der Platz unten nicht, darueber
        let mut clicked = None;
        if let (Some(p), Some(c)) = (&self.popup, cursor) {
            let caret = o.galley.pos_from_cursor(CCursor::new(c));
            let row = (font_size() + 8.0).round().max(20.0);
            let rows = p.items.len().min(16) as f32;
            let list_h = rows * row;
            let below = o.galley_pos + caret.left_bottom().to_vec2() + egui::vec2(-4.0, 3.0);
            let screen = ctx.content_rect();
            let pos = if below.y + list_h + 12.0 > screen.bottom() && below.y - caret.height() - list_h - 12.0 > screen.top() {
                below - egui::vec2(0.0, caret.height() + list_h + 14.0)
            } else {
                below
            };
            let mono = FontId::monospace(font_size());
            let small = FontId::proportional((font_size() - 1.0).max(11.0));
            let area = egui::Area::new(self.id.with("popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(&ctx, |ui| {
                    egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
                        let chars = p.items.iter().map(|i| i.label.chars().count()).max().unwrap_or(10) as f32;
                        let detail = p.items.iter().map(|i| i.detail.chars().count().min(40) + i.kind.chars().count() + 4).max().unwrap_or(10) as f32;
                        let width = (chars * font_size() * 0.62 + detail * font_size() * 0.5 + 44.0).clamp(300.0, 720.0);
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
                            for (i, it) in p.items.iter().enumerate() {
                                let sel = i == p.selected;
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(width - 6.0, row), egui::Sense::click());
                                if sel {
                                    ui.painter().rect_filled(r, 4.0, pal.selection);
                                } else if resp.hovered() {
                                    ui.painter().rect_filled(r, 4.0, pal.hover);
                                }
                                // Farbiges Kennzeichen je Art (Methode, Feld, Klasse …)
                                let (mark, color) = kind_mark(it.kind, &pal);
                                let mr = egui::Rect::from_center_size(egui::pos2(r.left() + 13.0, r.center().y), egui::vec2(16.0, 16.0));
                                ui.painter().rect_filled(mr, 4.0, color.gamma_multiply(0.22));
                                ui.painter().text(mr.center(), egui::Align2::CENTER_CENTER, mark, FontId::proportional(11.0), color);
                                let fg = if sel && pal.dark { Color32::WHITE } else { pal.text };
                                let label_g = ui.painter().layout_no_wrap(it.label.clone(), mono.clone(), fg);
                                let right = if it.detail.is_empty() { it.kind.to_string() } else { it.detail.chars().take(40).collect::<String>() };
                                let right_g = ui.painter().layout_no_wrap(right, small.clone(), if sel && pal.dark { Color32::from_white_alpha(190) } else { pal.text_weak });
                                let rx = r.right() - 8.0 - right_g.size().x;
                                ui.painter().with_clip_rect(egui::Rect::from_min_max(r.min, egui::pos2(rx - 10.0, r.bottom()))).galley(egui::pos2(r.left() + 28.0, r.center().y - label_g.size().y / 2.0), label_g, fg);
                                ui.painter().galley(egui::pos2(rx, r.center().y - right_g.size().y / 2.0), right_g, pal.text_weak);
                                if sel {
                                    resp.scroll_to_me(None);
                                }
                                if resp.clicked() {
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
            out.add_import = self.accept(&ctx, text);
            out.changed = true;
            ctx.memory_mut(|m| m.request_focus(self.id));
        }
        out
    }
}

/// Wurde seit `old` nur etwas eingefuegt, das genau vor dem Cursor endet? (auch mehrere Zeichen in einem Bild,
/// etwa ";" und Enter bei schnellem Tippen)
fn typed_before_cursor(old: &str, new: &[char], cursor: usize) -> bool {
    let old: Vec<char> = old.chars().collect();
    if cursor == 0 || cursor > new.len() || new.len() <= old.len() {
        return false;
    }
    // Neu = Alt mit k Zeichen, die genau vor dem Cursor eingefuegt wurden
    let k = new.len() - old.len();
    cursor >= k && new[..cursor - k] == old[..cursor - k] && new[cursor..] == old[cursor - k..]
}

/// Buchstabe und Farbe fuer die Art eines Vorschlags (wie in IntelliJ: m, f, c, v …)
fn kind_mark(kind: &str, pal: &crate::style::Pal) -> (&'static str, Color32) {
    use crate::i18n::{text, tr};
    if kind == text("Tabelle") || kind == text("Alias") {
        return ("T", pal.syn_type);
    }
    if kind == text("Spalte") {
        return ("c", pal.syn_variable);
    }
    if kind == text("Funktion") {
        return ("f", pal.syn_function);
    }
    if kind == text("Join") {
        return ("j", pal.accent);
    }
    if kind == text("Datenbank") {
        return ("D", pal.syn_number);
    }
    if kind == text("Vorlage") {
        return ("s", pal.accent);
    }
    if kind == tr("Methode", "Method") || kind == tr("Konstruktor", "Constructor") || kind == tr("Funktion", "Function") {
        ("m", pal.syn_function)
    } else if kind == tr("Feld", "Field") || kind == tr("Spalte", "Column") {
        ("f", pal.syn_variable)
    } else if kind == tr("Variable", "Variable") {
        ("v", pal.syn_number)
    } else if kind == tr("Klasse", "Class") || kind == "Interface" || kind == "Enum" || kind == tr("Typ", "Type") || kind == tr("Tabelle", "Table") {
        ("C", pal.syn_type)
    } else if kind == tr("Konstante", "Constant") {
        ("K", pal.syn_const)
    } else if kind == tr("Vorlage", "Snippet") {
        ("s", pal.accent)
    } else if kind == tr("Datenbank", "Database") {
        ("D", pal.syn_type)
    } else {
        ("k", pal.syn_keyword)
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
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn comment_toggle() {
        let t = "SELECT 1;\n  SELECT 2;\nSELECT 3;";
        let c = toggle_comment_with(t, 0, 1, "--");
        assert_eq!(c, "-- SELECT 1;\n  -- SELECT 2;\nSELECT 3;");
        assert_eq!(toggle_comment_with(&c, 0, 1, "--"), t);
        assert_eq!(toggle_comment_with("int a;\n", 0, 0, "//"), "// int a;\n");
    }

    #[test]
    fn typing_is_recognised_before_the_cursor() {
        let new: Vec<char> = "a;\nb".chars().collect();
        assert!(typed_before_cursor("ab", &new, 3), "zwei Zeichen in einem Bild");
        assert!(!typed_before_cursor("ab", &new, 4), "Cursor nicht hinter der Eingabe");
        let new: Vec<char> = "xx\n".chars().collect();
        assert!(typed_before_cursor("xx", &new, 3));
        // Enter vor einem vorhandenen Zeilenumbruch
        let new: Vec<char> = "}\n\nab".chars().collect();
        assert!(typed_before_cursor("}\nab", &new, 2));
        assert!(!typed_before_cursor("xy", &new, 3), "Ersetzt statt eingefuegt");
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
        let mut render = || {
            let _ = ctx.run_ui(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 760.0))),
                ..Default::default()
            }, |ui| { editor.show(ui, &mut text, 700.0); });
        };
        for _ in 0..3 { render(); }
        let start = std::time::Instant::now();
        for _ in 0..10 { render(); }
        println!("5,000 SQL lines: {:.3} ms/frame", start.elapsed().as_secs_f64() * 1000.0 / 10.0);
    }
}

#[cfg(test)]
mod indent_tests {
    use super::*;

    /// Tippen im echten Editor: Enter nach `{` rueckt ein, zwischen `{}` steht die Klammer danach auf eigener Zeile.
    #[test]
    fn enter_keeps_and_adds_indentation_in_java() {
        let ctx = egui::Context::default();
        let mut ed = SqlEditor::new(egui::Id::new("indent-test"));
        ed.lang = Lang::Java;
        let mut text = String::from("class A {\n    void m() {}\n}");
        let frame = |ed: &mut SqlEditor, text: &mut String, events: Vec<egui::Event>| {
            let _ = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() }, |ui| {
                ed.show(ui, text, 500.0);
            });
        };
        frame(&mut ed, &mut text, vec![]);
        // Cursor zwischen {}
        let pos = text.find("{}").unwrap() + 1;
        ctx.memory_mut(|m| m.request_focus(ed.id));
        ed.set_cursor(&ctx, text[..pos].chars().count());
        frame(&mut ed, &mut text, vec![]);
        let enter = egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut ed, &mut text, vec![enter.clone()]);
        assert_eq!(text, "class A {\n    void m() {\n        \n    }\n}");
        frame(&mut ed, &mut text, vec![egui::Event::Text("x;".into()), enter.clone()]);
        assert_eq!(text, "class A {\n    void m() {\n        x;\n        \n    }\n}");
        // Enter und der naechste Buchstabe im selben Bild
        frame(&mut ed, &mut text, vec![egui::Event::Text("y;".into()), enter, egui::Event::Text("z".into())]);
        assert_eq!(text, "class A {\n    void m() {\n        x;\n        y;\n        z\n    }\n}");
    }

    #[test]
    fn java_sprachserver_fix_vor_dem_kompilieren() {
        let mut ed = SqlEditor::new(egui::Id::new("java-fix"));
        ed.lang = Lang::Java;
        let mut text = "class Main {\n void f() {\n  int x = 1\n }\n}\n".to_string();
        let at = text.find("int x").unwrap();
        ed.set_external(vec![(at, at + 3, "Syntax error, insert \";\" to complete LocalVariableDeclarationStatement".into(), true)]);
        assert_eq!(ed.fix_all(&mut text), 1);
        assert!(text.contains("int x = 1;\n"));
        assert_eq!(ed.fix_all(&mut text), 0);
    }
}
