// Auto-Fix mit Probelauf: Das Skript laeuft in Wegwerf-Kopien der beteiligten Datenbanken auf dem echten
// Server. Scheitert eine Anweisung, werden Korrekturen gesucht (Regeln, Skript-Simulation, Serverfehler,
// auf Wunsch die lokale KI); eine Korrektur gilt erst, wenn der naechste Probelauf damit weiter kommt.
// Die echten Datenbanken werden nur gelesen: Struktur und kleine Datenmengen werden kopiert, jede Anweisung
// wird auf die Kopien umgeschrieben, und was ausserhalb wirken koennte (Benutzer, Rechte, Dateien,
// globale Einstellungen), wird im Probelauf nicht ausgefuehrt.

mod aufbau;
pub(crate) use aufbau::beispieldaten;

use super::dialect::{group_end, insert_parts, items};
use super::errfix;
use super::lint::{Env, Kind, Problem, Tok, apply_fixes, lex, lint};
use crate::db::{self, Schema};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Eine Sitzung auf dem Server (im Test nachgebaut)
pub trait Server {
    /// Anweisung ausfuehren, Ergebnis verwerfen; Fehler wie "ERROR 1146 (42S02): …"
    fn exec(&mut self, sql: &str) -> Result<(), String>;
    /// Zeilen einer Abfrage, Werte als Text
    fn rows(&mut self, sql: &str) -> Result<Vec<db::Row>, String>;
    /// Struktur einer Datenbank
    fn schema(&mut self, db: &str) -> Option<Schema>;
}

/// Der echte Server: eine eigene Verbindung (Sitzungszustand wie USE) plus Pool fuer die Struktur
pub struct Live {
    pub conn: mysql::Conn,
    pub db: crate::db::Db,
}

impl Server for Live {
    fn exec(&mut self, sql: &str) -> Result<(), String> {
        use mysql::prelude::Queryable;
        self.conn.query_drop(sql).map_err(db::err_text)
    }

    fn rows(&mut self, sql: &str) -> Result<Vec<db::Row>, String> {
        use mysql::prelude::Queryable;
        self.conn.query_map(sql, |r: mysql::Row| r.unwrap().into_iter().map(db::value_into_string).collect()).map_err(db::err_text)
    }

    fn schema(&mut self, name: &str) -> Option<Schema> {
        self.db.schema(name).ok()
    }
}

/// Was der Probelauf wissen muss
#[derive(Clone, Default)]
pub struct Setup {
    /// Im Reiter gewaehlte Datenbank
    pub database: Option<String>,
    /// Datenbanken auf dem Server
    pub databases: Vec<String>,
    /// Dateiname ohne Endung (Vorschlag fuer eine Datenbank, wenn keine gewaehlt ist)
    pub file_stem: Option<String>,
}

