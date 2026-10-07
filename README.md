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

- Write and run SQL, inspect execution plans, and export results as CSV, JSON or SQL.
- Browse and edit tables, import SQL, and manage database structure.
- Arrange ER diagrams and export them as SVG or SQL.
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
