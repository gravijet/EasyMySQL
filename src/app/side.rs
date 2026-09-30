// Seitenleisten: Explorer (Projektordner), Suchen in Dateien, Datenbanken, Verlauf.

use super::*;
use crate::db::q;
use crate::icons::{self, Icon};
use crate::workspace::{self, Node};
use eframe::egui::{Color32, RichText, Sense, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SideView {
    Explorer,
    Search,
    Databases,
    History,
}

impl SideView {
    pub fn name(&self) -> &'static str {
        match self {
            SideView::Explorer => "explorer",
            SideView::Search => "suche",
            SideView::Databases => "datenbanken",
            SideView::History => "verlauf",
        }
    }
    pub fn from_name(s: &str) -> SideView {
        match s {
            "datenbanken" => SideView::Databases,
            "suche" => SideView::Search,
            "verlauf" => SideView::History,
            _ => SideView::Explorer,
        }
    }
}

pub enum ExplorerDlg {
    NewFile { dir: PathBuf, name: String },
    NewFolder { dir: PathBuf, name: String },
    Rename { path: PathBuf, name: String },
    Delete { path: PathBuf },
    NewProject { name: String },
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub case: bool,
    pub focus: bool,
    searched: Option<(String, bool)>,
    results: Hits,
    truncated: bool,
}

/// Dateien, die durchsucht bzw. schnell geoeffnet werden koennen
pub fn text_files(n: &Node, out: &mut Vec<PathBuf>) {
    for c in &n.children {
        if c.is_dir {
            text_files(c, out);
        } else {
            let ext = c.name.rsplit('.').next().unwrap_or("").to_lowercase();
            if matches!(ext.as_str(), "sql" | "txt" | "md" | "csv" | "json") {
                out.push(c.path.clone());
            }
        }
    }
}

/// Treffer je Datei: (Datei, [(Zeile, Text)])
type Hits = Vec<(PathBuf, Vec<(usize, String)>)>;

fn search_files(files: &[PathBuf], query: &str, case: bool) -> (Hits, bool) {
    let q = if case { query.to_string() } else { query.to_lowercase() };
    let mut out = Vec::new();
    let mut total = 0;
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let mut hits = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let l = if case { line.to_string() } else { line.to_lowercase() };
            if l.contains(&q) {
                hits.push((i, line.trim().chars().take(160).collect()));
                total += 1;
                if total >= 2000 {
                    out.push((f.clone(), hits));
                    return (out, true);
                }
            }
        }
        if !hits.is_empty() {
            out.push((f.clone(), hits));
        }
    }
    (out, false)
}

fn section_header(ui: &mut egui::Ui, title: &str, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(title).small().strong().color(style::pal().text_weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 1.0;
            add_buttons(ui);
        });
    });
}

