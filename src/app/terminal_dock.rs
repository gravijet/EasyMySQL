// Terminal-Bereich unter dem Editor (wie in Visual Studio Code): mehrere Terminals in Registerkarten.

use super::*;
use crate::icons::{self, Icon};
use crate::terminal::{self, Profile, Session};
use eframe::egui::{Sense, Stroke, pos2, vec2};

struct TermTab {
    uid: usize,
    title: String,
    session: Session,
    /// Von "Ausfuehren" gestartetes Programm (Java) statt einer Shell
    program: bool,
    /// Erkannte Fehler mit Behebung; Laenge der Ausgabe bei der letzten Untersuchung
    fixes: Vec<crate::dev::autofix::Suggestion>,
    analyzed: (usize, Option<Instant>),
    /// Schon angewendete Behebungen (Index in `fixes`)
    applied: Vec<usize>,
}

impl TermTab {
    fn new(uid: usize, title: String, session: Session, program: bool) -> Self {
        TermTab { uid, title, session, program, fixes: Vec::new(), analyzed: (0, None), applied: Vec::new() }
    }
}

#[derive(Default)]
pub struct TerminalDock {
    pub open: bool,
    tabs: Vec<TermTab>,
    active: usize,
    next_uid: usize,
    /// Der Fokus soll im naechsten Bild auf das aktive Terminal gehen
    focus: bool,
}

impl TerminalDock {
    pub fn close_all(&mut self) {
        self.tabs.clear();
    }
}

impl EasyApp {
    fn terminal_profiles(&self) -> Vec<Profile> {
        // Mit SSH-Tunnel gilt die Adresse des Tunnels
        let conn = self.db.as_ref().map(|d| d.info.clone()).unwrap_or_else(|| self.settings.conn.clone());
        terminal::profiles(self.server.paths().as_ref().map(|p| p.bin.as_path()), &conn)
    }

    /// Neues Terminal in der gewaehlten Shell (None = Standard) im Projektordner
    pub(super) fn new_terminal(&mut self, ctx: &egui::Context, profile: Option<Profile>) {
        self.new_terminal_in(ctx, profile, None);
    }

