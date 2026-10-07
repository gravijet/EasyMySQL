(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };

  // ---- Sprache -------------------------------------------------------------
  var texts = new Map(), alts = new Map();
  $$("[data-en]").forEach(function (el) { texts.set(el, el.textContent); });
  $$("[data-en-alt]").forEach(function (el) { alts.set(el, el.getAttribute("alt")); });
  var lang = root.getAttribute("lang") === "en" ? "en" : "de";
  var rel = null;

  function applyLang(l) {
    lang = l;
    root.setAttribute("lang", l);
    texts.forEach(function (de, el) { el.textContent = l === "en" ? el.getAttribute("data-en") : de; });
    alts.forEach(function (de, el) { el.setAttribute("alt", l === "en" ? el.getAttribute("data-en-alt") : de); });
    $$(".lang button").forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.lang === l)); });
    var d = $('meta[name="description"]');
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

  // ---- Neueste Version -----------------------------------------------------
  function mb(n) {
    if (!n) return "";
    var v = n / 1048576;
    return (v >= 100 ? Math.round(v) : v.toFixed(1)).toString().replace(".", lang === "de" ? "," : ".") + " MB";
  }
  function render(r) {
    var f = r.files || {}, s = f.setup, p = f.portable;
    var hero = $("#dl-meta");
    if (hero) {
      hero.textContent = (lang === "en" ? "Version " : "Version ") + r.version + (s && s.size ? " · " + mb(s.size) : "");
      hero.hidden = false;
    }
    var info = $("#dl-info");
    if (info) {
      var parts = ["Version " + r.version];
      if (r.published) {
        var d = new Date(r.published);
        if (!isNaN(d)) parts.push(d.toLocaleDateString(lang === "de" ? "de-AT" : "en-GB", { day: "numeric", month: "long", year: "numeric" }));
      }
      if (s && s.size) parts.push((lang === "en" ? "Installer " : "Installer ") + mb(s.size));
      if (p && p.size) parts.push("Portable " + mb(p.size));
      info.textContent = parts.join(" · ");
    }
    if (s && s.url) { ["dl-main", "dl-setup"].forEach(function (id) { var a = document.getElementById(id); if (a) a.setAttribute("href", s.url); }); }
    if (p && p.url) { var pa = document.getElementById("dl-portable"); if (pa) pa.setAttribute("href", p.url); }
  }
  try { var c = JSON.parse(localStorage.getItem("em-rel") || "null"); if (c && c.version) { rel = c; render(c); } } catch (e) {}
  fetch("/api/release", { headers: { accept: "application/json" } })
    .then(function (r) { if (!r.ok) throw 0; return r.json(); })
    .then(function (r) {
      rel = r; render(r);
      try { localStorage.setItem("em-rel", JSON.stringify(r)); } catch (e) {}
    })
    .catch(function () {});

  // ---- Bilder umschalten -----------------------------------------------------
  var tabs = $$("#gallery [role=tab]");
  function show(tab) {
    tabs.forEach(function (t) {
      var on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      document.getElementById(t.getAttribute("aria-controls")).hidden = !on;
    });
  }
  tabs.forEach(function (t, i) {
    t.addEventListener("click", function () { show(t); });
    t.addEventListener("keydown", function (e) {
      var n = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : -1;
      if (n < 0) return;
      e.preventDefault();
      var next = tabs[(n + tabs.length) % tabs.length];
      show(next); next.focus();
    });
  });

  applyLang(lang);
})();
