// Neues Node.js-Projekt mit fertiger Datenbankverbindung: package.json, Verbindungspool, eine
// Repository-Klasse je Tabelle und Tests mit dem eingebauten Testlaeufer (node --test). Dazu Vorlagen.

use super::Driver;
use crate::db::{ColumnInfo, TableInfo};
use crate::dev::names::{js_ident, pascal, quoted, sql_name};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Spec {
    pub name: String,
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

fn is_auto(c: &ColumnInfo) -> bool {
    c.extra.to_lowercase().contains("auto_increment")
}

/// Name fuer package.json: klein, ohne Leerzeichen
fn package_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let t = out.trim_end_matches('-').to_string();
    if t.is_empty() { "app".into() } else { t }
}

/// Text in einer SQL-Zeichenkette mit einfachen Anfuehrungszeichen
fn js_sql(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// db.js: Pool mit den Verbindungsdaten aus der Umgebung (Start in EasyMySQL) bzw. db.json
pub fn db_module(driver: Driver) -> String {
    let doc = if en() {
        "/**\n * Connection to the database.\n *\n * The connection settings are in db.json. When the program is started from EasyMySQL,\n * EasyMySQL passes them itself (server, database and password of the tab).\n */"
    } else {
        "/**\n * Verbindung zur Datenbank.\n *\n * Die Verbindungsdaten stehen in db.json. Startet man das Programm in EasyMySQL,\n * gibt EasyMySQL sie selbst mit (Server, Datenbank und Passwort aus dem Reiter).\n */"
    };
    let (require, create, query) = match driver {
        Driver::Mysql2 => ("const mysql = require('mysql2/promise');", "mysql.createPool({ ...settings(), connectionLimit: 5, charset: 'utf8mb4', ssl: tls() })", "const [rows] = await pool().query(sql, params);\n  return rows;"),
        Driver::Mariadb => ("const mariadb = require('mariadb');", "mariadb.createPool({ ...settings(), connectionLimit: 5, charset: 'utf8mb4', insertIdAsNumber: true, bigIntAsNumber: true, ssl: tls() })", "return pool().query(sql, params);"),
    };
    let query_doc = if en() { "Runs a statement with ? placeholders. SELECT returns the rows, INSERT/UPDATE/DELETE the result (insertId, affectedRows)." } else { "Führt eine Anweisung mit ?-Platzhaltern aus. SELECT liefert die Zeilen, INSERT/UPDATE/DELETE das Ergebnis (insertId, affectedRows)." };
    let nofile = if en() { "Without db.json the defaults apply" } else { "Ohne db.json gelten die Standardwerte" };
    let tls_note = if en() { "Own server with its own certificate: encrypted and checked against its CA" } else { "Eigener Server mit eigenem Zertifikat: verschlüsselt und gegen dessen CA geprüft" };
    let close_doc = if en() { "Closes all connections (at the end of the program)." } else { "Schließt alle Verbindungen (am Ende des Programms)." };
    format!(
        r#"{doc}

{require}
const fs = require('node:fs');
const path = require('node:path');

function env(name, fallback) {{
  const value = process.env[name];
  return value ? value : fallback;
}}

function settings() {{
  let file = {{}};
  try {{
    file = JSON.parse(fs.readFileSync(path.join(__dirname, 'db.json'), 'utf8'));
  }} catch {{
    // {nofile}
  }}
  return {{
    host: env('EASYMYSQL_DB_HOST', file.host ?? '127.0.0.1'),
    port: Number(env('EASYMYSQL_DB_PORT', file.port ?? 3306)),
    user: env('EASYMYSQL_DB_USER', file.user ?? 'root'),
    password: process.env.EASYMYSQL_DB_PASSWORD ?? file.password ?? '',
    database: env('EASYMYSQL_DB_NAME', file.database ?? ''),
  }};
}}

// {tls_note}
function tls() {{
  const ca = process.env.EASYMYSQL_DB_SSL_CA;
  return ca ? {{ ca: fs.readFileSync(ca) }} : undefined;
}}

let shared = null;

function pool() {{
  if (!shared) {{
    shared = {create};
  }}
  return shared;
}}

/** {query_doc} */
async function query(sql, params = []) {{
  {query}
}}

/** {close_doc} */
async function close() {{
  if (shared) {{
    await shared.end();
    shared = null;
  }}
}}

module.exports = {{ query, pool, close, settings }};
"#
    )
}

fn db_json(s: &Spec) -> String {
    let v = serde_json::json!({
        "host": s.host,
        "port": s.port,
        "user": s.user,
        "password": s.password.clone().unwrap_or_default(),
        "database": s.database,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// Repository-Klasse: findAll, findById, insert, update, remove
pub fn repo(t: &TableInfo) -> String {
    let class = pascal(&t.name);
    let table = js_sql(&sql_name(&t.name));
    let cols: Vec<&ColumnInfo> = t.columns.iter().collect();
    let select = cols.iter().map(|c| js_sql(&sql_name(&c.name))).collect::<Vec<_>>().join(", ");
    let map = cols.iter().map(|c| format!("{}: row[{}]", js_ident(&c.name), quoted(&c.name))).collect::<Vec<_>>().join(", ");
    let pk: Vec<&ColumnInfo> = cols.iter().copied().filter(|c| c.is_pk()).collect();
    let mut out = format!(
        "/** {} `{}` */\nclass {class}Repo {{\n  static fromRow(row) {{\n    return {{ {map} }};\n  }}\n\n  async findAll() {{\n    const rows = await db.query('SELECT {select} FROM {table}');\n    return rows.map({class}Repo.fromRow);\n  }}\n",
        if en() { "Read and change rows of" } else { "Zeilen lesen und ändern in" },
        t.name
    );
    if !pk.is_empty() {
        let args = pk.iter().map(|c| js_ident(&c.name)).collect::<Vec<_>>().join(", ");
        let cond = pk.iter().map(|c| format!("{} = ?", js_sql(&sql_name(&c.name)))).collect::<Vec<_>>().join(" AND ");
        out.push_str(&format!(
            "\n  async findById({args}) {{\n    const rows = await db.query('SELECT {select} FROM {table} WHERE {cond}', [{args}]);\n    return rows.length ? {class}Repo.fromRow(rows[0]) : null;\n  }}\n"
        ));
        let insertable: Vec<&ColumnInfo> = cols.iter().copied().filter(|c| !is_auto(c) && !c.is_generated()).collect();
        if !insertable.is_empty() {
            let names = insertable.iter().map(|c| js_sql(&sql_name(&c.name))).collect::<Vec<_>>().join(", ");
            let marks = vec!["?"; insertable.len()].join(", ");
            let values = insertable.iter().map(|c| format!("row.{}", js_ident(&c.name))).collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "\n  /** {} */\n  async insert(row) {{\n    const result = await db.query('INSERT INTO {table} ({names}) VALUES ({marks})', [{values}]);\n    return result.insertId;\n  }}\n",
                if en() { "Inserts the row and returns the new id (with AUTO_INCREMENT)." } else { "Fügt die Zeile ein und liefert die neue id (bei AUTO_INCREMENT)." }
            ));
        }
        let changeable: Vec<&ColumnInfo> = insertable.iter().copied().filter(|c| !c.is_pk()).collect();
        if !changeable.is_empty() {
            let set = changeable.iter().map(|c| format!("{} = ?", js_sql(&sql_name(&c.name)))).collect::<Vec<_>>().join(", ");
            let values = changeable.iter().chain(pk.iter()).map(|c| format!("row.{}", js_ident(&c.name))).collect::<Vec<_>>().join(", ");
            out.push_str(&format!("\n  async update(row) {{\n    const result = await db.query('UPDATE {table} SET {set} WHERE {cond}', [{values}]);\n    return result.affectedRows;\n  }}\n"));
        }
        out.push_str(&format!("\n  async remove({args}) {{\n    const result = await db.query('DELETE FROM {table} WHERE {cond}', [{args}]);\n    return result.affectedRows;\n  }}\n"));
    }
    out.push_str("}\n");
    out
}

fn repos_module(tables: &[TableInfo]) -> String {
    let mut out = format!(
        "// {}\n\nconst db = require('./db');\n",
        if en() { "One repository class per table: findAll, findById, insert, update, remove." } else { "Je Tabelle eine Repository-Klasse: findAll, findById, insert, update, remove." }
    );
    for t in tables {
        out.push('\n');
        out.push_str(&repo(t));
    }
    let names = tables.iter().map(|t| format!("{}Repo", pascal(&t.name))).collect::<Vec<_>>().join(", ");
    out.push_str(&format!("\nmodule.exports = {{ {names} }};\n"));
    out
}

fn index_js(s: &Spec) -> String {
    let connected = if en() { "Connected to" } else { "Verbunden mit" };
    let (import, body) = match s.tables.first() {
        Some(t) => {
            let class = pascal(&t.name);
            let comment = if en() { format!("All rows of the table {}", t.name) } else { format!("Alle Zeilen der Tabelle {}", t.name) };
            (format!("const {{ {class}Repo }} = require('./repos');\n"), format!("  // {comment}\n  for (const row of await new {class}Repo().findAll()) {{\n    console.log(row);\n  }}\n"))
        }
        None => {
            let comment = if en() { "Tables of the database" } else { "Tabellen der Datenbank" };
            (String::new(), format!("  // {comment}\n  for (const row of await db.query('SHOW TABLES')) {{\n    console.log(Object.values(row)[0]);\n  }}\n"))
        }
    };
    format!(
        "const db = require('./db');\n{import}\nasync function main() {{\n  const s = db.settings();\n  console.log('{connected}', s.database || s.host);\n{body}}}\n\nmain()\n  .catch((err) => {{\n    console.error(err);\n    process.exitCode = 1;\n  }})\n  .finally(() => db.close());\n"
    )
}

fn test_js(s: &Spec) -> String {
    let mut tests = String::from("test('connection works', async () => {\n  const rows = await db.query('SELECT 1 AS one');\n  assert.equal(Number(rows[0].one), 1);\n});\n");
    let mut imports = String::new();
    for t in s.tables.iter().take(3) {
        let class = pascal(&t.name);
        imports.push_str(&format!("const {{ {class}Repo }} = require('../repos');\n"));
        tests.push_str(&format!("\ntest('findAll {}', async () => {{\n  assert.ok(Array.isArray(await new {class}Repo().findAll()));\n}});\n", t.name.replace('\'', "")));
    }
    format!("const test = require('node:test');\nconst assert = require('node:assert/strict');\nconst db = require('../db');\n{imports}\ntest.after(() => db.close());\n\n{tests}")
}

fn package_json(s: &Spec) -> String {
    let mut deps = serde_json::Map::new();
    deps.insert(s.driver.package().into(), serde_json::Value::String(format!("^{}", s.driver_version)));
    let v = serde_json::json!({
        "name": package_name(&s.name),
        "version": "1.0.0",
        "private": true,
        "type": "commonjs",
        "main": "index.js",
        "scripts": { "start": "node index.js", "test": "node --test" },
        "dependencies": deps,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}


pub fn files(s: &Spec) -> Vec<(PathBuf, String)> {
    let mut out = vec![
        (PathBuf::from("package.json"), package_json(s)),
        (PathBuf::from("index.js"), index_js(s)),
        (PathBuf::from("db.js"), db_module(s.driver)),
        (PathBuf::from("db.json"), db_json(s)),
        (PathBuf::from(".gitignore"), "node_modules/\ndist/\n.env\n".into()),
    ];
    if !s.tables.is_empty() {
        out.push((PathBuf::from("repos.js"), repos_module(&s.tables)));
    }
    if s.tests {
        out.push((PathBuf::from("test/db.test.js"), test_js(s)));
    }
    out
}

pub fn write(dir: &Path, s: &Spec) -> Result<PathBuf, String> {
    if dir.exists() && std::fs::read_dir(dir).map(|mut r| r.next().is_some()).unwrap_or(true) {
        return Err(crate::tr_format!("„{}“ gibt es schon und ist nicht leer.", "“{}” already exists and is not empty.", dir.display()));
    }
    for (rel, content) in files(s) {
        let p = dir.join(&rel);
        crate::workspace::atomic_write(&p, &content).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let main = dir.join("index.js");
    crate::workspace::set_file_db(dir, &main, &s.database);
    Ok(main)
}

/// Ausdruck fuer einen Pool ohne db.js (Werte aus der Umgebung, sonst die eingetragenen)
fn inline_pool(driver: Driver) -> (String, String) {
    let opts = "{\n  host: process.env.EASYMYSQL_DB_HOST || '127.0.0.1',\n  port: Number(process.env.EASYMYSQL_DB_PORT || 3306),\n  user: process.env.EASYMYSQL_DB_USER || 'root',\n  password: process.env.EASYMYSQL_DB_PASSWORD || '',\n  database: process.env.EASYMYSQL_DB_NAME || '',\n}";
    match driver {
        Driver::Mysql2 => ("const mysql = require('mysql2/promise');".into(), format!("const pool = mysql.createPool({opts});")),
        Driver::Mariadb => ("const mariadb = require('mariadb');".into(), format!("const pool = mariadb.createPool({opts});")),
    }
}

/// Inhalt einer neuen JavaScript-Datei und Zeile fuer den Cursor
pub fn template(kind: &str, driver: Driver, has_db_module: bool) -> (String, usize) {
    match kind {
        "database" => {
            let body = if has_db_module {
                "const db = require('./db');\n\nasync function main() {\n  const rows = await db.query('SELECT DATABASE() AS db');\n  console.log(rows[0]);\n  \n}\n\nmain()\n  .catch((err) => {\n    console.error(err);\n    process.exitCode = 1;\n  })\n  .finally(() => db.close());\n".to_string()
            } else {
                let (require, pool) = inline_pool(driver);
                let select = match driver {
                    Driver::Mysql2 => "const [rows] = await pool.query('SELECT DATABASE() AS db');",
                    Driver::Mariadb => "const rows = await pool.query('SELECT DATABASE() AS db');",
                };
                format!("{require}\n\n{pool}\n\nasync function main() {{\n  {select}\n  console.log(rows[0]);\n  \n}}\n\nmain()\n  .catch((err) => {{\n    console.error(err);\n    process.exitCode = 1;\n  }})\n  .finally(() => pool.end());\n")
            };
            let line = body.lines().position(|l| l == "  ").unwrap_or(0);
            (body, line)
        }
        "main" => ("async function main() {\n  \n}\n\nmain().catch((err) => {\n  console.error(err);\n  process.exitCode = 1;\n});\n".into(), 1),
        "module" => ("function hello(name) {\n  return `Hallo ${name}`;\n}\n\nmodule.exports = { hello };\n".into(), 1),
        "class" => ("class Name {\n  constructor() {\n    \n  }\n}\n\nmodule.exports = { Name };\n".into(), 2),
        "test" => ("const test = require('node:test');\nconst assert = require('node:assert/strict');\n\ntest('works', () => {\n  assert.equal(1 + 1, 2);\n});\n".into(), 4),
        _ => (String::new(), 0),
    }
}

/// Vorlage: alle Zeilen einer Tabelle lesen
pub fn snippet_select(t: &TableInfo, has_db_module: bool, driver: Driver) -> String {
    let cols = t.columns.iter().map(|c| js_sql(&sql_name(&c.name))).collect::<Vec<_>>().join(", ");
    let sql = format!("'SELECT {cols} FROM {}'", js_sql(&sql_name(&t.name)));
    let fetch = if has_db_module {
        format!("const rows = await db.query({sql});")
    } else {
        match driver {
            Driver::Mysql2 => format!("const [rows] = await pool.query({sql});"),
            Driver::Mariadb => format!("const rows = await pool.query({sql});"),
        }
    };
    format!("{fetch}\nfor (const row of rows) {{\n  console.log(row);\n}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(driver: Driver) -> Spec {
        let col = |name: &str, ty: &str, key: &str, extra: &str| ColumnInfo { name: name.into(), col_type: ty.into(), nullable: key.is_empty(), key: key.into(), extra: extra.into(), ..Default::default() };
        Spec {
            name: "Mein Shop".into(),
            database: "shop".into(),
            tables: vec![TableInfo { name: "kunde".into(), columns: vec![col("id", "int", "PRI", "auto_increment"), col("Voller Name", "varchar(50)", "", "")], ..Default::default() }],
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: None,
            driver,
            driver_version: "3.15.3".into(),
            tests: true,
        }
    }

    #[test]
    fn package_and_generated_code() {
        let f = files(&spec(Driver::Mysql2));
        let get = |n: &str| f.iter().find(|(p, _)| p.to_string_lossy().replace('\\', "/") == n).map(|(_, c)| c.clone()).unwrap();
        let pj: serde_json::Value = serde_json::from_str(&get("package.json")).unwrap();
        assert_eq!(pj["name"], "mein-shop");
        assert_eq!(pj["dependencies"]["mysql2"], "^3.15.3");
        let repos = get("repos.js");
        assert!(repos.contains("vollerName: row[\"Voller Name\"]") && repos.contains("INSERT INTO `kunde` (`Voller Name`) VALUES (?)"), "{repos}");
        assert!(get("test/db.test.js").contains("findAll kunde"));
    }

    /// Erzeugter Code ist gueltiges JavaScript (mit dem Node des Systems)
    #[test]
    fn generated_javascript_parses() {
        let Some(node) = crate::dev::which("node") else { return };
        let dir = std::env::temp_dir().join(format!("easymysql-jsprojekt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for driver in Driver::ALL {
            let s = spec(driver);
            let sub = dir.join(driver.id());
            write(&sub, &s).unwrap();
            let mut all: Vec<PathBuf> = files(&s).into_iter().map(|(p, _)| p).filter(|p| p.extension().is_some_and(|e| e == "js")).map(|p| sub.join(p)).collect();
            for kind in ["database", "main", "module", "class", "test"] {
                for with_db in [false, true] {
                    let (t, line) = template(kind, driver, with_db);
                    assert!(line < t.lines().count());
                    let f = sub.join(format!("t_{kind}_{with_db}.js"));
                    std::fs::write(&f, t).unwrap();
                    all.push(f);
                }
            }
            for f in all {
                let out = std::process::Command::new(&node).arg("--check").arg(&f).output().unwrap();
                assert!(out.status.success(), "{f:?}: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
