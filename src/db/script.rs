// Skripte in einzelne Anweisungen zerlegen und mit Zeilen- und Speichergrenze ausfuehren.

use super::*;

/// Ergebnis einer einzelnen Anweisung.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub affected: u64,
    pub truncated: bool,
}

impl ResultSet {
    pub fn has_table(&self) -> bool {
        !self.columns.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryOutput {
    pub sets: Vec<ResultSet>,
    pub error: Option<String>,
    pub elapsed: Duration,
}

pub const MAX_ROWS: usize = 50_000;

/// Geschaetzter Speicherbedarf eines Ergebnisses, ab dem nicht weiter gelesen wird. Jede Zelle
/// zaehlt mit ihrer Laenge plus einem festen Aufschlag fuer `Option<String>` und die Zuteilung.
pub const MAX_RESULT_BYTES: usize = 192 * 1024 * 1024;
const CELL_OVERHEAD: usize = 40;

/// Ist der Text eine reine Leseabfrage? Nur dann darf ein zu grosses Ergebnis serverseitig
/// abgebrochen werden – ein CALL oder INSERT ... SELECT soll nie mittendrin enden.
fn is_read_only(sql: &str) -> bool {
    let mut t = sql.trim_start();
    while t.starts_with('(') {
        t = t[1..].trim_start();
    }
    let word: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    matches!(word.as_str(), "SELECT" | "WITH" | "SHOW" | "TABLE" | "VALUES" | "DESCRIBE" | "DESC" | "EXPLAIN")
        && !t.to_ascii_uppercase().contains("INTO ")
}

/// Fuehrt einen (ggf. mehrteiligen) SQL-Text aus und sammelt alle Ergebnisse.
pub fn run_script<C: Queryable>(conn: &mut C, sql: &str) -> QueryOutput {
    run_script_with(conn, sql, None)
}

/// Wie `run_script`. `abort` wird einmal aufgerufen, sobald eine Leseabfrage mehr Zeilen
/// liefert, als gespeichert werden (siehe `MAX_ROWS`): Der Aufrufer kann dann die Abfrage auf
/// dem Server beenden (`KILL QUERY`), statt den Rest noch ueber das Netz zu lesen und zu
/// verwerfen.
pub fn run_script_with<C: Queryable>(conn: &mut C, sql: &str, abort: Option<&dyn Fn()>) -> QueryOutput {
    let start = Instant::now();
    let mut out = QueryOutput::default();
    let read_only = is_read_only(sql);
    match conn.query_iter(sql) {
        Ok(mut result) => {
            while let Some(set) = result.iter() {
                let mut rs = ResultSet {
                    columns: set
                        .columns()
                        .as_ref()
                        .iter()
                        .map(|c| c.name_str().into_owned())
                        .collect(),
                    ..Default::default()
                };
                rs.affected = set.affected_rows();
                let mut failed = None;
                let mut bytes = 0usize;
                let mut aborted = false;
                for row in set {
                    match row {
                        Ok(row) => {
                            if rs.truncated {
                                // Abbruch ist angefordert; bis er greift, nichts mehr speichern.
                                continue;
                            }
                            if rs.rows.len() >= MAX_ROWS || bytes >= MAX_RESULT_BYTES {
                                rs.truncated = true;
                                if read_only
                                    && let Some(abort) = abort {
                                        abort();
                                        aborted = true;
                                    }
                                continue;
                            }
                            let r: Row = row.unwrap().into_iter().map(value_into_string).collect();
                            bytes += r.iter().map(|c| c.as_ref().map_or(0, String::len) + CELL_OVERHEAD).sum::<usize>();
                            rs.rows.push(r);
                        }
                        // Nach einem gewollten Abbruch meldet der Server "Query execution was interrupted".
                        Err(_) if aborted => break,
                        Err(e) => {
                            failed = Some(err_text(e));
                            break;
                        }
                    }
                }
                rs.rows.shrink_to_fit();
                out.sets.push(rs);
                if let Some(e) = failed {
                    out.error = Some(e);
                    break;
                }
            }
        }
        Err(e) => out.error = Some(err_text(e)),
    }
    out.elapsed = start.elapsed();
    out
}

// ---------------------------------------------------------------------------
// Anweisungen einzeln ausfuehren

/// Eine einzelne SQL-Anweisung aus einem Skript.
#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub sql: String,
    /// Position im Skript (Zeichenindizes)
    pub start: usize,
    pub end: usize,
    /// Zeile (0-basiert), in der die Anweisung beginnt
    pub line: usize,
}

/// Zerlegt ein SQL-Skript in Anweisungen. Beachtet Texte, Kommentare, `Bezeichner`
/// und DELIMITER (fuer Prozeduren/Trigger).
pub fn split_statements(text: &str) -> Vec<Stmt> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut delim: Vec<char> = vec![';'];
    let mut i = 0;
    let mut start: Option<usize> = None;
    let mut line = 0usize;
    let mut start_line = 0usize;
    let at_line_start = |i: usize| i == 0 || chars[i - 1] == '\n';
    let push = |out: &mut Vec<Stmt>, s: usize, e: usize, l: usize| {
        let sql: String = chars[s..e].iter().collect();
        if !sql.trim().is_empty() && !is_only_comments(&sql) {
            out.push(Stmt { sql: sql.trim().to_string(), start: s, end: e, line: l });
        }
    };
    while i < n {
        let c = chars[i];
        let nx = chars.get(i + 1).copied().unwrap_or('\0');
        // DELIMITER-Zeile (nur am Zeilenanfang, ausserhalb einer Anweisung)
        if start.is_none() && at_line_start(i) {
            let rest: String = chars[i..].iter().take(10).collect();
            if rest.to_uppercase().starts_with("DELIMITER ") {
                let mut j = i + 10;
                let mut d = Vec::new();
                while j < n && chars[j] != '\n' {
                    if !chars[j].is_whitespace() {
                        d.push(chars[j]);
                    }
                    j += 1;
                }
                if !d.is_empty() {
                    delim = d;
                }
                i = j;
                continue;
            }
        }
        if c == '\n' {
            line += 1;
        }
        if start.is_none() && !c.is_whitespace() {
            // Kommentarzeilen vor einer Anweisung gehoeren nicht dazu
            if (c == '-' && nx == '-') || c == '#' {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            start = Some(i);
            start_line = line;
        }
        if (c == '-' && nx == '-') || c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && nx == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            i += 1;
            while i < n && chars[i] != c {
                if chars[i] == '\\' && c != '`' {
                    i += 1;
                } else if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if chars[i..].starts_with(&delim) {
            if let Some(s) = start.take() {
                push(&mut out, s, i, start_line);
            }
            i += delim.len();
            continue;
        }
        i += 1;
    }
    if let Some(s) = start {
        push(&mut out, s, n, start_line);
    }
    out
}

fn is_only_comments(sql: &str) -> bool {
    sql.lines().all(|l| {
        let t = l.trim();
        t.is_empty() || t.starts_with("--") || t.starts_with('#')
    }) && !sql.contains("/*!")
}

/// Ergebnis einer einzelnen Anweisung.
#[derive(Clone, Debug, Default)]
pub struct StmtResult {
    pub line: usize,
    pub preview: String,
    pub sets: Vec<ResultSet>,
    pub affected: u64,
    /// Gelieferte Zeilen (bleibt, auch wenn ein Ergebnis weggeklickt wird)
    pub returned: Option<usize>,
    pub error: Option<String>,
    /// Zeile des Fehlers im Skript (0-basiert), falls bekannt
    pub error_line: Option<usize>,
    pub elapsed: Duration,
}

/// Fuehrt die Anweisungen nacheinander aus; bricht beim ersten Fehler ab.
pub fn run_statements<C: Queryable>(conn: &mut C, stmts: &[Stmt], abort: Option<&dyn Fn()>) -> Vec<StmtResult> {
    let mut out = Vec::new();
    for st in stmts {
        let start = Instant::now();
        let mut r = run_script_with(conn, &st.sql, abort);
        let affected = r.sets.iter().filter(|s| !s.has_table()).map(|s| s.affected).sum();
        let sets: Vec<ResultSet> = std::mem::take(&mut r.sets).into_iter().filter(|s| s.has_table()).collect();
        let mut res = StmtResult {
            line: st.line,
            preview: st.sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(90).collect(),
            affected,
            returned: (!sets.is_empty()).then(|| sets.iter().map(|s| s.rows.len()).sum()),
            sets,
            error: r.error.take(),
            error_line: None,
            elapsed: start.elapsed(),
        };
        if let Some(e) = &res.error {
            // MariaDB meldet "... at line N" relativ zur Anweisung
            let rel = e
                .rsplit("at line ")
                .next()
                .and_then(|t| t.trim().trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<usize>().ok())
                .filter(|_| e.contains("at line "));
            // Zeilen vor dem ersten Zeichen der Anweisung (fuehrende Leerzeilen) beachten
            res.error_line = Some(st.line + rel.map(|l| l.saturating_sub(1)).unwrap_or(0));
            out.push(res);
            break;
        }
        out.push(res);
    }
    out
}

/// Anweisung, in der der Cursor steht (oder die direkt davor endet).
pub fn statement_at(stmts: &[Stmt], cursor: usize) -> Option<&Stmt> {
    stmts
        .iter()
        .find(|s| cursor >= s.start && cursor <= s.end + 1)
        .or_else(|| stmts.iter().rev().find(|s| s.end <= cursor))
}

#[cfg(test)]
mod bench {
    use super::*;

    /// Braucht einen Server auf 127.0.0.1:3306: `cargo test bench_select -- --ignored --nocapture`
    #[test]
    #[ignore = "manuelle Messung"]
    fn bench_select() {
        let db = Db::connect(&ConnInfo::default()).unwrap();
        let mut c = db.new_conn().unwrap();
        run_script(&mut c, "DROP DATABASE IF EXISTS bench; CREATE DATABASE bench; USE bench; CREATE TABLE t (id INT PRIMARY KEY AUTO_INCREMENT, a VARCHAR(40), b DOUBLE, c DATETIME, d TEXT);");
        c.query_drop("SET @n := 0").unwrap();
        c.query_drop("INSERT INTO t (a, b, c, d) SELECT CONCAT('name', seq), seq / 7, NOW(), REPEAT('x', 50) FROM seq_1_to_200000").unwrap();
        for sql in ["SELECT * FROM t", "SELECT id FROM t", "SELECT COUNT(*) FROM t"] {
            let t = Instant::now();
            let out = run_script(&mut c, sql);
            println!("{sql}: {} rows in {:.0} ms", out.sets[0].rows.len(), t.elapsed().as_secs_f64() * 1000.0);
        }
        let t = Instant::now();
        let n: usize = c.query_iter("SELECT * FROM t LIMIT 50000").unwrap().count();
        println!("raw iteration: {n} rows in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        c.query_drop("DROP DATABASE bench").unwrap();
    }
}
