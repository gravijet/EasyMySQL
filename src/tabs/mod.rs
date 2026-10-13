// Registerkarten im Hauptbereich.

pub mod builder;
pub mod data;
pub mod designer;
pub mod er;
pub mod help;
mod help_content;
pub mod import;
pub mod log;
pub mod preview;
pub mod safety;
pub mod settings;
pub mod sql;
pub mod structure;
pub mod users;

use crate::db::{Db, Schema};
use crate::keymap::Cmd;
use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Aktionen, die eine Registerkarte beim Hauptfenster anfordert.
pub enum Action {
    OpenData { db: String, table: String },
    OpenStructure { db: String, table: String },
    OpenSql { db: Option<String>, sql: String, run: bool },
    OpenEr(String),
    OpenBuilder(Option<String>),
    NewTable(String),
    /// Datenbankliste und alle Strukturen neu laden
    RefreshAll,
    /// Struktur einer Datenbank hat sich geaendert
    SchemaChanged(String),
    Status(String),
    Error(String),
    /// Nach Bestaetigung SQL ausfuehren
    Confirm { text: String, db: Option<String>, sql: String },
    /// Nach Server-Rettung/Neustart wieder verbinden
    Reconnect,
    /// Verbindung trennen (z. B. vor der Server-Rettung)
    Disconnect,
    OpenSafety,
    OpenImport,
    OpenUsers,
    /// Dateien im Projekt haben sich geaendert (Explorer neu einlesen)
    FilesChanged,
    /// Datei oeffnen und zu einer Zeile (0-basiert) springen
    OpenFileAt { path: std::path::PathBuf, line: usize },
    /// Hilfe oeffnen, optional bei einem Abschnitt
    OpenHelp(Option<String>),
    /// Einstellungen oeffnen, optional bei einem Abschnitt
    OpenSettings(Option<String>),
    OpenLog,
    /// Text in die Zwischenablage
    CopyText(String),
    /// Befehl ausfuehren (wie per Tastenkuerzel)
    Run(Cmd),
    /// Ausgefuehrte Anweisungen fuer den Verlauf
    History(Vec<crate::qhistory::Entry>),
    /// Einstellungen wurden geaendert (neu anwenden und speichern)
    SettingsChanged,
    /// Verbindungsdaten wurden geaendert: neu verbinden
    Connect,
    /// Speicherordner wurde verschoben (offene Dateien und Projekte anpassen)
    FilesMoved { from: std::path::PathBuf, to: std::path::PathBuf },
}

/// Strukturen der Datenbanken. Gelesen wird im Hintergrund, die Oberflaeche bleibt bedienbar.
pub struct SchemaCache {
    map: HashMap<String, Arc<Schema>>,
    errors: HashMap<String, String>,
    /// Laufende Abfragen: Datenbank -> Nummer (veraltete Ergebnisse werden verworfen)
    pending: HashMap<String, u64>,
    counter: u64,
    tx: Sender<(String, u64, Result<Schema, String>)>,
    rx: Receiver<(String, u64, Result<Schema, String>)>,
    repaint: Option<egui::Context>,
}

impl Default for SchemaCache {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { map: HashMap::new(), errors: HashMap::new(), pending: HashMap::new(), counter: 0, tx, rx, repaint: None }
    }
}

impl SchemaCache {
    pub fn set_repaint(&mut self, ctx: egui::Context) {
        self.repaint = Some(ctx);
    }

    fn poll(&mut self) {
        while let Ok((name, id, r)) = self.rx.try_recv() {
            if self.pending.get(&name) != Some(&id) {
                continue;
            }
            self.pending.remove(&name);
            match r {
                Ok(s) => {
                    self.map.insert(name, Arc::new(s));
                }
                Err(e) => {
                    self.errors.insert(name, e);
                }
            }
        }
    }

    /// Geteilter Zugriff ohne Kopie (Strukturen grosser Datenbanken sind gross).
    pub fn get_arc(&mut self, db: Option<&Db>, name: &str) -> Option<&Arc<Schema>> {
        self.poll();
        if !self.map.contains_key(name) && !self.errors.contains_key(name) && !self.pending.contains_key(name) {
            let db = db?.clone();
            self.counter += 1;
            let id = self.counter;
            self.pending.insert(name.to_string(), id);
            let (tx, name, ctx) = (self.tx.clone(), name.to_string(), self.repaint.clone());
            std::thread::spawn(move || {
                let r = db.schema(&name);
                let _ = tx.send((name, id, r));
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            });
        }
        self.map.get(name)
    }

    /// Wird die Struktur gerade gelesen?
    pub fn is_loading(&mut self, name: &str) -> bool {
        self.poll();
        self.pending.contains_key(name)
    }

