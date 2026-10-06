// Liefert die Website aus und stellt immer das neueste Release bereit.
// Besucher sehen nur Adressen dieser Domain: /api/release liefert die Metadaten,
// /download/<art> streamt die Datei durch. Die Quelle wird nie ausgeliefert.

const REPO = "gravijet/EasyMySQL";
const FRESH_SECONDS = 300;
const STALE_SECONDS = 60 * 60 * 24 * 7;

const KINDS = {
  setup: { match: /^EasyMySQL-Setup-[\d.]+\.exe$/, name: (v) => `EasyMySQL-Setup-${v}.exe`, type: "application/vnd.microsoft.portable-executable" },
  portable: { match: /^EasyMySQL-portable-[\d.]+\.zip$/, name: (v) => `EasyMySQL-portable-${v}.zip`, type: "application/zip" },
};

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "cache-control": "public, max-age=60",
  "access-control-allow-origin": "*",
  "x-content-type-options": "nosniff",
};

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    try {
      if (url.pathname === "/api/release") return await releaseJson(request, env, ctx);
      if (url.pathname.startsWith("/download/")) return await download(request, env, ctx, url.pathname.slice("/download/".length));
    } catch (e) {
      return json({ error: "unavailable" }, 503);
    }
    return env.ASSETS.fetch(request);
  },
};

function json(body, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: JSON_HEADERS });
}

// ---------------------------------------------------------------------------
// Release ermitteln (Edge-Cache, bei Fehlern letzte bekannte Fassung)

async function getRelease(env, ctx) {
  const cache = caches.default;
  const key = new Request(`https://cache.invalid/release/${REPO}`);
  const hit = await cache.match(key);
  let stale = null;
  if (hit) {
    const entry = await hit.json();
    if (Date.now() - entry.at < FRESH_SECONDS * 1000) return entry.release;
    stale = entry.release;
  }
  let release = null;
  try {
    release = await fromApi(env);
  } catch {}
  if (!release) {
    try {
      release = await fromRedirect();
    } catch {}
  }
  if (!release) {
    if (stale) return stale;
    throw new Error("no release");
  }
  const body = JSON.stringify({ at: Date.now(), release });
  ctx.waitUntil(cache.put(key, new Response(body, { headers: { "content-type": "application/json", "cache-control": `max-age=${STALE_SECONDS}` } })));
  return release;
}

function ghHeaders(env, accept) {
  const h = { "user-agent": "easymysql-site", accept };
  if (env.GITHUB_TOKEN) h.authorization = `Bearer ${env.GITHUB_TOKEN}`;
  return h;
}

async function fromApi(env) {
  const r = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: ghHeaders(env, "application/vnd.github+json") });
  if (!r.ok) throw new Error(`api ${r.status}`);
  const d = await r.json();
  const version = String(d.tag_name || "").replace(/^v/, "");
  if (!/^\d+\.\d+\.\d+$/.test(version) || d.prerelease || d.draft) throw new Error("bad release");
  const files = {};
  for (const a of d.assets || []) {
    for (const [kind, k] of Object.entries(KINDS)) {
      if (k.match.test(a.name) && a.browser_download_url?.startsWith(`https://github.com/${REPO}/releases/download/`)) {
        files[kind] = { source: a.browser_download_url, size: a.size ?? null, sha256: /^sha256:([0-9a-f]{64})$/.exec(a.digest || "")?.[1] ?? null };
      }
    }
  }
  if (!files.setup) throw new Error("no setup asset");
  const mariadb = /MariaDB\s+(\d+\.\d+\.\d+)/.exec(d.body || "")?.[1] ?? null;
  return { version, published: d.published_at ?? null, mariadb, files };
}

// Ohne API (z. B. Rate-Limit): Tag aus der Weiterleitung lesen, Dateinamen sind festgelegt.
async function fromRedirect() {
  const r = await fetch(`https://github.com/${REPO}/releases/latest`, { redirect: "manual", headers: { "user-agent": "easymysql-site" } });
  const version = /\/releases\/tag\/v(\d+\.\d+\.\d+)$/.exec(r.headers.get("location") || "")?.[1];
  if (!version) throw new Error("no tag");
  const files = {};
  for (const [kind, k] of Object.entries(KINDS)) {
    files[kind] = { source: `https://github.com/${REPO}/releases/download/v${version}/${k.name(version)}`, size: null, sha256: null };
  }
  return { version, published: null, mariadb: null, files };
}

async function releaseJson(request, env, ctx) {
  const rel = await getRelease(env, ctx);
  const files = {};
  for (const [kind, f] of Object.entries(rel.files)) {
    files[kind] = { name: KINDS[kind].name(rel.version), url: `/download/${kind}`, size: f.size, sha256: f.sha256 };
  }
  return json({ version: rel.version, published: rel.published, mariadb: rel.mariadb, files });
}

// ---------------------------------------------------------------------------
// Download durchreichen

async function download(request, env, ctx, kind) {
  if (!["GET", "HEAD"].includes(request.method)) return new Response("Method not allowed", { status: 405, headers: { allow: "GET, HEAD" } });
  const k = KINDS[kind];
  if (!k) return new Response("Not found", { status: 404 });
  const rel = await getRelease(env, ctx);
  const f = rel.files[kind];
  if (!f || !f.source.startsWith(`https://github.com/${REPO}/releases/download/`)) return new Response("Not found", { status: 404 });

  const headers = { "user-agent": "easymysql-site" };
  const range = request.headers.get("range");
  if (range) headers.range = range;
  const up = await fetch(f.source, { method: request.method, headers, redirect: "follow" });
  if (!up.ok && up.status !== 206) return new Response("Download currently unavailable", { status: 502, headers: { "retry-after": "60" } });

  const out = new Headers({
    "content-type": k.type,
    "content-disposition": `attachment; filename="${k.name(rel.version)}"`,
    "cache-control": "no-store",
    "x-content-type-options": "nosniff",
    "accept-ranges": "bytes",
  });
  for (const h of ["content-length", "content-range"]) {
    const v = up.headers.get(h);
    if (v) out.set(h, v);
  }
  return new Response(request.method === "HEAD" ? null : up.body, { status: up.status, headers: out });
}
