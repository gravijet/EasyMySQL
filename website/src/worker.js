// Liefert die Website aus und stellt immer das neueste Release bereit.
// Besucher sehen nur Adressen dieser Domain: /api/release liefert die Metadaten,
// /download/<art> streamt die Datei durch. Die Quelle wird nie ausgeliefert.

const REPO = "gravijet/EasyMySQL";
const FRESH_SECONDS = 300;
const PARTIAL_SECONDS = 60;
const STALE_SECONDS = 60 * 60 * 24 * 7;

const KINDS = {
  app: { match: /^EasyMySQL-app-[\d.]+\.zip$/, name: (v) => `EasyMySQL-app-${v}.zip`, type: "application/zip" },
  setup: { match: /^EasyMySQL-Setup-[\d.]+\.exe$/, name: (v) => `EasyMySQL-Setup-${v}.exe`, type: "application/vnd.microsoft.portable-executable" },
  portable: { match: /^EasyMySQL-portable-[\d.]+\.zip$/, name: (v) => `EasyMySQL-portable-${v}.zip`, type: "application/zip" },
  macos: { match: /^EasyMySQL-macos-universal-[\d.]+\.tar\.gz$/, name: (v) => `EasyMySQL-macos-universal-${v}.tar.gz`, type: "application/gzip" },
  linux: { match: /^EasyMySQL-linux-x64-[\d.]+\.tar\.gz$/, name: (v) => `EasyMySQL-linux-x64-${v}.tar.gz`, type: "application/gzip" },
};

// Installationsskripte: `irm …/install | iex` (PowerShell) und `curl -fsSL …/install | sh` landen beim passenden
const SCRIPTS = { "/install.ps1": "/install.ps1", "/install.sh": "/install.sh" };

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
      if (url.pathname === "/install" || SCRIPTS[url.pathname]) return await script(request, env, url);
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
    // Unvollstaendige Angaben (Notweg ohne Groesse/Pruefsumme) gelten nur kurz, damit bald wieder die volle Fassung kommt
    const fresh = (isComplete(entry.release) ? FRESH_SECONDS : PARTIAL_SECONDS) * 1000;
    if (Date.now() - entry.at < fresh) return entry.release;
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
  // Der Notweg liefert weniger als die API: dieselbe Version nie mit schlechteren Angaben ueberschreiben
  if (stale && isComplete(stale) && !isComplete(release) && stale.version === release.version) release = stale;
  const body = JSON.stringify({ at: Date.now(), release });
  ctx.waitUntil(cache.put(key, new Response(body, { headers: { "content-type": "application/json", "cache-control": `max-age=${STALE_SECONDS}` } })));
  return release;
}

function ghHeaders(env, accept) {
  const h = { "user-agent": "easymysql-site", accept };
  if (env.GITHUB_TOKEN) h.authorization = `Bearer ${env.GITHUB_TOKEN}`;
  return h;
}

// Vollstaendig: jede Datei hat Groesse und SHA-256 (das braucht das automatische Update im Programm)
function isComplete(rel) {
  return Object.values(rel.files || {}).every((f) => f.size > 0 && /^[0-9a-f]{64}$/.test(f.sha256 || ""));
}

// SHA256SUMS.txt des Releases (vom Release-Workflow angelegt); die Download-Adressen unterliegen nicht dem API-Limit
async function sums(version) {
  try {
    const r = await fetch(`https://github.com/${REPO}/releases/download/v${version}/SHA256SUMS.txt`, { headers: { "user-agent": "easymysql-site" }, redirect: "follow" });
    if (!r.ok) return {};
    const out = {};
    for (const line of (await r.text()).split("\n")) {
      const m = /^([0-9a-fA-F]{64})\s+\*?(\S+)$/.exec(line.trim());
      if (m) out[m[2]] = m[1].toLowerCase();
    }
    return out;
  } catch {
    return {};
  }
}

async function sizeOf(url) {
  try {
    const r = await fetch(url, { method: "HEAD", headers: { "user-agent": "easymysql-site" }, redirect: "follow" });
    const n = Number(r.headers.get("content-length"));
    return r.ok && n > 0 ? n : null;
  } catch {
    return null;
  }
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
  // Aeltere Releases haben keine "digest"-Angabe: dann aus SHA256SUMS.txt
  if (Object.values(files).some((f) => !f.sha256)) {
    const list = await sums(version);
    for (const [kind, f] of Object.entries(files)) f.sha256 ||= list[KINDS[kind].name(version)] ?? null;
  }
  const mariadb = /MariaDB\s+(\d+\.\d+\.\d+)/.exec(d.body || "")?.[1] ?? null;
  return { version, published: d.published_at ?? null, mariadb, files };
}

