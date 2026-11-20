// Ergebnispruefung: eine SELECT-Abfrage lief ohne Fehler und lieferte Zeilen, aber stimmt das Ergebnis?
// Gesucht werden die typischen Denkfehler, bei denen MariaDB still etwas anderes liefert als gemeint:
// zufaellige Werte bei GROUP BY, AND/OR ohne Klammern, ein LEFT JOIN, den WHERE zum normalen JOIN macht,
// BETWEEN mit Datum bei Zeitpunkten, als Text sortierte Zahlen und doppelte Zeilen durch einen JOIN.
// Jede Korrektur wird auf dem Server nachgezaehlt und nur gezeigt, wenn sie wirklich etwas aendert.

use super::leer::{Befund, Zaehler, bedingungen, ersetzt, kurz, top_level, txt, wert};
use super::lint::{Kind, Tok, lex};

/// Teile einer SELECT-Abfrage: Positionen der Schluesselwoerter auf oberster Ebene (Tokenindex)
struct Aufbau {
    chars: Vec<char>,
    toks: Vec<Tok>,
    tiefe: Vec<i32>,
    /// Erstes Token der Spaltenliste (hinter SELECT und DISTINCT usw.)
    liste: usize,
    distinct: bool,
    from: usize,
    wo: Option<usize>,
    gruppe: Option<usize>,
    having: Option<usize>,
    ordnung: Option<usize>,
    limit: Option<usize>,
}

const AGGREGATE: [&str; 12] = ["COUNT", "SUM", "AVG", "MIN", "MAX", "GROUP_CONCAT", "STD", "STDDEV", "VARIANCE", "BIT_AND", "BIT_OR", "JSON_ARRAYAGG"];

impl Aufbau {
    fn lesen(sql: &str) -> Option<Aufbau> {
        let chars: Vec<char> = sql.trim().trim_end_matches(';').trim_end().chars().collect();
        let mut toks = lex(&chars).toks;
        if let Some(k) = toks.iter().position(|t| t.kind == Kind::Semi) {
            toks.truncate(k);
        }
        if !toks.first()?.is_word("SELECT") {
            return None;
        }
        let tiefe = top_level(&toks);
        let oben = |k: usize, w: &str| tiefe[k] == 0 && toks[k].is_word(w);
        if (0..toks.len()).any(|k| oben(k, "UNION") || oben(k, "INTERSECT") || oben(k, "EXCEPT") || oben(k, "INTO")) {
            return None;
        }
        let finde = |w: &str| (1..toks.len()).find(|&k| oben(k, w));
        let from = finde("FROM")?;
        let mut liste = 1;
        let mut distinct = false;
        while liste < from && ["DISTINCT", "ALL", "DISTINCTROW", "STRAIGHT_JOIN", "SQL_CALC_FOUND_ROWS", "SQL_NO_CACHE", "SQL_CACHE", "HIGH_PRIORITY"].iter().any(|w| toks[liste].is_word(w)) {
            distinct |= toks[liste].is_word("DISTINCT") || toks[liste].is_word("DISTINCTROW");
            liste += 1;
        }
        Some(Aufbau {
            wo: finde("WHERE"),
            gruppe: finde("GROUP"),
            having: finde("HAVING"),
            ordnung: finde("ORDER"),
            limit: finde("LIMIT"),
            chars,
            toks,
            tiefe,
            liste,
            distinct,
            from,
        })
    }

    fn sql(&self) -> String {
        self.chars.iter().collect()
    }

    /// Zeichenposition des Tokens k (bzw. Textende)
    fn pos(&self, k: Option<usize>) -> usize {
        k.and_then(|k| self.toks.get(k)).map_or(self.chars.len(), |t| t.start)
    }

    /// Ende eines Abschnitts, der bei Token `ab` beginnt: das naechste der spaeteren Schluesselwoerter
    fn ende(&self, ab: usize) -> usize {
        let naechste = [self.wo, self.gruppe, self.having, self.ordnung, self.limit].into_iter().flatten().filter(|&k| k > ab).min();
        let fenster = (ab + 1..self.toks.len()).find(|&k| self.tiefe[k] == 0 && (self.toks[k].is_word("WINDOW") || self.toks[k].is_word("FOR") || self.toks[k].is_word("LOCK")));
        self.pos([naechste, fenster].into_iter().flatten().min())
    }

    /// Spalten der SELECT-Liste: Zeichenbereich des Ausdrucks (ohne Alias) und ob er ein Aggregat enthaelt
    fn spalten(&self) -> Vec<((usize, usize), bool)> {
        let mut out = Vec::new();
        let mut start = self.liste;
        for k in self.liste..=self.from {
            if k == self.from || (self.tiefe[k] == 0 && self.toks[k].kind == Kind::Comma) {
                if start < k {
                    let mut ende = k;
                    // Alias abschneiden: "ausdruck AS name" bzw. "ausdruck name"
                    if ende - start >= 2 && self.toks[ende - 1].is_name() {
                        if self.toks[ende - 2].is_word("AS") {
                            ende -= 2;
                        } else if matches!(self.toks[ende - 2].kind, Kind::Word | Kind::Quoted | Kind::Close | Kind::Str | Kind::Num) && self.toks[ende - 2].kind != Kind::Dot && !self.toks[ende - 2].is_word("DISTINCT") {
                            ende -= 1;
                        }
                    }
                    let agg = (start..ende).any(|i| AGGREGATE.iter().any(|w| self.toks[i].is_word(w)) && self.toks.get(i + 1).is_some_and(|t| t.kind == Kind::Open));
                    out.push(((self.toks[start].start, self.toks[ende - 1].end), agg));
                }
                start = k + 1;
            }
        }
        out
    }

