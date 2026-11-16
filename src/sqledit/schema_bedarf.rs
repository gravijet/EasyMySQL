// Welche Tabellen und Spalten eine Anweisung braucht (SELECT/INSERT/UPDATE/DELETE, auch mit JOIN, Alias und
// Unterabfrage). Grundlage fuer "Alles einrichten": fehlende Tabellen und Spalten werden daraus angelegt.
// Unklare Faelle (unqualifizierte Spalte bei mehreren Tabellen) werden ausgelassen statt geraten.
use super::lint::{Kind, Tok, lex};
use std::collections::HashSet;

/// Bedarf einer Anweisung an eine Tabelle
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Nutzung {
    pub datenbank: Option<String>,
    pub tabelle: String,
    /// Spalte und Typ-Hinweis (leer = unbekannt), in der Reihenfolge des ersten Auftretens
    pub spalten: Vec<(String, String)>,
    /// INSERT ohne Spaltenliste: Anzahl der Werte
    pub positionell: Option<usize>,
}

impl Nutzung {
    fn spalte(&mut self, name: &str, typ: Option<String>) {
        if name == "*" || name.is_empty() {
            return;
        }
        match self.spalten.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some((_, t)) => {
                if let Some(neu) = typ {
                    *t = breiter(t, &neu);
                }
            }
            None => self.spalten.push((name.to_string(), typ.unwrap_or_default())),
        }
    }
}

