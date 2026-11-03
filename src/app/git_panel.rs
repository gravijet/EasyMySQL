// Seitenleiste "Quellcodeverwaltung": Git fuer den Projektordner und GitHub (Anmeldung, Repository
// veroeffentlichen, klonen, Pull Requests, Issues, Actions). Alles, was dauert, laeuft im Hintergrund.

use super::*;
use crate::git::{self, Change, FileStatus, github};
use crate::icons::{self, Icon};
use crate::workspace;
use eframe::egui::{RichText, Sense, vec2};
use std::sync::mpsc::{Receiver, channel};

type Job<T> = Receiver<Result<T, String>>;

/// Daten von GitHub zum Repository des Projekts
#[derive(Clone, Default)]
pub struct GhData {
    pub owner: String,
    pub repo: String,
    pub pulls: Vec<github::Item>,
    pub issues: Vec<github::Item>,
    pub runs: Vec<github::Run>,
    pub default_branch: String,
}

pub enum GitDialog {
    Login { token: String, error: Option<String>, job: Option<Job<github::User>> },
    Clone { url: String, location: PathBuf, repos: Option<Result<Vec<github::Repo>, String>>, list: Option<Job<Vec<github::Repo>>>, filter: String, job: Option<Job<PathBuf>> },
    Publish { name: String, private: bool, description: String, job: Option<Job<String>> },
    PullRequest { title: String, body: String, base: String, draft: bool, job: Option<Job<github::Item>> },
    Issue { title: String, body: String, job: Option<Job<github::Item>> },
    NewBranch { name: String },
    Log { commits: Vec<git::Commit> },
}

#[derive(Default)]
pub struct GitPanel {
    root: Option<PathBuf>,
    status: Option<Result<git::Status, String>>,
    loading: Option<Receiver<(PathBuf, Result<git::Status, String>)>>,
    last_refresh: Option<Instant>,
    last_fetch: Option<Instant>,
    pub message: String,
    amend: bool,
    /// Laufender Auftrag (Push, Pull, …): Beschreibung und Ergebnis
    busy: Option<(String, Job<String>)>,
    note: Option<(String, bool)>,
    user: Option<github::User>,
    user_job: Option<Job<github::User>>,
    gh: Option<GhData>,
    gh_job: Option<Job<GhData>>,
    gh_for: Option<String>,
    pub dialog: Option<GitDialog>,
    branches: Option<(Vec<String>, Vec<String>)>,
    /// Rueckfrage vor dem Verwerfen von Aenderungen
    discard: Option<FileStatus>,
}

fn spawn<T: Send + 'static>(ctx: &egui::Context, f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Job<T> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(f());
        ctx.request_repaint();
    });
    rx
}

fn ctx() -> egui::Context {
    CTX.lock().unwrap().clone().unwrap_or_default()
}

impl EasyApp {
    fn git_root(&self) -> Option<PathBuf> {
        self.project.as_deref().and_then(git::repo_root)
    }

    /// Stand neu lesen (im Hintergrund)
    pub(super) fn git_refresh(&mut self) {
        let Some(root) = self.git_root() else {
            self.git.root = None;
            self.git.status = None;
            return;
        };
        if self.git.loading.is_some() {
            return;
        }
        self.git.root = Some(root.clone());
        self.git.last_refresh = Some(Instant::now());
        let (tx, rx) = channel();
        let c = ctx();
        std::thread::spawn(move || {
            let st = git::status(&root);
            let _ = tx.send((root, st));
            c.request_repaint();
        });
        self.git.loading = Some(rx);
        self.git.branches = None;
    }

    fn git_poll(&mut self) {
        if let Some(rx) = &self.git.loading
            && let Ok((root, st)) = rx.try_recv()
        {
            self.git.loading = None;
            if Some(&root) == self.git_root().as_ref() {
                self.git.status = Some(st);
            }
        }
        let finished = self.git.busy.as_ref().and_then(|(what, rx)| rx.try_recv().ok().map(|r| (what.clone(), r)));
        if let Some((what, r)) = finished {
            self.git.busy = None;
            self.git.note = Some(match r {
                Ok(t) => (if t.trim().is_empty() { crate::tr_format!("{what}: fertig", "{what}: done") } else { t.trim().lines().last().unwrap_or("").to_string() }, false),
                Err(e) => (e, true),
            });
            self.git_refresh();
            self.tree_scan = None;
            self.git.gh_for = None;
        }
        if let Some(rx) = &self.git.user_job
            && let Ok(r) = rx.try_recv()
        {
            self.git.user_job = None;
            if let Ok(u) = r {
                // Name und E-Mail fuer Commits von GitHub, wenn in den Einstellungen nichts steht
                if self.settings.git.user_email.is_empty() {
                    self.settings.git.user_name = u.name.clone();
                    self.settings.git.user_email = u.email.clone();
                    git::set_config(self.settings.git.clone());
                    self.settings.save();
                }
                self.git.user = Some(u);
            }
        }
        if let Some(rx) = &self.git.gh_job
            && let Ok(r) = rx.try_recv()
        {
            self.git.gh_job = None;
            match r {
                Ok(d) => self.git.gh = Some(d),
                Err(e) => self.git.note = Some((e, true)),
            }
        }
    }

    /// Langer Auftrag im Hintergrund (nur einer gleichzeitig)
    fn git_job(&mut self, what: &str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.git.busy.is_some() {
            return;
        }
        self.git.note = None;
        self.git.busy = Some((what.to_string(), spawn(&ctx(), f)));
    }

    pub(super) fn git_status_now(&self) -> Option<git::Status> {
        self.git.status.as_ref().and_then(|s| s.as_ref().ok()).cloned()
    }

