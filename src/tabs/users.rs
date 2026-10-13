// Benutzer und Rechte: Konten anlegen, Passwort aendern, Rechte je Datenbank oder Tabelle vergeben.

use super::{Action, Ctx, TabView};
use crate::db::{Account, Scope, diff_sql, lit, q};
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, RichText};

struct NewUser {
    name: String,
    host: String,
    password: String,
    own_db: bool,
}

pub struct UsersTab {
    accounts: Vec<Account>,
    loaded: bool,
    error: Option<String>,
    filter: String,
    sel: Option<usize>,
    grants: Vec<(Scope, Vec<String>)>,
    /// Gewaehlte Ebene: leere Datenbank = global, leere Tabelle = ganze Datenbank
    db: String,
    table: String,
    wanted: Vec<String>,
    wanted_for: Option<Scope>,
    new_user: Option<NewUser>,
    password: Option<String>,
}

impl UsersTab {
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            loaded: false,
            error: None,
            filter: String::new(),
            sel: None,
            grants: Vec::new(),
            db: String::new(),
            table: String::new(),
            wanted: Vec::new(),
            wanted_for: None,
            new_user: None,
            password: None,
        }
    }

    fn scope(&self) -> Scope {
        match (self.db.is_empty(), self.table.is_empty()) {
            (true, _) => Scope::Global,
            (false, true) => Scope::Db(self.db.clone()),
            (false, false) => Scope::Table(self.db.clone(), self.table.clone()),
        }
    }

    fn current(&self) -> Vec<String> {
        let s = self.scope();
        self.grants.iter().find(|(g, _)| *g == s).map(|(_, p)| p.clone()).unwrap_or_default()
    }

    fn account(&self) -> Option<&Account> {
        self.sel.and_then(|i| self.accounts.get(i))
    }

    fn reload(&mut self, cx: &mut Ctx) {
        self.loaded = true;
        let Some(dbc) = cx.db else { return };
        match dbc.accounts() {
            Ok(a) => {
                self.error = None;
                self.accounts = a;
            }
            Err(e) => self.error = Some(e),
        }
        self.load_grants(cx);
    }

    fn load_grants(&mut self, cx: &mut Ctx) {
        self.grants.clear();
        self.wanted_for = None;
        if let (Some(dbc), Some(a)) = (cx.db, self.account().cloned()) {
            match dbc.grants(&a) {
                Ok(g) => self.grants = g,
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn select(&mut self, i: usize, cx: &mut Ctx) {
        self.sel = Some(i);
        self.password = None;
        self.db.clear();
        self.table.clear();
        self.load_grants(cx);
    }

    fn run(&mut self, cx: &mut Ctx, sql: &[String]) -> bool {
        let Some(dbc) = cx.db else { return false };
        for s in sql {
            if let Err(e) = dbc.exec(s, ()) {
                cx.error(format!("{e}\n\nSQL: {s}"));
                return false;
            }
        }
        true
    }

    fn preset(&mut self, which: u8) {
        let all = self.scope().privileges();
        self.wanted = match which {
            0 => vec![],
            1 => ["SELECT", "SHOW VIEW"].iter().map(|s| s.to_string()).collect(),
            2 => ["SELECT", "SHOW VIEW", "INSERT", "UPDATE", "DELETE"].iter().map(|s| s.to_string()).collect(),
            _ => all.iter().filter(|p| **p != "GRANT OPTION").map(|s| s.to_string()).collect(),
        };
        self.wanted.retain(|p| all.contains(&p.as_str()));
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        ui.horizontal(|ui| {
            ui.add_sized([ui.available_width() - 56.0, 22.0], egui::TextEdit::singleline(&mut self.filter).hint_text(tr("Filtern", "Filter")));
            if icons::button(ui, Icon::Plus, tr("Neuer Benutzer", "New user")).clicked() {
                self.new_user = Some(NewUser { name: String::new(), host: "localhost".into(), password: String::new(), own_db: true });
            }
            if icons::button(ui, Icon::Refresh, tr("Aktualisieren", "Refresh")).clicked() {
                self.reload(cx);
            }
        });
        ui.add_space(4.0);
        let f = self.filter.to_lowercase();
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("users-list").auto_shrink([false, false]).show(ui, |ui| {
            for (i, a) in self.accounts.iter().enumerate() {
                if !f.is_empty() && !a.label().to_lowercase().contains(&f) {
                    continue;
                }
                let sel = self.sel == Some(i);
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
                if sel {
                    ui.painter().rect_filled(r, 4.0, pal.accent.gamma_multiply(0.28));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, pal.hover);
                }
                icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 14.0, r.center().y), egui::vec2(15.0, 15.0)), Icon::Users, pal.text_weak);
                let name = if a.user.is_empty() { tr("(anonym)", "(anonymous)").to_string() } else { a.user.clone() };
                let g = ui.painter().layout_no_wrap(name, egui::FontId::proportional(13.0), pal.text);
                let w = g.size().x;
                ui.painter().galley(egui::pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, pal.text);
                ui.painter().text(egui::pos2(r.left() + 34.0 + w, r.center().y), egui::Align2::LEFT_CENTER, &a.host, egui::FontId::proportional(11.5), pal.text_weak);
                if resp.clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            self.select(i, cx);
        }
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let Some(acc) = self.account().cloned() else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(tr("Benutzer wählen", "Select a user")).color(pal.text_weak)));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(acc.label()).size(16.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::text_button(ui, Icon::Trash, tr("Löschen", "Delete")).clicked() {
                    cx.actions.push(Action::Confirm { text: crate::tr_format!("Benutzer {} löschen?", "Delete user {}?", acc.label()), db: None, sql: format!("DROP USER {}", acc.sql()) });
                }
                if icons::text_button(ui, Icon::Lock, tr("Passwort ändern", "Change password")).clicked() {
                    self.password = if self.password.is_some() { None } else { Some(String::new()) };
                }
            });
        });
        if let Some(pw) = self.password.as_mut() {
            let mut apply = false;
            ui.horizontal(|ui| {
                let r = ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(pw).password(true).hint_text(tr("Neues Passwort", "New password")));
                if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button("OK").clicked() {
                    apply = true;
                }
            });
            if apply {
                let sql = format!("ALTER USER {} IDENTIFIED BY {}", acc.sql(), lit(pw));
                if self.run(cx, &[sql]) {
                    cx.status(tr("Passwort geändert.", "Password changed."));
                    self.password = None;
                }
            }
        }
        ui.add_space(10.0);

        // Wo hat der Benutzer Rechte? (Klick wechselt die Ebene)
        let mut jump: Option<Scope> = None;
        ui.label(RichText::new(tr("Rechte", "Privileges")).strong());
        style::group_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let shown: Vec<&(Scope, Vec<String>)> = self.grants.iter().filter(|(_, p)| !p.is_empty()).collect();
            if shown.is_empty() {
                ui.label(RichText::new(tr("Keine Rechte vergeben", "No privileges granted")).color(pal.text_weak));
            }
            for (s, p) in shown {
                let name = match s {
                    Scope::Global => tr("Global (alle Datenbanken)", "Global (all databases)").to_string(),
                    Scope::Db(d) => d.clone(),
                    Scope::Table(d, t) => format!("{d}.{t}"),
                };
                ui.horizontal(|ui| {
                    if ui.link(name).clicked() {
                        jump = Some(s.clone());
                    }
                    let list = if p.len() > 6 { format!("{} …", p[..6].join(", ")) } else { p.join(", ") };
                    ui.label(RichText::new(list).small().color(pal.text_weak));
                });
            }
        });
        if let Some(s) = jump {
            match s {
                Scope::Global => (self.db.clear(), self.table.clear()),
                Scope::Db(d) => (self.db = d, self.table.clear()),
                Scope::Table(d, t) => (self.db = d, self.table = t),
            };
        }
        ui.add_space(10.0);

        // Ebene waehlen
        let mut dbs = vec![String::new()];
        dbs.extend(cx.databases.iter().filter(|d| !crate::db::SYSTEM_DATABASES.contains(&d.as_str())).cloned());
        ui.horizontal(|ui| {
            ui.label(tr("Gilt für", "Applies to"));
            let mut d = self.db.clone();
            let label = if d.is_empty() { tr("Alle Datenbanken (global)", "All databases (global)").to_string() } else { d.clone() };
            egui::ComboBox::from_id_salt("users-db").selected_text(label).width(220.0).show_ui(ui, |ui| {
                for x in &dbs {
                    let l = if x.is_empty() { tr("Alle Datenbanken (global)", "All databases (global)").to_string() } else { x.clone() };
                    if ui.selectable_label(&d == x, l).clicked() {
                        d = x.clone();
                    }
                }
            });
            if d != self.db {
                self.db = d;
                self.table.clear();
            }
            if !self.db.is_empty() {
                let mut tables = vec![String::new()];
                if let Some(s) = cx.schema(&self.db) {
                    tables.extend(s.tables.iter().filter(|t| !t.is_view).map(|t| t.name.clone()));
                }
                let label = if self.table.is_empty() { tr("Alle Tabellen", "All tables").to_string() } else { self.table.clone() };
                egui::ComboBox::from_id_salt("users-table").selected_text(label).width(200.0).show_ui(ui, |ui| {
                    for t in &tables {
                        let l = if t.is_empty() { tr("Alle Tabellen", "All tables").to_string() } else { t.clone() };
                        if ui.selectable_label(&self.table == t, l).clicked() {
                            self.table = t.clone();
                        }
                    }
                });
            }
        });
        let scope = self.scope();
        if self.wanted_for.as_ref() != Some(&scope) {
            self.wanted = self.current();
            self.wanted_for = Some(scope.clone());
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (i, l) in [tr("Keine", "None"), tr("Nur lesen", "Read only"), tr("Lesen & Schreiben", "Read & write"), tr("Alles", "Everything")].into_iter().enumerate() {
                if ui.button(l).clicked() {
                    self.preset(i as u8);
                }
            }
        });
        ui.add_space(4.0);
        style::group_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::Grid::new("users-privs").num_columns(3).striped(false).spacing([24.0, 4.0]).show(ui, |ui| {
                for (i, p) in scope.privileges().iter().enumerate() {
                    let mut on = self.wanted.iter().any(|w| w == p);
                    if ui.checkbox(&mut on, *p).changed() {
                        if on {
                            self.wanted.push(p.to_string());
                        } else {
                            self.wanted.retain(|w| w != p);
                        }
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        });
        let current = self.current();
        let sql = diff_sql(&acc, &scope, &current, &self.wanted);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let r = ui.add_enabled(!sql.is_empty(), egui::Button::new(RichText::new(tr("Übernehmen", "Apply")).strong()));
            if r.on_hover_text(sql.join(";\n")).clicked() && self.run(cx, &sql) {
                cx.status(tr("Rechte geändert.", "Privileges changed."));
                self.load_grants(cx);
            }
            if ui.add_enabled(!sql.is_empty(), egui::Button::new(tr("Zurücksetzen", "Reset"))).clicked() {
                self.wanted = current;
            }
        });
    }

    fn new_user_ui(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(n) = self.new_user.as_mut() else { return };
        let (mut create, mut cancel) = (false, false);
        egui::Window::new(tr("Neuer Benutzer", "New user")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::Grid::new("newuser").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Name", "Name"));
                ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut n.name));
                ui.end_row();
                ui.label("Host");
                ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut n.host)).on_hover_text(tr("localhost = nur dieser Computer, % = von überall", "localhost = this computer only, % = from anywhere"));
                ui.end_row();
                ui.label(tr("Passwort", "Password"));
                ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut n.password).password(true));
                ui.end_row();
            });
            ui.checkbox(&mut n.own_db, tr("Eigene Datenbank mit allen Rechten anlegen", "Create a database of its own with full access"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                create = ui.add_enabled(!n.name.trim().is_empty() && !n.host.trim().is_empty(), egui::Button::new(tr("Anlegen", "Create"))).clicked();
                cancel = ui.button(tr("Abbrechen", "Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        });
        if create {
            let acc = Account { user: n.name.trim().to_string(), host: n.host.trim().to_string() };
            let mut sql = vec![if n.password.is_empty() { format!("CREATE USER {}", acc.sql()) } else { format!("CREATE USER {} IDENTIFIED BY {}", acc.sql(), lit(&n.password)) }];
            if n.own_db {
                sql.push(format!("CREATE DATABASE IF NOT EXISTS {}", q(&acc.user)));
                sql.push(format!("GRANT ALL PRIVILEGES ON {} TO {}", Scope::Db(acc.user.clone()).sql(), acc.sql()));
            }
            self.new_user = None;
            if self.run(cx, &sql) {
                cx.status(crate::tr_format!("Benutzer {} angelegt.", "User {} created.", acc.label()));
                self.reload(cx);
                self.sel = self.accounts.iter().position(|a| *a == acc);
                self.load_grants(cx);
                if sql.len() > 1 {
                    cx.actions.push(Action::RefreshAll);
                }
            }
        } else if cancel {
            self.new_user = None;
        }
    }
}

impl TabView for UsersTab {
    fn icon(&self) -> Option<Icon> {
        Some(Icon::Users)
    }

    fn title(&self) -> String {
        tr("Benutzer & Rechte", "Users & privileges").into()
    }

    fn key(&self) -> Option<String> {
        Some("users".into())
    }

    fn session(&self) -> Option<String> {
        Some("users".into())
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.reload(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if !self.loaded {
            self.reload(cx);
        }
        let pal = style::pal();
        if cx.db.is_none() {
            ui.label(RichText::new(tr("Keine Verbindung.", "Not connected.")).color(pal.text_weak));
            return;
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(pal.error_text));
        }
        egui::Panel::left("users-left").resizable(true).default_size(260.0).min_size(180.0).frame(egui::Frame::new().inner_margin(egui::Margin { right: 8, ..Default::default() })).show(ui, |ui| self.list_ui(ui, cx));
        egui::Frame::new().inner_margin(egui::Margin { left: 12, ..Default::default() }).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("users-detail").auto_shrink([false, false]).show(ui, |ui| self.detail_ui(ui, cx));
        });
        let ctx = ui.ctx().clone();
        self.new_user_ui(&ctx, cx);
    }
}
