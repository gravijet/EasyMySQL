// Aktionen des Editors: Generieren, Umgeben mit, Variable einfuehren, Gross/klein, Zeilen verbinden und
// sortieren, Auswahl erweitern, neue Zeile darunter/darueber, Blockkommentar, Rechtsklickmenue und Umbenennen
// in der Datei (wenn kein Sprachserver hilft).

use super::refactor::{self, Generate, Surround};
use super::*;
use crate::icons::{self, Icon};
use crate::keymap::{Cmd, take};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PickKind {
    Generate,
    Surround,
}

/// Kleine Auswahlliste am Cursor (Alt+Einfg, Strg+Alt+T)
pub(super) struct Picker {
    pub kind: PickKind,
    pub selected: usize,
}

/// Umbenennen in der Datei: Feld direkt am Wort
pub(super) struct LocalRename {
    pub old: String,
    pub new: String,
    pub first: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum EditAction {
    Generate(Generate),
    Surround(Surround),
    IntroduceVariable,
    ToggleCase,
    JoinLines,
    SortLines,
    Expand,
    Shrink,
    NewLineBelow,
    NewLineAbove,
    BlockComment,
    Comment,
    Duplicate,
    DeleteLine,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl SqlEditor {
    /// Zeilen der Auswahl (erste, letzte; eine Auswahl, die am Zeilenanfang endet, zaehlt die Zeile nicht mit)
    fn selected_lines(text: &str, a: usize, b: usize) -> (usize, usize) {
        let (first, _) = line_col(text, a);
        let (mut last, col) = line_col(text, b);
        if b > a && col == 0 && last > first {
            last -= 1;
        }
        (first, last)
    }

    /// Tastenkuerzel der Umbauten (vor dem Zeichnen, nur mit Fokus)
    pub(super) fn handle_refactor_keys(&mut self, ui: &mut egui::Ui, text: &mut String, out: &mut EditorOutput) {
        let java = self.lang == Lang::Java;
        let pairs: [(Cmd, Option<EditAction>, bool); 11] = [
            (Cmd::GenerateCode, None, java),
            (Cmd::SurroundWith, None, java),
            (Cmd::IntroduceVariable, Some(EditAction::IntroduceVariable), java),
            (Cmd::ToggleCase, Some(EditAction::ToggleCase), true),
            (Cmd::JoinLines, Some(EditAction::JoinLines), true),
            (Cmd::SortLines, Some(EditAction::SortLines), true),
            (Cmd::ExpandSelection, Some(EditAction::Expand), true),
            (Cmd::ShrinkSelection, Some(EditAction::Shrink), true),
            (Cmd::NewLineBelow, Some(EditAction::NewLineBelow), self.popup.is_none()),
            (Cmd::NewLineAbove, Some(EditAction::NewLineAbove), true),
            (Cmd::BlockComment, Some(EditAction::BlockComment), matches!(self.lang, Lang::Java | Lang::Sql | Lang::Gradle)),
        ];
        for (cmd, action, allowed) in pairs {
            if !allowed || !take(ui, cmd) {
                continue;
            }
            match (cmd, action) {
                (Cmd::GenerateCode, _) => self.picker = Some(Picker { kind: PickKind::Generate, selected: 0 }),
                (Cmd::SurroundWith, _) => self.picker = Some(Picker { kind: PickKind::Surround, selected: 0 }),
                (_, Some(a)) => self.run_action(ui.ctx(), text, a, out),
                _ => {}
            }
        }
    }

    /// Eine Aktion auf Auswahl bzw. Cursorzeile anwenden
    pub(super) fn run_action(&mut self, ctx: &egui::Context, text: &mut String, action: EditAction, out: &mut EditorOutput) {
        let (a, b) = self.selection(ctx).unwrap_or((self.cursor, self.cursor));
        let (first, last) = Self::selected_lines(text, a, b);
        let step = self.indent_step();
        let mut changed = true;
        match action {
            EditAction::Generate(g) => match refactor::generate(text, a, g, &step) {
                Some((t, c)) => {
                    *text = t;
                    self.set_cursor(ctx, c);
                }
                None => {
                    changed = false;
                    out.notice = Some(crate::i18n::tr("Nichts zu erzeugen: alles ist schon da oder die Klasse hat keine Felder.", "Nothing to generate: everything exists already or the class has no fields.").into());
                }
            },
            EditAction::Surround(s) => {
                let (t, c) = refactor::surround(text, first, last, s, &step);
                *text = t;
                self.set_cursor(ctx, c);
            }
            EditAction::IntroduceVariable => {
                let range = if a == b { hover::word_at(text, a).map(|(x, y, _)| (x, y)) } else { Some((a, b)) };
                match range.and_then(|(x, y)| refactor::introduce_variable(text, x, y, "wert")) {
                    Some((t, s, e)) => {
                        *text = t;
                        self.set_selection(ctx, s, e);
                    }
                    None => changed = false,
                }
            }
            EditAction::ToggleCase => {
                let (x, y) = if a == b { hover::word_at(text, a).map(|(x, y, _)| (x, y)).unwrap_or((a, b)) } else { (a, b) };
                let sel: String = text.chars().skip(x).take(y - x).collect();
                text.replace_range(byte_of(text, x)..byte_of(text, y), &refactor::toggle_case(&sel));
                self.set_selection(ctx, x, y);
            }
            EditAction::JoinLines => {
                let (t, c) = refactor::join_lines(text, first, last);
                changed = t != *text;
                *text = t;
                self.set_cursor(ctx, c);
            }
            EditAction::SortLines => {
                *text = refactor::sort_lines(text, first, last);
                self.set_selection(ctx, line_start(text, first), line_start(text, last + 1).saturating_sub(1).max(line_start(text, first)));
            }
            EditAction::Expand => {
                changed = false;
                if let Some((x, y)) = expand(text, a, b) {
                    self.sel_stack.push((a, b));
                    self.set_selection(ctx, x, y);
                }
            }
            EditAction::Shrink => {
                changed = false;
                if let Some((x, y)) = self.sel_stack.pop() {
                    self.set_selection(ctx, x, y);
                }
            }
            EditAction::NewLineBelow | EditAction::NewLineAbove => {
                let line = if action == EditAction::NewLineBelow { last } else { first };
                let ls = line_start(text, line);
                let content: String = text.split('\n').nth(line).unwrap_or("").to_string();
                let mut ind: String = content.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                if action == EditAction::NewLineBelow {
                    if self.lang.braces() && content.trim_end().ends_with('{') {
                        ind.push_str(&step);
                    }
                    let le = ls + content.chars().count();
                    text.insert_str(byte_of(text, le), &format!("\n{ind}"));
                    self.set_cursor(ctx, le + 1 + ind.chars().count());
                } else {
                    text.insert_str(byte_of(text, ls), &format!("{ind}\n"));
                    self.set_cursor(ctx, ls + ind.chars().count());
                }
            }
            EditAction::BlockComment => {
                let (x, y) = if a == b {
                    let ls = line_start(text, first);
                    let line: String = text.split('\n').nth(first).unwrap_or("").to_string();
                    let lead = line.chars().take_while(|c| c.is_whitespace()).count();
                    (ls + lead, ls + line.trim_end().chars().count())
                } else {
                    (a, b)
                };
                let sel: String = text.chars().skip(x).take(y - x).collect();
                let new = match sel.strip_prefix("/*").and_then(|s| s.strip_suffix("*/")) {
                    Some(inner) => inner.strip_prefix(' ').unwrap_or(inner).strip_suffix(' ').unwrap_or(inner.strip_prefix(' ').unwrap_or(inner)).to_string(),
                    None => format!("/* {sel} */"),
                };
                text.replace_range(byte_of(text, x)..byte_of(text, y), &new);
                self.set_selection(ctx, x, x + new.chars().count());
            }
            EditAction::Comment => match self.lang.comment() {
                Some(prefix) => *text = toggle_comment_with(text, first, last, prefix),
                None => changed = false,
            },
            EditAction::Duplicate => *text = duplicate_lines(text, first, last),
            EditAction::DeleteLine => {
                let (t, _, cur) = remove_lines(text, first, last);
                *text = t;
                self.set_cursor(ctx, cur);
            }
            EditAction::Cut | EditAction::Copy => {
                let (x, y) = if a == b { (line_start(text, first), line_start(text, last + 1)) } else { (a, b) };
                ctx.copy_text(text.chars().skip(x).take(y - x).collect());
                if action == EditAction::Cut {
                    text.replace_range(byte_of(text, x)..byte_of(text, y), "");
                    self.set_cursor(ctx, x);
                } else {
                    changed = false;
                }
            }
            EditAction::Paste => {
                changed = false;
                ctx.memory_mut(|m| m.request_focus(self.id));
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            EditAction::SelectAll => {
                changed = false;
                self.set_selection(ctx, 0, text.chars().count());
            }
        }
        if changed {
            out.changed = true;
            self.prev_text = text.clone();
            self.popup = None;
        }
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    /// Auswahlliste fuer Generieren / Umgeben mit an der Stelle `pos`
    pub(super) fn picker_ui(&mut self, ctx: &egui::Context, text: &mut String, pos: egui::Pos2, out: &mut EditorOutput) {
        let Some(p) = self.picker.as_ref() else { return };
        let labels: Vec<&str> = match p.kind {
            PickKind::Generate => Generate::ALL.iter().map(|g| g.label()).collect(),
            PickKind::Surround => Surround::ALL.iter().map(|s| s.label()).collect(),
        };
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape))
        });
        let esc = esc || ESCAPE.swap(false, std::sync::atomic::Ordering::Relaxed);
        let mut chosen = None;
        let mut selected = p.selected;
        if down {
            selected = (selected + 1) % labels.len();
        }
        if up {
            selected = (selected + labels.len() - 1) % labels.len();
        }
        if enter {
            chosen = Some(selected);
        }
        let pal = crate::style::pal();
        let title = match p.kind {
            PickKind::Generate => crate::i18n::tr("Generieren", "Generate"),
            PickKind::Surround => crate::i18n::tr("Umgeben mit", "Surround with"),
        };
        let area = egui::Area::new(self.id.with("picker")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(260.0);
                ui.label(RichText::new(title).strong().color(pal.text_weak));
                ui.add_space(2.0);
                for (i, l) in labels.iter().enumerate() {
                    let r = ui.add(egui::Button::selectable(i == selected, *l).min_size(egui::vec2(252.0, 26.0)));
                    if r.clicked() {
                        chosen = Some(i);
                    }
                    if r.hovered() {
                        selected = i;
                    }
                }
            });
        });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        if let Some(p) = self.picker.as_mut() {
            p.selected = selected;
        }
        if let Some(i) = chosen {
            let kind = self.picker.take().map(|p| p.kind);
            let action = match kind {
                Some(PickKind::Generate) => EditAction::Generate(Generate::ALL[i]),
                _ => EditAction::Surround(Surround::ALL[i]),
            };
            self.run_action(ctx, text, action, out);
        } else if esc || clicked_outside {
            self.picker = None;
            ctx.memory_mut(|m| m.request_focus(self.id));
        }
    }

    /// Rechtsklickmenue des Editors
    pub(super) fn context_menu(&mut self, resp: &egui::Response, text: &mut String, out: &mut EditorOutput) {
        let ctx = resp.ctx.clone();
        let mut action: Option<EditAction> = None;
        let mut command: Option<Cmd> = None;
        let java = self.lang == Lang::Java;
        let sql = self.lang == Lang::Sql;
        let has_sel = self.selection(&ctx).is_some_and(|(a, b)| a != b);
        resp.context_menu(|ui| {
            crate::style::menu_item_look(ui);
            ui.set_min_width(280.0);
            let pal = crate::style::pal();
            let item = |ui: &mut egui::Ui, icon: Icon, color: Color32, label: &str, cmd: Option<Cmd>| -> bool {
                let keys = cmd.map(crate::keymap::text).unwrap_or_default();
                icons::menu_item_colored(ui, Some((icon, Some(color))), label, &keys).clicked()
            };
            if sql {
                if item(ui, Icon::Play, pal.ok_text, crate::i18n::tr("Anweisung ausführen", "Run statement"), Some(Cmd::RunStatement)) {
                    command = Some(Cmd::RunStatement);
                }
                if item(ui, Icon::Play, pal.ok_text, if has_sel { crate::i18n::tr("Auswahl ausführen", "Run selection") } else { crate::i18n::tr("Alles ausführen", "Run all") }, Some(Cmd::RunAll)) {
                    command = Some(Cmd::RunAll);
                }
                if item(ui, Icon::Explain, pal.syn_type, Cmd::Explain.label(), Some(Cmd::Explain)) {
                    command = Some(Cmd::Explain);
                }
                ui.separator();
            }
            if java {
                icons::submenu(ui, Some(Icon::Wizard), crate::i18n::tr("Generieren", "Generate"), |ui| {
                    crate::style::menu_item_look(ui);
                    for g in Generate::ALL {
                        if icons::menu_item(ui, None, g.label(), "").clicked() {
                            action = Some(EditAction::Generate(g));
                            ui.close();
                        }
                    }
                });
                icons::submenu(ui, Some(Icon::Braces), crate::i18n::tr("Umgeben mit", "Surround with"), |ui| {
                    crate::style::menu_item_look(ui);
                    for s in Surround::ALL {
                        if icons::menu_item(ui, None, s.label(), "").clicked() {
                            action = Some(EditAction::Surround(s));
                            ui.close();
                        }
                    }
                });
                if item(ui, Icon::ArrowRight, pal.syn_function, Cmd::GotoDefinition.label(), Some(Cmd::GotoDefinition)) {
                    command = Some(Cmd::GotoDefinition);
                }
            }
            if (java || sql) && item(ui, Icon::Edit, pal.syn_variable, crate::i18n::tr("Umbenennen …", "Rename …"), Some(Cmd::RenameSymbol)) {
                command = Some(Cmd::RenameSymbol);
            }
            if java && item(ui, Icon::Plus, pal.syn_number, Cmd::IntroduceVariable.label(), Some(Cmd::IntroduceVariable)) {
                action = Some(EditAction::IntroduceVariable);
            }
            if java || sql {
                if item(ui, Icon::Sparkles, pal.accent, crate::i18n::tr("Formatieren", "Format"), Some(Cmd::Format)) {
                    command = Some(Cmd::Format);
                }
                if item(ui, Icon::Wizard, pal.warn_text, Cmd::FixAll.label(), Some(Cmd::FixAll)) {
                    command = Some(Cmd::FixAll);
                }
            }
            if sql && item(ui, Icon::Coffee, pal.syn_string, Cmd::CopyJava.label(), Some(Cmd::CopyJava)) {
                command = Some(Cmd::CopyJava);
            }
            ui.separator();
            if item(ui, Icon::Scissors, pal.text, crate::i18n::text("Ausschneiden"), Some(Cmd::CutLine)) {
                action = Some(EditAction::Cut);
            }
            if item(ui, Icon::Copy, pal.text, crate::i18n::text("Kopieren"), None) {
                action = Some(EditAction::Copy);
            }
            if item(ui, Icon::Clipboard, pal.text, crate::i18n::text("Einfügen"), None) {
                action = Some(EditAction::Paste);
            }
            if item(ui, Icon::Check, pal.text, crate::i18n::tr("Alles auswählen", "Select all"), None) {
                action = Some(EditAction::SelectAll);
            }
            ui.separator();
            icons::submenu(ui, Some(Icon::Layers), crate::i18n::tr("Zeilen", "Lines"), |ui| {
                crate::style::menu_item_look(ui);
                for (cmd, a) in [
                    (Cmd::ToggleComment, EditAction::Comment),
                    (Cmd::BlockComment, EditAction::BlockComment),
                    (Cmd::DuplicateLine, EditAction::Duplicate),
                    (Cmd::DeleteLine, EditAction::DeleteLine),
                    (Cmd::JoinLines, EditAction::JoinLines),
                    (Cmd::SortLines, EditAction::SortLines),
                    (Cmd::ToggleCase, EditAction::ToggleCase),
                    (Cmd::NewLineBelow, EditAction::NewLineBelow),
                    (Cmd::NewLineAbove, EditAction::NewLineAbove),
                ] {
                    if icons::menu_item(ui, None, cmd.label(), &crate::keymap::text(cmd)).clicked() {
                        action = Some(a);
                        ui.close();
                    }
                }
            });
            icons::submenu(ui, Some(Icon::Fit), crate::i18n::tr("Auswahl", "Selection"), |ui| {
                crate::style::menu_item_look(ui);
                if icons::menu_item(ui, None, Cmd::ExpandSelection.label(), &crate::keymap::text(Cmd::ExpandSelection)).clicked() {
                    action = Some(EditAction::Expand);
                    ui.close();
                }
                if icons::menu_item(ui, None, Cmd::ShrinkSelection.label(), &crate::keymap::text(Cmd::ShrinkSelection)).clicked() {
                    action = Some(EditAction::Shrink);
                    ui.close();
                }
            });
            ui.separator();
            if item(ui, Icon::Search, pal.text_weak, crate::i18n::text("Suchen …"), Some(Cmd::Find)) {
                command = Some(Cmd::Find);
            }
            if item(ui, Icon::Refresh, pal.text_weak, crate::i18n::tr("Ersetzen …", "Replace …"), Some(Cmd::Replace)) {
                command = Some(Cmd::Replace);
            }
            if action.is_some() || command.is_some() {
                ui.close();
            }
        });
        if let Some(a) = action {
            self.run_action(&ctx, text, a, out);
        }
        match command {
            Some(c @ (Cmd::Find | Cmd::Replace)) => self.pending = Some(c),
            Some(c) => out.command = Some(c),
            None => {}
        }
    }

    /// Umbenennen in dieser Datei beginnen (Wort am Cursor)
    pub fn start_local_rename(&mut self, text: &str) {
        if let Some((_, _, w)) = hover::word_at(text, self.cursor) {
            self.rename = Some(LocalRename { new: w.clone(), old: w, first: true });
        }
    }

    /// Feld zum Umbenennen an der Stelle `pos`
    pub(super) fn rename_ui(&mut self, ctx: &egui::Context, text: &mut String, pos: egui::Pos2, out: &mut EditorOutput) {
        let Some(r) = self.rename.as_mut() else { return };
        let mut done = None;
        let id = self.id.with("rename-field");
        let count = refactor::occurrences(text, &r.old).len();
        egui::Area::new(self.id.with("rename")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.label(RichText::new(crate::tr_format!("„{}“ umbenennen ({} Stellen in dieser Datei)", "Rename “{}” ({} places in this file)", r.old, count)).color(crate::style::pal().text_weak));
                let resp = ui.add(crate::widgets::edit(&mut r.new).id(id).desired_width(260.0).font(FontId::monospace(font_size())));
                if r.first {
                    resp.request_focus();
                    let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
                    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(r.new.chars().count()))));
                    st.store(ctx, id);
                    r.first = false;
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) || ESCAPE.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let r = self.rename.take().unwrap();
                let valid = r.new.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && r.new.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');
                if valid && r.new != r.old {
                    *text = refactor::rename_in_file(text, &r.old, &r.new);
                    self.prev_text = text.clone();
                    out.changed = true;
                }
                ctx.memory_mut(|m| m.request_focus(self.id));
            }
            Some(false) => {
                self.rename = None;
                ctx.memory_mut(|m| m.request_focus(self.id));
            }
            None => {}
        }
    }
}

