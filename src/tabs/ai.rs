// Native Chat-Fenster fuer Claude und Codex, aufgebaut wie die Chat-Apps der Anbieter:
// Kopfzeile mit Konto, Verlauf als eigene Ansicht, Nachrichten als Blasen bzw. Fliesstext,
// Eingabekarte mit Projektordner, Berechtigung und Modellwahl.
use super::{Action, Ctx, TabView};
use crate::ai::{Anfrage, Auftrag, Auswahl, Befehl, Konfig, Konto, Modell, Nachricht, Rolle, Sitzung, Stand};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::mcp::setup::Target;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

/// Breite der Lesespalte, wenn der Chat breit (als Registerkarte) angezeigt wird
const SPALTE: f32 = 760.0;

#[derive(Clone, Copy, PartialEq)]
enum Ansicht {
    Chat,
    Verlauf,
}

/// Gespeicherter Chat in der Verlaufsliste
#[derive(Clone)]
struct ChatEintrag {
    pfad: PathBuf,
    titel: String,
    ordner: Option<PathBuf>,
    zeit: SystemTime,
    anzahl: usize,
}

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
    /// Stand des Hintergrunds, der zuletzt kopiert wurde
    live_rev: u64,
    meldung: Option<(bool, String)>,
    seit: Option<Instant>,
    vorschlag: usize,
    vorschlag_aus: bool,
    tokens: Option<u64>,
    kontext: Option<u64>,
    speicher: PathBuf,
    ansicht: Ansicht,
    /// Verlaufsliste (im Hintergrund gelesen) und Suchtext
    liste: Option<Vec<ChatEintrag>>,
    liste_rx: Option<Receiver<Vec<ChatEintrag>>>,
    suche: String,
    /// Anmeldestand und wann zuletzt gefragt
    konto: Option<Konto>,
    konto_rx: Option<Receiver<Konto>>,
    konto_seit: Option<Instant>,
    /// Gemessene Hoehe je Nachricht (fuer die Breite): Nachrichten ausserhalb des Bildes werden nur als Platz eingesetzt
    hoehen: HashMap<usize, (f32, f32)>,
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
    serde_json::from_reader(std::io::BufReader::new(datei)).map_err(|e| e.to_string())
}

/// Titel eines Chats: Anfang der ersten eigenen Nachricht
fn chat_titel(verlauf: &[(bool, Nachricht)]) -> String {
    verlauf
        .iter()
        .find(|(nutzer, _)| *nutzer)
        .map(|(_, n)| n.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().chars().take(80).collect::<String>())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| tr("Neuer Chat", "New chat").into())
}

/// Alle gespeicherten Chats lesen, neueste zuerst (laeuft im Hintergrund)
fn chats_lesen(ziel: Target) -> Vec<ChatEintrag> {
    let mut liste = std::fs::read_dir(chat_ordner(ziel))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|pfad| {
            let zeit = std::fs::metadata(&pfad).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let chat = chat_lesen(&pfad).ok()?;
            Some(ChatEintrag { titel: chat_titel(&chat.verlauf), ordner: chat.ordner, anzahl: chat.verlauf.iter().filter(|(n, _)| *n).count(), pfad, zeit })
        })
        .collect::<Vec<_>>();
    liste.sort_by_key(|e| std::cmp::Reverse(e.zeit));
    liste
}

/// Gruppe in der Verlaufsliste nach Alter
fn zeitgruppe(zeit: SystemTime) -> &'static str {
    let tage = SystemTime::now().duration_since(zeit).unwrap_or_default().as_secs() / 86_400;
    match tage {
        0 => tr("Heute", "Today"),
        1 => tr("Gestern", "Yesterday"),
        2..=6 => tr("Letzte 7 Tage", "Previous 7 days"),
        7..=29 => tr("Letzte 30 Tage", "Previous 30 days"),
        _ => tr("Älter", "Older"),
    }
}

/// Markenfarbe fuer Logo, Rahmen und Senden
fn farbe(ziel: Target) -> Color32 {
    if ziel == Target::Claude { Color32::from_rgb(0xD9, 0x77, 0x57) } else { Color32::from_rgb(0x7A, 0x9D, 0xFF) }
}

/// Kurzer Name des Anbieters fuer Ueberschriften ("Claude" statt "Claude Code")
fn kurzname(ziel: Target) -> &'static str {
    if ziel == Target::Claude { "Claude" } else { "Codex" }
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