/// Eine vorgenommene Aenderung
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub label: String,
    /// Zeile im fertigen Text (0-basiert), falls auffindbar
    pub line: Option<usize>,
    /// Mit Platzhaltern oder von der KI: bitte ansehen
    pub check: bool,
    snippet: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum End {
    /// Das ganze Skript lief im Probelauf ohne Fehler
    Clean,
    /// Ohne Server: nur die Pruefung des Editors
    Offline,
    /// Fuer diesen Fehler gibt es keine sichere Korrektur
    Stuck { line: usize, error: String },
    Cancelled,
    /// Probelauf nicht moeglich (Grund)
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Report {
    pub text: String,
    pub changes: Vec<Change>,
    pub end: End,
    pub runs: usize,
    /// Anweisungen, die im Probelauf bewusst nicht liefen (Benutzer, Rechte, Dateien …)
    pub skipped: usize,
    /// Daten zu gross zum Kopieren: Fehler, die vom Inhalt abhaengen, wurden nicht geprueft
    pub without_data: bool,
}

/// Korrektur-Kandidat: neuer Gesamttext
#[derive(Clone, Debug)]
pub struct Cand {
    pub text: String,
    pub label: String,
    pub check: bool,
    pub snippet: String,
    /// Aus dem spaeteren Skript vor den Fehler gezogene Anweisungen (zaehlen nicht als Fortschritt)
    pub moved: usize,
}

/// Lokale KI: (Anweisung, Fehler, Schema) -> korrigierte Anweisung
pub type Ai<'a> = &'a dyn Fn(&str, &str, Option<&Schema>) -> Option<String>;

const SYSTEM: [&str; 4] = ["mysql", "information_schema", "performance_schema", "sys"];
const MAX_RUNS: usize = 40;
const MAX_TIME: Duration = Duration::from_secs(120);
/// Bis zu so vielen Zeilen (alle kopierten Datenbanken zusammen) werden die Daten mitkopiert
const COPY_ROWS: u64 = 20_000;
const COPY_TABLES: usize = 80;

fn is_system(name: &str) -> bool {
    SYSTEM.iter().any(|s| s.eq_ignore_ascii_case(name))
}

fn str_lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

// ---------------------------------------------------------------------------
// Statische Korrekturen bis nichts mehr zu tun ist

/// Korrekturen des Editors wiederholt anwenden, bis keine mehr greift (hoechstens 10 Runden).
/// `trial`: Ein Probelauf folgt; "gibt es schon" (1007/1050) entscheidet dann er, weil nur er weiss, ob das
/// Skript seine Tabellen neu anlegen soll (wiederholbar) oder nur fehlende ergaenzt.
pub fn static_fix(text: &str, env: &Env, trial: bool) -> (String, Vec<Change>) {
    let mut text = text.to_string();
    let mut changes: Vec<Change> = Vec::new();
    for _ in 0..10 {
        let mut problems = lint(&text, env);
        if trial {
            // shortcut: erkennt die Meldung an der Fehlernummer; bei neuen Meldungen mit 1007/1050 anpassen
            problems.retain(|p| !(p.message.contains(" 1050)") || p.message.contains(" 1007)")));
        }
        let picked: Vec<Problem> = chosen(&problems).into_iter().cloned().collect();
        let (fixed, n) = apply_fixes(&text, &problems);
        if n == 0 || fixed == text {
            break;
        }
        // Zeile jeder Korrektur im neuen Text: Position plus Laengenaenderung der Korrekturen davor
        let mut delta: isize = 0;
        for p in &picked {
            let f = p.fix.as_ref().expect("gewaehlt");
            let at = (f.start as isize + delta).max(0) as usize;
            delta += f.text.chars().count() as isize - (f.end - f.start) as isize;
            if !changes.iter().any(|c| c.label == p.message) {
                let line_start = fixed.chars().take(at).collect::<String>().rfind('\n').map_or(0, |b| b + 1);
                let snippet = fixed[line_start..].lines().next().unwrap_or("").trim().to_string();
                changes.push(Change { label: p.message.clone(), line: None, check: false, snippet });
            }
        }
        text = fixed;
    }
    (text, changes)
}

/// Die Probleme, deren Korrektur `apply_fixes` nimmt (gleiche Auswahl)
fn chosen(problems: &[Problem]) -> Vec<&Problem> {
    let mut with: Vec<&Problem> = problems.iter().filter(|p| p.fix.is_some()).collect();
    with.sort_by_key(|p| p.fix.as_ref().map(|f| (f.start, f.end)));
    let mut out: Vec<&Problem> = Vec::new();
    for p in with {
        let f = p.fix.as_ref().expect("gefiltert");
        if out.last().and_then(|l| l.fix.as_ref()).is_none_or(|l| f.start >= l.end && !(f.start == l.start && f.end == l.end)) {
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Umschreiben auf die Probe-Datenbanken

/// Vorsilbe aller Probe-Datenbanken (werden nie angezeigt und nach Abstuerzen aufgeraeumt)
pub const SANDBOX_PREFIX: &str = "easymysql_probe_";

/// Startzeit dieses Prozesses (Sekunden seit 1970): steht im Namen jeder Probe-Datenbank, damit nur alte Reste
/// aufgeraeumt werden und nie die Kopien eines gleichzeitig laufenden anderen Prozesses
fn started() -> u64 {
    static T: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

/// Ist die Probe-Datenbank `name` ein Rest (aelter als eine halbe Stunde bzw. im alten Namensformat)?
fn stale_sandbox(name: &str, now: u64) -> bool {
    let Some(rest) = name.strip_prefix(SANDBOX_PREFIX) else { return false };
    let parts: Vec<&str> = rest.split('_').collect();
    match parts.as_slice() {
        [pid, t, _, _] => {
            let own = pid.parse::<u32>().ok() == Some(std::process::id()) && t.parse::<u64>().ok() == Some(started());
            // Ein Auto-Fix dauert hoechstens MAX_TIME; ein Prozess kann aber lange laufen und immer wieder probieren
            !own && t.parse::<u64>().is_ok_and(|t| t + 1800 < now) && !alive_elsewhere(pid)
        }
        _ => true,
    }
}

/// Laeuft der Prozess noch (nur unter Linux pruefbar; sonst entscheidet allein das Alter)
fn alive_elsewhere(pid: &str) -> bool {
    cfg!(target_os = "linux") && std::path::Path::new("/proc").join(pid).exists()
}

/// Original -> Probe-Datenbank
#[derive(Default)]
struct Map {
    pairs: Vec<(String, String)>,
    /// Laufende Nummer dieses Auto-Fix im Prozess (zwei Reiter duerfen gleichzeitig probieren)
    run: u32,
}

impl Map {
    fn new() -> Self {
        static RUN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        Map { pairs: Vec::new(), run: RUN.fetch_add(1, Ordering::Relaxed) }
    }

    fn sandbox(&self, n: usize) -> String {
        format!("{SANDBOX_PREFIX}{}_{}_{}_{n}", std::process::id(), started(), self.run)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.pairs.iter().find(|(o, _)| o.eq_ignore_ascii_case(name)).map(|(_, s)| s.as_str())
    }

    fn add(&mut self, name: &str) {
        if self.get(name).is_none() && !is_system(name) {
            let s = self.sandbox(self.pairs.len() + 1);
            self.pairs.push((name.to_string(), s));
        }
    }

    /// Namen der Probe-Datenbanken in einer Meldung zurueck uebersetzen
    fn unmap(&self, msg: &str) -> String {
        let mut out = msg.to_string();
        for (o, s) in &self.pairs {
            out = out.replace(s.as_str(), o);
        }
        out
    }
}

/// Index des Namens nach CREATE/DROP/ALTER [OR REPLACE] DATABASE|SCHEMA [IF [NOT] EXISTS] bzw. USE
fn database_name_at(toks: &[Tok]) -> Option<Option<usize>> {
    let first = toks.first()?;
    if first.is_word("USE") {
        return Some(toks.get(1).filter(|t| t.is_name()).map(|_| 1));
    }
    let k = if first.is_word("CREATE") && toks.get(1).is_some_and(|t| t.is_word("OR")) { 3 } else { 1 };
    if !["CREATE", "DROP", "ALTER"].iter().any(|w| first.is_word(w)) || !toks.get(k).is_some_and(|t| t.is_word("DATABASE") || t.is_word("SCHEMA")) {
        return None;
    }
    let mut j = k + 1;
    if toks.get(j).is_some_and(|t| t.is_word("IF")) {
        j += if toks.get(j + 1).is_some_and(|t| t.is_word("NOT")) { 3 } else { 2 };
    }
    let named = toks.get(j).is_some_and(|t| t.kind == Kind::Quoted || t.kind == Kind::Word && !super::lint::known_word(&t.upper()));
    Some(named.then_some(j))
}

/// Datenbanken, die das Skript nennt (USE, CREATE/DROP DATABASE, db.tabelle bei bekannten Datenbanken)
fn referenced(text: &str, setup: &Setup) -> Map {
    let mut map = Map::new();
    if let Some(d) = setup.database.as_deref().filter(|d| !d.is_empty()) {
        map.add(d);
    }
    for st in db::split_statements(text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        if let Some(Some(j)) = database_name_at(&toks) {
            map.add(&toks[j].text);
        }
        for k in 0..toks.len() {
            if toks[k].is_name() && toks.get(k + 1).is_some_and(|t| t.kind == Kind::Dot) && (k == 0 || toks[k - 1].kind != Kind::Dot) && setup.databases.iter().any(|d| d.eq_ignore_ascii_case(&toks[k].text)) {
                map.add(&toks[k].text);
            }
        }
    }
    map
}

/// Darf die Anweisung im Probelauf laufen? (nur, was innerhalb der Probe-Datenbanken bleibt)
fn allowed(sql: &str, toks: &[Tok], dynamic: bool) -> bool {
    let Some(first) = toks.first() else { return false };
    if toks.iter().any(|t| t.kind == Kind::Word && ["OUTFILE", "DUMPFILE", "LOAD_FILE", "INFILE"].iter().any(|w| t.is_word(w))) {
        return false;
    }
    // Schreiben in Systemdatenbanken
    if toks.windows(2).any(|w| w[0].is_name() && is_system(&w[0].text) && w[1].kind == Kind::Dot) && !["SELECT", "WITH", "VALUES", "TABLE"].iter().any(|w| first.is_word(w)) {
        return false;
    }
    match first.upper().as_str() {
        "SELECT" | "WITH" | "VALUES" | "TABLE" | "INSERT" | "REPLACE" | "UPDATE" | "DELETE" | "TRUNCATE" | "RENAME" | "USE" | "START" | "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "DO" | "LOCK" | "UNLOCK" => true,
        "CALL" => !dynamic,
        "SET" => !toks.iter().any(|t| ["GLOBAL", "PASSWORD", "ROLE", "SQL_LOG_BIN"].iter().any(|w| t.is_word(w))) && !sql.to_ascii_lowercase().contains("@@global"),
        "CREATE" | "DROP" | "ALTER" => {
            for t in toks.iter().skip(1).take(14) {
                let u = t.upper();
                if ["TABLE", "VIEW", "INDEX", "DATABASE", "SCHEMA", "TRIGGER", "PROCEDURE", "FUNCTION", "SEQUENCE"].contains(&u.as_str()) {
                    return true;
                }
                if ["USER", "ROLE", "SERVER", "EVENT", "LOGFILE", "TABLESPACE", "PLUGIN"].contains(&u.as_str()) {
                    return false;
                }
            }
            false
        }
        _ => false,
    }
}

/// Datenbanknamen der Anweisung auf die Probe-Datenbanken umschreiben; None: nicht sicher ausfuehrbar
fn rewrite(sql: &str, map: &Map) -> Option<String> {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let mut at: Vec<usize> = Vec::new();
    match database_name_at(&toks) {
        Some(Some(j)) => {
            map.get(&toks[j].text)?;
            at.push(j);
        }
        // ALTER DATABASE ohne Namen betrifft die aktuelle (Probe-)Datenbank; USE/CREATE/DROP brauchen ihn
        Some(None) if !toks[0].is_word("ALTER") => return None,
        _ => {}
    }
    for k in 0..toks.len() {
        if toks[k].is_name() && toks.get(k + 1).is_some_and(|t| t.kind == Kind::Dot) && (k == 0 || toks[k - 1].kind != Kind::Dot) && map.get(&toks[k].text).is_some() && !at.contains(&k) {
            at.push(k);
        }
    }
    at.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = chars;
    for k in at {
        let name = map.get(&toks[k].text)?;
        out.splice(toks[k].start..toks[k].end, db::q(name).chars());
    }
    Some(out.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Ein Probelauf

#[derive(Clone, Debug)]
struct Fail {
    stmt: db::Stmt,
    error: String,
    schema: Option<Schema>,
}

#[derive(Clone, Debug)]
struct Trial {
    /// Erfolgreich ausgefuehrte (oder bewusst uebersprungene) Anweisungen vor dem Fehler
    ok: usize,
    fail: Option<Fail>,
}

struct Probe<'a> {
    server: &'a mut dyn Server,
    map: Map,
    /// Probe-Datenbanken, die zu Beginn als Kopie (bzw. leer) bestehen muessen: (Original, kopieren?)
    existing: Vec<(String, bool)>,
    current: Option<String>,
    copy_data: bool,
    dynamic: bool,
    skipped: usize,
    /// Datenbanken auf dem echten Server
    on_server: Vec<String>,
    /// Struktur der Originale (aendert sich waehrend des Auto-Fix nicht): einmal lesen statt je Probelauf
    cache: std::collections::HashMap<String, Vec<Item>>,
    /// Woerter des gerade probierten Skripts (klein): nur genannte Tabellen werden kopiert
    names: HashSet<String>,
}

/// Tabelle oder Sicht eines Originals
#[derive(Clone)]
struct Item {
    name: String,
    ddl: String,
    view: bool,
    /// Spalten zum Kopieren der Daten (leer = keine Daten)
    cols: Vec<String>,
}

/// Ab so vielen Tabellen werden nur die vom Skript genannten (samt Fremdschluessel-Nachbarn) kopiert
const COPY_ALL_UP_TO: usize = 8;

/// Tabellen, auf die eine CREATE-Anweisung per REFERENCES bzw. FROM/JOIN verweist (klein geschrieben)
fn mentioned_tables(ddl: &str) -> Vec<String> {
    let chars: Vec<char> = ddl.chars().collect();
    let toks = lex(&chars).toks;
    let mut out = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        if (t.is_word("REFERENCES") || t.is_word("FROM") || t.is_word("JOIN")) && let Some(n) = toks.get(k + 1).filter(|n| n.is_name()) {
            // `db`.`tabelle` -> tabelle
            let name = if toks.get(k + 2).is_some_and(|d| d.kind == Kind::Dot) { toks.get(k + 3).map_or(&n.text, |x| &x.text) } else { &n.text };
            out.push(name.to_ascii_lowercase());
        }
    }
    out
}

impl Probe<'_> {
    fn cleanup(&mut self) {
        for (_, s) in &self.map.pairs {
            let _ = self.server.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(s)));
        }
    }

    /// Kopie einer echten Datenbank anlegen (Struktur, Sichten, kleine Daten)
    /// Tabellen und Sichten eines Originals (einmal je Auto-Fix gelesen)
    fn items(&mut self, orig: &str) -> Result<Vec<Item>, String> {
        if let Some(v) = self.cache.get(orig) {
            return Ok(v.clone());
        }
        let tables = self.server.rows(&format!("SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA = {}", str_lit(orig)))?;
        let schema = if self.copy_data { self.server.schema(orig) } else { None };
        // Auch eine spaeter hinzukommende Datenbank wird nur mit Inhalt kopiert, wenn sie klein ist
        let gross = schema.as_ref().is_some_and(|s| s.tables.iter().map(|t| t.rows.unwrap_or(0)).sum::<u64>() > COPY_ROWS);
        let mut out = Vec::new();
        for r in &tables {
            let name = r.first().cloned().flatten().unwrap_or_default();
            let view = r.get(1).cloned().flatten().unwrap_or_default().contains("VIEW");
            let rows = self.server.rows(&format!("SHOW CREATE {} {}.{}", if view { "VIEW" } else { "TABLE" }, db::q(orig), db::q(&name)))?;
            let Some(ddl) = rows.first().and_then(|r| r.get(1).cloned().flatten()) else { continue };
            let cols = match schema.as_ref().and_then(|s| s.tables.iter().find(|t| t.name == name)) {
                Some(t) if !view && !gross => t.columns.iter().filter(|c| !c.is_generated()).map(|c| db::q(&c.name)).collect(),
                _ => Vec::new(),
            };
            out.push(Item { name, ddl, view, cols });
        }
        self.cache.insert(orig.to_string(), out.clone());
        Ok(out)
    }

    fn copy(&mut self, orig: &str, with_tables: bool) -> Result<(), String> {
        let s = self.map.get(orig).ok_or("map")?.to_string();
        self.server.exec(&format!("CREATE DATABASE {}", db::q(&s)))?;
        if !with_tables {
            return Ok(());
        }
        self.server.exec(&format!("USE {}", db::q(&s)))?;
        self.server.exec("SET FOREIGN_KEY_CHECKS = 0")?;
        let items = self.items(orig)?;
        // Grosse Datenbanken: nur, was das Skript nennt, samt Fremdschluessel-Nachbarn und Tabellen der Sichten
        let mut keep: HashSet<String> = HashSet::new();
        let tables = items.iter().filter(|i| !i.view).count();
        if tables <= COPY_ALL_UP_TO {
            keep.extend(items.iter().map(|i| i.name.to_ascii_lowercase()));
        } else {
            keep.extend(items.iter().map(|i| i.name.to_ascii_lowercase()).filter(|n| self.names.contains(n)));
            loop {
                let before = keep.len();
                for i in &items {
                    let me = i.name.to_ascii_lowercase();
                    let refs = mentioned_tables(&i.ddl);
                    if keep.contains(&me) {
                        keep.extend(refs);
                    } else if !i.view && refs.iter().any(|r| keep.contains(r)) {
                        // Kindtabellen: DROP/DELETE der Eltern verhalten sich sonst anders
                        keep.insert(me);
                    }
                }
                if keep.len() == before {
                    break;
                }
            }
        }
        let mut views: Vec<String> = Vec::new();
        for i in items.iter().filter(|i| keep.contains(&i.name.to_ascii_lowercase())) {
            if i.view {
                views.push(i.ddl.clone());
                continue;
            }
            self.server.exec(&i.ddl)?;
            if self.copy_data && !i.cols.is_empty() {
                let cols = i.cols.join(", ");
                self.server.exec(&format!("INSERT INTO {}.{} ({cols}) SELECT {cols} FROM {}.{}", db::q(&s), db::q(&i.name), db::q(orig), db::q(&i.name)))?;
            }
        }
        // Sichten koennen aufeinander aufbauen: mehrere Durchgaenge
        for _ in 0..3 {
            views.retain(|ddl| rewrite(ddl, &self.map).is_none_or(|sql| self.server.exec(&sql).is_err()));
            if views.is_empty() {
                break;
            }
        }
        self.server.exec("SET FOREIGN_KEY_CHECKS = 1")?;
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.cleanup();
        for (orig, with_tables) in self.existing.clone() {
            self.copy(&orig, with_tables)?;
        }
        let _ = self.server.exec("SET SESSION max_statement_time = 10");
        self.server.exec("SET FOREIGN_KEY_CHECKS = 1")?;
        match self.current.as_deref().and_then(|c| self.map.get(c)).map(str::to_string) {
            Some(s) => self.server.exec(&format!("USE {}", db::q(&s)))?,
            // Ohne gewaehlte Datenbank: "No database selected" soll wie in echt auftreten
            None => self.unselect()?,
        }
        Ok(())
    }

    /// Datenbanken, die eine Korrektur neu ins Spiel bringt, ebenfalls abschirmen (echte als Kopie)
    fn shield(&mut self, toks: &[Tok]) -> Result<(), String> {
        let mut names: Vec<String> = Vec::new();
        if let Some(Some(j)) = database_name_at(toks) {
            names.push(toks[j].text.clone());
        }
        for k in 0..toks.len() {
            if toks[k].is_name() && toks.get(k + 1).is_some_and(|t| t.kind == Kind::Dot) && (k == 0 || toks[k - 1].kind != Kind::Dot) && self.on_server.iter().any(|d| d.eq_ignore_ascii_case(&toks[k].text)) {
                names.push(toks[k].text.clone());
            }
        }
        for n in names {
            if self.map.get(&n).is_some() || is_system(&n) {
                continue;
            }
            self.map.add(&n);
            if self.on_server.iter().any(|d| d.eq_ignore_ascii_case(&n)) {
                self.existing.push((n.clone(), true));
                // Die Kopie wechselt die Datenbank der Sitzung: danach zurueck
                let cur = self.server.rows("SELECT DATABASE()")?.first().cloned().and_then(|r| r.first().cloned().flatten());
                self.copy(&n, true)?;
                match cur {
                    Some(c) => self.server.exec(&format!("USE {}", db::q(&c)))?,
                    None => self.unselect()?,
                }
            }
        }
        Ok(())
    }

    /// Keine Datenbank gewaehlt (wie eine neue Verbindung ohne Datenbank)
    fn unselect(&mut self) -> Result<(), String> {
        let s = self.map.sandbox(0);
        self.server.exec(&format!("CREATE DATABASE IF NOT EXISTS {}", db::q(&s)))?;
        self.server.exec(&format!("USE {}", db::q(&s)))?;
        self.server.exec(&format!("DROP DATABASE {}", db::q(&s)))
    }

    fn trial(&mut self, text: &str) -> Result<Trial, String> {
        let chars: Vec<char> = text.chars().collect();
        self.names = lex(&chars).toks.iter().filter(|t| t.is_name()).map(|t| t.text.to_ascii_lowercase()).collect();
        self.reset()?;
        self.skipped = 0;
        let mut ok = 0;
        for st in db::split_statements(text) {
            let chars: Vec<char> = st.sql.chars().collect();
            let toks = lex(&chars).toks;
            self.shield(&toks)?;
            let sql = if allowed(&st.sql, &toks, self.dynamic) { rewrite(&st.sql, &self.map) } else { None };
            let Some(sql) = sql else {
                self.skipped += 1;
                ok += 1;
                continue;
            };
            if let Err(e) = self.server.exec(&sql) {
                let error = self.map.unmap(&e);
                // Ohne kopierte Daten sagen Schluessel-Fehler nichts ueber das Skript
                if !self.copy_data && matches!(errfix::code(&error), Some(1062 | 1451 | 1452)) {
                    ok += 1;
                    continue;
                }
                let cur: Option<String> = self.server.rows("SELECT DATABASE()").ok().and_then(|r| r.first().cloned()).and_then(|r| r.first().cloned().flatten());
                let schema = cur.and_then(|c| {
                    let orig = self.map.pairs.iter().find(|(_, s)| *s == c).map(|(o, _)| o.clone())?;
                    let mut s = self.server.schema(&c)?;
                    s.name = orig;
                    Some(s)
                });
                return Ok(Trial { ok, fail: Some(Fail { stmt: st, error, schema }) });
            }
            ok += 1;
        }
        Ok(Trial { ok, fail: None })
    }
}

/// Faengt das Skript mit dieser Datenbank an, bevor es ihre Tabellen braucht? (dann keine Kopie noetig)
fn dropped_first(text: &str, name: &str) -> bool {
    for st in db::split_statements(text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        let mentions = toks.iter().any(|t| t.is_name() && t.text.eq_ignore_ascii_case(name));
        if !mentions {
            continue;
        }
        return toks[0].is_word("DROP") && matches!(database_name_at(&toks), Some(Some(j)) if toks[j].text.eq_ignore_ascii_case(name));
    }
    false
}

// ---------------------------------------------------------------------------
// Korrekturen fuer einen Fehler

/// Anweisung `st` im Text durch `new` ersetzen
fn replace_stmt(text: &str, st: &db::Stmt, new: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let seg: String = chars[st.start.min(chars.len())..st.end.min(chars.len())].iter().collect();
    let pos = seg.find(&st.sql)?;
    let a = st.start + seg[..pos].chars().count();
    let b = a + st.sql.chars().count();
    let mut out = chars;
    out.splice(a..b, new.trim().trim_end_matches(';').chars());
    Some(out.into_iter().collect())
}

/// Vor der Anweisung `st` Text einfuegen
fn insert_before(text: &str, st: &db::Stmt, new: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let seg: String = chars[st.start.min(chars.len())..st.end.min(chars.len())].iter().collect();
    let a = st.start + seg.find(&st.sql).map_or(0, |p| seg[..p].chars().count());
    let mut out = chars;
    out.splice(a..a, new.chars());
    out.into_iter().collect()
}

fn first_line(s: &str) -> String {
    let l = s.trim().lines().next().unwrap_or("").trim();
    if l.chars().count() > 70 { format!("{} …", l.chars().take(68).collect::<String>()) } else { l.to_string() }
}

/// Tabellen, die das Skript mit CREATE TABLE anlegt, in Reihenfolge
fn created_tables(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for st in db::split_statements(text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        if let Some((name, _)) = super::script::create_columns(&chars, &toks)
            && !out.iter().any(|o| o.eq_ignore_ascii_case(&name))
        {
            out.push(name);
        }
    }
    out
}

/// Tabellen, in die das Skript Zeilen einfuegt
fn filled_tables(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for st in db::split_statements(text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        if let Some((ti, _)) = insert_parts(&toks) {
            out.push(toks[ti].text.to_ascii_lowercase());
        }
    }
    out
}

/// Text in einfachen Anfuehrungszeichen nach `start` (aus Fehlermeldungen)
fn quoted(err: &str, start: &str) -> Option<String> {
    let i = err.find(start)? + start.len();
    let r = &err[i..];
    r.find(['\'', '`']).map(|e| r[..e].to_string())
}

/// Platzhalter fuer eine Pflichtspalte
fn placeholder(table: &str, col: &crate::db::ColumnInfo, key: &str) -> String {
    let t = col.col_type.to_ascii_lowercase();
    if t.starts_with("enum(") || t.starts_with("set(") {
        return t[t.find('(').map_or(0, |p| p + 1)..].split(',').next().unwrap_or("''").trim_end_matches(')').to_string();
    }
    if ["int", "decimal", "float", "double", "bit", "bool", "numeric", "real"].iter().any(|k| t.contains(k)) {
        return "0".into();
    }
    if t.starts_with("datetime") || t.starts_with("timestamp") {
        return "NOW()".into();
    }
    if t.starts_with("date") {
        return "CURDATE()".into();
    }
    if t.starts_with("time") {
        return "'00:00:00'".into();
    }
    if t.starts_with("year") {
        return "YEAR(CURDATE())".into();
    }
    // Laenge beachten
    let len: usize = t.split(['(', ')']).nth(1).and_then(|n| n.parse().ok()).unwrap_or(255);
    let s = format!("{table} {key}");
    str_lit(&s.chars().take(len).collect::<String>())
}

/// Vorschlaege fuer den Fehler `f` im Text, beste zuerst
fn candidates(text: &str, f: &Fail, setup: &Setup, server: &mut dyn Server, sandbox: &Map) -> Vec<Cand> {
    let mut out: Vec<Cand> = Vec::new();
    let st = &f.stmt;
    let chars: Vec<char> = st.sql.chars().collect();
    let toks = lex(&chars).toks;
    let code = errfix::code(&f.error);
    let push_replace = |out: &mut Vec<Cand>, sql: &str, label: String, check: bool| {
        if let Some(t) = replace_stmt(text, st, sql) {
            out.push(Cand { text: t, label, check, moved: 0, snippet: first_line(sql) });
        }
    };
    let mut late: Vec<Cand> = Vec::new();
    // Liefert eine spaetere Anweisung, was hier fehlt (Tabelle, Elternzeilen, Datenbank …)? Dann zuerst sie.
    if let Some(c) = aufbau::vorziehen(text, f) {
        out.push(c);
    }
    let (art, _) = head(&st.sql);
    match code {
        // Tabelle fehlt: nach den Namens-Korrekturen aus einer anderen Datenbank uebernehmen oder ableiten
        Some(1146) if !art.starts_with("DROP") && art != "TRUNCATE" => {
            if let Some(full) = quoted(&f.error, "Table '") {
                let (db_name, name) = full.rsplit_once('.').map_or((None, full.as_str()), |(d, n)| (Some(d), n));
                let current = f.schema.as_ref().map(|s| s.name.as_str());
                let qual = db_name.filter(|d| current.is_none_or(|c| !c.eq_ignore_ascii_case(d)) && toks.windows(3).any(|w| w[0].text.eq_ignore_ascii_case(d) && w[1].kind == Kind::Dot && w[2].text.eq_ignore_ascii_case(name)));
                late.extend(aufbau::tabelle(text, f, setup, server, sandbox, name, qual));
            }
        }
        // Fremdschluessel auf eine Tabelle, die es nirgends gibt: sie aus dem Skript ableiten
        Some(1005 | 150 | 1215 | 1824) => {
            for t in aufbau::fehlende_eltern(text, f) {
                late.extend(aufbau::tabelle(text, f, setup, server, sandbox, &t, None));
            }
        }
        Some(1049) => out.extend(aufbau::datenbank(text, f, setup)),
        Some(1054) => late.extend(aufbau::spalte(text, f)),
        // Spalte anpassen; laesst sich der Wert selbst umschreiben ('9,50 €' -> 9.50), kommt das zuerst
        Some(1406 | 1264 | 1366 | 1265 | 1292) => {
            let value_first = matches!(code, Some(1366 | 1265 | 1292)) && errfix::value_fits(&f.error, &st.sql, f.schema.as_ref());
            if value_first { late.extend(aufbau::typ_anpassen(text, f)) } else { out.extend(aufbau::typ_anpassen(text, f)) }
        }
        Some(1451) => late.extend(aufbau::kinder_zuerst(text, f)),
        // Tabelle gibt es schon: Skript wiederholbar machen (alte Tabellen vorher loeschen) oder IF NOT EXISTS
        Some(1050) => {
            if let Some(name) = quoted(&f.error, "Table '") {
                let created = created_tables(text);
                let existing: Vec<String> = created
                    .iter()
                    .filter(|c| f.schema.as_ref().is_some_and(|s| s.table(c).is_some()))
                    .cloned()
                    .collect();
                let filled = filled_tables(text);
                let lower = name.to_ascii_lowercase();
                let empty = f.schema.as_ref().and_then(|s| s.table(&name)).is_some_and(|t| t.rows == Some(0));
                if !existing.is_empty() {
                    let list: Vec<String> = existing.iter().rev().map(|t| super::lint::quote_if_needed(t)).collect();
                    let drop = format!("DROP TABLE IF EXISTS {};\n", list.join(", "));
                    let cand = Cand {
                        text: insert_before(text, st, &drop),
                        label: crate::tr_format!("Vorhandene Tabellen vorher löschen, damit das Skript wiederholt laufen kann ({})", "Drop the existing tables first so the script can run again ({})", existing.join(", ")),
                        check: false,
                        moved: 0, snippet: first_line(&drop),
                    };
                    if filled.contains(&lower) || empty {
                        out.push(cand);
                    } else {
                        late.push(cand);
                    }
                }
            }
        }
        // Datenbank gibt es schon: ersetzen nur, wenn das Skript alle ihre Tabellen selbst anlegt
        Some(1007) => {
            if let Some(Some(j)) = database_name_at(&toks) {
                let name = toks[j].text.clone();
                let real: Vec<String> = server.schema(&name).map(|s| s.tables.iter().map(|t| t.name.to_ascii_lowercase()).collect()).unwrap_or_default();
                let created: Vec<String> = created_tables(text).iter().map(|t| t.to_ascii_lowercase()).collect();
                if real.iter().all(|t| created.contains(t)) {
                    let drop = format!("DROP DATABASE IF EXISTS {};\n", super::lint::quote_if_needed(&name));
                    out.push(Cand {
                        text: insert_before(text, st, &drop),
                        label: crate::tr_format!("Datenbank „{name}“ vorher löschen, damit das Skript wiederholt laufen kann", "Drop the database “{name}” first so the script can run again"),
                        check: false,
                        moved: 0, snippet: first_line(&drop),
                    });
                }
            }
        }
        // Keine Datenbank gewaehlt: die Datenbank mit allen benutzten Tabellen, sonst eine anlegen und benutzen
        // (Name aus dem Skript oder der Datei)
        Some(1046) => {
            if let Some(c) = aufbau::datenbank_waehlen(text, f, server) {
                out.push(c);
            }
            let name = errfix::created_database(text).or_else(|| {
                setup.file_stem.as_deref().map(|s| s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect::<String>()).filter(|s| !s.is_empty())
            });
            if let Some(n) = name {
                let q = super::lint::quote_if_needed(&n);
                let head = format!("CREATE DATABASE IF NOT EXISTS {q};\nUSE {q};\n\n");
                out.push(Cand {
                    text: insert_before(text, st, &head),
                    label: crate::tr_format!("Datenbank „{n}“ anlegen und benutzen", "Create and use the database “{n}”"),
                    check: false,
                    moved: 0, snippet: format!("USE {q};"),
                });
            }
        }
        // Pflichtfeld ohne Wert: im CREATE TABLE des Skripts AUTO_INCREMENT (Schluessel) bzw. NULL erlauben
        Some(1364 | 1048) => {
            if let (Some(col), Some((ti, _))) = (quoted(&f.error, "Field '").or_else(|| quoted(&f.error, "Column '")), insert_parts(&toks)) {
                let table = toks[ti].text.clone();
                if let Some(c) = loosen_column(text, &table, &col) {
                    out.push(c);
                }
                if code == Some(1364)
                    && let Some(c) = add_required_value(text, f, &toks, &chars, &table, &col)
                {
                    late.push(c);
                }
            }
        }
        // Elternzeile fehlt: Zeile mit Platzhaltern anlegen (nach den anderen Vorschlaegen)
        Some(1452) => {
            if let Some(c) = parent_rows(text, f, &toks, &chars, server, sandbox) {
                late.push(c);
            }
        }
        _ => {}
    }
    // Korrekturen fuer den Serverfehler
    for e in errfix::for_error(&f.error, &st.sql, f.schema.as_ref()) {
        push_replace(&mut out, &e.sql, e.label, false);
    }
    // Syntaxfehler: Vermutungen an der genannten Stelle (der naechste Probelauf entscheidet)
    if code == Some(1064) {
        for e in errfix::syntax_guesses(&f.error, &st.sql) {
            push_replace(&mut out, &e.sql, e.label, false);
        }
    }
    // Pruefung des Editors fuer diese Anweisung allein, mit der echten Struktur zum Zeitpunkt des Fehlers
    let env = Env { schema: f.schema.as_ref(), databases: &setup.databases };
    let probs = lint(&st.sql, &env);
    let (fixed, n) = apply_fixes(&st.sql, &probs);
    if n > 0 && fixed != st.sql {
        let label = chosen(&probs).first().map(|p| p.message.clone()).unwrap_or_default();
        push_replace(&mut out, &fixed, label, false);
    }
    out.extend(late);
    out
}

/// Letzte Idee: die lokale KI (nur gleiche Art Anweisung mit gleichem Ziel; der Probelauf prueft sie)
fn ai_candidate(text: &str, f: &Fail, ai: Ai<'_>) -> Option<Cand> {
    let st = &f.stmt;
    let sql = ai(&st.sql, &f.error, f.schema.as_ref())?;
    if sql.trim().trim_end_matches(';') == st.sql.trim().trim_end_matches(';') || !same_kind(&st.sql, &sql) {
        return None;
    }
    let t = replace_stmt(text, st, &sql)?;
    Some(Cand { text: t, label: crate::tr_format!("Von der lokalen KI umgeschrieben: {}", "Rewritten by the local AI: {}", first_line(&sql)), check: true, moved: 0, snippet: first_line(&sql) })
}

/// Art und Ziel einer Anweisung: ("CREATE TABLE", Some("kunde")), ("INSERT", Some("kunde")), ("SELECT", None)
fn head(sql: &str) -> (String, Option<String>) {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let Some(first) = toks.first() else { return (String::new(), None) };
    // Tippfehler im ersten Wort zaehlen als das naechstliegende Schluesselwort (SELEC -> SELECT, nie DROP)
    const KINDS: [&str; 15] = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "ALTER", "DROP", "REPLACE", "WITH", "TRUNCATE", "RENAME", "USE", "SET", "CALL", "SHOW"];
    let mut kind = first.upper();
    if !KINDS.contains(&kind.as_str()) {
        kind = super::lint::nearest(&kind, KINDS).map_or(kind.clone(), str::to_string);
    }
    let mut k = 1;
    if ["CREATE", "DROP", "ALTER"].contains(&kind.as_str()) {
        while toks.get(k).is_some_and(|t| ["OR", "REPLACE", "TEMPORARY", "UNIQUE", "IF", "NOT", "EXISTS"].iter().any(|w| t.is_word(w))) {
            k += 1;
        }
        kind = format!("{kind} {}", toks.get(k).map(|t| t.upper()).unwrap_or_default());
        k += 1;
        while toks.get(k).is_some_and(|t| ["IF", "NOT", "EXISTS"].iter().any(|w| t.is_word(w))) {
            k += 1;
        }
    } else if ["INSERT", "REPLACE"].contains(&kind.as_str()) {
        return (kind, insert_parts(&toks).map(|(i, _)| toks[i].text.to_ascii_lowercase()));
    } else if kind == "DELETE" {
        k = toks.iter().position(|t| t.is_word("FROM")).map_or(toks.len(), |p| p + 1);
    } else if kind != "UPDATE" && kind != "TRUNCATE" {
        return (kind, None);
    }
    if toks.get(k).is_some_and(|t| t.is_word("TABLE")) {
        k += 1;
    }
    (kind, toks.get(k).filter(|t| t.is_name()).map(|t| t.text.to_ascii_lowercase()))
}

/// Gleiche Art von Anweisung mit demselben Ziel (die KI darf z. B. kein DROP aus einem INSERT oder keine
/// Datenbank aus einer Tabelle machen)
fn same_kind(a: &str, b: &str) -> bool {
    db::split_statements(b).len() == 1 && head(a) == head(b)
}

/// CREATE TABLE `table` im Skript: Spalte `col` als Schluessel automatisch zaehlen lassen bzw. NULL erlauben
fn loosen_column(text: &str, table: &str, col: &str) -> Option<Cand> {
    for st in db::split_statements(text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        let Some((name, _)) = super::script::create_columns(&chars, &toks) else { continue };
        if !name.eq_ignore_ascii_case(table) {
            continue;
        }
        let open = toks.iter().position(|t| t.kind == Kind::Open)?;
        let close = group_end(&toks, open)?;
        for (a, b) in items(&toks, open, close) {
            if !toks[a].is_name() || !toks[a].text.eq_ignore_ascii_case(col) {
                continue;
            }
            let item = &toks[a..b];
            let key = item.iter().any(|t| t.is_word("PRIMARY"));
            let int = item.get(1).is_some_and(|t| t.upper().contains("INT"));
            let edit = if key && int {
                // AUTO_INCREMENT hinter dem Typ (samt Klammer)
                let mut k = a + 2;
                if toks.get(k).is_some_and(|t| t.kind == Kind::Open) {
                    k = group_end(&toks, k)? + 1;
                }
                while toks.get(k).is_some_and(|t| t.is_word("UNSIGNED") || t.is_word("ZEROFILL")) {
                    k += 1;
                }
                let at = toks[k - 1].end;
                let label = crate::tr_format!("{table}.{col} zählt automatisch (AUTO_INCREMENT)", "{table}.{col} counts automatically (AUTO_INCREMENT)");
                (at, at, " AUTO_INCREMENT".to_string(), label)
            } else {
                let k = (a..b.saturating_sub(1)).find(|&k| toks[k].is_word("NOT") && toks[k + 1].is_word("NULL"))?;
                let label = crate::tr_format!("{table}.{col} darf leer bleiben (NULL)", "{table}.{col} may be left empty (NULL)");
                (toks[k].start, toks[k + 1].end, "NULL".to_string(), label)
            };
            let mut out: Vec<char> = chars.clone();
            out.splice(edit.0..edit.1, edit.2.chars());
            let new: String = out.into_iter().collect();
            let t = replace_stmt(text, &st, &new)?;
            return Some(Cand { text: t, label: edit.3, check: false, moved: 0, snippet: first_line(&new) });
        }
        return None;
    }
    None
}

/// Pflichtfeld ohne Wert (1364): Spalte mit Platzhalter in die Spaltenliste und jede Zeile aufnehmen (bitte ansehen)
fn add_required_value(text: &str, f: &Fail, toks: &[Tok], chars: &[char], table: &str, col: &str) -> Option<Cand> {
    let info = f.schema.as_ref()?.table(table)?;
    let column = info.columns.iter().find(|c| c.name.eq_ignore_ascii_case(col))?;
    let (ti, vi) = insert_parts(toks)?;
    if !toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) {
        return None;
    }
    let list_end = group_end(toks, ti + 1)?;
    let value = placeholder(table, column, "neu");
    let mut edits = vec![(toks[list_end].start, format!(", {}", super::lint::quote_if_needed(&column.name)))];
    let mut k = vi + 1;
    while toks.get(k).is_some_and(|t| t.kind == Kind::Open) {
        let c = group_end(toks, k)?;
        edits.push((toks[c].start, format!(", {value}")));
        k = c + 1;
        if toks.get(k).is_some_and(|t| t.kind == Kind::Comma) {
            k += 1;
        }
    }
    let mut out: Vec<char> = chars.to_vec();
    for (at, ins) in edits.into_iter().rev() {
        out.splice(at..at, ins.chars());
    }
    let new: String = out.into_iter().collect();
    let t = replace_stmt(text, &f.stmt, &new)?;
    Some(Cand {
        text: t,
        label: crate::tr_format!("{table}.{col} muss ausgefüllt sein: Platzhalter {value} eingetragen", "{table}.{col} is required: placeholder {value} filled in"),
        check: true,
        moved: 0,
        snippet: first_line(&new),
    })
}

