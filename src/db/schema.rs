// Metadaten (Tabellen, Spalten, Fremdschluessel) und ihre Abfrage.

use super::*;

#[derive(Clone, Debug, Default)]
pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub comment: String,
}

impl ColumnInfo {
    pub fn is_pk(&self) -> bool {
        self.key == "PRI"
    }
}

#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub name: String,
    pub is_view: bool,
    pub engine: String,
    pub rows: Option<u64>,
    pub comment: String,
    pub columns: Vec<ColumnInfo>,
}

impl TableInfo {
    pub fn pk_columns(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|c| c.is_pk())
            .map(|c| c.name.clone())
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ForeignKey {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Clone, Debug, Default)]
pub struct IndexInfo {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct Schema {
    pub name: String,
    pub tables: Vec<TableInfo>,
    pub fks: Vec<ForeignKey>,
    /// Eindeutige Indizes (inkl. Primaerschluessel): (Tabelle, Spalten)
    pub unique_keys: Vec<(String, Vec<String>)>,
    /// Suchtabellen, beim ersten Nachschlagen aufgebaut (siehe `invalidate_index`)
    pub(crate) index: std::sync::OnceLock<SchemaIndex>,
}

/// Schnelle Nachschlagetabellen: Bei tausenden Tabellen waeren lineare Suchen pro Bild zu langsam.
#[derive(Clone, Debug, Default)]
pub(crate) struct SchemaIndex {
    tables: HashMap<String, usize>,
    /// Tabelle -> Spalten, die an einem Fremdschluessel beteiligt sind
    fk_columns: HashMap<String, HashSet<String>>,
    /// Tabelle -> eindeutige Schluessel (Spalten sortiert)
    unique: HashMap<String, Vec<Vec<String>>>,
}

/// Art einer Beziehung
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RelKind {
    OneToOne,
    OneToMany,
}

impl Schema {
    fn index(&self) -> &SchemaIndex {
        self.index.get_or_init(|| {
            let mut ix = SchemaIndex::default();
            for (i, t) in self.tables.iter().enumerate() {
                ix.tables.entry(t.name.clone()).or_insert(i);
            }
            for f in &self.fks {
                ix.fk_columns.entry(f.table.clone()).or_default().extend(f.columns.iter().cloned());
            }
            for (t, k) in &self.unique_keys {
                let mut k = k.clone();
                k.sort();
                ix.unique.entry(t.clone()).or_default().push(k);
            }
            ix
        })
    }

    /// Nach Aenderungen an `tables`, `fks` oder `unique_keys` aufrufen, damit die
    /// Suchtabellen neu aufgebaut werden.
    pub fn invalidate_index(&mut self) {
        self.index = Default::default();
    }

    /// 1:1, wenn die Fremdschluessel-Spalten eindeutig sind, sonst 1:n.
    pub fn rel_kind(&self, fk: &ForeignKey) -> RelKind {
        let mut cols = fk.columns.clone();
        cols.sort();
        let unique = self.index().unique.get(&fk.table).is_some_and(|keys| keys.contains(&cols));
        if unique { RelKind::OneToOne } else { RelKind::OneToMany }
    }

    /// Darf die Beziehung leer sein (FK-Spalte erlaubt NULL)?
    pub fn rel_optional(&self, fk: &ForeignKey) -> bool {
        self.table(&fk.table)
            .map(|t| fk.columns.iter().any(|c| t.columns.iter().any(|x| &x.name == c && x.nullable)))
            .unwrap_or(false)
    }

    /// Zwischentabellen fuer n:m-Beziehungen: (Tabelle, Index FK1, Index FK2).
    /// Erkennung: genau zwei Fremdschluessel auf verschiedene Tabellen, die zusammen den
    /// Primaerschluessel (bzw. einen eindeutigen Schluessel) bilden.
    pub fn junctions(&self) -> Vec<(String, usize, usize)> {
        let mut out = Vec::new();
        let mut by_table: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut referenced: HashSet<&str> = HashSet::new();
        for (i, f) in self.fks.iter().enumerate() {
            by_table.entry(f.table.as_str()).or_default().push(i);
            referenced.insert(f.ref_table.as_str());
        }
        for t in &self.tables {
            let Some(fks) = by_table.get(t.name.as_str()).filter(|v| v.len() == 2) else { continue };
            let (a, b) = (&self.fks[fks[0]], &self.fks[fks[1]]);
            if a.ref_table == b.ref_table || a.ref_table == t.name || b.ref_table == t.name {
                continue;
            }
            // Wird die Tabelle selbst referenziert, ist sie eine eigene Entitaet.
            if referenced.contains(t.name.as_str()) {
                continue;
            }
            let mut both: Vec<String> = a.columns.iter().chain(b.columns.iter()).cloned().collect();
            both.sort();
            let pk_match = self.index().unique.get(&t.name).is_some_and(|keys| keys.contains(&both));
            if pk_match {
                out.push((t.name.clone(), fks[0], fks[1]));
            }
        }
        out
    }

    pub fn table(&self, name: &str) -> Option<&TableInfo> {
        // Veraltete Eintraege (Tabellen wurden ohne `invalidate_index` geaendert) werden erkannt
        // und per lineare Suche korrigiert.
        match self.index().tables.get(name).and_then(|&i| self.tables.get(i)) {
            Some(t) if t.name == name => Some(t),
            _ => self.tables.iter().find(|t| t.name == name),
        }
    }