    fn hat_aggregat(&self) -> bool {
        self.spalten().iter().any(|(_, a)| *a)
    }

    /// Tabellen hinter FROM mit Alias: (Name wie geschrieben, Alias oder Name)
    fn tabellen(&self) -> Vec<(String, String)> {
        let ende = self.ende(self.from);
        let toks: Vec<usize> = (self.from + 1..self.toks.len()).filter(|&k| self.toks[k].start < ende).collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let k = toks[i];
            let nach = k == self.from + 1 || self.toks[k - 1].kind == Kind::Comma || self.toks[k - 1].is_word("JOIN");
            if nach && self.tiefe[k] == 0 && self.toks[k].is_name() {
                let mut name = self.toks[k].text.clone();
                let mut j = i + 1;
                if toks.get(j).is_some_and(|&x| self.toks[x].kind == Kind::Dot) && toks.get(j + 1).is_some_and(|&x| self.toks[x].is_name()) {
                    name = format!("{name}.{}", self.toks[toks[j + 1]].text);
                    j += 2;
                }
                if toks.get(j).is_some_and(|&x| self.toks[x].is_word("AS")) {
                    j += 1;
                }
                let alias = toks
                    .get(j)
                    .map(|&x| &self.toks[x])
                    .filter(|t| t.is_name() && !["ON", "USING", "JOIN", "LEFT", "RIGHT", "INNER", "CROSS", "NATURAL", "STRAIGHT_JOIN", "WHERE", "GROUP", "ORDER", "LIMIT", "HAVING", "FULL", "OUTER"].iter().any(|w| t.is_word(w)))
                    .map(|t| t.text.clone())
                    .unwrap_or_else(|| kurz(&name).to_string());
                out.push((name, alias));
                i = j;
            }
            i += 1;
        }
        out
    }

    /// Tabelle zu einer Spalte "a.b" (Alias aufgeloest) oder "b" (nur bei einer einzigen Tabelle)
    fn tabelle_von(&self, spalte: &str) -> Option<(String, String)> {
        let tabs = self.tabellen();
        match spalte.rsplit_once('.') {
            Some((q, c)) => tabs.iter().find(|(n, a)| a.eq_ignore_ascii_case(q) || kurz(n).eq_ignore_ascii_case(q)).map(|(n, _)| (n.clone(), c.to_string())),
            None if tabs.len() == 1 => Some((tabs[0].0.clone(), spalte.to_string())),
            None => None,
        }
    }
}

fn zaehle(z: &mut dyn Zaehler, sql: &str) -> Option<u64> {
    z.zahl(&format!("SELECT COUNT(*) FROM ({sql}) AS _p"))
}

fn zeilen(n: u64) -> String {
    match n {
        1 => crate::i18n::tr("1 Zeile", "1 row").to_string(),
        n => crate::tr_format!("{} Zeilen", "{} rows", crate::i18n::tausend(n)),
    }
}

/// Spaltenname (a.b oder b) ab Token k als Text
fn spaltenname(toks: &[Tok], k: usize) -> Option<(String, usize)> {
    let t = toks.get(k)?;
    if !t.is_name() || toks.get(k + 1).is_some_and(|t| t.kind == Kind::Open) {
        return None;
    }
    if toks.get(k + 1).is_some_and(|t| t.kind == Kind::Dot) && toks.get(k + 2).is_some_and(Tok::is_name) {
        return Some((format!("{}.{}", t.text, toks[k + 2].text), k + 2));
    }
    Some((t.text.clone(), k))
}

/// Untersucht eine SELECT-Abfrage, die Zeilen geliefert hat. Hoechstens drei Befunde.
pub fn untersuchen(sql: &str, z: &mut dyn Zaehler) -> Vec<Befund> {
    let Some(a) = Aufbau::lesen(sql) else { return Vec::new() };
    let Some(n) = zaehle(z, &a.sql()) else { return Vec::new() };
    let mut out = Vec::new();
    out.extend(gruppe(&a, z));
    out.extend(vorrang(&a, n, z));
    out.extend(links_verbund(&a, n, z));
    out.extend(datum_bis(&a, n, z));
    out.extend(text_sortiert(&a, z));
    if out.is_empty() {
        out.extend(doppelt(&a, n, z));
    }
    out.truncate(3);
    out
}

