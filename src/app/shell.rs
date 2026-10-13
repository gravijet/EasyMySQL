// Rahmen des Hauptfensters: Menue, Aktivitaetsleiste, Registerkarten, leere Flaeche, Meldungen, Sitzung.

use super::*;
use crate::icons::{self, Icon};
use crate::workspace::{self, Session};
use eframe::egui::{Sense, Stroke, pos2, vec2};

impl EasyApp {
    // ------------------------------------------------------------------
    // Sitzung

    pub(super) fn tab_from_session(&self, entry: &str) -> Option<Box<dyn TabView>> {
        let parts: Vec<&str> = entry.split('\t').collect();
        Some(match parts.as_slice() {
            ["preview", path] => {
                let p = Path::new(path);
                let mut head = [0u8; 8192];
                let n = std::fs::File::open(p).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).ok()?;
                let ctx = crate::app::CTX.lock().unwrap().clone()?;
                Box::new(crate::tabs::preview::PreviewTab::open(&ctx, p, crate::tabs::preview::detect(p, &head[..n])).ok()?)
            }
            ["sql", path] => Box::new(SqlTab::open_file(Path::new(path), "", self.project.as_deref()).ok()?),
            ["scratch", id, db] => Box::new(SqlTab::restore_scratch(id.parse().ok()?, db.to_string())?),
            ["er", db] => Box::new(ErTab::new(db.to_string())),
            ["data", db, t] => Box::new(DataTab::new(db.to_string(), t.to_string())),
            ["struct", db, t] => Box::new(StructureTab::new(db.to_string(), t.to_string())),
            ["builder", db] => Box::new(BuilderTab::new(Some(db.to_string()).filter(|d| !d.is_empty()))),
            ["builder", id, db] => Box::new(BuilderTab::restore(id, db)),
            ["safety"] => Box::new(SafetyTab::new()),
            ["users"] => Box::new(crate::tabs::users::UsersTab::new()),
            ["import"] => Box::new(crate::tabs::import::ImportTab::new()),
            ["log"] => Box::new(LogTab::new()),
            ["settings"] => Box::new(SettingsTab::new()),
            ["help"] => Box::new(HelpTab::new()),
            _ => return None,
        })
    }

    pub(super) fn restore_session(&mut self, session: Option<Session>) {
        let mut restored_scratch = Vec::new();
        if let Some(s) = &session {
            self.side_view = SideView::from_name(&s.side_view);
            self.sidebar = s.sidebar;
            let tabs: &[String] = if self.settings.restore_tabs { &s.tabs } else { &[] };
            for t in tabs {
                if let Some(id) = t.strip_prefix("scratch\t").and_then(|r| r.split('\t').next()).and_then(|i| i.parse::<usize>().ok()) {
                    restored_scratch.push(id);
                }
                if let Some(tab) = self.tab_from_session(t) {
                    self.tabs.push(tab);
                }
            }
            self.active = if self.settings.restore_tabs { s.active.min(self.tabs.len().saturating_sub(1)) } else { 0 };
        }
        // Unbenannte Abfragen, die in keiner Sitzung stehen (z. B. nach Absturz), nicht verlieren
        if let Ok(rd) = std::fs::read_dir(workspace::scratch_dir()) {
            let mut ids: Vec<usize> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.strip_prefix("unbenannt-")?.strip_suffix(".sql")?.parse().ok()
                })
                .filter(|id| !restored_scratch.contains(id))
                .collect();
            ids.sort();
            for id in ids {
                if let Some(t) = SqlTab::restore_scratch(id, String::new()) {
                    if !t.text.trim().is_empty() {
                        self.tabs.push(Box::new(t));
                    } else {
                        let _ = std::fs::remove_file(workspace::scratch_path(id));
                    }
                }
            }
        }
    }

    pub(super) fn save_session(&mut self) {
        self.session_saved = Instant::now();
        let s = Session {
            project: self.project.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            tabs: self.tabs.iter().filter_map(|t| t.session()).collect(),
            active: self.active,
            side_view: self.side_view.name().into(),
            sidebar: self.sidebar,
        };
        if self.saved_session.as_ref() != Some(&s) {
            s.save();
            self.saved_session = Some(s);
        }
    }

    /// Alle offenen Dateien speichern und Sitzung sichern.
    pub(super) fn save_all(&mut self) {
        for t in self.tabs.iter_mut() {
            t.save_now_quiet();
        }
        self.save_session();
    }

    // ------------------------------------------------------------------
    // Menue

    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let mut cmd: Option<Cmd> = None;
        let item = |ui: &mut egui::Ui, text: &str, c: Cmd, cmd: &mut Option<Cmd>| {
            if ui.add(egui::Button::new(text).shortcut_text(keymap::text(c))).clicked() {
                *cmd = Some(c);
                ui.close();
            }
        };
        ui.horizontal(|ui| {
            ui.set_min_height(34.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding = vec2(11.0, 6.0);
            // Zeichen und Name des Programms
            {
                let pal = style::pal();
                let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                ui.painter().rect_filled(r, 8.0, pal.accent);
                icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), Icon::Database, egui::Color32::WHITE);
                ui.add_space(6.0);
                ui.label(egui::RichText::new("EasyMySQL").strong().color(pal.text));
                ui.add_space(14.0);
            }
            egui::MenuBar::new().ui(ui, |ui| {
                ui.spacing_mut().button_padding = vec2(12.0, 6.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.menu_button(crate::i18n::text("Datei"), |ui| {
                    item(ui, crate::i18n::text("Neue Abfrage"), Cmd::NewQuery, &mut cmd);
                    item(ui, crate::i18n::text("Datei öffnen …"), Cmd::OpenFile, &mut cmd);
                    item(ui, crate::i18n::text("Ordner öffnen …"), Cmd::OpenFolder, &mut cmd);
                    ui.menu_button(crate::i18n::text("Zuletzt geöffnet"), |ui| {
                        for p in self.settings.recent.clone() {
                            if ui.button(workspace::dir_name(&p)).on_hover_text(p.display().to_string()).clicked() {
                                self.switch_project(Some(p));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    item(ui, crate::i18n::text("Speichern"), Cmd::Save, &mut cmd);
                    item(ui, crate::i18n::text("Registerkarte schließen"), Cmd::CloseTab, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Einstellungen"), Cmd::Settings, &mut cmd);
                    if ui.button(crate::i18n::text("Beenden")).clicked() {
                        ui.close();
                        self.request_quit(&ctx);
                    }
                });
                ui.menu_button(crate::i18n::text("Ansicht"), |ui| {
                    item(ui, crate::i18n::text("Seitenleiste ein/aus"), Cmd::ToggleSidebar, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Terminal ein/aus"), Cmd::ToggleTerminal, &mut cmd);
                    item(ui, crate::i18n::text("Neues Terminal"), Cmd::NewTerminal, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Editor-Schrift vergrößern"), Cmd::FontLarger, &mut cmd);
                    item(ui, crate::i18n::text("Editor-Schrift verkleinern"), Cmd::FontSmaller, &mut cmd);
                    item(ui, crate::i18n::text("Editor-Schrift zurücksetzen"), Cmd::FontReset, &mut cmd);
                });
                ui.menu_button(crate::i18n::text("Datenbank"), |ui| {
                    let connected = self.db.is_some();
                    if ui.add_enabled(connected, egui::Button::new(crate::i18n::text("Neue Datenbank …"))).clicked() {
                        self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                        ui.close();
                    }
                    if ui.add_enabled(connected, egui::Button::new(crate::i18n::text("Neue Tabelle …"))).clicked() {
                        if let Some(d) = self.need_db() {
                            actions.push(Action::NewTable(d));
                        }
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled_ui(connected, |ui| {
                        item(ui, crate::i18n::text("ER-Diagramm"), Cmd::ErDiagram, &mut cmd);
                        item(ui, crate::i18n::text("Abfrage-Assistent"), Cmd::QueryBuilder, &mut cmd);
                    });
                    ui.separator();
                    if ui.add_enabled(connected, egui::Button::new(crate::i18n::text("Exportieren …"))).clicked() {
                        if let Some(d) = self.need_db() {
                            self.dialogs.push(Dialog::Export { db: d, only: None, with_data: true, format: crate::transfer::export::Format::Sql });
                        }
                        ui.close();
                    }
                    item(ui, crate::i18n::text("Importieren …"), Cmd::Import, &mut cmd);
                    if ui.add_enabled(connected, egui::Button::new(crate::i18n::text("SQL-Datei importieren …"))).clicked() {
                        ui.close();
                        self.open_sql_file(true);
                    }
                });
                ui.menu_button("Server", |ui| {
                    let st = self.server.state();
                    if ui.add_enabled(!st.is_running() && !st.is_busy(), egui::Button::new(crate::i18n::text("Starten"))).clicked() {
                        self.start_server(&ctx);
                        ui.close();
                    }
                    if ui.add_enabled(st == State::Running { own: true }, egui::Button::new(crate::i18n::text("Stoppen"))).clicked() {
                        self.stop_server(&ctx);
                        ui.close();
                    }
                    if ui.add_enabled(!st.is_busy(), egui::Button::new(crate::i18n::text("Neu starten"))).clicked() {
                        self.restart_server(&ctx);
                        ui.close();
                    }
                    ui.separator();
                    item(ui, crate::i18n::text("Server-Log"), Cmd::ServerLog, &mut cmd);
                    item(ui, crate::i18n::text("Sicherungen & Reparatur"), Cmd::Backups, &mut cmd);
                    item(ui, crate::i18n::text("Benutzer & Rechte"), Cmd::Users, &mut cmd);
                    ui.separator();
                    if ui.button(crate::i18n::text("Verbindung …")).clicked() {
                        actions.push(Action::OpenSettings(Some("verbindung".into())));
                        ui.close();
                    }
                });
                ui.menu_button(crate::i18n::text("Hilfe"), |ui| {
                    item(ui, crate::i18n::text("Handbuch"), Cmd::Help, &mut cmd);
                    item(ui, crate::i18n::text("Nach Updates suchen"), Cmd::CheckUpdates, &mut cmd);
                    ui.separator();
                    if ui.button(crate::i18n::text("Über EasyMySQL")).clicked() {
                        self.dialogs.push(Dialog::About);
                        ui.close();
                    }
                });
            });
            // Rechts: Neue Abfrage, Werkzeuge und der Serverzustand
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let st = self.server.state();
                let pal = style::pal();
                let color = if st.is_running() {
                    pal.ok_text
                } else if matches!(st, State::Failed(_) | State::NotFound) {
                    pal.error_text
                } else {
                    pal.warn_text
                };
                // Zustandsanzeige: Punkt und kurzer Text
                let text = match &self.db {
                    Some(d) => d.info.label(),
                    None => st.text().to_string(),
                };
                let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(12.5), pal.text_weak);
                let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 36.0, 28.0), Sense::click());
                ui.painter().rect(r, 14.0, if resp.hovered() { pal.hover } else { pal.face_light }, egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
                ui.painter().circle_filled(pos2(r.left() + 15.0, r.center().y), 4.0, color);
                ui.painter().galley(pos2(r.left() + 26.0, r.center().y - galley.size().y / 2.0), galley, pal.text_weak);
                let tip = match &self.db {
                    Some(d) => format!("Server {} – {} (MariaDB {})", st.text(), d.info.label(), d.version.split('-').next().unwrap_or("")),
                    None => format!("Server {}", st.text()),
                };
                if resp.on_hover_text(tip).clicked() {
                    cmd = Some(Cmd::ServerLog);
                }
                ui.add_space(6.0);
                for (icon, c) in [(Icon::Wizard, Cmd::QueryBuilder), (Icon::Diagram, Cmd::ErDiagram)] {
                    let tip = match keymap::text(c) {
                        k if k.is_empty() => c.label().to_string(),
                        k => format!("{} ({k})", c.label()),
                    };
                    if icons::button(ui, icon, &tip).clicked() {
                        cmd = Some(c);
                    }
                }
                ui.add_space(6.0);
                let tip = match keymap::text(Cmd::NewQuery) {
                    k if k.is_empty() => Cmd::NewQuery.label().to_string(),
                    k => format!("{} ({k})", Cmd::NewQuery.label()),
                };
                if icons::primary_button(ui, Icon::Plus, crate::i18n::tr("Abfrage", "Query")).on_hover_text(tip).clicked() {
                    cmd = Some(Cmd::NewQuery);
                }
            });
        });
        if let Some(c) = cmd {
            self.run_cmd(&ctx, c);
        }
    }

    // ------------------------------------------------------------------
    // Aktivitaetsleiste (nur Seitenleisten)

    pub(super) fn activity_bar(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        ui.add_space(6.0);
        for (icon, view, cmd) in [
            (Icon::Files, SideView::Explorer, Cmd::ShowExplorer),
            (Icon::Search, SideView::Search, Cmd::ShowSearch),
            (Icon::Database, SideView::Databases, Cmd::ShowDatabases),
            (Icon::History, SideView::History, Cmd::ShowHistory),
        ] {
            let (slot, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
            let r = Rect::from_center_size(slot.center(), vec2(40.0, 38.0));
            let active = self.sidebar && self.side_view == view;
            if active {
                ui.painter().rect_filled(r, style::RADIUS as f32 + 2.0, pal.selection.gamma_multiply(if pal.dark { 0.75 } else { 0.9 }));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, style::RADIUS as f32 + 2.0, pal.hover);
            }
            let color = if active { pal.activity_active } else if resp.hovered() { pal.text } else { pal.activity_fg };
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(22.0, 22.0)), icon, color);
            let k = keymap::text(cmd);
            let tip = if k.is_empty() { cmd.label().to_string() } else { format!("{} ({k})", cmd.label()) };
            if resp.on_hover_text(tip).clicked() {
                if active {
                    self.sidebar = false;
                } else {
                    self.show_side(view);
                    if view == SideView::Search {
                        self.search.focus = true;
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Registerkarten

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        const BAR_H: f32 = 42.0;
        const PILL_H: f32 = 30.0;
        const GAP: f32 = 4.0;
        const MAX_W: f32 = 216.0;
        const ACTIVE_W: f32 = 190.0;
        const MIN_W: f32 = 132.0;
        const ICON_W: f32 = 42.0;
        const LIST_W: f32 = 34.0;
        let pal = style::pal();
        let font = egui::FontId::proportional(13.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR_H), Sense::hover());
        block_autoscroll(bar);
        let n = self.tabs.len();
        let looks: Vec<TabLook> = self.tabs.iter().map(|t| TabLook { title: t.title(), icon: t.icon(), busy: t.busy(), modified: t.modified() }).collect();
        // Wunschbreite je Reiter. Passen nicht alle, schrumpfen zuerst die langen Titel, dann die
        // inaktiven Reiter bis auf ihr Symbol; der aktive behaelt seinen Titel.
        let desired: Vec<f32> = looks
            .iter()
            .map(|l| (ui.painter().layout_no_wrap(l.title.clone(), font.clone(), pal.text).size().x + 52.0 + 22.0).clamp(90.0, MAX_W))
            .collect();
        let gaps = GAP * n.saturating_sub(1) as f32;
        let avail = bar.width() - 4.0;
        let active_i = self.active.min(n.saturating_sub(1));
        let fit = |avail: f32| -> Vec<f32> {
            if desired.iter().sum::<f32>() + gaps <= avail {
                return desired.clone();
            }
            // Wasserstandsverfahren: gemeinsame Obergrenze der Breite, passend zum Platz
            let budget = (avail - gaps).max(0.0);
            let level = |floor: f32| -> Vec<f32> {
                let (mut lo, mut hi) = (floor, MAX_W);
                for _ in 0..24 {
                    let mid = (lo + hi) / 2.0;
                    let sum: f32 = desired.iter().enumerate().map(|(i, d)| if i == active_i { d.min(ACTIVE_W) } else { d.min(mid).max(floor.min(*d)) }).sum();
                    if sum > budget { hi = mid } else { lo = mid }
                }
                desired.iter().enumerate().map(|(i, d)| if i == active_i { d.min(ACTIVE_W) } else { d.min(lo).max(floor.min(*d)) }).collect()
            };
            let readable = level(MIN_W);
            if readable.iter().sum::<f32>() + gaps <= avail + 0.5 {
                return readable;
            }
            desired.iter().enumerate().map(|(i, d)| if i == active_i { d.min(ACTIVE_W) } else { ICON_W }).collect()
        };
        let mut widths = fit(avail);
        let scroll = widths.iter().sum::<f32>() + gaps > avail + 0.5;
        if scroll {
            widths = fit(avail - LIST_W);
        }
        let tabs_rect = Rect::from_min_max(bar.min + vec2(2.0, 0.0), bar.max - vec2(if scroll { LIST_W + 2.0 } else { 2.0 }, 0.0));

        let mut close = None;
        let mut close_others = None;
        let mut close_right = None;
        let mut close_all = false;
        let mut activate = None;
        let mut rects: Vec<Rect> = Vec::with_capacity(n);
        let mut dragging: Option<(usize, f32)> = None;
        let mut dropped: Option<(usize, f32)> = None;
        let active = self.active;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        egui::ScrollArea::horizontal().id_salt("tabbar").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let time = ui.input(|i| i.time) as f32;
            for (i, look) in looks.iter().enumerate() {
                let sel = i == active;
                let (r, resp) = ui.allocate_exact_size(vec2(widths[i], PILL_H), Sense::click_and_drag());
                rects.push(r);
                if sel {
                    resp.scroll_to_me(None);
                }
                if resp.dragged() {
                    if let Some(p) = ui.ctx().pointer_latest_pos() {
                        dragging = Some((i, p.x));
                    }
                    continue;
                }
                if resp.drag_stopped()
                    && let Some(p) = ui.ctx().pointer_latest_pos()
                {
                    dropped = Some((i, p.x));
                }
                let over_close = r.width() >= 100.0 && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| tab_close_rect(r).contains(p));
                paint_tab(ui.painter(), r, look, sel, resp.hovered(), over_close, time, &font);
                if look.busy {
                    ui.ctx().request_repaint();
                }
                if resp.clicked() {
                    if over_close {
                        close = Some(i);
                    } else {
                        activate = Some(i);
                    }
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                let cut = widths[i] < 100.0 || widths[i] + 1.0 < desired[i];
                let resp = if cut { resp.on_hover_text_at_pointer(&look.title) } else { resp };
                resp.context_menu(|ui| {
                    if ui.button(crate::i18n::text("Schließen")).clicked() {
                        close = Some(i);
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Andere schließen")).clicked() {
                        close_others = Some(i);
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Rechts davon schließen")).clicked() {
                        close_right = Some(i);
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Alle Registerkarten schließen")).clicked() {
                        close_all = true;
                        ui.close();
                    }
                    if let Some(f) = self_file(&self.tabs, i) {
                        ui.separator();
                        if ui.button(crate::i18n::text("Im Datei-Explorer zeigen")).clicked() {
                            if let Some(d) = f.parent() {
                                open_path(d);
                            }
                            ui.close();
                        }
                        if ui.button(crate::i18n::text("Pfad kopieren")).clicked() {
                            ui.ctx().copy_text(f.display().to_string());
                            ui.close();
                        }
                    }
                });
            }
        });
        // Gezogener Reiter schwebt unter dem Mauszeiger; beim Loslassen wird er dort einsortiert
        if let Some((i, x)) = dragging {
            let w = widths[i];
            let r = Rect::from_center_size(pos2(x.clamp(tabs_rect.left() + w / 2.0, tabs_rect.right() - w / 2.0), tabs_rect.center().y), vec2(w, PILL_H));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab-drag")));
            painter.rect_filled(r.translate(vec2(0.0, 2.0)), style::RADIUS as f32 + 1.0, egui::Color32::from_black_alpha(60));
            paint_tab(&painter, r, &looks[i], true, false, false, 0.0, &font);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            self.active = i;
        }
        if let Some((i, x)) = dropped {
            let to = rects.iter().enumerate().filter(|(k, r)| *k != i && r.center().x < x).count();
            if to != i {
                let t = self.tabs.remove(i);
                self.tabs.insert(to, t);
                self.active = to;
            }
        }
        if scroll {
            let r = Rect::from_center_size(pos2(bar.right() - LIST_W / 2.0 - 2.0, bar.center().y), vec2(28.0, 28.0));
            let resp = ui.interact(r, egui::Id::new("tab-list"), Sense::click()).on_hover_text(crate::i18n::text("Alle Registerkarten"));
            if resp.hovered() {
                ui.painter().rect_filled(r, style::RADIUS as f32, pal.hover);
            }
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(14.0, 14.0)), Icon::ChevronDown, pal.text_weak);
            egui::Popup::menu(&resp).show(|ui| {
                for (i, l) in looks.iter().enumerate() {
                    if ui.add(egui::Button::new(&l.title).selected(i == active).min_size(vec2(180.0, 0.0))).clicked() {
                        activate = Some(i);
                        ui.close();
                    }
                }
            });
        }
        if close_all {
            self.run_cmd(&ui.ctx().clone(), Cmd::CloseAllTabs);
            return;
        }
        if let Some(i) = activate {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
        if let Some(i) = close_others {
            self.close_tabs_where(|k| k == i);
        }
        if let Some(i) = close_right {
            self.close_tabs_where(|k| k <= i);
        }
    }

    /// Keine Registerkarte offen: leere Flaeche mit den wichtigsten Kuerzeln (wie in VS Code)
    pub(super) fn empty_area(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click());
        let c = rect.center() - vec2(0.0, 40.0);
        let logo = Rect::from_center_size(c - vec2(0.0, 30.0), vec2(90.0, 90.0));
        icons::paint(ui.painter(), logo, Icon::Database, pal.text_weak.gamma_multiply(0.25));
        let mut y = c.y + 40.0;
        for cmd in [Cmd::NewQuery, Cmd::OpenFolder, Cmd::QuickOpen, Cmd::CommandPalette] {
            let k = keymap::text(cmd);
            if k.is_empty() {
                continue;
            }
            ui.painter().text(pos2(c.x - 8.0, y), egui::Align2::RIGHT_CENTER, cmd.label(), egui::FontId::proportional(13.0), pal.text_weak);
            ui.painter().text(pos2(c.x + 8.0, y), egui::Align2::LEFT_CENTER, k, egui::FontId::monospace(12.5), pal.text_weak);
            y += 22.0;
        }
        if resp.double_clicked() {
            self.run_cmd(&ui.ctx().clone(), Cmd::NewQuery);
        }
    }

    // ------------------------------------------------------------------
    // Meldungen unten rechts

    pub(super) fn toasts_ui(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_millis(if t.error { 8000 } else { 3500 }));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        let pal = style::pal();
        let screen = ctx.content_rect();
        let mut remove = None;
        let mut y = screen.bottom() - 12.0;
        for (i, t) in self.toasts.iter().enumerate().rev() {
            let galley = ctx.fonts_mut(|f| f.layout(t.text.clone(), egui::FontId::proportional(13.0), pal.text, 380.0));
            let size = galley.size() + vec2(28.0 + 22.0, 16.0);
            let r = Rect::from_min_size(pos2(screen.right() - 12.0 - size.x, y - size.y), size);
            let area = egui::Area::new(egui::Id::new(("toast", i, &t.text))).order(egui::Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
                let (rr, resp) = ui.allocate_exact_size(size, Sense::click());
                ui.painter().rect(rr, 5.0, pal.face_light, Stroke::new(1.0, if t.error { pal.error_text } else { pal.border }), egui::StrokeKind::Inside);
                ui.painter().rect_filled(Rect::from_min_size(rr.min, vec2(3.0, rr.height())), 0.0, if t.error { pal.error_text } else { pal.accent });
                let (icon, tint) = if t.error { (Icon::Alert, pal.error_text) } else { (Icon::Check, pal.accent) };
                icons::paint(ui.painter(), Rect::from_center_size(pos2(rr.left() + 22.0, rr.center().y), vec2(16.0, 16.0)), icon, tint);
                ui.painter().galley(rr.min + vec2(36.0, 8.0), galley, pal.text);
                resp
            });
            if area.inner.clicked() {
                remove = Some(i);
            }
            y -= size.y + 8.0;
        }
        if let Some(i) = remove {
            self.toasts.remove(i);
        }
    }
}

