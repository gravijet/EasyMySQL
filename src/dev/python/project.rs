// Neues Python-Projekt mit fertiger Datenbankverbindung: requirements.txt oder pyproject.toml, eine
// dataclass und eine Repository-Klasse je Tabelle, Tests mit unittest. Dazu Vorlagen fuer den Editor.

use super::Driver;
use crate::db::{ColumnInfo, TableInfo};
use crate::dev::names::{pascal, py_ident, quoted, snake, sql_name};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Requirements,
    Pyproject,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Requirements, Kind::Pyproject];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Requirements => "requirements.txt",
            Kind::Pyproject => "pyproject.toml",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Kind::Requirements => "requirements",
            Kind::Pyproject => "pyproject",
        }
    }

    pub fn from_id(s: &str) -> Kind {
        if s == "pyproject" { Kind::Pyproject } else { Kind::Requirements }
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub name: String,
    pub kind: Kind,
    pub database: String,
    pub tables: Vec<TableInfo>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub driver: Driver,
    pub driver_version: String,
    pub tests: bool,
}

fn en() -> bool {
    crate::i18n::is_english()
}

/// Python-Typ einer Spalte
fn py_type(c: &ColumnInfo) -> &'static str {
    let t = c.col_type.to_lowercase();
    let base = t.split(['(', ' ']).next().unwrap_or("");
    match base {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "year" | "bit" => "int",
        "decimal" | "numeric" => "Decimal",
        "float" | "double" | "real" => "float",
        "date" => "datetime.date",
        "datetime" | "timestamp" => "datetime.datetime",
        "time" => "datetime.timedelta",
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => "bytes",
        _ => "str",
    }
}

fn is_auto(c: &ColumnInfo) -> bool {
    c.extra.to_lowercase().contains("auto_increment")
}

/// db.py: Verbindung aus Umgebung (Start in EasyMySQL) bzw. db.ini
pub fn db_module(driver: Driver) -> String {
    let doc = if en() {
        "\"\"\"Connection to the database.\n\nThe connection settings are in db.ini. When the program is started from EasyMySQL,\nEasyMySQL passes them itself (server, database and password of the tab).\n\"\"\""
    } else {
        "\"\"\"Verbindung zur Datenbank.\n\nDie Verbindungsdaten stehen in db.ini. Startet man das Programm in EasyMySQL,\ngibt EasyMySQL sie selbst mit (Server, Datenbank und Passwort aus dem Reiter).\n\"\"\""
    };
    let (imports, open, cursor) = match driver {
        Driver::PyMySql => (
            "import pymysql\nimport pymysql.cursors",
            "pymysql.connect(**settings(), charset=\"utf8mb4\", cursorclass=pymysql.cursors.DictCursor, ssl={\"ca\": ca} if ca else None)",
            "con.cursor()",
        ),
        Driver::MysqlConnector => ("import mysql.connector", "mysql.connector.connect(**settings(), charset=\"utf8mb4\", **({\"ssl_ca\": ca} if ca else {}))", "con.cursor(dictionary=True)"),
    };
    let connect_doc = if en() { "New connection. Use `with connect() as con:` so it is closed afterwards." } else { "Neue Verbindung. Mit `with connect() as con:` wird sie danach geschlossen." };
    let tls_note = if en() { "Own server with its own certificate: encrypted and checked against its CA" } else { "Eigener Server mit eigenem Zertifikat: verschlüsselt und gegen dessen CA geprüft" };
    let cursor_doc = if en() { "Cursor that returns rows as dictionaries (column -> value)." } else { "Cursor, der Zeilen als Wörterbuch (Spalte -> Wert) liefert." };
    format!(
        r#"{doc}

import configparser
import os
from pathlib import Path

{imports}

_CONFIG = Path(__file__).with_name("db.ini")


def _env(name, default):
    value = os.environ.get(name)
    return value if value else default


def settings():
    cfg = configparser.ConfigParser()
    cfg.read(_CONFIG, encoding="utf-8")
    s = cfg["database"] if cfg.has_section("database") else {{}}
    return {{
        "host": _env("EASYMYSQL_DB_HOST", s.get("host", "127.0.0.1")),
        "port": int(_env("EASYMYSQL_DB_PORT", s.get("port", "3306"))),
        "user": _env("EASYMYSQL_DB_USER", s.get("user", "root")),
        "password": os.environ.get("EASYMYSQL_DB_PASSWORD", s.get("password", "")),
        "database": _env("EASYMYSQL_DB_NAME", s.get("database", "")),
    }}


def connect():
    """{connect_doc}"""
    # {tls_note}
    ca = os.environ.get("EASYMYSQL_DB_SSL_CA")
    return {open}


def cursor(con):
    """{cursor_doc}"""
    return {cursor}
"#
    )
}

