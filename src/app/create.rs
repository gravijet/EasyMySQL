// Neue Datei (Name links, Dateiendung fest rechts daneben) und neues Projekt (Java).

use crate::style::Semi;
use super::*;
use crate::dev::java::{self, Driver, project};
use crate::dev;
use crate::icons::{self, Icon};
use crate::workspace;
use eframe::egui::{Margin, Sense, Stroke, vec2};

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

/// Bekannte Endungen: alles andere nach einem Punkt ist ein Ordner bzw. Paket ("model.Person")
const KNOWN_EXT: &[&str] = &[
    "sql", "java", "md", "txt", "json", "csv", "xml", "properties", "html", "htm", "css", "js", "mjs", "ts", "py", "yml", "yaml", "sh", "bat",
    "ps1", "kt", "kts", "gradle", "gz", "zip", "tar", "png", "jpg", "svg", "ini", "cfg", "toml", "log", "php", "c", "h", "cpp", "rs", "go",
];

/// Was eine neue Java-Datei enthaelt (ergibt sich aus dem Namen und der gewaehlten Datenbank)
#[derive(Clone, Debug, PartialEq)]
pub enum JavaTemplate {
    /// Modellklasse einer gleichnamigen Tabelle
    Model(String),
    /// DBManager mit Methoden fuer alle Tabellen; fehlende Modellklassen entstehen mit
    Manager { tables: usize, new_models: Vec<String> },
    /// main mit dem DBManager im selben Ordner
    MainWithManager(String),
    /// main mit Verbindung (Database.java im Projekt oder Adresse im Code)
    MainWithDatabase,
    Main,
    Class,
}

pub struct NewFile {
    pub dir: PathBuf,
    /// Was getippt wurde: Dateiname, auch mit Ordnern ("sql/abfrage", "model/Person.java") oder Paket ("model.Person")
    pub name: String,
    /// Art, wenn keine Endung getippt wurde (passend zum Ordner oder zuletzt benutzt)
    pub fallback: FileKind,
    /// Erstes Bild: Art vorschlagen
    ready: bool,
}

/// Aufgeloester Name: Zielordner, Dateiname, Art, Name ohne Endung und die neuen Ordner (relativ)
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub dir: PathBuf,
    pub file: String,
    pub kind: FileKind,
    pub stem: String,
    pub folders: Vec<String>,
    /// Endung selbst getippt (oder Name mit Punkt am Anfang wie .gitignore)
    pub typed_ext: bool,
}

