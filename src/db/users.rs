// Benutzer und Rechte: SHOW GRANTS lesen und GRANT/REVOKE-Anweisungen erzeugen.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Account {
    pub user: String,
    pub host: String,
}

impl Account {
    /// 'name'@'host'
    pub fn sql(&self) -> String {
        format!("{}@{}", lit(&self.user), lit(&self.host))
    }

    pub fn label(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Global,
    Db(String),
    Table(String, String),
}

/// Rechte, die es auf der jeweiligen Ebene gibt
const TABLE_PRIVS: &[&str] = &["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "INDEX", "REFERENCES", "CREATE VIEW", "SHOW VIEW", "TRIGGER", "GRANT OPTION"];
const DB_PRIVS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "INDEX", "REFERENCES", "CREATE VIEW", "SHOW VIEW", "TRIGGER", "EXECUTE", "CREATE ROUTINE", "ALTER ROUTINE", "EVENT",
    "LOCK TABLES", "CREATE TEMPORARY TABLES", "GRANT OPTION",
];
const GLOBAL_PRIVS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "INDEX", "REFERENCES", "CREATE VIEW", "SHOW VIEW", "TRIGGER", "EXECUTE", "CREATE ROUTINE", "ALTER ROUTINE", "EVENT",
    "LOCK TABLES", "CREATE TEMPORARY TABLES", "RELOAD", "PROCESS", "FILE", "SHUTDOWN", "SUPER", "CREATE USER", "SHOW DATABASES", "GRANT OPTION",
];

impl Scope {
    pub fn privileges(&self) -> &'static [&'static str] {
        match self {
            Scope::Global => GLOBAL_PRIVS,
            Scope::Db(_) => DB_PRIVS,
            Scope::Table(..) => TABLE_PRIVS,
        }
    }

    /// Ziel fuer GRANT/REVOKE; "_" und "%" in Datenbanknamen sind sonst Platzhalter
    pub fn sql(&self) -> String {
        match self {
            Scope::Global => "*.*".into(),
            Scope::Db(d) => format!("{}.*", q(&d.replace('_', "\\_").replace('%', "\\%"))),
            Scope::Table(d, t) => format!("{}.{}", q(d), q(t)),
        }
    }

    fn parse(s: &str) -> Option<Scope> {
        let s = s.trim();
        if s == "*.*" {
            return Some(Scope::Global);
        }
        let (d, t) = split_ident(s)?;
        let d = d.replace("\\_", "_").replace("\\%", "%");
        Some(if t == "*" { Scope::Db(d) } else { Scope::Table(d, t) })
    }
}

/// "`a`.`b`" oder "`a`.*" in ("a", "b") bzw. ("a", "*")
fn split_ident(s: &str) -> Option<(String, String)> {
    fn one(s: &str) -> Option<(String, &str)> {
        if let Some(r) = s.strip_prefix('`') {
            let mut out = String::new();
            let mut it = r.char_indices().peekable();
            while let Some((i, c)) = it.next() {
                if c == '`' {
                    if matches!(it.peek(), Some((_, '`'))) {
                        it.next();
                        out.push('`');
                    } else {
                        return Some((out, &r[i + 1..]));
                    }
                } else {
                    out.push(c);
                }
            }
            None
        } else if let Some(r) = s.strip_prefix('*') {
            Some(("*".into(), r))
        } else {
            None
        }
    }
    let (a, rest) = one(s)?;
    let (b, rest) = one(rest.strip_prefix('.')?)?;
    rest.is_empty().then_some((a, b))
}

/// Rechte je Ebene, wie sie der Server meldet. Spaltenrechte bleiben unberuehrt (nicht enthalten).
pub fn parse_grants(lines: &[String]) -> Vec<(Scope, Vec<String>)> {
    let mut out: Vec<(Scope, Vec<String>)> = Vec::new();
    for line in lines {
        let Some(rest) = line.strip_prefix("GRANT ") else { continue };
        let (Some(on), Some(to)) = (rest.find(" ON "), rest.rfind(" TO ")) else { continue };
        if to < on {
            continue;
        }
        let Some(scope) = Scope::parse(&rest[on + 4..to]) else { continue };
        let tail = &rest[to..];
        let mut privs: Vec<String> = Vec::new();
        let mut depth = 0;
        let mut cur = String::new();
        let mut parts = Vec::new();
        for c in rest[..on].chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
            cur.push(c);
        }
        parts.push(cur);
        for p in parts {
            let p = p.trim().to_uppercase();
            if p == "ALL PRIVILEGES" || p == "ALL" {
                privs.extend(scope.privileges().iter().filter(|x| **x != "GRANT OPTION").map(|x| x.to_string()));
            } else if p != "USAGE" && !p.contains('(') && !p.is_empty() {
                privs.push(p);
            }
        }
        if tail.contains("WITH GRANT OPTION") {
            privs.push("GRANT OPTION".into());
        }
        match out.iter_mut().find(|(s, _)| *s == scope) {
            Some((_, v)) => v.extend(privs),
            None => out.push((scope, privs)),
        }
    }
    out
}

