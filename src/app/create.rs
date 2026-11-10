// Neue Datei (Name links, Dateiendung fest rechts daneben) und neues Projekt (Java).

use super::*;
use crate::dev::java::{self, Driver, project};
use crate::dev;
use crate::icons::{self, Icon};
use crate::workspace;
use eframe::egui::{Color32, Margin, Sense, Stroke, vec2};

// ---------------------------------------------------------------------------
// Neue Datei

/// Dateiart im Dialog "Neue Datei"
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Sql,
    Java,
    Markdown,
    Text,
    Json,
    Csv,
    Xml,
    Properties,
    /// Selbst getippte Endung
    Other,
}

impl FileKind {
    const ALL: [FileKind; 8] = [
        FileKind::Sql,
        FileKind::Java,
        FileKind::Markdown,
        FileKind::Text,
        FileKind::Json,
        FileKind::Csv,
        FileKind::Xml,
        FileKind::Properties,
    ];

    pub fn ext(self) -> &'static str {
        match self {
            FileKind::Sql => "sql",
            FileKind::Java => "java",
            FileKind::Markdown => "md",
            FileKind::Text => "txt",
            FileKind::Json => "json",
            FileKind::Csv => "csv",
            FileKind::Xml => "xml",
            FileKind::Properties => "properties",
            FileKind::Other => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileKind::Sql => crate::i18n::tr("SQL-Datei", "SQL file"),
            FileKind::Java => crate::i18n::tr("Java-Klasse", "Java class"),
            FileKind::Markdown => crate::i18n::tr("Markdown-Datei", "Markdown file"),
            FileKind::Text => crate::i18n::tr("Textdatei", "Text file"),
            FileKind::Json => "JSON",
            FileKind::Csv => "CSV",
            FileKind::Xml => "XML",
            FileKind::Properties => "Properties",
            FileKind::Other => crate::i18n::tr("Datei", "File"),
        }
    }

    /// Arten im Menue "Neu"
    pub const MENU: [FileKind; 3] = [FileKind::Java, FileKind::Sql, FileKind::Markdown];


    pub fn from_ext(ext: &str) -> Option<FileKind> {
        let e = ext.to_ascii_lowercase();
        FileKind::ALL.into_iter().find(|k| k.ext() == e)
    }
}

/// Art der neuen Java-Datei
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JavaKind {
    Class,
    Main,
    Database,
    Record,
    Interface,
    Enum,
}

impl JavaKind {
    const ALL: [JavaKind; 6] = [JavaKind::Database, JavaKind::Class, JavaKind::Main, JavaKind::Record, JavaKind::Interface, JavaKind::Enum];

    fn label(self) -> &'static str {
        match self {
            JavaKind::Class => crate::i18n::tr("Klasse", "Class"),
            JavaKind::Main => crate::i18n::tr("Mit main", "With main"),
            JavaKind::Database => crate::i18n::tr("Mit Datenbank", "With database"),
            JavaKind::Record => "Record",
            JavaKind::Interface => "Interface",
            JavaKind::Enum => "Enum",
        }
    }
}

pub struct NewFile {
    pub dir: PathBuf,
    /// Was getippt wurde: der ganze Dateiname, meist mit Endung ("Main.java", "notizen.md", ".gitignore")
    pub name: String,
    /// Art, wenn keine Endung getippt wurde (passend zum Ordner oder zuletzt benutzt)
    pub fallback: FileKind,
    pub java: JavaKind,
    /// Erstes Bild: Art vorschlagen
    ready: bool,
}