    pub fn invalidate(&mut self, name: &str) {
        self.map.remove(name);
        self.errors.remove(name);
        self.pending.remove(name);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.errors.clear();
        self.pending.clear();
    }
}

pub struct Ctx<'a> {
    pub db: Option<&'a Db>,
    pub schemas: &'a mut SchemaCache,
    pub databases: &'a [String],
    pub server: &'a crate::server::Server,
    /// Geoeffneter Projektordner
    pub project: Option<&'a std::path::Path>,
    pub settings: &'a mut crate::settings::Settings,
    /// Verlauf der ausgefuehrten Anweisungen, neueste zuerst
    pub history: &'a [crate::qhistory::Entry],
    pub actions: &'a mut Vec<Action>,
}

impl Ctx<'_> {
    /// Struktur der Datenbank; `None` solange sie noch gelesen wird (siehe `schema_loading`).
    pub fn schema(&mut self, name: &str) -> Option<Arc<Schema>> {
        self.schemas.get_arc(self.db, name).cloned()
    }
    pub fn schema_loading(&mut self, name: &str) -> bool {
        self.schemas.is_loading(name)
    }
    pub fn error(&mut self, e: impl Into<String>) {
        self.actions.push(Action::Error(e.into()));
    }
    pub fn status(&mut self, s: impl Into<String>) {
        self.actions.push(Action::Status(s.into()));
    }
}

/// Ergebnis eines Reiters mit Schliessen-Kreuz
pub struct Chip {
    pub clicked: bool,
    pub closed: bool,
}

/// Kleiner Reiter mit Kreuz (Ergebnisbereich): Klick waehlt, Kreuz oder Mittelklick schliesst.
pub fn chip(ui: &mut egui::Ui, label: &str, color: Option<egui::Color32>, selected: bool) -> Chip {
    let pal = crate::style::pal();
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(12.5), pal.text);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 44.0, 26.0), egui::Sense::click());
    let p = ui.painter();
    let radius = crate::style::RADIUS as f32;
    if selected {
        p.rect(r, radius, pal.face_light, egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(r, radius, pal.hover);
    }
    let fg = color.unwrap_or(if selected { pal.text } else { pal.text_weak });
    p.galley(egui::pos2(r.left() + 12.0, r.center().y - galley.size().y / 2.0), galley, fg);
    let cr = egui::Rect::from_center_size(egui::pos2(r.right() - 15.0, r.center().y), egui::vec2(18.0, 18.0));
    let over = ui.ctx().pointer_hover_pos().is_some_and(|q| cr.contains(q));
    if selected || resp.hovered() {
        if over {
            p.rect_filled(cr, radius - 2.0, pal.hover.gamma_multiply(1.6));
        }
        crate::icons::paint(p, cr.shrink(3.0), crate::icons::Icon::Close, if over { pal.text } else { pal.text_weak });
    }
    Chip { clicked: resp.clicked() && !over, closed: (resp.clicked() && over) || resp.middle_clicked() }
}

/// Bild-rauf/-runter sowie Pos1/Ende scrollen den Bereich, der gleich danach folgt (umgebende oder naechste
/// `ScrollArea`), solange kein Textfeld getippt wird und die Maus ueber dem Bereich ist.
pub fn page_keys(ui: &egui::Ui) {
    let ctx = ui.ctx();
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let area = ui.max_rect().intersect(ui.clip_rect());
    if ctx.pointer_hover_pos().is_some_and(|p| !area.contains(p)) {
        return;
    }
    let page = (area.height() - 30.0).max(40.0);
    let (up, down, home, end) = ctx.input_mut(|i| {
        let n = egui::Modifiers::NONE;
        (i.consume_key(n, egui::Key::PageUp), i.consume_key(n, egui::Key::PageDown), i.consume_key(n, egui::Key::Home), i.consume_key(n, egui::Key::End))
    });
    let delta = if up {
        page
    } else if down {
        -page
    } else if home {
        1.0e7
    } else if end {
        -1.0e7
    } else {
        return;
    };
    ui.scroll_with_delta(egui::vec2(0.0, delta));
}

/// Einzeiliges Namensfeld: beim Oeffnen ist der Name markiert (bei `keep_extension` ohne Endung),
/// sodass man sofort lostippen kann.
pub fn name_field(ui: &mut egui::Ui, id: egui::Id, text: &mut String, width: f32, keep_extension: bool) -> egui::Response {
    use egui::text::{CCursor, CCursorRange};
    let ctx = ui.ctx().clone();
    if !ctx.memory(|m| m.has_focus(id)) {
        let end = if keep_extension { text.rfind('.').filter(|&i| i > 0).unwrap_or(text.len()) } else { text.len() };
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(text[..end].chars().count()))));
        state.store(&ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
    }
    ui.add(egui::TextEdit::singleline(text).id(id).desired_width(width))
}

