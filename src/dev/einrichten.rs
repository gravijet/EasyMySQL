// "Alles einrichten": Datenbank, Tabellen, Spalten und Startdaten, die ein Projekt braucht, in einem Schritt.
// Quellen: SQL-Dateien und CREATE-Texte im Code (deklariert) sowie jede Abfrage im Code (benutzt), mit Typen aus
// setInt/getString, Werten und Spaltennamen. Alles ist additiv: Es wird nie geloescht, umbenannt oder geaendert.

use crate::db::{Schema, q};
use crate::sqledit::{Nutzung, breiter};
use std::collections::HashMap;
use std::path::Path;

/// Was ein Projekt an der Datenbank braucht (aus den Dateien gelesen, ohne Verbindung)
#[derive(Clone, Debug, Default)]
pub struct Projekt {
    /// Datenbank aus JDBC-Adresse, Konfiguration oder `USE` (die haeufigste)
    pub datenbank: Option<String>,
    /// SQL-Dateien und feste SQL-Texte im Code
    pub quellen: Vec<String>,
    /// Benutzte Tabellen samt Spalten (aus allen Abfragen im Code)
    pub nutzung: Vec<Nutzung>,
}

/// Ergebnis: auszufuehrendes SQL und eine Zusammenfassung
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub datenbank: String,
    pub db_neu: bool,
    pub tabellen: Vec<String>,
    /// (Tabelle, Spalte) neu in bestehenden Tabellen
    pub spalten: Vec<(String, String)>,
    pub zeilen: usize,
    /// Was nicht automatisch ging (z. B. INSERT ohne Spaltenliste)
    pub hinweise: Vec<String>,
    pub sql: String,
}

impl Plan {
    /// "Datenbank shop, 3 Tabellen (kunde, …), 2 Spalten, 12 Datensätze"
    pub fn zusammenfassung(&self) -> String {
        let mut teile = Vec::new();
        if self.db_neu {
            teile.push(crate::tr_format!("Datenbank {}", "database {}", self.datenbank));
        }
        if !self.tabellen.is_empty() {
            let namen = kurz(&self.tabellen);
            teile.push(crate::tr_format!("{} Tabelle(n) ({})", "{} table(s) ({})", self.tabellen.len(), namen));
        }
        if !self.spalten.is_empty() {
            let namen = kurz(&self.spalten.iter().map(|(t, c)| format!("{t}.{c}")).collect::<Vec<_>>());
            teile.push(crate::tr_format!("{} Spalte(n) ({})", "{} column(s) ({})", self.spalten.len(), namen));
        }
        if self.zeilen > 0 {
            teile.push(crate::tr_format!("{} Datensatz-Anweisung(en)", "{} data statement(s)", self.zeilen));
        }
        teile.join(", ")
    }
}

fn kurz(namen: &[String]) -> String {
    if namen.len() <= 4 {
        namen.join(", ")
    } else {
        format!("{}, …", namen[..3].join(", "))
    }
}

// ---------------------------------------------------------------------------
// Projekt lesen

/// Projektordner lesen: SQL-Dateien, Code (Java, Python, JavaScript, PHP) und Konfiguration
pub fn lesen(root: &Path) -> Projekt {
    let mut dateien = Vec::new();
    super::autofix::source_files(root, &mut dateien, 0);
    let mut texte: Vec<(String, String)> = Vec::new();
    for f in &dateien {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if let Ok(meta) = std::fs::metadata(f)
            && meta.len() < 2_000_000
            && let Ok(t) = std::fs::read_to_string(f)
        {
            texte.push((ext, t));
        }
    }
    let quellen = super::autofix::sql_quellen(root);
    let mut p = analysieren(&texte, &quellen);
    if p.datenbank.is_none() {
        // USE / CREATE DATABASE in den SQL-Dateien
        let mut sql_dateien = Vec::new();
        super::autofix::migrations_dateien(root, &mut sql_dateien, 0);
        let t: Vec<(String, String)> = sql_dateien.iter().filter_map(|f| std::fs::read_to_string(f).ok().map(|t| ("sql".to_string(), t))).collect();
        p.datenbank = datenbank_aus(&t);
    }
    p
}

/// Hat das Projekt ueberhaupt SQL (Abfragen im Code oder CREATE TABLE in Dateien)?
pub fn hat_bedarf(root: &Path) -> bool {
    let p = lesen(root);
    !p.nutzung.is_empty() || p.quellen.iter().any(|q| q.to_ascii_uppercase().contains("CREATE TABLE"))
}

