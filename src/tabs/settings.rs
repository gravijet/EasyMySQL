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
    ("abfragen", "Abfragen und Daten"),
    ("ausfuehren", "Ausführen"),
    ("java", "Java"),
    ("python", "Python"),
    ("javascript", "JavaScript"),
    ("sprachserver", "Sprachserver"),
    ("git", "Git & GitHub"),
    ("terminal", "Terminal"),
    ("maus", "Maus"),
    ("speicherorte", "Speicherorte"),
    ("dateien", "Dateien"),
    ("verbindung", "Verbindung"),
    ("server", "Server"),
    ("sicherungen", "Sicherungen"),
    ("ki", "KI-Assistenten"),
    ("tastenkuerzel", "Tastenkürzel"),
];

/// Gruppen der Navigation (wie in IntelliJ IDEA)
const GROUPS: &[(&str, &[&str])] = &[
    ("Allgemein", &["darstellung", "start", "updates", "maus", "dateien", "speicherorte"]),
    ("Editor", &["editor", "abfragen", "tastenkuerzel"]),
    ("Programmieren", &["ausfuehren", "java", "python", "javascript", "sprachserver", "git", "terminal"]),
    ("Datenbank", &["verbindung", "server", "sicherungen", "ki"]),
];

fn section_icon(id: &str) -> crate::icons::Icon {
    use crate::icons::Icon;
    match id {
        "darstellung" => Icon::Sparkles,
        "start" => Icon::Play,
        "updates" => Icon::CloudDownload,
        "editor" => Icon::Edit,
        "abfragen" => Icon::Table,
        "ausfuehren" => Icon::Terminal,
        "java" => Icon::Coffee,
        "python" | "javascript" => Icon::Script,
        "sprachserver" => Icon::Braces,
        "git" => Icon::GitBranch,
        "terminal" => Icon::Terminal,
        "maus" => Icon::ArrowUp,
        "speicherorte" => Icon::Folder,
        "dateien" => Icon::File,
        "verbindung" => Icon::Link,
        "server" => Icon::Database,
        "sicherungen" => Icon::Shield,
        "tastenkuerzel" => Icon::Key,
        "ki" => Icon::Sparkles,
        _ => Icon::Settings,
    }
}

