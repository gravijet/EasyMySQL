// Rahmen des Hauptfensters: Menue, Aktivitaetsleiste, Registerkarten, leere Flaeche, Meldungen, Sitzung.

use crate::sperre::Sperre;
use super::*;
use crate::icons::{self, Icon};
use crate::workspace::{self, Session};
use eframe::egui::{Sense, Stroke, pos2, vec2};

impl EasyApp {
    // ------------------------------------------------------------------
    // Sitzung

    pub(super) fn tab_from_session(&self, entry: &str) -> Option<Box<dyn TabView>> {
        let mut parts: Vec<&str> = entry.split('\t').collect();
        // Am Ende steht der Cursor ("@1234"), damit die Datei an derselben Stelle aufgeht
        let cursor = parts.last().and_then(|p| p.strip_prefix('@')).and_then(|c| c.parse::<usize>().ok());
        if cursor.is_some() {
            parts.pop();
        }
        let mut tab: Box<dyn TabView> = match parts.as_slice() {
            ["preview", path] => {
                let p = Path::new(path);
                let mut head = [0u8; 8192];
                let n = std::fs::File::open(p).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).ok()?;
                let ctx = crate::app::CTX.lock_ok().clone()?;
                Box::new(crate::tabs::preview::PreviewTab::open(&ctx, p, crate::tabs::preview::detect(p, &head[..n])).ok()?)
            }
            ["sql", path] => Box::new(SqlTab::open_file(Path::new(path), "", self.project.as_deref()).ok()?),
            ["code", path] => Box::new(crate::tabs::code::CodeTab::open_file(Path::new(path), "", self.project.as_deref()).ok()?),
            ["scratch", id, db] => Box::new(SqlTab::restore_scratch(id.parse().ok()?, db.to_string())?),
            ["er", db] => Box::new(ErTab::new(db.to_string())),
            ["data", db, t] => Box::new(DataTab::new(db.to_string(), t.to_string())),
            ["struct", db, t] => Box::new(StructureTab::new(db.to_string(), t.to_string())),
            ["builder", db] => Box::new(BuilderTab::new(Some(db.to_string()).filter(|d| !d.is_empty()))),
            ["builder", id, db] => Box::new(BuilderTab::restore(id, db)),
            ["safety"] => Box::new(SafetyTab::new()),
            ["users"] => Box::new(crate::tabs::users::UsersTab::new()),
            ["import"] => Box::new(crate::tabs::transfer::TransferTab::new(crate::tabs::transfer::Mode::Import)),
            ["export"] => Box::new(crate::tabs::transfer::TransferTab::new(crate::tabs::transfer::Mode::Export)),
            ["log"] => Box::new(LogTab::new()),
            ["settings"] => Box::new(SettingsTab::new()),
            ["help"] => Box::new(HelpTab::new()),
            _ => return None,
        };
        if let Some(c) = cursor {
            tab.restore_cursor(c);
        }
        Some(tab)
    }

    pub(super) fn restore_session(&mut self, session: Option<Session>) {
        let mut restored_scratch = Vec::new();
        if let Some(s) = &session {
            self.side_view = SideView::from_name(&s.side_view);
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
            tabs: self.tabs.iter().filter_map(|t| t.session().map(|s| match t.cursor() {
                Some(c) if c > 0 => format!("{s}\t@{c}"),
                _ => s,
            })).collect(),
            active: self.active,
            side_view: self.side_view.name().into(),
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

    /// Eintraege fuer den Server (im Menue "Server" und im Verbindungsknopf der Leiste)
    fn server_items(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, actions: &mut Vec<Action>, cmd: &mut Option<Cmd>) {
        let st = self.server.state();
        let pal = style::pal();
        if ui.add_enabled_ui(!st.is_running() && !st.is_busy(), |ui| icons::menu_item_colored(ui, Some((Icon::Play, Some(pal.ok_text))), crate::i18n::text("Starten"), "")).inner.clicked() {
            self.start_server(ctx);
            ui.close();
        }
        if ui.add_enabled_ui(st == State::Running { own: true }, |ui| icons::menu_item_colored(ui, Some((Icon::Stop, Some(pal.error_text))), crate::i18n::text("Stoppen"), "")).inner.clicked() {
            self.stop_server(ctx);
            ui.close();
        }
        if ui.add_enabled_ui(!st.is_busy(), |ui| icons::menu_item(ui, Some(Icon::RotateCcw), crate::i18n::text("Neu starten"), "")).inner.clicked() {
            self.restart_server(ctx);
            ui.close();
        }
        ui.separator();
        for c in [Cmd::ServerLog, Cmd::Backups, Cmd::Users] {
            if icons::menu_item(ui, cmd_icon(c), c.label(), &keymap::text(c)).clicked() {
                *cmd = Some(c);
                ui.close();
            }
        }
        ui.separator();
        if icons::menu_item(ui, Some(Icon::Link), crate::i18n::text("Verbindung …"), "").clicked() {
            actions.push(Action::OpenSettings(Some("verbindung".into())));
            ui.close();
        }
    }

    /// Obere Leiste wie die Hauptwerkzeugleiste von IntelliJ IDEA: Menues, Projekt und Branch links,
    /// Suche in der Mitte, Ausfuehren, Werkzeuge, Verbindung und Fensterknoepfe rechts.
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let pal = style::pal();
        let mut cmd: Option<Cmd> = None;
        let item = |ui: &mut egui::Ui, text: &str, c: Cmd, cmd: &mut Option<Cmd>| {
            if icons::menu_item(ui, cmd_icon(c), text, &keymap::text(c)).clicked() {
                *cmd = Some(c);
                ui.close();
            }
        };
        let tip_of = |c: Cmd| match keymap::text(c) {
            k if k.is_empty() => c.label().to_string(),
            k => format!("{} ({k})", c.label()),
        };
        let bar_w = ui.available_width();
        let bar = Rect::from_min_size(ui.next_widget_position(), vec2(bar_w, titlebar::HEIGHT));
        titlebar::drag_area(ui, bar);
        // Duenne senkrechte Trennlinie zwischen Gruppen
        let divider = |ui: &mut egui::Ui| {
            let (r, _) = ui.allocate_exact_size(vec2(9.0, 18.0), Sense::hover());
            ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, pal.chrome_weak.gamma_multiply(0.35)));
        };
        let mut left_end = bar.left();
        let mut right_start = bar.right();
        egui::MenuBar::new().ui(ui, |ui| {
                // Volle Leistenhoehe von Anfang an: alles wird auf dieselbe Mittellinie gesetzt
                ui.set_min_height(titlebar::HEIGHT);
                ui.spacing_mut().item_spacing.x = 2.0;
                // Zeichen des Programms
                // Menues in den Farben der Leiste (Schrift, Flaeche unter der Maus und bei offenem Menue)
                {
                    let v = ui.visuals_mut();
                    v.override_text_color = Some(pal.chrome_text);
                    for w in [&mut v.widgets.hovered, &mut v.widgets.open, &mut v.widgets.active] {
                        w.weak_bg_fill = pal.chrome_hover;
                        w.bg_fill = pal.chrome_hover;
                        w.bg_stroke = Stroke::NONE;
                        w.fg_stroke.color = pal.chrome_text;
                    }
                }
                // Zeichen des Programms: Etikett eines Ordnerrueckens
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
                ui.painter().rect_filled(r, 6.0, pal.primary);
                icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(15.0, 15.0)), Icon::Database, pal.on_primary);
                ui.add_space(6.0);
                ui.spacing_mut().button_padding = vec2(8.0, 5.0);
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.menu_button(crate::i18n::text("Datei"), |ui| {
                    style::menu_item_look(ui);
                    item(ui, crate::i18n::text("Neue Abfrage"), Cmd::NewQuery, &mut cmd);
                    item(ui, crate::i18n::text("Neue Datei …"), Cmd::NewFile, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Datei öffnen …"), Cmd::OpenFile, &mut cmd);
                    item(ui, crate::i18n::text("Ordner öffnen …"), Cmd::OpenFolder, &mut cmd);
                    icons::submenu(ui, Some(Icon::History), crate::i18n::text("Zuletzt geöffnet"), |ui| {
                        for p in self.settings.recent.clone().into_iter().filter(|p| p.is_dir()) {
                            if side::recent_row(ui, &p).clicked() {
                                self.switch_project(Some(p));
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    item(ui, crate::i18n::tr("Neues Java-Projekt …", "New Java project …"), Cmd::NewJavaProject, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Speichern"), Cmd::Save, &mut cmd);
                    item(ui, Cmd::SaveAll.label(), Cmd::SaveAll, &mut cmd);
                    item(ui, crate::i18n::text("Registerkarte schließen"), Cmd::CloseTab, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Einstellungen"), Cmd::Settings, &mut cmd);
                    if icons::menu_item(ui, Some(Icon::Power), crate::i18n::text("Beenden"), "").clicked() {
                        ui.close();
                        self.request_quit(&ctx);
                    }
                });
                ui.menu_button(crate::i18n::text("Ansicht"), |ui| {
                    style::menu_item_look(ui);
                    item(ui, Cmd::CommandPalette.label(), Cmd::CommandPalette, &mut cmd);
                    item(ui, Cmd::QuickOpen.label(), Cmd::QuickOpen, &mut cmd);
                    ui.separator();
                    for c in [Cmd::ShowExplorer, Cmd::ShowSearch, Cmd::ShowDatabases, Cmd::ShowHistory, Cmd::ShowGit] {
                        item(ui, c.label(), c, &mut cmd);
                    }
                    ui.separator();
                    item(ui, crate::i18n::text("Neues Terminal"), Cmd::NewTerminal, &mut cmd);
                    ui.separator();
                    item(ui, crate::i18n::text("Editor-Schrift vergrößern"), Cmd::FontLarger, &mut cmd);
                    item(ui, crate::i18n::text("Editor-Schrift verkleinern"), Cmd::FontSmaller, &mut cmd);
                    item(ui, crate::i18n::text("Editor-Schrift zurücksetzen"), Cmd::FontReset, &mut cmd);
                });
                ui.menu_button(crate::i18n::text("Datenbank"), |ui| {
                    style::menu_item_look(ui);
                    let connected = self.db.is_some();
                    if ui.add_enabled_ui(connected, |ui| icons::menu_item(ui, Some(Icon::Database), crate::i18n::text("Neue Datenbank …"), "")).inner.clicked() {
                        self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                        ui.close();
                    }
                    if ui.add_enabled_ui(connected, |ui| icons::menu_item(ui, Some(Icon::Table), crate::i18n::text("Neue Tabelle …"), "")).inner.clicked() {
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
                    if ui.add_enabled_ui(connected, |ui| icons::menu_item(ui, Some(Icon::Shield), crate::i18n::tr("Prüfen und reparieren …", "Check and repair …"), "")).inner.clicked() {
                        if let Some(d) = self.need_db() {
                            actions.push(Action::OpenDoktor(d));
                        }
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled_ui(connected, |ui| {
                        item(ui, crate::i18n::text("Exportieren …"), Cmd::Export, &mut cmd);
                    });
                    item(ui, crate::i18n::text("Importieren …"), Cmd::Import, &mut cmd);
                    if ui.add_enabled_ui(connected, |ui| icons::menu_item(ui, Some(Icon::FileCode), crate::i18n::text("SQL-Datei importieren …"), "")).inner.clicked() {
                        ui.close();
                        self.open_sql_file(true);
                    }
                });
                ui.menu_button("Server", |ui| {
                    style::menu_item_look(ui);
                    self.server_items(ui, &ctx, actions, &mut cmd);
                });
                ui.menu_button("Plugins", |ui| {
                    style::menu_item_look(ui);
                    for target in [crate::mcp::setup::Target::Claude, crate::mcp::setup::Target::Codex] {
                        if icons::menu_item(ui, Some(match target { crate::mcp::setup::Target::Claude => Icon::Claude, crate::mcp::setup::Target::Codex => Icon::Codex }), target.label(), "").clicked() {
                            actions.push(Action::OpenAi(target));
                            ui.close();
                        }
                    }
                });
                ui.menu_button(crate::i18n::text("Hilfe"), |ui| {
                    style::menu_item_look(ui);
                    item(ui, crate::i18n::text("Handbuch"), Cmd::Help, &mut cmd);
                    item(ui, crate::i18n::text("Nach Updates suchen"), Cmd::CheckUpdates, &mut cmd);
                });
            // Die Menueleiste belegt die ganze Breite: Projekt, Branch und die rechte Gruppe gehoeren mit hinein
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(4.0);
            divider(ui);
            // Projekt: Kaestchen mit Kuerzel und Name, Klick zeigt zuletzt geoeffnete und neue Projekte
            let resp = match &self.project {
                Some(p) => {
                    let name = workspace::dir_name(p);
                    let (initials, color) = titlebar::project_badge(&name);
                    titlebar::tool_button(ui, titlebar::Lead::Badge(initials, color), &name, pal.chrome_text, true).on_hover_text(p.display().to_string())
                }
                None => titlebar::tool_button(ui, titlebar::Lead::Icon(Icon::Folder, pal.chrome_text), crate::i18n::tr("Projekt öffnen …", "Open project …"), pal.chrome_text, true)
                    .on_hover_text(crate::i18n::tr("Gespeichert wird immer in einem Projekt oder Ordner", "Files are always saved in a project or folder")),
            };
            egui::Popup::menu(&resp).show(|ui| {
                crate::style::menu_item_look(ui);
                ui.set_min_width(270.0);
                // Wie das Projektmenue von IntelliJ IDEA: Aktionen oben, darunter die letzten Projekte mit Kuerzel und Ordner
                item(ui, crate::i18n::text("Ordner öffnen …"), Cmd::OpenFolder, &mut cmd);
                if icons::menu_item(ui, Some(Icon::NewFolder), crate::i18n::text("Neues Projekt …"), "").clicked() {
                    self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                    ui.close();
                }
                // Neues Java-Projekt (mit Datenbank, Build-Werkzeug und Tests)
                let r = icons::menu_item(ui, None, crate::i18n::tr("Neues Java-Projekt …", "New Java project …"), &keymap::text(Cmd::NewJavaProject));
                icons::paint_file(ui.painter(), Rect::from_center_size(pos2(r.rect.left() + 17.0, r.rect.center().y), vec2(15.0, 15.0)), "x.java");
                if r.clicked() {
                    cmd = Some(Cmd::NewJavaProject);
                    ui.close();
                }
                if icons::menu_item(ui, Some(Icon::GitBranch), crate::i18n::tr("Repository klonen …", "Clone repository …"), "").clicked() {
                    self.open_clone_dialog();
                    ui.close();
                }
                let recent: Vec<PathBuf> = self.settings.recent.iter().filter(|p| Some(*p) != self.project.as_ref() && p.is_dir()).take(8).cloned().collect();
                if !recent.is_empty() {
                    ui.separator();
                    icons::menu_heading(ui, crate::i18n::text("Zuletzt geöffnet"));
                    for p in recent {
                        if side::recent_row(ui, &p).clicked() {
                            self.switch_project(Some(p));
                            ui.close();
                        }
                    }
                }
                if let Some(p) = self.project.clone() {
                    ui.separator();
                    if icons::menu_item(ui, Some(Icon::FolderInput), crate::i18n::text("Im Datei-Explorer zeigen"), "").clicked() {
                        open_path(&p);
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Close), crate::i18n::tr("Projekt schließen", "Close project"), "").clicked() {
                        self.switch_project(None);
                        ui.close();
                    }
                }
            });
            // Branch (nur in Git-Projekten); Klick oeffnet die Git-Ansicht
            if let Some(st) = self.git_status_now()
                && !st.branch.is_empty()
                && titlebar::tool_button(ui, titlebar::Lead::Icon(Icon::GitBranch, pal.chrome_weak), &st.branch, pal.chrome_text, true).on_hover_text(Cmd::ShowGit.label()).clicked()
            {
                cmd = Some(Cmd::ShowGit);
            }
            left_end = ui.cursor().left();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                titlebar::controls(ui);
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                // Update installiert: zum Benutzen neu starten
                if let Some(v) = self.updater.installed_version() {
                    let label = crate::tr_format!("Neu starten ({v})", "Restart ({v})");
                    if crate::widgets::primary(ui, &label).on_hover_text(crate::i18n::text("Das Update ist installiert. Neu starten, um die neue Version zu benutzen.")).clicked() {
                        actions.push(Action::UpdateRestart);
                    }
                    ui.add_space(6.0);
                }
                // Verbindung: Zustandspunkt und Benutzer@Server; Klick zeigt die Server-Befehle
                let st = self.server.state();
                // Leuchtende Zustandsfarben: sie stehen auf der dunklen Titelleiste
                let color = if st.is_running() {
                    egui::Color32::from_rgb(0x6F, 0xD8, 0x9F)
                } else if matches!(st, State::Failed(_) | State::NotFound) {
                    egui::Color32::from_rgb(0xFF, 0x8A, 0x8F)
                } else {
                    egui::Color32::from_rgb(0xF4, 0xA5, 0x5A)
                };
                let text = match &self.db {
                    Some(d) => d.info.label(),
                    None => st.text().to_string(),
                };
                let tip = match &self.db {
                    Some(d) => format!("Server {} – {} (MariaDB {})", st.text(), d.info.label(), d.version.split('-').next().unwrap_or("")),
                    None => format!("Server {}", st.text()),
                };
                let resp = titlebar::tool_button(ui, titlebar::Lead::Dot(color), if bar_w >= 1000.0 { &text } else { "" }, pal.chrome_text, bar_w >= 1000.0).on_hover_text(tip);
                egui::Popup::menu(&resp).show(|ui| {
                    crate::style::menu_item_look(ui);
                    ui.set_min_width(220.0);
                    self.server_items(ui, &ctx, actions, &mut cmd);
                });
                divider(ui);
                // Werkzeuge
                let tools: &[(Icon, Cmd)] = if bar_w >= 820.0 { &[(Icon::Diagram, Cmd::ErDiagram), (Icon::Wizard, Cmd::QueryBuilder)] } else { &[] };
                for &(icon, c) in tools {
                    if titlebar::tool_button(ui, titlebar::Lead::Icon(icon, pal.chrome_text), "", pal.chrome_text, false).on_hover_text(tip_of(c)).clicked() {
                        cmd = Some(c);
                    }
                }
                if titlebar::tool_button(ui, titlebar::Lead::Icon(Icon::Plus, pal.chrome_text), if bar_w >= 1100.0 { crate::i18n::tr("Abfrage", "Query") } else { "" }, pal.chrome_text, false)
                    .on_hover_text(tip_of(Cmd::NewQuery))
                    .clicked()
                {
                    cmd = Some(Cmd::NewQuery);
                }
                // Ausfuehren der aktiven Datei (SQL, Java, Python, JavaScript) wie der Startknopf in IntelliJ
                let (runnable, busy) = self.tabs.get(self.active).map(|t| (t.runnable(), t.busy())).unwrap_or((false, false));
                if runnable {
                    divider(ui);
                    let resp = ui
                        .add_enabled_ui(!busy, |ui| titlebar::tool_button(ui, titlebar::Lead::Icon(Icon::Play, if busy { pal.chrome_weak } else { egui::Color32::from_rgb(0x6F, 0xD8, 0x9F) }), "", pal.chrome_text, false))
                        .inner
                        .on_hover_text(tip_of(Cmd::RunAll));
                    if resp.clicked() {
                        cmd = Some(Cmd::RunAll);
                    }
                }
                right_start = ui.cursor().right();
            });
        });
        // Suchfeld mittig in der freien Flaeche zwischen links und rechts
        let (lo, hi) = (left_end + 16.0, right_start - 16.0);
        if hi - lo >= 180.0 {
            let w = (hi - lo).min(420.0);
            let cx = bar.center().x.clamp(lo + w / 2.0, hi - w / 2.0);
            let r = Rect::from_center_size(pos2(cx, bar.center().y), vec2(w, titlebar::TOOL_H));
            // Wie die Befehlszentrale von VS Code: mit Projekt die Dateisuche (">" wechselt zu den Befehlen), sonst die Befehle
            let (c, hint) = if self.project.is_some() {
                (Cmd::QuickOpen, crate::i18n::tr("Dateien suchen, mit > Befehle", "Search files, > for commands"))
            } else {
                (Cmd::CommandPalette, crate::i18n::tr("Befehle suchen", "Search commands"))
            };
            if titlebar::search_box(ui, r, hint, &keymap::text(c)).clicked() {
                cmd = Some(c);
            }
        }
        if let Some(c) = cmd {
            self.run_cmd(&ctx, c);
        }
    }

    // ------------------------------------------------------------------
    // Aktivitaetsleiste (nur Seitenleisten)

    pub(super) fn activity_bar(&mut self, ui: &mut egui::Ui) {
        const TILE: f32 = 46.0;
        const GAP: f32 = 4.0;
        let pal = style::pal();
        let ctx = ui.ctx().clone();
        // Registerleiste: jedes Werkzeug ist ein Register in seiner eigenen Farbe (Symbol und kurze Beschriftung);
        // das offene ist ganz eingefaerbt. Klick blendet ein/aus, Rechtsklick verschiebt, Ziehen auf eine Zone ebenfalls.
        let button = |ui: &mut egui::Ui, icon: Icon, hue: egui::Color32, caption: &str, active: bool, tip: String| -> egui::Response {
            let (slot, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TILE), Sense::click_and_drag());
            let r = Rect::from_center_size(slot.center(), vec2(slot.width() - 10.0, TILE));
            let t = ui.ctx().animate_bool_with_time(resp.id.with("offen"), active, 0.2);
            let hot = ui.ctx().animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
            let p = ui.painter();
            // Unter der Maus eine leise Flaeche, offen ganz in der Registerfarbe (waechst aus der Mitte)
            if hot > 0.0 && t < 1.0 {
                p.rect_filled(r, 8.0, pal.hover.gamma_multiply(hot * (1.0 - t)));
            }
            if t > 0.0 {
                let tile = Rect::from_center_size(r.center(), r.size() * egui::lerp(0.86..=1.0, t));
                p.rect_filled(tile, 8.0, hue.gamma_multiply(t));
            }
            let on = if pal.dark { pal.face } else { egui::Color32::WHITE };
            let idle = hue.lerp_to_gamma(pal.activity_fg, 0.3 * (1.0 - hot));
            let fg = idle.lerp_to_gamma(on, t);
            icons::paint(p, Rect::from_center_size(pos2(r.center().x, r.top() + 17.0), vec2(19.0, 19.0)), icon, fg);
            let text_c = pal.text_weak.lerp_to_gamma(pal.text, hot).lerp_to_gamma(on, t);
            p.text(pos2(r.center().x, r.bottom() - 9.5), egui::Align2::CENTER_CENTER, caption, style::medium(10.5), text_c);
            resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        ui.spacing_mut().item_spacing.y = GAP;
        let tool = |this: &mut Self, ui: &mut egui::Ui, view: SideView, cmd: Option<Cmd>| {
            let mut tip = super::dock::tool_title(view).to_string();
            if let Some(cmd) = cmd {
                let k = keymap::text(cmd);
                tip = if k.is_empty() { cmd.label().to_string() } else { format!("{} ({k})", cmd.label()) };
            }
            let place = this.place_of(view);
            if place != super::dock::Place::Left {
                tip.push_str(&format!(" · {}", place.label()));
            }
            tip.push_str(crate::i18n::tr("\nZiehen zum Verschieben oder Abdocken", "\nDrag to move or undock"));
            let resp = button(ui, super::dock::tool_icon(view), super::dock::tool_hue(view), super::dock::tool_caption(view), this.tool_open(view), tip);
            if resp.drag_started() {
                this.dock.start_drag(view, ui.ctx().viewport_id());
            }
            if resp.clicked() {
                let open = this.tool_open(view);
                this.toggle_tool(view);
                if !open && view == SideView::Search {
                    this.search.focus = true;
                }
            }
            egui::Popup::context_menu(&resp).show(|ui| this.tool_place_menu(ui, view));
        };
        for (view, cmd) in [
            (SideView::Explorer, Cmd::ShowExplorer),
            (SideView::Search, Cmd::ShowSearch),
            (SideView::Databases, Cmd::ShowDatabases),
            (SideView::History, Cmd::ShowHistory),
            (SideView::Git, Cmd::ShowGit),
        ] {
            tool(self, ui, view, Some(cmd));
        }
        ui.add_space(10.0);
        for view in [SideView::Claude, SideView::Codex] {
            tool(self, ui, view, None);
        }
        ui.add_space(10.0);
        tool(self, ui, SideView::Terminal, Some(Cmd::ToggleTerminal));
        // Einstellungen unten, getrennt von den Ansichten
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(2.0);
            let k = keymap::text(Cmd::Settings);
            let tip = if k.is_empty() { Cmd::Settings.label().to_string() } else { format!("{} ({k})", Cmd::Settings.label()) };
            let open = self.tabs.get(self.active).is_some_and(|t| t.icon() == Some(Icon::Settings));
            if button(ui, Icon::Settings, pal.text_weak, crate::i18n::tr("Optionen", "Options"), open, tip).clicked() {
                self.run_cmd(&ctx, Cmd::Settings);
            }
        });
    }

    // ------------------------------------------------------------------
    // Registerkarten

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        const BAR_H: f32 = 40.0;
        // Reiter stehen unten auf der Leiste; oben bleibt Luft, damit die Laschen als Laschen erkennbar sind
        const PILL_H: f32 = 34.0;
        const GAP: f32 = 2.0;
        const MAX_W: f32 = 220.0;
        const ACTIVE_W: f32 = 200.0;
        const MIN_W: f32 = 112.0;
        const LIST_W: f32 = 34.0;
        let pal = style::pal();
        let font = style::medium(13.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR_H), Sense::hover());
        block_autoscroll(bar);
        // Haengeregister: graue Leiste, darauf die Reiter; der aktive geht nahtlos in das Papier darunter ueber
        ui.painter().rect_filled(bar, 0.0, pal.face);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, pal.border));
        let n = self.tabs.len();
        let mut looks: Vec<TabLook> = self
            .tabs
            .iter()
            .map(|t| TabLook {
                title: t.title(),
                icon: t.icon(),
                file: t.file().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()),
                hint: String::new(),
                busy: t.busy(),
                modified: t.modified(),
                hue: match t.database() {
                    Some(d) if !d.is_empty() => style::db_color(&d),
                    _ => t.icon().map(icons::tint).unwrap_or(pal.text_weak),
                },
            })
            .collect();
        // Gleichnamige Dateien: Ordner dahinter (wie in VS Code)
        for i in 0..looks.len() {
            if looks.iter().filter(|l| l.title == looks[i].title).count() > 1 {
                looks[i].hint = self.tabs[i].file().and_then(|f| f.parent()).and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            }
        }
        // Wunschbreite je Reiter. Passen nicht alle, schrumpfen zuerst die langen Titel bis auf eine lesbare
        // Mindestbreite; reicht das nicht, laesst sich die Leiste verschieben (und die Liste zeigt alle).
        let desired: Vec<f32> = looks
            .iter()
