// Registerkarten im Hauptbereich.

pub mod builder;
pub mod ai;
pub mod code;
pub mod data;
pub mod designer;
pub mod diff;
pub mod doktor;
pub mod er;
pub mod help;
mod help_content;
pub mod log;
pub mod preview;
pub mod safety;
pub mod settings;
pub mod sql;
pub mod structure;
pub mod transfer;
pub mod users;

use crate::style::Semi;
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
    /// Datenbank gebraucht, aber keine Verbindung: eigenen Server starten bzw. verbinden
    EnsureServer,
    OpenSafety,
    OpenImport,
    /// Export einer Datenbank (optional nur einzelne Tabellen)
    OpenExport { db: String, only: Option<Vec<String>>, format: crate::transfer::export::Format },
    OpenUsers,
    /// Datenbank pruefen und reparieren (Datenbank-Doktor)
    OpenDoktor(String),
    /// Dateien im Projekt haben sich geaendert (Explorer neu einlesen)
    FilesChanged,
    /// Datei oeffnen und zu einer Zeile (0-basiert) springen
    OpenFileAt { path: std::path::PathBuf, line: usize },
    /// Hilfe oeffnen, optional bei einem Abschnitt
    OpenHelp(Option<String>),
    /// Einstellungen oeffnen, optional bei einem Abschnitt
    OpenSettings(Option<String>),
    OpenLog,
    OpenAi(crate::mcp::setup::Target),
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
    /// Gefundenes Update jetzt herunterladen und installieren
    UpdateInstall,
    /// Programm neu starten, um das installierte Update zu benutzen
    UpdateRestart,
    /// Speicherordner wurde verschoben (offene Dateien und Projekte anpassen)
    FilesMoved { from: std::path::PathBuf, to: std::path::PathBuf },
    /// Eigenen Server neu starten (neue Einstellungen uebernehmen)
    RestartServer,
    /// Anderen Datenordner fuer den eigenen Server (None = Standard); `copy`: bisherige Daten mitnehmen
    ServerDataDir { dir: Option<std::path::PathBuf>, copy: bool },
    /// Programm im Terminal starten (Java, Python, JavaScript): mit zusaetzlicher Umgebung
    RunProgram { launch: crate::dev::process::Launch, env: Vec<(String, String)> },
    /// Aenderungen des Sprachservers anwenden (Umbenennen ueber mehrere Dateien)
    ApplyEdit(crate::dev::lsp::WorkspaceEdit),
    /// Anmeldedialog fuer GitHub oeffnen
    GitHubLogin,
    /// Erkannten Fehler beheben
    AutoFix(crate::dev::autofix::Fix),
    /// Eigenen Server auf diese MariaDB-Version aktualisieren
    MariaDbUpdate(crate::mariadb_update::Release),
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
    /// Stand des Updates (Einstellungen)
    pub update: crate::updates::Status,
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
    let (r, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 40.0, 28.0), egui::Sense::click());
    let p = ui.painter();
    let radius = crate::style::RADIUS as f32;
    // Kleiner Reiter: der gewaehlte mit Strich unten in seiner Farbe (waechst aus der Mitte)
    let hue = color.unwrap_or(pal.accent);
    let hot = ui.ctx().animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
    let on = ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected, 0.16);
    if hot > 0.0 && on < 1.0 {
        p.rect_filled(r, radius, pal.hover.gamma_multiply(hot * (1.0 - on)));
    }
    if on > 0.0 {
        let w = (r.width() - 8.0) * on;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(r.center().x - w / 2.0, r.bottom() - 2.0), egui::pos2(r.center().x + w / 2.0, r.bottom())), 1.0, hue);
    }
    let cy = r.center().y - 1.0;
    let fg = if selected { color.map(|c| c.lerp_to_gamma(pal.text, 0.3)).unwrap_or(pal.text) } else { color.unwrap_or(pal.text_weak) };
    p.galley(egui::pos2(r.left() + 10.0, cy - galley.size().y / 2.0), galley, fg);
    let cr = egui::Rect::from_center_size(egui::pos2(r.right() - 14.0, cy), egui::vec2(16.0, 16.0));
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
    ui.add(crate::widgets::edit(text).id(id).desired_width(width))
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
    /// Beim erneuten Oeffnen mit gleichem Schluessel den Inhalt der vorhandenen Registerkarte ersetzen
    /// (z. B. Diff mit neuem Stand) statt nur zu ihr zu wechseln
    fn replace_on_reopen(&self) -> bool {
        false
    }
    /// Der Stand des Git-Repositorys hat sich geaendert (Aktualisieren, Commit, Pull …)
    fn git_changed(&mut self) {}
    /// Laesst sich mit "Ausfuehren" starten (Startknopf in der oberen Leiste)
    fn runnable(&self) -> bool {
        false
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
    /// Gespeicherter Server, auf dem der Reiter arbeitet; `None` = der globale Server
    fn server(&self) -> Option<String> {
        None
    }
    /// Anderen Server wählen (`None` = globaler Server)
    fn use_server(&mut self, _server: Option<String>) {}
    /// Datenbank, mit der diese Registerkarte gerade arbeitet (ER-Diagramm, Assistent, neue Abfrage folgen ihr)
    fn database(&self) -> Option<String> {
        None
    }
    /// In der Seitenleiste wurde eine Datenbank gewaehlt: uebernehmen, solange die Registerkarte noch keine hat
    fn adopt_database(&mut self, _db: &str) {}
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
    /// Cursor (Zeichenindex) fuer die Sitzung: beim Schliessen merken, beim Start wieder setzen
    fn cursor(&self) -> Option<usize> {
        None
    }
    fn restore_cursor(&mut self, _pos: usize) {}
    /// Textaenderungen (Zeile, Spalte in UTF-16) auf den Inhalt anwenden; false, wenn die Registerkarte keinen Text hat
    fn apply_edits(&mut self, _edits: &[crate::dev::lsp::TextEdit]) -> bool {
        false
    }
    /// Abschnitt zeigen (Hilfe, Einstellungen)
    fn show_section(&mut self, _id: &str) {}
}

