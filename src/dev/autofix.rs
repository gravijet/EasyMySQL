// Fehler erkennen und automatisch beheben: Ausgabe eines Programms (Java, Python, JavaScript) und
// Compilerfehler werden auf bekannte Muster untersucht. Jeder Treffer erklaert den Fehler und bietet,
// wo moeglich, eine Behebung an (Datenbank anlegen, Code aendern, Paket eintragen, Server starten …).

use super::Tool;
use super::lsp::{FileEdit, TextEdit, WorkspaceEdit};
use super::process::Diag;
use std::path::{Path, PathBuf};

/// Was eine Behebung tut (ausgefuehrt von der App)
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// Textaenderungen in Dateien
    Edit(WorkspaceEdit),
    CreateDatabase(String),
    /// Tabellen-Designer fuer diese Datenbank oeffnen
    NewTable(String),
    /// Abgeleitete Struktur, vor der Ausfuehrung mit SQL-Vorschau bestaetigen
    Schema {
        db: Option<String>,
        table: String,
        columns: Vec<(String, String)>,
    },
    DeklarierteStruktur { datenbank: Option<String>, plaene: Vec<crate::sqledit::TabellenPlan>, quelle: String },
    StartServer,
    ConnectionSettings,
    Install(Tool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    /// Kurze Erklaerung des Fehlers
    pub problem: String,
    /// Beschriftung des Knopfes (leer = nur Erklaerung)
    pub label: String,
    pub fix: Option<Fix>,
}

impl Suggestion {
    fn new(problem: String, label: &str, fix: Fix) -> Self {
        Suggestion { problem, label: label.to_string(), fix: Some(fix) }
    }
    fn hint(problem: String) -> Self {
        Suggestion { problem, label: String::new(), fix: None }
    }
}

fn tr(de: &'static str, en: &'static str) -> &'static str {
    crate::i18n::tr(de, en)
}

// ---------------------------------------------------------------------------
// Hilfen

/// Steuerzeichen (Farben, Cursor) aus der Ausgabe eines Terminals entfernen
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            match it.peek() {
                Some('[') => {
                    it.next();
                    for d in it.by_ref() {
                        if ('@'..='~').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC bis BEL oder ESC \
                    it.next();
                    while let Some(d) = it.next() {
                        if d == '\x07' || (d == '\x1b' && it.peek() == Some(&'\\')) {
                            it.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                _ => {
                    it.next();
                }
            }
        } else if c == '\r' {
            if it.peek() != Some(&'\n') {
                out.push('\n');
            }
        } else if c != '\x07' {
            out.push(c);
        }
    }
    out
}

/// Text zwischen `start` und dem naechsten `end` nach dem ersten Vorkommen von `start`
fn between<'a>(text: &'a str, start: &str, end: char) -> Option<&'a str> {
    let i = text.find(start)? + start.len();
    let rest = &text[i..];
    rest.find(end).map(|j| &rest[..j])
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Alle Vorkommen von `from` in `text` durch `to` ersetzen (als Textaenderungen)
fn replace_edits(text: &str, from: &str, to: &str) -> Vec<TextEdit> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for (i, _) in line.match_indices(from) {
            let a = utf16(&line[..i]);
            out.push(((n, a), (n, a + utf16(from)), to.to_string()));
        }
    }
    out
}

/// Quelltexte des Projekts (ohne Build-Ordner und Abhaengigkeiten)
fn source_files(root: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 || out.len() > 3000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !name.starts_with('.') && !matches!(name.as_str(), "node_modules" | "target" | "build" | "bin" | "out" | "dist" | "__pycache__" | "lib") {
                source_files(&p, out, depth + 1);
            }
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("java" | "py" | "js" | "mjs" | "cjs" | "ts" | "properties" | "ini" | "json" | "env")) {
            out.push(p);
        }
    }
}

/// `from` -> `to` in allen Quelltexten des Projekts (bzw. nur in `file`, wenn kein Projekt offen ist)
fn replace_in_project(project: Option<&Path>, file: Option<&Path>, from: &str, to: &str) -> Option<WorkspaceEdit> {
    let mut files = Vec::new();
    match project {
        Some(p) => source_files(p, &mut files, 0),
        None => files.extend(file.map(Path::to_path_buf)),
    }
    let mut edit = WorkspaceEdit::default();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let edits = replace_edits(&text, from, to);
        if !edits.is_empty() {
            edit.files.push(FileEdit { path: f, edits });
        }
    }
    (!edit.files.is_empty()).then_some(edit)
}

/// Eine Zeile einfuegen: vor Zeile `line` (0-basiert)
fn insert_line(path: &Path, line: usize, text: &str) -> WorkspaceEdit {
    WorkspaceEdit { files: vec![FileEdit { path: path.to_path_buf(), edits: vec![((line, 0), (line, 0), format!("{text}\n"))] }], renames: vec![] }
}

fn single(path: &Path, edit: TextEdit) -> WorkspaceEdit {
    WorkspaceEdit { files: vec![FileEdit { path: path.to_path_buf(), edits: vec![edit] }], renames: vec![] }
}

/// Wort (Bezeichner) an Spalte `col` in `line`
fn word_at(line: &str, col: usize) -> Option<&str> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let idx = col.min(chars.len().checked_sub(1)?);
    let is_w = |c: char| c.is_alphanumeric() || c == '_';
    if !chars.get(idx).is_some_and(|(_, c)| is_w(*c)) {
        return None;
    }
    let mut a = idx;
    while a > 0 && is_w(chars[a - 1].1) {
        a -= 1;
    }
    let mut b = idx;
    while b + 1 < chars.len() && is_w(chars[b + 1].1) {
        b += 1;
    }
    let end = chars.get(b + 1).map(|(i, _)| *i).unwrap_or(line.len());
    Some(&line[chars[a].0..end])
}

// ---------------------------------------------------------------------------
// Bekannte Java-Klassen fuer fehlende Imports

const JAVA_IMPORTS: &[(&str, &str)] = &[
    ("Connection", "java.sql.Connection"),
    ("DriverManager", "java.sql.DriverManager"),
    ("SQLException", "java.sql.SQLException"),
    ("PreparedStatement", "java.sql.PreparedStatement"),
    ("ResultSet", "java.sql.ResultSet"),
    ("ResultSetMetaData", "java.sql.ResultSetMetaData"),
    ("Statement", "java.sql.Statement"),
    ("Types", "java.sql.Types"),
    ("Timestamp", "java.sql.Timestamp"),
    ("List", "java.util.List"),
    ("ArrayList", "java.util.ArrayList"),
    ("LinkedList", "java.util.LinkedList"),
    ("Map", "java.util.Map"),
    ("HashMap", "java.util.HashMap"),
    ("LinkedHashMap", "java.util.LinkedHashMap"),
    ("TreeMap", "java.util.TreeMap"),
    ("Set", "java.util.Set"),
    ("HashSet", "java.util.HashSet"),
    ("TreeSet", "java.util.TreeSet"),
    ("Arrays", "java.util.Arrays"),
    ("Collections", "java.util.Collections"),
    ("Optional", "java.util.Optional"),
    ("Scanner", "java.util.Scanner"),
    ("Random", "java.util.Random"),
    ("Objects", "java.util.Objects"),
    ("Iterator", "java.util.Iterator"),
    ("Properties", "java.util.Properties"),
    ("UUID", "java.util.UUID"),
    ("Collectors", "java.util.stream.Collectors"),
    ("Stream", "java.util.stream.Stream"),
    ("IntStream", "java.util.stream.IntStream"),
    ("LocalDate", "java.time.LocalDate"),
    ("LocalDateTime", "java.time.LocalDateTime"),
    ("LocalTime", "java.time.LocalTime"),
    ("Duration", "java.time.Duration"),
    ("DateTimeFormatter", "java.time.format.DateTimeFormatter"),
    ("BigDecimal", "java.math.BigDecimal"),
    ("BigInteger", "java.math.BigInteger"),
    ("File", "java.io.File"),
    ("IOException", "java.io.IOException"),
    ("BufferedReader", "java.io.BufferedReader"),
    ("InputStreamReader", "java.io.InputStreamReader"),
    ("FileReader", "java.io.FileReader"),
    ("FileWriter", "java.io.FileWriter"),
    ("PrintWriter", "java.io.PrintWriter"),
    ("InputStream", "java.io.InputStream"),
    ("Files", "java.nio.file.Files"),
    ("Path", "java.nio.file.Path"),
    ("Paths", "java.nio.file.Paths"),
    ("Function", "java.util.function.Function"),
    ("Consumer", "java.util.function.Consumer"),
    ("Supplier", "java.util.function.Supplier"),
    ("Predicate", "java.util.function.Predicate"),
    ("Callable", "java.util.concurrent.Callable"),
    ("ExecutorService", "java.util.concurrent.ExecutorService"),
    ("Executors", "java.util.concurrent.Executors"),
    ("Future", "java.util.concurrent.Future"),
    ("Queue", "java.util.Queue"),
    ("Deque", "java.util.Deque"),
    ("ArrayDeque", "java.util.ArrayDeque"),
    ("Comparator", "java.util.Comparator"),
    ("Collection", "java.util.Collection"),
    ("DatabaseMetaData", "java.sql.DatabaseMetaData"),
    ("BatchUpdateException", "java.sql.BatchUpdateException"),
    (
        "SQLIntegrityConstraintViolationException",
        "java.sql.SQLIntegrityConstraintViolationException",
    ),
    ("Savepoint", "java.sql.Savepoint"),
    ("OutputStream", "java.io.OutputStream"),
    ("BufferedWriter", "java.io.BufferedWriter"),
    ("StandardCharsets", "java.nio.charset.StandardCharsets"),
    ("StandardOpenOption", "java.nio.file.StandardOpenOption"),
];

