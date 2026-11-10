// Native Werkzeugfenster fuer Claude und Codex, passend zur linken Seitenleiste.
// Aufbau wie im Terminal: Verlauf, Statuszeile, Eingabe mit Befehlsvervollstaendigung, Modell und Aufwand.
use super::{Action, Ctx, TabView};
use crate::ai::{Anfrage, Auftrag, Auswahl, Befehl, Konfig, Modell, Nachricht, Rolle, Sitzung, Stand};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::mcp::setup::Target;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::time::Instant;

pub struct AiTab {
    ziel: Target,
    ordner: Option<PathBuf>,
    sitzung: Option<String>,
    verlauf: Vec<(bool, Nachricht)>,
    eingabe: String,
    schreiben: bool,
    auswahl: Auswahl,
    modelle: Vec<Modell>,
    befehle: Vec<Befehl>,
    verbindung: Option<Sitzung>,
    /// Startfehler nicht in jedem Bild erneut versuchen
    start_versucht: bool,
    auftrag: Option<Auftrag>,
    /// Anzeige der laufenden Antwort (Kopie des Hintergrundstands)
    live: Stand,
    meldung: Option<(bool, String)>,
    seit: Option<Instant>,
    vorschlag: usize,
    vorschlag_aus: bool,
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

/// Markenfarbe fuer Punkte, Statuszeile und Senden
fn farbe(ziel: Target) -> Color32 {
    if ziel == Target::Claude { Color32::from_rgb(0xD9, 0x77, 0x57) } else { Color32::from_rgb(0x7A, 0x9D, 0xFF) }
}

/// Passende Befehle zum Wort am Cursor: `/…` am Anfang oder `$…` (Codex-Skills) irgendwo.
fn vorschlaege<'a>(eingabe: &str, befehle: &'a [Befehl]) -> (usize, Vec<&'a Befehl>) {
    let start = eingabe.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
    let wort = &eingabe[start..];
    let passt = (wort.starts_with('/') && start == 0) || wort.starts_with('$');
    if !passt || eingabe.ends_with(char::is_whitespace) {
        return (start, Vec::new());
    }
    let klein = wort.to_lowercase();
    let mut treffer = befehle.iter().filter(|b| b.name.to_lowercase().starts_with(&klein)).collect::<Vec<_>>();
    treffer.extend(befehle.iter().filter(|b| !b.name.to_lowercase().starts_with(&klein) && b.name.to_lowercase().contains(&klein[1..])));
    // Ein vollstaendig getippter Befehl braucht keine Liste mehr.
    if treffer.len() == 1 && treffer[0].name == wort {
        treffer.clear();
    }
    treffer.truncate(8);
    (start, treffer)
}

impl AiTab {
    pub fn new(ziel: Target, ordner: Option<PathBuf>) -> Self {
        Self {
            ziel,
            ordner,
            sitzung: None,
            verlauf: Vec::new(),
            eingabe: String::new(),
            schreiben: false,
            auswahl: Auswahl::default(),
            modelle: Vec::new(),
            befehle: Vec::new(),
            verbindung: None,
            start_versucht: false,
            auftrag: None,
            live: Stand::default(),
            meldung: None,
            seit: None,
            vorschlag: 0,
            vorschlag_aus: false,
            tokens: None,
            kontext: None,
            speicher: neuer_speicher(ziel),
        }
    }

    fn neuer_chat(&mut self) {
        self.sitzung = None;
        self.eingabe.clear();
        self.verlauf.clear();
        self.verbindung = None;
        self.start_versucht = false;
        self.live = Stand::default();
        self.meldung = None;
        self.tokens = None;
        self.kontext = None;
        self.speicher = neuer_speicher(self.ziel);
    }

    fn konfig(&self) -> Option<Konfig> {
        // Ohne Projektordner laeuft die Sitzung im Temp-Ordner, damit Modelle und Befehle schon da sind; senden geht erst mit Ordner.
        Some(Konfig { ordner: self.ordner.clone().unwrap_or_else(std::env::temp_dir), schreiben: self.schreiben, auswahl: self.auswahl.clone() })
    }