/// Freier Vorschlag fuer einen neuen Dateinamen
pub fn new_name(dir: &Path) -> String {
    workspace::unique_file(dir, "Neue Abfrage", "sql").file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl EasyApp {
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.add_space(6.0);
        match self.side_view {
            SideView::Explorer => self.explorer(ui),
            SideView::Search => self.search_side(ui, actions),
            SideView::Databases => self.databases_side(ui, actions),
            SideView::History => self.history_side(ui, actions),
        }
    }

    pub(super) fn rescan_tree(&mut self) {
        let Some(p) = &self.project else {
            self.tree = None;
            return;
        };
        if self.tree_scan.is_none_or(|t| t.elapsed().as_secs() >= 2) {
            self.tree = Some(workspace::scan(p));
            self.tree_scan = Some(Instant::now());
        }
    }

    // ------------------------------------------------------------------
    // Explorer

    fn explorer(&mut self, ui: &mut egui::Ui) {
        self.rescan_tree();
        let Some(project) = self.project.clone() else {
            section_header(ui, "EXPLORER", |_| {});
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let w = ui.available_width();
                if ui.add_sized([w, 26.0], egui::Button::new("Ordner öffnen")).clicked() {
                    self.pick_project();
                }
                if ui.add_sized([w, 26.0], egui::Button::new("Neues Projekt")).clicked() {
                    self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                }
                if !self.settings.recent.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Zuletzt geöffnet").small().color(style::pal().text_weak));
                    for p in self.settings.recent.clone() {
                        if !p.is_dir() {
                            continue;
                        }
                        if ui.link(workspace::dir_name(&p)).on_hover_text(p.display().to_string()).clicked() {
                            self.switch_project(Some(p));
                        }
                    }
                }
            });
            return;
        };
        let mut new_file = false;
        let mut new_folder = false;
        let mut refresh = false;
        let mut collapse = false;
        let mut menu_resp = None;
        section_header(ui, &workspace::dir_name(&project).to_uppercase(), |ui| {
            menu_resp = Some(icons::button(ui, Icon::More, "Projekt"));
            if icons::button(ui, Icon::Collapse, "Alle Ordner zuklappen").clicked() {
                collapse = true;
            }
            if icons::button(ui, Icon::Refresh, "Aktualisieren").clicked() {
                refresh = true;
            }
            if icons::button(ui, Icon::NewFolder, "Neuer Ordner").clicked() {
                new_folder = true;
            }
            if icons::button(ui, Icon::NewFile, "Neue Datei").clicked() {
                new_file = true;
            }
        });
        if let Some(r) = &menu_resp {
            egui::Popup::menu(r).show(|ui| {
                if ui.button("Ordner öffnen …").clicked() {
                    ui.close();
                    self.pick_project();
                }
                if ui.button("Neues Projekt …").clicked() {
                    self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                    ui.close();
                }
                ui.menu_button("Zuletzt geöffnet", |ui| {
                    for p in self.settings.recent.clone() {
                        if p != project && ui.button(workspace::dir_name(&p)).on_hover_text(p.display().to_string()).clicked() {
                            self.switch_project(Some(p));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Im Datei-Explorer zeigen").clicked() {
                    open_path(&project);
                    ui.close();
                }
                if ui.button("Ordner schließen").clicked() {
                    self.switch_project(None);
                    ui.close();
                }
            });
        }
        if refresh {
            self.tree_scan = None;
        }
        if new_file {
            self.explorer_dlg = Some(ExplorerDlg::NewFile { name: new_name(&project), dir: project.clone() });
        }
        if new_folder {
            self.explorer_dlg = Some(ExplorerDlg::NewFolder { dir: project.clone(), name: String::new() });
        }
        let active_file = self.tabs.get(self.active).and_then(|t| t.file().map(|p| p.to_path_buf()));
        let Some(tree) = self.tree.clone() else { return };
        if collapse {
            collapse_all(ui.ctx(), &tree);
        }
        let mut open: Option<PathBuf> = None;
        let mut dlg: Option<ExplorerDlg> = None;
        egui::ScrollArea::vertical().id_salt("explorer").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for n in &tree.children {
                draw_node(ui, n, 0, active_file.as_deref(), &mut open, &mut dlg);
            }
            // Rechtsklick auf freie Flaeche
            let rest = ui.allocate_response(ui.available_size().max(vec2(10.0, 40.0)), Sense::click());
            if rest.double_clicked() {
                dlg = Some(ExplorerDlg::NewFile { dir: tree.path.clone(), name: new_name(&tree.path) });
            }
            rest.context_menu(|ui| {
                if ui.button("Neue Datei …").clicked() {
                    dlg = Some(ExplorerDlg::NewFile { dir: tree.path.clone(), name: new_name(&tree.path) });
                    ui.close();
                }
                if ui.button("Neuer Ordner …").clicked() {
                    dlg = Some(ExplorerDlg::NewFolder { dir: tree.path.clone(), name: String::new() });
                    ui.close();
                }
                if ui.button("Im Datei-Explorer zeigen").clicked() {
                    open_path(&tree.path);
                    ui.close();
                }
            });
        });
        if let Some(p) = open {
            self.open_file(&p);
        }
        if dlg.is_some() {
            self.explorer_dlg = dlg;
        }
    }

    // ------------------------------------------------------------------
    // Suchen in Dateien

    fn search_side(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        section_header(ui, "SUCHEN", |_| {});
        let Some(tree) = (self.rescan_tree(), self.tree.clone()).1 else {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                if ui.button("Ordner öffnen").clicked() {
                    self.pick_project();
                }
            });
            return;
        };
        let mut run = false;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            let w = ui.available_width();
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.search.query).hint_text("Suchen").desired_width((w - 52.0).max(60.0)));
                if self.search.focus {
                    r.request_focus();
                    self.search.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = true;
                }
                if ui.toggle_value(&mut self.search.case, "Aa").on_hover_text("Groß-/Kleinschreibung beachten").changed() {
                    run = true;
                }
            });
        });
        // Suche beim Tippen (kurze Texte erst ab 2 Zeichen)
        let key = (self.search.query.clone(), self.search.case);
        if run || (self.search.searched.as_ref() != Some(&key) && self.search.query.chars().count() >= 2) {
            let mut files = Vec::new();
            text_files(&tree, &mut files);
            let (res, trunc) = search_files(&files, &self.search.query, self.search.case);
            self.search.results = res;
            self.search.truncated = trunc;
            self.search.searched = Some(key);
        }
        if self.search.query.is_empty() {
            self.search.results.clear();
            self.search.searched = None;
            return;
        }
        let pal = style::pal();
        let n: usize = self.search.results.iter().map(|(_, h)| h.len()).sum();
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let t = if self.search.truncated { format!("{n}+ Treffer") } else { format!("{n} Treffer in {} Dateien", self.search.results.len()) };
            ui.label(RichText::new(t).small().color(pal.text_weak));
        });
        let project = self.project.clone().unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("searchres").auto_shrink([false, false]).show(ui, |ui| {
            for (f, hits) in &self.search.results {
                let rel = f.strip_prefix(&project).unwrap_or(f).to_string_lossy().replace('\\', "/");
                egui::CollapsingHeader::new(RichText::new(format!("{rel}  ({})", hits.len())).strong()).id_salt(("sr", f)).default_open(true).show(ui, |ui| {
                    for (line, text) in hits {
                        let r = ui.add(egui::Label::new(RichText::new(format!("{:>4}  {text}", line + 1)).monospace().small()).truncate().sense(Sense::click()));
                        if r.hovered() {
                            ui.painter().rect_filled(r.rect, 0.0, pal.hover.gamma_multiply(0.5));
                        }
                        if r.clicked() {
                            actions.push(Action::OpenFileAt { path: f.clone(), line: *line });
                        }
                    }
                });
            }
        });
    }

    // ------------------------------------------------------------------
    // Verlauf

    fn history_side(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut clear = false;
        section_header(ui, "VERLAUF", |ui| {
            if icons::button(ui, Icon::Trash, "Verlauf leeren").clicked() {
                clear = true;
            }
        });
        if clear {
            crate::qhistory::clear();
            self.history.clear();
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.history_filter).hint_text("Filtern").desired_width(f32::INFINITY));
        });
        let pal = style::pal();
        let f = self.history_filter.to_lowercase();
        let items: Vec<&crate::qhistory::Entry> = self
            .history
            .iter()
            .filter(|e| f.is_empty() || e.sql.to_lowercase().contains(&f) || e.db.to_lowercase().contains(&f))
            .collect();
        let row_h = 38.0;
        egui::ScrollArea::vertical().id_salt("history").auto_shrink([false, false]).show_rows(ui, row_h, items.len(), |ui, range| {
            for e in &items[range] {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, pal.hover);
                }
                let first: String = e.sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
                let color = if e.error { pal.error_text } else { pal.text };
                let p = ui.painter_at(r);
                p.text(pos2(r.left() + 10.0, r.top() + 11.0), egui::Align2::LEFT_CENTER, first, egui::FontId::monospace(12.0), color);
                let meta = format!("{}  ·  {}  ·  {} ms", e.short_time(), if e.db.is_empty() { "–" } else { &e.db }, e.millis);
                p.text(pos2(r.left() + 10.0, r.top() + 27.0), egui::Align2::LEFT_CENTER, meta, egui::FontId::proportional(11.0), pal.text_weak);
                let resp = resp.on_hover_text(e.sql.chars().take(1500).collect::<String>());
                if resp.double_clicked() {
                    actions.push(Action::OpenSql { db: Some(e.db.clone()), sql: e.sql.clone() + ";\n", run: false });
                }
                resp.context_menu(|ui| {
                    if ui.button("In neuer Abfrage öffnen").clicked() {
                        actions.push(Action::OpenSql { db: Some(e.db.clone()), sql: e.sql.clone() + ";\n", run: false });
                        ui.close();
                    }
                    if ui.button("Erneut ausführen").clicked() {
                        actions.push(Action::OpenSql { db: Some(e.db.clone()), sql: e.sql.clone() + ";\n", run: true });
                        ui.close();
                    }
                    if ui.button("Kopieren").clicked() {
                        ui.ctx().copy_text(e.sql.clone());
                        ui.close();
                    }
                });
            }
        });
    }

    // ------------------------------------------------------------------
    // Datenbanken

    fn databases_side(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut new_db = false;
        let mut refresh = false;
        section_header(ui, "DATENBANKEN", |ui| {
            if icons::button(ui, Icon::Refresh, "Aktualisieren").clicked() {
                refresh = true;
            }
            if icons::button(ui, Icon::Plus, "Neue Datenbank").clicked() {
                new_db = true;
            }
        });
        if refresh {
            actions.push(Action::RefreshAll);
        }
        if new_db {
            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
            self.db_tree(ui, actions);
        });
    }

    fn db_tree(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(dbc) = &self.db else {
            ui.label(RichText::new("Nicht verbunden").color(style::pal().text_weak));
            if ui.button("Verbindung …").clicked() {
                actions.push(Action::OpenSettings(Some("verbindung".into())));
            }
            return;
        };
        let show_sys = self.settings.show_system_dbs;
        let mut dialogs = Vec::new();
        let pal = style::pal();
        egui::ScrollArea::both().id_salt("tree").auto_shrink([false, false]).show(ui, |ui| {
            for d in &self.databases {
                let sys = db::SYSTEM_DATABASES.contains(&d.as_str());
                if sys && !show_sys {
                    continue;
                }
                let title = if *d == self.current_db { RichText::new(d).strong() } else { RichText::new(d) };
                let resp = egui::CollapsingHeader::new(title).id_salt(("db", d)).show(ui, |ui| match self.schemas.get(Some(dbc), d) {
                    Some(schema) => {
                        for t in &schema.tables {
                            let label = if t.is_view { RichText::new(&t.name).italics() } else { RichText::new(&t.name) };
                            let h = egui::CollapsingHeader::new(label).id_salt(("t", d, &t.name)).show(ui, |ui| {
                                for c in &t.columns {
                                    ui.horizontal(|ui| {
                                        let (tag, color) = if c.is_pk() {
                                            ("PK", Color32::from_rgb(0xC0, 0x90, 0x10))
                                        } else if schema.is_fk_column(&t.name, &c.name) {
                                            ("FK", Color32::from_rgb(0x40, 0x80, 0xD0))
                                        } else {
                                            ("", pal.text_weak)
                                        };
                                        ui.add_sized([18.0, 14.0], egui::Label::new(RichText::new(tag).small().color(color)));
                                        ui.label(RichText::new(&c.name).small());
                                        ui.label(RichText::new(&c.col_type).small().color(pal.text_weak));
                                    });
                                }
                            });
                            let hr = h.header_response;
                            if hr.clicked() {
                                self.current_db = d.clone();
                            }
                            if hr.double_clicked() {
                                actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                            }
                            hr.context_menu(|ui| {
                                if ui.button("Daten").clicked() {
                                    actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                                    ui.close();
                                }
                                if ui.button("Struktur").clicked() {
                                    actions.push(Action::OpenStructure { db: d.clone(), table: t.name.clone() });
                                    ui.close();
                                }
                                if ui.button("SELECT").clicked() {
                                    actions.push(Action::OpenSql { db: Some(d.clone()), sql: format!("SELECT * FROM {} LIMIT 100;", q(&t.name)), run: true });
                                    ui.close();
                                }
                                if ui.button("Name kopieren").clicked() {
                                    ui.ctx().copy_text(t.name.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if !t.is_view && ui.button("Leeren …").clicked() {
                                    actions.push(Action::Confirm {
                                        text: format!("Alle Zeilen der Tabelle „{}“ löschen?", t.name),
                                        db: Some(d.clone()),
                                        sql: format!("DELETE FROM {}", q(&t.name)),
                                    });
                                    ui.close();
                                }
                                if ui.button("Löschen …").clicked() {
                                    actions.push(Action::Confirm {
                                        text: format!("„{}“ mit allen Daten löschen?", t.name),
                                        db: Some(d.clone()),
                                        sql: format!("DROP {} {}", if t.is_view { "VIEW" } else { "TABLE" }, q(&t.name)),
                                    });
                                    ui.close();
                                }
                            });
                        }
                    }
                    None => {
                        ui.label(RichText::new("kein Zugriff").color(pal.error_text));
                    }
                });
                let hr = resp.header_response;
                if hr.clicked() {
                    self.current_db = d.clone();
                }
                hr.context_menu(|ui| {
                    if ui.button("Neue Abfrage").clicked() {
                        actions.push(Action::OpenSql { db: Some(d.clone()), sql: String::new(), run: false });
                        ui.close();
                    }
                    if ui.button("Neue Tabelle …").clicked() {
                        actions.push(Action::NewTable(d.clone()));
                        ui.close();
                    }
                    if ui.button("ER-Diagramm").clicked() {
                        actions.push(Action::OpenEr(d.clone()));
                        ui.close();
                    }
                    if ui.button("Abfrage-Assistent").clicked() {
                        actions.push(Action::OpenBuilder(Some(d.clone())));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exportieren …").clicked() {
                        dialogs.push(Dialog::Export { db: d.clone(), with_data: true });
                        ui.close();
                    }
                    if !sys && ui.button("Löschen …").clicked() {
                        actions.push(Action::Confirm {
                            text: format!("Datenbank „{d}“ mit allen Tabellen und Daten löschen?"),
                            db: None,
                            sql: format!("DROP DATABASE {}", q(d)),
                        });
                        ui.close();
                    }
                });
            }
        });
        self.dialogs.extend(dialogs);
    }

    // ------------------------------------------------------------------
    // Dialoge des Explorers

    pub(super) fn explorer_dialogs(&mut self, ctx: &egui::Context) {
        let Some(dlg) = self.explorer_dlg.as_mut() else { return };
        let mut ok = false;
        let mut cancel = false;
        let title = match dlg {
            ExplorerDlg::NewFile { .. } => "Neue Datei",
            ExplorerDlg::NewFolder { .. } => "Neuer Ordner",
            ExplorerDlg::Rename { .. } => "Umbenennen",
            ExplorerDlg::Delete { .. } => "Löschen",
            ExplorerDlg::NewProject { .. } => "Neues Projekt",
        };
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(14));
        egui::Modal::new(egui::Id::new("explorer-dlg")).frame(frame).show(ctx, |ui| {
            ui.label(RichText::new(title).strong());
            ui.add_space(6.0);
            match dlg {
                ExplorerDlg::NewFile { name, .. } | ExplorerDlg::NewFolder { name, .. } | ExplorerDlg::Rename { name, .. } | ExplorerDlg::NewProject { name } => {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(300.0));
                    r.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    if matches!(dlg, ExplorerDlg::NewProject { .. }) {
                        let root = workspace::storage_root();
                        ui.label(RichText::new(root.display().to_string()).small().color(style::pal().text_weak));
                    }
                }
                ExplorerDlg::Delete { path } => {
                    ui.label(format!("„{}“ in den Papierkorb des Projekts verschieben?", workspace::dir_name(path)));
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    ok = true;
                }
                if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if cancel {
            self.explorer_dlg = None;
            return;
        }
        if !ok {
            return;
        }
        let dlg = self.explorer_dlg.take().unwrap();
        let res: Result<Option<PathBuf>, String> = match dlg {
            ExplorerDlg::NewFile { dir, name } => workspace::create_file(&dir, &name, "").map(Some),
            ExplorerDlg::NewFolder { dir, name } => workspace::create_folder(&dir, &name).map(|_| None),
            ExplorerDlg::NewProject { name } => {
                let root = workspace::storage_root();
                if root.join(name.trim()).exists() {
                    Err(format!("„{}“ gibt es schon.", name.trim()))
                } else {
                    workspace::create_folder(&root, &name).map(|p| {
                        self.switch_project(Some(p));
                        None
                    })
                }
            }
            ExplorerDlg::Rename { path, name } => workspace::rename(&path, &name).map(|new| {
                for t in self.tabs.iter_mut() {
                    t.file_renamed(&path, &new);
                }
                None
            }),
            ExplorerDlg::Delete { path } => {
                // Offene Registerkarten dieser Datei(en) schliessen
                let mut i = 0;
                while i < self.tabs.len() {
                    if self.tabs[i].file().is_some_and(|f| f.starts_with(&path)) {
                        self.tabs.remove(i);
                    } else {
                        i += 1;
                    }
                }
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
                match &self.project {
                    Some(p) => workspace::trash(p, &path).map(|_| None),
                    None => Err("Kein Projekt geöffnet.".into()),
                }
            }
        };
        self.tree_scan = None;
        match res {
            Ok(Some(p)) => self.open_file(&p),
            Ok(None) => {}
            Err(e) => self.error(e),
        }
    }
}

fn collapse_all(ctx: &egui::Context, n: &Node) {
    for c in n.children.iter().filter(|c| c.is_dir) {
        let id = egui::Id::new(("dir", &c.path));
        let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, false);
        st.set_open(false);
        st.store(ctx);
        collapse_all(ctx, c);
    }
}

/// Kleines Dateisymbol vor Eintraegen im Explorer
fn file_icon(p: &egui::Painter, r: Rect, name: &str) {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let color = match ext.as_str() {
        "sql" => Color32::from_rgb(0xE3, 0x8C, 0x3B),
        "md" | "txt" => Color32::from_rgb(0x51, 0x9A, 0xBA),
        "csv" | "json" => Color32::from_rgb(0x89, 0xD1, 0x85),
        _ => style::pal().text_weak,
    };
    let body = Rect::from_center_size(r.center(), vec2(9.0, 12.0));
    p.rect_stroke(body, 1.0, egui::Stroke::new(1.2, color), egui::StrokeKind::Middle);
    for k in 0..3 {
        let y = body.top() + 4.0 + k as f32 * 2.5;
        p.line_segment([pos2(body.left() + 2.0, y), pos2(body.right() - 2.0, y)], egui::Stroke::new(1.0, color));
    }
}

/// Eintrag im Dateibaum zeichnen (rekursiv).
fn draw_node(ui: &mut egui::Ui, n: &Node, depth: usize, active: Option<&Path>, open: &mut Option<PathBuf>, dlg: &mut Option<ExplorerDlg>) {
    let pal = style::pal();
    let indent = 8.0 + depth as f32 * 14.0;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    if n.is_dir {
        let id = egui::Id::new(("dir", &n.path));
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, pal.hover);
        }
        let chev = Rect::from_center_size(pos2(r.left() + indent + 6.0, r.center().y), vec2(14.0, 14.0));
        icons::paint(ui.painter(), chev, if state.is_open() { Icon::ChevronDown } else { Icon::ChevronRight }, pal.text_weak);
        ui.painter().text(pos2(r.left() + indent + 18.0, r.center().y), egui::Align2::LEFT_CENTER, &n.name, egui::FontId::proportional(13.0), pal.text);
        if resp.clicked() {
            state.toggle(ui);
        }
        state.store(ui.ctx());
        dir_menu(&resp, n, dlg);
        if state.is_open() {
            for c in &n.children {
                draw_node(ui, c, depth + 1, active, open, dlg);
            }
        }
    } else {
        let sel = active.is_some_and(|a| a == n.path);
        if sel {
            ui.painter().rect_filled(r, 0.0, if pal.dark { Color32::from_rgb(0x37, 0x37, 0x3D) } else { Color32::from_rgb(0xE4, 0xE6, 0xF1) });
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, pal.hover);
        }
        file_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + indent + 24.0, r.center().y), vec2(14.0, 16.0)), &n.name);
        ui.painter().text(pos2(r.left() + indent + 34.0, r.center().y), egui::Align2::LEFT_CENTER, &n.name, egui::FontId::proportional(13.0), pal.text);
        if resp.clicked() {
            *open = Some(n.path.clone());
        }
        resp.context_menu(|ui| {
            if ui.button("Öffnen").clicked() {
                *open = Some(n.path.clone());
                ui.close();
            }
            if ui.button("Umbenennen …").clicked() {
                *dlg = Some(ExplorerDlg::Rename { path: n.path.clone(), name: n.name.clone() });
                ui.close();
            }
            if ui.button("Löschen").clicked() {
                *dlg = Some(ExplorerDlg::Delete { path: n.path.clone() });
                ui.close();
            }
            ui.separator();
            if ui.button("Pfad kopieren").clicked() {
                ui.ctx().copy_text(n.path.display().to_string());
                ui.close();
            }
            if ui.button("Im Datei-Explorer zeigen").clicked() {
                if let Some(d) = n.path.parent() {
                    open_path(d);
                }
                ui.close();
            }
        });
    }
}

