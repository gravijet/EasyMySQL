// Weitere Server neben dem globalen: jede Datei kann einen gespeicherten Server waehlen (siehe `profiles`).
// Verbunden wird erst, wenn eine Registerkarte den Server braucht; Datenbankliste und Strukturen gehoeren zum Server.

use super::*;
use crate::profiles::Profile;
use crate::tabs::SchemaCache;
use std::sync::mpsc::Receiver;

type Opened = Result<(Db, Option<crate::ssh::Tunnel>, Vec<String>), String>;

pub(super) struct Remote {
    /// Stand der Verbindungsdaten beim Verbinden (aendern sie sich, wird neu verbunden)
    profile: Profile,
    pub db: Option<Db>,
    _tunnel: Option<crate::ssh::Tunnel>,
    pub databases: Vec<String>,
    pub schemas: SchemaCache,
    job: Option<Receiver<Opened>>,
    pub error: Option<String>,
}

impl Remote {
    pub fn connecting(&self) -> bool {
        self.job.is_some()
    }
}

fn open(p: &Profile) -> Opened {
    let (info, tunnel) = if p.ssh.enabled {
        let t = crate::ssh::open(&p.ssh, &p.conn.host, p.conn.port)?;
        (crate::db::ConnInfo { host: "127.0.0.1".into(), port: t.local_port, ..p.conn.clone() }, Some(t))
    } else {
        (p.conn.clone(), None)
    };
    let db = Db::connect(&info)?;
    let databases = db.databases()?;
    Ok((db, tunnel, databases))
}

impl EasyApp {
    /// Verbindung zum gespeicherten Server `name` sicherstellen (startet sie im Hintergrund, falls noetig)
    pub(super) fn ensure_remote(&mut self, ctx: &egui::Context, name: &str) {
        let current = crate::profiles::find(name);
        if let Some(r) = self.remotes.get(name)
            && current.as_ref() == Some(&r.profile)
        {
            return;
        }
        // Neu oder geaendert (oder geloescht: dann Fehler anzeigen)
        let Some(profile) = current else {
            self.remotes.remove(name);
            return;
        };
        let mut schemas = SchemaCache::default();
        schemas.set_repaint(ctx.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let (p, c) = (profile.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(open(&p));
            c.request_repaint();
        });
        self.remotes.insert(name.to_string(), Remote { profile, db: None, _tunnel: None, databases: Vec::new(), schemas, job: Some(rx), error: None });
    }

    /// Ergebnisse laufender Verbindungsversuche abholen
    pub(super) fn poll_remotes(&mut self, ctx: &egui::Context) {
        for r in self.remotes.values_mut() {
            let Some(rx) = &r.job else { continue };
            ctx.request_repaint_after(Duration::from_millis(200));
            if let Ok(res) = rx.try_recv() {
                r.job = None;
                match res {
                    Ok((db, tunnel, databases)) => {
                        r.db = Some(db);
                        r._tunnel = tunnel;
                        r.databases = databases;
                    }
                    Err(e) => r.error = Some(e),
                }
            }
        }
    }

    /// Anzeige statt des Inhalts, solange der Server der Datei nicht verbunden ist.
    /// Gibt zurueck, was der Nutzer gewaehlt hat.
    pub(super) fn remote_status(&self, ui: &mut egui::Ui, name: &str) -> Option<RemoteChoice> {
        let pal = style::pal();
        let remote = self.remotes.get(name);
        let mut choice = None;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.25).max(20.0));
            match remote {
                Some(r) if r.connecting() => {
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(RichText::new(crate::tr_format!("Verbinde mit {name} …", "Connecting to {name} …")).size(15.0));
                }
                Some(r) => {
                    ui.label(RichText::new(crate::tr_format!("Keine Verbindung zu {name}", "Not connected to {name}")).size(16.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new(r.error.clone().unwrap_or_default()).color(pal.error_text));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        // Knoepfe mittig
                        ui.add_space(((ui.available_width() - 360.0) / 2.0).max(0.0));
                        if crate::widgets::primary(ui, crate::i18n::text("Erneut versuchen")).clicked() {
                            choice = Some(RemoteChoice::Retry);
                        }
                        if ui.button(crate::i18n::tr("Standard-Server verwenden", "Use default server")).clicked() {
                            choice = Some(RemoteChoice::UseDefault);
                        }
                        if ui.button(crate::i18n::tr("Server verwalten …", "Manage servers …")).clicked() {
                            choice = Some(RemoteChoice::Manage);
                        }
                    });
                }
                None => {
                    ui.label(RichText::new(crate::tr_format!("Der Server „{name}“ existiert nicht mehr.", "The server “{name}” no longer exists.")).size(16.0).strong());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - 260.0) / 2.0).max(0.0));
                        if crate::widgets::primary(ui, crate::i18n::tr("Standard-Server verwenden", "Use default server")).clicked() {
                            choice = Some(RemoteChoice::UseDefault);
                        }
                        if ui.button(crate::i18n::tr("Server verwalten …", "Manage servers …")).clicked() {
                            choice = Some(RemoteChoice::Manage);
                        }
                    });
                }
            }
        });
        choice
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum RemoteChoice {
    Retry,
    UseDefault,
    Manage,
}
