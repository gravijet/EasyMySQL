// Import aus SQLite, MongoDB-Exporten (JSON, BSON), CSV und Redis.

use super::{Action, Ctx, TabView};
use crate::icons::{self, Icon};
use crate::style;
use crate::transfer::import::write::{self, Options, Progress};
use crate::transfer::import::{self, RedisConf, Source, TableScan};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Sqlite,
    Mongo,
    Csv,
    Redis,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Sqlite => "SQLite",
            Kind::Mongo => "MongoDB",
            Kind::Csv => "CSV",
            Kind::Redis => "Redis",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Kind::Sqlite => &["sqlite", "sqlite3", "db", "db3"],
            Kind::Mongo => &["json", "jsonl", "ndjson", "bson"],
            Kind::Csv => &["csv", "tsv", "txt"],
            Kind::Redis => &[],
        }
    }

    fn from_path(p: &std::path::Path) -> Option<Kind> {
        let ext = p.extension()?.to_string_lossy().to_lowercase();
        [Kind::Sqlite, Kind::Mongo, Kind::Csv].into_iter().find(|k| k.extensions().contains(&ext.as_str()))
    }
}

struct Entry {
    scan: TableScan,
    on: bool,
}

struct Running {
    rx: Receiver<Result<u64, String>>,
    progress: Arc<Progress>,
}

type ScanResult = Result<(Source, Vec<TableScan>), String>;

pub struct ImportTab {
    kind: Kind,
    files: Vec<PathBuf>,
    redis: RedisConf,
    scanning: Option<(Receiver<ScanResult>, Arc<AtomicBool>)>,
    source: Option<Source>,
    entries: Vec<Entry>,
    error: Option<String>,
    database: String,
    new_db: bool,
    new_name: String,
    replace: bool,
    job: Option<Running>,
    log: Vec<(bool, String)>,
    done: Option<u64>,
}

impl ImportTab {
    pub fn new() -> Self {
        Self {
            kind: Kind::Sqlite,
            files: Vec::new(),
            redis: RedisConf::default(),
            scanning: None,
            source: None,
            entries: Vec::new(),
            error: None,
            database: String::new(),
            new_db: true,
            new_name: String::new(),
            replace: false,
            job: None,
            log: Vec::new(),
            done: None,
        }
    }

    fn reset_result(&mut self) {
        self.source = None;
        self.entries.clear();
        self.error = None;
        self.log.clear();
        self.done = None;
    }

    fn start_scan(&mut self, ctx: &egui::Context, source: Source) {
        self.reset_result();
        if let Some((_, c)) = &self.scanning {
            c.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (c, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| import::scan(&source, &c).map(|t| (source.clone(), t))))
                .unwrap_or_else(|_| Err(crate::i18n::text("Die Quelle konnte nicht gelesen werden.").into()));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.scanning = Some((rx, cancel));
    }

    fn choose_files(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new();
        let exts = self.kind.extensions();
        dlg = dlg.add_filter(self.kind.label(), exts).add_filter(crate::i18n::text("Alle Dateien"), &["*"]);
        let picked = if self.kind == Kind::Sqlite { dlg.pick_file().map(|f| vec![f]) } else { dlg.pick_files() };
        if let Some(files) = picked {
            self.set_files(ctx, files);
        }
    }

    fn set_files(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        self.new_name = files[0].file_stem().map(|s| sanitize(&s.to_string_lossy())).unwrap_or_default();
        let source = match self.kind {
            Kind::Sqlite => Source::Sqlite(files[0].clone()),
            Kind::Mongo => Source::Docs(files.clone()),
            _ => Source::Csv(files.clone()),
        };
        self.files = files;
        self.start_scan(ctx, source);
    }