/// Was im Chatbereich angeklickt wurde; wird nach dem Zeichnen ausgefuehrt
#[derive(Default)]
struct Klicks {
    vorschlag: Option<String>,
    anmelden: bool,
    installieren: bool,
    ordner: bool,
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
            live_rev: u64::MAX,
            meldung: None,
            seit: None,
            vorschlag: 0,
            vorschlag_aus: false,
            tokens: None,
            kontext: None,
            speicher: neuer_speicher(ziel),
            ansicht: Ansicht::Chat,
            liste: None,
            liste_rx: None,
            suche: String::new(),
            konto: None,
            konto_rx: None,
            konto_seit: None,
            hoehen: HashMap::new(),
        }
    }

    fn neuer_chat(&mut self) {
        self.sitzung = None;
        self.eingabe.clear();
        self.verlauf.clear();
        self.verbindung = None;
        self.start_versucht = false;
        self.live = Stand::default();
        self.live_rev = u64::MAX;
        self.meldung = None;
        self.tokens = None;
        self.kontext = None;
        self.hoehen.clear();
        self.speicher = neuer_speicher(self.ziel);
        self.ansicht = Ansicht::Chat;
    }

    fn chat_oeffnen(&mut self, pfad: PathBuf) {
        match chat_lesen(&pfad) {
            Ok(chat) => {
                self.neuer_chat();
                self.ordner = chat.ordner.or(self.ordner.take());
                self.sitzung = chat.sitzung;
                self.verlauf = chat.verlauf;
                self.auswahl = chat.auswahl;
                self.speicher = pfad;
            }
            Err(e) => self.meldung = Some((true, e)),
        }
    }

    fn chat_loeschen(&mut self, pfad: &std::path::Path) {
        if let Err(e) = std::fs::remove_file(pfad) {
            self.meldung = Some((true, e.to_string()));
        }
        if pfad == self.speicher {
            self.neuer_chat();
            self.ansicht = Ansicht::Verlauf;
        }
        if let Some(l) = &mut self.liste {
            l.retain(|e| e.pfad != pfad);
        }
    }

    /// Verlaufsliste im Hintergrund neu lesen (ein Chat kann viele MB haben)
    fn liste_laden(&mut self, ctx: &egui::Context) {
        if self.liste_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (ziel, ctx) = (self.ziel, ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(chats_lesen(ziel));
            ctx.request_repaint();
        });
        self.liste_rx = Some(rx);
    }

    /// Anmeldestand im Hintergrund fragen: beim Oeffnen, nach Anmelden/Abmelden und alle 5 Minuten
    fn konto_pruefen(&mut self, ctx: &egui::Context, sofort: bool) {
        if let Some(rx) = &self.konto_rx
            && let Ok(k) = rx.try_recv()
        {
            self.konto = Some(k);
            self.konto_rx = None;
        }
        let faellig = sofort || self.konto_seit.is_none_or(|t| t.elapsed() > Duration::from_secs(300));
        // Tests starten nie die echten Programme
        if cfg!(test) || self.konto_rx.is_some() || !faellig || self.ziel.program().is_none() {
            return;
        }
        self.konto_seit = Some(Instant::now());
        let (tx, rx) = std::sync::mpsc::channel();
        let (ziel, ctx) = (self.ziel, ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::ai::konto_lesen(ziel));
            ctx.request_repaint();
        });
        self.konto_rx = Some(rx);
    }

    fn abmelden(&mut self, ctx: &egui::Context) {
        self.verbindung = None;
        self.start_versucht = false;
        match crate::ai::abmelden(self.ziel) {
            Ok(()) => self.konto = Some(Konto { angemeldet: Some(false), art: String::new() }),
            Err(e) => self.meldung = Some((true, e)),
        }
        self.konto_pruefen(ctx, true);
    }

    fn konfig(&self) -> Option<Konfig> {
        // Ohne Projektordner laeuft die Sitzung im Temp-Ordner, damit Modelle und Befehle schon da sind; senden geht erst mit Ordner.
        Some(Konfig { ordner: self.ordner.clone().unwrap_or_else(std::env::temp_dir), schreiben: self.schreiben, auswahl: self.auswahl.clone() })
    }

    fn laeuft(&self) -> bool {
        self.live.laeuft
    }

    fn angemeldet(&self) -> Option<bool> {
        self.konto.as_ref().and_then(|k| k.angemeldet)
    }

    fn speichern(&self) -> Result<(), String> {
        let chat = Chat { ordner: self.ordner.clone(), sitzung: self.sitzung.clone(), verlauf: self.verlauf.clone(), auswahl: self.auswahl.clone() };
        let text = serde_json::to_string(&chat).map_err(|e| e.to_string())?;
        crate::workspace::atomic_write_private(&self.speicher, &text).map_err(|e| e.to_string())
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
                // Nach Installation oder Anmeldung neu verbinden und den Anmeldestand neu lesen
                self.verbindung = None;
                self.start_versucht = false;
                self.konto_seit = None;
            } else {
                self.meldung = Some((false, s.text.trim().to_string()));
            }
        }
        let Some(v) = &self.verbindung else { return };
        let mut s = v.stand.lock().unwrap();
        // Nichts Neues: nichts kopieren (vorher wurde der ganze Antwortstand in jedem Bild kopiert)
        if s.rev == self.live_rev && !s.zug_fertig {
            return;
        }
        self.live_rev = s.rev;
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
            // Die Liste zeigt den neuen Stand beim naechsten Oeffnen
            self.liste = None;
        }
        // Ein Anmeldefehler heisst: Konto neu pruefen
        if s.fehler.as_deref().is_some_and(anmeldefehler) && self.angemeldet() != Some(false) {
            self.konto_seit = None;
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
        if self.auftrag.is_some() || self.ziel.program().is_none() || self.laeuft() || self.angemeldet() == Some(false) {
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
                self.live_rev = u64::MAX;
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
            self.ansicht = Ansicht::Chat;
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
        // Anmeldestand kurz danach wieder fragen (Login im Browser dauert)
        self.konto_seit = Some(Instant::now() - Duration::from_secs(290));
        self.meldung = Some((false, tr("Die Anmeldung läuft unten im Terminal (Browser öffnet sich). Danach hier einfach weiterschreiben.", "Sign-in runs in the terminal below (a browser opens). Then just keep writing here.").into()));
    }

    fn icon(&self) -> Icon {
        if self.ziel == Target::Claude { Icon::Claude } else { Icon::Codex }
    }

    fn modell_name(&self) -> String {
        self.modelle.iter().find(|m| m.id == self.auswahl.modell).map(|m| m.name.clone()).unwrap_or_else(|| {
            if self.auswahl.modell.is_empty() { self.live.modell.clone().map(|m| kurzer_modellname(&m)).unwrap_or_else(|| tr("Standardmodell", "Default model").into()) } else { self.auswahl.modell.clone() }
        })
    }

    // ------------------------------------------------------------------
    // Kopfzeile

    fn kopf(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, installiert: bool) {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
        ui.horizontal(|ui| {
            ui.set_min_height(38.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(8.0);
            logo(ui, self.icon(), akzent, 22.0);
            ui.label(RichText::new(kurzname(self.ziel)).strong().size(14.0));
            // Zustand als Punkt: gruen bereit, gelb verbindet, grau aus
            let verbindet = self.verbindung.as_ref().is_some_and(|_| !self.live.bereit && !self.live.beendet);
            let (punkt, text) = if !installiert {
                (pal.text_weak, tr("Nicht installiert", "Not installed"))
            } else if self.angemeldet() == Some(false) {
                (pal.warn_text, tr("Nicht angemeldet", "Not signed in"))
            } else if self.live.bereit && !self.live.beendet {
                (pal.ok_text, tr("Bereit", "Ready"))
            } else if verbindet {
                (pal.warn_text, tr("Verbindet …", "Connecting …"))
            } else {
                (pal.text_weak, tr("Getrennt", "Disconnected"))
            };
            let (r, resp) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, punkt);
            resp.on_hover_text(text);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                self.konto_knopf(ui, cx, installiert);
                let verlauf_an = self.ansicht == Ansicht::Verlauf;
                if kopf_knopf(ui, Icon::History, verlauf_an, tr("Chatverlauf", "Chat history")).clicked() {
                    self.ansicht = if verlauf_an { Ansicht::Chat } else { Ansicht::Verlauf };
                    if self.ansicht == Ansicht::Verlauf {
                        self.liste = None;
                        self.liste_rx = None;
                    }
                }
                if kopf_knopf(ui, Icon::Plus, false, tr("Neuer Chat (/clear)", "New chat (/clear)")).clicked() {
                    self.neuer_chat();
                }
            });
        });
        let r = ui.cursor();
        ui.painter().hline(r.x_range(), r.top(), egui::Stroke::new(1.0, pal.border.gamma_multiply(0.7)));
    }

    /// Rundes Konto-Symbol: angemeldet gefuellt, sonst "Anmelden"; das Menue enthaelt Konto, Installation und Rechte
    fn konto_knopf(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, installiert: bool) {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
        let angemeldet = self.angemeldet();
        let resp = if installiert && angemeldet == Some(false) && ui.available_width() > 120.0 {
            pill(ui, Some(Icon::Key), tr("Anmelden", "Sign in"), akzent, true)
        } else {
            let (r, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 6.0, pal.hover);
            }
            let c = r.center();
            match angemeldet {
                Some(true) => {
                    ui.painter().circle_filled(c, 10.0, akzent.gamma_multiply(0.25));
                    ui.painter().circle_stroke(c, 10.0, egui::Stroke::new(1.2, akzent));
                    icons::paint(ui.painter(), egui::Rect::from_center_size(c, egui::vec2(11.0, 11.0)), Icon::Users, pal.text);
                }
                _ => {
                    ui.painter().circle_stroke(c, 10.0, egui::Stroke::new(1.2, pal.text_weak));
                    icons::paint(ui.painter(), egui::Rect::from_center_size(c, egui::vec2(11.0, 11.0)), Icon::Users, pal.text_weak);
                }
            }
            resp
        };
        let resp = resp.on_hover_text(tr("Konto und Einstellungen", "Account and settings"));
        egui::Popup::menu(&resp).width(260.0).show(|ui| {
            crate::style::menu_item_look(ui);
            ui.set_min_width(250.0);
            // Kontokarte
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    logo(ui, self.icon(), akzent, 26.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(self.ziel.label()).strong());
                        let (text, farbe) = match (&self.konto, installiert) {
                            (_, false) => (tr("Nicht installiert", "Not installed").to_string(), pal.text_weak),
                            (Some(Konto { angemeldet: Some(true), art }), _) => (crate::tr_format!("Angemeldet · {art}", "Signed in · {art}"), pal.ok_text),
                            (Some(Konto { angemeldet: Some(false), .. }), _) => (tr("Nicht angemeldet", "Not signed in").into(), pal.warn_text),
                            _ if self.konto_rx.is_some() => (tr("Prüfe Anmeldung …", "Checking sign-in …").into(), pal.text_weak),
                            _ => (tr("Anmeldestand unbekannt", "Sign-in status unknown").into(), pal.text_weak),
                        };
                        ui.label(RichText::new(text).size(11.5).color(farbe));
                    });
                });
            });
            ui.separator();
            if installiert {
                if angemeldet == Some(true) {
                    if icons::menu_item(ui, Some(Icon::Power), tr("Abmelden", "Sign out"), "").clicked() {
                        self.abmelden(ui.ctx());
                        ui.close();
                    }
                } else if icons::menu_item(ui, Some(Icon::Key), tr("Anmelden …", "Sign in …"), "").clicked() {
                    self.anmelden(cx);
                    ui.close();
                }
                if icons::menu_item(ui, Some(Icon::Refresh), tr("Anmeldestand prüfen", "Check sign-in"), "").clicked() {
                    self.konto_pruefen(ui.ctx(), true);
                    ui.close();
                }
            }
            ui.add_enabled_ui(self.auftrag.is_none(), |ui| {
                let text = if installiert { tr("Auf neueste Version aktualisieren", "Update to latest version") } else { tr("Installieren", "Install") };
                if icons::menu_item(ui, Some(Icon::Download), text, "").clicked() {
                    self.verbindung = None;
                    self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
                    ui.close();
                }
            });
            ui.separator();
            if ui.add_enabled_ui(self.sitzung.is_some() && !self.laeuft(), |ui| icons::menu_item(ui, Some(Icon::Layers), tr("Kontext komprimieren", "Compact context"), "/compact")).inner.clicked() {
                self.eingabe = "/compact".into();
                self.senden(ui);
                ui.close();
            }
            if icons::menu_item(ui, Some(Icon::Database), tr("Datenbankzugriff …", "Database access …"), "").clicked() {
                cx.actions.push(Action::OpenSettings(Some("ki".into())));
                ui.close();
            }
        });
    }

    // ------------------------------------------------------------------
    // Verlauf

    fn verlauf_ui(&mut self, ui: &mut egui::Ui) {
        let pal = crate::style::pal();
        if self.liste.is_none() {
            self.liste_laden(ui.ctx());
        }
        if let Some(rx) = &self.liste_rx
            && let Ok(l) = rx.try_recv()
        {
            self.liste = Some(l);
            self.liste_rx = None;
        }
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 10, bottom: 6 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                if kopf_knopf(ui, Icon::ArrowLeft, false, tr("Zurück zum Chat", "Back to chat")).clicked() {
                    self.ansicht = Ansicht::Chat;
                }
                ui.label(RichText::new(tr("Chats", "Chats")).strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pill(ui, Some(Icon::Plus), tr("Neuer Chat", "New chat"), farbe(self.ziel), false).clicked() {
                        self.neuer_chat();
                    }
                });
            });
            ui.add_space(8.0);
            suchfeld(ui, &mut self.suche, tr("Chats durchsuchen", "Search chats"));
        });
        let mut oeffnen = None;
        let mut loeschen = None;
        egui::ScrollArea::vertical().id_salt("chat-liste").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 6, right: 6, top: 0, bottom: 10 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let Some(liste) = &self.liste else {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Spinner::new().size(16.0));
                    });
                    return;
                };
                let suche = self.suche.trim().to_lowercase();
                let treffer = liste.iter().filter(|e| suche.is_empty() || e.titel.to_lowercase().contains(&suche)).collect::<Vec<_>>();
                if treffer.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(if liste.is_empty() { tr("Noch keine Chats", "No chats yet") } else { tr("Keine Treffer", "No matches") }).color(pal.text_weak));
                    });
                    return;
                }
                let mut gruppe = "";
                for e in treffer {
                    let g = zeitgruppe(e.zeit);
                    if g != gruppe {
                        gruppe = g;
                        ui.add_space(8.0);
                        ui.label(RichText::new(g).size(11.5).strong().color(pal.text_weak));
                        ui.add_space(2.0);
                    }
                    match chat_zeile(ui, e, e.pfad == self.speicher) {
                        Some(true) => oeffnen = Some(e.pfad.clone()),
                        Some(false) => loeschen = Some(e.pfad.clone()),
                        None => {}
                    }
                }
            });
        });
        if let Some(p) = oeffnen {
            self.chat_oeffnen(p);
        }
        if let Some(p) = loeschen {
            self.chat_loeschen(&p);
        }
    }

    // ------------------------------------------------------------------
    // Eingabe

    fn eingabefeld(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, installiert: bool) {
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
        spalte(ui, |ui| {
            if zeigen && uebernehmen.is_none() {
                karte(ui, pal.face_light, 10, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, b) in treffer.iter().enumerate() {
                        let gewaehlt = i == self.vorschlag;
                        let breite = ui.available_width();
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, 26.0), egui::Sense::click());
                        if gewaehlt || resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, if gewaehlt { akzent.gamma_multiply(0.18) } else { pal.hover });
                        }
                        let mut job = egui::text::LayoutJob::default();
                        job.append(&b.name, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.0), color: if gewaehlt { akzent } else { pal.text }, ..Default::default() });
                        if !b.hinweis.is_empty() {
                            job.append(&format!(" {}", b.hinweis), 0.0, egui::TextFormat { font_id: egui::FontId::monospace(11.0), color: pal.text_weak, ..Default::default() });
                        }
                        // Nur der erste Satz, einzeilig gekuerzt: lange Skill-Texte sprengen sonst die Liste
                        let kurz = b.beschreibung.split(". ").next().unwrap_or("").lines().next().unwrap_or("");
                        if !kurz.is_empty() {
                            job.append(&format!("   {kurz}"), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.0), color: pal.text_weak, ..Default::default() });
                        }
                        job.wrap = egui::text::TextWrapping::truncate_at_width(breite - 16.0);
                        let g = ui.painter().layout_job(job);
                        ui.painter().galley(egui::pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, pal.text);
                        if resp.on_hover_text(&b.beschreibung).clicked() {
                            self.eingabe.truncate(start);
                            self.eingabe.push_str(&b.name);
                            self.eingabe.push(' ');
                            ui.memory_mut(|m| m.request_focus(feld_id));
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new(tr("Tab übernehmen · ↑↓ wählen · Esc schließen", "Tab complete · ↑↓ select · Esc close")).size(11.5).color(pal.text_weak));
                });
                ui.add_space(6.0);
            }
            let rahmen = if fokus { akzent.gamma_multiply(0.75) } else { pal.border };
            let antwort = egui::Frame::new()
                .fill(pal.bg)
                .stroke(egui::Stroke::new(1.0, rahmen))
                .corner_radius(14.0)
                .shadow(egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(if pal.dark { 60 } else { 18 }) })
                .inner_margin(egui::Margin { left: 12, right: 8, top: 10, bottom: 8 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let hinweis = if self.ziel == Target::Codex {
                        tr("Nachricht an Codex … (/ Befehle, $ Skills)", "Message Codex … (/ commands, $ skills)")
                    } else {
                        tr("Nachricht an Claude … (/ für Befehle)", "Message Claude … (/ for commands)")
                    };
                    // Waechst mit dem Text bis 8 Zeilen, dann scrollt das Feld
                    let feld = egui::ScrollArea::vertical().id_salt("chat-feld-scroll").max_height(170.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.eingabe)
                                .id(feld_id)
                                .frame(egui::Frame::NONE)
                                .desired_rows(1)
                                .lock_focus(true)
                                .margin(egui::vec2(0.0, 2.0))
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::proportional(14.0))
                                .hint_text(RichText::new(hinweis).color(pal.text_weak)),
                        )
                    }).inner;
                    if feld.changed() {
                        self.vorschlag_aus = false;
                        self.vorschlag = 0;
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let schmal = ui.available_width() < 330.0;
                        self.ordner_knopf(ui, schmal);
                        self.rechte_knopf(ui, cx, schmal);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let aktiv = self.laeuft() || (installiert && self.ordner.is_some() && !self.eingabe.trim().is_empty());
                            if senden_knopf(ui, self.laeuft(), aktiv, akzent).clicked() {
                                if self.laeuft() {
                                    if let Some(v) = &self.verbindung {
                                        v.unterbrechen();
                                    }
                                } else {
                                    senden = true;
                                }
                            }
                            self.modellwahl(ui, schmal);
                            if let Some(tokens) = self.tokens.filter(|t| *t > 0)
                                && !schmal
                            {
                                kontext_ring(ui, tokens, self.kontext);
                            }
                        });
                    });
                });
            if antwort.response.clicked() {
                ui.memory_mut(|m| m.request_focus(feld_id));
            }
            ui.add_space(4.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(crate::tr_format!(
                        "{} kann Fehler machen. Nachrichten und gelesene Dateien gehen an den Anbieter.",
                        "{} can make mistakes. Messages and files read are sent to the provider.",
                        kurzname(self.ziel)
                    ))
                    .size(10.5)
                    .color(pal.text_weak.gamma_multiply(0.8)),
                );
            });
        });
        if senden {
            self.senden(ui);
        }
    }

    fn ordner_knopf(&mut self, ui: &mut egui::Ui, schmal: bool) {
        let name = self.ordner.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let text = name.unwrap_or_else(|| tr("Ordner wählen", "Choose folder").into());
        let tipp = self.ordner.as_ref().map(|p| crate::tr_format!("Projektordner: {}", "Project folder: {}", p.display())).unwrap_or_else(|| tr("Projektordner wählen", "Choose project folder").into());
        let resp = ui.add_enabled_ui(!self.laeuft(), |ui| chip(ui, Icon::Folder, if schmal { "" } else { &text }, false)).inner.on_hover_text(tipp);
        if resp.clicked()
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            self.neuer_chat();
            self.ordner = Some(p);
        }
    }

    /// Berechtigungen wie in den Chat-Apps: nur lesen oder Dateien bearbeiten
    fn rechte_knopf(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, schmal: bool) {
        let pal = crate::style::pal();
        let (icon, text) = if self.schreiben { (Icon::Edit, tr("Bearbeiten", "Edit files")) } else { (Icon::Shield, tr("Nur lesen", "Read-only")) };
        let resp = chip(ui, icon, if schmal { "" } else { text }, self.schreiben).on_hover_text(tr("Berechtigungen", "Permissions"));
        egui::Popup::menu(&resp).width(290.0).show(|ui| {
            crate::style::menu_item_look(ui);
            ui.set_min_width(280.0);
            icons::menu_heading(ui, tr("Berechtigungen", "Permissions"));
            for (wert, icon, titel, text) in [
                (false, Icon::Shield, tr("Nur lesen", "Read-only"), tr("Liest und durchsucht Dateien, ändert nichts", "Reads and searches files, changes nothing")),
                (true, Icon::Edit, tr("Dateien bearbeiten", "Edit files"), tr("Darf Dateien im Projektordner anlegen und ändern", "May create and change files in the project folder")),
            ] {
                if auswahl_zeile(ui, Some(icon), titel, text, self.schreiben == wert, farbe(self.ziel)).clicked() {
                    self.schreiben = wert;
                    ui.close();
                }
            }
            ui.separator();
            ui.label(RichText::new(tr("Programme ausführen ist immer gesperrt. Für die Datenbank gelten die Rechte aus den Einstellungen.", "Running programs is always blocked. Database access follows the settings.")).size(11.5).color(pal.text_weak));
            if icons::menu_item(ui, Some(Icon::Database), tr("Datenbankzugriff …", "Database access …"), "").clicked() {
                cx.actions.push(Action::OpenSettings(Some("ki".into())));
                ui.close();
            }
        });
    }

    /// Modell und Denkaufwand, wie die Modellauswahl der Chat-Apps
    fn modellwahl(&mut self, ui: &mut egui::Ui, schmal: bool) {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
        let mut name = self.modell_name();
        if !self.auswahl.aufwand.is_empty() && !schmal {
            name = format!("{name} · {}", self.auswahl.aufwand);
        }
        let knopf = chip_rechts(ui, &name, ui.available_width().min(if schmal { 120.0 } else { 220.0 })).on_hover_text(tr("Modell und Denkaufwand", "Model and effort"));
        egui::Popup::menu(&knopf).width(310.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            crate::style::menu_item_look(ui);
            ui.set_min_width(300.0);
            icons::menu_heading(ui, tr("Modell", "Model"));
            if self.modelle.is_empty() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(RichText::new(tr("Modelle werden geladen …", "Loading models …")).size(11.5).color(pal.text_weak));
                });
            }
            let mut neu = None;
            let standard = self.live.modell.as_deref().map(kurzer_modellname);
            let text = match &standard {
                Some(m) => crate::tr_format!("Empfohlen vom Anbieter ({m})", "Recommended by the provider ({m})"),
                None => tr("Empfohlen vom Anbieter", "Recommended by the provider").into(),
            };
            if auswahl_zeile(ui, None, tr("Standard", "Default"), &text, self.auswahl.modell.is_empty(), akzent).clicked() {
                neu = Some(String::new());
            }
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for m in &self.modelle {
                    let r = auswahl_zeile(ui, None, &m.name, &m.beschreibung, self.auswahl.modell == m.id, akzent);
                    if r.clicked() {
                        neu = Some(m.id.clone());
                    }
                }
            });
            if let Some(id) = neu {
                if id != self.auswahl.modell {
                    self.auswahl.aufwand.clear();
                }
                self.auswahl.modell = id;
            }
            let stufen = self.modelle.iter().find(|m| m.id == self.auswahl.modell).map(|m| m.aufwand.clone()).unwrap_or_default();
            if !stufen.is_empty() {
                ui.add_space(6.0);
                icons::menu_heading(ui, tr("Denkaufwand", "Effort"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    if segment(ui, tr("Auto", "Auto"), self.auswahl.aufwand.is_empty(), akzent).clicked() {
                        self.auswahl.aufwand.clear();
                    }
                    for e in stufen {
                        if segment(ui, &e, self.auswahl.aufwand == e, akzent).clicked() {
                            self.auswahl.aufwand = e;
                        }
                    }
                });
                ui.label(RichText::new(tr("Mehr Aufwand: gründlicher, aber langsamer.", "More effort: more thorough, but slower.")).size(11.5).color(pal.text_weak));
            }
        });
    }

    // ------------------------------------------------------------------
    // Chatbereich

    fn chat_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx, installiert: bool) -> Klicks {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
        let mut klicks = Klicks::default();
        let laeuft = self.laeuft();
        let ziel = self.ziel;
        let icon = self.icon();
        egui::ScrollArea::vertical().id_salt("chat-verlauf").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(10.0);
            spalte(ui, |ui| {
                // Hinweise oben: nicht installiert, nicht angemeldet, Ordner fehlt
                if !installiert {
                    if hinweis_karte(ui, Icon::Download, crate::tr_format!("{} ist noch nicht installiert", "{} is not installed yet", self.ziel.label()), tr("EasyMySQL lädt die neueste Version mit Prüfsumme herunter.", "EasyMySQL downloads the latest version and verifies its checksum."), Some(if self.auftrag.is_some() { tr("Wird installiert …", "Installing …") } else { tr("Installieren", "Install") }), akzent) {
                        klicks.installieren = self.auftrag.is_none();
                    }
                } else if self.angemeldet() == Some(false) {
                    let text = if self.ziel == Target::Claude { tr("Mit deinem Claude-Konto oder API-Schlüssel anmelden.", "Sign in with your Claude account or an API key.") } else { tr("Mit ChatGPT oder einem API-Schlüssel anmelden.", "Sign in with ChatGPT or an API key.") };
                    if hinweis_karte(ui, Icon::Key, tr("Bitte anmelden", "Please sign in").into(), text, Some(tr("Anmelden", "Sign in")), akzent) {
                        klicks.anmelden = true;
                    }
                } else if self.ordner.is_none()
                    && hinweis_karte(ui, Icon::Folder, tr("Projektordner wählen", "Choose a project folder").into(), tr("Der Assistent arbeitet in diesem Ordner.", "The assistant works in this folder."), Some(tr("Ordner wählen", "Choose folder")), akzent)
                {
                    klicks.ordner = true;
                }
                if self.verlauf.is_empty() && !laeuft {
                    leerer_chat(ui, ziel, icon, &mut klicks);
                }
                // Fertige Nachrichten ausserhalb des sichtbaren Bereichs nur als Platz einsetzen
                let breite = ui.available_width();
                let clip = ui.clip_rect().expand(300.0);
                let mut vorher_nutzer = true;
                for (i, (nutzer, n)) in self.verlauf.iter().enumerate() {
                    let oben = ui.cursor().top();
                    if let Some(&(b, h)) = self.hoehen.get(&i)
                        && (b - breite).abs() < 0.5
                        && (oben + h < clip.top() || oben > clip.bottom())
                    {
                        ui.allocate_space(egui::vec2(breite, h));
                        vorher_nutzer = *nutzer;
                        continue;
                    }
                    ui.push_id(i, |ui| eintrag(ui, *nutzer, n, ziel, vorher_nutzer, cx));
                    self.hoehen.insert(i, (breite, ui.cursor().top() - oben));
                    vorher_nutzer = *nutzer;
                }
                for (i, n) in self.live.nachrichten.iter().enumerate() {
                    ui.push_id(("antwort", i), |ui| eintrag(ui, false, n, ziel, vorher_nutzer && i == 0, cx));
                }
                if laeuft {
                    self.statuszeile(ui);
                }
                if let Some((fehler, text)) = &self.meldung
                    && !text.is_empty()
                {
                    ui.add_space(4.0);
                    karte(ui, pal.face_light, 10, |ui| {
                        ui.add(egui::Label::new(RichText::new(text).monospace().size(11.5).color(if *fehler { pal.error_text } else { pal.text_weak })).wrap().selectable(true));
                    });
                }
                if let Some(e) = self.live.fehler.clone() {
                    ui.add_space(4.0);
                    let anmelden = anmeldefehler(&e);
                    egui::Frame::new()
                        .fill(pal.error_text.gamma_multiply(0.08))
                        .stroke(egui::Stroke::new(1.0, pal.error_text.gamma_multiply(0.45)))
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                icons::paint(ui.painter(), r, Icon::Alert, pal.error_text);
                                ui.add_space(4.0);
                                ui.add(egui::Label::new(RichText::new(&e).color(pal.text)).wrap().selectable(true));
                            });
                            if anmelden {
                                ui.add_space(6.0);
                                if pill(ui, Some(Icon::Key), tr("Jetzt anmelden", "Sign in now"), akzent, true).clicked() {
                                    klicks.anmelden = true;
                                }
                            }
                        });
                }
                ui.add_space(10.0);
            });
        });
        klicks
    }

    /// Laufende Antwort: drei pulsierende Punkte, Taetigkeit und Dauer
    fn statuszeile(&self, ui: &mut egui::Ui) {
        let pal = crate::style::pal();
        let akzent = farbe(self.ziel);
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
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 16.0), egui::Sense::hover());
            for k in 0..3 {
                let phase = (t * 5.0 - k as f64 * 0.7).sin() as f32 * 0.5 + 0.5;
                let c = egui::pos2(r.left() + 5.0 + k as f32 * 9.0, r.center().y - phase * 2.5);
                ui.painter().circle_filled(c, 2.6, akzent.gamma_multiply(0.45 + 0.55 * phase));
            }
            ui.label(RichText::new(format!("{taetigkeit} …")).color(pal.text));
            let sekunden = self.seit.map(|s| s.elapsed().as_secs()).unwrap_or(0);
            ui.label(RichText::new(crate::tr_format!("{sekunden} s · Esc bricht ab", "{sekunden}s · Esc to stop")).size(11.5).color(pal.text_weak));
        });
        ui.add_space(6.0);
    }
}

