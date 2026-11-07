// Claude und Codex als native Chat-Registerkarten; keine Terminal-Oberflaeche.

use super::{Action, Ctx, TabView};
use crate::ai::{Auftrag, Stand};
use crate::i18n::tr;
use crate::mcp::setup::Target;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

pub struct AiTab {
    ziel: Target,
    ordner: Option<PathBuf>,
    sitzung: Option<String>,
    verlauf: Vec<(bool, String)>,
    eingabe: String,
    schreiben: bool,
    auftrag: Option<Auftrag>,
    antwort: Stand,
    /// Anmeldung und Installation erscheinen als Status, nicht als Chat-Nachricht.
    verwaltung: bool,
}

impl AiTab {
    pub fn new(ziel: Target, ordner: Option<PathBuf>) -> Self {
        Self { ziel, ordner, sitzung: None, verlauf: Vec::new(), eingabe: String::new(), schreiben: false, auftrag: None, antwort: Stand::default(), verwaltung: false }
    }

    fn abholen(&mut self, cx: &mut Ctx) {
        let Some(auftrag) = &self.auftrag else { return };
        self.antwort = auftrag.stand.lock().unwrap().clone();
        if !self.antwort.fertig {
            return;
        }
        self.auftrag = None;
        if !self.verwaltung {
            if let Some(id) = &self.antwort.sitzung {
                self.sitzung = Some(id.clone());
            }
            if !self.antwort.text.trim().is_empty() {
                self.verlauf.push((false, std::mem::take(&mut self.antwort.text)));
            }
            cx.actions.push(Action::FilesChanged);
        }
    }

    fn starten(&mut self, ui: &egui::Ui, frage: Option<String>) {
        let Some(ordner) = &self.ordner else { return };
        self.verwaltung = frage.is_none();
        self.antwort = Stand::default();
        match Auftrag::starten(ui.ctx(), self.ziel, ordner, self.sitzung.as_deref(), self.schreiben, frage) {
            Ok(a) => self.auftrag = Some(a),
            Err(e) => self.antwort.fehler = Some(e),
        }
    }
}

/// Codebloecke bleiben kopierbar; SQL wird nur im Editor geoeffnet, nie sofort ausgefuehrt.
fn nachricht(ui: &mut egui::Ui, text: &str, cx: &mut Ctx) {
    let mut rest = text;
    while let Some((vorher, code)) = rest.split_once("```") {
        if !vorher.trim().is_empty() {
            ui.label(vorher.trim());
        }
        let Some((sprache, code)) = code.split_once('\n') else {
            ui.label(rest);
            return;
        };
        let Some((code, danach)) = code.split_once("```") else {
            ui.label(rest);
            return;
        };
        crate::style::sunken_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(sprache).small());
                if ui.small_button(tr("Kopieren", "Copy")).clicked() {
                    ui.ctx().copy_text(code.into());
                }
                if sprache.trim().eq_ignore_ascii_case("sql") && ui.small_button(tr("Im SQL-Editor öffnen", "Open in SQL editor")).clicked() {
                    cx.actions.push(Action::OpenSql { db: None, sql: code.into(), run: false });
                }
            });
            ui.add(egui::Label::new(RichText::new(code.trim_end()).monospace()).selectable(true).wrap());
        });
        rest = danach;
    }
    if !rest.trim().is_empty() {
        ui.add(egui::Label::new(rest.trim()).selectable(true).wrap());
    }
}

