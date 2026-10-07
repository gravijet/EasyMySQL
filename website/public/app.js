(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var reduce = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---- Sprache -------------------------------------------------------------
  var texts = new Map();
  $$("[data-en]").forEach(function (el) { texts.set(el, el.textContent); });
  $$("[data-en-title]").forEach(function (el) { el.setAttribute("data-de-title", el.title); });
  var lang = root.getAttribute("lang") === "en" ? "en" : "de";
  var rel = null;

  function applyLang(l) {
    lang = l;
    root.setAttribute("lang", l);
    texts.forEach(function (de, el) { el.textContent = l === "en" ? el.getAttribute("data-en") : de; });
    $$("[data-en-title]").forEach(function (el) {
      el.title = l === "en" ? el.getAttribute("data-en-title") : el.getAttribute("data-de-title");
    });
    $$(".lang button").forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.lang === l)); });
    document.title = "EasyMySQL";
    var d = document.querySelector('meta[name="description"]');
    if (d) d.content = l === "en"
      ? "EasyMySQL is a Windows program that brings a MariaDB server and manages databases: queries, tables, ER diagrams, backups."
      : "EasyMySQL ist ein Windows-Programm, das einen MariaDB-Server mitbringt und Datenbanken verwaltet: Abfragen, Tabellen, ER-Diagramme, Sicherungen.";
    if (rel) render(rel);
    splitStatement();
  }
  $$(".lang button").forEach(function (b) {
    b.addEventListener("click", function () {
      try { localStorage.setItem("em-lang", b.dataset.lang); } catch (e) {}
      applyLang(b.dataset.lang);
    });
  });

  // ---- Release -------------------------------------------------------------
  function mb(n) {
    if (!n) return "";
    var v = n / 1048576;
    return (v >= 100 ? Math.round(v) : v.toFixed(1)).toString().replace(".", lang === "de" ? "," : ".") + " MB";
  }
  function setText(id, t) { var el = document.getElementById(id); if (el) el.textContent = t; }

  function render(r) {
    var f = r.files || {}, s = f.setup, p = f.portable;
    var loc = lang === "de" ? "de-AT" : "en-GB";
    var meta = $("#dl-meta");
    if (meta) { meta.textContent = r.version + (s && s.size ? " · " + mb(s.size) : ""); meta.hidden = false; }
    setText("f-version", r.version);
    if (r.published) {
      var d = new Date(r.published);
      if (!isNaN(d)) setText("f-date", d.toLocaleDateString(loc, { day: "numeric", month: "long", year: "numeric" }));
    }
    if (r.mariadb) { setText("f-mdb", r.mariadb); $$("[data-needs-mdb]").forEach(function (e) { e.hidden = false; }); }
    [["setup", s], ["portable", p]].forEach(function (x) {
      var k = x[0], a = x[1];
      if (!a) return;
      setText(k + "-name", a.name);
      setText(k + "-size", mb(a.size));
      var h = document.getElementById("h-" + k);
      if (h) { h.textContent = a.sha256 || "–"; h.dataset.full = a.sha256 || ""; }
      var row = document.getElementById("row-" + k);
      if (row && a.url) row.setAttribute("href", a.url);
    });
    if (s && s.url) { var b = $("#dl-main"); if (b) b.setAttribute("href", s.url); }
  }

  try { var c = JSON.parse(localStorage.getItem("em-rel") || "null"); if (c && c.version) { rel = c; render(c); } } catch (e) {}
  fetch("/api/release", { headers: { accept: "application/json" } })
    .then(function (r) { if (!r.ok) throw 0; return r.json(); })
    .then(function (r) {
      rel = r; render(r);
      try { localStorage.setItem("em-rel", JSON.stringify(r)); } catch (e) {}
    })
    .catch(function () {});

  $$(".hash").forEach(function (b) {
    b.addEventListener("click", function () {
      var v = b.dataset.full;
      if (!v || !navigator.clipboard) return;
      navigator.clipboard.writeText(v).then(function () {
        var old = b.textContent;
        b.textContent = lang === "en" ? "Copied" : "Kopiert";
        setTimeout(function () { b.textContent = old; }, 1400);
      }).catch(function () {});
    });
  });

  // ---- Aussage: Wörter leuchten beim Scrollen auf ----------------------------
  var words = [];
  function splitStatement() {
    var p = document.getElementById("statement");
    if (!p) return;
    var text = p.textContent;
    p.textContent = "";
    words = text.split(" ").map(function (w, i, a) {
      var s = document.createElement("span");
      s.className = "w";
      s.textContent = w + (i < a.length - 1 ? " " : "");
      p.appendChild(s);
      return s;
    });
    reveal();
  }
  function reveal() {
    if (!words.length) return;
    var p = document.getElementById("statement");
    var r = p.getBoundingClientRect(), vh = window.innerHeight;
    // 0 wenn der Absatz unten einläuft, 1 wenn seine Mitte etwa im oberen Drittel steht
    var t = (vh * 0.92 - r.top) / (vh * 0.55 + r.height * 0.5);
    var n = reduce ? words.length : Math.round(Math.max(0, Math.min(1, t)) * words.length);
    words.forEach(function (w, i) { w.classList.toggle("on", i < n); });
  }
  var tick = false;
  window.addEventListener("scroll", function () {
    if (tick) return;
    tick = true;
    requestAnimationFrame(function () { tick = false; reveal(); });
  }, { passive: true });
  window.addEventListener("resize", reveal);

  // ---- Einblenden ---------------------------------------------------------------
  var rv = $$(".features li, .more, .rows, .facts, .steps li, .faq details");
  if ("IntersectionObserver" in window && !reduce) {
    rv.forEach(function (e) { e.classList.add("rv"); });
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.05 });
    rv.forEach(function (e) { io.observe(e); });
  }

  applyLang(lang);
})();
