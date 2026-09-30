// Registerkarten im Hauptbereich.

pub mod builder;
pub mod data;
pub mod designer;
pub mod er;
mod er_layout;
pub mod help;
mod help_content;
pub mod log;
pub mod safety;
pub mod settings;
pub mod sql;
pub mod structure;

use crate::db::{Db, Schema};
use crate::keymap::Cmd;
use eframe::egui;
use std::collections::HashMap;

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
    /// Dateien im Projekt haben sich geaendert (Explorer neu einlesen)
    FilesChanged,
    /// Datei oeffnen und zu einer Zeile (0-basiert) springen
    OpenFileAt { path: std::path::PathBuf, line: usize },
    /// Hilfe oeffnen, optional bei einem Abschnitt
    OpenHelp(Option<String>),
    /// Einstellungen oeffnen, optional bei einem Abschnitt
    OpenSettings(Option<String>),
    OpenLog,
    /// Befehl ausfuehren (wie per Tastenkuerzel)
    Run(Cmd),
    /// Ausgefuehrte Anweisungen fuer den Verlauf
    History(Vec<crate::qhistory::Entry>),
    /// Einstellungen wurden geaendert (neu anwenden und speichern)
    SettingsChanged,
    /// Verbindungsdaten wurden geaendert: neu verbinden
    Connect,
    SetupVsCode,
    /// Speicherordner wurde verschoben (offene Dateien und Projekte anpassen)
    FilesMoved { from: std::path::PathBuf, to: std::path::PathBuf },
}

#[derive(Default)]
pub struct SchemaCache {
    map: HashMap<String, Schema>,
    errors: HashMap<String, String>,
}

impl SchemaCache {
    pub fn get(&mut self, db: Option<&Db>, name: &str) -> Option<&Schema> {
        if !self.map.contains_key(name) && !self.errors.contains_key(name) {
            let db = db?;
            match db.schema(name) {
                Ok(s) => {
                    self.map.insert(name.to_string(), s);
                }
                Err(e) => {
                    self.errors.insert(name.to_string(), e);
                }
            }
        }
        self.map.get(name)
    }

    pub fn invalidate(&mut self, name: &str) {
        self.map.remove(name);
        self.errors.remove(name);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.errors.clear();
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
    pub actions: &'a mut Vec<Action>,
}

impl Ctx<'_> {
    pub fn schema(&mut self, name: &str) -> Option<Schema> {
        self.schemas.get(self.db, name).cloned()
    }
    pub fn error(&mut self, e: impl Into<String>) {
        self.actions.push(Action::Error(e.into()));
    }
    pub fn status(&mut self, s: impl Into<String>) {
        self.actions.push(Action::Status(s.into()));
    }
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
    /// Eintrag fuer die Sitzungsdatei (zum Wiederherstellen beim Start)
    fn session(&self) -> Option<String> {
        None
    }
    /// Zugehoerige Datei (fuer den Explorer)
    fn file(&self) -> Option<&std::path::Path> {
        None
    }
    /// Datei/Ordner wurde im Explorer umbenannt
    fn file_renamed(&mut self, _old: &std::path::Path, _new: &std::path::Path) {}
    /// Vor dem Schliessen (speichern). false = nicht schliessen
    fn on_close(&mut self) -> bool {
        true
    }
    fn save_now(&mut self) {}
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
pub fn db_combo(ui: &mut egui::Ui, id: &str, dbs: &[String], value: &mut String) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(if value.is_empty() { "(keine)" } else { value.as_str() })
        .width(170.0)
        .show_ui(ui, |ui| {
            for d in dbs {
                if ui.selectable_label(value == d, d).clicked() {
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
    egui::ComboBox::from_id_salt(id)
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
