# EasyMySQL website

Static site plus a small Cloudflare Worker (`src/worker.js`) for `mysql.benjaminberger.at`.

- `public/` is the bilingual page: `index.html`, `style.css`, `app.js` (language, release info, scroll scenes) and `gl.js` (WebGL background with drifting clouds and the 3D database icon). Two sections stay pinned while scrolling: six app screenshots and eight measurements. Design rules are in `DESIGN.md`.
- `/api/release` returns the newest release (version, size, SHA-256) as JSON. It is cached at the edge for five minutes and falls back to the last known value.
- `/download/setup`, `/download/portable`, `/download/macos` and `/download/linux` stream the files of the newest release. Visitors only ever see this domain. Missing packages are marked unavailable. The terminal command is visible above file downloads; system requirements are collapsed beneath them.

Every push to `main` deploys the current website through `.github/workflows/website.yml`, without waiting for Rust tests or an app release. Successful app releases also trigger a deployment. GitHub Actions and Cloudflare need a short time to finish; CSS and JavaScript are revalidated when the page is loaded again.

The repository secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` are required. Missing credentials fail the deployment visibly. To deploy by hand:

```console
cd website
npm install
CLOUDFLARE_API_TOKEN=... npx wrangler deploy
```

Set the secret `GITHUB_TOKEN` (`npx wrangler secret put GITHUB_TOKEN`) if the unauthenticated release lookup ever gets rate limited. Without it the worker falls back to resolving the latest tag.

`public/benchmarks.json` records five measured runs of EasyMySQL 1.1.15 (built as 7.1.0 before the renumbering), MySQL Workbench 26.10.0 and XAMPP Portable 8.2.12 with phpMyAdmin 5.2.1 / Chrome 156 on this Windows PC. The page charts start time and RAM; the file also holds private resident RAM, private committed memory, instrumented warm-start readiness and total-capacity CPU share during idle. Client process trees are included; the isolated shared MariaDB server and GPU memory are excluded. Workbench and phpMyAdmin are measured at loaded start pages; EasyMySQL uses Windows `WaitForInputIdle`. These readiness markers differ, so the start row is not an identical first-paint ranking. The raw data records every sample, hardware, tool versions and binary hashes.

Reproduce with `python -m pip install psutil==7.2.2 playwright==1.60.0`, then `python website/measure-runtime.py --bench-root <prepared-test-directory>`. The prepared directory contains `easymysql/EasyMySQL.exe`, `workbench/MySQL/MySQL Workbench/MySQL Workbench.exe`, and an extracted `xampp/xampp/`. Use the official release links and exact versions in the JSON. Relocate the portable XAMPP configuration with its supplied `php -n install/install.php usb`; bind Apache to `127.0.0.1:18080`, MariaDB to `127.0.0.1:13307`, and point phpMyAdmin at that database port. Start these isolated servers before measuring; an installed Chrome is required. The script creates fresh dedicated profiles, warms each app, takes five runs and stops only its own client processes. Do not run against real database profiles. Update the displayed snapshot and JSON together after another measurement.

The earlier headless UI render benchmark remains reproducible with `powershell -File website/measure-performance.ps1`; it writes `public/render-benchmarks.json` (or `-Ausgabepfad <path>`) and does not overwrite the cross-application runtime comparison.
