// Registerkarte "Sicherungen & Reparatur".

use super::{Action, Ctx, TabView};
use crate::backup::{self, BackupInfo, Config};
use crate::db::ConnInfo;
use crate::style;
use eframe::egui::{self, RichText};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

struct Job {
    title: String,
    log: Arc<Mutex<Vec<String>>>,
    rx: Receiver<Result<String, String>>,
    reconnect: bool,
}

struct RestoreDlg {
    backup: BackupInfo,
    db: String,
    target: String,
}

pub struct SafetyTab {
    cfg: Config,
    list: Vec<BackupInfo>,
    loaded: bool,
    job: Option<Job>,
    log: Vec<String>,
    result: Option<Result<String, String>>,
    restore: Option<RestoreDlg>,
    confirm_rescue: bool,
    confirm_delete: Option<BackupInfo>,
    myisam: Option<Vec<(String, String)>>,
}

impl SafetyTab {
    pub fn new() -> Self {
        Self {
            cfg: Config::load(),
            list: Vec::new(),
            loaded: false,
            job: None,
            log: Vec::new(),
            result: None,
            restore: None,
            confirm_rescue: false,
            confirm_delete: None,
            myisam: None,
        }
    }

    fn refresh(&mut self, cx: &mut Ctx) {
        self.loaded = true;
        self.list = backup::list(&self.cfg.dir());
        self.myisam = cx.db.and_then(|d| crate::repair::myisam_tables(&d.info).ok());
    }

    fn conn(cx: &Ctx) -> ConnInfo {
        cx.db.map(|d| d.info.clone()).unwrap_or_default()
    }

    fn env(&self, cx: &Ctx) -> Option<backup::Env> {
        Some(backup::Env { paths: cx.server.paths.clone()?, conn: Self::conn(cx), dir: self.cfg.dir(), force: false })
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        reconnect: bool,
        f: impl FnOnce(&dyn Fn(String)) -> Result<String, String> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let log = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel();
        let l2 = log.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let logger = |m: String| {
                l2.lock().unwrap().push(m);
                ctx2.request_repaint();
            };
            let r = f(&logger);
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.result = None;
        self.log.clear();
        self.job = Some(Job { title: title.to_string(), log, rx, reconnect });
    }

    fn poll(&mut self, cx: &mut Ctx) {
        let Some(job) = &self.job else { return };
        self.log = job.log.lock().unwrap().clone();
        if let Ok(r) = job.rx.try_recv() {
            let reconnect = job.reconnect;
            let title = job.title.clone();
            self.job = None;
            match &r {
                Ok(m) => cx.status(format!("{title}: {}", m.lines().next().unwrap_or("fertig"))),
                Err(e) => cx.status(format!("{title}: Fehler – {}", e.lines().next().unwrap_or(""))),
            }
            self.result = Some(r);
            if reconnect {
                cx.actions.push(Action::Reconnect);
            }
            cx.actions.push(Action::RefreshAll);
            self.refresh(cx);
        }
    }