/// Auswahlfeld fuer eine Datenbank. Liefert true bei Aenderung.
/// Abschnittsueberschrift einer Seite: kleiner, abgesetzter Text mit einheitlichem Abstand
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(egui::RichText::new(text).size(12.5).semi().color(crate::style::pal().text_weak));
    ui.add_space(4.0);
}

/// Systemdatenbanken in Auswahllisten zeigen? (Einstellung)
pub static SHOW_SYSTEM_DBS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_system_db(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "information_schema" | "mysql" | "performance_schema" | "sys")
}

pub fn db_combo(ui: &mut egui::Ui, id: &str, dbs: &[String], value: &mut String) -> bool {
    let show_system = SHOW_SYSTEM_DBS.load(std::sync::atomic::Ordering::Relaxed);
    let dbs: Vec<String> = dbs.iter().filter(|d| show_system || !is_system_db(d) || *d == value).cloned().collect();
    db_select(ui, id, &dbs, value, 170.0)
}

/// Auswahl der Datenbank: Knopf mit dem Datenbanksymbol in ihrer Farbe;
/// leer heisst ueberall gleich "Keine Datenbank". Die Liste zeigt jede Datenbank mit ihrer Farbe.
pub fn db_select(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, items: &[String], value: &mut String, width: f32) -> bool {
    let none = crate::i18n::tr("Keine Datenbank", "No database");
    let resp = db_chip(ui, egui::Id::new(&id), value, width);
    let mut changed = false;
    egui::Popup::menu(&resp).show(|ui| {
        crate::style::menu_item_look(ui);
        ui.set_min_width(resp.rect.width().max(180.0));
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for it in items {
                let c = crate::style::db_color(it);
                let label = if it.is_empty() { none } else { it.as_str() };
                let r = crate::icons::menu_item_colored(ui, Some((crate::icons::Icon::Database, Some(c))), label, if value == it { "✓" } else { "" });
                if r.clicked() {
                    if value != it {
                        *value = it.clone();
                        changed = true;
                    }
                    ui.close();
                }
            }
        });
    });
    changed
}