/// Ohne Dateizugriff (testbar): `texte` = (Endung, Inhalt) der Code- und Konfigurationsdateien
pub fn analysieren(texte: &[(String, String)], quellen: &[String]) -> Projekt {
    let mut nutzung: Vec<Nutzung> = Vec::new();
    for (ext, text) in texte {
        let abfragen: Vec<(String, Option<String>)> = match ext.as_str() {
            "java" => crate::sqledit::java_abfragen(text),
            "py" | "js" | "mjs" | "cjs" | "ts" | "php" => sql_literale(text).into_iter().map(|s| (s, None)).collect(),
            _ => continue,
        };
        let getter = if ext == "java" { getter_typen(text) } else { HashMap::new() };
        let mut datei: Vec<Nutzung> = Vec::new();
        let einziges_prepare = text.matches("prepareStatement").count() == 1;
        for (sql, empfaenger) in &abfragen {
            let setter = if ext == "java" && (empfaenger.is_some() || einziges_prepare) { setter_typen(text, empfaenger.as_deref()) } else { HashMap::new() };
            for n in crate::sqledit::nutzung(sql, &|i| setter.get(&i).cloned()) {
                vereinen(&mut datei, n);
            }
        }
        // Spalten aus rs.getString("name"): Typ fuer bekannte Spalten; nutzt die Datei genau eine Tabelle,
        // gehoeren auch sonst unbekannte Namen (SELECT *) zu ihr.
        let eine = datei.len() == 1;
        for n in &mut datei {
            for (c, t) in n.spalten.iter_mut() {
                if let Some(g) = getter.get(&c.to_ascii_lowercase()) {
                    *t = breiter(t, g);
                }
            }
            if eine {
                let mut neu: Vec<(&String, &String)> = getter.iter().filter(|(c, _)| !n.spalten.iter().any(|(s, _)| s.eq_ignore_ascii_case(c))).collect();
                neu.sort();
                for (c, t) in neu {
                    n.spalten.push((c.clone(), t.clone()));
                }
            }
        }
        for n in datei {
            vereinen(&mut nutzung, n);
        }
    }
    Projekt { datenbank: datenbank_aus(texte), quellen: quellen.to_vec(), nutzung }
}

fn vereinen(ziel: &mut Vec<Nutzung>, n: Nutzung) {
    match ziel.iter_mut().find(|o| o.tabelle.eq_ignore_ascii_case(&n.tabelle) && o.datenbank == n.datenbank) {
        Some(o) => {
            for (c, t) in n.spalten {
                match o.spalten.iter_mut().find(|(x, _)| x.eq_ignore_ascii_case(&c)) {
                    Some((_, alt)) => *alt = breiter(alt, &t),
                    None => o.spalten.push((c, t)),
                }
            }
            o.positionell = o.positionell.or(n.positionell);
        }
        None => ziel.push(n),
    }
}

/// SQL-Typ zu einer JDBC-Methode (setInt / getString …)
fn jdbc_typ(methode: &str) -> Option<&'static str> {
    Some(match methode {
        "Int" | "Short" | "Byte" => "INT",
        "Long" => "BIGINT",
        "Boolean" => "BOOLEAN",
        "Double" | "Float" => "DOUBLE",
        "BigDecimal" => "DECIMAL(10,2)",
        "String" | "NString" => "VARCHAR(255)",
        "Date" => "DATE",
        "Time" => "TIME",
        "Timestamp" => "DATETIME",
        "Bytes" | "Blob" | "BinaryStream" => "LONGBLOB",
        "Clob" | "CharacterStream" => "TEXT",
        _ => return None,
    })
}

/// `ps.setInt(2, …)`: Typ je Parameter; bei `empfaenger` nur dessen Aufrufe
fn setter_typen(java: &str, empfaenger: Option<&str>) -> HashMap<usize, String> {
    let mut out: HashMap<usize, String> = HashMap::new();
    for (i, _) in java.match_indices(".set") {
        if let Some(e) = empfaenger {
            let name = java[..i].trim_end().rsplit(|c: char| !c.is_alphanumeric() && c != '_' && c != '$').next().unwrap_or("");
            if name != e {
                continue;
            }
        }
        let rest = &java[i + 4..];
        let Some((methode, args)) = rest.split_once('(') else { continue };
        let Some(typ) = jdbc_typ(methode.trim()) else { continue };
        let Ok(n) = args.split(',').next().unwrap_or("").trim().parse::<usize>() else { continue };
        let neu = out.get(&n).map(|alt| breiter(alt, typ)).unwrap_or_else(|| typ.to_string());
        out.insert(n, neu);
    }
    out
}

/// `rs.getString("name")`: Typ je Spaltenname (klein geschrieben)
fn getter_typen(java: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for (i, _) in java.match_indices(".get") {
        let rest = &java[i + 4..];
        let Some((methode, args)) = rest.split_once('(') else { continue };
        let Some(typ) = jdbc_typ(methode.trim()) else { continue };
        let arg = args.split(')').next().unwrap_or("").trim();
        let Some(name) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) else { continue };
        // "k.name" oder Ausdruecke sind keine einfachen Spaltennamen
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let key = name.to_ascii_lowercase();
        let neu = out.get(&key).map(|alt| breiter(alt, typ)).unwrap_or_else(|| typ.to_string());
        out.insert(key, neu);
    }
    out
}

/// Zeichenketten im Code, die wie SQL aussehen (Python, JavaScript, PHP). Platzhalter `%s`, `$1`, `:name` werden `?`.
fn sql_literale(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Kommentare ueberspringen (# und // bis Zeilenende)
        if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if !matches!(c, '"' | '\'' | '`') {
            i += 1;
            continue;
        }
        let dreifach = chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c);
        let start = if dreifach { i + 3 } else { i + 1 };
        let mut j = start;
        let mut s = String::new();
        while j < chars.len() {
            if chars[j] == '\\' {
                if let Some(n) = chars.get(j + 1) {
                    s.push(if *n == 'n' { '\n' } else { *n });
                }
                j += 2;
                continue;
            }
            if chars[j] == c && (!dreifach || (chars.get(j + 1) == Some(&c) && chars.get(j + 2) == Some(&c))) {
                break;
            }
            if chars[j] == '\n' && !dreifach && c != '`' {
                break;
            }
            s.push(chars[j]);
            j += 1;
        }
        i = j + if dreifach { 3 } else { 1 };
        if crate::dev::java::source::looks_like_sql(&s) {
            out.push(platzhalter(&s));
        }
    }
    out
}