    pub(super) fn git_commit(&mut self) {
        let Some(root) = self.git_root() else { return };
        let Some(st) = self.git_status_now() else { return };
        let msg = self.git.message.clone();
        let amend = self.git.amend;
        let cfg = git::config();
        let nothing_staged = !st.files.iter().any(|f| f.staged.is_some());
        if nothing_staged && !amend && !cfg.auto_stage {
            self.git.note = Some((crate::i18n::tr("Nichts vorgemerkt. Mit + Dateien vormerken.", "Nothing staged. Stage files with +.").into(), true));
            return;
        }
        let push = cfg.push_after_commit && git::remote_url(&root).is_some();
        self.git.message.clear();
        self.git.amend = false;
        self.git_job(crate::i18n::tr("Commit", "Commit"), move || {
            if nothing_staged && !amend {
                git::stage_all(&root)?;
            }
            let done = git::commit(&root, &msg, amend)?;
            if push {
                let st = git::status(&root)?;
                git::push(&root, &st, amend)?;
                return Ok(crate::tr_format!("{done} – gepusht", "{done} – pushed"));
            }
            Ok(done)
        });
    }

    pub(super) fn git_push(&mut self) {
        let (Some(root), Some(st)) = (self.git_root(), self.git_status_now()) else { return };
        if git::remote_url(&root).is_none() {
            self.open_publish_dialog();
            return;
        }
        self.git_job("Push", move || git::push(&root, &st, false));
    }

    pub(super) fn git_pull(&mut self) {
        let Some(root) = self.git_root() else { return };
        self.git_job("Pull", move || git::pull(&root));
    }

