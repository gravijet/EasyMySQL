// Java-Code (JDBC) aus Tabellen und Abfragen erzeugen: ein Record mit passenden Typen samt
// DAO-Klasse (lesen, einfuegen, aendern, loeschen) und eine fertige Klasse fuer eine einzelne Abfrage.

use crate::db::{ColumnInfo, TableInfo};

/// Wohin sich der erzeugte Code verbindet
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
}

impl Target {
    pub fn new(conn: &crate::db::ConnInfo, database: &str) -> Self {
        Self { host: conn.host.clone(), port: conn.port, user: conn.user.clone(), database: database.to_string() }
    }

    fn url(&self) -> String {
        format!("jdbc:mariadb://{}:{}/{}", self.host, self.port, self.database)
    }
}

const JAVA_KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue", "default", "do", "double", "else",
    "enum", "extends", "final", "finally", "float", "for", "goto", "if", "implements", "import", "instanceof", "int", "interface", "long",
    "native", "new", "package", "private", "protected", "public", "record", "return", "short", "static", "strictfp", "super", "switch",
    "synchronized", "this", "throw", "throws", "transient", "try", "void", "volatile", "while", "var", "yield",
];

/// Java-Typ einer Spalte: (Typ im Record, Klasse fuer `getObject`)
fn java_type(col: &ColumnInfo) -> (&'static str, &'static str) {
    let t = col.col_type.to_ascii_lowercase();
    let base = t.split(['(', ' ']).next().unwrap_or("");
    let args = t.split_once('(').and_then(|(_, r)| r.split_once(')')).map(|(a, _)| a).unwrap_or("");
    let unsigned = t.contains("unsigned");
    let (prim, boxed): (&str, &str) = match base {
        "tinyint" if args == "1" => ("boolean", "Boolean"),
        "bit" if args == "1" || args.is_empty() => ("boolean", "Boolean"),
        "tinyint" | "smallint" | "mediumint" | "year" => ("int", "Integer"),
        "int" | "integer" if unsigned => ("long", "Long"),
        "int" | "integer" => ("int", "Integer"),
        "bigint" if unsigned => return ("BigInteger", "BigInteger"),
        "bigint" | "bit" => ("long", "Long"),
        "float" => ("float", "Float"),
        "double" | "real" => ("double", "Double"),
        "decimal" | "numeric" | "dec" | "fixed" => return ("BigDecimal", "BigDecimal"),
        "date" => return ("LocalDate", "LocalDate"),
        "datetime" | "timestamp" => return ("LocalDateTime", "LocalDateTime"),
        "time" => return ("LocalTime", "LocalTime"),
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => return ("byte[]", "byte[]"),
        _ => return ("String", "String"),
    };
    if col.nullable { (boxed, boxed) } else { (prim, boxed) }
}

/// Bezeichner mit Gross-/Kleinschreibung nach Java-Art: `klasse_id` -> `klasseId`, `Schueler` -> `schueler`
fn camel(name: &str, upper_first: bool) -> String {
    let mut out = String::new();
    let mut up = upper_first;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if up {
                out.extend(c.to_uppercase());
            } else if out.is_empty() {
                out.extend(c.to_lowercase());
            } else {
                out.push(c);
            }
            up = false;
        } else if c == '_' {
            up = !out.is_empty() || upper_first;
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if JAVA_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// Name in SQL, bei Bedarf in Backticks
fn sql_name(name: &str) -> String {
    let simple = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !crate::sqledit::is_keyword(name);
    if simple { name.to_string() } else { format!("`{}`", name.replace('`', "``")) }
}

/// Text fuer ein Java-Textblock-Literal (Anfuehrungszeichen und Backslashes maskieren)
fn java_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace("\"\"\"", "\\\"\"\"")
}

fn english() -> bool {
    crate::i18n::is_english()
}

fn imports(cols: &[&ColumnInfo]) -> String {
    let mut out = String::from("import java.sql.*;\n");
    let types: Vec<&str> = cols.iter().map(|c| java_type(c).0).collect();
    if types.iter().any(|t| t.starts_with("Local")) {
        out.push_str("import java.time.*;\n");
    }
    if types.contains(&"BigDecimal") {
        out.push_str("import java.math.BigDecimal;\n");
    }
    if types.contains(&"BigInteger") {
        out.push_str("import java.math.BigInteger;\n");
    }
    out
}