impl NewFile {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, name: String::new(), fallback: FileKind::Sql, java: JavaKind::Database, ready: false }
    }

    /// Mit fester Art (aus dem Menue "Neu"): ohne Endung wird diese ergaenzt
    pub fn of_kind(dir: PathBuf, kind: FileKind) -> Self {
        Self { fallback: kind, ready: true, java: if kind == FileKind::Java { JavaKind::Class } else { JavaKind::Database }, ..Self::new(dir) }
    }

    /// Getippte Endung (ohne Punkt) und der Name davor. Eine Endung braucht einen Namen davor und Zeichen danach;
    /// ".gitignore" ist ein ganzer Name.
    fn typed_ext(&self) -> Option<(&str, &str)> {
        let (stem, ext) = self.name.trim().rsplit_once('.')?;
        (!stem.is_empty() && !ext.is_empty()).then_some((stem, ext))
    }

    /// Name ohne die getippte Endung
    pub fn stem(&self) -> &str {
        let n = self.name.trim();
        match self.typed_ext() {
            Some((stem, _)) => stem,
            // "Main." beim Tippen: der Punkt gehoert noch nicht zum Namen
            None if n.starts_with('.') => n,
            None => n.trim_end_matches('.'),
        }
    }

    /// Art der Datei: nach der getippten Endung, ohne Endung nach `fallback`
    pub fn kind(&self) -> FileKind {
        match (self.typed_ext(), self.name.trim().starts_with('.')) {
            (Some((_, e)), _) => FileKind::from_ext(e).unwrap_or(FileKind::Other),
            (None, true) => FileKind::Other,
            (None, false) => self.fallback,
        }
    }

    /// Hat der Nutzer die Endung selbst getippt?
    pub fn has_typed_ext(&self) -> bool {
        self.typed_ext().is_some() || self.name.trim().starts_with('.')
    }

    /// Vollstaendiger Dateiname: getippt, sonst mit der Endung der Fallback-Art
    pub fn file_name(&self) -> String {
        if self.has_typed_ext() {
            return self.name.trim().to_string();
        }
        let ext = self.fallback.ext();
        if ext.is_empty() { self.stem().to_string() } else { format!("{}.{ext}", self.stem()) }
    }

    /// Fehler im Namen (oder None)
    fn problem(&self) -> Option<String> {
        let name = self.stem();
        if name.is_empty() {
            return Some(crate::i18n::tr("Bitte einen Namen eingeben.", "Please enter a name.").into());
        }
        if !workspace::valid_name(&self.file_name()) {
            return Some(crate::i18n::tr("Der Name darf / \\ : * ? \" < > | nicht enthalten.", "The name must not contain / \\ : * ? \" < > |.").into());
        }
        if self.kind() == FileKind::Java && !java::source::is_identifier(name) {
            return Some(crate::i18n::tr(
                "Java-Klassen brauchen einen Namen aus Buchstaben, Ziffern und _ – ohne Leerzeichen, nicht mit einer Ziffer am Anfang.",
                "Java classes need a name made of letters, digits and _ – no spaces, not starting with a digit.",
            ).into());
        }
        if self.dir.join(self.file_name()).exists() {
            return Some(crate::tr_format!("„{}“ gibt es hier schon.", "“{}” already exists here.", self.file_name()));
        }
        None
    }

    /// Inhalt der neuen Datei und Zeile (0-basiert), in der der Cursor stehen soll
    pub fn content(&self, conn: &crate::db::ConnInfo, database: &str) -> (String, usize) {
        let name = self.stem();
        match self.kind() {
            FileKind::Java => java_template(name, self.java, package_for(&self.dir).as_deref(), self.dir.join("Database.java").is_file(), conn, database),
            // Wie in einem Skript fuer die Schule: die Datenbank wird gleich zu Beginn gewaehlt
            FileKind::Sql if !database.is_empty() => (format!("USE {};\n\n", crate::db::q(database)), 2),
            FileKind::Markdown => (format!("# {name}\n\n"), 2),
            FileKind::Json => ("{\n  \n}\n".into(), 1),
            FileKind::Xml => ("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n".into(), 1),
            _ => (String::new(), 0),
        }
    }
}

/// Paket fuer eine Datei in `dir`: Ordner unterhalb von src/main/java, src/test/java oder src
pub fn package_for(dir: &Path) -> Option<String> {
    let comps: Vec<String> = dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let root = (0..comps.len())
        .rev()
        .find(|&i| comps[i] == "java" && i >= 2 && matches!(comps[i - 1].as_str(), "main" | "test") && comps[i - 2] == "src")
        .or_else(|| comps.iter().rposition(|c| c == "src"))?;
    let parts = &comps[root + 1..];
    if parts.is_empty() || !parts.iter().all(|p| java::source::is_identifier(p)) {
        return None;
    }
    Some(parts.join("."))
}