.map(|l| (ui.painter().layout_no_wrap(format!("{}  {}", l.title, l.hint), font.clone(), pal.text).size().x + 52.0 + 22.0).clamp(MIN_W, MAX_W))
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
            level(MIN_W)
        };
        let mut widths = fit(avail);
        let scroll = widths.iter().sum::<f32>() + gaps > avail + 0.5;
        if scroll {
            widths = fit(avail - LIST_W);
        }
        let tabs_rect = Rect::from_min_max(bar.min, bar.max - vec2(if scroll { LIST_W + 2.0 } else { 0.0 }, 0.0));

        let mut close = None;
        let mut close_others = None;
        let mut close_right = None;
        let mut close_all = false;
        let mut activate = None;
        let mut rects: Vec<Rect> = Vec::with_capacity(n);
        let mut dragging: Option<(usize, f32)> = None;
        let mut dropped: Option<(usize, f32)> = None;
        let active = self.active;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tabs_rect.with_min_x(tabs_rect.left() + 6.0)).layout(egui::Layout::left_to_right(egui::Align::Max)));
        egui::ScrollArea::horizontal().id_salt("tabbar").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let time = ui.input(|i| i.time) as f32;
            for (i, look) in looks.iter().enumerate() {
                let sel = i == active;
                let (slot, resp) = ui.allocate_exact_size(vec2(widths[i], BAR_H), Sense::click_and_drag());
                let r = slot.with_min_y(slot.max.y - PILL_H);
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
                let over_close = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| tab_close_rect(r).contains(p));
                let hot = ui.ctx().animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
                let lift = ui.ctx().animate_bool_with_time(resp.id.with("sel"), sel, 0.18);
                paint_tab(ui.painter(), r, look, lift, hot, over_close, time, &font);
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
                let cut = widths[i] + 1.0 < desired[i];
                let resp = if cut { resp.on_hover_text_at_pointer(&look.title) } else { resp };
                resp.context_menu(|ui| {
                    crate::style::menu_item_look(ui);
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
            let r = Rect::from_center_size(pos2(x.clamp(tabs_rect.left() + w / 2.0, tabs_rect.right() - w / 2.0), tabs_rect.bottom() - PILL_H / 2.0 - 3.0), vec2(w, PILL_H));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tab-drag")));
            painter.add(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: egui::Color32::from_black_alpha(50) }.as_shape(r, 6));
            paint_tab(&painter, r, &looks[i], 1.0, 0.0, false, 0.0, &font);
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
                crate::style::menu_item_look(ui);
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

    /// Keine Registerkarte offen: das Ordnerregal. Jede Datenbank steht als Ordnerruecken in ihrer Farbe da
    /// (Klick: neue Abfrage darin), daneben ein leerer Ruecken fuer eine neue Datenbank. Darunter die
    /// wichtigsten Befehle mit Kuerzel und, ohne Projekt, die zuletzt geoeffneten Ordner.
    pub(super) fn empty_area(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click());
        let mut cmds = vec![Cmd::NewQuery];
        cmds.push(if self.project.is_some() { Cmd::QuickOpen } else { Cmd::OpenFolder });
        cmds.extend([Cmd::NewFile, Cmd::CommandPalette, Cmd::ToggleTerminal]);
        let recent: Vec<PathBuf> = if self.project.is_none() { self.settings.recent.iter().filter(|p| p.is_dir()).take(4).cloned().collect() } else { Vec::new() };
        let dbs = if self.db.is_some() { self.user_databases() } else { Vec::new() };
        const ROW: f32 = 32.0;
        const SPINE_W: f32 = 58.0;
        const SPINE_H: f32 = 200.0;
        const SPINE_GAP: f32 = 10.0;
        let w = (rect.width() - 64.0).clamp(280.0, 760.0);
        let per_row = (((w + SPINE_GAP) / (SPINE_W + SPINE_GAP)).floor() as usize).max(1);
        let spines = dbs.len() + usize::from(self.db.is_some());
        let shelf_rows = if spines == 0 { 0 } else { spines.div_ceil(per_row).min(2) };
        let shelf_h = shelf_rows as f32 * (SPINE_H + 30.0);
        let list_h = 30.0 + cmds.len() as f32 * ROW + if recent.is_empty() { 0.0 } else { 34.0 + recent.len() as f32 * 42.0 };
        let head_h = 64.0;
        let total = head_h + shelf_h + list_h;
        let top = (rect.center().y - total / 2.0).max(rect.top() + 20.0);
        let x0 = rect.center().x - w / 2.0;
        let p = ui.painter().clone();
        // Kopf: Etikett des Programms, daneben Verbindung bzw. Version
        let logo = Rect::from_min_size(pos2(x0, top), vec2(40.0, 40.0));
        p.rect_filled(logo, 10.0, pal.primary);
        icons::paint(&p, Rect::from_center_size(logo.center(), vec2(24.0, 24.0)), Icon::Database, pal.on_primary);
        p.text(pos2(logo.right() + 14.0, logo.center().y - 9.0), egui::Align2::LEFT_CENTER, "EasyMySQL", crate::style::semi(21.0), pal.text);
        let sub = match &self.db {
            Some(d) => crate::tr_format!("Verbunden mit {}", "Connected to {}", d.info.label()),
            None => crate::tr_format!("Version {}", "Version {}", env!("CARGO_PKG_VERSION")),
        };
        p.text(pos2(logo.right() + 14.0, logo.center().y + 12.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.5), pal.text_weak);
        let mut run = None;
        let mut open_db: Option<String> = None;
        let mut new_db = false;
        let mut y = top + head_h;
        if shelf_rows > 0 {
            let now = ui.input(|i| i.time);
            let mut items: Vec<Option<&String>> = dbs.iter().map(Some).collect();
            items.push(None);
            for (row, chunk) in items.chunks(per_row).take(2).enumerate() {
                let base = y + row as f32 * (SPINE_H + 30.0) + SPINE_H;
                for (k, it) in chunk.iter().enumerate() {
                    let x = x0 + k as f32 * (SPINE_W + SPINE_GAP);
                    let slot = Rect::from_min_max(pos2(x, base - SPINE_H - 12.0), pos2(x + SPINE_W, base));
                    let id = egui::Id::new(("regal", row, k, it.map(|s| s.as_str()).unwrap_or("+")));
                    let r = ui.interact(slot, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    // Herausziehen: der Ruecken hebt sich unter der Maus; beim ersten Zeichnen stehen alle
                    // nacheinander auf (kurz versetzt, von links nach rechts)
                    let lift = ui.ctx().animate_bool_with_time(id.with("lift"), r.hovered(), 0.16) * 12.0;
                    let born = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(egui::Id::new("regal-start"), || now));
                    let appear = (((now - born) as f32 - (row * per_row + k) as f32 * 0.035) / 0.32).clamp(0.0, 1.0);
                    if appear < 1.0 {
                        ui.ctx().request_repaint();
                    }
                    let ease = 1.0 - (1.0 - appear).powi(3);
                    let drop = (1.0 - ease) * 24.0;
                    let spine = Rect::from_min_size(pos2(x, base - SPINE_H - lift + drop), vec2(SPINE_W, SPINE_H));
                    let alpha = ease;
                    match it {
                        Some(name) => {
                            let hue = style::db_color(name);
                            if lift > 0.5 {
                                p.add(egui::Shadow { offset: [0, 6], blur: 14, spread: 0, color: egui::Color32::from_black_alpha((40.0 * lift / 12.0) as u8) }.as_shape(spine, 6));
                            }
                            p.rect_filled(spine, 6.0, hue.gamma_multiply(alpha));
                            // Kante des Rueckens (Pappe) etwas dunkler
                            p.rect_filled(Rect::from_min_size(spine.min, vec2(5.0, SPINE_H)), egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, hue.lerp_to_gamma(egui::Color32::BLACK, 0.18).gamma_multiply(alpha));
                            // Etikett mit dem Namen, von unten nach oben gelesen
                            let pocket = Rect::from_min_max(pos2(spine.left() + 11.0, spine.top() + 14.0), pos2(spine.right() - 7.0, spine.top() + 138.0));
                            p.rect_filled(pocket, 3.0, pal.bg.gamma_multiply(alpha));
                            let mut job = egui::text::LayoutJob::simple_singleline(name.to_string(), crate::style::semi(14.0), pal.text);
                            job.wrap = egui::text::TextWrapping { max_width: pocket.height() - 14.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                            let g = p.layout_job(job);
                            let origin = pos2(pocket.center().x - g.size().y / 2.0, pocket.center().y + g.size().x / 2.0);
                            p.add(egui::epaint::TextShape::new(origin, g, pal.text.gamma_multiply(alpha)).with_angle(-std::f32::consts::FRAC_PI_2));
                            // Griffloch
                            let hole = pos2(spine.center().x + 2.0, spine.bottom() - 36.0);
                            p.circle_filled(hole, 10.0, hue.lerp_to_gamma(egui::Color32::BLACK, 0.35).gamma_multiply(alpha));
                            p.circle_stroke(hole, 10.0, Stroke::new(1.5, hue.lerp_to_gamma(egui::Color32::WHITE, 0.35).gamma_multiply(alpha)));
                            let r = r.on_hover_text(crate::tr_format!("Neue Abfrage in {}", "New query in {}", name));
                            if r.clicked() {
                                open_db = Some(name.to_string());
                            }
                        }
                        None => {
                            // Leerer Ruecken: neue Datenbank anlegen
                            let c = pal.text_weak.lerp_to_gamma(pal.text, lift / 12.0).gamma_multiply(alpha);
                            let pts = [spine.left_top(), spine.right_top(), spine.right_bottom(), spine.left_bottom(), spine.left_top()];
                            for seg in pts.windows(2) {
                                p.extend(egui::Shape::dashed_line(&[seg[0], seg[1]], Stroke::new(1.5, c), 6.0, 4.0));
                            }
                            icons::paint(&p, Rect::from_center_size(pos2(spine.center().x, spine.top() + 30.0), vec2(20.0, 20.0)), Icon::Plus, c);
                            let g = p.layout_no_wrap(crate::i18n::tr("Neue Datenbank", "New database").to_string(), crate::style::medium(13.0), c);
                            let origin = pos2(spine.center().x - g.size().y / 2.0, spine.top() + 60.0 + g.size().x);
                            p.add(egui::epaint::TextShape::new(origin, g, c).with_angle(-std::f32::consts::FRAC_PI_2));
                            if r.clicked() {
                                new_db = true;
                            }
                        }
                    }
                }
                // Regalbrett
                let row_w = (chunk.len() as f32 * (SPINE_W + SPINE_GAP) - SPINE_GAP).max(420.0).min(w);
                let board = Rect::from_min_max(pos2(x0 - 10.0, base), pos2(x0 + row_w + 10.0, base + 6.0));
                p.rect_filled(board, 2.0, pal.border.lerp_to_gamma(pal.text_weak, 0.2));
            }
            y += shelf_h;
            if dbs.len() + 1 > per_row * 2 {
                p.text(pos2(x0, y - 12.0), egui::Align2::LEFT_CENTER, crate::tr_format!("Weitere {} Datenbanken in der Seitenleiste", "{} more databases in the sidebar", dbs.len() + 1 - per_row * 2), egui::FontId::proportional(12.5), pal.text_weak);
            }
        }
        // Befehle als Zeilen mit Kuerzel
        y += 6.0;
        p.text(pos2(x0, y), egui::Align2::LEFT_CENTER, crate::i18n::tr("Starten", "Start"), crate::style::semi(13.0), pal.text);
        y += 14.0;
        let lw = w.min(420.0);
        let n_cmds = cmds.len();
        for (i, cmd) in cmds.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x0 - 8.0, y + i as f32 * ROW), vec2(lw + 8.0, ROW - 2.0));
            let row = ui.interact(r, egui::Id::new(("empty-cmd", i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let hot = ui.ctx().animate_bool_with_time(row.id.with("hot"), row.hovered(), 0.12);
            if hot > 0.0 {
                p.rect_filled(r, style::RADIUS as f32 + 1.0, pal.hover.gamma_multiply(hot));
            }
            let cy = r.center().y;
            if let Some(icon) = cmd_icon(cmd) {
                icons::paint(&p, Rect::from_center_size(pos2(r.left() + 18.0, cy), vec2(16.0, 16.0)), icon, icons::tint(icon));
            }
            p.text(pos2(r.left() + 38.0, cy), egui::Align2::LEFT_CENTER, cmd.label(), style::medium(13.0), pal.text);
            let k = keymap::text(cmd);
            if !k.is_empty() && lw >= 300.0 {
                let g = p.layout_no_wrap(k, egui::FontId::proportional(12.0), pal.text_weak);
                let kr = Rect::from_min_size(pos2(r.right() - 10.0 - g.size().x - 12.0, cy - 10.0), vec2(g.size().x + 12.0, 20.0));
                p.rect_filled(kr.translate(vec2(0.0, 1.5)), 4.0, pal.border);
                p.rect(kr, 4.0, pal.bg, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
                p.galley(pos2(kr.left() + 6.0, cy - g.size().y / 2.0), g, pal.text_weak);
            }
            if row.clicked() {
                run = Some(cmd);
            }
        }
        y += ROW * n_cmds as f32;
        if !recent.is_empty() {
            y += 18.0;
            p.text(pos2(x0, y), egui::Align2::LEFT_CENTER, crate::i18n::text("Zuletzt geöffnet"), crate::style::semi(13.0), pal.text);
            y += 12.0;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(x0 - 8.0, y), vec2(lw + 8.0, recent.len() as f32 * 42.0))).layout(egui::Layout::top_down_justified(egui::Align::Min)));
            child.spacing_mut().item_spacing.y = 2.0;
            for path in recent {
                if super::side::recent_row(&mut child, &path).clicked() {
                    self.switch_project(Some(path));
                }
            }
        }
        if resp.double_clicked() {
            run = Some(Cmd::NewQuery);
        }
        if new_db {
            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
        }
        if let Some(db) = open_db {
            self.current_db = db.clone();
            self.handle_actions(vec![Action::OpenSql { db: Some(db), sql: String::new(), run: false }]);
        }
        if let Some(c) = run {
            self.run_cmd(&ui.ctx().clone(), c);
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
    /// Dateiname (fuer das Dateisymbol)
    file: Option<String>,
    /// Ordner bei gleichnamigen Dateien
    hint: String,
    busy: bool,
    modified: bool,
    /// Reiterfarbe: Farbe der Datenbank, sonst die Farbe der Art
    hue: egui::Color32,
}

