// Neue Datei (Name links, Dateiendung fest rechts daneben) und neues Projekt (Java, Python, JavaScript).

use super::*;
use crate::dev::java::{self, Driver, project};
use crate::dev::{self, node, python};
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
    Python,
    JavaScript,
    TypeScript,
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
    const ALL: [FileKind; 11] = [
        FileKind::Sql,
        FileKind::Java,
        FileKind::Python,
        FileKind::JavaScript,
        FileKind::TypeScript,
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
            FileKind::Python => "py",
            FileKind::JavaScript => "js",
            FileKind::TypeScript => "ts",
            FileKind::Markdown => "md",
            FileKind::Text => "txt",
            FileKind::Json => "json",
            FileKind::Csv => "csv",
            FileKind::Xml => "xml",
            FileKind::Properties => "properties",
            FileKind::Other => "",
        }
    }

    fn label(self) -> &'static str {
        match self {
            FileKind::Sql => crate::i18n::tr("SQL-Abfrage", "SQL query"),
            FileKind::Java => crate::i18n::tr("Java-Klasse", "Java class"),
            FileKind::Python => crate::i18n::tr("Python-Skript", "Python script"),
            FileKind::JavaScript => "JavaScript (Node.js)",
            FileKind::TypeScript => "TypeScript (Node.js)",
            FileKind::Markdown => crate::i18n::tr("Notiz (Markdown)", "Note (Markdown)"),
            FileKind::Text => "Text",
            FileKind::Json => "JSON",
            FileKind::Csv => crate::i18n::tr("Tabelle (CSV)", "Table (CSV)"),
            FileKind::Xml => "XML",
            FileKind::Properties => crate::i18n::tr("Einstellungen (.properties)", "Settings (.properties)"),
            FileKind::Other => crate::i18n::tr("Andere Endung …", "Other extension …"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            FileKind::Sql => Icon::FileCode,
            FileKind::Java => Icon::Coffee,
            FileKind::Python => Icon::Script,
            FileKind::JavaScript | FileKind::TypeScript => Icon::Braces,
            FileKind::Csv => Icon::FileSheet,
            FileKind::Json | FileKind::Xml | FileKind::Properties => Icon::FileJson,
            _ => Icon::FileText,
        }
    }

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

/// Art einer neuen Python- bzw. JavaScript-Datei
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptKind {
    Empty,
    Main,
    Database,
    Class,
    Module,
    Test,
}

impl ScriptKind {
    fn all(kind: FileKind) -> &'static [ScriptKind] {
        if kind == FileKind::Python {
            &[ScriptKind::Empty, ScriptKind::Main, ScriptKind::Database, ScriptKind::Class, ScriptKind::Test]
        } else {
            &[ScriptKind::Empty, ScriptKind::Main, ScriptKind::Database, ScriptKind::Module, ScriptKind::Class, ScriptKind::Test]
        }
    }

    fn label(self) -> &'static str {
        match self {
            ScriptKind::Empty => crate::i18n::tr("Leer", "Empty"),
            ScriptKind::Main => crate::i18n::tr("Mit main", "With main"),
            ScriptKind::Database => crate::i18n::tr("Mit Datenbank", "With database"),
            ScriptKind::Class => crate::i18n::tr("Klasse", "Class"),
            ScriptKind::Module => crate::i18n::tr("Modul", "Module"),
            ScriptKind::Test => "Test",
        }
    }

    fn id(self) -> &'static str {
        match self {
            ScriptKind::Empty => "",
            ScriptKind::Main => "main",
            ScriptKind::Database => "database",
            ScriptKind::Class => "class",
            ScriptKind::Module => "module",
            ScriptKind::Test => "test",
        }
    }
}

impl JavaKind {
    const ALL: [JavaKind; 6] = [JavaKind::Class, JavaKind::Main, JavaKind::Database, JavaKind::Record, JavaKind::Interface, JavaKind::Enum];

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
    pub name: String,
    pub kind: FileKind,
    /// Endung bei `FileKind::Other`
    pub other_ext: String,
    pub java: JavaKind,
    pub script: ScriptKind,
    /// Erstes Bild: Art und Namen vorschlagen
    ready: bool,
}

