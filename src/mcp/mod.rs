// KI-Anbindung: MCP-Server (Model Context Protocol) ueber stdin/stdout und Einrichtung in Claude Code und Codex.
//
//   easymysql mcp [--allow-write | --read-only] [--database DB] [--profile NAME] [--host H] [--port P] [--user U] [--max-rows N]
//   easymysql setup-ai [claude | codex | all] [--remove]
//   easymysql --version | --help

pub mod guard;
pub mod protocol;
pub mod setup;
mod tools;

pub use tools::Options;

/// Gesetzt, wenn das MCP-Werkzeug `start_server` EasyMySQL oeffnet: der Server startet dann auch ohne Autostart
pub const START_SERVER_VAR: &str = "EASYMYSQL_START_SERVER";

/// Passwort fuer `easymysql mcp` (statt es in eine Datei zu schreiben)
pub const PASSWORD_VAR: &str = "EASYMYSQL_PASSWORD";

const HELP: &str = "EasyMySQL – MariaDB server and database manager

Usage:
  easymysql                      open the app (files given as arguments are opened)
  easymysql mcp [options]        run the MCP server for AI assistants on stdin/stdout
  easymysql setup-ai [target]    register the MCP server and skill in Claude Code and/or Codex
                                 target: all (default), claude, codex; --remove undoes it
  easymysql --version            print the version

MCP options (defaults come from the EasyMySQL settings):
  --allow-write | --read-only    allow or forbid changes (Settings → AI assistants)
  --database NAME                default database for tools that need one
  --profile NAME                 use a saved server instead of the default connection
  --host HOST --port PORT --user USER
  --max-rows N                   rows per query result (default 200)
  The password comes from the settings or the EASYMYSQL_PASSWORD environment variable.
";

/// Kommandozeile der MCP-Sitzung
#[derive(Debug, Default, PartialEq)]
pub struct McpArgs {
    pub allow_write: Option<bool>,
    pub database: Option<String>,
    pub profile: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub max_rows: Option<usize>,
}

pub fn parse_mcp_args(args: &[String]) -> Result<McpArgs, String> {
    let mut out = McpArgs::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next().cloned()).filter(|v| !v.is_empty()).ok_or(format!("{key} needs a value"));
        match key {
            "--allow-write" => out.allow_write = Some(true),
            "--read-only" => out.allow_write = Some(false),
            "--database" | "-d" => out.database = Some(value()?),
            "--profile" => out.profile = Some(value()?),
            "--host" | "-h" => out.host = Some(value()?),
            "--port" | "-P" => out.port = Some(value()?.parse().map_err(|_| "--port needs a number")?),
            "--user" | "-u" => out.user = Some(value()?),
            "--max-rows" => out.max_rows = Some(value()?.parse().map_err(|_| "--max-rows needs a number")?),
            other => return Err(format!("Unknown option {other}")),
        }
    }
    Ok(out)
}

/// Sitzung aus Einstellungen und Kommandozeile zusammensetzen
pub fn options(s: &crate::settings::Settings, a: &McpArgs) -> Result<Options, String> {
    let (mut conn, mut ssh) = (s.conn.clone(), s.ssh.clone());
    if let Some(name) = &a.profile {
        let p = crate::profiles::find(name).ok_or_else(|| format!("No saved server named {name:?}"))?;
        (conn, ssh) = (p.conn, p.ssh);
    }
    if let Some(h) = &a.host {
        conn.host = h.clone();
        ssh.enabled = false;
    }
    if let Some(p) = a.port {
        conn.port = p;
    }
    if let Some(u) = &a.user {
        conn.user = u.clone();
    }
    if let Ok(pw) = std::env::var(PASSWORD_VAR) {
        conn.password = pw;
    }
    Ok(Options {
        conn,
        ssh,
        database: a.database.clone().or_else(|| Some(s.ai.database.clone()).filter(|d| !d.is_empty())),
        allow_write: a.allow_write.unwrap_or(s.ai.allow_write),
        backup: s.backup_before_delete,
        max_rows: a.max_rows.unwrap_or(s.ai.max_rows),
        server: s.server.clone(),
    })
}