/// Naheliegende Art fuer einen Ordner: Java in Java-Projekten und neben .java-Dateien
fn context_kind(dir: &Path) -> Option<FileKind> {
    if dir.components().any(|c| c.as_os_str() == "java") || dir.join("pom.xml").is_file() {
        return Some(FileKind::Java);
    }
    let names: Vec<String> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default();
    names.iter().any(|n| n.ends_with(".java")).then_some(FileKind::Java)
}

fn java_template(name: &str, kind: JavaKind, package: Option<&str>, has_database: bool, conn: &crate::db::ConnInfo, database: &str) -> (String, usize) {
    let mut head = package.map(|p| format!("package {p};\n\n")).unwrap_or_default();
    let (body, line) = match kind {
        JavaKind::Class => (format!("public class {name} {{\n    \n}}\n"), 1),
        JavaKind::Main => (format!("public class {name} {{\n    public static void main(String[] args) {{\n        \n    }}\n}}\n"), 2),
        JavaKind::Record => (format!("public record {name}() {{\n}}\n"), 0),
        JavaKind::Interface => (format!("public interface {name} {{\n    \n}}\n"), 1),
        JavaKind::Enum => (format!("public enum {name} {{\n    \n}}\n"), 1),
        JavaKind::Database => {
            head.push_str("import java.sql.*;\n\n");
            let connected = crate::i18n::tr("Verbunden mit ", "Connected to ");
            // Wie im Unterricht: Adresse, Benutzer und Passwort stehen direkt im Code (kein Treiber-Laden, keine Umgebungsvariablen)
            let (open, method) = if has_database {
                ("Database.connect()", String::new())
            } else {
                (
                    "getConnection()",
                    format!(
                        "\n    public static Connection getConnection() throws SQLException {{\n        return DriverManager.getConnection(\n                \"{}\",  // DB\n                \"{}\",  // User\n                \"\"  // Passwort\n        );\n    }}\n",
                        java::jdbc_url(java::driver(), &conn.host, conn.port, database),
                        conn.user.replace('\\', "\\\\").replace('"', "\\\"")
                    ),
                )
            };
            let body = format!(
                "public class {name} {{\n    public static void main(String[] args) {{\n        try (Connection con = {open}) {{\n            System.out.println(\"{connected}\" + con.getMetaData().getURL());\n            \n        }} catch (SQLException e) {{\n            System.err.println(e.getMessage());\n        }}\n    }}\n{method}}}\n"
            );
            let line = body.lines().position(|l| l.trim().is_empty() && l.len() > 8).unwrap_or(3);
            (body, line)
        }
    };
    let head_lines = head.matches('\n').count();
    (format!("{head}{body}"), head_lines + line)
}