/// Was von einer Registerkarte zum Zeichnen gebraucht wird
struct TabLook {
    title: String,
    icon: Option<Icon>,
    busy: bool,
    modified: bool,
}

fn self_file(tabs: &[Box<dyn TabView>], i: usize) -> Option<PathBuf> {
    tabs.get(i).and_then(|t| t.file().map(|p| p.to_path_buf()))
}

/// Klickflaeche des Schliessen-Kreuzes (rechts im Reiter)
fn tab_close_rect(r: Rect) -> Rect {
    Rect::from_center_size(pos2(r.right() - 17.0, r.center().y), vec2(20.0, 20.0))
}

/// Reiter als abgerundete Kapsel: der aktive hebt sich ab, die anderen sind nur Text
#[allow(clippy::too_many_arguments)]
fn paint_tab(p: &egui::Painter, r: Rect, look: &TabLook, sel: bool, hovered: bool, over_close: bool, time: f32, font: &egui::FontId) {
    let pal = style::pal();
    let radius = style::RADIUS as f32 + 1.0;
    if sel {
        p.rect(r, radius, pal.bg, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
    } else if hovered {
        p.rect_filled(r, radius, pal.hover);
    }
    let color = if sel { pal.text } else { pal.text_weak };
    let icon = look.icon.unwrap_or(Icon::File);
    // Zu schmal fuer den Titel: nur das Symbol (Titel im Tooltip)
    if r.width() < 100.0 {
        let c = if sel { pal.accent } else if hovered { pal.text } else { pal.text_weak };
        icons::paint(p, Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, c);
        if look.busy {
            p.circle_filled(r.right_top() + vec2(-8.0, 8.0), 3.0, pal.accent);
        } else if look.modified {
            p.circle_filled(r.right_top() + vec2(-8.0, 8.0), 3.0, pal.text_weak);
        }
        return;
    }
    let mut x = r.left() + 12.0;
    icons::paint(p, Rect::from_center_size(pos2(x + 7.0, r.center().y), vec2(14.0, 14.0)), icon, if sel { pal.accent } else { pal.text_weak });
    x += 22.0;
    let max_w = (r.right() - 30.0 - x).max(10.0);
    let mut job = egui::text::LayoutJob::simple_singleline(look.title.clone(), font.clone(), color);
    job.wrap = egui::text::TextWrapping { max_width: max_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = p.layout_job(job);
    p.galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, color);
    // Kreuz bzw. Laufanzeige bzw. Punkt fuer Ungespeichertes
    let cr = tab_close_rect(r);
    if look.busy {
        let a = time * 6.0;
        p.add(egui::Shape::line(
            (0..=10).map(|k| cr.center() + vec2((a + k as f32 * 0.45).cos(), (a + k as f32 * 0.45).sin()) * 5.0).collect(),
            Stroke::new(1.5, pal.accent),
        ));
    } else if look.modified && !(hovered && over_close) {
        p.circle_filled(cr.center(), 3.5, color);
    } else if sel || hovered {
        if over_close {
            p.rect_filled(cr, style::RADIUS as f32 - 1.0, pal.hover.gamma_multiply(1.6));
        }
        icons::paint(p, cr.shrink(4.0), Icon::Close, if over_close { pal.text } else { color });
    }
}