/// GROUP BY bzw. Aggregat mit Spalten, die weder gruppiert noch zusammengefasst sind: MariaDB nimmt einen
/// beliebigen Wert je Gruppe (nur mit ONLY_FULL_GROUP_BY waere das ein Fehler)
fn gruppe(a: &Aufbau, z: &mut dyn Zaehler) -> Option<Befund> {
    if a.gruppe.is_none() && !a.hat_aggregat() {
        return None;
    }
    let fehler = z.streng(&a.sql())?;
    let tr = crate::i18n::tr;
    match super::errfix::code(&fehler) {
        Some(1055) => {
            let mut sql = a.sql();
            let mut namen: Vec<String> = Vec::new();
            for _ in 0..4 {
                let Some(e) = z.streng(&sql) else { break };
                if super::errfix::code(&e) != Some(1055) {
                    return None;
                }
                let spalte = e.split('\'').nth(1)?.rsplit('.').next()?.to_string();
                let b = Aufbau::lesen(&sql)?;
                // Den Ausdruck aus der SELECT-Liste nehmen (mit Alias der Tabelle), sonst den blossen Namen
                let ausdruck = b
                    .spalten()
                    .into_iter()
                    .filter(|(_, agg)| !agg)
                    .map(|(r, _)| txt(&b.chars, r))
                    .find(|t| t.rsplit('.').next().is_some_and(|x| x.trim_matches('`').eq_ignore_ascii_case(&spalte)))
                    .unwrap_or(spalte);
                if namen.contains(&ausdruck) {
                    return None;
                }
                sql = gruppe_ergaenzen(&b, &ausdruck)?;
                namen.push(ausdruck);
            }
            if namen.is_empty() || z.streng(&sql).is_some() {
                return None;
            }
            let m = zaehle(z, &sql)?;
            let liste = namen.join(", ");
            Some(Befund {
                text: crate::tr_format!(
                    "Für {} nimmt MariaDB aus jeder Gruppe irgendeinen Wert – das Ergebnis ist Zufall. {} gehört in GROUP BY.",
                    "For {} MariaDB takes any value from each group – the result is random. {} belongs in GROUP BY.",
                    liste,
                    liste
                ),
                fix: Some(sql),
                zeilen: Some(m),
            })
        }
        Some(1140) if a.gruppe.is_none() => {
            let einzeln: Vec<String> = a.spalten().into_iter().filter(|(r, agg)| !agg && !txt(&a.chars, *r).starts_with('\'') && txt(&a.chars, *r) != "*").map(|(r, _)| txt(&a.chars, r)).filter(|t| t.parse::<f64>().is_err()).collect();
            if einzeln.is_empty() {
                return None;
            }
            let liste = einzeln.join(", ");
            let ab = a.pos([a.having, a.ordnung, a.limit].into_iter().flatten().min());
            let sql = einfuegen(&a.chars, ab, &format!("GROUP BY {liste}"));
            if z.streng(&sql).is_some() {
                return None;
            }
            let m = zaehle(z, &sql)?;
            if m <= 1 {
                return None;
            }
            Some(Befund {
                text: crate::tr_format!(
                    "Neben dem Zusammenfassen steht {} ohne GROUP BY: es kommt nur eine Zeile mit irgendeinem Wert. {}",
                    "{} stands next to an aggregate without GROUP BY: only one row with an arbitrary value comes back. {}",
                    liste,
                    tr("Je Wert eine Zeile:", "One row per value:")
                ),
                fix: Some(sql),
                zeilen: Some(m),
            })
        }
        _ => None,
    }
}

/// Text an Zeichenposition `pos` einfuegen (mit Leerzeichen davor und danach)
fn einfuegen(chars: &[char], pos: usize, neu: &str) -> String {
    let vor: String = chars[..pos.min(chars.len())].iter().collect();
    let nach: String = chars[pos.min(chars.len())..].iter().collect();
    let nach = nach.trim_start();
    if nach.is_empty() { format!("{} {neu}", vor.trim_end()) } else { format!("{} {neu} {nach}", vor.trim_end()) }
}

/// Ausdruck hinten an GROUP BY anhaengen (vor WITH ROLLUP)
fn gruppe_ergaenzen(a: &Aufbau, ausdruck: &str) -> Option<String> {
    let g = a.gruppe?;
    let mut ende = a.ende(g);
    if let Some(w) = (g..a.toks.len()).find(|&k| a.tiefe[k] == 0 && a.toks[k].is_word("WITH") && a.toks[k].start < ende) {
        ende = a.toks[w].start;
    }
    let vor: String = a.chars[..ende].iter().collect();
    let nach: String = a.chars[ende..].iter().collect();
    let nach = nach.trim_start();
    Some(if nach.is_empty() { format!("{}, {ausdruck}", vor.trim_end()) } else { format!("{}, {ausdruck} {nach}", vor.trim_end()) })
}

/// Bereich der WHERE-Bedingung (Zeichen)
fn wo_bereich(a: &Aufbau) -> Option<(usize, usize)> {
    let w = a.wo?;
    Some((a.toks[w].end, a.ende(w)))
}

/// Teile auf oberster Ebene, getrennt durch ein Wort (OR bzw. AND; AND hinter BETWEEN zaehlt nicht)
pub(super) fn teilen(toks: &[Tok], (a, b): (usize, usize), wort: &str) -> Vec<(usize, usize)> {
    let mut tiefe = 0;
    let mut out = Vec::new();
    let mut start = a;
    let mut between = false;
    for t in toks.iter().filter(|t| t.start >= a && t.end <= b) {
        match t.kind {
            Kind::Open => tiefe += 1,
            Kind::Close => tiefe -= 1,
            _ if tiefe == 0 && t.is_word("BETWEEN") => between = true,
            _ if tiefe == 0 && t.is_word(wort) => {
                if wort == "AND" && between {
                    between = false;
                } else {
                    out.push((start, t.start));
                    start = t.end;
                }
            }
            _ => {}
        }
    }
    out.push((start, b));
    out
}

