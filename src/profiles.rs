// Gespeicherte Server (Datei server.txt im Programmdatenordner) und die Zuordnung Datei -> Server.
// Der globale Server steht in den Einstellungen; hier stehen die weiteren, die eine Datei statt dessen wählen kann.

use crate::db::ConnInfo;
use crate::ssh::SshConfig;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub conn: ConnInfo,
    pub ssh: SshConfig,
}

impl Profile {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), conn: ConnInfo::default(), ssh: SshConfig::default() }
    }

    /// Kurzbeschreibung fuer Listen: Benutzer@Server:Port
    pub fn label(&self) -> String {
        let via = if self.ssh.enabled { format!(" ({} {})", crate::i18n::tr("über", "via"), self.ssh.host) } else { String::new() };
        format!("{}@{}:{}{via}", self.conn.user, self.conn.host, self.conn.port)
    }
}

fn path() -> std::path::PathBuf {
    crate::server::app_data_dir().join("server.txt")
}

fn field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn to_text(list: &[Profile], with_passwords: bool) -> String {
    let mut out = String::new();
    for p in list {
        let (c, s) = (&p.conn, &p.ssh);
        let (pw, spw) = if with_passwords { (c.password.as_str(), s.password.as_str()) } else { ("", "") };
        let cols = [
            field(&p.name),
            field(&c.host),
            c.port.to_string(),
            field(&c.user),
            field(pw),
            (s.enabled as u8).to_string(),
            field(&s.host),
            s.port.to_string(),
            field(&s.user),
            field(&s.key),
            field(spw),
        ];
        out.push_str(&cols.join("\t"));
        out.push('\n');
    }
    out
}

fn from_text(text: &str) -> Vec<Profile> {
    let mut out: Vec<Profile> = Vec::new();
    for line in text.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() < 11 || c[0].trim().is_empty() || out.iter().any(|p| p.name == c[0]) {
            continue;
        }
        out.push(Profile {
            name: c[0].to_string(),
            conn: ConnInfo { host: c[1].to_string(), port: c[2].parse().unwrap_or(3306), user: c[3].to_string(), password: c[4].to_string() },
            ssh: SshConfig { enabled: c[5] == "1", host: c[6].to_string(), port: c[7].parse().unwrap_or(22), user: c[8].to_string(), key: c[9].to_string(), password: c[10].to_string() },
        });
    }
    out
}

fn store() -> &'static Mutex<Vec<Profile>> {
    static S: OnceLock<Mutex<Vec<Profile>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(std::fs::read_to_string(path()).map(|t| from_text(&t)).unwrap_or_default()))
}

/// Alle gespeicherten Server
pub fn all() -> Vec<Profile> {
    store().lock().unwrap().clone()
}

pub fn find(name: &str) -> Option<Profile> {
    store().lock().unwrap().iter().find(|p| p.name == name).cloned()
}

/// Liste ersetzen und speichern. Passwoerter werden nur gespeichert, wenn `save_passwords` gilt
/// (wie beim globalen Server); bis zum Neustart bleiben sie im Speicher.
pub fn set(list: Vec<Profile>, save_passwords: bool) {
    let _ = crate::workspace::atomic_write(&path(), &to_text(&list, save_passwords));
    *store().lock().unwrap() = list;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_password_policy() {
        let mut a = Profile::new("Test");
        a.conn = ConnInfo { host: "db.example.org".into(), port: 3307, user: "ben".into(), password: "geheim".into() };
        a.ssh = SshConfig { enabled: true, host: "jump".into(), port: 2222, user: "u".into(), key: "/k".into(), password: "x".into() };
        let b = Profile::new("Zweiter");
        let list = vec![a.clone(), b.clone()];
        assert_eq!(from_text(&to_text(&list, true)), list);
        let without = from_text(&to_text(&list, false));
        assert_eq!(without[0].conn.password, "");
        assert_eq!(without[0].ssh.password, "");
        assert_eq!(without[0].conn.host, "db.example.org");
        // doppelte und leere Namen werden verworfen, Tabulatoren im Namen zerstoeren die Zeile nicht
        let dup = format!("{}{}", to_text(&[a.clone()], true), to_text(&[a], true));
        assert_eq!(from_text(&dup).len(), 1);
        assert_eq!(from_text("\thost\t1\tu\tp\t0\t\t22\t\t\t\n").len(), 0);
        let tabbed = Profile { name: "a\tb".into(), ..b };
        assert_eq!(from_text(&to_text(&[tabbed], true))[0].name, "a b");
    }
}
