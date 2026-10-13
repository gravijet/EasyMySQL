# EasyMySQL

MariaDB server and database manager for Windows. Includes an SQL editor, table editing, ER diagrams, a query builder and backups.

## Install

Download the installer or portable ZIP from [mysql.benjaminberger.at](https://mysql.benjaminberger.at). MariaDB is included. Launch EasyMySQL to initialize the server.

The default connection uses port `3306`, user `root` and an empty password. The server accepts connections from the same PC only. The installer adds the command-line tools to `PATH`:

```console
mysql -u root
```

Closing the window leaves the server running in the system tray. Use **File → Quit** or the tray menu to stop it.

## Usage

- Write and run SQL with context-aware suggestions, typo and name checking (`Ctrl+.` fixes everything it can), formatting, and execution plans; export results as CSV, JSON or SQL.
- Edit values and structure right in the table: rename, retype, move, copy and delete columns from the header menu, add rows with one click, undo with `Ctrl+Z`.
- Copy, rename and merge tables (rows below each other or joined on matching values).
- Export a database or selected tables as SQL, JSON, CSV, XML or a SQLite file; import SQLite files, MongoDB exports (JSON, BSON), CSV files and Redis data.
- Manage users and privileges per database or table under **Server → Users & privileges**.
- Reach a database on another machine through an SSH tunnel (**Settings → Connection**).
- Rerun earlier statements from the history tab next to the results.
- Build queries visually, including INSERT, UPDATE and DELETE, with undo and a live preview.
- Arrange ER diagrams and export them as SVG or SQL.
- Open an integrated terminal (`Ctrl+\``) with several tabs; PowerShell, Command Prompt or the MariaDB console.
- Preview images and PDF files directly from the project explorer.
- Copy a table or a query as ready-to-run Java (JDBC) code.
- Manage automatic backups and restores under **Server → Backups & repair**.
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

MIT license. Bundled MariaDB uses GPL v2.
