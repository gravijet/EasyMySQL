// Tastenkuerzel: alle Befehle mit Standardbelegung, frei aenderbar (Datei tastenkuerzel.txt).

use eframe::egui::{self, Event, Key, Modifiers};
use std::sync::RwLock;

/// Alle Befehle, die ein Tastenkuerzel haben koennen.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cmd {
    // Ausfuehren
    RunAll,
    RunStatement,
    Explain,
    // Datei
    NewQuery,
    NewFile,
    OpenFile,
    OpenFolder,
    Save,
    SaveAll,
    CloseTab,
    CloseAllTabs,
    ReopenTab,
    NextTab,
    PrevTab,
    // Ansicht
    CommandPalette,
    QuickOpen,
    ToggleSidebar,
    ShowExplorer,
    ShowSearch,
    ShowDatabases,
    ShowHistory,
    Settings,
    Help,
    CheckUpdates,
    ServerLog,
    ErDiagram,
    QueryBuilder,
    Backups,
    // Editor
    Format,
    Suggest,
    Find,
    Replace,
    GotoLine,
    ToggleComment,
    CutLine,
    DuplicateLine,
    DeleteLine,
    SelectLine,
    MoveLineUp,
    MoveLineDown,
    CopyLineUp,
    CopyLineDown,
    Indent,
    Outdent,
}

use Cmd::*;

pub const GROUPS: &[(&str, &[Cmd])] = &[
    ("Ausführen", &[RunAll, RunStatement, Explain]),
    ("Dateien und Registerkarten", &[NewQuery, NewFile, OpenFile, OpenFolder, Save, SaveAll, CloseTab, CloseAllTabs, ReopenTab, NextTab, PrevTab]),
    (
        "Ansicht",
        &[CommandPalette, QuickOpen, ToggleSidebar, ShowExplorer, ShowSearch, ShowDatabases, ShowHistory, Settings, Help, CheckUpdates, ServerLog, ErDiagram, QueryBuilder, Backups],
    ),
    (
        "Editor",
        &[
            Format, Suggest, Find, Replace, GotoLine, ToggleComment, CutLine, DuplicateLine, DeleteLine, SelectLine, MoveLineUp, MoveLineDown,
            CopyLineUp, CopyLineDown, Indent, Outdent,
        ],
    ),
];

impl Cmd {
    pub fn all() -> impl Iterator<Item = Cmd> {
        GROUPS.iter().flat_map(|(_, c)| c.iter().copied())
    }

