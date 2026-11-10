// EasyMySQL in KI-Assistenten eintragen: MCP-Server und Skill fuer Claude Code und Codex.
// Claude Code: `claude mcp add --scope user …` und ~/.claude/skills/easymysql/SKILL.md
// Codex: Abschnitt [mcp_servers.easymysql] in ~/.codex/config.toml (vorher gesichert) und ~/.agents/skills/easymysql/SKILL.md

use std::path::{Path, PathBuf};
use std::process::Command;

/// Anleitung fuer die KI, dieselbe wie im Plugin des Repositorys
pub const SKILL: &str = include_str!("../../plugins/easymysql/skills/easymysql/SKILL.md");

const NAME: &str = "easymysql";
/// Kommentarzeile ueber unserem Abschnitt in config.toml (wird beim Ersetzen mit entfernt)
const MARK: &str = "# EasyMySQL: MariaDB-Datenbanken fuer Codex (easymysql setup-ai)";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Claude,
    Codex,
}

impl Target {
    pub fn program(self) -> Option<PathBuf> {
        find_program(match self { Self::Claude => "claude", Self::Codex => "codex" })
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Claude => "Claude Code",
            Target::Codex => "Codex",
        }
    }
}

fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| home().map(|h| h.join(".codex")))
}

fn skill_dir(t: Target) -> Option<PathBuf> {
    let h = home()?;
    Some(match t {
        Target::Claude => h.join(".claude").join("skills").join(NAME),
        Target::Codex => h.join(".agents").join("skills").join(NAME),
    })
}

/// TOML-Zeichenkette in Anfuehrungszeichen (Windows-Pfade enthalten Backslashes)
pub fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Kopfzeile einer TOML-Tabelle (`[a.b]`), ohne Kommentar; None fuer andere Zeilen
fn table_header(line: &str) -> Option<String> {
    let t = line.trim();
    let t = t.split_once(" #").map(|(a, _)| a.trim()).unwrap_or(t);
    (t.starts_with('[') && t.ends_with(']')).then(|| t.trim_matches(['[', ']']).trim().replace(['"', '\''], ""))
}

