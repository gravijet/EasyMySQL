// Native Werkzeugfenster fuer Claude und Codex, passend zur linken Seitenleiste.
use super::{Action, Ctx, TabView};
use crate::ai::{Anfrage, Auftrag, Auswahl, Modell, Nachricht, Rolle, Stand};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::mcp::setup::Target;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

pub struct AiTab {
    ziel: Target,
    ordner: Option<PathBuf>,
    sitzung: Option<String>,
    verlauf: Vec<(bool, Nachricht)>,
    eingabe: String,
    schreiben: bool,
    auswahl: Auswahl,
    modelle: Vec<Modell>,
    modelle_geladen: bool,
    auftrag: Option<Auftrag>,
    antwort: Stand,
    verwaltung: bool,
    kompakt: bool,
    tokens: Option<u64>,
    kontext: Option<u64>,
    speicher: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Chat {
    ordner: Option<PathBuf>,
    sitzung: Option<String>,
    verlauf: Vec<(bool, Nachricht)>,
    auswahl: Auswahl,
}

fn chat_ordner(ziel: Target) -> PathBuf {
    crate::server::app_data_dir().join("ki-chats").join(if ziel == Target::Claude { "claude" } else { "codex" })
}

fn neuer_speicher(ziel: Target) -> PathBuf {
    let zeit = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    chat_ordner(ziel).join(format!("{zeit}.json"))
}

fn chat_lesen(pfad: &std::path::Path) -> Result<Chat, String> {
    let datei = std::fs::File::open(pfad).map_err(|e| e.to_string())?;
    // shortcut: pro Chat maximal 16 MiB laden, bei groesseren Verlaeufen seitenweise lesen.
    if datei.metadata().map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err(tr("Chatverlauf ist zu groß.", "Chat history is too large.").into());
    }
    serde_json::from_reader(datei).map_err(|e| e.to_string())
}

impl AiTab {
    pub fn new(ziel: Target, ordner: Option<PathBuf>) -> Self {
        let modelle = if ziel == Target::Claude {
            [("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku")].into_iter().map(|(id, name)| Modell { id: id.into(), name: name.into(), aufwand: Vec::new() }).collect()
        } else {
            Vec::new()
        };
        Self {
            ziel,
            ordner,
            sitzung: None,
            verlauf: Vec::new(),
            eingabe: String::new(),
            schreiben: false,
            auswahl: Auswahl::default(),
            modelle,
            modelle_geladen: false,
            auftrag: None,
            antwort: Stand::default(),
            verwaltung: false,
            kompakt: false,
            tokens: None,
            kontext: None,
            speicher: neuer_speicher(ziel),
        }
    }

    fn neuer_chat(&mut self) {
        self.sitzung = None;
        self.eingabe.clear();
        self.verlauf.clear();
        self.antwort = Stand::default();
        self.tokens = None;
        self.kontext = None;
        self.speicher = neuer_speicher(self.ziel);
    }

    fn speichern(&self) -> Result<(), String> {
        let chat = Chat { ordner: self.ordner.clone(), sitzung: self.sitzung.clone(), verlauf: self.verlauf.clone(), auswahl: self.auswahl.clone() };
        let text = serde_json::to_string(&chat).map_err(|e| e.to_string())?;
        crate::workspace::atomic_write_private(&self.speicher, &text).map_err(|e| e.to_string())
    }

    fn verlauf_menue(&mut self, ui: &mut egui::Ui) {
        let mut dateien =
            std::fs::read_dir(chat_ordner(self.ziel)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect::<Vec<_>>();
        dateien.sort();
        dateien.reverse();
        if dateien.is_empty() {
            ui.label(tr("Noch keine gespeicherten Chats", "No saved chats yet"));
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for pfad in dateien {
                match chat_lesen(&pfad) {
                    Ok(chat) => {
                        let titel = chat
                            .verlauf
                            .iter()
                            .find(|(nutzer, _)| *nutzer)
                            .map(|(_, n)| n.text.lines().next().unwrap_or("").chars().take(48).collect::<String>())
                            .unwrap_or_else(|| tr("Chat", "Chat").into());
                        if ui.selectable_label(pfad == self.speicher, titel).on_hover_text(chat.ordner.as_ref().map(|p| p.display().to_string()).unwrap_or_default()).clicked() {
                            self.neuer_chat();
                            self.ordner = chat.ordner;
                            self.sitzung = chat.sitzung;
                            self.verlauf = chat.verlauf;
                            self.auswahl = chat.auswahl;
                            self.speicher = pfad;
                            self.eingabe.clear();
                            ui.close();
                        }
                    }
                    Err(e) => {
                        ui.label(RichText::new(e).color(crate::style::pal().error_text));
                    }
                }
            }
        });
    }

