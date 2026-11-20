// Langsame Abfrage schneller machen: EXPLAIN zeigt, welche Tabelle ganz durchsucht wird; aus den Spalten, nach
// denen die Anweisung dort filtert oder verbindet, wird ein passender Index (erst Gleichheit, dann Bereich).

use super::lint::{Kind, Tok, lex};
use mysql::prelude::Queryable;

#[derive(Clone, Debug, PartialEq)]
pub struct Vorschlag {
    pub text: String,
    /// CREATE INDEX …
    pub sql: String,
}

/// Zeile aus EXPLAIN: (Tabelle bzw. Alias, Zugriffsart, geschaetzte Zeilen)
pub type Plan = Vec<(String, String, u64)>;

/// Was die Analyse vom Server braucht (im Test ersetzt)
pub trait Quelle {
    fn plan(&mut self, sql: &str) -> Plan;
    /// `db`: Datenbank aus dem Namen (db.tabelle), sonst die gewaehlte
    fn spalten(&mut self, db: Option<&str>, tabelle: &str) -> Vec<String>;
    /// Vorhandene Indizes (je Index die Spalten in Reihenfolge)
    fn indizes(&mut self, db: Option<&str>, tabelle: &str) -> Vec<Vec<String>>;
}

impl<C: Queryable> Quelle for C {
    fn plan(&mut self, sql: &str) -> Plan {
        let Ok(rows) = self.query::<mysql::Row, _>(format!("EXPLAIN {sql}")) else { return Vec::new() };
        rows.into_iter()
            .map(|r| {
                let s = |n: &str| r.get_opt::<Option<String>, _>(n).and_then(|v| v.ok()).flatten().unwrap_or_default();
                (s("table"), s("type"), s("rows").parse().unwrap_or(0))
            })
            .collect()
    }
    fn spalten(&mut self, db: Option<&str>, tabelle: &str) -> Vec<String> {
        self.exec("SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION", (db, tabelle)).unwrap_or_default()
    }
    fn indizes(&mut self, db: Option<&str>, tabelle: &str) -> Vec<Vec<String>> {
        let rows: Vec<(String, String)> = self
            .exec("SELECT INDEX_NAME, COLUMN_NAME FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = COALESCE(?, DATABASE()) AND TABLE_NAME = ? ORDER BY INDEX_NAME, SEQ_IN_INDEX", (db, tabelle))
            .unwrap_or_default();
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (i, c) in rows {
            match out.last_mut() {
                Some((n, v)) if *n == i => v.push(c),
                _ => out.push((i, vec![c])),
            }
        }
        out.into_iter().map(|(_, v)| v).collect()
    }
}

/// Ab so vielen (geschaetzten) Zeilen lohnt ein Index
const AB_ZEILEN: u64 = 2000;

/// Tabellen der Anweisung: (Datenbank, Name, Alias)
fn tabellen(toks: &[Tok]) -> Vec<(Option<String>, String, String)> {
    let mut out = Vec::new();
    let stop = ["WHERE", "GROUP", "ORDER", "LIMIT", "HAVING", "ON", "USING", "SET", "JOIN", "INNER", "LEFT", "RIGHT", "CROSS", "STRAIGHT_JOIN", "NATURAL", "UNION", "FOR"];
    let mut k = 0;
    while k < toks.len() {
        let t = &toks[k];
        let start = t.is_word("FROM") || t.is_word("JOIN") || t.is_word("UPDATE") || t.is_word("STRAIGHT_JOIN") || (t.kind == Kind::Comma && out.last().is_some() && !toks[..k].iter().rev().take_while(|x| !x.is_word("FROM")).any(|x| x.is_word("WHERE") || x.is_word("SELECT")));
        if start && toks.get(k + 1).is_some_and(Tok::is_name) && !stop.iter().any(|w| toks[k + 1].is_word(w)) {
            let mut i = k + 1;
            let mut name = toks[i].text.clone();
            let mut db = None;
            if toks.get(i + 1).is_some_and(|x| x.kind == Kind::Dot) && toks.get(i + 2).is_some_and(Tok::is_name) {
                db = Some(name);
                name = toks[i + 2].text.clone();
                i += 2;
            }
            let mut alias = name.clone();
            if toks.get(i + 1).is_some_and(|x| x.is_word("AS")) && toks.get(i + 2).is_some_and(Tok::is_name) {
                alias = toks[i + 2].text.clone();
            } else if let Some(n) = toks.get(i + 1).filter(|x| x.is_name() && !stop.iter().any(|w| x.is_word(w))) {
                alias = n.text.clone();
            }
            out.push((db, name, alias));
        }
        k += 1;
    }
    out
}