fn platzhalter(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_str: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = in_str {
            out.push(c);
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if c == '\'' || c == '"' {
            in_str = Some(c);
            out.push(c);
            i += 1;
            continue;
        }
        // %s, %d, %(name)s
        if c == '%' && let Some(n) = chars.get(i + 1) {
            if matches!(n, 's' | 'd' | 'f') {
                out.push('?');
                i += 2;
                continue;
            }
            if *n == '(' && let Some(e) = chars[i..].iter().position(|x| *x == ')') && chars.get(i + e + 1) == Some(&'s') {
                out.push('?');
                i += e + 2;
                continue;
            }
        }
        // $1 (PostgreSQL-Stil) und :name (benannte Parameter), nicht ::
        if (c == '$' && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()))
            || (c == ':' && chars.get(i + 1).is_some_and(|n| n.is_alphabetic() || *n == '_') && i > 0 && chars[i - 1] != ':')
        {
            out.push('?');
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Datenbankname aus `jdbc:mysql://host:3306/shop?…`, `mysql://…/shop`, `database="shop"`, `USE shop`
pub fn datenbank_aus(texte: &[(String, String)]) -> Option<String> {
    let mut zaehler: HashMap<String, usize> = HashMap::new();
    let gueltig = |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '-');
    for (ext, text) in texte {
        let lower = text.to_ascii_lowercase();
        // Adressen
        for marke in ["mysql://", "mariadb://", "mysql+pymysql://", "mysql+mysqlconnector://", "mariadb+mariadbconnector://"] {
            for (i, _) in lower.match_indices(marke) {
                let rest = &text[i + marke.len()..];
                let ende = rest.find(['"', '\'', ' ', '\n', '\r', '`', ';', ')']).unwrap_or(rest.len());
                let adresse = &rest[..ende];
                // host[:port]/db?param oder nur host (dann keine Datenbank)
                let ohne_login = adresse.rsplit_once('@').map(|(_, h)| h).unwrap_or(adresse);
                if let Some((_, pfad)) = ohne_login.split_once('/') {
                    let db = pfad.split(['?', '/', '&']).next().unwrap_or("");
                    if gueltig(db) {
                        *zaehler.entry(db.to_string()).or_default() += 2;
                    }
                }
            }
        }
        // database="shop", database: 'shop', DB_NAME=shop, spring.datasource … (.env/.properties)
        for schluessel in ["database", "db_name", "dbname", "db_database", "mysql_database"] {
            for (i, _) in lower.match_indices(schluessel) {
                if i > 0 && lower[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric()) {
                    continue;
                }
                let rest = text[i + schluessel.len()..].trim_start_matches(['"', '\'']);
                let rest = rest.trim_start();
                let Some(rest) = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':')) else { continue };
                let rest = rest.trim_start();
                let (quote, rest) = match rest.chars().next() {
                    Some(c @ ('"' | '\'')) => (Some(c), &rest[1..]),
                    _ => (None, rest),
                };
                let ende = match quote {
                    Some(qc) => rest.find(qc),
                    None if matches!(ext.as_str(), "env" | "properties" | "ini" | "yml" | "yaml") => rest.find(['\n', '\r', ' ', '#']).or(Some(rest.len())),
                    None => None,
                };
                if let Some(e) = ende {
                    let db = rest[..e].trim();
                    if gueltig(db) {
                        *zaehler.entry(db.to_string()).or_default() += 2;
                    }
                }
            }
        }
        if ext == "sql" {
            for st in crate::db::split_statements(text) {
                let s = st.sql.trim().trim_end_matches(';').trim();
                let up = s.to_ascii_uppercase();
                let name = if up.starts_with("USE ") {
                    Some(s[4..].trim())
                } else if up.starts_with("CREATE DATABASE") || up.starts_with("CREATE SCHEMA") {
                    s.split_whitespace().find(|w| !matches!(w.to_ascii_uppercase().as_str(), "CREATE" | "DATABASE" | "SCHEMA" | "IF" | "NOT" | "EXISTS"))
                } else {
                    None
                };
                if let Some(n) = name.map(|n| n.trim_matches('`')).filter(|n| gueltig(n)) {
                    *zaehler.entry(n.to_string()).or_default() += 1;
                }
            }
        }
    }
    let systeme = ["mysql", "information_schema", "performance_schema", "sys", "test"];
    let mut liste: Vec<(String, usize)> = zaehler.into_iter().filter(|(n, _)| !systeme.contains(&n.to_ascii_lowercase().as_str())).collect();
    liste.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    liste.into_iter().next().map(|(n, _)| n)
}

// ---------------------------------------------------------------------------
// Typen und Plan

/// Wortteile eines Spaltennamens: `kundeId` / `kunde_id` -> ["kunde", "id"]
fn wortteile(spalte: &str) -> Vec<String> {
    let mut teile: Vec<String> = Vec::new();
    let mut akt = String::new();
    let mut vorher_klein = false;
    for c in spalte.chars() {
        if c == '_' || c == '-' || c == ' ' {
            if !akt.is_empty() {
                teile.push(std::mem::take(&mut akt));
            }
            vorher_klein = false;
            continue;
        }
        if c.is_uppercase() && vorher_klein && !akt.is_empty() {
            teile.push(std::mem::take(&mut akt));
        }
        vorher_klein = c.is_lowercase() || c.is_ascii_digit();
        akt.extend(c.to_lowercase());
    }
    if !akt.is_empty() {
        teile.push(akt);
    }
    teile
}

