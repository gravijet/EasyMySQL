// Spaltendefinitionen und CREATE TABLE.

use super::*;

pub const COMMON_TYPES: &[&str] = &[
    "INT",
    "BIGINT",
    "SMALLINT",
    "TINYINT",
    "DECIMAL(10,2)",
    "FLOAT",
    "DOUBLE",
    "VARCHAR(50)",
    "VARCHAR(255)",
    "CHAR(10)",
    "TEXT",
    "LONGTEXT",
    "DATE",
    "DATETIME",
    "TIMESTAMP",
    "TIME",
    "YEAR",
    "BOOLEAN",
    "BLOB",
    "ENUM('a','b')",
    "JSON",
];

/// Spaltendefinition (fuer Tabellen-Designer und ALTER TABLE).
#[derive(Clone, Debug, Default)]
pub struct ColDef {
    pub name: String,
    pub col_type: String,
    pub not_null: bool,
    pub primary: bool,
    pub auto_inc: bool,
    pub unique: bool,
    pub default: String,
    pub comment: String,
    /// Fremdschluessel-Ziel im Format "tabelle.spalte" (leer = keiner)
    pub references: String,
}

impl ColDef {
    pub fn new(name: &str, ty: &str) -> Self {
        Self {
            name: name.into(),
            col_type: ty.into(),
            ..Default::default()
        }
    }

    pub fn from_info(c: &ColumnInfo) -> Self {
        Self {
            name: c.name.clone(),
            col_type: c.col_type.clone(),
            not_null: !c.nullable,
            primary: c.is_pk(),
            auto_inc: c.extra.to_lowercase().contains("auto_increment"),
            unique: c.key == "UNI",
            default: c.default.clone().unwrap_or_default(),
            comment: c.comment.clone(),
            references: String::new(),
        }
    }

    /// Spaltendefinition ohne PRIMARY KEY/UNIQUE (die kommen separat).
    pub fn sql(&self) -> String {
        let mut s = format!("{} {}", q(&self.name), self.col_type.trim());
        if self.not_null || self.primary {
            s.push_str(" NOT NULL");
        } else {
            s.push_str(" NULL");
        }
        let d = self.default.trim();
        if !d.is_empty() && !self.auto_inc {
            let upper = d.to_uppercase();
            let is_expr = upper == "NULL"
                || upper.starts_with("CURRENT_TIMESTAMP")
                || upper == "NOW()"
                || upper == "CURDATE()"
                || (d.starts_with('\'') && d.ends_with('\'') && d.len() >= 2)
                || (d.starts_with('(') && d.ends_with(')'));
            if is_expr {
                s.push_str(&format!(" DEFAULT {d}"));
            } else {
                s.push_str(&format!(" DEFAULT {}", smart_lit(d)));
            }
        }
        if self.auto_inc {
            s.push_str(" AUTO_INCREMENT");
        }
        if !self.comment.trim().is_empty() {
            s.push_str(&format!(" COMMENT {}", lit(self.comment.trim())));
        }
        s
    }

    pub fn fk_target(&self) -> Option<(String, String)> {
        let r = self.references.trim();
        let (t, c) = r.split_once('.')?;
        if t.is_empty() || c.is_empty() {
            return None;
        }
        Some((t.to_string(), c.to_string()))
    }
}

pub fn create_table_sql(db: &str, table: &str, cols: &[ColDef], engine: &str, comment: &str) -> String {
    let mut parts: Vec<String> = cols
        .iter()
        .filter(|c| !c.name.trim().is_empty())
        .map(|c| format!("  {}", c.sql()))
        .collect();
    let pks: Vec<String> = cols
        .iter()
        .filter(|c| c.primary && !c.name.trim().is_empty())
        .map(|c| q(&c.name))
        .collect();
    if !pks.is_empty() {
        parts.push(format!("  PRIMARY KEY ({})", pks.join(", ")));
    }
    for c in cols.iter().filter(|c| c.unique && !c.primary && !c.name.trim().is_empty()) {
        parts.push(format!("  UNIQUE KEY {} ({})", q(&format!("uq_{}", c.name)), q(&c.name)));
    }
    for c in cols.iter().filter(|c| !c.name.trim().is_empty()) {
        if let Some((rt, rc)) = c.fk_target() {
            parts.push(format!(
                "  CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE RESTRICT ON UPDATE CASCADE",
                q(&format!("fk_{}_{}", table, c.name)),
                q(&c.name),
                q(&rt),
                q(&rc)
            ));
        }
    }
    let mut sql = format!(
        "CREATE TABLE {}.{} (\n{}\n) ENGINE={} DEFAULT CHARSET=utf8mb4",
        q(db),
        q(table),
        parts.join(",\n"),
        if engine.is_empty() { "InnoDB" } else { engine }
    );
    if !comment.trim().is_empty() {
        sql.push_str(&format!(" COMMENT={}", lit(comment.trim())));
    }
    sql
}
