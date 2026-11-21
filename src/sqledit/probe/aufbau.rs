// Fehlendes aus dem Skript ableiten und ergaenzen: Anweisungen in die richtige Reihenfolge bringen, fehlende
// Datenbanken, Tabellen (samt Daten) und Spalten anlegen, zu kleine Spalten vergroessern. Alles ist additiv;
// erst der naechste Probelauf entscheidet, ob eine Ergaenzung weiterhilft.

use super::super::dialect::{group_end, insert_parts, items};
use super::super::lint::{Kind, Tok, known_word, lex, nearest, quote_if_needed};
use super::{Cand, Fail, Map, SANDBOX_PREFIX, Server, Setup, created_tables, filled_tables, first_line, head, insert_before, is_system, mentioned_tables, quoted, str_lit};
use crate::db::{self, Schema};
use crate::sqledit::errfix;
use std::collections::HashMap;

fn lexed(sql: &str) -> (Vec<char>, Vec<Tok>) {
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    (chars, toks)
}

fn cand(text: String, label: String, check: bool, snippet: &str) -> Cand {
    Cand { text, label, check, moved: 0, snippet: first_line(snippet) }
}

/// Name ohne Datenbank: "shop.kunde" -> "kunde"
fn kurz(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

// ---------------------------------------------------------------------------
// Reihenfolge

/// (Art, Ziel, SQL) einer spaeteren Anweisung -> liefert sie das Fehlende?
type Liefert = Box<dyn Fn(&str, Option<&str>, &str) -> bool>;

/// Was der Anweisung fehlt und welche spaetere Anweisung es liefert
fn liefert(f: &Fail) -> Option<Liefert> {
    let code = errfix::code(&f.error)?;
    let (art, _) = head(&f.stmt.sql);
    let e = &f.error;
    Some(match code {
        1146 => {
            let t = kurz(&quoted(e, "Table '")?).to_ascii_lowercase();
            Box::new(move |k, z, _| (k == "CREATE TABLE" || k == "CREATE VIEW") && z == Some(t.as_str()))
        }
        // Fremdschluessel auf eine Tabelle, die erst spaeter angelegt wird
        1005 | 150 | 1215 | 1824 if art == "CREATE TABLE" => {
            let fehlt: Vec<String> = mentioned_tables(&f.stmt.sql).into_iter().filter(|t| f.schema.as_ref().is_none_or(|s| s.table(t).is_none())).collect();
            if fehlt.is_empty() {
                return None;
            }
            Box::new(move |k, z, _| k == "CREATE TABLE" && z.is_some_and(|z| fehlt.iter().any(|t| t == z)))
        }
        1452 => {
            let eltern = quoted(e, "REFERENCES `")?.to_ascii_lowercase();
            Box::new(move |k, z, _| (k == "INSERT" || k == "REPLACE") && z == Some(eltern.as_str()))
        }
        // Elterntabelle loeschen, bevor die Kindtabelle weg ist: deren DROP/DELETE zuerst
        1451 if art == "DROP TABLE" || art == "DELETE" => {
            let kind = kind_tabelle(e)?;
            let art = art.clone();
            Box::new(move |k, z, _| k == art && z == Some(kind.as_str()))
        }
        1049 => {
            let d = quoted(e, "Unknown database '")?.to_ascii_lowercase();
            Box::new(move |k, z, _| (k == "CREATE DATABASE" || k == "CREATE SCHEMA") && z == Some(d.as_str()))
        }
        1054 => {
            let c = kurz(&quoted(e, "Unknown column '")?).to_ascii_lowercase();
            Box::new(move |k, _, sql| k == "ALTER TABLE" && lexed(sql).1.iter().any(|t| t.is_name() && t.text.eq_ignore_ascii_case(&c)))
        }
        _ => return None,
    })
}

/// Kindtabelle aus "… a foreign key constraint fails (`db`.`kind`, CONSTRAINT …"
fn kind_tabelle(e: &str) -> Option<String> {
    let r = &e[e.find("fails (")? + 7..];
    let teil = r.split(',').next()?;
    Some(kurz(&teil.replace('`', "")).to_ascii_lowercase())
}

/// Spaetere Anweisungen, die liefern, was fehlt (Tabelle, Elternzeilen, Datenbank, Spalte), vor die Stelle ziehen
pub(super) fn vorziehen(text: &str, f: &Fail) -> Option<Cand> {
    let passt = liefert(f)?;
    let stmts = db::split_statements(text);
    let idx = stmts.iter().position(|s| s.start == f.stmt.start)?;
    let spaeter: Vec<&db::Stmt> = stmts[idx + 1..]
        .iter()
        .filter(|s| {
            let (k, z) = head(&s.sql);
            passt(&k, z.as_deref(), &s.sql)
        })
        .collect();
    if spaeter.is_empty() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = chars.clone();
    // Von hinten entfernen (samt Strichpunkt und Zeilenende), damit die Positionen davor gueltig bleiben
    for s in spaeter.iter().rev() {
        let mut e = s.end;
        if chars.get(e) == Some(&';') {
            e += 1;
        }
        while chars.get(e).is_some_and(|c| *c == ' ' || *c == '\t') {
            e += 1;
        }
        if chars.get(e) == Some(&'\r') {
            e += 1;
        }
        if chars.get(e) == Some(&'\n') {
            e += 1;
        }
        out.drain(s.start..e.min(out.len()));
    }
    let block: String = spaeter.iter().map(|s| format!("{};\n", s.sql.trim_end_matches(';'))).collect();
    let neu = insert_before(&out.into_iter().collect::<String>(), &f.stmt, &block);
    let erste = first_line(&spaeter[0].sql);
    let label = if spaeter.len() == 1 {
        crate::tr_format!("„{erste}“ nach vorne gezogen – es wird schon hier gebraucht", "Moved “{erste}” up – it is needed here already")
    } else {
        crate::tr_format!("{} Anweisungen nach vorne gezogen (ab „{erste}“) – sie werden schon hier gebraucht", "Moved {} statements up (from “{erste}”) – they are needed here already", spaeter.len())
    };
    Some(Cand { text: neu, label, check: false, moved: spaeter.len(), snippet: erste })
}

// ---------------------------------------------------------------------------
// Datenbank

/// Unbekannte Datenbank: aehnlicher Name, sonst anlegen
pub(super) fn datenbank(text: &str, f: &Fail, setup: &Setup) -> Vec<Cand> {
    let mut out = Vec::new();
    let Some(name) = quoted(&f.error, "Unknown database '") else { return out };
    if name.starts_with(SANDBOX_PREFIX) || is_system(&name) {
        return out;
    }
    let echte: Vec<&str> = setup.databases.iter().map(String::as_str).filter(|d| !is_system(d) && !d.starts_with(SANDBOX_PREFIX)).collect();
    if let Some(b) = echte.iter().find(|d| d.eq_ignore_ascii_case(&name)).copied().or_else(|| nearest(&name, echte.iter().copied())) {
        let (chars, toks) = lexed(text);
        let edits: Vec<(usize, usize)> = toks.iter().filter(|t| t.is_name() && t.text == name).map(|t| (t.start, t.end)).collect();
        if !edits.is_empty() {
            let mut neu = chars;
            for (a, e) in edits.into_iter().rev() {
                neu.splice(a..e, quote_if_needed(b).chars());
            }
            out.push(cand(neu.into_iter().collect(), crate::tr_format!("Datenbank „{b}“ statt „{name}“", "Database “{b}” instead of “{name}”"), false, &format!("USE {b}")));
        }
    }
    let sql = format!("CREATE DATABASE IF NOT EXISTS {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;\n", quote_if_needed(&name));
    out.push(cand(insert_before(text, &f.stmt, &sql), crate::tr_format!("Datenbank „{name}“ angelegt", "Created the database “{name}”"), false, &sql));
    out
}

/// Keine Datenbank gewaehlt (1046): gibt es alle benutzten Tabellen in genau einer Datenbank, dort arbeiten
pub(super) fn datenbank_waehlen(text: &str, f: &Fail, server: &mut dyn Server) -> Option<Cand> {
    let angelegt: Vec<String> = created_tables(text).iter().map(|t| t.to_ascii_lowercase()).collect();
    let mut namen: Vec<String> = crate::sqledit::nutzung(text, &|_| None)
        .into_iter()
        .filter(|n| n.datenbank.is_none())
        .map(|n| n.tabelle.to_ascii_lowercase())
        .filter(|t| !angelegt.contains(t))
        .collect();
    namen.dedup();
    if namen.is_empty() || namen.len() > 30 {
        return None;
    }
    let liste: Vec<String> = namen.iter().map(|n| str_lit(n)).collect();
    let rows = server
        .rows(&format!(
            "SELECT TABLE_SCHEMA FROM information_schema.TABLES WHERE LOWER(TABLE_NAME) IN ({}) GROUP BY TABLE_SCHEMA HAVING COUNT(DISTINCT LOWER(TABLE_NAME)) = {}",
            liste.join(", "),
            namen.len()
        ))
        .ok()?;
    let treffer: Vec<String> = rows.into_iter().filter_map(|r| r.into_iter().next().flatten()).filter(|d| !is_system(d) && !d.starts_with(SANDBOX_PREFIX)).collect();
    let [d] = treffer.as_slice() else { return None };
    let schreibt = db::split_statements(text).iter().any(|s| {
        let k = head(&s.sql).0;
        !(k == "SELECT" || k == "WITH" || k == "SHOW" || k == "SET")
    });
    let sql = format!("USE {};\n", quote_if_needed(d));
    Some(cand(
        insert_before(text, &f.stmt, &sql),
        crate::tr_format!("In der Datenbank „{d}“ arbeiten – dort gibt es alle benutzten Tabellen", "Work in the database “{d}” – all tables used are there"),
        schreibt,
        &sql,
    ))
}

// ---------------------------------------------------------------------------
// Werte im Skript

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Wert {
    Text(String),
    Zahl(String),
    Wahr(bool),
    Null,
}

impl Wert {
    fn sql(&self) -> String {
        match self {
            Wert::Text(s) => str_lit(s),
            Wert::Zahl(n) => n.clone(),
            Wert::Wahr(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Wert::Null => "NULL".into(),
        }
    }
}

/// Literal aus den Tokens `a..b` (Text, Zahl, TRUE/FALSE, NULL); Ausdruecke ergeben None
fn wert(chars: &[char], toks: &[Tok], a: usize, b: usize) -> Option<Wert> {
    let t = toks.get(a..b)?;
    let roh = |x: &Tok| chars[x.start..x.end].iter().collect::<String>();
    match t {
        [x] if x.kind == Kind::Str => {
            let s: String = chars[x.start + 1..x.end.saturating_sub(1).max(x.start + 1)].iter().collect();
            Some(Wert::Text(s.replace("''", "'").replace("\\'", "'")))
        }
        [x] if x.kind == Kind::Num => Some(Wert::Zahl(roh(x))),
        [m, x] if m.kind == Kind::Op && m.text == "-" && x.kind == Kind::Num => Some(Wert::Zahl(format!("-{}", roh(x)))),
        [x] if x.is_word("NULL") => Some(Wert::Null),
        [x] if x.is_word("TRUE") => Some(Wert::Wahr(true)),
        [x] if x.is_word("FALSE") => Some(Wert::Wahr(false)),
        _ => None,
    }
}

/// Werte, die das Skript in Spalten von `tabelle` schreibt (INSERT, UPDATE … SET) oder mit `spalten`
/// vergleicht (=, IN, BETWEEN …). Schluessel: Spalte klein geschrieben.
pub(super) fn werte(text: &str, tabelle: &str, spalten: &[String]) -> HashMap<String, Vec<Wert>> {
    let mut out: HashMap<String, Vec<Wert>> = HashMap::new();
    let mut add = |c: &str, w: Wert| {
        let v = out.entry(c.to_ascii_lowercase()).or_default();
        if !v.contains(&w) && v.len() < 200 {
            v.push(w);
        }
    };
    let bekannt = |c: &str| spalten.iter().any(|s| s.eq_ignore_ascii_case(c));
    for st in db::split_statements(text) {
        let (chars, toks) = lexed(&st.sql);
        if !toks.iter().any(|t| t.is_name() && t.text.eq_ignore_ascii_case(tabelle)) {
            continue;
        }
        let (art, ziel) = head(&st.sql);
        let eigene = ziel.as_deref().is_some_and(|z| z.eq_ignore_ascii_case(tabelle));
        // INSERT INTO tabelle (a, b) VALUES (…), (…)
        if eigene
            && (art == "INSERT" || art == "REPLACE")
            && let Some((ti, vi)) = insert_parts(&toks)
            && toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open)
            && let Some(close) = group_end(&toks, ti + 1)
        {
            let namen: Vec<String> = items(&toks, ti + 1, close).iter().filter_map(|(a, _)| toks.get(*a).filter(|t| t.is_name()).map(|t| t.text.clone())).collect();
            let mut k = vi + 1;
            while let Some(t) = toks.get(k) {
                if t.kind != Kind::Open {
                    k += 1;
                    continue;
                }
                let Some(end) = group_end(&toks, k) else { break };
                for (i, (a, b)) in items(&toks, k, end).into_iter().enumerate() {
                    if let (Some(n), Some(w)) = (namen.get(i), wert(&chars, &toks, a, b)) {
                        add(n, w);
                    }
                }
                k = end + 1;
            }
        }
        // Vergleiche und Zuweisungen: [x.]spalte = Wert, spalte IN (Werte), spalte BETWEEN a AND b
        for k in 0..toks.len() {
            let t = &toks[k];
            if !t.is_name() || (k + 1 < toks.len() && toks[k + 1].kind == Kind::Dot) {
                continue;
            }
            let eigen = bekannt(&t.text) || (eigene && art == "UPDATE");
            if !eigen || known_word(&t.upper()) && t.kind == Kind::Word && !bekannt(&t.text) {
                continue;
            }
            let Some(op) = toks.get(k + 1) else { continue };
            if op.kind == Kind::Op && ["=", "<>", "!=", "<", ">", "<=", ">="].contains(&op.text.as_str()) {
                let b = if toks.get(k + 2).is_some_and(|m| m.kind == Kind::Op && m.text == "-") { k + 4 } else { k + 3 };
                if let Some(w) = wert(&chars, &toks, k + 2, b.min(toks.len())) {
                    add(&t.text, w);
                }
            } else if (op.is_word("IN") && toks.get(k + 2).is_some_and(|o| o.kind == Kind::Open))
                && let Some(end) = group_end(&toks, k + 2)
            {
                for (a, b) in items(&toks, k + 2, end) {
                    if let Some(w) = wert(&chars, &toks, a, b) {
                        add(&t.text, w);
                    }
                }
            } else if op.is_word("BETWEEN") {
                if let Some(w) = wert(&chars, &toks, k + 2, k + 3) {
                    add(&t.text, w);
                }
                if toks.get(k + 3).is_some_and(|a| a.is_word("AND"))
                    && let Some(w) = wert(&chars, &toks, k + 4, k + 5)
                {
                    add(&t.text, w);
                }
            }
        }
    }
    out
}

fn ist_datum(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 10 && b[4] == b'-' && b[7] == b'-' && s.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn ist_zeitpunkt(s: &str) -> bool {
    s.len() >= 16 && ist_datum(&s[..10]) && matches!(s.as_bytes()[10], b' ' | b'T') && ist_uhrzeit(&s[11..])
}

fn ist_uhrzeit(s: &str) -> bool {
    let teile: Vec<&str> = s.split('.').next().unwrap_or("").split(':').collect();
    (2..=3).contains(&teile.len()) && teile.iter().all(|t| t.len() == 2 && t.chars().all(|c| c.is_ascii_digit()))
}

/// Spaltentyp aus Name, Hinweis (aus der Verwendung) und den Werten, die das Skript hineinschreibt
pub(super) fn typ(name: &str, hinweis: &str, ws: &[Wert]) -> String {
    let aus_name = crate::dev::einrichten::typ_aus_name(name);
    let echte: Vec<&Wert> = ws.iter().filter(|w| **w != Wert::Null).collect();
    if echte.is_empty() {
        return if hinweis.is_empty() || hinweis == "VARCHAR(255)" && aus_name != "VARCHAR(255)" { aus_name.to_string() } else { hinweis.to_string() };
    }
    let texte: Vec<&str> = echte.iter().filter_map(|w| if let Wert::Text(s) = w { Some(s.as_str()) } else { None }).collect();
    let zahlen: Vec<&str> = echte.iter().filter_map(|w| if let Wert::Zahl(s) = w { Some(s.as_str()) } else { None }).collect();
    if echte.iter().all(|w| matches!(w, Wert::Wahr(_))) {
        return "BOOLEAN".into();
    }
    if texte.len() == echte.len() {
        if texte.iter().all(|s| ist_datum(s)) {
            return "DATE".into();
        }
        if texte.iter().all(|s| ist_datum(s) || ist_zeitpunkt(s)) {
            return "DATETIME".into();
        }
        if texte.iter().all(|s| ist_uhrzeit(s)) {
            return "TIME".into();
        }
        if aus_name == "TEXT" {
            return "TEXT".into();
        }
        let laenge = texte.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        return match laenge {
            0..=30 => "VARCHAR(50)",
            31..=70 => "VARCHAR(100)",
            71..=200 => "VARCHAR(255)",
            201..=900 => "VARCHAR(1000)",
            _ => "TEXT",
        }
        .into();
    }
    if zahlen.len() == echte.len() {
        if zahlen.iter().all(|z| z.parse::<i64>().is_ok()) {
            let max = zahlen.iter().filter_map(|z| z.parse::<i64>().ok()).map(|z| z.unsigned_abs()).max().unwrap_or(0);
            if aus_name == "BOOLEAN" && max <= 1 {
                return "BOOLEAN".into();
            }
            if aus_name == "DECIMAL(10,2)" {
                return "DECIMAL(10,2)".into();
            }
            return if max > i32::MAX as u64 { "BIGINT" } else { "INT" }.into();
        }
        // Kommazahlen: so viele Stellen wie noetig, mindestens DECIMAL(10,2)
        let mut vor = 1;
        let mut nach = 2;
        for z in &zahlen {
            let z = z.trim_start_matches('-');
            if z.contains(['e', 'E']) {
                return "DOUBLE".into();
            }
            let (a, b) = z.split_once('.').unwrap_or((z, ""));
            vor = vor.max(a.len());
            nach = nach.max(b.len());
        }
        let nach = nach.min(10);
        return format!("DECIMAL({},{nach})", (vor + nach).clamp(10, 38));
    }
    "VARCHAR(255)".into()
}

// ---------------------------------------------------------------------------
// Fehlende Tabelle

/// Abgeleitete Tabelle
#[derive(Debug)]
pub(super) struct Entwurf {
    pub name: String,
    /// (Spalte, Typ)
    pub spalten: Vec<(String, String)>,
    pub pk: Option<usize>,
    /// Spalten mit Index (Verweise wie kunde_id)
    pub keys: Vec<String>,
    /// Spaltennamen geraten (INSERT ohne Spaltenliste)
    pub geraten: bool,
}

/// Typ einer Spalte in einem CREATE TABLE (Tokens), z. B. "INT" oder "VARCHAR(20)"
fn typ_in_create(chars: &[char], toks: &[Tok], spalte: &str) -> Option<String> {
    let open = toks.iter().position(|t| t.kind == Kind::Open)?;
    let close = group_end(toks, open)?;
    for (a, b) in items(toks, open, close) {
        if toks[a].is_name() && toks[a].text.eq_ignore_ascii_case(spalte) && a + 1 < b {
            let mut e = a + 2;
            if toks.get(e).is_some_and(|t| t.kind == Kind::Open) {
                e = group_end(toks, e)? + 1;
            }
            while e < b && (toks[e].is_word("UNSIGNED") || toks[e].is_word("ZEROFILL")) {
                e += 1;
            }
            return Some(chars[toks[a + 1].start..toks[e - 1].end].iter().collect::<String>().to_ascii_uppercase());
        }
    }
    None
}

/// Fremdschluessel anderer CREATE TABLE im Skript auf `tabelle`: (Zielspalte, Typ der verweisenden Spalte)
fn verweise_auf(text: &str, tabelle: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for st in db::split_statements(text) {
        let (chars, toks) = lexed(&st.sql);
        if !toks.first().is_some_and(|t| t.is_word("CREATE")) {
            continue;
        }
        for k in 0..toks.len() {
            if !toks[k].is_word("REFERENCES") || !toks.get(k + 1).is_some_and(|t| t.is_name() && t.text.eq_ignore_ascii_case(tabelle)) {
                continue;
            }
            let ziel = if toks.get(k + 2).is_some_and(|t| t.kind == Kind::Open) { toks.get(k + 3).filter(|t| t.is_name()).map(|t| t.text.clone()) } else { None };
            // FOREIGN KEY (spalte) REFERENCES … oder spalte TYP … REFERENCES …
            let quelle = if k >= 2 && toks[k - 1].kind == Kind::Close && toks[k - 2].is_name() {
                Some(toks[k - 2].text.clone())
            } else {
                let mut j = k;
                while j > 0 && toks[j - 1].kind != Kind::Comma && toks[j - 1].kind != Kind::Open {
                    j -= 1;
                }
                toks.get(j).filter(|t| t.is_name()).map(|t| t.text.clone())
            };
            let typ = quelle.and_then(|q| typ_in_create(&chars, &toks, &q)).unwrap_or_else(|| "INT".into());
            let ziel = ziel.unwrap_or_else(|| "id".into());
            if !out.iter().any(|(z, _)| z.eq_ignore_ascii_case(&ziel)) {
                out.push((ziel, typ));
            }
        }
    }
    out
}

/// Struktur der fehlenden Tabelle aus allem, was das Skript mit ihr macht
pub(super) fn entwurf(text: &str, tabelle: &str, fail_sql: &str) -> Option<Entwurf> {
    let mut spalten: Vec<(String, String)> = Vec::new();
    let mut positionell = None;
    for n in crate::sqledit::nutzung(text, &|_| None).into_iter().filter(|n| n.tabelle.eq_ignore_ascii_case(tabelle)) {
        for (c, t) in n.spalten {
            match spalten.iter_mut().find(|(x, _)| x.eq_ignore_ascii_case(&c)) {
                Some((_, alt)) => *alt = crate::sqledit::breiter(alt, &t),
                None => spalten.push((c, t)),
            }
        }
        positionell = positionell.or(n.positionell);
    }
    let verweise = verweise_auf(text, tabelle);
    let mut pk = None;
    for (ziel, typ) in &verweise {
        match spalten.iter().position(|(c, _)| c.eq_ignore_ascii_case(ziel)) {
            Some(i) => spalten[i].1 = typ.clone(),
            None => spalten.insert(0, (ziel.clone(), typ.clone())),
        }
    }
    if let Some((ziel, _)) = verweise.first() {
        pk = spalten.iter().position(|(c, _)| c.eq_ignore_ascii_case(ziel));
    }
    let mut geraten = false;
    // INSERT ohne Spaltenliste: Spalten nach den Werten benennen
    if spalten.is_empty() {
        let (chars, toks) = lexed(fail_sql);
        let (ti, vi) = insert_parts(&toks)?;
        if !toks[ti].text.eq_ignore_ascii_case(tabelle) {
            return None;
        }
        let open = (vi + 1..toks.len()).find(|&k| toks[k].kind == Kind::Open)?;
        let close = group_end(&toks, open)?;
        let ws: Vec<Option<Wert>> = items(&toks, open, close).into_iter().map(|(a, b)| wert(&chars, &toks, a, b)).collect();
        if ws.is_empty() {
            return None;
        }
        for (i, w) in ws.iter().enumerate() {
            let erste_id = i == 0 && ws.len() > 1 && matches!(w, Some(Wert::Zahl(z)) if z.parse::<i64>().is_ok());
            let name = if erste_id { "id".to_string() } else { format!("spalte_{}", i + 1) };
            spalten.push((name.clone(), typ(&name, "", w.as_slice())));
        }
        pk = (spalten[0].0 == "id").then_some(0);
        geraten = true;
        positionell = Some(ws.len());
    }
    if spalten.is_empty() {
        return None;
    }
    // Typen aus den Werten, die das Skript schreibt oder vergleicht
    if !geraten {
        let namen: Vec<String> = spalten.iter().map(|(c, _)| c.clone()).collect();
        let ws = werte(text, tabelle, &namen);
        for (c, t) in spalten.iter_mut() {
            if verweise.iter().any(|(z, _)| z.eq_ignore_ascii_case(c)) {
                continue;
            }
            *t = typ(c, t, ws.get(&c.to_ascii_lowercase()).map_or(&[], Vec::as_slice));
        }
    }
    if pk.is_none() {
        pk = crate::dev::einrichten::schluessel(tabelle, &spalten);
    }
    // Ohne erkennbaren Schluessel bekommt die Tabelle eine eigene ID (nicht bei INSERT ohne Spaltenliste)
    if pk.is_none() && positionell.is_none() {
        spalten.insert(0, ("id".into(), "INT".into()));
        pk = Some(0);
    }
    let keys = spalten
        .iter()
        .enumerate()
        .filter(|(i, (c, _))| Some(*i) != pk && (c.to_ascii_lowercase().ends_with("_id") || c.to_ascii_lowercase().starts_with("id_")))
        .map(|(_, (c, _))| c.clone())
        .collect();
    Some(Entwurf { name: tabelle.to_string(), spalten, pk, keys, geraten })
}

impl Entwurf {
    pub fn ddl(&self, qual: Option<&str>) -> String {
        let mut defs: Vec<String> = Vec::new();
        if let Some(i) = self.pk {
            defs.push(crate::dev::einrichten::spalte_ddl(&self.spalten[i].0, &self.spalten[i].1, true));
        }
        for (i, (c, t)) in self.spalten.iter().enumerate() {
            if Some(i) != self.pk {
                defs.push(crate::dev::einrichten::spalte_ddl(c, t, false));
            }
        }
        for k in &self.keys {
            defs.push(format!("KEY ({})", quote_if_needed(k)));
        }
        let name = match qual {
            Some(d) => format!("{}.{}", quote_if_needed(d), quote_if_needed(&self.name)),
            None => quote_if_needed(&self.name),
        };
        // Backticks aus spalte_ddl durch die Schreibweise des Editors ersetzen (nur wo noetig)
        let defs: Vec<String> = defs.into_iter().map(|d| lesbar(&d)).collect();
        format!("CREATE TABLE IF NOT EXISTS {name} (\n  {}\n)", defs.join(",\n  "))
    }

    /// Wird die Schluesselspalte automatisch gezaehlt?
    fn auto_pk(&self) -> Option<usize> {
        let i = self.pk?;
        let t = self.spalten[i].1.to_ascii_uppercase();
        (t.is_empty() || t.starts_with("INT") || t.starts_with("BIGINT") || t.starts_with("SMALLINT")).then_some(i)
    }
}

/// `name` in Backticks nur, wenn noetig
fn lesbar(def: &str) -> String {
    let (chars, toks) = lexed(def);
    let mut out = chars.clone();
    for t in toks.iter().rev().filter(|t| t.kind == Kind::Quoted) {
        let q = quote_if_needed(&t.text);
        out.splice(t.start..t.end, q.chars());
    }
    out.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Beispieldaten

const VORNAMEN: [&str; 12] = ["Anna", "Ben", "Clara", "David", "Emma", "Felix", "Hannah", "Jonas", "Lena", "Lukas", "Marie", "Noah"];
const NACHNAMEN: [&str; 12] = ["Huber", "Gruber", "Wagner", "Bauer", "Pichler", "Steiner", "Moser", "Mayer", "Hofer", "Leitner", "Berger", "Fuchs"];
const ORTE: [&str; 12] = ["Wien", "Graz", "Linz", "Salzburg", "Innsbruck", "Klagenfurt", "Villach", "Wels", "St. Pölten", "Dornbirn", "Steyr", "Bregenz"];
const PLZ: [&str; 12] = ["1010", "8010", "4020", "5020", "6020", "9020", "9500", "4600", "3100", "6850", "4400", "6900"];
const STRASSEN: [&str; 12] = ["Hauptstraße", "Bahnhofstraße", "Schulgasse", "Kirchenplatz", "Gartenweg", "Lindenallee", "Mühlweg", "Ringstraße", "Bergstraße", "Feldgasse", "Seestraße", "Wiesenweg"];
const PRODUKTE: [&str; 12] = ["Laptop", "Maus", "Tastatur", "Monitor", "Drucker", "USB-Stick", "Headset", "Webcam", "Router", "Tablet", "Ladekabel", "Lautsprecher"];
const PREISE: [&str; 12] = ["899.00", "19.90", "49.95", "229.00", "149.00", "12.50", "59.90", "39.00", "79.00", "349.00", "9.99", "89.00"];
const KATEGORIEN: [&str; 6] = ["Elektronik", "Büro", "Zubehör", "Software", "Haushalt", "Sport"];
const FAECHER: [&str; 12] = ["Mathematik", "Deutsch", "Englisch", "Informatik", "Physik", "Geschichte", "Biologie", "Chemie", "Geografie", "Musik", "Sport", "Kunst"];
const ABTEILUNGEN: [&str; 6] = ["Vertrieb", "Einkauf", "IT", "Buchhaltung", "Personal", "Marketing"];
const LAENDER: [&str; 6] = ["Österreich", "Deutschland", "Schweiz", "Italien", "Ungarn", "Tschechien"];
const KLASSEN: [&str; 8] = ["1A", "1B", "2A", "2B", "3A", "3B", "4A", "4B"];
const BUECHER: [&str; 8] = ["Die Verwandlung", "Faust", "Der Process", "Die Physiker", "Momo", "Der Vorleser", "Tschick", "Emil und die Detektive"];

/// Wortteile eines Namens: `kundeId` / `kunde_id` -> ["kunde", "id"]
fn teile(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut akt = String::new();
    let mut klein = false;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            if !akt.is_empty() {
                out.push(std::mem::take(&mut akt));
            }
            klein = false;
            continue;
        }
        if c.is_uppercase() && klein && !akt.is_empty() {
            out.push(std::mem::take(&mut akt));
        }
        klein = c.is_lowercase() || c.is_ascii_digit();
        akt.extend(c.to_lowercase());
    }
    if !akt.is_empty() {
        out.push(akt);
    }
    out
}

fn ascii(s: &str) -> String {
    s.to_lowercase().replace('ä', "ae").replace('ö', "oe").replace('ü', "ue").replace('ß', "ss").chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Laenge aus VARCHAR(n)/CHAR(n)
fn laenge(typ: &str) -> Option<usize> {
    let t = typ.to_ascii_uppercase();
    if !(t.starts_with("VARCHAR") || t.starts_with("CHAR")) {
        return None;
    }
    t.split(['(', ')']).nth(1)?.trim().parse().ok()
}

/// Ein passender, erfundener Wert (als SQL) fuer Zeile `i`
pub(super) fn beispiel(spalte: &str, typ: &str, tabelle: &str, i: usize) -> String {
    let w = teile(spalte);
    let s = spalte.to_lowercase();
    let hat = |xs: &[&str]| w.iter().any(|t| xs.contains(&t.as_str()));
    let enthaelt = |xs: &[&str]| xs.iter().any(|x| s.contains(x));
    let t = typ.to_ascii_uppercase();
    let tab = tabelle.to_lowercase();
    let zahl = t.starts_with("INT") || t.starts_with("BIGINT") || t.starts_with("SMALLINT") || t.starts_with("TINYINT") || t.starts_with("MEDIUMINT");
    let komma = t.starts_with("DECIMAL") || t.starts_with("NUMERIC") || t.starts_with("FLOAT") || t.starts_with("DOUBLE");
    let k = i % 12;
    let text = |v: String| {
        let v = match laenge(&t) {
            Some(n) => v.chars().take(n).collect(),
            None => v,
        };
        str_lit(&v)
    };
    if t.starts_with("BOOL") || t.starts_with("TINYINT(1)") || t == "BIT" || t == "BIT(1)" {
        return if i % 3 == 2 { "FALSE" } else { "TRUE" }.into();
    }
    if t.starts_with("DATETIME") || t.starts_with("TIMESTAMP") {
        return format!("'2025-{:02}-{:02} {:02}:{:02}:00'", k % 12 + 1, (i * 5) % 27 + 1, 8 + i % 10, (i * 15) % 60);
    }
    if t.starts_with("DATE") {
        let jahr = if enthaelt(&["geburt", "birth"]) || hat(&["dob"]) { 2000 + i % 9 } else { 2025 };
        return format!("'{jahr}-{:02}-{:02}'", (i * 5) % 12 + 1, (i * 7) % 27 + 1);
    }
    if t.starts_with("TIME") {
        return format!("'{:02}:{:02}:00'", 8 + i % 10, (i * 15) % 60);
    }
    if t.starts_with("YEAR") {
        return (2018 + i % 8).to_string();
    }
    if komma {
        if enthaelt(&["preis", "price", "kosten", "cost", "betrag", "amount", "summe", "total", "umsatz"]) {
            return PREISE[k].into();
        }
        if enthaelt(&["gehalt", "lohn", "salary"]) {
            return format!("{}.00", 2400 + i * 350);
        }
        return format!("{}.{:02}", (i + 1) * 12, (i * 25) % 100);
    }
    if zahl {
        if hat(&["alter", "age"]) {
            return (17 + (i * 7) % 50).to_string();
        }
        if hat(&["jahr", "year", "jahrgang", "baujahr"]) {
            return (2015 + i % 10).to_string();
        }
        if hat(&["note", "grade"]) {
            return (i % 5 + 1).to_string();
        }
        if hat(&["plz", "zip", "postleitzahl"]) {
            return PLZ[k].into();
        }
        if enthaelt(&["menge", "anzahl", "stueck", "stück", "qty", "quantity", "count", "bestand", "lager", "stock"]) {
            return ((i * 7) % 20 + 1).to_string();
        }
        if enthaelt(&["punkte", "points", "score"]) {
            return (40 + (i * 13) % 60).to_string();
        }
        if enthaelt(&["preis", "price", "betrag", "gehalt"]) {
            return PREISE[k].split('.').next().unwrap_or("10").to_string();
        }
        return (i + 1).to_string();
    }
    let vor = VORNAMEN[k];
    let nach = NACHNAMEN[(k * 5 + 3) % 12];
    let wert = if hat(&["vorname", "firstname", "first"]) || s == "first_name" {
        vor.to_string()
    } else if hat(&["nachname", "familienname", "lastname", "last", "surname"]) {
        nach.to_string()
    } else if enthaelt(&["mail"]) {
        format!("{}.{}@example.com", ascii(vor), ascii(nach))
    } else if enthaelt(&["telefon", "phone", "handy", "mobil"]) || hat(&["tel"]) {
        format!("+43 660 {:07}", 1_234_567 + i * 104_729)
    } else if hat(&["plz", "zip", "postleitzahl"]) {
        PLZ[k].into()
    } else if enthaelt(&["strasse", "straße", "street", "adresse", "address", "anschrift"]) {
        format!("{} {}", STRASSEN[k], i * 3 + 1)
    } else if hat(&["ort", "stadt", "city", "wohnort", "town", "standort"]) {
        ORTE[k].into()
    } else if hat(&["land", "country", "staat"]) {
        LAENDER[i % 6].into()
    } else if enthaelt(&["passwort", "password", "hash"]) {
        format!("geheim{}", i + 1)
    } else if enthaelt(&["benutzer", "username", "login"]) {
        format!("{}{}", ascii(vor), i + 1)
    } else if hat(&["url", "website", "homepage", "link"]) {
        format!("https://example.com/{}/{}", ascii(&tab), i + 1)
    } else if enthaelt(&["kategorie", "category"]) {
        KATEGORIEN[i % 6].into()
    } else if hat(&["fach", "subject", "gegenstand"]) {
        FAECHER[k].into()
    } else if enthaelt(&["abteilung", "department"]) {
        ABTEILUNGEN[i % 6].into()
    } else if hat(&["klasse", "class"]) {
        KLASSEN[i % 8].into()
    } else if hat(&["status"]) {
        ["offen", "bezahlt", "versendet", "erledigt"][i % 4].into()
    } else if hat(&["geschlecht", "gender"]) {
        ["w", "m"][i % 2].into()
    } else if t == "TEXT" || enthaelt(&["beschreibung", "description", "kommentar", "comment", "notiz", "bemerkung", "inhalt", "nachricht", "message", "text"]) {
        format!("Beispieltext {} für {}", i + 1, tabelle)
    } else if hat(&["name", "bezeichnung", "titel", "title", "label"]) {
        let person = ["kunde", "customer", "schueler", "schüler", "student", "lehrer", "teacher", "person", "mitarbeiter", "employee", "user", "benutzer", "mitglied", "member", "autor", "author", "patient", "spieler", "player", "kontakt", "contact", "fahrer", "arzt", "trainer"];
        if person.iter().any(|p| tab.contains(p)) {
            format!("{vor} {nach}")
        } else if ["produkt", "artikel", "product", "item", "ware", "geraet", "gerät"].iter().any(|p| tab.contains(p)) {
            PRODUKTE[k].into()
        } else if ["fach", "kurs", "subject", "course", "gegenstand"].iter().any(|p| tab.contains(p)) {
            FAECHER[k].into()
        } else if ["abteilung", "department", "team"].iter().any(|p| tab.contains(p)) {
            ABTEILUNGEN[i % 6].into()
        } else if ["kategorie", "category", "gruppe"].iter().any(|p| tab.contains(p)) {
            KATEGORIEN[i % 6].into()
        } else if ["ort", "stadt", "city", "standort", "filiale"].iter().any(|p| tab.contains(p)) {
            ORTE[k].into()
        } else if ["land", "country"].iter().any(|p| tab.contains(p)) {
            LAENDER[i % 6].into()
        } else if ["klasse", "class"].iter().any(|p| tab.contains(p)) {
            KLASSEN[i % 8].into()
        } else if ["buch", "book", "film", "movie", "titel"].iter().any(|p| tab.contains(p)) {
            BUECHER[i % 8].into()
        } else {
            let mut t = tabelle.to_string();
            if let Some(c) = t.get(..1) {
                t = c.to_uppercase() + &t[1..];
            }
            format!("{t} {}", i + 1)
        }
    } else {
        let mut n = spalte.replace('_', " ");
        if let Some(c) = n.get(..1) {
            n = c.to_uppercase() + &n[1..];
        }
        format!("{n} {}", i + 1)
    };
    text(wert)
}

/// Erfundene Zeilen fuer eine neue Tabelle. `eltern(spalte)`: vorhandene Schluessel einer verwiesenen Tabelle.
/// Werte, mit denen das Skript vergleicht, kommen in den ersten Zeilen vor, damit die Abfragen etwas finden.
pub(super) fn beispielzeilen(e: &Entwurf, ws: &HashMap<String, Vec<Wert>>, eltern: &mut dyn FnMut(&str) -> Vec<String>, n: usize, qual: Option<&str>) -> Option<String> {
    let auto = e.auto_pk();
    let spalten: Vec<&(String, String)> = e.spalten.iter().enumerate().filter(|(i, _)| Some(*i) != auto).map(|(_, s)| s).collect();
    if spalten.is_empty() {
        return None;
    }
    let mut zeilen: Vec<Vec<String>> = vec![Vec::new(); n];
    for (c, t) in &spalten {
        let lc = c.to_ascii_lowercase();
        let ist_pk = e.pk.is_some_and(|p| e.spalten[p].0 == *c);
        let verweis = e.keys.iter().any(|k| k == c);
        let ids = if verweis { eltern(c) } else { Vec::new() };
        let bekannte: Vec<&Wert> = ws.get(&lc).map(|v| v.iter().filter(|w| **w != Wert::Null).collect()).unwrap_or_default();
        for (i, z) in zeilen.iter_mut().enumerate() {
            let v = if ist_pk {
                // Eindeutig: Zahlen bzw. Kennungen mit Nummer
                let up = t.to_ascii_uppercase();
                if up.starts_with("CHAR") || up.starts_with("VARCHAR") {
                    str_lit(&format!("{}{:03}", ascii(&e.name).chars().next().unwrap_or('k').to_ascii_uppercase(), i + 1))
                } else {
                    (i + 1).to_string()
                }
            } else if verweis {
                if ids.is_empty() { ((i % 3) + 1).to_string() } else { ids[i % ids.len()].clone() }
            } else if !bekannte.is_empty() && i < bekannte.len().max(n / 2) {
                bekannte[i % bekannte.len()].sql()
            } else {
                beispiel(c, t, &e.name, i)
            };
            z.push(v);
        }
    }
    let name = match qual {
        Some(d) => format!("{}.{}", quote_if_needed(d), quote_if_needed(&e.name)),
        None => quote_if_needed(&e.name),
    };
    let cols: Vec<String> = spalten.iter().map(|(c, _)| quote_if_needed(c)).collect();
    let rows: Vec<String> = zeilen.iter().map(|z| format!("  ({})", z.join(", "))).collect();
    Some(format!("INSERT INTO {name} ({}) VALUES\n{}", cols.join(", "), rows.join(",\n")))
}

/// Beispielzeilen fuer eine neu abgeleitete Tabelle (auch "Alles einrichten" fuer Programme, die nur lesen)
pub(crate) fn beispieldaten(tabelle: &str, spalten: &[(String, String)], pk: Option<usize>, n: usize) -> Option<String> {
    let spalten: Vec<(String, String)> = spalten
        .iter()
        .map(|(c, t)| (c.clone(), if t.is_empty() { crate::dev::einrichten::typ_aus_name(c).to_string() } else { t.clone() }))
        .collect();
    let keys = spalten
        .iter()
        .enumerate()
        .filter(|(i, (c, _))| Some(*i) != pk && (c.to_ascii_lowercase().ends_with("_id") || c.to_ascii_lowercase().starts_with("id_")))
        .map(|(_, (c, _))| c.clone())
        .collect();
    let e = Entwurf { name: tabelle.to_string(), spalten, pk, keys, geraten: false };
    let mut keine = |_: &str| Vec::new();
    beispielzeilen(&e, &HashMap::new(), &mut keine, n, None)
}

/// Tabelle, auf die eine Verweis-Spalte zeigt (kunde_id -> kunde/kunden), mit ihrer Schluesselspalte
fn eltern_tabelle<'a>(schema: &'a Schema, spalte: &str) -> Option<(&'a str, String)> {
    let lc = spalte.to_ascii_lowercase();
    let basis = lc.strip_suffix("_id").or_else(|| lc.strip_prefix("id_")).or_else(|| lc.strip_suffix("id"))?;
    let namen = [basis.to_string(), format!("{basis}s"), format!("{basis}n"), format!("{basis}en"), format!("{basis}e")];
    let t = schema.tables.iter().find(|t| !t.is_view && namen.iter().any(|n| t.name.eq_ignore_ascii_case(n)))?;
    let pk = t.columns.iter().find(|c| c.key == "PRI").or_else(|| t.columns.iter().find(|c| c.name.eq_ignore_ascii_case("id")))?;
    Some((t.name.as_str(), pk.name.clone()))
}

/// Fehlende Tabelle: aus einer anderen Datenbank uebernehmen oder aus dem Skript ableiten (samt Beispieldaten)
pub(super) fn tabelle(text: &str, f: &Fail, setup: &Setup, server: &mut dyn Server, sandbox: &Map, name: &str, qual: Option<&str>) -> Vec<Cand> {
    let mut out = Vec::new();
    if name.starts_with(SANDBOX_PREFIX) || name.is_empty() {
        return out;
    }
    let aktuell = f.schema.as_ref().map(|s| s.name.clone()).or_else(|| setup.database.clone()).unwrap_or_default();
    // 1. Gibt es die Tabelle in einer anderen Datenbank? Dann dort arbeiten bzw. sie samt Daten uebernehmen.
    if qual.is_none() {
        let rows = server
            .rows(&format!("SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_ROWS, TABLE_TYPE FROM information_schema.TABLES WHERE LOWER(TABLE_NAME) = LOWER({}) ORDER BY TABLE_SCHEMA", str_lit(name)))
            .unwrap_or_default();
        let fund = rows.iter().find_map(|r| {
            let d = r.first().cloned().flatten()?;
            let t = r.get(1).cloned().flatten()?;
            let n: u64 = r.get(2).cloned().flatten().and_then(|n| n.parse().ok()).unwrap_or(0);
            let view = r.get(3).cloned().flatten().unwrap_or_default().contains("VIEW");
            (!is_system(&d) && !d.starts_with(SANDBOX_PREFIX) && !d.eq_ignore_ascii_case(&aktuell)).then_some((d, t, n, view))
        });
        if let Some((d, t, n, view)) = fund {
            // Alle Tabellen, die das Skript liest und die hier fehlen, gibt es dort: in dieser Datenbank arbeiten
            let dort: Vec<String> = server
                .rows(&format!("SELECT LOWER(TABLE_NAME) FROM information_schema.TABLES WHERE TABLE_SCHEMA = {}", str_lit(&d)))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .collect();
            let angelegt: Vec<String> = created_tables(text).iter().map(|t| t.to_ascii_lowercase()).collect();
            let fehlend: Vec<String> = crate::sqledit::nutzung(text, &|_| None)
                .into_iter()
                .filter(|n| n.datenbank.is_none())
                .map(|n| n.tabelle.to_ascii_lowercase())
                .filter(|t| !angelegt.contains(t) && f.schema.as_ref().is_none_or(|s| s.table(t).is_none()))
                .collect();
            let hat_use = db::split_statements(text).iter().any(|s| head(&s.sql).0 == "USE");
            if !hat_use && !fehlend.is_empty() && fehlend.iter().all(|t| dort.contains(t)) {
                let schreibt = db::split_statements(text).iter().any(|s| {
                    let k = head(&s.sql).0;
                    !(k == "SELECT" || k == "WITH" || k == "SHOW" || k == "SET")
                });
                let sql = format!("USE {};\n", quote_if_needed(&d));
                out.push(cand(
                    insert_before(text, &f.stmt, &sql),
                    crate::tr_format!("In der Datenbank „{d}“ arbeiten – dort gibt es „{t}“", "Work in the database “{d}” – “{t}” exists there"),
                    schreibt,
                    &sql,
                ));
            }
            if !view && n <= 100_000 {
                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS {q} LIKE {}.{q};\nINSERT INTO {q} SELECT * FROM {}.{q};\n",
                    quote_if_needed(&d),
                    quote_if_needed(&d),
                    q = quote_if_needed(&t)
                );
                out.push(cand(
                    insert_before(text, &f.stmt, &sql),
                    crate::tr_format!("Tabelle „{t}“ samt Daten aus „{d}“ übernommen", "Copied the table “{t}” with its data from “{d}”"),
                    !filled_tables(text).iter().any(|x| x.eq_ignore_ascii_case(name)),
                    &sql,
                ));
            }
        }
    }
    // 2. Aus dem Skript ableiten
    let Some(e) = entwurf(text, name, &f.stmt.sql) else { return out };
    let ddl = e.ddl(qual);
    let spalten: Vec<&str> = e.spalten.iter().map(|(c, _)| c.as_str()).collect();
    let liste = if spalten.len() > 5 { format!("{}, …", spalten[..4].join(", ")) } else { spalten.join(", ") };
    let befuellt = filled_tables(text).iter().any(|t| t.eq_ignore_ascii_case(name));
    if !befuellt && !e.geraten {
        let namen: Vec<String> = e.spalten.iter().map(|(c, _)| c.clone()).collect();
        let ws = werte(text, name, &namen);
        let schema = f.schema.clone();
        let probe_db = schema.as_ref().and_then(|s| sandbox.get(&s.name)).map(str::to_string);
        let mut eltern = |spalte: &str| -> Vec<String> {
            let (Some(s), Some(pdb)) = (schema.as_ref(), probe_db.as_deref()) else { return Vec::new() };
            let Some((t, pk)) = eltern_tabelle(s, spalte) else { return Vec::new() };
            server
                .rows(&format!("SELECT {} FROM {}.{} ORDER BY 1 LIMIT 12", db::q(&pk), db::q(pdb), db::q(t)))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .map(|v| if v.parse::<f64>().is_ok() { v } else { str_lit(&v) })
                .collect()
        };
        if let Some(ins) = beispielzeilen(&e, &ws, &mut eltern, 8, qual) {
            let sql = format!("{ddl};\n{ins};\n\n");
            out.push(cand(
                insert_before(text, &f.stmt, &sql),
                crate::tr_format!(
                    "Tabelle „{name}“ aus dem Skript abgeleitet ({liste}) und mit 8 Beispielzeilen gefüllt",
                    "Derived the table “{name}” from the script ({liste}) and filled it with 8 sample rows"
                ),
                // Erfundene Daten fuer eine Abfrage: erst nach einem Blick darauf in der echten Datenbank
                true,
                &ddl,
            ));
        }
    }
    let sql = format!("{ddl};\n\n");
    out.push(cand(
        insert_before(text, &f.stmt, &sql),
        if e.geraten {
            crate::tr_format!("Tabelle „{name}“ angelegt ({liste}) – Spaltennamen bitte anpassen", "Created the table “{name}” ({liste}) – please rename the columns")
        } else {
            crate::tr_format!("Tabelle „{name}“ aus dem Skript abgeleitet und angelegt ({liste})", "Derived the table “{name}” from the script and created it ({liste})")
        },
        // Schreibt das Skript selbst in die Tabelle, ist sie eindeutig gewollt; sonst bitte ansehen
        e.geraten || !befuellt,
        &ddl,
    ));
    out
}