    fn laeuft(&self) -> bool {
        self.live.laeuft
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

    /// Hintergrundstand uebernehmen; fertige Antworten wandern in den Verlauf.
    fn abholen(&mut self, cx: &mut Ctx) {
        if let Some(a) = &self.auftrag {
            let s = a.stand.lock().unwrap().clone();
            if s.fertig {
                self.auftrag = None;
                self.meldung = Some(match s.fehler {
                    Some(e) => (true, e),
                    None => (false, s.text.trim().to_string()),
                });
                // Nach Installation oder Anmeldung neu verbinden
                self.verbindung = None;
                self.start_versucht = false;
            } else {
                self.meldung = Some((false, s.text.trim().to_string()));
            }
        }
        let Some(v) = &self.verbindung else { return };
        let mut s = v.stand.lock().unwrap();
        if !s.modelle.is_empty() {
            self.modelle = s.modelle.clone();
        }
        if !s.befehle.is_empty() {
            self.befehle = s.befehle.clone();
        }
        self.tokens = s.tokens.or(self.tokens);
        self.kontext = s.kontext.or(self.kontext);
        if s.zug_fertig {
            s.zug_fertig = false;
            // Erst nach einer echten Antwort gibt es etwas fortzusetzen (leere Sitzungen speichert der Anbieter nicht).
            if let Some(id) = &s.sitzung {
                self.sitzung = Some(id.clone());
            }
            for n in std::mem::take(&mut s.nachrichten) {
                if !n.text.is_empty() {
                    self.verlauf.push((false, n));
                }
            }
            if s.komprimiert {
                s.komprimiert = false;
                self.verlauf.push((
                    false,
                    Nachricht { id: String::new(), rolle: Rolle::Status, titel: String::new(), text: tr("Kontext komprimiert", "Context compacted").into(), fertig: true },
                ));
                self.tokens = s.tokens;
            }
            self.seit = None;
            cx.actions.push(Action::FilesChanged);
            if let Err(e) = self.speichern() {
                s.fehler = Some(crate::tr_format!("Chatverlauf konnte nicht gespeichert werden: {e}", "Could not save chat history: {e}"));
            }
        }
        self.live = Stand {
            nachrichten: if s.laeuft { s.nachrichten.clone() } else { Vec::new() },
            laeuft: s.laeuft,
            denkt: s.denkt,
            bereit: s.bereit,
            beendet: s.beendet,
            fehler: s.fehler.clone(),
            modell: s.modell.clone(),
            ..Default::default()
        };
    }

    /// Haelt die Sitzung bereit: startet sie vorab und nach Aenderungen von Modell, Aufwand oder Rechten neu.
    fn verbinden(&mut self, ui: &egui::Ui, erzwingen: bool) {
        let Some(konfig) = self.konfig() else { return };
        if self.auftrag.is_some() || self.ziel.program().is_none() || self.laeuft() {
            return;
        }
        let veraltet = self.verbindung.as_ref().is_some_and(|v| v.konfig != konfig || (erzwingen && v.stand.lock().unwrap().beendet));
        if veraltet {
            self.verbindung = None;
            self.start_versucht = false;
        }
        if self.verbindung.is_some() || (self.start_versucht && !erzwingen) {
            return;
        }
        self.start_versucht = true;
        match Sitzung::starten(ui.ctx(), self.ziel, konfig, self.sitzung.as_deref()) {
            Ok(v) => {
                self.verbindung = Some(v);
                self.live = Stand::default();
            }
            Err(e) => self.live.fehler = Some(e),
        }
    }

    fn senden(&mut self, ui: &egui::Ui) {
        let text = self.eingabe.trim().to_string();
        if text.is_empty() || self.laeuft() || self.ordner.is_none() {
            return;
        }
        if text.len() > 128 * 1024 {
            self.live.fehler = Some(tr("Die Nachricht ist zu lang (maximal 128 KiB).", "The message is too long (maximum 128 KiB).").into());
            return;
        }
        let anfrage = match text.as_str() {
            "/clear" | "/new" => {
                self.neuer_chat();
                return;
            }
            "/compact" => Anfrage::Kompakt,
            _ => Anfrage::Text(text.clone()),
        };
        if matches!(anfrage, Anfrage::Kompakt) && self.sitzung.is_none() {
            self.live.fehler = Some(tr("Bitte zuerst einen Chat beginnen.", "Please start a conversation first.").into());
            return;
        }
        self.verbinden(ui, true);
        let Some(v) = &self.verbindung else { return };
        if v.senden(anfrage, &self.befehle).is_ok() {
            self.verlauf.push((true, Nachricht { id: String::new(), rolle: Rolle::Assistent, titel: String::new(), text, fertig: true }));
            self.eingabe.clear();
            self.seit = Some(Instant::now());
            self.live.laeuft = true;
            self.live.fehler = None;
            self.meldung = None;
        }
    }

    /// Anmeldung im eingebauten Terminal: Claude Code und Codex brauchen fuer den Login ein echtes Terminal
    /// (Browser-Link, Code eingeben)
    fn anmelden(&mut self, cx: &mut Ctx) {
        let Some(programm) = self.ziel.program() else {
            self.meldung = Some((true, tr("Bitte zuerst installieren.", "Please install it first.").into()));
            return;
        };
        let args: Vec<String> = match self.ziel {
            Target::Claude => vec!["auth".into(), "login".into()],
            Target::Codex => vec!["login".into()],
        };
        let ordner = self.ordner.clone().unwrap_or_else(crate::workspace::storage_root);
        let launch = crate::dev::process::Launch::new(crate::tr_format!("{} anmelden", "Sign in to {}", self.ziel.label()), programm, args, ordner);
        cx.actions.push(Action::RunProgram { launch, env: Vec::new() });
        // Danach neu verbinden, damit der Chat die Anmeldung sieht
        self.verbindung = None;
        self.start_versucht = false;
        self.live.fehler = None;
        self.meldung = Some((false, tr("Die Anmeldung läuft unten im Terminal. Danach hier einfach weiterschreiben.", "Sign-in runs in the terminal below. Then just keep writing here.").into()));
    }

    fn icon(&self) -> Icon {
        if self.ziel == Target::Claude { Icon::Claude } else { Icon::Codex }
    }

    fn modell_name(&self) -> String {
        let name = self.modelle.iter().find(|m| m.id == self.auswahl.modell).map(|m| m.name.clone()).unwrap_or_else(|| {
            if self.auswahl.modell.is_empty() { self.live.modell.clone().unwrap_or_else(|| tr("Standardmodell", "Default model").into()) } else { self.auswahl.modell.clone() }
        });
        if self.auswahl.aufwand.is_empty() { name } else { format!("{name} · {}", self.auswahl.aufwand) }
    }

    fn kopf(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, installiert: bool) {
        let pal = crate::style::pal();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, self.icon(), pal.text);
            ui.label(RichText::new(self.ziel.label()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                    ui.menu_button("…", |ui| {
                        if ui.button(if installiert { tr("Aktualisieren", "Update") } else { tr("Installieren", "Install") }).clicked() {
                            self.verbindung = None;
                            self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                            ui.close();
                        }
                        if ui.add_enabled(installiert, egui::Button::new(tr("Anmelden", "Sign in"))).clicked() {
                            self.anmelden(cx);
                            ui.close();
                        }
                        if ui.add_enabled(self.sitzung.is_some() && !self.laeuft(), egui::Button::new(tr("Kontext komprimieren", "Compact context"))).clicked() {
                            self.eingabe = "/compact".into();
                            self.senden(ui);
                            ui.close();
                        }
                        if ui.button(tr("Datenbankzugriff …", "Database access …")).clicked() {
                            cx.actions.push(Action::OpenSettings(Some("ki".into())));
                            ui.close();
                        }
                    });
                    let verlauf = icons::button(ui, Icon::History, tr("Gespeicherte Chats", "Saved chats"));
                    egui::Popup::menu(&verlauf).show(|ui| {
                        self.verlauf_menue(ui);
                    });
                    if icons::button(ui, Icon::Plus, tr("Neuer Chat (/clear)", "New chat (/clear)")).clicked() {
                        self.neuer_chat();
                    }
                });
            });
        });
        // Projektordner wie das Arbeitsverzeichnis im Terminal
        let name = self.ordner.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let text = name.as_deref().unwrap_or(tr("Projektordner wählen", "Choose project folder")).to_string();
        let knopf = ui.add_enabled_ui(!self.laeuft(), |ui| symbol_knopf(ui, Icon::Folder, &text, false)).inner;
        if knopf.on_hover_text(self.ordner.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| tr("Projektordner wählen", "Choose project folder").into())).clicked()
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            self.neuer_chat();
            self.ordner = Some(p);
        }
        ui.add_space(2.0);
        ui.separator();
    }

    fn eingabefeld(&mut self, ui: &mut egui::Ui, installiert: bool) {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
        let feld_id = ui.id().with("chat-feld");
        let fokus = ui.memory(|m| m.has_focus(feld_id));
        let (start, treffer) = vorschlaege(&self.eingabe, &self.befehle);
        let treffer = treffer.into_iter().cloned().collect::<Vec<_>>();
        let zeigen = fokus && !treffer.is_empty() && !self.vorschlag_aus;
        self.vorschlag = self.vorschlag.min(treffer.len().saturating_sub(1));
        let mut uebernehmen = None;
        let mut senden = false;
        if fokus {
            ui.input_mut(|i| {
                if zeigen {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        self.vorschlag = (self.vorschlag + 1) % treffer.len();
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        self.vorschlag = (self.vorschlag + treffer.len() - 1) % treffer.len();
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                        self.vorschlag_aus = true;
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || (!i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                        uebernehmen = Some(self.vorschlag);
                    }
                } else if (!i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter) {
                    senden = true;
                }
            });
        }
        if let Some(i) = uebernehmen {
            self.eingabe.truncate(start);
            self.eingabe.push_str(&treffer[i].name);
            self.eingabe.push(' ');
            self.vorschlag = 0;
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), feld_id) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.eingabe.chars().count()))));
                st.store(ui.ctx(), feld_id);
            }
        }
        if zeigen && uebernehmen.is_none() {
            crate::style::sunken_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (i, b) in treffer.iter().enumerate() {
                    let gewaehlt = i == self.vorschlag;
                    let mut job = egui::text::LayoutJob::default();
                    job.append(&b.name, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.0), color: if gewaehlt { akzent } else { pal.text }, ..Default::default() });
                    if !b.hinweis.is_empty() {
                        job.append(&format!(" {}", b.hinweis), 0.0, egui::TextFormat { font_id: egui::FontId::monospace(11.0), color: pal.text_weak, ..Default::default() });
                    }
                    // Nur der erste Satz, einzeilig gekuerzt: lange Skill-Texte sprengen sonst die Liste
                    let kurz = b.beschreibung.split(". ").next().unwrap_or("").lines().next().unwrap_or("");
                    if !kurz.is_empty() {
                        job.append(&format!("  {kurz}"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: pal.text_weak, ..Default::default() });
                    }
                    job.wrap = egui::text::TextWrapping::truncate_at_width(ui.available_width());
                    let r = ui.add(egui::Button::selectable(gewaehlt, job).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate)).on_hover_text(&b.beschreibung);
                    if r.clicked() {
                        self.eingabe.truncate(start);
                        self.eingabe.push_str(&b.name);
                        self.eingabe.push(' ');
                        ui.memory_mut(|m| m.request_focus(feld_id));
                    }
                }
                ui.label(RichText::new(tr("Tab übernehmen · ↑↓ wählen · Esc schließen", "Tab complete · ↑↓ select · Esc close")).small().color(pal.text_weak));
            });
            ui.add_space(4.0);
        }
        let rahmen = if fokus { akzent.gamma_multiply(0.7) } else { pal.border };
        egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, rahmen)).corner_radius(8.0).inner_margin(egui::Margin { left: 8, right: 6, top: 6, bottom: 4 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let hinweis = if self.ziel == Target::Codex {
                tr("Nachricht an Codex … (/ Befehle, $ Skills)", "Message Codex … (/ commands, $ skills)")
            } else {
                tr("Nachricht an Claude … (/ für Befehle)", "Message Claude … (/ for commands)")
            };
            let feld = ui.add(
                egui::TextEdit::multiline(&mut self.eingabe)
                    .id(feld_id)
                    .frame(egui::Frame::NONE)
                    .desired_rows(2)
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
                    .hint_text(hinweis),
            );
            if feld.changed() {
                self.vorschlag_aus = false;
                self.vorschlag = 0;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // Senden/Stopp rechts, Modell links mit dem restlichen Platz
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (r, klick) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                    let aktiv = self.laeuft() || (installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty());
                    let fuellung = if aktiv { akzent } else { pal.face_light };
                    ui.painter().circle_filled(r.center(), 13.0, if klick.hovered() && aktiv { fuellung.gamma_multiply(1.15) } else { fuellung });
                    if self.laeuft() {
                        ui.painter().rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(9.0, 9.0)), 2.0, Color32::WHITE);
                    } else {
                        icons::paint(ui.painter(), r.shrink(6.0), Icon::ArrowUp, if aktiv { Color32::WHITE } else { pal.text_weak });
                    }
                    let klick = klick.on_hover_text(if self.laeuft() { tr("Abbrechen (Esc)", "Interrupt (Esc)") } else { tr("Senden (Enter)", "Send (Enter)") });
                    if klick.clicked() {
                        if self.laeuft() {
                            if let Some(v) = &self.verbindung {
                                v.unterbrechen();
                            }
                        } else {
                            senden = true;
                        }
                    }
                    if let Some(tokens) = self.tokens.filter(|t| *t > 0) {
                        let text = self.kontext.map(|k| format!("{:.0}%", (tokens as f64 / k.max(1) as f64 * 100.0).min(100.0))).unwrap_or_else(|| format!("{tokens}"));
                        ui.label(RichText::new(text).small().color(pal.text_weak)).on_hover_text(crate::tr_format!("Kontext: {tokens} Tokens", "Context: {tokens} tokens"));
                    }
                    let (r, stift) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                    if self.schreiben || stift.hovered() {
                        ui.painter().rect_filled(r, 4.0, if self.schreiben { akzent.gamma_multiply(0.18) } else { pal.hover });
                    }
                    icons::paint(ui.painter(), r.shrink(4.0), Icon::Edit, if self.schreiben { akzent } else { pal.text_weak });
                    let stift = stift.on_hover_text(if self.schreiben {
                            tr("Dateiänderungen erlaubt – klicken für nur lesen", "File edits allowed – click for read-only")
                        } else {
                            tr("Nur lesen – klicken, um Dateiänderungen zu erlauben", "Read-only – click to allow file edits")
                        });
                    if stift.clicked() {
                        self.schreiben = !self.schreiben;
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        self.modellwahl(ui);
                    });
                });
            });
        });
        if senden {
            self.senden(ui);
        }
    }

    /// Ein Knopf fuer Modell und Aufwand, wie `/model` in der Konsole
    fn modellwahl(&mut self, ui: &mut egui::Ui) {
        let pal = crate::style::pal();
        let breite = ui.available_width().max(60.0);
        let knopf = symbol_knopf(ui, Icon::ChevronDown, &self.modell_name(), true).on_hover_text(tr("Modell und Aufwand", "Model and effort"));
        egui::Popup::menu(&knopf).width(breite.max(220.0)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(240.0);
            icons::menu_heading(ui, tr("Modell", "Model"));
            if self.modelle.is_empty() {
                ui.label(RichText::new(tr("Modelle werden geladen …", "Loading models …")).small().color(pal.text_weak));
            }
            let mut neu = None;
            if ui.selectable_label(self.auswahl.modell.is_empty(), tr("Standard", "Default")).clicked() {
                neu = Some(String::new());
            }
            for m in &self.modelle {
                let mut job = egui::text::LayoutJob::default();
                job.append(&m.name, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(13.0), color: pal.text, ..Default::default() });
                if !m.beschreibung.is_empty() {
                    job.append(&format!("  {}", m.beschreibung), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(11.5), color: pal.text_weak, ..Default::default() });
                }
                let r = ui.add(egui::Button::selectable(self.auswahl.modell == m.id, job).wrap_mode(egui::TextWrapMode::Truncate));
                let r = if m.beschreibung.is_empty() { r } else { r.on_hover_text(&m.beschreibung) };
                if r.clicked() {
                    neu = Some(m.id.clone());
                }
            }
            if let Some(id) = neu {
                if id != self.auswahl.modell {
                    self.auswahl.aufwand.clear();
                }
                self.auswahl.modell = id;
            }
            let stufen = self.modelle.iter().find(|m| m.id == self.auswahl.modell).map(|m| m.aufwand.clone()).unwrap_or_default();
            if !stufen.is_empty() {
                ui.add_space(4.0);
                icons::menu_heading(ui, tr("Aufwand", "Effort"));
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(self.auswahl.aufwand.is_empty(), tr("Auto", "Auto")).clicked() {
                        self.auswahl.aufwand.clear();
                    }
                    for e in stufen {
                        if ui.selectable_label(self.auswahl.aufwand == e, &e).clicked() {
                            self.auswahl.aufwand = e;
                        }
                    }
                });
            }
        });
    }

    /// Statuszeile wie in der Konsole: pulsierendes Logo, Taetigkeit, Dauer
    fn statuszeile(&self, ui: &mut egui::Ui) {
        let pal = crate::style::pal();
        let t = ui.input(|i| i.time);
        let laufend = self.live.nachrichten.iter().rev().find(|n| n.rolle == Rolle::Werkzeug && !n.fertig);
        let taetigkeit = if !self.live.bereit && self.live.nachrichten.is_empty() && self.seit.is_some_and(|s| s.elapsed().as_secs() < 1) {
            tr("Sendet", "Sending").to_string()
        } else if let Some(w) = laufend {
            w.titel.split('(').next().unwrap_or("").to_string()
        } else if self.live.denkt {
            tr("Denkt nach", "Thinking").into()
        } else if self.live.nachrichten.iter().any(|n| n.rolle == Rolle::Assistent && !n.text.is_empty()) {
            tr("Schreibt", "Writing").into()
        } else {
            tr("Arbeitet", "Working").into()
        };
        ui.horizontal_wrapped(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            let puls = (0.55 + 0.45 * (t * 3.0).sin()) as f32;
            let groesse = 9.0 + 3.0 * puls;
            icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), egui::vec2(groesse, groesse)), self.icon(), Color32::from_white_alpha((120.0 + 135.0 * puls) as u8));
            let sekunden = self.seit.map(|s| s.elapsed().as_secs()).unwrap_or(0);
            ui.label(RichText::new(format!("{taetigkeit} …")).color(farbe(self.ziel)));
            ui.label(RichText::new(crate::tr_format!("{sekunden} s · Esc zum Abbrechen", "{sekunden}s · Esc to interrupt")).small().color(pal.text_weak));
        });
    }
}