/// Fehlende Elternzeilen (1452) mit Platzhalterwerten vor dem INSERT anlegen
fn parent_rows(text: &str, f: &Fail, toks: &[Tok], chars: &[char], server: &mut dyn Server, sandbox: &Map) -> Option<Cand> {
    // "... FOREIGN KEY (`kunde_id`) REFERENCES `kunde` (`id`))"
    let e = &f.error;
    let fk_col = quoted(e, "FOREIGN KEY (`")?;
    let parent = quoted(e, "REFERENCES `")?;
    let ref_col = quoted(&e[e.find("REFERENCES `")?..], "(`")?;
    let schema = f.schema.as_ref()?;
    let ptable = schema.table(&parent)?;
    let (ti, vi) = insert_parts(toks)?;
    // Position der Spalte in der Liste bzw. in der Tabelle
    let list: Vec<String> = if toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) {
        let close = group_end(toks, ti + 1)?;
        toks[ti + 2..close].iter().filter(|t| t.is_name()).map(|t| t.text.to_ascii_lowercase()).collect()
    } else {
        schema.table(&toks[ti].text)?.columns.iter().filter(|c| !c.is_generated()).map(|c| c.name.to_ascii_lowercase()).collect()
    };
    let pos = list.iter().position(|c| c.eq_ignore_ascii_case(&fk_col))?;
    let mut wanted: Vec<String> = Vec::new();
    let mut k = vi + 1;
    while let Some(t) = toks.get(k) {
        if t.kind == Kind::Open {
            let close = group_end(toks, k)?;
            let (a, b) = *items(toks, k, close).get(pos)?;
            let v: String = chars[toks[a].start..toks[b - 1].end].iter().collect();
            if !v.eq_ignore_ascii_case("NULL") && !wanted.contains(&v) {
                wanted.push(v);
            }
            k = close + 1;
        } else {
            k += 1;
        }
    }
    // Vorhandene Werte in der Probe-Datenbank
    let s = sandbox.get(&schema.name)?;
    let have: HashSet<String> = server
        .rows(&format!("SELECT {} FROM {}.{}", db::q(&ref_col), db::q(s), db::q(&ptable.name)))
        .ok()?
        .into_iter()
        .filter_map(|r| r.into_iter().next().flatten())
        .collect();
    let missing: Vec<&String> = wanted.iter().filter(|v| !have.contains(v.trim_matches('\''))).collect();
    if missing.is_empty() || missing.len() > 20 {
        return None;
    }
    // Pflichtspalten der Elterntabelle (ohne Standardwert); Fremdschluessel darin waeren geraten
    let required: Vec<&crate::db::ColumnInfo> = ptable
        .columns
        .iter()
        .filter(|c| !c.name.eq_ignore_ascii_case(&ref_col) && !c.nullable && c.default.is_none() && !c.extra.contains("auto_increment") && !c.is_generated())
        .collect();
    if required.iter().any(|c| schema.is_fk_column(&ptable.name, &c.name)) {
        return None;
    }
    let mut cols = vec![super::lint::quote_if_needed(&ref_col)];
    cols.extend(required.iter().map(|c| super::lint::quote_if_needed(&c.name)));
    let rows: Vec<String> = missing
        .iter()
        .map(|v| {
            let mut vals = vec![(*v).clone()];
            vals.extend(required.iter().map(|c| placeholder(&ptable.name, c, v.trim_matches('\''))));
            format!("({})", vals.join(", "))
        })
        .collect();
    let sql = format!("INSERT INTO {} ({}) VALUES {};\n", super::lint::quote_if_needed(&ptable.name), cols.join(", "), rows.join(", "));
    let shown: Vec<&str> = missing.iter().map(|v| v.as_str()).collect();
    Some(Cand {
        text: insert_before(text, &f.stmt, &sql),
        label: crate::tr_format!(
            "Fehlende Zeile(n) in {} ({} = {}) mit Platzhaltern angelegt – Werte bitte prüfen",
            "Missing row(s) in {} ({} = {}) created with placeholders – please check the values",
            ptable.name,
            ref_col,
            shown.join(", ")
        ),
        check: true,
        moved: 0, snippet: first_line(&sql),
    })
}