/// Sichere lokale Korrekturen bereits beim Tippen, ohne Compiler oder Sprachserver.
pub fn java_probleme(text: &str) -> Vec<crate::sqledit::Problem> {
    use super::java::source::{self, Span};
    use crate::sqledit::{Fix, Problem, Severity};
    if text.chars().count() > crate::sqledit::MAX_CHARS {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let code: String = source::spans(&chars)
        .into_iter()
        .flat_map(|(k, a, b)| {
            chars[a..b].iter().map(move |c| {
                if k == Span::Code || *c == '\n' {
                    *c
                } else {
                    ' '
                }
            })
        })
        .collect();
    let tokens: Vec<&str> = code
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut imports = String::new();
    for &(short, full) in JAVA_IMPORTS {
        let package = full.rsplit_once('.').unwrap().0;
        let shadowed = tokens
            .windows(2)
            .any(|w| matches!(w[0], "class" | "interface" | "enum" | "record") && w[1] == short);
        let imported = code.lines().any(|l| {
            let compact: String = l.chars().filter(|c| !c.is_whitespace()).collect();
            compact == format!("import{full};")
                || compact == format!("import{package}.*;")
                || (compact.starts_with("import") && compact.ends_with(&format!(".{short};")))
        });
        // Ein fremder Wildcard-Import kann denselben Namen liefern.
        let ambiguous = code
            .lines()
            .any(|l| l.trim_start().starts_with("import ") && l.contains(".*"));
        if imported
            || shadowed
            || ambiguous
            || !tokens.contains(&short)
            || imports.contains(&format!("import {full};\n"))
        {
            continue;
        }
        let usage = code.match_indices(short).find(|(i, _)| {
            let before = code[..*i].chars().next_back();
            let after = code[*i + short.len()..].chars().next();
            before.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$' && c != '.')
                && after.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$')
        });
        if usage.is_some() {
            imports.push_str(&format!("import {full};\n"));
        }
    }
    if !imports.is_empty() {
        let at = text
            .lines()
            .take(import_line(text))
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
            .min(chars.len());
        if at == chars.len() && !text.ends_with('\n') && !text.is_empty() {
            imports.insert(0, '\n');
        }
        out.push(Problem {
            start: at,
            end: at,
            severity: Severity::Warning,
            message: tr(
                "Fehlende Java-Imports ergänzen.",
                "Add missing Java imports.",
            )
            .into(),
            fix: Some(Fix {
                start: at,
                end: at,
                text: imports,
            }),
        });
    }
    let mut offset = 0;
    for line in code.split_inclusive('\n') {
        let trimmed = line.trim();
        if ["System.out.print(", "System.out.println(", "System.err.print(", "System.err.println("].iter().any(|p| trimmed.starts_with(p))
            && trimmed.ends_with(')')
        {
            let mut balance = 0i32;
            let mut valid = true;
            for c in trimmed.chars() {
                if c == '(' { balance += 1; }
                if c == ')' { balance -= 1; valid &= balance >= 0; }
            }
            if valid && balance == 0 {
                let at = offset + line.trim_end().chars().count();
                out.push(Problem { start: at, end: at, severity: Severity::Error,
                    message: tr("Nach dem Ausgabeaufruf fehlt ein Semikolon.", "A semicolon is missing after the print call.").into(),
                    fix: Some(Fix { start: at, end: at, text: ";".into() }) });
            }
        }
        offset += line.chars().count();
    }
    for (from, to) in [
        ("System.out.printLn", "System.out.println"),
        ("System.out.prinln", "System.out.println"),
        ("System.err.prinln", "System.err.println"),
        ("System.err.printLn", "System.err.println"),
        ("System.out.Println", "System.out.println"),
        ("System.err.Println", "System.err.println"),
    ] {
        for (i, _) in code.match_indices(from) {
            if code[i + from.len()..].trim_start().starts_with('(')
                && code[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '.')
            {
                let start = code[..i].chars().count();
                let end = start + from.chars().count();
                out.push(Problem {
                    start,
                    end,
                    severity: Severity::Error,
                    message: crate::tr_format!("{from} heißt {to}.", "Use {to} instead of {from}."),
                    fix: Some(Fix {
                        start,
                        end,
                        text: to.into(),
                    }),
                });
            }
        }
    }
    out
}

/// Zeile, vor der ein neuer Import steht: nach package und den vorhandenen Imports
fn import_line(text: &str) -> usize {
    let mut at = 0;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim_start();
        if t.starts_with("package ") || t.starts_with("import ") {
            at = i + 1;
        } else if !t.is_empty() && !t.starts_with("//") && !t.starts_with("/*") && !t.starts_with('*') {
            break;
        }
    }
    at
}

/// Methodenkopf ueber Zeile `line` (fuer "throws"): Zeile mit ")" und "{", keine Schleife oder Abfrage
fn method_header(text: &str, line: usize) -> Option<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    for i in (0..=line.min(lines.len().saturating_sub(1))).rev() {
        let l = lines[i];
        let t = l.trim();
        let control = ["if", "for", "while", "switch", "catch", "try", "else", "do", "synchronized"].iter().any(|k| t.starts_with(k) && t[k.len()..].trim_start().starts_with(['(', '{']));
        if t.contains(')') && t.ends_with('{') && t.contains('(') && !control && !t.contains('=') && !t.starts_with("new ") && !t.contains("->") {
            return Some((i, l.to_string()));
        }
    }
    None
}

/// Python-Module, deren Paket anders heisst
fn pip_name(module: &str) -> &str {
    match module {
        "mysql" => "mysql-connector-python",
        "pymysql" => "PyMySQL",
        "MySQLdb" => "mysqlclient",
        "dotenv" => "python-dotenv",
        "yaml" => "PyYAML",
        "PIL" => "Pillow",
        "cv2" => "opencv-python",
        "sklearn" => "scikit-learn",
        "bs4" => "beautifulsoup4",
        "dateutil" => "python-dateutil",
        m => m,
    }
}

const PY_STDLIB: &[&str] = &["os", "sys", "json", "math", "random", "datetime", "time", "re", "csv", "pathlib", "decimal", "statistics", "itertools", "collections", "functools", "typing", "dataclasses", "unittest", "subprocess", "shutil", "uuid", "logging", "argparse", "getpass", "hashlib", "string", "textwrap"];

/// Paket in requirements.txt bzw. pyproject.toml eintragen
fn add_python_package(root: &Path, package: &str) -> WorkspaceEdit {
    let req = root.join("requirements.txt");
    let py = root.join("pyproject.toml");
    if !req.is_file()
        && let Ok(text) = std::fs::read_to_string(&py)
        && let Some(n) = text.lines().position(|l| l.trim_start().starts_with("dependencies") && l.contains('['))
    {
        return insert_line(&py, n + 1, &format!("    \"{package}\","));
    }
    let text = std::fs::read_to_string(&req).unwrap_or_default();
    let lines = text.lines().count();
    let prefix = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    single(&req, ((lines, 0), (lines, 0), format!("{prefix}{package}\n")))
}

/// Paket in package.json unter "dependencies" eintragen (Text bleibt sonst unveraendert)
fn add_node_package(root: &Path, package: &str) -> Option<WorkspaceEdit> {
    let pj = root.join("package.json");
    let text = std::fs::read_to_string(&pj).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if let Some(n) = lines.iter().position(|l| l.contains("\"dependencies\"") && l.contains('{')) {
        let empty = lines.get(n + 1).is_some_and(|l| l.trim_start().starts_with('}'));
        let comma = if empty || lines[n].trim_end().ends_with('}') { "" } else { "," };
        if lines[n].trim_end().ends_with('}') {
            // "dependencies": {} in einer Zeile
            let l = lines[n];
            let i = l.rfind('}')?;
            return Some(single(&pj, ((n, utf16(&l[..i])), (n, utf16(&l[..i])), format!("\"{package}\": \"latest\""))));
        }
        return Some(insert_line(&pj, n + 1, &format!("    \"{package}\": \"latest\"{comma}")));
    }
    // Ohne "dependencies": vor der letzten schliessenden Klammer
    let last = lines.iter().rposition(|l| l.trim() == "}")?;
    let prev = (0..last).rev().find(|&i| !lines[i].trim().is_empty())?;
    let l = lines[prev];
    let mut edit = insert_line(&pj, last, &format!("  \"dependencies\": {{\n    \"{package}\": \"latest\"\n  }}"));
    if !l.trim_end().ends_with([',', '{']) {
        edit.files[0].edits.push(((prev, utf16(l.trim_end())), (prev, utf16(l.trim_end())), ",".into()));
    }
    Some(edit)
}

