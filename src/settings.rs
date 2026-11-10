// Einstellungen (Datei einstellungen.txt im Programmdatenordner, schluessel=wert).

use crate::db::ConnInfo;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub conn: ConnInfo,
    /// Verbindung ueber einen SSH-Tunnel; Server und Port von `conn` gelten dann aus Sicht des SSH-Servers
    pub ssh: crate::ssh::SshConfig,
    pub language: crate::i18n::Language,
    pub dark: bool,
    pub save_password: bool,
    pub show_system_dbs: bool,
    /// Neue Versionen automatisch im Hintergrund laden und installieren (sonst erst nach Zustimmung)
    pub auto_install: bool,
    /// Schriftgroesse im SQL-Editor (Pixel)
    pub editor_font: u32,
    /// Vorschlaege beim Tippen automatisch zeigen (sonst nur mit Strg+Leertaste)
    pub auto_suggest: bool,
    /// Klammern und Anfuehrungszeichen automatisch schliessen
    pub auto_close: bool,
    /// Faktor fuer das Mausrad (1 = normal)
    pub scroll_speed: f32,
    /// Mittlere Maustaste gedrueckt halten und ziehen = schnell scrollen
    pub autoscroll: bool,
    /// Ordner fuer Projekte, unbenannte Abfragen, Verlauf usw. (leer = Dokumente\EasyMySQL)
    pub storage_dir: String,
    /// Zuletzt geoeffnete Projektordner, neueste zuerst
    pub recent: Vec<PathBuf>,
    /// Alle Anweisungen an den Server mitschreiben (MariaDB general log)
    pub query_log: bool,
    /// Dateien im Editor kurz nach der letzten Aenderung automatisch speichern
    pub autosave: bool,
    /// Fehler im SQL waehrend des Tippens markieren
    pub lint: bool,
    /// Beim Schliessen des Fensters im Infobereich weiterlaufen (sonst wird alles beendet)
    pub background: bool,
    /// Beim naechsten Start alle Registerkarten wieder oeffnen
    pub restore_tabs: bool,
    /// Seitenleiste (Explorer, Suche, Datenbanken, Verlauf) zeigen
    pub sidebar: bool,
    /// Zuletzt gewaehlte Endung im Dialog "Neue Datei"
    pub new_file_ext: String,
    /// Zuletzt gewaehlter Aufbau neuer Java-Projekte
    pub java_project_kind: String,
    /// Eigener Server: Port, Netzwerk, Datenordner, Speicher, …
    pub server: crate::server::Config,
    /// Server beim Start von EasyMySQL starten
    pub server_autostart: bool,
    /// Zuletzt gewaehlter Aufbau neuer Python-Projekte
    pub python_project_kind: String,
    /// Neue Projekte bekommen ein Git-Repository
    pub git_init_projects: bool,
    /// Java, Python, JavaScript, Ausfuehren, Sprachserver
    pub dev: crate::dev::Config,
    /// Editor: Einrueckung, Zeilennummern, …
    pub editor: crate::sqledit::Prefs,
    /// Automatisch speichern nach so vielen Millisekunden ohne Tippen
    pub autosave_ms: u64,
    pub terminal: crate::terminal::Prefs,
    pub git: crate::git::Config,
    /// Git-Bereich: Stand automatisch auffrischen, wenn er sichtbar ist
    pub git_panel_refresh: bool,
    /// Neue MariaDB-Versionen: "aus", "melden" (Standard) oder "automatisch"
    pub mariadb_updates: String,
    /// Welche Versionen: "lts" (Standard) oder "stable"
    pub mariadb_track: String,
    /// Vor dem Update den Datenordner zusaetzlich kopieren
    pub mariadb_copy_data: bool,
    /// Hoechstens so viele Zeilen je Abfrageergebnis
    pub result_max_rows: usize,
    /// Zeilen je Seite in der Datenansicht
    pub data_page_rows: usize,
    /// Eintraege im Verlauf
    pub history_max: usize,
    /// Vor DROP/TRUNCATE/DELETE ohne WHERE sichern
    pub backup_before_delete: bool,
    /// Nach neuen EasyMySQL-Versionen alle so viele Stunden suchen
    pub app_update_hours: u64,
    /// KI-Assistenten (MCP-Server `easymysql mcp`)
    pub ai: AiConfig,
}