    fn abholen(&mut self, cx: &mut Ctx) {
        let Some(auftrag) = &self.auftrag else { return };
        self.antwort = auftrag.stand.lock().unwrap().clone();
        if !self.antwort.fertig {
            return;
        }
        self.auftrag = None;
        if !self.antwort.modelle.is_empty() {
            self.modelle = std::mem::take(&mut self.antwort.modelle);
        }
        if !self.verwaltung {
            if let Some(id) = &self.antwort.sitzung {
                self.sitzung = Some(id.clone());
            }
            self.tokens = self.antwort.tokens.or(self.tokens);
            self.kontext = self.antwort.kontext.or(self.kontext);
            if self.antwort.nachrichten.is_empty() && !self.antwort.text.trim().is_empty() {
                self.antwort.nachrichten.push(Nachricht { id: String::new(), rolle: Rolle::Assistent, titel: String::new(), text: self.antwort.text.clone(), fertig: true });
            }
            for n in std::mem::take(&mut self.antwort.nachrichten) {
                if !n.text.is_empty() {
                    self.verlauf.push((false, n));
                }
            }
            if self.antwort.komprimiert {
                self.verlauf.push((
                    false,
                    Nachricht { id: String::new(), rolle: Rolle::Status, titel: String::new(), text: tr("Kontext komprimiert", "Context compacted").into(), fertig: true },
                ));
                self.tokens = self.antwort.tokens;
            }
            self.antwort.text.clear();
            if self.kompakt && !self.antwort.komprimiert && self.antwort.fehler.is_none() {
                self.antwort.fehler = Some(tr("Die Komprimierung wurde vom Anbieter nicht bestätigt.", "The provider did not confirm compaction.").into());
            }
            cx.actions.push(Action::FilesChanged);
            if let Err(e) = self.speichern() {
                self.antwort.fehler = Some(crate::tr_format!("Chatverlauf konnte nicht gespeichert werden: {e}", "Could not save chat history: {e}"));
            }
        }
    }

    fn starten(&mut self, ui: &egui::Ui, anfrage: Anfrage) {
        let Some(ordner) = &self.ordner else { return };
        self.verwaltung = matches!(anfrage, Anfrage::Anmelden | Anfrage::Modelle);
        self.kompakt = matches!(anfrage, Anfrage::Kompakt);
        self.antwort = Stand::default();
        match Auftrag::starten(ui.ctx(), self.ziel, ordner, self.sitzung.as_deref(), self.schreiben, &self.auswahl, anfrage) {
            Ok(a) => self.auftrag = Some(a),
            Err(e) => self.antwort.fehler = Some(e),
        }
    }

    fn icon(&self) -> Icon {
        if self.ziel == Target::Claude { Icon::Claude } else { Icon::Codex }
    }
}

fn absatz(ui: &mut egui::Ui, text: &str) {
    let pal = crate::style::pal();
    for zeile in text.trim().lines() {
        let (zeile, gross) =
            if let Some(s) = zeile.strip_prefix("### ").or_else(|| zeile.strip_prefix("## ")).or_else(|| zeile.strip_prefix("# ")) { (s, true) } else { (zeile, false) };
        let mut job = egui::text::LayoutJob::default();
        let (mut rest, mut fett, mut code) = (zeile, gross, false);
        while !rest.is_empty() {
            let i = rest.find('`').into_iter().chain(rest.find("**")).min().unwrap_or(rest.len());
            let format = egui::TextFormat {
                font_id: if code { egui::FontId::monospace(12.0) } else { egui::FontId::proportional(if fett { 14.0 } else { 13.0 }) },
                color: pal.text,
                background: if code { pal.face_light } else { egui::Color32::TRANSPARENT },
                ..Default::default()
            };
            job.append(&rest[..i], 0.0, format);
            rest = &rest[i..];
            if let Some(s) = rest.strip_prefix("**") {
                fett = !fett;
                rest = s;
            } else if let Some(s) = rest.strip_prefix('`') {
                code = !code;
                rest = s;
            }
        }
        if job.text.is_empty() {
            ui.add_space(4.0);
        } else {
            ui.add(egui::Label::new(job).selectable(true).wrap());
        }
    }
}