pub trait TabView {
    fn title(&self) -> String;
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx);
    /// F5 / Ausfuehren
    fn execute(&mut self, _cx: &mut Ctx) {}
    /// Befehl fuer diese Registerkarte (Formatieren, Suchen, ...). true = erledigt
    fn command(&mut self, cmd: Cmd, cx: &mut Ctx) -> bool {
        if cmd == Cmd::RunAll {
            self.execute(cx);
            return true;
        }
        false
    }
    /// Wird aufgerufen, wenn sich die Struktur einer Datenbank geaendert hat.
    fn schema_changed(&mut self, _db: &str, _cx: &mut Ctx) {}
    /// Gleiche Registerkarte schon offen? (fuer Wiederverwendung)
    fn key(&self) -> Option<String> {
        None
    }
    fn busy(&self) -> bool {
        false
    }
    /// Symbol vor dem Titel in der Registerkartenleiste
    fn icon(&self) -> Option<crate::icons::Icon> {
        None
    }
    /// Hat die Datei Aenderungen, die noch nicht gespeichert sind? (Punkt statt Kreuz)
    fn modified(&self) -> bool {
        false
    }
    /// Eintrag fuer die Sitzungsdatei (zum Wiederherstellen beim Start)
    fn session(&self) -> Option<String> {
        None
    }
    /// Zugehoerige Datei (fuer den Explorer)
    fn file(&self) -> Option<&std::path::Path> {
        None
    }
    /// Verarbeitet die Registerkarte selbst, wenn diese Datei auf das Fenster gezogen wird?
    fn takes_drop(&self, _path: &std::path::Path) -> bool {
        false
    }
    /// Datei/Ordner wurde im Explorer umbenannt
    fn file_renamed(&mut self, _old: &std::path::Path, _new: &std::path::Path) {}
    /// Vor dem Schliessen (speichern). false = nicht schliessen
    fn on_close(&mut self) -> bool {
        true
    }
    fn save_now(&mut self) {}
    /// Name der Datei, wenn sie Aenderungen hat, die ohne Autosave noch nicht gespeichert sind
    fn unsaved(&self) -> Option<String> {
        None
    }
    /// Nicht gespeicherte Aenderungen verwerfen (nach der Rueckfrage)
    fn discard_changes(&mut self) {}
    /// Speichern ohne Rueckfrage (beim Beenden, Projektwechsel)
    fn save_now_quiet(&mut self) {
        self.save_now();
    }
    /// Zu einer Zeile springen (Suchergebnisse)
    fn goto_line(&mut self, _line: usize) {}
    /// Abschnitt zeigen (Hilfe, Einstellungen)
    fn show_section(&mut self, _id: &str) {}
}

/// Auswahlfeld fuer eine Datenbank. Liefert true bei Aenderung.
/// Abschnittsueberschrift einer Seite: kleiner, abgesetzter Text mit einheitlichem Abstand
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(text).size(12.5).strong().color(crate::style::pal().text_weak));
    ui.add_space(4.0);
}

/// Systemdatenbanken in Auswahllisten zeigen? (Einstellung)
pub static SHOW_SYSTEM_DBS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_system_db(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "information_schema" | "mysql" | "performance_schema" | "sys")
}

pub fn db_combo(ui: &mut egui::Ui, id: &str, dbs: &[String], value: &mut String) -> bool {
    let mut changed = false;
    let show_system = SHOW_SYSTEM_DBS.load(std::sync::atomic::Ordering::Relaxed);
    let dbs: Vec<&String> = dbs.iter().filter(|d| show_system || !is_system_db(d) || *d == value).collect();
    crate::widgets::combo(id)
        .selected_text(if value.is_empty() { crate::i18n::text("(keine)") } else { value.as_str() })
        .width(170.0)
        .show_ui(ui, |ui| {
            for d in dbs {
                if ui.selectable_label(value == d, d.as_str()).clicked() {
                    *value = d.clone();
                    changed = true;
                }
            }
        });
    changed
}

/// Einfaches Auswahlfeld fuer eine Liste von Strings.
pub fn str_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, items: &[String], value: &mut String, width: f32) -> bool {
    let mut changed = false;
    crate::widgets::combo(id)
        .selected_text(if value.is_empty() { "–" } else { value.as_str() })
        .width(width)
        .show_ui(ui, |ui| {
            for it in items {
                if ui.selectable_label(value == it, if it.is_empty() { "–" } else { it.as_str() }).clicked() {
                    *value = it.clone();
                    changed = true;
                }
            }
        });
    changed
}
