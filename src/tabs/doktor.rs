// Datenbank pruefen: Befunde des Datenbank-Doktors mit Auswahl und Reparatur (vorher gesichert, danach neu geprueft).

use super::{Action, Ctx, TabView};
use crate::doktor::{Art, Fund};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::style::{self, Semi};
use eframe::egui::{self, RichText};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

type Lauf = (Vec<Result<(), String>>, Option<String>);

pub struct DoktorTab {
    db: String,
    funde: Vec<Fund>,
    wahl: Vec<bool>,
    sql_offen: Vec<bool>,
    pruefung: Option<Receiver<Result<Vec<Fund>, String>>>,
    lauf: Option<Receiver<Lauf>>,
    status: Arc<Mutex<String>>,
    fehler: Option<String>,
    /// Ergebnis der letzten Reparatur: (Titel, Ergebnis) und Hinweis zur Sicherung
    zuletzt: Vec<(String, Result<(), String>)>,
    sicherung: Option<String>,
    gestartet: bool,
    /// Zeitpunkt des letzten Ergebnisses (fuer das Einblenden)
    seit: Instant,
}

impl DoktorTab {
    pub fn new(db: String) -> Self {
        Self {
            db,
            funde: Vec::new(),
            wahl: Vec::new(),
            sql_offen: Vec::new(),
            pruefung: None,
            lauf: None,
            status: Arc::new(Mutex::new(String::new())),
            fehler: None,
            zuletzt: Vec::new(),
            sicherung: None,
            gestartet: false,
            seit: Instant::now(),
        }
    }

    fn sag(status: &Arc<Mutex<String>>, m: String) {
        if let Ok(mut g) = status.lock() {
            *g = m;
        }
    }