/// Auch ein noch offener Codeblock bleibt beim Streaming als Code lesbar.
fn nachricht(ui: &mut egui::Ui, text: &str, cx: &mut Ctx) {
    let mut rest = text;
    while let Some((vorher, code)) = rest.split_once("```") {
        if !vorher.trim().is_empty() {
            absatz(ui, vorher);
        }
        let Some((sprache, inhalt)) = code.split_once('\n') else {
            absatz(ui, code);
            return;
        };
        let (code, danach) = inhalt.split_once("```").unwrap_or((inhalt, ""));
        crate::style::sunken_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(sprache.trim()).small().color(crate::style::pal().text_weak));
                if icons::button(ui, Icon::Copy, tr("Code kopieren", "Copy code")).clicked() {
                    ui.ctx().copy_text(code.into());
                }
                if sprache.trim().eq_ignore_ascii_case("sql") && icons::button(ui, Icon::External, tr("Im SQL-Editor öffnen", "Open in SQL editor")).clicked() {
                    cx.actions.push(Action::OpenSql { db: None, sql: code.into(), run: false });
                }
            });
            egui::ScrollArea::horizontal().id_salt(ui.next_auto_id()).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(code.trim_end()).monospace().size(12.0)).selectable(true).extend());
            });
        });
        rest = danach;
    }
    if !rest.trim().is_empty() {
        absatz(ui, rest);
    }
}

fn eintrag(ui: &mut egui::Ui, nutzer: bool, n: &Nachricht, ziel: Target, cx: &mut Ctx) {
    let pal = crate::style::pal();
    if n.rolle == Rolle::Status {
        ui.label(RichText::new(&n.text).small().color(pal.text_weak));
        return;
    }
    if n.rolle == Rolle::Werkzeug {
        let titel = egui::WidgetText::from(RichText::new(&n.titel).small().color(pal.text_weak)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (ui.available_width() - ui.spacing().indent - 8.0).max(1.0),
            egui::TextStyle::Button,
        );
        egui::CollapsingHeader::new(titel)
            .id_salt(&n.id)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&n.text).monospace().small()).selectable(true).wrap());
            })
            .header_response
            .on_hover_text(&n.titel);
        return;
    }
    egui::Frame::new().fill(if nutzer { pal.face_light } else { egui::Color32::TRANSPARENT }).inner_margin(8.0).corner_radius(crate::style::RADIUS).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(if nutzer { tr("Sie", "You") } else { ziel.label() }).strong().size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Copy, tr("Nachricht kopieren", "Copy message")).clicked() {
                    ui.ctx().copy_text(n.text.clone());
                }
            });
        });
        nachricht(ui, &n.text, cx);
    });
    ui.add_space(8.0);
}

