# EasyMySQL website

## Product and audience

EasyMySQL is a free, MIT-licensed native database and development application for Windows, macOS and Linux. It combines MariaDB server management, SQL editing, editable tables, ER diagrams, a visual query builder, imports/exports, MCP integration, code projects, a terminal and Git/GitHub tools. Developers working with local SQL databases are the inferred primary audience; this audience description follows the README and implemented feature set rather than user research.

The website helps visitors understand the application, inspect real screenshots and install the current release. Its scope is the public website, not the native application's interface.

## Durable constraints

- Preserve real application screenshots and German/English behavior. User direction of 2026-11-11: round and smooth, not angular; no typical AI look (color gradients, glows, hover lighting, blur fades, ticker); real 3D animation, especially for the statistics; terminal installation is the main action and file downloads are secondary; never advertise that the app is free; little text and no technical details for ordinary users, pupils and teachers. See DESIGN.md.
- Keep copy concise and concrete. Terminal installation precedes the equally discoverable direct Windows EXE, macOS and Linux file options near the top. Avoid a large script demonstration or filler content.
- Package availability comes from `/api/release` and the existing `/download/*` routes. Missing packages must have an explicit unavailable state. At this review the latest release lacks a macOS package; this does not mean the application lacks macOS support.
- Windows includes MariaDB. macOS and Linux require separate MariaDB installation; installation guidance must preserve that distinction.
- Compare measured runtime values across the actual named versions. The recorded five-run Windows client test compares EasyMySQL 1.1.15 (built as 7.1.0 before the renumbering), Workbench 26.10.0 and XAMPP Portable 8.2.12 with phpMyAdmin and dedicated Chrome. It measures private resident RAM, private commit, idle CPU and instrumented start readiness; includes client subprocesses and Apache/PHP, and excludes the shared isolated MariaDB server and GPU memory. Readiness markers differ by application and do not establish an identical first-paint or SQL-speed ranking. Raw samples and method are in `public/benchmarks.json`; the runner is `measure-runtime.py`.
- The page shows only start time, memory (in MB) and download size; versions and method stay in `public/benchmarks.json` and README.md, not on the page. Download sizes compare the EasyMySQL setup (77 MB, with MariaDB), the XAMPP 8.5.12 installer (191 MB) and the Workbench 26.10.0 MSI (261 MB). Workbench 26 is a rebuilt Electron application, so do not carry forward unverified legacy Workbench 8 modeling features.
- Keep keyboard operation, visible focus, reduced-motion support and narrow-screen reading intact in both languages.

## Evidence

Product capabilities and platform requirements: `../README.md`. Website behavior and wording: `public/index.html`, `public/app.js` and `public/style.css`. Performance evidence: `public/benchmarks.json`. Refinement constraints derive from the user's website request and follow-ups.
