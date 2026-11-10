// Hinweise beim Zeigen mit der Maus: Signatur farbig, darunter die Beschreibung. Java (Methoden, Felder,
// Variablen, Klassen, Enum-Werte mit Javadoc) und SQL (Tabellen mit Spalten, Spalten mit Typ und Schluesseln,
// Funktionen). Der Java-Sprachserver ergaenzt, sobald er bereit ist.

use super::Lang;
use super::highlight::is_word_char;
use crate::db::Schema;
use eframe::egui;

/// Wort an `index`: (von, bis, Text)
pub fn word_at(text: &str, index: usize) -> Option<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if index > chars.len() {
        return None;
    }
    let mut a = index;
    while a > 0 && is_word_char(chars[a - 1]) {
        a -= 1;
    }
    let mut b = index;
    while b < chars.len() && is_word_char(chars[b]) {
        b += 1;
    }
    (b > a && !chars[a].is_ascii_digit()).then(|| (a, b, chars[a..b].iter().collect()))
}

/// Karte am Mauszeiger
pub fn card(ctx: &egui::Context, layer: egui::LayerId, id: egui::Id, title: &str, body: &str, lang: Lang) {
    let pal = crate::style::pal();
    egui::Tooltip::always_open(ctx.clone(), layer, id, egui::PopupAnchor::Pointer).show(|ui| {
        ui.set_max_width(560.0);
        if !title.is_empty() {
            let font = egui::FontId::monospace(super::font_size());
            let mut job = super::lang::highlight_lang(title, font, lang);
            job.wrap.max_width = 540.0;
            ui.label(job);
        }
        if !body.is_empty() {
            if !title.is_empty() {
                ui.add_space(4.0);
                ui.separator();
            }
            ui.label(egui::RichText::new(body).color(pal.text));
        }
    });
}

/// Kurze Beschreibung haeufiger SQL-Funktionen
fn sql_function_doc(f: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match f {
        "COUNT" => ("COUNT(expr)", "Anzahl der Zeilen (ohne NULL-Werte; COUNT(*) zählt alle).", "Number of rows (without NULL values; COUNT(*) counts all)."),
        "SUM" => ("SUM(expr)", "Summe der Werte.", "Sum of the values."),
        "AVG" => ("AVG(expr)", "Durchschnitt der Werte.", "Average of the values."),
        "MIN" => ("MIN(expr)", "Kleinster Wert.", "Smallest value."),
        "MAX" => ("MAX(expr)", "Größter Wert.", "Largest value."),
        "NOW" => ("NOW()", "Aktuelles Datum und Uhrzeit.", "Current date and time."),
        "CURDATE" => ("CURDATE()", "Heutiges Datum.", "Today's date."),
        "CONCAT" => ("CONCAT(a, b, …)", "Hängt Texte aneinander.", "Joins texts together."),
        "CONCAT_WS" => ("CONCAT_WS(sep, a, b, …)", "Hängt Texte mit Trennzeichen aneinander.", "Joins texts with a separator."),
        "LENGTH" => ("LENGTH(text)", "Länge in Bytes.", "Length in bytes."),
        "CHAR_LENGTH" => ("CHAR_LENGTH(text)", "Länge in Zeichen.", "Length in characters."),
        "UPPER" => ("UPPER(text)", "In Großbuchstaben.", "To upper case."),
        "LOWER" => ("LOWER(text)", "In Kleinbuchstaben.", "To lower case."),
        "SUBSTRING" => ("SUBSTRING(text, start, len)", "Teil eines Textes (Start bei 1).", "Part of a text (start at 1)."),
        "TRIM" => ("TRIM(text)", "Entfernt Leerzeichen am Anfang und Ende.", "Removes spaces at the start and end."),
        "REPLACE" => ("REPLACE(text, from, to)", "Ersetzt alle Vorkommen.", "Replaces all occurrences."),
        "ROUND" => ("ROUND(x, digits)", "Rundet auf die Stellen.", "Rounds to the digits."),
        "IFNULL" => ("IFNULL(x, fallback)", "Ersatzwert, wenn x NULL ist.", "Fallback when x is NULL."),
        "COALESCE" => ("COALESCE(a, b, …)", "Erster Wert, der nicht NULL ist.", "First value that is not NULL."),
        "IF" => ("IF(cond, then, else)", "Wert je nach Bedingung.", "Value depending on the condition."),
        "DATE_FORMAT" => ("DATE_FORMAT(date, '%d.%m.%Y')", "Datum als Text formatieren.", "Format a date as text."),
        "DATEDIFF" => ("DATEDIFF(a, b)", "Tage zwischen zwei Daten.", "Days between two dates."),
        "YEAR" => ("YEAR(date)", "Jahr eines Datums.", "Year of a date."),
        "MONTH" => ("MONTH(date)", "Monat eines Datums.", "Month of a date."),
        "DAY" => ("DAY(date)", "Tag eines Datums.", "Day of a date."),
        "GROUP_CONCAT" => ("GROUP_CONCAT(expr SEPARATOR ', ')", "Werte einer Gruppe als ein Text.", "Values of a group as one text."),
        "LAST_INSERT_ID" => ("LAST_INSERT_ID()", "Id der zuletzt eingefügten Zeile.", "Id of the last inserted row."),
        "ABS" => ("ABS(x)", "Betrag.", "Absolute value."),
        "RAND" => ("RAND()", "Zufallszahl zwischen 0 und 1.", "Random number between 0 and 1."),
        _ => return None,
    })
}