    fn open_publish_dialog(&mut self) {
        let name = self.project.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().replace(' ', "-")).unwrap_or_default();
        self.git.dialog = Some(GitDialog::Publish { name, private: git::config().private_repos, description: String::new(), job: None });
    }

    pub(super) fn open_clone_dialog(&mut self) {
        let list = github::token().map(|_| spawn(&ctx(), github::repos));
        self.git.dialog = Some(GitDialog::Clone { url: String::new(), location: workspace::storage_root(), repos: None, list, filter: String::new(), job: None });
    }

    /// Aenderungen des Sprachservers (Umbenennen) auf offene Registerkarten und Dateien anwenden
    pub(super) fn apply_workspace_edit(&mut self, edit: crate::dev::lsp::WorkspaceEdit) {
        let mut changed = 0;
        for f in &edit.files {
            let applied = self.tabs.iter_mut().find(|t| t.file().is_some_and(|p| p == f.path)).is_some_and(|t| t.apply_edits(&f.edits));
            match applied {
                true => changed += 1,
                false => match std::fs::read_to_string(&f.path) {
                    Ok(text) => {
                        workspace::snapshot(&f.path, &text);
                        if workspace::atomic_write(&f.path, &crate::dev::lsp::apply_edits(&text, &f.edits)).is_ok() {
                            changed += 1;
                        }
                    }
                    Err(e) => self.toast_error(format!("{}: {e}", f.path.display())),
                },
            }
        }
        for (old, new) in &edit.renames {
            // Offene Datei erst speichern, dann umbenennen
            for t in self.tabs.iter_mut().filter(|t| t.file().is_some_and(|p| p == old)) {
                t.save_now();
            }
            match std::fs::rename(old, new) {
                Ok(()) => {
                    for t in self.tabs.iter_mut() {
                        t.file_renamed(old, new);
                    }
                }
                Err(e) => self.toast_error(format!("{}: {e}", old.display())),
            }
        }
        self.tree_scan = None;
        self.toast(crate::tr_format!("In {changed} Datei(en) umbenannt.", "Renamed in {changed} file(s)."));
    }

    // ------------------------------------------------------------------
    // Seitenleiste

    pub(super) fn git_side(&mut self, ui: &mut egui::Ui, _actions: &mut Vec<Action>) {
        let pal = style::pal();
        self.git_poll();
        let root = self.git_root();
        if root != self.git.root || (self.git.status.is_none() && self.git.loading.is_none() && root.is_some()) {
            self.git.root = root.clone();
            self.git.status = None;
            self.git.gh = None;
            self.git.gh_for = None;
            self.git_refresh();
        }
        // Regelmaessig auffrischen, solange der Bereich sichtbar ist
        if self.settings.git_panel_refresh && self.git.last_refresh.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) && self.git.busy.is_none() {
            self.git_refresh();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(3));
        let cfg = git::config();
        if let Some(root) = &root
            && cfg.auto_fetch_secs > 0
            && self.git.last_fetch.is_none_or(|t| t.elapsed() > Duration::from_secs(cfg.auto_fetch_secs as u64))
            && self.git.busy.is_none()
            && git::remote_url(root).is_some()
        {
            self.git.last_fetch = Some(Instant::now());
            let r = root.clone();
            self.git_job("Fetch", move || git::fetch(&r).map(|_| String::new()));
        }
        if self.git.user.is_none() && self.git.user_job.is_none() && github::token().is_some() {
            self.git.user_job = Some(spawn(&ctx(), github::user));
        }

        super::side::section_header(ui, crate::i18n::tr("QUELLCODEVERWALTUNG", "SOURCE CONTROL"), |ui| {
            if icons::button(ui, Icon::Refresh, crate::i18n::text("Aktualisieren")).clicked() {
                self.git_refresh();
                self.git.gh_for = None;
            }
        });
        egui::ScrollArea::vertical().id_salt("git-side").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 8, top: 0, bottom: 8 }).show(ui, |ui| {
                if crate::git::git_exe().is_none() {
                    ui.label(RichText::new(crate::i18n::tr("Git ist nicht installiert.", "Git is not installed.")).color(pal.warn_text));
                    if cfg!(windows) {
                        if ui.button(crate::i18n::tr("Git herunterladen", "Download Git")).clicked() {
                            crate::dev::install::install(&[crate::dev::Tool::Git], ui.ctx());
                        }
                        let st = crate::dev::install::status();
                        if st.busy {
                            ui.label(RichText::new(crate::dev::install::status_line(&st)).small().color(pal.text_weak));
                        }
                    } else {
                        ui.label(RichText::new(crate::i18n::tr("Bitte über die Paketverwaltung installieren, z. B. sudo apt install git", "Please install it with the package manager, e.g. sudo apt install git")).small().color(pal.text_weak));
                    }
                    return;
                }
                let Some(root) = root.clone() else {
                    match &self.project {
                        None => {
                            ui.label(RichText::new(crate::i18n::tr("Öffne einen Ordner, um Git zu verwenden.", "Open a folder to use Git.")).color(pal.text_weak));
                        }
                        Some(p) => {
                            let p = p.clone();
                            ui.label(RichText::new(crate::i18n::tr("Der Ordner ist noch kein Git-Repository.", "The folder is not a Git repository yet.")).color(pal.text_weak));
                            if crate::widgets::primary(ui, crate::i18n::tr("Repository anlegen", "Initialize repository")).clicked() {
                                self.git_job(crate::i18n::tr("Repository anlegen", "Initialize"), move || git::init(&p).map(|_| String::new()));
                            }
                        }
                    }
                    if ui.button(crate::i18n::tr("Repository klonen …", "Clone repository …")).clicked() {
                        self.open_clone_dialog();
                    }
                    self.github_section(ui, None);
                    return;
                };
                let status = self.git.status.clone();
                match status {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).small().color(pal.error_text));
                    }
                    Some(Ok(st)) => self.git_repo_ui(ui, &root, &st),
                }
                let remote = git::remote_url(&root).and_then(|u| git::github_repo(&u));
                self.github_section(ui, remote);
            });
        });
    }

    fn git_repo_ui(&mut self, ui: &mut egui::Ui, root: &Path, st: &git::Status) {
        let pal = style::pal();
        // Branch, Synchronisieren
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::GitBranch, pal.text_weak);
            let resp = ui.add(egui::Button::new(RichText::new(&st.branch).strong()).frame(false)).on_hover_text(crate::i18n::tr("Branch wechseln", "Switch branch"));
            if resp.clicked() && self.git.branches.is_none() {
                self.git.branches = git::branches(root).ok();
            }
            let mut switch_to: Option<String> = None;
            let mut merge_from: Option<String> = None;
            let mut delete: Option<String> = None;
            egui::Popup::menu(&resp).show(|ui| {
                crate::style::menu_item_look(ui);
                if ui.button(crate::i18n::tr("Neuer Branch …", "New branch …")).clicked() {
                    self.git.dialog = Some(GitDialog::NewBranch { name: String::new() });
                    ui.close();
                }
                ui.separator();
                if let Some((local, remote)) = &self.git.branches {
                    for b in local {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(*b == st.branch, b).clicked() && *b != st.branch {
                                switch_to = Some(b.clone());
                                ui.close();
                            }
                            if *b != st.branch {
                                ui.menu_button("…", |ui| {
                                    if ui.button(crate::tr_format!("In {} zusammenführen", "Merge into {}", st.branch)).clicked() {
                                        merge_from = Some(b.clone());
                                        ui.close();
                                    }
                                    if ui.button(crate::i18n::text("Löschen")).clicked() {
                                        delete = Some(b.clone());
                                        ui.close();
                                    }
                                });
                            }
                        });
                    }
                    if !remote.is_empty() {
                        ui.separator();
                        ui.label(RichText::new(crate::i18n::tr("Entfernt", "Remote")).small().color(pal.text_weak));
                        for b in remote {
                            if ui.button(b).clicked() {
                                switch_to = Some(b.clone());
                                ui.close();
                            }
                        }
                    }
                }
            });
            if let Some(b) = switch_to {
                let r = root.to_path_buf();
                self.git_job(crate::i18n::tr("Branch wechseln", "Switch branch"), move || git::switch(&r, &b, false).map(|_| String::new()));
            }
            if let Some(b) = merge_from {
                let r = root.to_path_buf();
                self.git_job("Merge", move || git::merge(&r, &b));
            }
            if let Some(b) = delete {
                let r = root.to_path_buf();
                self.git_job(crate::i18n::text("Löschen"), move || git::delete_branch(&r, &b, false).map(|_| String::new()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let more = icons::button(ui, Icon::More, crate::i18n::text("Weitere"));
                egui::Popup::menu(&more).show(|ui| {
                    crate::style::menu_item_look(ui);
                    if ui.button("Fetch").clicked() {
                        let r = root.to_path_buf();
                        self.git_job("Fetch", move || git::fetch(&r).map(|_| String::new()));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Push (erzwingen, sicher)", "Push (force with lease)")).clicked() {
                        let (r, s) = (root.to_path_buf(), st.clone());
                        self.git_job("Push", move || git::push(&r, &s, true));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Verlauf …", "History …")).clicked() {
                        if let Ok(commits) = git::log(root, 200) {
                            self.git.dialog = Some(GitDialog::Log { commits });
                        }
                        ui.close();
                    }
                    if ui.checkbox(&mut self.git.amend, crate::i18n::tr("Letzten Commit ändern (amend)", "Amend last commit")).clicked() {
                        ui.close();
                    }
                    if git::remote_url(root).is_none() && ui.button(crate::i18n::tr("Auf GitHub veröffentlichen …", "Publish to GitHub …")).clicked() {
                        self.open_publish_dialog();
                        ui.close();
                    }
                    if let Some(url) = git::remote_url(root) {
                        ui.label(RichText::new(&url).small().monospace().color(pal.text_weak));
                    }
                    ui.separator();
                    if ui.button(crate::i18n::tr("Git-Einstellungen …", "Git settings …")).clicked() {
                        self.handle_actions(vec![Action::OpenSettings(Some("git".into()))]);
                        ui.close();
                    }
                });
                let push_tip = if st.ahead > 0 { crate::tr_format!("Push ({} Commit(s))", "Push ({} commit(s))", st.ahead) } else { "Push".to_string() };
                if icons::button(ui, Icon::CloudUpload, &push_tip).clicked() {
                    self.git_push();
                }
                if icons::button(ui, Icon::CloudDownload, "Pull").clicked() {
                    self.git_pull();
                }
                if st.ahead + st.behind > 0 {
                    ui.label(RichText::new(format!("↓{} ↑{}", st.behind, st.ahead)).small().color(pal.text_weak));
                }
            });
        });
        if let Some((what, _)) = &self.git.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("{what} …")).small().color(pal.text_weak));
            });
        } else if let Some((n, err)) = &self.git.note {
            ui.add(egui::Label::new(RichText::new(n).small().color(if *err { pal.error_text } else { pal.ok_text })).wrap());
        }
        // Commit
        let hint = if self.git.amend { crate::i18n::tr("Beschreibung (leer = unverändert)", "Message (empty = unchanged)").to_string() } else { crate::tr_format!("Beschreibung (Commit auf {})", "Message (commit on {})", st.branch) };
        ui.add(egui::TextEdit::multiline(&mut self.git.message).hint_text(hint).desired_rows(2).desired_width(f32::INFINITY));
        let staged: Vec<FileStatus> = st.files.iter().filter(|f| f.staged.is_some()).cloned().collect();
        let changes: Vec<FileStatus> = st.files.iter().filter(|f| f.unstaged.is_some()).cloned().collect();
        ui.horizontal(|ui| {
            let label = if self.git.amend { crate::i18n::tr("Commit ändern", "Amend") } else if staged.is_empty() && git::config().auto_stage { crate::i18n::tr("Alles committen", "Commit all") } else { "Commit" };
            let can = self.git.busy.is_none() && (self.git.amend || !st.files.is_empty()) && (self.git.amend || !self.git.message.trim().is_empty());
            if ui.add_enabled(can, egui::Button::new(RichText::new(label).color(egui::Color32::WHITE)).fill(pal.accent).min_size(vec2(ui.available_width(), 26.0))).on_hover_text(crate::keymap::text(Cmd::GitCommit)).clicked() {
                self.git_commit();
            }
        });
        let mut act: Option<(&'static str, FileStatus)> = None;
        let mut all: Option<&'static str> = None;
        let mut open_diff: Option<(FileStatus, bool)> = None;
        let section = |ui: &mut egui::Ui, title: String, list: &[FileStatus], staged_list: bool, act: &mut Option<(&'static str, FileStatus)>, all: &mut Option<&'static str>, open_diff: &mut Option<(FileStatus, bool)>| {
            if list.is_empty() {
                return;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).small().strong().color(pal.text_weak));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if staged_list {
                        if icons::button(ui, Icon::Minus, crate::i18n::tr("Alle zurücknehmen", "Unstage all")).clicked() {
                            *all = Some("unstage");
                        }
                    } else if icons::button(ui, Icon::Plus, crate::i18n::tr("Alle vormerken", "Stage all")).clicked() {
                        *all = Some("stage");
                    }
                });
            });
            for f in list {
                let change = if staged_list { f.staged } else { f.unstaged }.unwrap_or(Change::Modified);
                ui.horizontal(|ui| {
                    let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                    let color = match change {
                        Change::Added | Change::Untracked => pal.ok_text,
                        Change::Deleted | Change::Conflict => pal.error_text,
                        _ => pal.warn_text,
                    };
                    ui.label(RichText::new(change.letter()).monospace().small().color(color)).on_hover_text(change.label());
                    let r = ui.add(egui::Label::new(name).truncate().sense(Sense::click())).on_hover_text(&f.path);
                    if r.clicked() {
                        *open_diff = Some((f.clone(), staged_list));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        if staged_list {
                            if icons::button(ui, Icon::Minus, crate::i18n::tr("Zurücknehmen", "Unstage")).clicked() {
                                *act = Some(("unstage", f.clone()));
                            }
                        } else {
                            if icons::button(ui, Icon::Plus, crate::i18n::tr("Vormerken", "Stage")).clicked() {
                                *act = Some(("stage", f.clone()));
                            }
                            if icons::button(ui, Icon::RotateCcw, crate::i18n::tr("Änderungen verwerfen", "Discard changes")).clicked() {
                                *act = Some(("discard", f.clone()));
                            }
                        }
                    });
                });
            }
        };
        section(ui, crate::tr_format!("VORGEMERKT ({})", "STAGED ({})", staged.len()), &staged, true, &mut act, &mut all, &mut open_diff);
        section(ui, crate::tr_format!("ÄNDERUNGEN ({})", "CHANGES ({})", changes.len()), &changes, false, &mut act, &mut all, &mut open_diff);
        if st.files.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Keine Änderungen.", "No changes.")).small().color(pal.text_weak));
        }
        let r = root.to_path_buf();
        let fresh = st.fresh;
        match all {
            Some("stage") => {
                let _ = git::stage_all(&r);
                self.git_refresh();
            }
            Some(_) => {
                let paths: Vec<String> = staged.iter().map(|f| f.path.clone()).collect();
                let _ = git::unstage(&r, &paths, fresh);
                self.git_refresh();
            }
            None => {}
        }
        if let Some((what, f)) = act {
            let res = match what {
                "stage" => git::stage(&r, std::slice::from_ref(&f.path)),
                "unstage" => git::unstage(&r, std::slice::from_ref(&f.path), fresh),
                _ => {
                    self.git.discard = Some(f.clone());
                    Ok(())
                }
            };
            if let Err(e) = res {
                self.git.note = Some((e, true));
            }
            self.git_refresh();
        }
        if let Some(f) = self.git.discard.clone() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(egui::Label::new(crate::tr_format!("Änderungen an „{}“ verwerfen? Das lässt sich nicht rückgängig machen.", "Discard the changes to “{}”? This cannot be undone.", f.path)).wrap());
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Verwerfen", "Discard")).clicked() {
                        if let Err(e) = git::discard(&r, &f) {
                            self.git.note = Some((e, true));
                        }
                        self.git.discard = None;
                        self.git_refresh();
                        self.tree_scan = None;
                    }
                    if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                        self.git.discard = None;
                    }
                });
            });
        }
        if let Some((f, staged)) = open_diff {
            match git::diff(&r, &f, staged) {
                Ok(text) => self.open_tab(Box::new(crate::tabs::diff::DiffTab::new(r.join(&f.path), f.path.clone(), staged, text))),
                Err(e) => self.git.note = Some((e, true)),
            }
        }
    }

    fn github_section(&mut self, ui: &mut egui::Ui, remote: Option<(String, String)>) {
        let pal = style::pal();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Github, pal.text_weak);
            ui.label(RichText::new("GITHUB").small().strong().color(pal.text_weak));
        });
        if github::token().is_none() {
            ui.label(RichText::new(crate::i18n::tr("Anmelden, um Repositorys zu veröffentlichen, zu klonen und Pull Requests zu öffnen.", "Sign in to publish and clone repositories and open pull requests.")).small().color(pal.text_weak));
            if ui.button(crate::i18n::tr("Bei GitHub anmelden …", "Sign in to GitHub …")).clicked() {
                self.git.dialog = Some(GitDialog::Login { token: String::new(), error: None, job: None });
            }
            return;
        }
        match self.git.user.clone() {
            Some(u) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("@{}", u.login)).small());
                    if ui.small_button(crate::i18n::tr("Abmelden", "Sign out")).clicked() {
                        let _ = github::set_token("");
                        self.git.user = None;
                        self.git.gh = None;
                    }
                });
            }
            None if self.git.user_job.is_some() => {
                ui.spinner();
            }
            None => {}
        }
        let Some((owner, repo)) = remote else {
            if self.git_root().is_some() && ui.button(crate::i18n::tr("Auf GitHub veröffentlichen …", "Publish to GitHub …")).clicked() {
                self.open_publish_dialog();
            }
            return;
        };
        let key = format!("{owner}/{repo}");
        if self.git.gh_for.as_deref() != Some(&key) && self.git.gh_job.is_none() {
            self.git.gh_for = Some(key.clone());
            let (o, r) = (owner.clone(), repo.clone());
            self.git.gh_job = Some(spawn(&ctx(), move || {
                let info = github::repo(&o, &r)?;
                Ok(GhData { pulls: github::pulls(&o, &r)?, issues: github::issues(&o, &r).unwrap_or_default(), runs: github::runs(&o, &r).unwrap_or_default(), default_branch: info.default_branch, owner: o, repo: r })
            }));
        }
        ui.horizontal(|ui| {
            if ui.link(&key).on_hover_text(crate::i18n::tr("Im Browser öffnen", "Open in the browser")).clicked() {
                github::open_url(&format!("https://github.com/{key}"));
            }
            if self.git.gh_job.is_some() {
                ui.spinner();
            }
        });
        let Some(d) = self.git.gh.clone() else { return };
        let branch = self.git_status_now().map(|s| s.branch).unwrap_or_default();
        // Pull Requests
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::GitPullRequest, pal.text_weak);
            ui.label(RichText::new(crate::tr_format!("Pull Requests ({})", "Pull requests ({})", d.pulls.len())).small().strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if branch != d.default_branch && icons::button(ui, Icon::Plus, crate::i18n::tr("Pull Request erstellen", "Create pull request")).clicked() {
                    let title = self.git.message.lines().next().unwrap_or("").to_string();
                    let title = if title.is_empty() { git::log(&self.git_root().unwrap_or_default(), 1).ok().and_then(|l| l.first().map(|c| c.subject.clone())).unwrap_or_else(|| branch.clone()) } else { title };
                    self.git.dialog = Some(GitDialog::PullRequest { title, body: String::new(), base: d.default_branch.clone(), draft: false, job: None });
                }
            });
        });
        for p in &d.pulls {
            ui.horizontal(|ui| {
                let label = format!("#{} {}{}", p.number, if p.draft { "[Draft] " } else { "" }, p.title);
                if ui.add(egui::Label::new(RichText::new(label).small()).truncate().sense(Sense::click())).on_hover_text(format!("{} → {} · @{}", p.head, p.base, p.user)).clicked() {
                    github::open_url(&p.html_url);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let m = icons::button(ui, Icon::GitMerge, crate::i18n::tr("Zusammenführen", "Merge"));
                    egui::Popup::menu(&m).show(|ui| {
                        crate::style::menu_item_look(ui);
                        for (method, label) in [("squash", "Squash and merge"), ("merge", "Merge commit"), ("rebase", "Rebase and merge")] {
                            if ui.button(label).clicked() {
                                let (o, r, n) = (d.owner.clone(), d.repo.clone(), p.number);
                                self.git_job("Merge", move || github::merge_pull(&o, &r, n, method).map(|_| crate::tr_format!("#{n} zusammengeführt", "#{n} merged")));
                                ui.close();
                            }
                        }
                    });
                });
            });
        }
        // Issues
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::CircleDot, pal.text_weak);
            ui.label(RichText::new(crate::tr_format!("Issues ({})", "Issues ({})", d.issues.len())).small().strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Plus, crate::i18n::tr("Issue anlegen", "New issue")).clicked() {
                    self.git.dialog = Some(GitDialog::Issue { title: String::new(), body: String::new(), job: None });
                }
            });
        });
        for i in d.issues.iter().take(30) {
            if ui.add(egui::Label::new(RichText::new(format!("#{} {}", i.number, i.title)).small()).truncate().sense(Sense::click())).on_hover_text(format!("@{} · {} {}", i.user, i.comments, crate::i18n::tr("Kommentare", "comments"))).clicked() {
                github::open_url(&i.html_url);
            }
        }
        // Actions
        if !d.runs.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Actions").small().strong());
            for r in d.runs.iter().take(6) {
                let (mark, color) = match (r.status.as_str(), r.conclusion.as_str()) {
                    ("completed", "success") => ("✓", pal.ok_text),
                    ("completed", "failure") | ("completed", "timed_out") => ("✗", pal.error_text),
                    ("completed", _) => ("–", pal.text_weak),
                    _ => ("●", pal.warn_text),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(mark).color(color));
                    if ui.add(egui::Label::new(RichText::new(format!("{} · {}", r.name, r.branch)).small()).truncate().sense(Sense::click())).on_hover_text(&r.when).clicked() {
                        github::open_url(&r.html_url);
                    }
                });
            }
        }
    }

    // ------------------------------------------------------------------
    // Dialoge

    pub(super) fn git_dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.git.dialog.take() else { return };
        let pal = style::pal();
        let mut close = false;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16));
        let title = |ui: &mut egui::Ui, icon: Icon, t: &str| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, icon, pal.accent);
                ui.label(RichText::new(t).strong().size(15.0));
            });
            ui.add_space(8.0);
        };
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut relogin = false;
        egui::Modal::new(egui::Id::new("git-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(520.0);
            match &mut dlg {
                GitDialog::Login { token, error, job } => {
                    title(ui, Icon::Github, crate::i18n::tr("Bei GitHub anmelden", "Sign in to GitHub"));
                    ui.label(crate::i18n::tr("1. Auf GitHub ein persönliches Token anlegen (Rechte repo, workflow, read:user sind schon ausgewählt).", "1. Create a personal access token on GitHub (the scopes repo, workflow, read:user are preselected)."));
                    if ui.button(crate::i18n::tr("Token-Seite öffnen", "Open token page")).clicked() {
                        github::open_url(github::TOKEN_URL);
                    }
                    ui.add_space(4.0);
                    ui.label(crate::i18n::tr("2. Das Token hier einfügen:", "2. Paste the token here:"));
                    ui.add(crate::widgets::edit(token).password(true).desired_width(f32::INFINITY).hint_text("ghp_…"));
                    if let Some(e) = error {
                        ui.label(RichText::new(e.as_str()).small().color(pal.error_text));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::i18n::tr("Oder mit der GitHub CLI im Browser anmelden (wird bei Bedarf geladen):", "Or sign in in the browser with the GitHub CLI (downloaded when needed):")).small().color(pal.text_weak));
                    let gh_ready = crate::dev::tools().gh.is_some();
                    if ui.button(if gh_ready { crate::i18n::tr("Im Browser anmelden (gh auth login)", "Sign in in the browser (gh auth login)") } else { crate::i18n::tr("GitHub CLI laden", "Download GitHub CLI") }).clicked() {
                        if gh_ready {
                            if let Some(gh) = crate::dev::tools().gh.clone() {
                                let launch = crate::dev::process::Launch::new("GitHub", gh.launcher, vec!["auth".into(), "login".into(), "--web".into(), "-h".into(), "github.com".into(), "-p".into(), "https".into(), "-s".into(), "workflow,read:user,user:email".into()], workspace::storage_root());
                                self.handle_actions(vec![Action::RunProgram { launch, env: vec![] }]);
                                github::reload();
                            }
                        } else {
                            crate::dev::install::install(&[crate::dev::Tool::GhCli], ui.ctx());
                        }
                    }
                    if gh_ready && ui.small_button(crate::i18n::tr("Anmeldung der GitHub CLI übernehmen", "Use the GitHub CLI sign-in")).clicked() {
                        github::reload();
                        if github::token().is_some() {
                            self.git.user = None;
                            close = true;
                        } else {
                            *error = Some(crate::i18n::tr("Die GitHub CLI ist noch nicht angemeldet.", "The GitHub CLI is not signed in yet.").into());
                        }
                    }
                    if let Some(rx) = job
                        && let Ok(r) = rx.try_recv()
                    {
                        *job = None;
                        match r {
                            Ok(u) => {
                                self.git.user = Some(u);
                                self.git.gh_for = None;
                                close = true;
                            }
                            Err(e) => {
                                let _ = github::set_token("");
                                *error = Some(e);
                            }
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(job.is_none() && !token.trim().is_empty(), egui::Button::new(crate::i18n::tr("Anmelden", "Sign in"))).clicked() {
                            match github::set_token(token) {
                                Ok(()) => *job = Some(spawn(ui.ctx(), github::user)),
                                Err(e) => *error = Some(e),
                            }
                        }
                        if job.is_some() {
                            ui.spinner();
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                            close = true;
                        }
                    });
                }
                GitDialog::Clone { url, location, repos, list, filter, job } => {
                    title(ui, Icon::Download, crate::i18n::tr("Repository klonen", "Clone repository"));
                    ui.add(crate::widgets::edit(url).hint_text("https://github.com/owner/repo.git").desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("Ziel:", "Into:"));
                        let t = location.join(git::clone_dir_name(url)).display().to_string();
                        ui.add(egui::Label::new(RichText::new(t).small().monospace()).truncate());
                        if ui.small_button(crate::i18n::text("Ändern …")).clicked()
                            && let Some(p) = rfd::FileDialog::new().set_directory(&*location).pick_folder()
                        {
                            *location = p;
                        }
                    });
                    if let Some(rx) = list
                        && let Ok(r) = rx.try_recv()
                    {
                        *repos = Some(r);
                        *list = None;
                    }
                    if list.is_some() {
                        ui.spinner();
                    }
                    if let Some(Ok(rs)) = repos {
                        ui.add_space(4.0);
                        ui.add(crate::widgets::edit(filter).hint_text(crate::i18n::tr("Eigene Repositorys filtern", "Filter your repositories")).desired_width(f32::INFINITY));
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            let f = filter.to_lowercase();
                            for r in rs.iter().filter(|r| f.is_empty() || r.full_name.to_lowercase().contains(&f)) {
                                let label = format!("{}{}", r.full_name, if r.private { "  🔒" } else { "" });
                                if ui.selectable_label(*url == r.clone_url, label).on_hover_text(&r.description).clicked() {
                                    *url = r.clone_url.clone();
                                }
                            }
                        });
                    }
                    let target = location.join(git::clone_dir_name(url));
                    if let Some(rx) = job
                        && let Ok(r) = rx.try_recv()
                    {
                        *job = None;
                        match r {
                            Ok(dir) => {
                                self.switch_project(Some(dir));
                                close = true;
                            }
                            Err(e) => self.git.note = Some((e, true)),
                        }
                    }
                    if let Some((e, true)) = &self.git.note {
                        ui.label(RichText::new(e).small().color(pal.error_text));
                    }
                    ui.horizontal(|ui| {
                        let can = job.is_none() && !url.trim().is_empty() && !target.exists();
                        if ui.add_enabled(can, egui::Button::new(crate::i18n::tr("Klonen", "Clone"))).clicked() {
                            let (u, t) = (url.trim().to_string(), target.clone());
                            self.git.note = None;
                            *job = Some(spawn(ui.ctx(), move || git::clone(&u, &t).map(|_| t)));
                        }
                        if job.is_some() {
                            ui.spinner();
                        }
                        if target.exists() && !url.is_empty() {
                            ui.label(RichText::new(crate::i18n::tr("Den Zielordner gibt es schon.", "The target folder already exists.")).small().color(pal.warn_text));
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                            close = true;
                        }
                    });
                }
                GitDialog::Publish { name, private, description, job } => {
                    title(ui, Icon::CloudUpload, crate::i18n::tr("Auf GitHub veröffentlichen", "Publish to GitHub"));
                    if github::token().is_none() {
                        ui.label(crate::i18n::tr("Bitte zuerst bei GitHub anmelden.", "Please sign in to GitHub first."));
                        if ui.button(crate::i18n::tr("Anmelden …", "Sign in …")).clicked() {
                            relogin = true;
                        }
                    } else {
                        crate::widgets::row(ui, crate::i18n::text("Name"), |ui| {
                            ui.add(crate::widgets::edit(name).desired_width(280.0));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Beschreibung", "Description"), |ui| {
                            ui.add(crate::widgets::edit(description).desired_width(280.0));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Sichtbar", "Visibility"), |ui| {
                            crate::widgets::segmented(ui, private, &[(true, crate::i18n::tr("Privat", "Private")), (false, crate::i18n::tr("Öffentlich", "Public"))]);
                        });
                        if let Some(rx) = job
                            && let Ok(r) = rx.try_recv()
                        {
                            *job = None;
                            match r {
                                Ok(m) => {
                                    self.git.note = Some((m, false));
                                    self.git.gh_for = None;
                                    self.git_refresh();
                                    close = true;
                                }
                                Err(e) => self.git.note = Some((e, true)),
                            }
                        }
                        if let Some((e, true)) = &self.git.note {
                            ui.label(RichText::new(e).small().color(pal.error_text));
                        }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(job.is_none() && !name.trim().is_empty(), egui::Button::new(crate::i18n::tr("Veröffentlichen", "Publish"))).clicked()
                                && let Some(root) = self.git_root().or_else(|| self.project.clone())
                            {
                                let (n, p, d) = (name.trim().to_string(), *private, description.clone());
                                self.git.note = None;
                                *job = Some(spawn(ui.ctx(), move || {
                                    if git::repo_root(&root).is_none() {
                                        git::init_with_commit(&root, "Initial commit")?;
                                    }
                                    let repo = github::create_repo(&n, p, &d)?;
                                    git::set_remote(&root, &repo.clone_url)?;
                                    let st = git::status(&root)?;
                                    if !st.fresh {
                                        git::push(&root, &st, false)?;
                                    }
                                    Ok(crate::tr_format!("Veröffentlicht: {}", "Published: {}", repo.html_url))
                                }));
                            }
                            if job.is_some() {
                                ui.spinner();
                            }
                            if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                                close = true;
                            }
                        });
                    }
                }
                GitDialog::PullRequest { title: t, body, base, draft, job } => {
                    title(ui, Icon::GitPullRequest, crate::i18n::tr("Pull Request erstellen", "Create pull request"));
                    let st = self.git_status_now();
                    let branch = st.as_ref().map(|s| s.branch.clone()).unwrap_or_default();
                    ui.label(RichText::new(format!("{branch} → {base}")).monospace().small());
                    if st.as_ref().is_some_and(|s| s.upstream.is_none() || s.ahead > 0) {
                        ui.label(RichText::new(crate::i18n::tr("Der Branch wird vorher gepusht.", "The branch is pushed first.")).small().color(pal.text_weak));
                    }
                    ui.add(crate::widgets::edit(t).hint_text(crate::i18n::tr("Titel", "Title")).desired_width(f32::INFINITY));
                    ui.add(egui::TextEdit::multiline(body).hint_text(crate::i18n::tr("Beschreibung (Markdown)", "Description (Markdown)")).desired_rows(6).desired_width(f32::INFINITY));
                    ui.checkbox(draft, crate::i18n::tr("Als Entwurf", "As draft"));
                    if let Some(rx) = job
                        && let Ok(r) = rx.try_recv()
                    {
                        *job = None;
                        match r {
                            Ok(pr) => {
                                github::open_url(&pr.html_url);
                                self.git.note = Some((crate::tr_format!("Pull Request #{} erstellt", "Pull request #{} created", pr.number), false));
                                self.git.gh_for = None;
                                close = true;
                            }
                            Err(e) => self.git.note = Some((e, true)),
                        }
                    }
                    if let Some((e, true)) = &self.git.note {
                        ui.label(RichText::new(e).small().color(pal.error_text));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(job.is_none() && !t.trim().is_empty(), egui::Button::new(crate::i18n::tr("Erstellen", "Create"))).clicked()
                            && let (Some(root), Some(d), Some(st)) = (self.git_root(), self.git.gh.clone(), st.clone())
                        {
                            let (t, b, base, draft) = (t.trim().to_string(), body.clone(), base.clone(), *draft);
                            self.git.note = None;
                            *job = Some(spawn(ui.ctx(), move || {
                                if st.upstream.is_none() || st.ahead > 0 {
                                    git::push(&root, &st, false)?;
                                }
                                github::create_pull(&d.owner, &d.repo, &t, &st.branch, &base, &b, draft)
                            }));
                        }
                        if job.is_some() {
                            ui.spinner();
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                            close = true;
                        }
                    });
                }
                GitDialog::Issue { title: t, body, job } => {
                    title(ui, Icon::CircleDot, crate::i18n::tr("Issue anlegen", "New issue"));
                    ui.add(crate::widgets::edit(t).hint_text(crate::i18n::tr("Titel", "Title")).desired_width(f32::INFINITY));
                    ui.add(egui::TextEdit::multiline(body).hint_text(crate::i18n::tr("Beschreibung (Markdown)", "Description (Markdown)")).desired_rows(6).desired_width(f32::INFINITY));
                    if let Some(rx) = job
                        && let Ok(r) = rx.try_recv()
                    {
                        *job = None;
                        match r {
                            Ok(i) => {
                                self.git.note = Some((crate::tr_format!("Issue #{} angelegt", "Issue #{} created", i.number), false));
                                self.git.gh_for = None;
                                close = true;
                            }
                            Err(e) => self.git.note = Some((e, true)),
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(job.is_none() && !t.trim().is_empty(), egui::Button::new(crate::i18n::tr("Anlegen", "Create"))).clicked()
                            && let Some(d) = self.git.gh.clone()
                        {
                            let (t, b) = (t.trim().to_string(), body.clone());
                            *job = Some(spawn(ui.ctx(), move || github::create_issue(&d.owner, &d.repo, &t, &b)));
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                            close = true;
                        }
                    });
                }
                GitDialog::NewBranch { name } => {
                    title(ui, Icon::GitBranch, crate::i18n::tr("Neuer Branch", "New branch"));
                    let r = ui.add(crate::widgets::edit(name).hint_text("feature/…").desired_width(f32::INFINITY));
                    r.request_focus();
                    let valid = !name.trim().is_empty() && !name.contains(' ') && !name.contains("..");
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if (ui.add_enabled(valid, egui::Button::new(crate::i18n::tr("Anlegen und wechseln", "Create and switch"))).clicked() || enter && valid)
                            && let Some(root) = self.git_root()
                        {
                            let n = name.trim().to_string();
                            self.git_job(crate::i18n::tr("Neuer Branch", "New branch"), move || git::switch(&root, &n, true).map(|_| String::new()));
                            close = true;
                        }
                        if ui.button(crate::i18n::text("Abbrechen")).clicked() || esc {
                            close = true;
                        }
                    });
                }
                GitDialog::Log { commits } => {
                    title(ui, Icon::GitCommit, crate::i18n::tr("Verlauf", "History"));
                    let mut show: Option<String> = None;
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for c in commits.iter() {
                            ui.horizontal(|ui| {
                                if ui.link(RichText::new(&c.hash).monospace()).clicked() {
                                    show = Some(c.hash.clone());
                                }
                                ui.add(egui::Label::new(&c.subject).truncate());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(format!("{} · {}", c.author, c.when)).small().color(pal.text_weak));
                                });
                            });
                        }
                    });
                    if let (Some(h), Some(root)) = (show, self.git_root())
                        && let Ok(text) = git::show(&root, &h)
                    {
                        self.open_tab(Box::new(crate::tabs::diff::DiffTab::new(root.clone(), h.clone(), false, text)));
                        close = true;
                    }
                    if ui.button(crate::i18n::text("Schließen")).clicked() || esc {
                        close = true;
                    }
                }
            }
        });
        // Laufende Auftraege im Dialog weiter beobachten
        let waiting = matches!(&dlg, GitDialog::Login { job: Some(_), .. } | GitDialog::Clone { job: Some(_), .. } | GitDialog::Clone { list: Some(_), .. } | GitDialog::Publish { job: Some(_), .. } | GitDialog::PullRequest { job: Some(_), .. } | GitDialog::Issue { job: Some(_), .. });
        if waiting {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if relogin {
            dlg = GitDialog::Login { token: String::new(), error: None, job: None };
        }
        if !close {
            self.git.dialog = Some(dlg);
        }
    }
}