/// Fehlermeldungen, die auf eine fehlende Anmeldung deuten
fn anmeldefehler(e: &str) -> bool {
    let l = e.to_lowercase();
    l.contains("log in") || l.contains("login") || l.contains("logged") || l.contains("authent") || l.contains("api key") || l.contains("unauthorized")
}

/// "claude-opus-5-5" wird "Opus 5.5", "gpt-5.2-codex" bleibt lesbar
fn kurzer_modellname(id: &str) -> String {
    let rest = id.strip_prefix("claude-").unwrap_or(id);
    let teile = rest.split('-').collect::<Vec<_>>();
    if id.starts_with("claude-") && teile.len() >= 2 && teile[1].chars().all(|c| c.is_ascii_digit()) {
        let mut name = teile[0].to_string();
        if let Some(c) = name.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        let version = teile[1..].iter().take_while(|t| t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit())).copied().collect::<Vec<_>>().join(".");
        return format!("{name} {version}");
    }
    id.to_string()
}

// ----------------------------------------------------------------------
// Bausteine

/// Lesespalte: in breiten Fenstern mittig und begrenzt, in der Seitenleiste mit kleinem Rand
fn spalte<R>(ui: &mut egui::Ui, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let frei = ui.available_width();
    let rand = 10.0;
    let breite = (frei - 2.0 * rand).clamp(40.0, SPALTE);
    let links = ((frei - breite) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(links);
        ui.vertical(|ui| {
            ui.set_width(breite);
            inhalt(ui)
        })
        .inner
    })
    .inner
}

