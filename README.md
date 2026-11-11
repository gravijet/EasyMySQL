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

To download and connect Claude Code and Codex later, run `easymysql install-ai`. For other MCP clients, use `easymysql mcp` over standard input/output. `easymysql --version` checks the installed version without opening a window.

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
- **Claude Code and Codex plugins**: dedicated tool windows with brand icons in the **left sidebar**. Select a model, stream real assistant messages, inspect tool activity, compact the provider's actual conversation context, and resume locally saved chats. Codex discovers models and reasoning efforts through the official [App Server](https://learn.chatgpt.com/docs/app-server); Claude uses its [streaming SDK protocol](https://code.claude.com/docs/en/headless) with Sonnet/Opus/Haiku aliases. Installation, updates and browser sign-in are in the tool-window menu. File edits require permission; database access has separate settings. Provider account limits and costs apply. Conversation files stay in the app data directory with private file permissions on Unix.
- Open an integrated terminal (**View → New terminal**, `Ctrl+\``) with several tabs; PowerShell, Command Prompt or the MariaDB console.
- Preview images and PDF files directly from the project explorer.
- **Java**: *File → New Java project* creates a simple, Maven or Gradle project with a ready connection (`Database.connect()`) and a record plus DAO class for every chosen table. ▶ Run compiles it and starts it in the terminal (input works too); compile errors are underlined in the code. EasyMySQL downloads whatever is missing – the Temurin JDK (newest LTS), MariaDB and MySQL Connector/J, Maven and Gradle – always the newest stable version with checksum verification, and looks for updates once a week (*Settings → Java*). In Java files, SQL inside strings is highlighted, completed and checked against the chosen database, and *Insert* adds code for a connection or a table. Terminals have `java`, `javac`, `mvn`, `gradle` and both drivers on the class path. Tables and queries can also be copied as ready-to-run JDBC code.
- **Coding features**: run tests (JUnit), a Java debugger with breakpoints, stepping, variables and expression evaluation, a self-contained `.jar` (`dist/<project>.jar`), adding libraries from Maven Central through a search dialog, and the Eclipse JDT language server for errors while typing, go to definition (`F12`, `Ctrl+click`), project-wide rename (`F2`), hover hints and formatting. The editor also generates getters, setters and constructors, surrounds code with try/catch, moves lines and shows the file structure (`Ctrl+F12`).
- **Git and GitHub**: the *Source control* sidebar shows changes and diffs, stages, commits, switches/creates/merges branches, pulls and pushes, and shows the history. Sign in to GitHub (personal token or GitHub CLI) to publish a project as a new repository, clone your repositories, create and merge pull requests, create issues and watch Actions runs. On Windows, Git (MinGit) downloads itself.
- **Everything is configurable**: per language which runtime to use (automatic, own download, system or a path), compiler/JVM/Maven/Gradle options, program arguments, environment variables, working folder, terminal reuse, tool update interval, the language server, editor indentation and display, terminal font/scrollback/shell, Git identity and pull mode, rows per result and page, history length and more.
- New files: type the name, pick the extension that stays on the right (SQL, Java with a skeleton, Markdown, JSON …).
- Configure the built-in server under *Settings → Server*: port, access from this PC only or the network, data folder (moved with its data), memory, connections, packet size, default collation, event scheduler, extra `my.ini` lines and whether it starts with EasyMySQL.
- **Encryption (TLS)** for the built-in server with its own certificate authority: EasyMySQL and JDBC programs connect encrypted and verify the certificate; *Encrypted only* rejects unencrypted network connections. The CA certificate can be saved for other computers.
- **Java autofix** works in the editor before compilation: missing standard imports, common `println` typos, missing semicolons after print calls and supported compiler/language-server diagnostics offer fixes. *More → Java fixes* requests actual JDT quick fixes and shows their edits before applying them. Missing tables and columns are derived first from SQL migrations (`CREATE TABLE`, additive `ALTER TABLE`) and JPA entities (field/getter access, IDs, identity generation, column lengths/precision, explicit defaults, unique keys and supported join columns), then from unambiguous JDBC SQL. Constant variables, concatenation, text blocks, StringBuilder and multiple prepared-statement receivers are supported. Constant JDBC `CREATE TABLE` definitions are also supported. Fully declared new tables in a known database can be created in one click, including required tables, keys, indexes, defaults and relationships. Existing tables and inferred types retain the SQL preview. JDBC inference merges compatible columns across DAO files. Existing columns are preserved; NULLs, duplicates and required-column backfills are checked before adding constraints. Runtime-dependent identifiers, unsupported mappings and non-additive migrations require an explicit schema/migration rather than guessing.
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

Version numbers grow in **small steps**. The current line is `1.1.x`; numbers like `7.4.0` are gone.

| Change | Number | Example |
| --- | --- | --- |
| Normal release: fixes, improvements, new features | **patch** (`1.1.17` → `1.1.18`) | new chat panel, website redesign, a crash fix |
| Rare milestone, only when explicitly requested | **minor** (`1.1.x` → `1.2.0`) | a complete rebuild of the app |
| Never without the owner's explicit decision | **major** (`2.0.0`) | – |

Every release also has an internal running number, `build-number` under `[package.metadata.easymysql]` in `Cargo.toml`: +1 per release (`1.0.0` = 1, `1.1.16` = 29, `1.1.17` = 30, the 1000th release = 1000). Users see only the version; the number appears as a tooltip on the version in Settings › Updates.

On November 10, 2026 the 29 older releases were renumbered without rebuilding them: `1.0.0`–`3.5.0` became `1.0.0`–`1.0.11`, `4.0.0`–`7.4.0` became `1.1.0`–`1.1.16`. Programs installed before that still report their old number (up to `7.4.0`); `website/src/worker.js` maps the numbers for them so they keep receiving updates. New programs send `X-Version-Scheme: 2` and see the real number.

App releases are built only on explicit request by manually running **EasyMySQL Release** in GitHub Actions. Pushes do not build Windows packages. The **Tests** workflow also skips the Windows UI build on pushes; its Windows job runs only when that workflow is started manually. Linux/macOS tests and the website workflow still run on pushes. Bump `Cargo.toml` normally; publish the new version when requested.

MIT license. Bundled MariaDB uses GPL v2.
