// Korrekturen, fuer die der Serverfehler den Ausschlag gibt: Werte in die Form der Spalte bringen
// ('9,50 €' -> 9.50, 'ja' -> TRUE, '8 Uhr' -> '08:00:00'), Aggregat im Aggregat, fremde Tabelle per JOIN
// ergaenzen, doppelte Spalten in INSERT, Tabelle ohne Spalten.

use super::{ErrorFix, edit, fix, quoted_after};
use crate::db::{Schema, TableInfo};
use crate::sqledit::dialect::{group_end, insert_parts, items, text};
use crate::sqledit::lint::{Kind, Tok, known_word, quote_if_needed};

/// Spalte aus "… for column `db`.`t`.`c` at row 1" bzw. "… for column 'c' at row 1"
fn column_of(err: &str) -> Option<String> {
    let r = &err[err.find("for column ")? + 11..];
    let raw = r.split(" at row").next()?.trim();
    raw.rsplit('.').next().map(|c| c.trim_matches(['\'', '`']).to_string()).filter(|c| !c.is_empty())
}

/// Wert in der Form, die die Spalte annimmt; None, wenn keine sichere Umwandlung bekannt ist
pub(crate) fn convert(v: &str, col_type: &str, nullable: bool) -> Option<String> {
    let t = col_type.to_ascii_lowercase();
    let s = v.trim();
    let boolean = t.starts_with("tinyint(1)") || t.starts_with("bool") || t == "bit(1)";
    if boolean {
        return match s.to_lowercase().as_str() {
            "ja" | "yes" | "true" | "wahr" | "j" | "y" | "x" | "on" | "an" => Some("TRUE".into()),
            "nein" | "no" | "false" | "falsch" | "n" | "off" | "aus" | "" => Some("FALSE".into()),
            _ => None,
        };
    }
    let int = ["int", "bigint", "smallint", "mediumint", "tinyint", "year"].iter().any(|k| t.starts_with(k));
    let numeric = int || ["decimal", "numeric", "float", "double", "real"].iter().any(|k| t.starts_with(k));
    if numeric {
        if s.is_empty() {
            return Some(if nullable { "NULL" } else { "0" }.into());
        }
        // Waehrung, Prozent und Leerzeichen daneben weg
        let mut n = s.to_string();
        for sym in ["€", "$", "£", "EUR", "CHF", "Fr.", "%", "Euro"] {
            n = n.replace(sym, "");
        }
        let n = n.trim().replace('\'', "");
        if n.is_empty() || !n.chars().enumerate().all(|(i, c)| c.is_ascii_digit() || c == '.' || c == ',' || c == '-' && i == 0) {
            return None;
        }
        let (dots, commas) = (n.matches('.').count(), n.matches(',').count());
        let groups_of_three = |sep: char| {
            let parts: Vec<&str> = n.trim_start_matches('-').split(sep).collect();
            parts.len() > 1 && !parts[0].is_empty() && parts[0].len() <= 3 && parts[1..].iter().all(|p| p.len() == 3)
        };
        let plain = match (dots, commas) {
            (0, 0) => n.clone(),
            // 1.500,50 bzw. 1,500.50: das letzte Zeichen trennt die Nachkommastellen
            (_, _) if dots > 0 && commas > 0 => {
                let last = n.rfind(['.', ',']).unwrap_or(0);
                let (a, b) = n.split_at(last);
                format!("{}.{}", a.replace(['.', ','], ""), &b[1..])
            }
            (0, 1) if !(int && groups_of_three(',')) => n.replace(',', "."),
            (0, _) if groups_of_three(',') => n.replace(',', ""),
            (_, 0) if dots > 1 && groups_of_three('.') || int && groups_of_three('.') => n.replace('.', ""),
            (1, 0) => n.clone(),
            _ => return None,
        };
        let x: f64 = plain.parse().ok()?;
        if int && x.fract() != 0.0 {
            return None;
        }
        return (plain != s).then_some(plain);
    }
    if t.starts_with("time") {
        // 8 Uhr, 8:30 Uhr, 8.30 Uhr, 8:30h
        let lower = s.to_lowercase();
        let core = lower.trim_end_matches("uhr").trim_end_matches('h').trim();
        let (h, m) = core.split_once([':', '.']).unwrap_or((core, "0"));
        let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
        return (h < 24 && m < 60 && core != s).then(|| format!("'{h:02}:{m:02}:00'"));
    }
    None
}

