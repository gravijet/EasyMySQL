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
        ui.separator();
        if self.goto.as_deref() == Some(id) {
            r.scroll_to_me(Some(egui::Align::TOP));
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
            ui.add(egui::TextEdit::singleline(&mut self.key_filter).hint_text("Befehl oder Taste suchen").desired_width(240.0));
            if ui.button("Alle auf Standard").clicked() {
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
            ui.label(RichText::new(*group).strong().color(pal.text_weak));
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
                            let text = if rec { "Taste drücken …".to_string() } else { b.text() };
                            let conflicts = self.keymap.conflicts(cmd, b);
                            let mut rt = RichText::new(text).monospace();
                            if !conflicts.is_empty() {
                                rt = rt.color(pal.error_text);
                            }
                            let r = ui.add(egui::Button::new(rt).selected(rec));
                            let r = if conflicts.is_empty() {
                                r.on_hover_text("Klicken zum Ändern, Rechtsklick zum Entfernen")
                            } else {
                                let names: Vec<&str> = conflicts.iter().map(|c| c.label()).collect();
                                r.on_hover_text(format!("Auch belegt: {}", names.join(", ")))
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
                        if ui.add(egui::Button::new(if rec_new { "Taste drücken …" } else { "+" }).selected(rec_new)).on_hover_text("Weiteres Kürzel").clicked() {
                            self.record = Some((cmd, None));
                        }
                    });
                    if self.keymap.get(cmd) != cmd.defaults().as_slice() {
                        if ui.small_button("Standard").clicked() {
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
        "Einstellungen".into()
    }

    fn key(&self) -> Option<String> {
        Some("settings".into())
    }

    fn session(&self) -> Option<String> {
        Some("settings".into())
    }

    fn show_section(&mut self, id: &str) {
        self.goto = Some(id.to_string());
    }

    fn on_close(&mut self) -> bool {
        RECORDING.store(false, Ordering::Relaxed);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let before = cx.settings.clone();
        egui::Panel::left("settings-nav").resizable(false).exact_size(170.0).frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 8))).show(ui, |ui| {
            for (id, name) in SECTIONS {
                if ui.add(egui::Button::new(*name).frame(false)).clicked() {
                    self.goto = Some(id.to_string());
                }
            }
        });
        egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(760.0);
            let s = &mut *cx.settings;
            let grid = |ui: &mut egui::Ui, id: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                egui::Grid::new(id).num_columns(2).striped(false).spacing([24.0, 10.0]).min_col_width(170.0).show(ui, |ui| add(ui));
            };

            self.heading(ui, "darstellung", "Darstellung");
            grid(ui, "g-look", &mut |ui| {
                ui.label("Design");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.dark, true, "Dunkel");
                    ui.radio_value(&mut s.dark, false, "Hell");
                });
                ui.end_row();
                ui.label("Systemdatenbanken zeigen");
                ui.checkbox(&mut s.show_system_dbs, "information_schema, mysql, …");
                ui.end_row();
            });

            self.heading(ui, "editor", "Editor");
            grid(ui, "g-editor", &mut |ui| {
                ui.label("Schriftgröße");
                ui.add(egui::Slider::new(&mut s.editor_font, 10..=28).suffix(" px"));
                ui.end_row();
                ui.label("Minimap");
                ui.checkbox(&mut s.minimap, "Übersicht rechts neben dem Text");
                ui.end_row();
                ui.label("Vorschläge beim Tippen");
                ui.checkbox(&mut s.auto_suggest, format!("sonst nur mit {}", keymap::text(Cmd::Suggest)));
                ui.end_row();
                ui.label("Klammern schließen");
                ui.checkbox(&mut s.auto_close, "( [ { ' \" ` automatisch schließen");
                ui.end_row();
            });

            self.heading(ui, "maus", "Maus");
            grid(ui, "g-mouse", &mut |ui| {
                ui.label("Scrollgeschwindigkeit");
                ui.add(egui::Slider::new(&mut s.scroll_speed, 0.5..=5.0).step_by(0.25).suffix("×"));
                ui.end_row();
                ui.label("Mittlere Maustaste");
                ui.checkbox(&mut s.autoscroll, "gedrückt halten und ziehen = schnell scrollen");
                ui.end_row();
            });

            self.heading(ui, "speicherorte", "Speicherorte");
            let root = workspace::storage_root();
            let mut pick_root = false;
            let mut reset_root = false;
            grid(ui, "g-paths", &mut |ui| {
                ui.label("Speicherordner").on_hover_text("Neue Projekte, unbenannte Abfragen, frühere Fassungen, Verlauf, Diagramm-Anordnungen und gespeicherte Abfragen des Assistenten");
                ui.vertical(|ui| {
                    ui.label(RichText::new(root.display().to_string()).monospace());
                    ui.horizontal(|ui| {
                        if ui.button("Ändern …").clicked() {
                            pick_root = true;
                        }
                        if !s.storage_dir.is_empty() && ui.button("Standard").clicked() {
                            reset_root = true;
                        }
                        if ui.button("Öffnen").clicked() {
                            let _ = std::fs::create_dir_all(&root);
                            crate::app::open_path(&root);
                        }
                    });
                });
                ui.end_row();
                ui.label("Sicherungen");
                ui.vertical(|ui| {
                    ui.label(RichText::new(self.backup.dir().display().to_string()).monospace());
                    ui.horizontal(|ui| {
                        if ui.button("Ändern …").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                self.backup.dir = p.display().to_string();
                                self.backup.save();
                            }
                        }
                        if !self.backup.dir.is_empty() && ui.button("Standard").clicked() {
                            self.backup.dir.clear();
                            self.backup.save();
                        }
                        if ui.button("Öffnen").clicked() {
                            let d = self.backup.dir();
                            let _ = std::fs::create_dir_all(&d);
                            crate::app::open_path(&d);
                        }
                    });
                });
                ui.end_row();
                if let Some(p) = &cx.server.paths {
                    ui.label("Datenbanken (MariaDB)");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(p.data.display().to_string()).monospace());
                        if ui.button("Öffnen").clicked() {
                            crate::app::open_path(&p.data);
                        }
                    });
                    ui.end_row();
                }
                ui.label("Einstellungen und Log");
                ui.horizontal(|ui| {
                    let d = crate::server::app_data_dir();
                    ui.label(RichText::new(d.display().to_string()).monospace());
                    if ui.button("Öffnen").clicked() {
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
                    ui.label(format!("Neuer Speicherordner: {}", new.display()));
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
                        if ui.button("Inhalt mitnehmen").on_hover_text("Alles aus dem bisherigen Ordner in den neuen verschieben").clicked() {
                            res = Some(apply(true, s, cx.actions));
                        }
                        if ui.button("Nur umstellen").clicked() {
                            res = Some(apply(false, s, cx.actions));
                        }
                        if ui.button("Abbrechen").clicked() {
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

            self.heading(ui, "verbindung", "Verbindung");
            let (info, save_pw) = self.conn.get_or_insert_with(|| (s.conn.clone(), s.save_password));
            grid(ui, "g-conn", &mut |ui| {
                ui.label("Server");
                ui.text_edit_singleline(&mut info.host);
                ui.end_row();
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut info.port).range(1..=65535));
                ui.end_row();
                ui.label("Benutzer");
                ui.text_edit_singleline(&mut info.user);
                ui.end_row();
                ui.label("Passwort");
                ui.add(egui::TextEdit::singleline(&mut info.password).password(true));
                ui.end_row();
                ui.label("");
                ui.checkbox(save_pw, "Passwort speichern");
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Verbinden").clicked() {
                    s.conn = info.clone();
                    s.save_password = *save_pw;
                    cx.actions.push(Action::Connect);
                }
                if ui.button("Standard").on_hover_text("127.0.0.1, Port 3306, root, ohne Passwort").clicked() {
                    *info = ConnInfo::default();
                }
                let connected = cx.db.map(|d| d.info.label()).unwrap_or_else(|| "nicht verbunden".into());
                ui.label(RichText::new(connected).color(pal.text_weak));
            });

            self.heading(ui, "server", "Server");
            grid(ui, "g-server", &mut |ui| {
                ui.label("Anweisungen mitschreiben");
                ui.checkbox(&mut s.query_log, "jede Anweisung im Server-Log (auch von mysql in der Eingabeaufforderung)");
                ui.end_row();
                ui.label("Server-Log");
                if ui.button("Öffnen").clicked() {
                    cx.actions.push(Action::OpenLog);
                }
                ui.end_row();
            });

            self.heading(ui, "sicherungen", "Sicherungen");
            let old = self.backup.clone();
            grid(ui, "g-backup", &mut |ui| {
                ui.label("Automatisch sichern");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.backup.auto, "alle");
                    ui.add(egui::DragValue::new(&mut self.backup.interval_hours).range(1..=168).suffix(" h"));
                });
                ui.end_row();
                ui.label("Aufbewahren");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.backup.keep_count).range(1..=200));
                    ui.label("neueste, dazu eine je Tag für");
                    ui.add(egui::DragValue::new(&mut self.backup.keep_days).range(0..=365).suffix(" Tage"));
                });
                ui.end_row();
                ui.label("");
                if ui.button("Sicherungen & Reparatur").clicked() {
                    cx.actions.push(Action::OpenSafety);
                }
                ui.end_row();
            });
            if self.backup != old {
                self.backup.save();
            }

            self.heading(ui, "tastenkuerzel", "Tastenkürzel");
            self.keys_ui(ui);

            self.heading(ui, "vscode", "Visual Studio Code");
            ui.horizontal(|ui| {
                if ui.button("VS Code einrichten").on_hover_text("SQLTools mit MariaDB-Treiber installieren, Verbindung „EasyMySQL“ anlegen").clicked() {
                    cx.actions.push(Action::SetupVsCode);
                }
            });
            ui.add_space(30.0);
        });
        if *cx.settings != before {
            cx.actions.push(Action::SettingsChanged);
        }
    }
}
