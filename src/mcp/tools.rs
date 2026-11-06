// Werkzeuge des MCP-Servers: Datenbanken, Tabellen und Schema lesen, Leseabfragen, Aenderungen
// (nur wenn erlaubt, vorher wird gesichert) und den eingebauten Server starten.

use super::guard;
use super::protocol::{Handler, ToolResult};
use crate::db::{ConnInfo, Db, SYSTEM_DATABASES, q};
use mysql::prelude::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// Einstellungen einer MCP-Sitzung (aus den EasyMySQL-Einstellungen und der Kommandozeile)
#[derive(Clone, Debug)]
pub struct Options {
    pub conn: ConnInfo,
    pub ssh: crate::ssh::SshConfig,
    /// Datenbank, wenn ein Aufruf keine nennt
    pub database: Option<String>,
    pub allow_write: bool,
    /// Vor DROP/TRUNCATE/DELETE ohne WHERE sichern
    pub backup: bool,
    /// Hoechstzahl Zeilen je Abfrage (Standard; ein Aufruf darf weniger verlangen)
    pub max_rows: usize,
    /// Eigener Server (Port, Datenordner) fuer Sicherungen und `start_server`
    pub server: crate::server::Config,
}

/// Laengere Zellen werden gekuerzt, damit ein Ergebnis den Kontext der KI nicht sprengt
const MAX_CELL: usize = 2000;
/// Obergrenze fuer `max_rows`
const HARD_MAX_ROWS: usize = 5000;

pub struct Session {
    opts: Options,
    db: Option<Db>,
    tunnel: Option<crate::ssh::Tunnel>,
}