/// Flacher Knopf mit Symbol und gekuerztem Text (Symbol links oder rechts)
fn symbol_knopf(ui: &mut egui::Ui, icon: Icon, text: &str, rechts: bool) -> egui::Response {
    let pal = crate::style::pal();
    let font = egui::FontId::proportional(12.0);
    let breite = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text_weak).size().x + 22.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite.min(ui.available_width()).max(24.0), 22.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, pal.hover);
    }
    let farbe = if resp.hovered() { pal.text } else { pal.text_weak };
    let symbol = if rechts { r.right() - 9.0 } else { r.left() + 9.0 };
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(symbol, r.center().y), egui::vec2(13.0, 13.0)), icon, farbe);
    let galley = ui.painter().layout_job({
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, farbe);
        job.wrap = egui::text::TextWrapping::truncate_at_width((r.width() - 22.0).max(1.0));
        job
    });
    let x = if rechts { r.left() + 4.0 } else { r.left() + 19.0 };
    ui.painter().galley(egui::pos2(x, r.center().y - galley.size().y / 2.0), galley, farbe);
    resp
}

fn absatz(ui: &mut egui::Ui, text: &str) {
    let pal = crate::style::pal();
    for zeile in text.trim().lines() {
        let (zeile, gross) =
            if let Some(s) = zeile.strip_prefix("### ").or_else(|| zeile.strip_prefix("## ")).or_else(|| zeile.strip_prefix("# ")) { (s, true) } else { (zeile, false) };
        let einzug = zeile.len() - zeile.trim_start().len();
        let (zeile, punkt) = match zeile.trim_start().strip_prefix("- ").or_else(|| zeile.trim_start().strip_prefix("* ")) {
            Some(rest) => (rest, true),
            None => (zeile, false),
        };
        let mut job = egui::text::LayoutJob::default();
        if punkt {
            job.append("•  ", einzug as f32 * 4.0, egui::TextFormat { font_id: egui::FontId::proportional(13.0), color: pal.text_weak, ..Default::default() });
        }
        let (mut rest, mut fett, mut code) = (zeile, gross, false);
        while !rest.is_empty() {
            let i = rest.find('`').into_iter().chain(rest.find("**")).min().unwrap_or(rest.len());
            let format = egui::TextFormat {
                font_id: if code { egui::FontId::monospace(12.0) } else { egui::FontId::proportional(if gross { 15.0 } else { 13.0 }) },
                color: if fett || code { pal.text } else { pal.text.gamma_multiply(0.92) },
                background: if code { pal.face_light } else { Color32::TRANSPARENT },
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
            if gross {
                ui.add_space(4.0);
            }
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
            ui.horizontal(|ui| {
                ui.label(RichText::new(sprache.trim()).small().color(crate::style::pal().text_weak));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, Icon::Copy, tr("Code kopieren", "Copy code")).clicked() {
                        ui.ctx().copy_text(code.into());
                    }
                    if sprache.trim().eq_ignore_ascii_case("sql") && icons::button(ui, Icon::External, tr("Im SQL-Editor öffnen", "Open in SQL editor")).clicked() {
                        cx.actions.push(Action::OpenSql { db: None, sql: code.into(), run: false });
                    }
                });
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

/// Spalte links fuer Punkt oder Logo, rechts der Inhalt
fn mit_rand<R>(ui: &mut egui::Ui, malen: impl FnOnce(&egui::Painter, egui::Pos2), inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rand = 16.0;
    let start = ui.cursor().min;
    let r = ui
        .horizontal(|ui| {
            ui.add_space(rand);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                inhalt(ui)
            })
            .inner
        })
        .inner;
    malen(ui.painter(), start + egui::vec2(5.0, 9.0));
    r
}

fn eintrag(ui: &mut egui::Ui, nutzer: bool, n: &Nachricht, ziel: Target, cx: &mut Ctx) {
    let pal = crate::style::pal();
    let akzent = farbe(ziel);
    if n.rolle == Rolle::Status {
        ui.vertical_centered(|ui| ui.label(RichText::new(format!("— {} —", n.text)).small().color(pal.text_weak)));
        ui.add_space(6.0);
        return;
    }
    if n.rolle == Rolle::Werkzeug {
        let t = ui.input(|i| i.time);
        let fertig = n.fertig;
        mit_rand(
            ui,
            |p, pos| {
                let farbe = if fertig { pal.ok_text } else { akzent.gamma_multiply((0.5 + 0.5 * (t * 4.0).sin()) as f32) };
                p.circle_filled(pos, 3.5, farbe);
            },
            |ui| {
                let (name, arg) = n.titel.split_once('(').map(|(a, b)| (a, b.trim_end_matches(')'))).unwrap_or((&n.titel, ""));
                let mut job = egui::text::LayoutJob::default();
                job.append(name, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.0), color: pal.text, ..Default::default() });
                if !arg.is_empty() {
                    job.append(&format!(" {arg}"), 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.0), color: pal.text_weak, ..Default::default() });
                }
                let breite = (ui.available_width() - ui.spacing().indent - 8.0).max(1.0);
                let titel = egui::WidgetText::from(job).into_galley(ui, Some(egui::TextWrapMode::Truncate), breite, egui::TextStyle::Button);
                egui::CollapsingHeader::new(titel)
                    .id_salt(&n.id)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(240.0).id_salt(("werkzeug", &n.id)).show(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new(&n.text).monospace().small().color(pal.text_weak)).selectable(true).wrap());
                        });
                    })
                    .header_response
                    .on_hover_text(&n.titel);
            },
        );
        ui.add_space(2.0);
        return;
    }
    if nutzer {
        // Eigene Nachricht: hinterlegt mit Markenstrich links, wie die Eingabezeile im Terminal
        let antwort = egui::Frame::new().fill(pal.face_light).inner_margin(egui::Margin { left: 10, right: 8, top: 6, bottom: 6 }).corner_radius(6.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(&n.text).color(pal.text)).selectable(true).wrap());
        });
        let r = antwort.response.rect;
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(2.0, r.height())), 1.0, akzent);
        ui.add_space(8.0);
        return;
    }
    let oben = ui.cursor().min;
    mit_rand(ui, |p, pos| { p.circle_filled(pos, 3.5, akzent); }, |ui| nachricht(ui, &n.text, cx));
    // Kopieren nur bei Bedarf einblenden, damit der Verlauf ruhig bleibt
    let flaeche = egui::Rect::from_min_max(oben, egui::pos2(ui.max_rect().right(), ui.cursor().min.y));
    if n.fertig && ui.rect_contains_pointer(flaeche) {
        let knopf = egui::Rect::from_min_size(egui::pos2(flaeche.right() - 22.0, flaeche.top()), egui::vec2(22.0, 22.0));
        if ui.scope_builder(egui::UiBuilder::new().max_rect(knopf), |ui| icons::button(ui, Icon::Copy, tr("Nachricht kopieren", "Copy message"))).inner.clicked() {
            ui.ctx().copy_text(n.text.clone());
        }
    }
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
        self.laeuft() || self.auftrag.is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.abholen(cx);
        if self.ordner.is_none() {
            self.ordner = cx.project.map(PathBuf::from);
        }
        let pal = crate::style::pal();
        let installiert = self.ziel.program().is_some();
        self.verbinden(ui, false);
        if self.laeuft()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && let Some(v) = &self.verbindung
        {
            v.unterbrechen();
        }
        self.kopf(ui, cx, installiert);
        egui::Panel::bottom(ui.id().with("chat-eingabe")).frame(egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 6 })).show(ui, |ui| {
            self.eingabefeld(ui, installiert);
        });
        let laeuft = self.laeuft();
        let verbindet = self.verbindung.as_ref().is_some_and(|_| !self.live.bereit && !self.live.beendet);
        if laeuft || verbindet || self.auftrag.is_some() {
            // Fluessige Animation nur solange gearbeitet wird
            ui.ctx().request_repaint();
        }
        egui::ScrollArea::vertical().id_salt("chat-verlauf").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(6.0);
            if self.verlauf.is_empty() && !laeuft {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                    icons::paint(ui.painter(), r, self.icon(), pal.text);
                    ui.add_space(6.0);
                    ui.label(RichText::new(self.ziel.label()).strong().size(16.0));
                    ui.label(RichText::new(tr("Code besprechen, Änderungen umsetzen und SQL prüfen – direkt im Projekt.", "Discuss code, make changes and review SQL – right in your project.")).color(pal.text_weak));
                    ui.add_space(10.0);
                    if !installiert {
                        if ui.add_enabled(self.auftrag.is_none(), egui::Button::new(tr("Installieren", "Install"))).clicked() {
                            self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                        }
                    } else if self.ordner.is_none() {
                        if ui.button(tr("Projektordner wählen", "Choose project folder")).clicked()
                            && let Some(p) = rfd::FileDialog::new().pick_folder()
                        {
                            self.ordner = Some(p);
                        }
                    } else {
                        let status = if self.live.bereit {
                            crate::tr_format!("Bereit · {} Modelle · {} Befehle", "Ready · {} models · {} commands", self.modelle.len(), self.befehle.len())
                        } else if verbindet {
                            tr("Verbindet …", "Connecting …").into()
                        } else {
                            String::new()
                        };
                        ui.label(RichText::new(status).small().color(pal.text_weak));
                        ui.label(RichText::new(tr("Enter senden · Umschalt+Enter neue Zeile · Tab vervollständigt", "Enter send · Shift+Enter new line · Tab completes")).small().color(pal.text_weak));
                    }
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new(tr(
                            "Nachrichten und gelesene Dateien gehen an den Anbieter; dessen Limits und Kosten gelten.",
                            "Messages and files read are sent to the provider; its limits and costs apply.",
                        ))
                        .small()
                        .color(pal.text_weak.gamma_multiply(0.8)),
                    );
                });
                ui.add_space(12.0);
            }
            for (i, (nutzer, n)) in self.verlauf.iter().enumerate() {
                ui.push_id(i, |ui| eintrag(ui, *nutzer, n, self.ziel, cx));
            }
            for (i, n) in self.live.nachrichten.iter().enumerate() {
                ui.push_id(("antwort", i), |ui| eintrag(ui, false, n, self.ziel, cx));
            }
            if laeuft {
                self.statuszeile(ui);
            }
            if let Some((fehler, text)) = &self.meldung
                && !text.is_empty()
            {
                ui.add(egui::Label::new(RichText::new(text).monospace().small().color(if *fehler { pal.error_text } else { pal.text_weak })).wrap().selectable(true));
            }
            if let Some(e) = self.live.fehler.clone() {
                ui.add(egui::Label::new(RichText::new(&e).color(pal.error_text)).wrap().selectable(true));
                // Nicht angemeldet: gleich anbieten
                let l = e.to_lowercase();
                if (l.contains("log in") || l.contains("login") || l.contains("logged") || l.contains("authent") || l.contains("api key"))
                    && icons::primary_button(ui, Icon::Key, tr("Jetzt anmelden", "Sign in now")).clicked()
                {
                    self.anmelden(cx);
                }
            }
            ui.add_space(6.0);
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
    fn befehle_werden_am_wortanfang_vervollstaendigt() {
        let befehle = ["/compact", "/clear", "/context", "$review", "/model"].map(|n| Befehl { name: n.into(), ..Default::default() });
        let namen = |e: &str| vorschlaege(e, &befehle).1.iter().map(|b| b.name.as_str()).collect::<Vec<_>>();
        assert_eq!(namen("/c"), ["/compact", "/clear", "/context"]);
        assert_eq!(namen("/CON"), ["/context"]);
        assert_eq!(namen("/ode"), ["/model"], "auch Treffer in der Mitte");
        assert_eq!(namen("bitte $re"), ["$review"]);
        assert_eq!(vorschlaege("bitte $re", &befehle).0, 6);
        assert!(namen("text /c").is_empty(), "Befehle nur am Anfang");
        assert!(namen("/compact").is_empty() && namen("/compact ").is_empty() && namen("").is_empty());
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
            // Im Test nie die echten Programme starten
            tab.start_versucht = true;
            tab.modelle = vec![Modell { id: "test".into(), name: "Ein Modell mit einem langen Namen".into(), aufwand: vec!["medium".into()], ..Default::default() }];
            tab.befehle = vec![Befehl { name: "/compact".into(), beschreibung: "Eine lange Beschreibung des Befehls, die gekürzt werden muss".into(), ..Default::default() }];
            tab.auswahl = Auswahl { modell: "test".into(), aufwand: "medium".into() };
            tab.tokens = Some(1200);
            for breite in [250.0, 320.0, 600.0] {
                for text in ["", "## SQL\n**Hinweis** und `Code`.\n- Punkt\n```sql\nSELECT 1;\n```\nErklärung", "```sql\nnoch unvollständig"] {
                    tab.eingabe = if text.is_empty() { "/co".into() } else { "Frage".into() };
                    tab.live.laeuft = text.is_empty();
                    tab.verlauf = vec![
                        (true, Nachricht { id: "u".into(), rolle: Rolle::Assistent, titel: String::new(), text: "Eine Frage mit sehr langem Text, der umbrechen muss".into(), fertig: true }),
                        (
                            false,
                            Nachricht {
                                id: "tool".into(),
                                rolle: Rolle::Werkzeug,
                                titel: "Read(ein/sehr/langer/Pfad/zu/einer/Datei/mit/vielen/Ordnern/Main.java)".into(),
                                text: "Ausgabe".into(),
                                fertig: !text.is_empty(),
                            },
                        ),
                        (false, Nachricht { id: "a".into(), rolle: Rolle::Assistent, titel: String::new(), text: text.into(), fertig: true }),
                        (false, Nachricht { id: String::new(), rolle: Rolle::Status, titel: String::new(), text: "Kontext komprimiert".into(), fertig: true }),
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
                            assert!(ui.min_rect().right() <= rechts + 1.0, "Chat ragt ueber die Seitenleiste ({breite}, {text:?}): {:?}", ui.min_rect());
                        });
                    });
                }
            }
            assert!(tab.verbindung.is_none() && tab.auftrag.is_none());
        }
        assert!(actions.is_empty());
    }
}
