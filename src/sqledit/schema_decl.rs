// Deklarierte Strukturen aus Migrationen/JPA: nur additive SQL-Vorschlaege, keine Datenmigration.
use super::lint::{Kind, Tok, lex};
use crate::db::{Schema, q};

#[derive(Clone, Debug, PartialEq)]
pub struct Teil {
    pub name: Option<String>,
    pub sql: String,
    pub kennung: String,
    pub referenz: Option<(Option<String>, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabellenPlan {
    pub datenbank: Option<String>,
    pub tabelle: String,
    pub teile: Vec<Teil>,
}

fn bezeichner(ts: &[Tok], at: &mut usize) -> Option<(Option<String>, String)> {
    let first = ts.get(*at).filter(|t| t.is_name())?.text.clone();
    *at += 1;
    if ts.get(*at).is_some_and(|t| t.kind == Kind::Dot) {
        *at += 1;
        let last = ts.get(*at).filter(|t| t.is_name())?.text.clone();
        *at += 1;
        Some((Some(first), last))
    } else {
        Some((None, first))
    }
}

fn teil(chars: &[char], ts: &[Tok]) -> Option<Teil> {
    let first = ts.first()?;
    let mut i = 0;
    if first.is_word("CONSTRAINT") {
        i = 1;
        if ts.get(i).is_some_and(|t| t.is_name() && !["PRIMARY", "UNIQUE", "FOREIGN", "CHECK"].contains(&t.upper().as_str())) {
            i += 1;
        }
    }
    let t = ts.get(i)?;
    let regel = t.kind == Kind::Word && ["PRIMARY", "UNIQUE", "KEY", "INDEX", "FOREIGN", "CHECK"].contains(&t.upper().as_str());
    let name = if regel { None } else { Some(t.text.clone()) };
    if name.is_some() && (!t.is_name() || !ts.get(i + 1).is_some_and(|t| sql_typ(&t.upper()))) {
        return None;
    }
    // Keine Abfragen, Routinen oder unbekannten Funktionen in DEFAULT/CHECK zulassen.
    let mut balance = 0i32;
    for (j, t) in ts.iter().enumerate() {
        if t.kind == Kind::Semi || t.kind == Kind::Var {
            return None;
        }
        if t.is_name()
            && t.kind != Kind::Quoted
            && ["SELECT", "INSERT", "DROP", "GRANT", "REVOKE", "LOAD", "OUTFILE", "PROCEDURE", "TRIGGER", "AS"]
                .contains(&t.upper().as_str())
        {
            return None;
        }
        if (t.is_word("DELETE") || t.is_word("UPDATE")) && !j.checked_sub(1).is_some_and(|k| ts[k].is_word("ON")) {
            return None;
        }
        if t.kind == Kind::Open {
            balance += 1;
        }
        if t.kind == Kind::Close {
            balance -= 1;
        }
        if balance < 0 {
            return None;
        }
        if t.is_name() && ts.get(j + 1).is_some_and(|n| n.kind == Kind::Open) {
            let context = j.checked_sub(1).and_then(|k| ts.get(k));
            let allowed = sql_typ(&t.upper())
                || [
                    "KEY",
                    "INDEX",
                    "UNIQUE",
                    "PRIMARY",
                    "FOREIGN",
                    "CHECK",
                    "CURRENT_TIMESTAMP",
                    "NOW",
                    "CURDATE",
                    "UUID",
                    "CHAR_LENGTH",
                    "LENGTH",
                    "ABS",
                    "COALESCE",
                ]
                .contains(&t.upper().as_str())
                || context.is_some_and(|p| p.is_word("REFERENCES") || p.is_word("KEY") || p.is_word("INDEX"))
                || (j >= 3 && ts[j - 1].kind == Kind::Dot && ts[j - 3].is_word("REFERENCES"));
            if !allowed {
                return None;
            }
        }
    }
    if balance != 0 {
        return None;
    }
    let sql: String = chars[first.start..ts.last()?.end].iter().collect();
    let mut referenz = None;
    let kennung = if regel {
        let open = ts.iter().position(|t| t.kind == Kind::Open)?;
        let close = open + ts[open..].iter().position(|t| t.kind == Kind::Close)?;
        let columns = ts[open..=close].iter().filter(|t| !t.is_word("ASC")).map(|t| t.text.to_lowercase()).collect::<Vec<_>>().join(" ");
        let kind = if t.is_word("INDEX") { "KEY".into() } else { t.upper() };
        if kind == "FOREIGN" {
            let r = ts.iter().position(|t| t.is_word("REFERENCES"))?;
            let mut r = r + 1;
            let (db, table) = bezeichner(ts, &mut r)?;
            referenz = Some((db.clone(), table.clone()));
            let end = r + ts[r..].iter().position(|t| t.kind == Kind::Close)?;
            format!("FOREIGN {columns} {table} {}", ts[r..=end].iter().map(|t| t.text.to_lowercase()).collect::<Vec<_>>().join(" "))
        } else if kind == "CHECK" {
            format!("CHECK {}", sql.to_lowercase())
        } else {
            format!("{kind} {columns}")
        }
    } else {
        format!("COLUMN {}", name.as_ref()?.to_lowercase())
    };
    Some(Teil { name, sql, kennung, referenz })
}

pub(super) fn sql_typ(typ: &str) -> bool {
    [
        "BIT",
        "BOOL",
        "BOOLEAN",
        "TINYINT",
        "SMALLINT",
        "MEDIUMINT",
        "INT",
        "INTEGER",
        "BIGINT",
        "DECIMAL",
        "NUMERIC",
        "FLOAT",
        "DOUBLE",
        "REAL",
        "DATE",
        "TIME",
        "DATETIME",
        "TIMESTAMP",
        "YEAR",
        "CHAR",
        "VARCHAR",
        "BINARY",
        "VARBINARY",
        "TINYTEXT",
        "TEXT",
        "MEDIUMTEXT",
        "LONGTEXT",
        "TINYBLOB",
        "BLOB",
        "MEDIUMBLOB",
        "LONGBLOB",
        "JSON",
        "ENUM",
        "SET",
        "UUID",
    ]
    .contains(&typ)
}

fn teile(chars: &[char], ts: &[Tok]) -> Option<Vec<Teil>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, t) in ts.iter().enumerate() {
        match t.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return None;
        }
        if t.kind == Kind::Comma && depth == 0 {
            result.push(teil(chars, &ts[start..i])?);
            start = i + 1;
        }
    }
    if depth != 0 {
        return None;
    }
    result.push(teil(chars, &ts[start..])?);
    Some(result)
}

