# EasyMySQL website

## Product and audience

EasyMySQL is a free, MIT-licensed native database and development application for Windows, macOS and Linux. It combines MariaDB server management, SQL editing, editable tables, ER diagrams, a visual query builder, imports/exports, MCP integration, code projects, a terminal and Git/GitHub tools. Developers working with local SQL databases are the inferred primary audience; this audience description follows the README and implemented feature set rather than user research.

The website helps visitors understand the application, inspect real screenshots and install the current release. Its scope is the public website, not the native application's interface.

## Durable constraints

- Preserve real application screenshots and German/English behavior. Final user direction of 2026-11-11, chosen when asked: the whole card/icon/pill/check-circle style and the 3D charts looked like generic AI graphics; the page should be plain and large instead (very big numbers, thin clean bars, much white space, motion only for fading in and counting up). Terminal installation is the main action and file downloads are secondary; never advertise that the app is free; little text and no technical details; numbers may be rounded and the page does not comment on that. See DESIGN.md.
- Keep copy concise and concrete. Terminal installation precedes the equally discoverable direct Windows EXE, macOS and Linux file options near the top. Avoid a large script demonstration or filler content.
- Package availability comes from `/api/release` and the existing `/download/*` routes. Missing packages must have an explicit unavailable state. At this review the latest release lacks a macOS package; this does not mean the application lacks macOS support.
- Windows includes MariaDB. macOS and Linux require separate MariaDB installation; installation guidance must preserve that distinction.
- Numbers come from the user's own comparison of 2026-11-11 (EasyMySQL 1.1.17, XAMPP 8.5.12 with phpMyAdmin 5.2.3, MySQL Workbench 26.10.0, one million rows), stored outside the repo in `/home/benj/client-benchmark-2026-11`. The page shows only measurements where the task is the same for all products. Workbench 26.10 could not connect to MariaDB and has no ER diagram and no CSV import; phpMyAdmin stops at its 40 MB upload limit.
- The page shows only start time, memory (in MB) and download size; versions and method stay in `public/benchmarks.json` and README.md, not on the page. Download sizes compare the EasyMySQL setup (77 MB, with MariaDB), the XAMPP 8.5.12 installer (191 MB) and the Workbench 26.10.0 MSI (261 MB). Workbench 26 is a rebuilt Electron application, so do not carry forward unverified legacy Workbench 8 modeling features.
- Keep keyboard operation, visible focus, reduced-motion support and narrow-screen reading intact in both languages.

## Evidence

Product capabilities and platform requirements: `../README.md`. Website behavior and wording: `public/index.html`, `public/app.js` and `public/style.css`. Performance evidence: `public/benchmarks.json`. Refinement constraints derive from the user's website request and follow-ups.
