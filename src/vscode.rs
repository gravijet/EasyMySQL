// Anbindung an Visual Studio Code: Arbeitsordner mit fertiger SQLTools-Verbindung,
// Oeffnen von Abfragen und Installation der Erweiterungen (SQLTools, GitHub Copilot).

use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXTENSIONS: &[(&str, &str)] = &[
    ("mtxr.sqltools", "SQLTools (Datenbank-Explorer und Abfragen)"),
    ("mtxr.sqltools-driver-mysql", "SQLTools-Treiber für MySQL/MariaDB"),
];

/// Neuere VS-Code-Versionen haben GitHub Copilot fest eingebaut; nur aeltere brauchen die Erweiterung.
const COPILOT_EXTENSION: &str = "GitHub.copilot-chat";

/// Installationsordner von VS Code (enthaelt resources/).
fn vscode_root(exe: &Path) -> Option<PathBuf> {
    let p = exe.parent()?;
    if p.file_name().is_some_and(|n| n == "bin") {
        p.parent().map(|x| x.to_path_buf())
    } else {
        Some(p.to_path_buf())
    }
}

fn copilot_builtin(exe: &Path) -> bool {
    vscode_root(exe).is_some_and(|r| {
        let ext = r.join("resources").join("app").join("extensions");
        ext.join("copilot").is_dir() || ext.join("copilot-chat").is_dir()
    })
}

/// Ordner "Dokumente\EasyMySQL" fuer SQL-Dateien.
pub fn workspace_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::server::app_data_dir());
    let docs = home.join("Documents");
    let base = if docs.is_dir() { docs } else { home };
    base.join("EasyMySQL")
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Legt den Arbeitsordner samt .vscode-Einstellungen an.
pub fn prepare_workspace(database: &str) -> Result<PathBuf, String> {
    let dir = workspace_dir();
    let vs = dir.join(".vscode");
    std::fs::create_dir_all(&vs).map_err(|e| format!("{}: {e}", vs.display()))?;
    let settings = format!(
        r#"{{
  "sqltools.connections": [
    {{
      "name": "EasyMySQL",
      "driver": "MariaDB",
      "server": "127.0.0.1",
      "port": 3306,
      "username": "root",
      "password": "",
      "askForPassword": false,
      "database": {},
      "previewLimit": 100,
      "connectionTimeout": 15
    }}
  ],
  "sqltools.autoConnectTo": ["EasyMySQL"],
  "sqltools.format": {{ "language": "sql", "reservedWordCase": "upper", "linesBetweenQueries": 2 }},
  "files.associations": {{ "*.sql": "sql" }},
  "github.copilot.enable": {{ "*": true, "sql": true }}
}}
"#,
        json_str(database)
    );
    std::fs::write(vs.join("settings.json"), settings).map_err(|e| e.to_string())?;
    let recs: Vec<String> = EXTENSIONS.iter().map(|(id, _)| json_str(id)).collect();
    // (Copilot ist in aktuellen VS-Code-Versionen eingebaut und wird daher nicht empfohlen)
    std::fs::write(
        vs.join("extensions.json"),
        format!("{{\n  \"recommendations\": [{}]\n}}\n", recs.join(", ")),
    )
    .map_err(|e| e.to_string())?;
    let readme = dir.join("LIESMICH.txt");
    if !readme.exists() {
        let _ = std::fs::write(
            &readme,
            "In diesem Ordner speichert EasyMySQL Abfragen für Visual Studio Code.\r\n\r\n\
             Beim ersten Öffnen fragt VS Code, ob Sie dem Ordner vertrauen: \"Ja\" / \"Trust\" wählen.\r\n\r\n\
             In VS Code (Erweiterung SQLTools):\r\n\
             - Links auf das Datenbank-Symbol klicken: Verbindung \"EasyMySQL\" (127.0.0.1, root)\r\n\
             - In einer .sql-Datei: Strg+E Strg+E führt die aktuelle Abfrage aus\r\n\
             - GitHub Copilot macht beim Tippen Vorschläge (Tab übernimmt)\r\n\r\n\
             Der Datenbankserver läuft, solange EasyMySQL geöffnet ist.\r\n",
        );
    }
    Ok(dir)
}