fn dir_menu(resp: &egui::Response, n: &Node, dlg: &mut Option<ExplorerDlg>) {
    resp.context_menu(|ui| {
        if ui.button("Neue Datei …").clicked() {
            *dlg = Some(ExplorerDlg::NewFile { dir: n.path.clone(), name: new_name(&n.path) });
            ui.close();
        }
        if ui.button("Neuer Ordner …").clicked() {
            *dlg = Some(ExplorerDlg::NewFolder { dir: n.path.clone(), name: String::new() });
            ui.close();
        }
        ui.separator();
        if ui.button("Umbenennen …").clicked() {
            *dlg = Some(ExplorerDlg::Rename { path: n.path.clone(), name: n.name.clone() });
            ui.close();
        }
        if ui.button("Löschen").clicked() {
            *dlg = Some(ExplorerDlg::Delete { path: n.path.clone() });
            ui.close();
        }
        if ui.button("Im Datei-Explorer zeigen").clicked() {
            open_path(&n.path);
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_search() {
        let dir = std::env::temp_dir().join(format!("easymysql-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.sql"), "SELECT * FROM kunde;\nselect 1;").unwrap();
        std::fs::write(dir.join("sub/b.sql"), "-- nichts").unwrap();
        std::fs::write(dir.join("bild.png"), "kunde").unwrap();
        let mut files = Vec::new();
        text_files(&workspace::scan(&dir), &mut files);
        assert_eq!(files.len(), 2);
        let (res, _) = search_files(&files, "SELECT", false);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1.len(), 2);
        let (res, _) = search_files(&files, "SELECT", true);
        assert_eq!(res[0].1, vec![(0, "SELECT * FROM kunde;".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