/// Eintrag der Navigation: Symbol und Text, gewaehlt in der Auswahlfarbe
fn nav_row(ui: &mut egui::Ui, icon: crate::icons::Icon, title: &str, active: bool) -> egui::Response {
    let pal = style::pal();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    if active || resp.hovered() {
        ui.painter().rect_filled(r, style::RADIUS as f32, if active { pal.selection } else { pal.hover });
    }
    let fg = if active && pal.dark { egui::Color32::WHITE } else { pal.text };
    let ir = egui::Rect::from_center_size(egui::pos2(r.left() + 18.0, r.center().y), egui::vec2(15.0, 15.0));
    // Sprachen mit ihrem Dateisymbol (wie im Explorer)
    match title {
        "Java" => crate::icons::paint_file(ui.painter(), ir, "x.java"),
        "Python" => crate::icons::paint_file(ui.painter(), ir, "x.py"),
        "JavaScript" => crate::icons::paint_file(ui.painter(), ir, "x.js"),
        _ => crate::icons::paint(ui.painter(), ir, icon, if active { fg } else { pal.text_weak }),
    }
    ui.painter().text(egui::pos2(r.left() + 34.0, r.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.0), fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub struct SettingsTab {
    active_section: String,
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
    /// Neuer Datenordner des Servers, wartet auf "Daten mitnehmen?" (None im Inneren = Standard)
    pending_data: Option<Option<std::path::PathBuf>>,
    /// Laufende Einrichtung in Claude Code/Codex und ihr letztes Ergebnis (ok?, Text)
    ai_job: Option<std::sync::mpsc::Receiver<Vec<(bool, String)>>>,
    ai_report: Vec<(bool, String)>,
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            active_section: "darstellung".into(),
            conn: None,
            ssh: None,
            keymap: keymap::current(),
            key_filter: String::new(),
            record: None,
            backup: crate::backup::Config::load(),
            servers: crate::profiles::all(),
            server_edit: None,
            pending_root: None,
            pending_data: None,
            ai_job: None,
            ai_report: Vec::new(),
        }
    }

    /// Titel der Seite mit Symbol
    fn heading(&mut self, ui: &mut egui::Ui, id: &str, title: &str) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
            crate::icons::paint(ui.painter(), r, section_icon(id), style::pal().accent);
            ui.add_space(4.0);
            ui.label(RichText::new(title).size(19.0));
        });
        ui.add_space(12.0);
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
        self.active_section = id.to_string();
    }

    fn on_close(&mut self) -> bool {
        RECORDING.store(false, Ordering::Relaxed);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let before = cx.settings.clone();
        if ui.available_width() < 700.0 {
            // Schmales Fenster: die Abschnitte als Reihe von Schaltflaechen ueber dem Inhalt
            let items: Vec<(String, String)> = SECTIONS.iter().map(|(id, name)| (id.to_string(), crate::i18n::text(name).to_string())).collect();
            let mut active = self.active_section.clone();
            if crate::widgets::chips(ui, &mut active, &items) {
                self.show_section(&active);
            }
            ui.add_space(6.0);
        } else {
            // Wie der Einstellungsdialog von IntelliJ IDEA: Gruppen links, rechts nur die gewaehlte Seite
            egui::Panel::left("settings-nav").resizable(false).show_separator_line(false).exact_size(210.0).frame(egui::Frame::new().fill(pal.sidebar_bg).corner_radius(style::RADIUS + 3).inner_margin(egui::Margin::symmetric(8, 10))).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("settings-nav-scroll").show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (group, ids) in GROUPS {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.label(RichText::new(crate::i18n::text(group).to_uppercase()).size(10.5).color(pal.text_weak));
                        });
                        ui.add_space(2.0);
                        for id in *ids {
                            let name = SECTIONS.iter().find(|(i, _)| i == id).map(|(_, n)| crate::i18n::text(n)).unwrap_or(id);
                            if nav_row(ui, section_icon(id), name, self.active_section == *id).clicked() {
                                self.show_section(id);
                            }
                        }
                    }
                });
            });
        }
        crate::tabs::page_keys(ui);
        // Eigene Bildlaufposition je Seite
        egui::ScrollArea::vertical().id_salt(("settings", self.active_section.clone())).auto_shrink([false, false]).show(ui, |ui| {
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

                let ctx = ui.ctx().clone();
                let page = self.active_section.clone();
                let page = page.as_str();
                if page == "darstellung" {
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
                }
                if page == "start" {
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
                }
                if page == "updates" {
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
                                    Phase::Idle => crate::tr_format!("Die Suche läuft beim Start und danach alle {} Stunden.", "The check runs at startup and every {} hours after that.", s.app_update_hours),
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
                        crate::widgets::row(ui, crate::i18n::tr("Suchen alle", "Check every"), |ui| {
                            crate::widgets::drag(ui, &mut s.app_update_hours, 1..=720, crate::i18n::tr(" Stunden", " hours"));
                        });
                        crate::widgets::row(ui, crate::i18n::text("Automatisch aktualisieren"), |ui| {
                            crate::widgets::switch(ui, &mut s.auto_install, crate::i18n::text("Updates im Hintergrund laden und installieren"))
                                .on_hover_text(crate::i18n::text("Aus: Neue Versionen werden gemeldet und erst nach Ihrer Zustimmung geladen. Nach dem Installieren fragt EasyMySQL, ob es neu starten soll."));
                        });
                    });
                }
                if page == "editor" {
                    self.heading(ui, "editor", "Editor");
                    grid(ui, "g-editor", &mut |ui| {
                        crate::widgets::row(ui, crate::i18n::text("Schriftgröße"), |ui| {
                            crate::widgets::slider(ui, &mut s.editor_font, crate::settings::MIN_FONT..=crate::settings::MAX_FONT, 1.0, " px");
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
                        editor_settings(ui, s);
                    });
                }
                if page == "abfragen" {
                    self.heading(ui, "abfragen", crate::i18n::tr("Abfragen und Daten", "Queries and data"));
                    grid(ui, "g-data", &mut |ui| {
                        let pal = style::pal();
                        crate::widgets::row(ui, crate::i18n::tr("Zeilen je Ergebnis", "Rows per result"), |ui| {
                            ui.horizontal(|ui| {
                                crate::widgets::drag(ui, &mut s.result_max_rows, 100..=10_000_000, "");
                                ui.label(RichText::new(crate::i18n::tr("mehr werden nicht geladen (der Hinweis zeigt, dass es mehr gibt)", "more are not loaded (a note shows that there are more)")).small().color(pal.text_weak));
                            });
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Zeilen je Seite", "Rows per page"), |ui| {
                            ui.horizontal(|ui| {
                                crate::widgets::drag(ui, &mut s.data_page_rows, 10..=100_000, "");
                                ui.label(RichText::new(crate::i18n::tr("in der Datenansicht einer Tabelle", "in the data view of a table")).small().color(pal.text_weak));
                            });
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Verlauf", "History"), |ui| {
                            crate::widgets::drag(ui, &mut s.history_max, 10..=1_000_000, crate::i18n::tr(" Anweisungen", " statements"));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Vor dem Löschen", "Before deleting"), |ui| {
                            crate::widgets::switch(ui, &mut s.backup_before_delete, crate::i18n::tr("Betroffene Tabellen sichern (DROP, TRUNCATE, DELETE ohne WHERE)", "Back up the affected tables (DROP, TRUNCATE, DELETE without WHERE)"));
                        });
                    });
                }
                if page == "ausfuehren" {
                    self.heading(ui, "ausfuehren", crate::i18n::tr("Ausführen", "Running programs"));
                    grid(ui, "g-run", &mut |ui| run_settings(ui, &ctx, s));
                }
                if page == "java" {
                    self.heading(ui, "java", "Java");
                    grid(ui, "g-java", &mut |ui| java_settings(ui, &ctx, s));
                }
                if page == "python" {
                    self.heading(ui, "python", "Python");
                    grid(ui, "g-python", &mut |ui| python_settings(ui, &ctx, s));
                }
                if page == "javascript" {
                    self.heading(ui, "javascript", "JavaScript & TypeScript");
                    grid(ui, "g-js", &mut |ui| node_settings(ui, &ctx, s));
                }
                if page == "sprachserver" {
                    self.heading(ui, "sprachserver", crate::i18n::tr("Sprachserver", "Language servers"));
                    grid(ui, "g-lsp", &mut |ui| lsp_settings(ui, &ctx, s));
                }
                if page == "git" {
                    self.heading(ui, "git", "Git & GitHub");
                    let actions = &mut *cx.actions;
                    grid(ui, "g-git", &mut |ui| git_settings(ui, &ctx, s, actions));
                }
                if page == "terminal" {
                    self.heading(ui, "terminal", "Terminal");
                    let profiles = crate::terminal::profiles(None, &s.conn.clone());
                    grid(ui, "g-term", &mut |ui| terminal_settings(ui, s, &profiles));
                }
                if page == "maus" {
                    self.heading(ui, "maus", crate::i18n::text("Maus"));
                    grid(ui, "g-mouse", &mut |ui| {
                        crate::widgets::row(ui, crate::i18n::text("Scrollgeschwindigkeit"), |ui| {
                            crate::widgets::slider(ui, &mut s.scroll_speed, 0.5..=5.0, 0.25, "×");
                        });
                        crate::widgets::row(ui, crate::i18n::text("Mittlere Maustaste"), |ui| {
                            crate::widgets::switch(ui, &mut s.autoscroll, crate::i18n::text("gedrückt halten und ziehen = schnell scrollen"));
                        });
                    });
                }
                if page == "speicherorte" {
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
                        if let Some(p) = &cx.server.paths() {
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
                }
                if page == "dateien" {
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
                }
                if page == "verbindung" {
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
                }
                if page == "server" {
                    self.heading(ui, "server", "Server");
                    let server = cx.server;
                    let actions = &mut *cx.actions;
                    let pending_data = &mut self.pending_data;
                    grid(ui, "g-server", &mut |ui| {
                        server_settings(ui, s, server, actions, pending_data);
                        crate::widgets::row(ui, crate::i18n::text("Anweisungen mitschreiben"), |ui| {
                            crate::widgets::switch(ui, &mut s.query_log, crate::i18n::text("jede Anweisung im Server-Log"));
                        });
                    });
                }
                if page == "sicherungen" {
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
                }
                if page == "ki" {
                    self.heading(ui, "ki", crate::i18n::text("KI-Assistenten"));
                    ui.label(RichText::new(crate::i18n::tr(
                        "Claude Code, Codex und andere KI-Assistenten arbeiten über den MCP-Server von EasyMySQL mit Ihren Datenbanken: Schema lesen, Abfragen ausführen und – wenn erlaubt – Daten ändern.",
                        "Claude Code, Codex and other AI assistants work with your databases through the EasyMySQL MCP server: they read the schema, run queries and – if allowed – change data.",
                    )).color(pal.text_weak));
                    ui.add_space(8.0);
                    grid(ui, "g-ai", &mut |ui| {
                        crate::widgets::row(ui, crate::i18n::tr("Änderungen", "Changes"), |ui| {
                            crate::widgets::switch(ui, &mut s.ai.allow_write, crate::i18n::tr("KI darf Daten und Struktur ändern", "AI may change data and structure"))
                                .on_hover_text(crate::i18n::tr(
                                    "Aus: nur lesen (SELECT, SHOW, EXPLAIN …), jede Abfrage in einer READ-ONLY-Transaktion. An: vor DROP, TRUNCATE und DELETE/UPDATE ohne WHERE sichert EasyMySQL die betroffenen Datenbanken.",
                                    "Off: read only (SELECT, SHOW, EXPLAIN …), every query in a READ ONLY transaction. On: before DROP, TRUNCATE and DELETE/UPDATE without WHERE, EasyMySQL backs up the affected databases.",
                                ));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Standard-Datenbank", "Default database"), |ui| {
                            ui.add(egui::TextEdit::singleline(&mut s.ai.database).hint_text(crate::i18n::tr("keine", "none")).desired_width(220.0));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Zeilen je Abfrage", "Rows per query"), |ui| {
                            crate::widgets::drag(ui, &mut s.ai.max_rows, 1..=5000, "");
                        });
                    });
                    ui.add_space(8.0);
                    if let Some(rx) = &self.ai_job {
                        match rx.try_recv() {
                            Ok(r) => {
                                self.ai_report = r;
                                self.ai_job = None;
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.ai_job = None,
                            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(200)),
                        }
                    }
                    grid(ui, "g-ai-setup", &mut |ui| {
                        use crate::mcp::setup::{Target, available};
                        crate::widgets::row(ui, crate::i18n::tr("Eintragen in", "Register in"), |ui| {
                            ui.horizontal(|ui| {
                                let busy = self.ai_job.is_some();
                                let mut start: Option<(Vec<Target>, bool)> = None;
                                for t in [Target::Claude, Target::Codex] {
                                    let label = if available(t) { t.label().to_string() } else { crate::tr_format!("{} (nicht gefunden)", "{} (not found)", t.label()) };
                                    if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                                        start = Some((vec![t], false));
                                    }
                                }
                                let remove = ui
                                    .add_enabled(!busy, egui::Button::new(crate::i18n::tr("Entfernen", "Remove")))
                                    .on_hover_text(crate::i18n::tr("Eintrag und Skill aus beiden Assistenten entfernen", "Remove the entry and skill from both assistants"));
                                if remove.clicked() {
                                    start = Some((vec![Target::Claude, Target::Codex], true));
                                }
                                if busy {
                                    ui.spinner();
                                }
                                if let Some((targets, remove)) = start {
                                    let (tx, rx) = std::sync::mpsc::channel();
                                    let ctx = ui.ctx().clone();
                                    std::thread::spawn(move || {
                                        let r = crate::mcp::setup::apply(&targets, remove)
                                            .into_iter()
                                            .map(|(t, r)| match r {
                                                Ok(m) => (true, format!("{}: {m}", t.label())),
                                                Err(e) => (false, format!("{}: {e}", t.label())),
                                            })
                                            .collect();
                                        let _ = tx.send(r);
                                        ctx.request_repaint();
                                    });
                                    self.ai_job = Some(rx);
                                }
                            });
                        });
                        for (ok, text) in &self.ai_report {
                            ui.label(RichText::new(format!("{} {text}", if *ok { "✓" } else { "✗" })).color(if *ok { pal.ok_text } else { pal.error_text }));
                        }
                        crate::widgets::row(ui, crate::i18n::tr("Andere Programme", "Other clients"), |ui| {
                            let cmd = format!("\"{}\" mcp", crate::updates::exe_path().display());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&cmd).monospace().size(12.0));
                                if ui.small_button(crate::i18n::tr("Kopieren", "Copy")).clicked() {
                                    ui.ctx().copy_text(cmd.clone());
                                }
                            })
                            .response
                            .on_hover_text(crate::i18n::tr("Befehl für jeden MCP-Client (stdio), etwa Cursor, VS Code oder Claude Desktop.", "Command for any MCP client (stdio), e.g. Cursor, VS Code or Claude Desktop."));
                        });
                    });
                }
                if page == "tastenkuerzel" {
                    self.heading(ui, "tastenkuerzel", crate::i18n::text("Tastenkürzel"));
                    self.keys_ui(ui);
                }
                ui.add_space(30.0);
            });
        });
        if *cx.settings != before {
            cx.actions.push(Action::SettingsChanged);
        }
    }
}

