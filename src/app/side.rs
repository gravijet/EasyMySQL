// Seitenleisten: Explorer (Projektordner), Suchen in Dateien, Datenbanken, Verlauf.

use super::*;
use crate::db::q;
use crate::icons::{self, Icon};
use crate::workspace::{self, Node};
use crate::keymap::{self, Cmd};
use eframe::egui::{Color32, RichText, Sense, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SideView {
    Explorer,
    Search,
    Databases,
    History,
    Git,
    Claude,
    Codex,
}

impl SideView {
    pub fn name(&self) -> &'static str {
        match self {
            SideView::Explorer => "explorer",
            SideView::Search => "suche",
            SideView::Databases => "datenbanken",
            SideView::History => "verlauf",
            SideView::Git => "git",
            SideView::Claude => "claude",
            SideView::Codex => "codex",
        }
    }
    pub fn from_name(s: &str) -> SideView {
        match s {
            "datenbanken" => SideView::Databases,
            "suche" => SideView::Search,
            "verlauf" => SideView::History,
            "git" => SideView::Git,
            "claude" => SideView::Claude,
            "codex" => SideView::Codex,
            _ => SideView::Explorer,
        }
    }
}

pub enum ExplorerDlg {
    NewFile(Box<super::create::NewFile>),
    NewFolder { dir: PathBuf, name: String },
    Rename { path: PathBuf, name: String },
    Delete { paths: Vec<PathBuf> },
    NewProject { name: String },
}

/// Beim Ziehen im Explorer: Pfad der gezogenen Datei bzw. des Ordners
struct DragPath(PathBuf);

/// Auswahl, Zwischenablage und Fokus des Explorers
#[derive(Default)]
pub struct ExplorerState {
    /// Ausgewaehlte Dateien und Ordner (Strg+Klick, Umschalt+Klick)
    pub sel: Vec<PathBuf>,
    anchor: Option<PathBuf>,
    /// Kopiert bzw. ausgeschnitten (true)
    clip: Option<(Vec<PathBuf>, bool)>,
    /// Zuletzt aktive Datei im Editor (die Auswahl folgt ihr)
    last_active: Option<PathBuf>,
    /// Tasten gehen an den Baum (nach einem Klick hinein, bis woanders geklickt oder getippt wird)
    pub focused: bool,
    /// Ein paar Bilder nach dem Klick: Editor einer gerade geoeffneten Datei bekommt den Fokus nicht
    hold: u8,
}

impl ExplorerState {
    /// Ordner fuer Neues und Einfuegen: der gewaehlte Ordner bzw. der Ordner der gewaehlten Datei
    pub fn target_dir(&self, project: &Path) -> PathBuf {
        match self.sel.last() {
            Some(p) if p.is_dir() => p.clone(),
            Some(p) => p.parent().filter(|d| d.starts_with(project)).map(Path::to_path_buf).unwrap_or_else(|| project.to_path_buf()),
            None => project.to_path_buf(),
        }
    }
}

/// Befehl aus dem Kontextmenue oder per Taste
enum TreeCmd {
    NewFile(PathBuf, Option<super::create::FileKind>),
    NewFolder(PathBuf),
    NewDatabase,
    Open(Vec<PathBuf>),
    Rename(PathBuf),
    Delete(Vec<PathBuf>),
    Copy(Vec<PathBuf>),
    Cut(Vec<PathBuf>),
    Paste(PathBuf),
    CopyPath(PathBuf, bool),
    Reveal(PathBuf),
    Terminal(PathBuf),
}

/// Zustand beim Zeichnen des Dateibaums: Rueckgaben und Ziehen/Ablegen
struct TreeIo<'a> {
    /// Hervorgehoben (Auswahl, sonst die aktive Datei)
    sel: &'a [PathBuf],
    /// Ausgeschnitten (blass)
    cut: &'a [PathBuf],
    can_paste: bool,
    connected: bool,
    focused: bool,
    /// Sichtbare Zeilen in Reihenfolge (fuer Umschalt+Klick und Pfeiltasten)
    order: Vec<(PathBuf, bool)>,
    /// Klick auf eine Zeile: Pfad, Ordner?, Tasten
    click: Option<(PathBuf, bool, egui::Modifiers)>,
    /// Rechtsklick (waehlt die Zeile, wenn sie nicht schon gewaehlt ist)
    right: Option<PathBuf>,
    cmd: Option<TreeCmd>,
    /// Ablage: (Zielordner, Quellen, verschieben statt kopieren)
    drop: Option<(PathBuf, Vec<PathBuf>, bool)>,
    /// Mauszeiger waehrend Dateien von aussen gezogen werden bzw. beim Ablegen
    os_pos: Option<egui::Pos2>,
    os_files: &'a [PathBuf],
}

impl TreeIo<'_> {
    /// Ablage auf `rect` (Ordner `dir`): zeichnet die Markierung und merkt sich die Ablage.
    fn target(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, dir: &Path) {
        let mut hot = false;
        if self.os_pos.is_some_and(|p| rect.contains(p)) {
            hot = true;
            if !self.os_files.is_empty() {
                self.drop = Some((dir.to_path_buf(), self.os_files.to_vec(), false));
            }
        }
        if let Some(src) = resp.dnd_release_payload::<DragPath>() {
            let srcs = if self.sel.contains(&src.0) { self.sel.to_vec() } else { vec![src.0.clone()] };
            self.drop = Some((dir.to_path_buf(), srcs, true));
        }
        if resp.dnd_hover_payload::<DragPath>().is_some() {
            hot = true;
        }
        if hot {
            let accent = style::pal().accent;
            ui.painter().rect(rect, 3.0, accent.gamma_multiply(0.18), egui::Stroke::new(1.0, accent), egui::StrokeKind::Inside);
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub case: bool,
    pub focus: bool,
    searched: Option<(String, bool)>,
    results: Hits,
    truncated: bool,
    changed_at: Option<Instant>,
    job: Option<SearchJob>,
}

struct SearchJob {
    receiver: Receiver<(Hits, bool)>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for SearchJob {
    fn drop(&mut self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    }
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

/// Dateien fuer die Schnellsuche: Textdateien sowie Bilder und PDF-Dokumente
pub fn openable_files(n: &Node, out: &mut Vec<PathBuf>) {
    for c in &n.children {
        if c.is_dir {
            openable_files(c, out);
        } else {
            let ext = c.name.rsplit('.').next().unwrap_or("").to_lowercase();
            if matches!(ext.as_str(), "sql" | "txt" | "md" | "csv" | "json") || crate::tabs::preview::previewable_name(&c.name) {
                out.push(c.path.clone());
            }
        }
    }
}

/// Treffer je Datei: (Datei, [(Zeile, Text)])
type Hits = Vec<(PathBuf, Vec<(usize, String)>)>;

#[cfg(test)]
fn search_files(files: &[PathBuf], query: &str, case: bool) -> (Hits, bool) {
    search_files_cancellable(files, query, case, &std::sync::atomic::AtomicBool::new(false))
}

fn search_files_cancellable(files: &[PathBuf], query: &str, case: bool, cancelled: &std::sync::atomic::AtomicBool) -> (Hits, bool) {
    let q = if case { query.to_string() } else { query.to_lowercase() };
    let mut out = Vec::new();
    let mut total = 0;
    for f in files {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) { return (Vec::new(), false); }
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let mut hits = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if i % 256 == 0 && cancelled.load(std::sync::atomic::Ordering::Relaxed) { return (Vec::new(), false); }
            let matches = if case { line.contains(&q) } else { line.to_lowercase().contains(&q) };
            if matches {
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

pub(super) fn section_header(ui: &mut egui::Ui, title: &str, add_buttons: impl FnOnce(&mut egui::Ui)) {
    // Feste Zeilenhoehe: Titel und Knoepfe stehen in jeder Ansicht auf derselben Mittellinie
    // Knoepfe zuerst (rechts), der Titel bekommt den Rest und wird bei Bedarf mit "…" gekuerzt
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            add_buttons(ui);
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(title).size(11.0).color(style::pal().text_weak)).truncate()).on_hover_text(title);
            });
        });
    });
}