/// Abgerundete Flaeche ueber die ganze Breite
fn karte<R>(ui: &mut egui::Ui, fill: Color32, rand: i8, inhalt: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let pal = crate::style::pal();
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, pal.border.gamma_multiply(0.6)))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(rand))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            inhalt(ui)
        })
        .inner
}

/// Logo im farbigen Kreis
fn logo(ui: &mut egui::Ui, icon: Icon, farbe: Color32, groesse: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(groesse, groesse), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), groesse / 2.0, farbe.gamma_multiply(0.18));
    icons::paint(ui.painter(), r.shrink(groesse * 0.22), icon, farbe);
}

/// Symbolknopf der Kopfzeile (28 px, runde Flaeche beim Zeigen)
fn kopf_knopf(ui: &mut egui::Ui, icon: Icon, an: bool, tipp: &str) -> egui::Response {
    let pal = crate::style::pal();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    if an || resp.hovered() {
        ui.painter().rect_filled(r, 6.0, if an { pal.selection } else { pal.hover });
    }
    icons::paint(ui.painter(), r.shrink(7.0), icon, if an || resp.hovered() { pal.text } else { pal.text_weak });
    resp.on_hover_text(tipp)
}

/// Pillenknopf; `voll`: gefuellt in der Markenfarbe
fn pill(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, farbe: Color32, voll: bool) -> egui::Response {
    let pal = crate::style::pal();
    let font = egui::FontId::proportional(12.5);
    let g = ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let breite = g.size().x + if icon.is_some() { 34.0 } else { 20.0 };
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, 26.0), egui::Sense::click());
    let (fill, fg) = if voll { (if resp.hovered() { farbe.gamma_multiply(1.12) } else { farbe }, Color32::WHITE) } else { (if resp.hovered() { pal.hover } else { Color32::TRANSPARENT }, pal.text) };
    ui.painter().rect(r, 13.0, fill, if voll { egui::Stroke::NONE } else { egui::Stroke::new(1.0, pal.border) }, egui::StrokeKind::Inside);
    let mut x = r.left() + 10.0;
    if let Some(i) = icon {
        icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(x + 6.0, r.center().y), egui::vec2(12.0, 12.0)), i, fg);
        x += 16.0;
    }
    ui.painter().galley(egui::pos2(x, r.center().y - g.size().y / 2.0), g, fg);
    resp
}