/// Ist der Port schon belegt? Das Ergebnis gilt, bis ein anderer Port eingetragen wird (nicht in jedem Bild pruefen).
fn port_taken(port: u16) -> bool {
    static LAST: std::sync::Mutex<Option<(u16, bool)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    match *last {
        Some((p, taken)) if p == port => taken,
        _ => {
            let taken = crate::server::port_open(port);
            *last = Some((port, taken));
            taken
        }
    }
}

/// Abschnitt Server: Port, Erreichbarkeit, Datenordner, Speicher und eigene my.ini-Zeilen des eigenen Servers
fn server_settings(ui: &mut egui::Ui, s: &mut crate::settings::Settings, server: &crate::server::Server, actions: &mut Vec<Action>, pending_data: &mut Option<Option<std::path::PathBuf>>) {
    let pal = style::pal();
    let weak = |ui: &mut egui::Ui, t: &str| {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(t).small().color(pal.text_weak)).wrap());
        });
    };
    // Kurzer Hinweis neben einem Feld (mittig zur Zeile)
    let inline = |ui: &mut egui::Ui, t: &str| {
        ui.label(RichText::new(t).small().color(pal.text_weak));
    };
    let cfg = &mut s.server;
    crate::widgets::row(ui, crate::i18n::tr("Automatisch starten", "Start automatically"), |ui| {
        crate::widgets::switch(ui, &mut s.server_autostart, crate::i18n::tr("beim Start von EasyMySQL", "when EasyMySQL starts"))
            .on_hover_text(crate::i18n::tr("Aus: Den Server startet man über Server → Starten.", "Off: start the server through Server → Start."));
    });
    crate::widgets::row(ui, "Port", |ui| {
        ui.horizontal(|ui| {
            crate::widgets::number_field(ui, &mut cfg.port, 1..=65535);
            if cfg.port != crate::server::DEFAULT_PORT && ui.small_button(crate::i18n::text("Standard")).clicked() {
                cfg.port = crate::server::DEFAULT_PORT;
            }
            if cfg.port != server.port() && port_taken(cfg.port) {
                ui.label(RichText::new(crate::tr_format!("Port {} ist schon belegt.", "Port {} is already in use.", cfg.port)).small().color(pal.warn_text));
            }
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Erreichbar", "Reachable"), |ui| {
        crate::widgets::segmented(ui, &mut cfg.network, &[(false, crate::i18n::tr("Nur dieser PC", "This PC only")), (true, crate::i18n::tr("Im Netzwerk", "On the network"))]);
    });
    if cfg.network {
        crate::widgets::row(ui, "", |ui| {
            weak(ui, crate::i18n::tr(
                "Andere Rechner brauchen einen eigenen Benutzer mit Host % und Passwort (Server → Benutzer & Rechte); root bleibt auf diesen PC beschränkt. Die Firewall fragt beim nächsten Start eventuell nach.",
                "Other computers need their own user with host % and a password (Server → Users & privileges); root stays limited to this PC. The firewall may ask on the next start.",
            ));
        });
    }
    crate::widgets::row(ui, crate::i18n::tr("Datenordner", "Data folder"), |ui| {
        ui.vertical(|ui| {
            let current = server.paths().map(|p| p.data.display().to_string()).unwrap_or_default();
            ui.add(egui::Label::new(RichText::new(&current).monospace()).truncate()).on_hover_text(&current);
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::text("Ändern …")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                    && Some(&p) != cfg.data_dir.as_ref()
                {
                    *pending_data = Some(Some(p));
                }
                if cfg.data_dir.is_some() && ui.button(crate::i18n::text("Standard")).clicked() {
                    *pending_data = Some(None);
                }
                if ui.button(crate::i18n::text("Öffnen")).clicked()
                    && let Some(p) = server.paths()
                {
                    crate::app::open_path(&p.data);
                }
            });
        });
    });
    if let Some(target) = pending_data.clone() {
        let dir = target.clone().unwrap_or_else(|| crate::server::app_data_dir().join("data"));
        let existing = dir.join("mysql").is_dir();
        crate::widgets::row(ui, "", |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(crate::tr_format!("Neuer Datenordner: {}", "New data folder: {}", dir.display()));
                    if existing {
                        weak(ui, crate::i18n::tr("Dort liegen schon Datenbanken. EasyMySQL startet den Server damit neu; der bisherige Ordner bleibt unverändert.", "That folder already holds databases. EasyMySQL restarts the server with it; the current folder stays unchanged."));
                    } else {
                        weak(ui, crate::i18n::tr("Der Server wird dafür kurz beendet. Beim Kopieren bleibt der bisherige Ordner unverändert erhalten.", "The server stops briefly. When copying, the current folder is kept unchanged."));
                    }
                    ui.horizontal(|ui| {
                        if existing {
                            if crate::widgets::primary(ui, crate::i18n::tr("Verwenden", "Use it")).clicked() {
                                actions.push(Action::ServerDataDir { dir: target.clone(), copy: false });
                                *pending_data = None;
                            }
                        } else {
                            if crate::widgets::primary(ui, crate::i18n::tr("Daten kopieren", "Copy data")).clicked() {
                                actions.push(Action::ServerDataDir { dir: target.clone(), copy: true });
                                *pending_data = None;
                            }
                            if ui.button(crate::i18n::tr("Leer beginnen", "Start empty")).clicked() {
                                actions.push(Action::ServerDataDir { dir: target.clone(), copy: false });
                                *pending_data = None;
                            }
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                            *pending_data = None;
                        }
                    });
                });
            });
        });
    }
    crate::widgets::row(ui, crate::i18n::tr("Arbeitsspeicher", "Memory"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut cfg.buffer_pool_mb, 16..=262_144, " MB");
            inline(ui, crate::i18n::tr("Zwischenspeicher für Tabellen (innodb_buffer_pool_size)", "Cache for tables (innodb_buffer_pool_size)"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Verbindungen", "Connections"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut cfg.max_connections, 10..=100_000, "");
            inline(ui, crate::i18n::tr("höchstens gleichzeitig (max_connections)", "at most at the same time (max_connections)"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Größte Anweisung", "Largest statement"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut cfg.packet_mb, 1..=1024, " MB");
            inline(ui, crate::i18n::tr("z. B. für große Bilder in BLOB-Spalten (max_allowed_packet)", "e.g. for large images in BLOB columns (max_allowed_packet)"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Sortierung", "Collation"), |ui| {
        ui.horizontal(|ui| {
            let items: Vec<String> = crate::server::COLLATIONS.iter().map(|c| c.to_string()).collect();
            crate::tabs::str_combo(ui, "server-collation", &items, &mut cfg.collation, 210.0);
            inline(ui, crate::i18n::tr("Standard für neue Datenbanken", "Default for new databases"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Ereignisse", "Events"), |ui| {
        crate::widgets::switch(ui, &mut cfg.event_scheduler, crate::i18n::tr("CREATE EVENT ausführen (event_scheduler)", "Run CREATE EVENT (event_scheduler)"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Verschlüsselung (TLS)", "Encryption (TLS)"), |ui| {
        ui.vertical(|ui| {
            let items: Vec<(crate::tls::Mode, &str)> = crate::tls::Mode::ALL.iter().map(|m| (*m, m.label())).collect();
            crate::widgets::segmented(ui, &mut cfg.tls, &items);
            match cfg.tls {
                crate::tls::Mode::Default => weak(ui, crate::i18n::tr("Wie MariaDB es selbst macht: ab Version 11.4 verschlüsselt mit einem bei jedem Start neuen Zertifikat, das Programme nicht prüfen können.", "What MariaDB does by itself: from version 11.4 encrypted with a certificate that changes on every start and that programs cannot verify.")),
                crate::tls::Mode::On => weak(ui, crate::i18n::tr("Dauerhaftes Zertifikat einer eigenen CA. EasyMySQL und gestartete Programme verbinden sich verschlüsselt und prüfen es; unverschlüsselte Verbindungen bleiben erlaubt.", "Permanent certificate from an own CA. EasyMySQL and started programs connect encrypted and verify it; unencrypted connections stay allowed.")),
                crate::tls::Mode::Required => weak(ui, crate::i18n::tr("Wie oben, aber Verbindungen über das Netz müssen verschlüsselt sein (require_secure_transport). Empfohlen, wenn der Server im Netzwerk erreichbar ist.", "As above, but connections over the network must be encrypted (require_secure_transport). Recommended when the server is reachable on the network.")),
            }
            if cfg.network && cfg.tls == crate::tls::Mode::Default {
                ui.label(RichText::new(crate::i18n::tr("Der Server ist im Netzwerk erreichbar: „Nur verschlüsselt“ wird empfohlen.", "The server is reachable on the network: “Encrypted only” is recommended.")).small().color(pal.warn_text));
            }
            if cfg.tls != crate::tls::Mode::Default {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Weitere Namen:", "More names:")).small());
                    ui.add(egui::TextEdit::singleline(&mut cfg.tls_names).hint_text("db.firma.local 192.168.1.20").desired_width(260.0));
                });
                ui.horizontal(|ui| {
                    if ui.small_button(crate::i18n::tr("CA-Zertifikat speichern …", "Save CA certificate …")).on_hover_text(crate::i18n::tr("Für andere Rechner und Programme, die das Zertifikat prüfen sollen", "For other computers and programs that should verify the certificate")).clicked() {
                        match crate::tls::ensure(&cfg.tls_names) {
                            Ok(f) => {
                                if let Some(p) = rfd::FileDialog::new().set_file_name("easymysql-ca.pem").save_file() {
                                    let _ = std::fs::copy(&f.ca, p);
                                }
                            }
                            Err(e) => actions.push(Action::Error(e)),
                        }
                    }
                    if ui.small_button(crate::i18n::tr("Neu erzeugen", "Regenerate")).on_hover_text(crate::i18n::tr("Neue CA und neues Zertifikat (danach den Server neu starten)", "New CA and certificate (restart the server afterwards)")).clicked() {
                        match crate::tls::generate(&cfg.tls_names) {
                            Ok(_) => actions.push(Action::RestartServer),
                            Err(e) => actions.push(Action::Error(e)),
                        }
                    }
                    if ui.small_button(crate::i18n::text("Öffnen")).clicked() {
                        let _ = std::fs::create_dir_all(crate::tls::dir());
                        crate::app::open_path(&crate::tls::dir());
                    }
                });
            }
        });
    });
    mariadb_version_ui(ui, s, server, actions);
    let cfg = &mut s.server;
    crate::widgets::row(ui, crate::i18n::tr("Weitere Einstellungen", "More settings"), |ui| {
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::multiline(&mut cfg.extra).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY).hint_text("sql_mode=STRICT_ALL_TABLES\nlong_query_time=2"));
            match cfg.extra_problem() {
                Some((line, why)) => {
                    ui.label(RichText::new(crate::tr_format!("Zeile {line}: {why} – die Zeilen werden erst übernommen, wenn alle stimmen.", "Line {line}: {why} – the lines are only used once all of them are valid.")).small().color(pal.error_text));
                }
                None => weak(ui, crate::i18n::tr("Eine Einstellung je Zeile für [mysqld] in my.ini. Sie überschreiben die Werte oben.", "One setting per line for [mysqld] in my.ini. They override the values above.")),
            }
            if let Some(p) = server.paths()
                && p.ini.is_file()
                && ui.small_button(crate::i18n::tr("my.ini ansehen", "View my.ini")).clicked()
            {
                actions.push(Action::OpenFileAt { path: p.ini.clone(), line: 0 });
            }
        });
    });
    // Am Ende, damit das Formular beim Tippen nicht verrutscht (das Feld behielte sonst den Fokus nicht)
    if server.needs_restart() {
        crate::widgets::row(ui, "", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Die Änderungen gelten nach einem Neustart des Servers.", "The changes take effect after restarting the server.")).color(pal.warn_text));
                if crate::widgets::primary(ui, crate::i18n::tr("Jetzt neu starten", "Restart now")).clicked() {
                    actions.push(Action::RestartServer);
                }
            });
        });
    }
}

/// Hinweis unter einer Einstellung (bricht um, statt die Karte zu verbreitern)
fn note(ui: &mut egui::Ui, t: &str) {
    let pal = style::pal();
    ui.vertical(|ui| {
        ui.add(egui::Label::new(RichText::new(t).small().color(pal.text_weak)).wrap());
    });
}

/// Einzeiliges Feld fuer Optionen (Monospace, ganze Breite)
fn args_field(ui: &mut egui::Ui, value: &mut String, hint: &str) {
    ui.add(egui::TextEdit::singleline(value).font(egui::TextStyle::Monospace).hint_text(hint).desired_width(f32::INFINITY));
}

/// Wahl eines Werkzeugs: automatisch, eigenes, vom System oder ein Pfad
fn tool_choice(ui: &mut egui::Ui, id: &str, value: &mut String, pick_folder: bool) {
    let kind = match value.as_str() {
        "" => 0,
        "own" => 1,
        "system" => 2,
        _ => 3,
    };
    let mut k = kind;
    ui.vertical(|ui| {
        ui.push_id(id, |ui| {
            crate::widgets::segmented(
                ui,
                &mut k,
                &[(0, crate::i18n::tr("Automatisch", "Automatic")), (1, crate::i18n::tr("Eigenes", "Own")), (2, crate::i18n::tr("System", "System")), (3, crate::i18n::tr("Pfad", "Path"))],
            );
        });
        if k != kind {
            *value = match k {
                0 => String::new(),
                1 => "own".into(),
                2 => "system".into(),
                _ => String::from(if cfg!(windows) { "C:\\" } else { "/" }),
            };
        }
        if k == 3 {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(value).font(egui::TextStyle::Monospace).desired_width(ui.available_width() - 80.0));
                if ui.button(crate::i18n::text("Ändern …")).clicked() {
                    let picked = if pick_folder { rfd::FileDialog::new().pick_folder() } else { rfd::FileDialog::new().pick_file() };
                    if let Some(p) = picked {
                        *value = p.display().to_string();
                    }
                }
            });
        }
    });
}