fn db_ini(s: &Spec) -> String {
    let head = if en() { "; Connection to the database (for starting outside EasyMySQL)." } else { "; Verbindung zur Datenbank (für den Start außerhalb von EasyMySQL)." };
    format!("{head}\n[database]\nhost = {}\nport = {}\nuser = {}\npassword = {}\ndatabase = {}\n", s.host, s.port, s.user, s.password.as_deref().unwrap_or(""), s.database)
}

/// dataclass fuer eine Tabelle
pub fn model(t: &TableInfo) -> String {
    let class = pascal(&t.name);
    let mut fields = String::new();
    for c in &t.columns {
        fields.push_str(&format!("    {}: Optional[{}] = None\n", py_ident(&c.name), py_type(c)));
    }
    if fields.is_empty() {
        fields.push_str("    pass\n");
    }
    format!("@dataclass\nclass {class}:\n    \"\"\"{} `{}`\"\"\"\n\n{fields}", if en() { "Row of the table" } else { "Zeile der Tabelle" }, t.name)
}

fn models_module(tables: &[TableInfo]) -> String {
    let mut out = String::from("\"\"\"Tabellen als dataclasses.\"\"\"\n\nimport datetime\nfrom dataclasses import dataclass\nfrom decimal import Decimal\nfrom typing import Optional\n");
    if en() {
        out = out.replace("Tabellen als dataclasses.", "Tables as dataclasses.");
    }
    for t in tables {
        out.push_str("\n\n");
        out.push_str(&model(t));
    }
    out
}

/// Repository-Klasse: find_all, find_by_id, insert, update, delete
pub fn repo(t: &TableInfo) -> String {
    let class = pascal(&t.name);
    let table = sql_name(&t.name);
    let cols: Vec<&ColumnInfo> = t.columns.iter().collect();
    let select_cols = cols.iter().map(|c| sql_name(&c.name)).collect::<Vec<_>>().join(", ");
    let map_row = cols.iter().map(|c| format!("{}=row[{}]", py_ident(&c.name), quoted(&c.name))).collect::<Vec<_>>().join(", ");
    let pk: Vec<&ColumnInfo> = cols.iter().copied().filter(|c| c.is_pk()).collect();
    let mut out = format!(
        "class {class}Repo:\n    \"\"\"{} `{}`.\"\"\"\n\n    def __init__(self, con):\n        self.con = con\n\n    def _row(self, row):\n        return {class}({map_row})\n\n    def find_all(self):\n        with db.cursor(self.con) as cur:\n            cur.execute(\"SELECT {select_cols} FROM {table}\")\n            return [self._row(r) for r in cur.fetchall()]\n",
        if en() { "Read and change rows of" } else { "Zeilen lesen und ändern in" },
        t.name,
        select_cols = select_cols.replace('"', "\\\""),
        table = table.replace('"', "\\\"")
    );
    let insertable: Vec<&ColumnInfo> = cols.iter().copied().filter(|c| !is_auto(c) && !c.is_generated()).collect();
    if !pk.is_empty() {
        let args = pk.iter().map(|c| py_ident(&c.name)).collect::<Vec<_>>().join(", ");
        let cond = pk.iter().map(|c| format!("{} = %s", sql_name(&c.name))).collect::<Vec<_>>().join(" AND ");
        out.push_str(&format!(
            "\n    def find_by_id(self, {args}):\n        with db.cursor(self.con) as cur:\n            cur.execute(\"SELECT {select_cols} FROM {table} WHERE {cond}\", ({args},))\n            row = cur.fetchone()\n            return self._row(row) if row else None\n"
        ));
        if !insertable.is_empty() {
            let names = insertable.iter().map(|c| sql_name(&c.name)).collect::<Vec<_>>().join(", ");
            let marks = vec!["%s"; insertable.len()].join(", ");
            let values = insertable.iter().map(|c| format!("row.{}", py_ident(&c.name))).collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "\n    def insert(self, row):\n        \"\"\"{}\"\"\"\n        with db.cursor(self.con) as cur:\n            cur.execute(\"INSERT INTO {table} ({names}) VALUES ({marks})\", ({values},))\n            self.con.commit()\n            return cur.lastrowid\n",
                if en() { "Inserts the row and returns the new id (with AUTO_INCREMENT)." } else { "Fügt die Zeile ein und liefert die neue id (bei AUTO_INCREMENT)." }
            ));
        }
        let changeable: Vec<&ColumnInfo> = insertable.iter().copied().filter(|c| !c.is_pk()).collect();
        if !changeable.is_empty() {
            let set = changeable.iter().map(|c| format!("{} = %s", sql_name(&c.name))).collect::<Vec<_>>().join(", ");
            let values = changeable.iter().chain(pk.iter()).map(|c| format!("row.{}", py_ident(&c.name))).collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "\n    def update(self, row):\n        with db.cursor(self.con) as cur:\n            cur.execute(\"UPDATE {table} SET {set} WHERE {cond}\", ({values},))\n            self.con.commit()\n            return cur.rowcount\n"
            ));
        }
        out.push_str(&format!(
            "\n    def delete(self, {args}):\n        with db.cursor(self.con) as cur:\n            cur.execute(\"DELETE FROM {table} WHERE {cond}\", ({args},))\n            self.con.commit()\n            return cur.rowcount\n"
        ));
    }
    out
}

