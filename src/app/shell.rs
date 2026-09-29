// VS-Code-artige Oberflaeche: Aktivitaetsleiste, Seitenleiste (Explorer/Datenbanken/Sicherungen),
// Registerkarten, Statusleiste, Willkommensseite und Sitzung.

use super::*;
use crate::workspace::{self, Node, Session};
use eframe::egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SideView {
    Explorer,
    Datenbanken,
    Sicherungen,
}

impl SideView {
    fn name(&self) -> &'static str {
        match self {
            SideView::Explorer => "explorer",
            SideView::Datenbanken => "datenbanken",
            SideView::Sicherungen => "sicherungen",
        }
    }
    fn from_name(s: &str) -> SideView {
        match s {
            "datenbanken" => SideView::Datenbanken,
            "sicherungen" => SideView::Sicherungen,
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

// ---------------------------------------------------------------------------
// Symbole (selbst gezeichnet, damit sie ueberall gleich aussehen)

#[derive(Clone, Copy)]
enum Icon {
    Files,
    Database,
    Diagram,
    Wizard,
    Backup,
    Gear,
}

fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    let s = Stroke::new(1.6, color);
    let c = r.center();
    match icon {
        Icon::Files => {
            // zwei Blaetter
            let back = Rect::from_min_size(c + vec2(-6.0, -9.0), vec2(11.0, 14.0));
            p.rect_stroke(back, 1.0, s, egui::StrokeKind::Middle);
            let front = Rect::from_min_size(c + vec2(-3.0, -5.0), vec2(11.0, 14.0));
            p.rect_filled(front, 1.0, style::pal().activity_bg);
            p.rect_stroke(front, 1.0, s, egui::StrokeKind::Middle);
        }
        Icon::Database => {
            let w = 9.0;
            p.add(egui::Shape::ellipse_stroke(c + vec2(0.0, -7.0), vec2(w, 3.0), s));
            p.line_segment([c + vec2(-w, -7.0), c + vec2(-w, 7.0)], s);
            p.line_segment([c + vec2(w, -7.0), c + vec2(w, 7.0)], s);
            let arc = |y: f32| -> Vec<Pos2> {
                (0..=12).map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 12.0;
                    c + vec2(-w * a.cos(), y + 3.0 * a.sin())
                }).collect()
            };
            p.add(egui::Shape::line(arc(0.0), s));
            p.add(egui::Shape::line(arc(7.0), s));
        }
        Icon::Diagram => {
            let a = Rect::from_center_size(c + vec2(-6.0, -6.0), vec2(8.0, 6.0));
            let b = Rect::from_center_size(c + vec2(6.0, -6.0), vec2(8.0, 6.0));
            let d = Rect::from_center_size(c + vec2(0.0, 7.0), vec2(8.0, 6.0));
            for x in [a, b, d] {
                p.rect_stroke(x, 1.0, s, egui::StrokeKind::Middle);
            }
            p.line_segment([a.center_bottom(), d.left_top()], s);
            p.line_segment([b.center_bottom(), d.right_top()], s);
        }
        Icon::Wizard => {
            // Trichter (Filter)
            let pts = vec![c + vec2(-9.0, -8.0), c + vec2(9.0, -8.0), c + vec2(2.0, 0.0), c + vec2(2.0, 8.0), c + vec2(-2.0, 6.0), c + vec2(-2.0, 0.0)];
            p.add(egui::Shape::closed_line(pts, s));
        }
        Icon::Backup => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.35 + 1.6 * i as f32 / 20.0);
                    c + vec2(8.0 * a.cos(), 8.0 * a.sin())
                })
                .collect();
            let tip = pts[0];
            p.add(egui::Shape::line(pts, s));
            p.line_segment([tip, tip + vec2(-4.0, 0.0)], s);
            p.line_segment([tip, tip + vec2(1.0, -4.0)], s);
            p.line_segment([c, c + vec2(0.0, -4.5)], s);
            p.line_segment([c, c + vec2(3.5, 2.0)], s);
        }
        Icon::Gear => {
            p.circle_stroke(c, 4.0, s);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 6.0, c + d * 9.0], Stroke::new(2.4, color));
            }
        }
    }
}

