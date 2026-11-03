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
    /// Treffer zur letzten Suche (Kapitel, Abschnitt): nicht in jedem Bild neu suchen
    hits: (String, Vec<(usize, usize)>),
    /// Zu diesem Abschnitt scrollen
    goto: Option<String>,
    active_section: String,
    scroll_offset: Option<f32>,
    pinned_section: bool,
}

impl HelpTab {
    pub fn new() -> Self {
        Self { chapter: 0, search: String::new(), hits: (String::new(), Vec::new()), goto: None, active_section: CHAPTERS[0].sections[0].id.into(), scroll_offset: None, pinned_section: false }
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
fn hanging(ui: &mut egui::Ui, marker: RichText, text: &str, words: &[String]) {
    ui.horizontal_top(|ui| {
        ui.add_sized([20.0, 19.0], egui::Label::new(marker));
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width());
            ui.label(body(text, words));
        });
    });
}

/// Fliesstext mit etwas mehr Zeilenabstand; Suchwoerter farbig hinterlegt
fn body(text: &str, words: &[String]) -> egui::text::LayoutJob {
    marked(text, words, egui::FontId::proportional(13.0), style::pal().text, Some(19.5))
}

/// Text mit hervorgehobenen Treffern (Gross-/Kleinschreibung egal)
fn marked(text: &str, words: &[String], font: egui::FontId, color: egui::Color32, line_height: Option<f32>) -> egui::text::LayoutJob {
    let pal = style::pal();
    let plain = egui::TextFormat { font_id: font.clone(), color, line_height, ..Default::default() };
    let hit = egui::TextFormat { background: pal.warn_text.gamma_multiply(if pal.dark { 0.35 } else { 0.45 }), color: if pal.dark { egui::Color32::WHITE } else { color }, ..plain.clone() };
    let mut job = egui::text::LayoutJob::default();
    let ranges = match_ranges(text, words);
    let mut at = 0;
    for (a, b) in ranges {
        job.append(&text[at..a], 0.0, plain.clone());
        job.append(&text[a..b], 0.0, hit.clone());
        at = b;
    }
    job.append(&text[at..], 0.0, plain);
    job
}