/// Kleiner Knopf in der Eingabekarte (Ordner, Rechte); ohne Text nur das Symbol
fn chip(ui: &mut egui::Ui, icon: Icon, text: &str, betont: bool) -> egui::Response {
    let pal = crate::style::pal();
    let font = egui::FontId::proportional(12.0);
    let g = (!text.is_empty()).then(|| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, pal.text_weak);
        job.wrap = egui::text::TextWrapping::truncate_at_width(130.0);
        ui.painter().layout_job(job)
    });
    let breite = g.as_ref().map_or(28.0, |g| g.size().x + 32.0);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, 26.0), egui::Sense::click());
    let farbe = if betont { pal.warn_text } else if resp.hovered() { pal.text } else { pal.text_weak };
    if resp.hovered() || betont {
        ui.painter().rect_filled(r, 13.0, if betont { pal.warn_text.gamma_multiply(0.12) } else { pal.hover });
    }
    let symbol = if g.is_some() { r.left() + 15.0 } else { r.center().x };
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(symbol, r.center().y), egui::vec2(13.0, 13.0)), icon, farbe);
    if let Some(g) = g {
        ui.painter().galley(egui::pos2(r.left() + 25.0, r.center().y - g.size().y / 2.0), g, farbe);
    }
    resp
}

/// Text mit Pfeil nach unten (Modellwahl), hoechstens `max` breit
fn chip_rechts(ui: &mut egui::Ui, text: &str, max: f32) -> egui::Response {
    let pal = crate::style::pal();
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), egui::FontId::proportional(12.5), pal.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max - 30.0).max(20.0));
    let g = ui.painter().layout_job(job);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 30.0, 26.0), egui::Sense::click());
    let farbe = if resp.hovered() { pal.text } else { pal.text.gamma_multiply(0.85) };
    if resp.hovered() {
        ui.painter().rect_filled(r, 13.0, pal.hover);
    }
    ui.painter().galley(egui::pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, farbe);
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.right() - 12.0, r.center().y), egui::vec2(11.0, 11.0)), Icon::ChevronDown, pal.text_weak);
    resp
}