fn repos_module(tables: &[TableInfo]) -> String {
    let names = tables.iter().map(|t| pascal(&t.name)).collect::<Vec<_>>().join(", ");
    let mut out = format!(
        "\"\"\"{}\"\"\"\n\nimport db\nfrom models import {names}\n",
        if en() { "One repository class per table: find_all, find_by_id, insert, update, delete." } else { "Je Tabelle eine Repository-Klasse: find_all, find_by_id, insert, update, delete." }
    );
    for t in tables {
        out.push_str("\n\n");
        out.push_str(&repo(t));
    }
    out
}

fn main_py(s: &Spec) -> String {
    let connected = if en() { "Connected to" } else { "Verbunden mit" };
    let body = match s.tables.first() {
        Some(t) => {
            let class = pascal(&t.name);
            let comment = if en() { format!("All rows of the table {}", t.name) } else { format!("Alle Zeilen der Tabelle {}", t.name) };
            format!("        # {comment}\n        for row in {class}Repo(con).find_all():\n            print(row)\n")
        }
        None => {
            let comment = if en() { "Tables of the database" } else { "Tabellen der Datenbank" };
            format!("        # {comment}\n        with db.cursor(con) as cur:\n            cur.execute(\"SHOW TABLES\")\n            for row in cur.fetchall():\n                print(*row.values())\n")
        }
    };
    let import = match s.tables.first() {
        Some(t) => format!("from repos import {}Repo\n", pascal(&t.name)),
        None => String::new(),
    };
    format!(
        "import db\n{import}\n\ndef main():\n    with db.connect() as con:\n        print(\"{connected}\", db.settings()[\"database\"] or db.settings()[\"host\"])\n{body}\n\nif __name__ == \"__main__\":\n    main()\n"
    )
}

fn test_py(s: &Spec) -> String {
    let mut tests = String::from("    def test_connection(self):\n        with db.connect() as con:\n            with db.cursor(con) as cur:\n                cur.execute(\"SELECT 1 AS one\")\n                self.assertEqual(cur.fetchone()[\"one\"], 1)\n");
    let mut imports = String::new();
    for t in s.tables.iter().take(3) {
        let class = pascal(&t.name);
        imports.push_str(&format!("from repos import {class}Repo\n"));
        tests.push_str(&format!("\n    def test_find_all_{}(self):\n        with db.connect() as con:\n            self.assertIsInstance({class}Repo(con).find_all(), list)\n", snake(&t.name)));
    }
    format!(
        "import os\nimport sys\nimport unittest\n\nsys.path.insert(0, os.path.join(os.path.dirname(__file__), \"..\"))\n\nimport db  # noqa: E402\n{imports}\n\nclass DatabaseTest(unittest.TestCase):\n{tests}\n\nif __name__ == \"__main__\":\n    unittest.main()\n"
    )
}

fn package_name(name: &str) -> String {
    let s = snake(name).replace('_', "-");
    if s.is_empty() { "app".into() } else { s }
}