// ---------------------------------------------------------------------------
// Ablauf

/// Ist `new` weiter gekommen als `old`? `inserted`: Anweisungen, die der Vorschlag hinzugefuegt hat
fn progressed(old: &Trial, new: &Trial, inserted: isize, seen: &HashSet<(String, Option<u32>)>) -> bool {
    let Some(nf) = &new.fail else { return true };
    let adj = new.ok as isize - inserted;
    if adj > old.ok as isize {
        return true;
    }
    let of = old.fail.as_ref();
    // Gleicher Syntaxfehler, aber spaeter in der Anweisung: auch ein Schritt (z. B. zwei reservierte Namen)
    let later = |of: &Fail| errfix::code(&nf.error) == Some(1064) && errfix::code(&of.error) == Some(1064) && syntax_tail(nf).zip(syntax_tail(of)).is_some_and(|(n, o)| n < o);
    adj == old.ok as isize
        && of.is_some_and(|of| errfix::code(&nf.error) != errfix::code(&of.error) || later(of))
        && !seen.contains(&(nf.stmt.sql.clone(), errfix::code(&nf.error)))
}

/// Zeichen der Anweisung ab der Stelle eines Syntaxfehlers ("near '…'")
fn syntax_tail(f: &Fail) -> Option<usize> {
    let rest = &f.error[f.error.find("near '")? + 6..];
    let near = rest.rfind("' at line").map_or(rest, |e| &rest[..e]);
    let probe: String = near.chars().take(30).collect();
    if probe.trim().is_empty() {
        return Some(0);
    }
    let at = f.stmt.sql.find(probe.as_str())?;
    Some(f.stmt.sql[at..].chars().count())
}

