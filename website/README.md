# EasyMySQL website

Static site plus a small Cloudflare Worker (`src/worker.js`) for `mysql.benjaminberger.at`.

- `public/` is the page (HTML, CSS, JS). Colors and layout follow `src/style.rs`.
- `/api/release` returns the newest release (version, size, SHA-256) as JSON. It is cached at the edge for five minutes and falls back to the last known value.
- `/download/setup` and `/download/portable` stream the files of the newest release. Visitors only ever see this domain.

Deploy:

```console
cd website
npm install
CLOUDFLARE_API_TOKEN=... npx wrangler deploy
```

Set the secret `GITHUB_TOKEN` (`npx wrangler secret put GITHUB_TOKEN`) if the unauthenticated release lookup ever gets rate limited. Without it the worker falls back to resolving the latest tag.