/// Stand der Downloads (Fortschritt, Fehler, fertig) mit Abbrechen
fn install_state(ui: &mut egui::Ui) {
    use crate::dev::install;
    let pal = style::pal();
    let st = install::status();
    if st.busy || install::checking() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(install::status_line(&st)).small().color(pal.text_weak));
            if st.busy && ui.small_button(crate::i18n::text("Abbrechen")).clicked() {
                install::cancel();
            }
        });
        if let Some(f) = st.fraction {
            ui.add(egui::ProgressBar::new(f).desired_width(260.0).desired_height(8.0).corner_radius(4));
        }
    } else if let Some(e) = &st.error {
        ui.label(RichText::new(e).small().color(pal.error_text));
    } else if !st.stage.is_empty() {
        ui.label(RichText::new(&st.stage).small().color(pal.ok_text));
    }
}

/// Knopf "Herunterladen"/"Aktualisieren" fuer ein Werkzeug
fn tool_button(ui: &mut egui::Ui, ctx: &egui::Context, tool: crate::dev::Tool, have: bool) {
    use crate::dev::install;
    let busy = install::status().busy || install::checking();
    let label = if have { crate::i18n::tr("Aktualisieren", "Update") } else { crate::i18n::tr("Herunterladen", "Download") };
    if ui.add_enabled(!busy, egui::Button::new(label).small()).on_hover_text(crate::i18n::tr("Lädt die neueste stabile Version", "Downloads the newest stable version")).clicked() {
        install::install(&[tool], ctx);
    }
}

