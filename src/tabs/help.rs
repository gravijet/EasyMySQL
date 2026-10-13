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
            B::P(x) | B::Tip(x) | B::Code(x) | B::Source(x) | B::Shell(x) | B::Open(x, _) => {
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
        ui.add_sized([20.0, 19.0], egui::Label::new(marker));
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.label(body(text));
        });
    });
}

/// Fliesstext mit etwas mehr Zeilenabstand
fn body(text: &str) -> RichText {
    RichText::new(text).line_height(Some(19.5))
}

/// Kleiner Knopf mit Symbol und Text (Kopieren, Oeffnen)
fn small_action(ui: &mut egui::Ui, icon: crate::icons::Icon, text: &str) -> egui::Response {
    crate::icons::text_button(ui, icon, text)
}

fn shortcut_chip(ui: &mut egui::Ui, text: &str) {
    crate::widgets::keycap(ui, text, crate::widgets::Keycap::Normal);
}

/// Linksbuendiger Text in fester Breite
fn left_cell(ui: &mut egui::Ui, width: f32, text: RichText) {
    ui.allocate_ui_with_layout(egui::vec2(width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(text).truncate());
    });
}

/// Zeilen mit feiner Trennlinie (Tabellen und Kuerzel)
fn rows(ui: &mut egui::Ui, n: usize, mut row: impl FnMut(&mut egui::Ui, usize)) {
    let line = style::pal().border.gamma_multiply(0.5);
    for i in 0..n {
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            row(ui, i);
        });
        if i + 1 < n {
            let y = ui.cursor().top();
            ui.painter().hline(ui.max_rect().x_range(), y - 2.0, egui::Stroke::new(1.0, line));
        }
    }
}

/// Rahmen fuer Codebloecke
fn code_frame() -> egui::Frame {
    let p = style::pal();
    egui::Frame::new().fill(if p.dark { rgb_dark() } else { p.face_light }).stroke(egui::Stroke::new(1.0, p.border.gamma_multiply(0.7))).corner_radius(6)
}

fn rgb_dark() -> egui::Color32 {
    egui::Color32::from_rgb(0x1A, 0x1A, 0x1B)
}

/// Kapitel im Inhaltsverzeichnis: Pfeil, fett wenn aufgeklappt
fn toc_chapter(ui: &mut egui::Ui, title: &str, open: bool) -> egui::Response {
    let p = style::pal();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let icon = if open { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronRight };
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(14.0, 14.0)), icon, p.text_weak);
    let font = egui::FontId::proportional(13.5);
    ui.painter().text(egui::pos2(rect.left() + 28.0, rect.center().y), egui::Align2::LEFT_CENTER, title, font, if open { p.text } else { p.text.gamma_multiply(0.85) });
    resp
}

fn section_ui(ui: &mut egui::Ui, s: &Section, cx: &mut Ctx, goto: &mut Option<String>) {
    let pal = style::pal();
    ui.add_space(10.0);
    let r = ui.label(RichText::new(crate::i18n::text(s.title)).size(20.0).strong());
    if goto.as_deref() == Some(s.id) {
        r.scroll_to_me_animation(Some(egui::Align::TOP), egui::style::ScrollAnimation::none());
        *goto = None;
    }
    ui.add_space(6.0);
    for b in s.blocks.iter() {
        match b {
            B::P(t) => {
                ui.label(body(crate::i18n::text(t)));
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
                    code_frame().inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                        ui.label(RichText::new(*cmd).monospace());
                    });
                    if crate::icons::button(ui, crate::icons::Icon::Copy, crate::i18n::text("Kopieren")).clicked() {
                        ui.ctx().copy_text(cmd.to_string());
                    }
                });
            }
            B::Table(rows) => {
                let list: Vec<(&str, &str)> = rows.to_vec();
                self::rows(ui, list.len(), |ui, i| {
                    left_cell(ui, 150.0, RichText::new(crate::i18n::text(list[i].0)).strong());
                    ui.label(crate::i18n::text(list[i].1));
                });
            }
            B::Keys(cmds) => {
                let km = keymap::current();
                self::rows(ui, cmds.len(), |ui, i| {
                    let c = cmds[i];
                    left_cell(ui, 230.0, RichText::new(c.label()));
                    let bs = km.get(c);
                    if bs.is_empty() {
                        ui.label(RichText::new(crate::i18n::text("kein Kürzel")).color(pal.text_weak));
                    }
                    for b in bs {
                        shortcut_chip(ui, &b.text());
                    }
                });
            }
            B::Tip(t) => {
                let r = egui::Frame::new().fill(pal.face_light).corner_radius(6).inner_margin(egui::Margin { left: 38, right: 12, top: 9, bottom: 9 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(body(crate::i18n::text(t)));
                });
                crate::icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.response.rect.left() + 18.0, r.response.rect.top() + 16.0), egui::vec2(16.0, 16.0)), crate::icons::Icon::Info, pal.accent);
                ui.painter().rect_filled(egui::Rect::from_min_size(r.response.rect.min + egui::vec2(0.0, 5.0), egui::vec2(3.0, r.response.rect.height() - 10.0)), 1.5, pal.accent);
            }
            B::Code(code) => {
                code_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut job = crate::sqledit::highlight(code, egui::FontId::monospace(13.0));
                    job.wrap.max_width = f32::INFINITY;
                    ui.add(egui::Label::new(job).selectable(true));
                });
                ui.horizontal(|ui| {
                    if small_action(ui, crate::icons::Icon::Play, crate::i18n::text("In neuer Abfrage öffnen")).clicked() {
                        cx.actions.push(Action::OpenSql { db: None, sql: format!("{code}\n"), run: false });
                    }
                    if small_action(ui, crate::icons::Icon::Copy, crate::i18n::text("Kopieren")).clicked() {
                        ui.ctx().copy_text(code.to_string());
                    }
                });
            }
            B::Source(code) => {
                code_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(*code).monospace().color(pal.text)).selectable(true));
                });
                if small_action(ui, crate::icons::Icon::Copy, crate::i18n::text("Kopieren")).clicked() {
                    ui.ctx().copy_text(code.to_string());
                }
            }
            B::Open(label, target) => {
                if crate::icons::text_button_right(ui, crate::icons::Icon::ArrowRight, crate::i18n::text(label)).clicked() {
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
    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Book)
    }

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
                    if toc_chapter(ui, crate::i18n::text(c.title), sel).clicked() {
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
        crate::tabs::page_keys(ui);
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
                    if self.chapter > 0 && crate::icons::text_button(ui, crate::icons::Icon::ArrowLeft, crate::i18n::text(CHAPTERS[self.chapter - 1].title)).clicked() {
                        self.chapter -= 1;
                        self.goto = CHAPTERS[self.chapter].sections.first().map(|s| s.id.to_string());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.chapter + 1 < CHAPTERS.len() && crate::icons::text_button_right(ui, crate::icons::Icon::ArrowRight, crate::i18n::text(CHAPTERS[self.chapter + 1].title)).clicked() {
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
        if !jumping && !self.pinned_section && query.is_empty() && self.scroll_offset != Some(offset)
            && let Some((id, _)) = headings.iter().rev().find(|(_, y)| *y <= body.inner_rect.top() + 36.0).or(headings.first()) {
                self.active_section = (*id).into();
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