/// Unseren Abschnitt (samt Untertabellen wie `.env`) aus einer config.toml entfernen
pub fn remove_codex_block(text: &str) -> String {
    let own = |h: &str| h == format!("mcp_servers.{NAME}") || h.starts_with(&format!("mcp_servers.{NAME}."));
    let mut out = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if let Some(h) = table_header(line) {
            skipping = own(&h);
        }
        if !skipping && line.trim() != MARK {
            out.push(line);
        }
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// config.toml mit (neuem) Abschnitt fuer EasyMySQL
pub fn codex_config(text: &str, exe: &Path) -> String {
    let mut s = remove_codex_block(text);
    if !s.is_empty() {
        s.push('\n');
    }
    s.push_str(&format!(
        "# EasyMySQL: MariaDB-Datenbanken fuer Codex (easymysql setup-ai)\n[mcp_servers.{NAME}]\ncommand = {}\nargs = [\"mcp\"]\nstartup_timeout_sec = 30\ntool_timeout_sec = 300\n",
        toml_str(&exe.display().to_string())
    ));
    s
}

fn write_skill(t: Target) -> Result<PathBuf, String> {
    let dir = skill_dir(t).ok_or("home folder not found")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join("SKILL.md");
    std::fs::write(&file, SKILL).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

fn remove_skill(t: Target) {
    if let Some(d) = skill_dir(t) {
        let _ = std::fs::remove_file(d.join("SKILL.md"));
        let _ = std::fs::remove_dir(d);
    }
}

/// Programm im PATH suchen (unter Windows auch .exe und .cmd, wie npm sie anlegt)
fn find_program(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    let mut dirs = Vec::new();
    for (_, dir) in crate::dev::versioned_dirs(&crate::dev::tool_root().join("ki"), &format!("{name}-")) {
        dirs.push(dir.join("bin"));
        dirs.push(dir);
    }
    dirs.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
    // Der eigene Installer von Claude Code legt das Programm hier ab, auch wenn PATH noch nicht neu geladen ist
    if let Some(h) = home() {
        dirs.push(h.join(".local").join("bin"));
        dirs.push(h.join(".claude").join("local"));
        dirs.push(h.join(".npm-global/bin"));
        if cfg!(windows) { dirs.push(h.join("AppData/Roaming/npm")); }
    }
    dirs.iter().flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}")))).find(|p| p.is_file())
}

fn setup_codex(exe: &Path) -> Result<String, String> {
    let home = codex_home().ok_or("home folder not found")?;
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let file = home.join("config.toml");
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    if !old.is_empty() {
        let backup = home.join("config.toml.bak-easymysql");
        std::fs::write(&backup, &old).map_err(|e| format!("{}: {e}", backup.display()))?;
    }
    std::fs::write(&file, codex_config(&old, exe)).map_err(|e| format!("{}: {e}", file.display()))?;
    let skill = write_skill(Target::Codex)?;
    Ok(format!("MCP server in {}, skill in {}", file.display(), skill.display()))
}

fn remove_codex() -> Result<String, String> {
    let file = codex_home().ok_or("home folder not found")?.join("config.toml");
    if let Ok(old) = std::fs::read_to_string(&file) {
        std::fs::write(&file, remove_codex_block(&old)).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    remove_skill(Target::Codex);
    Ok(format!("removed from {}", file.display()))
}

fn claude(args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    let program = find_program("claude").ok_or("Claude Code (`claude`) was not found in PATH")?;
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null());
    crate::server::hidden(&mut cmd).output().map_err(|e| e.to_string())
}

fn setup_claude(exe: &Path) -> Result<String, String> {
    let manual = format!("claude mcp add --scope user {NAME} -- \"{}\" mcp", exe.display());
    // Ein frueherer Eintrag (anderer Pfad) wuerde sonst das Anlegen verhindern
    let _ = claude(&["mcp".as_ref(), "remove".as_ref(), "--scope".as_ref(), "user".as_ref(), NAME.as_ref()]);
    let out = claude(&["mcp".as_ref(), "add".as_ref(), "--scope".as_ref(), "user".as_ref(), NAME.as_ref(), "--".as_ref(), exe.as_os_str(), "mcp".as_ref()])
        .map_err(|e| format!("{e}. Install Claude Code, then run: {manual}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("`claude mcp add` failed: {err}. Run it yourself: {manual}"));
    }
    let skill = write_skill(Target::Claude)?;
    Ok(format!("MCP server (user scope), skill in {}", skill.display()))
}

fn remove_claude() -> Result<String, String> {
    remove_skill(Target::Claude);
    let out = claude(&["mcp".as_ref(), "remove".as_ref(), "--scope".as_ref(), "user".as_ref(), NAME.as_ref()])?;
    if out.status.success() { Ok("removed".into()) } else { Ok("skill removed; no MCP entry was registered".into()) }
}

/// Einrichten (oder mit `remove` entfernen); je Ziel eine Meldung
pub fn apply(targets: &[Target], remove: bool) -> Vec<(Target, Result<String, String>)> {
    let exe = crate::updates::exe_path();
    targets
        .iter()
        .map(|&t| {
            let r = match (t, remove) {
                (Target::Claude, false) => setup_claude(&exe),
                (Target::Claude, true) => remove_claude(),
                (Target::Codex, false) => setup_codex(&exe),
                (Target::Codex, true) => remove_codex(),
            };
            (t, r)
        })
        .collect()
}

/// Ist das Programm des Assistenten auf diesem PC zu finden?
pub fn available(t: Target) -> bool {
    match t {
        Target::Claude => find_program("claude").is_some() || home().is_some_and(|h| h.join(".claude").is_dir()),
        Target::Codex => find_program("codex").is_some() || codex_home().is_some_and(|h| h.is_dir()),
    }
}

/// `easymysql setup-ai [claude|codex|all] [--remove]`
pub fn run(args: &[String]) -> i32 {
    let remove = args.iter().any(|a| a == "--remove" || a == "remove");
    let which: Vec<&str> = args.iter().map(String::as_str).filter(|a| !matches!(*a, "--remove" | "remove")).collect();
    let targets: Vec<Target> = match which.as_slice() {
        [] | ["all"] => {
            // Ohne Angabe: alle gefundenen Assistenten (keiner gefunden: beide versuchen und erklaeren)
            let found: Vec<Target> = [Target::Claude, Target::Codex].into_iter().filter(|t| available(*t)).collect();
            if found.is_empty() { vec![Target::Claude, Target::Codex] } else { found }
        }
        ["claude"] => vec![Target::Claude],
        ["codex"] => vec![Target::Codex],
        _ => {
            eprintln!("Usage: easymysql setup-ai [claude|codex|all] [--remove]");
            return 2;
        }
    };
    let mut failed = false;
    for (t, r) in apply(&targets, remove) {
        match r {
            Ok(m) => println!("  ✓ {:<12} {m}", t.label()),
            Err(e) => {
                failed = true;
                println!("  ✗ {:<12} {e}", t.label());
            }
        }
    }
    if !remove && !failed {
        println!("\nRestart Claude Code / Codex, then ask for example: \"Show me the tables in my shop database\".");
    }
    if failed { 1 } else { 0 }
}

/// `easymysql install-ai [claude|codex|all]`: Programme laden (neueste Version mit Pruefsumme, ohne Node.js) und
/// danach wie `setup-ai` eintragen. Fuer die Installer; bereits vorhandene Programme bleiben.
pub fn install(args: &[String]) -> i32 {
    let targets: Vec<Target> = match args.first().map(String::as_str) {
        None | Some("all") => vec![Target::Claude, Target::Codex],
        Some("claude") => vec![Target::Claude],
        Some("codex") => vec![Target::Codex],
        _ => {
            eprintln!("Usage: easymysql install-ai [claude|codex|all]");
            return 2;
        }
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut failed = false;
    let mut ready = Vec::new();
    for t in targets {
        if let Some(p) = t.program() {
            println!("  ✓ {:<12} {}", t.label(), p.display());
            ready.push(t);
            continue;
        }
        println!("  … {:<12} downloading", t.label());
        match crate::dev::install::install_ai(t, &cancel) {
            Ok(p) => {
                println!("  ✓ {:<12} {}", t.label(), p.display());
                ready.push(t);
            }
            Err(e) => {
                failed = true;
                println!("  ✗ {:<12} {e}", t.label());
            }
        }
    }
    for (t, r) in apply(&ready, false) {
        match r {
            Ok(m) => println!("  ✓ {:<12} {m}", t.label()),
            Err(e) => {
                failed = true;
                println!("  ✗ {:<12} {e}", t.label());
            }
        }
    }
    if !ready.is_empty() {
        println!("\nSign in once in EasyMySQL (Claude/Codex panel on the left → Sign in) or with: claude auth login / codex login");
    }
    if failed { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_block_is_replaced_not_duplicated() {
        let exe = Path::new(r"C:\Program Files\EasyMySQL\EasyMySQL.exe");
        let start = "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        let once = codex_config(start, exe);
        assert!(once.starts_with(start));
        assert!(once.contains(r#"command = "C:\\Program Files\\EasyMySQL\\EasyMySQL.exe""#), "{once}");
        let twice = codex_config(&(once.clone() + "[mcp_servers.easymysql.env]\nA = \"1\"\n[profiles.x]\nmodel = \"o3\"\n"), exe);
        assert_eq!(twice.matches("[mcp_servers.easymysql]").count(), 1);
        assert!(!twice.contains("A = \"1\""), "Untertabellen gehoeren zum alten Eintrag");
        assert!(twice.contains("[profiles.x]") && twice.contains("[mcp_servers.other]"));
        let removed = remove_codex_block(&twice);
        assert!(!removed.contains("easymysql]") && removed.contains("[profiles.x]"));
        assert_eq!(codex_config("", exe).lines().next(), Some(MARK));
        assert_eq!(codex_config(&once, exe).matches(MARK).count(), 1);
    }

    #[test]
    fn quoted_table_names_are_recognized() {
        let text = "[mcp_servers.\"easymysql\"]\ncommand = \"alt\"\n[other]\nx = 1\n";
        assert_eq!(remove_codex_block(text), "[other]\nx = 1\n");
    }

    #[test]
    fn toml_strings_escape() {
        assert_eq!(toml_str(r#"a\b"c"#), r#""a\\b\"c""#);
        assert_eq!(toml_str("x\u{1}"), "\"x\\u0001\"");
    }

    #[test]
    fn skill_has_frontmatter() {
        let mut zeilen = SKILL.lines();
        assert_eq!(zeilen.next(), Some("---"));
        assert_eq!(zeilen.next(), Some("name: easymysql"));
        assert!(zeilen.next().is_some_and(|zeile| zeile.starts_with("description: ")), "SKILL.md braucht eine Beschreibung");
    }
}