/// Sucht VS Code. Liefert (Programm, CLI-Skript).
pub fn find_vscode() -> Option<(PathBuf, Option<PathBuf>)> {
    #[cfg(windows)]
    {
        let mut cands: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            cands.push(PathBuf::from(p).join("Programs").join("Microsoft VS Code").join("Code.exe"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(p) = std::env::var(var) {
                cands.push(PathBuf::from(p).join("Microsoft VS Code").join("Code.exe"));
            }
        }
        if let Ok(path) = std::env::var("PATH") {
            for d in std::env::split_paths(&path) {
                if d.join("code.cmd").exists() {
                    if let Some(parent) = d.parent() {
                        cands.push(parent.join("Code.exe"));
                    }
                }
            }
        }
        for exe in cands {
            if exe.exists() {
                let cli = exe.parent().map(|p| p.join("bin").join("code.cmd")).filter(|p| p.exists());
                return Some((exe, cli));
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let path = std::env::var("PATH").ok()?;
        std::env::split_paths(&path)
            .map(|d| d.join("code"))
            .find(|p| p.exists())
            .map(|p| (p.clone(), Some(p)))
    }
}

pub const NOT_FOUND: &str = "Visual Studio Code wurde nicht gefunden.\n\n\
Bitte VS Code von https://code.visualstudio.com installieren und danach in EasyMySQL \
\"Werkzeuge → VS Code einrichten\" wählen.";

fn launch(exe: &Path, args: &[&Path]) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    for a in args {
        cmd.arg(a);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("VS Code konnte nicht gestartet werden: {e}"))
}

/// Oeffnet den Arbeitsordner in VS Code.
pub fn open_workspace(database: &str) -> Result<PathBuf, String> {
    let dir = prepare_workspace(database)?;
    let (exe, _) = find_vscode().ok_or_else(|| NOT_FOUND.to_string())?;
    launch(&exe, &[&dir])?;
    Ok(dir)
}

/// Speichert eine Abfrage im Arbeitsordner und oeffnet sie in VS Code.
pub fn open_sql(file_name: &str, sql: &str, database: &str) -> Result<PathBuf, String> {
    let dir = prepare_workspace(database)?;
    let (exe, _) = find_vscode().ok_or_else(|| NOT_FOUND.to_string())?;
    let mut name: String = file_name
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' })
        .collect();
    if !name.to_lowercase().ends_with(".sql") {
        name.push_str(".sql");
    }
    let file = dir.join(name);
    let mut content = String::new();
    if !database.is_empty() && !sql.to_uppercase().contains("USE ") {
        content.push_str(&format!("USE {};\n\n", crate::db::q(database)));
    }
    content.push_str(sql);
    std::fs::write(&file, content).map_err(|e| e.to_string())?;
    launch(&exe, &[&dir, &file])?;
    Ok(file)
}

/// Installiert die Erweiterungen (blockierend, im Hintergrund-Thread aufrufen).
pub fn install_extensions() -> Result<String, String> {
    let (exe, cli) = find_vscode().ok_or_else(|| NOT_FOUND.to_string())?;
    let cli = cli.ok_or("Das VS-Code-Kommandozeilenprogramm (code) wurde nicht gefunden.")?;
    let builtin = copilot_builtin(&exe);
    let mut list: Vec<(&str, &str)> = EXTENSIONS.to_vec();
    if !builtin {
        list.push((COPILOT_EXTENSION, "GitHub Copilot"));
    }
    let mut report = String::new();
    for (id, name) in list {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&cli);
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = Command::new(&cli);
        let out = cmd
            .args(["--install-extension", id, "--force"])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            report.push_str(&format!("✔ {name}\n"));
        } else {
            let err = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
            let line = err
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty() && !l.contains("eprecat"))
                .unwrap_or("Fehler")
                .to_string();
            report.push_str(&format!("✖ {name}: {line}\n"));
        }
    }
    if builtin {
        report.push_str("✔ GitHub Copilot (in VS Code bereits eingebaut)\n");
    }
    Ok(report)
}
