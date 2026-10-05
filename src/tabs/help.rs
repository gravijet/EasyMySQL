// Registerkarte "Hilfe": Handbuch mit Inhaltsverzeichnis, Suche, aktuellen Tastenkuerzeln und
// Knoepfen, die das Beschriebene direkt oeffnen.

use super::help_content::{B, CHAPTERS, Section, Target};
use super::{Action, Ctx, TabView};
use crate::keymap;
use crate::style;
use eframe::egui::{self, RichText};

pub struct HelpTab {
    chapter: usize,
    search: String,
    /// Zu diesem Abschnitt scrollen
    goto: Option<String>,
    active_section: String,
    scroll_offset: Option<f32>,
    pinned_section: bool,
}

impl HelpTab {
    pub fn new() -> Self {
        Self { chapter: 0, search: String::new(), goto: None, active_section: CHAPTERS[0].sections[0].id.into(), scroll_offset: None, pinned_section: false }
    }
}

/// Gesamter Text eines Abschnitts (fuer die Suche)
fn section_text(s: &Section) -> String {
    let mut t = crate::i18n::text(s.title).to_string();
    for b in s.blocks {
        match b {
            B::P(x) | B::Tip(x) | B::Code(x) | B::Shell(x) | B::Open(x, _) => {
                t.push(' ');
                t.push_str(crate::i18n::text(x));
            }
            B::List(l) | B::Steps(l) => {
                for x in l.iter() {
                    t.push(' ');
                    t.push_str(crate::i18n::text(x));
                }
            }
            B::Table(rows) => {
                for (a, b) in rows.iter() {
                    t.push_str(&format!(" {} {}", crate::i18n::text(a), crate::i18n::text(b)));
                }
            }
            B::Keys(cmds) => {
                for c in cmds.iter() {
                    t.push_str(&format!(" {} {}", c.label(), keymap::text(*c)));
                }
            }
        }
    }
    t.to_lowercase()
}

/// Aufzaehlungspunkt mit umbrechendem Text (haengender Einzug)
fn hanging(ui: &mut egui::Ui, marker: RichText, text: &str) {
    ui.horizontal_top(|ui| {
        ui.add_sized([14.0, 16.0], egui::Label::new(marker));
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.label(text);
        });
    });
}

fn shortcut_chip(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(style::pal().face_light)
        .stroke(egui::Stroke::new(1.0, style::pal().border))
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(12.0));
        });
}