/// Byte-Bereiche der Suchwoerter in `text`, sortiert und ohne Ueberschneidung
fn match_ranges(text: &str, words: &[String]) -> Vec<(usize, usize)> {
    if words.is_empty() {
        return Vec::new();
    }
    // Je Zeichen ein kleines Zeichen (1:1, damit die Positionen stimmen)
    let chars: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, c.to_lowercase().next().unwrap_or(c))).collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for w in words {
        let wc: Vec<char> = w.chars().collect();
        if wc.is_empty() || wc.len() > chars.len() {
            continue;
        }
        for i in 0..=chars.len() - wc.len() {
            if chars[i..i + wc.len()].iter().map(|(_, c)| *c).eq(wc.iter().copied()) {
                let end = chars.get(i + wc.len()).map(|(b, _)| *b).unwrap_or(text.len());
                out.push((chars[i].0, end));
            }
        }
    }
    out.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in out {
        match merged.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
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

fn section_ui(ui: &mut egui::Ui, s: &Section, cx: &mut Ctx, goto: &mut Option<String>, words: &[String]) {
    let pal = style::pal();
    ui.add_space(10.0);
    let r = ui.label(marked(crate::i18n::text(s.title), words, egui::FontId::proportional(20.0), pal.text, None));
    if goto.as_deref() == Some(s.id) {
        r.scroll_to_me_animation(Some(egui::Align::TOP), egui::style::ScrollAnimation::none());
        *goto = None;
    }
    ui.add_space(6.0);
    for b in s.blocks.iter() {
        match b {
            B::P(t) => {
                ui.label(body(crate::i18n::text(t), words));
            }
            B::List(items) => {
                for it in items.iter() {
                    hanging(ui, RichText::new("•").color(pal.text_weak), crate::i18n::text(it), words);
                }
            }
            B::Steps(items) => {
                for (i, it) in items.iter().enumerate() {
                    hanging(ui, RichText::new(format!("{}.", i + 1)).strong().color(pal.accent), crate::i18n::text(it), words);
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
                    ui.label(marked(crate::i18n::text(list[i].1), words, egui::FontId::proportional(13.0), pal.text, None));
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
                    ui.label(body(crate::i18n::text(t), words));
                });
                crate::icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.response.rect.left() + 18.0, r.response.rect.top() + 16.0), egui::vec2(16.0, 16.0)), crate::icons::Icon::Info, pal.accent);
                ui.painter().rect_filled(egui::Rect::from_min_size(r.response.rect.min + egui::vec2(0.0, 5.0), egui::vec2(3.0, r.response.rect.height() - 10.0)), 1.5, pal.accent);
            }
            B::Code(code) => {
                code_frame().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut job = highlighted(code);
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

/// Eingefaerbter SQL-Code der Beispiele, einmal je Text und Design berechnet
fn highlighted(code: &'static str) -> egui::text::LayoutJob {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<(usize, bool), egui::text::LayoutJob>>> = Mutex::new(None);
    let key = (code.as_ptr() as usize, style::pal().dark);
    let mut c = CACHE.lock().unwrap();
    c.get_or_insert_with(HashMap::new).entry(key).or_insert_with(|| crate::sqledit::highlight(code, egui::FontId::monospace(13.0))).clone()
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
        let words: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        // Suche: Abschnitte, in denen alle Woerter vorkommen (nur neu, wenn sich die Suche geaendert hat)
        if self.hits.0 != query {
            let found = if query.is_empty() {
                Vec::new()
            } else {
                CHAPTERS
                    .iter()
                    .enumerate()
                    .flat_map(|(ci, c)| c.sections.iter().enumerate().map(move |(si, s)| (ci, si, s)))
                    .filter(|(_, _, s)| {
                        let t = section_text(s);
                        words.iter().all(|w| t.contains(w.as_str()))
                    })
                    .map(|(ci, si, _)| (ci, si))
                    .collect()
            };
            self.hits = (query.clone(), found);
        }
        let hits: Vec<(usize, &Section)> = self.hits.1.iter().map(|&(ci, si)| (ci, &CHAPTERS[ci].sections[si])).collect();

        if ui.available_width() < 720.0 {
            // Schmales Fenster: Suche und Kapitelwahl in einer Zeile ueber dem Text statt Inhaltsverzeichnis links
            ui.horizontal_wrapped(|ui| {
                let r = crate::widgets::text_field(ui, &mut self.search, crate::i18n::text("Im Handbuch suchen"), Some((ui.available_width() - 200.0).max(140.0)));
                if r.changed() {
                    self.goto = None;
                }
                let mut ch = self.chapter;
                crate::widgets::combo("help-chapter").selected_text(crate::i18n::text(CHAPTERS[self.chapter.min(CHAPTERS.len() - 1)].title)).width(180.0).show_ui(ui, |ui| {
                    for (ci, c) in CHAPTERS.iter().enumerate() {
                        if ui.selectable_label(ch == ci, crate::i18n::text(c.title)).clicked() {
                            ch = ci;
                        }
                    }
                });
                if ch != self.chapter {
                    self.chapter = ch;
                    self.search.clear();
                    self.goto = Some(CHAPTERS[ch].sections[0].id.into());
                    self.active_section = CHAPTERS[ch].sections[0].id.into();
                    self.pinned_section = true;
                }
            });
            ui.add_space(4.0);
        } else {
        egui::Panel::left("help-toc").resizable(true).show_separator_line(false).default_size(230.0).min_size(170.0).frame(egui::Frame::new().inner_margin(egui::Margin { left: 4, right: 10, top: 6, bottom: 6 })).show(ui, |ui| {
            let r = crate::widgets::text_field(ui, &mut self.search, crate::i18n::text("Im Handbuch suchen"), None);
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
        }

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
                        section_ui(ui, s, cx, &mut self.goto, &words);
                    }
                    return;
                }
                let c = &CHAPTERS[self.chapter.min(CHAPTERS.len() - 1)];
                ui.label(RichText::new(crate::i18n::text(c.title)).size(24.0));
                ui.add_space(10.0);
                for s in c.sections {
                    headings.push((s.id, ui.cursor().top()));
                    section_ui(ui, s, cx, &mut self.goto, &[]);
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
    fn search_words_are_marked_case_insensitive() {
        let w = vec!["tab".to_string(), "über".to_string()];
        assert_eq!(match_ranges("Tabelle und Übersicht", &w), vec![(0, 3), (12, 17)]);
        assert!(match_ranges("nichts", &w).is_empty());
        assert_eq!(match_ranges("aaa", &["aa".to_string()]), vec![(0, 3)]);
    }

    #[test]
    fn search_text_contains_blocks() {
        let s = CHAPTERS.iter().flat_map(|c| c.sections.iter()).find(|s| s.id == "assistent-mengen").unwrap();
        let t = section_text(s);
        assert!(t.contains("intersect all") && t.contains("except"));
    }
}