/// Zeile mit Symbol und Text ueber die ganze Breite (Willkommensansicht)
fn action_row(ui: &mut egui::Ui, icon: Icon, text: &str, detail: &str) -> egui::Response {
    let pal = style::pal();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, style::RADIUS as f32, pal.hover);
    }
    let cy = r.center().y;
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 16.0, cy), egui::vec2(16.0, 16.0)), icon, pal.accent);
    ui.painter().text(egui::pos2(r.left() + 34.0, cy), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), pal.text);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if detail.is_empty() { resp } else { resp.on_hover_text(detail) }
}

/// Zuletzt geoeffnetes Projekt: farbiges Kaestchen, Name und Ordner darunter (auch im Projektmenue der Leiste)
pub(super) fn recent_row(ui: &mut egui::Ui, p: &Path) -> egui::Response {
    let pal = style::pal();
    let resp = ui.allocate_response(egui::vec2(260.0, 40.0), egui::Sense::click());
    let r = resp.rect;
    if resp.hovered() {
        ui.painter().rect_filled(r, style::RADIUS as f32, pal.hover);
    }
    let name = workspace::dir_name(p);
    let (initials, color) = super::titlebar::project_badge(&name);
    let b = egui::Rect::from_center_size(egui::pos2(r.left() + 20.0, r.center().y), egui::vec2(26.0, 26.0));
    icons::badge(ui.painter(), b, &initials, color, egui::Color32::WHITE, 6.0);
    let x = r.left() + 40.0;
    let painter = ui.painter_at(r.shrink2(egui::vec2(4.0, 0.0)));
    let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), egui::FontId::proportional(13.0), pal.text);
    job.wrap = egui::text::TextWrapping { max_width: (r.right() - x - 8.0).max(20.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = painter.layout_job(job);
    painter.galley(egui::pos2(x, r.center().y - 8.0 - g.size().y / 2.0), g, pal.text);
    // Ordner mit ~ statt des Benutzerordners, lange Pfade vorne gekuerzt
    let mut dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| h.to_string_lossy().into_owned())
        && !home.is_empty()
        && dir.starts_with(&home)
    {
        dir = format!("~{}", &dir[home.len()..]);
    }
    let max = ((r.width() - 48.0) / 6.2).max(10.0) as usize;
    if dir.chars().count() > max {
        dir = format!("…{}", dir.chars().rev().take(max - 1).collect::<Vec<_>>().into_iter().rev().collect::<String>());
    }
    painter.text(egui::pos2(x, r.center().y + 8.0), egui::Align2::LEFT_CENTER, dir, egui::FontId::proportional(11.0), pal.text_weak);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(p.display().to_string())
}