/// Spalten von `alias`, nach denen gefiltert oder verbunden wird: (Spalte, Gleichheit?)
fn bedingungsspalten(toks: &[Tok], alias: &str, spalten: &[String], allein: bool) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // Nur hinter WHERE / ON / HAVING
    let mut aktiv = false;
    for k in 0..toks.len() {
        let t = &toks[k];
        if t.is_word("WHERE") || t.is_word("ON") || t.is_word("HAVING") {
            aktiv = true;
            continue;
        }
        if t.is_word("GROUP") || t.is_word("ORDER") || t.is_word("LIMIT") || t.is_word("JOIN") || t.is_word("SELECT") {
            aktiv = false;
            continue;
        }
        if !aktiv || !t.is_name() {
            continue;
        }
        // a.spalte bzw. spalte (nur, wenn nur eine Tabelle beteiligt ist)
        let (spalte, nach) = if toks.get(k + 1).is_some_and(|x| x.kind == Kind::Dot) && toks.get(k + 2).is_some_and(Tok::is_name) {
            if !t.text.eq_ignore_ascii_case(alias) {
                continue;
            }
            (toks[k + 2].text.clone(), k + 3)
        } else if k > 0 && toks[k - 1].kind == Kind::Dot {
            continue;
        } else if allein {
            (t.text.clone(), k + 1)
        } else {
            continue;
        };
        let Some(echt) = spalten.iter().find(|s| s.eq_ignore_ascii_case(&spalte)) else { continue };
        let vor = k.checked_sub(1).map(|i| &toks[i]);
        let op = toks.get(nach);
        let gleich = op.is_some_and(|o| (o.kind == Kind::Op && (o.text == "=" || o.text == "<=>")) || o.is_word("IN")) || vor.is_some_and(|o| o.kind == Kind::Op && o.text == "=");
        let bereich = op.is_some_and(|o| (o.kind == Kind::Op && ["<", ">", "<=", ">="].contains(&o.text.as_str())) || o.is_word("BETWEEN") || o.is_word("LIKE"))
            || vor.is_some_and(|o| o.kind == Kind::Op && ["<", ">", "<=", ">="].contains(&o.text.as_str()));
        if !gleich && !bereich {
            continue;
        }
        match out.iter_mut().find(|(s, _)| s == echt) {
            Some(e) => e.1 |= gleich,
            None => out.push((echt.clone(), gleich)),
        }
    }
    out
}