// Ohne API (z. B. Rate-Limit): Tag aus der Weiterleitung lesen, Dateinamen sind festgelegt.
async function fromRedirect() {
  const r = await fetch(`https://github.com/${REPO}/releases/latest`, { redirect: "manual", headers: { "user-agent": "easymysql-site" } });
  const version = /\/releases\/tag\/v(\d+\.\d+\.\d+)$/.exec(r.headers.get("location") || "")?.[1];
  if (!version) throw new Error("no tag");
  const list = await sums(version);
  const files = {};
  await Promise.all(
    Object.entries(KINDS).map(async ([kind, k]) => {
      const source = `https://github.com/${REPO}/releases/download/v${version}/${k.name(version)}`;
      files[kind] = { source, size: await sizeOf(source), sha256: list[k.name(version)] ?? null };
    }),
  );
  return { version, published: null, mariadb: null, files };
}

// Versionen wurden am 10.11.2026 auf 1.x umbenannt (README "Versioning"). Bereits installierte Programme
// tragen noch die alte Nummer (bis 7.4.0) und halten 1.x fuer aelter. Sie erhalten deshalb die alte Nummer
// bzw. ab 1.1.17 eine hoehere Ersatznummer; neue Programme senden "x-version-scheme: 2" und sehen die echte.
const OLD_NUMBERS = {
  "1.0.2": "1.1.0", "1.0.3": "1.2.0", "1.0.4": "2.0.0", "1.0.5": "3.0.0", "1.0.6": "3.0.1", "1.0.7": "3.1.0",
  "1.0.8": "3.2.0", "1.0.9": "3.3.0", "1.0.10": "3.4.0", "1.0.11": "3.5.0", "1.1.0": "4.0.0", "1.1.1": "4.0.1",
  "1.1.2": "4.0.2", "1.1.3": "4.0.3", "1.1.4": "4.0.5", "1.1.5": "5.0.0", "1.1.6": "5.0.1", "1.1.7": "5.1.1",
  "1.1.8": "5.2.0", "1.1.9": "6.0.0", "1.1.10": "6.1.0", "1.1.11": "6.2.0", "1.1.12": "6.3.0", "1.1.13": "6.3.1",
  "1.1.14": "7.0.1", "1.1.15": "7.1.0", "1.1.16": "7.4.0",
};

function legacyClient(request) {
  return /^EasyMySQL\/\d+\.\d+\.\d+/.test(request.headers.get("user-agent") || "") && request.headers.get("x-version-scheme") !== "2";
}

// Ersatznummer: (Haupt+6).(Neben+4).Patch bleibt geordnet und liegt ab 1.1.0 ueber jeder alten Nummer.
function legacyVersion(version) {
  if (OLD_NUMBERS[version]) return OLD_NUMBERS[version];
  const [major, minor, patch] = version.split(".").map(Number);
  return `${major + 6}.${minor + 4}.${patch}`;
}

async function releaseJson(request, env, ctx) {
  const rel = await getRelease(env, ctx);
  const version = legacyClient(request) && /^1\.\d+\.\d+$/.test(rel.version) && rel.version !== "1.0.0" && rel.version !== "1.0.1" ? legacyVersion(rel.version) : rel.version;
  const files = {};
  for (const [kind, f] of Object.entries(rel.files)) {
    files[kind] = { name: KINDS[kind].name(version), url: `/download/${kind}`, size: f.size, sha256: f.sha256 };
  }
  return new Response(JSON.stringify({ version, published: rel.published, mariadb: rel.mariadb, files }), {
    headers: { ...JSON_HEADERS, vary: "user-agent, x-version-scheme" },
  });
}

// ---------------------------------------------------------------------------
// Installationsskripte als Text ausliefern (PowerShell braucht text/plain, sonst liefert irm Bytes)

async function script(request, env, url) {
  let path = SCRIPTS[url.pathname];
  if (!path) {
    const ua = request.headers.get("user-agent") || "";
    if (/PowerShell/i.test(ua)) path = "/install.ps1";
    else if (/^(curl|Wget|fetch)\b/i.test(ua) || !/Mozilla/i.test(ua)) path = "/install.sh";
    // Im Browser: zur Anleitung auf der Seite
    else return Response.redirect(new URL("/#install", url).toString(), 302);
  }
  const asset = await env.ASSETS.fetch(new Request(new URL(path, url), { method: "GET" }));
  if (!asset.ok) return new Response("Not found", { status: 404 });
  return new Response(request.method === "HEAD" ? null : asset.body, {
    headers: {
      "content-type": "text/plain; charset=utf-8",
      "cache-control": "public, max-age=300",
      "x-content-type-options": "nosniff",
      "access-control-allow-origin": "*",
    },
  });
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