    /// Neues Terminal in einem bestimmten Ordner (None = Projektordner)
    pub(super) fn new_terminal_in(&mut self, ctx: &egui::Context, profile: Option<Profile>, dir: Option<PathBuf>) {
        let profile = profile.or_else(|| self.terminal_profiles().into_iter().next());
        let Some(profile) = profile else { return };
        let cwd = dir
            .filter(|p| p.is_dir())
            .or_else(|| self.project.clone())
            .filter(|p| p.is_dir())
            .or_else(|| std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from))
            .unwrap_or_else(std::env::temp_dir);
        let mut profile = profile;
        // Java, javac, mvn und gradle stehen in jedem Terminal bereit (wenn vorhanden)
        let tools = crate::dev::tools();
        profile.env.extend(tools.shell_env());
        let prefix = self.path_prefix();
        let d = &mut self.terminal;
        let same = d.tabs.iter().filter(|t| t.session.profile.name == profile.name && !t.program).count();
        let title = if same == 0 { profile.name.clone() } else { format!("{} {}", profile.name, same + 1) };
        d.next_uid += 1;
        d.tabs.push(TermTab::new(d.next_uid, title, Session::spawn(ctx, profile, cwd, prefix), false));
        d.active = d.tabs.len() - 1;
        d.open = true;
        d.focus = true;
    }

    /// Ordner vorne in PATH: MariaDB-Programme und die Java-Werkzeuge
    fn path_prefix(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.server.paths().map(|p| p.bin).into_iter().collect();
        v.extend(crate::dev::tools().path_dirs());
        v
    }

    /// Programm im Terminal starten. Ein frueherer Lauf desselben Programms wird beendet und ersetzt,
    /// so sammeln sich bei jedem Klick auf "Ausfuehren" keine Registerkarten an.
    pub(super) fn run_program(&mut self, ctx: &egui::Context, launch: crate::dev::process::Launch, mut env: Vec<(String, String)>) {
        // Die Umgebung des Starts (z. B. PYTHONPATH einer .venv) hat Vorrang
        env.retain(|(k, _)| !launch.env.iter().any(|(k2, _)| k2 == k));
        env.extend(launch.env.iter().cloned());
        let profile = Profile { name: launch.title.clone(), program: launch.program.display().to_string(), args: launch.args, env };
        let prefix = self.path_prefix();
        let session = Session::spawn(ctx, profile, launch.cwd, prefix);
        let d = &mut self.terminal;
        d.next_uid += 1;
        let tab = TermTab::new(d.next_uid, launch.title.clone(), session, true);
        let reuse = crate::dev::config().reuse_terminal;
        match d.tabs.iter().position(|t| reuse && t.program && t.title == launch.title) {
            Some(i) => {
                d.tabs[i] = tab;
                d.active = i;
            }
            None => {
                d.tabs.push(tab);
                d.active = d.tabs.len() - 1;
            }
        }
        d.open = true;
        d.focus = true;
    }

    /// Terminal-Bereich ein- oder ausblenden (laufende Shells bleiben erhalten)
    pub(super) fn toggle_terminal(&mut self, ctx: &egui::Context) {
        if self.terminal.open {
            self.terminal.open = false;
        } else if self.terminal.tabs.is_empty() {
            self.new_terminal(ctx, None);
        } else {
            self.terminal.open = true;
            self.terminal.focus = true;
        }
    }

    pub(super) fn terminal_panel(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let ctx = ui.ctx().clone();
        let mut new_profile: Option<Option<Profile>> = None;
        let mut close: Option<usize> = None;
        let mut hide = false;
        let mut fixes: Vec<crate::dev::autofix::Fix> = Vec::new();
        let mut link: Option<(String, usize, String, PathBuf)> = None;
        let frame = egui::Frame::new().fill(pal.bg);
        egui::Panel::bottom("terminal-dock").resizable(true).show_separator_line(true).default_size(280.0).min_size(110.0).frame(frame).show(ui, |ui| {
          style::panel_card(ui, pal.bg, egui::Margin::symmetric(8, 6), |ui| {
            // Kopfzeile: Registerkarten links, Aktionen rechts
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
            let painter = ui.painter_at(bar);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
            child.spacing_mut().item_spacing.x = 0.0;
            child.add_space(10.0);
            child.label(egui::RichText::new(crate::i18n::text("TERMINAL")).small().color(pal.text_weak));
            child.add_space(10.0);
            let active = self.terminal.active.min(self.terminal.tabs.len().saturating_sub(1));
            for (i, t) in self.terminal.tabs.iter().enumerate() {
                let sel = i == active;
                let galley = painter.layout_no_wrap(t.title.clone(), egui::FontId::proportional(13.0), pal.text);
                let w = galley.size().x + 56.0;
                let (r, resp) = child.allocate_exact_size(vec2(w, 28.0), Sense::click());
                if sel {
                    painter.rect(r, style::RADIUS as f32, pal.face_light, Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
                } else if resp.hovered() {
                    painter.rect_filled(r, style::RADIUS as f32, pal.hover);
                }
                let color = if sel { pal.text } else { pal.text_weak };
                let icon = if t.program { Icon::Play } else { Icon::Terminal };
                icons::paint(&painter, egui::Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0)), icon, if t.program && !t.session.exited() { pal.ok_text } else { color });
                painter.galley(pos2(r.left() + 30.0, r.center().y - galley.size().y / 2.0), galley, color);
                let cr = egui::Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0));
                let over_close = ctx.pointer_hover_pos().is_some_and(|p| cr.contains(p));
                if sel || resp.hovered() {
                    if over_close {
                        painter.rect_filled(cr, 3.0, pal.hover);
                    }
                    icons::paint(&painter, cr, Icon::Close, color);
                }
                if resp.clicked() {
                    if over_close {
                        close = Some(i);
                    } else {
                        self.terminal.active = i;
                        self.terminal.focus = true;
                    }
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
            }
            // Aktionen rechts
            let mut right = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            if icons::button(&mut right, Icon::Close, crate::i18n::text("Terminal ausblenden")).clicked() {
                hide = true;
            }
            if icons::button(&mut right, Icon::Trash, crate::i18n::text("Dieses Terminal beenden")).clicked() && !self.terminal.tabs.is_empty() {
                close = Some(active);
            }
            let more = icons::button(&mut right, Icon::ChevronDown, crate::i18n::text("Shell wählen"));
            egui::Popup::menu(&more).show(|ui| {
                crate::style::menu_item_look(ui);
                // Erst beim Oeffnen ermitteln (sucht Programme auf der Platte)
                for p in &self.terminal_profiles() {
                    if icons::menu_item(ui, Some(Icon::Terminal), &p.name, "").clicked() {
                        new_profile = Some(Some(p.clone()));
                        ui.close();
                    }
                }
            });
            if icons::button(&mut right, Icon::Plus, crate::i18n::text("Neues Terminal")).clicked() {
                new_profile = Some(None);
            }

            // Erkannte Fehler eines Programms: Erklaerung und Behebung
            let local = self.is_local();
            if let Some(t) = self.terminal.tabs.get_mut(active)
                && t.program
            {
                let len = t.session.log.lock().map(|l| l.len()).unwrap_or(0);
                let due = t.session.exited() || t.analyzed.1.is_none_or(|at| at.elapsed() >= Duration::from_millis(1500));
                if len != t.analyzed.0 && due {
                    let log = t.session.log.lock().map(|l| l.clone()).unwrap_or_default();
                    t.fixes = crate::dev::autofix::analyze_output(&log, Some(t.session.cwd()), None, local);
                    t.analyzed = (len, Some(Instant::now()));
                    t.applied.clear();
                } else if len != t.analyzed.0 {
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
                if !t.fixes.is_empty() {
                    let (apply, rerun) = fix_bar(ui, &t.fixes, &t.applied, t.session.exited());
                    for i in apply {
                        if let Some(f) = t.fixes[i].fix.clone() {
                            if !matches!(f, crate::dev::autofix::Fix::Schema { .. } | crate::dev::autofix::Fix::DeklarierteStruktur { .. }) {
                                t.applied.push(i);
                            }
                            fixes.push(f);
                        }
                    }
                    if rerun {
                        t.session.restart(&ctx);
                        t.fixes.clear();
                        t.analyzed = (0, None);
                    }
                }
            }
            // Inhalt
            if let Some(t) = self.terminal.tabs.get_mut(active) {
                let focus = std::mem::take(&mut self.terminal.focus);
                if let Some((file, line, text)) = terminal::show(ui, egui::Id::new(("terminal", t.uid)), &mut t.session, focus) {
                    link = Some((file, line, text, t.session.cwd().to_path_buf()));
                }
            }
          });
        });
        if let Some(i) = close
            && i < self.terminal.tabs.len()
        {
            self.terminal.tabs.remove(i);
            self.terminal.active = self.terminal.active.min(self.terminal.tabs.len().saturating_sub(1));
            self.terminal.focus = true;
            if self.terminal.tabs.is_empty() {
                self.terminal.open = false;
            }
        }
        if hide {
            self.terminal.open = false;
        }
        if let Some(p) = new_profile {
            self.new_terminal(&ctx, p);
        }
        for f in fixes {
            self.apply_fix(f);
        }
        if let Some((file, line, text, cwd)) = link {
            match crate::dev::autofix::resolve(self.project.as_deref(), Some(&cwd), &text, &file) {
                Some(path) => self.handle_actions(vec![Action::OpenFileAt { path, line }]),
                None => self.toast(crate::tr_format!("„{file}“ wurde im Projekt nicht gefunden.", "“{file}” was not found in the project.")),
            }
        }
    }

    /// Erkannten Fehler beheben (aus dem Terminal oder der Ausgabe eines Editors)
    pub(super) fn apply_fix(&mut self, fix: crate::dev::autofix::Fix) {
        use crate::dev::autofix::Fix;
        match fix {
            Fix::Edit(edit) => {
                let n = self.write_edit(&edit);
                self.toast(crate::tr_format!("Behoben in {n} Datei(en).", "Fixed in {n} file(s)."));
            }
            Fix::CreateDatabase(name) => match &self.db {
                Some(dbc) => match dbc.exec(&format!("CREATE DATABASE IF NOT EXISTS {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", crate::db::q(&name)), ()) {
                    Ok(_) => {
                        self.toast(crate::tr_format!("Datenbank {name} erstellt.", "Database {name} created."));
                        self.refresh_all();
                        self.current_db = name;
                    }
                    Err(e) => self.error(e),
                },
                None => self.error(crate::i18n::text("Nicht verbunden").to_string()),
            },
            Fix::NewTable(db) => self.handle_actions(vec![Action::NewTable(db)]),
            Fix::DeklarierteStruktur { datenbank, plaene, quelle } => {
                let db = datenbank.unwrap_or_else(|| self.current_db.clone());
                if db.is_empty() { self.error(crate::i18n::tr("Bitte zuerst die Zieldatenbank wählen.", "Choose the target database first.")); return; }
                let Some(dbc) = &self.db else { return };
                let schema = match dbc.schema(&db) { Ok(s) => s, Err(e) => { self.error(e); return; } };
                let mut existing = Vec::new();
                for plan in &plaene {
                    if let Some(table) = schema.tables.iter().find(|t| t.name == plan.tabelle) {
                        let result = dbc.indexes(&db, &plan.tabelle).and_then(|indexes| {
                            dbc.create_statement(&db, &plan.tabelle).map(|ddl| crate::sqledit::bestehender_plan(&schema, table, &indexes, &ddl))
                        });
                        match result { Ok(p) => existing.push(p), Err(e) => { self.error(e); return; } }
                    }
                }
                let Some(sql) = crate::sqledit::plan_sql(&plaene, &schema, &existing) else {
                    self.toast(crate::i18n::tr("Keine eindeutige fehlende Struktur gefunden.", "No unambiguous missing structure found.")); return;
                };
                // Vollstaendig deklarierte neue Tabellen brauchen keine zweite Freigabe.
                if datenbank_festgelegt(&plaene, &schema) {
                    let result = crate::sqledit::pruefe_daten(dbc, &db, &plaene).and_then(|_| dbc.exec_schema(&db, &sql));
                    match result {
                        Ok(_) => {
                            for terminal in &mut self.terminal.tabs {
                                for (i, vorschlag) in terminal.fixes.iter().enumerate() {
                                    if matches!(&vorschlag.fix, Some(Fix::DeklarierteStruktur { plaene: p, .. }) if *p == plaene) { terminal.applied.push(i); }
                                }
                            }
                            self.toast(crate::tr_format!("Tabellen samt Struktur in {db} erstellt.", "Created tables with structure in {db}."));
                            self.handle_actions(vec![Action::SchemaChanged(db)]);
                        }
                        Err(e) => {
                            self.handle_actions(vec![Action::SchemaChanged(db)]);
                            self.error(crate::tr_format!("Die Struktur konnte nicht vollständig erstellt werden. Bereits ausgeführte DDL bleibt bestehen.\n{e}", "The structure could not be created completely. DDL already executed remains applied.\n{e}"));
                        }
                    }
                    return;
                }
                let host = &dbc.info.host; let port = dbc.info.port;
                self.dialogs.push(super::dialogs::Dialog::ConfirmStructure {
                    text: crate::tr_format!("Struktur aus {quelle} auf {host}:{port} in {db} ergänzen? SQL und JDBC-Ziel prüfen. Benötigte Tabellen, Spalten und deklarierte Schlüssel/Beziehungen werden ergänzt. Bestehende Spalten und Daten werden nicht ersetzt.", "Add structure from {quelle} on {host}:{port} in {db}? Review the SQL and JDBC target. Required tables, columns and declared keys/relationships will be added. Existing columns and data will not be replaced."),
                    db: Some(db), sql,
                });
            }
            Fix::Schema { db, table, columns } => {
                let db = db.unwrap_or_else(|| self.current_db.clone());
                if db.is_empty() {
                    self.error(crate::i18n::tr("Bitte zuerst die Zieldatenbank wählen.", "Choose the target database first."));
                    return;
                }
                let Some(dbc) = &self.db else { return };
                let schema = match dbc.schema(&db) {
                    Ok(s) => s,
                    Err(e) => { self.error(e); return; }
                };
                let Some(sql) = crate::dev::autofix::struktur_sql(&schema, &table, &columns) else {
                    self.toast(crate::i18n::tr("Die vorgeschlagene Struktur ist bereits vorhanden.", "The proposed structure already exists."));
                    return;
                };
                let host = &dbc.info.host;
                let port = dbc.info.port;
                self.dialogs.push(super::dialogs::Dialog::ConfirmStructure {
                    text: crate::tr_format!("Struktur auf {host}:{port} in {db} erstellen? Bitte mit dem JDBC-Ziel vergleichen. Typen sind aus dem Code abgeleitet; Schlüssel, Standardwerte und Beziehungen müssen geprüft werden. Vorhandene Spalten werden nicht verändert.", "Create structure on {host}:{port} in {db}? Compare with the JDBC target. Types are inferred from code; keys, defaults and relationships need review. Existing columns will not be changed."),
                    db: Some(db), sql,
                });
            }
            Fix::StartServer => {
                if let Some(ctx) = CTX.lock().unwrap().clone() {
                    self.start_server(&ctx);
                }
            }
            Fix::ConnectionSettings => self.handle_actions(vec![Action::OpenSettings(Some("verbindung".into()))]),
            Fix::Install(tool) => {
                if let Some(ctx) = CTX.lock().unwrap().clone() {
                    crate::dev::install::install(&[tool], &ctx);
                }
            }
        }
    }
}