fn line_of(text: &str, snippet: &str) -> Option<usize> {
    if snippet.is_empty() {
        return None;
    }
    let b = text.find(snippet)?;
    Some(text[..b].matches('\n').count())
}

/// Anweisungen, die Daten oder Struktur loeschen (DROP, TRUNCATE, DELETE, ALTER … DROP, UPDATE ohne WHERE)
fn destructive_count(text: &str) -> usize {
    db::split_statements(text)
        .iter()
        .filter(|st| {
            let chars: Vec<char> = st.sql.chars().collect();
            let toks = lex(&chars).toks;
            let has = |k: &str| toks.iter().any(|t| t.is_word(k));
            match toks.first() {
                Some(t) if t.is_word("DROP") || t.is_word("TRUNCATE") || t.is_word("DELETE") => true,
                Some(t) if t.is_word("ALTER") => has("DROP"),
                Some(t) if t.is_word("UPDATE") => !has("WHERE"),
                _ => false,
            }
        })
        .count()
}

/// Loescht die Korrektur mehr als das Original? Dann laeuft sie nie ungefragt auf den echten Daten.
pub fn adds_destruction(before: &str, after: &str) -> bool {
    destructive_count(after) > destructive_count(before)
}

/// Auto-Fix: erst die Pruefung des Editors, dann Probelaeufe auf dem Server bis das Skript fehlerfrei laeuft.
pub fn repair(text: &str, setup: &Setup, server: Option<&mut dyn Server>, status: &dyn Fn(String), cancel: &AtomicBool, ai: Option<Ai<'_>>) -> Report {
    let started = Instant::now();
    let Some(server) = server else {
        let schema = None;
        let env = Env { schema, databases: &setup.databases };
        let (text, changes) = static_fix(text, &env, false);
        return Report { text, changes, end: End::Offline, runs: 0, skipped: 0, without_data: false };
    };
    status(crate::i18n::tr("Skript wird geprüft …", "Checking the script …").into());
    // Die gewaehlte Datenbank gibt es auf dem Server, auch wenn die Liste des Aufrufers aelter ist
    let mut fresh = setup.clone();
    if let Some(d) = fresh.database.clone().filter(|d| !d.is_empty())
        && !fresh.databases.iter().any(|x| x.eq_ignore_ascii_case(&d))
        && server.schema(&d).is_some()
    {
        fresh.databases.push(d);
    }
    let setup = &fresh;
    let real = setup.database.as_deref().filter(|d| !d.is_empty()).and_then(|d| server.schema(d));
    let env = Env { schema: real.as_ref(), databases: &setup.databases };
    let (mut text, mut changes) = static_fix(text, &env, true);
    let finish = |text: String, mut changes: Vec<Change>, end: End, runs: usize, skipped: usize, without_data: bool| {
        for c in &mut changes {
            c.line = line_of(&text, &c.snippet);
        }
        // Von oben nach unten lesen
        changes.sort_by_key(|c| c.line.unwrap_or(usize::MAX));
        Report { text, changes, end, runs, skipped, without_data }
    };
    let map = referenced(&text, setup);
    // Arbeitet das Skript in einer Systemdatenbank, laesst es sich nicht abgeschirmt probieren
    for st in db::split_statements(&text) {
        let chars: Vec<char> = st.sql.chars().collect();
        let toks = lex(&chars).toks;
        if toks.first().is_some_and(|t| t.is_word("USE")) && toks.get(1).is_some_and(|t| is_system(&t.text)) {
            return finish(text, changes, End::Failed(crate::i18n::tr("Das Skript arbeitet in einer Systemdatenbank – kein Probelauf.", "The script works in a system database – no trial run.").into()), 0, 0, false);
        }
    }
    let on_server: Vec<String> = map.pairs.iter().map(|(o, _)| o.clone()).filter(|o| setup.databases.iter().any(|d| d.eq_ignore_ascii_case(o))).collect();
    // Datenmenge: kleine Datenbanken werden samt Inhalt kopiert
    let mut rows = 0u64;
    let mut tables = 0usize;
    for o in &on_server {
        if let Some(s) = server.schema(o) {
            tables += s.tables.len();
            rows += s.tables.iter().map(|t| t.rows.unwrap_or(0)).sum::<u64>();
        }
    }
    let copy_data = rows <= COPY_ROWS && tables <= COPY_TABLES;
    let existing: Vec<(String, bool)> = on_server.iter().map(|o| (o.clone(), !dropped_first(&text, o))).collect();
    let dynamic = {
        let chars: Vec<char> = text.chars().collect();
        lex(&chars).toks.iter().any(|t| t.is_word("PREPARE") || t.is_word("IMMEDIATE"))
    };
    let current = setup.database.clone().filter(|d| !d.is_empty());
    // Reste eines abgebrochenen Laufs (anderer Prozess, z. B. nach einem Absturz) entfernen
    if let Ok(rows) = server.rows(&format!("SHOW DATABASES LIKE '{}%'", SANDBOX_PREFIX.replace('_', "\\_"))) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        for r in rows {
            if let Some(Some(name)) = r.first()
                && stale_sandbox(name, now)
            {
                let _ = server.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(name)));
            }
        }
    }
    let mut probe = Probe { server, map, existing, current, copy_data, dynamic, skipped: 0, on_server: setup.databases.clone(), cache: Default::default(), names: HashSet::new() };
    let mut runs = 0;
    let mut tried: HashSet<String> = HashSet::new();
    let mut seen: HashSet<(String, Option<u32>)> = HashSet::new();
    tried.insert(text.clone());
    let end = 'outer: loop {
        status(crate::tr_format!("Probelauf {} …", "Trial run {} …", runs + 1));
        let cur = match probe.trial(&text) {
            Ok(t) => t,
            Err(e) => break End::Failed(e),
        };
        runs += 1;
        let Some(fail) = cur.fail.clone() else { break End::Clean };
        seen.insert((fail.stmt.sql.clone(), errfix::code(&fail.error)));
        if cancel.load(Ordering::Relaxed) {
            break End::Cancelled;
        }
        let stuck = || End::Stuck { line: fail.stmt.line, error: fail.error.clone() };
        if runs >= MAX_RUNS || started.elapsed() > MAX_TIME {
            break stuck();
        }
        let base = db::split_statements(&text).len() as isize;
        let mut cands = {
            let Probe { server, map, .. } = &mut probe;
            candidates(&text, &fail, setup, &mut **server, map)
        };
        let mut asked = ai.is_none();
        let mut k = 0;
        loop {
            if k == cands.len() {
                // Die KI erst fragen, wenn keine Regel geholfen hat
                if asked {
                    break;
                }
                asked = true;
                status(crate::tr_format!("Probelauf {}: die lokale KI wird gefragt …", "Trial run {}: asking the local AI …", runs + 1));
                match ai.and_then(|a| ai_candidate(&text, &fail, a)) {
                    Some(c) => cands.push(c),
                    None => break,
                }
            }
            let cand = cands[k].clone();
            k += 1;
            if !tried.insert(cand.text.clone()) {
                continue;
            }
            if cancel.load(Ordering::Relaxed) {
                break 'outer End::Cancelled;
            }
            status(crate::tr_format!("Probelauf {}: {} …", "Trial run {}: {} …", runs + 1, cand.label));
            let next = match probe.trial(&cand.text) {
                Ok(t) => t,
                Err(e) => break 'outer End::Failed(e),
            };
            runs += 1;
            let inserted = db::split_statements(&cand.text).len() as isize - base + cand.moved as isize;
            if progressed(&cur, &next, inserted, &seen) {
                text = cand.text;
                changes.push(Change { label: cand.label, line: None, check: cand.check, snippet: cand.snippet });
                if next.fail.is_none() {
                    break 'outer End::Clean;
                }
                continue 'outer;
            }
            if runs >= MAX_RUNS || started.elapsed() > MAX_TIME {
                break;
            }
        }
        break stuck();
    };
    let (skipped, without_data) = (probe.skipped, !probe.copy_data);
    probe.cleanup();
    finish(text, changes, end, runs, skipped, without_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(sql: &str) -> Vec<Tok> {
        let chars: Vec<char> = sql.chars().collect();
        lex(&chars).toks
    }

    fn map(names: &[&str]) -> Map {
        let mut m = Map::default();
        for n in names {
            m.add(n);
        }
        m
    }

    #[test]
    fn statements_are_rewritten_to_the_sandbox() {
        let m = map(&["schule", "shop"]);
        let s = m.get("schule").unwrap().to_string();
        let p = m.get("shop").unwrap().to_string();
        assert_eq!(rewrite("USE schule", &m).unwrap(), format!("USE `{s}`"));
        assert_eq!(rewrite("DROP DATABASE IF EXISTS `schule`", &m).unwrap(), format!("DROP DATABASE IF EXISTS `{s}`"));
        assert_eq!(rewrite("CREATE DATABASE IF NOT EXISTS schule", &m).unwrap(), format!("CREATE DATABASE IF NOT EXISTS `{s}`"));
        assert_eq!(rewrite("SELECT k.name FROM shop.kunde k", &m).unwrap(), format!("SELECT k.name FROM `{p}`.kunde k"));
        // Unbekannte Datenbank: nicht ausfuehren
        assert!(rewrite("DROP DATABASE fremd", &m).is_none());
        assert!(rewrite("USE", &m).is_none());
        assert_eq!(m.unmap(&format!("Table '{s}.x' doesn't exist")), "Table 'schule.x' doesn't exist");
    }

    #[test]
    fn only_harmless_statements_run() {
        let ok = |s: &str| allowed(s, &toks(s), false);
        assert!(ok("CREATE TABLE user (id INT)"));
        assert!(ok("CREATE OR REPLACE VIEW v AS SELECT 1"));
        assert!(ok("INSERT INTO t VALUES (1)"));
        assert!(ok("SET FOREIGN_KEY_CHECKS = 0"));
        assert!(!ok("CREATE USER 'a'@'%' IDENTIFIED BY 'x'"));
        assert!(!ok("GRANT ALL ON *.* TO 'a'@'%'"));
        assert!(!ok("SET GLOBAL max_connections = 5"));
        assert!(!ok("SET @@global.sql_mode = ''"));
        assert!(!ok("SELECT * FROM t INTO OUTFILE '/tmp/x'"));
        assert!(!ok("LOAD DATA INFILE 'x' INTO TABLE t"));
        assert!(!ok("DELETE FROM mysql.user"));
        assert!(ok("SELECT * FROM mysql.user"));
        assert!(!allowed("CALL p()", &toks("CALL p()"), true));
        assert!(!ok("SHUTDOWN"));
    }

    #[test]
    fn progress_is_measured_without_inserted_statements() {
        let st = |sql: &str| db::Stmt { sql: sql.into(), start: 0, end: 0, line: 0 };
        let fail = |sql: &str, e: &str| Some(Fail { stmt: st(sql), error: e.into(), schema: None });
        let old = Trial { ok: 3, fail: fail("INSERT", "ERROR 1452 (23000): x") };
        let seen = HashSet::new();
        assert!(progressed(&old, &Trial { ok: 5, fail: None }, 1, &seen));
        assert!(progressed(&old, &Trial { ok: 5, fail: fail("X", "ERROR 1062 (23000): y") }, 1, &seen));
        // Nur eine eingefuegte Anweisung mehr, gleicher Fehler: kein Fortschritt
        assert!(!progressed(&old, &Trial { ok: 4, fail: fail("INSERT", "ERROR 1452 (23000): x") }, 1, &seen));
        // Gleiche Stelle, anderer Fehler: weiter, aber nicht im Kreis
        assert!(progressed(&old, &Trial { ok: 3, fail: fail("INSERT", "ERROR 1364 (HY000): z") }, 0, &seen));
        let mut seen2 = HashSet::new();
        seen2.insert(("INSERT".to_string(), Some(1364)));
        assert!(!progressed(&old, &Trial { ok: 3, fail: fail("INSERT", "ERROR 1364 (HY000): z") }, 0, &seen2));
    }

    #[test]
    fn required_key_columns_count_automatically() {
        let sql = "CREATE TABLE kunde (\n  id INT PRIMARY KEY,\n  name VARCHAR(50) NOT NULL\n);\nINSERT INTO kunde (name) VALUES ('Ada');";
        let c = loosen_column(sql, "kunde", "id").unwrap();
        assert!(c.text.contains("id INT AUTO_INCREMENT PRIMARY KEY"), "{}", c.text);
        let c = loosen_column(sql, "kunde", "name").unwrap();
        assert!(c.text.contains("name VARCHAR(50) NULL"), "{}", c.text);
    }

    #[test]
    fn ai_answers_must_keep_kind_and_target() {
        assert!(same_kind("SELEC name FORM kunde", "SELECT name FROM kunde"));
        assert!(same_kind("INSERT INTO kunde VALUES (1 'a')", "INSERT INTO kunde VALUES (1, 'a')"));
        assert!(!same_kind("CREATE TABLE schueler (id INT", "CREATE DATABASE schueler"));
        assert!(!same_kind("INSERT INTO kunde VALUES (1)", "INSERT INTO bestellung VALUES (1)"));
        assert!(!same_kind("UPDATE kunde SET a = 1", "DELETE FROM kunde"));
        assert!(!same_kind("SELECT 1", "SELECT 1; DROP TABLE kunde"));
        assert!(same_kind("CREATE TABLE IF NOT EXISTS t (a INT)", "CREATE TABLE t (a INT)"));
        assert!(!same_kind("DELET FROM kunde", "DROP TABLE kunde"));
        assert!(same_kind("DELET FROM kunde WHERE id = 1", "DELETE FROM kunde WHERE id = 1"));
    }

    #[test]
    fn added_drops_are_never_safe() {
        let orig = "CREATE DATABASE schule;\nUSE schule;\nDELETE FROM t WHERE id = 1;";
        assert!(adds_destruction(orig, &format!("DROP DATABASE IF EXISTS schule;\n{orig}")));
        assert!(adds_destruction("UPDATE t SET a = 1 WHERE id = 2", "UPDATE t SET a = 1"));
        assert!(!adds_destruction("DROP TABLE t; SELECT 1", "DROP TABLE IF EXISTS t; SELECT 1"));
        assert!(!adds_destruction("INSERT INTO t VALUES (1)", "INSERT IGNORE INTO t (id) VALUES (1)"));
        assert!(!adds_destruction("SELECT 1 -- DROP TABLE x", "SELECT 1 -- DROP TABLE x\n;"));
    }

    #[test]
    fn only_old_leftovers_are_cleaned_up() {
        let now = started() + 10;
        // Eigene und frische fremde Kopien bleiben, alte und solche im alten Namensformat gehen
        assert!(!stale_sandbox(&Map::new().sandbox(1), now));
        assert!(!stale_sandbox(&format!("{SANDBOX_PREFIX}999999999_{}_1_1", now - 60), now));
        assert!(stale_sandbox(&format!("{SANDBOX_PREFIX}999999999_{}_1_1", now - 7200), now));
        assert!(stale_sandbox(&format!("{SANDBOX_PREFIX}123_4_5"), now));
        assert!(!stale_sandbox("andere_db", now));
    }

    #[test]
    fn scripts_that_drop_their_database_first_need_no_copy() {
        assert!(dropped_first("DROP DATABASE IF EXISTS schule;\nCREATE DATABASE schule;", "schule"));
        assert!(!dropped_first("USE schule;\nDROP DATABASE schule;", "schule"));
    }

    /// Echter Server: EASYMYSQL_AUTOFIX_TEST_PORT=3306 cargo test probe_ -- --ignored
    #[test]
    #[ignore]
    fn probe_repairs_a_school_script_on_mariadb() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("probe_test_{}", std::process::id());
        let script = format!(
            "CREATE DATABASE {name};\nUSE {name};\n\
             CREATE TABLE bestellung (id INT PRIMARY KEY AUTO_INCREMENT, kunde_id INT NOT NULL, betrag DECIMAL(8,2),\n  FOREIGN KEY (kunde_id) REFERENCES kunde(id));\n\
             CREATE TABLE kunde (id INT PRIMARY KEY, name VARCHAR(5) NOT NULL);\n\
             INSERT INTO kunde (name) VALUES ('Ada'), ('Grace Hopper');\n\
             INSERT INTO bestellung (kunde_id, betrag) VALUES (1, 9.5), (7, 3);\n\
             SELECT k.name, SUM(b.betrag) FROM kunde k JOIN bestellung b ON b.kunde_id = k.id GROUP BY k.name;\n"
        );
        let setup = Setup { database: None, databases: dbc.databases().unwrap(), file_stem: None };
        let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
        let r = repair(&script, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}\n{:#?}", r.text, r.changes);
        assert_eq!(r.end, End::Clean, "{}", r.text);
        // Nichts davon ist auf dem echten Server geblieben
        assert!(!dbc.databases().unwrap().iter().any(|d| d == &name || d.starts_with("easymysql_probe_")));
        // Das reparierte Skript laeuft auch in echt
        let out = crate::db::run_script(&mut dbc.new_conn().unwrap(), &r.text);
        assert!(out.error.is_none(), "{:?}", out.error);
        // Zweiter Lauf: die Datenbank gibt es nun (mit Daten) - das Skript soll wiederholbar werden
        dbc.exec(&format!("INSERT INTO {}.kunde (name) VALUES ('echt')", db::q(&name)), ()).unwrap();
        let setup = Setup { database: Some(name.clone()), databases: dbc.databases().unwrap(), file_stem: None };
        let r2 = repair(&r.text, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}
{:#?}", r2.text, r2.changes);
        assert_eq!(r2.end, End::Clean);
        assert!(r2.text.contains("DROP DATABASE IF EXISTS"), "{}", r2.text);
        // Die echten Daten sind unberuehrt
        let rows = dbc.query(&format!("SELECT COUNT(*) FROM {}.kunde WHERE name = 'echt'", db::q(&name))).unwrap();
        assert_eq!(rows.rows[0][0].as_deref(), Some("1"));
        // Ohne CREATE DATABASE, in der gewaehlten Datenbank mit vorhandenen Tabellen
        let tables_only = "CREATE TABLE kunde (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL);
INSERT INTO kunde (name) VALUES ('Neu');";
        let r3 = repair(tables_only, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}
{:#?}", r3.text, r3.changes);
        assert_eq!(r3.end, End::Clean);
        assert!(!dbc.databases().unwrap().iter().any(|d| d.starts_with("easymysql_probe_")));
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(&name)), ());
    }
}

