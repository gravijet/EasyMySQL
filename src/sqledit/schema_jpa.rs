// JPA-Feld-/Property-Zugriff mit expliziten Namen, Typen, IDs und JoinColumn auswerten.
use super::schema_decl::{TabellenPlan, migrationen};
use crate::db::{ColDef, q};
use crate::dev::java::source::{self, Span};
use std::collections::BTreeMap;

type Attribute = BTreeMap<String, Vec<String>>;
type Annotationen = BTreeMap<String, Attribute>;

pub(super) fn java_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for (kind, a, b) in source::spans(&chars) {
        match kind {
            Span::Comment | Span::Char => {}
            Span::Str => out.push(chars[a..b].iter().collect()),
            Span::Code => {
                let mut i = a;
                while i < b {
                    if chars[i].is_whitespace() {
                        i += 1;
                        continue;
                    }
                    let start = i;
                    i += 1;
                    if chars[start].is_alphanumeric() || chars[start] == '_' || chars[start] == '$' {
                        while i < b && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$' | '.')) {
                            i += 1;
                        }
                    }
                    out.push(chars[start..i].iter().collect());
                }
            }
        }
    }
    out
}

fn gruppe(ts: &[String], at: &mut usize, open: &str, close: &str) -> Option<Vec<String>> {
    if ts.get(*at)? != open {
        return None;
    }
    *at += 1;
    let start = *at;
    let mut depth = 1;
    while *at < ts.len() {
        if ts[*at] == open {
            depth += 1;
        }
        if ts[*at] == close {
            depth -= 1;
        }
        if depth == 0 {
            let out = ts[start..*at].to_vec();
            *at += 1;
            return Some(out);
        }
        *at += 1;
    }
    None
}

fn attribute(ts: &[String]) -> Option<Attribute> {
    let mut out = Attribute::new();
    let mut i = 0;
    if !ts.is_empty() && ts.get(1).is_none_or(|v| v != "=") {
        out.insert("value".into(), ts.to_vec());
        return Some(out);
    }
    while i < ts.len() {
        let name = ts.get(i)?.clone();
        if ts.get(i + 1)? != "=" {
            return None;
        }
        i += 2;
        let start = i;
        let mut depth = 0i32;
        while i < ts.len() {
            match ts[i].as_str() {
                "(" | "{" => depth += 1,
                ")" | "}" => depth -= 1,
                _ => {}
            }
            if ts[i] == "," && depth == 0 {
                break;
            }
            i += 1;
        }
        if depth != 0 || start == i || out.insert(name, ts[start..i].to_vec()).is_some() {
            return None;
        }
        if i < ts.len() {
            i += 1;
        }
    }
    Some(out)
}

fn annotation(ts: &[String], at: &mut usize, anns: &mut Annotationen) -> Option<()> {
    if ts.get(*at)? != "@" {
        return None;
    }
    *at += 1;
    let name = ts.get(*at)?.rsplit('.').next()?.to_string();
    *at += 1;
    let attrs = if ts.get(*at).is_some_and(|t| t == "(") { attribute(&gruppe(ts, at, "(", ")")?)? } else { Attribute::new() };
    anns.insert(name, attrs);
    Some(())
}

fn wert(attrs: &Attribute, key: &str) -> Option<String> {
    let values = attrs.get(key)?;
    if values.len() != 1 {
        return None;
    }
    let v = &values[0];
    if v.starts_with('"') { serde_json::from_str(v).ok() } else { Some(v.clone()) }
}
fn zahl(attrs: &Attribute, key: &str, default: usize) -> Option<usize> {
    match wert(attrs, key) {
        Some(v) => v.parse().ok(),
        None if attrs.contains_key(key) => None,
        None => Some(default),
    }
}

#[derive(Clone)]
struct Mitglied {
    anns: Annotationen,
    typ: String,
    name: String,
    methode: bool,
}
struct Modell {
    klasse: String,
    table: String,
    db: Option<String>,
    cols: Vec<ColDef>,
    links: Vec<(ColDef, String, Option<String>, bool)>,
    regeln: Vec<String>,
}