fn s(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

fn cell(v: Option<String>) -> Value {
    match v {
        None => Value::Null,
        Some(t) if t.chars().count() > MAX_CELL => Value::String(format!("{}… [{} chars]", t.chars().take(MAX_CELL).collect::<String>(), t.chars().count())),
        Some(t) => Value::String(t),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

fn schema_obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

impl Session {
    pub fn new(opts: Options) -> Self {
        crate::db::MAX_ROWS.store(opts.max_rows.clamp(1, HARD_MAX_ROWS), std::sync::atomic::Ordering::Relaxed);
        Self { opts, db: None, tunnel: None }
    }

    /// Verbindung herstellen bzw. die bestehende pruefen
    fn db(&mut self) -> Result<Db, String> {
        if let Some(db) = &self.db
            && db.pool.get_conn().map(|mut c| c.query_drop("DO 1").is_ok()).unwrap_or(false)
        {
            return Ok(db.clone());
        }
        self.db = None;
        let info = if self.opts.ssh.enabled {
            let t = crate::ssh::open(&self.opts.ssh, &self.opts.conn.host, self.opts.conn.port)?;
            let via = ConnInfo { host: "127.0.0.1".into(), port: t.local_port, ..self.opts.conn.clone() };
            self.tunnel = Some(t);
            via
        } else {
            self.opts.conn.clone()
        };
        let db = Db::connect(&info).map_err(|e| {
            let hint = if self.is_builtin() && !crate::server::port_open(self.opts.conn.port) {
                " The EasyMySQL server is not running: call the `start_server` tool (or open EasyMySQL) and try again."
            } else {
                ""
            };
            format!("Could not connect to {}: {e}.{hint}", self.opts.conn.label())
        })?;
        self.db = Some(db.clone());
        Ok(db)
    }

    /// Verbindung zum eingebauten Server dieses PCs?
    fn is_builtin(&self) -> bool {
        !self.opts.ssh.enabled && crate::dev::is_local(&self.opts.conn.host) && self.opts.conn.port == self.opts.server.port
    }

    fn database<'a>(&'a self, args: &'a Value) -> Result<&'a str, String> {
        str_arg(args, "database")
            .or(self.opts.database.as_deref())
            .ok_or_else(|| "Missing `database`. Call `list_databases` to see the available ones.".into())
    }

    fn server_info(&mut self) -> Result<Value, String> {
        let db = self.db()?;
        let rows = db.rows("SELECT CURRENT_USER(), @@version_comment, @@character_set_server, @@collation_server, @@sql_mode", ())?;
        let r = rows.first().cloned().unwrap_or_default();
        Ok(json!({
            "server": db.version,
            "connection": self.opts.conn.label(),
            "via_ssh": self.opts.ssh.enabled,
            "current_user": s(&r.first().cloned().flatten()),
            "distribution": s(&r.get(1).cloned().flatten()),
            "charset": s(&r.get(2).cloned().flatten()),
            "collation": s(&r.get(3).cloned().flatten()),
            "sql_mode": s(&r.get(4).cloned().flatten()),
            "default_database": self.opts.database,
            "write_allowed": self.opts.allow_write,
            "backup_before_destructive_changes": self.opts.backup,
            "max_rows": crate::db::max_rows(),
            "easymysql": env!("CARGO_PKG_VERSION"),
        }))
    }

    fn list_databases(&mut self, args: &Value) -> Result<Value, String> {
        let system = args.get("include_system").and_then(Value::as_bool).unwrap_or(false);
        let db = self.db()?;
        let rows = db.rows(
            "SELECT s.SCHEMA_NAME, s.DEFAULT_CHARACTER_SET_NAME, COUNT(t.TABLE_NAME) FROM information_schema.SCHEMATA s \
             LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME GROUP BY s.SCHEMA_NAME, s.DEFAULT_CHARACTER_SET_NAME ORDER BY s.SCHEMA_NAME",
            (),
        )?;
        let list: Vec<Value> = rows
            .into_iter()
            .filter(|r| system || !SYSTEM_DATABASES.contains(&s(&r[0]).to_lowercase().as_str()))
            .map(|r| json!({ "name": s(&r[0]), "charset": s(&r[1]), "tables": s(&r[2]).parse::<u64>().unwrap_or(0) }))
            .collect();
        Ok(json!({ "databases": list }))
    }

    fn list_tables(&mut self, args: &Value) -> Result<Value, String> {
        let name = self.database(args)?.to_string();
        let db = self.db()?;
        let rows = db.rows(
            "SELECT TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_ROWS, TABLE_COMMENT FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (name.as_str(),),
        )?;
        if rows.is_empty() && !db.databases()?.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
            return Err(format!("Database `{name}` does not exist."));
        }
        let list: Vec<Value> = rows
            .into_iter()
            .map(|r| {
                let mut t = json!({ "name": s(&r[0]), "type": if s(&r[1]).contains("VIEW") { "view" } else { "table" } });
                if let Some(e) = r[2].clone().filter(|e| !e.is_empty()) {
                    t["engine"] = json!(e);
                }
                if let Some(n) = r[3].as_deref().and_then(|n| n.parse::<u64>().ok()) {
                    t["rows_estimate"] = json!(n);
                }
                if let Some(c) = r[4].clone().filter(|c| !c.is_empty()) {
                    t["comment"] = json!(c);
                }
                t
            })
            .collect();
        Ok(json!({ "database": name, "tables": list }))
    }

    fn describe_table(&mut self, args: &Value) -> Result<Value, String> {
        let name = self.database(args)?.to_string();
        let table = str_arg(args, "table").ok_or("Missing `table`.")?.to_string();
        let db = self.db()?;
        let cols = db.rows(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (name.as_str(), table.as_str()),
        )?;
        if cols.is_empty() {
            return Err(format!("Table `{name}`.`{table}` does not exist. Call `list_tables` to see the tables."));
        }
        let columns: Vec<Value> = cols
            .into_iter()
            .map(|r| {
                json!({
                    "name": s(&r[0]), "type": s(&r[1]), "nullable": s(&r[2]) == "YES", "key": s(&r[3]),
                    "default": r[4], "extra": s(&r[5]), "comment": s(&r[6]),
                })
            })
            .collect();
        let indexes: Vec<Value> = db.indexes(&name, &table)?.into_iter().map(|i| json!({ "name": i.name, "unique": i.unique, "columns": i.columns })).collect();
        let fks = db.rows(
            "SELECT k.CONSTRAINT_NAME, GROUP_CONCAT(k.COLUMN_NAME ORDER BY k.ORDINAL_POSITION), k.REFERENCED_TABLE_NAME, \
             GROUP_CONCAT(k.REFERENCED_COLUMN_NAME ORDER BY k.ORDINAL_POSITION) FROM information_schema.KEY_COLUMN_USAGE k \
             WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL GROUP BY k.CONSTRAINT_NAME, k.REFERENCED_TABLE_NAME",
            (name.as_str(), table.as_str()),
        )?;
        let fks: Vec<Value> = fks.into_iter().map(|r| json!({ "name": s(&r[0]), "columns": s(&r[1]), "references": format!("{}({})", s(&r[2]), s(&r[3])) })).collect();
        let create = db.create_statement(&name, &table).unwrap_or_default();
        Ok(json!({ "database": name, "table": table, "columns": columns, "indexes": indexes, "foreign_keys": fks, "create_statement": create }))
    }

    /// Kompakte Textform des ganzen Schemas: wenig Zeichen, alles Noetige fuer SQL
    fn get_schema(&mut self, args: &Value) -> Result<String, String> {
        let name = self.database(args)?.to_string();
        let only: Option<Vec<String>> = args.get("tables").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect());
        let db = self.db()?;
        let schema = db.schema(&name)?;
        if schema.tables.is_empty() && !db.databases()?.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
            return Err(format!("Database `{name}` does not exist."));
        }
        let wanted = |t: &str| only.as_ref().is_none_or(|o| o.contains(&t.to_lowercase()));
        let mut out = format!("-- Database `{name}` on {} ({} tables)\n", db.version, schema.tables.len());
        for t in schema.tables.iter().filter(|t| wanted(&t.name)) {
            out.push('\n');
            let kind = if t.is_view { "VIEW" } else { "TABLE" };
            let rows = t.rows.map(|r| format!(", ~{r} rows")).unwrap_or_default();
            let engine = t.engine.clone();
            out.push_str(&format!("{kind} {} ({engine}{rows})", q(&t.name)));
            if !t.comment.is_empty() {
                out.push_str(&format!(" -- {}", t.comment));
            }
            out.push('\n');
            for c in &t.columns {
                let mut line = format!("  {} {}", q(&c.name), c.col_type);
                if c.is_pk() {
                    line.push_str(" PRIMARY KEY");
                } else if c.key == "UNI" {
                    line.push_str(" UNIQUE");
                }
                if !c.nullable {
                    line.push_str(" NOT NULL");
                }
                if let Some(d) = &c.default {
                    line.push_str(&format!(" DEFAULT {d}"));
                }
                if !c.extra.is_empty() {
                    line.push_str(&format!(" {}", c.extra));
                }
                if !c.comment.is_empty() {
                    line.push_str(&format!(" -- {}", c.comment));
                }
                out.push_str(&line);
                out.push('\n');
            }
            for fk in schema.fks.iter().filter(|f| f.table == t.name) {
                out.push_str(&format!("  FOREIGN KEY ({}) REFERENCES {}({})", fk.columns.join(", "), q(&fk.ref_table), fk.ref_columns.join(", ")));
                if !fk.on_delete.is_empty() && fk.on_delete != "RESTRICT" {
                    out.push_str(&format!(" ON DELETE {}", fk.on_delete));
                }
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// Ergebnismengen als JSON (Spalten, Zeilen, gekuerzt?)
    fn sets_json(sets: Vec<crate::db::ResultSet>) -> Vec<Value> {
        sets.into_iter()
            .filter(|s| s.has_table())
            .map(|s| {
                let n = s.rows.len();
                let rows: Vec<Value> = s.rows.into_iter().map(|r| Value::Array(r.into_iter().map(cell).collect())).collect();
                json!({ "columns": s.columns, "rows": rows, "row_count": n, "truncated": s.truncated })
            })
            .collect()
    }

    fn query(&mut self, args: &Value) -> Result<Value, String> {
        let sql = str_arg(args, "sql").ok_or("Missing `sql`.")?.to_string();
        let stmts = crate::db::split_statements(&sql);
        let [stmt] = stmts.as_slice() else { return Err("Send exactly one statement per `query` call.".into()) };
        guard::check_read_only(&stmt.sql)?;
        let limit = args.get("max_rows").and_then(Value::as_u64).map(|n| n as usize).unwrap_or(self.opts.max_rows).clamp(1, HARD_MAX_ROWS);
        let database = str_arg(args, "database").or(self.opts.database.as_deref()).map(str::to_string);
        let db = self.db()?;
        let mut c = db.new_conn()?;
        if let Some(d) = &database {
            c.query_drop(format!("USE {}", q(d))).map_err(crate::db::err_text)?;
        }
        // Die Transaktion verhindert jede Aenderung, auch durch Funktionen oder WITH ... UPDATE
        c.query_drop("START TRANSACTION READ ONLY").map_err(crate::db::err_text)?;
        crate::db::MAX_ROWS.store(limit, std::sync::atomic::Ordering::Relaxed);
        let (info, id) = (db.info.clone(), c.connection_id());
        let kill = move || crate::db::kill_query(&info, id);
        let start = Instant::now();
        let mut out = crate::db::run_script_with(&mut c, &stmt.sql, Some(&kill));
        let _ = c.query_drop("ROLLBACK");
        if let Some(e) = out.error.take() {
            return Err(e);
        }
        let elapsed = start.elapsed().as_millis() as u64;
        let mut sets = Self::sets_json(std::mem::take(&mut out.sets));
        let mut r = if sets.len() == 1 { sets.remove(0) } else { json!({ "results": sets }) };
        r["elapsed_ms"] = json!(elapsed);
        if r["truncated"] == json!(true) {
            r["note"] = json!(format!("Only the first {limit} rows are shown. Add WHERE/LIMIT or aggregate to see the rest."));
        }
        Ok(r)
    }

    fn execute(&mut self, args: &Value) -> Result<Value, String> {
        if !self.opts.allow_write {
            return Err("Changing data is switched off for AI assistants. The user can allow it in EasyMySQL under Settings → AI assistants \
                        (or start the MCP server with `--allow-write`). Until then only read with `query`."
                .into());
        }
        let sql = str_arg(args, "sql").ok_or("Missing `sql`.")?.to_string();
        let stmts = crate::db::split_statements(&sql);
        if stmts.is_empty() {
            return Err("No statement found.".into());
        }
        let database = str_arg(args, "database").or(self.opts.database.as_deref()).map(str::to_string);
        let db = self.db()?;
        let mut backup = Value::Null;
        if self.opts.backup {
            let targets = crate::backup::destructive_targets(&sql, database.as_deref());
            if !targets.is_empty() {
                let server = crate::server::Server::new(self.opts.server.clone());
                let paths = server.paths().ok_or("A backup is required before this change, but the MariaDB tools (mariadb-dump) were not found. Nothing was changed. The user can switch off \"Back up the affected tables\" in EasyMySQL (Settings → Queries and data).")?;
                let env = crate::backup::Env { paths, conn: db.info.clone(), dir: crate::backup::Config::load().dir(), force: false };
                let dir = crate::backup::create(&env, Some(targets.clone()), "vor-ki", &|_| {}).map_err(|e| format!("Backup before the change failed, nothing was changed: {e}"))?;
                backup = json!({ "databases": targets, "folder": dir.display().to_string() });
            }
        }
        let mut c = db.new_conn()?;
        if let Some(d) = &database {
            c.query_drop(format!("USE {}", q(d))).map_err(crate::db::err_text)?;
        }
        let results = crate::db::run_statements(&mut c, &stmts, None);
        let failed = results.iter().any(|r| r.error.is_some());
        let list: Vec<Value> = results
            .into_iter()
            .map(|r| {
                let mut v = json!({ "statement": guard::preview(&r.preview, 90), "affected_rows": r.affected, "elapsed_ms": r.elapsed.as_millis() as u64 });
                let sets = Self::sets_json(r.sets);
                if !sets.is_empty() {
                    v["results"] = json!(sets);
                }
                if let Some(e) = r.error {
                    v["error"] = json!(e);
                }
                v
            })
            .collect();
        let mut out = json!({ "statements": list, "completed": !failed });
        if failed {
            out["note"] = json!("Execution stopped at the first error; earlier statements stay applied (autocommit).");
        }
        if !backup.is_null() {
            out["backup"] = backup;
        }
        Ok(out)
    }

    fn start_server(&mut self) -> Result<Value, String> {
        let port = self.opts.conn.port;
        if crate::server::port_open(port) {
            return Ok(json!({ "running": true, "message": format!("A database server is already running on port {port}.") }));
        }
        if !self.is_builtin() {
            return Err(format!("{} is not the EasyMySQL server of this computer; it has to be started where it runs.", self.opts.conn.label()));
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(exe);
        cmd.env(super::START_SERVER_VAR, "1").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        cmd.spawn().map_err(|e| format!("EasyMySQL could not be started: {e}"))?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(120) {
            if crate::server::port_open(port) {
                self.db = None;
                return Ok(json!({ "running": true, "message": format!("EasyMySQL started the server on port {port}. It keeps running while the EasyMySQL window is open.") }));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err("EasyMySQL was opened, but the server did not start within 2 minutes. If EasyMySQL was already open, start the server there (Server → Start).".into())
    }
}

impl Handler for Session {
    fn tools(&self) -> Vec<Value> {
        let db_prop = json!({ "type": "string", "description": "Database (schema) name. Optional when a default database is configured." });
        let read = json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
        let write_state = if self.opts.allow_write { "Writing is ENABLED." } else { "Writing is currently DISABLED by the user; calls fail until they allow it in EasyMySQL." };
        vec![
            json!({ "name": "server_info", "title": "Server info",
                "description": "Version, user, character set, sql_mode and permissions of the connected MariaDB/MySQL server. Call once at the start.",
                "inputSchema": schema_obj(json!({}), &[]), "annotations": read }),
            json!({ "name": "list_databases", "title": "List databases",
                "description": "All databases with their character set and number of tables (system databases hidden unless include_system).",
                "inputSchema": schema_obj(json!({ "include_system": { "type": "boolean", "description": "Also list information_schema, mysql, performance_schema and sys." } }), &[]),
                "annotations": read }),
            json!({ "name": "list_tables", "title": "List tables",
                "description": "Tables and views of a database with engine, estimated row count and comment.",
                "inputSchema": schema_obj(json!({ "database": db_prop }), &[]), "annotations": read }),
            json!({ "name": "describe_table", "title": "Describe table",
                "description": "Columns (type, null, key, default, extra, comment), indexes, foreign keys and the CREATE TABLE statement of one table.",
                "inputSchema": schema_obj(json!({ "database": db_prop, "table": { "type": "string" } }), &["table"]), "annotations": read }),
            json!({ "name": "get_schema", "title": "Get schema",
                "description": "Compact text schema of a whole database (or selected tables): every column with type and keys plus all foreign keys. Read this before writing SQL against an unfamiliar database.",
                "inputSchema": schema_obj(json!({ "database": db_prop, "tables": { "type": "array", "items": { "type": "string" }, "description": "Only these tables (optional)." } }), &[]),
                "annotations": read }),
            json!({ "name": "query", "title": "Run read-only query",
                "description": "Run exactly one read-only statement (SELECT, WITH, SHOW, DESCRIBE, EXPLAIN) inside a READ ONLY transaction and return columns and rows as JSON. Results are capped by max_rows; long cell values are shortened.",
                "inputSchema": schema_obj(json!({
                    "sql": { "type": "string", "description": "One MariaDB statement, without a trailing semicolon being required." },
                    "database": db_prop,
                    "max_rows": { "type": "integer", "minimum": 1, "maximum": HARD_MAX_ROWS, "description": "Maximum rows to return (default from the EasyMySQL settings)." }
                }), &["sql"]),
                "annotations": read }),
            json!({ "name": "execute", "title": "Execute changes",
                "description": format!("Run one or more statements that change data or structure (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP …). \
                    Before DROP, TRUNCATE or DELETE/UPDATE without WHERE, EasyMySQL backs up the affected databases. Stops at the first error. \
                    Ask the user before destructive changes. {write_state}"),
                "inputSchema": schema_obj(json!({ "sql": { "type": "string", "description": "SQL script; statements separated by `;` (DELIMITER is supported)." }, "database": db_prop }), &["sql"]),
                "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false } }),
            json!({ "name": "start_server", "title": "Start the EasyMySQL server",
                "description": "Start the MariaDB server built into EasyMySQL on this computer (opens EasyMySQL). Only needed when a call reports that the server is not running.",
                "inputSchema": schema_obj(json!({}), &[]),
                "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false } }),
        ]
    }

    fn call(&mut self, name: &str, args: &Value) -> ToolResult {
        let r = match name {
            "server_info" => self.server_info().map(|v| ToolResult::json(&v)),
            "list_databases" => self.list_databases(args).map(|v| ToolResult::json(&v)),
            "list_tables" => self.list_tables(args).map(|v| ToolResult::json(&v)),
            "describe_table" => self.describe_table(args).map(|v| ToolResult::json(&v)),
            "get_schema" => self.get_schema(args).map(ToolResult::ok),
            "query" => self.query(args).map(|v| ToolResult::json(&v)),
            "execute" => self.execute(args).map(|v| ToolResult::json(&v)),
            "start_server" => self.start_server().map(|v| ToolResult::json(&v)),
            _ => Err(format!("Unknown tool {name}")),
        };
        r.unwrap_or_else(ToolResult::err)
    }

    fn instructions(&self) -> String {
        let write = if self.opts.allow_write {
            "Changes are allowed through `execute`; EasyMySQL backs up affected databases before DROP/TRUNCATE and DELETE/UPDATE without WHERE. Still ask the user before destructive changes."
        } else {
            "The user has not allowed changes: only read. If a change is needed, show the SQL and tell the user they can allow writing in EasyMySQL under Settings → AI assistants."
        };
        let default = self.opts.database.as_ref().map(|d| format!(" The default database is `{d}`.")).unwrap_or_default();
        format!(
            "EasyMySQL connects you to a MariaDB (MySQL-compatible) server at {}.{default} \
             Workflow: call `list_databases`, then `get_schema` for the database before writing SQL; use `describe_table` for details. \
             Run reads with `query` (one statement, READ ONLY transaction, use LIMIT for large tables). {write} \
             Write MariaDB SQL: quote identifiers with backticks, use LIMIT instead of TOP. \
             If the server is not running, call `start_server`.",
            self.opts.conn.label()
        )
    }
}
