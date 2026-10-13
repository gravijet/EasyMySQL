// Einstellungen (Datei einstellungen.txt im Programmdatenordner, schluessel=wert).

use crate::db::ConnInfo;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub conn: ConnInfo,
    /// Verbindung ueber einen SSH-Tunnel; Server und Port von `conn` gelten dann aus Sicht des SSH-Servers
    pub ssh: crate::ssh::SshConfig,
    pub language: crate::i18n::Language,
    pub dark: bool,
    pub save_password: bool,
    pub show_system_dbs: bool,
    /// Beim Start und alle sechs Stunden nach neuen Versionen suchen.
    pub auto_update: bool,
    /// Schriftgroesse im SQL-Editor (Pixel)
    pub editor_font: u32,
    /// Vorschlaege beim Tippen automatisch zeigen (sonst nur mit Strg+Leertaste)
    pub auto_suggest: bool,
    /// Klammern und Anfuehrungszeichen automatisch schliessen
    pub auto_close: bool,
    /// Faktor fuer das Mausrad (1 = normal)
    pub scroll_speed: f32,
    /// Mittlere Maustaste gedrueckt halten und ziehen = schnell scrollen
    pub autoscroll: bool,
    /// Ordner fuer Projekte, unbenannte Abfragen, Verlauf usw. (leer = Dokumente\EasyMySQL)
    pub storage_dir: String,
    /// Zuletzt geoeffnete Projektordner, neueste zuerst
    pub recent: Vec<PathBuf>,
    /// Alle Anweisungen an den Server mitschreiben (MariaDB general log)
    pub query_log: bool,
    /// Dateien im Editor kurz nach der letzten Aenderung automatisch speichern
    pub autosave: bool,
    /// Fehler im SQL waehrend des Tippens markieren
    pub lint: bool,
    /// Beim Schliessen des Fensters im Infobereich weiterlaufen (sonst wird alles beendet)
    pub background: bool,
    /// Beim naechsten Start alle Registerkarten wieder oeffnen
    pub restore_tabs: bool,
}

/// Schriftgroesse im SQL-Editor (Pixel): Standard und Grenzen
pub const DEFAULT_FONT: u32 = 13;
pub const MIN_FONT: u32 = 8;
pub const MAX_FONT: u32 = 40;

fn path() -> PathBuf {
    crate::server::app_data_dir().join("einstellungen.txt")
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            conn: ConnInfo::default(),
            ssh: Default::default(),
            language: crate::i18n::Language::English,
            dark: true,
            save_password: false,
            show_system_dbs: false,
            auto_update: true,
            editor_font: DEFAULT_FONT,
            auto_suggest: true,
            auto_close: true,
            scroll_speed: 1.5,
            autoscroll: true,
            storage_dir: String::new(),
            recent: Vec::new(),
            query_log: false,
            autosave: true,
            lint: true,
            background: false,
            restore_tabs: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).map(|text| Self::from_text(&text)).unwrap_or_default()
    }

    fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().to_string();
            let on = v == "1";
            match k.trim() {
                "language" => s.language = crate::i18n::Language::from_id(&v),
                "dark" => s.dark = on,
                "host" => s.conn.host = v,
                "port" => s.conn.port = v.parse().unwrap_or(3306),
                "user" => s.conn.user = v,
                "password" => s.conn.password = v,
                "ssh" => s.ssh.enabled = on,
                "ssh_host" => s.ssh.host = v,
                "ssh_port" => s.ssh.port = v.parse().unwrap_or(22),
                "ssh_user" => s.ssh.user = v,
                "ssh_key" => s.ssh.key = v,
                "ssh_password" => s.ssh.password = v,
                "save_password" => s.save_password = on,
                "show_system_dbs" => s.show_system_dbs = on,
                "auto_update" => s.auto_update = on,
                "editor_font" => s.editor_font = v.parse().unwrap_or(DEFAULT_FONT).clamp(MIN_FONT, MAX_FONT),
                "auto_suggest" => s.auto_suggest = on,
                "auto_close" => s.auto_close = on,
                "scroll_speed" => s.scroll_speed = v.parse().unwrap_or(1.5f32).clamp(0.25, 6.0),
                "autoscroll" => s.autoscroll = on,
                "storage_dir" => s.storage_dir = v,
                "recent" if !v.is_empty() => s.recent.push(PathBuf::from(v)),
                "query_log" => s.query_log = on,
                "autosave" => s.autosave = on,
                "lint" => s.lint = on,
                "background" => s.background = on,
                "restore_tabs" => s.restore_tabs = on,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let _ = crate::workspace::atomic_write(&path(), &self.to_text());
    }

    fn to_text(&self) -> String {
        let b = |x: bool| if x { "1" } else { "0" };
        let mut text = format!(
            "language={}\ndark={}\nhost={}\nport={}\nuser={}\nsave_password={}\nshow_system_dbs={}\nauto_update={}\neditor_font={}\n\
             auto_suggest={}\nauto_close={}\nscroll_speed={}\nautoscroll={}\nstorage_dir={}\nquery_log={}\nautosave={}\nlint={}\nbackground={}\nrestore_tabs={}\n",
            self.language.id(),
            b(self.dark),
            self.conn.host,
            self.conn.port,
            self.conn.user,
            b(self.save_password),
            b(self.show_system_dbs),
            b(self.auto_update),
            self.editor_font,
            b(self.auto_suggest),
            b(self.auto_close),
            self.scroll_speed,
            b(self.autoscroll),
            self.storage_dir,
            b(self.query_log),
            b(self.autosave),
            b(self.lint),
            b(self.background),
            b(self.restore_tabs),
        );
        for r in &self.recent {
            text.push_str(&format!("recent={}\n", r.display()));
        }
        let s = &self.ssh;
        text.push_str(&format!("ssh={}\nssh_host={}\nssh_port={}\nssh_user={}\nssh_key={}\n", b(s.enabled), s.host, s.port, s.user, s.key));
        if self.save_password {
            text.push_str(&format!("password={}\nssh_password={}\n", self.conn.password, s.password));
        }
        text
    }

    /// Projektordner an den Anfang der Liste "Zuletzt geoeffnet" setzen.
    pub fn add_recent(&mut self, dir: &std::path::Path) {
        self.recent.retain(|p| p != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_legacy_settings_and_language_roundtrip() {
        let defaults = Settings::from_text("host=localhost\nport=3307\n");
        assert_eq!(defaults.language, crate::i18n::Language::English);
        assert!(defaults.dark);
        assert_eq!(defaults.conn.port, 3307);
        let custom = Settings::from_text("language=de\ndark=0\neditor_font=18\nauto_update=0\n");
        assert_eq!(custom.language, crate::i18n::Language::German);
        assert!(!custom.dark);
        assert_eq!(Settings::from_text(&custom.to_text()), custom);
        assert_eq!(Settings::from_text("language=invalid").language, crate::i18n::Language::English);
    }

    #[test]
    fn recent_list() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.add_recent(std::path::Path::new(&format!("/p{i}")));
        }
        s.add_recent(std::path::Path::new("/p5"));
        assert_eq!(s.recent.len(), 12);
        assert_eq!(s.recent[0], PathBuf::from("/p5"));
        assert_eq!(s.recent.iter().filter(|p| **p == PathBuf::from("/p5")).count(), 1);
    }
}