/// Werte, die die Anweisung in `col` schreibt (INSERT-Liste oder SET col = …): Token-Indizes
fn written(toks: &[Tok], table: Option<&TableInfo>, col: &str) -> Vec<usize> {
    let mut out = Vec::new();
    if let Some((ti, vi)) = insert_parts(toks) {
        let pos = if toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open) {
            let Some(close) = group_end(toks, ti + 1) else { return out };
            items(toks, ti + 1, close).iter().position(|(a, _)| toks[*a].text.eq_ignore_ascii_case(col))
        } else {
            table.and_then(|t| t.columns.iter().filter(|c| !c.is_generated()).position(|c| c.name.eq_ignore_ascii_case(col)))
        };
        let Some(pos) = pos else { return out };
        let mut k = vi + 1;
        while toks.get(k).is_some_and(|t| t.kind == Kind::Open) {
            let Some(c) = group_end(toks, k) else { break };
            if let Some(&(a, b)) = items(toks, k, c).get(pos)
                && b == a + 1
            {
                out.push(a);
            }
            k = c + 1;
            if toks.get(k).is_some_and(|t| t.kind == Kind::Comma) {
                k += 1;
            }
        }
        return out;
    }
    for (k, t) in toks.iter().enumerate() {
        if t.is_name() && t.text.eq_ignore_ascii_case(col) && toks.get(k + 1).is_some_and(|o| o.kind == Kind::Op && o.text == "=") && toks.get(k + 3).is_none_or(|n| n.kind == Kind::Comma || n.is_word("WHERE")) {
            out.push(k + 2);
        }
    }
    out
}

/// Werte in die Form der Zielspalte bringen (1366, 1265, 1292)
pub(super) fn value_fixes(err: &str, chars: &[char], toks: &[Tok], schema: Option<&Schema>) -> Vec<ErrorFix> {
    let Some(col) = column_of(err) else { return Vec::new() };
    let table_name = insert_parts(toks).map(|(ti, _)| toks[ti].text.clone()).or_else(|| toks.get(1).filter(|_| toks[0].is_word("UPDATE")).map(|t| t.text.clone()));
    let table = schema.zip(table_name).and_then(|(s, n)| s.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&n)));
    let Some(info) = table.and_then(|t| t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&col))) else { return Vec::new() };
    let only = quoted_after(err, "value: '");
    let mut edits = Vec::new();
    for k in written(toks, table, &col) {
        let t = &toks[k];
        if t.kind != Kind::Str || t.end < t.start + 2 {
            continue;
        }
        let v = text(chars, t.start + 1, t.end - 1);
        if only.is_some_and(|o| o != v) {
            continue;
        }
        if let Some(new) = convert(&v, &info.col_type, info.nullable) {
            edits.push((t.start, t.end, new));
        }
    }
    if edits.is_empty() {
        return Vec::new();
    }
    let example = edits[0].2.clone();
    vec![fix(crate::tr_format!("Wert passend für {col} ({}) schreiben: {example}", "Write the value to fit {col} ({}): {example}", info.col_type), edit(chars, edits))]
}

/// MAX(COUNT(*)) … -> SELECT MAX(wert) FROM (SELECT COUNT(*) AS wert …) AS t
pub(super) fn nested_aggregate(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    const AGG: [&str; 5] = ["MAX", "MIN", "AVG", "SUM", "COUNT"];
    if !toks.first()?.is_word("SELECT") {
        return None;
    }
    let outer = toks.get(1).filter(|t| AGG.iter().any(|a| t.is_word(a)))?;
    let open = 2;
    let close = group_end(toks, open)?;
    let inner = toks.get(3).filter(|t| AGG.iter().any(|a| t.is_word(a)))?;
    let inner_close = group_end(toks, 4)?;
    if inner_close + 1 != close || !toks.get(close + 1).is_some_and(|t| t.is_word("FROM") || t.is_word("AS")) {
        return None;
    }
    let mut from = close + 1;
    let mut alias = String::new();
    if toks[from].is_word("AS") {
        alias = format!(" AS {}", toks.get(from + 1)?.text);
        from += 2;
    }
    if !toks.get(from).is_some_and(|t| t.is_word("FROM")) {
        return None;
    }
    let name = if inner.is_word("COUNT") { "anzahl" } else { "wert" };
    let rest = text(chars, toks[from].start, toks.last()?.end);
    let inner_text = text(chars, inner.start, toks[inner_close].end);
    let new = format!("SELECT {}({name}){alias} FROM (SELECT {inner_text} AS {name} {rest}) AS t", outer.text.to_ascii_uppercase());
    Some(fix(
        crate::tr_format!("{}({}(…)) über eine Unterabfrage berechnen", "Compute {}({}(…)) through a subquery", outer.text.to_ascii_uppercase(), inner.text.to_ascii_uppercase()),
        new,
    ))
}