impl NewFile {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, name: String::new(), fallback: FileKind::Sql, ready: false }
    }

    /// Mit fester Art (aus dem Menue "Neu"): ohne Endung wird diese ergaenzt
    pub fn of_kind(dir: PathBuf, kind: FileKind) -> Self {
        Self { fallback: kind, ready: true, ..Self::new(dir) }
    }

    /// Den getippten Namen aufloesen: "/" und "\" trennen Ordner; Punkte vor einer unbekannten Endung (und in
    /// Java-Namen) ebenfalls, so wird "model.Person" zu model/Person.java im Quellordner.
    pub fn target(&self) -> Target {
        let n = self.name.trim().replace('\\', "/");
        let mut parts: Vec<&str> = n.split('/').map(str::trim).collect();
        let last = parts.pop().unwrap_or("");
        let mut folders: Vec<String> = parts.into_iter().filter(|p| !p.is_empty()).map(String::from).collect();
        let dotfile = last.starts_with('.');
        let (mut stem, ext): (String, Option<String>) = match last.rsplit_once('.') {
            _ if dotfile => (last.to_string(), None),
            Some((s, e)) if !s.is_empty() && KNOWN_EXT.contains(&e.to_ascii_lowercase().as_str()) => (s.to_string(), Some(e.to_string())),
            _ => (last.trim_end_matches('.').to_string(), None),
        };
        let kind = match (&ext, dotfile) {
            (Some(e), _) => FileKind::from_ext(e).unwrap_or(FileKind::Other),
            (None, true) => FileKind::Other,
            (None, false) => self.fallback,
        };
        // Punkte im Namen: Pakete (Java) bzw. Ordner (ohne Endung)
        if !dotfile && stem.contains('.') && (kind == FileKind::Java || ext.is_none()) {
            let segs: Vec<String> = stem.split('.').map(|s| s.trim().to_string()).collect();
            if segs.iter().all(|s| !s.is_empty()) {
                stem = segs.last().cloned().unwrap_or_default();
                folders.extend(segs[..segs.len() - 1].iter().cloned());
            }
        }
        let file = match (&ext, dotfile) {
            (Some(e), _) => format!("{stem}.{e}"),
            (None, true) => stem.clone(),
            (None, false) if kind.ext().is_empty() => stem.clone(),
            (None, false) => format!("{stem}.{}", kind.ext()),
        };
        let base = if kind == FileKind::Java { java_source_dir(&self.dir) } else { self.dir.clone() };
        let dir = folders.iter().fold(base, |d, f| d.join(f));
        Target { dir, file, kind, stem, folders, typed_ext: ext.is_some() || dotfile }
    }

    #[cfg(test)]
    /// Name ohne Endung und Ordner
    pub fn stem(&self) -> String {
        self.target().stem
    }

    #[cfg(test)]
    /// Art der Datei: nach der getippten Endung, ohne Endung nach `fallback`
    pub fn kind(&self) -> FileKind {
        self.target().kind
    }

    /// Dateiname ohne Ordner
    pub fn file_name(&self) -> String {
        self.target().file
    }

    /// Fehler im Namen (oder None)
    fn problem(&self) -> Option<String> {
        let t = self.target();
        if t.stem.is_empty() {
            return Some(crate::i18n::tr("Bitte einen Namen eingeben.", "Please enter a name.").into());
        }
        if !workspace::valid_name(&t.file) || t.folders.iter().any(|f| !workspace::valid_name(f)) {
            return Some(crate::i18n::tr("Namen dürfen : * ? \" < > | nicht enthalten und nicht mit einem Punkt beginnen.", "Names must not contain : * ? \" < > | or start with a dot.").into());
        }
        if t.kind == FileKind::Java && !java::source::is_identifier(&t.stem) {
            return Some(crate::i18n::tr(
                "Java-Klassen brauchen einen Namen aus Buchstaben, Ziffern und _ – ohne Leerzeichen, nicht mit einer Ziffer am Anfang.",
                "Java classes need a name made of letters, digits and _ – no spaces, not starting with a digit.",
            ).into());
        }
        if t.kind == FileKind::Java && t.folders.iter().any(|f| !java::source::is_identifier(f)) {
            return Some(crate::i18n::tr("Paketnamen bestehen aus Buchstaben, Ziffern und _ (z. B. model.Person).", "Package names consist of letters, digits and _ (e.g. model.Person).").into());
        }
        if t.dir.join(&t.file).exists() {
            return Some(crate::tr_format!("„{}“ gibt es hier schon.", "“{}” already exists here.", t.file));
        }
        None
    }

    /// Welche Java-Vorlage der Name ergibt
    pub fn java_template(&self, schema: Option<&crate::db::Schema>) -> JavaTemplate {
        let t = self.target();
        java_template_for(&t.stem, &t.dir, schema)
    }

    /// Inhalt der neuen Datei, Zeile (0-basiert) fuer den Cursor und weitere Dateien, die mit entstehen
    pub fn content(&self, conn: &crate::db::ConnInfo, database: &str, schema: Option<&crate::db::Schema>) -> (String, usize, Vec<(PathBuf, String)>) {
        let t = self.target();
        let name = t.stem.as_str();
        match t.kind {
            FileKind::Java => java_content(name, &t.dir, conn, database, schema),
            // Wie in einem Skript fuer die Schule: die Datenbank wird gleich zu Beginn gewaehlt
            FileKind::Sql if !database.is_empty() => (format!("USE {};\n\n", crate::db::q(database)), 2, Vec::new()),
            FileKind::Markdown => (format!("# {name}\n\n"), 2, Vec::new()),
            FileKind::Json => ("{\n  \n}\n".into(), 1, Vec::new()),
            FileKind::Xml => ("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n".into(), 1, Vec::new()),
            _ => (String::new(), 0, Vec::new()),
        }
    }
}

