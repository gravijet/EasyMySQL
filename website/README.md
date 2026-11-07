# EasyMySQL website

Static site plus a small Cloudflare Worker (`src/worker.js`) for `mysql.benjaminberger.at`.

- `public/` is the bilingual page (HTML, CSS, JS), with terminal installation and direct downloads at the top, app screenshots, measured UI timings and a sourced feature/download-size comparison. It retains the app's blue accent and self-hosted fonts.
- `/api/release` returns the newest release (version, size, SHA-256) as JSON. It is cached at the edge for five minutes and falls back to the last known value.
- `/download/setup`, `/download/portable`, `/download/macos` and `/download/linux` stream the files of the newest release. Visitors only ever see this domain. Missing packages are marked unavailable; terminal installation is optional and collapsed by default.

Every push to `main` deploys the current website through `.github/workflows/website.yml`, without waiting for Rust tests or an app release. Successful app releases also trigger a deployment. GitHub Actions and Cloudflare need a short time to finish; CSS and JavaScript are revalidated when the page is loaded again.

The repository secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` are required. Missing credentials fail the deployment visibly. To deploy by hand:

```console
cd website
npm install
CLOUDFLARE_API_TOKEN=... npx wrangler deploy
```

Set the secret `GITHUB_TOKEN` (`npx wrangler secret put GITHUB_TOKEN`) if the unauthenticated release lookup ever gets rate limited. Without it the worker falls back to resolving the latest tag.

The performance section reports measured CPU UI preparation time, excluding GPU presentation and SQL execution. `public/benchmarks.json` contains all seven samples, medians, build settings, source fingerprints and hardware. Reproduce on Windows from the repository root with `powershell -File website/measure-performance.ps1`. The page's timings are a dated snapshot and must be updated alongside the JSON after a new measurement. Windows package sizes are a separate comparison of actual release file sizes, not a runtime speed ranking.