    fn backups_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let busy = self.job.is_some();
        let connected = cx.db.is_some();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!busy && connected, egui::Button::new("Jetzt sichern")).clicked() {
                if let Some(env) = self.env(cx) {
                    let cfg = self.cfg.clone();
                    self.start(ui.ctx(), "Sicherung", false, move |log| {
                        let dir = backup::create(&env, None, "manuell", log)?;
                        backup::rotate(&cfg.dir(), cfg.keep_count, cfg.keep_days);
                        Ok(format!("Sicherung erstellt: {}", dir.display()))
                    });
                }
            }
            if ui.button("Ordner öffnen").clicked() {
                let d = self.cfg.dir();
                let _ = std::fs::create_dir_all(&d);
                crate::app::open_folder(&d);
            }
            if ui.button("Aktualisieren").clicked() {
                self.refresh(cx);
            }
        });
        let old = self.cfg.clone();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.cfg.auto, "Automatisch sichern, alle");
            ui.add(egui::DragValue::new(&mut self.cfg.interval_hours).range(1..=168).suffix(" h"));
            ui.label("· behalten: die letzten");
            ui.add(egui::DragValue::new(&mut self.cfg.keep_count).range(1..=200));
            ui.label("+ eine pro Tag für");
            ui.add(egui::DragValue::new(&mut self.cfg.keep_days).range(0..=365).suffix(" Tage"));
        });
        ui.horizontal(|ui| {
            ui.label("Ordner:");
            ui.label(RichText::new(self.cfg.dir().display().to_string()).monospace().small());
            if ui.small_button("Ändern...").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.cfg.dir = p.display().to_string();
                }
            }
            if !self.cfg.dir.is_empty() && ui.small_button("Standard").clicked() {
                self.cfg.dir.clear();
            }
        });
        if self.cfg != old {
            self.cfg.save();
            if self.cfg.dir != old.dir {
                self.refresh(cx);
            }
        }
        ui.label(
            RichText::new("Zusätzlich wird automatisch gesichert, bevor Datenbanken/Tabellen gelöscht oder geleert werden.")
                .small()
                .color(style::pal().null_text),
        );
        ui.add_space(4.0);
        style::sunken_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("backuplist").max_height(220.0).show(ui, |ui| {
                if self.list.is_empty() {
                    ui.label(RichText::new("Noch keine Sicherungen vorhanden.").color(style::pal().null_text));
                }
                egui::Grid::new("backups").striped(true).num_columns(5).spacing([16.0, 4.0]).show(ui, |ui| {
                    for h in ["Zeitpunkt", "Anlass", "Datenbanken", "Größe", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for b in self.list.clone() {
                        ui.label(&b.time);
                        ui.label(backup::reason_text(&b.reason));
                        let names: Vec<&str> = b.dbs.iter().map(|d| d.0.as_str()).collect();
                        ui.label(if names.is_empty() { "–".to_string() } else { names.join(", ") });
                        ui.label(backup::human_size(b.total()));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy && connected && !b.dbs.is_empty(), egui::Button::new("Wiederherstellen...").small()).clicked() {
                                let db = b.dbs[0].0.clone();
                                self.restore = Some(RestoreDlg { target: db.clone(), db, backup: b.clone() });
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Löschen").small()).clicked() {
                                self.confirm_delete = Some(b.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        });
    }

    fn repair_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let busy = self.job.is_some();
        let st = cx.server.state();
        ui.label(format!("Server: {}", st.text()));
        ui.horizontal_wrapped(|ui| {
            let connected = cx.db.is_some();
            if ui
                .add_enabled(!busy && connected, egui::Button::new("Alle Tabellen prüfen"))
                .on_hover_text("CHECK TABLE für alle Tabellen – ändert nichts")
                .clicked()
            {
                let conn = Self::conn(cx);
                self.start(ui.ctx(), "Prüfung", false, move |log| {
                    let r = crate::repair::check_all(&conn, false, log)?;
                    Ok(if r.problems.is_empty() {
                        format!("{} Tabellen geprüft – alles in Ordnung.", r.checked)
                    } else {
                        format!("{} Tabellen geprüft, {} mit Problemen. Bitte \"Prüfen und reparieren\" wählen.", r.checked, r.problems.len())
                    })
                });
            }
            if ui
                .add_enabled(!busy && connected, egui::Button::new("Prüfen und reparieren"))
                .on_hover_text("Prüft alle Tabellen und repariert beschädigte (vorher wird gesichert)")
                .clicked()
            {
                let conn = Self::conn(cx);
                let env = self.env(cx);
                self.start(ui.ctx(), "Reparatur", false, move |log| {
                    if let Some(env) = &env {
                        log("Sichere vorher alle Datenbanken ...".into());
                        if let Err(e) = backup::create(env, None, "vor-reparatur", log) {
                            log(format!("Hinweis: Sicherung nicht vollständig: {e}"));
                        }
                    }
                    let r = crate::repair::check_all(&conn, true, log)?;
                    let mut msg = format!("{} Tabellen geprüft", r.checked);
                    if r.problems.is_empty() {
                        msg.push_str(" – alles in Ordnung.");
                    } else {
                        msg.push_str(&format!(", {} repariert", r.repaired.len()));
                        if !r.failed.is_empty() {
                            msg.push_str(&format!(
                                ", {} nicht reparierbar:\n{}",
                                r.failed.len(),
                                r.failed.iter().map(|(t, m)| format!("  {t}: {m}")).collect::<Vec<_>>().join("\n")
                            ));
                        } else {
                            msg.push('.');
                        }
                    }
                    Ok(msg)
                });
            }
            if let Some(list) = self.myisam.clone().filter(|l| !l.is_empty()) {
                if ui
                    .add_enabled(!busy, egui::Button::new(format!("{} MyISAM-Tabellen → InnoDB", list.len())))
                    .on_hover_text("MyISAM ist nicht absturzsicher. InnoDB verliert bei Absturz/Stromausfall keine bestätigten Daten.")
                    .clicked()
                {
                    let conn = Self::conn(cx);
                    self.start(ui.ctx(), "Umwandlung", false, move |log| {
                        let n = crate::repair::convert_to_innodb(&conn, &list, log)?;
                        Ok(format!("{n} Tabellen in InnoDB umgewandelt."))
                    });
                }
            }
            if ui
                .add_enabled(!busy && !st.is_busy() && cx.server.paths.is_some(), egui::Button::new("Server retten..."))
                .on_hover_text("Wenn der Server nicht mehr startet oder die Daten stark beschädigt sind")
                .clicked()
            {
                self.confirm_rescue = true;
            }
        });
        ui.label(
            RichText::new(
                "Prüfen/Reparieren: bei einzelnen beschädigten Tabellen.  Server retten: wenn MariaDB nicht mehr startet – \
                 die Daten werden aus dem alten Datenordner gerettet (notfalls aus der neuesten Sicherung) und in einen \
                 neuen Datenordner eingespielt. Der alte Ordner bleibt immer erhalten.",
            )
            .small()
            .color(style::pal().null_text),
        );
    }

    fn dialogs(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        if let Some(dlg) = self.restore.as_mut() {
            let mut go = false;
            let mut cancel = false;
            egui::Window::new("Aus Sicherung wiederherstellen")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Sicherung vom {} ({})", dlg.backup.time, backup::reason_text(&dlg.backup.reason)));
                    egui::Grid::new("restgrid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Datenbank:");
                        let dbs: Vec<String> = dlg.backup.dbs.iter().map(|d| d.0.clone()).collect();
                        let old = dlg.db.clone();
                        super::str_combo(ui, "restdb", &dbs, &mut dlg.db, 180.0);
                        if old != dlg.db {
                            dlg.target = dlg.db.clone();
                        }
                        ui.end_row();
                        ui.label("Einspielen als:");
                        ui.text_edit_singleline(&mut dlg.target);
                        ui.end_row();
                    });
                    if cx.databases.contains(&dlg.target) {
                        ui.label(
                            RichText::new(format!(
                                "Die vorhandene Datenbank \"{}\" wird ersetzt (vorher wird sie automatisch gesichert).",
                                dlg.target
                            ))
                            .color(style::pal().error_text),
                        );
                    } else {
                        ui.label(RichText::new("Wird als neue Datenbank angelegt.").color(style::pal().ok_text));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!dlg.target.trim().is_empty(), egui::Button::new("Wiederherstellen")).clicked() {
                            go = true;
                        }
                        if ui.button("Abbrechen").clicked() {
                            cancel = true;
                        }
                    });
                });
            if go {
                let d = self.restore.take().unwrap();
                if let Some(env) = self.env(cx) {
                    let file = d.backup.path.join(format!("{}.sql.gz", d.db));
                    let target = d.target.trim().to_string();
                    self.start(ctx, "Wiederherstellung", false, move |log| {
                        backup::restore(&env, &file, &target, log)?;
                        Ok(format!("Datenbank {target} wiederhergestellt."))
                    });
                }
            } else if cancel {
                self.restore = None;
            }
        }

        if self.confirm_rescue {
            let mut go = false;
            egui::Window::new("Server retten")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        "Der Server wird beendet, der aktuelle Datenordner wird umbenannt und bleibt erhalten.\n\
                         Danach werden die Daten gerettet, ein neuer Datenordner angelegt und alles wieder eingespielt.\n\
                         Benutzerkonten werden zurückgesetzt (root ohne Passwort).\n\n\
                         Das kann einige Minuten dauern. Fortfahren?",
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Ja, Server retten").clicked() {
                            go = true;
                        }
                        if ui.button("Abbrechen").clicked() {
                            self.confirm_rescue = false;
                        }
                    });
                });
            if go {
                self.confirm_rescue = false;
                let server = cx.server.clone();
                let conn = Self::conn(cx);
                let dir = self.cfg.dir();
                let ctx2 = ctx.clone();
                cx.actions.push(Action::Disconnect);
                self.start(ctx, "Server-Rettung", true, move |_log| {
                    let r = server.rescue(&conn, &dir, &|| ctx2.request_repaint());
                    if let Err(e) = &r {
                        server.log(format!("Rettung fehlgeschlagen: {e}"));
                    }
                    r
                });
            }
        }

        if let Some(b) = self.confirm_delete.clone() {
            egui::Window::new("Sicherung löschen")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Sicherung vom {} endgültig löschen?", b.time));
                    ui.horizontal(|ui| {
                        if ui.button("Löschen").clicked() {
                            let _ = std::fs::remove_dir_all(&b.path);
                            self.confirm_delete = None;
                            self.refresh(cx);
                        }
                        if ui.button("Abbrechen").clicked() {
                            self.confirm_delete = None;
                        }
                    });
                });
        }
    }
}