// ---------------------------------------------------------------------------
// Ausgabe eines Programms

/// Erklaerungen und Behebungen fuer die Ausgabe eines Programms. `project`: Projektordner,
/// `file`: gestartete Datei, `local`: eigener Server (kann gestartet werden).
pub fn analyze_output(raw: &str, project: Option<&Path>, file: Option<&Path>, local: bool) -> Vec<Suggestion> {
    let out = strip_ansi(raw);
    let text = out.as_str();
    let mut v: Vec<Suggestion> = Vec::new();
    let root = project.or_else(|| file.and_then(Path::parent));

    // --- Datenbank ---------------------------------------------------------
    if let Some(db) = between(text, "Unknown database '", '\'').or_else(|| between(text, "Unknown database \\'", '\\')) {
        v.push(Suggestion::new(crate::tr_format!("Die Datenbank „{db}“ gibt es nicht.", "The database “{db}” does not exist."), tr("Datenbank anlegen", "Create database"), Fix::CreateDatabase(db.to_string())));
    }
    if let Some(t) = between(text, "Table '", '\'').filter(|_| text.contains("doesn't exist")) {
        let (db, table) = t.split_once('.').unwrap_or(("", t));
        let p = crate::tr_format!(
            "Die Tabelle „{table}“ gibt es nicht.",
            "The table “{table}” does not exist."
        );
        let inferred = java_struktur_fix(
            project,
            file,
            (!db.is_empty()).then_some(db),
            Some(table),
            None,
        );
        v.push(if let Some(fix) = inferred {
            Suggestion::new(p, tr("Struktur erstellen …", "Create structure …"), fix)
        } else if db.is_empty() {
            Suggestion::hint(p)
        } else {
            Suggestion::new(
                p,
                tr("Tabelle anlegen …", "Create table …"),
                Fix::NewTable(db.to_string()),
            )
        });
    }
    if let Some(c) = between(text, "Unknown column '", '\'') {
        let p = crate::tr_format!(
            "Die Spalte „{c}“ gibt es nicht – Schreibweise und Tabelle prüfen.",
            "The column “{c}” does not exist – check the spelling and the table."
        );
        let column = c.rsplit('.').next().unwrap_or(c);
        v.push(match java_struktur_fix(project, file, None, c.rsplit_once('.').map(|(t, _)| t), Some(column)) {
            Some(fix) => Suggestion::new(p, tr("Spalte ergänzen …", "Add column …"), fix),
            None => Suggestion::hint(p),
        });
    }
    if text.contains("Access denied for user") {
        v.push(Suggestion::new(tr("Benutzer oder Passwort stimmen nicht.", "User name or password is wrong.").into(), tr("Verbindung prüfen …", "Check connection …"), Fix::ConnectionSettings));
    }
    let refused = ["Communications link failure", "Connection refused", "Can't connect to", "Socket fail to connect", "Could not connect to address", "ECONNREFUSED", "(2003,", "2003 (HY000)"];
    if refused.iter().any(|p| text.contains(p)) {
        let p = tr("Der Datenbankserver ist nicht erreichbar.", "The database server cannot be reached.").to_string();
        v.push(if local { Suggestion::new(p, tr("Server starten", "Start server"), Fix::StartServer) } else { Suggestion::new(p, tr("Verbindung prüfen …", "Check connection …"), Fix::ConnectionSettings) });
    }
    if text.contains("You have an error in your SQL syntax") {
        let near = between(text, "near '", '\'').unwrap_or("");
        v.push(Suggestion::hint(if near.is_empty() {
            tr("Fehler im SQL-Befehl (Syntax).", "Error in the SQL statement (syntax).").into()
        } else {
            crate::tr_format!("Fehler im SQL-Befehl bei „{near}“ – Komma, Anführungszeichen oder Schlüsselwort prüfen.", "Error in the SQL statement near “{near}” – check commas, quotes and keywords.")
        }));
    }
    if let Some(e) = between(text, "Duplicate entry '", '\'') {
        v.push(Suggestion::hint(crate::tr_format!("Den Wert „{e}“ gibt es in einer eindeutigen Spalte schon (z. B. Primärschlüssel).", "The value “{e}” already exists in a unique column (e.g. the primary key).")));
    }
    if text.contains("No database selected") {
        v.push(Suggestion::hint(tr("Keine Datenbank gewählt: Datenbank in der Adresse angeben oder USE … ausführen.", "No database selected: put the database into the URL or run USE ….").into()));
    }
    if text.contains("cannot be null") && text.contains("Column '") {
        let c = between(text, "Column '", '\'').unwrap_or("");
        v.push(Suggestion::hint(crate::tr_format!("Die Spalte „{c}“ braucht einen Wert (NOT NULL).", "The column “{c}” needs a value (NOT NULL).")));
    }
    if text.contains("foreign key constraint fails") {
        v.push(Suggestion::hint(tr("Fremdschlüssel verletzt: Der verknüpfte Datensatz fehlt oder wird noch verwendet.", "Foreign key violated: the linked row is missing or still in use.").into()));
    }

    // --- Java ----------------------------------------------------------------
    if text.contains("com.mysql.jdbc.Driver") {
        let p = tr("Veraltete Treiberklasse com.mysql.jdbc.Driver.", "Deprecated driver class com.mysql.jdbc.Driver.").to_string();
        match replace_in_project(project, file, "com.mysql.jdbc.Driver", "com.mysql.cj.jdbc.Driver") {
            Some(e) => v.push(Suggestion::new(p, tr("Durch com.mysql.cj.jdbc.Driver ersetzen", "Replace with com.mysql.cj.jdbc.Driver"), Fix::Edit(e))),
            None => v.push(Suggestion::hint(p)),
        }
    }
    if text.contains("No suitable driver found for jdbc:mysql") || text.contains("ClassNotFoundException: com.mysql") {
        match replace_in_project(project, file, "jdbc:mysql://", "jdbc:mariadb://") {
            Some(e) if super::config().java_driver == super::java::Driver::MariaDb => {
                v.push(Suggestion::new(tr("Der MySQL-Treiber fehlt, eingebunden ist der MariaDB-Treiber.", "The MySQL driver is missing; the MariaDB driver is installed.").into(), tr("Adresse auf jdbc:mariadb:// umstellen", "Switch URL to jdbc:mariadb://"), Fix::Edit(e)))
            }
            _ => v.push(Suggestion::new(tr("Der JDBC-Treiber fehlt.", "The JDBC driver is missing.").into(), tr("Treiber laden", "Download driver"), Fix::Install(Tool::JavaDrivers))),
        }
        if let Some(e) = forname_removal(project, file) {
            v.push(Suggestion::new(tr("Class.forName(…) ist unnötig: der Treiber meldet sich selbst an.", "Class.forName(…) is not needed: the driver registers itself.").into(), tr("Class.forName entfernen", "Remove Class.forName"), Fix::Edit(e)));
        }
    } else if text.contains("No suitable driver found for jdbc:mariadb") || text.contains("ClassNotFoundException: org.mariadb") {
        v.push(Suggestion::new(tr("Der MariaDB-Treiber fehlt.", "The MariaDB driver is missing.").into(), tr("Treiber laden", "Download driver"), Fix::Install(Tool::JavaDrivers)));
    }
    if text.contains("Public Key Retrieval is not allowed") {
        let mut edit = WorkspaceEdit::default();
        let mut files = Vec::new();
        match project {
            Some(p) => source_files(p, &mut files, 0),
            None => files.extend(file.map(Path::to_path_buf)),
        }
        for f in files {
            let Ok(t) = std::fs::read_to_string(&f) else { continue };
            let mut edits = Vec::new();
            for (n, line) in t.lines().enumerate() {
                for (i, _) in line.match_indices("jdbc:mysql://") {
                    let end = line[i..].find(['"', '\'', ' ']).map(|j| i + j).unwrap_or(line.len());
                    if line[i..end].contains("allowPublicKeyRetrieval") {
                        continue;
                    }
                    let add = if line[i..end].contains('?') { "&allowPublicKeyRetrieval=true" } else { "?allowPublicKeyRetrieval=true" };
                    let c = utf16(&line[..end]);
                    edits.push(((n, c), (n, c), add.to_string()));
                }
            }
            if !edits.is_empty() {
                edit.files.push(FileEdit { path: f, edits });
            }
        }
        let p = tr("Der MySQL-Treiber darf den Schlüssel des Servers nicht abrufen.", "The MySQL driver may not fetch the server's public key.").to_string();
        v.push(if edit.files.is_empty() { Suggestion::hint(p) } else { Suggestion::new(p, tr("allowPublicKeyRetrieval=true ergänzen", "Add allowPublicKeyRetrieval=true"), Fix::Edit(edit)) });
    }
    let java_hints: [(&str, &str, &str); 9] = [
        ("NullPointerException", "Ein Objekt ist null (nicht angelegt oder nicht gefunden). Die Zeile steht unten im Stacktrace.", "An object is null (never created or not found). The line is shown in the stack trace."),
        ("ArrayIndexOutOfBoundsException", "Zugriff außerhalb des Arrays: Index ist zu groß oder negativ.", "Access outside the array: the index is too large or negative."),
        ("IndexOutOfBoundsException", "Zugriff außerhalb der Liste: Index ist zu groß oder negativ.", "Access outside the list: the index is too large or negative."),
        ("NumberFormatException", "Text lässt sich nicht in eine Zahl umwandeln.", "Text cannot be converted to a number."),
        ("InputMismatchException", "Die Eingabe passt nicht zum erwarteten Typ (z. B. Text statt Zahl).", "The input does not match the expected type (e.g. text instead of a number)."),
        ("ArithmeticException: / by zero", "Division durch 0.", "Division by zero."),
        ("ClassCastException", "Falsche Typumwandlung (Cast).", "Wrong type cast."),
        ("StackOverflowError", "Endlose Rekursion: eine Methode ruft sich immer wieder selbst auf.", "Endless recursion: a method keeps calling itself."),
        ("SQLException: Column", "Spalte im ResultSet nicht gefunden – Name im SELECT prüfen.", "Column not found in the ResultSet – check the name in the SELECT."),
    ];
    for (pat, de, en) in java_hints {
        if text.contains(pat) {
            v.push(Suggestion::hint(tr(de, en).into()));
            if pat == "IndexOutOfBoundsException" || pat == "ArrayIndexOutOfBoundsException" {
                break;
            }
        }
    }
    if text.contains("UnsupportedClassVersionError") {
        v.push(Suggestion::new(tr("Das Programm braucht ein neueres Java.", "The program needs a newer Java.").into(), tr("JDK laden", "Download JDK"), Fix::Install(Tool::Jdk)));
    }

    // --- Python --------------------------------------------------------------
    if let Some(m) = between(text, "No module named '", '\'') {
        let top = m.split('.').next().unwrap_or(m);
        let pkg = pip_name(top);
        let p = crate::tr_format!("Das Python-Paket „{pkg}“ fehlt.", "The Python package “{pkg}” is missing.");
        v.push(match root {
            Some(r) => Suggestion::new(p, tr("Paket eintragen und installieren", "Add and install package"), Fix::Edit(add_python_package(r, pkg))),
            None => Suggestion::hint(p),
        });
    }
    let py_frame = last_python_frame(text);
    if let Some(name) = between(text, "NameError: name '", '\'') {
        let p = crate::tr_format!("„{name}“ ist nicht definiert (Tippfehler oder fehlender Import?).", "“{name}” is not defined (typo or missing import?).");
        match (&py_frame, PY_STDLIB.contains(&name)) {
            (Some((f, _)), true) => {
                let t = std::fs::read_to_string(f).unwrap_or_default();
                let at = t.lines().position(|l| !l.starts_with('#') && !l.starts_with("\"\"\"") && !l.trim().is_empty()).unwrap_or(0);
                v.push(Suggestion::new(p, tr("import ergänzen", "Add import"), Fix::Edit(insert_line(f, at, &format!("import {name}")))));
            }
            _ => v.push(Suggestion::hint(p)),
        }
    }
    if text.contains("SyntaxError: expected ':'") && let Some((f, line)) = &py_frame {
        let t = std::fs::read_to_string(f).unwrap_or_default();
        if let Some(l) = t.lines().nth(*line) {
            let c = utf16(l.trim_end());
            v.push(Suggestion::new(tr("Am Zeilenende fehlt ein Doppelpunkt.", "A colon is missing at the end of the line.").into(), tr("„:“ ergänzen", "Add “:”"), Fix::Edit(single(f, ((*line, c), (*line, c), ":".into())))));
        }
    }
    let py_hints: [(&str, &str, &str); 6] = [
        ("IndentationError", "Die Einrückung stimmt nicht (Leerzeichen und Tabs nicht mischen).", "The indentation is wrong (do not mix spaces and tabs)."),
        ("TypeError: can only concatenate str", "Text und Zahl lassen sich nicht mit + verbinden: str(…) oder f-String verwenden.", "Text and numbers cannot be joined with +: use str(…) or an f-string."),
        ("KeyError:", "Schlüssel fehlt im Dictionary.", "The key is missing in the dictionary."),
        ("ZeroDivisionError", "Division durch 0.", "Division by zero."),
        ("ValueError: invalid literal for int()", "Text lässt sich nicht in eine Zahl umwandeln.", "Text cannot be converted to a number."),
        ("AttributeError: 'NoneType'", "Ein Wert ist None (z. B. fetchone() hat nichts gefunden).", "A value is None (e.g. fetchone() found nothing)."),
    ];
    for (pat, de, en) in py_hints {
        if text.contains(pat) {
            v.push(Suggestion::hint(tr(de, en).into()));
        }
    }

    // --- JavaScript ----------------------------------------------------------
    if let Some(m) = between(text, "Cannot find module '", '\'').filter(|m| !m.starts_with(['.', '/']) && !m.contains(":\\")) {
        let pkg = if m.starts_with('@') { m.splitn(3, '/').take(2).collect::<Vec<_>>().join("/") } else { m.split('/').next().unwrap_or(m).to_string() };
        let p = crate::tr_format!("Das npm-Paket „{pkg}“ fehlt.", "The npm package “{pkg}” is missing.");
        v.push(match root.and_then(|r| add_node_package(r, &pkg)) {
            Some(e) => Suggestion::new(p, tr("In package.json eintragen", "Add to package.json"), Fix::Edit(e)),
            None => Suggestion::hint(p),
        });
    }
    if text.contains("Cannot use import statement outside a module")
        && let Some(r) = root
        && let Ok(t) = std::fs::read_to_string(r.join("package.json"))
        && !t.contains("\"type\"")
        && let Some(n) = t.lines().position(|l| l.trim() == "{")
    {
        v.push(Suggestion::new(
            tr("import braucht ein ES-Modul.", "import needs an ES module.").into(),
            tr("\"type\": \"module\" in package.json", "\"type\": \"module\" in package.json"),
            Fix::Edit(insert_line(&r.join("package.json"), n + 1, "  \"type\": \"module\",")),
        ));
    }
    if text.contains("require is not defined in ES module scope")
        && let Some(r) = root
        && let Some(e) = replace_in_project(None, Some(&r.join("package.json")), "\"type\": \"module\"", "\"type\": \"commonjs\"")
    {
        v.push(Suggestion::new(tr("require gibt es in ES-Modulen nicht.", "require does not exist in ES modules.").into(), tr("Auf CommonJS umstellen", "Switch to CommonJS"), Fix::Edit(e)));
    }
    if let Some(name) = between(text, "ReferenceError: ", ' ').filter(|_| text.contains("is not defined")) {
        v.push(Suggestion::hint(crate::tr_format!("„{name}“ ist nicht definiert (Tippfehler oder fehlendes require/import?).", "“{name}” is not defined (typo or missing require/import?).")));
    }
    if text.contains("TypeError: Cannot read properties of undefined") || text.contains("TypeError: Cannot read properties of null") {
        v.push(Suggestion::hint(tr("Ein Wert ist undefined/null (z. B. leeres Ergebnis einer Abfrage).", "A value is undefined/null (e.g. an empty query result).").into()));
    }
    dedup(v)
}