/// Symbol eines Befehls in Menues
pub(super) fn cmd_icon(c: Cmd) -> Option<Icon> {
    Some(match c {
        Cmd::RunAll | Cmd::RunStatement => Icon::Play,
        Cmd::Explain => Icon::Explain,
        Cmd::NewQuery => Icon::Plus,
        Cmd::NewFile => Icon::NewFile,
        Cmd::NewJavaProject => Icon::Coffee,
        Cmd::OpenFile => Icon::File,
        Cmd::OpenFolder => Icon::Folder,
        Cmd::Save => Icon::Save,
        Cmd::SaveAll => Icon::Save,
        Cmd::CloseTab | Cmd::CloseAllTabs => Icon::Close,
        Cmd::ReopenTab => Icon::RotateCcw,
        Cmd::CommandPalette => Icon::Search,
        Cmd::QuickOpen => Icon::Files,
        Cmd::ToggleTerminal | Cmd::NewTerminal => Icon::Terminal,
        Cmd::FontLarger => Icon::ZoomIn,
        Cmd::FontSmaller => Icon::ZoomOut,
        Cmd::FontReset => Icon::Fit,
        Cmd::ShowExplorer => Icon::Files,
        Cmd::ShowSearch => Icon::Search,
        Cmd::ShowDatabases => Icon::Database,
        Cmd::ShowHistory => Icon::History,
        Cmd::ShowGit => Icon::GitBranch,
        Cmd::Settings => Icon::Settings,
        Cmd::Help => Icon::Book,
        Cmd::CheckUpdates => Icon::CloudDownload,
        Cmd::ServerLog => Icon::FileText,
        Cmd::ErDiagram => Icon::Diagram,
        Cmd::QueryBuilder => Icon::Wizard,
        Cmd::Backups => Icon::Shield,
        Cmd::Import => Icon::Upload,
        Cmd::Export => Icon::Download,
        Cmd::Users => Icon::Users,
        Cmd::Format => Icon::Code,
        Cmd::FixAll => Icon::Wrench,
        Cmd::CopyJava => Icon::Coffee,
        Cmd::Find | Cmd::Replace => Icon::Search,
        Cmd::RunTests => Icon::Flask,
        Cmd::BuildJar => Icon::Package,
        Cmd::AddLibrary => Icon::Package,
        Cmd::DebugStart => Icon::Bug,
        Cmd::StepOver => Icon::StepOver,
        Cmd::StepInto => Icon::StepInto,
        Cmd::StepOut => Icon::StepOut,
        Cmd::DebugStop => Icon::Stop,
        Cmd::GitCommit => Icon::GitCommit,
        Cmd::GitPush => Icon::CloudUpload,
        Cmd::GitPull => Icon::CloudDownload,
        _ => return None,
    })
}