/// Erste Spalte einer einfachen Bedingung ("ort = 'Wien'" -> ort)
fn bedingungs_spalte(toks: &[Tok], (a, b): (usize, usize)) -> Option<String> {
    let k = toks.iter().position(|t| t.start >= a && t.end <= b)?;
    spaltenname(toks, k).map(|(s, _)| s.rsplit('.').next().unwrap_or(&s).to_ascii_lowercase())
}

/// "a = 1 OR a = 2 AND b = 3": AND bindet staerker, b = 3 gilt nur fuer a = 2. Gemeint ist fast immer
/// "(a = 1 OR a = 2) AND b = 3" (bzw. gespiegelt "a = 1 AND b = 2 OR b = 3" -> "a = 1 AND (b = 2 OR b = 3)").
fn vorrang(a: &Aufbau, n: u64, z: &mut dyn Zaehler) -> Option<Befund> {
    let w = wo_bereich(a)?;
    let (neu, rest, nachbar, spalte) = klammern(&a.chars, &a.toks, w)?;
    let sql = ersetzt(&a.chars, w.0, w.1, &format!(" {neu} "));
    let sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let m = zaehle(z, &sql)?;
    if m == n {
        return None;
    }
    Some(Befund {
        text: crate::tr_format!(
            "AND bindet stärker als OR: „{}“ gilt so nur zusammen mit „{}“, nicht für alle {}-Werte. Mit Klammern kommen {} statt {}.",
            "AND binds tighter than OR: “{}” only applies together with “{}”, not to every {} value. With parentheses: {} instead of {}.",
            rest,
            nachbar,
            spalte,
            zeilen(m),
            zeilen(n)
        ),
        fix: Some(sql),
        zeilen: Some(m),
    })
}

/// Bedingung `w` mit Klammern, wie sie vermutlich gemeint ist: (neue Bedingung, die AND-Bedingung, die
/// Nachbarbedingung, die Spalte der verODERten Bedingungen). Nur fuer den klassischen Fall gleicher Spalten.
pub(super) fn klammern(chars: &[char], toks: &[Tok], w: (usize, usize)) -> Option<(String, String, String, String)> {
    let terme = teilen(toks, w, "OR");
    if terme.len() < 2 {
        return None;
    }
    let teile: Vec<Vec<(usize, usize)>> = terme.iter().map(|t| teilen(toks, *t, "AND")).collect();
    let mit_and: Vec<usize> = (0..teile.len()).filter(|&i| teile[i].len() > 1).collect();
    if mit_and.len() != 1 {
        return None;
    }
    let i = mit_and[0];
    let t = |r: (usize, usize)| txt(chars, r);
    let (neu, nachbar, rest) = if i == terme.len() - 1 {
        // ... OR x AND y  ->  (... OR x) AND y
        let x = teile[i][0];
        let oder: Vec<String> = terme[..i].iter().map(|r| t(*r)).chain([t(x)]).collect();
        let und: Vec<String> = teile[i][1..].iter().map(|r| t(*r)).collect();
        (format!("({}) AND {}", oder.join(" OR "), und.join(" AND ")), (terme[..i].to_vec(), x), und.join(" AND "))
    } else if i == 0 {
        // x AND y OR ...  ->  x AND (y OR ...)
        let y = *teile[0].last()?;
        let und: Vec<String> = teile[0][..teile[0].len() - 1].iter().map(|r| t(*r)).collect();
        let oder: Vec<String> = [t(y)].into_iter().chain(terme[1..].iter().map(|r| t(*r))).collect();
        (format!("{} AND ({})", und.join(" AND "), oder.join(" OR ")), (terme[1..].to_vec(), y), und.join(" AND "))
    } else {
        return None;
    };
    // Nur wenn die verODERten Bedingungen dieselbe Spalte betreffen (der klassische Fall)
    let spalte = bedingungs_spalte(toks, nachbar.1)?;
    if !nachbar.0.iter().all(|r| bedingungs_spalte(toks, *r).as_deref() == Some(spalte.as_str())) {
        return None;
    }
    Some((neu, rest, t(nachbar.1), spalte))
}

