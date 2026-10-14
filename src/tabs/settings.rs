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
    ("start", "Start"),
    ("updates", "Updates"),
    ("editor", "Editor"),
    ("maus", "Maus"),
    ("speicherorte", "Speicherorte"),
    ("dateien", "Dateien"),
    ("verbindung", "Verbindung"),
    ("server", "Server"),
    ("sicherungen", "Sicherungen"),
    ("tastenkuerzel", "Tastenkürzel"),
];

pub struct SettingsTab {
    goto: Option<String>,
    active_section: String,
    scroll_offset: Option<f32>,
    pinned_section: bool,
    headings: Vec<(String, f32)>,
    conn: Option<(ConnInfo, bool)>,
    ssh: Option<crate::ssh::SshConfig>,
    keymap: Keymap,
    key_filter: String,
    /// Aufzeichnung: (Befehl, Index der Belegung; None = neue hinzufuegen)
    record: Option<(Cmd, Option<usize>)>,
    backup: crate::backup::Config,
    /// Gespeicherte Server und der gerade bearbeitete (None = neuer Eintrag)
    servers: Vec<crate::profiles::Profile>,
    server_edit: Option<(Option<usize>, crate::profiles::Profile)>,
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
            ssh: None,
            keymap: keymap::current(),
            key_filter: String::new(),
            record: None,
            backup: crate::backup::Config::load(),
            servers: crate::profiles::all(),
            server_edit: None,
            pending_root: None,
        }
    }

    fn heading(&mut self, ui: &mut egui::Ui, id: &str, title: &str) {
        ui.add_space(22.0);
        let r = ui.label(RichText::new(title).size(18.0).strong());
        self.headings.push((id.to_string(), r.rect.top()));
        ui.add_space(6.0);
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
            ui.add(crate::widgets::edit(&mut self.key_filter).hint_text(crate::i18n::text("Befehl oder Taste suchen")).desired_width(240.0));
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
            ui.add_space(12.0);
            ui.label(RichText::new(crate::i18n::text(group)).strong().color(pal.text_weak));
            ui.add_space(4.0);
            crate::widgets::card().inner_margin(egui::Margin::symmetric(16, 4)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let n = visible.len();
                for (row, cmd) in visible.into_iter().enumerate() {
                    let top = ui.cursor().top();
                    ui.horizontal(|ui| {
                        ui.set_min_height(34.0);
                        ui.allocate_ui_with_layout(egui::vec2(210.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(cmd.label()).truncate());
                        });
                        let list = self.keymap.get(cmd).to_vec();
                        for (i, b) in list.iter().enumerate() {
                            let rec = self.record == Some((cmd, Some(i)));
                            let text = if rec { crate::i18n::text("Taste drücken …").to_string() } else { b.text() };
                            let conflicts = self.keymap.conflicts(cmd, b);
                            let state = if rec { crate::widgets::Keycap::Recording } else if !conflicts.is_empty() { crate::widgets::Keycap::Conflict } else { crate::widgets::Keycap::Normal };
                            let r = crate::widgets::keycap(ui, &text, state);
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
                        if rec_new {
                            crate::widgets::keycap(ui, crate::i18n::text("Taste drücken …"), crate::widgets::Keycap::Recording);
                        } else if crate::widgets::ghost_plus(ui, crate::i18n::text("Weiteres Kürzel")).clicked() {
                            self.record = Some((cmd, None));
                        }
                        if self.keymap.get(cmd) != cmd.defaults().as_slice() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if crate::icons::button(ui, crate::icons::Icon::Undo, crate::i18n::text("Standard")).clicked() {
                                    self.keymap.set(cmd, cmd.defaults());
                                    self.keymap.save();
                                    keymap::install(self.keymap.clone());
                                }
                            });
                        }
                    });
                    if row + 1 < n {
                        let y = ui.cursor().top();
                        let _ = top;
                        ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, pal.border.gamma_multiply(0.5)));
                    }
                }
            });
        }
    }
}