/// Quellordner fuer Java-Dateien: liegt `dir` schon darin, bleibt es; sonst src/main/java bzw. src, wenn vorhanden
fn java_source_dir(dir: &Path) -> PathBuf {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if package_for(dir).is_some() || name == "src" || name == "java" && dir.parent().and_then(|p| p.file_name()).is_some_and(|p| p == "main" || p == "test") {
        return dir.to_path_buf();
    }
    for sub in ["src/main/java", "src"] {
        let p = dir.join(sub);
        if p.is_dir() {
            return p;
        }
    }
    dir.to_path_buf()
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
    if dir.components().any(|c| c.as_os_str() == "java") || ["pom.xml", "build.gradle", "build.gradle.kts"].iter().any(|f| dir.join(f).is_file()) {
        return Some(FileKind::Java);
    }
    // Java-Dateien im Ordner selbst oder (ueblich in Schulprojekten) in src/
    let has_java = |d: &Path| std::fs::read_dir(d).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".java"))).unwrap_or(false);
    (has_java(dir) || has_java(&dir.join("src"))).then_some(FileKind::Java)
}

/// Tabelle, zu der eine Klasse dieses Namens gehoert ("Personen" -> Personen, "SchuelerKlasse" -> schueler_klasse)
fn table_for_class<'a>(name: &str, schema: &'a crate::db::Schema) -> Option<&'a crate::db::TableInfo> {
    schema.tables.iter().filter(|t| !t.is_view).find(|t| t.name.eq_ignore_ascii_case(name) || crate::javagen::class_name(&t.name) == name)
}

fn is_manager_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    matches!(l.as_str(), "dbmanager" | "datenbank" | "db" | "dbconnection" | "databasemanager" | "datenbankmanager") || l.ends_with("dbmanager")
}

/// Vorlage nach Name und Umgebung: Tabellenname -> Modell, DBManager -> alle Tabellen, Main -> main
fn java_template_for(name: &str, dir: &Path, schema: Option<&crate::db::Schema>) -> JavaTemplate {
    let tables: Vec<&crate::db::TableInfo> = schema.map(|s| s.tables.iter().filter(|t| !t.is_view).collect()).unwrap_or_default();
    if let Some(t) = schema.and_then(|s| table_for_class(name, s)) {
        return JavaTemplate::Model(t.name.clone());
    }
    if is_manager_name(name) && !tables.is_empty() {
        let new_models = tables.iter().map(|t| crate::javagen::class_name(&t.name)).filter(|c| !dir.join(format!("{c}.java")).exists()).collect();
        return JavaTemplate::Manager { tables: tables.len(), new_models };
    }
    if matches!(name, "Main" | "App") {
        if let Some(m) = existing_manager(dir) {
            return JavaTemplate::MainWithManager(m);
        }
        return if schema.is_some() || dir.join("Database.java").is_file() { JavaTemplate::MainWithDatabase } else { JavaTemplate::Main };
    }
    JavaTemplate::Class
}

/// Vorhandene DBManager-Klasse im Ordner
fn existing_manager(dir: &Path) -> Option<String> {
    std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".java").map(String::from)).find(|n| is_manager_name(n))
}

/// Zeile nach der Klassen-Deklaration (fuer den Cursor)
fn line_after_class(text: &str) -> usize {
    text.lines().position(|l| l.starts_with("public class") || l.starts_with("public final class")).map_or(0, |l| l + 1)
}