    fn poll(&mut self, cx: &mut Ctx) {
        if let Some((rx, _)) = &self.scanning
            && let Ok(r) = rx.try_recv()
        {
            self.scanning = None;
            match r {
                Ok((src, tables)) => {
                    if tables.is_empty() {
                        self.error = Some(crate::i18n::text("Keine Tabellen gefunden.").into());
                    }
                    self.entries = tables.into_iter().map(|scan| Entry { scan, on: true }).collect();
                    self.source = Some(src);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(job) = &self.job
            && let Ok(r) = job.rx.try_recv()
        {
            self.log = job.progress.log.lock().unwrap().clone();
            self.job = None;
            match r {
                Ok(n) => {
                    self.done = Some(n);
                    cx.status(crate::tr_format!("{n} Zeile(n) importiert.", "{n} row(s) imported."));
                    cx.actions.push(Action::RefreshAll);
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn start_import(&mut self, cx: &mut Ctx) {
        let (Some(dbc), Some(source)) = (cx.db, self.source.clone()) else { return };
        let tables: Vec<TableScan> = self.entries.iter().filter(|e| e.on).map(|e| e.scan.clone()).collect();
        let mut names: Vec<String> = tables.iter().map(|t| t.name.trim().to_lowercase()).collect();
        names.sort();
        if tables.iter().any(|t| t.name.trim().is_empty()) || names.windows(2).any(|w| w[0] == w[1]) {
            self.error = Some(crate::i18n::text("Jede Tabelle braucht einen eigenen Namen.").into());
            return;
        }
        let database = if self.new_db { self.new_name.trim().to_string() } else { self.database.clone() };
        if database.is_empty() {
            self.error = Some(crate::i18n::text("Datenbank wählen oder Namen für eine neue eingeben.").into());
            return;
        }
        self.error = None;
        self.log.clear();
        self.done = None;
        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let (dbc, p, create_db, replace) = (dbc.clone(), progress.clone(), self.new_db, self.replace);
        std::thread::spawn(move || {
            let opts = Options { database, create_db, replace };
            let r = write::run(&dbc, &source, &tables, &opts, &p);
            let _ = tx.send(r);
        });
        self.job = Some(Running { rx, progress });
    }

    fn source_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let items: Vec<(Kind, &str)> = [Kind::Sqlite, Kind::Mongo, Kind::Csv, Kind::Redis].into_iter().map(|k| (k, k.label())).collect();
        if crate::widgets::segmented(ui, &mut self.kind, &items) {
            self.files.clear();
            self.reset_result();
        }
        ui.add_space(6.0);
        if self.kind == Kind::Redis {
            egui::Grid::new("redis").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Server");
                ui.add_sized([180.0, 22.0], egui::TextEdit::singleline(&mut self.redis.host));
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut self.redis.port).range(1..=65535));
                ui.end_row();
                ui.label(crate::i18n::text("Passwort"));
                ui.add_sized([180.0, 22.0], egui::TextEdit::singleline(&mut self.redis.password).password(true));
                ui.label("DB");
                ui.add(egui::DragValue::new(&mut self.redis.db).range(0..=63));
                ui.end_row();
            });
            ui.add_space(4.0);
            if icons::text_button(ui, Icon::Refresh, crate::i18n::text("Lesen")).clicked() {
                self.new_name = "redis".into();
                self.start_scan(&ctx, Source::Redis(self.redis.clone()));
            }
        } else {
            ui.horizontal(|ui| {
                if icons::text_button(ui, Icon::File, crate::i18n::text("Datei wählen …")).clicked() {
                    self.choose_files(&ctx);
                }
                let names: Vec<String> = self.files.iter().filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
                if names.is_empty() {
                    ui.label(RichText::new(crate::i18n::text("oder Datei hierher ziehen")).color(style::pal().text_weak));
                } else {
                    ui.label(names.join(", "));
                }
            });
        }
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

impl TabView for ImportTab {
    fn icon(&self) -> Option<Icon> {
        Some(Icon::Upload)
    }

    fn title(&self) -> String {
        crate::i18n::text("Importieren").into()
    }

    fn key(&self) -> Option<String> {
        Some("import".into())
    }

    fn session(&self) -> Option<String> {
        Some("import".into())
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn takes_drop(&self, path: &std::path::Path) -> bool {
        Kind::from_path(path).is_some()
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        let pal = style::pal();
        if self.scanning.is_some() || self.job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Dateien auf die Registerkarte ziehen
        let dropped: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if let Some(k) = dropped.first().and_then(|p| Kind::from_path(p)) {
            let same: Vec<PathBuf> = dropped.into_iter().filter(|p| Kind::from_path(p) == Some(k)).collect();
            self.kind = k;
            let files = if k == Kind::Sqlite { same.into_iter().take(1).collect() } else { same };
            let ctx = ui.ctx().clone();
            self.set_files(&ctx, files);
        }
        super::page_keys(ui);
        egui::ScrollArea::vertical().id_salt("importscroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(crate::i18n::text("Importieren"));
            ui.add_enabled_ui(self.job.is_none(), |ui| style::group_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.source_ui(ui);
            }));
            if self.scanning.is_some() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::text("Wird gelesen …"));
                });
            }
            if let Some(e) = &self.error {
                ui.add_space(8.0);
                ui.label(RichText::new(e).color(pal.error_text));
            }
            if self.source.is_some() && !self.entries.is_empty() {
                ui.add_space(10.0);
                ui.heading(crate::i18n::text("Tabellen"));
                ui.add_enabled_ui(self.job.is_none(), |ui| style::group_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    for e in self.entries.iter_mut() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut e.on, "");
                            ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut e.scan.name));
                            let cols: Vec<String> = e.scan.columns.iter().map(|c| format!("{}  {}", c.name, c.ty.sql())).collect();
                            ui.label(RichText::new(crate::tr_format!("{} Zeilen, {} Spalten", "{} rows, {} columns", e.scan.rows, e.scan.columns.len())).color(pal.text_weak))
                                .on_hover_text(cols.join("\n"));
                        });
                    }
                }));
                ui.add_space(10.0);
                ui.heading(crate::i18n::text("Ziel"));
                let mut start = false;
                ui.add_enabled_ui(self.job.is_none(), |ui| style::group_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.new_db, true, crate::i18n::text("Neue Datenbank"));
                        ui.add_enabled_ui(self.new_db, |ui| ui.add_sized([180.0, 22.0], egui::TextEdit::singleline(&mut self.new_name)));
                    });
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.new_db, false, crate::i18n::text("Vorhandene"));
                        ui.add_enabled_ui(!self.new_db, |ui| super::db_combo(ui, "imp-db", cx.databases, &mut self.database));
                    });
                    ui.checkbox(&mut self.replace, crate::i18n::text("Vorhandene Tabellen gleichen Namens ersetzen"));
                    ui.add_space(4.0);
                    let any = self.entries.iter().any(|e| e.on);
                    if ui.add_enabled_ui(any && cx.db.is_some(), |ui| icons::primary_button(ui, Icon::Upload, crate::i18n::text("Importieren"))).inner.clicked() {
                        start = true;
                    }
                }));
                if start {
                    self.start_import(cx);
                }
            }
            if let Some(job) = &self.job {
                ui.add_space(10.0);
                let total = job.progress.total.load(Ordering::Relaxed).max(1);
                let done = job.progress.done.load(Ordering::Relaxed);
                let table = job.progress.table.lock().unwrap().clone();
                ui.add(egui::ProgressBar::new((done as f32 / total as f32).min(1.0)).text(format!("{table} · {done} / {total}")));
                if icons::text_button(ui, Icon::Close, crate::i18n::text("Abbrechen")).clicked() {
                    job.progress.cancel.store(true, Ordering::Relaxed);
                }
                self.log = job.progress.log.lock().unwrap().clone();
            }
            if let Some(n) = self.done {
                ui.add_space(8.0);
                ui.label(RichText::new(crate::tr_format!("Fertig: {n} Zeile(n) importiert.", "Done: {n} row(s) imported.")).color(pal.ok_text));
            }
            for (err, line) in &self.log {
                ui.label(RichText::new(line).small().color(if *err { pal.error_text } else { pal.text_weak }));
            }
        });
    }
}
