// Einstellungen (Datei einstellungen.txt im Programmdatenordner, schluessel=wert).

use crate::db::ConnInfo;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub conn: ConnInfo,
    pub save_password: bool,
    pub show_system_dbs: bool,
    pub dark: bool,
    /// Schriftgroesse im SQL-Editor (Pixel)
    pub editor_font: u32,
    pub minimap: bool,
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
}

fn path() -> PathBuf {
    crate::server::app_data_dir().join("einstellungen.txt")
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            conn: ConnInfo::default(),
            save_password: false,
            show_system_dbs: false,
            dark: true,
            editor_font: 13,
            minimap: true,
            auto_suggest: true,
            auto_close: true,
            scroll_speed: 1.5,
            autoscroll: true,
            storage_dir: String::new(),
            recent: Vec::new(),
            query_log: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(path()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().to_string();
            let on = v == "1";
            match k.trim() {
                "host" => s.conn.host = v,
                "port" => s.conn.port = v.parse().unwrap_or(3306),
                "user" => s.conn.user = v,
                "password" => s.conn.password = v,
                "save_password" => s.save_password = on,
                "show_system_dbs" => s.show_system_dbs = on,
                "dark" => s.dark = on,
                "editor_font" => s.editor_font = v.parse().unwrap_or(13),
                "minimap" => s.minimap = on,
                "auto_suggest" => s.auto_suggest = on,
                "auto_close" => s.auto_close = on,
                "scroll_speed" => s.scroll_speed = v.parse().unwrap_or(1.5f32).clamp(0.25, 6.0),
                "autoscroll" => s.autoscroll = on,
                "storage_dir" => s.storage_dir = v,
                "recent" if !v.is_empty() => s.recent.push(PathBuf::from(v)),
                "query_log" => s.query_log = on,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let b = |x: bool| if x { "1" } else { "0" };
        let mut text = format!(
            "host={}\nport={}\nuser={}\nsave_password={}\nshow_system_dbs={}\ndark={}\neditor_font={}\n\
             minimap={}\nauto_suggest={}\nauto_close={}\nscroll_speed={}\nautoscroll={}\nstorage_dir={}\nquery_log={}\n",
            self.conn.host,
            self.conn.port,
            self.conn.user,
            b(self.save_password),
            b(self.show_system_dbs),
            b(self.dark),
            self.editor_font,
            b(self.minimap),
            b(self.auto_suggest),
            b(self.auto_close),
            self.scroll_speed,
            b(self.autoscroll),
            self.storage_dir,
            b(self.query_log),
        );
        for r in &self.recent {
            text.push_str(&format!("recent={}\n", r.display()));
        }
        if self.save_password {
            text.push_str(&format!("password={}\n", self.conn.password));
        }
        let _ = crate::workspace::atomic_write(&path(), &text);
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