fn open_button(ui: &mut egui::Ui, dir: &std::path::Path) {
    if ui.small_button(crate::i18n::text("Öffnen")).on_hover_text(dir.display().to_string()).clicked() {
        crate::app::open_path(dir);
    }
}

/// Weitere Einstellungen des Editors
fn editor_settings(ui: &mut egui::Ui, s: &mut crate::settings::Settings) {
    let e = &mut s.editor;
    crate::widgets::row(ui, crate::i18n::tr("Einrückung", "Indentation"), |ui| {
        ui.horizontal(|ui| {
            let mut mode = if e.indent == 0 { 0 } else { 1 };
            if crate::widgets::segmented(ui, &mut mode, &[(0, crate::i18n::tr("Je nach Sprache", "By language")), (1, crate::i18n::tr("Fest", "Fixed"))]) {
                e.indent = if mode == 0 { 0 } else { 4 };
            }
            if e.indent > 0 {
                crate::widgets::drag(ui, &mut e.indent, 1..=16, crate::i18n::tr(" Leerzeichen", " spaces"));
            }
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Automatisch einrücken", "Auto-indent"), |ui| {
        crate::widgets::switch(ui, &mut e.auto_indent, crate::i18n::tr("nach Enter, { und : (Python)", "after Enter, { and : (Python)"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Anzeige", "Display"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut e.line_numbers, crate::i18n::tr("Zeilennummern", "Line numbers"));
            crate::widgets::switch(ui, &mut e.highlight_line, crate::i18n::tr("Zeile mit dem Cursor hervorheben", "Highlight the cursor line"));
            crate::widgets::switch(ui, &mut e.match_brackets, crate::i18n::tr("Zusammengehörige Klammern markieren", "Highlight matching brackets"));
            crate::widgets::switch(ui, &mut e.scroll_past_end, crate::i18n::tr("Über die letzte Zeile hinaus scrollen", "Scroll past the last line"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("SQL in Programmcode", "SQL in program code"), |ui| {
        crate::widgets::switch(ui, &mut e.sql_in_strings, crate::i18n::tr("SQL in Zeichenketten von Java, Python und JavaScript prüfen", "Check SQL in strings of Java, Python and JavaScript"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Prüfen nach", "Check after"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut e.lint_delay_ms, 0..=5000, " ms");
            ui.label(RichText::new(crate::i18n::tr("Pause beim Tippen", "pause in typing")).small().color(style::pal().text_weak));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Speichern nach", "Save after"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut s.autosave_ms, 200..=60_000, " ms");
            ui.label(RichText::new(crate::i18n::tr("ohne Tippen (wenn automatisches Speichern an ist)", "without typing (when auto-save is on)")).small().color(style::pal().text_weak));
        });
    });
}

/// Version des eigenen MariaDB-Servers, Suche nach Updates und Update ohne Administratorrechte
fn mariadb_version_ui(ui: &mut egui::Ui, s: &mut crate::settings::Settings, server: &crate::server::Server, actions: &mut Vec<Action>) {
    use crate::mariadb_update::{self as mu, Track};
    let pal = style::pal();
    let st = mu::status();
    let installed = server.paths().and_then(|p| mu::installed_version(&p.bin));
    crate::widgets::row(ui, crate::i18n::tr("MariaDB-Version", "MariaDB version"), |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(installed.clone().unwrap_or_else(|| "?".into())).monospace());
                if s.server.bin_dir.is_some() {
                    ui.label(RichText::new(crate::i18n::tr("aktualisiert", "updated")).small().color(pal.text_weak));
                }
                if ui.add_enabled(!st.busy, egui::Button::new(crate::i18n::tr("Nach Updates suchen", "Check for updates")).small()).clicked() {
                    mu::check(server, Track::from_id(&s.mariadb_track), ui.ctx());
                }
            });
            if st.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(&st.stage).small().color(pal.text_weak));
                });
                if let Some(f) = st.fraction {
                    ui.add(egui::ProgressBar::new(f).desired_width(260.0).desired_height(8.0).corner_radius(4));
                }
            } else if let Some(rel) = &st.available {
                ui.label(RichText::new(crate::tr_format!("MariaDB {} ist verfügbar.", "MariaDB {} is available.", rel.version)).color(pal.ok_text));
                note(ui, crate::i18n::tr("Vorher werden alle Datenbanken gesichert und der Datenordner kopiert. Der Server ist kurz nicht erreichbar. Klappt der Start nicht, läuft wieder die bisherige Version.", "All databases are backed up and the data folder is copied first. The server is briefly unavailable. If the start fails, the previous version runs again."));
                if crate::widgets::primary(ui, &crate::tr_format!("Auf {} aktualisieren", "Update to {}", rel.version)).clicked() {
                    actions.push(Action::MariaDbUpdate(rel.clone()));
                }
            } else if let Some(e) = &st.error {
                ui.add(egui::Label::new(RichText::new(e).small().color(pal.error_text)).wrap());
            } else if let Some(d) = &st.done {
                ui.label(RichText::new(d).small().color(pal.ok_text));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Neue Versionen:", "New versions:")).small());
                let mut m = s.mariadb_updates.clone();
                if crate::widgets::segmented(ui, &mut m, &[("aus".to_string(), crate::i18n::tr("nicht suchen", "don't check")), ("melden".to_string(), crate::i18n::tr("melden", "notify")), ("automatisch".to_string(), crate::i18n::tr("automatisch installieren", "install automatically"))]) {
                    s.mariadb_updates = m;
                }
            });
            ui.horizontal(|ui| {
                let mut t = s.mariadb_track.clone();
                if crate::widgets::segmented(ui, &mut t, &[("lts".to_string(), crate::i18n::tr("Langzeitversionen (LTS)", "Long-term versions (LTS)")), ("stable".to_string(), crate::i18n::tr("Neueste stabile", "Newest stable"))]) {
                    s.mariadb_track = t;
                }
            });
            crate::widgets::switch(ui, &mut s.mariadb_copy_data, crate::i18n::tr("Datenordner vor dem Update zusätzlich kopieren", "Also copy the data folder before the update"));
            if s.server.bin_dir.is_some() && ui.small_button(crate::i18n::tr("Mitgelieferte Version verwenden", "Use the bundled version")).on_hover_text(crate::i18n::tr("Nur möglich, wenn sie nicht älter ist als die Daten", "Only possible when it is not older than the data")).clicked() {
                s.server.bin_dir = None;
                actions.push(Action::RestartServer);
            }
        });
    });
}