fn pyproject(s: &Spec) -> String {
    format!(
        "[project]\nname = \"{}\"\nversion = \"1.0\"\nrequires-python = \">=3.9\"\ndependencies = [\n    \"{}>={}\",\n]\n\n[build-system]\nrequires = [\"setuptools>=68\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[tool.setuptools]\npy-modules = [\"main\", \"db\", \"models\", \"repos\"]\n",
        package_name(&s.name),
        s.driver.package(),
        s.driver_version
    )
}


pub fn files(s: &Spec) -> Vec<(PathBuf, String)> {
    let mut out = vec![
        (PathBuf::from("main.py"), main_py(s)),
        (PathBuf::from("db.py"), db_module(s.driver)),
        (PathBuf::from("db.ini"), db_ini(s)),
        (PathBuf::from(".gitignore"), ".venv/\n__pycache__/\n*.pyc\n.pytest_cache/\n".into()),
    ];
    if !s.tables.is_empty() {
        out.push((PathBuf::from("models.py"), models_module(&s.tables)));
        out.push((PathBuf::from("repos.py"), repos_module(&s.tables)));
    }
    match s.kind {
        Kind::Requirements => out.push((PathBuf::from("requirements.txt"), format!("{}>={}\n", s.driver.package(), s.driver_version))),
        Kind::Pyproject => out.push((PathBuf::from("pyproject.toml"), pyproject(s))),
    }
    if s.tests {
        out.push((PathBuf::from("tests/__init__.py"), String::new()));
        out.push((PathBuf::from("tests/test_db.py"), test_py(s)));
    }
    out
}

/// Projekt in `dir` anlegen. Liefert main.py.
pub fn write(dir: &Path, s: &Spec) -> Result<PathBuf, String> {
    if dir.exists() && std::fs::read_dir(dir).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(crate::tr_format!("„{}“ gibt es schon und ist nicht leer.", "“{}” already exists and is not empty.", dir.display()));
    }
    for (rel, content) in files(s) {
        let p = dir.join(&rel);
        crate::workspace::atomic_write(&p, &content).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let main = dir.join("main.py");
    crate::workspace::set_file_db(dir, &main, &s.database);
    Ok(main)
}

/// Imports, Ausdruck zum Verbinden (eingerueckt fuer eine Ebene) und Cursor
fn connect_parts(driver: Driver, has_db_module: bool) -> (String, String, &'static str) {
    if has_db_module {
        return ("import db".into(), "db.connect()".into(), "db.cursor(con)");
    }
    let env = |k: &str, d: &str| format!("os.environ.get(\"{k}\") or \"{d}\"");
    let args = format!(
        "\n        host={},\n        port=int(os.environ.get(\"EASYMYSQL_DB_PORT\") or 3306),\n        user={},\n        password=os.environ.get(\"EASYMYSQL_DB_PASSWORD\", \"\"),\n        database={},",
        env("EASYMYSQL_DB_HOST", "127.0.0.1"),
        env("EASYMYSQL_DB_USER", "root"),
        env("EASYMYSQL_DB_NAME", "")
    );
    match driver {
        Driver::PyMySql => ("import os\n\nimport pymysql\nimport pymysql.cursors".into(), format!("pymysql.connect({args}\n        cursorclass=pymysql.cursors.DictCursor,\n    )"), "con.cursor()"),
        Driver::MysqlConnector => ("import os\n\nimport mysql.connector".into(), format!("mysql.connector.connect({args}\n    )"), "con.cursor(dictionary=True)"),
    }
}

/// Inhalt einer neuen Python-Datei und Zeile (0-basiert) fuer den Cursor
pub fn template(kind: &str, driver: Driver, has_db_module: bool) -> (String, usize) {
    match kind {
        "database" => {
            let (imports, open, cursor) = connect_parts(driver, has_db_module);
            let body = format!(
                "{imports}\n\n\ndef main():\n    with {open} as con:\n        with {cursor} as cur:\n            cur.execute(\"SELECT DATABASE()\")\n            print(cur.fetchone())\n            \n\n\nif __name__ == \"__main__\":\n    main()\n"
            );
            let line = body.lines().position(|l| l == "            ").unwrap_or(0);
            (body, line)
        }
        "main" => ("def main():\n    pass\n\n\nif __name__ == \"__main__\":\n    main()\n".into(), 1),
        "class" => ("class Name:\n    def __init__(self):\n        pass\n".into(), 2),
        "test" => ("import unittest\n\n\nclass Test(unittest.TestCase):\n    def test_something(self):\n        self.assertTrue(True)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n".into(), 5),
        _ => (String::new(), 0),
    }
}

