// Registerkarte "Einstellungen": alle Einstellungen an einem Ort, inkl. Tastenkuerzel.

use super::{Action, Ctx, TabView};
use crate::db::ConnInfo;
use crate::keymap::{self, Binding, Cmd, Keymap};
use crate::style;
use crate::workspace;
use eframe::egui::{self, RichText};
use std::sync::atomic::{AtomicBool, Ordering};

/// Wird gerade ein Tastenkuerzel aufgezeichnet? (dann loest die App keine Befehle aus)
static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

pub const SECTIONS: &[(&str, &str)] = &[
    ("darstellung", "Darstellung"),
    ("updates", "Updates"),
    ("editor", "Editor"),
    ("maus", "Maus"),
    ("speicherorte", "Speicherorte"),
    ("verbindung", "Verbindung"),
    ("server", "Server"),
    ("sicherungen", "Sicherungen"),
    ("tastenkuerzel", "Tastenkürzel"),
    ("vscode", "Visual Studio Code"),
];

pub struct SettingsTab {
    goto: Option<String>,
    active_section: String,
    scroll_offset: Option<f32>,
    pinned_section: bool,
    headings: Vec<(String, f32)>,
    conn: Option<(ConnInfo, bool)>,
    keymap: Keymap,
    key_filter: String,
    /// Aufzeichnung: (Befehl, Index der Belegung; None = neue hinzufuegen)
    record: Option<(Cmd, Option<usize>)>,
    backup: crate::backup::Config,
    /// Neuer Speicherordner, wartet auf "verschieben?"
    pending_root: Option<std::path::PathBuf>,
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            goto: None,
            active_section: "darstellung".into(),
            scroll_offset: None,
            pinned_section: false,
            headings: Vec::new(),
            conn: None,
            keymap: keymap::current(),
            key_filter: String::new(),
            record: None,
            backup: crate::backup::Config::load(),
            pending_root: None,
        }
    }

    fn heading(&mut self, ui: &mut egui::Ui, id: &str, title: &str) {
        ui.add_space(14.0);
        let r = ui.label(RichText::new(title).size(17.0).strong());
        self.headings.push((id.to_string(), r.rect.top()));
        ui.separator();
        if self.goto.as_deref() == Some(id) {
            r.scroll_to_me_animation(Some(egui::Align::TOP), egui::style::ScrollAnimation::none());
            self.goto = None;
        }
    }

    fn keys_ui(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        // Tastendruck aufzeichnen
        if let Some((cmd, idx)) = self.record {
            let ev = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => Some(None),
                    e => Binding::from_event(e, &i.modifiers).map(Some),
                })
            });
            if let Some(res) = ev {
                ui.input_mut(|i| i.events.clear());
                if let Some(b) = res {
                    let mut list = self.keymap.get(cmd).to_vec();
                    match idx {
                        Some(i) if i < list.len() => list[i] = b,
                        _ if !list.contains(&b) => list.push(b),
                        _ => {}
                    }
                    self.keymap.set(cmd, list);
                    self.keymap.save();
                    keymap::install(self.keymap.clone());
                }
                self.record = None;
            }
        }
        RECORDING.store(self.record.is_some(), Ordering::Relaxed);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.key_filter).hint_text(crate::i18n::text("Befehl oder Taste suchen")).desired_width(240.0));
            if ui.button(crate::i18n::text("Alle auf Standard")).clicked() {
                self.keymap = Keymap::default();
                self.keymap.save();
                keymap::install(self.keymap.clone());
            }
        });
        let f = self.key_filter.to_lowercase();
        for (group, cmds) in keymap::GROUPS {
            let visible: Vec<Cmd> = cmds
                .iter()
                .copied()
                .filter(|c| f.is_empty() || c.label().to_lowercase().contains(&f) || self.keymap.get(*c).iter().any(|b| b.text().to_lowercase().contains(&f)))
                .collect();
            if visible.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::text(group)).strong().color(pal.text_weak));
            egui::Grid::new(("keys", *group)).num_columns(3).striped(true).spacing([16.0, 4.0]).min_col_width(80.0).show(ui, |ui| {
                for cmd in visible {
                    ui.allocate_ui_with_layout(egui::vec2(290.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(290.0);
                        ui.label(cmd.label());
                    });
                    ui.horizontal(|ui| {
                        let list = self.keymap.get(cmd).to_vec();
                        for (i, b) in list.iter().enumerate() {
                            let rec = self.record == Some((cmd, Some(i)));
                            let text = if rec { crate::i18n::text("Taste drücken …").to_string() } else { b.text() };
                            let conflicts = self.keymap.conflicts(cmd, b);
                            let mut rt = RichText::new(text).monospace();
                            if !conflicts.is_empty() {
                                rt = rt.color(pal.error_text);
                            }
                            let r = ui.add(egui::Button::new(rt).selected(rec));
                            let r = if conflicts.is_empty() {
                                r.on_hover_text(crate::i18n::text("Klicken zum Ändern, Rechtsklick zum Entfernen"))
                            } else {
                                let names: Vec<&str> = conflicts.iter().map(|c| c.label()).collect();
                                r.on_hover_text(crate::tr_format!("Auch belegt: {}", "Also assigned: {}", names.join(", ")))
                            };
                            if r.clicked() {
                                self.record = Some((cmd, Some(i)));
                            }
                            if r.secondary_clicked() {
                                let mut l = list.clone();
                                l.remove(i);
                                self.keymap.set(cmd, l);
                                self.keymap.save();
                                keymap::install(self.keymap.clone());
                            }
                        }
                        let rec_new = self.record == Some((cmd, None));
                        if ui.add(egui::Button::new(if rec_new { crate::i18n::text("Taste drücken …") } else { "+" }).selected(rec_new)).on_hover_text(crate::i18n::text("Weiteres Kürzel")).clicked() {
                            self.record = Some((cmd, None));
                        }
                    });
                    if self.keymap.get(cmd) != cmd.defaults().as_slice() {
                        if ui.small_button(crate::i18n::text("Standard")).clicked() {
                            self.keymap.set(cmd, cmd.defaults());
                            self.keymap.save();
                            keymap::install(self.keymap.clone());
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        }
    }
}

impl TabView for SettingsTab {
    fn title(&self) -> String {
        crate::i18n::text("Einstellungen").into()
    }

    fn key(&self) -> Option<String> {
        Some("settings".into())
    }

    fn session(&self) -> Option<String> {
        Some("settings".into())
    }

    fn show_section(&mut self, id: &str) {
        self.goto = Some(id.to_string());
        self.active_section = id.to_string();
        self.pinned_section = true;
    }

    fn on_close(&mut self) -> bool {
        RECORDING.store(false, Ordering::Relaxed);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let before = cx.settings.clone();
        self.headings.clear();
        egui::Panel::left("settings-nav").resizable(false).exact_size(190.0).frame(egui::Frame::new().fill(pal.sidebar_bg).inner_margin(egui::Margin::symmetric(8, 12))).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("settings-nav-scroll").show(ui, |ui| {
                for (id, name) in SECTIONS {
                    if style::nav_item(ui, crate::i18n::text(name), self.active_section == *id, 0.0).clicked() {
                        self.show_section(id);
                    }
                }
            });
        });
        let jumping = self.goto.is_some();
        let mut offset = 0.0;
        let body = egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            offset = viewport.min.y;
            egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 12)).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(760.0));
                let s = &mut *cx.settings;
                let grid = |ui: &mut egui::Ui, id: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                    egui::Grid::new(id).num_columns(2).striped(false).spacing([24.0, 10.0]).min_col_width(170.0).show(ui, |ui| add(ui));
                };

                self.heading(ui, "darstellung", crate::i18n::text("Darstellung"));
                grid(ui, "g-look", &mut |ui| {
                    ui.label(crate::i18n::text("Sprache"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.language, crate::i18n::Language::English, "English");
                        ui.selectable_value(&mut s.language, crate::i18n::Language::German, "Deutsch");
                    });
                    ui.end_row();
                    ui.label(crate::i18n::text("Design"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut s.dark, true, crate::i18n::text("Dunkel"));
                        ui.selectable_value(&mut s.dark, false, crate::i18n::text("Hell"));
                    });
                    ui.end_row();

                    ui.label(crate::i18n::text("Systemdatenbanken zeigen"));
                    ui.checkbox(&mut s.show_system_dbs, "information_schema, mysql, …");
                    ui.end_row();
                });

                self.heading(ui, "updates", "Updates");
                ui.label(crate::tr_format!("Installierte Version: {}", "Installed version: {}", env!("CARGO_PKG_VERSION")));
                ui.checkbox(&mut s.auto_update, crate::i18n::text("Beim Start und alle sechs Stunden nach Updates suchen"));
                ui.label(crate::i18n::text("Neue Versionen werden automatisch gefunden. Die Installation starten Sie selbst."));
                if ui.button(crate::i18n::text("Jetzt nach Updates suchen")).clicked() {
                    cx.actions.push(Action::Run(Cmd::CheckUpdates));
                }

                self.heading(ui, "editor", "Editor");
                grid(ui, "g-editor", &mut |ui| {
                    ui.label(crate::i18n::text("Schriftgröße"));
                    ui.add(egui::Slider::new(&mut s.editor_font, 10..=28).suffix(" px"));
                    ui.end_row();
                    ui.label("Minimap");
                    ui.checkbox(&mut s.minimap, crate::i18n::text("Übersicht rechts neben dem Text"));
                    ui.end_row();
                    ui.label(crate::i18n::text("Vorschläge beim Tippen"));
                    ui.checkbox(&mut s.auto_suggest, crate::i18n::text("Automatisch Vorschläge zeigen"))
                        .on_hover_text(crate::tr_format!("sonst nur mit {}", "otherwise only with {}", keymap::text(Cmd::Suggest)));
                    ui.end_row();
                    ui.label(crate::i18n::text("Klammern schließen"));
                    ui.checkbox(&mut s.auto_close, crate::i18n::text("( [ { ' \" ` automatisch schließen"));
                    ui.end_row();
                });

                self.heading(ui, "maus", crate::i18n::text("Maus"));
                grid(ui, "g-mouse", &mut |ui| {
                    ui.label(crate::i18n::text("Scrollgeschwindigkeit"));
                    ui.add(egui::Slider::new(&mut s.scroll_speed, 0.5..=5.0).step_by(0.25).suffix("×"));
                    ui.end_row();
                    ui.label(crate::i18n::text("Mittlere Maustaste"));
                    ui.checkbox(&mut s.autoscroll, crate::i18n::text("gedrückt halten und ziehen = schnell scrollen"));
                    ui.end_row();
                });

                self.heading(ui, "speicherorte", crate::i18n::text("Speicherorte"));
                let root = workspace::storage_root();
                let mut pick_root = false;
                let mut reset_root = false;
                grid(ui, "g-paths", &mut |ui| {
                    ui.label(crate::i18n::text("Speicherordner")).on_hover_text(crate::i18n::text("Neue Projekte, unbenannte Abfragen, frühere Fassungen, Verlauf, Diagramm-Anordnungen und gespeicherte Abfragen des Assistenten"));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(root.display().to_string()).monospace());
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::text("Ändern …")).clicked() {
                                pick_root = true;
                            }
                            if !s.storage_dir.is_empty() && ui.button(crate::i18n::text("Standard")).clicked() {
                                reset_root = true;
                            }
                            if ui.button(crate::i18n::text("Öffnen")).clicked() {
                                let _ = std::fs::create_dir_all(&root);
                                crate::app::open_path(&root);
                            }
                        });
                    });
                    ui.end_row();
                    ui.label(crate::i18n::text("Sicherungen"));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(self.backup.dir().display().to_string()).monospace());
                        ui.horizontal(|ui| {
                            if ui.button(crate::i18n::text("Ändern …")).clicked() {
                                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                    self.backup.dir = p.display().to_string();
                                    self.backup.save();
                                }
                            }
                            if !self.backup.dir.is_empty() && ui.button(crate::i18n::text("Standard")).clicked() {
                                self.backup.dir.clear();
                                self.backup.save();
                            }
                            if ui.button(crate::i18n::text("Öffnen")).clicked() {
                                let d = self.backup.dir();
                                let _ = std::fs::create_dir_all(&d);
                                crate::app::open_path(&d);
                            }
                        });
                    });
                    ui.end_row();
                    if let Some(p) = &cx.server.paths {
                        ui.label(crate::i18n::text("Datenbanken (MariaDB)"));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(p.data.display().to_string()).monospace());
                            if ui.button(crate::i18n::text("Öffnen")).clicked() {
                                crate::app::open_path(&p.data);
                            }
                        });
                        ui.end_row();
                    }
                    ui.label(crate::i18n::text("Einstellungen und Log"));
                    ui.horizontal(|ui| {
                        let d = crate::server::app_data_dir();
                        ui.label(RichText::new(d.display().to_string()).monospace());
                        if ui.button(crate::i18n::text("Öffnen")).clicked() {
                            crate::app::open_path(&d);
                        }
                    });
                    ui.end_row();
                });
                if pick_root {
                    if let Some(p) = rfd::FileDialog::new().set_directory(&root).pick_folder() {
                        if p != root {
                            self.pending_root = Some(p);
                        }
                    }
                }
                if reset_root && workspace::default_root() != root {
                    self.pending_root = Some(workspace::default_root());
                }
                if let Some(new) = self.pending_root.clone() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(crate::tr_format!("Neuer Speicherordner: {}", "New storage folder: {}", new.display()));
                        ui.horizontal(|ui| {
                            let apply = |move_files: bool, s: &mut crate::settings::Settings, actions: &mut Vec<Action>| -> Result<(), String> {
                                if move_files {
                                    workspace::move_storage(&root, &new)?;
                                    actions.push(Action::FilesMoved { from: root.clone(), to: new.clone() });
                                }
                                s.storage_dir = if new == workspace::default_root() { String::new() } else { new.display().to_string() };
                                Ok(())
                            };
                            let mut res = None;
                            if ui.button(crate::i18n::text("Inhalt mitnehmen")).on_hover_text(crate::i18n::text("Alles aus dem bisherigen Ordner in den neuen verschieben")).clicked() {
                                res = Some(apply(true, s, cx.actions));
                            }
                            if ui.button(crate::i18n::text("Nur umstellen")).clicked() {
                                res = Some(apply(false, s, cx.actions));
                            }
                            if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                                self.pending_root = None;
                            }
                            match res {
                                Some(Ok(())) => self.pending_root = None,
                                Some(Err(e)) => cx.actions.push(Action::Error(e)),
                                None => {}
                            }
                        });
                    });
                }

                self.heading(ui, "verbindung", crate::i18n::text("Verbindung"));
                let (info, save_pw) = self.conn.get_or_insert_with(|| (s.conn.clone(), s.save_password));
                grid(ui, "g-conn", &mut |ui| {
                    ui.label("Server");
                    ui.text_edit_singleline(&mut info.host);
                    ui.end_row();
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut info.port).range(1..=65535));
                    ui.end_row();
                    ui.label(crate::i18n::text("Benutzer"));
                    ui.text_edit_singleline(&mut info.user);
                    ui.end_row();
                    ui.label(crate::i18n::text("Passwort"));
                    ui.add(egui::TextEdit::singleline(&mut info.password).password(true));
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(save_pw, crate::i18n::text("Passwort speichern"));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::text("Verbinden")).clicked() {
                        s.conn = info.clone();
                        s.save_password = *save_pw;
                        cx.actions.push(Action::Connect);
                    }
                    if ui.button(crate::i18n::text("Standard")).on_hover_text(crate::i18n::text("127.0.0.1, Port 3306, root, ohne Passwort")).clicked() {
                        *info = ConnInfo::default();
                    }
                    let connected = cx.db.map(|d| d.info.label()).unwrap_or_else(|| crate::i18n::text("nicht verbunden").into());
                    ui.label(RichText::new(connected).color(pal.text_weak));
                });

                self.heading(ui, "server", "Server");
                grid(ui, "g-server", &mut |ui| {
                    ui.label(crate::i18n::text("Anweisungen mitschreiben"));
                    ui.checkbox(&mut s.query_log, crate::i18n::text("jede Anweisung im Server-Log (auch von mysql in der Eingabeaufforderung)"));
                    ui.end_row();
                    ui.label(crate::i18n::text("Server-Log"));
                    if ui.button(crate::i18n::text("Öffnen")).clicked() {
                        cx.actions.push(Action::OpenLog);
                    }
                    ui.end_row();
                });

                self.heading(ui, "sicherungen", crate::i18n::text("Sicherungen"));
                let old = self.backup.clone();
                grid(ui, "g-backup", &mut |ui| {
                    ui.label(crate::i18n::text("Automatisch sichern"));
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.backup.auto, crate::i18n::text("alle"));
                        ui.add(egui::DragValue::new(&mut self.backup.interval_hours).range(1..=168).suffix(" h"));
                    });
                    ui.end_row();
                    ui.label(crate::i18n::text("Aufbewahren"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.backup.keep_count).range(1..=200));
                        ui.label(crate::i18n::text("neueste, dazu eine je Tag für"));
                        ui.add(egui::DragValue::new(&mut self.backup.keep_days).range(0..=365).suffix(crate::i18n::text(" Tage")));
                    });
                    ui.end_row();
                    ui.label("");
                    if ui.button(crate::i18n::text("Sicherungen & Reparatur")).clicked() {
                        cx.actions.push(Action::OpenSafety);
                    }
                    ui.end_row();
                });
                if self.backup != old {
                    self.backup.save();
                }

                self.heading(ui, "tastenkuerzel", crate::i18n::text("Tastenkürzel"));
                self.keys_ui(ui);

                self.heading(ui, "vscode", "Visual Studio Code");
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::text("VS Code einrichten")).on_hover_text(crate::i18n::text("SQLTools mit MariaDB-Treiber installieren, Verbindung „EasyMySQL“ anlegen")).clicked() {
                        cx.actions.push(Action::SetupVsCode);
                    }
                });
                ui.add_space(30.0);
            });
        });
        // Nach einem Sprung die angeklickte Kategorie behalten, auch wenn der
        // letzte Abschnitt wegen der Fensterhoehe nicht bis nach oben scrollt.
        if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| egui::Rect::from_min_max(body.inner_rect.min, body.inner_rect.max + egui::vec2(16.0, 0.0)).contains(p))
            && (i.smooth_scroll_delta != egui::Vec2::ZERO || i.pointer.any_down())) {
            self.pinned_section = false;
        }
        if !jumping && !self.pinned_section && self.scroll_offset != Some(offset) {
            if let Some((id, _)) = self.headings.iter().rev().find(|(_, y)| *y <= body.inner_rect.top() + 36.0).or(self.headings.first()) {
                self.active_section = id.clone();
            }
        }
        self.scroll_offset = Some(offset);
        self.pinned_section |= jumping;
        if *cx.settings != before {
            cx.actions.push(Action::SettingsChanged);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_keeps_clicked_section_selected_at_bottom_and_tracks_scrolling() {
        let ctx = egui::Context::default();
        style::apply(&ctx, true);
        let mut tab = SettingsTab::new();
        let server = crate::server::Server::new(3306);
        let mut settings = crate::settings::Settings::default();
        let mut schemas = super::super::SchemaCache::default();
        let mut actions = Vec::new();
        let mut render = |tab: &mut SettingsTab, events: Vec<egui::Event>| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 480.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        tab.ui(
                            ui,
                            &mut Ctx {
                                db: None,
                                schemas: &mut schemas,
                                databases: &[],
                                server: &server,
                                project: None,
                                settings: &mut settings,
                                actions: &mut actions,
                            },
                        );
                    });
                },
            )
        };
        for _ in 0..3 {
            let _ = render(&mut tab, vec![]);
        }
        tab.show_section("vscode");
        for _ in 0..5 {
            let _ = render(&mut tab, vec![]);
        }
        assert_eq!(tab.active_section, "vscode", "Ein Klick muss auch den letzten Abschnitt dauerhaft markieren");
        assert!(tab.scroll_offset.unwrap() > 0.0);
        let _ = render(
            &mut tab,
            vec![
                egui::Event::PointerMoved(egui::pos2(500.0, 250.0)),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 10000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..5 {
            let _ = render(&mut tab, vec![]);
        }
        assert_eq!(tab.active_section, "darstellung", "Manuelles Scrollen muss die aktive Kategorie nachführen");
    }
}