impl TabView for SettingsTab {
    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Settings)
    }

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
        if ui.available_width() < 700.0 {
            // Schmales Fenster: die Abschnitte als Reihe von Schaltflaechen ueber dem Inhalt
            let items: Vec<(String, String)> = SECTIONS.iter().map(|(id, name)| (id.to_string(), crate::i18n::text(name).to_string())).collect();
            let mut active = self.active_section.clone();
            if crate::widgets::chips(ui, &mut active, &items) {
                self.show_section(&active);
            }
            ui.add_space(6.0);
        } else {
            egui::Panel::left("settings-nav").resizable(false).show_separator_line(false).exact_size(190.0).frame(egui::Frame::new().fill(pal.sidebar_bg).corner_radius(style::RADIUS + 3).inner_margin(egui::Margin::symmetric(8, 12))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("settings-nav-scroll").show(ui, |ui| {
                    for (id, name) in SECTIONS {
                        if style::nav_item(ui, crate::i18n::text(name), self.active_section == *id, 0.0).clicked() {
                            self.show_section(id);
                        }
                    }
                });
            });
        }
        let jumping = self.goto.is_some();
        let mut offset = 0.0;
        crate::tabs::page_keys(ui);
        let body = egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
            offset = viewport.min.y;
            egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 24, top: 12, bottom: 12 }).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(760.0));
                let s = &mut *cx.settings;
                let grid = |ui: &mut egui::Ui, id: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                    crate::widgets::card().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 12.0;
                        let _ = id;
                        add(ui);
                    });

                };

                self.heading(ui, "darstellung", crate::i18n::text("Darstellung"));
                grid(ui, "g-look", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Sprache"), |ui| {
                        crate::widgets::segmented(ui, &mut s.language, &[(crate::i18n::Language::English, "English"), (crate::i18n::Language::German, "Deutsch")]);
                    });
                    crate::widgets::row(ui, crate::i18n::text("Design"), |ui| {
                        crate::widgets::segmented(ui, &mut s.dark, &[(true, crate::i18n::text("Dunkel")), (false, crate::i18n::text("Hell"))]);
                    });

                    crate::widgets::row(ui, crate::i18n::text("Seitenleiste"), |ui| {
                        crate::widgets::switch(ui, &mut s.sidebar, crate::i18n::text("Explorer, Suche, Datenbanken und Verlauf zeigen"));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Systemdatenbanken zeigen"), |ui| {
                        crate::widgets::switch(ui, &mut s.show_system_dbs, "information_schema, mysql, …");
                    });
                });

                self.heading(ui, "start", crate::i18n::text("Start"));
                grid(ui, "g-start", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Registerkarten"), |ui| {
                        crate::widgets::switch(ui, &mut s.restore_tabs, crate::i18n::text("Beim Start wieder öffnen"))
                            .on_hover_text(crate::i18n::text("Ungespeicherte Abfragen bleiben immer erhalten."));
                    });
                    if crate::platform::CAN_ASSOCIATE {
                        crate::widgets::row(ui, crate::i18n::text("Schließen"), |ui| {
                            crate::widgets::switch(ui, &mut s.background, crate::i18n::text("Im Infobereich weiterlaufen"))
                                .on_hover_text(crate::i18n::text("Sonst beendet das Schließen des Fensters EasyMySQL und den Server."));
                        });
                    }
                });

                self.heading(ui, "updates", "Updates");
                grid(ui, "g-update", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Installierte Version"), |ui| {
                        ui.label(RichText::new(env!("CARGO_PKG_VERSION")).monospace());
                    });
                    crate::widgets::row(ui, crate::i18n::text("Stand"), |ui| {
                        ui.vertical(|ui| {
                            use crate::updates::Phase;
                            let st = &cx.update;
                            let text = match st.phase {
                                Phase::Idle => crate::i18n::text("Die Suche läuft beim Start und danach alle sechs Stunden.").to_string(),
                                _ => st.text.clone(),
                            };
                            ui.label(RichText::new(text).color(match st.phase {
                                Phase::Failed => pal.error_text,
                                Phase::Installed => pal.ok_text,
                                _ => pal.text_weak,
                            }));
                            if st.phase == Phase::Downloading {
                                ui.add(egui::ProgressBar::new(st.progress).desired_width(260.0).desired_height(8.0).corner_radius(4));
                            }
                            ui.horizontal(|ui| match st.phase {
                                Phase::Installed => {
                                    if crate::widgets::primary(ui, crate::i18n::text("Jetzt neu starten")).clicked() {
                                        cx.actions.push(Action::UpdateRestart);
                                    }
                                }
                                Phase::Available => {
                                    if crate::widgets::primary(ui, crate::i18n::text("Installieren")).clicked() {
                                        cx.actions.push(Action::UpdateInstall);
                                    }
                                }
                                Phase::Checking | Phase::Downloading => {
                                    ui.spinner();
                                }
                                _ => {
                                    if ui.button(crate::i18n::text("Jetzt nach Updates suchen")).clicked() {
                                        cx.actions.push(Action::Run(Cmd::CheckUpdates));
                                    }
                                }
                            });
                        });
                    });
                    crate::widgets::row(ui, crate::i18n::text("Automatisch aktualisieren"), |ui| {
                        crate::widgets::switch(ui, &mut s.auto_install, crate::i18n::text("Updates im Hintergrund laden und installieren"))
                            .on_hover_text(crate::i18n::text("Aus: Neue Versionen werden gemeldet und erst nach Ihrer Zustimmung geladen. Nach dem Installieren fragt EasyMySQL, ob es neu starten soll."));
                    });
                });

                self.heading(ui, "editor", "Editor");
                grid(ui, "g-editor", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Schriftgröße"), |ui| {
                        ui.add(egui::Slider::new(&mut s.editor_font, crate::settings::MIN_FONT..=crate::settings::MAX_FONT).suffix(" px"));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Vorschläge beim Tippen"), |ui| {
                        crate::widgets::switch(ui, &mut s.auto_suggest, crate::i18n::text("Automatisch Vorschläge zeigen"))
                            .on_hover_text(crate::tr_format!("sonst nur mit {}", "otherwise only with {}", keymap::text(Cmd::Suggest)));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Speichern"), |ui| {
                        crate::widgets::switch(ui, &mut s.autosave, crate::i18n::text("Dateien automatisch speichern"))
                            .on_hover_text(crate::tr_format!("sonst nur mit {}; beim Schließen wird dann nachgefragt", "otherwise only with {}; closing then asks first", keymap::text(Cmd::Save)));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Fehler im SQL"), |ui| {
                        crate::widgets::switch(ui, &mut s.lint, crate::i18n::text("Tippfehler und unbekannte Namen markieren"))
                            .on_hover_text(crate::tr_format!("Mit {} werden sie automatisch korrigiert", "{} corrects them automatically", keymap::text(Cmd::FixAll)));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Klammern schließen"), |ui| {
                        crate::widgets::switch(ui, &mut s.auto_close, crate::i18n::text("( [ { ' \" ` automatisch schließen"));
                    });
                });

                self.heading(ui, "maus", crate::i18n::text("Maus"));
                grid(ui, "g-mouse", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Scrollgeschwindigkeit"), |ui| {
                        ui.add(egui::Slider::new(&mut s.scroll_speed, 0.5..=5.0).step_by(0.25).suffix("×"));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Mittlere Maustaste"), |ui| {
                        crate::widgets::switch(ui, &mut s.autoscroll, crate::i18n::text("gedrückt halten und ziehen = schnell scrollen"));
                    });
                });

                self.heading(ui, "speicherorte", crate::i18n::text("Speicherorte"));
                let root = workspace::storage_root();
                let mut pick_root = false;
                let mut reset_root = false;
                grid(ui, "g-paths", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Speicherordner"), |ui| {
                        ui.vertical(|ui| {
                            { let t = root.display().to_string(); ui.add(egui::Label::new(RichText::new(&t).monospace()).truncate()).on_hover_text(&t); }
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
                    });
                    crate::widgets::row(ui, crate::i18n::text("Sicherungen"), |ui| {
                        ui.vertical(|ui| {
                            { let t = self.backup.dir().display().to_string(); ui.add(egui::Label::new(RichText::new(&t).monospace()).truncate()).on_hover_text(&t); }
                            ui.horizontal(|ui| {
                                if ui.button(crate::i18n::text("Ändern …")).clicked()
                                    && let Some(p) = rfd::FileDialog::new().pick_folder() {
                                        self.backup.dir = p.display().to_string();
                                        self.backup.save();
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
                    });
                    if let Some(p) = &cx.server.paths {
                        crate::widgets::row(ui, crate::i18n::text("Datenbanken (MariaDB)"), |ui| {
                            if path_with_open(ui, &p.data.display().to_string()) {
                                crate::app::open_path(&p.data);
                            }
                        });
                    }
                    crate::widgets::row(ui, crate::i18n::text("Einstellungen und Log"), |ui| {
                        let d = crate::server::app_data_dir();
                        if path_with_open(ui, &d.display().to_string()) {
                            crate::app::open_path(&d);
                        }
                    });
                });
                if pick_root
                    && let Some(p) = rfd::FileDialog::new().set_directory(&root).pick_folder()
                        && p != root {
                            self.pending_root = Some(p);
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
                            if ui.button(crate::i18n::text("Inhalt mitnehmen")).clicked() {
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

                self.heading(ui, "dateien", crate::i18n::text("Dateien"));
                let mut associate = false;
                grid(ui, "g-files", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("SQL-Dateien (.sql)"), |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(crate::i18n::text("Ein Doppelklick auf eine .sql-Datei öffnet sie in EasyMySQL.")).color(style::pal().text_weak));
                            if crate::platform::CAN_ASSOCIATE && ui.button(crate::i18n::text("Mit EasyMySQL verknüpfen")).clicked() {
                                associate = true;
                            }
                        });
                    });
                });
                if associate {
                    match crate::platform::register_sql_association() {
                        Ok(()) => cx.actions.push(Action::Status(crate::i18n::text("Verknüpft. Falls Windows ein anderes Programm bevorzugt, hier EasyMySQL für .sql wählen.").into())),
                        Err(e) => cx.actions.push(Action::Error(e)),
                    }
                }

                self.heading(ui, "verbindung", crate::i18n::text("Verbindung"));
                let (info, save_pw) = self.conn.get_or_insert_with(|| (s.conn.clone(), s.save_password));
                grid(ui, "g-conn", &mut |ui| {
                    crate::widgets::row(ui, "Server", |ui| {
                        ui.add(crate::widgets::edit(&mut info.host));
                    });
                    crate::widgets::row(ui, "Port", |ui| {
                        ui.add(egui::DragValue::new(&mut info.port).range(1..=65535));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Benutzer"), |ui| {
                        ui.add(crate::widgets::edit(&mut info.user));
                    });
                    crate::widgets::row(ui, crate::i18n::text("Passwort"), |ui| {
                        ui.add(crate::widgets::edit(&mut info.password).password(true));
                    });
                    crate::widgets::row(ui, "", |ui| {
                        crate::widgets::switch(ui, save_pw, crate::i18n::text("Passwort speichern"));
                    });
                });
                let ssh = self.ssh.get_or_insert_with(|| s.ssh.clone());
                ssh_form(ui, "g-ssh", ssh);
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::text("Verbinden")).clicked() {
                        s.conn = info.clone();
                        s.save_password = *save_pw;
                        s.ssh = ssh.clone();
                        cx.actions.push(Action::Connect);
                    }
                    if ui.button(crate::i18n::text("Standard")).clicked() {
                        *info = ConnInfo::default();
                        *ssh = Default::default();
                    }
                    let connected = cx.db.map(|d| d.info.label()).unwrap_or_else(|| crate::i18n::text("nicht verbunden").into());
                    ui.label(RichText::new(connected).color(pal.text_weak));
                });

                ui.add_space(14.0);
                ui.label(RichText::new(crate::i18n::tr("Weitere Server", "More servers")).strong());
                ui.label(RichText::new(crate::i18n::tr("Jede SQL-Datei kann einen dieser Server wählen. Ohne Auswahl gilt der Server oben.", "Each SQL file can pick one of these servers. Without a choice the server above is used.")).small().color(pal.text_weak));
                ui.add_space(4.0);
                let mut delete = None;
                let mut edit = None;
                for (i, p) in self.servers.iter().enumerate() {
                    crate::widgets::card().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&p.name).strong());
                                ui.label(RichText::new(p.label()).small().color(pal.text_weak));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if crate::icons::button(ui, crate::icons::Icon::Trash, crate::i18n::text("Löschen")).clicked() {
                                    delete = Some(i);
                                }
                                if ui.button(crate::i18n::tr("Bearbeiten", "Edit")).clicked() {
                                    edit = Some(i);
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = edit {
                    self.server_edit = Some((Some(i), self.servers[i].clone()));
                }
                if let Some(i) = delete {
                    self.servers.remove(i);
                    crate::profiles::set(self.servers.clone(), s.save_password);
                    self.server_edit = None;
                }
                if let Some((index, p)) = &mut self.server_edit {
                    let mut save = false;
                    let mut cancel = false;
                    let taken = self.servers.iter().enumerate().any(|(i, o)| Some(i) != *index && o.name == p.name.trim());
                    crate::widgets::card().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 10.0;
                        crate::widgets::row(ui, crate::i18n::text("Name"), |ui| {
                            ui.add_enabled(index.is_none(), crate::widgets::edit(&mut p.name).hint_text(crate::i18n::tr("z. B. Produktion", "e.g. Production")));
                        });
                        crate::widgets::row(ui, "Server", |ui| {
                            ui.add(crate::widgets::edit(&mut p.conn.host));
                        });
                        crate::widgets::row(ui, "Port", |ui| {
                            ui.add(egui::DragValue::new(&mut p.conn.port).range(1..=65535));
                        });
                        crate::widgets::row(ui, crate::i18n::text("Benutzer"), |ui| {
                            ui.add(crate::widgets::edit(&mut p.conn.user));
                        });
                        crate::widgets::row(ui, crate::i18n::text("Passwort"), |ui| {
                            ui.add(crate::widgets::edit(&mut p.conn.password).password(true));
                        });
                        ssh_form(ui, "g-srv-ssh", &mut p.ssh);
                        if taken {
                            ui.label(RichText::new(crate::i18n::tr("Diesen Namen gibt es schon.", "This name already exists.")).small().color(pal.error_text));
                        }
                        ui.horizontal(|ui| {
                            let ok = !p.name.trim().is_empty() && !taken && !p.conn.host.trim().is_empty();
                            if ui.add_enabled(ok, egui::Button::new(crate::i18n::text("Speichern"))).clicked() {
                                save = true;
                            }
                            if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                                cancel = true;
                            }
                            if !s.save_password {
                                ui.label(RichText::new(crate::i18n::tr("Passwörter werden nur gespeichert, wenn oben „Passwort speichern“ an ist.", "Passwords are only saved when “Save password” above is on.")).small().color(pal.text_weak));
                            }
                        });
                    });
                    if save {
                        let mut p = p.clone();
                        p.name = p.name.trim().to_string();
                        match *index {
                            Some(i) => self.servers[i] = p,
                            None => self.servers.push(p),
                        }
                        crate::profiles::set(self.servers.clone(), s.save_password);
                        self.server_edit = None;
                    } else if cancel {
                        self.server_edit = None;
                    }
                } else if ui.button(crate::i18n::tr("+ Server hinzufügen", "+ Add server")).clicked() {
                    let mut p = crate::profiles::Profile::new("");
                    p.conn.host = String::new();
                    self.server_edit = Some((None, p));
                }

                self.heading(ui, "server", "Server");
                grid(ui, "g-server", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Anweisungen mitschreiben"), |ui| {
                        crate::widgets::switch(ui, &mut s.query_log, crate::i18n::text("jede Anweisung im Server-Log"));
                    });
                });

                self.heading(ui, "sicherungen", crate::i18n::text("Sicherungen"));
                let old = self.backup.clone();
                grid(ui, "g-backup", &mut |ui| {
                    crate::widgets::row(ui, crate::i18n::text("Automatisch sichern"), |ui| {
                        ui.horizontal(|ui| {
                            crate::widgets::switch(ui, &mut self.backup.auto, crate::i18n::text("alle"));
                            ui.add(egui::DragValue::new(&mut self.backup.interval_hours).range(1..=168).suffix(" h"));
                        });
                    });
                    crate::widgets::row(ui, crate::i18n::text("Aufbewahren"), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.backup.keep_count).range(1..=200));
                            ui.label(crate::i18n::text("neueste, dazu eine je Tag für"));
                            ui.add(egui::DragValue::new(&mut self.backup.keep_days).range(0..=365).suffix(crate::i18n::text(" Tage")));
                        });
                    });
                });
                if self.backup != old {
                    self.backup.save();
                }

                self.heading(ui, "tastenkuerzel", crate::i18n::text("Tastenkürzel"));
                self.keys_ui(ui);
                ui.add_space(30.0);
            });
        });
        // Nach einem Sprung die angeklickte Kategorie behalten, auch wenn der
        // letzte Abschnitt wegen der Fensterhoehe nicht bis nach oben scrollt.
        if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| egui::Rect::from_min_max(body.inner_rect.min, body.inner_rect.max + egui::vec2(16.0, 0.0)).contains(p))
            && (i.smooth_scroll_delta != egui::Vec2::ZERO || i.pointer.any_down())) {
            self.pinned_section = false;
        }
        if !jumping && !self.pinned_section && self.scroll_offset != Some(offset)
            && let Some((id, _)) = self.headings.iter().rev().find(|(_, y)| *y <= body.inner_rect.top() + 36.0).or(self.headings.first()) {
                self.active_section = id.clone();
            }
        self.scroll_offset = Some(offset);
        self.pinned_section |= jumping;
        if *cx.settings != before {
            cx.actions.push(Action::SettingsChanged);
        }
    }
}