/// Runder Senden-/Stopp-Knopf
fn senden_knopf(ui: &mut egui::Ui, laeuft: bool, aktiv: bool, akzent: Color32) -> egui::Response {
    let pal = crate::style::pal();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
    let fuellung = if aktiv { if laeuft { pal.text } else { akzent } } else { pal.face_light };
    let fuellung = if resp.hovered() && aktiv { fuellung.gamma_multiply(1.12) } else { fuellung };
    ui.painter().circle_filled(r.center(), 15.0, fuellung);
    if laeuft {
        ui.painter().rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(10.0, 10.0)), 2.0, pal.bg);
    } else {
        icons::paint(ui.painter(), r.shrink(8.0), Icon::ArrowUp, if aktiv { Color32::WHITE } else { pal.text_weak });
    }
    resp.on_hover_text(if laeuft { tr("Stoppen (Esc)", "Stop (Esc)") } else { tr("Senden (Enter)", "Send (Enter)") })
}

/// Belegter Kontext als kleiner Ring
fn kontext_ring(ui: &mut egui::Ui, tokens: u64, kontext: Option<u64>) {
    let pal = crate::style::pal();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 26.0), egui::Sense::hover());
    let anteil = kontext.map(|k| (tokens as f32 / k.max(1) as f32).clamp(0.0, 1.0));
    let c = r.center();
    ui.painter().circle_stroke(c, 7.0, egui::Stroke::new(2.0, pal.border));
    if let Some(a) = anteil {
        let farbe = if a > 0.85 { pal.error_text } else if a > 0.6 { pal.warn_text } else { pal.text_weak };
        let punkte = (0..=32)
            .map(|k| {
                let w = -std::f32::consts::FRAC_PI_2 + a * std::f32::consts::TAU * k as f32 / 32.0;
                c + 7.0 * egui::vec2(w.cos(), w.sin())
            })
            .collect::<Vec<_>>();
        ui.painter().add(egui::Shape::line(punkte, egui::Stroke::new(2.0, farbe)));
    }
    let text = match anteil {
        Some(a) => crate::tr_format!("Kontext {:.0} % belegt ({tokens} Tokens)", "Context {:.0}% used ({tokens} tokens)", a * 100.0),
        None => crate::tr_format!("Kontext: {tokens} Tokens", "Context: {tokens} tokens"),
    };
    resp.on_hover_text(text);
}

/// Segment fuer den Denkaufwand
fn segment(ui: &mut egui::Ui, text: &str, an: bool, akzent: Color32) -> egui::Response {
    let pal = crate::style::pal();
    let g = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(12.0), pal.text);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 20.0, 24.0), egui::Sense::click());
    let fill = if an { akzent.gamma_multiply(0.22) } else if resp.hovered() { pal.hover } else { pal.face_light };
    ui.painter().rect(r, 12.0, fill, egui::Stroke::new(1.0, if an { akzent } else { pal.border.gamma_multiply(0.7) }), egui::StrokeKind::Inside);
    ui.painter().galley(egui::pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0), g, if an { pal.text } else { pal.text_weak });
    resp
}

/// Auswahlzeile mit Symbol, Titel, Beschreibung und Haken
fn auswahl_zeile(ui: &mut egui::Ui, icon: Option<Icon>, titel: &str, text: &str, an: bool, akzent: Color32) -> egui::Response {
    let pal = crate::style::pal();
    let breite = ui.available_width();
    let text_breite = breite - 64.0;
    let titel_g = ui.painter().layout(titel.to_string(), egui::FontId::proportional(13.0), pal.text, text_breite);
    let text_g = (!text.is_empty()).then(|| {
        let mut job = egui::text::LayoutJob::simple(text.to_string(), egui::FontId::proportional(11.5), pal.text_weak, text_breite);
        job.wrap.max_rows = 2;
        ui.painter().layout_job(job)
    });
    let hoehe = titel_g.size().y + text_g.as_ref().map_or(0.0, |g| g.size().y + 2.0) + 12.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, hoehe.max(32.0)), egui::Sense::click());
    if resp.hovered() || an {
        ui.painter().rect_filled(r, 8.0, if an { akzent.gamma_multiply(0.12) } else { pal.hover });
    }
    let x = if let Some(icon) = icon {
        icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 16.0, r.top() + 6.0 + titel_g.size().y / 2.0), egui::vec2(14.0, 14.0)), icon, if an { akzent } else { pal.text_weak });
        r.left() + 32.0
    } else {
        r.left() + 12.0
    };
    let mut y = r.top() + 6.0;
    let th = titel_g.size().y;
    ui.painter().galley(egui::pos2(x, y), titel_g, pal.text);
    y += th + 2.0;
    if let Some(g) = text_g {
        ui.painter().galley(egui::pos2(x, y), g, pal.text_weak);
    }
    if an {
        icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.right() - 16.0, r.center().y), egui::vec2(14.0, 14.0)), Icon::Check, akzent);
    }
    resp
}

/// Suchfeld mit Lupe und runden Ecken
fn suchfeld(ui: &mut egui::Ui, text: &mut String, hinweis: &str) {
    let pal = crate::style::pal();
    egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(8.0).inner_margin(egui::Margin { left: 8, right: 6, top: 5, bottom: 5 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, Icon::Search, pal.text_weak);
            ui.add(egui::TextEdit::singleline(text).frame(egui::Frame::NONE).desired_width(f32::INFINITY).hint_text(hinweis));
        });
    });
}

/// Zeile der Verlaufsliste; `Some(true)` oeffnen, `Some(false)` loeschen
fn chat_zeile(ui: &mut egui::Ui, e: &ChatEintrag, aktiv: bool) -> Option<bool> {
    let pal = crate::style::pal();
    let breite = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, 46.0), egui::Sense::click());
    let zeigen = resp.hovered() || ui.rect_contains_pointer(r);
    if aktiv || zeigen {
        ui.painter().rect_filled(r, 8.0, if aktiv { pal.selection } else { pal.hover });
    }
    let platz = breite - if zeigen { 44.0 } else { 20.0 };
    let mut job = egui::text::LayoutJob::simple_singleline(e.titel.clone(), egui::FontId::proportional(13.0), pal.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(platz);
    let titel = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(r.left() + 10.0, r.top() + 7.0), titel, pal.text);
    let ordner = e.ordner.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let alter = alter_text(e.zeit);
    let unten = [ordner, alter, crate::tr_format!("{} Nachrichten", "{} messages", e.anzahl)].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ");
    let mut job = egui::text::LayoutJob::simple_singleline(unten, egui::FontId::proportional(11.0), pal.text_weak);
    job.wrap = egui::text::TextWrapping::truncate_at_width(platz);
    let unten = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(r.left() + 10.0, r.top() + 26.0), unten, pal.text_weak);
    let mut ergebnis = None;
    if zeigen {
        let knopf = egui::Rect::from_center_size(egui::pos2(r.right() - 20.0, r.center().y), egui::vec2(26.0, 26.0));
        let k = ui.interact(knopf, ui.id().with(("loeschen", &e.pfad)), egui::Sense::click());
        if k.hovered() {
            ui.painter().rect_filled(knopf, 6.0, pal.error_text.gamma_multiply(0.15));
        }
        icons::paint(ui.painter(), knopf.shrink(6.0), Icon::Trash, if k.hovered() { pal.error_text } else { pal.text_weak });
        if k.on_hover_text(tr("Chat löschen", "Delete chat")).clicked() {
            ergebnis = Some(false);
        }
    }
    if ergebnis.is_none() && resp.clicked() {
        ergebnis = Some(true);
    }
    ui.add_space(2.0);
    ergebnis
}

fn alter_text(zeit: SystemTime) -> String {
    let s = SystemTime::now().duration_since(zeit).unwrap_or_default().as_secs();
    match s {
        0..=59 => tr("gerade eben", "just now").into(),
        60..=3599 => crate::tr_format!("vor {} min", "{} min ago", s / 60),
        3600..=86_399 => crate::tr_format!("vor {} h", "{} h ago", s / 3600),
        _ => crate::tr_format!("vor {} Tagen", "{} days ago", s / 86_400),
    }
}