fn self_file(tabs: &[Box<dyn TabView>], i: usize) -> Option<PathBuf> {
    tabs.get(i).and_then(|t| t.file().map(|p| p.to_path_buf()))
}

/// Klickflaeche des Schliessen-Kreuzes (rechts im Reiter)
fn tab_close_rect(r: Rect) -> Rect {
    Rect::from_center_size(pos2(r.right() - 17.0, r.center().y + 1.0), vec2(18.0, 18.0))
}

/// Reiter als Haengeregister: oben ein Streifen in der Farbe seiner Datenbank. Der aktive Reiter (`sel` 0..1,
/// animiert) hebt sich als Papier aus der Leiste und geht nahtlos in den Inhalt ueber; `hot` ist das Darueberfahren.
#[allow(clippy::too_many_arguments)]
fn paint_tab(p: &egui::Painter, r: Rect, look: &TabLook, sel: f32, hot: f32, over_close: bool, time: f32, font: &egui::FontId) {
    let pal = style::pal();
    let top = egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 };
    if hot > 0.0 && sel < 1.0 {
        p.rect_filled(r, top, pal.hover.gamma_multiply(hot * (1.0 - sel)));
    }
    if sel > 0.0 {
        // Papier des aktiven Reiters mit farbiger Oberkante, die der Rundung folgt; unten offen zum Inhalt
        let paper = Rect::from_min_max(r.min, r.max + vec2(0.0, 1.0));
        p.rect_filled(paper, top, look.hue.gamma_multiply(sel));
        let inner = Rect::from_min_max(paper.min + vec2(1.0, 2.5), paper.max - vec2(1.0, 0.0));
        p.rect_filled(inner, egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, pal.bg.gamma_multiply(sel));
        // Seitenkanten wie die Linie unter der Leiste
        let edge = Stroke::new(1.0, pal.border.gamma_multiply(sel));
        p.vline(paper.left() + 0.5, (paper.top() + 7.0)..=(paper.bottom() - 1.0), edge);
        p.vline(paper.right() - 0.5, (paper.top() + 7.0)..=(paper.bottom() - 1.0), edge);
    }
    let cy = r.center().y + 1.0;
    let color = pal.text_weak.lerp_to_gamma(pal.text, sel.max(hot * 0.7));
    let icon = look.icon.unwrap_or(Icon::File);
    let mut x = r.left() + 12.0;
    match &look.file {
        Some(f) => icons::paint_file(p, Rect::from_center_size(pos2(x + 7.0, cy), vec2(15.0, 15.0)), f),
        None => icons::paint(p, Rect::from_center_size(pos2(x + 7.0, cy), vec2(14.0, 14.0)), icon, look.hue),
    }
    x += 22.0;
    let max_w = (r.right() - 30.0 - x).max(10.0);
    let font = if sel > 0.5 { style::semi(font.size) } else { font.clone() };
    let mut job = egui::text::LayoutJob::simple_singleline(look.title.clone(), font, color);
    if !look.hint.is_empty() {
        job.append(&format!("  {}", look.hint), 0.0, egui::TextFormat::simple(egui::FontId::proportional(12.5), pal.text_weak));
    }
    job.wrap = egui::text::TextWrapping { max_width: max_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = p.layout_job(job);
    p.galley(pos2(x, cy - galley.size().y / 2.0), galley, color);
    // Kreuz bzw. Laufanzeige bzw. Punkt fuer Ungespeichertes
    let cr = tab_close_rect(r);
    if look.busy {
        let a = time * 6.0;
        p.add(egui::Shape::line(
            (0..=10).map(|k| cr.center() + vec2((a + k as f32 * 0.45).cos(), (a + k as f32 * 0.45).sin()) * 5.0).collect(),
            Stroke::new(1.5, look.hue),
        ));
    } else if look.modified && !(hot > 0.5 && over_close) {
        p.circle_filled(cr.center(), 3.5, look.hue);
    } else if sel > 0.5 || hot > 0.0 {
        if over_close {
            p.rect_filled(cr, style::RADIUS as f32 - 1.0, pal.hover);
        }
        let c = if over_close { pal.text } else { color.gamma_multiply(sel.max(hot)) };
        icons::paint(p, cr.shrink(4.0), Icon::Close, c);
    }
}