/// Tabellen, auf die ein CREATE TABLE verweist und die es weder gibt noch spaeter im Skript gibt
pub(super) fn fehlende_eltern(text: &str, f: &Fail) -> Vec<String> {
    if head(&f.stmt.sql).0 != "CREATE TABLE" {
        return Vec::new();
    }
    let (_, toks) = lexed(&f.stmt.sql);
    let angelegt: Vec<String> = created_tables(text).iter().map(|t| t.to_ascii_lowercase()).collect();
    let mut out: Vec<String> = Vec::new();
    for k in 0..toks.len() {
        if toks[k].is_word("REFERENCES")
            && let Some(t) = toks.get(k + 1).filter(|t| t.is_name() && !toks.get(k + 2).is_some_and(|d| d.kind == Kind::Dot))
            && f.schema.as_ref().is_none_or(|s| s.table(&t.text).is_none())
            && !angelegt.contains(&t.text.to_ascii_lowercase())
            && !out.iter().any(|o| o.eq_ignore_ascii_case(&t.text))
        {
            out.push(t.text.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Fehlende Spalte

/// Unbekannte Spalte, fuer die es keinen aehnlichen Namen gibt: in der Tabelle anlegen
pub(super) fn spalte(text: &str, f: &Fail) -> Vec<Cand> {
    let mut out = Vec::new();
    let Some(full) = quoted(&f.error, "Unknown column '") else { return out };
    let (qual, col) = match full.rsplit_once('.') {
        Some((q, c)) => (Some(q.to_string()), c.to_string()),
        None => (None, full.clone()),
    };
    let Some(schema) = f.schema.as_ref() else { return out };
    if col.is_empty() || known_word(&col.to_ascii_uppercase()) || col.chars().any(|c| c.is_whitespace()) {
        return out;
    }
    let (chars, toks) = lexed(&f.stmt.sql);
    // In VALUES ist ein unbekannter Name fast immer Text ohne Anfuehrungszeichen
    if let Some((_, vi)) = insert_parts(&toks)
        && toks[vi..].iter().any(|t| t.is_name() && t.text == col)
    {
        return out;
    }
    let _ = chars;
    let nutz = crate::sqledit::nutzung(&f.stmt.sql, &|_| None);
    let basis = |n: &&crate::sqledit::Nutzung| schema.table(&n.tabelle).is_some_and(|t| !t.is_view);
    let mit: Vec<&crate::sqledit::Nutzung> = nutz.iter().filter(basis).filter(|n| n.spalten.iter().any(|(c, _)| c.eq_ignore_ascii_case(&col))).collect();
    let tab = match (mit.as_slice(), qual.as_deref()) {
        ([n], _) => n.tabelle.clone(),
        (_, Some(q)) if schema.table(q).is_some() => q.to_string(),
        _ => {
            let alle: Vec<&crate::sqledit::Nutzung> = nutz.iter().filter(basis).collect();
            match alle.as_slice() {
                [n] => n.tabelle.clone(),
                _ => return out,
            }
        }
    };
    let Some(info) = schema.table(&tab) else { return out };
    if info.columns.iter().any(|c| c.name.eq_ignore_ascii_case(&col)) {
        return out;
    }
    // Typ aus allem, was das Skript mit der Spalte macht
    let hinweis = crate::sqledit::nutzung(text, &|_| None)
        .into_iter()
        .filter(|n| n.tabelle.eq_ignore_ascii_case(&tab))
        .flat_map(|n| n.spalten)
        .find(|(c, _)| c.eq_ignore_ascii_case(&col))
        .map(|(_, t)| t)
        .unwrap_or_default();
    let ws = werte(text, &tab, std::slice::from_ref(&col));
    let typ = typ(&col, &hinweis, ws.get(&col.to_ascii_lowercase()).map_or(&[], Vec::as_slice));
    // Schreibt die Anweisung in die Spalte? Sonst ist es eher ein Tippfehler: bitte ansehen.
    let (art, _) = head(&f.stmt.sql);
    let schreibt = match art.as_str() {
        "INSERT" | "REPLACE" => true,
        "UPDATE" => {
            let set = toks.iter().position(|t| t.is_word("SET")).unwrap_or(toks.len());
            let ende = toks.iter().position(|t| t.is_word("WHERE")).unwrap_or(toks.len());
            toks[set.min(ende)..ende].windows(2).any(|w| w[0].is_name() && w[0].text.eq_ignore_ascii_case(&col) && w[1].kind == Kind::Op && w[1].text == "=")
        }
        _ => false,
    };
    let def = format!("{} {typ} NULL", quote_if_needed(&col));
    let label = crate::tr_format!("Spalte „{tab}.{col}“ ({typ}) angelegt", "Created the column “{tab}.{col}” ({typ})");
    // Legt das Skript die Tabelle selbst an, gehoert die Spalte dorthin
    for st in db::split_statements(text) {
        if st.start >= f.stmt.start {
            break;
        }
        let (k, z) = head(&st.sql);
        if k != "CREATE TABLE" || !z.as_deref().is_some_and(|z| z.eq_ignore_ascii_case(&tab)) {
            continue;
        }
        let (cc, ct) = lexed(&st.sql);
        let Some(open) = ct.iter().position(|t| t.kind == Kind::Open) else { continue };
        let Some(close) = group_end(&ct, open) else { continue };
        // Vor den Tabellen-Einschraenkungen (PRIMARY KEY, FOREIGN KEY …) einfuegen
        let its = items(&ct, open, close);
        let vor = its
            .iter()
            .find(|(a, _)| ["PRIMARY", "FOREIGN", "KEY", "INDEX", "UNIQUE", "CONSTRAINT", "CHECK", "FULLTEXT"].iter().any(|w| ct[*a].is_word(w)))
            .map(|(a, _)| ct[*a].start);
        let (at, ins) = match vor {
            Some(at) => (at, format!("{def},\n  ")),
            None => (ct[close - 1].end, format!(",\n  {def}")),
        };
        let mut neu = cc.clone();
        neu.splice(at..at, ins.chars());
        let neu: String = neu.into_iter().collect();
        if let Some(t) = super::replace_stmt(text, &st, &neu) {
            out.push(cand(t, label.clone(), !schreibt, &def));
        }
        break;
    }
    let sql = format!("ALTER TABLE {} ADD COLUMN IF NOT EXISTS {def};\n", quote_if_needed(&info.name));
    out.push(cand(insert_before(text, &f.stmt, &sql), label, !schreibt, &sql));
    out
}

// ---------------------------------------------------------------------------
// Zu kleiner oder falscher Spaltentyp

/// Spaltenname aus "… for column 'x' at row 1" bzw. "… for column `db`.`t`.`x` at row 1"
fn spalte_aus(e: &str) -> Option<(Option<String>, String)> {
    let r = &e[e.find("for column ")? + 11..];
    let roh = r.split(" at row").next()?.trim();
    let teile: Vec<String> = roh.split('.').map(|t| t.trim_matches(['\'', '`']).to_string()).collect();
    match teile.as_slice() {
        [c] => Some((None, c.clone())),
        [.., t, c] => Some((Some(t.clone()), c.clone())),
        _ => None,
    }
}

/// Werte, die die Anweisung in `spalte` schreibt (INSERT-Liste oder SET)
fn geschrieben(chars: &[char], toks: &[Tok], spalte: &str, info: Option<&crate::db::TableInfo>) -> Vec<Wert> {
    let mut out = Vec::new();
    if let Some((ti, vi)) = insert_parts(toks) {
        let pos = if toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) {
            let Some(close) = group_end(toks, ti + 1) else { return out };
            items(toks, ti + 1, close).iter().position(|(a, _)| toks[*a].text.eq_ignore_ascii_case(spalte))
        } else {
            info.and_then(|t| t.columns.iter().filter(|c| !c.is_generated()).position(|c| c.name.eq_ignore_ascii_case(spalte)))
        };
        let Some(pos) = pos else { return out };
        let mut k = vi + 1;
        while let Some(t) = toks.get(k) {
            if t.kind != Kind::Open {
                k += 1;
                continue;
            }
            let Some(end) = group_end(toks, k) else { break };
            if let Some((a, b)) = items(toks, k, end).get(pos)
                && let Some(w) = wert(chars, toks, *a, *b)
            {
                out.push(w);
            }
            k = end + 1;
        }
    } else {
        for k in 0..toks.len().saturating_sub(2) {
            if toks[k].is_name() && toks[k].text.eq_ignore_ascii_case(spalte) && toks[k + 1].kind == Kind::Op && toks[k + 1].text == "=" {
                let b = if toks[k + 2].kind == Kind::Op { k + 4 } else { k + 3 };
                if let Some(w) = wert(chars, toks, k + 2, b.min(toks.len())) {
                    out.push(w);
                }
            }
        }
    }
    out
}

/// Neuer Typ fuer eine Spalte, in die Werte nicht passen (zu lang, zu gross, Text in einer Zahlenspalte)
fn groesserer_typ(code: u32, spalte: &str, alt: &str, ws: &[Wert]) -> Option<String> {
    let a = alt.to_ascii_uppercase();
    let texte: Vec<&str> = ws.iter().filter_map(|w| if let Wert::Text(s) = w { Some(s.as_str()) } else { None }).collect();
    let laengste = ws.iter().map(|w| match w { Wert::Text(s) | Wert::Zahl(s) => s.chars().count(), _ => 0 }).max().unwrap_or(0);
    let text_typ = |mind: usize| -> String {
        let n = [50, 100, 255, 500, 1000, 2000, 4000].into_iter().find(|s| *s >= laengste.max(mind) && *s > laenge(&a).unwrap_or(0));
        n.map_or("TEXT".into(), |n| format!("VARCHAR({n})"))
    };
    let ganz = ["TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "INTEGER"].iter().any(|t| a.starts_with(t));
    match code {
        1406 if a.starts_with("VARCHAR") || a.starts_with("CHAR") => Some(text_typ(0)),
        1406 if a == "TINYTEXT" || a == "TEXT" => Some("MEDIUMTEXT".into()),
        // Text in einer Zahlen- oder Datumsspalte (Telefonnummer als INT, PLZ mit Buchstaben …)
        1366 | 1265 | 1292 if !texte.is_empty() && (ganz || a.starts_with("DECIMAL") || a.starts_with("DOUBLE") || a.starts_with("FLOAT")) => {
            let zahlen = texte.iter().all(|t| t.trim().replace(',', ".").parse::<f64>().is_ok());
            if zahlen {
                return None;
            }
            Some(text_typ(20))
        }
        1264 if ganz => {
            let max = ws.iter().filter_map(|w| if let Wert::Zahl(z) = w { z.parse::<f64>().ok() } else { None }).fold(0f64, |m, z| m.max(z.abs()));
            let neg = ws.iter().any(|w| matches!(w, Wert::Zahl(z) if z.starts_with('-')));
            // Telefonnummern und Kennungen sind keine Zahlen zum Rechnen
            let lc = spalte.to_lowercase();
            if ["telefon", "phone", "handy", "mobil", "fax", "iban", "plz", "zip", "svnr"].iter().any(|w| lc.contains(w)) || teile(spalte).iter().any(|t| t == "tel") {
                return Some(text_typ(20));
            }
            let basis = if max > i32::MAX as f64 || a.starts_with("INT") || a.starts_with("INTEGER") { "BIGINT" } else { "INT" };
            let unsigned = a.contains("UNSIGNED") && !neg;
            Some(format!("{basis}{}", if unsigned { " UNSIGNED" } else { "" }))
        }
        1264 if a.starts_with("DECIMAL") || a.starts_with("NUMERIC") => {
            let (p, s) = a.split(['(', ')']).nth(1).and_then(|x| x.split_once(',')).map(|(p, s)| (p.trim().parse::<usize>().ok(), s.trim().parse::<usize>().ok())).unwrap_or((Some(10), Some(0)));
            let s = s.unwrap_or(0);
            let vor = ws.iter().filter_map(|w| if let Wert::Zahl(z) = w { Some(z.trim_start_matches('-').split('.').next().unwrap_or("").len()) } else { None }).max().unwrap_or(0);
            let p = (vor + s).max(p.unwrap_or(10) + 2).min(65);
            Some(format!("DECIMAL({p},{s})"))
        }
        // Wert fehlt in der ENUM-Liste: Liste um die neuen Werte erweitern
        1265 | 1366 if a.starts_with("ENUM(") && !texte.is_empty() => {
            let mut liste = enum_werte(alt);
            for t in texte {
                if !liste.iter().any(|l| l.eq_ignore_ascii_case(t)) {
                    liste.push(t.to_string());
                }
            }
            if liste.len() == enum_werte(alt).len() {
                return None;
            }
            Some(format!("ENUM({})", liste.iter().map(|w| format!("'{}'", w.replace('\'', "''"))).collect::<Vec<_>>().join(",")))
        }
        _ => None,
    }
}

/// Werte einer ENUM-Definition wie `enum('a','b''c')`
fn enum_werte(def: &str) -> Vec<String> {
    let innen = def.split_once('(').map_or("", |(_, r)| r.rsplit_once(')').map_or(r, |(i, _)| i));
    let mut out = Vec::new();
    let mut cs = innen.chars().peekable();
    while let Some(c) = cs.next() {
        if c != '\'' {
            continue;
        }
        let mut w = String::new();
        while let Some(c) = cs.next() {
            if c != '\'' {
                w.push(c);
            } else if cs.peek() == Some(&'\'') {
                cs.next();
                w.push(c);
            } else {
                break;
            }
        }
        out.push(w);
    }
    out
}

/// Spalte zu klein (1406, 1264) oder vom falschen Typ (1366): im CREATE TABLE des Skripts bzw. per ALTER aendern
pub(super) fn typ_anpassen(text: &str, f: &Fail) -> Vec<Cand> {
    let mut out = Vec::new();
    let Some(code) = errfix::code(&f.error) else { return out };
    let Some((tab_aus_fehler, col)) = spalte_aus(&f.error) else { return out };
    let Some(schema) = f.schema.as_ref() else { return out };
    let (art, ziel) = head(&f.stmt.sql);
    let tab = tab_aus_fehler.or(if matches!(art.as_str(), "INSERT" | "REPLACE" | "UPDATE") { ziel } else { None });
    let Some(info) = tab.as_deref().and_then(|t| schema.table(t)) else { return out };
    let Some(spalte) = info.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&col)) else { return out };
    let (chars, toks) = lexed(&f.stmt.sql);
    // Emoji oder andere 4-Byte-Zeichen in einer Tabelle mit altem Zeichensatz
    if code == 1366 && f.error.contains("Incorrect string value") {
        let def = format!("ALTER TABLE {} CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;\n", quote_if_needed(&info.name));
        let label = crate::tr_format!("Tabelle „{}“ auf utf8mb4 umgestellt, damit alle Zeichen passen", "Converted the table “{}” to utf8mb4 so every character fits", info.name);
        out.push(cand(insert_before(text, &f.stmt, &def), label, false, &def));
        return out;
    }
    let ws = geschrieben(&chars, &toks, &spalte.name, Some(info));
    let Some(neu) = groesserer_typ(code, &spalte.name, &spalte.col_type, &ws) else { return out };
    let label = crate::tr_format!("Spalte „{}.{}“ auf {neu} geändert, damit die Werte passen", "Changed the column “{}.{}” to {neu} so the values fit", info.name, spalte.name);
    // Im CREATE TABLE des Skripts (vor der Anweisung) den Typ ersetzen
    for st in db::split_statements(text) {
        if st.start >= f.stmt.start {
            break;
        }
        let (k, z) = head(&st.sql);
        if k != "CREATE TABLE" || !z.as_deref().is_some_and(|z| z.eq_ignore_ascii_case(&info.name)) {
            continue;
        }
        let (cc, ct) = lexed(&st.sql);
        let Some(open) = ct.iter().position(|t| t.kind == Kind::Open) else { continue };
        let Some(close) = group_end(&ct, open) else { continue };
        for (a, b) in items(&ct, open, close) {
            if !ct[a].is_name() || !ct[a].text.eq_ignore_ascii_case(&spalte.name) || a + 1 >= b {
                continue;
            }
            let mut e = a + 2;
            if ct.get(e).is_some_and(|t| t.kind == Kind::Open)
                && let Some(g) = group_end(&ct, e)
            {
                e = g + 1;
            }
            while e < b && (ct[e].is_word("UNSIGNED") || ct[e].is_word("ZEROFILL")) {
                e += 1;
            }
            let mut n = cc.clone();
            n.splice(ct[a + 1].start..ct[e - 1].end, neu.chars());
            let n: String = n.into_iter().collect();
            if let Some(t) = super::replace_stmt(text, &st, &n) {
                out.push(cand(t, label.clone(), false, &n));
            }
            break;
        }
        break;
    }
    let mut def = format!("ALTER TABLE {} MODIFY {} {neu}", quote_if_needed(&info.name), quote_if_needed(&spalte.name));
    if !spalte.nullable {
        def.push_str(" NOT NULL");
    }
    if let Some(d) = &spalte.default {
        def.push_str(&format!(" DEFAULT {d}"));
    }
    def.push_str(";\n");
    out.push(cand(insert_before(text, &f.stmt, &def), label, false, &def));
    out
}

// ---------------------------------------------------------------------------
// Abhaengige Zeilen und Tabellen

/// Elternzeile soll geloescht werden, Kindzeilen verweisen darauf (1451)
pub(super) fn kinder_zuerst(text: &str, f: &Fail) -> Vec<Cand> {
    let mut out = Vec::new();
    let Some(kind) = kind_tabelle(&f.error) else { return out };
    let e = &f.error;
    let (Some(fk), Some(r)) = (quoted(e, "FOREIGN KEY (`"), e.find("REFERENCES `").map(|i| &e[i..])) else { return out };
    let (Some(eltern), Some(ref_col)) = (quoted(r, "REFERENCES `"), quoted(&r[r.find('(').unwrap_or(0)..], "(`")) else { return out };
    let (art, _) = head(&f.stmt.sql);
    match art.as_str() {
        // Rerun: DROP TABLE eltern, das Skript legt die Kindtabelle spaeter neu an -> sie zuerst loeschen
        "DROP TABLE" if created_tables(text).iter().any(|t| t.eq_ignore_ascii_case(&kind)) => {
            let sql = format!("DROP TABLE IF EXISTS {};\n", quote_if_needed(&kind));
            out.push(cand(
                insert_before(text, &f.stmt, &sql),
                crate::tr_format!("Abhängige Tabelle „{kind}“ zuerst löschen – das Skript legt sie danach neu an", "Drop the dependent table “{kind}” first – the script creates it again later"),
                false,
                &sql,
            ));
        }
        "DELETE" => {
            let (chars, toks) = lexed(&f.stmt.sql);
            let bedingung = toks.iter().position(|t| t.is_word("WHERE")).map(|w| chars[toks[w].start..].iter().collect::<String>());
            let sql = format!(
                "DELETE FROM {} WHERE {} IN (SELECT {} FROM {}{});\n",
                quote_if_needed(&kind),
                quote_if_needed(&fk),
                quote_if_needed(&ref_col),
                quote_if_needed(&eltern),
                bedingung.map(|b| format!(" {}", b.trim_end_matches(';'))).unwrap_or_default()
            );
            out.push(cand(
                insert_before(text, &f.stmt, &sql),
                crate::tr_format!("Zuerst die abhängigen Zeilen in „{kind}“ löschen", "Delete the dependent rows in “{kind}” first"),
                true,
                &sql,
            ));
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_follow_the_values_the_script_writes() {
        let txt = |s: &str| Wert::Text(s.into());
        let num = |s: &str| Wert::Zahl(s.into());
        assert_eq!(typ("geburt", "VARCHAR(255)", &[txt("2008-01-02")]), "DATE");
        assert_eq!(typ("x", "", &[txt("2008-01-02 10:00:00"), txt("2008-01-02")]), "DATETIME");
        assert_eq!(typ("name", "VARCHAR(255)", &[txt("Ada")]), "VARCHAR(50)");
        assert_eq!(typ("preis", "", &[num("9.5"), num("129.99")]), "DECIMAL(10,2)");
        assert_eq!(typ("menge", "", &[num("3")]), "INT");
        assert_eq!(typ("aktiv", "", &[num("1"), num("0")]), "BOOLEAN");
        assert_eq!(typ("beschreibung", "", &[]), "TEXT");
        assert_eq!(typ("x", "INT", &[]), "INT");
    }

    #[test]
    fn a_missing_table_is_derived_from_every_use_in_the_script() {
        let script = "SELECT k.name, k.ort FROM kunde k WHERE k.ort = 'Wien';\n\
                      INSERT INTO bestellung (kunde_id, betrag, datum) VALUES (1, 19.90, '2025-03-01');\n\
                      SELECT * FROM bestellung WHERE betrag > 10;";
        let e = entwurf(script, "bestellung", "INSERT INTO bestellung (kunde_id, betrag, datum) VALUES (1, 19.90, '2025-03-01')").unwrap();
        let ddl = e.ddl(None);
        assert!(ddl.contains("id INT NOT NULL AUTO_INCREMENT PRIMARY KEY"), "{ddl}");
        assert!(ddl.contains("betrag DECIMAL(10,2)"), "{ddl}");
        assert!(ddl.contains("datum DATE"), "{ddl}");
        assert!(ddl.contains("KEY (kunde_id)"), "{ddl}");
        let k = entwurf(script, "kunde", "SELECT k.name, k.ort FROM kunde k WHERE k.ort = 'Wien'").unwrap();
        let mut keine = |_: &str| Vec::new();
        let ws = werte(script, "kunde", &["name".into(), "ort".into()]);
        let rows = beispielzeilen(&k, &ws, &mut keine, 8, None).unwrap();
        // Die gesuchte Stadt kommt vor, damit die Abfrage etwas findet
        assert!(rows.contains("'Wien'"), "{rows}");
        assert!(rows.starts_with("INSERT INTO kunde (name, ort) VALUES"), "{rows}");
        assert_eq!(rows.matches("\n  (").count(), 8, "{rows}");
    }

    #[test]
    fn referenced_parents_get_the_key_the_child_expects() {
        let script = "CREATE TABLE note (id INT PRIMARY KEY, schueler_nr VARCHAR(10), FOREIGN KEY (schueler_nr) REFERENCES schueler(nr));";
        let e = entwurf(script, "schueler", "").unwrap();
        assert!(e.ddl(None).contains("nr VARCHAR(191) NOT NULL PRIMARY KEY"), "{}", e.ddl(None));
    }

    #[test]
    fn insert_without_columns_names_them_by_position() {
        let sql = "INSERT INTO tier VALUES (1, 'Bello', '2020-05-01')";
        let e = entwurf(sql, "tier", sql).unwrap();
        assert!(e.geraten);
        let ddl = e.ddl(None);
        assert!(ddl.contains("id INT NOT NULL AUTO_INCREMENT PRIMARY KEY") && ddl.contains("spalte_2 VARCHAR(50)") && ddl.contains("spalte_3 DATE"), "{ddl}");
    }

    #[test]
    fn sample_values_fit_their_columns() {
        assert_eq!(beispiel("email", "VARCHAR(255)", "kunde", 0), "'anna.bauer@example.com'");
        assert_eq!(beispiel("name", "VARCHAR(50)", "kunde", 1), "'Ben Hofer'");
        assert_eq!(beispiel("name", "VARCHAR(50)", "produkt", 0), "'Laptop'");
        assert_eq!(beispiel("preis", "DECIMAL(10,2)", "produkt", 0), "899.00");
        assert_eq!(beispiel("ort", "VARCHAR(4)", "kunde", 0), "'Wien'");
        assert_eq!(beispiel("ort", "VARCHAR(3)", "kunde", 0), "'Wie'");
        assert!(beispiel("geburtsdatum", "DATE", "kunde", 0).starts_with("'2000-"));
    }

    #[test]
    fn wider_types_for_values_that_do_not_fit() {
        let txt = |s: &str| Wert::Text(s.into());
        assert_eq!(groesserer_typ(1406, "name", "varchar(10)", &[txt("Abschlussklasse 2026")]).as_deref(), Some("VARCHAR(50)"));
        assert_eq!(groesserer_typ(1264, "menge", "tinyint(4)", &[Wert::Zahl("300".into())]).as_deref(), Some("INT"));
        assert_eq!(groesserer_typ(1264, "einwohner", "int(11)", &[Wert::Zahl("9000000000".into())]).as_deref(), Some("BIGINT"));
        assert_eq!(groesserer_typ(1264, "telefon", "int(11)", &[Wert::Zahl("06641234567".into())]).as_deref(), Some("VARCHAR(50)"));
        assert_eq!(groesserer_typ(1366, "plz", "int(11)", &[txt("A-1010")]).as_deref(), Some("VARCHAR(50)"));
        assert_eq!(groesserer_typ(1366, "plz", "int(11)", &[txt("1010")]), None);
        assert_eq!(groesserer_typ(1265, "status", "enum('aktiv','inaktiv')", &[txt("gesperrt"), txt("AKTIV")]).as_deref(), Some("ENUM('aktiv','inaktiv','gesperrt')"));
        assert_eq!(groesserer_typ(1265, "status", "enum('a''b')", &[txt("a'b")]), None);
        assert_eq!(spalte_aus("ERROR 1366 (22007): Incorrect integer value: 'A-1010' for column `db`.`kunde`.`plz` at row 1"), Some((Some("kunde".into()), "plz".into())));
        assert_eq!(spalte_aus("ERROR 1406 (22001): Data too long for column 'name' at row 1"), Some((None, "name".into())));
    }
}