/// GRANT- und REVOKE-Anweisungen, die von `current` zu `wanted` fuehren
pub fn diff_sql(account: &Account, scope: &Scope, current: &[String], wanted: &[String]) -> Vec<String> {
    let order = scope.privileges();
    let pick = |from: &[String], not_in: &[String]| -> Vec<&'static str> { order.iter().copied().filter(|p| from.iter().any(|f| f == p) && !not_in.iter().any(|f| f == p)).collect() };
    let (add, remove) = (pick(wanted, current), pick(current, wanted));
    let mut out = Vec::new();
    // GRANT OPTION getrennt, damit "WITH GRANT OPTION" nicht in der Liste steht
    for (verb, list, prep) in [("REVOKE", &remove, "FROM"), ("GRANT", &add, "TO")] {
        let plain: Vec<&str> = list.iter().copied().filter(|p| *p != "GRANT OPTION").collect();
        if !plain.is_empty() {
            out.push(format!("{verb} {} ON {} {prep} {}", plain.join(", "), scope.sql(), account.sql()));
        }
        if list.contains(&"GRANT OPTION") {
            out.push(if verb == "GRANT" { format!("GRANT USAGE ON {} TO {} WITH GRANT OPTION", scope.sql(), account.sql()) } else { format!("REVOKE GRANT OPTION ON {} FROM {}", scope.sql(), account.sql()) });
        }
    }
    out
}

impl Db {
    pub fn accounts(&self) -> Result<Vec<Account>, String> {
        // Rollen (MariaDB) sind keine Konten
        let rows = self.rows("SELECT User, Host FROM mysql.user WHERE is_role <> 'Y' ORDER BY User, Host", ()).or_else(|_| self.rows("SELECT User, Host FROM mysql.user ORDER BY User, Host", ()))?;
        Ok(rows.into_iter().map(|r| Account { user: s(&r[0]), host: s(&r[1]) }).collect())
    }

    pub fn grants(&self, a: &Account) -> Result<Vec<(Scope, Vec<String>)>, String> {
        let lines: Vec<String> = self.rows(&format!("SHOW GRANTS FOR {}", a.sql()), ())?.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect();
        Ok(parse_grants(&lines))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc() -> Account {
        Account { user: "anna".into(), host: "%".into() }
    }

    #[test]
    fn parse() {
        let g = parse_grants(&[
            "GRANT USAGE ON *.* TO `anna`@`%` IDENTIFIED BY PASSWORD '*ABC'".into(),
            "GRANT SELECT, INSERT, UPDATE ON `shop`.* TO `anna`@`%`".into(),
            "GRANT ALL PRIVILEGES ON `my\\_db`.* TO `anna`@`%` WITH GRANT OPTION".into(),
            "GRANT SELECT (id, name), DELETE ON `shop`.`kunde` TO `anna`@`%`".into(),
        ]);
        assert_eq!(g[0], (Scope::Global, vec![]));
        assert_eq!(g[1], (Scope::Db("shop".into()), vec!["SELECT".into(), "INSERT".into(), "UPDATE".into()]));
        assert_eq!(g[2].0, Scope::Db("my_db".into()));
        assert!(g[2].1.contains(&"CREATE ROUTINE".into()) && g[2].1.contains(&"GRANT OPTION".into()));
        assert_eq!(g[3], (Scope::Table("shop".into(), "kunde".into()), vec!["DELETE".into()]));
        assert_eq!(Scope::Db("my_db".into()).sql(), "`my\\_db`.*");
        assert_eq!(acc().sql(), "'anna'@'%'");
    }

    #[test]
    fn diff() {
        let s = Scope::Db("shop".into());
        let cur = vec!["SELECT".into(), "INSERT".into()];
        let want = vec!["SELECT".into(), "DELETE".into(), "GRANT OPTION".into()];
        assert_eq!(
            diff_sql(&acc(), &s, &cur, &want),
            vec!["REVOKE INSERT ON `shop`.* FROM 'anna'@'%'", "GRANT DELETE ON `shop`.* TO 'anna'@'%'", "GRANT USAGE ON `shop`.* TO 'anna'@'%' WITH GRANT OPTION"]
        );
        assert!(diff_sql(&acc(), &s, &cur, &cur).is_empty());
    }
}