fn section_ui(ui: &mut egui::Ui, s: &Section, cx: &mut Ctx, goto: &mut Option<String>) {
    let pal = style::pal();
    let r = ui.label(RichText::new(crate::i18n::text(s.title)).size(18.0).strong());
    if goto.as_deref() == Some(s.id) {
        r.scroll_to_me_animation(Some(egui::Align::TOP), egui::style::ScrollAnimation::none());
        *goto = None;
    }
    ui.add_space(2.0);
    for (bi, b) in s.blocks.iter().enumerate() {
        match b {
            B::P(t) => {
                ui.label(crate::i18n::text(t));
            }
            B::List(items) => {
                for it in items.iter() {
                    hanging(ui, RichText::new("•").color(pal.text_weak), crate::i18n::text(it));
                }
            }
            B::Steps(items) => {
                for (i, it) in items.iter().enumerate() {
                    hanging(ui, RichText::new(format!("{}.", i + 1)).strong().color(pal.accent), crate::i18n::text(it));
                }
            }
            B::Shell(cmd) => {
                ui.horizontal(|ui| {
                    style::sunken_frame().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                        ui.label(RichText::new(*cmd).monospace());
                    });
                    if ui.small_button(crate::i18n::text("Kopieren")).clicked() {
                        ui.ctx().copy_text(cmd.to_string());
                    }
                });
            }
            B::Table(rows) => {
                egui::Grid::new((s.id, bi)).num_columns(2).striped(true).spacing([18.0, 5.0]).show(ui, |ui| {
                    for (a, b) in rows.iter() {
                        ui.label(RichText::new(crate::i18n::text(a)).strong());
                        ui.label(crate::i18n::text(b));
                        ui.end_row();
                    }
                });
            }
            B::Keys(cmds) => {
                let km = keymap::current();
                egui::Grid::new((s.id, bi, "k")).num_columns(2).striped(true).spacing([18.0, 4.0]).show(ui, |ui| {
                    for c in cmds.iter() {
                        ui.label(c.label());
                        ui.horizontal(|ui| {
                            let bs = km.get(*c);
                            if bs.is_empty() {
                                ui.label(RichText::new(crate::i18n::text("kein Kürzel")).color(pal.text_weak));
                            }
                            for b in bs {
                                shortcut_chip(ui, &b.text());
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            B::Tip(t) => {
                let r = egui::Frame::new().fill(pal.face_light).inner_margin(egui::Margin { left: 12, right: 8, top: 6, bottom: 6 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(crate::i18n::text(t));
                });
                ui.painter().rect_filled(egui::Rect::from_min_size(r.response.rect.min, egui::vec2(3.0, r.response.rect.height())), 0.0, pal.accent);
            }
            B::Code(code) => {
                style::sunken_frame().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut job = crate::sqledit::highlight(code, egui::FontId::monospace(13.0));
                    job.wrap.max_width = f32::INFINITY;
                    ui.add(egui::Label::new(job).selectable(true));
                });
                ui.horizontal(|ui| {
                    if ui.small_button(crate::i18n::text("In neuer Abfrage öffnen")).clicked() {
                        cx.actions.push(Action::OpenSql { db: None, sql: format!("{code}\n"), run: false });
                    }
                    if ui.small_button(crate::i18n::text("Kopieren")).clicked() {
                        ui.ctx().copy_text(code.to_string());
                    }
                });
            }
            B::Open(label, target) => {
                if ui.button(crate::i18n::text(label)).clicked() {
                    match target {
                        Target::Cmd(c) => cx.actions.push(Action::Run(*c)),
                        Target::Settings(id) => cx.actions.push(Action::OpenSettings(Some(id.to_string()))),
                        Target::Section(id) => *goto = Some(id.to_string()),
                    }
                }
            }
        }
        ui.add_space(4.0);
    }
    ui.add_space(18.0);
}

impl TabView for HelpTab {
    fn title(&self) -> String {
        crate::i18n::text("Handbuch").into()
    }

    fn key(&self) -> Option<String> {
        Some("help".into())
    }

    fn session(&self) -> Option<String> {
        Some("help".into())
    }

    fn show_section(&mut self, id: &str) {
        if let Some(i) = CHAPTERS.iter().position(|c| c.sections.iter().any(|s| s.id == id)) {
            self.chapter = i;
            self.search.clear();
            self.goto = Some(id.to_string());
            self.active_section = id.to_string();
            self.pinned_section = true;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let query = self.search.trim().to_lowercase();
        // Suche: Abschnitte, in denen alle Woerter vorkommen
        let hits: Vec<(usize, &Section)> = if query.is_empty() {
            Vec::new()
        } else {
            CHAPTERS
                .iter()
                .enumerate()
                .flat_map(|(ci, c)| c.sections.iter().map(move |s| (ci, s)))
                .filter(|(_, s)| {
                    let t = section_text(s);
                    query.split_whitespace().all(|w| t.contains(w))
                })
                .collect()
        };

        egui::Panel::left("help-toc").resizable(true).default_size(230.0).min_size(170.0).frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 6))).show(ui, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(crate::i18n::text("Im Handbuch suchen")).desired_width(f32::INFINITY));
            if r.changed() {
                self.goto = None;
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().id_salt("help-toc-scroll").auto_shrink([false, false]).show(ui, |ui| {
                for (ci, c) in CHAPTERS.iter().enumerate() {
                    let sel = self.chapter == ci && query.is_empty();
                    let title = RichText::new(crate::i18n::text(c.title)).strong();
                    if ui.selectable_label(sel, if sel { title.color(if pal.dark { pal.text } else { pal.accent }) } else { title }).clicked() {
                        self.chapter = ci;
                        self.search.clear();
                        self.goto = Some(c.sections[0].id.into());
                        self.active_section = c.sections[0].id.into();
                        self.pinned_section = true;
                    }
                    if sel {
                        for s in c.sections {
                            if style::nav_item(ui, crate::i18n::text(s.title), self.active_section == s.id, 16.0).clicked() {
                                self.goto = Some(s.id.to_string());
                                self.active_section = s.id.to_string();
                                self.pinned_section = true;
                            }
                        }
                    }
                }
            });
        });

        let jumping = self.goto.is_some();
        let mut offset = 0.0;
        let mut headings = Vec::new();
        let body = egui::ScrollArea::vertical().id_salt(("help-body", self.chapter, query.is_empty())).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            offset = viewport.min.y;
            egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 12)).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(820.0));
                ui.add_space(6.0);
                if !query.is_empty() {
                    ui.label(RichText::new(crate::tr_format!("{} Abschnitt(e) gefunden", "{} section(s) found", hits.len())).color(pal.text_weak));
                    ui.add_space(8.0);
                    for (ci, s) in &hits {
                        ui.label(RichText::new(crate::i18n::text(CHAPTERS[*ci].title)).small().color(pal.text_weak));
                        section_ui(ui, s, cx, &mut self.goto);
                    }
                    return;
                }
                let c = &CHAPTERS[self.chapter.min(CHAPTERS.len() - 1)];
                ui.label(RichText::new(crate::i18n::text(c.title)).size(24.0));
                ui.add_space(10.0);
                for s in c.sections {
                    headings.push((s.id, ui.cursor().top()));
                    section_ui(ui, s, cx, &mut self.goto);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if self.chapter > 0 && ui.button(format!("← {}", crate::i18n::text(CHAPTERS[self.chapter - 1].title))).clicked() {
                        self.chapter -= 1;
                        self.goto = CHAPTERS[self.chapter].sections.first().map(|s| s.id.to_string());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.chapter + 1 < CHAPTERS.len() && ui.button(format!("{} →", crate::i18n::text(CHAPTERS[self.chapter + 1].title))).clicked() {
                            self.chapter += 1;
                            self.goto = CHAPTERS[self.chapter].sections.first().map(|s| s.id.to_string());
                        }
                    });
                });
            });
        });
        // Nach einem Sprung die angeklickte Kategorie behalten, auch wenn der
        // letzte Abschnitt wegen der Fensterhoehe nicht bis nach oben scrollt.
        if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| egui::Rect::from_min_max(body.inner_rect.min, body.inner_rect.max + egui::vec2(16.0, 0.0)).contains(p))
            && (i.smooth_scroll_delta != egui::Vec2::ZERO || i.pointer.any_down())) {
            self.pinned_section = false;
        }
        if !jumping && !self.pinned_section && query.is_empty() && self.scroll_offset != Some(offset) {
            if let Some((id, _)) = headings.iter().rev().find(|(_, y)| *y <= body.inner_rect.top() + 36.0).or(headings.first()) {
                self.active_section = (*id).into();
            }
        }
        self.scroll_offset = Some(offset);
        self.pinned_section |= jumping;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_text_contains_blocks() {
        let s = CHAPTERS.iter().flat_map(|c| c.sections.iter()).find(|s| s.id == "assistent-mengen").unwrap();
        let t = section_text(s);
        assert!(t.contains("intersect all") && t.contains("except"));
    }
}