/// Record, DAO-Klasse und `main` fuer eine Tabelle.
pub fn table_class(table: &TableInfo, target: &Target) -> String {
    let en = english();
    let record = camel(&table.name, true);
    let dao = format!("{record}Dao");
    let cols: Vec<&ColumnInfo> = table.columns.iter().collect();
    let fields: Vec<String> = cols.iter().map(|c| camel(&c.name, false)).collect();
    let sql_table = sql_name(&table.name);
    let all_columns = cols.iter().map(|c| sql_name(&c.name)).collect::<Vec<_>>().join(", ");
    let pk: Vec<usize> = cols.iter().enumerate().filter(|(_, c)| c.is_pk()).map(|(i, _)| i).collect();
    let auto = cols.iter().position(|c| c.extra.to_ascii_lowercase().contains("auto_increment"));

    let mut o = imports(&cols);
    o.push_str("import java.util.*;\n\n");
    o.push_str(&format!(
        "record {record}({}) {{}}\n\n",
        cols.iter().zip(&fields).map(|(c, f)| format!("{} {f}", java_type(c).0)).collect::<Vec<_>>().join(", ")
    ));
    o.push_str(&format!("public class {dao} {{\n"));
    o.push_str(&format!(
        "    // {}\n    static final String URL = \"{}\";\n\n    private final Connection con;\n\n    {dao}(Connection con) {{\n        this.con = con;\n    }}\n\n",
        if en { "Connection settings: adjust user and password" } else { "Verbindungsdaten: Benutzer und Passwort anpassen" },
        target.url()
    ));

    // Zeile -> Record
    o.push_str(&format!("    private static {record} map(ResultSet rs) throws SQLException {{\n        return new {record}(\n"));
    let reads: Vec<String> = cols
        .iter()
        .map(|c| {
            let (ty, boxed) = java_type(c);
            let name = java_text(&c.name);
            match ty {
                "String" => format!("                rs.getString(\"{name}\")"),
                "byte[]" => format!("                rs.getBytes(\"{name}\")"),
                _ => format!("                rs.getObject(\"{name}\", {boxed}.class)"),
            }
        })
        .collect();
    o.push_str(&reads.join(",\n"));
    o.push_str(");\n    }\n\n");

    // Alle lesen
    o.push_str(&format!(
        "    List<{record}> findAll() throws SQLException {{\n        String sql = \"SELECT {all_columns} FROM {sql_table}\";\n        try (PreparedStatement ps = con.prepareStatement(sql);\n             ResultSet rs = ps.executeQuery()) {{\n            List<{record}> list = new ArrayList<>();\n            while (rs.next()) {{\n                list.add(map(rs));\n            }}\n            return list;\n        }}\n    }}\n\n"
    ));

    if !pk.is_empty() && !table.is_view {
        let params = pk.iter().map(|&i| format!("{} {}", java_type(cols[i]).0, fields[i])).collect::<Vec<_>>().join(", ");
        let where_sql = pk.iter().map(|&i| format!("{} = ?", sql_name(&cols[i].name))).collect::<Vec<_>>().join(" AND ");
        let set_pk = |first: usize| -> String {
            pk.iter().enumerate().map(|(n, &i)| format!("            ps.setObject({}, {});\n", first + n, fields[i])).collect()
        };
        // Eine Zeile ueber den Primaerschluessel
        o.push_str(&format!(
            "    Optional<{record}> findById({params}) throws SQLException {{\n        String sql = \"SELECT {all_columns} FROM {sql_table} WHERE {where_sql}\";\n        try (PreparedStatement ps = con.prepareStatement(sql)) {{\n{}            try (ResultSet rs = ps.executeQuery()) {{\n                return rs.next() ? Optional.of(map(rs)) : Optional.empty();\n            }}\n        }}\n    }}\n\n",
            set_pk(1)
        ));
        // Einfuegen (ohne AUTO_INCREMENT-Spalte)
        let ins: Vec<usize> = (0..cols.len()).filter(|i| Some(*i) != auto).collect();
        let ins_cols = ins.iter().map(|&i| sql_name(&cols[i].name)).collect::<Vec<_>>().join(", ");
        let marks = vec!["?"; ins.len()].join(", ");
        let sets: String = ins.iter().enumerate().map(|(n, &i)| format!("            ps.setObject({}, row.{}());\n", n + 1, fields[i])).collect();
        if auto.is_some() {
            o.push_str(&format!(
                "    /** {} */\n    long insert({record} row) throws SQLException {{\n        String sql = \"INSERT INTO {sql_table} ({ins_cols}) VALUES ({marks})\";\n        try (PreparedStatement ps = con.prepareStatement(sql, Statement.RETURN_GENERATED_KEYS)) {{\n{sets}            ps.executeUpdate();\n            try (ResultSet keys = ps.getGeneratedKeys()) {{\n                return keys.next() ? keys.getLong(1) : -1;\n            }}\n        }}\n    }}\n\n",
                if en { "Inserts the row and returns the new AUTO_INCREMENT value." } else { "Fügt die Zeile ein und liefert den neuen AUTO_INCREMENT-Wert." }
            ));
        } else {
            o.push_str(&format!(
                "    void insert({record} row) throws SQLException {{\n        String sql = \"INSERT INTO {sql_table} ({ins_cols}) VALUES ({marks})\";\n        try (PreparedStatement ps = con.prepareStatement(sql)) {{\n{sets}            ps.executeUpdate();\n        }}\n    }}\n\n"
            ));
        }
        // Aendern (alle Nicht-Schluessel-Spalten)
        let upd: Vec<usize> = (0..cols.len()).filter(|i| !pk.contains(i)).collect();
        if !upd.is_empty() {
            let set_sql = upd.iter().map(|&i| format!("{} = ?", sql_name(&cols[i].name))).collect::<Vec<_>>().join(", ");
            let mut sets: String = upd.iter().enumerate().map(|(n, &i)| format!("            ps.setObject({}, row.{}());\n", n + 1, fields[i])).collect();
            for (n, &i) in pk.iter().enumerate() {
                sets.push_str(&format!("            ps.setObject({}, row.{}());\n", upd.len() + 1 + n, fields[i]));
            }
            o.push_str(&format!(
                "    int update({record} row) throws SQLException {{\n        String sql = \"UPDATE {sql_table} SET {set_sql} WHERE {where_sql}\";\n        try (PreparedStatement ps = con.prepareStatement(sql)) {{\n{sets}            return ps.executeUpdate();\n        }}\n    }}\n\n"
            ));
        }
        o.push_str(&format!(
            "    int delete({params}) throws SQLException {{\n        String sql = \"DELETE FROM {sql_table} WHERE {where_sql}\";\n        try (PreparedStatement ps = con.prepareStatement(sql)) {{\n{}            return ps.executeUpdate();\n        }}\n    }}\n\n",
            set_pk(1)
        ));
    }

    o.push_str(&format!(
        "    public static void main(String[] args) throws SQLException {{\n        try (Connection con = DriverManager.getConnection(URL, \"{}\", \"\")) {{\n            for ({record} row : new {dao}(con).findAll()) {{\n                System.out.println(row);\n            }}\n        }}\n    }}\n}}\n",
        java_text(&target.user)
    ));
    o
}

