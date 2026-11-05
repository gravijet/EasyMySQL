(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  var de = function () { return lang === "de"; };

  // ---- Sprache -------------------------------------------------------------
  var lang = root.lang === "en" ? "en" : "de";
  var texts = $$("[data-en]").map(function (el) { return [el, el.textContent]; });
  var alts = $$("[data-en-alt]").map(function (el) { return [el, el.alt]; });
  var labels = $$("[data-en-label]").map(function (el) { return [el, el.getAttribute("aria-label")]; });
  var desc = {
    de: "MariaDB-Server und Datenbank-Werkzeug für Windows: SQL-Editor, Tabellen, Diagramme, Import und Export, Java, Python und JavaScript.",
    en: "MariaDB server and database tool for Windows: SQL editor, tables, diagrams, import and export, Java, Python and JavaScript.",
  };
  var rel = null, demo = null, langBtn = $("#lang");

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
    if (demo) demo.relang();
  }
  langBtn.addEventListener("click", function () {
    var l = de() ? "en" : "de";
    try { localStorage.setItem("em-lang", l); } catch (e) {}
    applyLang(l);
  });

  // ---- Neueste Version -----------------------------------------------------
  function mb(n) {
    var v = n / 1048576;
    return (v >= 100 ? Math.round(v) : v.toFixed(1)).toString().replace(".", de() ? "," : ".") + "\u00a0MB";
  }
  function render(r) {
    var f = r.files || {}, s = f.setup;
    var parts = ["Version " + r.version];
    if (r.published) {
      var d = new Date(r.published);
      if (!isNaN(d)) parts.push(d.toLocaleDateString(de() ? "de-AT" : "en-GB", { day: "numeric", month: "long", year: "numeric" }));
    }
    if (s && s.size) parts.push(mb(s.size));
    $("#dl-info").textContent = parts.join(" · ");
    $$("[data-ver]").forEach(function (el) { el.textContent = "Version " + r.version + " · Windows 10 & 11"; });
    if (r.mariadb) $("#spec-db").textContent = "MariaDB " + r.mariadb + (de() ? ", mitinstalliert" : ", included");
    $$("[data-dl]").forEach(function (a) { var x = f[a.dataset.dl]; if (x && x.url) a.href = x.url; });
    var sha = $("#sha");
    sha.hidden = !(s && s.sha256);
    sha.dataset.sum = (s && s.sha256) || "";
  }
  try { var c = JSON.parse(localStorage.getItem("em-rel") || "null"); if (c && c.version) { rel = c; render(c); } } catch (e) {}
  fetch("/api/release", { headers: { accept: "application/json" } })
    .then(function (r) { if (!r.ok) throw 0; return r.json(); })
    .then(function (r) {
      rel = r; render(r);
      try { localStorage.setItem("em-rel", JSON.stringify(r)); } catch (e) {}
    })
    .catch(function () {});

  var toast;
  $("#sha").addEventListener("click", function () {
    var sum = this.dataset.sum;
    var done = function () {
      if (!toast) { toast = document.createElement("div"); toast.className = "toast"; toast.setAttribute("role", "status"); document.body.appendChild(toast); }
      toast.textContent = de() ? "SHA-256 kopiert" : "SHA-256 copied";
      toast.classList.add("on");
      clearTimeout(toast.t);
      toast.t = setTimeout(function () { toast.classList.remove("on"); }, 2200);
    };
    if (navigator.clipboard) navigator.clipboard.writeText(sum).then(done, function () { prompt("SHA-256", sum); });
    else prompt("SHA-256", sum);
  });

  // ---- Sichtbarkeit --------------------------------------------------------
  function watch(el, fn, opts) {
    if (!("IntersectionObserver" in window)) { fn(true); return; }
    new IntersectionObserver(function (es) { fn(es[es.length - 1].isIntersecting); }, opts).observe(el);
  }
  $$("[data-reveal]").forEach(function (el) {
    $$("tbody tr", el).forEach(function (tr, i) { tr.style.setProperty("--i", i); });
    watch(el, function (v) { if (v) el.classList.add("in"); }, { rootMargin: "0px 0px -10% 0px" });
  });

  // ---- Kopfzeile und Lichtfleck --------------------------------------------
  var nav = $(".nav"), hero = $(".hero"), ticking = false;
  function onScroll() {
    ticking = false;
    nav.classList.toggle("on-paper", hero.getBoundingClientRect().bottom < 70);
  }
  addEventListener("scroll", function () { if (!ticking) { ticking = true; requestAnimationFrame(onScroll); } }, { passive: true });
  onScroll();
  if (!still && matchMedia("(pointer: fine)").matches) {
    hero.addEventListener("pointermove", function (e) {
      var r = hero.getBoundingClientRect();
      hero.style.setProperty("--gx", (e.clientX - r.left) + "px");
      hero.style.setProperty("--gy", (e.clientY - r.top) + "px");
    }, { passive: true });
  }

  // ---- Konsole: tippt Abfragen wie im Editor --------------------------------
  var KW = /^(SELECT|FROM|WHERE|ORDER|GROUP|BY|DESC|ASC|LIMIT|AS|AND|OR)$/i, FN = /^(COUNT|SUM|AVG|MIN|MAX)$/i;
  var esc = function (s) { return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"); };
  var RUN = function () { return de() ? "Strg + Enter" : "Ctrl + Enter"; };
  var SCENES = [
    { // Vervollstaendigen
      steps: [["type", "SELECT name, price\nFROM pro"], ["pop", [["product", ""], ["product_sales", "v"]]], ["wait", 1100], ["key", "Tab"], ["swap", "pro", "product"],
        ["type", "\nORDER BY price DESC\nLIMIT 5;"], ["wait", 450], ["key", RUN], ["result"]],
      cols: ["name", "price"], num: [0, 1], ms: 2,
      rows: [["Aero 16 Pro", "1899.00"], ["Aero 14 Laptop", "1299.99"], ["Slate 13", "999.00"], ["Pixel Phone X", "799.50"], ["Curve 34", "649.50"]],
    },
    { // Ausfuehren, mit Balken
      steps: [["type", "SELECT city, COUNT(*) AS customers\nFROM customer\nGROUP BY city\nORDER BY customers DESC\nLIMIT 5;"], ["wait", 450], ["key", RUN], ["result"]],
      cols: ["city", "customers"], num: [0, 1], bar: 1, ms: 3,
      rows: [["Munich", "8"], ["Salzburg", "7"], ["Graz", "6"], ["Vienna", "6"], ["Berlin", "4"]],
    },
    { // Beheben
      steps: [["type", "SELECT name, stock\nFROM prodcut\nWHERE stock < 20;"], ["wait", 500], ["mark", "prodcut"], ["wait", 450], ["tip"], ["wait", 2000],
        ["key", function () { return de() ? "Strg + ." : "Ctrl + ."; }], ["swap", "prodcut", "product"], ["wait", 650], ["key", RUN], ["result"]],
      tip: function () { return de() ? "Die Tabelle „prodcut“ gibt es in „shop“ nicht. Meinten Sie „product“?" : "Table “prodcut” does not exist in “shop”. Did you mean “product”?"; },
      cols: ["name", "stock"], num: [0, 1], ms: 2,
      rows: [["Pixel Phone Mini", "15"], ["Controller Pro", "10"], ["Gaming Headset", "17"], ["Smart Bulb Set", "15"]],
    },
  ];

  demo = (function () {
    var code = $("#code"), ln = $("#ln"), res = $("#res"), stat = $("#stat"), press = $("#press");
    var chips = $$(".chips button");
    var doc = "", mark = -1, pop = null, tip = null, idle = true, scene = 0, token = 0, shown = null, visible = true, timer = 0, pending = false;

    function paint() {
      var html = "", re = /([A-Za-z_][A-Za-z0-9_]*)|(\d+(?:\.\d+)?)|(\s+)|([^\sA-Za-z0-9_])/g, m;
      while ((m = re.exec(doc))) {
        var t = m[0], cls = m[1] ? (KW.test(t) ? "k" : FN.test(t) ? "f" : "i") : m[2] ? "n" : m[4] ? "o" : "";
        var part = cls ? '<span class="' + cls + '">' + esc(t) + "</span>" : esc(t);
        if (m.index === mark) part = '<span class="bad">' + part + (tip ? '<span class="tip">' + esc(tip) + "</span>" : "") + "</span>";
        html += part;
      }
      var p = pop ? '<span class="pop">' + pop.map(function (o) { return '<span class="' + o[1] + '">' + esc(o[0]) + "<em>" + (o[1] ? "view" : "table") + "</em></span>"; }).join("") + "</span>" : "";
      code.innerHTML = html + '<span class="car' + (idle ? " idle" : "") + '">' + p + "</span>";
      var n = doc.split("\n").length, l = [];
      for (var i = 1; i <= n; i++) l.push(i);
      ln.textContent = l.join("\n");
    }

    function table(s) {
      res.textContent = "";
      var head = res.createTHead().insertRow();
      head.appendChild(document.createElement("th"));
      s.cols.forEach(function (c, k) { var th = document.createElement("th"); th.textContent = c; if (s.num[k]) th.className = "num"; head.appendChild(th); });
      var body = res.createTBody(), max = 0;
      if (s.bar != null) s.rows.forEach(function (r) { max = Math.max(max, +r[s.bar]); });
      s.rows.forEach(function (r, i) {
        var tr = body.insertRow();
        tr.style.setProperty("--i", i);
        tr.insertCell().textContent = i + 1;
        r.forEach(function (v, k) {
          var td = tr.insertCell();
          if (s.num[k]) td.className = "num";
          if (k === s.bar) {
            td.className += " bar";
            var bar = document.createElement("i"), b = document.createElement("b");
            bar.style.setProperty("--w", +v / max);
            b.textContent = v;
            td.appendChild(bar); td.appendChild(b);
          } else td.textContent = v;
        });
      });
      shown = s;
      statLine();
    }
    function statLine() {
      stat.textContent = "";
      if (!shown) return;
      var a = document.createElement("span"), b = document.createElement("span");
      a.textContent = shown.rows.length + (de() ? " Zeilen" : " rows");
      b.textContent = shown.ms + " ms";
      stat.appendChild(a); stat.appendChild(b);
    }

    function sleep(ms, t) {
      return new Promise(function (ok, no) { setTimeout(function () { t === token ? ok() : no(); }, ms); });
    }
    function key(label) {
      press.textContent = typeof label === "function" ? label() : label;
      press.classList.remove("on");
      void press.offsetWidth;
      press.classList.add("on");
    }

    function play(i, instant) {
      var t = ++token, s = SCENES[i];
      clearTimeout(timer);
      scene = i;
      chips.forEach(function (c, k) { c.setAttribute("aria-pressed", String(k === i)); });
      doc = ""; mark = -1; pop = null; tip = null; idle = false; shown = null;
      res.textContent = ""; statLine();
      var chain = Promise.resolve();
      s.steps.forEach(function (st) {
        chain = chain.then(function () {
          var kind = st[0];
          if (kind === "type") {
            if (instant) { doc += st[1]; return; }
            var text = st[1], k = 0;
            var next = function () {
              if (k >= text.length) return;
              var ch = text[k++];
              doc += ch; paint();
              return sleep(28 + Math.random() * 46 + (ch === " " || ch === "\n" ? 60 : 0), t).then(next);
            };
            return next();
          }
          if (kind === "swap") {
            var at = doc.lastIndexOf(st[1]);
            doc = doc.slice(0, at) + st[2] + doc.slice(at + st[1].length);
            pop = null; mark = -1; tip = null;
            if (!instant) paint();
            return;
          }
          if (kind === "result") { if (instant) table(s); else return sleep(220, t).then(function () { table(s); }); return; }
          if (instant) return;
          if (kind === "wait") return sleep(st[1], t);
          if (kind === "key") key(st[1]);
          else if (kind === "pop") { pop = st[1]; paint(); }
          else if (kind === "mark") { mark = doc.indexOf(st[1]); paint(); }
          else if (kind === "tip") { tip = s.tip(); paint(); }
        });
      });
      chain.then(function () {
        if (t !== token) return;
        idle = true; paint();
        if (!still) queue();
      }, function () {});
    }
    // Naechste Szene erst, wenn die Konsole zu sehen ist
    function queue() {
      timer = setTimeout(function () {
        if (visible) play((scene + 1) % SCENES.length); else pending = true;
      }, 4200);
    }

    chips.forEach(function (c, k) { c.addEventListener("click", function () { play(k, still); }); });
    watch($(".con"), function (v) {
      visible = v;
      if (v && pending) { pending = false; play((scene + 1) % SCENES.length); }
    }, { threshold: .35 });
    play(0, still);

    return {
      // Sprache gewechselt: Hinweis und Statuszeile neu
      relang: function () {
        if (tip) tip = SCENES[scene].tip();
        paint(); statLine();
      },
    };
  })();

  // ---- Kapitel -------------------------------------------------------------
  var tour = $(".tour"), items = $$(".tour .it"), shots = $$(".shot"), cur = 0;
  function pick(i, focus) {
    cur = i;
    items.forEach(function (b, k) {
      b.classList.toggle("on", k === i);
      b.setAttribute("aria-selected", String(k === i));
      b.tabIndex = k === i ? 0 : -1;
    });
    shots.forEach(function (s, k) { s.classList.toggle("on", k === i); });
    if (focus) items[i].focus();
  }
  items.forEach(function (b, i) {
    b.addEventListener("click", function () { if (i !== cur) pick(i); });
    $("i", b).addEventListener("animationend", function () { if (i === cur) pick((i + 1) % items.length); });
  });
  $(".list").addEventListener("keydown", function (e) {
    var d = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
    if (!d) return;
    e.preventDefault();
    pick((cur + d + items.length) % items.length, true);
  });
  tour.classList.add("idle");
  pick(0);
  watch(tour, function (v) { tour.classList.toggle("idle", !v); }, { threshold: .3 });

  if (lang === "en") applyLang("en");
})();
