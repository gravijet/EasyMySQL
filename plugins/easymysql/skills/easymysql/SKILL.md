---
name: easymysql
description: Work with MariaDB/MySQL databases through the EasyMySQL MCP server — explore schemas, write and run SQL, analyse data, design tables and migrations, and change data safely. Use whenever the user mentions their database, tables, SQL, MariaDB, MySQL, EasyMySQL, or asks a question that their data can answer.
---

# EasyMySQL

EasyMySQL is a desktop app with a built-in MariaDB server. Its MCP server (`easymysql mcp`) gives you these tools:

| Tool | Use it for |
| --- | --- |
| `server_info` | Version, user, `sql_mode`, whether writing is allowed. Call once first. |
| `list_databases` | Find the database the user means. |
| `get_schema` | Compact schema of a database: columns, keys, foreign keys. Read it **before writing SQL**. |
| `list_tables`, `describe_table` | Row estimates, indexes, `CREATE TABLE` for one table. |
| `query` | One read-only statement (`SELECT`, `WITH`, `SHOW`, `DESCRIBE`, `EXPLAIN`), run in a READ ONLY transaction. |
| `execute` | Changes (`INSERT`, `UPDATE`, `DELETE`, DDL). Only works when the user allowed writing. |
| `start_server` | Starts the EasyMySQL server when a call says it is not running. |

## Workflow

1. Resolve the database: use the one the user named, otherwise `list_databases` and pick the obvious one or ask.
2. Load the structure with `get_schema` (pass `tables` for big databases). Never guess column names.
3. Answer questions with `query`. Start small: `LIMIT 20`, `COUNT(*)`, aggregates. Results are capped (`truncated: true` means there is more).
4. For slow or important queries, check `EXPLAIN` and suggest indexes that match the `WHERE`/`JOIN` columns.
5. Present results as a short table or a sentence, and show the SQL you ran when it helps the user learn or reuse it.

## Changing data

- Show the exact SQL and what it will affect (run a `SELECT COUNT(*)` with the same `WHERE` first), then ask before running `execute` for anything destructive: `DROP`, `TRUNCATE`, `DELETE`/`UPDATE` affecting many rows, `ALTER TABLE … DROP`.
- EasyMySQL backs up affected databases automatically before `DROP`, `TRUNCATE` and `DELETE`/`UPDATE` without `WHERE`; mention the backup folder it reports.
- `execute` runs statements in order with autocommit and stops at the first error. Wrap related changes in `START TRANSACTION; … COMMIT;` when they must succeed together.
- If writing is disabled, do not work around it. Give the user the SQL and tell them they can allow changes in EasyMySQL under **Settings → AI assistants** (or paste the SQL into an EasyMySQL query tab).

## MariaDB dialect

- Quote identifiers with backticks: `` `order` ``, `` `user name` ``. Strings use single quotes.
- `LIMIT n OFFSET m`, not `TOP`. `IFNULL`/`COALESCE`, `CONCAT()`, `GROUP_CONCAT()`, `DATE_FORMAT()`, `NOW()`, `CURDATE()`.
- Window functions and CTEs (`WITH`, `WITH RECURSIVE`) are supported. `RETURNING` works for `INSERT`/`DELETE`.
- New tables: `ENGINE=InnoDB`, `DEFAULT CHARSET=utf8mb4`, an `id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY` (or `BIGINT`), `DECIMAL(10,2)` for money, foreign keys with matching types and an index.
- Inspect with `SHOW CREATE TABLE`, `SHOW INDEX FROM t`, `SHOW TABLE STATUS`, `SHOW PROCESSLIST`.

## Code that talks to the database

EasyMySQL users often write Java (JDBC), Python (PyMySQL / MySQL Connector) or JavaScript (`mysql2`). Default connection: host `127.0.0.1`, port `3306`, user `root`, empty password (check `server_info`). Always use prepared statements / placeholders, never string concatenation.