/// Vorlage: Verbindung oeffnen (an der Cursorstelle)
pub fn snippet_connect(driver: Driver, has_db_module: bool) -> (String, Vec<String>) {
    let (imports, open, cursor) = connect_parts(driver, has_db_module);
    let open = open.replace("\n    ", "\n");
    (format!("with {open} as con:\n    with {cursor} as cur:\n        cur.execute(\"SELECT 1\")"), imports.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// Vorlage: alle Zeilen einer Tabelle lesen
pub fn snippet_select(t: &TableInfo, has_db_module: bool, driver: Driver) -> String {
    let cols = t.columns.iter().map(|c| sql_name(&c.name)).collect::<Vec<_>>().join(", ");
    let cursor = if has_db_module {
        "db.cursor(con)"
    } else {
        match driver {
            Driver::PyMySql => "con.cursor()",
            Driver::MysqlConnector => "con.cursor(dictionary=True)",
        }
    };
    let fields = t.columns.iter().take(4).map(|c| format!("row[{}]", quoted(&c.name))).collect::<Vec<_>>().join(", ");
    format!("with {cursor} as cur:\n    cur.execute(\"SELECT {cols} FROM {}\")\n    for row in cur.fetchall():\n        print({fields})", sql_name(&t.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        let col = |name: &str, ty: &str, key: &str, extra: &str| ColumnInfo { name: name.into(), col_type: ty.into(), nullable: key.is_empty(), key: key.into(), extra: extra.into(), ..Default::default() };
        Spec {
            name: "Mein Projekt".into(),
            kind: Kind::Requirements,
            database: "schule".into(),
            tables: vec![TableInfo { name: "schueler".into(), columns: vec![col("id", "int(11)", "PRI", "auto_increment"), col("Vor Name", "varchar(50)", "", ""), col("geboren", "date", "", "")], ..Default::default() }],
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: None,
            driver: Driver::PyMySql,
            driver_version: "1.1.2".into(),
            tests: true,
        }
    }

    #[test]
    fn project_files_and_generated_code() {
        let f = files(&spec());
        let get = |n: &str| f.iter().find(|(p, _)| p.to_string_lossy().replace('\\', "/") == n).map(|(_, c)| c.clone()).unwrap_or_else(|| panic!("{n} fehlt"));
        assert_eq!(get("requirements.txt"), "PyMySQL>=1.1.2\n");
        let repos = get("repos.py");
        assert!(repos.contains("class SchuelerRepo:") && repos.contains("vor_name=row[\"Vor Name\"]"), "{repos}");
        assert!(repos.contains("INSERT INTO `schueler` (`Vor Name`, `geboren`) VALUES (%s, %s)"), "{repos}");
        assert!(get("models.py").contains("geboren: Optional[datetime.date] = None"));
        assert!(get("tests/test_db.py").contains("def test_find_all_schueler"));
        assert!(get("db.ini").contains("database = schule"));
    }

    /// Erzeugter Code ist gueltiges Python (mit dem Python des Systems)
    #[test]
    fn generated_python_compiles() {
        let Some(py) = crate::dev::which("python3").or_else(|| crate::dev::which("python")) else { return };
        let dir = std::env::temp_dir().join(format!("easymysql-pyprojekt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for driver in Driver::ALL {
            let mut s = spec();
            s.driver = driver;
            let sub = dir.join(driver.id());
            write(&sub, &s).unwrap();
            for (name, _) in files(&s).iter().filter(|(p, _)| p.extension().is_some_and(|e| e == "py")) {
                let out = std::process::Command::new(&py).args(["-m", "py_compile"]).arg(sub.join(name)).output().unwrap();
                assert!(out.status.success(), "{name:?}: {}", String::from_utf8_lossy(&out.stderr));
            }
            for kind in ["database", "main", "class", "test"] {
                let (t, line) = template(kind, driver, false);
                assert!(line < t.lines().count(), "{kind}");
                let f = sub.join(format!("t_{kind}.py"));
                std::fs::write(&f, t).unwrap();
                let out = std::process::Command::new(&py).args(["-m", "py_compile"]).arg(&f).output().unwrap();
                assert!(out.status.success(), "{kind}: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