impl TabView for SafetyTab {
    fn title(&self) -> String {
        "Sicherungen & Reparatur".into()
    }

    fn key(&self) -> Option<String> {
        Some("safety".into())
    }

    fn session(&self) -> Option<String> {
        Some("safety".into())
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.refresh(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if !self.loaded {
            self.refresh(cx);
        }
        self.poll(cx);
        if self.job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        egui::ScrollArea::vertical().id_salt("safetyscroll").show(ui, |ui| {
            ui.heading("Sicherungen");
            style::group_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.backups_ui(ui, cx);
            });
            ui.add_space(10.0);
            ui.heading("Prüfen & Reparieren");
            style::group_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.repair_ui(ui, cx);
            });
            ui.add_space(10.0);
            if let Some(job) = &self.job {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(format!("{} läuft ...", job.title)).strong());
                });
            }
            if let Some(r) = &self.result {
                match r {
                    Ok(m) => ui.label(RichText::new(m).color(style::pal().ok_text)),
                    Err(e) => ui.label(RichText::new(format!("Fehler: {e}")).color(style::pal().error_text)),
                };
            }
            if !self.log.is_empty() {
                style::sunken_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    egui::ScrollArea::vertical().id_salt("safetylog").max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                        for l in &self.log {
                            ui.label(RichText::new(l).monospace().small());
                        }
                    });
                });
            }
        });
        let ctx = ui.ctx().clone();
        self.dialogs(&ctx, cx);
    }
}