impl NewFile {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, name: String::new(), kind: FileKind::Sql, other_ext: String::new(), java: JavaKind::Class, script: ScriptKind::Database, ready: false }
    }

    fn ext(&self) -> String {
        if self.kind == FileKind::Other { self.other_ext.trim().trim_start_matches('.').to_string() } else { self.kind.ext().to_string() }
    }

    /// Vollstaendiger Dateiname
    pub fn file_name(&self) -> String {
        let ext = self.ext();
        if ext.is_empty() { self.name.trim().to_string() } else { format!("{}.{ext}", self.name.trim()) }
    }

    /// Tippt man eine bekannte Endung mit ("liste.md"), wechselt die Art und die Endung verschwindet aus dem Namen.
    fn take_typed_extension(&mut self) {
        if let Some((stem, ext)) = self.name.rsplit_once('.')
            && !stem.is_empty()
            && let Some(k) = FileKind::from_ext(ext)
        {
            self.kind = k;
            self.name = stem.to_string();
        }
    }

    /// Fehler im Namen (oder None)
    fn problem(&self) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some(crate::i18n::tr("Bitte einen Namen eingeben.", "Please enter a name.").into());
        }
        if !workspace::valid_name(&self.file_name()) {
            return Some(crate::i18n::tr("Der Name darf / \\ : * ? \" < > | nicht enthalten.", "The name must not contain / \\ : * ? \" < > |.").into());
        }
        if self.kind == FileKind::Java && !java::source::is_identifier(name) {
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
        let name = self.name.trim();
        match self.kind {
            FileKind::Java => java_template(name, self.java, package_for(&self.dir).as_deref(), self.dir.join("Database.java").is_file(), conn, database),
            FileKind::Python => {
                let (t, l) = python::project::template(self.script.id(), dev::config().python_driver, self.dir.join("db.py").is_file());
                (t.replace("class Name", &format!("class {}", dev::names::pascal(name))), l)
            }
            FileKind::JavaScript | FileKind::TypeScript => {
                let (t, l) = node::project::template(self.script.id(), dev::config().node_driver, self.dir.join("db.js").is_file());
                (t.replace("class Name", &format!("class {}", dev::names::pascal(name))).replace("{ Name }", &format!("{{ {} }}", dev::names::pascal(name))), l)
            }
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

/// Naheliegende Art fuer einen Ordner: Java in Java-Projekten, Python neben .py-Dateien, JavaScript neben package.json
fn context_kind(dir: &Path) -> Option<FileKind> {
    if dir.components().any(|c| c.as_os_str() == "java") || dir.join("pom.xml").is_file() {
        return Some(FileKind::Java);
    }
    if dir.join("package.json").is_file() {
        return Some(if dir.join("tsconfig.json").is_file() { FileKind::TypeScript } else { FileKind::JavaScript });
    }
    if dir.join("requirements.txt").is_file() || dir.join("pyproject.toml").is_file() {
        return Some(FileKind::Python);
    }
    let names: Vec<String> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default();
    [("java", FileKind::Java), ("py", FileKind::Python), ("ts", FileKind::TypeScript), ("js", FileKind::JavaScript)].into_iter().find(|(e, _)| names.iter().any(|n| n.ends_with(&format!(".{e}")))).map(|(_, k)| k)
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
            let open = if has_database {
                "Database.connect()".to_string()
            } else {
                // Beim Start aus EasyMySQL kommen die Verbindungsdaten aus der Umgebung, sonst gelten die Werte hier
                format!(
                    "DriverManager.getConnection(\n                System.getenv().getOrDefault(\"EASYMYSQL_DB_URL\", \"{}\"),\n                System.getenv().getOrDefault(\"EASYMYSQL_DB_USER\", \"{}\"),\n                System.getenv().getOrDefault(\"EASYMYSQL_DB_PASSWORD\", \"\"))",
                    java::jdbc_url(java::driver(), &conn.host, conn.port, database),
                    conn.user.replace('\\', "\\\\").replace('"', "\\\"")
                )
            };
            let body = format!(
                "public class {name} {{\n    public static void main(String[] args) throws SQLException {{\n        try (Connection con = {open}) {{\n            System.out.println(\"{connected}\" + con.getMetaData().getURL());\n            \n        }}\n    }}\n}}\n"
            );
            let line = body.lines().position(|l| l.trim().is_empty() && l.len() > 8).unwrap_or(3);
            (body, line)
        }
    };
    let head_lines = head.matches('\n').count();
    (format!("{head}{body}"), head_lines + line)
}