fn datenbank_festgelegt(plaene: &[crate::sqledit::TabellenPlan], schema: &crate::db::Schema) -> bool {
    !plaene.is_empty() && plaene.iter().all(|p| p.datenbank.as_deref() == Some(&schema.name)
        && !schema.tables.iter().any(|t| t.name.eq_ignore_ascii_case(&p.tabelle))
        && p.teile.iter().any(|t| t.name.is_some()))
}

/// Leiste mit erkannten Fehlern ueber dem Terminal. Liefert die angeklickten Behebungen und "Erneut ausfuehren".
fn fix_bar(ui: &mut egui::Ui, fixes: &[crate::dev::autofix::Suggestion], applied: &[usize], exited: bool) -> (Vec<usize>, bool) {
    let pal = style::pal();
    let mut apply = Vec::new();
    let mut rerun = false;
    let open: Vec<usize> = (0..fixes.len()).filter(|i| fixes[*i].fix.is_some() && !applied.contains(i)).collect();
    let tone = if open.is_empty() && !applied.is_empty() { pal.ok_text } else if fixes.iter().any(|s| s.fix.is_some()) { pal.error_text } else { pal.warn_text };
    egui::Frame::new().fill(tone.gamma_multiply(0.07)).stroke(Stroke::new(1.0, tone.gamma_multiply(0.35))).corner_radius(style::RADIUS).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        for (i, s) in fixes.iter().enumerate().take(5) {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                let done = applied.contains(&i);
                let (icon, color) = if done { (Icon::Check, pal.ok_text) } else if s.fix.is_some() { (Icon::Alert, pal.error_text) } else { (Icon::Info, pal.text_weak) };
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), r, icon, color);
                ui.add(egui::Label::new(egui::RichText::new(&s.problem).color(if done { pal.text_weak } else { pal.text })).truncate());
                if s.fix.is_some() && !done && icons::text_button(ui, Icon::Wizard, &s.label).clicked() {
                    apply.push(i);
                }
            });
        }
        if open.len() > 1 || exited {
            ui.horizontal(|ui| {
                if open.len() > 1 && icons::primary_button(ui, Icon::Sparkles, crate::i18n::tr("Alle beheben", "Fix all")).clicked() {
                    apply.extend(open.iter().copied());
                }
                if exited && icons::run_button(ui, crate::i18n::tr("Erneut ausführen", "Run again")).clicked() {
                    rerun = true;
                }
            });
        }
    });
    ui.add_space(6.0);
    (apply, rerun)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "braucht isolierten MariaDB-Testserver auf EASYMYSQL_AUTOFIX_TEST_PORT"]
    fn e2e_java_tabellenfix_mit_vollstaendiger_struktur() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let dbc = crate::db::Db::connect(&crate::db::ConnInfo { port, ..Default::default() }).unwrap();
        let db = format!("ein_klick_{}", std::process::id());
        let root = std::env::temp_dir().join(&db); std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Main.java"), r#"
            static final String KUNDE = "CREATE TABLE kunde(id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(80) NOT NULL UNIQUE)";
            static final String AUFTRAG = "CREATE TABLE auftrag(id BIGINT AUTO_INCREMENT PRIMARY KEY, kunde_id BIGINT NOT NULL, status VARCHAR(20) NOT NULL DEFAULT 'offen', FOREIGN KEY(kunde_id) REFERENCES kunde(id))";
            void laden() { c.prepareStatement("SELECT * FROM auftrag"); }
        "#).unwrap();
        dbc.exec(&format!("CREATE DATABASE {}", crate::db::q(&db)), ()).unwrap();
        let ergebnis = (|| {
            let fixes = crate::dev::autofix::analyze_output(&format!("Table '{db}.auftrag' doesn't exist"), Some(&root), None, true);
            let Some(crate::dev::autofix::Fix::DeklarierteStruktur { plaene, .. }) = &fixes[0].fix else { panic!("{fixes:?}") };
            let schema = dbc.schema(&db)?;
            assert!(datenbank_festgelegt(plaene, &schema));
            let sql = crate::sqledit::plan_sql(plaene, &schema, &[]).unwrap();
            crate::sqledit::pruefe_daten(&dbc, &db, plaene)?; dbc.exec_schema(&db, &sql)?;
            dbc.exec_in(&db, "INSERT INTO kunde(name) VALUES ('Ada'); INSERT INTO auftrag(kunde_id) VALUES(1)")?;
            let rows = dbc.query(&format!("SELECT id, status FROM {}.auftrag", crate::db::q(&db)))?;
            assert_eq!(rows.rows[0], vec![Some("1".into()), Some("offen".into())]);
            assert!(dbc.exec_in(&db, "INSERT INTO kunde(name) VALUES ('Ada')").is_err());
            assert!(dbc.exec_in(&db, "INSERT INTO kunde(name) VALUES (NULL)").is_err());
            assert!(dbc.exec_in(&db, "INSERT INTO auftrag(kunde_id) VALUES(999)").is_err());
            assert!(!datenbank_festgelegt(plaene, &dbc.schema(&db)?));
            Ok::<_, String>(())
        })();
        dbc.exec(&format!("DROP DATABASE {}", crate::db::q(&db)), ()).unwrap(); std::fs::remove_dir_all(root).unwrap();
        ergebnis.unwrap();
    }
    #[test]
    fn ein_klick_nur_fuer_neue_vollstaendig_deklarierte_tabellen() {
        let mut schema = crate::db::Schema { name: "shop".into(), ..Default::default() };
        let mut plaene = crate::sqledit::migrationen("CREATE TABLE kunde(id BIGINT PRIMARY KEY)").unwrap();
        assert!(!datenbank_festgelegt(&plaene, &schema));
        plaene[0].datenbank = Some("shop".into());
        assert!(datenbank_festgelegt(&plaene, &schema));
        schema.tables.push(crate::db::TableInfo { name: "Kunde".into(), ..Default::default() });
        assert!(!datenbank_festgelegt(&plaene, &schema));
    }
}