    /// Schluessel in der Datei (nicht aendern)
    pub fn id(self) -> &'static str {
        match self {
            RunAll => "ausfuehren",
            RunStatement => "anweisung_ausfuehren",
            Explain => "explain",
            NewQuery => "neue_abfrage",
            NewFile => "neue_datei",
            OpenFile => "datei_oeffnen",
            OpenFolder => "ordner_oeffnen",
            Save => "speichern",
            SaveAll => "alle_speichern",
            CloseTab => "tab_schliessen",
            CloseAllTabs => "alle_tabs_schliessen",
            ReopenTab => "tab_wieder_oeffnen",
            NextTab => "naechster_tab",
            PrevTab => "vorheriger_tab",
            CommandPalette => "befehle",
            QuickOpen => "schnell_oeffnen",
            ToggleSidebar => "seitenleiste",
            ShowExplorer => "explorer",
            ShowSearch => "suche",
            ShowDatabases => "datenbanken",
            ShowHistory => "verlauf",
            Settings => "einstellungen",
            Help => "hilfe",
            CheckUpdates => "updates_pruefen",
            ServerLog => "server_log",
            ErDiagram => "er_diagramm",
            QueryBuilder => "abfrage_assistent",
            Backups => "sicherungen",
            Format => "formatieren",
            Suggest => "vorschlaege",
            Find => "suchen",
            Replace => "ersetzen",
            GotoLine => "gehe_zu_zeile",
            ToggleComment => "kommentar",
            CutLine => "zeile_ausschneiden",
            DuplicateLine => "zeile_duplizieren",
            DeleteLine => "zeile_loeschen",
            SelectLine => "zeile_markieren",
            MoveLineUp => "zeile_hoch",
            MoveLineDown => "zeile_runter",
            CopyLineUp => "zeile_kopieren_hoch",
            CopyLineDown => "zeile_kopieren_runter",
            Indent => "einruecken",
            Outdent => "ausruecken",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RunAll => "Datei oder Markierung ausführen",
            RunStatement => "Anweisung am Cursor ausführen",
            Explain => "Ausführungsplan (EXPLAIN)",
            NewQuery => "Neue Abfrage",
            NewFile => "Neue Datei im Projekt",
            OpenFile => "Datei öffnen",
            OpenFolder => "Ordner öffnen",
            Save => "Speichern",
            SaveAll => "Alle speichern",
            CloseTab => "Registerkarte schließen",
            CloseAllTabs => "Alle Registerkarten schließen",
            ReopenTab => "Geschlossene Registerkarte wieder öffnen",
            NextTab => "Nächste Registerkarte",
            PrevTab => "Vorherige Registerkarte",
            CommandPalette => "Befehle",
            QuickOpen => "Datei im Projekt öffnen",
            ToggleSidebar => "Seitenleiste ein/aus",
            ShowExplorer => "Explorer",
            ShowSearch => "Suchen in Dateien",
            ShowDatabases => "Datenbanken",
            ShowHistory => "Verlauf",
            Settings => "Einstellungen",
            Help => "Hilfe",
            CheckUpdates => "Nach Updates suchen",
            ServerLog => "Server-Log",
            ErDiagram => "ER-Diagramm",
            QueryBuilder => "Abfrage-Assistent",
            Backups => "Sicherungen & Reparatur",
            Format => "SQL formatieren",
            Suggest => "Vorschläge",
            Find => "Suchen",
            Replace => "Ersetzen",
            GotoLine => "Gehe zu Zeile",
            ToggleComment => "Kommentar ein/aus",
            CutLine => "Zeile ausschneiden (ohne Markierung)",
            DuplicateLine => "Zeile duplizieren",
            DeleteLine => "Zeile löschen",
            SelectLine => "Zeile markieren",
            MoveLineUp => "Zeile nach oben verschieben",
            MoveLineDown => "Zeile nach unten verschieben",
            CopyLineUp => "Zeile nach oben kopieren",
            CopyLineDown => "Zeile nach unten kopieren",
            Indent => "Einrücken (mehrere Zeilen)",
            Outdent => "Ausrücken",
        }
    }

    pub fn defaults(self) -> Vec<Binding> {
        let c = |k| Binding::new(true, false, false, k);
        let cs = |k| Binding::new(true, true, false, k);
        let ca = |k| Binding::new(true, false, true, k);
        let a = |k| Binding::new(false, false, true, k);
        let sa = |k| Binding::new(false, true, true, k);
        let n = |k| Binding::new(false, false, false, k);
        match self {
            RunAll => vec![ca(Key::S), n(Key::F5)],
            RunStatement => vec![c(Key::Enter)],
            Explain => vec![c(Key::E)],
            NewQuery => vec![c(Key::N)],
            NewFile => vec![ca(Key::N)],
            OpenFile => vec![c(Key::O)],
            OpenFolder => vec![cs(Key::O)],
            Save => vec![c(Key::S)],
            SaveAll => vec![cs(Key::S)],
            CloseTab => vec![c(Key::W), c(Key::F4)],
            CloseAllTabs => vec![cs(Key::W)],
            ReopenTab => vec![cs(Key::T)],
            NextTab => vec![c(Key::Tab), c(Key::PageDown)],
            PrevTab => vec![cs(Key::Tab), c(Key::PageUp)],
            CommandPalette => vec![cs(Key::P), n(Key::F1).with_shift()],
            QuickOpen => vec![c(Key::P)],
            ToggleSidebar => vec![c(Key::B)],
            ShowExplorer => vec![cs(Key::E)],
            ShowSearch => vec![cs(Key::F)],
            ShowDatabases => vec![cs(Key::D)],
            ShowHistory => vec![cs(Key::H)],
            Settings => vec![c(Key::Comma)],
            Help => vec![n(Key::F1)],
            CheckUpdates => vec![],
            ServerLog => vec![cs(Key::L)],
            ErDiagram => vec![cs(Key::R)],
            QueryBuilder => vec![cs(Key::Q)],
            Backups => vec![],
            Format => vec![ca(Key::L), sa(Key::F)],
            Suggest => vec![c(Key::Space)],
            Find => vec![c(Key::F)],
            Replace => vec![c(Key::H)],
            GotoLine => vec![c(Key::G)],
            // Strg+# auf deutscher Tastatur (physisch die Taste neben Enter) und Strg+/
            ToggleComment => vec![c(Key::Backslash), c(Key::Slash)],
            CutLine => vec![c(Key::X)],
            DuplicateLine => vec![c(Key::D)],
            DeleteLine => vec![cs(Key::K)],
            SelectLine => vec![c(Key::L)],
            MoveLineUp => vec![a(Key::ArrowUp)],
            MoveLineDown => vec![a(Key::ArrowDown)],
            CopyLineUp => vec![sa(Key::ArrowUp)],
            CopyLineDown => vec![sa(Key::ArrowDown)],
            Indent => vec![n(Key::Tab)],
            Outdent => vec![Binding::new(false, true, false, Key::Tab)],
        }
    }
}