/// Was KI-Assistenten ueber den MCP-Server duerfen
#[derive(Clone, Debug, PartialEq)]
pub struct AiConfig {
    /// Aenderungen (INSERT, UPDATE, DDL …) erlauben; sonst nur lesen
    pub allow_write: bool,
    /// Datenbank, wenn ein Aufruf keine nennt (leer = keine)
    pub database: String,
    /// Hoechstens so viele Zeilen je Abfrage
    pub max_rows: usize,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { allow_write: false, database: String::new(), max_rows: 200 }
    }
}

/// Schriftgroesse im SQL-Editor (Pixel): Standard und Grenzen
pub const DEFAULT_FONT: u32 = 13;
pub const MIN_FONT: u32 = 8;
pub const MAX_FONT: u32 = 40;

fn path() -> PathBuf {
    crate::server::app_data_dir().join("einstellungen.txt")
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            conn: ConnInfo::default(),
            ssh: Default::default(),
            language: crate::i18n::Language::English,
            dark: true,
            save_password: false,
            show_system_dbs: false,
            auto_install: false,
            editor_font: DEFAULT_FONT,
            auto_suggest: true,
            auto_close: true,
            scroll_speed: 1.5,
            autoscroll: true,
            storage_dir: String::new(),
            recent: Vec::new(),
            query_log: false,
            autosave: true,
            lint: true,
            background: false,
            restore_tabs: false,
            sidebar: true,
            new_file_ext: "sql".into(),
            java_project_kind: "einfach".into(),
            server: crate::server::Config::default(),
            server_autostart: true,
            python_project_kind: "requirements".into(),
            git_init_projects: true,
            dev: crate::dev::Config::default(),
            editor: crate::sqledit::Prefs::default(),
            autosave_ms: 1000,
            terminal: crate::terminal::Prefs::default(),
            git: crate::git::Config::default(),
            git_panel_refresh: true,
            mariadb_updates: "melden".into(),
            mariadb_track: "lts".into(),
            mariadb_copy_data: true,
            result_max_rows: 50_000,
            data_page_rows: 500,
            history_max: 3000,
            backup_before_delete: true,
            app_update_hours: 6,
            ai: AiConfig::default(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).map(|text| Self::from_text(&text)).unwrap_or_default()
    }

    fn from_text(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let raw = v;
            let v = v.trim().to_string();
            let on = v == "1";
            match k.trim() {
                "language" => s.language = crate::i18n::Language::from_id(&v),
                "dark" => s.dark = on,
                "host" => s.conn.host = v,
                "port" => s.conn.port = v.parse().unwrap_or(3306),
                "user" => s.conn.user = v,
                "password" => s.conn.password = secret_dec(raw),
                "ssh" => s.ssh.enabled = on,
                "ssh_host" => s.ssh.host = v,
                "ssh_port" => s.ssh.port = v.parse().unwrap_or(22),
                "ssh_user" => s.ssh.user = v,
                "ssh_key" => s.ssh.key = v,
                "ssh_password" => s.ssh.password = secret_dec(raw),
                "save_password" => s.save_password = on,
                "show_system_dbs" => s.show_system_dbs = on,
                "auto_install" => s.auto_install = on,
                "editor_font" => s.editor_font = v.parse().unwrap_or(DEFAULT_FONT).clamp(MIN_FONT, MAX_FONT),
                "auto_suggest" => s.auto_suggest = on,
                "auto_close" => s.auto_close = on,
                "scroll_speed" => s.scroll_speed = v.parse().unwrap_or(1.5f32).clamp(0.25, 6.0),
                "autoscroll" => s.autoscroll = on,
                "storage_dir" => s.storage_dir = v,
                "recent" if !v.is_empty() => s.recent.push(PathBuf::from(v)),
                "query_log" => s.query_log = on,
                "autosave" => s.autosave = on,
                "lint" => s.lint = on,
                "background" => s.background = on,
                "restore_tabs" => s.restore_tabs = on,
                "sidebar" => s.sidebar = on,
                "new_file_ext" => s.new_file_ext = v,
                "java_driver" => s.dev.java_driver = crate::dev::java::Driver::from_id(&v),
                "java_project_kind" => s.java_project_kind = v,
                "server_port" => s.server.port = v.parse().ok().filter(|p| *p > 0).unwrap_or(crate::server::DEFAULT_PORT),
                "server_network" => s.server.network = on,
                "server_data_dir" => s.server.data_dir = Some(PathBuf::from(&v)).filter(|_| !v.is_empty()),
                "server_buffer_mb" => s.server.buffer_pool_mb = v.parse().unwrap_or(128),
                "server_max_connections" => s.server.max_connections = v.parse().unwrap_or(151),
                "server_packet_mb" => s.server.packet_mb = v.parse().unwrap_or(64),
                "server_collation" if !v.is_empty() => s.server.collation = v,
                "server_events" => s.server.event_scheduler = on,
                "server_extra" => s.server.extra = unescape(&v),
                "server_tls" => s.server.tls = crate::tls::Mode::from_id(&v),
                "server_tls_names" => s.server.tls_names = unescape(&v),
                "server_bin_dir" => s.server.bin_dir = (!v.is_empty()).then(|| PathBuf::from(&v)),
                "mariadb_updates" => s.mariadb_updates = v,
                "mariadb_track" => s.mariadb_track = v,
                "mariadb_copy_data" => s.mariadb_copy_data = on,
                "result_max_rows" => s.result_max_rows = v.parse().unwrap_or(50_000).clamp(100, 10_000_000),
                "data_page_rows" => s.data_page_rows = v.parse().unwrap_or(500).clamp(10, 100_000),
                "history_max" => s.history_max = v.parse().unwrap_or(3000).clamp(10, 1_000_000),
                "backup_before_delete" => s.backup_before_delete = on,
                "app_update_hours" => s.app_update_hours = v.parse().unwrap_or(6).clamp(1, 720),
                "server_autostart" => s.server_autostart = on,
                "python_project_kind" => s.python_project_kind = v,
                "git_init_projects" => s.git_init_projects = on,
                "autosave_ms" => s.autosave_ms = v.parse().unwrap_or(1000).clamp(200, 60_000),
                "editor_indent" => s.editor.indent = v.parse().unwrap_or(0).min(16),
                "editor_line_numbers" => s.editor.line_numbers = on,
                "editor_highlight_line" => s.editor.highlight_line = on,
                "editor_match_brackets" => s.editor.match_brackets = on,
                "editor_auto_indent" => s.editor.auto_indent = on,
                "editor_sql_in_strings" => s.editor.sql_in_strings = on,
                "editor_lint_delay_ms" => s.editor.lint_delay_ms = v.parse().unwrap_or(350).min(5000),
                "editor_scroll_past_end" => s.editor.scroll_past_end = on,
                "terminal_font" => s.terminal.font_size = v.parse().unwrap_or(0),
                "terminal_scrollback" => s.terminal.scrollback = v.parse().unwrap_or(5000).clamp(100, 200_000),
                "terminal_default_shell" => s.terminal.default_shell = unescape(&v),
                "terminal_custom_shell" => s.terminal.custom_shell = unescape(&v),
                "git_user_name" => s.git.user_name = unescape(&v),
                "git_user_email" => s.git.user_email = unescape(&v),
                "git_pull_mode" if !v.is_empty() => s.git.pull_mode = v,
                "git_push_after_commit" => s.git.push_after_commit = on,
                "git_auto_stage" => s.git.auto_stage = on,
                "git_auto_fetch_secs" => s.git.auto_fetch_secs = v.parse().unwrap_or(0).min(86_400),
                "git_default_branch" if !v.is_empty() => s.git.default_branch = unescape(&v),
                "git_private_repos" => s.git.private_repos = on,
                "git_panel_refresh" => s.git_panel_refresh = on,
                "ai_allow_write" => s.ai.allow_write = on,
                "ai_database" => s.ai.database = unescape(&v),
                "ai_max_rows" => s.ai.max_rows = v.parse().unwrap_or(200).clamp(1, 5000),
                k if k.starts_with("dev_") => {
                    s.dev.set(&k[4..], &v);
                }
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let _ = crate::workspace::atomic_write_private(&path(), &self.to_text());
    }

    fn to_text(&self) -> String {
        let b = |x: bool| if x { "1" } else { "0" };
        let mut text = format!(
            "language={}\ndark={}\nhost={}\nport={}\nuser={}\nsave_password={}\nshow_system_dbs={}\nauto_install={}\neditor_font={}\n\
             auto_suggest={}\nauto_close={}\nscroll_speed={}\nautoscroll={}\nstorage_dir={}\nquery_log={}\nautosave={}\nlint={}\nbackground={}\nrestore_tabs={}\nsidebar={}\n",
            self.language.id(),
            b(self.dark),
            self.conn.host,
            self.conn.port,
            self.conn.user,
            b(self.save_password),
            b(self.show_system_dbs),
            b(self.auto_install),
            self.editor_font,
            b(self.auto_suggest),
            b(self.auto_close),
            self.scroll_speed,
            b(self.autoscroll),
            self.storage_dir,
            b(self.query_log),
            b(self.autosave),
            b(self.lint),
            b(self.background),
            b(self.restore_tabs),
            b(self.sidebar),
        );
        text.push_str(&format!("new_file_ext={}\njava_project_kind={}\n", self.new_file_ext, self.java_project_kind));
        let sv = &self.server;
        text.push_str(&format!(
            "server_port={}\nserver_network={}\nserver_data_dir={}\nserver_buffer_mb={}\nserver_max_connections={}\nserver_packet_mb={}\nserver_collation={}\nserver_events={}\nserver_extra={}\nserver_autostart={}\n",
            sv.port,
            b(sv.network),
            sv.data_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            sv.buffer_pool_mb,
            sv.max_connections,
            sv.packet_mb,
            sv.collation,
            b(sv.event_scheduler),
            escape(&sv.extra),
            b(self.server_autostart)
        ));
        text.push_str(&format!(
            "server_tls={}\nserver_tls_names={}\nserver_bin_dir={}\nmariadb_updates={}\nmariadb_track={}\nmariadb_copy_data={}\n",
            sv.tls.id(),
            escape(&sv.tls_names),
            sv.bin_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            self.mariadb_updates,
            self.mariadb_track,
            b(self.mariadb_copy_data)
        ));
        let e = &self.editor;
        text.push_str(&format!(
            "python_project_kind={}\ngit_init_projects={}\nautosave_ms={}\neditor_indent={}\neditor_line_numbers={}\neditor_highlight_line={}\neditor_match_brackets={}\neditor_auto_indent={}\neditor_sql_in_strings={}\neditor_lint_delay_ms={}\neditor_scroll_past_end={}\n",
            self.python_project_kind,
            b(self.git_init_projects),
            self.autosave_ms,
            e.indent,
            b(e.line_numbers),
            b(e.highlight_line),
            b(e.match_brackets),
            b(e.auto_indent),
            b(e.sql_in_strings),
            e.lint_delay_ms,
            b(e.scroll_past_end)
        ));
        let t = &self.terminal;
        text.push_str(&format!("terminal_font={}\nterminal_scrollback={}\nterminal_default_shell={}\nterminal_custom_shell={}\n", t.font_size, t.scrollback, escape(&t.default_shell), escape(&t.custom_shell)));
        let g = &self.git;
        text.push_str(&format!(
            "git_user_name={}\ngit_user_email={}\ngit_pull_mode={}\ngit_push_after_commit={}\ngit_auto_stage={}\ngit_auto_fetch_secs={}\ngit_default_branch={}\ngit_private_repos={}\ngit_panel_refresh={}\n",
            escape(&g.user_name),
            escape(&g.user_email),
            g.pull_mode,
            b(g.push_after_commit),
            b(g.auto_stage),
            g.auto_fetch_secs,
            escape(&g.default_branch),
            b(g.private_repos),
            b(self.git_panel_refresh)
        ));
        text.push_str(&format!(
            "result_max_rows={}\ndata_page_rows={}\nhistory_max={}\nbackup_before_delete={}\napp_update_hours={}\n",
            self.result_max_rows,
            self.data_page_rows,
            self.history_max,
            b(self.backup_before_delete),
            self.app_update_hours
        ));
        text.push_str(&format!("ai_allow_write={}\nai_database={}\nai_max_rows={}\n", b(self.ai.allow_write), escape(&self.ai.database), self.ai.max_rows));
        text.push_str(&self.dev.to_lines());
        for r in &self.recent {
            text.push_str(&format!("recent={}\n", r.display()));
        }
        let s = &self.ssh;
        text.push_str(&format!("ssh={}\nssh_host={}\nssh_port={}\nssh_user={}\nssh_key={}\n", b(s.enabled), s.host, s.port, s.user, s.key));
        if self.save_password {
            text.push_str(&format!("password={}\nssh_password={}\n", secret_enc(&self.conn.password), secret_enc(&s.password)));
        }
        text
    }

    /// Projektordner an den Anfang der Liste "Zuletzt geoeffnet" setzen.
    pub fn add_recent(&mut self, dir: &std::path::Path) {
        self.recent.retain(|p| p != dir);
        self.recent.insert(0, dir.to_path_buf());
        self.recent.truncate(12);
    }
}