fn java_content(name: &str, dir: &Path, conn: &crate::db::ConnInfo, database: &str, schema: Option<&crate::db::Schema>) -> (String, usize, Vec<(PathBuf, String)>) {
    let package = package_for(dir);
    let pkg = package.as_deref();
    let target = crate::javagen::Target::new(conn, database);
    match java_template_for(name, dir, schema) {
        JavaTemplate::Model(table) => {
            let t = schema.and_then(|s| s.tables.iter().find(|x| x.name == table)).expect("Tabelle aus der Vorlage");
            let text = crate::javagen::school_model(t, name, pkg);
            let line = line_after_class(&text);
            (text, line, Vec::new())
        }
        JavaTemplate::Manager { .. } => {
            let s = schema.expect("Schema aus der Vorlage");
            let models: Vec<(&crate::db::TableInfo, String)> = s.tables.iter().filter(|t| !t.is_view).map(|t| (t, crate::javagen::class_name(&t.name))).collect();
            let text = crate::javagen::school_manager(name, &models, &target, pkg);
            let extra = models
                .iter()
                .filter(|(_, c)| !dir.join(format!("{c}.java")).exists())
                .map(|(t, c)| (dir.join(format!("{c}.java")), crate::javagen::school_model(t, c, pkg)))
                .collect();
            let line = line_after_class(&text);
            (text, line, extra)
        }
        JavaTemplate::MainWithManager(m) => {
            let first = schema.and_then(|s| s.tables.iter().find(|t| !t.is_view && dir.join(format!("{}.java", crate::javagen::class_name(&t.name))).exists())).map(|t| crate::javagen::class_name(&t.name));
            let text = crate::javagen::school_main(name, &m, first.as_deref(), pkg);
            let line = text.lines().position(|l| l.contains("try (Connection con")).map_or(0, |l| l + 1);
            (text, line, Vec::new())
        }
        JavaTemplate::MainWithDatabase => {
            let (t, l) = java_template(name, true, pkg, dir.join("Database.java").is_file(), conn, database);
            (t, l, Vec::new())
        }
        JavaTemplate::Main => {
            let (t, l) = java_template(name, false, pkg, false, conn, database);
            (t, l, Vec::new())
        }
        JavaTemplate::Class => {
            let head = package.map(|p| format!("package {p};\n\n")).unwrap_or_default();
            let lines = head.matches('\n').count();
            (format!("{head}public class {name} {{\n    \n}}\n"), lines + 1, Vec::new())
        }
    }
}

