// Terminal-Bereich unter dem Editor (wie in Visual Studio Code): mehrere Terminals in Registerkarten.

use super::*;
use crate::icons::{self, Icon};
use crate::terminal::{self, Profile, Session};
use eframe::egui::{Sense, Stroke, pos2, vec2};

struct TermTab {
    uid: usize,
    title: String,
    session: Session,
}

#[derive(Default)]
pub struct TerminalDock {
    pub open: bool,
    tabs: Vec<TermTab>,
    active: usize,
    next_uid: usize,
    /// Der Fokus soll im naechsten Bild auf das aktive Terminal gehen
    focus: bool,
}

impl TerminalDock {
    pub fn close_all(&mut self) {
        self.tabs.clear();
    }
}

impl EasyApp {
    fn terminal_profiles(&self) -> Vec<Profile> {
        // Mit SSH-Tunnel gilt die Adresse des Tunnels
        let conn = self.db.as_ref().map(|d| d.info.clone()).unwrap_or_else(|| self.settings.conn.clone());
        terminal::profiles(self.server.paths.as_ref().map(|p| p.bin.as_path()), &conn)
    }

    /// Neues Terminal in der gewaehlten Shell (None = Standard) im Projektordner
    pub(super) fn new_terminal(&mut self, ctx: &egui::Context, profile: Option<Profile>) {
        let profile = profile.or_else(|| self.terminal_profiles().into_iter().next());
        let Some(profile) = profile else { return };
        let cwd = self
            .project
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        let prefix = self.server.paths.as_ref().map(|p| p.bin.clone());
        let d = &mut self.terminal;
        let same = d.tabs.iter().filter(|t| t.session.profile == profile).count();
        let title = if same == 0 { profile.name.clone() } else { format!("{} {}", profile.name, same + 1) };
        d.next_uid += 1;
        d.tabs.push(TermTab { uid: d.next_uid, title, session: Session::spawn(ctx, profile, cwd, prefix) });
        d.active = d.tabs.len() - 1;
        d.open = true;
        d.focus = true;
    }

    /// Terminal-Bereich ein- oder ausblenden (laufende Shells bleiben erhalten)
    pub(super) fn toggle_terminal(&mut self, ctx: &egui::Context) {
        if self.terminal.open {
            self.terminal.open = false;
        } else if self.terminal.tabs.is_empty() {
            self.new_terminal(ctx, None);
        } else {
            self.terminal.open = true;
            self.terminal.focus = true;
        }
    }

    pub(super) fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let ctx = ui.ctx().clone();
        let profiles = self.terminal_profiles();
        let mut new_profile: Option<Option<Profile>> = None;
        let mut close: Option<usize> = None;
        let mut hide = false;
        let frame = egui::Frame::new().fill(pal.bg).stroke(Stroke::new(1.0, pal.border));
        egui::Panel::bottom("terminal-dock").resizable(true).default_size(280.0).min_size(110.0).frame(frame).show(ui, |ui| {
            // Kopfzeile: Registerkarten links, Aktionen rechts
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            let painter = ui.painter_at(bar);
            painter.rect_filled(bar, 0.0, pal.tab_bar_bg);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
            child.spacing_mut().item_spacing.x = 0.0;
            child.add_space(10.0);
            child.label(egui::RichText::new(crate::i18n::text("TERMINAL")).small().color(pal.text_weak));
            child.add_space(10.0);
            let active = self.terminal.active.min(self.terminal.tabs.len().saturating_sub(1));
            for (i, t) in self.terminal.tabs.iter().enumerate() {
                let sel = i == active;
                let galley = painter.layout_no_wrap(t.title.clone(), egui::FontId::proportional(13.0), pal.text);
                let w = galley.size().x + 56.0;
                let (r, resp) = child.allocate_exact_size(vec2(w, 30.0), Sense::click());
                if sel {
                    painter.rect_filled(r, 0.0, pal.tab_active);
                    painter.rect_filled(egui::Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0.0, pal.accent);
                } else if resp.hovered() {
                    painter.rect_filled(r, 0.0, pal.hover);
                }
                let color = if sel { pal.text } else { pal.text_weak };
                icons::paint(&painter, egui::Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)), Icon::Terminal, color);
                painter.galley(pos2(r.left() + 30.0, r.center().y - galley.size().y / 2.0), galley, color);
                let cr = egui::Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0));
                let over_close = ctx.pointer_hover_pos().is_some_and(|p| cr.contains(p));
                if sel || resp.hovered() {
                    if over_close {
                        painter.rect_filled(cr, 3.0, pal.hover);
                    }
                    icons::paint(&painter, cr, Icon::Close, color);
                }
                if resp.clicked() {
                    if over_close {
                        close = Some(i);
                    } else {
                        self.terminal.active = i;
                        self.terminal.focus = true;
                    }
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
            }
            // Aktionen rechts
            let mut right = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if icons::button(&mut right, Icon::Close, crate::i18n::text("Terminal ausblenden")).clicked() {
                hide = true;
            }
            if icons::button(&mut right, Icon::Trash, crate::i18n::text("Dieses Terminal beenden")).clicked() && !self.terminal.tabs.is_empty() {
                close = Some(active);
            }
            let more = icons::button(&mut right, Icon::ChevronDown, crate::i18n::text("Shell wählen"));
            egui::Popup::menu(&more).show(|ui| {
                for p in &profiles {
                    if ui.button(&p.name).clicked() {
                        new_profile = Some(Some(p.clone()));
                        ui.close();
                    }
                }
            });
            if icons::button(&mut right, Icon::Plus, crate::i18n::text("Neues Terminal")).clicked() {
                new_profile = Some(None);
            }

            // Inhalt
            if let Some(t) = self.terminal.tabs.get_mut(active) {
                let focus = std::mem::take(&mut self.terminal.focus);
                terminal::show(ui, egui::Id::new(("terminal", t.uid)), &mut t.session, focus);
            }
        });
        if let Some(i) = close
            && i < self.terminal.tabs.len()
        {
            self.terminal.tabs.remove(i);
            self.terminal.active = self.terminal.active.min(self.terminal.tabs.len().saturating_sub(1));
            self.terminal.focus = true;
            if self.terminal.tabs.is_empty() {
                self.terminal.open = false;
            }
        }
        if hide {
            self.terminal.open = false;
        }
        if let Some(p) = new_profile {
            self.new_terminal(&ctx, p);
        }
    }
}