fn modell(klasse: String, anns: Annotationen, body: &[String], imports: &BTreeMap<String, String>) -> Option<Modell> {
    if !anns.contains_key("Entity")
        || ["Inheritance", "SecondaryTable", "SecondaryTables", "AttributeOverride", "AttributeOverrides"]
            .iter()
            .any(|a| anns.contains_key(*a))
    {
        return None;
    }
    let empty = Attribute::new();
    let table = anns.get("Table").unwrap_or(&empty);
    let name = wert(table, "name").filter(|s| !s.is_empty()).unwrap_or_else(|| {
        anns.get("Entity").and_then(|a| wert(a, "name")).unwrap_or_else(|| klasse.rsplit('.').next().unwrap_or(&klasse).to_string())
    });
    let db = wert(table, "schema").or_else(|| wert(table, "catalog")).filter(|s| !s.is_empty());
    let mut members = Vec::new();
    let mut i = 0;
    let mut annotations = Annotationen::new();
    while i < body.len() {
        if body[i] == "@" {
            annotation(body, &mut i, &mut annotations)?;
            continue;
        }
        let start = i;
        while i < body.len() && body[i] != ";" && body[i] != "{" {
            i += 1;
        }
        let declaration = &body[start..i];
        if !declaration.is_empty() {
            let mut words = declaration
                .iter()
                .filter(|t| !["public", "protected", "private", "final", "volatile", "synchronized", "abstract"].contains(&t.as_str()))
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            if words.get(1).is_some_and(|s| s == "[") && words.get(2).is_some_and(|s| s == "]") {
                words[0].push_str("[]");
                words.drain(1..3);
            }
            if words.iter().any(|s| matches!(s.as_str(), "class" | "enum" | "interface")) {
                words.clear();
            }
            if words.get(1).is_some_and(|s| s == "(") {
                words.clear();
            }
            if !words.iter().any(|t| matches!(t.as_str(), "static" | "transient")) && words.len() >= 2 {
                let method = words.get(2).is_some_and(|t| t.as_str() == "(");
                if !method || words.get(3).is_some_and(|t| t.as_str() == ")") {
                    if !method && words.iter().any(|t| t.as_str() == ",") {
                        return None;
                    }
                    members.push(Mitglied {
                        anns: std::mem::take(&mut annotations),
                        typ: words[0].to_string(),
                        name: words[1].to_string(),
                        methode: method,
                    });
                }
            }
        }
        annotations.clear();
        if body.get(i).is_some_and(|t| t == "{") {
            gruppe(body, &mut i, "{", "}")?;
        } else {
            i += 1;
        }
    }
    let property = members.iter().any(|m| m.methode && m.anns.contains_key("Id"));
    let mut model = Modell { klasse, table: name, db, cols: Vec::new(), links: Vec::new(), regeln: Vec::new() };
    for m in members.into_iter().filter(|m| m.methode == property && !m.anns.contains_key("Transient")) {
        if ["Embedded", "EmbeddedId", "ElementCollection", "Convert", "MapsId", "JoinTable", "JoinColumns"]
            .iter()
            .any(|a| m.anns.contains_key(*a))
        {
            return None;
        }
        if let Some(a) = m.anns.get("OneToMany").or_else(|| m.anns.get("ManyToMany")) {
            if wert(a, "mappedBy").is_some_and(|s| !s.is_empty()) {
                continue;
            }
            return None;
        }
        let field = if property {
            let Some(n) = m.name.strip_prefix("get").or_else(|| m.name.strip_prefix("is")) else { continue };
            let mut chars = n.chars();
            format!("{}{}", chars.next()?.to_lowercase(), chars.as_str())
        } else {
            m.name.clone()
        };
        if m.anns.contains_key("ManyToOne") || m.anns.contains_key("OneToOne") {
            let attrs = m.anns.get("JoinColumn").unwrap_or(&empty);
            let default_name = !attrs.contains_key("name");
            let col = wert(attrs, "name").unwrap_or(field);
            let mut def = ColDef::new(&col, "");
            def.not_null = wert(attrs, "nullable").as_deref() == Some("false")
                || m.anns
                    .get("ManyToOne")
                    .or_else(|| m.anns.get("OneToOne"))
                    .is_some_and(|a| wert(a, "optional").as_deref() == Some("false"));
            def.unique = wert(attrs, "unique").as_deref() == Some("true") || m.anns.contains_key("OneToOne");
            let target = imports.get(&m.typ).cloned().unwrap_or_else(|| {
                if m.typ.contains('.') {
                    m.typ.clone()
                } else if let Some((package, _)) = model.klasse.rsplit_once('.') {
                    format!("{package}.{}", m.typ)
                } else {
                    m.typ.clone()
                }
            });
            model.links.push((def, target, wert(attrs, "referencedColumnName"), default_name));
            continue;
        }
        let attrs = m.anns.get("Column").unwrap_or(&empty);
        if attrs.contains_key("table") || attrs.contains_key("options") {
            return None;
        }
        let colname = wert(attrs, "name").filter(|s| !s.is_empty()).unwrap_or(field);
        let typ = m.typ.rsplit('.').next()?;
        let ty = match typ {
            "byte" | "Byte" => "TINYINT".to_string(),
            "short" | "Short" => "SMALLINT".into(),
            "int" | "Integer" => "INT".into(),
            "long" | "Long" => "BIGINT".into(),
            "boolean" | "Boolean" => "BOOLEAN".into(),
            "float" | "Float" => "FLOAT".into(),
            "double" | "Double" => "DOUBLE".into(),
            "String" => {
                if m.anns.contains_key("Lob") {
                    "LONGTEXT".into()
                } else {
                    let len = zahl(attrs, "length", 255)?;
                    if len == 0 || len > 16383 {
                        return None;
                    }
                    format!("VARCHAR({len})")
                }
            }
            "BigDecimal" => {
                let p = zahl(attrs, "precision", 0)?;
                let s = zahl(attrs, "scale", 0)?;
                if p == 0 || p > 65 || s > 30 || s > p {
                    return None;
                }
                format!("DECIMAL({p},{s})")
            }
            "LocalDate" => "DATE".into(),
            "LocalTime" => "TIME".into(),
            "LocalDateTime" | "Timestamp" => "DATETIME(6)".into(),
            "UUID" => "UUID".into(),
            "byte[]" => {
                if m.anns.contains_key("Lob") {
                    "LONGBLOB".into()
                } else {
                    let n = zahl(attrs, "length", 255)?;
                    if n == 0 || n > 65535 {
                        return None;
                    }
                    format!("VARBINARY({n})")
                }
            }
            "Date" => match m.anns.get("Temporal").and_then(|a| wert(a, "value")).as_deref() {
                Some(v) if v.ends_with("DATE") => "DATE".into(),
                Some(v) if v.ends_with("TIME") => "TIME".into(),
                Some(v) if v.ends_with("TIMESTAMP") => "DATETIME(6)".into(),
                _ => return None,
            },
            _ if m.anns.contains_key("Enumerated") => {
                if m.anns.get("Enumerated").and_then(|a| wert(a, "value")).is_some_and(|v| v.ends_with("STRING")) {
                    "VARCHAR(255)".into()
                } else {
                    "INT".into()
                }
            }
            _ => return None,
        };
        let mut def = ColDef::new(&colname, &ty);
        def.primary = m.anns.contains_key("Id");
        def.not_null = def.primary || wert(attrs, "nullable").as_deref() == Some("false");
        def.unique = wert(attrs, "unique").as_deref() == Some("true");
        if let Some(a) = m.anns.get("ColumnDefault") {
            def.default = wert(a, "value")?;
        }
        def.not_null |= m.anns.get("Basic").is_some_and(|a| wert(a, "optional").as_deref() == Some("false"));
        if let Some(g) = m.anns.get("GeneratedValue") {
            if wert(g, "strategy").as_deref().is_none_or(|s| !s.ends_with("IDENTITY")) || !def.primary {
                return None;
            }
            def.auto_inc = true;
        }
        if let Some(custom) = wert(attrs, "columnDefinition") {
            let p = migrationen(&format!("CREATE TABLE x ({} {custom})", q(&colname))).ok()?;
            if p.len() != 1 || p[0].teile.len() != 1 {
                return None;
            }
            model.regeln.push(p[0].teile[0].sql.clone());
        } else {
            model.cols.push(def);
        }
    }
    for (key, expected) in [("uniqueConstraints", "UniqueConstraint"), ("indexes", "Index")] {
        if let Some(ts) = table.get(key) {
            let mut i = 0;
            while i < ts.len() {
                if ts[i] != "@" {
                    i += 1;
                    continue;
                }
                let mut ann = Annotationen::new();
                annotation(ts, &mut i, &mut ann)?;
                let attrs = ann.get(expected)?;
                let name = wert(attrs, "name").filter(|s| !s.is_empty()).map(|s| q(&s)).unwrap_or_default();
                let (unique, columns) = if expected == "UniqueConstraint" {
                    let columns = attrs
                        .get("columnNames")?
                        .iter()
                        .filter(|s| s.starts_with('"'))
                        .map(|s| serde_json::from_str::<String>(s).ok().map(|s| q(&s)))
                        .collect::<Option<Vec<_>>>()?;
                    (true, columns)
                } else {
                    let list = wert(attrs, "columnList")?;
                    let mut columns = Vec::new();
                    for col in list.split(',') {
                        let words = col.split_whitespace().collect::<Vec<_>>();
                        if words.is_empty()
                            || words.len() > 2
                            || (words.len() == 2 && !["ASC", "DESC"].contains(&words[1].to_uppercase().as_str()))
                        {
                            return None;
                        }
                        columns.push(format!("{}{}", q(words[0]), if words.len() == 2 { format!(" {}", words[1]) } else { String::new() }));
                    }
                    (wert(attrs, "unique").as_deref() == Some("true"), columns)
                };
                if columns.is_empty() {
                    return None;
                }
                model.regeln.push(format!("{} {name} ({})", if unique { "UNIQUE KEY" } else { "KEY" }, columns.join(", ")));
            }
        }
    }
    if model.cols.is_empty() && model.regeln.is_empty() {
        return None;
    }
    Some(model)
}