/// Abschnitt Ausfuehren: gilt fuer Java, Python und JavaScript
fn run_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings) {
    use crate::dev::{UpdatePolicy, install};
    let d = &mut s.dev;
    crate::widgets::row(ui, crate::i18n::tr("Programmargumente", "Program arguments"), |ui| {
        ui.vertical(|ui| {
            args_field(ui, &mut d.program_args, "--verbose \"Datei mit Leerzeichen.txt\"");
            note(ui, crate::i18n::tr("Werden jedem gestarteten Programm mitgegeben (args in main, sys.argv, process.argv).", "Passed to every started program (args in main, sys.argv, process.argv)."));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Umgebungsvariablen", "Environment variables"), |ui| {
        ui.vertical(|ui| {
            ui.add(egui::TextEdit::multiline(&mut d.program_env).font(egui::TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY).hint_text("APP_MODE=test\nLOG_LEVEL=debug"));
            note(ui, crate::i18n::tr("NAME=Wert je Zeile; # leitet einen Kommentar ein.", "NAME=value per line; # starts a comment."));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Datenbank übergeben", "Pass the database"), |ui| {
        crate::widgets::switch(ui, &mut d.pass_connection, crate::i18n::tr("Server, Datenbank und Passwort als EASYMYSQL_DB_* mitgeben", "Pass server, database and password as EASYMYSQL_DB_*"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Vor dem Start", "Before starting"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut d.save_before_run, crate::i18n::tr("Alle Dateien speichern", "Save all files"));
            crate::widgets::switch(ui, &mut d.check_before_run, crate::i18n::tr("Syntax prüfen (Python, JavaScript)", "Check syntax (Python, JavaScript)"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Arbeitsordner", "Working folder"), |ui| {
        crate::widgets::segmented(ui, &mut d.cwd_project, &[(true, crate::i18n::tr("Projekt", "Project")), (false, crate::i18n::tr("Ordner der Datei", "Folder of the file"))]);
    });
    crate::widgets::row(ui, "Terminal", |ui| {
        crate::widgets::switch(ui, &mut d.reuse_terminal, crate::i18n::tr("Ein neuer Start ersetzt den vorigen desselben Programms", "A new run replaces the previous one of the same program"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Werkzeuge laden", "Download tools"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut d.auto_download, crate::i18n::tr("Fehlendes ohne Nachfrage herunterladen (JDK, Python, Node.js, Treiber …)", "Download missing tools without asking (JDK, Python, Node.js, drivers …)"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Nach Updates suchen", "Check for updates"), |ui| {
        ui.vertical(|ui| {
            let items: Vec<(UpdatePolicy, &str)> = UpdatePolicy::ALL.iter().map(|p| (*p, p.label())).collect();
            crate::widgets::segmented(ui, &mut d.updates, &items);
            let busy = install::status().busy || install::checking();
            if ui.add_enabled(!busy, egui::Button::new(crate::i18n::text("Jetzt nach Updates suchen"))).clicked() {
                install::check_updates_now(ctx);
            }
            install_state(ui);
        });
    });
}

/// Abschnitt Java: JDK, Treiber, Maven, Gradle, JUnit und Optionen
fn java_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings) {
    use crate::dev::java::{self, Driver};
    use crate::dev::{self, Tool};
    let pal = style::pal();
    let tools = dev::tools();
    let weak = |ui: &mut egui::Ui, t: &str| {
        ui.label(RichText::new(t).color(pal.text_weak));
    };
    let d = &mut s.dev;
    crate::widgets::row(ui, "JDK", |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| match (&tools.jdk, &tools.old_jdk) {
                (Some(j), _) => {
                    ui.label(RichText::new(format!("Java {}", j.version)).monospace());
                    weak(ui, if j.own { "Eclipse Temurin" } else { crate::i18n::tr("vom System", "from the system") });
                    open_button(ui, &j.home);
                    tool_button(ui, ctx, Tool::Jdk, j.own);
                }
                (None, Some(old)) => {
                    ui.label(RichText::new(crate::tr_format!("Java {} ist zu alt (mindestens {})", "Java {} is too old (needs {} or newer)", old.version, java::MIN_JDK)).color(pal.warn_text));
                    tool_button(ui, ctx, Tool::Jdk, false);
                }
                (None, None) => {
                    weak(ui, crate::i18n::tr("Nicht installiert – wird beim ersten Start geladen", "Not installed – downloaded on the first run"));
                    tool_button(ui, ctx, Tool::Jdk, false);
                }
            });
            tool_choice(ui, "jdk-choice", &mut d.jdk, true);
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("JDBC-Treiber", "JDBC drivers"), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for drv in Driver::ALL {
                ui.horizontal(|ui| {
                    ui.label(drv.label());
                    match tools.driver(drv) {
                        Some((_, v, _)) => ui.label(RichText::new(v).monospace()),
                        None => ui.label(RichText::new(crate::i18n::tr("fehlt", "missing")).color(pal.text_weak)),
                    };
                });
            }
            ui.horizontal(|ui| {
                tool_button(ui, ctx, Tool::JavaDrivers, tools.has(Tool::JavaDrivers));
                open_button(ui, &java::lib_dir());
            });
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Treiber im Code", "Driver in code"), |ui| {
        ui.vertical(|ui| {
            let items: Vec<(Driver, &str)> = Driver::ALL.iter().map(|x| (*x, x.label())).collect();
            crate::widgets::segmented(ui, &mut d.java_driver, &items);
            note(ui, crate::i18n::tr("Gilt für erzeugten Code (jdbc:mariadb:// bzw. jdbc:mysql://) und neue Maven/Gradle-Projekte. Beide Treiber stehen beim Ausführen im Klassenpfad.", "Used for generated code (jdbc:mariadb:// or jdbc:mysql://) and new Maven/Gradle projects. Both drivers are on the class path when running."));
        });
    });
    for (tool, dist) in [(Tool::Maven, &tools.maven), (Tool::Gradle, &tools.gradle)] {
        crate::widgets::row(ui, tool.label(), |ui| {
            ui.horizontal(|ui| match dist {
                Some(x) => {
                    if x.version.is_empty() {
                        weak(ui, crate::i18n::tr("vom System", "from the system"));
                    } else {
                        ui.label(RichText::new(&x.version).monospace());
                    }
                    open_button(ui, &x.home);
                    if x.own {
                        tool_button(ui, ctx, tool, true);
                    }
                }
                None => {
                    weak(ui, crate::i18n::tr("Wird bei Bedarf geladen", "Downloaded when needed"));
                    tool_button(ui, ctx, tool, false);
                }
            });
        });
    }
    crate::widgets::row(ui, "JUnit", |ui| {
        ui.horizontal(|ui| {
            match &tools.junit {
                Some((v, _)) => ui.label(RichText::new(v).monospace()),
                None => ui.label(RichText::new(crate::i18n::tr("Wird für Tests in einfachen Projekten geladen", "Downloaded for tests in simple projects")).color(pal.text_weak)),
            };
            tool_button(ui, ctx, Tool::JUnit, tools.junit.is_some());
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Neue Projekte", "New projects"), |ui| {
        crate::widgets::switch(ui, &mut d.junit, crate::i18n::tr("mit JUnit-Tests anlegen", "create with JUnit tests"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Java-Version", "Java version"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut d.java_release, 0..=40, "");
            ui.label(RichText::new(crate::i18n::tr("für javac --release (0 = Version des JDK)", "for javac --release (0 = version of the JDK)")).small().color(pal.text_weak));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("javac-Optionen", "javac options"), |ui| args_field(ui, &mut d.javac_args, "-Xlint:all -parameters"));
    crate::widgets::row(ui, crate::i18n::tr("JVM-Optionen", "JVM options"), |ui| args_field(ui, &mut d.jvm_args, "-Xmx1g -ea -Dfile.encoding=UTF-8"));
    crate::widgets::row(ui, crate::i18n::tr("Maven-Optionen", "Maven options"), |ui| args_field(ui, &mut d.maven_args, "-DskipTests -P dev"));
    crate::widgets::row(ui, crate::i18n::tr("Gradle-Optionen", "Gradle options"), |ui| args_field(ui, &mut d.gradle_args, "--warning-mode all"));
    crate::widgets::row(ui, crate::i18n::tr("Maven und Gradle", "Maven and Gradle"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut d.use_wrapper, crate::i18n::tr("mvnw/gradlew des Projekts verwenden", "Use the project's mvnw/gradlew"));
            crate::widgets::switch(ui, &mut d.build_offline, crate::i18n::tr("Ohne Netz bauen (--offline)", "Build offline (--offline)"));
        });
    });
}

/// Abschnitt Python: Interpreter, Treiber, .venv, pip
fn python_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings) {
    use crate::dev::python::{self, Driver};
    use crate::dev::{self, Tool};
    let pal = style::pal();
    let tools = dev::tools();
    let d = &mut s.dev;
    crate::widgets::row(ui, "Python", |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                match (&tools.python, &tools.bad_python) {
                    (Some(p), _) => {
                        ui.label(RichText::new(format!("Python {}", p.version)).monospace());
                        ui.label(RichText::new(if p.own { "python-build-standalone" } else { crate::i18n::tr("vom System", "from the system") }).color(pal.text_weak));
                        open_button(ui, &p.home);
                    }
                    (None, Some((_, why))) => {
                        ui.label(RichText::new(why).color(pal.warn_text));
                    }
                    (None, None) => {
                        ui.label(RichText::new(crate::i18n::tr("Nicht installiert – wird beim ersten Start geladen", "Not installed – downloaded on the first run")).color(pal.text_weak));
                    }
                }
                tool_button(ui, ctx, Tool::Python, tools.python.as_ref().is_some_and(|p| p.own));
            });
            tool_choice(ui, "py-choice", &mut d.python, false);
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Treiber", "Driver"), |ui| {
        ui.vertical(|ui| {
            let items: Vec<(Driver, &str)> = Driver::ALL.iter().map(|x| (*x, x.label())).collect();
            crate::widgets::segmented(ui, &mut d.python_driver, &items);
            ui.horizontal(|ui| {
                match tools.python_drivers.iter().find(|x| x.0 == d.python_driver) {
                    Some((_, v)) => ui.label(RichText::new(crate::tr_format!("installiert: {v}", "installed: {v}")).small().monospace()),
                    None => ui.label(RichText::new(crate::i18n::tr("wird beim ersten Start installiert", "installed on the first run")).small().color(pal.text_weak)),
                };
                tool_button(ui, ctx, Tool::PythonDrivers, tools.has(Tool::PythonDrivers));
                open_button(ui, &python::site_dir());
            });
            note(ui, crate::tr_format!("Skripte ohne Projektdatei finden den Treiber über PYTHONPATH (import {}).", "Scripts without a project file find the driver through PYTHONPATH (import {}).", d.python_driver.module()).as_str());
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Virtuelle Umgebung", "Virtual environment"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut d.python_venv, crate::i18n::tr("Projekte mit requirements.txt/pyproject.toml in .venv ausführen", "Run projects with requirements.txt/pyproject.toml in .venv"));
            note(ui, crate::i18n::tr("EasyMySQL legt die .venv selbst an und installiert die Pakete neu, wenn sich die Datei ändert.", "EasyMySQL creates the .venv itself and reinstalls the packages when the file changes."));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Python-Optionen", "Python options"), |ui| args_field(ui, &mut d.python_args, "-X dev -W error"));
    crate::widgets::row(ui, crate::i18n::tr("pip-Optionen", "pip options"), |ui| args_field(ui, &mut d.pip_args, "--index-url https://…"));
}

/// Abschnitt JavaScript: Node.js, Treiber, Paketverwaltung
fn node_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings) {
    use crate::dev::node::{self, Driver};
    use crate::dev::{self, Tool};
    let pal = style::pal();
    let tools = dev::tools();
    let d = &mut s.dev;
    crate::widgets::row(ui, "Node.js", |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                match (&tools.node, &tools.bad_node) {
                    (Some(n), _) => {
                        ui.label(RichText::new(format!("Node.js {}", n.version)).monospace());
                        ui.label(RichText::new(if n.own { "LTS" } else { crate::i18n::tr("vom System", "from the system") }).color(pal.text_weak));
                        open_button(ui, &n.home);
                    }
                    (None, Some((_, why))) => {
                        ui.label(RichText::new(why).color(pal.warn_text));
                    }
                    (None, None) => {
                        ui.label(RichText::new(crate::i18n::tr("Nicht installiert – wird beim ersten Start geladen", "Not installed – downloaded on the first run")).color(pal.text_weak));
                    }
                }
                tool_button(ui, ctx, Tool::Node, tools.node.as_ref().is_some_and(|n| n.own));
            });
            tool_choice(ui, "node-choice", &mut d.node, false);
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Treiber", "Driver"), |ui| {
        ui.vertical(|ui| {
            let items: Vec<(Driver, &str)> = Driver::ALL.iter().map(|x| (*x, x.label())).collect();
            crate::widgets::segmented(ui, &mut d.node_driver, &items);
            ui.horizontal(|ui| {
                match tools.node_drivers.iter().find(|x| x.0 == d.node_driver) {
                    Some((_, v)) => ui.label(RichText::new(crate::tr_format!("installiert: {v}", "installed: {v}")).small().monospace()),
                    None => ui.label(RichText::new(crate::i18n::tr("wird beim ersten Start installiert", "installed on the first run")).small().color(pal.text_weak)),
                };
                tool_button(ui, ctx, Tool::NodeDrivers, tools.has(Tool::NodeDrivers));
                open_button(ui, &node::modules_dir());
            });
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Paketverwaltung", "Package manager"), |ui| {
        ui.vertical(|ui| {
            let mut pm = d.package_manager.clone();
            if crate::widgets::segmented(ui, &mut pm, &[("npm".to_string(), "npm"), ("pnpm".to_string(), "pnpm"), ("yarn".to_string(), "yarn")]) {
                d.package_manager = pm;
            }
            crate::widgets::switch(ui, &mut d.npm_auto_install, crate::i18n::tr("Pakete vor dem Start installieren, wenn package.json sich ändert", "Install packages before running when package.json changes"));
            note(ui, crate::i18n::tr("pnpm und yarn werden aus dem PATH verwendet; ohne sie gilt npm. Eine pnpm-lock.yaml bzw. yarn.lock im Projekt hat Vorrang.", "pnpm and yarn are used from the PATH; without them npm is used. A pnpm-lock.yaml or yarn.lock in the project takes precedence."));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Node-Optionen", "Node options"), |ui| args_field(ui, &mut d.node_args, "--enable-source-maps --max-old-space-size=4096"));
}

/// Abschnitt Sprachserver: an/aus je Sprache, Speicher, Verzoegerung
fn lsp_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings) {
    use crate::dev::{self, Tool, lsp};
    let pal = style::pal();
    let tools = dev::tools();
    let d = &mut s.dev;
    crate::widgets::row(ui, "", |ui| {
        note(ui, crate::i18n::tr("Sprachserver zeigen Fehler schon beim Tippen, springen zur Definition (F12, Strg+Klick), benennen über das ganze Projekt um (F2) und zeigen Hinweise beim Zeigen mit der Maus.", "Language servers show errors while typing, go to definitions (F12, Ctrl+click), rename across the project (F2) and show hints on hover."));
    });
    for (tool, on, name) in [(Tool::JavaLsp, &mut d.lsp_java, "Java (Eclipse JDT LS)"), (Tool::PythonLsp, &mut d.lsp_python, "Python (Pyright)"), (Tool::JsLsp, &mut d.lsp_js, "JavaScript/TypeScript")] {
        crate::widgets::row(ui, name, |ui| {
            ui.horizontal(|ui| {
                crate::widgets::switch(ui, on, crate::i18n::tr("an", "on"));
                let have = tools.own_version(tool);
                match &have {
                    Some(v) => ui.label(RichText::new(v).small().monospace()),
                    None => ui.label(RichText::new(crate::i18n::tr("wird bei Bedarf geladen", "downloaded when needed")).small().color(pal.text_weak)),
                };
                tool_button(ui, ctx, tool, have.is_some());
            });
        });
    }
    crate::widgets::row(ui, crate::i18n::tr("Speicher für Java", "Memory for Java"), |ui| {
        crate::widgets::drag(ui, &mut d.lsp_java_memory_mb, 256..=16384, " MB");
    });
    crate::widgets::row(ui, crate::i18n::tr("Prüfen nach", "Check after"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut d.lsp_delay_ms, 50..=5000, " ms");
            ui.label(RichText::new(crate::i18n::tr("Pause beim Tippen", "pause in typing")).small().color(pal.text_weak));
        });
    });
    let running = lsp::running();
    crate::widgets::row(ui, crate::i18n::tr("Laufend", "Running"), |ui| {
        ui.vertical(|ui| {
            if running.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("keine", "none")).color(pal.text_weak));
            }
            for (lang, root, phase) in &running {
                let st = match phase {
                    lsp::Phase::Starting => crate::i18n::tr("startet", "starting").to_string(),
                    lsp::Phase::Ready => crate::i18n::tr("bereit", "ready").to_string(),
                    lsp::Phase::Failed(e) => e.clone(),
                };
                ui.label(RichText::new(format!("{} · {} · {st}", lang.label(), root.display())).small());
            }
            if !running.is_empty() && ui.button(crate::i18n::tr("Alle neu starten", "Restart all")).clicked() {
                lsp::shutdown_all();
            }
        });
    });
    install_state(ui);
}

/// Abschnitt Git & GitHub
fn git_settings(ui: &mut egui::Ui, ctx: &egui::Context, s: &mut crate::settings::Settings, actions: &mut Vec<Action>) {
    use crate::dev::{self, Tool};
    let pal = style::pal();
    let tools = dev::tools();
    crate::widgets::row(ui, "Git", |ui| {
        ui.horizontal(|ui| match &tools.git {
            Some(g) => {
                ui.label(RichText::new(if g.version.is_empty() { crate::i18n::tr("vom System", "from the system").to_string() } else { format!("MinGit {}", g.version) }).monospace());
                if g.own {
                    tool_button(ui, ctx, Tool::Git, true);
                }
            }
            None if cfg!(windows) => {
                ui.label(RichText::new(crate::i18n::tr("nicht installiert", "not installed")).color(pal.warn_text));
                tool_button(ui, ctx, Tool::Git, false);
            }
            None => {
                ui.label(RichText::new(crate::i18n::tr("nicht installiert – bitte über die Paketverwaltung installieren", "not installed – please install it with the package manager")).color(pal.warn_text));
            }
        });
    });
    let g = &mut s.git;
    crate::widgets::row(ui, crate::i18n::tr("Name", "Name"), |ui| {
        ui.add(crate::widgets::edit(&mut g.user_name).hint_text(crate::i18n::tr("aus der Git-Konfiguration bzw. von GitHub", "from the Git configuration or GitHub")).desired_width(300.0));
    });
    crate::widgets::row(ui, "E-Mail", |ui| {
        ui.add(crate::widgets::edit(&mut g.user_email).hint_text("name@example.org").desired_width(300.0));
    });
    crate::widgets::row(ui, "Pull", |ui| {
        let mut m = g.pull_mode.clone();
        if crate::widgets::segmented(ui, &mut m, &[("ff-only".to_string(), crate::i18n::tr("Nur vorspulen", "Fast-forward only")), ("rebase".to_string(), "Rebase"), ("merge".to_string(), "Merge")]) {
            g.pull_mode = m;
        }
    });
    crate::widgets::row(ui, "Commit", |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut g.auto_stage, crate::i18n::tr("Alles committen, wenn nichts vorgemerkt ist", "Commit everything when nothing is staged"));
            crate::widgets::switch(ui, &mut g.push_after_commit, crate::i18n::tr("Danach gleich pushen", "Push right after"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Abrufen (fetch)", "Fetch"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut g.auto_fetch_secs, 0..=86_400, " s");
            ui.label(RichText::new(crate::i18n::tr("automatisch alle … Sekunden (0 = aus)", "automatically every … seconds (0 = off)")).small().color(pal.text_weak));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Erster Branch", "First branch"), |ui| {
        ui.add(crate::widgets::edit(&mut g.default_branch).desired_width(160.0));
    });
    crate::widgets::row(ui, crate::i18n::tr("Neue Repositorys", "New repositories"), |ui| {
        ui.vertical(|ui| {
            crate::widgets::switch(ui, &mut g.private_repos, crate::i18n::tr("auf GitHub privat anlegen", "create as private on GitHub"));
            crate::widgets::switch(ui, &mut s.git_init_projects, crate::i18n::tr("Neue Projekte mit Git-Repository anlegen", "Create new projects with a Git repository"));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Anzeige", "Display"), |ui| {
        crate::widgets::switch(ui, &mut s.git_panel_refresh, crate::i18n::tr("Stand alle 3 Sekunden auffrischen, solange der Git-Bereich offen ist", "Refresh every 3 seconds while the Git panel is open"));
    });
    crate::widgets::row(ui, "GitHub", |ui| {
        ui.horizontal(|ui| {
            if crate::git::github::token().is_some() {
                ui.label(RichText::new(crate::i18n::tr("angemeldet", "signed in")).color(pal.ok_text));
                if ui.button(crate::i18n::tr("Abmelden", "Sign out")).clicked() {
                    let _ = crate::git::github::set_token("");
                }
            } else if ui.button(crate::i18n::tr("Anmelden …", "Sign in …")).clicked() {
                actions.push(Action::GitHubLogin);
            }
        });
    });
}

/// Abschnitt Terminal
fn terminal_settings(ui: &mut egui::Ui, s: &mut crate::settings::Settings, profiles: &[crate::terminal::Profile]) {
    let pal = style::pal();
    let t = &mut s.terminal;
    crate::widgets::row(ui, crate::i18n::text("Schriftgröße"), |ui| {
        ui.horizontal(|ui| {
            crate::widgets::drag(ui, &mut t.font_size, 0..=48, " px");
            ui.label(RichText::new(crate::i18n::tr("0 = wie im Editor", "0 = same as the editor")).small().color(pal.text_weak));
        });
    });
    crate::widgets::row(ui, crate::i18n::tr("Verlauf", "Scrollback"), |ui| {
        crate::widgets::drag(ui, &mut t.scrollback, 100..=200_000, crate::i18n::tr(" Zeilen", " lines"));
    });
    crate::widgets::row(ui, crate::i18n::tr("Standard-Shell", "Default shell"), |ui| {
        let mut names = vec![String::new()];
        names.extend(profiles.iter().map(|p| p.name.clone()));
        crate::tabs::str_combo(ui, "term-default", &names, &mut t.default_shell, 240.0);
    });
    crate::widgets::row(ui, crate::i18n::tr("Eigene Shell", "Custom shell"), |ui| {
        ui.vertical(|ui| {
            args_field(ui, &mut t.custom_shell, if cfg!(windows) { "\"C:\\Program Files\\Git\\bin\\bash.exe\" --login" } else { "/usr/bin/fish" });
            note(ui, crate::i18n::tr("Programm und Argumente; erscheint als erste Shell in der Auswahl.", "Program and arguments; appears as the first shell in the list."));
        });
    });
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
    fn only_the_chosen_page_is_drawn() {
        let ctx = egui::Context::default();
        style::apply(&ctx, true);
        let mut tab = SettingsTab::new();
        let server = crate::server::Server::new(crate::server::Config::default());
        let mut settings = crate::settings::Settings::default();
        let mut schemas = super::super::SchemaCache::default();
        let mut actions = Vec::new();
        // Jede Seite laesst sich zeichnen, und die gewaehlte bleibt gewaehlt
        for (id, _) in SECTIONS {
            tab.show_section(id);
            for _ in 0..2 {
                let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))), ..Default::default() }, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        tab.ui(ui, &mut Ctx { db: None, schemas: &mut schemas, databases: &[], server: &server, project: None, settings: &mut settings, history: &[], update: Default::default(), actions: &mut actions });
                    });
                });
            }
            assert_eq!(tab.active_section, *id);
        }
        // Jeder Abschnitt steht in genau einer Gruppe
        for (id, _) in SECTIONS {
            assert_eq!(GROUPS.iter().filter(|(_, ids)| ids.contains(id)).count(), 1, "{id}");
        }
    }
}