/// LEFT JOIN b ... WHERE b.x = ...: die Bedingung wirft alle Zeilen ohne passende b-Zeile weg (dort ist b.x NULL),
/// der LEFT JOIN wirkt wie ein normaler JOIN. Gemeint ist meist die Bedingung im ON.
fn links_verbund(a: &Aufbau, n: u64, z: &mut dyn Zaehler) -> Option<Befund> {
    let w = wo_bereich(a)?;
    let ende_from = a.ende(a.from);
    let lj: Vec<usize> = (a.from..a.toks.len()).filter(|&k| a.tiefe[k] == 0 && a.toks[k].is_word("LEFT") && a.toks[k].start < ende_from).collect();
    if lj.len() != 1 {
        return None;
    }
    let mut k = lj[0] + 1;
    if a.toks.get(k)?.is_word("OUTER") {
        k += 1;
    }
    if !a.toks.get(k)?.is_word("JOIN") {
        return None;
    }
    let (tabelle, mut j) = spaltenname(&a.toks, k + 1)?;
    j += 1;
    if a.toks.get(j).is_some_and(|t| t.is_word("AS")) {
        j += 1;
    }
    let alias = match a.toks.get(j) {
        Some(t) if t.is_name() && !t.is_word("ON") => {
            j += 1;
            t.text.clone()
        }
        _ => kurz(&tabelle).to_string(),
    };
    if !a.toks.get(j)?.is_word("ON") {
        return None;
    }
    // Ende der ON-Bedingung: naechster JOIN-Teil oder Ende von FROM
    let on_ende = (j + 1..a.toks.len())
        .find(|&x| a.toks[x].start >= ende_from || (a.tiefe[x] == 0 && ["JOIN", "LEFT", "RIGHT", "INNER", "CROSS", "NATURAL", "STRAIGHT_JOIN"].iter().any(|w| a.toks[x].is_word(w))) || (a.tiefe[x] == 0 && a.toks[x].kind == Kind::Comma))
        .map_or(ende_from, |x| a.toks[x].start.min(ende_from));
    let teile = bedingungen(&a.toks, w.0, w.1);
    if teile.len() == 1 && teile[0] == w && teilen(&a.toks, w, "OR").len() > 1 {
        return None;
    }
    let betrifft = |r: (usize, usize)| {
        let ts: Vec<&Tok> = a.toks.iter().filter(|t| t.start >= r.0 && t.end <= r.1).collect();
        let rechts = ts.windows(2).any(|p| p[0].is_name() && p[0].text.eq_ignore_ascii_case(&alias) && p[1].kind == Kind::Dot);
        let null = ts.iter().any(|t| t.is_word("NULL") || t.is_word("IFNULL") || t.is_word("COALESCE"));
        rechts && !null
    };
    let idx = teile.iter().position(|r| betrifft(*r))?;
    let bed = txt(&a.chars, teile[idx]);
    let rest: Vec<String> = teile.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, r)| txt(&a.chars, *r)).collect();
    // Neu zusammensetzen: ON ... AND bed, WHERE ohne bed (oder ganz ohne WHERE)
    let mut sql = String::new();
    sql.extend(&a.chars[..on_ende]);
    let sql = format!("{} AND {}", sql.trim_end(), bed);
    let wo_start = a.toks[a.wo?].start;
    let zwischen: String = a.chars[on_ende..wo_start].iter().collect();
    let nach: String = a.chars[w.1..].iter().collect();
    let mut neu = format!("{sql} {}", zwischen.trim());
    if !rest.is_empty() {
        neu = format!("{} WHERE {}", neu.trim_end(), rest.join(" AND "));
    }
    let neu = format!("{} {}", neu.trim_end(), nach.trim()).trim().to_string();
    let m = zaehle(z, &neu)?;
    if m == n {
        return None;
    }
    Some(Befund {
        text: crate::tr_format!(
            "„{}“ im WHERE verwirft alle Zeilen ohne passende {}-Zeile – der LEFT JOIN wirkt so wie ein normaler JOIN. Im ON bleiben sie erhalten: {} statt {}.",
            "“{}” in WHERE drops every row without a matching {} row – the LEFT JOIN acts like a plain JOIN. In ON they are kept: {} instead of {}.",
            bed,
            kurz(&tabelle),
            zeilen(m),
            zeilen(n)
        ),
        fix: Some(neu),
        zeilen: Some(m),
    })
}