/// SQL: Tabelle, Spalte oder Funktion unter dem Mauszeiger
pub fn sql_info(text: &str, index: usize, schema: Option<&Schema>) -> Option<(String, String)> {
    use crate::i18n::tr;
    let (_, b, word) = word_at(text, index)?;
    let chars: Vec<char> = text.chars().collect();
    let call = chars[b..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
    let upper = word.to_uppercase();
    if call && let Some((sig, de, en)) = sql_function_doc(&upper) {
        return Some((sig.into(), tr(de, en).into()));
    }
    let schema = schema?;
    if let Some(t) = schema.tables.iter().find(|t| t.name.eq_ignore_ascii_case(&word)) {
        let mut body = String::new();
        for c in t.columns.iter().take(14) {
            let key = if c.is_pk() { "  PK" } else if c.key == "UNI" { "  UNIQUE" } else { "" };
            body.push_str(&format!("{}  {}{}{}\n", c.name, c.col_type, if c.nullable { "" } else { " NOT NULL" }, key));
        }
        if t.columns.len() > 14 {
            body.push_str(&crate::tr_format!("… und {} weitere\n", "… and {} more\n", t.columns.len() - 14));
        }
        for fk in schema.fks.iter().filter(|f| f.table == t.name) {
            body.push_str(&format!("{} → {}.{}\n", fk.columns.join(", "), fk.ref_table, fk.ref_columns.join(", ")));
        }
        if let Some(r) = t.rows {
            body.push_str(&crate::tr_format!("≈ {} Zeilen", "≈ {} rows", r));
        }
        let kind = if t.is_view { "VIEW" } else { "TABLE" };
        return Some((format!("{kind} {}.{}", schema.name, t.name), body.trim_end().to_string()));
    }
    // Spalte: zuerst in den Tabellen dieser Anweisung, sonst in allen
    let lower = text.to_lowercase();
    let mut hits: Vec<(&crate::db::TableInfo, &crate::db::ColumnInfo)> = Vec::new();
    for t in &schema.tables {
        if let Some(c) = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&word)) {
            hits.push((t, c));
        }
    }
    hits.sort_by_key(|(t, _)| !lower.contains(&t.name.to_lowercase()));
    let (t, c) = hits.first()?;
    let mut body = String::new();
    if c.is_pk() {
        body.push_str(tr("Primärschlüssel", "Primary key"));
        body.push('\n');
    }
    if let Some(fk) = schema.fks.iter().find(|f| f.table == t.name && f.columns.iter().any(|x| x.eq_ignore_ascii_case(&word))) {
        body.push_str(&crate::tr_format!("Verweist auf {}.{}\n", "References {}.{}\n", fk.ref_table, fk.ref_columns.join(", ")));
    }
    if let Some(d) = &c.default {
        body.push_str(&format!("DEFAULT {d}\n"));
    }
    if !c.extra.is_empty() {
        body.push_str(&format!("{}\n", c.extra));
    }
    if !c.comment.is_empty() {
        body.push_str(&c.comment);
    }
    if hits.len() > 1 {
        body.push_str(&crate::tr_format!("Auch in: {}", "Also in: {}", hits[1..].iter().map(|(t, _)| t.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Some((format!("{}.{} {}{}", t.name, c.name, c.col_type, if c.nullable { "" } else { " NOT NULL" }), body.trim_end().to_string()))
}

/// Java: Mitglied, Klasse oder Variable dieser Datei unter dem Mauszeiger
pub fn java_info(text: &str, index: usize) -> Option<(String, String)> {
    use crate::dev::java::outline::{self, Kind};
    use crate::i18n::tr;
    let (a, _, word) = word_at(text, index)?;
    if crate::dev::java::source::KEYWORDS.contains(&word.as_str()) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let classes = outline::outline(text);
    // Qualifiziert: Farbe.ROT, this.name, Klasse.methode
    let qualifier: Option<String> = (a > 0 && chars[a - 1] == '.').then(|| {
        let mut q = a - 1;
        while q > 0 && is_word_char(chars[q - 1]) {
            q -= 1;
        }
        chars[q..a - 1].iter().collect()
    });
    let here = outline::class_at(&classes, a);
    let owner = match qualifier.as_deref() {
        Some("this") | None => here,
        Some(q) => classes.iter().find(|c| c.name == q),
    };
    if let Some(c) = owner
        && let Some(m) = c.members.iter().find(|m| m.name == word && (qualifier.is_some() || m.kind != Kind::Constructor || m.name == c.name))
    {
        let mods = if m.is_static && m.kind != Kind::Constant { "static " } else { "" };
        let what = match m.kind {
            Kind::Method => tr("Methode in", "Method in"),
            Kind::Constructor => tr("Konstruktor von", "Constructor of"),
            Kind::Field => tr("Feld in", "Field in"),
            Kind::Constant => tr("Wert von", "Value of"),
        };
        let mut body = format!("{what} {}", c.name);
        if let Some(d) = &m.doc {
            body = format!("{d}\n\n{body}");
        }
        return Some((format!("{mods}{}", outline::signature(m)), body));
    }
    if let Some(c) = classes.iter().find(|c| c.name == word) {
        let mut body = crate::tr_format!("{} Mitglieder", "{} members", c.members.len());
        if let Some(d) = &c.doc {
            body = format!("{d}\n\n{body}");
        }
        return Some((format!("{} {}", c.kind, c.name), body));
    }
    if qualifier.is_none()
        && let Some((n, t, _)) = outline::locals(text, a + 1).into_iter().rev().find(|(n, _, _)| *n == word)
    {
        return Some((format!("{t} {n}"), tr("Lokale Variable", "Local variable").into()));
    }
    None
}

/// Antwort des Sprachservers in Signatur (Code-Block) und Beschreibung trennen
pub fn split_lsp(text: &str) -> (String, String) {
    let mut code = Vec::new();
    let mut rest = Vec::new();
    let mut in_code = false;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code { code.push(l) } else { rest.push(l) }
    }
    let body = rest.join("\n").replace("**", "").replace('`', "").trim().to_string();
    (code.join("\n").trim().to_string(), body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ColumnInfo, TableInfo};

    #[test]
    fn java_members_locals_and_enums() {
        let src = "class A {\n    enum Farbe { ROT }\n    /** Das Alter in Jahren. */\n    private int alter;\n    static int summe(int a, int b) { return a + b; }\n    void m() {\n        String s = \"x\";\n        int x = summe(alter, 2) + s.length();\n        Farbe f = Farbe.ROT;\n    }\n}\n";
        let at = |w: &str| src[..src.rfind(w).unwrap()].chars().count() + 1;
        assert_eq!(java_info(src, at("summe(alter")).unwrap().0, "static int summe(int a, int b)");
        let (t, b) = java_info(src, at("alter, 2")).unwrap();
        assert_eq!(t, "int alter");
        assert!(b.starts_with("Das Alter in Jahren."), "{b}");
        assert_eq!(java_info(src, at("s.length")).unwrap().0, "String s");
        assert_eq!(java_info(src, at("ROT;")).unwrap().0, "Farbe ROT");
        assert!(java_info(src, at("void")).is_none());
    }

    #[test]
    fn sql_tables_columns_and_functions() {
        let schema = Schema {
            name: "shop".into(),
            tables: vec![TableInfo {
                name: "kunde".into(),
                columns: vec![
                    ColumnInfo { name: "id".into(), col_type: "int(11)".into(), key: "PRI".into(), ..Default::default() },
                    ColumnInfo { name: "name".into(), col_type: "varchar(50)".into(), nullable: true, ..Default::default() },
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let sql = "SELECT COUNT(*), name FROM kunde WHERE id = 1";
        let at = |w: &str| sql.find(w).unwrap() + 1;
        let (t, b) = sql_info(sql, at("kunde"), Some(&schema)).unwrap();
        assert_eq!(t, "TABLE shop.kunde");
        assert!(b.contains("id  int(11) NOT NULL") && b.contains("name  varchar(50)"), "{b}");
        assert_eq!(sql_info(sql, at("id ="), Some(&schema)).unwrap().0, "kunde.id int(11) NOT NULL");
        assert_eq!(sql_info(sql, at("COUNT"), None).unwrap().0, "COUNT(expr)");
        assert!(sql_info(sql, at("WHERE"), Some(&schema)).is_none());
        assert_eq!(split_lsp("```java\nint summe(int a)\n```\nAddiert **zwei** Zahlen."), ("int summe(int a)".to_string(), "Addiert zwei Zahlen.".to_string()));
    }
}