/// Hinweis mit Knopf; true = Knopf geklickt
fn hinweis_karte(ui: &mut egui::Ui, icon: Icon, titel: String, text: &str, knopf: Option<&str>, akzent: Color32) -> bool {
    let pal = crate::style::pal();
    let mut geklickt = false;
    egui::Frame::new()
        .fill(akzent.gamma_multiply(0.08))
        .stroke(egui::Stroke::new(1.0, akzent.gamma_multiply(0.4)))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, icon, akzent);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(titel).strong());
                    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(pal.text_weak)).wrap());
                    if let Some(k) = knopf {
                        ui.add_space(6.0);
                        geklickt = pill(ui, None, k, akzent, true).clicked();
                    }
                });
            });
        });
    ui.add_space(10.0);
    geklickt
}

/// Begruessung und Vorschlaege fuer den ersten Satz
fn leerer_chat(ui: &mut egui::Ui, ziel: Target, icon: Icon, klicks: &mut Klicks) {
    let pal = crate::style::pal();
    let akzent = farbe(ziel);
    ui.add_space(14.0);
    ui.vertical_centered(|ui| {
        logo(ui, icon, akzent, 44.0);
        ui.add_space(12.0);
        ui.label(RichText::new(tr("Wie kann ich helfen?", "How can I help?")).size(20.0).strong());
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(tr("Fragen zur Datenbank, SQL schreiben und prüfen, Code im Projekt ändern.", "Ask about your database, write and review SQL, change code in your project.")).color(pal.text_weak)).wrap());
    });
    ui.add_space(18.0);
    let vorschlaege = [
        (Icon::Database, tr("Erkläre das Schema meiner Datenbank", "Explain my database schema")),
        (Icon::Explain, tr("Finde langsame Abfragen und schlage Indizes vor", "Find slow queries and suggest indexes")),
        (Icon::Code, tr("Schreibe eine SQL-Abfrage für …", "Write an SQL query that …")),
        (Icon::Bug, tr("Prüfe den Code im Projekt auf Fehler", "Review the project code for bugs")),
    ];
    let breite = ui.available_width();
    let zwei = breite > 420.0;
    let w = if zwei { (breite - 8.0) / 2.0 } else { breite };
    for zeile in vorschlaege.chunks(if zwei { 2 } else { 1 }) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (icon, text) in zeile {
                let g = ui.painter().layout(text.to_string(), egui::FontId::proportional(12.5), pal.text, w - 44.0);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(w, (g.size().y + 22.0).max(42.0)), egui::Sense::click());
                ui.painter().rect(r, 12.0, if resp.hovered() { pal.hover } else { pal.face_light.gamma_multiply(0.7) }, egui::Stroke::new(1.0, pal.border.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 18.0, r.center().y), egui::vec2(14.0, 14.0)), *icon, akzent);
                ui.painter().galley(egui::pos2(r.left() + 34.0, r.center().y - g.size().y / 2.0), g, pal.text);
                if resp.clicked() {
                    klicks.vorschlag = Some(text.trim_end_matches(" …").trim_end_matches('…').to_string());
                }
            }
        });
        ui.add_space(8.0);
    }
    ui.add_space(8.0);
}

// ----------------------------------------------------------------------
// Nachrichten

fn absatz(ui: &mut egui::Ui, text: &str) {
    let pal = crate::style::pal();
    for zeile in text.trim().lines() {
        let trimmed = zeile.trim_start();
        // Trennlinie
        if matches!(trimmed, "---" | "***" | "___") {
            ui.add_space(4.0);
            let r = ui.cursor();
            ui.painter().hline(r.x_range(), r.top(), egui::Stroke::new(1.0, pal.border));
            ui.add_space(6.0);
            continue;
        }
        let (zeile, stufe) = if let Some(s) = zeile.strip_prefix("### ") {
            (s, 3)
        } else if let Some(s) = zeile.strip_prefix("## ") {
            (s, 2)
        } else if let Some(s) = zeile.strip_prefix("# ") {
            (s, 1)
        } else {
            (zeile, 0)
        };
        let einzug = zeile.len() - zeile.trim_start().len();
        let rest = zeile.trim_start();
        // Zitat, Aufzaehlung, nummerierte Liste
        let (rest, zitat) = match rest.strip_prefix("> ") {
            Some(r) => (r, true),
            None => (rest, false),
        };
        let nummer = rest.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()));
        let (rest, marke) = if let Some(r) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
            (r, Some("•".to_string()))
        } else if let Some((n, r)) = nummer {
            (r, Some(format!("{n}.")))
        } else {
            (rest, None)
        };
        let groesse = match stufe {
            1 => 17.0,
            2 => 15.5,
            3 => 14.5,
            _ => 13.5,
        };
        let mut job = egui::text::LayoutJob::default();
        if let Some(m) = &marke {
            job.append(&format!("{m}  "), einzug as f32 * 4.0 + 2.0, egui::TextFormat { font_id: egui::FontId::proportional(13.5), color: pal.text_weak, ..Default::default() });
        }
        let (mut rest, mut fett, mut code) = (rest, stufe > 0, false);
        while !rest.is_empty() {
            let i = rest.find('`').into_iter().chain(rest.find("**")).min().unwrap_or(rest.len());
            let format = egui::TextFormat {
                font_id: if code { egui::FontId::monospace(12.5) } else { egui::FontId::proportional(groesse) },
                color: if zitat { pal.text_weak } else if fett || code { pal.text } else { pal.text.gamma_multiply(0.93) },
                background: if code { pal.face_light } else { Color32::TRANSPARENT },
                line_height: Some(groesse * 1.45),
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
            ui.add_space(6.0);
            continue;
        }
        if stufe > 0 {
            ui.add_space(6.0);
        }
        if zitat {
            let oben = ui.cursor().top();
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.add(egui::Label::new(job).selectable(true).wrap());
            });
            let r = egui::Rect::from_x_y_ranges(ui.cursor().left()..=ui.cursor().left() + 3.0, oben..=ui.cursor().top() - 2.0);
            ui.painter().rect_filled(r, 1.5, pal.border);
        } else {
            ui.add(egui::Label::new(job).selectable(true).wrap());
        }
    }
}

/// Text mit Codebloecken; auch ein noch offener Codeblock bleibt beim Streaming als Code lesbar.
fn nachricht(ui: &mut egui::Ui, text: &str, cx: &mut Ctx) {
    let pal = crate::style::pal();
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
        ui.add_space(4.0);
        egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border.gamma_multiply(0.8))).corner_radius(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Kopf mit Sprache und Knoepfen
            egui::Frame::new().fill(pal.face_light.gamma_multiply(0.6)).corner_radius(egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }).inner_margin(egui::Margin { left: 10, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let s = sprache.trim();
                    ui.label(RichText::new(if s.is_empty() { "code" } else { s }).size(11.5).color(pal.text_weak));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Copy, tr("Code kopieren", "Copy code")).clicked() {
                            ui.ctx().copy_text(code.into());
                        }
                        if s.eq_ignore_ascii_case("sql") && icons::button(ui, Icon::External, tr("Im SQL-Editor öffnen", "Open in SQL editor")).clicked() {
                            cx.actions.push(Action::OpenSql { db: None, sql: code.into(), run: false });
                        }
                    });
                });
            });
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 8, bottom: 8 }).show(ui, |ui| {
                egui::ScrollArea::horizontal().id_salt(ui.next_auto_id()).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(code.trim_end()).monospace().size(12.5).color(pal.text)).selectable(true).extend());
                });
            });
        });
        ui.add_space(4.0);
        rest = danach;
    }
    if !rest.trim().is_empty() {
        absatz(ui, rest);
    }
}

