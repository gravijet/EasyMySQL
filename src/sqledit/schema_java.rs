// Konstante Java-SQL-Ausdruecke lesen, ohne Java-Code auszufuehren.
use super::schema_jpa::java_tokens;
use std::collections::{HashMap, HashSet};

fn literal(s: &str) -> Option<String> {
    let block = s.starts_with("\"\"\"") && s.ends_with("\"\"\"") && s.len() >= 6;
    let content =
        if block { s[3..s.len() - 3].strip_prefix('\n').unwrap_or(&s[3..s.len() - 3]) } else { s.strip_prefix('"')?.strip_suffix('"')? };
    let mut chars = content.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            's' => out.push(' '),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '\n' if block => {}
            '\r' if block => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            'u' => {
                while chars.peek() == Some(&'u') {
                    chars.next();
                }
                let n = (0..4).try_fold(0u32, |n, _| Some(n * 16 + chars.next()?.to_digit(16)?))?;
                out.push(char::from_u32(n)?);
            }
            first @ '0'..='7' => {
                let mut n = first.to_digit(8)?;
                for _ in 0..if first <= '3' { 2 } else { 1 } {
                    if let Some(d) = chars.peek().and_then(|c| c.to_digit(8)) {
                        chars.next();
                        n = n * 8 + d;
                    } else {
                        break;
                    }
                }
                out.push(char::from_u32(n)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn ausdruck(ts: &[String], at: &mut usize, vars: &HashMap<String, String>) -> Option<String> {
    let mut result = atom(ts, at, vars)?;
    while ts.get(*at).is_some_and(|t| t == "+") {
        *at += 1;
        result.push_str(&atom(ts, at, vars)?);
    }
    Some(result)
}

fn atom(ts: &[String], at: &mut usize, vars: &HashMap<String, String>) -> Option<String> {
    let t = ts.get(*at)?;
    *at += 1;
    let mut value = if t.starts_with('"') {
        literal(t)?
    } else if t == "(" {
        let v = ausdruck(ts, at, vars)?;
        if ts.get(*at)? != ")" {
            return None;
        }
        *at += 1;
        v
    } else if t == "new" && ts.get(*at).is_some_and(|t| t == "StringBuilder" || t == "StringBuffer") {
        *at += 1;
        if ts.get(*at)? != "(" {
            return None;
        }
        *at += 1;
        let v = if ts.get(*at).is_some_and(|t| t == ")") { String::new() } else { ausdruck(ts, at, vars)? };
        if ts.get(*at)? != ")" {
            return None;
        }
        *at += 1;
        v
    } else if let Some(n) = t.strip_suffix(".append") {
        let mut v = vars.get(n)?.clone();
        if ts.get(*at)? != "(" {
            return None;
        }
        *at += 1;
        v.push_str(&ausdruck(ts, at, vars)?);
        if ts.get(*at)? != ")" {
            return None;
        }
        *at += 1;
        v
    } else if let Some(n) = t.strip_suffix(".toString") {
        let v = vars.get(n)?.clone();
        if ts.get(*at)? != "(" || ts.get(*at + 1)? != ")" {
            return None;
        }
        *at += 2;
        v
    } else {
        vars.get(t)?.clone()
    };
    while ts.get(*at).is_some_and(|t| t == ".") {
        *at += 1;
        let method = ts.get(*at)?;
        *at += 1;
        if ts.get(*at)? != "(" {
            return None;
        }
        *at += 1;
        if method == "append" {
            value.push_str(&ausdruck(ts, at, vars)?);
        } else if method != "toString" {
            return None;
        }
        if ts.get(*at)? != ")" {
            return None;
        }
        *at += 1;
    }
    Some(value)
}

/// SQL plus eindeutiger PreparedStatement-Empfaenger fuer mehrere Abfragen in einer Datei.
pub(super) fn abfragen(text: &str) -> Vec<(String, Option<String>)> {
    let ts = java_tokens(text);
    let mut vars = HashMap::new();
    let mut unknown = HashSet::new();
    let mut out = Vec::new();
    let mut receivers: HashMap<String, HashSet<String>> = HashMap::new();
    let mut builders = HashSet::new();
    let mut jdbc = false;
    for i in 0..ts.len() {
        if ts[i] == "=" && i > 0 {
            let name = ts[i - 1].clone();
            let mut at = i + 1;
            if name == "+" && i >= 2 {
                vars.remove(&ts[i - 2]);
                unknown.insert(ts[i - 2].clone());
                continue;
            }
            if ts.get(i + 1).is_some_and(|t| t == "new") && ts.get(i + 2).is_some_and(|t| t == "StringBuilder" || t == "StringBuffer") {
                builders.insert(name.clone());
            }
            if let Some(value) = ausdruck(&ts, &mut at, &vars).filter(|_| ts.get(at).is_some_and(|t| t == ";")) {
                if vars.get(&name).is_some_and(|old| old != &value) {
                    vars.remove(&name);
                    unknown.insert(name.clone());
                }
                if !unknown.contains(&name) {
                    vars.insert(name, value);
                }
            } else {
                vars.remove(&name);
            }
        }
        if let Some(receiver) = ts[i].strip_suffix(".append") {
            let mut at = i;
            if let Some(value) = atom(&ts, &mut at, &vars).filter(|_| ts.get(at).is_some_and(|t| t == ";")) {
                vars.insert(receiver.into(), value);
            } else {
                vars.remove(receiver);
            }
        }
        let method = ts[i].rsplit('.').next().unwrap_or("");
        if ["prepareStatement", "execute", "executeQuery", "executeUpdate", "addBatch"].contains(&method)
            && ts.get(i + 1).is_some_and(|t| t == "(")
        {
            jdbc = true;
            let mut at = i + 2;
            if let Some(sql) = ausdruck(&ts, &mut at, &vars).filter(|s| crate::dev::java::source::looks_like_sql(s)) {
                if ts.get(at).is_some_and(|t| t == ")" || t == ",") {
                    if method == "prepareStatement" && i >= 2 && ts[i - 1] == "=" {
                        receivers.entry(sql.clone()).or_default().insert(ts[i - 2].clone());
                    }
                    out.push(sql);
                }
            }
        }
    }
    if !jdbc {
        out.extend(
            vars.iter().filter(|(n, s)| !builders.contains(*n) && crate::dev::java::source::looks_like_sql(s)).map(|(_, s)| s.clone()),
        );
    }

    out.sort();
    out.dedup();
    out.into_iter()
        .map(|sql| {
            let receiver = receivers.get(&sql).filter(|r| r.len() == 1).and_then(|r| r.iter().next()).cloned();
            (sql, receiver)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variablen_builder_und_mehrere_statements() {
        let text = r#"final String table = "kunde"; String sql = "INSERT INTO " + table + " (id) VALUES (?)";
            PreparedStatement a = c.prepareStatement(sql); PreparedStatement b = c.prepareStatement("SELECT id FROM " + table);
            StringBuilder q = new StringBuilder("SELECT name"); q.append(" FROM kunde"); c.executeQuery(q.toString());"#;
        let q = abfragen(text);
        assert!(q.contains(&("INSERT INTO kunde (id) VALUES (?)".into(), Some("a".into()))));
        assert!(q.contains(&("SELECT id FROM kunde".into(), Some("b".into()))));
        assert!(q.iter().any(|(s, _)| s == "SELECT name FROM kunde"));
    }
    #[test]
    fn dynamische_bezeichner_werden_nicht_geraten() {
        let q = abfragen(r#"String table = leseEingabe(); c.prepareStatement("SELECT id FROM " + table);"#);
        assert!(q.is_empty());
        assert!(abfragen(r#"StringBuilder q = new StringBuilder("SELECT id FROM kunde"); q.append(eingabe); c.executeQuery(q.toString());"#).is_empty());
        assert!(abfragen(r#"String sql = "SELECT id FROM kunde"; sql += eingabe; c.executeQuery(sql);"#).is_empty());
        assert_eq!(literal("\"\"\"\nSELECT \"name\" FROM kunde\n\"\"\"").unwrap(), "SELECT \"name\" FROM kunde\n");
        assert_eq!(literal(r#""SELECT id\nFROM kunde""#).unwrap(), "SELECT id\nFROM kunde");
    }
}
