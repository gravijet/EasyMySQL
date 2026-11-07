# EasyMySQL

MariaDB server and database manager for Windows, macOS and Linux. Includes an SQL editor, table editing, ER diagrams, a query builder and backups.

## Install

Install the newest release with one command. The scripts verify its SHA-256 checksum and offer to connect Claude Code and Codex.

Windows 10/11 (64-bit), in PowerShell:

```powershell
irm https://mysql.benjaminberger.at/install.ps1 | iex
```

macOS 11+ (Apple Silicon and Intel) or Linux x86_64 (glibc 2.35+), in a terminal:

```sh
curl -fsSL https://mysql.benjaminberger.at/install | sh
```

On Windows, MariaDB is included (the newest stable long-term-support version). On macOS, install [Homebrew](https://brew.sh) first; the script offers `brew install mariadb`. On Linux, it offers MariaDB installation through apt, dnf, pacman or zypper. The Unix script installs the `easymysql` command in `~/.local/bin`; add that folder to `PATH` if prompted.

Alternatively, download the Windows installer/portable ZIP, macOS app or Linux archive from [mysql.benjaminberger.at](https://mysql.benjaminberger.at). Launch EasyMySQL to initialize the server.

To connect AI assistants later, run `easymysql setup-ai`. For other MCP clients, use `easymysql mcp` over standard input/output. `easymysql --version` checks the installed version without opening a window.

The default connection uses port `3306`, user `root` and an empty password. The server accepts connections from the same PC only (both can be changed under *Settings → Server*). The installer adds the command-line tools to `PATH`:

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
- **Claude Code and Codex plugins**: open **Plugins → Claude Code / Codex** for a native chat with separate conversations, copyable code blocks and SQL handoff to the query editor. Install or update the official assistants with checksum verification and sign in through your browser. Select a trusted project folder; file edits require an explicit checkbox. Database access uses **Settings → AI assistants** and its separate permissions. Your provider account, usage limits and costs apply. Chats remain visible until closed; the assistants keep their own local session files. See the official [Codex](https://developers.openai.com/codex/noninteractive) and [Claude Code](https://code.claude.com/docs/en/headless) integration documentation.
- Open an integrated terminal (**View → New terminal**, `Ctrl+\``) with several tabs; PowerShell, Command Prompt or the MariaDB console.
- Preview images and PDF files directly from the project explorer.
- **Java**: *File → New Java project* creates a simple, Maven or Gradle project with a ready connection (`Database.connect()`) and a record plus DAO class for every chosen table. ▶ Run compiles it and starts it in the terminal (input works too); compile errors are underlined in the code. EasyMySQL downloads whatever is missing – the Temurin JDK (newest LTS), MariaDB and MySQL Connector/J, Maven and Gradle – always the newest stable version with checksum verification, and looks for updates once a week (*Settings → Java*). In Java files, SQL inside strings is highlighted, completed and checked against the chosen database, and *Insert* adds code for a connection or a table. Terminals have `java`, `javac`, `mvn`, `gradle` and both drivers on the class path. Tables and queries can also be copied as ready-to-run JDBC code.
- **Python**: *File → New Python project* creates a project with `requirements.txt` or `pyproject.toml`, a connection module and a dataclass plus repository for every chosen table. ▶ Run creates a `.venv`, installs the packages, checks every file for syntax errors and starts the program. EasyMySQL downloads Python (newest stable, python-build-standalone) and PyMySQL or MySQL Connector/Python when missing; scripts without a project find the driver on `PYTHONPATH`.
- **JavaScript and TypeScript**: *File → New JavaScript project* creates a Node.js project with `mysql2` or the MariaDB connector, a connection pool and a repository class per table. ▶ Run runs `npm install` when `package.json` changes, checks the syntax and starts the file; `.ts` files run directly. Node.js (newest LTS) and the driver download themselves; npm, pnpm and yarn are supported.
- **Coding features** for all three languages: run tests (JUnit, pytest/unittest, `node --test`), a Java debugger with breakpoints, stepping, variables and expression evaluation, a self-contained `.jar` (`dist/<project>.jar`), adding libraries from Maven Central, PyPI or npm through a search dialog, and language servers (Eclipse JDT LS, Pyright, TypeScript) for errors while typing, go to definition (`F12`, `Ctrl+click`), project-wide rename (`F2`), hover hints and formatting. SQL inside strings is highlighted, completed and checked in every language.
- **Git and GitHub**: the *Source control* sidebar shows changes and diffs, stages, commits, switches/creates/merges branches, pulls and pushes, and shows the history. Sign in to GitHub (personal token or GitHub CLI) to publish a project as a new repository, clone your repositories, create and merge pull requests, create issues and watch Actions runs. On Windows, Git (MinGit) downloads itself.
- **Everything is configurable**: per language which runtime to use (automatic, own download, system or a path), compiler/JVM/Python/Node/pip/Maven/Gradle options, program arguments, environment variables, working folder, syntax check, terminal reuse, tool update interval, language servers per language, editor indentation and display, terminal font/scrollback/shell, Git identity and pull mode, rows per result and page, history length and more.
- New files: type the name, pick the extension that stays on the right (SQL, Java, Python, JavaScript, TypeScript with a skeleton, Markdown, JSON …).
- Configure the built-in server under *Settings → Server*: port, access from this PC only or the network, data folder (moved with its data), memory, connections, packet size, default collation, event scheduler, extra `my.ini` lines and whether it starts with EasyMySQL.
- **Encryption (TLS)** for the built-in server with its own certificate authority: EasyMySQL, JDBC, Python and Node programs connect encrypted and verify the certificate; *Encrypted only* rejects unencrypted network connections. The CA certificate can be saved for other computers.
- **Java autofix** works in the editor before compilation: missing standard imports, common `println` typos, missing semicolons after print calls and supported compiler/language-server diagnostics offer fixes. *More → Java fixes* requests actual JDT quick fixes and shows their edits before applying them. Missing tables and columns are derived first from SQL migrations (`CREATE TABLE`, additive `ALTER TABLE`) and JPA entities (field/getter access, IDs, identity generation, column lengths/precision, explicit defaults, unique keys and supported join columns), then from unambiguous JDBC SQL. Constant variables, concatenation, text blocks, StringBuilder and multiple prepared-statement receivers are supported. The confirmation shows the target server, database and complete SQL, including declared dependencies, keys and relationships. Existing columns are preserved; NULLs, duplicates and required-column backfills are checked before adding constraints. Runtime-dependent identifiers, unsupported mappings and non-additive migrations require an explicit schema/migration rather than guessing.
- **MariaDB updates in existing installations** without administrator rights: EasyMySQL finds the newest stable LTS version, backs up all databases and copies the data folder, switches the server to the new version and goes back automatically if it does not start.
- Manage automatic backups and restores under **Server → Backups & repair**.
- Updates download and install in the background without closing the app (switch on *Settings → Updates → Update automatically* to skip the question); afterwards a button restarts EasyMySQL.
- Open the searchable manual with `F1`.

English and dark mode are the defaults. German and a light theme are available in Settings.

## Build

Requires stable Rust.

```console
cargo build --release --locked
cargo clippy --locked
cargo test --locked
```

The [release workflow](.github/workflows/release.yml) builds the Windows installer and portable ZIP, a universal macOS app and a Linux archive. Some ignored tests require a running database or are manual rendering benchmarks.

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