/// Eine Nachricht im Verlauf. `nach_nutzer`: erste Antwort nach einer eigenen Nachricht (dann mit Logo und Namen)
fn eintrag(ui: &mut egui::Ui, nutzer: bool, n: &Nachricht, ziel: Target, nach_nutzer: bool, cx: &mut Ctx) {
    let pal = crate::style::pal();
    let akzent = farbe(ziel);
    if n.rolle == Rolle::Status {
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            egui::Frame::new().fill(pal.face_light).corner_radius(10.0).inner_margin(egui::Margin::symmetric(10, 3)).show(ui, |ui| {
                ui.label(RichText::new(&n.text).size(11.0).color(pal.text_weak));
            });
        });
        ui.add_space(8.0);
        return;
    }
    if n.rolle == Rolle::Werkzeug {
        werkzeug(ui, n, akzent);
        return;
    }
    if nutzer {
        // Eigene Nachricht als Blase rechts, hoechstens 85 % breit
        ui.add_space(6.0);
        let breite = ui.available_width();
        let max = (breite * 0.85).max(120.0);
        let g = ui.painter().layout(n.text.clone(), egui::FontId::proportional(13.5), pal.text, max - 26.0);
        let w = (g.size().x + 26.0).min(max);
        ui.horizontal(|ui| {
            ui.add_space((breite - w).max(0.0));
            egui::Frame::new().fill(if pal.dark { pal.face_light } else { akzent.gamma_multiply(0.12) }).corner_radius(egui::CornerRadius { nw: 16, ne: 16, sw: 16, se: 4 }).inner_margin(egui::Margin::symmetric(13, 9)).show(ui, |ui| {
                ui.set_max_width(w - 26.0);
                ui.add(egui::Label::new(RichText::new(&n.text).size(13.5).color(pal.text)).selectable(true).wrap());
            });
        });
        ui.add_space(10.0);
        return;
    }
    if nach_nutzer {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            logo(ui, if ziel == Target::Claude { Icon::Claude } else { Icon::Codex }, akzent, 20.0);
            ui.label(RichText::new(kurzname(ziel)).strong().size(12.5));
        });
        ui.add_space(2.0);
    }
    let oben = ui.cursor().min;
    nachricht(ui, &n.text, cx);
    // Kopieren unter der Antwort, sichtbar beim Zeigen
    let flaeche = egui::Rect::from_min_max(oben, egui::pos2(ui.max_rect().right(), ui.cursor().min.y + 24.0));
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    if n.fertig && ui.rect_contains_pointer(flaeche) {
        let knopf = egui::Rect::from_min_size(r.min, egui::vec2(24.0, 24.0));
        if ui.scope_builder(egui::UiBuilder::new().max_rect(knopf), |ui| icons::button(ui, Icon::Copy, tr("Antwort kopieren", "Copy answer"))).inner.clicked() {
            ui.ctx().copy_text(n.text.clone());
        }
    }
    ui.add_space(4.0);
}

/// Werkzeugaufruf als kompakte Zeile; aufklappbar fuer Eingabe und Ergebnis
fn werkzeug(ui: &mut egui::Ui, n: &Nachricht, akzent: Color32) {
    let pal = crate::style::pal();
    let id = ui.id().with(("werkzeug", &n.id));
    let mut offen = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    let breite = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(breite, 28.0), egui::Sense::click());
    if resp.hovered() || offen {
        ui.painter().rect_filled(r, 8.0, pal.face_light.gamma_multiply(if offen { 0.9 } else { 0.6 }));
    }
    let c = egui::pos2(r.left() + 12.0, r.center().y);
    if n.fertig {
        icons::paint(ui.painter(), egui::Rect::from_center_size(c, egui::vec2(12.0, 12.0)), Icon::Check, pal.ok_text);
    } else {
        // Drehender Bogen
        let t = ui.input(|i| i.time) as f32;
        let punkte = (0..=16)
            .map(|k| {
                let w = t * 6.0 + k as f32 / 16.0 * 4.4;
                c + 5.0 * egui::vec2(w.cos(), w.sin())
            })
            .collect::<Vec<_>>();
        ui.painter().add(egui::Shape::line(punkte, egui::Stroke::new(1.6, akzent)));
    }
    let (name, arg) = n.titel.split_once('(').map(|(a, b)| (a, b.trim_end_matches(')'))).unwrap_or((&n.titel, ""));
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: pal.text, ..Default::default() });
    if !arg.is_empty() {
        job.append(&format!("  {arg}"), 0.0, egui::TextFormat { font_id: egui::FontId::monospace(11.5), color: pal.text_weak, ..Default::default() });
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width((breite - 50.0).max(20.0));
    let g = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(r.left() + 26.0, r.center().y - g.size().y / 2.0), g, pal.text);
    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.right() - 12.0, r.center().y), egui::vec2(11.0, 11.0)), if offen { Icon::ChevronDown } else { Icon::ChevronRight }, pal.text_weak);
    if resp.on_hover_text(&n.titel).clicked() {
        offen = !offen;
        ui.data_mut(|d| d.insert_temp(id, offen));
    }
    if offen {
        egui::Frame::new().fill(pal.bg).stroke(egui::Stroke::new(1.0, pal.border.gamma_multiply(0.7))).corner_radius(8.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(240.0).id_salt(("werkzeug-text", &n.id)).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(&n.text).monospace().size(11.5).color(pal.text_weak)).selectable(true).wrap());
            });
        });
    }
    ui.add_space(2.0);
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
        let installiert = self.ziel.program().is_some();
        self.konto_pruefen(ui.ctx(), false);
        self.verbinden(ui, false);
        if self.laeuft()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && let Some(v) = &self.verbindung
        {
            v.unterbrechen();
        }
        self.kopf(ui, cx, installiert);
        if self.ansicht == Ansicht::Verlauf {
            self.verlauf_ui(ui);
            return;
        }
        egui::Panel::bottom(ui.id().with("chat-eingabe")).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 8 })).show(ui, |ui| {
            self.eingabefeld(ui, cx, installiert);
        });
        let laeuft = self.laeuft();
        let verbindet = self.verbindung.as_ref().is_some_and(|_| !self.live.bereit && !self.live.beendet);
        if laeuft || verbindet || self.auftrag.is_some() {
            // Animation mit 30 Bildern je Sekunde; neue Daten zeichnen sofort (der Lesefaden weckt die Oberflaeche)
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        let klicks = self.chat_ui(ui, cx, installiert);
        if klicks.installieren {
            self.verbindung = None;
            self.auftrag = Some(Auftrag::installieren(ui.ctx(), self.ziel));
        }
        if klicks.anmelden {
            self.anmelden(cx);
        }
        if klicks.ordner
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            self.ordner = Some(p);
        }
        if let Some(text) = klicks.vorschlag {
            // Offene Vorschlaege ("… fuer") zum Weiterschreiben ins Feld, fertige gleich senden
            let offen = text.ends_with("für") || text.ends_with("that");
            self.eingabe = if offen { format!("{text} ") } else { text };
            if !offen && installiert && self.ordner.is_some() {
                self.senden(ui);
            }
        }
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
    #[ignore = "Messung: langer Verlauf"]
    fn langer_verlauf_bleibt_fluessig() {
        let ctx = egui::Context::default();
        crate::style::apply(&ctx, true);
        let server = crate::server::Server::new(Default::default());
        let mut settings = crate::settings::Settings::default();
        let mut schemas = super::super::SchemaCache::default();
        let mut actions = Vec::new();
        let mut tab = AiTab::new(Target::Claude, Some(PathBuf::from("projekt")));
        tab.start_versucht = true;
        tab.konto = Some(Konto { angemeldet: Some(true), art: "Test".into() });
        let antwort = "## Ergebnis\nText mit **fett** und `code`.\n- Punkt eins\n- Punkt zwei\n```sql\nSELECT * FROM t WHERE id = 1;\n```\nNoch ein Absatz mit etwas mehr Text, der umbrechen muss, damit es realistisch ist.";
        for i in 0..200 {
            tab.verlauf.push((true, Nachricht { id: format!("u{i}"), rolle: Rolle::Assistent, titel: String::new(), text: format!("Frage {i}"), fertig: true }));
            tab.verlauf.push((false, Nachricht { id: format!("a{i}"), rolle: Rolle::Assistent, titel: String::new(), text: antwort.into(), fertig: true }));
        }
        let mut bild = |tab: &mut AiTab| {
            let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(380.0, 800.0))), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    tab.ui(ui, &mut Ctx { db: None, schemas: &mut schemas, databases: &[], server: &server, project: None, settings: &mut settings, history: &[], update: Default::default(), actions: &mut actions });
                });
            });
        };
        let start = std::time::Instant::now();
        bild(&mut tab);
        println!("erstes Bild: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        for _ in 0..5 { bild(&mut tab); }
        let start = std::time::Instant::now();
        for _ in 0..20 { bild(&mut tab); }
        println!("400 Nachrichten: {:.2} ms/Bild", start.elapsed().as_secs_f64() * 1000.0 / 20.0);
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