/// Echter Server: der Rest eines Laufs (ohne USE) wird in der gewaehlten Datenbank repariert
/// EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test probe_heals -- --ignored --nocapture
#[cfg(test)]
mod heal_tests {
    use super::*;

    #[test]
    #[ignore]
    fn probe_heals_the_rest_of_a_run() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("heal_test_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(&name, "CREATE TABLE klasse (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(10)); INSERT INTO klasse (name) VALUES ('4AHIF')").unwrap();
        let rest = "SELECT nme FROM klasse;\nINSERT INTO klasse (name) VALUES ('5AHIF');\nSELECT * FROM klasse;\n";
        // Liste von vor dem Lauf: die eben angelegte Datenbank fehlt darin
        let databases: Vec<String> = dbc.databases().unwrap().into_iter().filter(|d| d != &name).collect();
        let setup = Setup { database: Some(name.clone()), databases, file_stem: None };
        let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
        let r = repair(rest, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}\n{:#?}", r.text, r.changes);
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(&name)), ());
        assert_eq!(r.end, End::Clean, "{:?}", r.end);
        assert!(r.text.starts_with("SELECT name FROM klasse"), "{}", r.text);
        assert!(!adds_destruction(rest, &r.text));
    }
}

/// Echter Server: ohne gewaehlte Datenbank findet der Auto-Fix die Datenbank mit den benutzten Tabellen
/// EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test probe_picks -- --ignored --nocapture
#[cfg(test)]
mod pick_tests {
    use super::*;

    #[test]
    #[ignore]
    fn probe_picks_the_database_with_the_tables() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("pick_test_{}", std::process::id());
        let table = format!("nur_hier_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(&name, &format!("CREATE TABLE {table} (id INT PRIMARY KEY, name VARCHAR(10)); INSERT INTO {table} VALUES (1, 'a')")).unwrap();
        let script = format!("SELECT name FROM {table};");
        let setup = Setup { database: None, databases: dbc.databases().unwrap(), file_stem: None };
        let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
        let r = repair(&script, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}\n{:#?}", r.text, r.changes);
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(&name)), ());
        assert_eq!(r.end, End::Clean);
        assert!(r.text.starts_with(&format!("USE {name};")), "{}", r.text);
        assert!(r.changes.iter().all(|c| !c.check));
    }

    /// Grosse Tabelle in einer anderen Datenbank (nur die Struktur wird kopiert)
    #[test]
    #[ignore]
    fn probe_picks_a_big_database() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("pick_big_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(&name, "CREATE TABLE riesig (id INT PRIMARY KEY AUTO_INCREMENT, sensor INT); INSERT INTO riesig (sensor) SELECT seq % 50 FROM seq_1_to_60000; ANALYZE TABLE riesig").unwrap();
        let other = format!("pick_other_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {other}"), ()).unwrap();
        let setup = Setup { database: Some(other.clone()), databases: dbc.databases().unwrap(), file_stem: None };
        let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
        let r = repair("SELECT sensor FROM riesig WHERE sensor = 4;", &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), None);
        eprintln!("{}\n{:#?}\n{:?}", r.text, r.changes, r.end);
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(&name)), ());
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", db::q(&other)), ());
        assert_eq!(r.end, End::Clean);
        assert!(r.changes.iter().all(|c| !c.check), "{:?}", r.changes);
    }
}

#[cfg(test)]
mod ai_tests {
    use super::*;

    /// Echter Server und installierte lokale KI:
    /// EASYMYSQL_AUTOFIX_TEST_PORT=3306 EASYMYSQL_TEST_DATA_DIR=… cargo test probe_with_ai -- --ignored
    #[test]
    #[ignore]
    fn probe_with_ai_on_mariadb() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let script = "CREATE TABLE schueler (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(50), klasse VARCHAR(5));\n\
                      INSERT INTO schueler (name, klasse) VALUES ('Ada', '3A'), ('Linus', '3B');\n\
                      SELECT klasse COUNT(*) FROM schueler GROUP BY klasse;\n\
                      SELECT name FROM schueler WHERE klasse = '3A' ORDER BY name DESC LIMT 1;\n";
        let setup = Setup { database: None, databases: dbc.databases().unwrap(), file_stem: Some("Schule Probe".into()) };
        let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
        let ask = |sql: &str, err: &str, schema: Option<&Schema>| {
            let r = crate::sqledit::ai::fix_statement(sql, err, schema);
            eprintln!("KI: {sql} / {err} -> {r:?}");
            r
        };
        let r = repair(script, &setup, Some(&mut live), &|s| eprintln!("{s}"), &AtomicBool::new(false), Some(&ask));
        eprintln!("{}\n{:#?}\n{:?}", r.text, r.changes, r.end);
        crate::sqledit::ai::shutdown();
        assert_eq!(r.end, End::Clean);
        assert!(!dbc.databases().unwrap().iter().any(|d| d.starts_with("easymysql_probe_") || d == "schule_probe"));
    }
}

/// Typische Fehler aus dem Unterricht gegen einen echten Server reparieren (ohne KI):
/// EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test autofix_corpus -- --ignored --nocapture
#[cfg(test)]
mod corpus_tests {
    use super::*;

    const BASE: &str = "CREATE TABLE klasse (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(10) NOT NULL);\n\
        CREATE TABLE schueler (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(50), klasse_id INT, geburt DATE,\n  FOREIGN KEY (klasse_id) REFERENCES klasse(id));\n\
        INSERT INTO klasse (name) VALUES ('3A'), ('3B');\n\
        INSERT INTO schueler (name, klasse_id, geburt) VALUES ('Ada', 1, '2008-01-02'), ('Linus', 2, '2008-05-06');\n";