impl TabView for AiTab {
    fn title(&self) -> String {
        self.ziel.label().into()
    }
    fn icon(&self) -> Option<Icon> {
        Some(self.icon())
    }
    fn busy(&self) -> bool {
        self.auftrag.is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.abholen(cx);
        if self.ordner.is_none() {
            self.ordner = cx.project.map(PathBuf::from);
        }
        let pal = crate::style::pal();
        let installiert = self.ziel.program().is_some();
        if installiert && self.ziel == Target::Codex && !self.modelle_geladen && self.auftrag.is_none() && self.ordner.is_some() {
            self.modelle_geladen = true;
            self.starten(ui, Anfrage::Modelle);
        }
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, self.icon(), pal.text);
            ui.label(RichText::new(self.ziel.label()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                    ui.menu_button("…", |ui| {
                        if ui.button(if installiert { tr("Aktualisieren", "Update") } else { tr("Installieren", "Install") }).clicked() {
                            self.verwaltung = true;
                            self.antwort = Stand::default();
                            self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                            self.modelle_geladen = false;
                            ui.close();
                        }
                        if ui.add_enabled(installiert && self.ordner.is_some(), egui::Button::new(tr("Anmelden", "Sign in"))).clicked() {
                            self.starten(ui, Anfrage::Anmelden);
                            ui.close();
                        }
                        if ui.button(tr("Datenbankzugriff …", "Database access …")).clicked() {
                            cx.actions.push(Action::OpenSettings(Some("ki".into())));
                            ui.close();
                        }
                    });
                    if icons::button(ui, Icon::Plus, tr("Neuer Chat", "New chat")).clicked() {
                        self.neuer_chat();
                    }
                    let verlauf = icons::button(ui, Icon::History, tr("Gespeicherte Chats", "Saved chats"));
                    egui::Popup::menu(&verlauf).show(|ui| {
                        self.verlauf_menue(ui);
                    });
                    if ui.add_enabled_ui(self.sitzung.is_some(), |ui| icons::button(ui, Icon::Layers, tr("Kontext komprimieren", "Compact context"))).inner.clicked() {
                        self.starten(ui, Anfrage::Kompakt);
                    }
                });
            });
        });
        ui.horizontal(|ui| {
            if ui.add_enabled_ui(self.auftrag.is_none(), |ui| icons::button(ui, Icon::Folder, tr("Projektordner wählen", "Choose project folder"))).inner.clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.ordner = Some(p);
                self.neuer_chat();
                self.modelle_geladen = false;
            }
            let name = self
                .ordner
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| tr("Projektordner wählen", "Choose project folder").into());
            ui.add(egui::Label::new(RichText::new(name).small().color(pal.text_weak)).truncate())
                .on_hover_text(self.ordner.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        });
        ui.separator();
        let mut senden = false;
        egui::Panel::bottom(ui.id().with("chat-eingabe")).frame(egui::Frame::new().inner_margin(6.0)).show(ui, |ui| {
            ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                let feld = ui.add(egui::TextEdit::multiline(&mut self.eingabe).desired_rows(3).desired_width(f32::INFINITY).hint_text(tr("Nachricht …", "Message …")));
                if feld.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter)) {
                    senden = true;
                }
                let schmal = ui.available_width() < 300.0;
                ui.horizontal_wrapped(|ui| {
                    let name = self.modelle.iter().find(|m| m.id == self.auswahl.modell).map(|m| m.name.as_str()).unwrap_or_else(|| {
                        if self.auswahl.modell.is_empty() { self.antwort.modell.as_deref().unwrap_or(tr("Standardmodell", "Default model")) } else { &self.auswahl.modell }
                    });
                    egui::ComboBox::from_id_salt("modell").width((ui.available_width() - 8.0).clamp(100.0, 170.0)).selected_text(name).truncate().show_ui(ui, |ui| {
                        if ui.selectable_value(&mut self.auswahl.modell, String::new(), tr("Standardmodell", "Default model")).changed() {
                            self.auswahl.aufwand.clear();
                        }
                        for m in &self.modelle {
                            if ui.selectable_value(&mut self.auswahl.modell, m.id.clone(), &m.name).changed() {
                                self.auswahl.aufwand.clear();
                            }
                        }
                    });
                    if let Some(m) = self.modelle.iter().find(|m| m.id == self.auswahl.modell).filter(|m| !m.aufwand.is_empty()) {
                        if schmal {
                            ui.end_row();
                        }
                        egui::ComboBox::from_id_salt("aufwand")
                            .width(90.0)
                            .selected_text(if self.auswahl.aufwand.is_empty() { tr("Aufwand", "Effort") } else { &self.auswahl.aufwand })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.auswahl.aufwand, String::new(), tr("Standard", "Default"));
                                for e in &m.aufwand {
                                    ui.selectable_value(&mut self.auswahl.aufwand, e.clone(), e);
                                }
                            });
                    }
                });
                crate::widgets::check(ui, &mut self.schreiben, tr("Dateiänderungen erlauben", "Allow file edits"));
            });
            ui.horizontal(|ui| {
                if let Some(a) = &self.auftrag {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Stop, tr("Abbrechen", "Cancel")).clicked() {
                            a.abbruch.store(true, Ordering::Relaxed);
                        }
                        ui.spinner();
                        ui.add(
                            egui::Label::new(
                                RichText::new(if self.verwaltung {
                                    tr("Verbinden …", "Connecting …")
                                } else if self.kompakt {
                                    tr("Komprimieren …", "Compacting …")
                                } else {
                                    tr("Antwortet …", "Responding …")
                                })
                                .small(),
                            )
                            .truncate(),
                        );
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                } else {
                    senden |= ui
                        .add_enabled_ui(installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty(), |ui| {
                            icons::primary_button(ui, Icon::ArrowUp, tr("Senden", "Send"))
                        })
                        .inner
                        .on_hover_text("Ctrl+Enter")
                        .clicked();
                    if let Some(tokens) = self.tokens {
                        let text = self.kontext.map(|k| format!("{:.0}%", (tokens as f64 / k.max(1) as f64 * 100.0).min(100.0))).unwrap_or_else(|| format!("{tokens}"));
                        ui.label(RichText::new(text).small().color(pal.text_weak)).on_hover_text(crate::tr_format!("Kontext: {tokens} Tokens", "Context: {tokens} tokens"));
                    }
                }
            });
        });
        if senden && self.auftrag.is_none() && installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty() {
            if self.eingabe.len() > 128 * 1024 {
                self.antwort.fehler = Some(tr("Die Nachricht ist zu lang (maximal 128 KiB).", "The message is too long (maximum 128 KiB).").into());
            } else {
                let frage = std::mem::take(&mut self.eingabe);
                self.starten(ui, if frage.trim() == "/compact" { Anfrage::Kompakt } else { Anfrage::Text(frage.clone()) });
                if self.auftrag.is_some() {
                    self.verlauf.push((true, Nachricht { id: String::new(), rolle: Rolle::Assistent, titel: String::new(), text: frage, fertig: true }));
                } else {
                    self.eingabe = frage;
                }
            }
        }
        egui::ScrollArea::vertical().id_salt("chat-verlauf").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.verlauf.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new(if installiert { tr("Am Projekt arbeiten", "Work on your project") } else { tr("Assistent einrichten", "Set up assistant") }).strong());
                ui.label(tr(
                    "Code besprechen, Änderungen umsetzen und SQL prüfen – direkt im Projekt.",
                    "Discuss code, implement changes and review SQL directly in your project.",
                ));
                if !installiert && ui.button(tr("Installieren", "Install")).clicked() {
                    self.verwaltung = true;
                    self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(tr(
                        "Nachrichten und gelesene Dateien werden an den Anbieter gesendet. Dessen Kontolimits und Kosten gelten. Datenbankrechte werden separat eingestellt.",
                        "Messages and files read are sent to the provider. Account limits and costs apply. Database permissions are configured separately.",
                    ))
                    .small()
                    .color(pal.text_weak),
                );
                ui.add_space(12.0);
            }
            for (i, (nutzer, n)) in self.verlauf.iter().enumerate() {
                ui.push_id(i, |ui| eintrag(ui, *nutzer, n, self.ziel, cx));
            }
            for (i, n) in self.antwort.nachrichten.iter().enumerate() {
                ui.push_id(("antwort", i), |ui| eintrag(ui, false, n, self.ziel, cx));
            }
            if self.verwaltung && !self.antwort.text.is_empty() {
                absatz(ui, &self.antwort.text);
            }
            if let Some(e) = &self.antwort.fehler {
                ui.add(egui::Label::new(RichText::new(e).color(pal.error_text)).wrap().selectable(true));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chatverlauf_speichert_sitzung_modell_und_nachrichten_privat() {
        let mut tab = AiTab::new(Target::Codex, Some(PathBuf::from("projekt")));
        tab.sitzung = Some("test-thread".into());
        tab.auswahl.modell = "test-model".into();
        tab.verlauf.push((true, Nachricht { id: "u".into(), rolle: Rolle::Assistent, titel: String::new(), text: "Nachricht".into(), fertig: true }));
        tab.speichern().unwrap();
        let chat = chat_lesen(&tab.speicher).unwrap();
        assert_eq!(chat.sitzung, tab.sitzung);
        assert_eq!(chat.auswahl.modell, "test-model");
        assert_eq!(chat.verlauf[0].1.text, "Nachricht");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&tab.speicher).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(tab.speicher).unwrap();
    }
    #[test]
    fn native_chats_zeichnen_auch_in_schmaler_seitenleiste() {
        let ctx = egui::Context::default();
        crate::style::apply(&ctx, true);
        let server = crate::server::Server::new(Default::default());
        let mut settings = crate::settings::Settings::default();
        let mut schemas = super::super::SchemaCache::default();
        let mut actions = Vec::new();
        for ziel in [Target::Claude, Target::Codex] {
            let mut tab = AiTab::new(ziel, None);
            tab.modelle = vec![Modell { id: "test".into(), name: "Ein Modell mit einem langen Namen".into(), aufwand: vec!["medium".into()] }];
            tab.auswahl = Auswahl { modell: "test".into(), aufwand: "medium".into() };
            for breite in [250.0, 320.0, 600.0] {
                for text in ["", "## SQL\n**Hinweis** und `Code`.\n```sql\nSELECT 1;\n```\nErklärung", "```sql\nnoch unvollständig"] {
                    tab.verlauf = vec![
                        (
                            false,
                            Nachricht {
                                id: "tool".into(),
                                rolle: Rolle::Werkzeug,
                                titel: "Ein langer Werkzeugaufruf mit vielen Argumenten und einem sehr langen Dateipfad".into(),
                                text: "Ausgabe".into(),
                                fertig: true,
                            },
                        ),
                        (false, Nachricht { id: "a".into(), rolle: Rolle::Assistent, titel: String::new(), text: text.into(), fertig: true }),
                    ];
                    let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(breite, 700.0))), ..Default::default() }, |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            let rechts = ui.max_rect().right();
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
                            assert!(ui.min_rect().right() <= rechts + 1.0, "Chat ragt ueber die Seitenleiste: {:?}", ui.min_rect());
                        });
                    });
                }
            }
            assert!(tab.auftrag.is_none());
        }
        assert!(actions.is_empty());
    }
}