impl EasyApp {
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        match self.side_view {
            SideView::Explorer => self.explorer(ui),
            SideView::Search => self.search_side(ui, actions),
            SideView::Databases => self.databases_side(ui, actions),
            SideView::History => self.history_side(ui, actions),
            SideView::Git => self.git_side(ui, actions),
            SideView::Claude | SideView::Codex => {
                let index = usize::from(self.side_view == SideView::Codex);
                self.chat_side(ui, actions, index);
            }
        }
    }

    pub(super) fn rescan_tree(&mut self) {
        let Some(p) = &self.project else {
            self.tree = None;
            return;
        };
        if let Some((project, tree)) = self.tree_job.as_ref().and_then(|job| job.try_recv().ok()) {
            if self.project.as_ref() == Some(&project) {
                self.tree = Some(std::sync::Arc::new(tree));
            }
            self.tree_job = None;
        }
        // Im Hintergrund (Fenster nicht aktiv) nicht laufend den ganzen Ordner lesen
        let focused = self.was_focused;
        if self.tree_job.is_none() && (self.tree.is_none() || focused) && self.tree_scan.is_none_or(|t| t.elapsed().as_secs() >= 3) {
            let project = p.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = CTX.lock().unwrap().clone();
            std::thread::spawn(move || {
                let tree = workspace::scan(&project);
                let _ = tx.send((project, tree));
                if let Some(ctx) = ctx { ctx.request_repaint(); }
            });
            self.tree_job = Some(rx);
            self.tree_scan = Some(Instant::now());
        }
    }

    // ------------------------------------------------------------------
    // Explorer

    fn explorer(&mut self, ui: &mut egui::Ui) {
        self.rescan_tree();
        let Some(project) = self.project.clone() else {
            section_header(ui, "EXPLORER", |_| {});
            // Wie die Willkommensseite von IntelliJ IDEA: Aktionen als Zeilen mit Symbol, darunter die letzten Projekte
            // Gemeinsame linke Kante: 8 px wie die Ueberschrift der Seitenleiste
            egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 2)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(4.0);
                if action_row(ui, Icon::Folder, crate::i18n::text("Ordner öffnen"), "").clicked() {
                    self.pick_project();
                }
                if action_row(ui, Icon::NewFolder, crate::i18n::text("Neues Projekt"), "").clicked() {
                    self.explorer_dlg = Some(ExplorerDlg::NewProject { name: String::new() });
                }
                if action_row(ui, Icon::FileCode, crate::i18n::tr("Neues Programmierprojekt", "New coding project"), "Java, Python, JavaScript").clicked() {
                    self.open_project_dialog(super::create::ProjectLang::Java);
                }
                if action_row(ui, Icon::GitBranch, crate::i18n::tr("Repository klonen", "Clone repository"), "").clicked() {
                    self.open_clone_dialog();
                }
                let recent: Vec<PathBuf> = self.settings.recent.iter().filter(|p| p.is_dir()).cloned().collect();
                if !recent.is_empty() {
                    ui.add_space(10.0);
                    section_header(ui, &crate::i18n::text("Zuletzt geöffnet").to_uppercase(), |_| {});
                    for p in recent {
                        if recent_row(ui, &p).clicked() {
                            self.switch_project(Some(p));
                        }
                    }
                }
            });
            return;
        };
        let active_file = self.tabs.get(self.active).and_then(|t| t.file().map(|p| p.to_path_buf()));
        // Die Auswahl folgt der Datei im Editor (wie "Immer die geoeffnete Datei waehlen" in IntelliJ IDEA)
        if active_file != self.explorer.last_active {
            if let Some(f) = active_file.as_ref().filter(|f| f.starts_with(&project)) {
                self.explorer.sel = vec![f.clone()];
                self.explorer.anchor = Some(f.clone());
            }
            self.explorer.last_active = active_file.clone();
        }
        let mut collapse = false;
        let mut cmd: Option<TreeCmd> = None;
        let target = self.explorer.target_dir(&project);
        let connected = self.db.is_some();
        let can_paste = self.explorer.clip.is_some();
        let title = workspace::dir_name(&project).to_uppercase();
        section_header(ui, &title, |ui| {
            let more = icons::button(ui, Icon::More, crate::i18n::text("Projekt"));
            egui::Popup::menu(&more).show(|ui| {
                crate::style::menu_item_look(ui);
                ui.set_min_width(230.0);
                if icons::menu_item(ui, Some(Icon::Refresh), crate::i18n::text("Aktualisieren"), "").clicked() {
                    self.tree_scan = None;
                    ui.close();
                }
                if icons::menu_item(ui, Some(Icon::External), crate::i18n::text("Im Datei-Explorer zeigen"), "").clicked() {
                    open_path(&project);
                    ui.close();
                }
                if icons::menu_item(ui, Some(Icon::Terminal), crate::i18n::tr("Terminal hier öffnen", "Open terminal here"), "").clicked() {
                    cmd = Some(TreeCmd::Terminal(project.clone()));
                    ui.close();
                }
                ui.separator();
                if icons::menu_item(ui, Some(Icon::Close), crate::i18n::tr("Projekt schließen", "Close project"), "").clicked() {
                    self.switch_project(None);
                    ui.close();
                }
            });
            if icons::button(ui, Icon::Collapse, crate::i18n::text("Alle Ordner zuklappen")).clicked() {
                collapse = true;
            }
            let plus = icons::button(ui, Icon::Plus, &crate::tr_format!("Neu in „{}“", "New in “{}”", workspace::dir_name(&target)));
            egui::Popup::menu(&plus).show(|ui| {
                crate::style::menu_item_look(ui);
                new_menu(ui, &target, connected, &mut cmd);
            });
        });
        if self.project.is_none() {
            return;
        }
        let Some(tree) = self.tree.clone() else { return };
        if collapse {
            collapse_all(ui.ctx(), &tree);
        }
        let os_files = std::mem::take(&mut self.os_files);
        let sel = self.explorer.sel.clone();
        let cut: Vec<PathBuf> = match &self.explorer.clip {
            Some((v, true)) => v.clone(),
            _ => Vec::new(),
        };
        let mut io = TreeIo { sel: &sel, cut: &cut, can_paste, connected, focused: self.explorer.focused, order: Vec::new(), click: None, right: None, cmd: None, drop: None, os_pos: self.os_pos, os_files: &os_files };
        let tree_id = egui::Id::new("explorer-tree");
        crate::tabs::page_keys(ui);
        let area = egui::ScrollArea::vertical().id_salt("explorer").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for n in &tree.children {
                draw_node(ui, n, 0, &mut io);
            }
            // Freie Flaeche: Ablage im Projektordner, Klick hebt die Auswahl auf, Doppelklick legt eine Datei an
            let rest = ui.allocate_response(ui.available_size().max(vec2(10.0, 40.0)), Sense::click());
            io.target(ui, &rest, rest.rect, &tree.path);
            if rest.clicked() {
                io.click = Some((tree.path.clone(), true, egui::Modifiers::NONE));
            }
            if rest.double_clicked() {
                io.cmd = Some(TreeCmd::NewFile(tree.path.clone(), None));
            }
            rest.context_menu(|ui| {
                crate::style::menu_item_look(ui);
                new_menu(ui, &tree.path, io.connected, &mut io.cmd);
                ui.separator();
                if icons::menu_item(ui, Some(Icon::Clipboard), crate::i18n::tr("Einfügen", "Paste"), "Strg+V").clicked() {
                    io.cmd = Some(TreeCmd::Paste(tree.path.clone()));
                    ui.close();
                }
                if icons::menu_item(ui, Some(Icon::External), crate::i18n::text("Im Datei-Explorer zeigen"), "").clicked() {
                    io.cmd = Some(TreeCmd::Reveal(tree.path.clone()));
                    ui.close();
                }
            });
        });
        let TreeIo { order, click, right, cmd: tree_cmd, drop, .. } = io;
        let cmd = cmd.or(tree_cmd);
        let ex = &mut self.explorer;
        let clicked_now = click.is_some();
        // Eigener Fokus: Klick in den Baum holt ihn, Klick woanders oder ein Eingabefeld mit Fokus gibt ihn ab
        if click.is_some() || right.is_some() {
            ex.focused = true;
            ex.hold = 4;
        }
        if ex.hold > 0 {
            ex.hold -= 1;
            ui.memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
            ui.ctx().request_repaint();
        } else if ui.input(|i| i.pointer.any_pressed()) && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.inner_rect.contains(p)) {
            ex.focused = false;
        }
        if ui.memory(|m| m.focused()).is_some_and(|f| f != tree_id) {
            ex.focused = false;
        }
        let mut open: Vec<PathBuf> = Vec::new();
        if let Some((path, is_dir, mods)) = click {
            if path == tree.path {
                ex.sel.clear();
                ex.anchor = None;
            } else if mods.command {
                if let Some(i) = ex.sel.iter().position(|p| *p == path) {
                    ex.sel.remove(i);
                } else {
                    ex.sel.push(path.clone());
                }
                ex.anchor = Some(path);
            } else if mods.shift && let Some(a) = ex.anchor.clone() {
                let ia = order.iter().position(|(p, _)| *p == a);
                let ib = order.iter().position(|(p, _)| *p == path);
                if let (Some(ia), Some(ib)) = (ia, ib) {
                    ex.sel = order[ia.min(ib)..=ia.max(ib)].iter().map(|(p, _)| p.clone()).collect();
                }
            } else {
                ex.sel = vec![path.clone()];
                ex.anchor = Some(path.clone());
                if !is_dir {
                    open.push(path);
                }
            }
        }
        if let Some(p) = right
            && !ex.sel.contains(&p)
        {
            ex.sel = vec![p.clone()];
            ex.anchor = Some(p);
        }
        let mut cmd = cmd;
        if std::env::var_os("EMDBG").is_some() && ui.input(|i| !i.events.is_empty()) { eprintln!("focused={} mem={:?} ev={:?}", ex.focused, ui.memory(|m| m.focused()), ui.input(|i| i.events.clone())); }
        if ex.focused && cmd.is_none() && !clicked_now {
            cmd = tree_keys(ui, ex, &order, &tree.path);
        }
        for p in open {
            self.open_file(&p);
        }
        if let Some(c) = cmd {
            self.tree_cmd(ui.ctx(), c);
        }
        if let Some((dir, srcs, mv)) = drop {
            self.apply_drop(&dir, &srcs, mv);
        } else {
            self.os_files = os_files;
        }
    }

    /// Befehl im Explorer ausfuehren
    fn tree_cmd(&mut self, ctx: &egui::Context, cmd: TreeCmd) {
        match cmd {
            TreeCmd::NewFile(dir, kind) => {
                let f = match kind {
                    Some(k) => super::create::NewFile::of_kind(dir, k),
                    None => super::create::NewFile::new(dir),
                };
                self.explorer_dlg = Some(ExplorerDlg::NewFile(Box::new(f)));
            }
            TreeCmd::NewFolder(dir) => self.explorer_dlg = Some(ExplorerDlg::NewFolder { dir, name: String::new() }),
            TreeCmd::NewDatabase => self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() }),
            TreeCmd::Open(paths) => {
                for p in paths.into_iter().filter(|p| p.is_file()) {
                    self.open_file(&p);
                }
            }
            TreeCmd::Rename(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.explorer_dlg = Some(ExplorerDlg::Rename { path, name });
            }
            TreeCmd::Delete(paths) if !paths.is_empty() => self.explorer_dlg = Some(ExplorerDlg::Delete { paths }),
            TreeCmd::Delete(_) => {}
            TreeCmd::Copy(paths) | TreeCmd::Cut(paths) if paths.is_empty() => {}
            TreeCmd::Copy(paths) => {
                // Auch als Text: so kommt Strg+V wieder an (und Pfade lassen sich woanders einfuegen)
                ctx.copy_text(paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"));
                self.toast(crate::tr_format!("{} kopiert.", "{} copied.", count_label(&paths)));
                self.explorer.clip = Some((paths, false));
            }
            TreeCmd::Cut(paths) => {
                ctx.copy_text(paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"));
                self.toast(crate::tr_format!("{} ausgeschnitten.", "{} cut.", count_label(&paths)));
                self.explorer.clip = Some((paths, true));
            }
            TreeCmd::Paste(dir) => match self.explorer.clip.clone() {
                Some((paths, cut)) => {
                    self.apply_drop(&dir, &paths, cut);
                    if cut {
                        self.explorer.clip = None;
                    }
                }
                None => {
                    // Pfade aus der Zwischenablage des Systems (z. B. aus einem anderen Programm kopiert)
                    let text = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None })).unwrap_or_default();
                    let paths: Vec<PathBuf> = text.lines().map(|l| PathBuf::from(l.trim().trim_matches('"'))).filter(|p| p.exists()).collect();
                    if !paths.is_empty() {
                        self.apply_drop(&dir, &paths, false);
                    }
                }
            },
            TreeCmd::CopyPath(p, relative) => {
                let text = match (&self.project, relative) {
                    (Some(root), true) => p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/"),
                    _ => p.display().to_string(),
                };
                ctx.copy_text(text);
                self.toast(crate::i18n::tr("Pfad kopiert.", "Path copied."));
            }
            TreeCmd::Reveal(p) => open_path(if p.is_dir() { &p } else { p.parent().unwrap_or(&p) }),
            TreeCmd::Terminal(dir) => self.new_terminal_in(ctx, None, Some(dir)),
        }
    }

    // ------------------------------------------------------------------
    // Suchen in Dateien

    fn search_side(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        section_header(ui, crate::i18n::text("SUCHEN"), |_| {});
        let Some(tree) = (self.rescan_tree(), self.tree.clone()).1 else {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                if ui.button(crate::i18n::text("Ordner öffnen")).clicked() {
                    self.pick_project();
                }
            });
            return;
        };
        let mut run = false;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            let w = ui.available_width();
            ui.horizontal(|ui| {
                let r = ui.add(crate::widgets::edit(&mut self.search.query).hint_text(crate::i18n::text("Suchen")).desired_width((w - 52.0).max(60.0)));
                if self.search.focus {
                    r.request_focus();
                    self.search.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    run = true;
                }
                if ui.toggle_value(&mut self.search.case, "Aa").on_hover_text(crate::i18n::text("Groß-/Kleinschreibung beachten")).changed() {
                    run = true;
                }
            });
        });
        // Debounce typing and cancel stale searches. File reads never run on the UI thread.
        let key = (self.search.query.clone(), self.search.case);
        if self.search.searched.as_ref() != Some(&key) || run {
            self.search.job = None;
            self.search.results.clear();
            self.search.truncated = false;
            self.search.searched = Some(key.clone());
            self.search.changed_at = Some(Instant::now());
        }
        if self.search.query.is_empty() {
            self.search.results.clear();
            self.search.changed_at = None;
            return;
        }
        if let Some(changed) = self.search.changed_at {
            let delay = Duration::from_millis(250);
            if run || (changed.elapsed() >= delay && self.search.query.chars().count() >= 2) {
                let mut files = Vec::new();
                text_files(&tree, &mut files);
                let (tx, receiver) = std::sync::mpsc::channel();
                let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let worker_cancelled = std::sync::Arc::clone(&cancelled);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = search_files_cancellable(&files, &key.0, key.1, &worker_cancelled);
                    if !worker_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    }
                });
                self.search.job = Some(SearchJob { receiver, cancelled });
                self.search.changed_at = None;
            } else if self.search.query.chars().count() >= 2 {
                ui.ctx().request_repaint_after(delay.saturating_sub(changed.elapsed()));
            }
        }
        if let Some(result) = self.search.job.as_ref().and_then(|job| job.receiver.try_recv().ok()) {
            self.search.results = result.0;
            self.search.truncated = result.1;
            self.search.job = None;
        }
        if self.search.job.is_some() { ui.spinner(); }
        let pal = style::pal();
        let n: usize = self.search.results.iter().map(|(_, h)| h.len()).sum();
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let t = if self.search.truncated { crate::tr_format!("{n}+ Treffer", "{n}+ matches") } else { crate::tr_format!("{n} Treffer in {} Dateien", "{n} matches in {} files", self.search.results.len()) };
            ui.label(RichText::new(t).small().color(pal.text_weak));
        });
        let project = self.project.clone().unwrap_or_default();
        crate::tabs::page_keys(ui);
        egui::ScrollArea::vertical().id_salt("searchres").auto_shrink([false, false]).show(ui, |ui| {
            for (f, hits) in &self.search.results {
                let rel = f.strip_prefix(&project).unwrap_or(f).to_string_lossy().replace('\\', "/");
                egui::CollapsingHeader::new(RichText::new(format!("{rel}  ({})", hits.len())).strong()).id_salt(("sr", f)).default_open(true).show(ui, |ui| {
                    for (line, text) in hits {
                        let r = ui.add(egui::Label::new(RichText::new(format!("{:>4}  {text}", line + 1)).monospace().small()).truncate().sense(Sense::click()));
                        if r.hovered() {
                            ui.painter().rect_filled(r.rect.expand2(vec2(3.0, 1.0)), 5.0, pal.hover.gamma_multiply(0.5));
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
        section_header(ui, crate::i18n::text("VERLAUF"), |ui| {
            if icons::button(ui, Icon::Trash, crate::i18n::text("Verlauf leeren")).clicked() {
                clear = true;
            }
        });
        if clear {
            crate::qhistory::clear();
            self.history.clear();
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.add(crate::widgets::edit(&mut self.history_filter).hint_text(crate::i18n::text("Filtern")).desired_width(f32::INFINITY));
        });
        let pal = style::pal();
        let f = self.history_filter.to_lowercase();
        let items: Vec<&crate::qhistory::Entry> = self
            .history
            .iter()
            .filter(|e| f.is_empty() || e.sql.to_lowercase().contains(&f) || e.db.to_lowercase().contains(&f))
            .collect();
        let row_h = 38.0;
        crate::tabs::page_keys(ui);
        egui::ScrollArea::vertical().id_salt("history").auto_shrink([false, false]).show_rows(ui, row_h, items.len(), |ui, range| {
            for e in &items[range] {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r.shrink2(vec2(4.0, 1.0)), 6.0, pal.hover);
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
                    crate::style::menu_item_look(ui);
                    if icons::menu_item(ui, Some(Icon::External), crate::i18n::text("In neuer Abfrage öffnen"), "").clicked() {
                        actions.push(Action::OpenSql { db: Some(e.db.clone()), sql: e.sql.clone() + ";\n", run: false });
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Play), crate::i18n::text("Erneut ausführen"), "").clicked() {
                        actions.push(Action::OpenSql { db: Some(e.db.clone()), sql: e.sql.clone() + ";\n", run: true });
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Copy), crate::i18n::text("Kopieren"), "").clicked() {
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
        section_header(ui, crate::i18n::text("DATENBANKEN"), |ui| {
            if icons::button(ui, Icon::Refresh, crate::i18n::text("Aktualisieren")).clicked() {
                refresh = true;
            }
            if icons::button(ui, Icon::Plus, crate::i18n::text("Neue Datenbank")).clicked() {
                new_db = true;
            }
        });
        if refresh {
            actions.push(Action::RefreshAll);
        }
        if new_db {
            self.dialogs.push(Dialog::NewDatabase { name: String::new(), collation: COLLATIONS[0].into() });
        }
        // Gleiche Raster wie der Dateibaum: Zeilen 22 px, Pfeil, Symbol und Text auf denselben Positionen
        egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.spacing_mut().interact_size.y = TREE_ROW;
            self.db_tree(ui, actions);
        });
    }

    fn db_tree(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(dbc) = &self.db else {
            ui.label(RichText::new(crate::i18n::text("Nicht verbunden")).color(style::pal().text_weak));
            if ui.button(crate::i18n::text("Verbindung …")).clicked() {
                actions.push(Action::OpenSettings(Some("verbindung".into())));
            }
            return;
        };
        let show_sys = self.settings.show_system_dbs;
        let conn = self.settings.conn.clone();
        let mut dialogs = Vec::new();
        let pal = style::pal();
        crate::tabs::page_keys(ui);
        egui::ScrollArea::both().id_salt("tree").auto_shrink([false, false]).show(ui, |ui| {
            for d in &self.databases {
                let sys = db::SYSTEM_DATABASES.contains(&d.as_str());
                if sys && !show_sys {
                    continue;
                }
                let title = if *d == self.current_db { RichText::new(d).strong() } else { RichText::new(d) };
                let (hr, _) = tree_node(ui, ui.make_persistent_id(("db", d)), Icon::Database, pal.accent, title, |ui| {
                    let schema = self.schemas.get_arc(Some(dbc), d).cloned();
                    let loading = self.schemas.is_loading(d);
                    match schema {
                        Some(schema) => {
                            // Bei tausenden Tabellen nur die sichtbaren Zeilen aufbauen: zugeklappte Zeilen
                            // ausserhalb des Fensters werden durch gleich hohen Leerraum ersetzt.
                            let clip = ui.clip_rect();
                            let mut closed_h: Option<f32> = None;
                            for t in &schema.tables {
                                if let Some(h) = closed_h {
                                    let id = ui.make_persistent_id(("t", d, &t.name));
                                    let open = egui::collapsing_header::CollapsingState::load(ui.ctx(), id).is_some_and(|s| s.is_open());
                                    let top = ui.cursor().min.y;
                                    if !open && (top + h < clip.top() || top > clip.bottom()) {
                                        ui.add_space(h);
                                        continue;
                                    }
                                }
                                let label = if t.is_view { RichText::new(&t.name).italics() } else { RichText::new(&t.name) };
                                let (icon, tint) = if t.is_view { (Icon::Layers, pal.syn_type) } else { (Icon::Table, pal.text_weak) };
                                let (hr, body) = tree_node(ui, ui.make_persistent_id(("t", d, &t.name)), icon, tint, label, |ui| {
                                    for c in &t.columns {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                            if c.is_pk() {
                                                icons::paint(ui.painter(), r, Icon::Key, Color32::from_rgb(0xD9, 0xA4, 0x1E));
                                            } else if schema.is_fk_column(&t.name, &c.name) {
                                                icons::paint(ui.painter(), r, Icon::Link, Color32::from_rgb(0x4F, 0x8F, 0xE0));
                                            }
                                            ui.label(RichText::new(&c.name).small());
                                            ui.label(RichText::new(&c.col_type).small().color(pal.text_weak));
                                        });
                                    }
                                });
                                if body.is_none() {
                                    closed_h = Some(TREE_ROW + ui.spacing().item_spacing.y);
                                }
                                if hr.clicked() {
                                    self.current_db = d.clone();
                                }
                                if hr.double_clicked() {
                                    actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                                }
                                hr.context_menu(|ui| {
                                    crate::style::menu_item_look(ui);
                                    if icons::menu_item(ui, Some(Icon::Table), crate::i18n::text("Daten"), "").clicked() {
                                        actions.push(Action::OpenData { db: d.clone(), table: t.name.clone() });
                                        ui.close();
                                    }
                                    if icons::menu_item(ui, Some(Icon::Columns), crate::i18n::text("Struktur"), "").clicked() {
                                        actions.push(Action::OpenStructure { db: d.clone(), table: t.name.clone() });
                                        ui.close();
                                    }
                                    if icons::menu_item(ui, Some(Icon::Play), "SELECT … LIMIT 100", "").clicked() {
                                        actions.push(Action::OpenSql { db: Some(d.clone()), sql: format!("SELECT * FROM {} LIMIT 100;", q(&t.name)), run: true });
                                        ui.close();
                                    }
                                    if icons::menu_item(ui, Some(Icon::Copy), crate::i18n::text("Name kopieren"), "").clicked() {
                                        ui.ctx().copy_text(t.name.clone());
                                        ui.close();
                                    }
                                    if icons::menu_item(ui, Some(Icon::Coffee), crate::i18n::text("Java-Klasse kopieren"), "").on_hover_text(crate::i18n::text("Record und DAO-Klasse (JDBC) für diese Tabelle")).clicked() {
                                        ui.ctx().copy_text(crate::javagen::table_class(t, &crate::javagen::Target::new(&conn, d)));
                                        actions.push(Action::Status(crate::i18n::text("Java-Code kopiert.").into()));
                                        ui.close();
                                    }
                                    if icons::menu_item(ui, Some(Icon::Download), crate::i18n::text("Exportieren …"), "").clicked() {
                                        actions.push(Action::OpenExport { db: d.clone(), only: Some(vec![t.name.clone()]), format: crate::transfer::export::Format::Csv });
                                        ui.close();
                                    }
                                    ui.separator();
                                    if !t.is_view {
                                        if icons::menu_item(ui, Some(Icon::Edit), crate::i18n::tr("Umbenennen …", "Rename …"), "").clicked() {
                                            dialogs.push(Dialog::TableOp { db: d.clone(), table: t.name.clone(), copy: false, with_data: true, name: t.name.clone() });
                                            ui.close();
                                        }
                                        if icons::menu_item(ui, Some(Icon::Copy), crate::i18n::tr("Kopieren …", "Copy …"), "").clicked() {
                                            dialogs.push(Dialog::TableOp { db: d.clone(), table: t.name.clone(), copy: true, with_data: true, name: format!("{}_kopie", t.name) });
                                            ui.close();
                                        }
                                        if icons::menu_item(ui, Some(Icon::GitMerge), crate::i18n::tr("Zusammenfügen …", "Merge …"), "").clicked() {
                                            let other = schema.tables.iter().find(|x| !x.is_view && x.name != t.name).map(|x| x.name.clone()).unwrap_or_default();
                                            dialogs.push(Dialog::Merge { db: d.clone(), a: t.name.clone(), b: other, mode: crate::app::dialogs::MergeKind::Rows, left_col: String::new(), right_col: String::new(), keep_all_a: true, name: format!("{}_zusammen", t.name) });
                                            ui.close();
                                        }
                                        ui.separator();
                                    }
                                    if !t.is_view && icons::menu_item_colored(ui, Some((Icon::Trash, Some(style::pal().error_text))), crate::i18n::text("Leeren …"), "").clicked() {
                                        actions.push(Action::Confirm {
                                            text: crate::tr_format!("Alle Zeilen der Tabelle „{}“ löschen?", "Delete all rows in table “{}”?", t.name),
                                            db: Some(d.clone()),
                                            sql: format!("DELETE FROM {}", q(&t.name)),
                                        });
                                        ui.close();
                                    }
                                    if icons::menu_item_colored(ui, Some((Icon::Trash, Some(style::pal().error_text))), crate::i18n::text("Löschen …"), "").clicked() {
                                        actions.push(Action::Confirm {
                                            text: crate::tr_format!("„{}“ mit allen Daten löschen?", "Delete “{}” and all its data?", t.name),
                                            db: Some(d.clone()),
                                            sql: format!("DROP {} {}", if t.is_view { "VIEW" } else { "TABLE" }, q(&t.name)),
                                        });
                                        ui.close();
                                    }
                                });
                            }
                        }
                        None if loading => {
                            ui.label(RichText::new(crate::i18n::text("Wird gelesen …")).color(pal.text_weak));
                        }
                        None => {
                            ui.label(RichText::new(crate::i18n::text("kein Zugriff")).color(pal.error_text));
                        }
                    }
                });
                if hr.clicked() {
                    self.current_db = d.clone();
                }
                hr.context_menu(|ui| {
                    crate::style::menu_item_look(ui);
                    if icons::menu_item(ui, Some(Icon::Plus), crate::i18n::text("Neue Abfrage"), "").clicked() {
                        actions.push(Action::OpenSql { db: Some(d.clone()), sql: String::new(), run: false });
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Table), crate::i18n::text("Neue Tabelle …"), "").clicked() {
                        actions.push(Action::NewTable(d.clone()));
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Diagram), crate::i18n::text("ER-Diagramm"), "").clicked() {
                        actions.push(Action::OpenEr(d.clone()));
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Wizard), crate::i18n::text("Abfrage-Assistent"), "").clicked() {
                        actions.push(Action::OpenBuilder(Some(d.clone())));
                        ui.close();
                    }
                    ui.separator();
                    if icons::menu_item(ui, Some(Icon::Download), crate::i18n::text("Exportieren …"), "").clicked() {
                        actions.push(Action::OpenExport { db: d.clone(), only: None, format: crate::transfer::export::Format::Sql });
                        ui.close();
                    }
                    if icons::menu_item(ui, Some(Icon::Upload), crate::i18n::text("Importieren …"), "").clicked() {
                        actions.push(Action::OpenImport);
                        ui.close();
                    }
                    if !sys && icons::menu_item_colored(ui, Some((Icon::Trash, Some(style::pal().error_text))), crate::i18n::text("Löschen …"), "").clicked() {
                        actions.push(Action::Confirm {
                            text: crate::tr_format!("Datenbank „{d}“ mit allen Tabellen und Daten löschen?", "Delete database “{d}” and all its tables and data?"),
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

    /// Dateien im Explorer ablegen: von aussen kopieren, innerhalb verschieben.
    fn apply_drop(&mut self, dir: &Path, srcs: &[PathBuf], mv: bool) {
        let mut copied: Vec<PathBuf> = Vec::new();
        for src in srcs {
            let res = if mv {
                workspace::move_into(src, dir).map(|new| {
                    if let Some(new) = new {
                        for t in self.tabs.iter_mut() {
                            t.file_renamed(src, &new);
                        }
                    }
                })
            } else {
                workspace::copy_into(src, dir).map(|new| copied.push(new))
            };
            if let Err(e) = res {
                self.error(e);
                break;
            }
        }
        self.tree_scan = None;
        // Eine von aussen abgelegte Datei gleich zeigen (Bild, PDF, Text)
        if let [one] = copied.as_slice()
            && one.is_file()
        {
            self.open_file(one);
        }
    }

    /// Von aussen auf das Fenster gezogene Dateien (nicht im Explorer abgelegt): im Editor
    /// oeffnen, ein Ordner wird zum Projekt – wie in VS Code.
    pub(super) fn open_dropped(&mut self, files: Vec<PathBuf>) {
        for f in files {
            if self.tabs.get(self.active).is_some_and(|t| t.takes_drop(&f)) {
                continue;
            }
            if f.is_dir() {
                self.switch_project(Some(f));
            } else if f.is_file() {
                self.open_file(&f);
            }
        }
    }

    // ------------------------------------------------------------------
    // Dialoge des Explorers

    pub(super) fn explorer_dialogs(&mut self, ctx: &egui::Context) {
        if matches!(self.explorer_dlg, Some(ExplorerDlg::NewFile(_))) {
            self.new_file_dialog(ctx);
            return;
        }
        let Some(dlg) = self.explorer_dlg.as_mut() else { return };
        let mut ok = false;
        let mut cancel = false;
        let title = match dlg {
            ExplorerDlg::NewFile(_) => crate::i18n::text("Neue Datei"),
            ExplorerDlg::NewFolder { .. } => crate::i18n::text("Neuer Ordner"),
            ExplorerDlg::Rename { .. } => crate::i18n::text("Umbenennen"),
            ExplorerDlg::Delete { .. } => crate::i18n::text("Löschen"),
            ExplorerDlg::NewProject { .. } => crate::i18n::text("Neues Projekt"),
        };
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18));
        let danger = matches!(dlg, ExplorerDlg::Delete { .. });
        egui::Modal::new(egui::Id::new("explorer-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_min_width(360.0);
            let pal = style::pal();
            ui.horizontal(|ui| {
                let icon = match dlg {
                    ExplorerDlg::Delete { .. } => Icon::Trash,
                    ExplorerDlg::Rename { .. } => Icon::Edit,
                    _ => Icon::NewFolder,
                };
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, icon, if danger { pal.error_text } else { pal.accent });
                ui.label(RichText::new(title).strong().size(15.0));
            });
            ui.add_space(10.0);
            match dlg {
                ExplorerDlg::NewFile(_) => {}
                ExplorerDlg::NewFolder { name, .. } | ExplorerDlg::Rename { name, .. } | ExplorerDlg::NewProject { name } => {
                    crate::tabs::name_field(ui, egui::Id::new("explorer-dlg-name"), name, 300.0, true);
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    if matches!(dlg, ExplorerDlg::NewProject { .. }) {
                        let root = workspace::storage_root();
                        ui.label(RichText::new(root.display().to_string()).small().color(style::pal().text_weak));
                    }
                }
                ExplorerDlg::Delete { paths } => {
                    let what = count_label(paths);
                    ui.label(crate::tr_format!("{what} in den Papierkorb des Projekts verschieben?", "Move {what} to the project's recycle bin?"));
                    ui.label(RichText::new(crate::i18n::tr("Der Papierkorb liegt im Projekt unter .papierkorb.", "The recycle bin is the .papierkorb folder in the project.")).small().color(pal.text_weak));
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                }
            }
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok_text = if danger { crate::i18n::text("Löschen") } else { "OK" };
                let fill = if danger { pal.error_text } else { pal.accent };
                if ui.add(egui::Button::new(RichText::new(ok_text).color(Color32::WHITE)).fill(fill).min_size(vec2(84.0, 28.0))).clicked() {
                    ok = true;
                }
                if ui.add(egui::Button::new(crate::i18n::text("Abbrechen")).min_size(vec2(84.0, 28.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
            ExplorerDlg::NewFile(_) => Ok(None),
            ExplorerDlg::NewFolder { dir, name } => workspace::create_folder(&dir, &name).map(|_| None),
            ExplorerDlg::NewProject { name } => {
                let root = workspace::storage_root();
                if root.join(name.trim()).exists() {
                    Err(crate::tr_format!("„{}“ gibt es schon.", "“{}” already exists.", name.trim()))
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
            ExplorerDlg::Delete { paths } => {
                // Offene Registerkarten dieser Datei(en) schliessen
                let mut i = 0;
                while i < self.tabs.len() {
                    if self.tabs[i].file().is_some_and(|f| paths.iter().any(|p| f.starts_with(p))) {
                        self.tabs.remove(i);
                    } else {
                        i += 1;
                    }
                }
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
                self.explorer.sel.retain(|s| !paths.contains(s));
                match &self.project {
                    // Ordner vor ihrem Inhalt: Eintraege innerhalb eines geloeschten Ordners sind schon weg
                    Some(p) => paths.iter().filter(|x| !paths.iter().any(|o| o != *x && x.starts_with(o))).try_for_each(|x| workspace::trash(p, x).map(|_| ())).map(|_| None),
                    None => Err(crate::i18n::text("Kein Projekt geöffnet.").into()),
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

/// Hoehe einer Baumzeile
const TREE_ROW: f32 = 20.0;

/// Knoten eines Baums: Pfeil, Symbol und Text. Liefert die Zeile zum Klicken (und fuer das Kontextmenue)
/// und den Inhalt, wenn der Knoten aufgeklappt ist.
fn tree_node<R>(ui: &mut egui::Ui, id: egui::Id, icon: Icon, tint: Color32, label: RichText, add_body: impl FnOnce(&mut egui::Ui) -> R) -> (egui::Response, Option<R>) {
    let pal = style::pal();
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let weak = pal.text_weak;
    // Hintergrund der Zeile liegt hinter dem Inhalt: Platz reservieren, nach dem Zeichnen fuellen
    let bg = ui.painter().add(egui::Shape::Noop);
    let sel_key = egui::Id::new("tree-selected");
    let selected = ui.ctx().data(|d| d.get_temp::<egui::Id>(sel_key)) == Some(id);
    let row = ui.horizontal(|ui| {
        ui.set_min_height(TREE_ROW);
        ui.spacing_mut().item_spacing.x = 4.0;
        // Der Aufklappknopf nimmt sonst die volle Hoehe der Bedienelemente (28 px)
        ui.spacing_mut().interact_size.y = TREE_ROW;
        state.show_toggle_button(ui, move |ui, openness, resp| {
            let r = Rect::from_center_size(resp.rect.center(), vec2(14.0, 14.0));
            icons::paint(ui.painter(), r, if openness > 0.5 { Icon::ChevronDown } else { Icon::ChevronRight }, weak);
        });
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, tint);
        ui.add_space(2.0);
        ui.add(egui::Label::new(label).sense(Sense::click()).selectable(false))
    });
    let label_resp = row.inner;
    if label_resp.clicked() || label_resp.secondary_clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(sel_key, id));
    }
    // Ausgewaehlte Zeile in der Auswahlfarbe, Zeile unter dem Mauszeiger dezent heller (ueber die ganze Breite)
    // Ueber die ganze Breite der Seitenleiste, auch bei eingerueckten Eintraegen (wie in IntelliJ IDEA)
    let full = egui::Rect::from_min_max(egui::pos2(ui.clip_rect().left() + 8.0, row.response.rect.top() - 1.0), egui::pos2(ui.clip_rect().right() - 8.0, row.response.rect.bottom() + 1.0));
    if selected || ui.rect_contains_pointer(row.response.rect) {
        let fill = if selected { pal.selection } else { pal.hover };
        ui.painter().set(bg, egui::epaint::RectShape::filled(full, 5.0, fill));
    }
    if label_resp.clicked() {
        state.toggle(ui);
        state.store(ui.ctx());
    }
    let body = state.show_body_indented(&row.response, ui, add_body);
    (label_resp, body.map(|b| b.inner))
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

/// Waehrend des Ziehens: Name der gezogenen Datei neben dem Mauszeiger
fn drag_ghost(ui: &egui::Ui, name: &str) {
    if let Some(p) = ui.ctx().pointer_latest_pos() {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("explorer-ghost")));
        let pal = style::pal();
        let galley = painter.layout_no_wrap(name.to_string(), egui::FontId::proportional(12.5), pal.text);
        let r = Rect::from_min_size(p + vec2(12.0, 8.0), galley.size() + vec2(14.0, 8.0));
        painter.rect(r, 5.0, pal.face_light, egui::Stroke::new(1.0, pal.accent), egui::StrokeKind::Inside);
        painter.galley(r.min + vec2(7.0, 4.0), galley, pal.text);
    }
}

/// "3 Elemente" bzw. der Name
fn count_label(paths: &[PathBuf]) -> String {
    match paths {
        [one] => format!("„{}“", workspace::dir_name(one)),
        _ => crate::tr_format!("{} Elemente", "{} items", paths.len()),
    }
}

/// Untermenue "Neu": Datei, Ordner, Dateiarten, Datenbank (wie "New" in IntelliJ IDEA)
fn new_menu(ui: &mut egui::Ui, dir: &Path, connected: bool, cmd: &mut Option<TreeCmd>) {
    use super::create::FileKind;
    ui.set_min_width(220.0);
    icons::menu_heading(ui, &crate::tr_format!("Neu in {}", "New in {}", workspace::dir_name(dir)));
    if icons::menu_item(ui, Some(Icon::NewFile), crate::i18n::text("Datei …"), &keymap::text(Cmd::NewFile)).clicked() {
        *cmd = Some(TreeCmd::NewFile(dir.to_path_buf(), None));
        ui.close();
    }
    if icons::menu_item(ui, Some(Icon::NewFolder), crate::i18n::text("Ordner …"), "").clicked() {
        *cmd = Some(TreeCmd::NewFolder(dir.to_path_buf()));
        ui.close();
    }
    ui.separator();
    for k in FileKind::MENU {
        let r = menu_file_item(ui, &format!("x.{}", k.ext()), k.label());
        if r.clicked() {
            *cmd = Some(TreeCmd::NewFile(dir.to_path_buf(), Some(k)));
            ui.close();
        }
    }
    ui.separator();
    if ui.add_enabled_ui(connected, |ui| icons::menu_item_colored(ui, Some((Icon::Database, Some(style::pal().accent))), crate::i18n::text("Datenbank …"), "")).inner.on_disabled_hover_text(crate::i18n::text("Nicht verbunden")).clicked() {
        *cmd = Some(TreeCmd::NewDatabase);
        ui.close();
    }
}

/// Menueeintrag mit dem Dateisymbol einer Art
fn menu_file_item(ui: &mut egui::Ui, sample: &str, text: &str) -> egui::Response {
    let r = icons::menu_item(ui, None, text, "");
    icons::paint_file(ui.painter(), Rect::from_center_size(pos2(r.rect.left() + 17.0, r.rect.center().y), vec2(15.0, 15.0)), sample);
    r
}

/// Tasten im Explorer: Strg+C/X/V, Entf, F2, Enter, Pfeile, Strg+A, Esc
fn tree_keys(ui: &egui::Ui, ex: &mut ExplorerState, order: &[(PathBuf, bool)], root: &Path) -> Option<TreeCmd> {
    let events = ui.input(|i| i.events.clone());
    let sel = ex.sel.clone();
    let dir = ex.target_dir(root);
    for e in events {
        match e {
            egui::Event::Copy => return Some(TreeCmd::Copy(sel)),
            egui::Event::Cut => return Some(TreeCmd::Cut(sel)),
            egui::Event::Paste(_) => return Some(TreeCmd::Paste(dir)),
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                egui::Key::Delete => return Some(TreeCmd::Delete(sel)),
                egui::Key::Backspace if modifiers.command => return Some(TreeCmd::Delete(sel)),
                egui::Key::F2 => return sel.last().cloned().map(TreeCmd::Rename),
                egui::Key::Enter => return Some(TreeCmd::Open(sel)),
                egui::Key::Escape => {
                    ex.clip = None;
                    ex.sel.clear();
                }
                egui::Key::A if modifiers.command => ex.sel = order.iter().map(|(p, _)| p.clone()).collect(),
                egui::Key::ArrowUp | egui::Key::ArrowDown => {
                    let cur = sel.last().and_then(|p| order.iter().position(|(q, _)| q == p));
                    let next = match (cur, key) {
                        (None, _) => 0,
                        (Some(i), egui::Key::ArrowUp) => i.saturating_sub(1),
                        (Some(i), _) => (i + 1).min(order.len().saturating_sub(1)),
                    };
                    if let Some((p, _)) = order.get(next) {
                        if modifiers.shift {
                            if !ex.sel.contains(p) {
                                ex.sel.push(p.clone());
                            }
                        } else {
                            ex.sel = vec![p.clone()];
                            ex.anchor = Some(p.clone());
                        }
                        ui.scroll_to_rect(Rect::NOTHING, None);
                    }
                }
                egui::Key::ArrowLeft | egui::Key::ArrowRight => {
                    if let Some(p) = sel.last().filter(|p| p.is_dir()) {
                        let id = egui::Id::new(("dir", p));
                        let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                        st.set_open(key == egui::Key::ArrowRight);
                        st.store(ui.ctx());
                    } else if key == egui::Key::ArrowLeft && let Some(parent) = sel.last().and_then(|p| p.parent()).filter(|d| *d != root) {
                        ex.sel = vec![parent.to_path_buf()];
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    None
}

/// Eintrag im Dateibaum zeichnen (rekursiv).
fn draw_node(ui: &mut egui::Ui, n: &Node, depth: usize, io: &mut TreeIo) {
    let pal = style::pal();
    let indent = 8.0 + depth as f32 * 16.0;
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TREE_ROW + 2.0), Sense::click_and_drag());
    io.order.push((n.path.clone(), n.is_dir));
    let visible = ui.is_rect_visible(r);
    resp.dnd_set_drag_payload(DragPath(n.path.clone()));
    if resp.dragged() {
        let label = if io.sel.contains(&n.path) && io.sel.len() > 1 { crate::tr_format!("{} Elemente", "{} items", io.sel.len()) } else { n.name.clone() };
        drag_ghost(ui, &label);
    }
    // Auf einer Datei abgelegt = in deren Ordner
    let drop_dir = if n.is_dir { Some(n.path.as_path()) } else { n.path.parent() };
    if let Some(dir) = drop_dir {
        io.target(ui, &resp, r, dir);
    }
    let id = egui::Id::new(("dir", &n.path));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    if visible {
        let row = r.shrink2(vec2(4.0, 0.0));
        let selected = io.sel.contains(&n.path);
        if selected {
            let fill = if io.focused { pal.selection } else { pal.selection.gamma_multiply(0.55) };
            ui.painter().rect_filled(row, 5.0, fill);
        } else if resp.hovered() {
            ui.painter().rect_filled(row, 5.0, pal.hover);
        }
        let cy = r.center().y;
        // Hilfslinien der Einrueckung (wie in VS Code)
        for d in 0..depth {
            let x = r.left() + 8.0 + d as f32 * 16.0 + 6.0;
            ui.painter().vline(x, r.y_range(), egui::Stroke::new(1.0, pal.border.gamma_multiply(0.6)));
        }
        if n.is_dir {
            let chev = Rect::from_center_size(pos2(r.left() + indent + 6.0, cy), vec2(14.0, 14.0));
            icons::paint(ui.painter(), chev, if state.is_open() { Icon::ChevronDown } else { Icon::ChevronRight }, pal.text_weak);
            let folder = Rect::from_center_size(pos2(r.left() + indent + 24.0, cy), vec2(16.0, 16.0));
            icons::paint(ui.painter(), folder, if state.is_open() { Icon::FolderOpen } else { Icon::Folder }, icons::folder_color(&n.name));
        } else {
            icons::paint_file(ui.painter(), Rect::from_center_size(pos2(r.left() + indent + 24.0, cy), vec2(16.0, 16.0)), &n.name);
        }
        let cut = io.cut.contains(&n.path);
        let color = if cut { pal.text_weak } else if selected && pal.dark { Color32::WHITE } else { pal.text };
        let painter = ui.painter_at(row);
        painter.text(pos2(r.left() + indent + 38.0, cy), egui::Align2::LEFT_CENTER, &n.name, egui::FontId::proportional(13.0), color);
    }
    if resp.clicked() {
        io.click = Some((n.path.clone(), n.is_dir, ui.input(|i| i.modifiers)));
        let plain = !ui.input(|i| i.modifiers.command || i.modifiers.shift);
        if n.is_dir && plain {
            state.toggle(ui);
        }
    }
    if resp.double_clicked() && !n.is_dir {
        io.cmd = Some(TreeCmd::Open(vec![n.path.clone()]));
    }
    if resp.secondary_clicked() {
        io.right = Some(n.path.clone());
    }
    state.store(ui.ctx());
    item_menu(&resp, n, io);
    if n.is_dir && state.is_open() {
        for c in &n.children {
            draw_node(ui, c, depth + 1, io);
        }
    }
}

/// Kontextmenue einer Datei bzw. eines Ordners. Die Befehle gelten fuer die ganze Auswahl, wenn die Zeile dazugehoert.
fn item_menu(resp: &egui::Response, n: &Node, io: &mut TreeIo) {
    let targets: Vec<PathBuf> = if io.sel.contains(&n.path) { io.sel.to_vec() } else { vec![n.path.clone()] };
    let dir = if n.is_dir { n.path.clone() } else { n.path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let (connected, can_paste) = (io.connected, io.can_paste);
    let cmd = &mut io.cmd;
    resp.context_menu(|ui| {
        crate::style::menu_item_look(ui);
        ui.set_min_width(240.0);
        icons::submenu(ui, Some(Icon::Plus), crate::i18n::tr("Neu", "New"), |ui| new_menu(ui, &dir, connected, cmd));
        if !n.is_dir && icons::menu_item(ui, Some(Icon::External), crate::i18n::text("Öffnen"), "Enter").clicked() {
            *cmd = Some(TreeCmd::Open(targets.clone()));
            ui.close();
        }
        ui.separator();
        if icons::menu_item(ui, Some(Icon::Scissors), crate::i18n::tr("Ausschneiden", "Cut"), "Strg+X").clicked() {
            *cmd = Some(TreeCmd::Cut(targets.clone()));
            ui.close();
        }
        if icons::menu_item(ui, Some(Icon::Copy), crate::i18n::text("Kopieren"), "Strg+C").clicked() {
            *cmd = Some(TreeCmd::Copy(targets.clone()));
            ui.close();
        }
        if ui.add_enabled_ui(can_paste, |ui| icons::menu_item(ui, Some(Icon::Clipboard), crate::i18n::tr("Einfügen", "Paste"), "Strg+V")).inner.clicked() {
            *cmd = Some(TreeCmd::Paste(dir.clone()));
            ui.close();
        }
        ui.separator();
        if ui.add_enabled_ui(targets.len() == 1, |ui| icons::menu_item(ui, Some(Icon::Edit), crate::i18n::text("Umbenennen …"), "F2")).inner.clicked() {
            *cmd = Some(TreeCmd::Rename(n.path.clone()));
            ui.close();
        }
        let del = if targets.len() > 1 { crate::tr_format!("{} Elemente löschen", "Delete {} items", targets.len()) } else { crate::i18n::text("Löschen").to_string() };
        if icons::menu_item_colored(ui, Some((Icon::Trash, Some(style::pal().error_text))), &del, "Entf").clicked() {
            *cmd = Some(TreeCmd::Delete(targets.clone()));
            ui.close();
        }
        ui.separator();
        if icons::menu_item(ui, Some(Icon::Link), crate::i18n::text("Pfad kopieren"), "").clicked() {
            *cmd = Some(TreeCmd::CopyPath(n.path.clone(), false));
            ui.close();
        }
        if icons::menu_item(ui, None, crate::i18n::tr("Relativen Pfad kopieren", "Copy relative path"), "").clicked() {
            *cmd = Some(TreeCmd::CopyPath(n.path.clone(), true));
            ui.close();
        }
        if icons::menu_item(ui, Some(Icon::FolderInput), crate::i18n::text("Im Datei-Explorer zeigen"), "").clicked() {
            *cmd = Some(TreeCmd::Reveal(n.path.clone()));
            ui.close();
        }
        if icons::menu_item(ui, Some(Icon::Terminal), crate::i18n::tr("Terminal hier öffnen", "Open terminal here"), "").clicked() {
            *cmd = Some(TreeCmd::Terminal(dir.clone()));
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_search_discards_results_and_stops_before_file_reads() {
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        let files = vec![PathBuf::from("/missing/file.sql")];
        assert_eq!(search_files_cancellable(&files, "SELECT", false, &cancelled), (vec![], false));
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (_, receiver) = std::sync::mpsc::channel();
        let job = SearchJob { receiver, cancelled: token.clone() };
        drop(job);
        assert!(token.load(std::sync::atomic::Ordering::Relaxed));
    }

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