impl EasyApp {
    /// Dialog "Neue Datei" wie in IntelliJ IDEA: ein Feld fuer den ganzen Dateinamen. Die Endung tippt man selbst
    /// ("Main.java", "notizen.md", "seite.html"); ohne Endung wird die zum Ordner passende ergaenzt.
    pub(super) fn new_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(side::ExplorerDlg::NewFile(dlg)) = self.explorer_dlg.as_mut() else { return };
        if !dlg.ready {
            dlg.ready = true;
            dlg.fallback = context_kind(&dlg.dir).unwrap_or_else(|| FileKind::from_ext(&self.settings.new_file_ext).unwrap_or(FileKind::Sql));
        }
        let pal = style::pal();
        let mut ok = false;
        let mut cancel = false;
        let name_id = egui::Id::new("new-file-name");
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16));
        let max_w = (ctx.content_rect().width() - 64.0).max(260.0);
        egui::Modal::new(egui::Id::new("new-file-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(480.0f32.min(max_w));
            ui.label(RichText::new(crate::i18n::text("Neue Datei")).strong().size(15.0));
            ui.add_space(10.0);

            // Ein Feld fuer den ganzen Namen; der Hinweis zeigt, wie man die Endung tippt
            let hint = format!("{}.{}", default_name(&dlg.dir, dlg.fallback), dlg.fallback.ext());
            let focused = ui.memory(|m| m.has_focus(name_id));
            let field = egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .stroke(Stroke::new(1.0, if focused { pal.accent } else { pal.border }))
                .corner_radius(style::RADIUS)
                .inner_margin(Margin::symmetric(8, 0));
            field.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(crate::widgets::CONTROL_H);
                    let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint_file(ui.painter(), ir, &dlg.file_name());
                    let r = ui.add(egui::TextEdit::singleline(&mut dlg.name).id(name_id).hint_text(hint).frame(egui::Frame::NONE).desired_width(f32::INFINITY).vertical_align(egui::Align::Center));
                    if !ui.memory(|m| m.had_focus_last_frame(name_id)) && !focused {
                        ui.memory_mut(|m| m.request_focus(name_id));
                    }
                    let _ = r;
                });
            });
            let kind = dlg.kind();
            if kind == FileKind::Java {
                ui.add_space(8.0);
                let items: Vec<(JavaKind, &str)> = JavaKind::ALL.iter().map(|k| (*k, k.label())).collect();
                crate::widgets::segmented(ui, &mut dlg.java, &items);
            }
            ui.add_space(8.0);
            // Hinweis: was entsteht und wo, bzw. was nicht stimmt
            let problem = dlg.problem();
            match &problem {
                Some(p) if !dlg.name.trim().is_empty() => {
                    ui.label(RichText::new(p).small().color(pal.error_text));
                    if kind == FileKind::Java && !java::source::is_identifier(dlg.stem()) {
                        let s = java::source::class_name_from(dlg.stem());
                        if java::source::is_identifier(&s) && ui.link(crate::tr_format!("Stattdessen „{s}“ verwenden", "Use “{s}” instead")).clicked() {
                            dlg.name = format!("{s}.java");
                        }
                    }
                }
                _ => {
                    let mut info = if dlg.name.trim().is_empty() {
                        crate::tr_format!("Dateiname mit Endung tippen, z. B. {} oder notizen.md", "Type the file name with its extension, e.g. {} or notes.md", hint_example(dlg.fallback))
                    } else {
                        crate::tr_format!("Wird angelegt: {} in {}", "Will be created: {} in {}", dlg.file_name(), workspace::dir_name(&dlg.dir))
                    };
                    if kind == FileKind::Java
                        && !dlg.name.trim().is_empty()
                        && let Some(p) = package_for(&dlg.dir)
                    {
                        info.push_str(&crate::tr_format!(" · Paket {p}", " · package {p}"));
                    }
                    ui.label(RichText::new(info).small().color(pal.text_weak));
                }
            }
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = problem.is_none();
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(Color32::WHITE)).fill(pal.accent).min_size(vec2(84.0, 28.0))).clicked() {
                    ok = true;
                }
                if ui.add(egui::Button::new(crate::i18n::text("Abbrechen")).min_size(vec2(84.0, 28.0))).clicked() {
                    cancel = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && problem.is_none() {
                ok = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
        });
        if cancel {
            self.explorer_dlg = None;
            return;
        }
        if !ok {
            return;
        }
        let Some(side::ExplorerDlg::NewFile(dlg)) = self.explorer_dlg.take() else { return };
        let conn = self.db.as_ref().map(|d| d.info.clone()).unwrap_or_else(|| self.settings.conn.clone());
        let (content, line) = dlg.content(&conn, &self.current_db);
        // Die zuletzt getippte bekannte Endung merken: sie wird ergaenzt, wenn man keine tippt
        if dlg.has_typed_ext() && dlg.kind() != FileKind::Other && self.settings.new_file_ext != dlg.kind().ext() {
            self.settings.new_file_ext = dlg.kind().ext().to_string();
            self.settings.save();
        }
        self.tree_scan = None;
        match workspace::create_file(&dlg.dir, &dlg.file_name(), &content) {
            Ok(p) => {
                self.open_file(&p);
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.goto_line(line);
                }
            }
            Err(e) => self.error(e),
        }
    }

    // ------------------------------------------------------------------
    // Neues Projekt (Java)

    pub(super) fn open_project_dialog(&mut self, lang: ProjectLang) {
        // Datenbanken und Tabellen frisch lesen (sie koennen auch von aussen angelegt worden sein)
        self.refresh_all();
        let database = self.current_db.clone();
        let tables = self.table_names(&database);
        let cfg = dev::config();
        let mut dlg = ProjectDlg {
            lang,
            name: String::new(),
            location: workspace::storage_root(),
            kind: project::Kind::from_id(&self.settings.java_project_kind),
            package: "app".into(),
            database,
            tables,
            driver: cfg.java_driver,
            tests: cfg.junit,
            git: self.settings.git_init_projects,
            job: None,
            error: None,
        };
        dlg.name = dlg.free_name();
        self.project_dlg = Some(dlg);
    }

    fn table_names(&mut self, db: &str) -> Vec<(String, bool)> {
        if db.is_empty() {
            return Vec::new();
        }
        self.schemas.get_arc(self.db.as_ref(), db).map(|s| s.tables.iter().filter(|t| !t.is_view).map(|t| (t.name.clone(), true)).collect()).unwrap_or_default()
    }

    pub(super) fn project_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.project_dlg.take() else { return };
        // Fertig?
        if let Some(rx) = &dlg.job {
            match rx.try_recv() {
                Ok(Ok((dir, main))) => {
                    self.switch_project(Some(dir));
                    self.open_file(&main);
                    // Laufzeit, Treiber und Build-Werkzeug schon jetzt laden, damit "Ausfuehren" gleich klappt
                    let tools: Vec<dev::Tool> = match dlg.lang {
                        ProjectLang::Java => {
                            let mut t = vec![dev::Tool::Jdk, dev::Tool::JavaDrivers];
                            match dlg.kind {
                                project::Kind::Maven => t.push(dev::Tool::Maven),
                                project::Kind::GradleKotlin | project::Kind::GradleGroovy => t.push(dev::Tool::Gradle),
                                project::Kind::Plain if dlg.tests => t.push(dev::Tool::JUnit),
                                project::Kind::Plain => {}
                            }
                            t
                        }
                    };
                    if dev::config().auto_download {
                        dev::install::ensure(&tools, ctx);
                    }
                    self.toast(crate::tr_format!("Projekt „{}“ angelegt.", "Created project “{}”.", dlg.name));
                    return;
                }
                Ok(Err(e)) => {
                    dlg.job = None;
                    dlg.error = Some(e);
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        // Tabellen nachladen, sobald die Struktur da ist
        if dlg.tables.is_empty() && !dlg.database.is_empty() {
            dlg.tables = self.table_names(&dlg.database);
        }
        let pal = style::pal();
        let mut create = false;
        let mut cancel = false;
        let mut pick_dir = false;
        let mut db_changed = false;
        let databases = self.user_databases();
        let busy = dlg.job.is_some();
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18));
        let max_w = (ctx.content_rect().width() - 64.0).max(280.0);
        egui::Modal::new(egui::Id::new("project-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(580.0f32.min(max_w));
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), r, dlg.lang.icon(), pal.accent);
                ui.label(RichText::new(crate::i18n::tr("Neues Projekt", "New project")).strong().size(15.0));
            });
            ui.add_space(10.0);
            ui.add_enabled_ui(!busy, |ui| {
                crate::widgets::row(ui, crate::i18n::text("Name"), |ui| {
                    crate::widgets::text_field(ui, &mut dlg.name, "", Some(300.0));
                });
                crate::widgets::row(ui, crate::i18n::tr("Ort", "Location"), |ui| {
                    let t = dlg.location.display().to_string();
                    ui.add(egui::Label::new(RichText::new(&t).small().monospace()).truncate()).on_hover_text(&t);
                    if ui.small_button(crate::i18n::text("Ändern …")).clicked() {
                        pick_dir = true;
                    }
                });
                match dlg.lang {
                    ProjectLang::Java => {
                        crate::widgets::row(ui, crate::i18n::tr("Aufbau", "Build"), |ui| {
                            let items: Vec<(project::Kind, &str)> = [project::Kind::Plain, project::Kind::Maven, project::Kind::GradleKotlin, project::Kind::GradleGroovy].iter().map(|k| (*k, k.label())).collect();
                            crate::widgets::segmented(ui, &mut dlg.kind, &items);
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Paket", "Package"), |ui| {
                            crate::widgets::text_field(ui, &mut dlg.package, "app", Some(200.0));
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Treiber", "Driver"), |ui| {
                            let items: Vec<(Driver, &str)> = Driver::ALL.iter().map(|d| (*d, d.label())).collect();
                            crate::widgets::segmented(ui, &mut dlg.driver, &items);
                        });
                    }
                }
                crate::widgets::row(ui, crate::i18n::tr("Dazu", "Also"), |ui| {
                    ui.vertical(|ui| {
                        let test = crate::i18n::tr("Tests mit JUnit", "Tests with JUnit");
                        crate::widgets::check(ui, &mut dlg.tests, test);
                        crate::widgets::check(ui, &mut dlg.git, crate::i18n::tr("Git-Repository anlegen", "Create a Git repository"));
                    });
                });
                crate::widgets::row(ui, crate::i18n::tr("Datenbank", "Database"), |ui| {
                    let mut d = dlg.database.clone();
                    let mut list = vec![String::new()];
                    list.extend(databases.iter().cloned());
                    if crate::tabs::db_pick(ui, "project-db", &list, &mut d, 200.0) {
                        dlg.database = d;
                        db_changed = true;
                    }
                });
                if !dlg.tables.is_empty() {
                    let label = crate::i18n::tr("Klassen für", "Classes for");
                    crate::widgets::row(ui, label, |ui| {
                        ui.vertical(|ui| {
                            let all = dlg.tables.iter().all(|t| t.1);
                            let mut a = all;
                            if crate::widgets::check(ui, &mut a, crate::i18n::tr("Alle Tabellen", "All tables")).changed() {
                                for t in dlg.tables.iter_mut() {
                                    t.1 = a;
                                }
                            }
                            egui::ScrollArea::vertical().max_height(150.0).id_salt("project-tables").show(ui, |ui| {
                                for (name, on) in dlg.tables.iter_mut() {
                                    crate::widgets::check(ui, on, name.as_str());
                                }
                            });
                        });
                    });
                } else if !dlg.database.is_empty() {
                    crate::widgets::row(ui, "", |ui| {
                        ui.label(RichText::new(crate::i18n::tr("Die Datenbank hat noch keine Tabellen.", "The database has no tables yet.")).small().color(pal.text_weak));
                    });
                }
            });
            let problem = if dlg.name.trim().is_empty() || !workspace::valid_name(&dlg.name) {
                Some(crate::i18n::tr("Bitte einen gültigen Namen eingeben.", "Please enter a valid name.").to_string())
            } else if dlg.lang == ProjectLang::Java && !project::valid_package(dlg.package.trim()) {
                Some(crate::i18n::tr("Der Paketname besteht aus Wörtern mit Punkt dazwischen, z. B. app oder de.schule.", "A package name is words separated by dots, e.g. app or com.example.").to_string())
            } else if !busy && dlg.location.join(dlg.name.trim()).exists() {
                Some(crate::tr_format!("„{}“ gibt es dort schon.", "“{}” already exists there.", dlg.name.trim()))
            } else {
                None
            };
            ui.add_space(8.0);
            if let Some(e) = dlg.error.as_ref().or(problem.as_ref()) {
                ui.label(RichText::new(e).small().color(pal.error_text));
            }
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = problem.is_none() && !busy;
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(Color32::WHITE)).fill(pal.accent).min_size(vec2(84.0, 28.0))).clicked() {
                    create = true;
                }
                if ui.add_enabled(!busy, egui::Button::new(crate::i18n::text("Abbrechen")).min_size(vec2(84.0, 28.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                if busy {
                    ui.label(RichText::new(crate::i18n::tr("Wird angelegt …", "Creating …")).small().color(pal.text_weak));
                    ui.spinner();
                }
            });
        });
        if cancel && !busy {
            return;
        }
        if db_changed {
            dlg.tables = self.table_names(&dlg.database);
        }
        if pick_dir && let Some(p) = rfd::FileDialog::new().set_directory(&dlg.location).pick_folder() {
            dlg.location = p;
        }
        if create {
            match dlg.lang {
                ProjectLang::Java => self.settings.java_project_kind = dlg.kind.id().to_string(),
            }
            self.settings.git_init_projects = dlg.git;
            self.settings.save();
            let schema = if dlg.database.is_empty() { None } else { self.schemas.get_arc(self.db.as_ref(), &dlg.database).cloned() };
            let chosen: Vec<&String> = dlg.tables.iter().filter(|t| t.1).map(|t| &t.0).collect();
            let tables: Vec<crate::db::TableInfo> = schema.map(|s| s.tables.iter().filter(|t| chosen.contains(&&t.name)).cloned().collect()).unwrap_or_default();
            let conn = self.settings.conn.clone();
            let password = (self.settings.save_password && !conn.password.is_empty()).then(|| conn.password.clone());
            let name = dlg.name.trim().to_string();
            let dir = dlg.location.join(&name);
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            let (lang, kind, package, database, driver, tests, git) = (dlg.lang, dlg.kind, dlg.package.trim().to_string(), dlg.database.clone(), dlg.driver, dlg.tests, dlg.git);
            // Treiber laden bzw. neueste Version erfragen kann dauern: im Hintergrund
            std::thread::spawn(move || {
                let r = match lang {
                    ProjectLang::Java => {
                        if kind == project::Kind::Plain && dev::tools().driver(driver).is_none() {
                            let _ = dev::install::install_now(dev::Tool::JavaDrivers);
                        }
                        let spec = project::Spec {
                            name,
                            kind,
                            package,
                            database,
                            tables,
                            host: conn.host.clone(),
                            port: conn.port,
                            user: conn.user.clone(),
                            password,
                            driver,
                            driver_version: dev::install::driver_version(driver),
                            junit_version: tests.then(dev::install::junit_version),
                        };
                        project::write(&dir, &spec)
                    }
                };
                let r = r.map(|main| {
                    if git {
                        // Ein fehlendes Git verhindert das Projekt nicht
                        let _ = crate::git::init_with_commit(&dir, crate::i18n::tr("Neues Projekt von EasyMySQL", "New project from EasyMySQL"));
                    }
                    (dir, main)
                });
                let _ = tx.send(r);
                repaint.request_repaint();
            });
            dlg.job = Some(rx);
            dlg.error = None;
        }
        self.project_dlg = Some(dlg);
    }
}

