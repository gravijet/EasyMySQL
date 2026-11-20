// Datenbank-Doktor: untersucht eine ganze Datenbank nur lesend und schlaegt fertige Reparaturen vor –
// fehlende Primaer- und Fremdschluessel (aus Spaltennamen wie kunde_id erkannt), verwaiste Zeilen, Datum und
// Zahlen als Text (auch 12.03.2024 und 3,50), doppelte Zeilen, Leerzeichen am Rand, alte Zeichensaetze und
// Speicher-Engines. Was Daten veraendert oder loescht, ist als solches markiert.

use crate::db::q;
use mysql::prelude::Queryable;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Art {
    Engine,
    Zeichensatz,
    Schluessel,
    Typ,
    Leerzeichen,
    Doppelt,
    Verweis,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fund {
    pub art: Art,
    pub tabelle: String,
    pub titel: String,
    pub text: String,
    /// Anweisungen der Reparatur in dieser Reihenfolge; leer = nur Hinweis
    pub sql: Vec<String>,
    /// Die Reparatur aendert oder loescht Werte (sonst nur die Struktur)
    pub daten: bool,
    /// Betroffene Spalte (Tabelle, Spalte), falls es um eine geht
    pub spalte: Option<(String, String)>,
}

/// Was der Doktor ueber eine Datenbank weiss (aus information_schema)
#[derive(Default, Debug)]
pub struct Bild {
    pub tabellen: Vec<Tab>,
    /// (Tabelle, Spalte) -> (Elterntabelle, Elternspalte)
    pub verweise: HashMap<(String, String), (String, String)>,
}

#[derive(Default, Debug, Clone)]
pub struct Tab {
    pub name: String,
    pub engine: String,
    pub collation: String,
    pub zeilen: u64,
    pub spalten: Vec<Sp>,
    pub pk: Vec<String>,
}

#[derive(Default, Debug, Clone)]
pub struct Sp {
    pub name: String,
    /// z. B. int(11) unsigned, varchar(50)
    pub typ: String,
    /// z. B. int, varchar
    pub art: String,
    pub null: bool,
    pub extra: String,
}

impl Sp {
    fn ganzzahl(&self) -> bool {
        ["tinyint", "smallint", "mediumint", "int", "bigint"].contains(&self.art.as_str())
    }
    fn text(&self) -> bool {
        ["char", "varchar", "tinytext", "text", "mediumtext"].contains(&self.art.as_str())
    }
}

/// Lesender Zugriff fuer die Pruefung (im Test durch Werte ersetzt)
pub trait Quelle {
    fn zahl(&mut self, sql: &str) -> Option<u64>;
    fn texte(&mut self, sql: &str) -> Vec<String>;
}

impl<C: Queryable> Quelle for C {
    fn zahl(&mut self, sql: &str) -> Option<u64> {
        self.query_first::<Option<u64>, _>(sql).ok().flatten().flatten()
    }
    fn texte(&mut self, sql: &str) -> Vec<String> {
        self.query::<Option<String>, _>(sql).map(|v| v.into_iter().flatten().collect()).unwrap_or_default()
    }
}

/// Aufbau der Datenbank lesen
pub fn bild<C: Queryable>(conn: &mut C, db: &str) -> Result<Bild, String> {
    let e = |e: mysql::Error| crate::db::err_text(e);
    type Zeile = (String, Option<String>, Option<String>, Option<u64>);
    type Schluessel = (String, String, String, Option<String>, Option<String>);
    let tabs: Vec<Zeile> = conn
        .exec("SELECT TABLE_NAME, ENGINE, TABLE_COLLATION, TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME", (db,))
        .map_err(e)?;
    let cols: Vec<(String, String, String, String, String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, DATA_TYPE, IS_NULLABLE, EXTRA FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (db,),
        )
        .map_err(e)?;
    let keys: Vec<Schluessel> = conn
        .exec(
            "SELECT TABLE_NAME, COLUMN_NAME, CONSTRAINT_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? ORDER BY ORDINAL_POSITION",
            (db,),
        )
        .map_err(e)?;
    let mut b = Bild::default();
    for (name, engine, coll, rows) in tabs {
        b.tabellen.push(Tab { name, engine: engine.unwrap_or_default(), collation: coll.unwrap_or_default(), zeilen: rows.unwrap_or(0), ..Default::default() });
    }
    for (t, c, typ, art, null, extra) in cols {
        if let Some(tab) = b.tabellen.iter_mut().find(|x| x.name == t) {
            tab.spalten.push(Sp { name: c, typ, art: art.to_ascii_lowercase(), null: null == "YES", extra: extra.to_ascii_lowercase() });
        }
    }
    for (t, c, cons, rt, rc) in keys {
        if cons == "PRIMARY" {
            if let Some(tab) = b.tabellen.iter_mut().find(|x| x.name == t) {
                tab.pk.push(c);
            }
        } else if let (Some(rt), Some(rc)) = (rt, rc) {
            b.verweise.insert((t, c), (rt, rc));
        }
    }
    Ok(b)
}

/// Ganze Pruefung: Aufbau lesen, dann jede Tabelle untersuchen
pub fn pruefen<C: Queryable>(conn: &mut C, db: &str, status: &dyn Fn(String)) -> Result<Vec<Fund>, String> {
    conn.query_drop(format!("USE {}", q(db))).map_err(crate::db::err_text)?;
    let b = bild(conn, db)?;
    Ok(befunde(&b, conn, status))
}

/// Groesse, bis zu der Daten (nicht nur die Struktur) durchsucht werden
const DATEN_BIS: u64 = 200_000;

/// Nur bestimmte Tabellen pruefen (z. B. die eben angelegten); Elterntabellen fuer Verweise kommen aus allen
pub fn pruefen_nur<C: Queryable>(conn: &mut C, db: &str, nur: &[String]) -> Result<Vec<Fund>, String> {
    conn.query_drop(format!("USE {}", q(db))).map_err(crate::db::err_text)?;
    let b = bild(conn, db)?;
    let nur: HashSet<String> = nur.iter().map(|t| t.to_lowercase()).collect();
    Ok(befunde_fuer(&b, conn, &|_| {}, Some(&nur)))
}

pub fn befunde(b: &Bild, z: &mut dyn Quelle, status: &dyn Fn(String)) -> Vec<Fund> {
    befunde_fuer(b, z, status, None)
}

fn befunde_fuer(b: &Bild, z: &mut dyn Quelle, status: &dyn Fn(String), nur: Option<&HashSet<String>>) -> Vec<Fund> {
    let mut out = Vec::new();
    let gewaehlt = |t: &str| nur.is_none_or(|n| n.contains(&t.to_lowercase()));
    status(crate::i18n::tr("Suche fehlende Verweise …", "Looking for missing references …").into());
    let verw: Vec<Fund> = verweise(b, z).into_iter().filter(|f| gewaehlt(&f.tabelle)).collect();
    // Spalten, deren Typ schon der Verweis anpasst
    let skip: HashSet<(String, String)> = verw.iter().filter_map(|f| f.sql.first().filter(|s| s.contains(" MODIFY ")).map(|_| f.spalte.clone())).flatten().collect();
    for (i, t) in b.tabellen.iter().enumerate() {
        if !gewaehlt(&t.name) {
            continue;
        }
        status(crate::tr_format!("Prüfe {} ({} von {}) …", "Checking {} ({} of {}) …", t.name, i + 1, b.tabellen.len()));
        out.extend(engine(t));
        out.extend(zeichensatz(t));
        out.extend(schluessel(t, z));
        let klein = t.zeilen <= DATEN_BIS;
        if klein {
            out.extend(typen(t, b, &skip, z));
            out.extend(leerzeichen(t, z));
            out.extend(doppelt(t, b, z));
        }
    }
    out.extend(verw);
    out.sort_by(|a, c| (a.art, &a.tabelle).cmp(&(c.art, &c.tabelle)));
    out
}

fn engine(t: &Tab) -> Option<Fund> {
    let e = t.engine.to_ascii_lowercase();
    if e.is_empty() || e == "innodb" {
        return None;
    }
    Some(Fund {
        art: Art::Engine,
        tabelle: t.name.clone(),
        titel: crate::tr_format!("{} auf InnoDB umstellen", "Switch {} to InnoDB", t.name),
        text: crate::tr_format!(
            "Die Speicher-Engine {} kennt keine Fremdschlüssel und keine Transaktionen und kann bei einem Absturz Daten verlieren.",
            "The {} storage engine has no foreign keys or transactions and can lose data in a crash.",
            t.engine
        ),
        sql: vec![format!("ALTER TABLE {} ENGINE = InnoDB", q(&t.name))],
        daten: false,
        spalte: None,
    })
}

fn zeichensatz(t: &Tab) -> Option<Fund> {
    let c = t.collation.to_ascii_lowercase();
    if c.is_empty() || c.starts_with("utf8mb4") || !t.spalten.iter().any(Sp::text) {
        return None;
    }
    let set = c.split('_').next().unwrap_or("").to_string();
    Some(Fund {
        art: Art::Zeichensatz,
        tabelle: t.name.clone(),
        titel: crate::tr_format!("{} auf utf8mb4 umstellen", "Convert {} to utf8mb4", t.name),
        text: crate::tr_format!("Mit dem Zeichensatz {set} gehen Emoji und viele Sonderzeichen verloren.", "The {set} character set cannot store emoji and many special characters."),
        sql: vec![format!("ALTER TABLE {} CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci", q(&t.name))],
        daten: false,
        spalte: None,
    })
}

fn schluessel(t: &Tab, z: &mut dyn Quelle) -> Option<Fund> {
    if !t.pk.is_empty() {
        return None;
    }
    let tn = t.name.to_lowercase();
    let kandidat = t.spalten.iter().find(|s| {
        let n = s.name.to_lowercase();
        s.ganzzahl() && (n == "id" || n == format!("{tn}_id") || n == format!("id_{tn}") || n == format!("{tn}id") || n == format!("{tn}_nr") || n == format!("{tn}nr"))
    });
    let titel = crate::tr_format!("{} bekommt einen Primärschlüssel", "Give {} a primary key", t.name);
    if let Some(s) = kandidat {
        let tq = q(&t.name);
        let c = q(&s.name);
        let alle = z.zahl(&format!("SELECT COUNT(*) FROM {tq}"))?;
        let eindeutig = z.zahl(&format!("SELECT COUNT(DISTINCT {c}) FROM {tq}"))?;
        let leer = z.zahl(&format!("SELECT COUNT(*) FROM {tq} WHERE {c} IS NULL"))?;
        if alle == eindeutig && leer == 0 {
            let auto = if s.extra.contains("auto_increment") { "" } else { " AUTO_INCREMENT" };
            return Some(Fund {
                art: Art::Schluessel,
                tabelle: t.name.clone(),
                titel,
                text: crate::tr_format!(
                    "Die Tabelle hat keinen Primärschlüssel; „{}“ ist schon eindeutig und wird zum Schlüssel.",
                    "The table has no primary key; “{}” is already unique and becomes the key.",
                    s.name
                ),
                sql: vec![format!("ALTER TABLE {tq} MODIFY {c} {} NOT NULL{auto}, ADD PRIMARY KEY ({c})", s.typ)],
                daten: false,
                spalte: None,
            });
        }
        if s.name.eq_ignore_ascii_case("id") {
            return Some(Fund {
                art: Art::Schluessel,
                tabelle: t.name.clone(),
                titel,
                text: crate::tr_format!(
                    "Die Tabelle hat keinen Primärschlüssel, und „id“ ist nicht eindeutig ({} Zeilen, {} verschiedene Werte).",
                    "The table has no primary key, and “id” is not unique ({} rows, {} distinct values).",
                    alle,
                    eindeutig
                ),
                sql: Vec::new(),
                daten: false,
                spalte: None,
            });
        }
    }
    let name = if t.spalten.iter().any(|s| s.name.eq_ignore_ascii_case("id")) { format!("{}_id", t.name) } else { "id".into() };
    Some(Fund {
        art: Art::Schluessel,
        tabelle: t.name.clone(),
        titel,
        text: crate::tr_format!(
            "Ohne Primärschlüssel lassen sich einzelne Zeilen nicht sicher ändern oder löschen. Eine neue Spalte „{name}“ nummeriert alle Zeilen durch.",
            "Without a primary key single rows cannot be changed or deleted safely. A new column “{name}” numbers all rows."
        ),
        sql: vec![format!("ALTER TABLE {} ADD COLUMN {} INT NOT NULL AUTO_INCREMENT PRIMARY KEY FIRST", q(&t.name), q(&name))],
        daten: false,
        spalte: None,
    })
}

/// Erkannter Inhalt einer Textspalte
#[derive(Debug, PartialEq)]
enum Inhalt {
    Datum,
    DatumDeutsch,
    Zeitpunkt,
    Ganz { gross: bool },
    Dezimal { vor: usize, nach: usize, komma: bool },
}

fn ziffern(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn iso_datum(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3 && p[0].len() == 4 && p[1].len() == 2 && p[2].len() == 2 && p.iter().all(|x| ziffern(x)) && {
        let (m, d) = (p[1].parse::<u32>().unwrap_or(0), p[2].parse::<u32>().unwrap_or(0));
        (1..=12).contains(&m) && (1..=31).contains(&d)
    }
}

fn deutsches_datum(s: &str) -> bool {
    let p: Vec<&str> = s.split('.').collect();
    p.len() == 3 && (1..=2).contains(&p[0].len()) && (1..=2).contains(&p[1].len()) && p[2].len() == 4 && p.iter().all(|x| ziffern(x)) && {
        let (d, m) = (p[0].parse::<u32>().unwrap_or(0), p[1].parse::<u32>().unwrap_or(0));
        (1..=12).contains(&m) && (1..=31).contains(&d)
    }
}

fn zeitpunkt(s: &str) -> bool {
    let Some((d, t)) = s.split_once([' ', 'T']) else { return false };
    let tp: Vec<&str> = t.split(':').collect();
    iso_datum(d) && (2..=3).contains(&tp.len()) && tp.iter().all(|x| x.len() == 2 && ziffern(x))
}

/// Woran man Kennungen erkennt, die wie Zahlen aussehen, aber Text bleiben muessen
fn kennung(spalte: &str) -> bool {
    let n = spalte.to_lowercase();
    ["tel", "phone", "handy", "mobil", "fax", "plz", "zip", "postleit", "iban", "bic", "svnr", "isbn", "ean", "gtin", "nummer", "code", "kennzeichen", "pin"].iter().any(|w| n.contains(w))
}

fn inhalt(spalte: &str, werte: &[String]) -> Option<Inhalt> {
    let w: Vec<&str> = werte.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if w.len() < 2 {
        return None;
    }
    if w.iter().all(|s| iso_datum(s)) {
        return Some(Inhalt::Datum);
    }
    if w.iter().all(|s| deutsches_datum(s)) {
        return Some(Inhalt::DatumDeutsch);
    }
    if w.iter().all(|s| zeitpunkt(s) || iso_datum(s)) {
        return Some(Inhalt::Zeitpunkt);
    }
    if kennung(spalte) {
        return None;
    }
    // Fuehrende Nullen (0664 …, 0042) sind Kennungen, keine Zahlen
    let ganz = |s: &str| {
        let d = s.strip_prefix('-').unwrap_or(s);
        ziffern(d) && (d == "0" || !d.starts_with('0')) && d.len() <= 18
    };
    if w.iter().all(|s| ganz(s)) {
        let gross = w.iter().any(|s| s.parse::<i64>().map_or(true, |v| v > i32::MAX as i64 || v < i32::MIN as i64));
        return Some(Inhalt::Ganz { gross });
    }
    let dez = |s: &str| {
        let d = s.strip_prefix('-').unwrap_or(s);
        match d.split_once(['.', ',']) {
            Some((a, b)) => ziffern(a) && ziffern(b) && (a == "0" || !a.starts_with('0')) && a.len() <= 20 && b.len() <= 10,
            None => ganz(s),
        }
    };
    if w.iter().all(|s| dez(s)) && w.iter().any(|s| s.contains(['.', ','])) {
        let punkt = w.iter().any(|s| s.contains('.'));
        let komma = w.iter().any(|s| s.contains(','));
        if punkt && komma {
            return None;
        }
        let vor = w.iter().map(|s| s.trim_start_matches('-').split(['.', ',']).next().unwrap_or("").len()).max().unwrap_or(1);
        let nach = w.iter().map(|s| s.split_once(['.', ',']).map_or(0, |(_, b)| b.len())).max().unwrap_or(0);
        return Some(Inhalt::Dezimal { vor, nach, komma });
    }
    None
}

fn typen(t: &Tab, b: &Bild, skip: &HashSet<(String, String)>, z: &mut dyn Quelle) -> Vec<Fund> {
    let mut out = Vec::new();
    let tq = q(&t.name);
    for s in t.spalten.iter().filter(|s| s.text() && !t.pk.contains(&s.name) && !b.verweise.contains_key(&(t.name.clone(), s.name.clone())) && !skip.contains(&(t.name.clone(), s.name.clone()))) {
        let c = q(&s.name);
        let werte = z.texte(&format!("SELECT DISTINCT {c} FROM {tq} WHERE {c} IS NOT NULL LIMIT 3000"));
        let Some(inh) = inhalt(&s.name, &werte) else { continue };
        // Die Stichprobe muss fuer alle Zeilen gelten
        let muster = match &inh {
            Inhalt::Datum => "^[0-9]{4}-[0-9]{2}-[0-9]{2}$",
            Inhalt::DatumDeutsch => "^[0-9]{1,2}[.][0-9]{1,2}[.][0-9]{4}$",
            Inhalt::Zeitpunkt => "^[0-9]{4}-[0-9]{2}-[0-9]{2}([ T][0-9]{2}:[0-9]{2}(:[0-9]{2})?)?$",
            Inhalt::Ganz { .. } => "^-?[0-9]+$",
            Inhalt::Dezimal { .. } => "^-?[0-9]+([.,][0-9]+)?$",
        };
        if z.zahl(&format!("SELECT COUNT(*) FROM {tq} WHERE {c} IS NOT NULL AND TRIM({c}) <> '' AND TRIM({c}) NOT REGEXP '{muster}'")) != Some(0) {
            continue;
        }
        let leer_da = werte.iter().any(|w| w.trim().is_empty());
        if leer_da && !s.null {
            continue;
        }
        let null = if s.null { "NULL" } else { "NOT NULL" };
        let mut sql = Vec::new();
        if leer_da {
            sql.push(format!("UPDATE {tq} SET {c} = NULL WHERE TRIM({c}) = ''"));
        }
        let (neu, was) = match &inh {
            Inhalt::Datum => ("DATE".to_string(), crate::i18n::tr("Datumswerte", "dates")),
            Inhalt::DatumDeutsch => {
                sql.push(format!("UPDATE {tq} SET {c} = DATE_FORMAT(STR_TO_DATE({c}, '%d.%m.%Y'), '%Y-%m-%d') WHERE {c} IS NOT NULL"));
                ("DATE".to_string(), crate::i18n::tr("Datumswerte im Format TT.MM.JJJJ", "dates in DD.MM.YYYY format"))
            }
            Inhalt::Zeitpunkt => ("DATETIME".to_string(), crate::i18n::tr("Zeitpunkte", "timestamps")),
            Inhalt::Ganz { gross } => ((if *gross { "BIGINT" } else { "INT" }).to_string(), crate::i18n::tr("ganze Zahlen", "whole numbers")),
            Inhalt::Dezimal { vor, nach, komma } => {
                if *komma {
                    sql.push(format!("UPDATE {tq} SET {c} = REPLACE({c}, ',', '.') WHERE {c} LIKE '%,%'"));
                }
                (format!("DECIMAL({},{})", (vor + nach + 2).min(65), nach), crate::i18n::tr("Kommazahlen", "decimal numbers"))
            }
        };
        sql.push(format!("ALTER TABLE {tq} MODIFY {c} {neu} {null}"));
        let daten = sql.len() > 1;
        out.push(Fund {
            art: Art::Typ,
            tabelle: t.name.clone(),
            titel: crate::tr_format!("{}.{} als {neu} speichern", "Store {}.{} as {neu}", t.name, s.name),
            text: crate::tr_format!(
                "Die Spalte ist Text ({}), enthält aber nur {was}. Sortieren, Vergleichen und Rechnen klappen erst mit dem richtigen Typ.",
                "The column is text ({}) but only holds {was}. Sorting, comparing and calculating only work with the right type.",
                s.typ
            ),
            sql,
            daten,
            spalte: Some((t.name.clone(), s.name.clone())),
        });
    }
    out
}

fn leerzeichen(t: &Tab, z: &mut dyn Quelle) -> Option<Fund> {
    let tq = q(&t.name);
    let mut spalten = Vec::new();
    for s in t.spalten.iter().filter(|s| s.art == "varchar" || s.art == "char") {
        let c = q(&s.name);
        if z.zahl(&format!("SELECT COUNT(*) FROM {tq} WHERE {c} <> '' AND CHAR_LENGTH({c}) <> CHAR_LENGTH(TRIM({c}))")).unwrap_or(0) > 0 {
            spalten.push(s.name.clone());
        }
    }
    if spalten.is_empty() {
        return None;
    }
    let sets: Vec<String> = spalten.iter().map(|s| format!("{0} = TRIM({0})", q(s))).collect();
    Some(Fund {
        art: Art::Leerzeichen,
        tabelle: t.name.clone(),
        titel: crate::tr_format!("Leerzeichen am Rand entfernen ({})", "Remove surrounding spaces ({})", t.name),
        text: crate::tr_format!(
            "In {} stehen Werte mit Leerzeichen am Anfang oder Ende; Vergleiche wie = 'Wien' finden sie sonst nicht zuverlässig.",
            "{} holds values with leading or trailing spaces; comparisons like = 'Wien' miss them.",
            spalten.join(", ")
        ),
        sql: vec![format!("UPDATE {tq} SET {}", sets.join(", "))],
        daten: true,
        spalte: None,
    })
}

fn doppelt(t: &Tab, b: &Bild, z: &mut dyn Quelle) -> Option<Fund> {
    if t.pk.len() != 1 {
        return None;
    }
    let pk = &t.pk[0];
    let rest: Vec<&Sp> = t.spalten.iter().filter(|s| s.name != *pk && !s.extra.contains("generated") && !["blob", "longblob", "longtext", "json"].contains(&s.art.as_str())).collect();
    // Zu wenig Inhalt (nur ein Name …): gleiche Werte koennen gewollt sein
    if rest.len() < 2 || rest.iter().all(|s| b.verweise.contains_key(&(t.name.clone(), s.name.clone()))) && rest.len() < 2 {
        return None;
    }
    let tq = q(&t.name);
    let cols: Vec<String> = rest.iter().map(|s| q(&s.name)).collect();
    let n = z.zahl(&format!("SELECT COALESCE(SUM(n - 1), 0) FROM (SELECT COUNT(*) AS n FROM {tq} GROUP BY {} HAVING COUNT(*) > 1) x", cols.join(", ")))?;
    if n == 0 {
        return None;
    }
    let gleich: Vec<String> = cols.iter().map(|c| format!("a.{c} <=> b.{c}")).collect();
    Some(Fund {
        art: Art::Doppelt,
        tabelle: t.name.clone(),
        titel: crate::tr_format!("{n} doppelte Zeile(n) in {} löschen", "Delete {n} duplicate row(s) in {}", t.name),
        text: crate::i18n::tr("Diese Zeilen gleichen einer anderen in allen Werten außer dem Schlüssel. Die jeweils erste bleibt.", "These rows equal another one in every value except the key. The first of each stays.").into(),
        sql: vec![format!("DELETE a FROM {tq} a JOIN {tq} b ON {} AND a.{p} > b.{p}", gleich.join(" AND "), p = q(pk))],
        daten: true,
        spalte: None,
    })
}

/// Moegliche Namen der Elterntabelle zu einem Wortstamm (kunde -> kunde, kunden; category -> categories)
fn tabellen_fuer(stamm: &str) -> Vec<String> {
    let s = stamm.to_lowercase();
    let mut v = vec![s.clone(), format!("{s}s"), format!("{s}e"), format!("{s}n"), format!("{s}en"), format!("{s}er")];
    if let Some(r) = s.strip_suffix('y') {
        v.push(format!("{r}ies"));
    }
    if let Some(r) = s.strip_suffix('e') {
        v.push(r.to_string());
    }
    v
}

/// Wortstamm einer Verweisspalte: kunde_id, kundeid, id_kunde, kunde_nr, kundennr -> kunde(n)
fn stamm(spalte: &str) -> Option<String> {
    let n = spalte.to_lowercase();
    for (pre, suf) in [("", "_id"), ("id_", ""), ("", "_nr"), ("", "id"), ("", "nr"), ("fk_", ""), ("", "_fk")] {
        if let Some(r) = n.strip_prefix(pre).and_then(|r| r.strip_suffix(suf))
            && r.chars().count() >= 2
            && r != n
        {
            return Some(r.trim_end_matches('_').to_string());
        }
    }
    None
}

fn kurz(s: &str) -> String {
    s.chars().take(64).collect()
}

fn verweise(b: &Bild, z: &mut dyn Quelle) -> Vec<Fund> {
    let mut out = Vec::new();
    let pk_von: HashMap<String, &Tab> = b.tabellen.iter().filter(|t| t.pk.len() == 1).map(|t| (t.name.to_lowercase(), t)).collect();
    let mut gesehen = HashSet::new();
    for t in &b.tabellen {
        for s in &t.spalten {
            if b.verweise.contains_key(&(t.name.clone(), s.name.clone())) || (t.pk.len() == 1 && t.pk[0] == s.name) {
                continue;
            }
            // Elterntabelle: ueber den Namensstamm oder weil die Spalte wie deren Schluessel heisst
            let mut eltern: Option<&Tab> = stamm(&s.name).and_then(|st| tabellen_fuer(&st).into_iter().find_map(|n| pk_von.get(&n).copied()));
            if eltern.is_none() && !s.name.eq_ignore_ascii_case("id") {
                eltern = b.tabellen.iter().find(|p| p.name != t.name && p.pk.len() == 1 && p.pk[0].eq_ignore_ascii_case(&s.name));
            }
            let Some(p) = eltern else { continue };
            if p.name == t.name {
                continue;
            }
            let Some(ps) = p.spalten.iter().find(|x| x.name == p.pk[0]) else { continue };
            // Nur sinnvolle Paare: Zahl zu Zahl oder Text zu Text (Text mit lauter Zahlen wird angepasst)
            let tq = q(&t.name);
            let (c, pq, pc) = (q(&s.name), q(&p.name), q(&ps.name));
            let mut sql = Vec::new();
            if s.typ.to_lowercase() != ps.typ.to_lowercase() {
                if ps.ganzzahl() && s.text() {
                    let nicht_zahl = z.zahl(&format!("SELECT COUNT(*) FROM {tq} WHERE {c} IS NOT NULL AND {c} NOT REGEXP '^-?[0-9]+$'")).unwrap_or(1);
                    if nicht_zahl > 0 {
                        continue;
                    }
                } else if ps.ganzzahl() != s.ganzzahl() || ps.text() != s.text() {
                    continue;
                }
                sql.push(format!("ALTER TABLE {tq} MODIFY {c} {} {}", ps.typ, if s.null { "NULL" } else { "NOT NULL" }));
            }
            if !gesehen.insert((t.name.clone(), s.name.clone())) {
                continue;
            }
            let waisen = z.zahl(&format!("SELECT COUNT(*) FROM {tq} k LEFT JOIN {pq} e ON k.{c} = e.{pc} WHERE k.{c} IS NOT NULL AND e.{pc} IS NULL")).unwrap_or(0);
            let mut daten = false;
            let mut text = crate::tr_format!(
                "„{}.{}“ verweist offenbar auf „{}.{}“, aber die Datenbank kennt den Verweis nicht. Mit Fremdschlüssel kann keine Zeile mehr ins Leere zeigen, und ER-Diagramm und Auto-Fix sehen die Verbindung.",
                "“{}.{}” apparently refers to “{}.{}”, but the database does not know it. With a foreign key no row can point nowhere, and the ER diagram and auto-fix see the link.",
                t.name, s.name, p.name, ps.name
            );
            if waisen > 0 {
                daten = true;
                if s.null {
                    sql.push(format!("UPDATE {tq} k LEFT JOIN {pq} e ON k.{c} = e.{pc} SET k.{c} = NULL WHERE k.{c} IS NOT NULL AND e.{pc} IS NULL"));
                    text.push_str(&crate::tr_format!(" {waisen} Zeile(n) zeigen auf nicht vorhandene Einträge; ihr Verweis wird geleert.", " {waisen} row(s) point to missing entries; their reference is cleared."));
                } else {
                    sql.push(format!("DELETE k FROM {tq} k LEFT JOIN {pq} e ON k.{c} = e.{pc} WHERE e.{pc} IS NULL"));
                    text.push_str(&crate::tr_format!(" {waisen} Zeile(n) zeigen auf nicht vorhandene Einträge und werden gelöscht.", " {waisen} row(s) point to missing entries and are deleted."));
                }
            }
            sql.push(format!("ALTER TABLE {tq} ADD CONSTRAINT {} FOREIGN KEY ({c}) REFERENCES {pq} ({pc})", q(&kurz(&format!("fk_{}_{}", t.name, s.name)))));
            out.push(Fund {
                art: Art::Verweis,
                tabelle: t.name.clone(),
                titel: crate::tr_format!("Verweis {}.{} → {}", "Reference {}.{} → {}", t.name, s.name, p.name),
                text,
                sql,
                daten,
                spalte: Some((t.name.clone(), s.name.clone())),
            });
        }
    }
    out
}

/// Ausgewaehlte Reparaturen ausfuehren (vorher gesichert); Ergebnis je Fund
pub fn reparieren<C: Queryable>(conn: &mut C, db: &str, funde: &[Fund], status: &dyn Fn(String)) -> Vec<Result<(), String>> {
    if let Err(e) = conn.query_drop(format!("USE {}", q(db))) {
        let e = crate::db::err_text(e);
        return funde.iter().map(|_| Err(e.clone())).collect();
    }
    let mut out = Vec::new();
    for (i, f) in funde.iter().enumerate() {
        status(crate::tr_format!("Repariere {} von {}: {} …", "Repairing {} of {}: {} …", i + 1, funde.len(), f.titel));
        let mut r = Ok(());
        for s in &f.sql {
            if let Err(e) = conn.query_drop(s) {
                r = Err(crate::db::err_text(e));
                break;
            }
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inhalte_werden_erkannt() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(inhalt("geburt", &v(&["2001-03-04", "1999-12-31"])), Some(Inhalt::Datum));
        assert_eq!(inhalt("geburt", &v(&["4.3.2001", "31.12.1999"])), Some(Inhalt::DatumDeutsch));
        assert_eq!(inhalt("menge", &v(&["3", "12", "-4"])), Some(Inhalt::Ganz { gross: false }));
        assert_eq!(inhalt("preis", &v(&["3,50", "12"])), Some(Inhalt::Dezimal { vor: 2, nach: 2, komma: true }));
        assert_eq!(inhalt("telefon", &v(&["06641234", "01234"])), None);
        assert_eq!(inhalt("kennung", &v(&["0042", "0043"])), None);
        assert_eq!(inhalt("name", &v(&["Anna", "3"])), None);
        assert_eq!(inhalt("x", &v(&["1"])), None);
    }

    #[test]
    fn verweisspalten_finden_ihre_tabelle() {
        assert_eq!(stamm("kunde_id").as_deref(), Some("kunde"));
        assert_eq!(stamm("KundeID").as_deref(), Some("kunde"));
        assert_eq!(stamm("id_klasse").as_deref(), Some("klasse"));
        assert_eq!(stamm("id"), None);
        assert!(tabellen_fuer("category").contains(&"categories".to_string()));
        assert!(tabellen_fuer("kunde").contains(&"kunden".to_string()));
    }

    struct Fake(HashMap<String, u64>, Vec<String>);
    impl Quelle for Fake {
        fn zahl(&mut self, sql: &str) -> Option<u64> {
            Some(*self.0.get(sql).unwrap_or(&0))
        }
        fn texte(&mut self, _sql: &str) -> Vec<String> {
            self.1.clone()
        }
    }

    fn sp(name: &str, typ: &str) -> Sp {
        Sp { name: name.into(), typ: typ.into(), art: typ.split('(').next().unwrap_or(typ).into(), null: true, extra: String::new() }
    }

    #[test]
    fn fehlender_verweis_mit_waisen() {
        let kunde = Tab { name: "kunde".into(), engine: "InnoDB".into(), spalten: vec![sp("id", "int(11)"), sp("name", "varchar(50)")], pk: vec!["id".into()], ..Default::default() };
        let best = Tab { name: "bestellung".into(), engine: "InnoDB".into(), spalten: vec![sp("id", "int(11)"), sp("kunde_id", "int(11)")], pk: vec!["id".into()], ..Default::default() };
        let b = Bild { tabellen: vec![kunde, best], verweise: HashMap::new() };
        let mut z = Fake(HashMap::from([("SELECT COUNT(*) FROM `bestellung` k LEFT JOIN `kunde` e ON k.`kunde_id` = e.`id` WHERE k.`kunde_id` IS NOT NULL AND e.`id` IS NULL".to_string(), 2)]), Vec::new());
        let f = verweise(&b, &mut z);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].daten);
        assert!(f[0].sql[0].starts_with("UPDATE `bestellung` k LEFT JOIN `kunde` e"), "{:?}", f[0].sql);
        assert_eq!(f[0].sql[1], "ALTER TABLE `bestellung` ADD CONSTRAINT `fk_bestellung_kunde_id` FOREIGN KEY (`kunde_id`) REFERENCES `kunde` (`id`)");
    }

    #[test]
    fn tabelle_ohne_schluessel() {
        let t = Tab { name: "notiz".into(), engine: "InnoDB".into(), spalten: vec![sp("text", "varchar(50)")], ..Default::default() };
        let f = schluessel(&t, &mut Fake(HashMap::new(), Vec::new())).unwrap();
        assert_eq!(f.sql[0], "ALTER TABLE `notiz` ADD COLUMN `id` INT NOT NULL AUTO_INCREMENT PRIMARY KEY FIRST");
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;

    /// `EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test doktor_echt -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn doktor_echt() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("doktor_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(
            &name,
            "CREATE TABLE kunde (id INT PRIMARY KEY, name VARCHAR(50), ort VARCHAR(30), geburt VARCHAR(20)) ENGINE=MyISAM CHARSET=latin1;
             INSERT INTO kunde VALUES (1, 'Anna', 'Wien ', '04.03.2001'), (2, 'Ben', 'Graz', '31.12.1999'), (3, 'Clara', ' Linz', '01.01.2000');
             CREATE TABLE bestellung (nr INT, kunde_id VARCHAR(10), preis VARCHAR(10), menge VARCHAR(5));
             INSERT INTO bestellung VALUES (1, '1', '3,50', '2'), (2, '3', '12,00', '1'), (3, '9', '1,20', '5'), (4, '9', '1,20', '5');",
        )
        .unwrap();
        let mut conn = dbc.new_conn().unwrap();
        let funde = pruefen(&mut conn, &name, &|_| {}).unwrap();
        for f in &funde {
            eprintln!("{:?} {} | {}\n    {:?}", f.art, f.titel, f.text, f.sql);
        }
        let ergebnis = reparieren(&mut conn, &name, &funde, &|_| {});
        for (f, r) in funde.iter().zip(&ergebnis) {
            eprintln!("{} -> {r:?}", f.titel);
        }
        let danach = pruefen(&mut conn, &name, &|_| {}).unwrap();
        for f in &danach {
            eprintln!("DANACH {:?} {}", f.art, f.titel);
        }
        let _ = dbc.exec(&format!("DROP DATABASE {name}"), ());
        assert!(ergebnis.iter().all(|r| r.is_ok()), "{ergebnis:?}");
        assert!(danach.iter().all(|f| f.sql.is_empty()), "{danach:?}");
    }
}