/// Datum ohne Uhrzeit ('2024-01-31')
fn ist_datum(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && s.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// "zeit BETWEEN '2024-01-01' AND '2024-01-31'" (bzw. "<= '2024-01-31'") bei einer DATETIME-Spalte endet um 0:00 Uhr
/// des letzten Tages – alles spaeter an diesem Tag fehlt.
fn datum_bis(a: &Aufbau, n: u64, z: &mut dyn Zaehler) -> Option<Befund> {
    let w = wo_bereich(a)?;
    for r in bedingungen(&a.toks, w.0, w.1) {
        let ks: Vec<usize> = (0..a.toks.len()).filter(|&k| a.toks[k].start >= r.0 && a.toks[k].end <= r.1).collect();
        let Some(&k0) = ks.first() else { continue };
        let Some((spalte, ks_ende)) = spaltenname(&a.toks, k0) else { continue };
        let t = |i: usize| a.toks.get(ks_ende + i);
        // (Bereich des Bisher-Ausdrucks, Bis-Datum, neuer Ausdruck)
        let (bereich, bis, neu) = if t(1).is_some_and(|t| t.is_word("BETWEEN")) && t(2).is_some_and(|t| t.kind == Kind::Str) && t(3).is_some_and(|t| t.is_word("AND")) && t(4).is_some_and(|t| t.kind == Kind::Str) {
            let (von, bis) = (wert(&a.chars, t(2)?), wert(&a.chars, t(4)?));
            if !ist_datum(&bis) {
                continue;
            }
            ((a.toks[k0].start, t(4)?.end), bis.clone(), format!("{spalte} >= '{von}' AND {spalte} < '{bis}' + INTERVAL 1 DAY"))
        } else if t(1).is_some_and(|t| t.kind == Kind::Op && t.text == "<=") && t(2).is_some_and(|t| t.kind == Kind::Str) {
            let bis = wert(&a.chars, t(2)?);
            if !ist_datum(&bis) {
                continue;
            }
            ((a.toks[k0].start, t(2)?.end), bis.clone(), format!("{spalte} < '{bis}' + INTERVAL 1 DAY"))
        } else {
            continue;
        };
        let Some((tabelle, c)) = a.tabelle_von(&spalte) else { continue };
        if !z.typ(&tabelle, &c).is_some_and(|t| t == "datetime" || t == "timestamp") {
            continue;
        }
        let sql = ersetzt(&a.chars, bereich.0, bereich.1, &neu);
        let m = zaehle(z, &sql)?;
        if m <= n {
            continue;
        }
        let tag = bis.split('-').rev().collect::<Vec<_>>().join(".");
        return Some(Befund {
            text: crate::tr_format!(
                "{} enthält Uhrzeiten: die Grenze '{}' bedeutet 0:00 Uhr, alles später am {} fehlt. Bis Tagesende kommen {} statt {}.",
                "{} holds times of day: the bound '{}' means 0:00, everything later on {} is missing. Up to the end of the day: {} instead of {}.",
                spalte,
                bis,
                tag,
                zeilen(m),
                zeilen(n)
            ),
            fix: Some(sql),
            zeilen: Some(m),
        });
    }
    None
}

/// ORDER BY auf eine Textspalte, die nur Zahlen enthaelt: sortiert wird wie im Woerterbuch (10 vor 9)
fn text_sortiert(a: &Aufbau, z: &mut dyn Zaehler) -> Option<Befund> {
    let o = a.ordnung?;
    if !a.toks.get(o + 1)?.is_word("BY") {
        return None;
    }
    let k = o + 2;
    let (spalte, letzt) = spaltenname(&a.toks, k)?;
    // Nur eine blosse Spalte (danach Komma, ASC/DESC, LIMIT oder Ende)
    if a.toks.get(letzt + 1).is_some_and(|t| !(t.kind == Kind::Comma || t.is_word("ASC") || t.is_word("DESC") || t.is_word("LIMIT"))) {
        return None;
    }
    // Alias aus der SELECT-Liste? Dann ist es keine Tabellenspalte
    let (tabelle, c) = a.tabelle_von(&spalte)?;
    let typ = z.typ(&tabelle, &c)?;
    if !["char", "varchar", "text", "tinytext", "mediumtext", "longtext"].contains(&typ.as_str()) {
        return None;
    }
    let q = super::leer::qname(&tabelle);
    let col = crate::sqledit::lint::quote_if_needed(&c);
    let keine_zahl = z.zahl(&format!("SELECT COUNT(*) FROM {q} WHERE {col} IS NOT NULL AND TRIM({col}) NOT REGEXP '^-?[0-9]+([.][0-9]+)?$'"))?;
    let laengen = z.zahl(&format!("SELECT COUNT(DISTINCT CHAR_LENGTH(TRIM({col}))) FROM {q} WHERE {col} IS NOT NULL"))?;
    if keine_zahl > 0 || laengen < 2 {
        return None;
    }
    let (s, e) = (a.toks[k].start, a.toks[letzt].end);
    let neu: String = a.chars[s..e].iter().collect();
    let sql = ersetzt(&a.chars, s, e, &format!("CAST({neu} AS DECIMAL(30,10))"));
    Some(Befund {
        text: crate::tr_format!(
            "{} enthält nur Zahlen, ist aber Text: sortiert wird wie im Wörterbuch, 10 kommt vor 9. Als Zahl sortieren:",
            "{} holds only numbers but is text: it sorts like a dictionary, 10 before 9. Sort as numbers:",
            spalte
        ),
        fix: Some(sql),
        zeilen: None,
    })
}

/// Doppelte Zeilen bei einem JOIN ohne DISTINCT/GROUP BY: meist trifft eine Zeile mehrfach
fn doppelt(a: &Aufbau, n: u64, z: &mut dyn Zaehler) -> Option<Befund> {
    if a.distinct || a.gruppe.is_some() || a.hat_aggregat() || n < 2 {
        return None;
    }
    let ende = a.ende(a.from);
    let verbund = (a.from..a.toks.len()).any(|k| a.toks[k].start < ende && a.tiefe[k] == 0 && (a.toks[k].is_word("JOIN") || a.toks[k].kind == Kind::Comma));
    if !verbund {
        return None;
    }
    let sql = a.sql();
    let d = z.zahl(&format!("SELECT COUNT(*) FROM (SELECT DISTINCT * FROM ({sql}) AS _p) AS _q"))?;
    if d >= n {
        return None;
    }
    let pos = a.toks[a.liste - 1].end;
    let neu = einfuegen(&a.chars, pos, "DISTINCT");
    Some(Befund {
        text: crate::tr_format!(
            "{} von {} Zeilen sind doppelt – meist trifft der JOIN eine Zeile mehrfach (etwa ein Kunde mit mehreren Bestellungen). Ohne Doppelte:",
            "{} of {} rows are duplicates – usually the JOIN matches a row several times (say a customer with several orders). Without duplicates:",
            crate::i18n::tausend(n - d),
            crate::i18n::tausend(n)
        ),
        fix: Some(neu),
        zeilen: Some(d),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Antworten nach SQL-Text (enthaelt), fuer streng() und typ() feste Werte
    #[derive(Default)]
    struct Fake {
        zahlen: Vec<(&'static str, u64)>,
        streng: HashMap<String, String>,
        typen: Vec<(&'static str, &'static str)>,
    }

    impl Zaehler for Fake {
        fn zahl(&mut self, sql: &str) -> Option<u64> {
            self.zahlen.iter().find(|(k, _)| sql.contains(k)).map(|(_, v)| *v)
        }
        fn texte(&mut self, _sql: &str) -> Vec<String> {
            Vec::new()
        }
        fn verweise(&mut self, _a: &str, _b: &str) -> Vec<(String, String)> {
            Vec::new()
        }
        fn streng(&mut self, sql: &str) -> Option<String> {
            self.streng.get(sql).cloned()
        }
        fn typ(&mut self, _t: &str, c: &str) -> Option<String> {
            self.typen.iter().find(|(k, _)| *k == c).map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn and_or_precedence() {
        let mut z = Fake { zahlen: vec![("(ort = 'Wien' OR ort = 'Graz') AND alter > 30", 2), ("FROM kunde WHERE", 5)], ..Default::default() };
        let b = untersuchen("SELECT * FROM kunde WHERE ort = 'Wien' OR ort = 'Graz' AND alter > 30", &mut z);
        assert_eq!(b.len(), 1, "{b:?}");
        assert_eq!(b[0].fix.as_deref(), Some("SELECT * FROM kunde WHERE (ort = 'Wien' OR ort = 'Graz') AND alter > 30"));
        assert_eq!(b[0].zeilen, Some(2));
        // Verschiedene Spalten: kein Befund (kann Absicht sein)
        let mut z = Fake { zahlen: vec![("(", 2), ("FROM", 5)], ..Default::default() };
        assert!(untersuchen("SELECT * FROM kunde WHERE ort = 'Wien' OR plz = 1 AND alter > 30", &mut z).is_empty());
    }

    #[test]
    fn group_by_random_values() {
        let mut z = Fake { zahlen: vec![("FROM (", 3)], ..Default::default() };
        z.streng.insert("SELECT k.name, k.ort, COUNT(*) FROM kunde k GROUP BY k.name".into(), "ERROR 1055 (42000): 'shop.k.ort' isn't in GROUP BY".into());
        let b = untersuchen("SELECT k.name, k.ort, COUNT(*) FROM kunde k GROUP BY k.name", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT k.name, k.ort, COUNT(*) FROM kunde k GROUP BY k.name, k.ort"));
        // Aggregat ohne GROUP BY
        let mut z = Fake { zahlen: vec![("FROM (", 4)], ..Default::default() };
        z.streng.insert("SELECT ort, COUNT(*) AS n FROM kunde ORDER BY n".into(), "ERROR 1140 (42000): Mixing of GROUP columns (MIN(),MAX(),COUNT(),...) with no GROUP columns is illegal if there is no GROUP BY clause".into());
        let b = untersuchen("SELECT ort, COUNT(*) AS n FROM kunde ORDER BY n", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT ort, COUNT(*) AS n FROM kunde GROUP BY ort ORDER BY n"));
        assert_eq!(b[0].zeilen, Some(4));
    }

    #[test]
    fn left_join_turned_inner() {
        let mut z = Fake { zahlen: vec![("AND b.jahr = 2024 WHERE", 7), ("FROM (", 3)], ..Default::default() };
        let sql = "SELECT k.name, b.id FROM kunde k LEFT JOIN bestellung b ON b.kunde_id = k.id WHERE b.jahr = 2024 AND k.ort = 'Wien' ORDER BY k.name";
        let b = untersuchen(sql, &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT k.name, b.id FROM kunde k LEFT JOIN bestellung b ON b.kunde_id = k.id AND b.jahr = 2024 WHERE k.ort = 'Wien' ORDER BY k.name"));
        // IS NULL ist der gewollte "ohne"-Fall
        let mut z = Fake { zahlen: vec![("FROM (", 3)], ..Default::default() };
        assert!(untersuchen("SELECT k.name FROM kunde k LEFT JOIN bestellung b ON b.kunde_id = k.id WHERE b.id IS NULL", &mut z).is_empty());
    }

    #[test]
    fn date_bound_on_datetime() {
        let mut z = Fake { zahlen: vec![("INTERVAL 1 DAY", 9), ("FROM (", 6)], typen: vec![("am", "datetime")], ..Default::default() };
        let b = untersuchen("SELECT * FROM besuch WHERE am BETWEEN '2024-01-01' AND '2024-01-31'", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT * FROM besuch WHERE am >= '2024-01-01' AND am < '2024-01-31' + INTERVAL 1 DAY"));
        // DATE-Spalte: alles richtig
        let mut z = Fake { zahlen: vec![("INTERVAL 1 DAY", 9), ("FROM (", 6)], typen: vec![("am", "date")], ..Default::default() };
        assert!(untersuchen("SELECT * FROM besuch WHERE am BETWEEN '2024-01-01' AND '2024-01-31'", &mut z).is_empty());
    }

    #[test]
    fn numbers_sorted_as_text() {
        let mut z = Fake { zahlen: vec![("NOT REGEXP", 0), ("DISTINCT CHAR_LENGTH", 2), ("FROM (", 6)], typen: vec![("nr", "varchar")], ..Default::default() };
        let b = untersuchen("SELECT * FROM zimmer ORDER BY nr DESC", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT * FROM zimmer ORDER BY CAST(nr AS DECIMAL(30,10)) DESC"));
    }

    #[test]
    fn duplicates_from_join() {
        let mut z = Fake { zahlen: vec![("SELECT DISTINCT *", 3), ("FROM (", 8)], ..Default::default() };
        let b = untersuchen("SELECT k.name FROM kunde k JOIN bestellung b ON b.kunde_id = k.id", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT DISTINCT k.name FROM kunde k JOIN bestellung b ON b.kunde_id = k.id"));
        assert_eq!(b[0].zeilen, Some(3));
        // Ohne JOIN nichts
        assert!(untersuchen("SELECT name FROM kunde", &mut z).is_empty());
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;
    use crate::sqledit::leer::Server;
    use mysql::prelude::Queryable;

    /// Gegen einen echten Testserver: `EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test pruef_echt -- --ignored`
    #[test]
    #[ignore]
    fn pruef_echt() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("pruef_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(
            &name,
            "CREATE TABLE kunde (id INT PRIMARY KEY, name VARCHAR(50), ort VARCHAR(30), alter_j INT, zimmer VARCHAR(5));
             INSERT INTO kunde VALUES (1, 'Anna', 'Wien', 25, '9'), (2, 'Ben', 'Graz', 41, '10'), (3, 'Clara', 'Wien', 52, '2'), (4, 'David', 'Linz', 35, '11'), (5, 'Emma', 'Graz', 19, '1');
             CREATE TABLE bestellung (id INT PRIMARY KEY, kunde_id INT, am DATETIME, jahr INT);
             INSERT INTO bestellung VALUES (10, 1, '2024-01-31 09:30:00', 2024), (11, 1, '2024-01-15 14:00:00', 2024), (12, 3, '2023-05-02 10:00:00', 2023), (13, 2, '2024-01-31 00:00:00', 2024);",
        )
        .unwrap();
        let mut conn = dbc.new_conn().unwrap();
        conn.query_drop(format!("USE {name}")).unwrap();
        let faelle = [
            ("SELECT * FROM kunde WHERE ort = 'Wien' OR ort = 'Graz' AND alter_j > 30", "(ort = 'Wien' OR ort = 'Graz') AND"),
            ("SELECT ort, name, COUNT(*) FROM kunde GROUP BY ort", "GROUP BY ort, name"),
            ("SELECT ort, COUNT(*) FROM kunde", "GROUP BY ort"),
            ("SELECT k.name, b.id FROM kunde k LEFT JOIN bestellung b ON b.kunde_id = k.id WHERE b.jahr = 2024", "AND b.jahr = 2024"),
            ("SELECT * FROM bestellung WHERE am BETWEEN '2024-01-01' AND '2024-01-31'", "INTERVAL 1 DAY"),
            ("SELECT * FROM kunde ORDER BY zimmer", "CAST(zimmer AS DECIMAL"),
            ("SELECT k.name FROM kunde k JOIN bestellung b ON b.kunde_id = k.id", "SELECT DISTINCT"),
        ];
        let mut fehler = Vec::new();
        for (sql, erwartet) in faelle {
            let mut z = Server { conn: &mut conn, mariadb: true, rest: 40 };
            let b = untersuchen(sql, &mut z);
            // Jede Korrektur muss laufen
            let ok = b.iter().any(|b| b.fix.as_deref().is_some_and(|f| f.contains(erwartet) && conn.query_drop(f).is_ok()));
            eprintln!("{} {sql}\n    {b:?}", if ok { "OK  " } else { "FAIL" });
            if !ok {
                fehler.push(sql);
            }
        }
        // Richtige Abfragen bleiben ohne Befund
        for sql in ["SELECT * FROM kunde WHERE ort = 'Wien'", "SELECT ort, COUNT(*) FROM kunde GROUP BY ort", "SELECT * FROM kunde ORDER BY alter_j", "SELECT DISTINCT k.name FROM kunde k JOIN bestellung b ON b.kunde_id = k.id"] {
            let mut z = Server { conn: &mut conn, mariadb: true, rest: 40 };
            let b = untersuchen(sql, &mut z);
            if !b.is_empty() {
                eprintln!("FAIL (falscher Alarm) {sql}\n    {b:?}");
                fehler.push(sql);
            }
        }
        dbc.exec(&format!("DROP DATABASE {name}"), ()).unwrap();
        assert!(fehler.is_empty(), "{fehler:?}");
    }
}
