// Einfache Einstellungsdatei (schluessel=wert).

use crate::db::ConnInfo;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Settings {
    pub conn: ConnInfo,
    pub save_password: bool,
    pub show_system_dbs: bool,
    pub tray_hint_shown: bool,
    pub dark: bool,
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
            tray_hint_shown: false,
            dark: true,
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
            match k.trim() {
                "host" => s.conn.host = v,
                "port" => s.conn.port = v.parse().unwrap_or(3306),
                "user" => s.conn.user = v,
                "password" => s.conn.password = v,
                "save_password" => s.save_password = v == "1",
                "show_system_dbs" => s.show_system_dbs = v == "1",
                "tray_hint_shown" => s.tray_hint_shown = v == "1",
                "dark" => s.dark = v == "1",
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let b = |x: bool| if x { "1" } else { "0" };
        let mut text = format!(
            "host={}\nport={}\nuser={}\nsave_password={}\nshow_system_dbs={}\ntray_hint_shown={}\ndark={}\n",
            self.conn.host,
            self.conn.port,
            self.conn.user,
            b(self.save_password),
            b(self.show_system_dbs),
            b(self.tray_hint_shown),
            b(self.dark)
        );
        if self.save_password {
            text.push_str(&format!("password={}\n", self.conn.password));
        }
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, text);
    }
}