/// SELECT schueler.name, klasse.name FROM schueler -> … JOIN klasse ON … (ueber den Fremdschluessel)
pub(super) fn join_missing_table(chars: &[char], toks: &[Tok], schema: &Schema, qual: &str) -> Option<ErrorFix> {
    let target = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(qual))?;
    let from = toks.iter().position(|t| t.is_word("FROM"))?;
    let base = toks.get(from + 1).filter(|t| t.is_name())?;
    if toks.iter().any(|t| t.is_word("JOIN")) || toks.get(from + 2).is_some_and(|t| t.kind == Kind::Comma) {
        return None;
    }
    let base_alias = toks.get(from + 2).filter(|t| t.kind == Kind::Word && !known_word(&t.upper())).map_or(base.text.clone(), |a| a.text.clone());
    let fk = schema.fks.iter().find(|f| f.table.eq_ignore_ascii_case(&base.text) && f.ref_table.eq_ignore_ascii_case(&target.name))
        .map(|f| (f.columns.first().cloned(), f.ref_columns.first().cloned(), true))
        .or_else(|| schema.fks.iter().find(|f| f.ref_table.eq_ignore_ascii_case(&base.text) && f.table.eq_ignore_ascii_case(&target.name)).map(|f| (f.columns.first().cloned(), f.ref_columns.first().cloned(), false)))?;
    let (Some(c), Some(r), forward) = fk else { return None };
    let t = quote_if_needed(&target.name);
    let on = if forward { format!("{base_alias}.{c} = {t}.{r}") } else { format!("{t}.{c} = {base_alias}.{r}") };
    // nach Tabelle (und Alias), vor WHERE/GROUP/ORDER/LIMIT
    let mut at = toks.last()?.end;
    for x in &toks[from + 2..] {
        if ["WHERE", "GROUP", "ORDER", "HAVING", "LIMIT"].iter().any(|w| x.is_word(w)) {
            at = x.start;
            break;
        }
    }
    let ins = if at == toks.last()?.end { format!(" JOIN {t} ON {on}") } else { format!("JOIN {t} ON {on} ") };
    Some(fix(crate::tr_format!("Tabelle {} dazunehmen (JOIN über {on})", "Join the table {} (JOIN on {on})", target.name), edit(chars, vec![(at, at, ins)])))
}

/// INSERT INTO t (a, a) VALUES (1, 2): zweite Spalte samt Werten weglassen
pub(super) fn drop_duplicate_insert_column(chars: &[char], toks: &[Tok], col: &str) -> Option<ErrorFix> {
    let (ti, vi) = insert_parts(toks)?;
    let close = group_end(toks, ti + 1).filter(|_| toks.get(ti + 1).is_some_and(|t| t.kind == Kind::Open))?;
    let cols = items(toks, ti + 1, close);
    let pos = cols.iter().enumerate().filter(|(_, (a, _))| toks[*a].text.eq_ignore_ascii_case(col)).nth(1).map(|(n, _)| n)?;
    let cut = |list: &[(usize, usize)], n: usize| {
        let (a, b) = list[n];
        (toks[a - 1].start, toks[b - 1].end, String::new())
    };
    let mut edits = vec![cut(&cols, pos)];
    let mut k = vi + 1;
    while toks.get(k).is_some_and(|t| t.kind == Kind::Open) {
        let c = group_end(toks, k)?;
        let its = items(toks, k, c);
        if its.len() > pos {
            edits.push(cut(&its, pos));
        }
        k = c + 1;
        if toks.get(k).is_some_and(|t| t.kind == Kind::Comma) {
            k += 1;
        }
    }
    Some(fix(crate::tr_format!("Spalte {col} nur einmal angeben", "Name the column {col} only once"), edit(chars, edits)))
}

/// CREATE TABLE t; -> CREATE TABLE t (id INT PRIMARY KEY AUTO_INCREMENT)
pub(super) fn table_without_columns(chars: &[char], toks: &[Tok]) -> Option<ErrorFix> {
    let k = toks.iter().position(|t| t.is_word("TABLE"))?;
    let mut n = k + 1;
    if toks.get(n).is_some_and(|t| t.is_word("IF")) {
        n += 3;
    }
    if toks.get(n + 1).is_some_and(|t| t.kind == Kind::Dot) {
        n += 2;
    }
    let name = toks.get(n).filter(|t| t.is_name())?;
    if toks.get(n + 1).is_some_and(|t| t.kind == Kind::Open) {
        return None;
    }
    Some(fix(
        crate::i18n::tr("Tabelle mit einer ID-Spalte anlegen", "Create the table with an ID column").into(),
        edit(chars, vec![(name.end, name.end, " (id INT PRIMARY KEY AUTO_INCREMENT)".into())]),
    ))
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn values_fit_their_column() {
        assert_eq!(convert("9,50 €", "decimal(8,2)", true).as_deref(), Some("9.50"));
        assert_eq!(convert("1.500,50", "decimal(8,2)", true).as_deref(), Some("1500.50"));
        assert_eq!(convert("1,500.50", "decimal(8,2)", true).as_deref(), Some("1500.50"));
        assert_eq!(convert("1.500", "int(11)", true).as_deref(), Some("1500"));
        assert_eq!(convert("19%", "int(11)", true).as_deref(), Some("19"));
        assert_eq!(convert("", "int(11)", true).as_deref(), Some("NULL"));
        assert_eq!(convert("", "int(11)", false).as_deref(), Some("0"));
        assert_eq!(convert("0664 1234567", "int(11)", true), None);
        assert_eq!(convert("2,5", "int(11)", true), None);
        assert_eq!(convert("ja", "tinyint(1)", true).as_deref(), Some("TRUE"));
        assert_eq!(convert("Nein", "tinyint(1)", true).as_deref(), Some("FALSE"));
        assert_eq!(convert("8 Uhr", "time", true).as_deref(), Some("'08:00:00'"));
        assert_eq!(convert("8.30 Uhr", "time", true).as_deref(), Some("'08:30:00'"));
        assert_eq!(convert("viele", "int(11)", true), None);
    }
}