/// Freier Vorschlag fuer den Namen je nach Art
/// Beispiel fuer die Eingabezeile, passend zum Ordner
fn hint_example(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Java => "Main.java",
        _ => "abfrage.sql",
    }
}

fn default_name(dir: &Path, kind: FileKind) -> String {
    let base = match kind {
        FileKind::Sql => crate::i18n::text("Neue Abfrage"),
        FileKind::Java => {
            if !dir.join("Main.java").exists() {
                return "Main".into();
            }
            crate::i18n::tr("NeueKlasse", "NewClass")
        }
        FileKind::Markdown => crate::i18n::tr("Notizen", "Notes"),
        _ => crate::i18n::tr("Neue Datei", "New file"),
    };
    let ext = if kind == FileKind::Other { "txt" } else { kind.ext() };
    workspace::unique_file(dir, base, ext).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Sprache eines neuen Projekts
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectLang {
    Java,
}

impl ProjectLang {
    fn icon(self) -> Icon {
        match self {
            ProjectLang::Java => Icon::Coffee,
        }
    }
}

pub struct ProjectDlg {
    pub lang: ProjectLang,
    pub name: String,
    pub location: PathBuf,
    pub kind: project::Kind,
    pub package: String,
    pub database: String,
    /// Tabellen und ob fuer sie Code entsteht
    pub tables: Vec<(String, bool)>,
    pub driver: Driver,
    pub tests: bool,
    pub git: bool,
    job: Option<std::sync::mpsc::Receiver<Result<(PathBuf, PathBuf), String>>>,
    error: Option<String>,
}

impl ProjectDlg {
    fn free_name_for(lang: ProjectLang, root: &Path) -> String {
        let base = match lang {
            ProjectLang::Java => crate::i18n::tr("Java-Projekt", "Java project"),
        };
        let mut name = base.to_string();
        let mut i = 2;
        while root.join(&name).exists() {
            name = format!("{base} {i}");
            i += 1;
        }
        name
    }

    fn free_name(&self) -> String {
        Self::free_name_for(self.lang, &self.location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_follow_the_source_folders() {
        assert_eq!(package_for(Path::new("/p/src/main/java/de/schule")).as_deref(), Some("de.schule"));
        assert_eq!(package_for(Path::new("/p/src/app")).as_deref(), Some("app"));
        assert_eq!(package_for(Path::new("/p/src")), None);
        assert_eq!(package_for(Path::new("/p/src/mein ordner")), None);
        assert_eq!(package_for(Path::new("/p/sonstwo")), None);
    }

    #[test]
    fn the_typed_extension_decides_the_kind() {
        let mut f = NewFile::new(PathBuf::from("/tmp"));
        f.fallback = FileKind::Java;
        for (typed, file, kind, stem) in [
            ("Main.java", "Main.java", FileKind::Java, "Main"),
            ("liste.md", "liste.md", FileKind::Markdown, "liste"),
            ("seite.html", "seite.html", FileKind::Other, "seite"),
            ("archiv.tar.gz", "archiv.tar.gz", FileKind::Other, "archiv.tar"),
            (".gitignore", ".gitignore", FileKind::Other, ".gitignore"),
            ("Kunde", "Kunde.java", FileKind::Java, "Kunde"),
            ("Kunde.", "Kunde.java", FileKind::Java, "Kunde"),
            ("  Kunde  ", "Kunde.java", FileKind::Java, "Kunde"),
            ("DATEN.SQL", "DATEN.SQL", FileKind::Sql, "DATEN"),
        ] {
            f.name = typed.into();
            assert_eq!((f.file_name().as_str(), f.kind(), f.stem()), (file, kind, stem), "{typed:?}");
        }
        // Java-Pruefung gilt dem Namen ohne Endung
        f.name = "Main.java".into();
        assert!(f.problem().is_none());
        f.name = "1x.java".into();
        assert!(f.problem().is_some());
        f.name = "mein Text.md".into();
        assert!(f.problem().is_none());
        // der Inhalt der Vorlage nutzt den Namen ohne Endung
        f.name = "Kunde.java".into();
        assert!(f.content(&crate::db::ConnInfo::default(), "").0.contains("public class Kunde {"));
    }

    #[test]
    fn java_templates_put_the_cursor_into_the_body() {
        let conn = crate::db::ConnInfo::default();
        let (t, line) = java_template("Kunde", JavaKind::Main, Some("app"), false, &conn, "shop");
        assert!(t.starts_with("package app;\n\npublic class Kunde {"));
        assert_eq!(t.lines().nth(line).map(str::trim), Some(""));
        assert_eq!(line, 4);
        let (t, line) = java_template("Start", JavaKind::Database, None, true, &conn, "shop");
        assert!(t.contains("Database.connect()") && t.contains("import java.sql.*;"));
        assert_eq!(t.lines().nth(line).map(str::trim), Some(""));
        let (t, line) = java_template("Start", JavaKind::Database, None, false, &conn, "shop");
        assert!(t.contains("jdbc:mariadb://127.0.0.1:3306/shop?sslMode=disable"));
        // Adresse, Benutzer und Passwort stehen im Code; kein Treiber-Laden, keine Umgebungsvariablen
        assert!(t.contains("getConnection()") && t.contains("// User") && t.contains("// Passwort"));
        assert!(!t.contains("forName") && !t.contains("getenv"));
        assert_eq!(t.lines().nth(line).map(str::trim), Some(""));
    }

    #[test]
    fn sql_files_start_with_use_and_new_java_files_default_to_the_database_class() {
        let mut f = NewFile::new(PathBuf::from("/tmp"));
        f.name = "abfrage.sql".into();
        let conn = crate::db::ConnInfo::default();
        assert_eq!(f.content(&conn, "schule"), ("USE `schule`;\n\n".to_string(), 2));
        assert_eq!(f.content(&conn, "").0, "");
        assert_eq!(f.java, JavaKind::Database);
        assert_eq!(JavaKind::ALL[0], JavaKind::Database);
    }
}