/// Zwei Typ-Hinweise vereinen: der allgemeinere gewinnt (Zahl + Text = Text)
pub fn breiter(a: &str, b: &str) -> String {
    if a.is_empty() || a == b {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    let rang = |t: &str| -> u8 {
        let u = t.to_ascii_uppercase();
        if u.starts_with("BOOL") {
            1
        } else if u.starts_with("INT") || u.starts_with("SMALLINT") || u.starts_with("TINYINT") {
            2
        } else if u.starts_with("BIGINT") {
            3
        } else if u.starts_with("DECIMAL") {
            4
        } else if u.starts_with("DOUBLE") || u.starts_with("FLOAT") {
            5
        } else if u.starts_with("DATE") || u.starts_with("TIME") {
            6
        } else if u.starts_with("VARCHAR") {
            7
        } else if u.contains("BLOB") {
            9
        } else {
            8
        }
    };
    let (ra, rb) = (rang(a), rang(b));
    // Datum und Zahl passen nicht zusammen: dann Text
    if (ra == 6) != (rb == 6) && ra.max(rb) < 7 {
        return "VARCHAR(255)".into();
    }
    if ra >= rb { a.to_string() } else { b.to_string() }
}

/// Woerter, die nie Spalten sind (Aufbau der Anweisung). Bewusst klein: `name`, `status`, `datum`, `year` sind Spalten.
const RESERVIERT: &[&str] = &[
    "ALL", "AND", "ANY", "AS", "ASC", "BETWEEN", "BINARY", "BY", "CASE", "COLLATE", "CROSS", "CURRENT_DATE", "CURRENT_TIME",
    "CURRENT_TIMESTAMP", "DEFAULT", "DELAYED", "DELETE", "DESC", "DISTINCT", "DISTINCTROW", "DIV", "DUPLICATE", "ELSE", "END",
    "ESCAPE", "EXISTS", "FALSE", "FETCH", "FIRST", "FOR", "FORCE", "FROM", "FULL", "GROUP", "HAVING", "HIGH_PRIORITY", "IF",
    "IGNORE", "IN", "INDEX", "INNER", "INSERT", "INTERVAL", "INTO", "IS", "JOIN", "KEY", "LEFT", "LIKE", "LIMIT", "LOCK",
    "LOW_PRIORITY", "MOD", "MODE", "NATURAL", "NEXT", "NOT", "NULL", "OFFSET", "ON", "ONLY", "OR", "ORDER", "OUTER", "OVER",
    "PARTITION", "QUICK", "RECURSIVE", "REGEXP", "REPLACE", "RETURNING", "RIGHT", "RLIKE", "ROW", "ROWS", "SELECT", "SET",
    "SHARE", "SOME", "SQL_CALC_FOUND_ROWS", "STRAIGHT_JOIN", "THEN", "TRUE", "UNION", "UNKNOWN", "UPDATE", "USE", "USING",
    "VALUE", "VALUES", "WHEN", "WHERE", "WINDOW", "WITH", "XOR",
];

fn reserviert(t: &Tok) -> bool {
    t.kind == Kind::Word && RESERVIERT.contains(&t.upper().as_str())
}

/// Woerter, nach denen ein Tabellenname folgt
fn vor_tabelle(t: &Tok) -> bool {
    t.kind == Kind::Word && matches!(t.upper().as_str(), "FROM" | "JOIN" | "INTO" | "STRAIGHT_JOIN")
}

/// Woerter, die eine FROM-Liste beenden
fn beendet_from(t: &Tok) -> bool {
    t.kind == Kind::Semi
        || t.kind == Kind::Close
        || t.kind == Kind::Word
            && matches!(
                t.upper().as_str(),
                "WHERE" | "GROUP" | "ORDER" | "LIMIT" | "HAVING" | "ON" | "USING" | "JOIN" | "LEFT" | "RIGHT" | "INNER" | "CROSS"
                    | "NATURAL" | "FULL" | "UNION" | "SET" | "VALUES" | "SELECT" | "WINDOW" | "FOR" | "LOCK" | "STRAIGHT_JOIN"
            )
}

struct Quelle {
    datenbank: Option<String>,
    tabelle: String,
    alias: Option<String>,
}

/// Name an `i` (optional `db.name`); liefert (db, name, naechster Index)
fn tabellenname(ts: &[Tok], i: usize) -> Option<(Option<String>, String, usize)> {
    let a = ts.get(i).filter(|t| t.is_name() && !reserviert(t))?;
    if ts.get(i + 1).is_some_and(|t| t.kind == Kind::Dot) {
        let b = ts.get(i + 2).filter(|t| t.is_name())?;
        Some((Some(a.text.clone()), b.text.clone(), i + 3))
    } else {
        Some((None, a.text.clone(), i + 1))
    }
}

/// Typ aus einem Wert: Text, Zahl, Wahrheitswert
fn literal_typ(chars: &[char], ts: &[Tok], i: usize) -> Option<String> {
    let t = ts.get(i)?;
    let neg = t.kind == Kind::Op && t.text == "-";
    let t = if neg { ts.get(i + 1)? } else { t };
    match t.kind {
        Kind::Str => Some("VARCHAR(255)".into()),
        Kind::Num => {
            let n: String = chars[t.start..t.end].iter().collect();
            if n.parse::<i64>().is_ok() {
                Some(if n.trim_start_matches('-').len() > 9 { "BIGINT" } else { "INT" }.into())
            } else if n.parse::<f64>().is_ok() {
                Some("DECIMAL(10,2)".into())
            } else {
                None
            }
        }
        _ if t.is_word("TRUE") || t.is_word("FALSE") => Some("BOOLEAN".into()),
        _ if t.is_word("NOW") || t.is_word("CURRENT_TIMESTAMP") => Some("DATETIME".into()),
        _ if t.is_word("CURDATE") || t.is_word("CURRENT_DATE") => Some("DATE".into()),
        _ => None,
    }
}

/// Bedarf aller Anweisungen in `sql`. `parameter(n)`: Typ des n-ten `?` (1-basiert), z. B. aus `setInt(n, …)`.
pub fn nutzung(sql: &str, parameter: &dyn Fn(usize) -> Option<String>) -> Vec<Nutzung> {
    let chars: Vec<char> = sql.chars().collect();
    if chars.len() > 200_000 {
        return Vec::new();
    }
    let toks = lex(&chars).toks;
    let mut out: Vec<Nutzung> = Vec::new();
    let mut par = 0usize;
    for st in toks.split(|t| t.kind == Kind::Semi) {
        let erste = st.iter().find(|t| t.kind == Kind::Word).map(|t| t.upper()).unwrap_or_default();
        if !matches!(erste.as_str(), "SELECT" | "WITH" | "INSERT" | "REPLACE" | "UPDATE" | "DELETE") {
            par += st.iter().filter(|t| t.kind == Kind::Op && t.text == "?").count();
            continue;
        }
        for n in anweisung(&chars, st, erste.as_str(), par, parameter) {
            match out.iter_mut().find(|o| o.tabelle.eq_ignore_ascii_case(&n.tabelle) && o.datenbank == n.datenbank) {
                Some(o) => {
                    for (c, t) in n.spalten {
                        o.spalte(&c, (!t.is_empty()).then_some(t));
                    }
                    o.positionell = o.positionell.or(n.positionell);
                }
                None => out.push(n),
            }
        }
        par += st.iter().filter(|t| t.kind == Kind::Op && t.text == "?").count();
    }
    out
}

fn anweisung(chars: &[char], ts: &[Tok], erste: &str, par0: usize, parameter: &dyn Fn(usize) -> Option<String>) -> Vec<Nutzung> {
    // Namen aus WITH x AS (…) sind keine Tabellen
    let mut cte: HashSet<String> = HashSet::new();
    if erste == "WITH" {
        for i in 0..ts.len().saturating_sub(2) {
            if ts[i].is_name() && ts[i + 1].is_word("AS") && ts[i + 2].kind == Kind::Open {
                cte.insert(ts[i].text.to_ascii_lowercase());
            }
        }
    }
    // Tabellen und Aliasse
    let mut quellen: Vec<Quelle> = Vec::new();
    let mut i = 0;
    while i < ts.len() {
        let start = vor_tabelle(&ts[i]) || (i == 0 || ts[..i].iter().all(|t| t.kind != Kind::Word)) && ts[i].is_word("UPDATE");
        if !start {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        // UPDATE LOW_PRIORITY IGNORE t …
        while ts.get(j).is_some_and(|t| t.is_word("LOW_PRIORITY") || t.is_word("IGNORE") || t.is_word("ONLY")) {
            j += 1;
        }
        while let Some((db, name, mut k)) = tabellenname(ts, j) {
            let mut alias = None;
            if ts.get(k).is_some_and(|t| t.is_word("AS")) {
                k += 1;
            }
            if let Some(a) = ts.get(k).filter(|t| t.is_name() && !reserviert(t) && !beendet_from(t)) {
                alias = Some(a.text.clone());
                k += 1;
            }
            if !cte.contains(&name.to_ascii_lowercase()) {
                quellen.push(Quelle { datenbank: db, tabelle: name, alias });
            }
            // weitere Tabellen per Komma (FROM a, b) – nur in einer FROM-Liste
            if ts.get(k).is_some_and(|t| t.kind == Kind::Comma) && !ts[i].is_word("INTO") {
                j = k + 1;
                continue;
            }
            break;
        }
        i = j.max(i + 1);
    }
    if quellen.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Nutzung> = Vec::new();
    // Index in `out` je Quelle (gleiche Tabelle mehrfach, z. B. Self-Join, landet zusammen)
    let mut platz: Vec<usize> = Vec::new();
    for q in &quellen {
        match out.iter().position(|o| o.tabelle.eq_ignore_ascii_case(&q.tabelle) && o.datenbank == q.datenbank) {
            Some(i) => platz.push(i),
            None => {
                platz.push(out.len());
                out.push(Nutzung { datenbank: q.datenbank.clone(), tabelle: q.tabelle.clone(), ..Default::default() });
            }
        }
    }
    let finde = |name: &str| -> Option<usize> {
        let i = quellen.iter().position(|q| q.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name)))
            .or_else(|| quellen.iter().position(|q| q.tabelle.eq_ignore_ascii_case(name)))?;
        Some(platz[i])
    };
    let einzig = (out.len() == 1).then_some(0usize);

    // Parameter-Nummer jedes Tokens
    let mut nummer = vec![0usize; ts.len()];
    let mut n = par0;
    for (k, t) in ts.iter().enumerate() {
        if t.kind == Kind::Op && t.text == "?" {
            n += 1;
            nummer[k] = n;
        }
    }
    let wert_typ = |k: usize| -> Option<String> {
        let t = ts.get(k)?;
        if t.kind == Kind::Op && t.text == "?" { parameter(nummer[k]) } else { literal_typ(chars, ts, k) }
    };

    // Namen, die keine Spalten sind: Tabellen, Aliasse, Ausgabe-Aliasse (… AS x), CTE
    let mut keine: HashSet<String> = HashSet::new();
    for q in &quellen {
        keine.insert(q.tabelle.to_ascii_lowercase());
        if let Some(a) = &q.alias {
            keine.insert(a.to_ascii_lowercase());
        }
        if let Some(d) = &q.datenbank {
            keine.insert(d.to_ascii_lowercase());
        }
    }
    keine.extend(cte.iter().cloned());
    for k in 1..ts.len() {
        if ts[k - 1].is_word("AS") && ts[k].is_name() {
            keine.insert(ts[k].text.to_ascii_lowercase());
        }
    }

    // INSERT INTO t (a, b) VALUES (…) bzw. ohne Spaltenliste
    if matches!(erste, "INSERT" | "REPLACE")
        && let Some(into) = ts.iter().position(|t| t.is_word("INTO"))
        && let Some((_, _, k)) = tabellenname(ts, into + 1)
    {
        let ziel = 0; // die erste Quelle ist das Ziel
        let mut k = k;
        if ts.get(k).is_some_and(|t| t.is_word("AS")) || ts.get(k).is_some_and(|t| t.is_name() && !reserviert(t)) {
            k += if ts.get(k).is_some_and(|t| t.is_word("AS")) { 2 } else { 1 };
        }
        let mut namen: Vec<String> = Vec::new();
        if ts.get(k).is_some_and(|t| t.kind == Kind::Open) {
            let mut e = k + 1;
            while let Some(t) = ts.get(e) {
                if t.kind == Kind::Close {
                    break;
                }
                if t.is_name() && ts.get(e + 1).is_some_and(|x| x.kind == Kind::Comma || x.kind == Kind::Close) {
                    namen.push(t.text.clone());
                }
                e += 1;
            }
            k = e + 1;
        }
        // Typ eines Werts von `a` bis vor `e`: einzelner Wert, negative Zahl oder NOW()/CURDATE()
        let wert_von = |a: usize, e: usize| -> Option<String> {
            if e == a + 1 || e == a + 2 && ts[a].text == "-" {
                wert_typ(a)
            } else if e == a + 3 && ts[a].kind == Kind::Word && ts[a + 1].kind == Kind::Open {
                literal_typ(chars, ts, a)
            } else {
                None
            }
        };
        // erste Wertezeile
        if ts.get(k).is_some_and(|t| t.is_word("VALUES") || t.is_word("VALUE")) && ts.get(k + 1).is_some_and(|t| t.kind == Kind::Open) {
            let mut werte: Vec<Option<String>> = Vec::new();
            let mut e = k + 2;
            let mut tiefe = 0;
            let mut anfang = e;
            while let Some(t) = ts.get(e) {
                match t.kind {
                    Kind::Open => tiefe += 1,
                    Kind::Close if tiefe == 0 => {
                        werte.push(wert_von(anfang, e));
                        break;
                    }
                    Kind::Close => tiefe -= 1,
                    Kind::Comma if tiefe == 0 => {
                        werte.push(wert_von(anfang, e));
                        anfang = e + 1;
                    }
                    _ => {}
                }
                e += 1;
            }
            if namen.is_empty() {
                out[ziel].positionell = Some(werte.len());
            } else {
                for (c, w) in namen.iter().zip(werte.into_iter().chain(std::iter::repeat(None))) {
                    out[ziel].spalte(c, w);
                }
            }
        } else {
            for c in &namen {
                out[ziel].spalte(c, None);
            }
        }
        // ON DUPLICATE KEY UPDATE a = …
        if let Some(dup) = ts.iter().position(|t| t.is_word("DUPLICATE")) {
            let mut e = dup;
            while e + 1 < ts.len() {
                if ts[e].is_name() && !reserviert(&ts[e]) && ts[e + 1].kind == Kind::Op && ts[e + 1].text == "=" {
                    out[ziel].spalte(&ts[e].text, wert_typ(e + 2));
                }
                e += 1;
            }
        }
        // INSERT … SELECT: die Spalten der Quelle laufen unten mit
        if !ts.iter().any(|t| t.is_word("SELECT")) {
            return out;
        }
    }

    // Spalten im Rest der Anweisung
    let ist_insert = matches!(erste, "INSERT" | "REPLACE");
    let into_ende = if ist_insert {
        // Spaltenliste des INSERT ueberspringen (oben behandelt)
        ts.iter().position(|t| t.is_word("SELECT")).unwrap_or(ts.len())
    } else {
        0
    };
    let mut k = into_ende;
    let mut intervall = false;
    while k < ts.len() {
        let t = &ts[k];
        if t.is_word("INTERVAL") {
            intervall = true;
            k += 1;
            continue;
        }
        if !t.is_name() || reserviert(t) {
            k += 1;
            continue;
        }
        // Einheit nach INTERVAL 1 DAY
        if intervall && k > 0 && matches!(ts[k - 1].kind, Kind::Num | Kind::Str | Kind::Op) {
            intervall = false;
            k += 1;
            continue;
        }
        let davor = k.checked_sub(1).map(|p| &ts[p]);
        // Tabellenname/Alias selbst, Funktionsaufruf, Typ nach AS, DATE 'x'
        if davor.is_some_and(|d| vor_tabelle(d) || d.is_word("AS") || d.is_word("UPDATE") || d.kind == Kind::Dot)
            || ts.get(k + 1).is_some_and(|n| n.kind == Kind::Open || n.kind == Kind::Str)
        {
            k += 1;
            continue;
        }
        // q.spalte
        if ts.get(k + 1).is_some_and(|n| n.kind == Kind::Dot)
            && let Some(c) = ts.get(k + 2).filter(|c| c.is_name() || c.text == "*")
        {
            if let Some(o) = finde(&t.text) {
                let typ = vergleich_typ(ts, k + 2, &wert_typ);
                out[o].spalte(&c.text, typ);
            }
            k += 3;
            continue;
        }
        // Tabellen der FROM-Liste ueberspringen (FROM a x, b y)
        if quellen.iter().any(|q| q.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&t.text)) || q.tabelle.eq_ignore_ascii_case(&t.text))
            && davor.is_some_and(|d| d.is_name() || d.kind == Kind::Comma)
        {
            k += 1;
            continue;
        }
        if keine.contains(&t.text.to_ascii_lowercase()) {
            k += 1;
            continue;
        }
        if let Some(o) = einzig {
            let typ = vergleich_typ(ts, k, &wert_typ);
            out[o].spalte(&t.text, typ);
        }
        k += 1;
    }
    out
}

