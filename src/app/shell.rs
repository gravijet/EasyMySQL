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
            ["sql", path] => Box::new(SqlTab::open_file(Path::new(path), "", self.project.as_deref()).ok()?),
            ["scratch", id, db] => Box::new(SqlTab::restore_scratch(id.parse().ok()?, db.to_string())?),
            ["er", db] => Box::new(ErTab::new(db.to_string())),
            ["data", db, t] => Box::new(DataTab::new(db.to_string(), t.to_string())),
            ["struct", db, t] => Box::new(StructureTab::new(db.to_string(), t.to_string())),
            ["builder", db] => Box::new(BuilderTab::new(Some(db.to_string()).filter(|d| !d.is_empty()))),
            ["builder", id, db] => Box::new(BuilderTab::restore(id, db)),
            ["safety"] => Box::new(SafetyTab::new()),
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
            for t in &s.tabs {
                if let Some(id) = t.strip_prefix("scratch\t").and_then(|r| r.split('\t').next()).and_then(|i| i.parse::<usize>().ok()) {
                    restored_scratch.push(id);
                }
                if let Some(tab) = self.tab_from_session(t) {
                    self.tabs.push(tab);
                }
            }
            self.active = s.active.min(self.tabs.len().saturating_sub(1));
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
        s.save();
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
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("Datei", |ui| {
                    item(ui, "Neue Abfrage", Cmd::NewQuery, &mut cmd);
                    ui.add_enabled_ui(self.project.is_some(), |ui| item(ui, "Neue Datei im Projekt …", Cmd::NewFile, &mut cmd));
                    item(ui, "Datei öffnen …", Cmd::OpenFile, &mut cmd);
                    ui.separator();
                    item(ui, "Ordner öffnen …", Cmd::OpenFolder, &mut cmd);
                    if ui.button("Neues Projekt …").clicked() {
                        self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                        ui.close();
                    }
                    ui.menu_button("Zuletzt geöffnet", |ui| {
                        for p in self.settings.recent.clone() {
                            if ui.button(workspace::dir_name(&p)).on_hover_text(p.display().to_string()).clicked() {
                                self.switch_project(Some(p));
                                ui.close();
                            }
                        }
                    });
                    if self.project.is_some() && ui.button("Ordner schließen").clicked() {
                        self.switch_project(None);
                        ui.close();
                    }
                    ui.separator();
                    item(ui, "Speichern", Cmd::Save, &mut cmd);
                    item(ui, "Alle speichern", Cmd::SaveAll, &mut cmd);
                    item(ui, "Registerkarte schließen", Cmd::CloseTab, &mut cmd);
                    item(ui, "Alle Registerkarten schließen", Cmd::CloseAllTabs, &mut cmd);
                    ui.separator();
                    item(ui, "Einstellungen", Cmd::Settings, &mut cmd);
                    ui.separator();
                    if self.tray.is_some() && ui.button("Fenster ausblenden").clicked() {
                        ui.close();
                        if let Some(h) = self.hwnd {
                            platform::hide_window(h);
                        }
                    }
                    if ui.button("Beenden").clicked() {
                        ui.close();
                        self.quit();
                    }
                });
                ui.menu_button("Bearbeiten", |ui| {
                    item(ui, "Suchen", Cmd::Find, &mut cmd);
                    item(ui, "Ersetzen", Cmd::Replace, &mut cmd);
                    item(ui, "Gehe zu Zeile", Cmd::GotoLine, &mut cmd);
                    ui.separator();
                    item(ui, "SQL formatieren", Cmd::Format, &mut cmd);
                    ui.separator();
                    item(ui, "Suchen in Dateien", Cmd::ShowSearch, &mut cmd);
                });
                ui.menu_button("Ansicht", |ui| {
                    item(ui, "Befehle …", Cmd::CommandPalette, &mut cmd);
                    item(ui, "Datei im Projekt öffnen …", Cmd::QuickOpen, &mut cmd);
                    ui.separator();
                    item(ui, "Explorer", Cmd::ShowExplorer, &mut cmd);
                    item(ui, "Suchen", Cmd::ShowSearch, &mut cmd);
                    item(ui, "Datenbanken", Cmd::ShowDatabases, &mut cmd);
                    item(ui, "Verlauf", Cmd::ShowHistory, &mut cmd);
                    item(ui, "Seitenleiste ein/aus", Cmd::ToggleSidebar, &mut cmd);
                });
                ui.menu_button("Ausführen", |ui| {
                    item(ui, "Datei oder Markierung ausführen", Cmd::RunAll, &mut cmd);
                    item(ui, "Anweisung am Cursor ausführen", Cmd::RunStatement, &mut cmd);
                    item(ui, "Ausführungsplan (EXPLAIN)", Cmd::Explain, &mut cmd);
                    ui.separator();
                    if ui.add_enabled(self.db.is_some(), egui::Button::new("SQL-Datei ausführen …")).clicked() {
                        ui.close();
                        self.open_sql_file(true);
                    }
                });
                ui.menu_button("Datenbank", |ui| {
                    let connected = self.db.is_some();
                    if ui.add_enabled(connected, egui::Button::new("Neue Datenbank …")).clicked() {
                        self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                        ui.close();
                    }
                    if ui.add_enabled(connected, egui::Button::new("Neue Tabelle …")).clicked() {
                        if let Some(d) = self.need_db() {
                            actions.push(Action::NewTable(d));
                        }
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled_ui(connected, |ui| {
                        item(ui, "ER-Diagramm", Cmd::ErDiagram, &mut cmd);
                        item(ui, "Abfrage-Assistent", Cmd::QueryBuilder, &mut cmd);
                    });
                    ui.separator();
                    if ui.add_enabled(connected, egui::Button::new("Exportieren …")).clicked() {
                        if let Some(d) = self.need_db() {
                            self.dialogs.push(Dialog::Export { db: d, with_data: true });
                        }
                        ui.close();
                    }
                    if ui.add_enabled(connected, egui::Button::new("SQL-Datei importieren …")).clicked() {
                        ui.close();
                        self.open_sql_file(true);
                    }
                    ui.separator();
                    if ui.add_enabled(connected, egui::Button::new("Aktualisieren")).clicked() {
                        actions.push(Action::RefreshAll);
                        ui.close();
                    }
                });
                ui.menu_button("Server", |ui| {
                    let st = self.server.state();
                    if ui.add_enabled(!st.is_running() && !st.is_busy(), egui::Button::new("Starten")).clicked() {
                        self.start_server(&ctx);
                        ui.close();
                    }
                    if ui.add_enabled(st == State::Running { own: true }, egui::Button::new("Stoppen")).clicked() {
                        self.stop_server(&ctx);
                        ui.close();
                    }
                    if ui.add_enabled(!st.is_busy(), egui::Button::new("Neu starten")).clicked() {
                        self.restart_server(&ctx);
                        ui.close();
                    }
                    ui.separator();
                    item(ui, "Server-Log", Cmd::ServerLog, &mut cmd);
                    item(ui, "Sicherungen & Reparatur", Cmd::Backups, &mut cmd);
                    if let Some(p) = &self.server.paths {
                        if ui.button("Datenordner öffnen").clicked() {
                            open_path(&p.base);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Verbindung …").clicked() {
                        actions.push(Action::OpenSettings(Some("verbindung".into())));
                        ui.close();
                    }
                    if ui.button("In VS Code öffnen").clicked() {
                        ui.close();
                        if let Err(e) = crate::vscode::open_workspace(&self.current_db) {
                            self.error(e);
                        }
                    }
                    if ui.add_enabled(self.vscode_job.is_none(), egui::Button::new("VS Code einrichten")).clicked() {
                        ui.close();
                        self.setup_vscode(&ctx);
                    }
                });
                ui.menu_button("Hilfe", |ui| {
                    item(ui, "Handbuch", Cmd::Help, &mut cmd);
                    item(ui, "Nach Updates suchen", Cmd::CheckUpdates, &mut cmd);
                    if ui.button("Tastenkürzel").clicked() {
                        actions.push(Action::OpenSettings(Some("tastenkuerzel".into())));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Über EasyMySQL").clicked() {
                        self.dialogs.push(Dialog::About);
                        ui.close();
                    }
                });
            });
            // Werkzeuge, die eine Registerkarte oeffnen
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let st = self.server.state();
                let pal = style::pal();
                let color = if st.is_running() {
                    pal.ok_text
                } else if matches!(st, State::Failed(_) | State::NotFound) {
                    pal.error_text
                } else {
                    style::pal().syn_function
                };
                let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 3.0, pal.hover);
                }
                ui.painter().circle_filled(r.center(), 4.5, color);
                let tip = match &self.db {
                    Some(d) => format!("Server {} – {} (MariaDB {})", st.text(), d.info.label(), d.version.split('-').next().unwrap_or("")),
                    None => format!("Server {}", st.text()),
                };
                if resp.on_hover_text(tip).clicked() {
                    cmd = Some(Cmd::ServerLog);
                }
                for (icon, c) in [(Icon::Help, Cmd::Help), (Icon::Log, Cmd::ServerLog), (Icon::Backup, Cmd::Backups), (Icon::Wizard, Cmd::QueryBuilder), (Icon::Diagram, Cmd::ErDiagram)] {
                    let tip = match keymap::text(c) {
                        k if k.is_empty() => c.label().to_string(),
                        k => format!("{} ({k})", c.label()),
                    };
                    if icons::button(ui, icon, &tip).clicked() {
                        cmd = Some(c);
                    }
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
        ui.add_space(4.0);
        for (icon, view, cmd) in [
            (Icon::Files, SideView::Explorer, Cmd::ShowExplorer),
            (Icon::Search, SideView::Search, Cmd::ShowSearch),
            (Icon::Database, SideView::Databases, Cmd::ShowDatabases),
            (Icon::History, SideView::History, Cmd::ShowHistory),
        ] {
            let (r, resp) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
            let active = self.sidebar && self.side_view == view;
            if active {
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 0.0, pal.activity_active);
            }
            let color = if active || resp.hovered() { pal.activity_active } else { pal.activity_fg };
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(24.0, 24.0)), icon, color);
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
        let pal = style::pal();
        let mut close = None;
        let mut close_others = None;
        let mut close_right = None;
        let mut close_all = false;
        let mut activate = None;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        block_autoscroll(bar);
        ui.painter().rect_filled(bar, 0.0, pal.tab_bar_bg);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        egui::ScrollArea::horizontal().id_salt("tabbar").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, t) in self.tabs.iter().enumerate() {
                let sel = i == self.active;
                let title = t.title();
                let galley = ui.painter().layout_no_wrap(title.clone(), egui::FontId::proportional(13.0), pal.text);
                let w = galley.size().x + 44.0;
                let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                if sel {
                    resp.scroll_to_me(None);
                }
                let hovered = resp.hovered();
                ui.painter().rect_filled(r, 0.0, if sel { pal.tab_active } else { pal.tab_inactive });
                if sel {
                    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0.0, pal.accent);
                }
                ui.painter().vline(r.right(), r.y_range(), Stroke::new(1.0, pal.tab_bar_bg));
                let color = if sel { pal.text } else { pal.text_weak };
                ui.painter().text(r.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, &title, egui::FontId::proportional(13.0), color);
                // Schliessen-Kreuz bzw. Laufanzeige
                let cr = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0));
                let over_close = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| cr.contains(p));
                if t.busy() {
                    let a = ui.input(|i| i.time) as f32 * 6.0;
                    ui.painter().add(egui::Shape::line(
                        (0..=10).map(|k| cr.center() + vec2((a + k as f32 * 0.45).cos(), (a + k as f32 * 0.45).sin()) * 5.0).collect(),
                        Stroke::new(1.5, pal.accent),
                    ));
                    ui.ctx().request_repaint();
                } else if sel || hovered {
                    if over_close {
                        ui.painter().rect_filled(cr, 3.0, pal.hover);
                    }
                    icons::paint(ui.painter(), cr, Icon::Close, color);
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
                resp.context_menu(|ui| {
                    if ui.button("Schließen").clicked() {
                        close = Some(i);
                        ui.close();
                    }
                    if ui.button("Andere schließen").clicked() {
                        close_others = Some(i);
                        ui.close();
                    }
                    if ui.button("Rechts davon schließen").clicked() {
                        close_right = Some(i);
                        ui.close();
                    }
                    if ui.button("Alle Registerkarten schließen").clicked() {
                        close_all = true;
                        ui.close();
                    }
                    if let Some(f) = t.file() {
                        ui.separator();
                        if ui.button("Im Datei-Explorer zeigen").clicked() {
                            if let Some(d) = f.parent() {
                                open_path(d);
                            }
                            ui.close();
                        }
                        if ui.button("Pfad kopieren").clicked() {
                            ui.ctx().copy_text(f.display().to_string());
                            ui.close();
                        }
                    }
                });
            }
        });
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
            let size = galley.size() + vec2(28.0, 16.0);
            let r = Rect::from_min_size(pos2(screen.right() - 12.0 - size.x, y - size.y), size);
            let area = egui::Area::new(egui::Id::new(("toast", i, &t.text))).order(egui::Order::Foreground).fixed_pos(r.min).show(ctx, |ui| {
                let (rr, resp) = ui.allocate_exact_size(size, Sense::click());
                ui.painter().rect(rr, 4.0, pal.face_light, Stroke::new(1.0, if t.error { pal.error_text } else { pal.border }), egui::StrokeKind::Inside);
                ui.painter().rect_filled(Rect::from_min_size(rr.min, vec2(3.0, rr.height())), 0.0, if t.error { pal.error_text } else { pal.accent });
                ui.painter().galley(rr.min + vec2(14.0, 8.0), galley, pal.text);
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