fn dedup(v: Vec<Suggestion>) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for s in v {
        if !out.iter().any(|o| o.problem == s.problem) {
            out.push(s);
        }
    }
    out
}

/// Nur fehlende Spalten hinzufuegen; bestehende Definitionen und Daten bleiben erhalten.
pub fn struktur_sql(schema: &crate::db::Schema, table: &str, columns: &[(String, String)]) -> Option<String> {
    let target = format!("{}.{}", crate::db::q(&schema.name), crate::db::q(table));
    let existing = schema.tables.iter().find(|t| t.name == table);
    let definitions: Vec<String> = columns.iter()
        .filter(|(name, _)| existing.is_none_or(|t| !t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(name))))
        .map(|(name, typ)| format!("{} {typ} NULL", crate::db::q(name))).collect();
    if definitions.is_empty() { return None; }
    Some(if existing.is_some() {
        format!("ALTER TABLE {target}\n  ADD COLUMN {};", definitions.join(",\n  ADD COLUMN "))
    } else {
        format!("CREATE TABLE {target} (\n  {}\n);", definitions.join(",\n  "))
    })
}

fn java_struktur_fix(
    project: Option<&Path>,
    file: Option<&Path>,
    db: Option<&str>,
    table: Option<&str>,
    column: Option<&str>,
) -> Option<Fix> {
    let mut files = Vec::new();
    if let Some(root) = project {
        source_files(root, &mut files, 0);
    } else {
        files.extend(file.map(Path::to_path_buf));
    }
    if let Some(root) = project.or_else(|| file.and_then(Path::parent)) {
        match deklarierter_fix(root, &files, db, table, column) {
            Ok(Some(fix)) => return Some(fix),
            Ok(None) => {},
            Err(_) => return None,
        }
    }
    let mut result = None;
    for path in files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "java"))
    {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some((db, table, columns)) = crate::sqledit::java_struktur(&text, db, table, column)
        {
            let fix = Fix::Schema { db, table, columns };
            if result.as_ref().is_some_and(|old| *old != fix) {
                return None;
            }
            result = Some(fix);
        }
    }
    result
}

