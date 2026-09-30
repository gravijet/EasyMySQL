// Befehle (Strg+Umschalt+P) und Datei im Projekt oeffnen (Strg+P), wie in VS Code.

use super::*;
use eframe::egui::{Key, RichText, Sense, vec2};

pub struct Palette {
    /// true = Befehle, false = Dateien
    commands: bool,
    query: String,
    selected: usize,
    focus: bool,
}

/// Befehle, die nur im Editor mit Cursor sinnvoll sind, fehlen in der Liste
fn listed(c: Cmd) -> bool {
    !matches!(
        c,
        Cmd::Suggest
            | Cmd::ToggleComment
            | Cmd::CutLine
            | Cmd::DuplicateLine
            | Cmd::DeleteLine
            | Cmd::SelectLine
            | Cmd::MoveLineUp
            | Cmd::MoveLineDown
            | Cmd::CopyLineUp
            | Cmd::CopyLineDown
            | Cmd::Indent
            | Cmd::Outdent
            | Cmd::CommandPalette
    )
}

/// Unscharfe Suche: alle Zeichen der Eingabe in dieser Reihenfolge. Kleinere Werte = besser.
pub fn fuzzy(text: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let lq = query.to_lowercase();
    if let Some(pos) = text.to_lowercase().find(&lq) {
        return Some(pos);
    }
    let mut score = 1000;
    let mut i = 0;
    let mut last = 0;
    for qc in lq.chars().filter(|c| !c.is_whitespace()) {
        let found = t[i..].iter().position(|c| *c == qc)?;
        score += found + if i > 0 && found > 0 { 5 } else { 0 };
        last = i + found;
        i = last + 1;
    }
    Some(score + last)
}

impl Palette {
    pub fn new(commands: bool) -> Self {
        Self { commands, query: if commands { ">".into() } else { String::new() }, selected: 0, focus: true }
    }
}

enum Pick {
    Cmd(Cmd),
    File(PathBuf),
}

impl EasyApp {
    pub(super) fn palette_ui(&mut self, ctx: &egui::Context) {
        if self.palette.as_ref().is_some_and(|p| !p.query.starts_with('>')) {
            self.rescan_tree();
        }
        let Some(p) = self.palette.as_mut() else { return };
        // ">" am Anfang = Befehle (wie in VS Code)
        p.commands = p.query.starts_with('>');
        let q = p.query.trim_start_matches('>').trim().to_string();
        let mut items: Vec<(usize, String, String, Pick)> = Vec::new();
        if p.commands {
            for c in Cmd::all().filter(|c| listed(*c)) {
                if let Some(s) = fuzzy(c.label(), &q) {
                    items.push((s, c.label().to_string(), keymap::text(c), Pick::Cmd(c)));
                }
            }
        } else {
            let mut files = Vec::new();
            if let Some(t) = &self.tree {
                side::text_files(t, &mut files);
            }
            let root = self.project.clone().unwrap_or_default();
            for f in files {
                let rel = f.strip_prefix(&root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if let Some(s) = fuzzy(&name, &q).map(|s| s / 2).or_else(|| fuzzy(&rel, &q)) {
                    items.push((s, name, rel, Pick::File(f)));
                }
            }
        }
        items.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        items.truncate(40);
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if !items.is_empty() {
            if down {
                p.selected = (p.selected + 1) % items.len();
            }
            if up {
                p.selected = (p.selected + items.len() - 1) % items.len();
            }
        }
        p.selected = p.selected.min(items.len().saturating_sub(1));
        let mut pick: Option<usize> = if enter && !items.is_empty() { Some(p.selected) } else { None };
        let mut close = esc || enter;
        let pal = style::pal();
        let screen = ctx.content_rect();
        let width = 560.0f32.min(screen.width() - 40.0);
        let area = egui::Area::new(egui::Id::new("palette"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.top() + 40.0))
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.set_width(width);
                    let hint = if p.commands { "Befehl" } else { "Datei im Projekt (> für Befehle)" };
                    let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(hint).desired_width(f32::INFINITY));
                    if p.focus {
                        r.request_focus();
                        p.focus = false;
                    }
                    if r.changed() {
                        p.selected = 0;
                    }
                    if items.is_empty() {
                        let t = if !p.commands && self.project.is_none() { "Kein Ordner geöffnet" } else { "Nichts gefunden" };
                        ui.label(RichText::new(t).color(pal.text_weak));
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (i, (_, label, right, _)) in items.iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
                            if i == p.selected {
                                ui.painter().rect_filled(rect, 2.0, pal.selection);
                                if down || up {
                                    resp.scroll_to_me(None);
                                }
                            } else if resp.hovered() {
                                ui.painter().rect_filled(rect, 2.0, pal.hover);
                            }
                            ui.painter().text(rect.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), pal.text);
                            ui.painter().text(rect.right_center() - vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, right, egui::FontId::proportional(12.0), pal.text_weak);
                            if resp.clicked() {
                                pick = Some(i);
                                close = true;
                            }
                        }
                    });
                });
            });
        // Klick ausserhalb schliesst
        if ctx.input(|i| i.pointer.any_pressed()) && !ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| area.response.rect.contains(pos)) {
            close = true;
        }
        if close {
            self.palette = None;
        }
        if let Some(i) = pick {
            match items.swap_remove(i).3 {
                Pick::Cmd(c) => self.run_cmd(ctx, c),
                Pick::File(f) => self.open_file(&f),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn fuzzy_order() {
        assert!(fuzzy("Neue Abfrage", "abfr").is_some());
        assert!(fuzzy("Neue Abfrage", "nab").is_some());
        assert!(fuzzy("Neue Abfrage", "xyz").is_none());
        assert!(fuzzy("kunden.sql", "kun").unwrap() < fuzzy("alte_kunden.sql", "kun").unwrap());
        assert!(fuzzy("ER-Diagramm", "er").unwrap() < fuzzy("Server-Log", "er").unwrap());
    }
}