/// Typ aus dem Spaltennamen, wenn der Code nichts verraet. Kurze Woerter zaehlen nur als ganzer Wortteil
/// (`age` nicht in `message`), lange deutsche Woerter auch im Kompositum (`geburtsdatum`).
pub fn typ_aus_name(spalte: &str) -> &'static str {
    let s = spalte.to_lowercase();
    let teile = wortteile(spalte);
    let wort = |w: &[&str]| teile.iter().any(|t| w.contains(&t.as_str()));
    let teil = |w: &[&str]| w.iter().any(|x| s.contains(x));
    let letztes = teile.last().map(String::as_str).unwrap_or("");
    if letztes == "id" || teile.first().is_some_and(|t| t == "id") && teile.len() > 1 {
        "INT"
    } else if wort(&["email", "mail", "url", "tel", "phone", "token", "hash", "password", "passwort", "login", "username", "benutzername"]) || teil(&["telefon", "passwort"]) {
        "VARCHAR(255)"
    } else if wort(&["price", "amount", "total", "cost", "salary", "balance", "discount", "sum", "fee"]) || teil(&["preis", "betrag", "summe", "kosten", "gehalt", "lohn", "umsatz", "rabatt", "saldo", "kontostand", "gebuehr"]) {
        "DECIMAL(10,2)"
    } else if letztes == "at" || wort(&["timestamp", "created", "updated", "modified", "time", "zeit", "zeitpunkt"]) || teil(&["erstellt", "geaendert", "zeitpunkt", "uhrzeit"]) {
        "DATETIME"
    } else if letztes == "on" || wort(&["date", "birthday", "dob", "am"]) || teil(&["datum", "geburtstag", "geboren", "termin"]) {
        "DATE"
    } else if teile.first().is_some_and(|t| matches!(t.as_str(), "is" | "has" | "ist" | "hat" | "can" | "darf"))
        || wort(&["active", "enabled", "done", "paid", "deleted", "visible", "verified", "aktiv", "erledigt", "bezahlt", "sichtbar", "fertig", "gesperrt", "blocked"])
        || teil(&["geloescht"])
    {
        "BOOLEAN"
    } else if wort(&["count", "qty", "quantity", "age", "year", "number", "nr", "no", "zip", "plz", "position", "rank", "points", "score", "stock", "level", "duration", "grade", "note", "jahr", "alter", "rang", "punkte", "klasse", "semester", "minuten", "sekunden", "stunden", "tage", "seiten", "pages", "size", "groesse"])
        || teil(&["anzahl", "menge", "stueck", "nummer", "bestand", "lager", "dauer", "jahrgang"])
    {
        "INT"
    } else if wort(&["description", "content", "comment", "message", "text", "body", "details", "notes", "bio", "info"]) || teil(&["beschreibung", "inhalt", "kommentar", "bemerkung", "notiz", "nachricht", "text"]) {
        "TEXT"
    } else {
        "VARCHAR(255)"
    }
}

fn ist_zahl(typ: &str) -> bool {
    let u = typ.to_ascii_uppercase();
    u.is_empty() || u.starts_with("INT") || u.starts_with("BIGINT") || u.starts_with("SMALLINT")
}

/// Primaerschluessel einer abgeleiteten Tabelle: `id`, sonst `<tabelle>_id` / `<tabelle>id` / `id_<tabelle>`
fn schluessel(tabelle: &str, spalten: &[(String, String)]) -> Option<usize> {
    let t = tabelle.to_ascii_lowercase();
    let kandidaten = ["id".to_string(), format!("{t}_id"), format!("{t}id"), format!("id_{t}"), format!("{t}_nr"), format!("{t}nr")];
    kandidaten.iter().find_map(|k| spalten.iter().position(|(c, _)| c.eq_ignore_ascii_case(k)))
}

/// Spaltendefinition einer abgeleiteten Spalte
fn spalte_ddl(name: &str, hinweis: &str, pk: bool) -> String {
    let typ = if hinweis.is_empty() { typ_aus_name(name).to_string() } else { hinweis.to_string() };
    if pk {
        if ist_zahl(&typ) {
            let t = if typ.to_ascii_uppercase().starts_with("BIGINT") { "BIGINT" } else { "INT" };
            return format!("{} {t} NOT NULL AUTO_INCREMENT PRIMARY KEY", q(name));
        }
        let t = if typ.to_ascii_uppercase().starts_with("VARCHAR") { "VARCHAR(191)" } else { typ.as_str() };
        return format!("{} {t} NOT NULL PRIMARY KEY", q(name));
    }
    format!("{} {typ} NULL", q(name))
}