/// Mehrzeiliger Wert in einer Zeile der Datei
pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "")
}

/// Marke vor einem gesicherten Geheimnis (aeltere Dateien haben den Wert roh, ohne Marke)
const SECRET_MARK: char = '\u{1}';

/// Passwort fuer eine Zeile der Datei: Zeilenumbrueche, Tabs, Backslashes und Leerzeichen am Rand bleiben erhalten
pub fn secret_enc(s: &str) -> String {
    format!("{SECRET_MARK}{}", escape(s))
}

/// Gegenstueck zu `secret_enc`; Werte ohne Marke (alte Dateien) werden wie bisher gelesen
pub fn secret_dec(raw: &str) -> String {
    match raw.strip_prefix(SECRET_MARK) {
        Some(rest) => unescape(rest),
        None => raw.trim().to_string(),
    }
}

pub fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_settings_roundtrip() {
        let mut s = Settings::default();
        s.server.port = 3307;
        s.server.network = true;
        s.server.data_dir = Some(PathBuf::from("D:\\db\\daten"));
        s.server.extra = "sql_mode=ANSI\nlong_query_time=2\\x".into();
        s.server_autostart = false;
        s.dev.use_wrapper = false;
        s.dev.maven_args = "-q".into();
        s.editor.indent = 3;
        s.terminal.custom_shell = "C:\\Git\\bin\\bash.exe --login".into();
        s.git.user_name = "Ada".into();
        s.git.pull_mode = "rebase".into();
        s.ai = AiConfig { allow_write: true, database: "shop".into(), max_rows: 750 };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert!(!Settings::from_text("").ai.allow_write, "KI darf ohne Zustimmung nur lesen");
        assert_eq!(Settings::from_text("ai_max_rows=999999\n").ai.max_rows, 5000);
        assert_eq!(Settings::from_text("java_driver=mysql\n").dev.java_driver, crate::dev::java::Driver::MySql);
        assert_eq!(Settings::from_text("").server, crate::server::Config::default());
    }

    #[test]
    fn defaults_legacy_settings_and_language_roundtrip() {
        let defaults = Settings::from_text("host=localhost\nport=3307\n");
        assert_eq!(defaults.language, crate::i18n::Language::English);
        assert!(defaults.dark);
        assert_eq!(defaults.conn.port, 3307);
        let custom = Settings::from_text("language=de\ndark=0\neditor_font=18\nauto_install=1\n");
        assert_eq!(custom.language, crate::i18n::Language::German);
        assert!(!custom.dark);
        assert_eq!(Settings::from_text(&custom.to_text()), custom);
        assert_eq!(Settings::from_text("language=invalid").language, crate::i18n::Language::English);
    }

    #[test]
    fn awkward_passwords_survive_a_restart() {
        for pw in [" leer ", "a\\b", "zwei\nZeilen\nhost=evil", "tab\there", "ende\\"] {
            let mut s = Settings::default();
            s.save_password = true;
            s.conn.password = pw.into();
            s.ssh.password = pw.into();
            let back = Settings::from_text(&s.to_text());
            assert_eq!((back.conn.password.as_str(), back.ssh.password.as_str(), back.conn.host.as_str()), (pw, pw, "127.0.0.1"), "{pw:?}");
        }
        // alte Dateien: roh, ohne Marke
        assert_eq!(Settings::from_text("password=geheim\n").conn.password, "geheim");
    }

    #[test]
    fn recent_list() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.add_recent(std::path::Path::new(&format!("/p{i}")));
        }
        s.add_recent(std::path::Path::new("/p5"));
        assert_eq!(s.recent.len(), 12);
        assert_eq!(s.recent[0], PathBuf::from("/p5"));
        assert_eq!(s.recent.iter().filter(|p| **p == PathBuf::from("/p5")).count(), 1);
    }
}