/// Unter Windows ist EasyMySQL ein Fensterprogramm: fuer Textausgaben die Konsole des Aufrufers verwenden
fn attach_console() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
        // Umgeleitete Ausgabe (Pipe, Datei) bleibt, wie sie ist
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// Befehle der Kommandozeile ausfuehren. `None`: kein Befehl, die Oberflaeche startet.
pub fn run_cli(args: &[std::ffi::OsString]) -> Option<i32> {
    let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let first = args.first()?.as_str();
    match first {
        "mcp" => Some(run_mcp(&args[1..])),
        "setup-ai" => {
            attach_console();
            Some(setup::run(&args[1..]))
        }
        "--version" | "-V" | "version" => {
            attach_console();
            println!("EasyMySQL {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        // Fuer die Pakete unter macOS und Linux: Programmsymbol als PNG (256 x 256) schreiben
        "__write-icon" => {
            let rgba = include_bytes!(concat!(env!("OUT_DIR"), "/icon256.rgba"));
            let ok = args.get(1).is_some_and(|p| image::save_buffer(p, rgba, 256, 256, image::ExtendedColorType::Rgba8).is_ok());
            Some(if ok { 0 } else { 1 })
        }
        "--help" | "-?" | "help" => {
            attach_console();
            print!("{HELP}");
            Some(0)
        }
        _ => None,
    }
}

fn run_mcp(args: &[String]) -> i32 {
    let settings = crate::settings::Settings::load();
    crate::i18n::install(crate::i18n::Language::English);
    let opts = match parse_mcp_args(args).and_then(|a| options(&settings, &a)) {
        Ok(o) => o,
        Err(e) => {
            attach_console();
            eprintln!("easymysql mcp: {e}\n\n{HELP}");
            return 2;
        }
    };
    // Verschluesselte Verbindungen zum eigenen Server wie in der App
    let tls = settings.server.tls != crate::tls::Mode::Default;
    crate::db::set_local_tls(tls.then(|| (settings.server.port, crate::tls::files().ca)));
    let mut session = tools::Session::new(opts);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match protocol::serve(stdin.lock(), stdout.lock(), &mut session) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_arguments() {
        let a = parse_mcp_args(&["--read-only".into(), "--database=shop".into(), "--port".into(), "3307".into(), "-u".into(), "app".into()]).unwrap();
        assert_eq!(a, McpArgs { allow_write: Some(false), database: Some("shop".into()), port: Some(3307), user: Some("app".into()), ..Default::default() });
        assert!(parse_mcp_args(&["--port".into(), "x".into()]).is_err());
        assert!(parse_mcp_args(&["--database".into()]).is_err());
        assert!(parse_mcp_args(&["--wat".into()]).is_err());
    }

    #[test]
    fn options_follow_settings_unless_overridden() {
        let mut s = crate::settings::Settings::default();
        s.ai.allow_write = true;
        s.ai.database = "shop".into();
        s.ssh.enabled = true;
        let o = options(&s, &McpArgs::default()).unwrap();
        assert!(o.allow_write && o.ssh.enabled);
        assert_eq!(o.database.as_deref(), Some("shop"));
        assert_eq!(o.max_rows, s.ai.max_rows);
        let o = options(&s, &McpArgs { allow_write: Some(false), host: Some("db.example".into()), ..Default::default() }).unwrap();
        assert!(!o.allow_write);
        assert!(!o.ssh.enabled, "eine eigene Adresse geht direkt, nicht ueber den gespeicherten Tunnel");
        assert_eq!(o.conn.host, "db.example");
    }

    #[test]
    fn cli_ignores_files() {
        assert_eq!(run_cli(&[]), None);
        assert_eq!(run_cli(&["C:\\daten\\abfrage.sql".into()]), None);
    }
}