/// Was fehlt, als SQL. `schema`: vorhandene Datenbank (None = gibt es noch nicht).
/// `alles`: auch deklarierte Tabellen, die der Code nicht benutzt (Schaltflaeche); sonst nur Benutztes samt
/// Fremdschluessel-Zielen (automatisch vor dem Start).
pub fn plan(p: &Projekt, db: &str, schema: Option<&Schema>, alles: bool) -> Option<Plan> {
    let dekl = super::autofix::deklarationen(&p.quellen, db);
    let vorhanden = |t: &str| schema.is_some_and(|s| s.tables.iter().any(|x| x.name.eq_ignore_ascii_case(t)));
    let leer = |t: &str| schema.and_then(|s| s.tables.iter().find(|x| x.name.eq_ignore_ascii_case(t))).is_some_and(|x| !x.is_view && x.rows == Some(0));
    let nutzung: Vec<&Nutzung> = p.nutzung.iter().filter(|n| n.datenbank.as_deref().is_none_or(|d| d.eq_ignore_ascii_case(db))).collect();
    let mut hinweise = Vec::new();

    // 1. Deklarierte Tabellen: benutzte (bzw. alle) und ihre Fremdschluessel-Ziele
    let start: Vec<String> = if alles || nutzung.is_empty() && schema.is_none() {
        dekl.tabellen.iter().map(|(t, _)| t.clone()).collect()
    } else {
        dekl.tabellen.iter().filter(|(t, _)| nutzung.iter().any(|n| n.tabelle.eq_ignore_ascii_case(t))).map(|(t, _)| t.clone()).collect()
    };
    let gewaehlt = super::autofix::mit_verweisen(&dekl.tabellen, start);
    let neu_dekl: Vec<String> = super::autofix::fk_reihenfolge(&dekl.tabellen, gewaehlt.iter().filter(|t| !vorhanden(t)).cloned().collect());
    let mut sql: Vec<String> = Vec::new();
    let mut tabellen: Vec<String> = Vec::new();
    // Spalten jeder deklarierten Tabelle (fuer den Abgleich mit dem Code)
    let mut dekl_spalten: HashMap<String, Vec<String>> = HashMap::new();
    for (t, create) in &dekl.tabellen {
        if let Ok(plaene) = crate::sqledit::migrationen(create) {
            for pl in plaene {
                dekl_spalten.entry(t.to_ascii_lowercase()).or_default().extend(pl.teile.iter().filter_map(|x| x.name.clone()));
            }
        }
    }
    for t in &neu_dekl {
        if let Some((_, c)) = dekl.tabellen.iter().find(|(n, _)| n == t) {
            sql.push(c.clone());
            tabellen.push(t.clone());
        }
    }
    // 2. Benutzte, nirgends deklarierte Tabellen aus den Abfragen ableiten
    for n in &nutzung {
        let name = &n.tabelle;
        if vorhanden(name) || dekl.tabellen.iter().any(|(t, _)| t.eq_ignore_ascii_case(name)) || tabellen.iter().any(|t| t.eq_ignore_ascii_case(name)) {
            continue;
        }
        if n.spalten.is_empty() {
            hinweise.push(match n.positionell {
                Some(k) => crate::tr_format!("Tabelle „{}“: INSERT ohne Spaltennamen ({} Werte) – Spalten bitte im INSERT angeben.", "Table “{}”: INSERT without column names ({} values) – please name the columns in the INSERT.", name, k),
                None => crate::tr_format!("Tabelle „{}“: Der Code nennt keine Spalten (nur SELECT *).", "Table “{}”: the code names no columns (only SELECT *).", name),
            });
            continue;
        }
        let pk = schluessel(name, &n.spalten);
        let mut defs: Vec<String> = Vec::new();
        if let Some(i) = pk {
            defs.push(spalte_ddl(&n.spalten[i].0, &n.spalten[i].1, true));
        }
        for (i, (c, t)) in n.spalten.iter().enumerate() {
            if Some(i) != pk {
                defs.push(spalte_ddl(c, t, false));
            }
        }
        // Verweise auf andere Tabellen (kunde_id) bekommen einen Index
        for (c, _) in &n.spalten {
            let lc = c.to_ascii_lowercase();
            if let Some(ziel) = lc.strip_suffix("_id")
                && pk.is_none_or(|i| !n.spalten[i].0.eq_ignore_ascii_case(c))
                && (nutzung.iter().any(|m| m.tabelle.eq_ignore_ascii_case(ziel)) || vorhanden(ziel) || dekl.tabellen.iter().any(|(t, _)| t.eq_ignore_ascii_case(ziel)))
            {
                defs.push(format!("KEY ({})", q(c)));
            }
        }
        if n.positionell.is_some_and(|k| k != n.spalten.len()) {
            hinweise.push(crate::tr_format!("Tabelle „{}“: Ein INSERT ohne Spaltennamen passt nicht sicher zur abgeleiteten Struktur.", "Table “{}”: an INSERT without column names may not match the derived structure.", name));
        }
        sql.push(format!("CREATE TABLE IF NOT EXISTS {} (\n  {}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4", q(name), defs.join(",\n  ")));
        tabellen.push(name.clone());
    }
    // 3. Fehlende Spalten in vorhandenen und eben deklarierten Tabellen
    let mut spalten: Vec<(String, String)> = Vec::new();
    for n in &nutzung {
        let name = &n.tabelle;
        let bekannt: Vec<String> = if vorhanden(name) {
            let Some(t) = schema.and_then(|s| s.tables.iter().find(|x| x.name.eq_ignore_ascii_case(name))) else { continue };
            if t.is_view {
                continue;
            }
            t.columns.iter().map(|c| c.name.clone()).collect()
        } else if let Some(d) = dekl_spalten.get(&name.to_ascii_lowercase()).filter(|_| neu_dekl.iter().any(|t| t.eq_ignore_ascii_case(name))) {
            d.clone()
        } else {
            continue;
        };
        let fehlend: Vec<&(String, String)> = n.spalten.iter().filter(|(c, _)| !bekannt.iter().any(|b| b.eq_ignore_ascii_case(c))).collect();
        if fehlend.is_empty() {
            continue;
        }
        sql.push(format!("ALTER TABLE {ziel}\n  ADD COLUMN IF NOT EXISTS {}", fehlend.iter().map(|(c, t)| spalte_ddl(c, t, false)).collect::<Vec<_>>().join(",\n  ADD COLUMN IF NOT EXISTS "), ziel = q(name)));
        spalten.extend(fehlend.iter().map(|(c, _)| (name.clone(), c.clone())));
    }
    // 4. Indizes und Sichten der neuen Tabellen, dann Startdaten fuer neue oder leere Tabellen
    for (t, s) in &dekl.extra {
        if (t.is_empty() && schema.is_none() && alles) || neu_dekl.iter().any(|g| g.eq_ignore_ascii_case(t)) {
            sql.push(s.clone());
        }
    }
    let mut zeilen = 0;
    let mit_daten: Vec<String> = super::autofix::fk_reihenfolge(
        &dekl.tabellen,
        gewaehlt.iter().filter(|t| neu_dekl.iter().any(|g| g.eq_ignore_ascii_case(t)) || leer(t)).cloned().collect(),
    );
    for t in &mit_daten {
        for (_, s) in dekl.zeilen.iter().filter(|(n, _)| n.eq_ignore_ascii_case(t)) {
            sql.push(s.clone());
            zeilen += 1;
        }
    }
    let db_neu = schema.is_none();
    if sql.is_empty() && !(db_neu && (!nutzung.is_empty() || !dekl.tabellen.is_empty())) {
        return None;
    }
    Some(Plan {
        datenbank: db.to_string(),
        db_neu,
        tabellen,
        spalten,
        zeilen,
        hinweise,
        sql: if sql.is_empty() { String::new() } else { sql.join(";\n\n") + ";" },
    })
}