/// `spalte = 'x'`, `spalte > 5`, `spalte = ?`, `spalte LIKE …`: Typ aus der anderen Seite
fn vergleich_typ(ts: &[Tok], k: usize, wert_typ: &dyn Fn(usize) -> Option<String>) -> Option<String> {
    let op = ts.get(k + 1)?;
    if op.is_word("LIKE") {
        return Some("VARCHAR(255)".into());
    }
    if op.kind == Kind::Op && matches!(op.text.as_str(), "=" | "<" | ">" | "<=" | ">=" | "<>" | "!=" | "<=>") {
        return wert_typ(k + 2);
    }
    if op.is_word("BETWEEN") {
        return wert_typ(k + 2);
    }
    if op.is_word("IN") && ts.get(k + 2).is_some_and(|t| t.kind == Kind::Open) {
        return wert_typ(k + 3);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(sql: &str) -> Vec<Nutzung> {
        nutzung(sql, &|_| None)
    }

    fn spalten(v: &Nutzung) -> Vec<&str> {
        v.spalten.iter().map(|(c, _)| c.as_str()).collect()
    }

    #[test]
    fn einfaches_select_mit_where() {
        let v = n("SELECT name, email FROM kunde WHERE id = 5 AND name LIKE 'A%' ORDER BY name");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].tabelle, "kunde");
        assert_eq!(spalten(&v[0]), ["name", "email", "id"]);
        assert_eq!(v[0].spalten[2].1, "INT");
        assert_eq!(v[0].spalten[0].1, "VARCHAR(255)");
    }

    #[test]
    fn join_mit_alias_ordnet_spalten_zu() {
        let v = n("SELECT k.name, b.datum, SUM(b.betrag) AS summe FROM kunde k JOIN bestellung AS b ON b.kunde_id = k.id WHERE b.betrag > 10.5 GROUP BY k.name ORDER BY summe");
        let kunde = v.iter().find(|x| x.tabelle == "kunde").unwrap();
        let best = v.iter().find(|x| x.tabelle == "bestellung").unwrap();
        assert_eq!(spalten(kunde), ["name", "id"]);
        assert_eq!(spalten(best), ["datum", "betrag", "kunde_id"]);
        assert_eq!(best.spalten[1].1, "DECIMAL(10,2)");
        // summe ist ein Ausgabe-Alias, keine Spalte; unqualifiziert bei zwei Tabellen: nichts raten
        assert!(!v.iter().any(|x| x.spalten.iter().any(|(c, _)| c == "summe")));
    }

    #[test]
    fn insert_mit_parametern_und_literalen() {
        let typ = |i: usize| match i {
            1 => Some("VARCHAR(255)".to_string()),
            2 => Some("DECIMAL(10,2)".to_string()),
            _ => None,
        };
        let v = nutzung("INSERT INTO produkt (name, preis, aktiv, lager) VALUES (?, ?, TRUE, 3)", &typ);
        assert_eq!(v[0].spalten, vec![
            ("name".into(), "VARCHAR(255)".into()),
            ("preis".into(), "DECIMAL(10,2)".into()),
            ("aktiv".into(), "BOOLEAN".into()),
            ("lager".into(), "INT".into()),
        ]);
    }

    #[test]
    fn insert_ohne_spaltenliste_ist_positionell() {
        let v = n("INSERT INTO log VALUES (?, ?, NOW())");
        assert_eq!(v[0].positionell, Some(3));
        assert!(v[0].spalten.is_empty());
    }

    #[test]
    fn update_und_delete() {
        let v = nutzung("UPDATE konto SET stand = stand - ?, geaendert = NOW() WHERE nr = ?", &|i| (i == 2).then(|| "INT".to_string()));
        assert_eq!(spalten(&v[0]), ["stand", "geaendert", "nr"]);
        assert_eq!(v[0].spalten[1].1, "DATETIME");
        assert_eq!(v[0].spalten[2].1, "INT");
        let v = n("DELETE FROM sitzung WHERE ablauf < NOW()");
        assert_eq!(v[0].tabelle, "sitzung");
        assert_eq!(spalten(&v[0]), ["ablauf"]);
    }

    #[test]
    fn unterabfrage_und_cte() {
        let v = n("WITH top AS (SELECT kunde_id FROM bestellung) SELECT name FROM kunde WHERE id IN (SELECT kunde_id FROM top)");
        assert!(v.iter().any(|x| x.tabelle == "bestellung"));
        assert!(v.iter().any(|x| x.tabelle == "kunde"));
        assert!(!v.iter().any(|x| x.tabelle == "top"));
    }

    #[test]
    fn intervall_einheit_ist_keine_spalte() {
        let v = n("SELECT titel FROM aufgabe WHERE faellig < NOW() + INTERVAL 1 DAY");
        assert_eq!(spalten(&v[0]), ["titel", "faellig"]);
    }

    #[test]
    fn datenbank_vor_dem_namen() {
        let v = n("SELECT * FROM shop.kunde");
        assert_eq!(v[0].datenbank.as_deref(), Some("shop"));
        assert!(v[0].spalten.is_empty());
    }

    #[test]
    fn breiter_typ() {
        assert_eq!(breiter("INT", "DECIMAL(10,2)"), "DECIMAL(10,2)");
        assert_eq!(breiter("INT", "VARCHAR(255)"), "VARCHAR(255)");
        assert_eq!(breiter("", "DATE"), "DATE");
        assert_eq!(breiter("DATE", "INT"), "VARCHAR(255)");
    }
}