    fn pruefen(&mut self, cx: &mut Ctx) {
        let Some(dbc) = cx.db else {
            cx.actions.push(Action::EnsureServer);
            return;
        };
        self.gestartet = true;
        self.fehler = None;
        let pool = crate::db::Db { pool: dbc.pool.clone(), info: dbc.info.clone(), version: dbc.version.clone() };
        let (db, st) = (self.db.clone(), self.status.clone());
        Self::sag(&st, tr("Lese den Aufbau …", "Reading the structure …").into());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = pool.new_conn().and_then(|mut c| crate::doktor::pruefen(&mut c, &db, &|m| Self::sag(&st, m)));
            let _ = tx.send(r);
        });
        self.pruefung = Some(rx);
    }

    fn reparieren(&mut self, cx: &mut Ctx) {
        let Some(dbc) = cx.db else { return };
        let gewaehlt: Vec<Fund> = self.funde.iter().zip(&self.wahl).filter(|(f, w)| **w && !f.sql.is_empty()).map(|(f, _)| f.clone()).collect();
        if gewaehlt.is_empty() {
            return;
        }
        let pool = crate::db::Db { pool: dbc.pool.clone(), info: dbc.info.clone(), version: dbc.version.clone() };
        let (db, st) = (self.db.clone(), self.status.clone());
        let (tx, rx) = channel();
        self.zuletzt = gewaehlt.iter().map(|f| (f.titel.clone(), Ok(()))).collect();
        std::thread::spawn(move || {
            // Erst sichern: die Reparatur kann Werte aendern und loeschen
            let mut hinweis = None;
            if let Some(env) = crate::backup::env() {
                Self::sag(&st, tr("Sichere die Datenbank …", "Backing up the database …").into());
                match crate::backup::create(&env, Some(vec![db.clone()]), "vor-reparatur", &|_| {}) {
                    Ok(p) => hinweis = Some(crate::tr_format!("Vorher gesichert: {}", "Backed up first: {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                    Err(e) => {
                        let e = crate::tr_format!("Nicht repariert: Die Sicherung ist fehlgeschlagen ({e})", "Not repaired: the backup failed ({e})");
                        let _ = tx.send((gewaehlt.iter().map(|_| Err(e.clone())).collect(), None));
                        return;
                    }
                }
            }
            let r = match pool.new_conn() {
                Ok(mut c) => crate::doktor::reparieren(&mut c, &db, &gewaehlt, &|m| Self::sag(&st, m)),
                Err(e) => gewaehlt.iter().map(|_| Err(e.clone())).collect(),
            };
            let _ = tx.send((r, hinweis));
        });
        self.lauf = Some(rx);
    }

    fn poll(&mut self, cx: &mut Ctx, ctx: &egui::Context) {
        if let Some(rx) = &self.pruefung {
            match rx.try_recv() {
                Ok(Ok(f)) => {
                    // Was nur die Struktur aendert, ist vorausgewaehlt; Datenaenderungen waehlt man selbst
                    self.wahl = f.iter().map(|f| !f.daten && !f.sql.is_empty()).collect();
                    self.sql_offen = vec![false; f.len()];
                    self.funde = f;
                    self.pruefung = None;
                    self.seit = Instant::now();
                }
                Ok(Err(e)) => {
                    self.fehler = Some(e);
                    self.pruefung = None;
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
            }
        }
        if let Some(rx) = &self.lauf {
            match rx.try_recv() {
                Ok((r, hinweis)) => {
                    for (z, r) in self.zuletzt.iter_mut().zip(r) {
                        z.1 = r;
                    }
                    self.sicherung = hinweis;
                    self.lauf = None;
                    cx.actions.push(Action::SchemaChanged(self.db.clone()));
                    // Gleich nachsehen, was noch offen ist
                    self.pruefen(cx);
                }
                Err(_) => ctx.request_repaint_after(std::time::Duration::from_millis(150)),
            }
        }
    }

    fn art_titel(a: Art) -> &'static str {
        match a {
            Art::Engine => tr("Speicher-Engine", "Storage engine"),
            Art::Zeichensatz => tr("Zeichensatz", "Character set"),
            Art::Schluessel => tr("Primärschlüssel", "Primary keys"),
            Art::Typ => tr("Datentypen", "Data types"),
            Art::Leerzeichen => tr("Leerzeichen", "Spaces"),
            Art::Doppelt => tr("Doppelte Zeilen", "Duplicate rows"),
            Art::Verweis => tr("Verweise zwischen Tabellen", "References between tables"),
        }
    }

    fn sql_text(&self) -> String {
        let mut out = format!("-- {}\nUSE {};\n", tr("Reparaturen des Datenbank-Doktors", "Repairs from the database doctor"), crate::db::q(&self.db));
        for (f, w) in self.funde.iter().zip(&self.wahl) {
            if !*w || f.sql.is_empty() {
                continue;
            }
            out.push_str(&format!("\n-- {}\n", f.titel));
            for s in &f.sql {
                out.push_str(s);
                out.push_str(";\n");
            }
        }
        out
    }

    fn liste(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let t = ui.ctx().animate_bool_with_time(egui::Id::new(("doktor-liste", self.seit)), true, 0.25);
        ui.set_opacity(t);
        let mut letzte_art = None;
        for i in 0..self.funde.len() {
            let f = &self.funde[i];
            if letzte_art != Some(f.art) {
                letzte_art = Some(f.art);
                ui.add_space(if i == 0 { 2.0 } else { 14.0 });
                ui.label(RichText::new(Self::art_titel(f.art)).size(12.5).color(pal.text_weak).semi());
                ui.add_space(2.0);
            }
            let hinweis = f.sql.is_empty();
            egui::Frame::new().fill(pal.face_light).corner_radius(style::RADIUS).inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 10 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    if hinweis {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        icons::paint(ui.painter(), r, Icon::Info, pal.text_weak);
                    } else {
                        crate::widgets::check(ui, &mut self.wahl[i], "");
                    }
                    ui.label(RichText::new(&f.titel).semi());
                    if f.daten {
                        let g = ui.painter().layout_no_wrap(tr("ändert Daten", "changes data").into(), egui::FontId::proportional(12.0), pal.orange);
                        let (r, _) = ui.allocate_exact_size(g.size() + egui::vec2(14.0, 6.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 3.0, pal.orange.gamma_multiply(0.16));
                        ui.painter().galley(r.min + egui::vec2(7.0, 3.0), g, pal.orange);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.add(egui::Label::new(RichText::new(&f.text).color(pal.text_weak)).wrap());
                });
                if !hinweis {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.spacing_mut().button_padding = egui::vec2(0.0, 1.0);
                        let open = self.sql_offen[i];
                        if ui.add(egui::Button::new(RichText::new(if open { tr("SQL ausblenden", "Hide SQL") } else { tr("SQL anzeigen", "Show SQL") }).size(12.5).color(pal.blue)).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.sql_offen[i] = !open;
                        }
                    });
                    if self.sql_offen[i] {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            let mut s = f.sql.iter().map(|s| format!("{s};")).collect::<Vec<_>>().join("\n");
                            ui.add(egui::TextEdit::multiline(&mut s).code_editor().desired_rows(1).desired_width(f32::INFINITY).interactive(false));
                        });
                    }
                }
            });
            ui.add_space(6.0);
        }
    }
}

impl TabView for DoktorTab {
    fn title(&self) -> String {
        crate::tr_format!("Prüfung: {}", "Check: {}", self.db)
    }

    fn icon(&self) -> Option<Icon> {
        Some(Icon::Shield)
    }

    fn database(&self) -> Option<String> {
        Some(self.db.clone())
    }

    fn key(&self) -> Option<String> {
        Some(format!("doktor:{}", self.db))
    }