/// Beide JPA-Namensraeume, Feld-/Getter-Zugriff und explizite JoinColumns. Unklare Modelle auslassen.
pub fn jpa_modelle(texte: &[String]) -> Result<Vec<TabellenPlan>, String> {
    let mut models = Vec::new();
    for text in texte {
        if text.chars().count() > super::MAX_CHARS {
            return Err("JPA-Datei zu gross".into());
        }
        let ts = java_tokens(text);
        let package = ts.windows(2).find(|w| w[0] == "package").map(|w| w[1].clone());
        let imports = ts
            .windows(2)
            .filter(|w| w[0] == "import" && !w[1].ends_with('.'))
            .map(|w| (w[1].rsplit('.').next().unwrap().to_string(), w[1].clone()))
            .collect::<BTreeMap<_, _>>();
        if !ts.windows(2).any(|w| w[0] == "@" && w[1].rsplit('.').next() == Some("Entity")) {
            continue;
        }
        let mut at = 0;
        let mut anns = Annotationen::new();
        while at < ts.len() {
            if ts[at] == "@" {
                annotation(&ts, &mut at, &mut anns).ok_or("Ungueltige Annotation")?;
            } else if ts[at] == "class" {
                at += 1;
                let klasse = ts.get(at).ok_or("Klassenname fehlt")?.clone();
                let klasse = package.as_ref().map(|p| format!("{p}.{klasse}")).unwrap_or(klasse);
                at += 1;
                if ts.get(at).is_some_and(|t| t == "extends") && anns.contains_key("Entity") {
                    return Err("JPA-Vererbung braucht explizite Migration".into());
                }
                while at < ts.len() && ts[at] != "{" {
                    at += 1;
                }
                let body = gruppe(&ts, &mut at, "{", "}").ok_or("Klassenrumpf fehlt")?;
                if anns.contains_key("Entity") {
                    models.push(modell(klasse, std::mem::take(&mut anns), &body, &imports).ok_or("JPA-Modell nicht eindeutig")?);
                }
                anns.clear();
            } else {
                at += 1;
            }
        }
    }
    let mut out = Vec::new();
    for (i, model) in models.iter().enumerate() {
        if models[..i].iter().any(|m| m.klasse == model.klasse) {
            return Err("Mehrdeutige JPA-Klasse".into());
        }
        let mut cols = model.cols.clone();
        let mut fks = Vec::new();
        for (col, klasse, referenced, default_name) in &model.links {
            let target = models.iter().find(|m| m.klasse == *klasse).ok_or("JPA-Beziehungsziel fehlt")?;
            if target.db != model.db {
                return Err("Datenbankuebergreifende Beziehung".into());
            }
            let keys = target.cols.iter().filter(|c| c.primary).collect::<Vec<_>>();
            let reference = if let Some(n) = referenced {
                target.cols.iter().find(|c| c.name == *n).ok_or("Zielspalte fehlt")?
            } else {
                if keys.len() != 1 {
                    return Err("Zusammengesetzte Beziehung braucht Migration".into());
                }
                keys[0]
            };
            let mut col = col.clone();
            col.col_type = reference.col_type.clone();
            if *default_name {
                col.name = format!("{}_{}", col.name, reference.name);
            }
            fks.push(format!("FOREIGN KEY ({}) REFERENCES {} ({})", q(&col.name), q(&target.table), q(&reference.name)));
            if let Some(old) = cols.iter().find(|c| c.name == col.name) {
                if old.col_type != col.col_type {
                    return Err("Widerspruechliche JoinColumn".into());
                }
            } else {
                cols.push(col);
            }
        }
        let mut sql = crate::db::create_table_sql(model.db.as_deref().unwrap_or("jpa"), &model.table, &cols, "InnoDB", "");
        // Zusaetzliche validierte Spaltendefinitionen/Fremdschluessel vor die aeussere Klammer setzen.
        let extra = model.regeln.iter().chain(fks.iter()).cloned().collect::<Vec<_>>();
        if !extra.is_empty() {
            let at = sql.rfind(") ENGINE").ok_or("DDL fehlerhaft")?;
            sql.insert_str(at, &format!("{}\n  {}\n", if cols.is_empty() { "" } else { "," }, extra.join(",\n  ")));
        }
        let mut plans = migrationen(&sql).map_err(|_| "JPA-DDL nicht unterstuetzt")?;
        for p in &mut plans {
            p.datenbank = model.db.clone();
        }
        out.extend(plans);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beziehungen_respektieren_packages_imports_und_konstruktoren() {
        let sources = [
            r#"package a; @Entity @Table(name="kunde_a") class Kunde { public Kunde() {} @Id long id; }"#.into(),
            r#"package b; @Entity @Table(name="kunde_b") class Kunde { @Id long id; }"#.into(),
            r#"package b; import a.Kunde; @Entity @Table(name="auftrag") class Auftrag { @Id long id; @ManyToOne @JoinColumn(name="kunde_id") Kunde kunde; }"#.into(),
        ];
        let models = jpa_modelle(&sources).unwrap();
        let sql =
            models.iter().find(|p| p.tabelle == "auftrag").unwrap().teile.iter().map(|p| p.sql.clone()).collect::<Vec<_>>().join("\n");
        assert!(sql.contains("REFERENCES `kunde_a`"));
        assert!(!sql.contains("REFERENCES `kunde_b`"));
    }
    #[test]
    fn jpa_mit_id_namen_typen_und_beziehung() {
        let sources = vec![r#"import jakarta.persistence.*; @Entity @Table(name="kunde") class Kunde { public Kunde() {} @Id @GeneratedValue(strategy=GenerationType.IDENTITY) long id; @Column(name="anzeige",length=80,nullable=false,unique=true) String name; }"#.into(),
            r#"@Entity @Table(name="auftrag") class Auftrag { @Id long id; @ManyToOne @JoinColumn(name="kunde_id",nullable=false) Kunde kunde; @Column(precision=12,scale=2) BigDecimal preis; }"#.into()];
        let plans = jpa_modelle(&sources).unwrap();
        assert_eq!(plans.len(), 2);
        let sql = plans.iter().flat_map(|p| &p.teile).map(|p| p.sql.clone()).collect::<Vec<_>>().join("\n");
        assert!(sql.contains("AUTO_INCREMENT"));
        assert!(sql.contains("VARCHAR(80) NOT NULL"));
        assert!(sql.contains("REFERENCES `kunde`"));
        assert!(sql.contains("DECIMAL(12,2)"));
    }
    #[test]
    fn getter_zugriff_und_unklare_modelle() {
        let s =
            r#"@Entity class Kunde { private long id; @Id public long getId() { return id; } public String getName() { return "x"; } }"#;
        assert_eq!(jpa_modelle(&[s.into()]).unwrap()[0].teile.iter().filter(|t| t.name.is_some()).count(), 2);
        assert!(jpa_modelle(&["@Entity class X { @Id long id; BigDecimal wert; }".into()]).is_err());
        assert!(jpa_modelle(&["@Entity class X extends Basis { @Id long id; }".into()]).is_err());
    }
}