/// Kleines Dateisymbol vor Eintraegen im Explorer
fn file_icon(ui: &mut egui::Ui, name: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
    let p = ui.painter();
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let color = match ext.as_str() {
        "sql" => Color32::from_rgb(0xE3, 0x8C, 0x3B),
        "md" | "txt" => Color32::from_rgb(0x51, 0x9A, 0xBA),
        "csv" => Color32::from_rgb(0x89, 0xD1, 0x85),
        _ => style::pal().text_weak,
    };
    let body = Rect::from_center_size(r.center(), vec2(9.0, 12.0));
    p.rect_stroke(body, 1.0, Stroke::new(1.2, color), egui::StrokeKind::Middle);
    for k in 0..3 {
        let y = body.top() + 4.0 + k as f32 * 2.5;
        p.line_segment([pos2(body.left() + 2.0, y), pos2(body.right() - 2.0, y)], Stroke::new(1.0, color));
    }
}

fn section_header(ui: &mut egui::Ui, title: &str, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(title).small().strong().color(style::pal().text_weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            add_buttons(ui);
        });
    });
}

fn icon_button(ui: &mut egui::Ui, text: &str, tip: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(14.0)).frame(false)).on_hover_text(tip)
}

impl EasyApp {
    // ------------------------------------------------------------------
    // Sitzung