    pub(super) const CASES: &[(&str, &str)] = &[
        ("Komma vor Funktion", "SELECT klasse_id COUNT(*) FROM schueler GROUP BY klasse_id;"),
        ("LIMT", "SELECT name FROM schueler ORDER BY name LIMT 1;"),
        ("SELEC", "SELEC * FROM schueler;"),
        ("FORM", "SELECT * FORM schueler;"),
        ("Werte ohne Spaltenliste", "INSERT INTO schueler VALUES ('Grace', 1, '2008-03-04');"),
        ("Komma in VALUES", "INSERT INTO schueler (name, klasse_id) VALUES ('Grace' 1);"),
        ("Text ohne Anfuehrungszeichen", "SELECT * FROM schueler WHERE name = Ada;"),
        ("Doppeltes Gleich", "SELECT * FROM schueler WHERE klasse_id == 1;"),
        ("VARCHAR ohne Laenge", "CREATE TABLE lehrer (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR);"),
        ("Komma vor Klammer zu", "CREATE TABLE fach (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(30),);"),
        ("DECS", "SELECT * FROM schueler ORDER BY name DECS;"),
        ("Strichpunkt fehlt", "SELECT name FROM schueler\nSELECT name FROM klasse;"),
        ("Komma in SET", "UPDATE schueler SET name = 'Ada L' klasse_id = 2 WHERE id = 1;"),
        ("Deutsches Datum", "INSERT INTO schueler (name, klasse_id, geburt) VALUES ('Grace', 1, '24.12.2008');"),
        ("DELETE *", "DELETE * FROM schueler WHERE id = 2;"),
        ("COUNT mit Leerzeichen", "SELECT COUNT (*) FROM schueler;"),
        ("Reserviertes Wort als Tabelle", "CREATE TABLE order (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(8,2));\nINSERT INTO order (betrag) VALUES (9.5);"),
        ("Typografische Anfuehrungszeichen", "SELECT * FROM schueler WHERE name = \u{2018}Ada\u{2019};"),
        ("IF NOT EXIST", "CREATE TABLE IF NOT EXIST raum (id INT PRIMARY KEY);"),
        ("AUTOINCREMENT", "CREATE TABLE raum (id INTEGER PRIMARY KEY AUTOINCREMENT, nummer VARCHAR(5));"),
        ("VARCHAR2", "CREATE TABLE raum (id NUMBER(10) PRIMARY KEY, nummer VARCHAR2(5));"),
        ("REFERENCES ohne Spalte", "CREATE TABLE note (id INT PRIMARY KEY AUTO_INCREMENT, schueler_id INT, FOREIGN KEY (schueler_id) REFERENCES schueler);"),
        ("TOP", "SELECT TOP 1 * FROM schueler;"),
        ("Apostroph im Text", "INSERT INTO schueler (name, klasse_id) VALUES ('O'Brien', 1);"),
        ("Klammer fehlt", "CREATE TABLE raum (id INT PRIMARY KEY, nummer VARCHAR(5);"),
        ("Alias statt Tabelle", "SELECT schueler.name FROM schueler s WHERE s.id = 1;"),
        ("Spalte vertippt", "SELECT nmae FROM schueler;"),
        ("Tabelle vertippt", "SELECT * FROM schuler;"),
        ("Tabelle in Mehrzahl", "SELECT * FROM klassen;"),
        ("Fehlende Elternzeile", "INSERT INTO schueler (name, klasse_id) VALUES ('Grace', 9);"),
        ("Doppelter Schluessel", "INSERT INTO klasse (id, name) VALUES (1, '4A');"),
        ("Zu langer Text", "INSERT INTO klasse (name) VALUES ('Abschlussklasse 2026');"),
        ("Tabelle gibt es schon", "CREATE TABLE klasse (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(10));"),
        ("Reihenfolge der Tabellen", "CREATE TABLE b (id INT PRIMARY KEY, a_id INT, FOREIGN KEY (a_id) REFERENCES a(id));\nCREATE TABLE a (id INT PRIMARY KEY);"),
        ("Gleichheit mit NULL", "SELECT * FROM schueler WHERE geburt = NULL;"),
        ("Aggregat in WHERE", "SELECT klasse_id FROM schueler WHERE COUNT(*) > 1 GROUP BY klasse_id;"),
        ("JOIN ohne ON", "SELECT s.name, k.name FROM schueler s JOIN klasse k WHERE s.klasse_id = k.id;"),
        ("Funktion vertippt", "SELECT COUNTT(*) FROM schueler;"),
        ("Datum ohne Anfuehrungszeichen", "SELECT * FROM schueler WHERE geburt > 2008-03-01;"),
        ("Tabelle fehlt ganz", "SELECT name, ort FROM kunde WHERE ort = 'Wien';"),
        ("Tabelle erst spaeter angelegt", "INSERT INTO fach (name) VALUES ('Mathe');\nCREATE TABLE fach (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(30));"),
        ("Elternzeile kommt spaeter", "INSERT INTO schueler (name, klasse_id) VALUES ('Grace', 3);\nINSERT INTO klasse (id, name) VALUES (3, '4C');"),
        ("Spalte fehlt", "INSERT INTO schueler (name, klasse_id, email) VALUES ('Grace', 1, 'grace@example.com');"),
        ("Spalte fehlt im eigenen CREATE", "CREATE TABLE lehrer (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(30));\nINSERT INTO lehrer (name, fach) VALUES ('Huber', 'Mathematik');"),
        ("Datenbank fehlt", "USE korpus_neu_xyz;\nCREATE TABLE t (id INT PRIMARY KEY);"),
        ("Verweis auf fehlende Tabelle", "CREATE TABLE note (id INT PRIMARY KEY AUTO_INCREMENT, wert INT, fach_id INT, FOREIGN KEY (fach_id) REFERENCES fach(id));\nINSERT INTO note (wert, fach_id) VALUES (1, 1);"),
        ("Zahl zu gross", "CREATE TABLE raum (nr TINYINT PRIMARY KEY);\nINSERT INTO raum VALUES (301);"),
        ("Telefon als Zahl", "CREATE TABLE kontakt (id INT PRIMARY KEY AUTO_INCREMENT, telefon INT);\nINSERT INTO kontakt (telefon) VALUES ('0664 1234567');"),
        ("Emoji in latin1", "CREATE TABLE kommentar (id INT PRIMARY KEY AUTO_INCREMENT, text VARCHAR(200)) CHARACTER SET latin1;\nINSERT INTO kommentar (text) VALUES ('Super 👍');"),
        ("ENUM-Wert fehlt", "CREATE TABLE auftrag (id INT PRIMARY KEY AUTO_INCREMENT, status ENUM('offen','erledigt') NOT NULL);\nINSERT INTO auftrag (status) VALUES ('offen'), ('storniert');"),
        ("AUTO_INCREMENT ohne Schluessel", "CREATE TABLE raum (id INT AUTO_INCREMENT, nummer VARCHAR(5));"),
        ("Zwei Primaerschluessel", "CREATE TABLE raum (id INT PRIMARY KEY, nummer VARCHAR(5) PRIMARY KEY);"),
        ("TEXT als Schluessel", "CREATE TABLE raum (name TEXT PRIMARY KEY);"),
        ("Eltern vor Kind geloescht", "DROP TABLE IF EXISTS klasse;\nDROP TABLE IF EXISTS schueler;\nCREATE TABLE klasse (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(10));\nCREATE TABLE schueler (id INT PRIMARY KEY AUTO_INCREMENT, klasse_id INT, FOREIGN KEY (klasse_id) REFERENCES klasse(id));"),
        ("Sicht gibt es schon", "CREATE VIEW v_schueler AS SELECT name FROM schueler;\nCREATE VIEW v_schueler AS SELECT name, geburt FROM schueler;"),
        ("Index gibt es schon", "CREATE INDEX ix_name ON schueler (name);\nCREATE INDEX ix_name ON schueler (name);"),
        ("Unterabfrage ohne Alias", "SELECT AVG(n) FROM (SELECT klasse_id, COUNT(*) AS n FROM schueler GROUP BY klasse_id);"),
        ("UNION ungleich", "SELECT name, geburt FROM schueler UNION SELECT name FROM klasse;"),
        ("Tabelle nur aus Werten", "INSERT INTO tier VALUES (1, 'Bello', '2020-05-01');\nSELECT * FROM tier;"),
        ("AUTO_INCREMENT auf Text", "CREATE TABLE raum (id VARCHAR(10) AUTO_INCREMENT PRIMARY KEY, name VARCHAR(20));"),
        ("Falscher Standardwert", "CREATE TABLE raum (id INT PRIMARY KEY, plaetze INT DEFAULT 'viele');"),
        ("IN mit zwei Spalten", "SELECT * FROM klasse WHERE id IN (SELECT klasse_id, name FROM schueler);"),
        ("SELECT fehlt", "* FROM schueler;"),
        ("SELECT zerstueckelt", "T name FROM schueler WHERE klasse_id = 1;"),
        ("WHRE", "SELECT * FROM schueler WHRE id = 1;"),
        ("ODER BY", "SELECT * FROM schueler ODER BY name;"),
        ("GROUP ohne BY", "SELECT klasse_id, COUNT(*) FROM schueler GROUP klasse_id;"),
        ("INSRT", "INSRT INTO klasse (name) VALUES ('4A');"),
        ("UPDTE", "UPDTE schueler SET name = 'Ada L' WHERE id = 1;"),
        ("DELTE", "DELTE FROM schueler WHERE id = 2;"),
        ("VALEUS", "INSERT INTO klasse (name) VALEUS ('4A');"),
        ("VALUES fehlt", "INSERT INTO klasse (name) ('4A');"),
        ("Zu viele Werte ohne Spaltenliste", "INSERT INTO klasse VALUES ('4A');"),
        ("Mehrdeutige Spalte", "SELECT name FROM schueler s JOIN klasse k ON s.klasse_id = k.id;"),
        ("HAVING statt WHERE", "SELECT name FROM schueler HAVING klasse_id = 1;"),
        ("ISNULL mit zwei Werten", "SELECT ISNULL(geburt, '2000-01-01') FROM schueler;"),
        ("GETDATE", "SELECT name, GETDATE() FROM schueler;"),
        ("LEN", "SELECT LEN(name) FROM schueler;"),
        ("IDENTITY", "CREATE TABLE raum (id INT IDENTITY(1,1) PRIMARY KEY, nummer NVARCHAR(5));"),
        ("GENERATED AS IDENTITY", "CREATE TABLE raum (id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, nummer VARCHAR(5));"),
        ("DATETIME2 und MONEY", "CREATE TABLE zahlung (id INT PRIMARY KEY AUTO_INCREMENT, am DATETIME2, betrag MONEY);"),
        ("Postgres-Cast", "SELECT name, klasse_id::TEXT FROM schueler;"),
        ("ILIKE", "SELECT * FROM schueler WHERE name ILIKE 'a%';"),
        ("DROP ohne Tabelle", "DROP TABLE lehrer;"),
        ("Doppelte Spalte", "CREATE TABLE raum (id INT PRIMARY KEY, name VARCHAR(5), name VARCHAR(10));"),
        ("Klammer in COUNT fehlt", "SELECT COUNT(* FROM schueler;"),
        ("Klammer zu viel", "SELECT * FROM schueler WHERE (id = 1));"),
        ("Komma vor FROM", "SELECT name, FROM schueler;"),
        ("AND in SET", "UPDATE schueler SET name = 'Ada L' AND klasse_id = 2 WHERE id = 1;"),
        ("Spaltenname in Anfuehrungszeichen", "INSERT INTO klasse ('name') VALUES ('4A');"),
        ("Tabelle in doppelten Anfuehrungszeichen", "SELECT * FROM \"schueler\";"),
        ("Eckige Klammern", "SELECT [name] FROM [schueler];"),
        ("FROM fehlt", "SELECT name, geburt WHERE klasse_id = 1;"),
        ("Leerzeichen im Namen", "CREATE TABLE lieblings fach (id INT PRIMARY KEY);"),
        ("Bindestrich im Namen", "CREATE TABLE klasse-neu (id INT PRIMARY KEY);"),
        ("Komma als Dezimalzeichen", "CREATE TABLE preis (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(6,2));\nINSERT INTO preis (betrag) VALUES ('9,50');"),
        ("Datum mit Schraegstrich", "INSERT INTO schueler (name, klasse_id, geburt) VALUES ('Grace', 1, '24/12/2008');"),
        ("Datum gibt es nicht", "INSERT INTO schueler (name, klasse_id, geburt) VALUES ('Grace', 1, '2008-02-30');"),
        ("Spalte ohne Typ", "CREATE TABLE raum (id INT PRIMARY KEY, nummer);"),
        ("ADD COLUMN ohne Typ", "ALTER TABLE schueler ADD COLUMN email;"),
        ("IN ohne Klammer", "SELECT * FROM schueler WHERE id IN 1, 2;"),
        ("BETWEEN mit Komma", "SELECT * FROM schueler WHERE id BETWEEN 1, 2;"),
        ("LEFT JOIN ohne ON", "SELECT s.name, k.name FROM schueler s LEFT JOIN klasse k WHERE s.klasse_id = k.id;"),
        ("ORDER BY vor WHERE", "SELECT * FROM schueler ORDER BY name WHERE klasse_id = 1;"),
        ("LIMIT vor ORDER BY", "SELECT * FROM schueler LIMIT 1 ORDER BY name;"),
        ("Strichpunkt zu frueh", "SELECT * FROM schueler;\nWHERE id = 1;"),
        ("SET fehlt", "UPDATE schueler name = 'Ada L' WHERE id = 1;"),
        ("Datenbank doppelt", "CREATE DATABASE korpus_doppelt;\nCREATE DATABASE korpus_doppelt;"),
        ("STRING und VARCAHR", "CREATE TABLE raum (id INT PRIMARY KEY, nummer STRING, name VARCAHR(20));"),
        ("Fremdschluessel mit anderem Typ", "CREATE TABLE note (id INT PRIMARY KEY AUTO_INCREMENT, schueler_id VARCHAR(10), FOREIGN KEY (schueler_id) REFERENCES schueler(id));"),
        ("Grossbuchstaben im Tabellennamen", "SELECT * FROM Schueler;"),
        ("Kommentar mit //", "SELECT * FROM schueler; // alle Schueler"),
        ("Kommentar ohne Leerzeichen", "--alle Schueler\nSELECT * FROM schueler;"),
        ("Unterabfrage mit mehreren Zeilen", "SELECT * FROM schueler WHERE klasse_id = (SELECT id FROM klasse);"),
        ("DISTINCT in der Mitte", "SELECT name, DISTINCT klasse_id FROM schueler;"),
        ("Zweimal WHERE", "SELECT * FROM schueler WHERE id > 0 WHERE name = 'Ada';"),
        ("Doppeltes FROM", "SELECT * FROM FROM schueler;"),
        ("SELECT in VALUES", "INSERT INTO klasse (name) VALUES (SELECT name FROM schueler WHERE id = 1);"),
        ("AUTO INCREMENT", "CREATE TABLE raum (id INT AUTO INCREMENT PRIMARY KEY);"),
        ("PRIMAY KEY", "CREATE TABLE raum (id INT PRIMAY KEY);"),
        ("NOTNULL", "CREATE TABLE raum (id INT PRIMARY KEY, nummer VARCHAR(5) NOTNULL);"),
        ("Alias in WHERE", "SELECT name AS n FROM schueler WHERE n = 'Ada';"),
        ("Summen-Alias in WHERE", "SELECT klasse_id, COUNT(*) AS anzahl FROM schueler WHERE anzahl > 0 GROUP BY klasse_id;"),
        ("LIMIT in IN-Unterabfrage", "SELECT * FROM schueler WHERE id IN (SELECT id FROM schueler ORDER BY geburt LIMIT 1);"),
        ("Text ohne Ende", "SELECT * FROM schueler WHERE name = 'Ada;"),
        ("Zeile ohne Klammer", "INSERT INTO klasse (name) VALUES ('4A'), '4B';"),
        ("Zeilen ohne Komma", "INSERT INTO klasse (name) VALUES ('4A') ('4B');"),
        ("CHANGE ohne neuen Namen", "ALTER TABLE schueler CHANGE name VARCHAR(80);"),
        ("DROP COLUMN fehlt", "ALTER TABLE schueler DROP COLUMN email;"),
        ("Reservierte Spaltennamen", "CREATE TABLE termin (id INT PRIMARY KEY, datum DATE, desc VARCHAR(50), order INT);"),
        ("DATEDIFF mit Einheit", "SELECT DATEDIFF(day, geburt, NOW()) FROM schueler;"),
        ("NOW ohne Klammern", "SELECT name, NOW FROM schueler;"),
        ("Ungleich NULL", "SELECT * FROM schueler WHERE geburt != NULL;"),
        ("=> statt >=", "SELECT * FROM schueler WHERE id => 1;"),
        ("=< statt <=", "SELECT * FROM schueler WHERE id =< 1;"),
        ("Fehlende Klammer bei Typ", "CREATE TABLE raum (id INT PRIMARY KEY, nummer VARCHAR(5, name VARCHAR(20));"),
        ("NOT NULL ohne Wert", "INSERT INTO klasse (id) VALUES (7);"),
        ("Prozent mit Gleich", "SELECT * FROM schueler WHERE name = 'A%';"),
        ("Verkettung mit ||", "SELECT name || ' (' || klasse_id || ')' FROM schueler;"),
        ("Leerer Text als Zahl", "INSERT INTO schueler (name, klasse_id) VALUES ('Grace', '');"),
        ("ja als Wahrheitswert", "CREATE TABLE kurs (id INT PRIMARY KEY, aktiv BOOLEAN);\nINSERT INTO kurs VALUES (1, 'ja');"),
        ("Tausenderpunkt", "CREATE TABLE preis (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(8,2));\nINSERT INTO preis (betrag) VALUES ('1.500,50');"),
        ("Euro-Zeichen im Betrag", "CREATE TABLE preis (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(8,2));\nINSERT INTO preis (betrag) VALUES ('9,50 €');"),
        ("Reservierte Spalte in SELECT", "CREATE TABLE termin (id INT PRIMARY KEY, `desc` VARCHAR(50));\nSELECT desc FROM termin;"),
        ("Dezimalkomma ohne Anfuehrungszeichen", "CREATE TABLE preis (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(8,2));\nINSERT INTO preis (betrag) VALUES (9,50);"),
        ("IS NUL", "SELECT * FROM schueler WHERE geburt IS NUL;"),
        ("LIKE mit Stern", "SELECT * FROM schueler WHERE name LIKE 'A*';"),
        ("Tabelle ohne Spalten", "CREATE TABLE leer;"),
        ("Spalte doppelt in INSERT", "INSERT INTO schueler (name, name) VALUES ('Grace', 'Hopper');"),
        ("Zu wenige Werte", "INSERT INTO schueler (name, klasse_id, geburt) VALUES ('Grace', 1);"),
        ("FOREIGN KEY ohne Klammern", "CREATE TABLE note (id INT PRIMARY KEY AUTO_INCREMENT, schueler_id INT, FOREIGN KEY schueler_id REFERENCES schueler(id));"),
        ("PRIMARY KEY ohne Klammern", "CREATE TABLE raum (nr INT, name VARCHAR(20), PRIMARY KEY nr);"),
        ("Verweis auf falsche Spalte", "CREATE TABLE note (id INT PRIMARY KEY AUTO_INCREMENT, schueler_id INT, FOREIGN KEY (schueler_id) REFERENCES schueler(schueler_id));"),
        ("Doppeltes Komma", "SELECT name,, klasse_id FROM schueler;"),
        ("SHOW TABLE", "SHOW TABLE;"),
        ("DATE_FORMAT wie in Java", "SELECT DATE_FORMAT(geburt, 'dd.MM.yyyy') FROM schueler;"),
        ("Aggregat verschachtelt", "SELECT MAX(COUNT(*)) FROM schueler GROUP BY klasse_id;"),
        ("Datentyp vertippt", "CREATE TABLE raum (id INTEGR PRIMARY KEY, name VARCHR(20));"),
        ("Fremde Tabelle in SELECT-Liste", "SELECT schueler.name, klasse.name FROM schueler;"),
        ("ORDER BY Spaltennummer zu gross", "SELECT name FROM schueler ORDER BY 3;"),
        ("INSERT SELECT mit VALUES-Klammer", "INSERT INTO klasse (name) VALUES ((SELECT name FROM schueler));"),
        ("Zahl zu lang fuer DECIMAL", "CREATE TABLE preis (id INT PRIMARY KEY AUTO_INCREMENT, betrag DECIMAL(4,2));\nINSERT INTO preis (betrag) VALUES (1234.5);"),
        ("Uhrzeit deutsch", "CREATE TABLE termin (id INT PRIMARY KEY AUTO_INCREMENT, beginn DATETIME);\nINSERT INTO termin (beginn) VALUES ('24.12.2026 18:00');"),
        ("Uhrzeit mit Uhr", "CREATE TABLE termin (id INT PRIMARY KEY AUTO_INCREMENT, beginn TIME);\nINSERT INTO termin (beginn) VALUES ('8 Uhr');"),
        ("CREATE INDEX ohne Klammer", "CREATE INDEX ix_name ON schueler name;"),
        ("DROP mehrere ohne Komma", "CREATE TABLE a (id INT);\nCREATE TABLE b (id INT);\nDROP TABLE a b;"),
        ("RENAME falsch", "ALTER TABLE schueler RENAME name TO vorname;"),
        ("MODIFY auf fehlende Spalte", "ALTER TABLE schueler MODIFY email VARCHAR(100);"),
        ("DEFAULT ohne Wert", "CREATE TABLE raum (id INT PRIMARY KEY, plaetze INT DEFAULT);"),
        ("VARCHAR ohne Klammer mit Zahl", "CREATE TABLE raum (id INT PRIMARY KEY, name VARCHAR 20);"),
        ("DECIMAL mit Punkt", "CREATE TABLE preis (id INT PRIMARY KEY, betrag DECIMAL(8.2));"),
        ("ENUM ohne Anfuehrungszeichen", "CREATE TABLE ampel (id INT PRIMARY KEY, farbe ENUM(rot, gelb, gruen));"),
        ("CHECK mit Text", "CREATE TABLE kurs (id INT PRIMARY KEY, stufe INT CHECK (stufe BETWEEN 1, 3));"),
        ("WHERE mit Zuweisung :=", "SELECT * FROM schueler WHERE id := 1;"),
        ("Vergleich mit !", "SELECT * FROM schueler WHERE id ! = 1;"),
        ("UPDATE mit FROM", "UPDATE schueler SET name = 'X' FROM schueler WHERE id = 1;"),
        ("DELETE mit Alias", "DELETE FROM schueler s WHERE s.id = 2;"),
        ("COUNT ohne Klammern", "SELECT COUNT * FROM schueler;"),
        ("AS mit Text-Alias in WHERE", "SELECT name AS 'Vorname' FROM schueler ORDER BY 'Vorname';"),
    ];

