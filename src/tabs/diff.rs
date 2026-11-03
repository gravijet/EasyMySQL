// Registerkarte mit den Unterschieden einer Datei bzw. eines Commits (aus Git): hinzugefuegte Zeilen gruen,
// entfernte rot, mit Zeilennummern der neuen Fassung.

use super::{Ctx, TabView};
use crate::icons::Icon;
use crate::style;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

pub struct DiffTab {
    /// Datei im Arbeitsordner (zum Oeffnen) bzw. Repository bei Commits
    path: PathBuf,
    title: String,
    staged: bool,
    text: String,
    /// Nur Aenderungen zeigen (Kopfzeilen von git ausblenden)
    compact: bool,
}

impl DiffTab {
    pub fn new(path: PathBuf, title: String, staged: bool, text: String) -> Self {
        Self { path, title, staged, text, compact: true }
    }
}

/// Zeile der Ausgabe von git diff: (Art, Nummer in der neuen Fassung, Text)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Header,
    Hunk,
    Added,
    Removed,
    Context,
}

pub fn classify(text: &str) -> Vec<(LineKind, Option<usize>, &str)> {
    let mut out = Vec::new();
    let mut new_line = 0usize;
    let mut in_hunk = false;
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("@@") {
            // @@ -a,b +c,d @@
            new_line = rest.split('+').nth(1).and_then(|x| x.split([',', ' ']).next()).and_then(|n| n.parse().ok()).unwrap_or(1);
            in_hunk = true;
            out.push((LineKind::Hunk, None, l));
        } else if !in_hunk || l.starts_with("diff --git") || l.starts_with("+++ ") || l.starts_with("--- ") {
            in_hunk = !l.starts_with("diff --git") && in_hunk;
            out.push((LineKind::Header, None, l));
        } else if let Some(t) = l.strip_prefix('+') {
            out.push((LineKind::Added, Some(new_line), t));
            new_line += 1;
        } else if let Some(t) = l.strip_prefix('-') {
            out.push((LineKind::Removed, None, t));
        } else {
            out.push((LineKind::Context, Some(new_line), l.strip_prefix(' ').unwrap_or(l)));
            new_line += 1;
        }
    }
    out
}

impl TabView for DiffTab {
    fn title(&self) -> String {
        let name = Path::new(&self.title).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.title.clone());
        if self.staged { crate::tr_format!("{name} (vorgemerkt)", "{name} (staged)") } else { crate::tr_format!("{name} (Änderungen)", "{name} (changes)") }
    }

    fn icon(&self) -> Option<Icon> {
        Some(Icon::GitCommit)
    }

    fn key(&self) -> Option<String> {
        Some(format!("diff:{}:{}", self.staged, self.path.display()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.title).monospace());
            if self.path.is_file() && ui.button(crate::i18n::tr("Datei öffnen", "Open file")).clicked() {
                cx.actions.push(super::Action::OpenFileAt { path: self.path.clone(), line: 0 });
            }
            ui.checkbox(&mut self.compact, crate::i18n::tr("Nur Änderungen", "Changes only"));
        });
        ui.add_space(4.0);
        let font = egui::FontId::monospace(crate::sqledit::font_size());
        egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(style::RADIUS + 1).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let lines = classify(&self.text);
                if lines.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Keine Unterschiede (z. B. nur Rechte geändert).", "No differences (e.g. only permissions changed).")).color(pal.text_weak));
                }
                for (kind, n, text) in lines {
                    if self.compact && kind == LineKind::Header {
                        continue;
                    }
                    let (bg, fg, mark) = match kind {
                        LineKind::Added => (pal.diff_add_bg, pal.ok_text, "+"),
                        LineKind::Removed => (pal.diff_del_bg, pal.error_text, "-"),
                        LineKind::Hunk => (egui::Color32::TRANSPARENT, pal.accent, ""),
                        LineKind::Header => (egui::Color32::TRANSPARENT, pal.text_weak, ""),
                        LineKind::Context => (egui::Color32::TRANSPARENT, pal.text, " "),
                    };
                    let num = n.map(|n| format!("{n:>5} ")).unwrap_or_else(|| "      ".into());
                    let job = {
                        let mut j = egui::text::LayoutJob::default();
                        j.append(&num, 0.0, egui::TextFormat::simple(font.clone(), pal.syn_gutter));
                        j.append(&format!("{mark}{text}"), 0.0, egui::TextFormat { font_id: font.clone(), color: fg, background: bg, ..Default::default() });
                        j
                    };
                    ui.label(job);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines_get_numbers_of_the_new_file() {
        let d = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        let l = classify(d);
        assert_eq!(l[3].0, LineKind::Hunk);
        assert_eq!((l[4].0, l[4].1), (LineKind::Context, Some(1)));
        assert_eq!((l[5].0, l[5].2), (LineKind::Removed, "b"));
        assert_eq!((l[6].0, l[6].1, l[6].2), (LineKind::Added, Some(2), "B"));
        assert_eq!(l[7].1, Some(3));
    }
}