/// Fertige Klasse, die eine einzelne Abfrage ausfuehrt und das Ergebnis ausgibt.
pub fn query_class(sql: &str, target: &Target) -> String {
    let en = english();
    let sql = sql.trim().trim_end_matches(';').trim_end();
    let first = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    let reads = matches!(first.as_str(), "SELECT" | "WITH" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" | "TABLE" | "VALUES");
    let text: String = sql.lines().map(|l| format!("                {}", java_text(l.trim_end()))).collect::<Vec<_>>().join("\n");
    let mut o = String::from("import java.sql.*;\n\npublic class Abfrage {\n");
    o.push_str(&format!(
        "    public static void main(String[] args) throws SQLException {{\n        // {}\n        String url = \"{}\";\n        String sql = \"\"\"\n{text}\n                \"\"\";\n\n",
        if en { "User and password: adjust to your server" } else { "Benutzer und Passwort an den Server anpassen" },
        target.url()
    ));
    if reads {
        o.push_str(&format!(
            "        try (Connection con = DriverManager.getConnection(url, \"{}\", \"\");\n             PreparedStatement ps = con.prepareStatement(sql);\n             ResultSet rs = ps.executeQuery()) {{\n            ResultSetMetaData meta = rs.getMetaData();\n            while (rs.next()) {{\n                for (int i = 1; i <= meta.getColumnCount(); i++) {{\n                    System.out.print(meta.getColumnLabel(i) + \"=\" + rs.getObject(i) + \"  \");\n                }}\n                System.out.println();\n            }}\n        }}\n    }}\n}}\n",
            java_text(&target.user)
        ));
    } else {
        o.push_str(&format!(
            "        try (Connection con = DriverManager.getConnection(url, \"{}\", \"\");\n             PreparedStatement ps = con.prepareStatement(sql)) {{\n            int rows = ps.executeUpdate();\n            System.out.println(rows + \" {}\");\n        }}\n    }}\n}}\n",
            java_text(&target.user),
            if en { "rows affected" } else { "Zeilen betroffen" }
        ));
    }
    if sql.contains('?') {
        o.push_str(&format!(
            "// {}\n",
            if en { "Values for ? are set with ps.setString(1, ...), ps.setInt(2, ...) before the query runs." } else { "Werte für ? setzt man vor dem Ausführen mit ps.setString(1, ...), ps.setInt(2, ...)." }
        ));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, nullable: bool, key: &str, extra: &str) -> ColumnInfo {
        ColumnInfo { name: name.into(), col_type: ty.into(), nullable, key: key.into(), extra: extra.into(), ..Default::default() }
    }

    fn table() -> TableInfo {
        TableInfo {
            name: "schueler_klasse".into(),
            columns: vec![
                col("id", "int(11)", false, "PRI", "auto_increment"),
                col("vorname", "varchar(50)", false, "", ""),
                col("klasse_id", "int(11)", true, "MUL", ""),
                col("geboren", "date", true, "", ""),
                col("note", "decimal(4,2)", true, "", ""),
                col("aktiv", "tinyint(1)", false, "", ""),
                col("order", "bigint(20) unsigned", false, "", ""),
            ],
            ..Default::default()
        }
    }

    fn target() -> Target {
        Target { host: "localhost".into(), port: 3306, user: "root".into(), database: "schule".into() }
    }

    #[test]
    fn types_and_names() {
        assert_eq!(java_type(&col("a", "int(10) unsigned", false, "", "")), ("long", "Long"));
        assert_eq!(java_type(&col("a", "int", true, "", "")), ("Integer", "Integer"));
        assert_eq!(java_type(&col("a", "tinyint(1)", false, "", "")), ("boolean", "Boolean"));
        assert_eq!(java_type(&col("a", "text", false, "", "")), ("String", "String"));
        assert_eq!(java_type(&col("a", "datetime", false, "", "")), ("LocalDateTime", "LocalDateTime"));
        assert_eq!(camel("klasse_id", false), "klasseId");
        assert_eq!(camel("schueler_klasse", true), "SchuelerKlasse");
        assert_eq!(camel("SchiedsrichterNr", false), "schiedsrichterNr");
        assert_eq!(camel("class", false), "class_");
        assert_eq!(camel("1a", false), "_1a");
        assert_eq!(sql_name("order"), "`order`");
        assert_eq!(sql_name("vorname"), "vorname");
    }

    #[test]
    fn table_class_has_crud() {
        let j = table_class(&table(), &target());
        assert!(j.contains("record SchuelerKlasse(int id, String vorname, Integer klasseId, LocalDate geboren, BigDecimal note, boolean aktiv, BigInteger order) {}"), "{j}");
        assert!(j.contains("jdbc:mariadb://localhost:3306/schule"));
        assert!(j.contains("INSERT INTO schueler_klasse (vorname, klasse_id, geboren, note, aktiv, `order`) VALUES (?, ?, ?, ?, ?, ?)"));
        assert!(j.contains("long insert(SchuelerKlasse row)"));
        assert!(j.contains("Optional<SchuelerKlasse> findById(int id)"));
        assert!(j.contains("UPDATE schueler_klasse SET vorname = ?, klasse_id = ?, geboren = ?, note = ?, aktiv = ?, `order` = ? WHERE id = ?"));
        assert!(j.contains("rs.getObject(\"klasse_id\", Integer.class)"));
    }

    #[test]
    fn query_class_reads_or_updates() {
        let q = query_class("SELECT * FROM t WHERE a = 1;", &target());
        assert!(q.contains("executeQuery") && q.contains("SELECT * FROM t WHERE a = 1\n"), "{q}");
        let u = query_class("UPDATE t SET a = ?", &target());
        assert!(u.contains("executeUpdate") && u.contains("setString"));
    }

    /// Erzeugter Code muss mit javac uebersetzbar sein (nur wenn javac vorhanden ist).
    #[test]
    fn generated_code_compiles() {
        // Records und Textbloecke brauchen mindestens Java 17
        let Ok(version) = std::process::Command::new("javac").arg("-version").output() else { return };
        let text = format!("{}{}", String::from_utf8_lossy(&version.stdout), String::from_utf8_lossy(&version.stderr));
        let major: u32 = text.split_whitespace().nth(1).and_then(|v| v.split('.').next()).and_then(|v| v.parse().ok()).unwrap_or(0);
        if major < 17 {
            return;
        }
        let dir = std::env::temp_dir().join(format!("easymysql-java-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SchuelerKlasseDao.java"), table_class(&table(), &target())).unwrap();
        std::fs::write(dir.join("Abfrage.java"), query_class("select * from t", &target())).unwrap();
        let out = std::process::Command::new("javac").current_dir(&dir).args(["SchuelerKlasseDao.java", "Abfrage.java"]).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