    pub fn is_fk_column(&self, table: &str, column: &str) -> bool {
        self.index().fk_columns.get(table).is_some_and(|cols| cols.contains(column))
    }

    /// Tabellen in Abhaengigkeitsreihenfolge (referenzierte zuerst).
    pub fn ordered_tables(&self) -> Vec<String> {
        let mut done: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        fn visit(s: &Schema, t: &str, done: &mut Vec<String>, seen: &mut HashSet<String>) {
            if !seen.insert(t.to_string()) {
                return;
            }
            for fk in s.fks.iter().filter(|f| f.table == t) {
                if fk.ref_table != t {
                    visit(s, &fk.ref_table, done, seen);
                }
            }
            if s.table(t).is_some() {
                done.push(t.to_string());
            }
        }
        for t in self.tables.iter().filter(|t| !t.is_view) {
            visit(self, &t.name, &mut done, &mut seen);
        }
        for t in self.tables.iter().filter(|t| t.is_view) {
            done.push(t.name.clone());
        }
        done
    }
}

const SQL_TABLES: &str = "SELECT TABLE_NAME, TABLE_TYPE, IFNULL(ENGINE,''), TABLE_ROWS, IFNULL(TABLE_COMMENT,'') \
    FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";
const SQL_COLUMNS: &str = "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, \
    EXTRA, IFNULL(COLUMN_COMMENT,'') FROM information_schema.COLUMNS \
    WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION";
const SQL_FKS: &str = "SELECT k.CONSTRAINT_NAME, k.TABLE_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_NAME, \
    k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE \
    FROM information_schema.KEY_COLUMN_USAGE k \
    JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
      ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME \
     AND r.TABLE_NAME = k.TABLE_NAME \
    WHERE k.TABLE_SCHEMA = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
    ORDER BY k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION";
const SQL_UNIQUE: &str = "SELECT TABLE_NAME, INDEX_NAME, COLUMN_NAME FROM information_schema.STATISTICS \
    WHERE TABLE_SCHEMA = ? AND NON_UNIQUE = 0 ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX";

impl Db {
    pub fn schema(&self, db: &str) -> Result<Schema, String> {
        let mut schema = Schema {
            name: db.to_string(),
            ..Default::default()
        };
        // Die vier Abfragen laufen gleichzeitig (jede mit eigener Verbindung aus dem Pool).
        let (tables, cols, fks, uniq) = std::thread::scope(|sc| {
            let tables = sc.spawn(|| self.rows(SQL_TABLES, (db,)));
            let cols = sc.spawn(|| self.rows(SQL_COLUMNS, (db,)));
            let fks = sc.spawn(|| self.rows(SQL_FKS, (db,)));
            let uniq = self.rows(SQL_UNIQUE, (db,));
            let join = |h: std::thread::ScopedJoinHandle<'_, Result<Vec<Row>, String>>| h.join().unwrap_or_else(|_| Err("Abbruch".into()));
            (join(tables), join(cols), join(fks), uniq)
        });
        let (tables, cols, fks, uniq) = (tables?, cols?, fks?, uniq?);
        let mut idx: HashMap<String, usize> = HashMap::with_capacity(tables.len());
        for r in tables {
            idx.insert(s(&r[0]), schema.tables.len());
            schema.tables.push(TableInfo {
                name: s(&r[0]),
                is_view: s(&r[1]).contains("VIEW"),
                engine: s(&r[2]),
                rows: r[3].as_ref().and_then(|v| v.parse().ok()),
                comment: s(&r[4]),
                columns: Vec::new(),
            });
        }
        for r in cols {
            if let Some(&i) = r[0].as_deref().and_then(|n| idx.get(n)) {
                schema.tables[i].columns.push(ColumnInfo {
                    name: s(&r[1]),
                    col_type: s(&r[2]),
                    nullable: s(&r[3]) == "YES",
                    key: s(&r[4]),
                    // MariaDB liefert "NULL" (ohne Anfuehrungszeichen) fuer "kein Standardwert"
                    default: r[5].clone().filter(|d| d != "NULL"),
                    extra: s(&r[6]),
                    comment: s(&r[7]),
                });
            }
        }
        let mut last: Option<(String, String)> = None;
        for r in uniq {
            let key = (s(&r[0]), s(&r[1]));
            if last.as_ref() == Some(&key) {
                if let Some(u) = schema.unique_keys.last_mut() {
                    u.1.push(s(&r[2]));
                }
            } else {
                schema.unique_keys.push((key.0.clone(), vec![s(&r[2])]));
                last = Some(key);
            }
        }
        let mut fk_idx: HashMap<(String, String), usize> = HashMap::new();
        for r in fks {
            let name = s(&r[0]);
            let table = s(&r[1]);
            if let Some(&i) = fk_idx.get(&(table.clone(), name.clone())) {
                let fk = &mut schema.fks[i];
                fk.columns.push(s(&r[2]));
                fk.ref_columns.push(s(&r[4]));
            } else {
                fk_idx.insert((table.clone(), name.clone()), schema.fks.len());
                schema.fks.push(ForeignKey {
                    name,
                    table,
                    columns: vec![s(&r[2])],
                    ref_table: s(&r[3]),
                    ref_columns: vec![s(&r[4])],
                    on_update: s(&r[5]),
                    on_delete: s(&r[6]),
                });
            }
        }
        Ok(schema)
    }
}