fn migrations_dateien(root: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 12 || out.len() >= 3000 { return; }
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for entry in rd.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() && !name.starts_with('.') && !matches!(name.as_str(), "node_modules" | "target" | "build" | "bin" | "out" | "dist" | "lib") {
            migrations_dateien(&entry.path(), out, depth + 1);
        } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == "sql") { out.push(entry.path()); }
    }
}

fn deklarierter_fix(root: &Path, files: &[PathBuf], db: Option<&str>, table: Option<&str>, column: Option<&str>) -> Result<Option<Fix>, String> {
    let mut migrations = Vec::new(); migrations_dateien(root, &mut migrations, 0);
    // Versionsnummern natuerlich sortieren: V2 vor V10, auch ueber mehrere Migrationen hinweg.
    migrations.sort_by_cached_key(|p| {
        let mut key = String::new(); let path = p.to_string_lossy(); let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_ascii_digit() {
                let mut n = String::from(c);
                while chars.peek().is_some_and(|c| c.is_ascii_digit()) { n.push(chars.next().unwrap()); }
                key.push_str(&format!("{n:0>20}"));
            } else { key.push(c); }
        }
        key
    });
    let mut sql = String::new();
    for path in &migrations {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if text.len() > crate::sqledit::MAX_CHARS * 4 || sql.len() + text.len() > 8_000_000 { return Err("Migration zu gross".into()); }
        sql.push_str(&text); sql.push_str("\n;\n");
    }
    let mut plans = crate::sqledit::migrationen(&sql)?;
    let texts = files.iter().filter(|f| f.extension().is_some_and(|e| e == "java")).filter_map(|f| std::fs::read_to_string(f).ok()).collect::<Vec<_>>();
    let mut has_jpa = false;
    match crate::sqledit::jpa_modelle(&texts) {
        Ok(models) => {
            for model in models {
                if !plans.iter().any(|p| p.tabelle == model.tabelle && p.datenbank == model.datenbank) { plans.push(model); has_jpa = true; }
            }
        }
        Err(e) if !plans.iter().any(|p| table.is_none_or(|t| p.tabelle == t) && column.is_none_or(|c| p.teile.iter().any(|d| d.name.as_deref() == Some(c)))) => return Err(e),
        Err(_) => {},
    }
    let source = if has_jpa { tr("SQL-Migrationen/JPA-Modelle", "SQL migrations/JPA models") } else { tr("SQL-Migrationen", "SQL migrations") };
    plans.retain(|p| db.zip(p.datenbank.as_deref()).is_none_or(|(a, b)| a == b));
    let targets = plans.iter().filter(|p| table.is_none_or(|t| p.tabelle == t) && column.is_none_or(|c| p.teile.iter().any(|d| d.name.as_deref() == Some(c)))).collect::<Vec<_>>();
    if targets.len() != 1 { return Ok(None); }
    let target = targets[0].tabelle.clone();
    let database = db.map(str::to_string).or_else(|| targets[0].datenbank.clone());
    plans.retain(|p| p.datenbank.as_ref().is_none_or(|d| Some(d) == database.as_ref()));
    // Nur das Ziel und deklarierte FK-Abhaengigkeiten vorschlagen, keine fremden Projekttabellen.
    let mut selected = vec![target];
    loop {
        let mut added = false;
        for p in plans.iter().filter(|p| selected.contains(&p.tabelle)).collect::<Vec<_>>() {
            for t in p.teile.iter().filter(|t| t.kennung.starts_with("FOREIGN ")) {
                if let Some((ref_db, ref_table)) = &t.referenz {
                    if ref_db.as_ref().is_some_and(|d| Some(d) != database.as_ref()) { return Err("Datenbankuebergreifende Beziehung".into()); }
                    if plans.iter().any(|p| &p.tabelle == ref_table) && !selected.contains(ref_table) {
                        selected.push(ref_table.clone()); added = true;
                    }
                }
            }
        }
        if !added { break; }
    }
    plans.retain(|p| selected.contains(&p.tabelle));
    for p in &mut plans { p.datenbank = database.clone(); }
    Ok(Some(Fix::DeklarierteStruktur { datenbank: database, plaene: plans, quelle: source.into() }))
}


/// Zeilen mit `Class.forName("…Driver")` entfernen
fn forname_removal(project: Option<&Path>, file: Option<&Path>) -> Option<WorkspaceEdit> {
    let mut files = Vec::new();
    match project {
        Some(p) => source_files(p, &mut files, 0),
        None => files.extend(file.map(Path::to_path_buf)),
    }
    let mut edit = WorkspaceEdit::default();
    for f in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "java")) {
        let Ok(t) = std::fs::read_to_string(&f) else { continue };
        let edits: Vec<TextEdit> = t.lines().enumerate().filter(|(_, l)| l.contains("Class.forName(") && l.contains("Driver") && l.trim_end().ends_with(';')).map(|(n, _)| ((n, 0), (n + 1, 0), String::new())).collect();
        if !edits.is_empty() {
            edit.files.push(FileEdit { path: f, edits });
        }
    }
    (!edit.files.is_empty()).then_some(edit)
}

/// Letzter Eintrag `File "…", line N` eines Python-Tracebacks (eigene Datei, 0-basierte Zeile)
fn last_python_frame(text: &str) -> Option<(PathBuf, usize)> {
    text.lines()
        .filter_map(|l| {
            let f = between(l, "File \"", '"')?;
            let n: usize = between(l, ", line ", ',').or_else(|| l.split(", line ").nth(1))?.trim().parse().ok()?;
            Some((PathBuf::from(f), n.saturating_sub(1)))
        })
        .rfind(|(f, _)| !f.to_string_lossy().contains("site-packages") && !f.to_string_lossy().contains("<frozen"))
}

// ---------------------------------------------------------------------------
// Compilerfehler (Java)