/// Naechstgroessere sinnvolle Auswahl um (a, b): Wort, Zeichenkette, Klammerinhalt, Klammer, Zeile, Block, alles
pub(super) fn expand(text: &str, a: usize, b: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut cands: Vec<(usize, usize)> = Vec::new();
    if let Some((x, y, _)) = hover::word_at(text, a) {
        cands.push((x, y));
    }
    // Zeichenketten
    for (k, x, y) in crate::dev::java::source::spans(&chars) {
        if matches!(k, crate::dev::java::source::Span::Str | crate::dev::java::source::Span::Char) && x <= a && b <= y {
            cands.push((x + 1, y.saturating_sub(1).max(x + 1)));
            cands.push((x, y));
        }
    }
    // Klammern rund um die Auswahl, von innen nach aussen
    let mut i = a;
    let mut depth = 0i32;
    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                if depth == 0 {
                    if let Some(j) = matching_bracket(&chars, i)
                        && j >= b
                    {
                        cands.push((i + 1, j));
                        // Inhalt ohne Leerraum am Rand (bei Bloecken)
                        let inner_a = (i + 1..j).find(|&k| !chars[k].is_whitespace()).unwrap_or(i + 1);
                        let inner_b = (i + 1..j).rev().find(|&k| !chars[k].is_whitespace()).map_or(j, |k| k + 1);
                        cands.push((inner_a, inner_b));
                        cands.push((i, j + 1));
                    }
                } else {
                    depth -= 1;
                }
            }
            _ => {}
        }
    }
    // Zeile (ohne und mit Einrueckung)
    let ls = chars[..a].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let le = chars[b..].iter().position(|c| *c == '\n').map_or(n, |p| b + p);
    let lead = (ls..le).find(|&k| !chars[k].is_whitespace()).unwrap_or(ls);
    cands.push((lead, le));
    cands.push((ls, (le + 1).min(n)));
    cands.push((0, n));
    cands.into_iter().filter(|&(x, y)| x <= a && b <= y && y - x > b - a).min_by_key(|&(x, y)| y - x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_grows_from_word_to_brackets_and_lines() {
        let t = "void m() {\n    foo(bar + 1, \"text\");\n}";
        let at = t.find("bar").unwrap() + 1;
        let w = expand(t, at, at).unwrap();
        assert_eq!(&t[w.0..w.1], "bar");
        let p = expand(t, w.0, w.1).unwrap();
        assert_eq!(&t[p.0..p.1], "bar + 1, \"text\"");
        let p2 = expand(t, p.0, p.1).unwrap();
        assert_eq!(&t[p2.0..p2.1], "(bar + 1, \"text\")");
        let l = expand(t, p2.0, p2.1).unwrap();
        assert_eq!(&t[l.0..l.1], "foo(bar + 1, \"text\");");
        let s = t.find("text").unwrap();
        let st = expand(t, s, s + 4).unwrap();
        assert_eq!(&t[st.0..st.1], "\"text\"");
    }
}