impl EasyApp {
    /// Dialog "Neue Datei": Name, daneben fest die Endung (waehlbar); bei Java zusaetzlich die Art der Klasse.
    pub(super) fn new_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(side::ExplorerDlg::NewFile(dlg)) = self.explorer_dlg.as_mut() else { return };
        if !dlg.ready {
            dlg.ready = true;
            dlg.kind = context_kind(&dlg.dir).unwrap_or_else(|| FileKind::from_ext(&self.settings.new_file_ext).unwrap_or(FileKind::Sql));
            dlg.name = default_name(&dlg.dir, dlg.kind);
        }
        let pal = style::pal();
        let mut ok = false;
        let mut cancel = false;
        let name_id = egui::Id::new("new-file-name");
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16));
        egui::Modal::new(egui::Id::new("new-file-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(crate::i18n::text("Neue Datei")).strong().size(15.0));
            ui.add_space(10.0);

            // Name und Endung in einem Feld: links der Name, rechts fest die Endung mit Auswahl
            let field = egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .stroke(Stroke::new(1.0, if ui.memory(|m| m.has_focus(name_id)) { pal.accent } else { pal.border }))
                .corner_radius(style::RADIUS)
                .inner_margin(Margin { left: 8, right: 3, top: 0, bottom: 0 });
            let mut kind_changed = false;
            field.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(crate::widgets::CONTROL_H);
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let ext_label = if dlg.kind == FileKind::Other { format!(".{}", dlg.other_ext.trim_start_matches('.')) } else { format!(".{}", dlg.kind.ext()) };
                    let ext_w = ui.painter().layout_no_wrap(ext_label.clone(), egui::FontId::monospace(13.0), pal.text).size().x + 38.0;
                    // Beim Oeffnen ist der Name markiert, damit man sofort lostippen kann
                    if !ui.memory(|m| m.had_focus_last_frame(name_id)) && !ui.memory(|m| m.has_focus(name_id)) && !dlg.name.is_empty() {
                        let mut st = egui::TextEdit::load_state(ui.ctx(), name_id).unwrap_or_default();
                        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(dlg.name.chars().count()))));
                        st.store(ui.ctx(), name_id);
                        ui.memory_mut(|m| m.request_focus(name_id));
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut dlg.name).id(name_id).frame(egui::Frame::NONE).desired_width(ui.available_width() - ext_w).vertical_align(egui::Align::Center));
                    if r.changed() {
                        dlg.take_typed_extension();
                    }
                    let (er, resp) = ui.allocate_exact_size(vec2(ext_w - 4.0, crate::widgets::CONTROL_H - 6.0), Sense::click());
                    ui.painter().rect_filled(er, style::RADIUS as f32 - 2.0, if resp.hovered() { pal.hover } else { pal.face_light });
                    ui.painter().text(er.left_center() + vec2(9.0, 0.0), egui::Align2::LEFT_CENTER, &ext_label, egui::FontId::monospace(13.0), pal.text);
                    icons::paint(ui.painter(), egui::Rect::from_center_size(er.right_center() - vec2(11.0, 0.0), vec2(12.0, 12.0)), Icon::ChevronDown, pal.text_weak);
                    let resp = resp.on_hover_text(crate::i18n::tr("Dateiart wählen", "Choose the file type"));
                    egui::Popup::menu(&resp).show(|ui| {
                        style::menu_item_look(ui);
                        for k in FileKind::ALL.into_iter().chain([FileKind::Other]) {
                            let text = if k == FileKind::Other { k.label().to_string() } else { format!("{}   .{}", k.label(), k.ext()) };
                            let row = ui.horizontal(|ui| {
                                let (ir, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                                icons::paint(ui.painter(), ir, k.icon(), pal.text_weak);
                                ui.selectable_label(dlg.kind == k, text)
                            });
                            if row.inner.clicked() {
                                dlg.kind = k;
                                kind_changed = true;
                                ui.close();
                            }
                        }
                    });
                });
            });
            if kind_changed {
                // Den Vorschlag nur ersetzen, wenn der Name noch der alte Vorschlag ist
                let suggested = FileKind::ALL.iter().any(|k| default_name(&dlg.dir, *k) == dlg.name) || dlg.name.is_empty();
                if suggested && dlg.kind != FileKind::Other {
                    dlg.name = default_name(&dlg.dir, dlg.kind);
                }
                ui.memory_mut(|m| m.request_focus(name_id));
            }
            if dlg.kind == FileKind::Other {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Endung", "Extension"));
                    ui.add(crate::widgets::edit(&mut dlg.other_ext).desired_width(120.0).hint_text("html"));
                });
            }
            if dlg.kind == FileKind::Java {
                ui.add_space(8.0);
                let items: Vec<(JavaKind, &str)> = JavaKind::ALL.iter().map(|k| (*k, k.label())).collect();
                crate::widgets::segmented(ui, &mut dlg.java, &items);
            }
            if matches!(dlg.kind, FileKind::Python | FileKind::JavaScript | FileKind::TypeScript) {
                ui.add_space(8.0);
                let items: Vec<(ScriptKind, &str)> = ScriptKind::all(dlg.kind).iter().map(|k| (*k, k.label())).collect();
                if !ScriptKind::all(dlg.kind).contains(&dlg.script) {
                    dlg.script = ScriptKind::Database;
                }
                crate::widgets::segmented(ui, &mut dlg.script, &items);
            }
            ui.add_space(8.0);
            // Hinweis: wo die Datei entsteht, bzw. was nicht stimmt
            let problem = dlg.problem();
            match &problem {
                Some(p) if !dlg.name.trim().is_empty() => {
                    ui.label(RichText::new(p).small().color(pal.error_text));
                    if dlg.kind == FileKind::Java && !java::source::is_identifier(dlg.name.trim()) {
                        let s = java::source::class_name_from(&dlg.name);
                        if java::source::is_identifier(&s) && ui.link(crate::tr_format!("Stattdessen „{s}“ verwenden", "Use “{s}” instead")).clicked() {
                            dlg.name = s;
                        }
                    }
                }
                _ => {
                    let mut info = crate::tr_format!("Wird angelegt in {}", "Will be created in {}", workspace::dir_name(&dlg.dir));
                    if dlg.kind == FileKind::Java {
                        match package_for(&dlg.dir) {
                            Some(p) => info.push_str(&crate::tr_format!(" · Paket {p}", " · package {p}")),
                            None => info.push_str(crate::i18n::tr(" · ohne Paket", " · no package")),
                        }
                        if dlg.name.starts_with(|c: char| c.is_lowercase()) {
                            info.push_str(crate::i18n::tr(" · Klassennamen beginnen üblicherweise groß", " · class names usually start with a capital letter"));
                        }
                    }
                    ui.label(RichText::new(info).small().color(pal.text_weak));
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let can = problem.is_none();
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(Color32::WHITE)).fill(pal.accent)).clicked() {
                    ok = true;
                }
                if ui.button(crate::i18n::text("Abbrechen")).clicked() {
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
        if dlg.kind != FileKind::Other && self.settings.new_file_ext != dlg.kind.ext() {
            self.settings.new_file_ext = dlg.kind.ext().to_string();
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
    // Neues Projekt (Java, Python, JavaScript)

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
            py_kind: python::project::Kind::from_id(&self.settings.python_project_kind),
            package: "app".into(),
            database,
            tables,
            driver: cfg.java_driver,
            py_driver: cfg.python_driver,
            js_driver: cfg.node_driver,
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
                        ProjectLang::Python => vec![dev::Tool::Python],
                        ProjectLang::Node => vec![dev::Tool::Node],
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
        egui::Modal::new(egui::Id::new("project-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(580.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                icons::paint(ui.painter(), r, dlg.lang.icon(), pal.accent);
                ui.label(RichText::new(crate::i18n::tr("Neues Projekt", "New project")).strong().size(15.0));
            });
            ui.add_space(10.0);
            ui.add_enabled_ui(!busy, |ui| {
                crate::widgets::row(ui, crate::i18n::tr("Sprache", "Language"), |ui| {
                    let before = dlg.lang;
                    let items: Vec<(ProjectLang, &str)> = ProjectLang::ALL.iter().map(|l| (*l, l.label())).collect();
                    if crate::widgets::segmented(ui, &mut dlg.lang, &items) && dlg.name == ProjectDlg::free_name_for(before, &dlg.location) {
                        dlg.name = dlg.free_name();
                    }
                });
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
                let hint = match dlg.lang {
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
                        match dlg.kind {
                            project::Kind::Plain => crate::i18n::tr("Ohne Build-Werkzeug. Startet direkt in EasyMySQL; der Treiber liegt in lib/.", "No build tool. Runs right away in EasyMySQL; the driver is in lib/."),
                            project::Kind::Maven => crate::i18n::tr("pom.xml mit dem Treiber. Maven lädt EasyMySQL bei Bedarf selbst.", "pom.xml with the driver. EasyMySQL downloads Maven when needed."),
                            _ => crate::i18n::tr("Gradle-Skript mit dem Treiber. Gradle lädt EasyMySQL bei Bedarf selbst.", "Gradle script with the driver. EasyMySQL downloads Gradle when needed."),
                        }
                    }
                    ProjectLang::Python => {
                        crate::widgets::row(ui, crate::i18n::tr("Pakete in", "Packages in"), |ui| {
                            let items: Vec<(python::project::Kind, &str)> = python::project::Kind::ALL.iter().map(|k| (*k, k.label())).collect();
                            crate::widgets::segmented(ui, &mut dlg.py_kind, &items);
                        });
                        crate::widgets::row(ui, crate::i18n::tr("Treiber", "Driver"), |ui| {
                            let items: Vec<(python::Driver, &str)> = python::Driver::ALL.iter().map(|d| (*d, d.label())).collect();
                            crate::widgets::segmented(ui, &mut dlg.py_driver, &items);
                        });
                        crate::i18n::tr("EasyMySQL legt beim ersten Start eine .venv an und installiert die Pakete selbst.", "EasyMySQL creates a .venv on the first run and installs the packages itself.")
                    }
                    ProjectLang::Node => {
                        crate::widgets::row(ui, crate::i18n::tr("Treiber", "Driver"), |ui| {
                            let items: Vec<(node::Driver, &str)> = node::Driver::ALL.iter().map(|d| (*d, d.label())).collect();
                            crate::widgets::segmented(ui, &mut dlg.js_driver, &items);
                        });
                        crate::i18n::tr("package.json mit dem Treiber. EasyMySQL führt npm install beim ersten Start selbst aus.", "package.json with the driver. EasyMySQL runs npm install itself on the first run.")
                    }
                };
                crate::widgets::row(ui, "", |ui| {
                    ui.add(egui::Label::new(RichText::new(hint).small().color(pal.text_weak)).wrap());
                });
                crate::widgets::row(ui, crate::i18n::tr("Dazu", "Also"), |ui| {
                    ui.vertical(|ui| {
                        let test = match dlg.lang {
                            ProjectLang::Java => crate::i18n::tr("Tests mit JUnit", "Tests with JUnit"),
                            ProjectLang::Python => crate::i18n::tr("Tests mit unittest", "Tests with unittest"),
                            ProjectLang::Node => crate::i18n::tr("Tests mit node --test", "Tests with node --test"),
                        };
                        crate::widgets::check(ui, &mut dlg.tests, test);
                        crate::widgets::check(ui, &mut dlg.git, crate::i18n::tr("Git-Repository anlegen", "Create a Git repository"));
                    });
                });
                crate::widgets::row(ui, crate::i18n::tr("Datenbank", "Database"), |ui| {
                    let mut d = dlg.database.clone();
                    let mut list = vec![String::new()];
                    list.extend(databases.iter().cloned());
                    if crate::tabs::str_combo(ui, "project-db", &list, &mut d, 200.0) {
                        dlg.database = d;
                        db_changed = true;
                    }
                });
                if !dlg.tables.is_empty() {
                    let label = match dlg.lang {
                        ProjectLang::Java => crate::i18n::tr("Klassen für", "Classes for"),
                        _ => crate::i18n::tr("Code für", "Code for"),
                    };
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
            } else if dlg.location.join(dlg.name.trim()).exists() {
                Some(crate::tr_format!("„{}“ gibt es dort schon.", "“{}” already exists there.", dlg.name.trim()))
            } else {
                None
            };
            ui.add_space(8.0);
            if let Some(e) = dlg.error.as_ref().or(problem.as_ref()) {
                ui.label(RichText::new(e).small().color(pal.error_text));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can = problem.is_none() && !busy;
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(Color32::WHITE)).fill(pal.accent)).clicked() {
                    create = true;
                }
                if ui.add_enabled(!busy, egui::Button::new(crate::i18n::text("Abbrechen"))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                if busy {
                    ui.spinner();
                    ui.label(RichText::new(crate::i18n::tr("Wird angelegt …", "Creating …")).small().color(pal.text_weak));
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
                ProjectLang::Python => self.settings.python_project_kind = dlg.py_kind.id().to_string(),
                ProjectLang::Node => {}
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
            let (lang, kind, py_kind, package, database, driver, py_driver, js_driver, tests, git) = (dlg.lang, dlg.kind, dlg.py_kind, dlg.package.trim().to_string(), dlg.database.clone(), dlg.driver, dlg.py_driver, dlg.js_driver, dlg.tests, dlg.git);
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
                    ProjectLang::Python => {
                        let version = python::latest_on_pypi(py_driver.package()).unwrap_or_else(|_| if py_driver == python::Driver::PyMySql { "1.1.2".into() } else { "9.5.0".into() });
                        let spec = python::project::Spec { name, kind: py_kind, database, tables, host: conn.host.clone(), port: conn.port, user: conn.user.clone(), password, driver: py_driver, driver_version: version, tests };
                        python::project::write(&dir, &spec)
                    }
                    ProjectLang::Node => {
                        let version = node::latest_on_npm(js_driver.package()).unwrap_or_else(|_| if js_driver == node::Driver::Mysql2 { "3.15.3".into() } else { "3.4.5".into() });
                        let spec = node::project::Spec { name, database, tables, host: conn.host.clone(), port: conn.port, user: conn.user.clone(), password, driver: js_driver, driver_version: version, tests };
                        node::project::write(&dir, &spec)
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
        FileKind::Python => {
            if !dir.join("main.py").exists() {
                return "main".into();
            }
            crate::i18n::tr("skript", "script")
        }
        FileKind::JavaScript | FileKind::TypeScript => {
            if !dir.join(format!("index.{}", kind.ext())).exists() {
                return "index".into();
            }
            crate::i18n::tr("modul", "module")
        }
        _ => crate::i18n::tr("Neue Datei", "New file"),
    };
    let ext = if kind == FileKind::Other { "txt" } else { kind.ext() };
    workspace::unique_file(dir, base, ext).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Sprache eines neuen Projekts
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectLang {
    Java,
    Python,
    Node,
}

impl ProjectLang {
    const ALL: [ProjectLang; 3] = [ProjectLang::Java, ProjectLang::Python, ProjectLang::Node];

    fn label(self) -> &'static str {
        match self {
            ProjectLang::Java => "Java",
            ProjectLang::Python => "Python",
            ProjectLang::Node => "JavaScript (Node.js)",
        }
    }

    fn icon(self) -> Icon {
        match self {
            ProjectLang::Java => Icon::Coffee,
            ProjectLang::Python => Icon::Script,
            ProjectLang::Node => Icon::Braces,
        }
    }
}

pub struct ProjectDlg {
    pub lang: ProjectLang,
    pub name: String,
    pub location: PathBuf,
    pub kind: project::Kind,
    pub py_kind: python::project::Kind,
    pub package: String,
    pub database: String,
    /// Tabellen und ob fuer sie Code entsteht
    pub tables: Vec<(String, bool)>,
    pub driver: Driver,
    pub py_driver: python::Driver,
    pub js_driver: node::Driver,
    pub tests: bool,
    pub git: bool,
    job: Option<std::sync::mpsc::Receiver<Result<(PathBuf, PathBuf), String>>>,
    error: Option<String>,
}

impl ProjectDlg {
    fn free_name_for(lang: ProjectLang, root: &Path) -> String {
        let base = match lang {
            ProjectLang::Java => crate::i18n::tr("Java-Projekt", "Java project"),
            ProjectLang::Python => crate::i18n::tr("Python-Projekt", "Python project"),
            ProjectLang::Node => crate::i18n::tr("JavaScript-Projekt", "JavaScript project"),
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
    fn typed_extension_switches_the_kind() {
        let mut f = NewFile::new(PathBuf::from("/tmp"));
        f.name = "liste.md".into();
        f.take_typed_extension();
        assert_eq!((f.name.as_str(), f.kind), ("liste", FileKind::Markdown));
        assert_eq!(f.file_name(), "liste.md");
        f.name = "a.b".into();
        f.take_typed_extension();
        assert_eq!(f.name, "a.b", "unbekannte Endung bleibt im Namen");
        f.kind = FileKind::Other;
        f.other_ext = ".html".into();
        f.name = "seite".into();
        assert_eq!(f.file_name(), "seite.html");
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
        let (t, _) = java_template("Start", JavaKind::Database, None, false, &conn, "shop");
        assert!(t.contains("jdbc:mariadb://127.0.0.1:3306/shop?sslMode=disable"));
    }
}
