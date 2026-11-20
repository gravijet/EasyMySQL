// Leeres Ergebnis erklaeren und reparieren: liefert ein SELECT keine Zeilen (oder trifft ein UPDATE/DELETE nichts),
// wird im Hintergrund nur lesend nachgezaehlt, welche Bedingung alles wegfiltert, und eine Korrektur gesucht, die
// wirklich Zeilen liefert: Schreibweise wie in den Daten ('wien' -> 'Wien', 'Mueller' -> 'Müller', Tippfehler),
// deutsches Datum, Datum gegen DATETIME-Spalte, = NULL, LIKE ohne Platzhalter oder mit * und ?, falsche JOIN-Bedingung.

use super::lint::{Kind, Tok, lex};
use mysql::prelude::Queryable;

/// Ein Grund fuer das leere Ergebnis, eventuell mit gepruefter Korrektur
#[derive(Clone, Debug, PartialEq)]
pub struct Befund {
    pub text: String,
    /// Ganze korrigierte Anweisung
    pub fix: Option<String>,
    /// So viele Zeilen liefert (bzw. trifft) die Korrektur
    pub zeilen: Option<u64>,
}

/// Zaehlt auf dem Server; jede Abfrage hoechstens wenige Sekunden, insgesamt nur eine begrenzte Zahl
pub trait Zaehler {
    fn zahl(&mut self, sql: &str) -> Option<u64>;
    fn texte(&mut self, sql: &str) -> Vec<String>;
    /// Fremdschluessel zwischen zwei Tabellen: (Spalte in a, Spalte in b)
    fn verweise(&mut self, a: &str, b: &str) -> Vec<(String, String)>;
    /// Spalten einer Tabelle der gewaehlten Datenbank
    fn spalten(&mut self, _t: &str) -> Vec<String> {
        Vec::new()
    }
}

pub struct Server<'a, C: Queryable> {
    pub conn: &'a mut C,
    /// MariaDB kennt SET STATEMENT max_statement_time (MySQL nicht)
    pub mariadb: bool,
    pub rest: usize,
}

impl<C: Queryable> Server<'_, C> {
    fn begrenzt(&self, sql: &str) -> String {
        if self.mariadb { format!("SET STATEMENT max_statement_time=3 FOR {sql}") } else { sql.to_string() }
    }
}

impl<C: Queryable> Zaehler for Server<'_, C> {
    fn zahl(&mut self, sql: &str) -> Option<u64> {
        if self.rest == 0 {
            return None;
        }
        self.rest -= 1;
        let q = self.begrenzt(sql);
        self.conn.query_first::<Option<u64>, _>(q).ok().flatten().flatten()
    }

    fn texte(&mut self, sql: &str) -> Vec<String> {
        if self.rest == 0 {
            return Vec::new();
        }
        self.rest -= 1;
        let q = self.begrenzt(sql);
        self.conn.query::<Option<String>, _>(q).map(|v| v.into_iter().flatten().collect()).unwrap_or_default()
    }

    fn verweise(&mut self, a: &str, b: &str) -> Vec<(String, String)> {
        let lit = |s: &str| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"));
        // "db.tabelle": in dieser Datenbank suchen
        let schema = a.split_once('.').map_or("DATABASE()".to_string(), |(d, _)| lit(d));
        let (a, b) = (kurz(a), kurz(b));
        let q = format!(
            "SELECT COLUMN_NAME, REFERENCED_COLUMN_NAME, TABLE_NAME FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = {schema} AND REFERENCED_TABLE_NAME IS NOT NULL AND \
             ((TABLE_NAME = {a} AND REFERENCED_TABLE_NAME = {b}) OR (TABLE_NAME = {b} AND REFERENCED_TABLE_NAME = {a}))",
            a = lit(a),
            b = lit(b)
        );
        let rows: Vec<(String, String, String)> = self.conn.query(q).unwrap_or_default();
        rows.into_iter().map(|(c, r, t)| if t.eq_ignore_ascii_case(a) { (c, r) } else { (r, c) }).collect()
    }

    fn spalten(&mut self, t: &str) -> Vec<String> {
        let db = t.split_once('.').map(|(d, _)| d.to_string());
        self.conn.exec("SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION", (db, kurz(t))).unwrap_or_default()
    }
}

/// Tabellenname ohne Datenbank ("shop.kunde" -> "kunde")
fn kurz(t: &str) -> &str {
    t.rsplit('.').next().unwrap_or(t)
}

/// Name fuer SQL, auch mit Datenbank ("shop.kunde" -> shop.kunde, Teile einzeln in Backticks falls noetig)
fn qname(t: &str) -> String {
    t.split('.').map(crate::sqledit::lint::quote_if_needed).collect::<Vec<_>>().join(".")
}

/// Teile der Anweisung (Zeichenbereiche)
struct Teile {
    /// Inhalt hinter FROM bzw. die Tabelle von UPDATE/DELETE
    from: (usize, usize),
    /// Inhalt hinter WHERE
    wo: Option<(usize, usize)>,
    /// Bereich "LIMIT 0"
    limit0: Option<(usize, usize)>,
    schreibt: bool,
}

fn top_level(toks: &[Tok]) -> Vec<i32> {
    let mut d = 0;
    toks.iter()
        .map(|t| {
            let here = d;
            match t.kind {
                Kind::Open => d += 1,
                Kind::Close => d -= 1,
                _ => {}
            }
            if t.kind == Kind::Close { d } else { here }
        })
        .collect()
}