/// Behebung fuer einen Compilerfehler (`text`: Inhalt der Datei)
pub fn fix_for_diag(d: &Diag, text: &str) -> Option<Suggestion> {
    let m = d.message.as_str();
    let line = text.lines().nth(d.line).unwrap_or("");
    let is_java = d.file.extension().is_some_and(|e| e == "java");
    if m.contains("';' expected") || m.contains("';' erwartet") || m.contains("insert \";\" to complete") {
        let chars: Vec<char> = line.chars().collect();
        let comment = super::java::source::spans(&chars)
            .into_iter()
            .find(|(k, _, _)| *k == super::java::source::Span::Comment)
            .map(|(_, a, _)| a);
        let before: String = chars[..comment.unwrap_or(chars.len())].iter().collect();
        let code_end = before.trim_end();
        if code_end.ends_with(';') {
            return None;
        }
        let c = utf16(code_end);
        return Some(Suggestion::new(
            tr(
                "Am Ende der Anweisung fehlt ein Semikolon.",
                "A semicolon is missing at the end of the statement.",
            )
            .into(),
            tr("„;“ ergänzen", "Add “;”"),
            Fix::Edit(single(&d.file, ((d.line, c), (d.line, c), ";".into()))),
        ));
    }
    if is_java && (m.contains("cannot find symbol") || m.contains("Symbol nicht gefunden") || m.contains("cannot be resolved to a type") || m.contains("cannot be resolved to a variable")) {
        let w = d.col.and_then(|c| word_at(line, c))?;
        let (_, full) = JAVA_IMPORTS.iter().find(|(n, _)| *n == w)?;
        if text.contains(&format!("import {full};")) || text.contains(&format!("import {}.*;", full.rsplit_once('.')?.0)) {
            return None;
        }
        let at = import_line(text);
        // Erster Import einer Datei ohne package: Leerzeile davor zum Code
        let line = if at == 0 { format!("import {full};\n") } else { format!("import {full};") };
        return Some(Suggestion::new(crate::tr_format!("Die Klasse {w} ist nicht importiert.", "The class {w} is not imported."), &format!("import {full};"), Fix::Edit(insert_line(&d.file, at, &line))));
    }
    if is_java && (m.contains("unreported exception") || m.contains("nicht gemeldete Ausnahme")) {
        let ex = m.split_whitespace().find(|w| w.ends_with("Exception;") || w.ends_with("Exception")).map(|w| w.trim_end_matches(';').to_string())?;
        let short = ex.rsplit('.').next().unwrap_or(&ex).to_string();
        let (n, header) = method_header(text, d.line)?;
        let brace = header.rfind('{')?;
        let before = header[..brace].trim_end();
        let insert = if before.contains(" throws ") { format!(", {short}") } else { format!(" throws {short}") };
        let c = utf16(before);
        let mut edit = single(&d.file, ((n, c), (n, c), insert));
        // Import fuer SQLException & Co. mitliefern
        if let Some((_, full)) = JAVA_IMPORTS.iter().find(|(s, _)| *s == short)
            && !text.contains(&format!("import {full};"))
            && !text.contains(&format!("import {}.*;", full.rsplit_once('.').map(|x| x.0).unwrap_or("")))
        {
            let at = import_line(text);
            edit.files[0].edits.push(((at, 0), (at, 0), format!("import {full};\n")));
        }
        return Some(Suggestion::new(crate::tr_format!("{short} wird weder abgefangen noch weitergegeben.", "{short} is neither caught nor declared."), &format!("throws {short}"), Fix::Edit(edit)));
    }
    if is_java && (m.contains("should be declared in a file named") || m.contains("in einer Datei namens")) {
        let stem = d.file.file_stem()?.to_string_lossy().into_owned();
        let class = m.split_whitespace().skip_while(|w| *w != "class" && *w != "Klasse").nth(1)?.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let i = line.find(&format!("class {class}")).or_else(|| line.find(&format!("record {class}"))).or_else(|| line.find(&format!("interface {class}"))).or_else(|| line.find(&format!("enum {class}")))?;
        let a = i + line[i..].find(class)?;
        let (ca, cb) = (utf16(&line[..a]), utf16(&line[..a + class.len()]));
        return Some(Suggestion::new(crate::tr_format!("Klasse und Datei heißen verschieden ({class} / {stem}.java).", "Class and file names differ ({class} / {stem}.java)."), &crate::tr_format!("Klasse in {stem} umbenennen", "Rename class to {stem}"), Fix::Edit(single(&d.file, ((d.line, ca), (d.line, cb), stem)))));
    }
    if m.contains("reached end of file while parsing")
        || m.contains("Dateiende beim Parsen erreicht")
    {
        let chars: Vec<char> = text.chars().collect();
        let mut balance = 0usize;
        for (kind, a, b) in super::java::source::spans(&chars) {
            if kind != super::java::source::Span::Code {
                continue;
            }
            for c in &chars[a..b] {
                if *c == '{' {
                    balance += 1;
                }
                if *c == '}' {
                    balance = balance.checked_sub(1)?;
                }
            }
        }
        if balance == 0 {
            return None;
        }
        let n = text.lines().count();
        let nl = if text.ends_with('\n') || text.is_empty() {
            ""
        } else {
            "\n"
        };
        let braces = "}\n".repeat(balance);
        return Some(Suggestion::new(
            tr(
                "Am Ende fehlen schließende Klammern }.",
                "Closing braces } are missing at the end.",
            )
            .into(),
            tr("„}“ ergänzen", "Add “}”"),
            Fix::Edit(single(&d.file, ((n, 0), (n, 0), format!("{nl}{braces}")))),
        ));
    }
    if m.contains("missing return statement") || m.contains("Rückgabeanweisung fehlt") {
        return Some(Suggestion::hint(tr("Die Methode muss auf jedem Weg einen Wert mit return liefern.", "The method must return a value on every path.").into()));
    }
    if m.contains("incompatible types") || m.contains("Inkompatible Typen") || m.contains("inkompatible Typen") {
        return Some(Suggestion::hint(tr("Typen passen nicht – z. B. Integer.parseInt(text) für Text → Zahl, String.valueOf(zahl) für Zahl → Text.", "Types do not match – e.g. Integer.parseInt(text) for text → number, String.valueOf(n) for number → text.").into()));
    }
    if m.contains("might not have been initialized") || m.contains("ist möglicherweise nicht initialisiert") {
        return Some(Suggestion::hint(tr("Die Variable bekommt nicht auf jedem Weg einen Wert – beim Anlegen einen Startwert geben.", "The variable is not assigned on every path – give it a start value.").into()));
    }
    if m.contains("unclosed string literal") || m.contains("nicht geschlossenes Zeichenfolgenliteral") {
        let c = utf16(line.trim_end().trim_end_matches(';'));
        return Some(Suggestion::new(tr("Ein Anführungszeichen \" fehlt.", "A quotation mark \" is missing.").into(), tr("„\"“ ergänzen", "Add “\"”"), Fix::Edit(single(&d.file, ((d.line, c), (d.line, c), "\"".into())))));
    }
    None
}

// ---------------------------------------------------------------------------
// Verweise auf Dateien in der Ausgabe (zum Anklicken)

/// Stelle in einer Zeile der Ausgabe: Zeichen von..bis, Datei (wie geschrieben) und Zeile (0-basiert)
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub file: String,
    pub line: usize,
}

/// Verweise in einer Zeile: `(Main.java:13)`, `File "x.py", line 5`, `/pfad/index.js:10:5`, `x.py:12`
pub fn links(line: &str) -> Vec<Link> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    // Python: File "…", line N
    if let Some(i) = line.find("File \"") {
        let a = line[..i].chars().count() + 6;
        if let Some(len) = chars[a..].iter().position(|&c| c == '"') {
            let file: String = chars[a..a + len].iter().collect();
            let rest: String = chars[a + len..].iter().collect();
            if let Some(n) = rest.strip_prefix("\", line ").and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse::<usize>().ok()) {
                let end = a + len + 8 + n.to_string().len();
                out.push(Link { start: a - 6, end, file, line: n.saturating_sub(1) });
                return out;
            }
        }
    }
    // Datei.endung:zeile
    let exts = ["java", "py", "js", "mjs", "cjs", "ts", "sql", "kt"];
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            // Dateiname rueckwaerts bis Klammer, Leerzeichen oder Anfang
            let mut a = i;
            while a > 0 && !matches!(chars[a - 1], '(' | ' ' | '\t' | '"' | '\'' | '[' | '<' | '@') {
                a -= 1;
            }
            let name: String = chars[a..i].iter().collect();
            // file:///pfad
            let name = name.strip_prefix("file://").map(str::to_string).unwrap_or(name);
            let ok = name.rsplit_once('.').is_some_and(|(_, e)| exts.contains(&e));
            let mut b = i + 1;
            while b < chars.len() && chars[b].is_ascii_digit() {
                b += 1;
            }
            if ok && let Ok(n) = chars[i + 1..b].iter().collect::<String>().parse::<usize>() {
                // :spalte mitnehmen
                let mut end = b;
                if end + 1 < chars.len() && chars[end] == ':' && chars[end + 1].is_ascii_digit() {
                    end += 1;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        end += 1;
                    }
                }
                out.push(Link { start: a, end, file: name, line: n.saturating_sub(1) });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Datei zu einem Verweis finden: absolut, relativ zum Projekt oder per Name (Java: Paket aus `at a.b.C.m(…)`)
pub fn resolve(project: Option<&Path>, cwd: Option<&Path>, line: &str, file: &str) -> Option<PathBuf> {
    let p = PathBuf::from(file);
    if p.is_absolute() {
        return p.is_file().then_some(p);
    }
    for base in [cwd, project].into_iter().flatten() {
        let q = base.join(&p);
        if q.is_file() {
            return Some(q);
        }
    }
    let root = project.or(cwd)?;
    // Java: "at model.Main.main(Main.java:13)" -> model/Main.java
    let pkg_path = line.trim_start().strip_prefix("at ").and_then(|s| s.split('(').next()).map(|q| {
        let parts: Vec<&str> = q.split('.').collect();
        let pkg = &parts[..parts.len().saturating_sub(2)];
        pkg.iter().map(|s| s.to_string()).chain(std::iter::once(file.to_string())).collect::<Vec<_>>().join("/")
    });
    let mut found: Vec<PathBuf> = Vec::new();
    let mut files = Vec::new();
    source_files(root, &mut files, 0);
    for f in files {
        let s = f.to_string_lossy().replace('\\', "/");
        if pkg_path.as_ref().is_some_and(|pp| s.ends_with(&format!("/{pp}"))) {
            return Some(f);
        }
        if f.file_name().is_some_and(|n| n.to_string_lossy() == *file) {
            found.push(f);
        }
    }
    found.into_iter().next()
}

/// Art einer Zeile der Ausgabe (fuer Farben)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineKind {
    /// Fehlermeldung (Exception, Error, Traceback)
    Error,
    /// Warnung, veraltet
    Warning,
    /// Eintrag im Stacktrace (at …, File "…")
    Frame,
    /// "Caused by:" und aehnliche Ueberschriften
    Cause,
    Plain,
}

