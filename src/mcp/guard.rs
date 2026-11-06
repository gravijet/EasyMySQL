// Lese-Schutz fuer KI-Abfragen: Nur Anweisungen, die nichts veraendern, duerfen ueber das Werkzeug
// "query" laufen. Zusaetzlich laeuft jede Leseabfrage in einer READ-ONLY-Transaktion (siehe tools.rs).

/// Woerter einer Anweisung ausserhalb von Texten, `Bezeichnern` und Kommentaren, gross geschrieben.
pub fn words(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let nx = chars.get(i + 1).copied().unwrap_or('\0');
        if c == '#' || (c == '-' && nx == '-' && chars.get(i + 2).is_none_or(|c| c.is_whitespace())) {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && nx == '*' {
            // Ausfuehrbare Kommentare (/*! ... */) zaehlen als Code
            let exec = chars.get(i + 2) == Some(&'!');
            i += 2;
            if exec {
                i += 1;
                while i < n && chars[i].is_ascii_digit() {
                    i += 1;
                }
                continue;
            }
            while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '*' && nx == '/' {
            i += 2;
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            i += 1;
            while i < n && chars[i] != c {
                if chars[i] == '\\' && c != '`' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            // Platzhalter, damit "INTO 'datei'" erkennbar bleibt
            out.push(if c == '`' { "`".into() } else { "'".into() });
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '@' || c == '$' {
            let start = i;
            while i < n && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '@' || chars[i] == '$') {
                i += 1;
            }
            out.push(chars[start..i].iter().collect::<String>().to_uppercase());
            continue;
        }
        if c == ';' {
            out.push(";".into());
        }
        i += 1;
    }
    out
}

/// Erstes Wort, mit dem die Anweisung lesend beginnen darf
const READ_START: &[&str] = &["SELECT", "WITH", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "TABLE", "VALUES", "HELP"];

/// Darf diese einzelne Anweisung ueber das Lese-Werkzeug laufen? Fehler = Begruendung fuer die KI.
pub fn check_read_only(sql: &str) -> Result<(), String> {
    let w = words(sql);
    let mut it = w.iter().skip_while(|t| t.is_empty());
    let Some(first) = it.next() else { return Err("Empty statement.".into()) };
    if !READ_START.contains(&first.as_str()) {
        return Err(format!("`{first}` is not a read-only statement. Use the `execute` tool for changes."));
    }
    if w.iter().any(|t| t == ";") {
        return Err("Send one statement per call (no `;` inside).".into());
    }
    // Alles Weitere (WITH ... UPDATE, Funktionen mit Seiteneffekten) verhindert die READ-ONLY-Transaktion.
    // Dateien schreiben gehoert nicht zur Transaktion und ist deshalb hier gesperrt.
    for pair in w.windows(2) {
        if pair[0] == "INTO" && matches!(pair[1].as_str(), "OUTFILE" | "DUMPFILE") {
            return Err("Writing files with INTO OUTFILE/DUMPFILE is not allowed.".into());
        }
    }
    Ok(())
}

/// Stark gekuerzte Vorschau einer Anweisung (eine Zeile)
pub fn preview(sql: &str, max: usize) -> String {
    let one: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pass() {
        for sql in [
            "SELECT * FROM kunde",
            "  -- Kommentar\nselect id from t where name = 'DROP TABLE x'",
            "WITH a AS (SELECT 1) SELECT * FROM a",
            "SHOW TABLES",
            "SHOW CREATE TABLE kunde",
            "SHOW GRANTS",
            "DESCRIBE kunde",
            "EXPLAIN SELECT * FROM t",
            "(SELECT 1) UNION (SELECT 2)",
            "SELECT `update`, \"delete\" FROM t",
            "SELECT 1 /* UPDATE x */",
            "SELECT start, replace(name, 'a', 'b') AS `set` FROM t ORDER BY id DESC",
        ] {
            assert_eq!(check_read_only(sql), Ok(()), "{sql}");
        }
    }

    #[test]
    fn writes_are_rejected() {
        for sql in [
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "DROP TABLE t",
            "INSERT INTO t VALUES (1)",
            "SELECT 1; DROP TABLE t",
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
            "SELECT 1 INTO DUMPFILE '/tmp/x'",
            "SELECT /*!50000 1; DROP TABLE t */",
            "CALL proc()",
            "SET GLOBAL read_only = 0",
            "",
            "-- nur Kommentar",
        ] {
            assert!(check_read_only(sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn words_skip_strings_and_comments() {
        assert_eq!(words("select 'a;b' -- x\n, `c` # y\n;"), vec!["SELECT", "'", "`", ";"]);
        assert_eq!(preview("SELECT\n  *\n  FROM t", 100), "SELECT * FROM t");
        assert_eq!(preview("abcdef", 3), "abc…");
    }
}