fn teile(chars: &[char], toks: &[Tok]) -> Option<Teile> {
    let depth = top_level(toks);
    let first = toks.first()?;
    let find = |from: usize, words: &[&str]| (from..toks.len()).find(|&k| depth[k] == 0 && (words.iter().any(|w| toks[k].is_word(w)) || toks[k].kind == Kind::Semi));
    let end_of = |k: usize| toks.get(k).map_or(chars.len(), |t| t.start);
    if (0..toks.len()).any(|k| depth[k] == 0 && (toks[k].is_word("UNION") || toks[k].is_word("INTERSECT") || toks[k].is_word("EXCEPT"))) {
        return None;
    }
    let (from_k, from_end, schreibt) = if first.is_word("SELECT") {
        let f = (1..toks.len()).find(|&k| depth[k] == 0 && toks[k].is_word("FROM"))?;
        let e = find(f + 1, &["WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "WINDOW", "FOR", "INTO", "LOCK"]);
        (f, e, false)
    } else if first.is_word("UPDATE") {
        let e = find(1, &["SET"])?;
        (0, Some(e), true)
    } else if first.is_word("DELETE") {
        let f = (1..toks.len()).find(|&k| depth[k] == 0 && toks[k].is_word("FROM"))?;
        // DELETE t1 FROM t1 JOIN … und DELETE … USING: zu verschachtelt
        if f != 1 || (0..toks.len()).any(|k| toks[k].is_word("USING")) {
            return None;
        }
        (f, find(f + 1, &["WHERE", "ORDER", "LIMIT"]), true)
    } else {
        return None;
    };
    let from = (toks[from_k].end, end_of(from_end.unwrap_or(toks.len())));
    let w = (from_k..toks.len()).find(|&k| depth[k] == 0 && toks[k].is_word("WHERE"));
    let wo = w.map(|w| (toks[w].end, end_of(find(w + 1, &["GROUP", "HAVING", "ORDER", "LIMIT", "WINDOW", "FOR", "INTO", "LOCK"]).unwrap_or(toks.len()))));
    let limit0 = (0..toks.len().saturating_sub(1)).find(|&k| depth[k] == 0 && toks[k].is_word("LIMIT") && toks[k + 1].kind == Kind::Num && chars[toks[k + 1].start..toks[k + 1].end].iter().collect::<String>() == "0").map(|k| (toks[k].start, toks[k + 1].end));
    Some(Teile { from, wo, limit0, schreibt })
}

/// Bedingungen, die mit AND verknuepft sind (Zeichenbereiche); bei OR auf oberster Ebene nur das Ganze
fn bedingungen(toks: &[Tok], a: usize, b: usize) -> Vec<(usize, usize)> {
    let inner: Vec<&Tok> = toks.iter().filter(|t| t.start >= a && t.end <= b).collect();
    let mut depth = 0;
    let mut out = Vec::new();
    let mut start = a;
    let mut between = false;
    let mut oder = false;
    for t in &inner {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ if depth == 0 && t.is_word("BETWEEN") => between = true,
            _ if depth == 0 && (t.is_word("OR") || t.is_word("XOR") || (t.kind == Kind::Op && t.text == "||")) => oder = true,
            _ if depth == 0 && (t.is_word("AND") || (t.kind == Kind::Op && t.text == "&&")) => {
                if between {
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
    if oder {
        return vec![(a, b)];
    }
    out
}

fn txt(chars: &[char], (a, b): (usize, usize)) -> String {
    chars[a.min(chars.len())..b.min(chars.len())].iter().collect::<String>().trim().to_string()
}

/// Wert eines Textes in SQL
fn literal(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

/// Inhalt eines '…'-Textes
fn wert(chars: &[char], t: &Tok) -> String {
    let inner = &chars[(t.start + 1).min(t.end)..t.end.saturating_sub(1).max(t.start + 1)];
    let mut out = String::new();
    let mut i = 0;
    while i < inner.len() {
        let c = inner[i];
        if c == '\\' && i + 1 < inner.len() {
            out.push(inner[i + 1]);
            i += 2;
        } else if (c == '\'' || c == '"') && inner.get(i + 1) == Some(&c) {
            out.push(c);
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Bereich a..b (mit dem Leerraum davor) durch eine Bedingung ersetzen; die ganze Anweisung, ohne Rand
fn einsetzen(chars: &[char], a: usize, b: usize, neu: &str) -> String {
    let pad = if b < chars.len() && !chars[b].is_whitespace() { " " } else { "" };
    ersetzt(chars, a, b, &format!(" {neu}{pad}")).trim().to_string()
}

fn ersetzt(chars: &[char], a: usize, b: usize, neu: &str) -> String {
    let mut v: Vec<char> = chars.to_vec();
    v.splice(a.min(v.len())..b.min(v.len()), neu.chars());
    v.into_iter().collect::<String>()
}

/// Vergleichbare Form: klein, ohne Leerraum an den Raendern, Umlaute ausgeschrieben
fn grob(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            'é' | 'è' | 'ê' => out.push('e'),
            'á' | 'à' | 'â' => out.push('a'),
            c if c.is_whitespace() => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn abstand(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut prev2 = prev.clone();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
            // Vertauschte Nachbarn ("wein" statt "wien") zaehlen als ein Fehler
            if i > 0 && j > 0 && *ca == b[j - 1] && a[i - 1] == *cb {
                cur[j + 1] = cur[j + 1].min(prev2[j - 1] + 1);
            }
        }
        prev2 = std::mem::replace(&mut prev, cur);
    }
    prev[b.len()]
}

/// Werte aus den Daten, die wohl gemeint sind (bester zuerst)
fn gemeint(gesucht: &str, werte: &[String]) -> Vec<String> {
    let g = grob(gesucht);
    if g.is_empty() {
        return Vec::new();
    }
    let mut treffer: Vec<(usize, usize, &String)> = Vec::new();
    for w in werte {
        if w == gesucht {
            continue;
        }
        let v = grob(w);
        let rang = if v == g {
            Some((0, 0))
        } else {
            let d = abstand(&v, &g);
            let erlaubt = (g.chars().count() / 4).clamp(1, 3);
            if d <= erlaubt {
                Some((1, d))
            } else if g.chars().count() >= 3 && v.starts_with(&g) {
                Some((2, v.len() - g.len()))
            } else if g.chars().count() >= 4 && v.contains(&g) {
                Some((3, v.len() - g.len()))
            } else {
                None
            }
        };
        if let Some((r, d)) = rang {
            treffer.push((r, d, w));
        }
    }
    treffer.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    treffer.into_iter().map(|t| t.2.clone()).take(3).collect()
}

/// Name einer Spalte (a.b oder b) ab Token k: Tokenbereich
fn spalte(toks: &[Tok], k: usize) -> Option<(usize, usize)> {
    let t = toks.get(k)?;
    if !t.is_name() || toks.get(k + 1).is_some_and(|t| t.kind == Kind::Open) {
        return None;
    }
    if toks.get(k + 1).is_some_and(|t| t.kind == Kind::Dot) && toks.get(k + 2).is_some_and(Tok::is_name) {
        return Some((k, k + 2));
    }
    Some((k, k))
}

/// Untersucht eine Anweisung, die nichts geliefert bzw. nichts getroffen hat
pub fn untersuchen(sql: &str, z: &mut dyn Zaehler) -> Vec<Befund> {
    let sql = sql.trim().trim_end_matches(';').trim_end();
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let Some(t) = teile(&chars, &toks) else { return Vec::new() };
    let mut out = Vec::new();
    let tr = crate::i18n::tr;
    if let Some((a, b)) = t.limit0 {
        let fix = ersetzt(&chars, a, b, "").trim_end().to_string();
        out.push(Befund { text: tr("LIMIT 0 liefert nie Zeilen.", "LIMIT 0 never returns rows.").into(), fix: Some(fix), zeilen: None });
        return out;
    }
    let from = txt(&chars, t.from);
    let alle = z.zahl(&format!("SELECT COUNT(*) FROM {from}"));
    if alle == Some(0) {
        out.extend(leere_quelle(&chars, &toks, t.from, z));
        return out;
    }
    let Some((wa, wb)) = t.wo else {
        return out;
    };
    let teile_wo = bedingungen(&toks, wa, wb);
    let mut schuld = Vec::new();
    for &(a, b) in &teile_wo {
        let c = txt(&chars, (a, b));
        if c.is_empty() {
            continue;
        }
        if z.zahl(&format!("SELECT COUNT(*) FROM {from} WHERE {c}")) == Some(0) {
            schuld.push((a, b));
        }
    }
    let ziel = |n: u64| {
        let z = crate::i18n::tausend(n);
        match (t.schreibt, n == 1) {
            (true, true) => crate::i18n::tr("Damit ist 1 Zeile betroffen.", "That affects 1 row.").to_string(),
            (true, false) => crate::tr_format!("Damit sind {z} Zeilen betroffen.", "That affects {z} rows."),
            (false, true) => crate::i18n::tr("Damit kommt 1 Zeile.", "That returns 1 row.").to_string(),
            (false, false) => crate::tr_format!("Damit kommen {z} Zeilen.", "That returns {z} rows."),
        }
    };
    for &(a, b) in &schuld {
        let c = txt(&chars, (a, b));
        let vorschlaege = korrekturen(&chars, &toks, a, b, &from, z);
        let mut erklaert = false;
        for (neu, warum) in vorschlaege {
            let fix = einsetzen(&chars, a, b, &neu);
            let teile2 = {
                let c2: Vec<char> = fix.chars().collect();
                let t2 = lex(&c2).toks;
                teile(&c2, &t2).and_then(|t| t.wo.map(|w| txt(&c2, w)))
            };
            let Some(wo2) = teile2 else { continue };
            let n = z.zahl(&format!("SELECT COUNT(*) FROM {from} WHERE {wo2}"));
            if let Some(n) = n.filter(|n| *n > 0) {
                out.push(Befund { text: format!("{warum}. {}", ziel(n)), fix: Some(fix), zeilen: Some(n) });
                erklaert = true;
                break;
            }
        }
        if !erklaert {
            out.push(Befund { text: erklaerung(&chars, &toks, a, b, &from, z).unwrap_or_else(|| crate::tr_format!("Keine Zeile erfüllt „{c}“.", "No row matches “{c}”.")), fix: None, zeilen: None });
        }
    }
    if schuld.is_empty() && teile_wo.len() > 1 {
        // Jede Bedingung allein trifft etwas, zusammen nichts: zwei schliessen sich aus
        let texte: Vec<String> = teile_wo.iter().map(|r| txt(&chars, *r)).collect();
        'paare: for i in 0..texte.len() {
            for j in i + 1..texte.len() {
                if z.zahl(&format!("SELECT COUNT(*) FROM {from} WHERE ({}) AND ({})", texte[i], texte[j])) == Some(0) {
                    // Gleiche Spalte mit zwei Werten: gemeint ist meist OR
                    let gleich = gleiche_spalte(&texte[i], &texte[j]);
                    let (fix, n) = if gleich {
                        let (a, _) = teile_wo[i];
                        let (_, b) = teile_wo[j];
                        let between: String = chars[teile_wo[i].1..teile_wo[j].0].iter().collect();
                        if between.trim().eq_ignore_ascii_case("AND") && j == i + 1 {
                            let fix = einsetzen(&chars, a, b, &format!("({} OR {})", texte[i], texte[j]));
                            let c2: Vec<char> = fix.chars().collect();
                            let t2 = lex(&c2).toks;
                            let n = teile(&c2, &t2).and_then(|t| t.wo.map(|w| txt(&c2, w))).and_then(|w| z.zahl(&format!("SELECT COUNT(*) FROM {from} WHERE {w}")));
                            (Some(fix), n)
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    };
                    let text = if gleich {
                        crate::tr_format!("„{}“ und „{}“ können nie zugleich gelten – gemeint ist wohl ODER", "“{}” and “{}” can never both hold – you probably mean OR", texte[i], texte[j])
                    } else {
                        crate::tr_format!("Jede Bedingung trifft Zeilen, aber keine Zeile erfüllt „{}“ und „{}“ zugleich.", "Each condition matches rows, but none matches both “{}” and “{}”.", texte[i], texte[j])
                    };
                    let fix = fix.filter(|_| n.is_some_and(|n| n > 0));
                    out.push(Befund { text: match n.filter(|_| fix.is_some()) { Some(n) => format!("{text}. {}", ziel(n)), None => text }, fix, zeilen: n });
                    break 'paare;
                }
            }
        }
    }
    out
}

/// Zwei Bedingungen wie a = 1 und a = 2 auf dieselbe Spalte
fn gleiche_spalte(x: &str, y: &str) -> bool {
    let kopf = |s: &str| s.split(['=', ' ']).next().unwrap_or("").trim().to_ascii_lowercase();
    let (a, b) = (kopf(x), kopf(y));
    !a.is_empty() && a == b && x.contains('=') && y.contains('=') && !x.contains("<") && !y.contains(">")
}

/// Moegliche Korrekturen einer Bedingung (neuer Text, Begruendung)
fn korrekturen(chars: &[char], toks: &[Tok], a: usize, b: usize, from: &str, z: &mut dyn Zaehler) -> Vec<(String, String)> {
    let idx: Vec<usize> = (0..toks.len()).filter(|&k| toks[k].start >= a && toks[k].end <= b).collect();
    let mut out = Vec::new();
    let Some(&k0) = idx.first() else { return out };
    let kn = *idx.last().unwrap_or(&k0);
    let col_txt = |s: usize, e: usize| -> String { chars[toks[s].start..toks[e].end].iter().collect() };
    // Spalte am Anfang, dann Operator, dann Wert(e)
    let Some((cs, ce)) = spalte(toks, k0) else { return out };
    if ce >= kn {
        return out;
    }
    let col = col_txt(cs, ce);
    let op = &toks[ce + 1];
    let rest: Vec<&Tok> = toks[ce + 2..=kn].iter().collect();
    let op_text = op.text.to_ascii_uppercase();
    // = NULL
    if op.kind == Kind::Op && rest.len() == 1 && rest[0].is_word("NULL") {
        match op.text.as_str() {
            "=" => out.push((format!("{col} IS NULL"), crate::i18n::tr("Mit = NULL findet SQL nie etwas, richtig ist IS NULL", "= NULL never matches, use IS NULL").to_string())),
            "!=" | "<>" => out.push((format!("{col} IS NOT NULL"), crate::i18n::tr("Mit <> NULL findet SQL nie etwas, richtig ist IS NOT NULL", "<> NULL never matches, use IS NOT NULL").to_string())),
            _ => {}
        }
        return out;
    }
    let einzelner_text = rest.len() == 1 && rest[0].kind == Kind::Str;
    let werte_der_spalte = |z: &mut dyn Zaehler| z.texte(&format!("SELECT DISTINCT CAST({col} AS CHAR) FROM {from} WHERE {col} IS NOT NULL LIMIT 5000"));
    if op.kind == Kind::Op && op.text == "=" && einzelner_text {
        let lit = wert(chars, rest[0]);
        // Deutsches Datum
        if let Some(iso) = super::dialect::german_date(&lit) {
            out.push((format!("{col} = {}", literal(&iso)), crate::tr_format!("Datum im Format JJJJ-MM-TT: {iso}", "Date in YYYY-MM-DD format: {iso}")));
        }
        let iso = super::dialect::german_date(&lit).unwrap_or(lit.clone());
        if iso.len() == 10 && iso.chars().filter(|c| *c == '-').count() == 2 {
            out.push((format!("DATE({col}) = {}", literal(&iso)), crate::i18n::tr("Die Spalte enthält auch eine Uhrzeit, verglichen wird nur das Datum", "The column also holds a time, only the date is compared").to_string()));
        }
        for w in gemeint(&lit, &werte_der_spalte(z)) {
            out.push((format!("{col} = {}", literal(&w)), crate::tr_format!("In den Daten steht „{w}“, nicht „{lit}“", "The data says “{w}”, not “{lit}”")));
        }
    } else if op_text == "LIKE" && einzelner_text {
        let lit = wert(chars, rest[0]);
        if lit.contains('*') || lit.contains('?') {
            let neu = lit.replace('*', "%").replace('?', "_");
            out.push((format!("{col} LIKE {}", literal(&neu)), crate::i18n::tr("In SQL heißen die Platzhalter % und _ (nicht * und ?)", "SQL wildcards are % and _ (not * and ?)").to_string()));
        }
        if !lit.contains('%') && !lit.contains('_') {
            out.push((format!("{col} LIKE {}", literal(&format!("%{lit}%"))), crate::i18n::tr("LIKE ohne % sucht nur genau diesen Text; mit % irgendwo darin", "LIKE without % only finds this exact text; with % anywhere inside").to_string()));
        }
        let ohne: String = lit.chars().filter(|c| *c != '%' && *c != '_').collect();
        for w in gemeint(&ohne, &werte_der_spalte(z)) {
            let neu = lit.replacen(&ohne, &w, 1);
            if neu != lit {
                out.push((format!("{col} LIKE {}", literal(&neu)), crate::tr_format!("In den Daten steht „{w}“", "The data says “{w}”")));
            }
        }
    } else if op_text == "IN" && rest.first().is_some_and(|t| t.kind == Kind::Open) {
        let texte: Vec<&Tok> = rest.iter().copied().filter(|t| t.kind == Kind::Str).collect();
        if !texte.is_empty() {
            let werte = werte_der_spalte(z);
            let mut neu = Vec::new();
            let mut geaendert = Vec::new();
            for t in &texte {
                let l = wert(chars, t);
                if werte.contains(&l) {
                    neu.push(literal(&l));
                } else if let Some(w) = gemeint(&l, &werte).into_iter().next() {
                    geaendert.push(format!("„{l}“ → „{w}“"));
                    neu.push(literal(&w));
                } else {
                    neu.push(literal(&l));
                }
            }
            if !geaendert.is_empty() {
                out.push((format!("{col} IN ({})", neu.join(", ")), crate::tr_format!("Schreibweise wie in den Daten: {}", "Spelling as in the data: {}", geaendert.join(", "))));
            }
        }
    }
    out
}

/// Warum eine Bedingung nichts trifft, wenn es keine Korrektur gibt (z. B. Zahlenbereich)
fn erklaerung(chars: &[char], toks: &[Tok], a: usize, b: usize, from: &str, z: &mut dyn Zaehler) -> Option<String> {
    let k0 = (0..toks.len()).find(|&k| toks[k].start >= a)?;
    let (cs, ce) = spalte(toks, k0)?;
    let col: String = chars[toks[cs].start..toks[ce].end].iter().collect();
    let op = toks.get(ce + 1)?;
    let wert_tok = toks.get(ce + 2).filter(|t| t.end <= b)?;
    let c: String = chars[a..b].iter().collect::<String>().trim().to_string();
    // Text gesucht, den es nicht gibt: die vorhandenen Werte nennen, wenn es wenige sind
    if op.kind == Kind::Op && op.text == "=" && wert_tok.kind == Kind::Str {
        let werte = z.texte(&format!("SELECT DISTINCT CAST({col} AS CHAR) FROM {from} WHERE {col} IS NOT NULL ORDER BY 1 LIMIT 9"));
        if werte.is_empty() || werte.len() > 8 {
            return None;
        }
        let liste = werte.iter().map(|w| format!("„{w}“")).collect::<Vec<_>>().join(", ");
        return Some(crate::tr_format!("Keine Zeile erfüllt „{c}“. In {col} gibt es nur {liste}.", "No row matches “{c}”. {col} only holds {liste}."));
    }
    let vergleich = (op.kind == Kind::Op && ["<", ">", "<=", ">=", "="].contains(&op.text.as_str())) || op.is_word("BETWEEN");
    if !vergleich {
        return None;
    }
    let werte = z.texte(&format!("SELECT CONCAT(CAST(MIN({col}) AS CHAR), ' … ', CAST(MAX({col}) AS CHAR)) FROM {from}"));
    let spanne = werte.into_iter().next()?;
    Some(crate::tr_format!("Keine Zeile erfüllt „{c}“: {col} liegt in den Daten zwischen {spanne}.", "No row matches “{c}”: {col} ranges from {spanne} in the data."))
}

/// FROM liefert schon nichts: leere Tabelle oder JOIN-Bedingung, die nie passt
fn leere_quelle(chars: &[char], toks: &[Tok], (fa, fb): (usize, usize), z: &mut dyn Zaehler) -> Vec<Befund> {
    let mut out = Vec::new();
    let idx: Vec<usize> = (0..toks.len()).filter(|&k| toks[k].start >= fa && toks[k].end <= fb).collect();
    // Tabellen: Name nach FROM, Komma oder JOIN, mit Alias
    let mut tabellen: Vec<(String, String)> = Vec::new();
    let mut ons: Vec<(usize, usize)> = Vec::new();
    let mut k = 0;
    let mut erwarte = true;
    while k < idx.len() {
        let t = &toks[idx[k]];
        if erwarte && t.is_name() && !t.is_word("SELECT") {
            let mut name = t.text.clone();
            if idx.get(k + 2).is_some() && toks[idx[k + 1]].kind == Kind::Dot {
                name = toks[idx[k + 2]].text.clone();
                k += 2;
            }
            let mut alias = name.clone();
            if let Some(&n) = idx.get(k + 1) {
                let nt = &toks[n];
                if nt.is_word("AS") {
                    if let Some(&m) = idx.get(k + 2) {
                        alias = toks[m].text.clone();
                        k += 2;
                    }
                } else if nt.is_name() && !["JOIN", "INNER", "LEFT", "RIGHT", "CROSS", "STRAIGHT_JOIN", "NATURAL", "ON", "USING"].iter().any(|w| nt.is_word(w)) {
                    alias = nt.text.clone();
                    k += 1;
                }
            }
            tabellen.push((name, alias));
            erwarte = false;
        } else if t.kind == Kind::Comma || t.is_word("JOIN") || t.is_word("STRAIGHT_JOIN") {
            erwarte = true;
        } else if t.is_word("ON") {
            // Bis zum naechsten JOIN auf oberster Ebene
            let start = t.end;
            let mut d = 0;
            let mut e = k + 1;
            while e < idx.len() {
                let u = &toks[idx[e]];
                match u.kind {
                    Kind::Open => d += 1,
                    Kind::Close => d -= 1,
                    _ => {}
                }
                if d == 0 && ["JOIN", "INNER", "LEFT", "RIGHT", "CROSS", "NATURAL", "STRAIGHT_JOIN"].iter().any(|w| u.is_word(w)) || (d == 0 && u.kind == Kind::Comma) {
                    break;
                }
                e += 1;
            }
            let end = idx.get(e).map_or(fb, |&i| toks[i].start);
            ons.push((start, end));
            k = e;
            continue;
        } else if t.kind == Kind::Open {
            // Unterabfrage als Tabelle: nicht weiter zerlegen
            return out;
        }
        k += 1;
    }
    let mut leer = Vec::new();
    for (name, _) in &tabellen {
        if z.zahl(&format!("SELECT COUNT(*) FROM {}", crate::sqledit::lint::quote_if_needed(name))) == Some(0) {
            leer.push(name.clone());
        }
    }
    if !leer.is_empty() {
        for n in leer {
            out.push(Befund { text: crate::tr_format!("Die Tabelle „{n}“ ist leer.", "The table “{n}” is empty."), fix: None, zeilen: None });
        }
        return out;
    }
    // Jede JOIN-Bedingung: passt sie nie, die Bedingung ueber den Fremdschluessel versuchen
    for &(a, b) in &ons {
        let cond = txt(chars, (a, b));
        let beteiligt: Vec<&(String, String)> = tabellen.iter().filter(|(_, al)| cond.to_lowercase().contains(&format!("{}.", al.to_lowercase()))).collect();
        if beteiligt.len() != 2 {
            continue;
        }
        let (t1, a1) = beteiligt[0];
        let (t2, a2) = beteiligt[1];
        for (c1, c2) in z.verweise(t1, t2) {
            let neu = format!("{a1}.{c1} = {a2}.{c2}");
            if neu.eq_ignore_ascii_case(&cond) {
                continue;
            }
            let fix = einsetzen(chars, a, b, &neu);
            let c2v: Vec<char> = fix.chars().collect();
            let t2v = lex(&c2v).toks;
            let Some(tt) = teile(&c2v, &t2v) else { continue };
            let from2 = txt(&c2v, tt.from);
            if let Some(n) = z.zahl(&format!("SELECT COUNT(*) FROM {from2}")).filter(|n| *n > 0) {
                out.push(Befund {
                    text: crate::tr_format!("„{cond}“ passt nie; verbunden sind die Tabellen über {neu} ({n} Zeilen vor dem WHERE)", "“{cond}” never matches; the tables are linked by {neu} ({n} rows before WHERE)"),
                    fix: Some(fix),
                    zeilen: Some(n),
                });
                return out;
            }
        }
        if z.zahl(&format!("SELECT COUNT(*) FROM {} JOIN {} ON {cond}", qa(t1, a1), qa(t2, a2))) == Some(0) {
            out.push(Befund { text: crate::tr_format!("Die JOIN-Bedingung „{cond}“ trifft kein einziges Paar.", "The JOIN condition “{cond}” matches no pair."), fix: None, zeilen: None });
        }
    }
    out
}

fn qa(t: &str, alias: &str) -> String {
    let q = crate::sqledit::lint::quote_if_needed;
    if t == alias { q(t) } else { format!("{} {}", q(t), q(alias)) }
}

// ---------------------------------------------------------------------------
// Kreuzprodukt: zwei Tabellen ohne Verbindung

/// Zwei Tabellen in FROM (Komma oder JOIN ohne ON/USING): (Name, Alias) beider und ob per Komma verbunden
type Paar = [(String, String); 2];

fn unverbunden_teile(chars: &[char], toks: &[Tok]) -> Option<(Teile, Paar, bool)> {
    let t = teile(chars, toks)?;
    if t.schreibt {
        return None;
    }
    let idx: Vec<usize> = (0..toks.len()).filter(|&k| toks[k].start >= t.from.0 && toks[k].end <= t.from.1).collect();
    if idx.iter().any(|&k| toks[k].kind == Kind::Open || toks[k].is_word("ON") || toks[k].is_word("USING") || toks[k].is_word("CROSS") || toks[k].is_word("NATURAL")) {
        return None;
    }
    let mut teile_from: Vec<Vec<usize>> = vec![Vec::new()];
    let mut komma = false;
    for &k in &idx {
        let x = &toks[k];
        if x.kind == Kind::Comma || x.is_word("JOIN") {
            komma |= x.kind == Kind::Comma;
            teile_from.push(Vec::new());
        } else if !(x.is_word("INNER") || x.is_word("LEFT") || x.is_word("RIGHT") || x.is_word("OUTER") || x.is_word("STRAIGHT_JOIN")) {
            teile_from.last_mut()?.push(k);
        }
    }
    if teile_from.len() != 2 {
        return None;
    }
    let mut paar: Vec<(String, String)> = Vec::new();
    for teil in &teile_from {
        let namen: Vec<&Tok> = teil.iter().map(|&k| &toks[k]).filter(|x| x.is_name() && !x.is_word("AS")).collect();
        let dot = teil.iter().any(|&k| toks[k].kind == Kind::Dot);
        let (name, alias) = match (namen.len(), dot) {
            (1, false) => (namen[0].text.clone(), namen[0].text.clone()),
            (2, false) => (namen[0].text.clone(), namen[1].text.clone()),
            (2, true) => (format!("{}.{}", namen[0].text, namen[1].text), namen[1].text.clone()),
            (3, true) => (format!("{}.{}", namen[0].text, namen[1].text), namen[2].text.clone()),
            _ => return None,
        };
        paar.push((name, alias));
    }
    // Verbindet eine Bedingung im WHERE die beiden (Spalte = Spalte)?
    if let Some((a, b)) = t.wo {
        let w: Vec<&Tok> = toks.iter().filter(|x| x.start >= a && x.end <= b).collect();
        for i in 1..w.len().saturating_sub(1) {
            if w[i].kind == Kind::Op && w[i].text == "=" && w[i - 1].is_name() && w[i + 1].is_name() && !w[i + 1].is_word("NULL") {
                return None;
            }
        }
    }
    Some((t, [paar[0].clone(), paar[1].clone()], komma))
}

/// Verbindung zweier Tabellen: Fremdschluessel, sonst Namen wie kunde_id -> kunde.id
fn verbindung(z: &mut dyn Zaehler, a: &(String, String), b: &(String, String)) -> Option<String> {
    if let Some((ca, cb)) = z.verweise(&a.0, &b.0).into_iter().next() {
        return Some(format!("{}.{ca} = {}.{cb}", a.1, b.1));
    }
    let (sa, sb) = (z.spalten(&a.0), z.spalten(&b.0));
    let hat = |v: &[String], n: &str| v.iter().find(|c| c.eq_ignore_ascii_case(n)).cloned();
    for (x, sx, y, sy) in [(a, &sa, b, &sb), (b, &sb, a, &sa)] {
        let pk = hat(sy, "id")?;
        let yn = kurz(&y.0);
        for kandidat in [format!("{yn}_id"), format!("{yn}id"), format!("id_{yn}")] {
            if let Some(c) = hat(sx, &kandidat) {
                return Some(format!("{}.{c} = {}.{pk}", x.1, y.1));
            }
        }
    }
    None
}

/// SELECT mit zwei Tabellen ohne Verbindung (liefert jede mit jeder Zeile kombiniert)
pub fn kreuzprodukt_moeglich(sql: &str) -> bool {
    let chars: Vec<char> = sql.trim().trim_end_matches(';').chars().collect();
    let toks = lex(&chars).toks;
    unverbunden_teile(&chars, &toks).is_some()
}

pub fn kreuzprodukt(sql: &str, z: &mut dyn Zaehler) -> Vec<Befund> {
    let sql = sql.trim().trim_end_matches(';').trim_end();
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let Some((t, [a, b], komma)) = unverbunden_teile(&chars, &toks) else { return Vec::new() };
    let Some(cond) = verbindung(z, &a, &b) else { return Vec::new() };
    let fix = if komma {
        match t.wo {
            Some((wa, wb)) => {
                let rest = txt(&chars, (wa, wb));
                let rest = if rest.to_ascii_uppercase().contains(" OR ") { format!("({rest})") } else { rest };
                einsetzen(&chars, wa, wb, &format!("{cond} AND {rest}"))
            }
            None => einsetzen(&chars, t.from.1, t.from.1, &format!("WHERE {cond}")),
        }
    } else {
        einsetzen(&chars, t.from.1, t.from.1, &format!("ON {cond}"))
    };
    let c2: Vec<char> = fix.chars().collect();
    let t2 = lex(&c2).toks;
    let Some(neu) = teile(&c2, &t2) else { return Vec::new() };
    let from2 = txt(&c2, neu.from);
    let n = match neu.wo {
        Some(w) => z.zahl(&format!("SELECT COUNT(*) FROM {from2} WHERE {}", txt(&c2, w))),
        None => z.zahl(&format!("SELECT COUNT(*) FROM {from2}")),
    };
    if n == Some(0) {
        return Vec::new();
    }
    let (na, nb) = (z.zahl(&format!("SELECT COUNT(*) FROM {}", qname(&a.0))), z.zahl(&format!("SELECT COUNT(*) FROM {}", qname(&b.0))));
    let mal = match (na, nb) {
        (Some(x), Some(y)) => format!(" ({} × {} = {})", crate::i18n::tausend(x), crate::i18n::tausend(y), crate::i18n::tausend(x * y)),
        _ => String::new(),
    };
    let text = crate::tr_format!(
        "„{}“ und „{}“ sind nicht verbunden: jede Zeile der einen wird mit jeder der anderen kombiniert{mal}. Über {cond} passen sie zusammen ({} Zeilen).",
        "“{}” and “{}” are not linked: every row of one is combined with every row of the other{mal}. Linked by {cond} they match up ({} rows).",
        kurz(&a.0),
        kurz(&b.0),
        n.map(crate::i18n::tausend).unwrap_or_else(|| "?".into())
    );
    vec![Befund { text, fix: Some(fix), zeilen: n }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Spielt den Server: Zaehlungen und Werte aus festen Tabellen
    struct Fake {
        zahlen: HashMap<String, u64>,
        werte: Vec<String>,
        fk: Vec<(String, String)>,
    }

    impl Zaehler for Fake {
        fn zahl(&mut self, sql: &str) -> Option<u64> {
            Some(*self.zahlen.get(sql).unwrap_or(&0))
        }
        fn texte(&mut self, _sql: &str) -> Vec<String> {
            self.werte.clone()
        }
        fn verweise(&mut self, _a: &str, _b: &str) -> Vec<(String, String)> {
            self.fk.clone()
        }
    }

    fn fake(z: &[(&str, u64)], werte: &[&str]) -> Fake {
        Fake { zahlen: z.iter().map(|(k, v)| (k.to_string(), *v)).collect(), werte: werte.iter().map(|s| s.to_string()).collect(), fk: Vec::new() }
    }

    #[test]
    fn schreibweise_aus_den_daten() {
        let mut z = fake(
            &[("SELECT COUNT(*) FROM kunde", 8), ("SELECT COUNT(*) FROM kunde WHERE ort = 'wien'", 0), ("SELECT COUNT(*) FROM kunde WHERE ort = 'Wien'", 3)],
            &["Wien", "Graz", "Linz"],
        );
        let b = untersuchen("SELECT * FROM kunde WHERE ort = 'wien';", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT * FROM kunde WHERE ort = 'Wien'"));
        assert_eq!(b[0].zeilen, Some(3));
    }

    #[test]
    fn umlaute_und_tippfehler() {
        assert_eq!(gemeint("Mueller", &["Müller".into(), "Maier".into()]), vec!["Müller".to_string()]);
        assert_eq!(gemeint("Kalgenfurt", &["Klagenfurt".into(), "Graz".into()]), vec!["Klagenfurt".to_string()]);
        assert!(gemeint("Berlin", &["Wien".into()]).is_empty());
    }

    #[test]
    fn nur_die_schuldige_bedingung_wird_ersetzt() {
        let mut z = fake(
            &[
                ("SELECT COUNT(*) FROM kunde k", 8),
                ("SELECT COUNT(*) FROM kunde k WHERE k.ort = 'Wien'", 3),
                ("SELECT COUNT(*) FROM kunde k WHERE k.name LIKE 'A*'", 0),
                ("SELECT COUNT(*) FROM kunde k WHERE k.ort = 'Wien' AND k.name LIKE 'A%'", 1),
            ],
            &["Anna Bauer"],
        );
        let b = untersuchen("SELECT name FROM kunde k WHERE k.ort = 'Wien' AND k.name LIKE 'A*'", &mut z);
        assert_eq!(b[0].fix.as_deref(), Some("SELECT name FROM kunde k WHERE k.ort = 'Wien' AND k.name LIKE 'A%'"));
    }

    #[test]
    fn null_und_limit() {
        let mut z = fake(&[("SELECT COUNT(*) FROM t", 5), ("SELECT COUNT(*) FROM t WHERE a IS NULL", 2)], &[]);
        assert_eq!(untersuchen("SELECT * FROM t WHERE a = NULL", &mut z)[0].fix.as_deref(), Some("SELECT * FROM t WHERE a IS NULL"));
        assert_eq!(untersuchen("SELECT * FROM t LIMIT 0", &mut z)[0].fix.as_deref(), Some("SELECT * FROM t"));
    }

    #[test]
    fn deutsches_datum_und_uhrzeit() {
        let mut z = fake(
            &[("SELECT COUNT(*) FROM b", 4), ("SELECT COUNT(*) FROM b WHERE datum = '2024-03-12'", 0), ("SELECT COUNT(*) FROM b WHERE DATE(datum) = '2024-03-12'", 2)],
            &[],
        );
        let r = untersuchen("SELECT * FROM b WHERE datum = '12.03.2024'", &mut z);
        assert_eq!(r[0].fix.as_deref(), Some("SELECT * FROM b WHERE DATE(datum) = '2024-03-12'"));
    }

    #[test]
    fn widerspruch_wird_zu_oder() {
        let mut z = fake(
            &[
                ("SELECT COUNT(*) FROM kunde", 8),
                ("SELECT COUNT(*) FROM kunde WHERE ort = 'Wien'", 3),
                ("SELECT COUNT(*) FROM kunde WHERE ort = 'Graz'", 2),
                ("SELECT COUNT(*) FROM kunde WHERE (ort = 'Wien' OR ort = 'Graz')", 5),
            ],
            &[],
        );
        let r = untersuchen("SELECT * FROM kunde WHERE ort = 'Wien' AND ort = 'Graz'", &mut z);
        assert_eq!(r[0].fix.as_deref(), Some("SELECT * FROM kunde WHERE (ort = 'Wien' OR ort = 'Graz')"));
        assert_eq!(r[0].zeilen, Some(5));
    }

    #[test]
    fn falsche_join_bedingung() {
        let mut z = fake(&[("SELECT COUNT(*) FROM kunde k JOIN bestellung b ON k.id = b.kunde_id", 7), ("SELECT COUNT(*) FROM kunde", 3), ("SELECT COUNT(*) FROM bestellung", 7)], &[]);
        z.fk = vec![("id".into(), "kunde_id".into())];
        let r = untersuchen("SELECT * FROM kunde k JOIN bestellung b ON k.id = b.id", &mut z);
        assert_eq!(r[0].fix.as_deref(), Some("SELECT * FROM kunde k JOIN bestellung b ON k.id = b.kunde_id"));
    }

    #[test]
    fn update_ohne_treffer() {
        let mut z = fake(&[("SELECT COUNT(*) FROM kunde", 8), ("SELECT COUNT(*) FROM kunde WHERE name = 'Anna Bauer'", 1)], &["Anna Bauer"]);
        let r = untersuchen("UPDATE kunde SET ort = 'Graz' WHERE name = 'anna bauer'", &mut z);
        assert_eq!(r[0].fix.as_deref(), Some("UPDATE kunde SET ort = 'Graz' WHERE name = 'Anna Bauer'"));
    }

    #[test]
    fn kreuzprodukt_wird_verbunden() {
        let mut z = fake(&[("SELECT COUNT(*) FROM kunde k, bestellung b WHERE k.id = b.kunde_id AND k.ort = 'Wien'", 4), ("SELECT COUNT(*) FROM kunde", 3), ("SELECT COUNT(*) FROM bestellung", 7)], &[]);
        z.fk = vec![("id".into(), "kunde_id".into())];
        // verweise liefert (Spalte in a, Spalte in b); a = kunde
        let r = kreuzprodukt("SELECT * FROM kunde k, bestellung b WHERE k.ort = 'Wien'", &mut z);
        assert_eq!(r[0].fix.as_deref(), Some("SELECT * FROM kunde k, bestellung b WHERE k.id = b.kunde_id AND k.ort = 'Wien'"), "{r:?}");
        assert_eq!(r[0].zeilen, Some(4));
        assert!(kreuzprodukt_moeglich("SELECT * FROM kunde JOIN bestellung"));
        assert!(!kreuzprodukt_moeglich("SELECT * FROM kunde k JOIN bestellung b ON k.id = b.kunde_id"));
        assert!(!kreuzprodukt_moeglich("SELECT * FROM kunde k, bestellung b WHERE k.id = b.kunde_id"));
        assert!(!kreuzprodukt_moeglich("SELECT * FROM groesse CROSS JOIN farbe"));
    }

    #[test]
    fn leere_tabelle() {
        let mut z = fake(&[], &[]);
        let r = untersuchen("SELECT * FROM kunde WHERE ort = 'Wien'", &mut z);
        assert!(r[0].text.contains("kunde"), "{r:?}");
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;

    /// Gegen einen echten Testserver: `EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test leer_echt -- --ignored`
    #[test]
    #[ignore]
    fn leer_echt() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("leer_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(
            &name,
            "CREATE TABLE kunde (id INT PRIMARY KEY, name VARCHAR(50), ort VARCHAR(30));
             INSERT INTO kunde VALUES (1, 'Anna Müller', 'Wien'), (2, 'Ben Hofer', 'Graz'), (3, 'Clara Gruber', 'Wien');
             CREATE TABLE bestellung (id INT PRIMARY KEY, kunde_id INT, am DATETIME, FOREIGN KEY (kunde_id) REFERENCES kunde(id));
             INSERT INTO bestellung VALUES (10, 1, '2024-03-12 09:30:00'), (11, 3, '2024-03-13 14:00:00');",
        )
        .unwrap();
        let mut conn = dbc.new_conn().unwrap();
        conn.query_drop(format!("USE {name}")).unwrap();
        let faelle = [
            ("SELECT * FROM kunde WHERE ort = 'wein'", "ort = 'Wien'"),
            ("SELECT * FROM kunde WHERE name LIKE 'Anna Mueller'", "LIKE 'Anna Müller'"),
            ("SELECT * FROM kunde WHERE name LIKE 'Anna*'", "LIKE 'Anna%'"),
            ("SELECT * FROM bestellung WHERE am = '12.03.2024'", "DATE(am) = '2024-03-12'"),
            ("SELECT * FROM kunde k JOIN bestellung b ON k.id = b.id", "k.id = b.kunde_id"),
            ("SELECT * FROM kunde WHERE ort = 'Wien' AND ort = 'Graz'", "OR"),
            ("DELETE FROM kunde WHERE ort = 'Grz'", "ort = 'Graz'"),
        ];
        let mut fehler = Vec::new();
        for (sql, erwartet) in faelle {
            let mut z = Server { conn: &mut conn, mariadb: true, rest: 40 };
            let b = untersuchen(sql, &mut z);
            let ok = b.iter().any(|b| b.fix.as_deref().is_some_and(|f| f.contains(erwartet)) && b.zeilen.is_some_and(|n| n > 0));
            eprintln!("{} {sql}\n    {b:?}", if ok { "OK  " } else { "FAIL" });
            if !ok {
                fehler.push(sql);
            }
        }
        // Kreuzprodukt ohne Fremdschluessel: ueber den Namen kunde_id
        dbc.exec_in(&name, "CREATE TABLE notiz (id INT PRIMARY KEY, kunde_id INT, text VARCHAR(20)); INSERT INTO notiz VALUES (1, 1, 'a'), (2, 3, 'b')").unwrap();
        let mut z = Server { conn: &mut conn, mariadb: true, rest: 40 };
        let k = kreuzprodukt("SELECT * FROM kunde, notiz", &mut z);
        eprintln!("{k:?}");
        if k.first().and_then(|b| b.fix.as_deref()) != Some("SELECT * FROM kunde, notiz WHERE notiz.kunde_id = kunde.id") || k[0].zeilen != Some(2) {
            fehler.push("kreuzprodukt");
        }
        let _ = dbc.exec(&format!("DROP DATABASE {name}"), ());
        assert!(fehler.is_empty(), "{fehler:?}");
    }
}