pub fn untersuchen(sql: &str, z: &mut dyn Quelle) -> Vec<Vorschlag> {
    let sql = sql.trim().trim_end_matches(';').trim_end();
    let kopf = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    if !["SELECT", "UPDATE", "DELETE"].contains(&kopf.as_str()) {
        return Vec::new();
    }
    let chars: Vec<char> = sql.chars().collect();
    let toks = lex(&chars).toks;
    let tabs = tabellen(&toks);
    let mut out = Vec::new();
    for (tab, art, zeilen) in z.plan(sql) {
        if art != "ALL" || zeilen < AB_ZEILEN {
            continue;
        }
        let Some((db, name, alias)) = tabs.iter().find(|(_, n, a)| a.eq_ignore_ascii_case(&tab) || n.eq_ignore_ascii_case(&tab)).cloned() else { continue };
        let spalten = z.spalten(db.as_deref(), &name);
        let mut cols = bedingungsspalten(&toks, &alias, &spalten, tabs.len() == 1);
        if cols.is_empty() {
            continue;
        }
        // Erst Gleichheit, dann hoechstens eine Bereichsspalte
        cols.sort_by_key(|(_, g)| !*g);
        let mut wahl: Vec<String> = cols.iter().filter(|c| c.1).map(|c| c.0.clone()).take(3).collect();
        if let Some(r) = cols.iter().find(|c| !c.1) {
            wahl.push(r.0.clone());
        }
        // Schon ein Index, der mit diesen Spalten beginnt?
        let vorhanden = z.indizes(db.as_deref(), &name);
        if vorhanden.iter().any(|ix| ix.first().is_some_and(|f| f.eq_ignore_ascii_case(&wahl[0]))) {
            continue;
        }
        let q = super::lint::quote_if_needed;
        let ix_name: String = format!("ix_{}_{}", name, wahl.join("_")).chars().take(64).collect();
        let liste = wahl.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
        out.push(Vorschlag {
            text: crate::tr_format!(
                "„{name}“ wird ganz durchsucht (etwa {} Zeilen). Ein Index auf ({}) lässt den Server direkt zu den passenden Zeilen springen.",
                "“{name}” is scanned completely (about {} rows). An index on ({}) lets the server jump straight to the matching rows.",
                crate::i18n::tausend(zeilen),
                wahl.join(", ")
            ),
            sql: format!("CREATE INDEX {} ON {}{} ({liste})", q(&ix_name), db.as_deref().map(|d| format!("{}.", q(d))).unwrap_or_default(), q(&name)),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        plan: Plan,
        spalten: Vec<String>,
        indizes: Vec<Vec<String>>,
    }

    impl Quelle for Fake {
        fn plan(&mut self, _sql: &str) -> Plan {
            self.plan.clone()
        }
        fn spalten(&mut self, _db: Option<&str>, _t: &str) -> Vec<String> {
            self.spalten.clone()
        }
        fn indizes(&mut self, _db: Option<&str>, _t: &str) -> Vec<Vec<String>> {
            self.indizes.clone()
        }
    }

    fn fake(plan: &[(&str, &str, u64)], spalten: &[&str]) -> Fake {
        Fake { plan: plan.iter().map(|(a, b, c)| (a.to_string(), b.to_string(), *c)).collect(), spalten: spalten.iter().map(|s| s.to_string()).collect(), indizes: vec![vec!["id".into()]] }
    }

    #[test]
    fn gleichheit_vor_bereich() {
        let mut z = fake(&[("messung", "ALL", 300000)], &["id", "sensor", "zeit", "wert"]);
        let v = untersuchen("SELECT * FROM messung WHERE zeit > '2026-11-01' AND sensor = 42", &mut z);
        assert_eq!(v[0].sql, "CREATE INDEX ix_messung_sensor_zeit ON messung (sensor, zeit)");
    }

    #[test]
    fn join_ueber_alias() {
        let mut z = fake(&[("k", "ALL", 50), ("b", "ALL", 90000)], &["id", "kunde_id", "betrag"]);
        let v = untersuchen("SELECT * FROM kunde k JOIN bestellung b ON b.kunde_id = k.id WHERE k.id = 3", &mut z);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].sql, "CREATE INDEX ix_bestellung_kunde_id ON bestellung (kunde_id)");
    }

    #[test]
    fn vorhandener_index_und_kleine_tabelle() {
        let mut z = fake(&[("messung", "ALL", 300000)], &["id", "sensor"]);
        z.indizes.push(vec!["sensor".into(), "zeit".into()]);
        assert!(untersuchen("SELECT * FROM messung WHERE sensor = 1", &mut z).is_empty());
        let mut z = fake(&[("messung", "ALL", 30)], &["id", "sensor"]);
        assert!(untersuchen("SELECT * FROM messung WHERE sensor = 1", &mut z).is_empty());
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;

    /// `EASYMYSQL_AUTOFIX_TEST_PORT=3407 cargo test tempo_echt -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tempo_echt() {
        let port = std::env::var("EASYMYSQL_AUTOFIX_TEST_PORT").unwrap().parse().unwrap();
        let info = crate::db::ConnInfo { port, ..Default::default() };
        let dbc = crate::db::Db::connect(&info).unwrap();
        let name = format!("tempo_{}", std::process::id());
        dbc.exec(&format!("CREATE DATABASE {name}"), ()).unwrap();
        dbc.exec_in(
            &name,
            "CREATE TABLE messung (id INT PRIMARY KEY AUTO_INCREMENT, sensor INT, zeit DATETIME, wert DOUBLE);
             INSERT INTO messung (sensor, zeit, wert) SELECT seq % 500, '2026-01-01' + INTERVAL seq MINUTE, seq / 7 FROM seq_1_to_50000;
             ANALYZE TABLE messung;",
        )
        .unwrap();
        let mut conn = dbc.new_conn().unwrap();
        conn.query_drop(format!("USE {name}")).unwrap();
        let v = untersuchen("SELECT * FROM messung WHERE sensor = 42 AND zeit > '2026-01-10'", &mut conn);
        eprintln!("{v:?}");
        let _ = dbc.exec(&format!("DROP DATABASE {name}"), ());
        assert_eq!(v.first().map(|v| v.sql.as_str()), Some("CREATE INDEX ix_messung_sensor_zeit ON messung (sensor, zeit)"));
    }
}