    pub(super) fn restore_session(&mut self, session: Option<Session>) {
        let mut restored_scratch = Vec::new();
        if let Some(s) = &session {
            self.side_view = SideView::from_name(&s.side_view);
            self.sidebar = s.sidebar;
            for t in &s.tabs {
                let parts: Vec<&str> = t.split('\t').collect();
                let tab: Option<Box<dyn TabView>> = match parts.as_slice() {
                    ["sql", path] => SqlTab::open_file(Path::new(path), "").ok().map(|t| Box::new(t) as Box<dyn TabView>),
                    ["scratch", id, db] => {
                        let id: usize = id.parse().unwrap_or(0);
                        restored_scratch.push(id);
                        SqlTab::restore_scratch(id, db.to_string()).map(|t| Box::new(t) as Box<dyn TabView>)
                    }
                    ["er", db] => Some(Box::new(ErTab::new(db.to_string()))),
                    ["data", db, t] => Some(Box::new(DataTab::new(db.to_string(), t.to_string()))),
                    ["struct", db, t] => Some(Box::new(StructureTab::new(db.to_string(), t.to_string()))),
                    ["builder", db] => Some(Box::new(BuilderTab::new(Some(db.to_string()).filter(|d| !d.is_empty())))),
                    ["safety"] => Some(Box::new(crate::tabs::safety::SafetyTab::new())),
                    _ => None,
                };
                if let Some(tab) = tab {
                    self.tabs.push(tab);
                }
            }
            self.active = s.active.min(self.tabs.len().saturating_sub(1));
        }
        // Unbenannte Abfragen, die in keiner Sitzung stehen (z. B. nach Absturz), nicht verlieren
        if let Ok(rd) = std::fs::read_dir(workspace::scratch_dir()) {
            let mut ids: Vec<usize> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.strip_prefix("unbenannt-")?.strip_suffix(".sql")?.parse().ok()
                })
                .filter(|id| !restored_scratch.contains(id))
                .collect();
            ids.sort();
            for id in ids {
                if let Some(t) = SqlTab::restore_scratch(id, String::new()) {
                    if !t.text.trim().is_empty() {
                        self.tabs.push(Box::new(t));
                    } else {
                        let _ = std::fs::remove_file(workspace::scratch_path(id));
                    }
                }
            }
        }
        if session.is_none() && self.tabs.is_empty() {
            let w = self.project.join("Willkommen.sql");
            if w.exists() {
                self.open_file(&w);
            }
        }
    }

    pub(super) fn save_session(&mut self) {
        self.session_saved = std::time::Instant::now();
        let s = Session {
            project: self.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            tabs: self.tabs.iter().filter_map(|t| t.session()).collect(),
            active: self.active,
            side_view: self.side_view.name().into(),
            sidebar: self.sidebar,
        };
        s.save();
    }

    /// Alle offenen Dateien speichern und Sitzung sichern.
    pub(super) fn save_all(&mut self) {
        for t in self.tabs.iter_mut() {
            t.save_now();
        }
        self.save_session();
    }

    // ------------------------------------------------------------------
    // Aktivitaetsleiste

    pub(super) fn activity_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let pal = style::pal();
        let items: [(Icon, &str, Option<SideView>); 5] = [
            (Icon::Files, "Explorer (Projekte und Dateien)", Some(SideView::Explorer)),
            (Icon::Database, "Datenbanken", Some(SideView::Datenbanken)),
            (Icon::Diagram, "ER-Diagramm der aktuellen Datenbank", None),
            (Icon::Wizard, "Abfrage-Assistent", None),
            (Icon::Backup, "Sicherungen & Reparatur", Some(SideView::Sicherungen)),
        ];
        ui.add_space(4.0);
        for (i, (icon, tip, view)) in items.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(48.0, 46.0), Sense::click());
            let active = view.is_some_and(|v| self.sidebar && v == self.side_view);
            let hovered = resp.hovered();
            if active {
                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 0.0, pal.activity_active);
            }
            let color = if active || hovered { pal.activity_active } else { pal.activity_fg };
            paint_icon(ui.painter(), r, *icon, color);
            let resp = resp.on_hover_text(*tip);
            if resp.clicked() {
                match (i, view) {
                    (_, Some(v)) => {
                        if self.sidebar && self.side_view == *v {
                            self.sidebar = false;
                        } else {
                            self.side_view = *v;
                            self.sidebar = true;
                        }
                    }
                    (2, None) => {
                        if let Some(d) = self.need_db() {
                            actions.push(Action::OpenEr(d));
                        }
                    }
                    (3, None) => actions.push(Action::OpenBuilder(Some(self.current_db.clone()))),
                    _ => {}
                }
            }
        }
        // Einstellungen unten
        let rest = ui.available_height() - 50.0;
        if rest > 0.0 {
            ui.add_space(rest);
        }
        let (r, resp) = ui.allocate_exact_size(vec2(48.0, 46.0), Sense::click());
        let color = if resp.hovered() { pal.activity_active } else { pal.activity_fg };
        paint_icon(ui.painter(), r, Icon::Gear, color);
        resp.clone().on_hover_text("Einstellungen");
        resp.context_menu(|ui| self.settings_menu(ui));
        if resp.clicked() {
            self.dialogs.push(Dialog::Settings);
        }
    }

    pub(super) fn settings_menu(&mut self, ui: &mut egui::Ui) {
        if ui.radio(style::pal().dark, "Dunkles Design").clicked() {
            self.set_dark(ui.ctx(), true);
            ui.close();
        }
        if ui.radio(!style::pal().dark, "Helles Design").clicked() {
            self.set_dark(ui.ctx(), false);
            ui.close();
        }
    }

    pub(super) fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        style::set_theme(ctx, dark);
        self.settings.dark = dark;
        self.settings.save();
    }

    // ------------------------------------------------------------------
    // Seitenleiste

    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.add_space(6.0);
        match self.side_view {
            SideView::Explorer => self.explorer(ui),
            SideView::Datenbanken => {
                let mut new_db = false;
                let mut refresh = false;
                section_header(ui, "DATENBANKEN", |ui| {
                    if icon_button(ui, "⟳", "Aktualisieren").clicked() {
                        refresh = true;
                    }
                    if icon_button(ui, "+", "Neue Datenbank").clicked() {
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
                    self.tree(ui, actions);
                });
            }
            SideView::Sicherungen => self.backups_side(ui, actions),
        }
    }

    fn rescan_tree(&mut self) {
        let due = self.tree_scan.is_none_or(|t| t.elapsed().as_secs() >= 2);
        if due {
            self.tree = Some(workspace::scan(&self.project));
            self.tree_scan = Some(std::time::Instant::now());
        }
    }

    fn explorer(&mut self, ui: &mut egui::Ui) {
        self.rescan_tree();
        let mut new_file = false;
        let mut new_folder = false;
        let mut refresh = false;
        section_header(ui, "EXPLORER", |ui| {
            if icon_button(ui, "⟳", "Aktualisieren").clicked() {
                refresh = true;
            }
            if icon_button(ui, "🗀", "Neuer Ordner").clicked() {
                new_folder = true;
            }
            if icon_button(ui, "+", "Neue Datei").clicked() {
                new_file = true;
            }
        });
        if refresh {
            self.tree_scan = None;
        }
        if new_file {
            self.explorer_dlg = Some(ExplorerDlg::NewFile { dir: self.project.clone(), name: new_name(&self.project.clone()) });
        }
        if new_folder {
            self.explorer_dlg = Some(ExplorerDlg::NewFolder { dir: self.project.clone(), name: String::new() });
        }
        // Projektauswahl
        let pname = self.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.menu_button(RichText::new(format!("▾ {}", pname.to_uppercase())).strong(), |ui| {
                ui.label(RichText::new("Projekt wechseln").small().color(style::pal().text_weak));
                for p in workspace::list_projects() {
                    if ui.selectable_label(p == pname, &p).clicked() {
                        self.switch_project(workspace::projects_root().join(&p));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Neues Projekt...").clicked() {
                    self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                    ui.close();
                }
                if ui.button("Projektordner öffnen").clicked() {
                    open_folder(&workspace::projects_root());
                    ui.close();
                }
            });
        });
        let active_file = self.tabs.get(self.active).and_then(|t| t.file().map(|p| p.to_path_buf()));
        let Some(tree) = self.tree.clone() else { return };
        let mut open: Option<PathBuf> = None;
        let mut dlg: Option<ExplorerDlg> = None;
        egui::ScrollArea::vertical().id_salt("explorer").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if tree.children.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Noch keine Dateien.").color(style::pal().text_weak));
                });
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if ui.button("Neue SQL-Datei").clicked() {
                        dlg = Some(ExplorerDlg::NewFile { dir: tree.path.clone(), name: new_name(&tree.path.clone()) });
                    }
                });
            }
            for n in &tree.children {
                draw_node(ui, n, 0, active_file.as_deref(), &mut open, &mut dlg);
            }
            // Rechtsklick auf freie Flaeche
            let rest = ui.allocate_response(ui.available_size().max(vec2(10.0, 40.0)), Sense::click());
            rest.context_menu(|ui| {
                if ui.button("Neue Datei...").clicked() {
                    dlg = Some(ExplorerDlg::NewFile { dir: tree.path.clone(), name: new_name(&tree.path.clone()) });
                    ui.close();
                }
                if ui.button("Neuer Ordner...").clicked() {
                    dlg = Some(ExplorerDlg::NewFolder { dir: tree.path.clone(), name: String::new() });
                    ui.close();
                }
                if ui.button("Im Datei-Explorer zeigen").clicked() {
                    open_folder(&tree.path);
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

    pub(super) fn switch_project(&mut self, dir: PathBuf) {
        self.save_all();
        self.project = dir;
        self.tree_scan = None;
        self.status = format!("Projekt: {}", self.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }

    fn backups_side(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let mut open = false;
        section_header(ui, "SICHERUNGEN", |ui| {
            if icon_button(ui, "↗", "Alle Sicherungen & Reparatur").clicked() {
                open = true;
            }
        });
        let cfg = crate::backup::Config::load();
        egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
            ui.label(
                RichText::new(if cfg.auto {
                    format!("Automatisch alle {} h, vor jedem Löschen", cfg.interval_hours)
                } else {
                    "Automatische Sicherung ist aus".into()
                })
                .small()
                .color(style::pal().text_weak),
            );
            if ui.button("Sicherungen & Reparatur öffnen").clicked() {
                open = true;
            }
            ui.add_space(6.0);
            for b in crate::backup::list(&cfg.dir()).iter().take(12) {
                ui.label(RichText::new(format!("{}  ·  {}", b.time, crate::backup::reason_text(&b.reason))).small());
                let names: Vec<&str> = b.dbs.iter().map(|d| d.0.as_str()).collect();
                ui.label(RichText::new(format!("   {}", names.join(", "))).small().color(style::pal().text_weak));
            }
        });
        if open {
            actions.push(Action::OpenSafety);
        }
    }

    // ------------------------------------------------------------------
    // Registerkarten

    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let mut close = None;
        let mut activate = None;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, pal.tab_bar_bg);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        egui::ScrollArea::horizontal().id_salt("tabbar").show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, t) in self.tabs.iter().enumerate() {
                let sel = i == self.active;
                let title = t.title();
                let galley = ui.painter().layout_no_wrap(title.clone(), egui::FontId::proportional(13.0), pal.text);
                let w = galley.size().x + 44.0;
                let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
                let hovered = resp.hovered();
                ui.painter().rect_filled(r, 0.0, if sel { pal.tab_active } else { pal.tab_inactive });
                if sel {
                    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 2.0)), 0.0, pal.accent);
                }
                ui.painter().vline(r.right(), r.y_range(), Stroke::new(1.0, pal.tab_bar_bg));
                let color = if sel { pal.text } else { pal.text_weak };
                let mut label = title;
                if t.busy() {
                    label.push_str(" …");
                }
                ui.painter().text(r.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), color);
                // Schliessen-Kreuz
                let cr = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0));
                let over_close = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| cr.contains(p));
                if sel || hovered {
                    if over_close {
                        ui.painter().rect_filled(cr, 3.0, pal.hover);
                    }
                    let st = Stroke::new(1.3, color);
                    ui.painter().line_segment([cr.center() + vec2(-4.0, -4.0), cr.center() + vec2(4.0, 4.0)], st);
                    ui.painter().line_segment([cr.center() + vec2(4.0, -4.0), cr.center() + vec2(-4.0, 4.0)], st);
                }
                if resp.clicked() {
                    if over_close {
                        close = Some(i);
                    } else {
                        activate = Some(i);
                    }
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                resp.context_menu(|ui| {
                    if ui.button("Schließen").clicked() {
                        close = Some(i);
                        ui.close();
                    }
                    if let Some(f) = t.file() {
                        if ui.button("Im Datei-Explorer zeigen").clicked() {
                            if let Some(d) = f.parent() {
                                open_folder(d);
                            }
                            ui.close();
                        }
                        if ui.button("Pfad kopieren").clicked() {
                            ui.ctx().copy_text(f.display().to_string());
                            ui.close();
                        }
                    }
                });
            }
        });
        if let Some(i) = activate {
            self.active = i;
        }
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    // ------------------------------------------------------------------
    // Statusleiste

    pub(super) fn status_bar_vs(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let fg = pal.status_fg;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            // Rechte Seite zuerst, damit links nichts darueber laeuft
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(d) = &self.db {
                    let v = d.version.split('-').next().unwrap_or("").to_string();
                    ui.label(RichText::new(format!("MariaDB {v}")).color(fg));
                }
                if let Some(t) = self.tabs.get(self.active) {
                    if t.file().is_some() || t.session().is_some_and(|s| s.starts_with("scratch")) {
                        ui.label(RichText::new("SQL").color(fg));
                        ui.label(RichText::new("UTF-8").color(fg));
                    }
                    if let Some(s) = t.status() {
                        ui.label(RichText::new(s).color(fg));
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let st = self.server.state();
                    let dot = if st.is_running() { "●" } else if st.is_busy() { "◌" } else { "○" };
                    let r = ui.add(egui::Label::new(RichText::new(format!("{dot} Server: {}", st.text())).color(fg)).sense(Sense::click()));
                    if r.on_hover_text("Server-Log anzeigen").clicked() {
                        self.show_log = true;
                    }
                    if let Some(d) = &self.db {
                        ui.label(RichText::new(d.info.label()).color(fg));
                    }
                    let proj = self.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(format!("Projekt: {proj}")).color(fg));
                    ui.add(egui::Label::new(RichText::new(&self.status).color(fg)).truncate());
                });
            });
        });
    }

    // ------------------------------------------------------------------
    // Willkommensseite

    pub(super) fn welcome(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let pal = style::pal();
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(40.0);
            ui.horizontal(|ui| {
                ui.add_space(50.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("EasyMySQL").size(34.0).color(pal.text));
                    ui.label(RichText::new("MariaDB-Server und SQL-Editor in einem Programm").size(16.0).color(pal.text_weak));
                    ui.add_space(24.0);
                    ui.columns(2, |cols| {
                        let ui = &mut cols[0];
                        ui.label(RichText::new("Start").size(18.0));
                        ui.add_space(4.0);
                        if ui.link("Neue SQL-Datei im Projekt …").clicked() {
                            self.explorer_dlg = Some(ExplorerDlg::NewFile { dir: self.project.clone(), name: new_name(&self.project.clone()) });
                        }
                        if ui.link("Neue unbenannte Abfrage (Strg+N)").clicked() {
                            actions.push(Action::OpenSql { db: Some(self.current_db.clone()), sql: String::new(), run: false });
                        }
                        if ui.link("Datei öffnen …").clicked() {
                            self.open_sql_file(false);
                        }
                        if ui.link("Neues Projekt …").clicked() {
                            self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                        }
                        if ui.link("Neue Datenbank …").clicked() {
                            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
                        }
                        if ui.link("ER-Diagramm (Reverse Engineering)").clicked() {
                            if let Some(d) = self.need_db() {
                                actions.push(Action::OpenEr(d));
                            }
                        }
                        ui.add_space(16.0);
                        ui.label(RichText::new("Server").size(18.0));
                        let st = self.server.state();
                        let color = if st.is_running() { pal.ok_text } else if matches!(st, State::Failed(_) | State::NotFound) { pal.error_text } else { pal.text };
                        ui.label(RichText::new(format!("{} (Port {})", st.text(), self.server.port)).color(color));
                        if let Some(d) = &self.db {
                            ui.label(format!("{} – MariaDB {}", d.info.label(), d.version));
                        }
                        ui.horizontal(|ui| {
                            if !st.is_running() && !st.is_busy() && ui.button("Server starten").clicked() {
                                self.start_server(&ctx);
                            }
                            if matches!(st, State::Failed(_)) && ui.button("Reparieren …").clicked() {
                                actions.push(Action::OpenSafety);
                            }
                            if ui.button("Server-Log").clicked() {
                                self.show_log = true;
                            }
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new("In der Eingabeaufforderung (cmd):").color(pal.text_weak));
                        ui.label(RichText::new("mysql -u root").monospace());

                        let ui = &mut cols[1];
                        ui.label(RichText::new("Tastenkürzel").size(18.0));
                        ui.add_space(4.0);
                        for (k, d) in [
                            ("Strg+Alt+S", "Datei oder Auswahl ausführen"),
                            ("Strg+Enter", "Anweisung am Cursor ausführen"),
                            ("Strg+Alt+L", "SQL formatieren"),
                            ("Strg+Leertaste", "Vorschläge"),
                            ("Strg+F / Strg+H", "Suchen / Ersetzen"),
                            ("Strg+G", "Gehe zu Zeile"),
                            ("Strg+#", "Kommentar ein/aus"),
                            ("Alt+↑ / Alt+↓", "Zeile verschieben"),
                            ("Strg+N", "Neue Abfrage"),
                            ("Strg+B", "Seitenleiste ein/aus"),
                            ("Strg+W", "Registerkarte schließen"),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(k).monospace().color(pal.syn_keyword));
                                ui.label(RichText::new(d).color(pal.text_weak));
                            });
                        }
                    });
                });
            });
        });
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
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(280.0));
                    r.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    if let ExplorerDlg::NewFile { .. } = dlg {
                        ui.label(RichText::new("Ohne Endung wird .sql angehängt.").small().color(style::pal().text_weak));
                    }
                }
                ExplorerDlg::Delete { path } => {
                    ui.label(format!("\"{}\" in den Papierkorb des Projekts verschieben?", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    ui.label(RichText::new("(Ordner .papierkorb im Projekt – kann wiederhergestellt werden)").small().color(style::pal().text_weak));
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
            ExplorerDlg::NewProject { name } => workspace::create_folder(&workspace::projects_root(), &name).map(|p| {
                self.switch_project(p);
                None
            }),
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
                workspace::trash(&self.project, &path).map(|_| None)
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

/// Freier Vorschlag fuer einen neuen Dateinamen
fn new_name(dir: &Path) -> String {
    workspace::unique_file(dir, "Neue Abfrage", "sql")
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Eintrag im Dateibaum zeichnen (rekursiv).
fn draw_node(ui: &mut egui::Ui, n: &Node, depth: usize, active: Option<&Path>, open: &mut Option<PathBuf>, dlg: &mut Option<ExplorerDlg>) {
    let pal = style::pal();
    let indent = 8.0 + depth as f32 * 14.0;
    if n.is_dir {
        let id = egui::Id::new(("dir", &n.path));
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth == 0 && false);
        let is_open = state.is_open();
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, pal.hover);
        }
        ui.painter().text(pos2(r.left() + indent, r.center().y), egui::Align2::LEFT_CENTER, if is_open { "⌄" } else { "›" }, egui::FontId::proportional(14.0), pal.text_weak);
        ui.painter().text(pos2(r.left() + indent + 14.0, r.center().y), egui::Align2::LEFT_CENTER, &n.name, egui::FontId::proportional(13.0), pal.text);
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
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        if sel {
            ui.painter().rect_filled(r, 0.0, if pal.dark { Color32::from_rgb(0x37, 0x37, 0x3D) } else { Color32::from_rgb(0xE4, 0xE6, 0xF1) });
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, pal.hover);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(r.left() + indent + 12.0, r.top() + 3.0), vec2(16.0, 16.0))));
        file_icon(&mut child, &n.name);
        ui.painter().text(pos2(r.left() + indent + 32.0, r.center().y), egui::Align2::LEFT_CENTER, &n.name, egui::FontId::proportional(13.0), pal.text);
        if resp.clicked() {
            *open = Some(n.path.clone());
        }
        resp.context_menu(|ui| {
            if ui.button("Öffnen").clicked() {
                *open = Some(n.path.clone());
                ui.close();
            }
            if ui.button("Umbenennen...").clicked() {
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
                    open_folder(d);
                }
                ui.close();
            }
        });
    }
}

fn dir_menu(resp: &egui::Response, n: &Node, dlg: &mut Option<ExplorerDlg>) {
    resp.context_menu(|ui| {
        if ui.button("Neue Datei...").clicked() {
            *dlg = Some(ExplorerDlg::NewFile { dir: n.path.clone(), name: new_name(&n.path.clone()) });
            ui.close();
        }
        if ui.button("Neuer Ordner...").clicked() {
            *dlg = Some(ExplorerDlg::NewFolder { dir: n.path.clone(), name: String::new() });
            ui.close();
        }
        ui.separator();
        if ui.button("Umbenennen...").clicked() {
            *dlg = Some(ExplorerDlg::Rename { path: n.path.clone(), name: n.name.clone() });
            ui.close();
        }
        if ui.button("Löschen").clicked() {
            *dlg = Some(ExplorerDlg::Delete { path: n.path.clone() });
            ui.close();
        }
        if ui.button("Im Datei-Explorer zeigen").clicked() {
            open_folder(&n.path);
            ui.close();
        }
    });
}