/// Eine Tastenkombination.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Binding {
    pub const fn new(ctrl: bool, shift: bool, alt: bool, key: Key) -> Self {
        Self { ctrl, shift, alt, key }
    }

    fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    fn mods_match(&self, m: &Modifiers) -> bool {
        (m.ctrl || m.command) == self.ctrl && m.shift == self.shift && m.alt == self.alt
    }

    fn key_name(key: Key) -> String {
        match key {
            Key::ArrowUp => "↑".into(),
            Key::ArrowDown => "↓".into(),
            Key::ArrowLeft => "←".into(),
            Key::ArrowRight => "→".into(),
            Key::Space => "Leertaste".into(),
            Key::Enter => "Enter".into(),
            Key::Escape => "Esc".into(),
            Key::Delete => "Entf".into(),
            Key::Backspace => "Rück".into(),
            Key::PageUp => "Bild↑".into(),
            Key::PageDown => "Bild↓".into(),
            Key::Home => "Pos1".into(),
            Key::End => "Ende".into(),
            Key::Insert => "Einfg".into(),
            // Auf deutschen Tastaturen liegt dort die #-Taste
            Key::Backslash => "#".into(),
            Key::Comma => ",".into(),
            Key::Period => ".".into(),
            Key::Minus => "-".into(),
            Key::Plus => "+".into(),
            Key::Slash => "/".into(),
            k => k.name().to_string(),
        }
    }

    /// Anzeige, z. B. "Strg+Umschalt+P"
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Strg+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Umschalt+");
        }
        s.push_str(&Self::key_name(self.key));
        s
    }

    /// Datei-Format, z. B. "ctrl+shift+P"
    fn to_file(self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("ctrl+");
        }
        if self.alt {
            s.push_str("alt+");
        }
        if self.shift {
            s.push_str("shift+");
        }
        s.push_str(self.key.name());
        s
    }

    fn from_file(t: &str) -> Option<Binding> {
        let mut b = Binding::new(false, false, false, Key::A);
        let mut key = None;
        for part in t.trim().split('+') {
            match part {
                "ctrl" => b.ctrl = true,
                "alt" => b.alt = true,
                "shift" => b.shift = true,
                // "+" selbst: leerer Teil zwischen zwei Pluszeichen
                "" => key = Some(Key::Plus),
                k => key = Some(Key::from_name(k)?),
            }
        }
        b.key = key?;
        Some(b)
    }

    /// Passt ein Tastenereignis zu dieser Kombination? `mods`: aktuell gedrueckte Zusatztasten
    fn matches(&self, e: &Event, mods: &Modifiers) -> bool {
        match e {
            Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                self.mods_match(modifiers) && (*key == self.key || *physical_key == Some(self.key))
            }
            // Strg+X/Strg+C kommen als eigene Ereignisse an (ohne Zusatztasten)
            Event::Cut => self.mods_match(mods) && self.key == Key::X,
            Event::Copy => self.mods_match(mods) && self.key == Key::C,
            _ => false,
        }
    }

    /// Aus einem Tastenereignis (zum Aufzeichnen in den Einstellungen)
    pub fn from_event(e: &Event, mods: &Modifiers) -> Option<Binding> {
        match e {
            Event::Key { key, pressed: true, modifiers, .. } => {
                if matches!(key, Key::Escape) {
                    return None;
                }
                Some(Binding::new(modifiers.ctrl || modifiers.command, modifiers.shift, modifiers.alt, *key))
            }
            Event::Cut => Some(Binding::new(true, mods.shift, mods.alt, Key::X)),
            Event::Copy => Some(Binding::new(true, mods.shift, mods.alt, Key::C)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    map: Vec<(Cmd, Vec<Binding>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self { map: Cmd::all().map(|c| (c, c.defaults())).collect() }
    }
}

fn file() -> std::path::PathBuf {
    crate::server::app_data_dir().join("tastenkuerzel.txt")
}

impl Keymap {
    pub fn get(&self, cmd: Cmd) -> &[Binding] {
        self.map.iter().find(|(c, _)| *c == cmd).map(|(_, b)| b.as_slice()).unwrap_or(&[])
    }

    pub fn set(&mut self, cmd: Cmd, bindings: Vec<Binding>) {
        if let Some(e) = self.map.iter_mut().find(|(c, _)| *c == cmd) {
            e.1 = bindings;
        }
    }

    /// Befehle, die diese Kombination ebenfalls verwenden
    pub fn conflicts(&self, cmd: Cmd, b: &Binding) -> Vec<Cmd> {
        self.map.iter().filter(|(c, bs)| *c != cmd && bs.contains(b)).map(|(c, _)| *c).collect()
    }

    /// Erste Belegung als Text (fuer Menues), leer wenn keine
    pub fn text(&self, cmd: Cmd) -> String {
        self.get(cmd).first().map(|b| b.text()).unwrap_or_default()
    }

    pub fn load() -> Self {
        let mut km = Keymap::default();
        let Ok(t) = std::fs::read_to_string(file()) else { return km };
        for line in t.lines() {
            let Some((id, v)) = line.split_once('=') else { continue };
            let Some(cmd) = Cmd::all().find(|c| c.id() == id.trim()) else { continue };
            let bs: Vec<Binding> = v.split(',').filter(|s| !s.trim().is_empty()).filter_map(Binding::from_file).collect();
            km.set(cmd, bs);
        }
        km
    }

    /// Nur Abweichungen von der Standardbelegung speichern.
    pub fn save(&self) {
        let mut t = String::new();
        for (c, bs) in &self.map {
            if *bs != c.defaults() {
                let v: Vec<String> = bs.iter().map(|b| b.to_file()).collect();
                t.push_str(&format!("{}={}\n", c.id(), v.join(",")));
            }
        }
        let _ = crate::workspace::atomic_write(&file(), &t);
    }
}

static KEYMAP: RwLock<Option<Keymap>> = RwLock::new(None);

pub fn current() -> Keymap {
    KEYMAP.read().unwrap().clone().unwrap_or_default()
}

pub fn install(km: Keymap) {
    *KEYMAP.write().unwrap() = Some(km);
}

/// Erste Belegung eines Befehls als Text
pub fn text(cmd: Cmd) -> String {
    KEYMAP.read().unwrap().as_ref().map(|k| k.text(cmd)).unwrap_or_else(|| cmd.defaults().first().map(|b| b.text()).unwrap_or_default())
}

/// Wurde das Kuerzel fuer `cmd` gedrueckt? Das Tastenereignis wird dabei verbraucht.
pub fn pressed(input: &mut egui::InputState, cmd: Cmd) -> bool {
    let guard = KEYMAP.read().unwrap();
    let defaults;
    let bindings = match guard.as_ref() {
        Some(k) => k.get(cmd),
        None => {
            defaults = cmd.defaults();
            &defaults
        }
    };
    if bindings.is_empty() {
        return false;
    }
    let mut hit = false;
    let mods = input.modifiers;
    input.events.retain(|e| {
        if !hit && bindings.iter().any(|b| b.matches(e, &mods)) {
            hit = true;
            return false;
        }
        true
    });
    hit
}

/// Wie `pressed`, fuer `ui.input_mut`
pub fn take(ui: &egui::Ui, cmd: Cmd) -> bool {
    ui.input_mut(|i| pressed(i, cmd))
}

pub fn take_ctx(ctx: &egui::Context, cmd: Cmd) -> bool {
    ctx.input_mut(|i| pressed(i, cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(key: Key, ctrl: bool, shift: bool, alt: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl, shift, alt, command: ctrl, mac_cmd: false },
        }
    }

    #[test]
    fn file_roundtrip_and_text() {
        for c in Cmd::all() {
            for b in c.defaults() {
                assert_eq!(Binding::from_file(&b.to_file()), Some(b), "{c:?}");
            }
        }
        assert_eq!(Binding::new(true, true, false, Key::P).text(), "Strg+Umschalt+P");
        assert_eq!(Binding::from_file("ctrl++"), Some(Binding::new(true, false, false, Key::Plus)));
    }

    #[test]
    fn ids_unique() {
        let mut ids: Vec<&str> = Cmd::all().map(|c| c.id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn matching() {
        let b = Binding::new(true, false, true, Key::S);
        let none = Modifiers::NONE;
        let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
        assert!(b.matches(&key_event(Key::S, true, false, true), &none));
        assert!(!b.matches(&key_event(Key::S, true, false, false), &none));
        assert!(Binding::new(true, false, false, Key::X).matches(&Event::Cut, &ctrl));
        assert!(!Binding::new(true, true, false, Key::X).matches(&Event::Cut, &ctrl));
        assert_eq!(Binding::from_event(&Event::Cut, &Modifiers { shift: true, ..ctrl }), Some(Binding::new(true, true, false, Key::X)));
        // Physische Taste (#-Taste auf deutscher Tastatur)
        let hash = Event::Key {
            key: Key::Backslash,
            physical_key: Some(Key::Backslash),
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl: true, command: true, ..Default::default() },
        };
        assert!(Binding::new(true, false, false, Key::Backslash).matches(&hash, &none));
    }

    #[test]
    fn default_conflicts() {
        // Die Standardbelegung darf keine doppelten Kuerzel enthalten
        let km = Keymap::default();
        for c in Cmd::all() {
            for b in km.get(c) {
                assert!(km.conflicts(c, b).is_empty(), "{c:?} {} doppelt: {:?}", b.text(), km.conflicts(c, b));
            }
        }
    }
}