    #[test]
    #[ignore]
    fn autofix_corpus() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("korpus_{}", std::process::id());
        let mut failed = Vec::new();
        for (label, case) in CASES {
            let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {name}"), ());
            dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
            dbc.exec_in(&name, BASE).unwrap();
            let setup = Setup { database: Some(name.clone()), databases: dbc.databases().unwrap(), file_stem: None };
            let mut live = Live { conn: dbc.new_conn().unwrap(), db: crate::db::Db::connect(&info).unwrap() };
            let t = Instant::now();
            let only = std::env::var("KORPUS_NUR").ok();
            if only.as_deref().is_some_and(|o| !label.contains(o)) {
                continue;
            }
            let t0 = Instant::now();
            let r = repair(case, &setup, Some(&mut live), &|m| if only.is_some() { eprintln!("  {:>6} ms  {m}", t0.elapsed().as_millis()) }, &AtomicBool::new(false), None);
            // "JOIN ohne ON" ist gueltig und darf unveraendert bleiben
            let ok = r.end == End::Clean && (r.text != *case || label.starts_with("JOIN"));
            eprintln!("{} {label} ({} ms, {} Laeufe)\n    vorher: {}\n    nachher: {}\n    {:?}", if ok { "OK  " } else { "FEHL" }, t.elapsed().as_millis(), r.runs, case.replace('\n', " | "), r.text.replace('\n', " | "), r.end);
            if !ok {
                failed.push(*label);
            }
        }
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {name}"), ());
        eprintln!("\n{} von {} repariert; offen: {failed:?}", CASES.len() - failed.len(), CASES.len());
        assert!(failed.is_empty(), "{failed:?}");
    }
}