/// Pfad (gekuerzt, wenn er nicht passt) mit "Oeffnen" darunter; true bei Klick auf "Oeffnen"
fn path_with_open(ui: &mut egui::Ui, path: &str) -> bool {
    ui.vertical(|ui| {
        ui.add(egui::Label::new(RichText::new(path).monospace()).truncate()).on_hover_text(path);
        ui.button(crate::i18n::text("Öffnen")).clicked()
    })
    .inner
}

/// SSH-Tunnel-Einstellungen: Schalter und, wenn an, die Felder
fn ssh_form(ui: &mut egui::Ui, id: &str, ssh: &mut crate::ssh::SshConfig) {
    crate::widgets::switch(ui, &mut ssh.enabled, crate::i18n::tr("Über SSH-Tunnel verbinden", "Connect through an SSH tunnel")).on_hover_text(crate::i18n::tr(
        "Server und Port oben gelten dann aus Sicht des SSH-Servers (meist 127.0.0.1).",
        "Server and port above then apply as seen from the SSH server (usually 127.0.0.1).",
    ));
    if !ssh.enabled {
        return;
    }
    ui.add_space(4.0);
    ui.push_id(id, |ui| {
        crate::widgets::row(ui, crate::i18n::tr("SSH-Server", "SSH server"), |ui| {
            ui.add(crate::widgets::edit(&mut ssh.host));
        });
        crate::widgets::row(ui, "SSH-Port", |ui| {
            ui.add(egui::DragValue::new(&mut ssh.port).range(1..=65535));
        });
        crate::widgets::row(ui, crate::i18n::text("Benutzer"), |ui| {
            ui.add(crate::widgets::edit(&mut ssh.user));
        });
        crate::widgets::row(ui, crate::i18n::tr("Schlüsseldatei", "Key file"), |ui| {
            ui.horizontal(|ui| {
                ui.add(crate::widgets::edit(&mut ssh.key).hint_text(crate::i18n::tr("leer = ssh-Agent", "empty = ssh agent")));
                if crate::icons::button(ui, crate::icons::Icon::File, crate::i18n::tr("Datei wählen …", "Choose file …")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                {
                    ssh.key = p.to_string_lossy().into_owned();
                }
            });
        });
        crate::widgets::row(ui, crate::i18n::text("Passwort"), |ui| {
            ui.add(crate::widgets::edit(&mut ssh.password).password(true).hint_text(crate::i18n::tr("nur ohne Schlüssel", "only without a key")));
        });
    });
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
                                history: &[],
                                update: Default::default(),
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
        tab.show_section("tastenkuerzel");
        for _ in 0..5 {
            let _ = render(&mut tab, vec![]);
        }
        assert_eq!(tab.active_section, "tastenkuerzel", "Ein Klick muss auch den letzten Abschnitt dauerhaft markieren");
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