/// CREATE TABLE und additive ALTER TABLE aus SQL-Migrationen lesen; DML wird nie uebernommen.
pub fn migrationen(text: &str) -> Result<Vec<TabellenPlan>, String> {
    let mut plans: Vec<TabellenPlan> = Vec::new();
    let mut active_db = None;
    for stmt in crate::db::split_statements(text) {
        if stmt.sql.contains("/*!") || stmt.sql.to_ascii_uppercase().contains("/*M!") {
            return Err("Ausfuehrbare SQL-Kommentare brauchen manuelle Pruefung".into());
        }
        let chars: Vec<char> = stmt.sql.chars().collect();
        let l = lex(&chars);
        let ts = &l.toks;
        if ts.first().is_some_and(|t| t.is_word("USE")) {
            if ts.len() != 2 || !ts[1].is_name() {
                return Err("Ungueltiges USE".into());
            }
            active_db = Some(ts[1].text.clone());
            continue;
        }
        if ts.first().is_some_and(|t| t.is_word("DROP") || t.is_word("RENAME")) {
            return Err("Nicht additive Migration braucht manuelle Pruefung".into());
        }
        let create = ts.first().is_some_and(|t| t.is_word("CREATE")) && ts.get(1).is_some_and(|t| t.is_word("TABLE"));
        let alter = ts.first().is_some_and(|t| t.is_word("ALTER")) && ts.get(1).is_some_and(|t| t.is_word("TABLE"));
        if !create && !alter {
            continue;
        }
        let parse = || -> Option<TabellenPlan> {
            if !l.problems.is_empty() {
                return None;
            }
            let mut at = 2;
            if create && ts.get(at).is_some_and(|t| t.is_word("IF")) {
                if !ts.get(at + 1)?.is_word("NOT") || !ts.get(at + 2)?.is_word("EXISTS") {
                    return None;
                }
                at += 3;
            }
            let (datenbank, tabelle) = bezeichner(ts, &mut at)?;
            let datenbank = datenbank.or_else(|| active_db.clone());
            let parts = if create {
                if ts.get(at)?.kind != Kind::Open {
                    return None;
                }
                let mut depth = 1;
                let begin = at + 1;
                at += 1;
                while at < ts.len() {
                    match ts[at].kind {
                        Kind::Open => depth += 1,
                        Kind::Close => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    at += 1;
                }
                if depth != 0 {
                    return None;
                }
                // Tabellenoptionen bleiben bewusst beim sicheren InnoDB/utf8mb4-Standard.
                if ts[at + 1..].iter().any(|t| t.is_word("SELECT") || t.is_word("AS") || t.is_word("LIKE") || t.kind == Kind::Semi) {
                    return None;
                }
                teile(&chars, &ts[begin..at])?
            } else {
                let mut parts = Vec::new();
                let mut depth = 0;
                let begin = at;
                for end in (at..ts.len()).chain(std::iter::once(ts.len())) {
                    if end < ts.len() {
                        match ts[end].kind {
                            Kind::Open => depth += 1,
                            Kind::Close => depth -= 1,
                            _ => {}
                        }
                    }
                    if end == ts.len() || (ts[end].kind == Kind::Comma && depth == 0) {
                        if !ts.get(at)?.is_word("ADD") {
                            return None;
                        }
                        at += 1;
                        if ts.get(at).is_some_and(|t| t.is_word("COLUMN")) {
                            at += 1;
                        }
                        parts.push(teil(&chars, &ts[at..end])?);
                        at = end + 1;
                    }
                }
                if begin == at {
                    return None;
                }
                parts
            };
            Some(TabellenPlan { datenbank, tabelle, teile: parts })
        };
        let plan = parse().ok_or_else(|| {
            crate::i18n::tr(
                "Migration enthält eine nicht eindeutig unterstützte Strukturänderung.",
                "Migration contains a schema change that cannot be inferred safely.",
            )
            .to_string()
        })?;
        if let Some(old) = plans.iter_mut().find(|p| p.datenbank == plan.datenbank && p.tabelle == plan.tabelle) {
            if create {
                return Err("Mehrere CREATE TABLE fuer dieselbe Tabelle".into());
            }
            for t in plan.teile {
                if old.teile.iter().any(|o| o.kennung == t.kennung) {
                    return Err("Widerspruechliche Definition".into());
                }
                old.teile.push(t);
            }
        } else {
            plans.push(plan);
        }
    }
    for plan in &mut plans {
        let mut extra = Vec::new();
        for part in plan.teile.iter_mut().filter(|p| p.name.is_some()) {
            let chars: Vec<char> = part.sql.chars().collect();
            let ts = lex(&chars).toks;
            let mut remove = Vec::new();
            for (i, t) in ts.iter().enumerate().skip(2) {
                let key = if t.is_word("PRIMARY") && ts.get(i + 1).is_some_and(|t| t.is_word("KEY")) {
                    Some(("PRIMARY KEY", i + 1))
                } else if t.is_word("UNIQUE") {
                    Some(("UNIQUE KEY", if ts.get(i + 1).is_some_and(|t| t.is_word("KEY")) { i + 1 } else { i }))
                } else {
                    None
                };
                if let Some((kind, end)) = key {
                    let ddl = format!("{kind} ({})", q(part.name.as_deref().unwrap()));
                    let cc: Vec<char> = ddl.chars().collect();
                    extra.push(teil(&cc, &lex(&cc).toks).ok_or("Ungueltiger Schluessel")?);
                    remove.push((t.start, ts[end].end));
                }
            }
            let mut filtered = chars;
            for (a, b) in remove.into_iter().rev() {
                filtered.drain(a..b);
            }
            part.sql = filtered.iter().collect::<String>().trim().to_string();
        }
        plan.teile.extend(extra);
    }
    Ok(plans)
}

/// Erst Tabellen/Spalten und Schluessel, dann Fremdschluessel. Bestehende Definitionen nie ersetzen.
pub fn plan_sql(plans: &[TabellenPlan], schema: &Schema, vorhanden: &[TabellenPlan]) -> Option<String> {
    let mut sql = Vec::new();
    let mut fks = Vec::new();
    for plan in plans {
        if plan.datenbank.as_ref().is_some_and(|d| d != &schema.name) {
            return None;
        }
        let current = vorhanden.iter().find(|p| p.tabelle == plan.tabelle);
        let exists = schema.tables.iter().any(|t| t.name == plan.tabelle);
        if exists && current.is_none() {
            return None;
        }
        let target = format!("{}.{}", q(&schema.name), q(&plan.tabelle));
        let mut parts = Vec::new();
        let mut additions = Vec::new();
        let mut keys = Vec::new();
        for t in &plan.teile {
            let is_present = current.is_some_and(|c| c.teile.iter().any(|x| x.kennung == t.kennung));
            if is_present {
                continue;
            }
            if t.kennung.starts_with("FOREIGN ") {
                fks.push(format!("ALTER TABLE {target} ADD {};", t.sql));
            } else if exists && t.name.is_none() {
                keys.push(t.sql.clone());
            } else if exists {
                additions.push(t.sql.clone());
            } else {
                parts.push(t.sql.clone());
            }
        }
        if exists {
            additions.extend(keys);
            if !additions.is_empty() {
                sql.push(format!("ALTER TABLE {target}\n  ADD {};", additions.join(",\n  ADD ")));
            }
        } else {
            if parts.is_empty() {
                return None;
            }
            sql.push(format!("CREATE TABLE {target} (\n  {}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;", parts.join(",\n  ")));
        }
    }
    sql.extend(fks);
    (!sql.is_empty()).then(|| sql.join("\n\n"))
}

/// Vorhandene Daten vor neuen NOT-NULL-/Primaer-/Unique-Anforderungen pruefen.
pub fn pruefe_daten(dbc: &crate::db::Db, db: &str, plans: &[TabellenPlan]) -> Result<(), String> {
    let schema = dbc.schema(db)?;
    for p in plans {
        let Some(table) = schema.tables.iter().find(|t| t.name == p.tabelle) else { continue };
        let target = format!("{}.{}", q(db), q(&p.tabelle));
        let has_rows = || dbc.rows(&format!("SELECT 1 FROM {target} LIMIT 1"), ()).map(|r| !r.is_empty());
        for part in &p.teile {
            let ts = lex(&part.sql.chars().collect::<Vec<_>>()).toks;
            if let Some(name) = &part.name {
                let required = ts.windows(2).any(|w| w[0].is_word("NOT") && w[1].is_word("NULL"));
                let automatic = ts.iter().any(|t| t.is_word("DEFAULT") || t.is_word("AUTO_INCREMENT"));
                if required && !automatic && !table.columns.iter().any(|c| c.name.eq_ignore_ascii_case(name)) && has_rows()? {
                    return Err(crate::tr_format!(
                        "Die neue Pflichtspalte {name} braucht Werte für vorhandene Zeilen. Bitte eine Datenmigration mit Startwerten verwenden.",
                        "The new required column {name} needs values for existing rows. Use a data migration with initial values."
                    ));
                }
            }
            let primary = part.kennung.starts_with("PRIMARY ");
            if !primary && !part.kennung.starts_with("UNIQUE ") {
                continue;
            }
            let Some(start) = ts.iter().position(|t| t.kind == Kind::Open) else { return Err("Schluessel ohne Spalten".into()) };
            let names = ts[start + 1..]
                .iter()
                .take_while(|t| t.kind != Kind::Close)
                .filter(|t| t.is_name() && !t.is_word("ASC") && !t.is_word("DESC"))
                .map(|t| t.text.clone())
                .collect::<Vec<_>>();
            if names.is_empty() {
                return Err("Leerer Schluessel".into());
            }
            if names.iter().any(|n| !table.columns.iter().any(|c| c.name.eq_ignore_ascii_case(n))) {
                if primary
                    && has_rows()?
                    && names
                        .iter()
                        .filter(|n| !table.columns.iter().any(|c| c.name.eq_ignore_ascii_case(n)))
                        .any(|n| !p.teile.iter().any(|t| t.name.as_ref() == Some(n) && t.sql.to_uppercase().contains("AUTO_INCREMENT")))
                {
                    return Err(crate::i18n::tr(
                        "Neue Primärschlüsselspalten brauchen Werte für vorhandene Zeilen.",
                        "New primary key columns need values for existing rows.",
                    )
                    .into());
                }
                continue;
            }
            let columns = names.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ");
            if primary {
                let nulls = names.iter().map(|n| format!("{} IS NULL", q(n))).collect::<Vec<_>>().join(" OR ");
                if !dbc.rows(&format!("SELECT 1 FROM {target} WHERE {nulls} LIMIT 1"), ())?.is_empty() {
                    return Err(crate::i18n::tr(
                        "NULL-Werte verhindern den Primärschlüssel. Es wurden keine Daten geändert.",
                        "NULL values prevent the primary key. No data was changed.",
                    )
                    .into());
                }
            }
            let present = names.iter().map(|n| format!("{} IS NOT NULL", q(n))).collect::<Vec<_>>().join(" AND ");
            if !dbc.rows(&format!("SELECT 1 FROM {target} WHERE {present} GROUP BY {columns} HAVING COUNT(*) > 1 LIMIT 1"), ())?.is_empty()
            {
                return Err(crate::i18n::tr(
                    "Doppelte Werte verhindern den eindeutigen Schlüssel. Es wurden keine Daten geändert.",
                    "Duplicate values prevent the unique key. No data was changed.",
                )
                .into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_mit_schluesseln_defaults_und_add() {
        let sql = "CREATE TABLE kunde (id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY, name VARCHAR(80) NOT NULL DEFAULT 'neu', UNIQUE KEY uk_name (name)); ALTER TABLE kunde ADD COLUMN aktiv BOOLEAN DEFAULT TRUE; INSERT INTO kunde VALUES (1,'x',1);";
        let plans = migrationen(sql).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].teile.len(), 5);
        let schema = Schema { name: "test".into(), ..Default::default() };
        let sql = plan_sql(&plans, &schema, &[]).unwrap();
        assert!(sql.contains("AUTO_INCREMENT"));
        assert!(sql.contains("PRIMARY KEY (`id`)"));
        assert!(sql.contains("DEFAULT 'neu'"));
        assert!(!sql.contains("INSERT"));
    }
    #[test]
    fn gefaehrliche_und_mehrdeutige_definitionen_ablehnen() {
        for sql in [
            "CREATE TABLE t AS SELECT 1",
            "CREATE TABLE t (x INT DEFAULT (fremde_funktion()))",
            "CREATE TABLE t (x INT); ALTER TABLE t DROP COLUMN x",
            "CREATE TABLE t (x INT); CREATE TABLE t (y TEXT)",
        ] {
            assert!(migrationen(sql).is_err(), "{sql}");
        }
    }
    #[test]
    fn fremdschluessel_erst_nach_tabellen_und_wiederholung_ohne_sql() {
        let plans = migrationen("CREATE TABLE kunde (id INT PRIMARY KEY); CREATE TABLE auftrag (kunde_id INT, CONSTRAINT fk FOREIGN KEY (kunde_id) REFERENCES kunde(id));").unwrap();
        let schema = Schema { name: "test".into(), ..Default::default() };
        let sql = plan_sql(&plans, &schema, &[]).unwrap();
        assert!(sql.find("CREATE TABLE `test`.`auftrag`").unwrap() < sql.find("ADD CONSTRAINT").unwrap());
        let mut existing = schema;
        existing.tables = plans.iter().map(|p| crate::db::TableInfo { name: p.tabelle.clone(), ..Default::default() }).collect();
        assert!(plan_sql(&plans, &existing, &plans).is_none());
    }
}