/// Plan ausfuehren: Datenbank anlegen (falls noetig), dann das SQL in ihr
pub fn ausfuehren(dbc: &crate::db::Db, plan: &Plan) -> Result<(), String> {
    dbc.exec(&format!("CREATE DATABASE IF NOT EXISTS {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", q(&plan.datenbank)), ())?;
    if !plan.sql.is_empty() {
        dbc.exec_in(&plan.datenbank, &plan.sql)?;
    }
    Ok(())
}

/// Projekt lesen, mit der Datenbank vergleichen und den Plan liefern (None = nichts zu tun).
/// `db`: Ziel aus der Fehlermeldung; sonst die Datenbank des Projekts, sonst `aktuell`.
pub fn planen(dbc: &crate::db::Db, root: &Path, db: Option<&str>, aktuell: &str, alles: bool) -> Result<Option<Plan>, String> {
    let projekt = lesen(root);
    let Some(ziel) = db.map(str::to_string).or_else(|| projekt.datenbank.clone()).or_else(|| (!aktuell.is_empty()).then(|| aktuell.to_string())) else {
        return Ok(None);
    };
    let gibt_es = dbc.databases()?.iter().any(|d| d.eq_ignore_ascii_case(&ziel));
    let schema = if gibt_es { Some(dbc.schema(&ziel)?) } else { None };
    Ok(plan(&projekt, &ziel, schema.as_ref(), alles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, TableInfo};

    fn java(t: &str) -> (String, String) {
        ("java".into(), t.into())
    }

    fn tabelle(name: &str, spalten: &[&str], rows: u64) -> TableInfo {
        TableInfo {
            name: name.into(),
            is_view: false,
            engine: "InnoDB".into(),
            rows: Some(rows),
            comment: String::new(),
            columns: spalten.iter().map(|c| ColumnInfo { name: c.to_string(), col_type: "int".into(), nullable: true, key: String::new(), default: None, extra: String::new(), comment: String::new() }).collect(),
        }
    }

    fn schema(tabellen: Vec<TableInfo>) -> Schema {
        Schema { name: "shop".into(), tables: tabellen, fks: vec![], unique_keys: vec![], index: Default::default() }
    }

    const DAO: &str = r#"
        class KundeDao {
            static final String URL = "jdbc:mariadb://localhost:3306/shop?user=root";
            List<Kunde> alle() throws SQLException {
                try (PreparedStatement ps = c.prepareStatement("SELECT id, name, email FROM kunde WHERE aktiv = ?")) {
                    ps.setBoolean(1, true);
                    ResultSet rs = ps.executeQuery();
                    while (rs.next()) { rs.getInt("id"); rs.getString("name"); rs.getString("email"); }
                }
            }
            void bestellen(int kunde, BigDecimal betrag) throws SQLException {
                PreparedStatement b = c.prepareStatement("INSERT INTO bestellung (kunde_id, betrag, datum) VALUES (?, ?, NOW())");
                b.setInt(1, kunde); b.setBigDecimal(2, betrag);
            }
        }
    "#;

    #[test]
    fn leere_datenbank_bekommt_alle_benutzten_tabellen() {
        let p = analysieren(&[java(DAO)], &[]);
        assert_eq!(p.datenbank.as_deref(), Some("shop"));
        let plan = plan(&p, "shop", None, false).unwrap();
        assert!(plan.db_neu);
        let mut t = plan.tabellen.clone();
        t.sort();
        assert_eq!(t, ["bestellung", "kunde"]);
        let s = &plan.sql;
        assert!(s.contains("`id` INT NOT NULL AUTO_INCREMENT PRIMARY KEY"), "{s}");
        assert!(s.contains("`aktiv` BOOLEAN NULL"), "{s}");
        assert!(s.contains("`email` VARCHAR(255) NULL"), "{s}");
        assert!(s.contains("`betrag` DECIMAL(10,2) NULL"), "{s}");
        assert!(s.contains("`datum` DATETIME NULL"), "{s}");
        assert!(s.contains("`kunde_id` INT NULL") && s.contains("KEY (`kunde_id`)"), "{s}");
        assert!(!s.to_uppercase().contains("DROP") && !s.contains("DELETE"));
    }

    #[test]
    fn vorhandene_tabelle_bekommt_nur_fehlende_spalten() {
        let p = analysieren(&[java(DAO)], &[]);
        let s = schema(vec![tabelle("kunde", &["id", "name"], 3), tabelle("bestellung", &["id", "kunde_id", "betrag", "datum"], 0)]);
        let plan = plan(&p, "shop", Some(&s), false).unwrap();
        assert!(!plan.db_neu);
        assert!(plan.tabellen.is_empty());
        assert_eq!(plan.spalten, [("kunde".to_string(), "email".to_string()), ("kunde".to_string(), "aktiv".to_string())]);
        assert!(plan.sql.contains("ALTER TABLE `kunde`\n  ADD COLUMN IF NOT EXISTS `email` VARCHAR(255) NULL"), "{}", plan.sql);
        // alles da: nichts zu tun
        let voll = schema(vec![tabelle("kunde", &["id", "name", "email", "aktiv"], 3), tabelle("bestellung", &["id", "kunde_id", "betrag", "datum"], 0)]);
        assert_eq!(super::plan(&p, "shop", Some(&voll), false), None);
    }

    #[test]
    fn deklarierte_tabellen_mit_daten_und_fk_reihenfolge() {
        let quellen = vec![
            "USE shop;\nCREATE TABLE bestellung (id INT PRIMARY KEY, kunde_id INT, FOREIGN KEY (kunde_id) REFERENCES kunde(id));\nCREATE TABLE kunde (id INT PRIMARY KEY, name VARCHAR(50));\nCREATE TABLE fremd_unbenutzt (x INT);\nINSERT INTO kunde VALUES (1, 'Ada');".to_string(),
        ];
        let code = java(r#"void f() { c.prepareStatement("SELECT id, kunde_id, status FROM bestellung"); }"#);
        let p = analysieren(&[code], &quellen);
        let plan = plan(&p, "shop", None, false).unwrap();
        // benutzt: bestellung, dazu das FK-Ziel kunde; die unbenutzte Tabelle nur mit "alles"
        assert_eq!(plan.tabellen, ["kunde", "bestellung"]);
        assert_eq!(plan.zeilen, 1);
        // status fehlt in der deklarierten Tabelle, der Code braucht sie
        assert_eq!(plan.spalten, [("bestellung".to_string(), "status".to_string())]);
        let pos = |x: &str| plan.sql.find(x).unwrap_or_else(|| panic!("{x} fehlt:\n{}", plan.sql));
        assert!(pos("`kunde`") < pos("`bestellung`"));
        assert!(pos("CREATE TABLE IF NOT EXISTS `bestellung`") < pos("ALTER TABLE `bestellung`"));
        assert!(pos("ALTER TABLE `bestellung`") < pos("INSERT IGNORE"));
        let alles = super::plan(&p, "shop", None, true).unwrap();
        assert!(alles.tabellen.contains(&"fremd_unbenutzt".to_string()));
        // Startdaten nur in neue oder leere Tabellen
        let s = schema(vec![tabelle("kunde", &["id", "name"], 5), tabelle("bestellung", &["id", "kunde_id", "status"], 0)]);
        assert_eq!(super::plan(&p, "shop", Some(&s), false), None);
        let s = schema(vec![tabelle("kunde", &["id", "name"], 0), tabelle("bestellung", &["id", "kunde_id", "status"], 0)]);
        let nur_daten = super::plan(&p, "shop", Some(&s), false).unwrap();
        assert_eq!((nur_daten.tabellen.len(), nur_daten.zeilen), (0, 1));
    }

    #[test]
    fn python_und_javascript_mit_platzhaltern() {
        let py = ("py".to_string(), "import mysql.connector\ndb = mysql.connector.connect(host='localhost', user='root', database='schule')\ncur.execute(\"INSERT INTO schueler (vorname, klasse, geburtsdatum) VALUES (%s, %s, %s)\", werte)\ncur.execute('''SELECT vorname FROM schueler WHERE klasse = %(k)s''', {'k': 3})\n".to_string());
        let js = ("js".to_string(), "const r = await pool.query(`SELECT titel, faellig FROM aufgabe WHERE erledigt = $1`, [false]);".to_string());
        let p = analysieren(&[py, js], &[]);
        assert_eq!(p.datenbank.as_deref(), Some("schule"));
        let plan = plan(&p, "schule", None, false).unwrap();
        let mut t = plan.tabellen.clone();
        t.sort();
        assert_eq!(t, ["aufgabe", "schueler"]);
        assert!(plan.sql.contains("`klasse` INT NULL") && plan.sql.contains("`geburtsdatum` DATE NULL"), "{}", plan.sql);
        assert!(plan.sql.contains("`erledigt` BOOLEAN NULL") && plan.sql.contains("`titel` VARCHAR(255) NULL"), "{}", plan.sql);
    }

    #[test]
    fn select_stern_mit_gettern_in_einer_datei() {
        let code = java(r#"void f() { ResultSet rs = st.executeQuery("SELECT * FROM artikel"); while (rs.next()) { rs.getString("bezeichnung"); rs.getDouble("preis"); rs.getInt("id"); } }"#);
        let p = analysieren(&[code], &[]);
        let plan = plan(&p, "laden", None, false).unwrap();
        assert!(plan.sql.contains("`id` INT NOT NULL AUTO_INCREMENT PRIMARY KEY"), "{}", plan.sql);
        assert!(plan.sql.contains("`preis` DOUBLE NULL") && plan.sql.contains("`bezeichnung` VARCHAR(255) NULL"), "{}", plan.sql);
    }

    #[test]
    fn insert_ohne_spalten_wird_erklaert() {
        let code = java(r#"void f() { st.executeUpdate("INSERT INTO protokoll VALUES (1, 'x')"); }"#);
        let p = analysieren(&[code], &[]);
        let plan = plan(&p, "app", None, false).unwrap();
        assert!(plan.tabellen.is_empty());
        assert_eq!(plan.hinweise.len(), 1);
    }

    #[test]
    fn datenbank_aus_konfiguration() {
        let env = ("env".to_string(), "DB_HOST=localhost\nDB_DATABASE=bibliothek\n".to_string());
        assert_eq!(datenbank_aus(&[env]), Some("bibliothek".into()));
        let props = ("properties".to_string(), "spring.datasource.url=jdbc:mysql://127.0.0.1:3306/kino?useSSL=false\n".to_string());
        assert_eq!(datenbank_aus(&[props]), Some("kino".into()));
        let ohne = ("java".to_string(), "String url = \"jdbc:mariadb://localhost:3306/\";".to_string());
        assert_eq!(datenbank_aus(&[ohne]), None);
    }

    #[test]
    fn typen_aus_namen() {
        assert_eq!(typ_aus_name("kundeId"), "INT");
        assert_eq!(typ_aus_name("preis"), "DECIMAL(10,2)");
        assert_eq!(typ_aus_name("created_at"), "DATETIME");
        assert_eq!(typ_aus_name("geburtsdatum"), "DATE");
        assert_eq!(typ_aus_name("ist_aktiv"), "BOOLEAN");
        assert_eq!(typ_aus_name("beschreibung"), "TEXT");
        assert_eq!(typ_aus_name("vorname"), "VARCHAR(255)");
        assert_eq!(typ_aus_name("anzahl"), "INT");
        assert_eq!(typ_aus_name("message"), "TEXT");
        assert_eq!(typ_aus_name("seiten_anzahl"), "INT");
        assert_eq!(typ_aus_name("titel"), "VARCHAR(255)");
    }

    #[test]
    #[ignore = "braucht isolierten MariaDB-Testserver auf EASYMYSQL_AUTOFIX_TEST_PORT"]
    fn e2e_alles_einrichten_und_die_abfragen_des_codes_laufen() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let dbc = crate::db::Db::connect(&crate::db::ConnInfo { port, ..Default::default() }).unwrap();
        let db = format!("einrichten_{}", std::process::id());
        let root = std::env::temp_dir().join(&db);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sql")).unwrap();
        std::fs::write(root.join("KundeDao.java"), DAO.replace("/shop?", &format!("/{db}?"))).unwrap();
        std::fs::write(root.join("Katalog.java"), r#"class Katalog { void f() throws Exception { var ps = c.prepareStatement("SELECT k.titel, p.name, p.preis FROM produkt p JOIN kategorie k ON k.id = p.kategorie_id WHERE p.preis < ?"); ps.setBigDecimal(1, x); } }"#).unwrap();
        std::fs::write(root.join("sql/01_schema.sql"), "CREATE TABLE kategorie (id INT AUTO_INCREMENT PRIMARY KEY, titel VARCHAR(80) NOT NULL);\nINSERT INTO kategorie (titel) VALUES ('Bücher'), ('Spiele');\n").unwrap();
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", q(&db)), ());
        let ergebnis = (|| -> Result<(), String> {
            let plan = planen(&dbc, &root, None, "", false)?.expect("Plan");
            assert!(plan.db_neu, "{plan:?}");
            let mut t = plan.tabellen.clone();
            t.sort();
            assert_eq!(t, ["bestellung", "kategorie", "kunde", "produkt"], "{}", plan.sql);
            assert_eq!(plan.zeilen, 1);
            ausfuehren(&dbc, &plan)?;
            // Die Abfragen aus dem Code laufen jetzt
            dbc.exec_in(&db, "INSERT INTO kunde (name, email, aktiv) VALUES ('Ada', 'ada@example.org', TRUE)")?;
            dbc.exec_in(&db, "INSERT INTO bestellung (kunde_id, betrag, datum) VALUES (1, 19.90, NOW())")?;
            dbc.exec_in(&db, "INSERT INTO produkt (name, preis, kategorie_id) VALUES ('Atlas', 12.50, 1)")?;
            dbc.exec_in(&db, "SELECT id, name, email FROM kunde WHERE aktiv = 1")?;
            dbc.exec_in(&db, "SELECT k.titel, p.name, p.preis FROM produkt p JOIN kategorie k ON k.id = p.kategorie_id WHERE p.preis < 20")?;
            let s = dbc.schema(&db)?;
            assert_eq!(s.tables.iter().find(|t| t.name == "kategorie").map(|t| t.columns.len()), Some(2));
            // Zweiter Lauf: nichts mehr zu tun
            assert_eq!(planen(&dbc, &root, None, "", false)?, None);
            // Spalte kommt im Code dazu: nur sie wird ergaenzt, Daten bleiben
            std::fs::write(root.join("Neu.java"), r#"class Neu { void f() { c.prepareStatement("UPDATE kunde SET telefon = ? WHERE id = ?"); } }"#).unwrap();
            let plan = planen(&dbc, &root, None, "", false)?.expect("Spalte");
            assert_eq!(plan.spalten, [("kunde".to_string(), "telefon".to_string())]);
            ausfuehren(&dbc, &plan)?;
            dbc.exec_in(&db, "UPDATE kunde SET telefon = '0664 1234' WHERE id = 1")?;
            let n = dbc.rows(&format!("SELECT COUNT(*) FROM {}.kunde", q(&db)), ())?;
            assert_eq!(n[0][0].as_deref(), Some("1"));
            Ok(())
        })();
        let _ = dbc.exec(&format!("DROP DATABASE IF EXISTS {}", q(&db)), ());
        let _ = std::fs::remove_dir_all(&root);
        ergebnis.unwrap();
    }
}