/// Der Knopf selbst: Symbol in der Farbe der Datenbank, Name, Pfeil
pub fn db_chip(ui: &mut egui::Ui, id: egui::Id, value: &str, width: f32) -> egui::Response {
    let p = crate::style::pal();
    let none = value.is_empty();
    let hue = crate::style::db_color(value);
    let text = if none { crate::i18n::tr("Keine Datenbank", "No database") } else { value };
    let font = if none { crate::style::medium(13.0) } else { crate::style::semi(13.0) };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, if none { p.text_weak } else { p.text });
    let w = (galley.size().x + 58.0).clamp(width.min(120.0), 260.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 28.0), egui::Sense::hover());
    let resp = ui.interact(r, id, egui::Sense::click());
    let hot = ui.ctx().animate_bool_with_time(id.with("hot"), resp.hovered() || resp.has_focus(), 0.12);
    let painter = ui.painter();
    // Wie die anderen Knoepfe: weisse Flaeche mit feiner Kante; die Farbe der Datenbank traegt das Symbol
    let (rest, over, _) = crate::style::button_fills(p);
    painter.rect(r, crate::style::RADIUS as f32 + 1.0, rest.lerp_to_gamma(over, hot), egui::Stroke::new(1.0, p.border.lerp_to_gamma(p.text_weak, 0.3 * hot)), egui::StrokeKind::Inside);
    crate::icons::paint(painter, egui::Rect::from_center_size(egui::pos2(r.left() + 17.0, r.center().y), egui::vec2(15.0, 15.0)), crate::icons::Icon::Database, if none { p.text_weak } else { hue });
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), if none { crate::style::medium(13.0) } else { crate::style::semi(13.0) }, if none { p.text_weak } else { p.text });
    job.wrap = egui::text::TextWrapping { max_width: r.width() - 56.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = painter.layout_job(job);
    painter.galley(egui::pos2(r.left() + 30.0, r.center().y - g.size().y / 2.0), g, p.text);
    crate::icons::paint(painter, egui::Rect::from_center_size(egui::pos2(r.right() - 13.0, r.center().y), egui::vec2(12.0, 12.0)), crate::icons::Icon::ChevronDown, p.text_weak);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Titel einer Tabellenansicht: Datenbank (Symbol in ihrer Farbe) / Tabellenname
pub fn table_title(ui: &mut egui::Ui, db: &str, table: &str) {
    let p = crate::style::pal();
    let hue = crate::style::db_color(db);
    let g = ui.painter().layout_no_wrap(db.to_string(), crate::style::medium(12.5), p.text);
    let (r, _) = ui.allocate_exact_size(egui::vec2(g.size().x + 22.0, 24.0), egui::Sense::hover());
    let painter = ui.painter();
    crate::icons::paint(painter, egui::Rect::from_center_size(egui::pos2(r.left() + 8.0, r.center().y), egui::vec2(15.0, 15.0)), crate::icons::Icon::Database, hue);
    painter.galley(egui::pos2(r.left() + 20.0, r.center().y - g.size().y / 2.0), g, p.text_weak);
    ui.label(egui::RichText::new("/").color(p.border.lerp_to_gamma(p.text_weak, 0.5)));
    ui.label(egui::RichText::new(table).font(crate::style::semi(16.0)).color(p.text)).on_hover_text(format!("{db}.{table}"));
}

/// Auswahl der Datenbank (gleich wie `db_select`; der Reiter traegt sein Symbol schon selbst)
pub fn db_pick(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, items: &[String], value: &mut String, width: f32) -> bool {
    db_select(ui, id, items, value, width)
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
