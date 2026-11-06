(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  var lang = root.lang === "en" ? "en" : "de";
  var de = function () { return lang === "de"; };
  var t = function (d, e) { return de() ? d : e; };

  // ---- Sprache -------------------------------------------------------------
  var texts = $$("[data-en]").map(function (el) { return [el, el.textContent]; });
  var alts = $$("[data-en-alt]").map(function (el) { return [el, el.alt]; });
  var labels = $$("[data-en-label]").map(function (el) { return [el, el.getAttribute("aria-label")]; });
  var desc = {
    de: "MariaDB-Server, SQL-Editor und KI-Anbindung in einer App. Für Windows, macOS und Linux, mit einem Befehl installiert.",
    en: "MariaDB server, SQL editor and AI bridge in one app. For Windows, macOS and Linux, installed with one command.",
  };
  var rel = null, langBtn = $("#lang");

  function applyLang(l) {
    lang = l;
    root.lang = l;
    texts.forEach(function (p) { p[0].textContent = l === "en" ? p[0].getAttribute("data-en") : p[1]; });
    alts.forEach(function (p) { p[0].alt = l === "en" ? p[0].getAttribute("data-en-alt") : p[1]; });
    labels.forEach(function (p) { p[0].setAttribute("aria-label", l === "en" ? p[0].getAttribute("data-en-label") : p[1]); });
    var other = l === "en" ? "de" : "en";
    langBtn.textContent = other.toUpperCase();
    langBtn.lang = other;
    langBtn.setAttribute("aria-label", other === "en" ? "English" : "Deutsch");
    $('meta[name="description"]').content = desc[l];
    if (rel) render(rel);
  }
  langBtn.addEventListener("click", function () {
    var l = de() ? "en" : "de";
    try { localStorage.setItem("em-lang", l); } catch (e) {}
    applyLang(l);
    term.relang();
  });
  if (lang === "en") applyLang("en");

  // ---- Neueste Version -----------------------------------------------------
  function mb(n) {
    var v = n / 1048576;
    return (v >= 100 ? Math.round(v) : v.toFixed(1)).toString().replace(".", de() ? "," : ".") + "\u00a0MB";
  }
  function render(r) {
    var f = r.files || {};
    $("#spec-ver").textContent = r.version;
    if (r.mariadb) $("#spec-db").textContent = "MariaDB " + r.mariadb;
    $$("[data-dl]").forEach(function (a) {
      var x = f[a.dataset.dl];
      var ok = !!(x && x.url && x.sha256);
      if (ok) a.href = x.url;
      else a.removeAttribute("href");
      a.tabIndex = ok ? 0 : -1;
      a.setAttribute("aria-disabled", ok ? "false" : "true");
      if (ok && x.size) a.title = x.name + ", " + mb(x.size);
      else if (!ok) a.title = t("Kommt mit dem nächsten Release", "Arrives with the next release");
    });
  }
  try { var c = JSON.parse(localStorage.getItem("em-rel") || "null"); if (c && c.version) { rel = c; render(c); } } catch (e) {}
  fetch("/api/release", { headers: { accept: "application/json" } })
    .then(function (r) { if (!r.ok) throw 0; return r.json(); })
    .then(function (r) {
      rel = r; render(r);
      try { localStorage.setItem("em-rel", JSON.stringify(r)); } catch (e) {}
    })
    .catch(function () {});

  // ---- Kopieren ------------------------------------------------------------
  var toast;
  function say(msg) {
    if (!toast) { toast = document.createElement("div"); toast.className = "toast"; toast.setAttribute("role", "status"); document.body.appendChild(toast); }
    toast.textContent = msg;
    toast.classList.add("on");
    clearTimeout(toast.t);
    toast.t = setTimeout(function () { toast.classList.remove("on"); }, 2000);
  }
  $$("[data-copy-from]").forEach(function (b) {
    b.addEventListener("click", function () {
      var text = $(b.getAttribute("data-copy-from")).textContent.trim();
      var done = function () {
        b.classList.add("done");
        setTimeout(function () { b.classList.remove("done"); }, 1600);
        say(t("Kopiert: ", "Copied: ") + text);
      };
      if (navigator.clipboard) navigator.clipboard.writeText(text).then(done, function () { prompt("", text); });
      else prompt("", text);
    });
  });

  // ---- Funktionen: Zeile waehlen -------------------------------------------
  var rows = $$(".row"), shots = $$(".shot");
  function pick(row, focus) {
    rows.forEach(function (r) { var on = r === row; r.setAttribute("aria-selected", on); r.tabIndex = on ? 0 : -1; });
    shots.forEach(function (s) { s.classList.toggle("on", s.dataset.shot === row.dataset.shot); });
    if (focus) row.focus();
  }
  rows.forEach(function (r, i) {
    r.tabIndex = i === 0 ? 0 : -1;
    r.addEventListener("click", function () { pick(r); });
    r.addEventListener("keydown", function (e) {
      var n = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
      if (!n) return;
      e.preventDefault();
      pick(rows[(i + n + rows.length) % rows.length], true);
    });
  });

  // ---- Demo-Datenbank ------------------------------------------------------
  var DB = {
    customer: {
      cols: [["id", "int(10) unsigned", "NO", "PRI", "auto_increment"], ["name", "varchar(80)", "NO", "", ""], ["city", "varchar(60)", "YES", "", ""], ["joined", "date", "NO", "", ""]],
      rows: [[1, "Ada Lovelace", "London", "2025-02-11"], [2, "Linus Berg", "Innsbruck", "2025-04-03"], [3, "Grace Hopper", "Graz", "2025-06-19"], [4, "Alan Kurz", "Vienna", "2025-09-30"], [5, "Hedy Lamarr", "Salzburg", "2026-01-08"], [6, "Ken Thompson", "Munich", "2026-03-22"]],
    },
    product: {
      cols: [["id", "int(10) unsigned", "NO", "PRI", "auto_increment"], ["name", "varchar(80)", "NO", "", ""], ["price", "decimal(10,2)", "NO", "", ""], ["stock", "int(11)", "NO", "", ""]],
      rows: [[1, "Aero 16 Pro", "1899.00", 12], [2, "Aero 14 Laptop", "1299.99", 31], [3, "Slate 13", "999.00", 24], [4, "Pixel Phone X", "799.50", 40], [5, "Curve 34", "649.50", 18], [6, "Controller Pro", "69.90", 10], [7, "Gaming Headset", "89.00", 17], [8, "Smart Bulb Set", "39.95", 15]],
    },
    orders: {
      cols: [["id", "int(10) unsigned", "NO", "PRI", "auto_increment"], ["customer_id", "int(10) unsigned", "NO", "MUL", ""], ["total", "decimal(10,2)", "NO", "", ""], ["ordered_at", "date", "NO", "", ""]],
      rows: [[1, 1, "1299.99", "2026-03-02"], [2, 3, "860.25", "2026-03-14"], [3, 2, "912.00", "2026-04-01"], [4, 1, "-15.49", "2026-04-02"], [5, 5, "129.85", "2026-05-17"], [6, 4, "649.50", "2026-06-09"], [7, 6, "69.90", "2026-07-21"], [8, 2, "39.95", "2026-08-30"]],
    },
  };

  // ASCII-Tabelle wie im MariaDB-Client
  function table(cols, data) {
    var w = cols.map(function (c, i) {
      return Math.max(String(c).length, data.reduce(function (m, r) { return Math.max(m, r[i] == null ? 4 : String(r[i]).length); }, 0));
    });
    var num = cols.map(function (c, i) { return data.length > 0 && data.every(function (r) { return r[i] == null || /^-?\d+(\.\d+)?$/.test(String(r[i])); }); });
    var line = "+" + w.map(function (n) { return new Array(n + 3).join("-"); }).join("+") + "+";
    var fmt = function (r, head) {
      return "| " + r.map(function (v, i) {
        var s = v == null ? "NULL" : String(v), pad = new Array(w[i] - s.length + 1).join(" ");
        return !head && num[i] ? pad + s : s + pad;
      }).join(" | ") + " |";
    };
    return [line, fmt(cols, true), line].concat(data.map(function (r) { return fmt(r); }), [line]).join("\n");
  }
  function inSet(n, ms) { return n === 0 ? "Empty set (0.00 sec)" : n + (n === 1 ? " row" : " rows") + " in set (0.0" + (ms || 0) + " sec)"; }
  function err(code, state, msg) { return { err: "ERROR " + code + " (" + state + "): " + msg }; }

  var dbName = "shop";
  function run(sql) {
    var s = sql.trim().replace(/;+\s*$/, "").trim();
    var m;
    if (!s) return { text: "" };
    if (/^(help|\\h|\?)$/i.test(s)) return { text: t(
      "Diese Demo kennt:\n  SHOW DATABASES; SHOW TABLES; DESCRIBE product;\n  SELECT * FROM customer; SELECT name, price FROM product WHERE price > 500 ORDER BY price DESC LIMIT 3;\n  SELECT COUNT(*) FROM orders; SELECT VERSION(); clear; exit\nDie echte Konsole kann alles: mysql -u root, sobald EasyMySQL installiert ist.",
      "This demo knows:\n  SHOW DATABASES; SHOW TABLES; DESCRIBE product;\n  SELECT * FROM customer; SELECT name, price FROM product WHERE price > 500 ORDER BY price DESC LIMIT 3;\n  SELECT COUNT(*) FROM orders; SELECT VERSION(); clear; exit\nThe real console does everything: mysql -u root, once EasyMySQL is installed.") };
    if (/^(exit|quit|\\q)$/i.test(s)) return { text: "Bye", exit: true };
    if (/^(clear|cls|\\c)$/i.test(s)) return { clear: true };
    if (/^show\s+databases$/i.test(s)) return { text: table(["Database"], [["information_schema"], ["mysql"], ["shop"]]) + "\n" + inSet(3) };
    if ((m = /^use\s+`?(\w+)`?$/i.exec(s))) {
      if (!/^(shop|mysql|information_schema)$/i.test(m[1])) return err(1049, "42000", "Unknown database '" + m[1] + "'");
      dbName = m[1].toLowerCase();
      return { text: "Database changed", db: dbName };
    }
    var tab = function (name) {
      var k = name.replace(/`/g, "").toLowerCase().replace(/^shop\./, "");
      return dbName === "shop" ? DB[k] : null;
    };
    if (/^show\s+tables$/i.test(s)) {
      var names = dbName === "shop" ? Object.keys(DB).map(function (k) { return [k]; }) : [];
      return { text: names.length ? table(["Tables_in_" + dbName], names) + "\n" + inSet(names.length) : inSet(0) };
    }
    if ((m = /^(?:describe|desc|show\s+columns\s+from)\s+([`\w.]+)$/i.exec(s))) {
      var d = tab(m[1]);
      if (!d) return err(1146, "42S02", "Table '" + dbName + "." + m[1] + "' doesn't exist");
      return { text: table(["Field", "Type", "Null", "Key", "Extra"], d.cols) + "\n" + inSet(d.cols.length, 1) };
    }
    if (/^select\s+version\(\)$/i.test(s)) return { text: table(["VERSION()"], [[(rel && rel.mariadb ? rel.mariadb : "12.3.3") + "-MariaDB"]]) + "\n" + inSet(1) };
    if (/^select\s+(now|current_timestamp)\(\)$/i.test(s)) {
      var now = new Date(), p = function (n) { return (n < 10 ? "0" : "") + n; };
      var v = now.getFullYear() + "-" + p(now.getMonth() + 1) + "-" + p(now.getDate()) + " " + p(now.getHours()) + ":" + p(now.getMinutes()) + ":" + p(now.getSeconds());
      return { text: table([/now/i.test(s) ? "NOW()" : "CURRENT_TIMESTAMP()"], [[v]]) + "\n" + inSet(1) };
    }
    m = /^select\s+(.+?)\s+from\s+([`\w.]+)(?:\s+where\s+(\w+)\s*(=|<>|!=|>=|<=|>|<|like)\s*('[^']*'|-?[\d.]+))?(?:\s+order\s+by\s+(\w+)(?:\s+(asc|desc))?)?(?:\s+limit\s+(\d+))?$/i.exec(s);
    if (m) {
      var tb = tab(m[2]);
      if (!tb) return err(1146, "42S02", "Table '" + dbName + "." + m[2].replace(/`/g, "") + "' doesn't exist");
      var names2 = tb.cols.map(function (c) { return c[0]; });
      var idx = function (c) { return names2.indexOf(c.replace(/`/g, "").toLowerCase()); };
      var data = tb.rows.slice();
      if (m[3]) {
        var ci = idx(m[3]);
        if (ci < 0) return err(1054, "42S22", "Unknown column '" + m[3] + "' in 'WHERE'");
        var raw = m[5], isStr = raw[0] === "'", val = isStr ? raw.slice(1, -1) : parseFloat(raw), op = m[4].toLowerCase();
        data = data.filter(function (r) {
          var x = isStr ? String(r[ci]) : parseFloat(r[ci]);
          if (op === "like") { var re = new RegExp("^" + String(val).replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/%/g, ".*").replace(/_/g, ".") + "$", "i"); return re.test(String(r[ci])); }
          if (isStr) { x = x.toLowerCase(); val = String(val).toLowerCase(); }
          return op === "=" ? x == val : op === "<>" || op === "!=" ? x != val : op === ">" ? x > val : op === "<" ? x < val : op === ">=" ? x >= val : x <= val;
        });
      }
      if (/^count\(\s*\*\s*\)$/i.test(m[1].trim())) return { text: table(["COUNT(*)"], [[data.length]]) + "\n" + inSet(1) };
      if (m[6]) {
        var oi = idx(m[6]);
        if (oi < 0) return err(1054, "42S22", "Unknown column '" + m[6] + "' in 'ORDER BY'");
        var dir = (m[7] || "asc").toLowerCase() === "desc" ? -1 : 1;
        data.sort(function (a, b) { var x = a[oi], y = b[oi], nx = parseFloat(x), ny = parseFloat(y); if (!isNaN(nx) && !isNaN(ny)) { x = nx; y = ny; } return (x > y ? 1 : x < y ? -1 : 0) * dir; });
      }
      if (m[8]) data = data.slice(0, +m[8]);
      var want = m[1].trim() === "*" ? names2 : m[1].split(",").map(function (c) { return c.trim(); });
      for (var i = 0; i < want.length; i++) {
        if (idx(want[i]) < 0) return err(1054, "42S22", "Unknown column '" + want[i] + "' in 'SELECT'");
      }
      var out = data.map(function (r) { return want.map(function (c) { return r[idx(c)]; }); });
      return { text: out.length ? table(want.map(function (c) { return c.replace(/`/g, ""); }), out) + "\n" + inSet(out.length) : inSet(0) };
    }
    if (/^(insert|update|delete|drop|create|alter|truncate|replace)\b/i.test(s))
      return err(1290, "HY000", t("Die Demo auf der Website ist nur lesend. In EasyMySQL geht das natürlich.", "The demo on the website is read-only. In EasyMySQL this works, of course."));
    return err(1064, "42000", t("Das kennt diese Demo nicht. Tippe help für Beispiele.", "This demo does not know that. Type help for examples."));
  }

  // ---- Terminal ------------------------------------------------------------
  var OS = {
    win: { ps: "PS>", prompt: "PS C:\\Users\\du>", promptEn: "PS C:\\Users\\you>", cmd: "irm https://mysql.benjaminberger.at/install | iex", sys: "Windows 11, 64-bit (build 26100)", mode: "installer, for all users", file: "EasyMySQL-Setup-", ext: ".exe", size: 73.2, dir: "C:\\Program Files\\EasyMySQL", extra: "MariaDB, Start menu entry, PATH (easymysql, mysql)", client: "mysql -u root" },
    mac: { ps: "%", prompt: "du@mac ~ %", promptEn: "you@mac ~ %", cmd: "curl -fsSL https://mysql.benjaminberger.at/install | sh", sys: "macOS 15.4, arm64 (universal app: Apple Silicon and Intel)", file: "EasyMySQL-macos-universal-", ext: ".tar.gz", size: 21.4, dir: "/Applications/EasyMySQL.app", extra: "Command: ~/.local/bin/easymysql", mariadb: "MariaDB found: /opt/homebrew/bin/mariadbd", client: "mariadb -u root" },
    linux: { ps: "$", prompt: "du@linux:~$", promptEn: "you@linux:~$", cmd: "curl -fsSL https://mysql.benjaminberger.at/install | sh", sys: "Ubuntu 24.04 LTS, x86_64", file: "EasyMySQL-linux-x64-", ext: ".tar.gz", size: 19.8, dir: "~/.local/share/easymysql-app/easymysql", extra: "Menu entry: ~/.local/share/applications/easymysql.desktop", mariadb: "MariaDB found: /usr/sbin/mariadbd", client: "mariadb -u root" },
  };

  var term = (function () {
    var screen = $("#screen"), out = $("#out"), form = $("#sql"), input = $("#sql-in"), promptEl = $("#prompt");
    var os = "win", token = 0, history = [], hpos = 0;

    function atBottom() { return screen.scrollHeight - screen.scrollTop - screen.clientHeight < 24; }
    function stick(fn) {
      var b = atBottom();
      fn();
      if (b) screen.scrollTop = screen.scrollHeight;
    }
    // Zeile aus Teilen [Klasse, Text]
    function line(parts) {
      var el = document.createElement("span");
      el.className = "ln";
      (typeof parts === "string" ? [["", parts]] : parts).forEach(function (p) {
        var s = document.createElement("span");
        if (p[0]) s.className = p[0];
        s.textContent = p[1];
        el.appendChild(s);
      });
      el.appendChild(document.createTextNode("\n"));
      stick(function () { out.appendChild(el); });
      return el;
    }
    function sleep(ms, tk) { return new Promise(function (ok, no) { setTimeout(function () { tk === token ? ok() : no(); }, still ? 0 : ms); }); }
    function type(promptText, text, tk) {
      var el = line([["w", promptText + " "], ["c", ""], ["cur", ""]]);
      var c = el.children[1];
      if (still) { c.textContent = text; el.removeChild(el.children[2]); return Promise.resolve(); }
      var i = 0;
      return new Promise(function (ok, no) {
        (function next() {
          if (tk !== token) return no();
          if (i >= text.length) { el.removeChild(el.children[2]); return ok(); }
          c.textContent += text[i++];
          setTimeout(next, 18 + Math.random() * 38);
        })();
      });
    }
    function version() { return rel && rel.version ? rel.version : "7.0.0"; }
    function me() { return de() ? OS[os].prompt : OS[os].promptEn; }

    async function play() {
      var tk = ++token, o = OS[os];
      out.textContent = "";
      form.hidden = true;
      dbName = "shop";
      try {
        await sleep(500, tk);
        await type(me(), o.cmd, tk);
        await sleep(300, tk);
        line(""); line([["a", "     .-~~~~~~-.   "], ["w", "EasyMySQL"]]);
        line([["a", "    (~~~~~~~~~~)  "], ["", "MariaDB server and database manager"]]);
        line([["a", "    |~~~~~~~~~~|  "], ["d", "SQL editor, ER diagrams, backups, AI assistants"]]);
        line([["a", "    (__________)  "], ["d", "mysql.benjaminberger.at"]]);
        var steps = [
          ["Checking this " + (os === "win" ? "PC" : "computer"), [["g", "+ "], ["", o.sys]]],
          ["Finding the newest version", [["g", "+ "], ["", "EasyMySQL " + version() + (rel && rel.mariadb && os === "win" ? ", MariaDB " + rel.mariadb : "")]]],
        ];
        for (var i = 0; i < steps.length; i++) {
          await sleep(380, tk);
          line(""); line([["d", "  " + (i + 1) + "/5  "], ["w", steps[i][0]]]);
          await sleep(260, tk);
          line([["", "        "]].concat(steps[i][1]));
        }
        await sleep(300, tk);
        line(""); line([["d", "  3/5  "], ["w", "Downloading"]]);
        var bar = line([["y", ""]]), total = o.size;
        for (var p = 0; p <= 28; p += still ? 28 : 2) {
          var done = (total * p / 28).toFixed(1);
          bar.children[0].textContent = "        [" + new Array(p + 1).join("#") + new Array(29 - p).join("-") + "] " + String(Math.round(p / 28 * 100)).padStart(3) + "%  " + done + " / " + total.toFixed(1) + " MB";
          await sleep(70, tk);
        }
        line(""); line([["d", "  4/5  "], ["w", "Verifying the SHA-256 checksum"]]);
        await sleep(350, tk);
        var f = rel && rel.files && rel.files[os === "win" ? "setup" : os === "mac" ? "macos" : "linux"];
        line([["", "        "], ["g", "+ "], ["m", f && f.sha256 ? f.sha256 : "4f1c9e0b7a6d52e83c1f0a9b8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b"]]);
        await sleep(300, tk);
        line(""); line([["d", "  5/5  "], ["w", "Installing"]]);
        if (os === "win") { await sleep(300, tk); line([["", "        "], ["d", ". "], ["m", "Windows asks for administrator rights now ..."]]); }
        await sleep(700, tk);
        line([["", "        "], ["g", "+ "], ["", (os === "linux" ? "Program: " : os === "mac" ? "App: " : "Installed to ") + o.dir]]);
        line([["", "        "], ["g", "+ "], ["", o.extra]]);
        if (o.mariadb) { await sleep(250, tk); line(""); line([["", "        "], ["g", "+ "], ["", o.mariadb]]); }
        await sleep(350, tk);
        line(""); line([["", "        "], ["d", ". "], ["m", "Found: Claude Code, Codex"]]);
        line([["", "        "], ["a", "? "], ["", "Connect EasyMySQL to your AI assistants (read-only until you allow changes)? [Y/n] "], ["w", "y"]]);
        await sleep(450, tk);
        line([["", "  "], ["g", "+ "], ["", "Claude Code  MCP server (user scope), skill in ~/.claude/skills/easymysql"]]);
        line([["", "  "], ["g", "+ "], ["", "Codex        MCP server in ~/.codex/config.toml, skill in ~/.agents/skills/easymysql"]]);
        await sleep(400, tk);
        line(""); line([["g", "  Done. "], ["w", "EasyMySQL " + version() + " is installed."]]);
        line("");
        await sleep(900, tk);
        await type(me(), o.client, tk);
        line("Welcome to the MariaDB monitor.  Commands end with ; or \\g.");
        line("Your MariaDB connection id is 3");
        line("Server version: " + (rel && rel.mariadb ? rel.mariadb : "12.3.3") + "-MariaDB EasyMySQL");
        line("");
        line([["m", "Type 'help;' or '\\h' for help. Type '\\c' to clear the current input statement."]]);
        line("");
        await sleep(500, tk);
        await type("MariaDB [(none)]>", "USE shop;", tk);
        line("Database changed");
        await sleep(400, tk);
        var q = "SELECT name, price FROM product WHERE price > 500 ORDER BY price DESC LIMIT 4;";
        await type("MariaDB [shop]>", q, tk);
        line(run(q).text);
        line("");
        promptEl.textContent = "MariaDB [" + dbName + "]>";
        stick(function () { form.hidden = false; });
      } catch (e) { /* neu gestartet */ }
    }

    form.addEventListener("submit", function (e) {
      e.preventDefault();
      var v = input.value;
      input.value = "";
      if (v.trim()) { history.push(v); hpos = history.length; }
      line([["w", "MariaDB [" + dbName + "]> "], ["c", v]]);
      var r = run(v);
      if (r.clear) { out.textContent = ""; return; }
      if (r.err) line([["e", r.err]]);
      else if (r.text) line(r.text);
      if (r.exit) {
        form.hidden = true;
        line([["d", t("Zum Neustarten: Nochmal abspielen.", "To start again: Replay.")]]);
        return;
      }
      line("");
      promptEl.textContent = "MariaDB [" + dbName + "]>";
      screen.scrollTop = screen.scrollHeight;
    });
    input.addEventListener("keydown", function (e) {
      if (e.key === "ArrowUp" && hpos > 0) { input.value = history[--hpos]; e.preventDefault(); }
      else if (e.key === "ArrowDown") { hpos = Math.min(history.length, hpos + 1); input.value = history[hpos] || ""; e.preventDefault(); }
    });
    // Klick ins Terminal setzt den Cursor in die Eingabe (ohne Markieren zu stoeren)
    screen.addEventListener("click", function () {
      if (!form.hidden && !String(getSelection())) input.focus({ preventScroll: true });
    });

    function setOs(next) {
      os = next;
      $$(".os button").forEach(function (b) { b.setAttribute("aria-selected", b.dataset.os === os); b.tabIndex = b.dataset.os === os ? 0 : -1; });
      $("#hero-ps").textContent = OS[os].ps;
      $("#hero-cmd").textContent = OS[os].cmd;
      play();
    }
    var tabs = $$(".os button");
    tabs.forEach(function (b, i) {
      b.tabIndex = i === 0 ? 0 : -1;
      b.addEventListener("click", function () { if (b.dataset.os !== os) setOs(b.dataset.os); });
      b.addEventListener("keydown", function (e) {
        var n = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
        if (!n) return;
        e.preventDefault();
        var nb = tabs[(i + n + tabs.length) % tabs.length];
        nb.focus();
        setOs(nb.dataset.os);
      });
    });
    $("#replay").addEventListener("click", play);

    // Passendes System vorwaehlen
    var ua = navigator.userAgent;
    var guess = /Mac OS X|Macintosh/.test(ua) && !/iPhone|iPad/.test(ua) ? "mac" : /Linux|X11|CrOS/.test(ua) && !/Android/.test(ua) ? "linux" : "win";
    return { start: function () { setOs(guess); }, relang: function () { /* Ausgabe bleibt englisch wie das echte Skript */ } };
  })();
  term.start();
})();
