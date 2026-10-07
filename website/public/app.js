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

  applyLang(lang);
})();
