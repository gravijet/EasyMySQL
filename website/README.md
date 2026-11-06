# EasyMySQL website

Static site plus a small Cloudflare Worker (`src/worker.js`) for `mysql.benjaminberger.at`.

- `public/` is the page (HTML, CSS, JS). Colors and layout follow `src/style.rs`.
- `/api/release` returns the newest release (version, size, SHA-256) as JSON. It is cached at the edge for five minutes and falls back to the last known value.
- `/download/setup` and `/download/portable` stream the files of the newest release. Visitors only ever see this domain.

Every push to `main` deploys the current website through `.github/workflows/website.yml`, without waiting for Rust tests or an app release. Successful app releases also trigger a deployment. GitHub Actions and Cloudflare need a short time to finish; CSS and JavaScript are revalidated when the page is loaded again.

The repository secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` are required. Missing credentials fail the deployment visibly. To deploy by hand:

```console
cd website
npm install
CLOUDFLARE_API_TOKEN=... npx wrangler deploy
```

Set the secret `GITHUB_TOKEN` (`npx wrangler secret put GITHUB_TOKEN`) if the unauthenticated release lookup ever gets rate limited. Without it the worker falls back to resolving the latest tag.