impl TabView for AiTab {
    fn title(&self) -> String {
        format!("{} · Chat", self.ziel.label())
    }
    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Sparkles)
    }
    fn key(&self) -> Option<String> {
        Some(format!("ai-{:?}", self.ziel))
    }
    fn busy(&self) -> bool {
        self.auftrag.is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.abholen(cx);
        let pal = crate::style::pal();
        let installiert = self.ziel.program().is_some();
        ui.horizontal_wrapped(|ui| {
            ui.heading(self.ziel.label());
            ui.label(tr("Plugin", "Plugin"));
            ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                if ui.button(if installiert { tr("Aktualisieren", "Update") } else { tr("Installieren", "Install") }).clicked() {
                    self.verwaltung = true;
                    self.antwort = Stand::default();
                    self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                }
                if ui.add_enabled(installiert && self.ordner.is_some(), egui::Button::new(tr("Anmelden", "Sign in"))).clicked() {
                    self.starten(ui, None);
                }
                if ui.button(tr("Neuer Chat", "New chat")).clicked() {
                    self.sitzung = None;
                    self.verlauf.clear();
                    self.antwort = Stand::default();
                }
            });
            if ui.button(tr("Datenbankzugriff …", "Database access …")).clicked() {
                cx.actions.push(Action::OpenSettings(Some("ki".into())));
            }
        });
        ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("Projekt:", "Project:"));
                ui.label(self.ordner.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| tr("Bitte Ordner auswählen", "Please select a folder").into()));
                if ui.button(tr("Ordner wählen …", "Choose folder …")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.ordner = Some(p);
                    self.sitzung = None;
                    self.verlauf.clear();
                    self.antwort = Stand::default();
                }
                crate::widgets::check(ui, &mut self.schreiben, tr("Dateiänderungen erlauben", "Allow file edits"));
            });
        });
        if !installiert {
            ui.label(tr(
                "Installieren Sie den Assistenten hier und melden Sie sich mit Ihrem Anbieter-Konto an.",
                "Install the assistant here and sign in with your provider account.",
            ));
        }
        ui.label(RichText::new(tr("Nachrichten und gelesene Projektdateien werden an den gewählten Anbieter gesendet. Es gelten dessen Kontolimits und Kosten. Datenbankrechte werden separat eingestellt.",
            "Messages and project files read by the assistant are sent to the selected provider. Your account limits and costs apply. Database permissions are configured separately.")).small().color(pal.text_weak));
        ui.separator();
        let mut senden = false;
        egui::Panel::bottom(ui.id().with("chat-eingabe")).show(ui, |ui| {
            ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                let feld = ui.add(
                    egui::TextEdit::multiline(&mut self.eingabe)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr("Nachricht … (Strg+Enter zum Senden)", "Message … (Ctrl+Enter to send)")),
                );
                if feld.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)) {
                    senden = true;
                }
            });
            ui.horizontal(|ui| {
                senden |= ui
                    .add_enabled(self.auftrag.is_none() && installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty(), egui::Button::new(tr("Senden", "Send")))
                    .clicked();
                if let Some(auftrag) = &self.auftrag {
                    ui.spinner();
                    ui.label(if self.verwaltung { tr("Wird eingerichtet …", "Setting up …") } else { tr("Antwortet …", "Responding …") });
                    if ui.button(tr("Abbrechen", "Cancel")).clicked() {
                        auftrag.abbruch.store(true, Ordering::Relaxed);
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
            });
        });
        if senden && self.auftrag.is_none() && installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty() {
            if self.eingabe.len() > 128 * 1024 {
                self.antwort.fehler = Some(tr("Die Nachricht ist zu lang (maximal 128 KiB).", "The message is too long (maximum 128 KiB).").into());
            } else {
                let frage = std::mem::take(&mut self.eingabe);
                self.starten(ui, Some(frage.clone()));
                if self.auftrag.is_some() {
                    self.verlauf.push((true, frage));
                } else {
                    self.eingabe = frage;
                }
            }
        }
        egui::ScrollArea::vertical().id_salt("chat-verlauf").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            if self.verlauf.is_empty() && self.auftrag.is_none() {
                ui.add_space(20.0);
                ui.label(tr(
                    "Fragen Sie nach Code, lassen Sie sich SQL erklären oder arbeiten Sie gemeinsam am Projekt.",
                    "Ask about code, explain SQL, or work together on your project.",
                ));
            }
            for (i, (nutzer, text)) in self.verlauf.iter().enumerate() {
                ui.push_id(i, |ui| {
                    egui::Frame::new().fill(if *nutzer { pal.face_light } else { pal.bg }).inner_margin(12.0).corner_radius(8.0).show(ui, |ui| {
                        ui.set_min_width((ui.available_width() - 8.0).max(0.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(if *nutzer { tr("Sie", "You") } else { self.ziel.label() }).strong().color(if *nutzer { pal.text } else { pal.accent }));
                            if ui.small_button(tr("Kopieren", "Copy")).clicked() {
                                ui.ctx().copy_text(text.clone());
                            }
                        });
                        nachricht(ui, text, cx);
                    });
                    ui.add_space(8.0);
                });
            }
            if !self.antwort.text.is_empty() {
                nachricht(ui, &self.antwort.text, cx);
            }
            if let Some(e) = &self.antwort.fehler {
                ui.colored_label(pal.error_text, e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_chats_zeichnen_ohne_terminal_und_ohne_konto() {
        let ctx = egui::Context::default();
        crate::style::apply(&ctx, true);
        let server = crate::server::Server::new(Default::default());
        let mut settings = crate::settings::Settings::default();
        let mut schemas = super::super::SchemaCache::default();
        let mut actions = Vec::new();
        for ziel in [Target::Claude, Target::Codex] {
            let mut tab = AiTab::new(ziel, None);
            for breite in [600.0, 1200.0] {
                for text in ["", "Hier ist SQL:\n```sql\nSELECT 1;\n```\nErklärung", "```sql\nnoch unvollständig"] {
                    tab.verlauf = vec![(true, "Hallo".into()), (false, text.into())];
                    let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(breite, 700.0))), ..Default::default() }, |ui| {
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
                    });
                }
            }
            assert!(tab.auftrag.is_none());
        }
        assert!(actions.is_empty());
    }
}
