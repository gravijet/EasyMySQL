# EasyMySQL

MariaDB server and database manager for Windows. Includes an SQL editor, table editing, ER diagrams, a query builder and backups.

## Install

Download the installer or portable ZIP from [mysql.benjaminberger.at](https://mysql.benjaminberger.at). MariaDB is included. Launch EasyMySQL to initialize the server.

The default connection uses port `3306`, user `root` and an empty password. The server accepts connections from the same PC only. The installer adds the command-line tools to `PATH`:

```console
mysql -u root
```

Closing the window quits EasyMySQL and shuts the server down cleanly. If you prefer it to keep running, turn on *Settings → Start → Keep running in the tray*.

## Usage

- Write and run SQL with context-aware suggestions, typo and name checking (`Ctrl+.` fixes everything it can), formatting, and execution plans; export results as CSV, JSON or SQL.
- Edit values and structure right in the table: rename, retype, move, copy and delete columns from the header menu, add rows with one click, undo with `Ctrl+Z`.
- Copy, rename and merge tables (rows below each other or joined on matching values).
- **Import** from SQLite, MongoDB, CSV, Redis (server or `dump.rdb`), another MariaDB/MySQL server and PostgreSQL. Every source also works from another computer: as a file from a web address or over SSH, and as a server with TLS and an SSH tunnel (key file or password).
- **Export** a database or selected tables as SQL, JSON, CSV, XML, SQLite, a PostgreSQL script, MongoDB JSON lines, a Redis file or redis-cli commands, or copy it straight to another MariaDB/MySQL, PostgreSQL or Redis server.
- Manage users and privileges per database or table under **Server → Users & privileges**.
- Reach a database on another machine through an SSH tunnel (**Settings → Connection**).
- Each SQL file remembers the database it works in and can run on a different saved server (default: the global one); the ER diagram and the query builder open for the database of the file you are in.
- ER diagrams for large databases: layered layout with few crossings, right-angled lines that share a track where they start at the same column, unlimited zoom out with names that stay readable. Export as SVG or SQL.
- Build queries visually, including INSERT, UPDATE and DELETE, with undo and a live preview.
- Open an integrated terminal (**View → New terminal**, `Ctrl+\``) with several tabs; PowerShell, Command Prompt or the MariaDB console.
- Preview images and PDF files directly from the project explorer; copy a table or a query as ready-to-run Java (JDBC) code.
- Manage automatic backups and restores under **Server → Backups & repair**.
- Updates download and install in the background without closing the app (switch on *Settings → Updates → Update automatically* to skip the question); afterwards a button restarts EasyMySQL.
- Open the searchable manual with `F1`.

English and dark mode are the defaults. German and a light theme are available in Settings.

## Build

Requires stable Rust.

```console
cargo build --release --locked
cargo test --locked
```

The [release workflow](.github/workflows/release.yml) builds the Windows installer. Some ignored tests require a running database or are manual rendering benchmarks.

Linux development builds use `/usr/sbin/mariadbd`; set `EASYMYSQL_MARIADB_BIN` to use a different binary.

## Versioning

The version number follows the size of the changes:

| Change | Number | Example |
| --- | --- | --- |
| Complete redesign or many new features at once | **major** (`4.0.0`) | new look, new importers, new updater |
| A new feature or a clearly noticeable improvement | **minor** (`4.1.0`) | one more export format |
| Bug fixes and small adjustments | **patch** (`4.1.1`) | a wrong label, a crash |

A release is only built when `version` in `Cargo.toml` has no tag yet, so each published number is a deliberate decision and small pushes never produce a release.

MIT license. Bundled MariaDB uses GPL v2.