    fn busy(&self) -> bool {
        self.pruefung.is_some() || self.lauf.is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx, &ui.ctx().clone());
        if !self.gestartet && cx.db.is_some() {
            self.pruefen(cx);
        }
        let pal = style::pal();
        let beschaeftigt = self.busy();
        egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 20, top: 14, bottom: 0 }).show(ui, |ui| {
            // Kopf: Datenbank, neu pruefen, Stand
            ui.horizontal(|ui| {
                let mut db = self.db.clone();
                let liste: Vec<String> = cx.databases.iter().filter(|d| !crate::db::SYSTEM_DATABASES.contains(&d.as_str())).cloned().collect();
                if ui.add_enabled_ui(!beschaeftigt, |ui| super::db_pick(ui, "doktor-db", &liste, &mut db, 200.0)).inner && !db.is_empty() {
                    self.db = db;
                    self.funde.clear();
                    self.zuletzt.clear();
                    self.pruefen(cx);
                }
                if ui.add_enabled(!beschaeftigt, egui::Button::new(tr("Erneut prüfen", "Check again"))).clicked() {
                    self.zuletzt.clear();
                    self.pruefen(cx);
                }
                if beschaeftigt {
                    ui.spinner();
                    let s = self.status.lock().map(|g| g.clone()).unwrap_or_default();
                    ui.label(RichText::new(s).color(pal.text_weak));
                }
            });
            ui.add_space(14.0);
            if let Some(e) = &self.fehler {
                crate::widgets::error_label(ui, e);
                return;
            }
            if self.pruefung.is_some() && self.funde.is_empty() {
                return;
            }
            // Ergebnis der letzten Reparatur
            if !self.zuletzt.is_empty() && self.lauf.is_none() {
                let ok = self.zuletzt.iter().filter(|z| z.1.is_ok()).count();
                ui.label(RichText::new(crate::tr_format!("{ok} von {} Reparaturen ausgeführt.", "{ok} of {} repairs done.", self.zuletzt.len())).color(if ok == self.zuletzt.len() { pal.ok_text } else { pal.warn_text }));
                if let Some(h) = &self.sicherung {
                    ui.label(RichText::new(h).small().color(pal.text_weak));
                }
                for (t, r) in &self.zuletzt {
                    if let Err(e) = r {
                        ui.label(RichText::new(format!("{t}: {e}")).color(pal.error_text));
                    }
                }
                ui.add_space(10.0);
            }
            let echte = self.funde.iter().filter(|f| !f.sql.is_empty()).count();
            if self.funde.is_empty() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Check, pal.ok_text);
                    ui.label(RichText::new(crate::tr_format!("{} ist in Ordnung", "{} is in good shape", self.db)).size(19.0).semi());
                });
                ui.label(RichText::new(tr("Schlüssel, Verweise, Datentypen, Zeichensatz und Daten sind geprüft.", "Keys, references, data types, character set and data are checked.")).color(pal.text_weak));
                return;
            }
            ui.label(RichText::new(crate::tr_format!("{echte} Verbesserung(en) für {}", "{echte} improvement(s) for {}", self.db)).size(19.0).semi());
            let daten = self.funde.iter().filter(|f| f.daten).count();
            let unter = if daten > 0 {
                crate::tr_format!("{daten} davon ändern Daten und sind nicht vorausgewählt. Vor dem Reparieren wird die Datenbank gesichert.", "{daten} of them change data and are not preselected. The database is backed up before repairing.")
            } else {
                tr("Alle ändern nur die Struktur. Vor dem Reparieren wird die Datenbank gesichert.", "All only change the structure. The database is backed up before repairing.").to_string()
            };
            ui.label(RichText::new(unter).color(pal.text_weak));
            ui.add_space(10.0);
            // Aktionen
            let n = self.funde.iter().zip(&self.wahl).filter(|(f, w)| **w && !f.sql.is_empty()).count();
            let mut los = false;
            let mut als_sql = false;
            ui.horizontal(|ui| {
                ui.add_enabled_ui(n > 0 && !beschaeftigt, |ui| {
                    if icons::primary_button(ui, Icon::Wrench, &crate::tr_format!("{n} reparieren", "Repair {n}")).clicked() {
                        los = true;
                    }
                    if icons::text_button(ui, Icon::Code, tr("Als SQL öffnen", "Open as SQL")).clicked() {
                        als_sql = true;
                    }
                });
                if echte > 0 {
                    let alle = self.funde.iter().zip(&self.wahl).filter(|(f, _)| !f.sql.is_empty()).all(|(_, w)| *w);
                    if ui.add(egui::Button::new(if alle { tr("Keine wählen", "Select none") } else { tr("Alle wählen", "Select all") }).frame(false)).clicked() {
                        for (w, f) in self.wahl.iter_mut().zip(&self.funde) {
                            *w = !alle && !f.sql.is_empty();
                        }
                    }
                }
            });
            if los {
                self.reparieren(cx);
            }
            if als_sql {
                cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql: self.sql_text(), run: false });
            }
            ui.add_space(12.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_enabled_ui(!beschaeftigt, |ui| self.liste(ui));
                ui.add_space(20.0);
            });
        });
    }
}