pub fn classify(line: &str) -> LineKind {
    let t = line.trim_start();
    if t.is_empty() {
        return LineKind::Plain;
    }
    if t.starts_with("at ") && (t.contains('(') || t.contains(':')) || t.starts_with("File \"") || t.starts_with("... ") && t.ends_with("more") {
        return LineKind::Frame;
    }
    if t.starts_with("Caused by:") || t.starts_with("Traceback (most recent call last)") || t.starts_with("During handling of the above exception") || t.starts_with("The above exception") {
        return LineKind::Cause;
    }
    let lower = t.to_lowercase();
    if lower.starts_with("warning") || lower.starts_with("warnung") || lower.contains("deprecated") || lower.contains("veraltet") || t.starts_with("[WARNING]") || t.starts_with("(node:") {
        return LineKind::Warning;
    }
    if t.starts_with("Exception in thread")
        || t.starts_with("Error:")
        || t.starts_with("error:")
        || t.starts_with("Fehler:")
        || t.starts_with("[ERROR]")
        || t.starts_with("FAILED")
        || t.starts_with("Uncaught")
        || t.contains(".java:") && (t.contains(": error:") || t.contains(": Fehler:"))
        || t.split(':').next().is_some_and(|head| !head.contains(' ') && (head.ends_with("Exception") || head.ends_with("Error")))
    {
        return LineKind::Error;
    }
    LineKind::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA_TRACE: &str = "Loading class `com.mysql.jdbc.Driver'. This is deprecated. The new driver class is `com.mysql.cj.jdbc.Driver'.\n\
Exception in thread \"main\" java.lang.RuntimeException: java.sql.SQLSyntaxErrorException: Unknown database 'schule1'\n\
        at model.Main.main(Main.java:13)\n\
Caused by: java.sql.SQLSyntaxErrorException: Unknown database 'schule1'\n\
        at com.mysql.cj.jdbc.exceptions.SQLError.createSQLException(SQLError.java:112)\n";

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("easymysql-autofix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unknown_database_and_deprecated_driver_are_fixed() {
        let d = tmp("java");
        std::fs::create_dir_all(d.join("src/model")).unwrap();
        let f = d.join("src/model/DBManager.java");
        std::fs::write(&f, "class DBManager {\n  static { Class.forName(\"com.mysql.jdbc.Driver\"); }\n}\n").unwrap();
        let s = analyze_output(JAVA_TRACE, Some(&d), None, true);
        assert_eq!(s[0].fix, Some(Fix::CreateDatabase("schule1".into())));
        let Some(Fix::Edit(e)) = s.iter().find(|s| s.problem.contains("com.mysql.jdbc.Driver")).and_then(|s| s.fix.clone()) else { panic!("{s:?}") };
        assert_eq!(e.files[0].path, f);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(super::super::lsp::apply_edits(&text, &e.files[0].edits).contains("\"com.mysql.cj.jdbc.Driver\""));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn server_down_offers_start_or_settings() {
        let s = analyze_output("pymysql.err.OperationalError: (2003, \"Can't connect to MySQL server\")", None, None, true);
        assert_eq!(s[0].fix, Some(Fix::StartServer));
        let s = analyze_output("Error: connect ECONNREFUSED 127.0.0.1:3306", None, None, false);
        assert_eq!(s[0].fix, Some(Fix::ConnectionSettings));
    }

    #[test]
    fn missing_packages_go_into_the_project_files() {
        let d = tmp("pkg");
        std::fs::write(d.join("requirements.txt"), "PyMySQL>=1.1").unwrap();
        let s = analyze_output("ModuleNotFoundError: No module named 'mysql'", Some(&d), None, false);
        let Some(Fix::Edit(e)) = &s[0].fix else { panic!() };
        assert_eq!(super::super::lsp::apply_edits("PyMySQL>=1.1", &e.files[0].edits), "PyMySQL>=1.1\nmysql-connector-python\n");
        std::fs::write(d.join("package.json"), "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"mysql2\": \"^3\"\n  }\n}\n").unwrap();
        let s = analyze_output("Error: Cannot find module 'dotenv'\nRequire stack:", Some(&d), None, false);
        let Some(Fix::Edit(e)) = &s[0].fix else { panic!() };
        let t = super::super::lsp::apply_edits(&std::fs::read_to_string(d.join("package.json")).unwrap(), &e.files[0].edits);
        assert!(serde_json::from_str::<serde_json::Value>(&t).unwrap()["dependencies"]["dotenv"] == "latest", "{t}");
        std::fs::write(d.join("package.json"), "{\n  \"name\": \"x\"\n}\n").unwrap();
        let e = add_node_package(&d, "mysql2").unwrap();
        let t = super::super::lsp::apply_edits(&std::fs::read_to_string(d.join("package.json")).unwrap(), &e.files[0].edits);
        assert!(serde_json::from_str::<serde_json::Value>(&t).unwrap()["dependencies"]["mysql2"] == "latest", "{t}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn compiler_errors_get_fixes() {
        let f = PathBuf::from("/p/Main.java");
        let text = "package app;\n\npublic class Haupt {\n    public static void main(String[] args) {\n        Scanner s = new Scanner(System.in)\n        Connection c = DriverManager.getConnection(\"x\");\n    }\n}\n";
        let diag = |line, col: Option<usize>, m: &str| Diag { file: f.clone(), line, col, message: m.into(), error: true };
        let apply = |s: Option<Suggestion>| match s.and_then(|s| s.fix) {
            Some(Fix::Edit(e)) => super::super::lsp::apply_edits(text, &e.files[0].edits),
            x => panic!("{x:?}"),
        };
        assert!(apply(fix_for_diag(&diag(4, Some(42), "';' expected"), text)).contains("new Scanner(System.in);\n"));
        assert!(apply(fix_for_diag(&diag(4, Some(8), "cannot find symbol"), text)).starts_with("package app;\nimport java.util.Scanner;\n"));
        let t = apply(fix_for_diag(&diag(5, Some(50), "unreported exception java.sql.SQLException; must be caught or declared to be thrown"), text));
        assert!(t.contains("main(String[] args) throws SQLException {") && t.contains("import java.sql.SQLException;"), "{t}");
        let t = apply(fix_for_diag(&diag(2, Some(7), "class Haupt is public, should be declared in a file named Haupt.java"), text));
        assert!(t.contains("public class Main {"), "{t}");
    }

    #[test]
    fn java_fixes_vor_dem_kompilieren() {
        let text = "package app;\nclass Main { void f() {\n  List<String> namen = new ArrayList<>();\n  System.out.printLn(namen); // System.err.printLn(x)\n  String s = \"Connection System.out.printLn(x)\";\n} }\n";
        let (fixed, n) = crate::sqledit::apply_fixes(text, &java_probleme(text));
        assert_eq!(n, 2);
        assert!(fixed.contains("import java.util.List;\nimport java.util.ArrayList;"));
        assert!(fixed.contains("System.out.println(namen)"));
        assert!(!fixed.contains("import java.sql.Connection"));
        assert!(fixed.contains("// System.err.printLn(x)"));
        assert!(java_probleme(&fixed).is_empty());
        assert!(java_probleme("import java.util.*;\nclass Main { List<String> x; }").is_empty());
        assert!(java_probleme("class List {}\nclass Main { List x; }").is_empty());
        let text = "System.out.println(\"https://beispiel.at\") // Ausgabe\n";
        let (fixed, n) = crate::sqledit::apply_fixes(text, &java_probleme(text));
        assert_eq!(n, 1);
        assert_eq!(fixed, "System.out.println(\"https://beispiel.at\"); // Ausgabe\n");
        assert!(java_probleme("System.out.println(\n  \"mehrzeilig\"\n);\n").is_empty());
    }

    #[test]
    fn sql_fehler_liefern_struktur_statt_leerer_tabelle() {
        let dir = tmp("struktur");
        let file = dir.join("Main.java");
        std::fs::write(&file, "q = \"INSERT INTO kunde (id, name) VALUES (1, 'Ada')\";").unwrap();
        let fixes = analyze_output("Table 'schule.kunde' doesn't exist", Some(&dir), Some(&file), false);
        assert!(matches!(&fixes[0].fix, Some(Fix::Schema { db: Some(db), table, columns }) if db == "schule" && table == "kunde" && columns.len() == 2));
        let fixes = analyze_output("Unknown column 'name' in 'field list'", Some(&dir), Some(&file), false);
        assert!(matches!(&fixes[0].fix, Some(Fix::Schema { columns, .. }) if columns == &vec![("name".into(), "TEXT".into())]));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn struktur_ergaenzt_nur_fehlende_spalten_und_quotiert_namen() {
        let mut schema = crate::db::Schema { name: "schule".into(), ..Default::default() };
        let cols = vec![("id".into(), "BIGINT".into()), ("na`me".into(), "TEXT".into())];
        let sql = struktur_sql(&schema, "kunde", &cols).unwrap();
        assert!(sql.starts_with("CREATE TABLE `schule`.`kunde`"));
        assert!(sql.contains("`na``me` TEXT NULL"));
        schema.tables.push(crate::db::TableInfo { name: "kunde".into(), columns: vec![crate::db::ColumnInfo { name: "ID".into(), ..Default::default() }], ..Default::default() });
        let sql = struktur_sql(&schema, "kunde", &cols).unwrap();
        assert!(sql.starts_with("ALTER TABLE"));
        assert!(!sql.contains("BIGINT"));
        schema.tables[0].columns.push(crate::db::ColumnInfo { name: "na`me".into(), ..Default::default() });
        assert!(struktur_sql(&schema, "kunde", &cols).is_none());
    }

    #[test]
    fn projektmigration_bevorzugen_und_abhaengigkeiten_einbeziehen() {
        let root = tmp("deklaration");
        let migrations = root.join("src/main/resources/db/migration");
        std::fs::create_dir_all(&migrations).unwrap();
        std::fs::write(migrations.join("V1__basis.sql"), "CREATE TABLE kunde (id BIGINT PRIMARY KEY AUTO_INCREMENT); CREATE TABLE auftrag (id BIGINT PRIMARY KEY, kunde_id BIGINT, FOREIGN KEY (kunde_id) REFERENCES kunde(id)); CREATE TABLE fremd (id INT);").unwrap();
        std::fs::write(migrations.join("V2__spalte.sql"), "ALTER TABLE auftrag ADD name VARCHAR(80) DEFAULT 'neu';").unwrap();
        std::fs::write(migrations.join("V10__spalte.sql"), "ALTER TABLE auftrag ADD aktiv BOOLEAN DEFAULT TRUE;").unwrap();
        let fix = java_struktur_fix(Some(&root), None, Some("shop"), Some("auftrag"), None).unwrap();
        let Fix::DeklarierteStruktur { datenbank, plaene, .. } = fix else { panic!() };
        assert_eq!(datenbank.as_deref(), Some("shop"));
        assert_eq!(plaene.len(), 2);
        assert!(plaene.iter().any(|p| p.tabelle == "kunde"));
        let sql = crate::sqledit::plan_sql(&plaene, &crate::db::Schema { name: "shop".into(), ..Default::default() }, &[]).unwrap();
        assert!(sql.contains("VARCHAR(80) DEFAULT 'neu'")); assert!(!sql.contains("`fremd`"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "braucht isolierten MariaDB-Testserver auf EASYMYSQL_AUTOFIX_TEST_PORT"]
    fn struktur_gegen_mariadb() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let dbc = crate::db::Db::connect(&crate::db::ConnInfo { port, ..Default::default() }).unwrap();
        let db = format!("autofix_test_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {}", crate::db::q(&db)), ()).unwrap();
        let java = "q = \"INSERT INTO kunde (id, name) VALUES (1, 'Ada')\";";
        let (_, table, columns) = crate::sqledit::java_struktur(java, Some(&db), Some("kunde"), None).unwrap();
        let sql = struktur_sql(&dbc.schema(&db).unwrap(), &table, &columns).unwrap();
        dbc.exec_in(&db, &sql).unwrap();
        dbc.exec_in(&db, "INSERT INTO kunde (id, name) VALUES (1, 'Ada')").unwrap();
        let columns = vec![("name".into(), "TEXT".into()), ("aktiv".into(), "BOOLEAN".into())];
        let sql = struktur_sql(&dbc.schema(&db).unwrap(), &table, &columns).unwrap();
        dbc.exec_in(&db, &sql).unwrap();
        let rows = dbc.query(&format!("SELECT name FROM {}.kunde WHERE id = 1", crate::db::q(&db))).unwrap();
        assert_eq!(rows.rows[0][0].as_deref(), Some("Ada"));
        assert!(struktur_sql(&dbc.schema(&db).unwrap(), &table, &columns).is_none());
        let plans = crate::sqledit::migrationen("CREATE TABLE kunde (id BIGINT PRIMARY KEY, name TEXT); CREATE TABLE auftrag (id BIGINT AUTO_INCREMENT PRIMARY KEY, kunde_id BIGINT NOT NULL, status VARCHAR(20) DEFAULT 'offen', FOREIGN KEY (kunde_id) REFERENCES kunde(id) ON DELETE RESTRICT);").unwrap();
        let schema = dbc.schema(&db).unwrap();
        let existing = schema.tables.iter().flat_map(|t| crate::sqledit::migrationen(&dbc.create_statement(&db, &t.name).unwrap()).unwrap()).collect::<Vec<_>>();
        let sql = crate::sqledit::plan_sql(&plans, &schema, &existing).unwrap();
        crate::sqledit::pruefe_daten(&dbc, &db, &plans).unwrap();
        dbc.exec_schema(&db, &sql).unwrap();
        dbc.exec_in(&db, "INSERT INTO auftrag (kunde_id) VALUES (1)").unwrap();
        assert!(dbc.exec_in(&db, "INSERT INTO auftrag (kunde_id) VALUES (999)").is_err());
        let rows = dbc.query(&format!("SELECT status FROM {}.auftrag", crate::db::q(&db))).unwrap();
        assert_eq!(rows.rows[0][0].as_deref(), Some("offen"));
        let schema = dbc.schema(&db).unwrap();
        let existing = schema.tables.iter().flat_map(|t| crate::sqledit::migrationen(&dbc.create_statement(&db, &t.name).unwrap()).unwrap()).collect::<Vec<_>>();
        assert!(crate::sqledit::plan_sql(&plans, &schema, &existing).is_none());
        let required = crate::sqledit::migrationen("ALTER TABLE kunde ADD pflicht INT NOT NULL").unwrap();
        assert!(crate::sqledit::pruefe_daten(&dbc, &db, &required).is_err());
        dbc.exec_in(&db, "CREATE TABLE ungueltig (n INT NULL); INSERT INTO ungueltig VALUES (NULL),(1),(1)").unwrap();
        for sql in ["ALTER TABLE ungueltig ADD PRIMARY KEY(n)", "ALTER TABLE ungueltig ADD UNIQUE KEY uk_n(n)"] {
            let plan = crate::sqledit::migrationen(sql).unwrap();
            assert!(crate::sqledit::pruefe_daten(&dbc, &db, &plan).is_err());
        }
        assert_eq!(dbc.query(&format!("SELECT COUNT(*) FROM {}.ungueltig", crate::db::q(&db))).unwrap().rows[0][0].as_deref(), Some("3"));
        dbc.exec(&format!("DROP DATABASE {}", crate::db::q(&db)), ()).unwrap();
    }

    #[test]
    fn semikolon_nach_jdbc_url_und_klammern_ohne_strings() {
        let file = PathBuf::from("Main.java");
        let text = "class Main {\n void f() {\n String url = \"jdbc:mysql://localhost/db\" // Kommentar\n";
        let d = Diag { file: file.clone(), line: 2, col: None, message: "';' expected".into(), error: true };
        let Some(Fix::Edit(edit)) = fix_for_diag(&d, text).and_then(|s| s.fix) else { panic!() };
        assert!(super::super::lsp::apply_edits(text, &edit.files[0].edits).contains("\"jdbc:mysql://localhost/db\"; //"));
        let d = Diag { file, line: 2, col: None, message: "reached end of file while parsing".into(), error: true };
        let Some(Fix::Edit(edit)) = fix_for_diag(&d, text).and_then(|s| s.fix) else { panic!() };
        let fixed = super::super::lsp::apply_edits(text, &edit.files[0].edits);
        assert!(fixed.ends_with("}\n}\n"));
        assert!(fix_for_diag(&d, &fixed).is_none());
    }

    #[test]
    fn links_and_line_kinds() {
        let l = links("        at model.Main.main(Main.java:13)");
        assert_eq!(l, vec![Link { start: 27, end: 39, file: "Main.java".into(), line: 12 }]);
        let l = links("  File \"/p/main.py\", line 5, in <module>");
        assert_eq!((l[0].file.as_str(), l[0].line), ("/p/main.py", 4));
        let l = links("    at Object.<anonymous> (/p/index.js:10:5)");
        assert_eq!((l[0].file.as_str(), l[0].line), ("/p/index.js", 9));
        assert!(links("Zeit 12:30 Uhr").is_empty());
        assert_eq!(classify("Exception in thread \"main\" java.lang.RuntimeException: x"), LineKind::Error);
        assert_eq!(classify("java.sql.SQLException: Access denied"), LineKind::Error);
        assert_eq!(classify("        at model.Main.main(Main.java:13)"), LineKind::Frame);
        assert_eq!(classify("Caused by: java.sql.SQLSyntaxErrorException: x"), LineKind::Cause);
        assert_eq!(classify("Loading class `com.mysql.jdbc.Driver'. This is deprecated."), LineKind::Warning);
        assert_eq!(classify("Verbunden mit jdbc:mariadb://x"), LineKind::Plain);
        assert_eq!(strip_ansi("\x1b[31mrot\x1b[0m\r\nok"), "rot\nok");
    }

    #[test]
    fn java_links_resolve_through_the_package() {
        let d = tmp("res");
        std::fs::create_dir_all(d.join("src/model")).unwrap();
        std::fs::create_dir_all(d.join("src/other")).unwrap();
        std::fs::write(d.join("src/other/Main.java"), "").unwrap();
        std::fs::write(d.join("src/model/Main.java"), "").unwrap();
        assert_eq!(resolve(Some(&d), None, "\tat model.Main.main(Main.java:13)", "Main.java"), Some(d.join("src/model/Main.java")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