/// main-Klasse; `database`: mit Verbindung (Database.connect() bzw. Adresse im Code)
fn java_template(name: &str, with_db: bool, package: Option<&str>, has_database: bool, conn: &crate::db::ConnInfo, database: &str) -> (String, usize) {
    let mut head = package.map(|p| format!("package {p};\n\n")).unwrap_or_default();
    let (body, line) = if !with_db {
        (format!("public class {name} {{\n    public static void main(String[] args) {{\n        \n    }}\n}}\n"), 2)
    } else {
        head.push_str("import java.sql.*;\n\n");
        let connected = crate::i18n::tr("Verbunden mit ", "Connected to ");
        // Wie im Unterricht: Adresse, Benutzer und Passwort stehen direkt im Code (kein Treiber-Laden, keine Umgebungsvariablen)
        let (open, method) = if has_database {
            ("Database.connect()", String::new())
        } else {
            (
                "getConnection()",
                format!(
                    "\n    public static Connection getConnection() throws SQLException {{\n        return DriverManager.getConnection(\n                \"{}\", // DB\n                \"{}\", // User\n                \"\"); // Passwort\n    }}\n",
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
        let schema = if self.current_db.is_empty() { None } else { self.schemas.get_arc(self.db.as_ref(), &self.current_db).cloned() };
        let Some(side::ExplorerDlg::NewFile(dlg)) = self.explorer_dlg.as_mut() else { return };
        let pal = style::pal();
        let mut ok = false;
        let mut cancel = false;
        let name_id = egui::Id::new("new-file-name");
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16));
        let max_w = (ctx.content_rect().width() - 64.0).max(260.0);
        egui::Modal::new(egui::Id::new("new-file-dlg")).frame(frame).show(ctx, |ui| {
            ui.set_width(480.0f32.min(max_w));
            ui.label(RichText::new(crate::i18n::text("Neue Datei")).semi().size(15.0));
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
            let target = dlg.target();
            let kind = target.kind;
            ui.add_space(8.0);
            // Hinweis: was entsteht und wo, bzw. was nicht stimmt
            let problem = dlg.problem();
            match &problem {
                Some(p) if !dlg.name.trim().is_empty() => {
                    ui.label(RichText::new(p).small().color(pal.error_text));
                    if kind == FileKind::Java && !java::source::is_identifier(&target.stem) {
                        let s = java::source::class_name_from(&target.stem);
                        if java::source::is_identifier(&s) && ui.link(crate::tr_format!("Stattdessen „{s}“ verwenden", "Use “{s}” instead")).clicked() {
                            let mut path = target.folders.join(if kind == FileKind::Java { "." } else { "/" });
                            if !path.is_empty() {
                                path.push('.');
                            }
                            dlg.name = format!("{path}{s}");
                        }
                    }
                }
                _ => {
                    let info = if dlg.name.trim().is_empty() {
                        crate::tr_format!(
                            "Name tippen, z. B. {} – mit / oder . auch in Unterordnern bzw. Paketen ({})",
                            "Type a name, e.g. {} – with / or . also in subfolders or packages ({})",
                            hint_example(dlg.fallback),
                            if dlg.fallback == FileKind::Java { "model.Person" } else { "abfragen/top10" }
                        )
                    } else {
                        let rel = target.dir.strip_prefix(&dlg.dir).map(|r| r.join(&target.file)).unwrap_or_else(|_| target.dir.join(&target.file));
                        let mut i = crate::tr_format!("Wird angelegt: {} in {}", "Will be created: {} in {}", rel.display().to_string().replace('\\', "/"), workspace::dir_name(&dlg.dir));
                        if kind == FileKind::Java {
                            if let Some(p) = package_for(&target.dir) {
                                i.push_str(&crate::tr_format!(" · Paket {p}", " · package {p}"));
                            }
                            i.push_str(" · ");
                            i.push_str(&template_label(&dlg.java_template(schema.as_deref())));
                        }
                        i
                    };
                    ui.label(RichText::new(info).small().color(pal.text_weak));
                }
            }
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = problem.is_none();
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(pal.on_primary)).fill(pal.primary).stroke(crate::style::primary_edge()).min_size(vec2(84.0, 28.0))).clicked() {
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
        let (content, line, extra) = dlg.content(&conn, &self.current_db, schema.as_deref());
        let target = dlg.target();
        // Die zuletzt getippte bekannte Endung merken: sie wird ergaenzt, wenn man keine tippt
        if target.typed_ext && target.kind != FileKind::Other && self.settings.new_file_ext != target.kind.ext() {
            self.settings.new_file_ext = target.kind.ext().to_string();
            self.settings.save();
        }
        self.tree_scan = None;
        if let Err(e) = std::fs::create_dir_all(&target.dir) {
            self.error(crate::tr_format!("Ordner {} konnte nicht angelegt werden: {e}", "Could not create folder {}: {e}", target.dir.display()));
            return;
        }
        // Mit entstehende Dateien (z. B. Modellklassen zum DBManager); vorhandene bleiben unangetastet
        for (path, text) in &extra {
            if !path.exists()
                && let Err(e) = workspace::create_file(path.parent().unwrap_or(&target.dir), &path.file_name().unwrap_or_default().to_string_lossy(), text)
            {
                self.error(e);
            }
        }
        match workspace::create_file(&target.dir, &target.file, &content) {
            Ok(p) => {
                self.open_file(&p);
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.goto_line(line);
                }
                if !extra.is_empty() {
                    let names: Vec<String> = extra.iter().filter_map(|(p, _)| p.file_stem().map(|n| n.to_string_lossy().into_owned())).collect();
                    self.toast(crate::tr_format!("Mit angelegt: {}", "Also created: {}", names.join(", ")));
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
                ui.label(RichText::new(crate::i18n::tr("Neues Projekt", "New project")).semi().size(15.0));
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
                if ui.add_enabled(can, egui::Button::new(RichText::new(crate::i18n::tr("Anlegen", "Create")).color(pal.on_primary)).fill(pal.primary).stroke(crate::style::primary_edge()).min_size(vec2(84.0, 28.0))).clicked() {
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

/// Kurzbeschreibung der Java-Vorlage fuer den Hinweis im Dialog
fn template_label(t: &JavaTemplate) -> String {
    match t {
        JavaTemplate::Model(table) => crate::tr_format!("Klasse mit den Spalten von {table}", "class with the columns of {table}"),
        JavaTemplate::Manager { tables, new_models } if new_models.is_empty() => crate::tr_format!("Verbindung und Methoden für {tables} Tabelle(n)", "connection and methods for {tables} table(s)"),
        JavaTemplate::Manager { tables, new_models } => crate::tr_format!(
            "Verbindung und Methoden für {tables} Tabelle(n), dazu {}",
            "connection and methods for {tables} table(s), plus {}",
            new_models.join(", ")
        ),
        JavaTemplate::MainWithManager(m) => crate::tr_format!("main mit {m}", "main using {m}"),
        JavaTemplate::MainWithDatabase => crate::i18n::tr("main mit Datenbankverbindung", "main with database connection").into(),
        JavaTemplate::Main => crate::i18n::tr("Klasse mit main", "class with main").into(),
        JavaTemplate::Class => crate::i18n::tr("leere Klasse – Tabellenname, DBManager oder Main füllen sie selbst", "empty class – a table name, DBManager or Main fill it in").into(),
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
    #[test]
    fn java_projects_with_sources_in_src_default_to_java() {
        let dir = std::env::temp_dir().join(format!("easymysql-ctx-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        assert_eq!(context_kind(&dir), None);
        std::fs::write(dir.join("src").join("Main.java"), "class Main {}").unwrap();
        assert_eq!(context_kind(&dir), Some(FileKind::Java));
        let _ = std::fs::remove_dir_all(&dir);
    }

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
            assert_eq!((f.file_name().as_str(), f.kind(), f.stem().as_str()), (file, kind, stem), "{typed:?}");
        }
        f.name = "Main.java".into();
        assert!(f.problem().is_none());
        f.name = "1x.java".into();
        assert!(f.problem().is_some());
        f.name = "mein Text.md".into();
        assert!(f.problem().is_none());
        f.name = "Kunde.java".into();
        assert!(f.content(&crate::db::ConnInfo::default(), "", None).0.contains("public class Kunde {"));
    }

    #[test]
    fn slashes_and_dots_make_folders_and_packages() {
        let root = std::env::temp_dir().join(format!("emt-newfile-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let mut f = NewFile::new(root.clone());
        f.fallback = FileKind::Java;
        // Paket: im Quellordner, Paketzeile passt
        f.name = "model.Person".into();
        let t = f.target();
        assert_eq!((t.dir.clone(), t.file.as_str(), t.folders.clone()), (root.join("src").join("model"), "Person.java", vec!["model".to_string()]));
        assert!(f.content(&crate::db::ConnInfo::default(), "", None).0.starts_with("package model;\n\npublic class Person {"));
        f.name = "model/dao\\Person.java".into();
        assert_eq!(f.target().dir, root.join("src").join("model").join("dao"));
        f.name = "model.1x".into();
        assert!(f.problem().is_some());
        // SQL: Ordner per / oder per Punkt vor einer unbekannten Endung; bekannte Endungen bleiben im Namen
        f.fallback = FileKind::Sql;
        f.name = "abfragen/top10".into();
        assert_eq!((f.target().dir, f.file_name()), (root.join("abfragen"), "top10.sql".to_string()));
        f.name = "abfragen.top10".into();
        assert_eq!((f.target().dir, f.file_name()), (root.join("abfragen"), "top10.sql".to_string()));
        f.name = "jquery.min.js".into();
        assert_eq!((f.target().dir, f.file_name()), (root.clone(), "jquery.min.js".to_string()));
        f.name = "../raus".into();
        assert!(f.problem().is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_name_picks_the_java_template() {
        use crate::db::{ColumnInfo, Schema, TableInfo};
        let col = |n: &str, t: &str, k: &str, e: &str| ColumnInfo { name: n.into(), col_type: t.into(), key: k.into(), extra: e.into(), ..Default::default() };
        let schema = Schema {
            name: "person".into(),
            tables: vec![
                TableInfo { name: "Personen".into(), columns: vec![col("pid", "int(11)", "PRI", "auto_increment"), col("vorname", "varchar(100)", "", "")], ..Default::default() },
                TableInfo { name: "Email".into(), columns: vec![col("pid", "int(11)", "", ""), col("email", "varchar(200)", "PRI", "")], ..Default::default() },
            ],
            ..Default::default()
        };
        let root = std::env::temp_dir().join(format!("emt-tpl-{}", std::process::id()));
        let dir = root.join("src").join("model");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Email.java"), "package model;\n").unwrap();
        let conn = crate::db::ConnInfo::default();
        let mut f = NewFile::new(dir.clone());
        f.fallback = FileKind::Java;
        f.name = "Personen".into();
        assert_eq!(f.java_template(Some(&schema)), JavaTemplate::Model("Personen".into()));
        assert!(f.content(&conn, "person", Some(&schema)).0.contains("public void setVorname(String vorname)"));
        f.name = "DBManager".into();
        assert_eq!(f.java_template(Some(&schema)), JavaTemplate::Manager { tables: 2, new_models: vec!["Personen".into()] });
        let (text, _, extra) = f.content(&conn, "person", Some(&schema));
        assert!(text.contains("public Personen getPersonenByPid(int pid, Connection con)") && text.contains("public List<Email> getEmails(Connection con)"), "{text}");
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].0, dir.join("Personen.java"));
        std::fs::write(dir.join("DBManager.java"), text).unwrap();
        f.name = "Main".into();
        assert_eq!(f.java_template(Some(&schema)), JavaTemplate::MainWithManager("DBManager".into()));
        assert!(f.content(&conn, "person", Some(&schema)).0.contains("DBManager db = new DBManager();"));
        f.name = "Rechner".into();
        assert_eq!(f.java_template(Some(&schema)), JavaTemplate::Class);
        f.name = "Main".into();
        let mut g = NewFile::new(root.join("leer"));
        g.fallback = FileKind::Java;
        g.name = "Main".into();
        assert_eq!(g.java_template(None), JavaTemplate::Main);
        assert_eq!(g.java_template(Some(&schema)), JavaTemplate::MainWithDatabase);
        let (t, line, _) = g.content(&conn, "shop", Some(&schema));
        assert!(t.contains("jdbc:mariadb://127.0.0.1:3306/shop?sslMode=disable") && t.contains("// Passwort") && !t.contains("forName"));
        assert_eq!(t.lines().nth(line).map(str::trim), Some(""));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sql_files_start_with_use() {
        let mut f = NewFile::new(PathBuf::from("/tmp"));
        f.name = "abfrage.sql".into();
        let conn = crate::db::ConnInfo::default();
        let (t, line, _) = f.content(&conn, "schule", None);
        assert_eq!((t.as_str(), line), ("USE `schule`;\n\n", 2));
        assert_eq!(f.content(&conn, "", None).0, "");
    }
}
