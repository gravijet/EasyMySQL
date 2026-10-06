(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, c) { return (c || document).querySelector(s); };
  var $$ = function (s, c) { return Array.prototype.slice.call((c || document).querySelectorAll(s)); };
  var store = {
    get: function (k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set: function (k, v) { try { localStorage.setItem(k, v); } catch (e) {} }
  };
  var lang = function () { return root.getAttribute("lang") === "en" ? "en" : "de"; };

  /* ---------- Sprache: Deutsch steht im HTML, Englisch in data-en ---------- */
  var ATTRS = [["data-en-aria", "aria-label"], ["data-en-title", "title"], ["data-en-tip", "data-tip"]];
  function applyLang() {
    var en = lang() === "en";
    $$("[data-en]").forEach(function (el) {
      if (el.__de === undefined) el.__de = el.innerHTML;
      el.innerHTML = en ? el.getAttribute("data-en") : el.__de;
    });
    ATTRS.forEach(function (p) {
      $$("[" + p[0] + "]").forEach(function (el) {
        var k = "__de_" + p[1];
        if (el[k] === undefined) el[k] = el.getAttribute(p[1]) || "";
        el.setAttribute(p[1], en ? el.getAttribute(p[0]) : el[k]);
      });
    });
    var t = $("title");
    if (t) {
      if (t.__de === undefined) t.__de = t.textContent;
      t.textContent = en ? "EasyMySQL – MariaDB server and database manager for Windows" : t.__de;
    }
    renderRelease();
  }
  var langBtn = $("#lang");
  if (langBtn) langBtn.addEventListener("click", function () {
    var n = lang() === "de" ? "en" : "de";
    root.setAttribute("lang", n);
    store.set("em-lang", n);
    applyLang();
  });
  var themeBtn = $("#theme");
  if (themeBtn) themeBtn.addEventListener("click", function () {
    var n = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", n);
    store.set("em-theme", n);
  });

  /* ---------- neuestes Release ---------- */
  var release = null;
  function mb(bytes) {
    if (!bytes) return "";
    var v = bytes / 1048576;
    return v.toLocaleString(lang() === "de" ? "de-AT" : "en-GB", { maximumFractionDigits: v < 10 ? 1 : 0 }) + " MB";
  }
  function setAll(sel, text) { $$(sel).forEach(function (el) { el.textContent = text; }); }
  function renderRelease() {
    var r = release;
    if (!r) return;
    var v = r.version, de = lang() === "de";
    var setup = r.files.setup || {}, portable = r.files.portable;
    setAll("[data-ver]", "v" + v);
    setAll("[data-ver-head]", "v" + v);
    setAll("[data-ver-plain]", v);
    setAll("[data-ver-plain-v]", v);
    setAll("[data-ver-status]", "EasyMySQL " + v);
    setAll("[data-ver-note]", (de ? "Version " : "Version ") + v);
    setAll("[data-ver-size]", "v" + v + (setup.size ? " · " + mb(setup.size) : ""));
    setAll("[data-mdb]", r.mariadb || "");
    setAll("[data-mdb-v]", r.mariadb || "");
    $$("[data-needs-mdb]").forEach(function (el) { el.hidden = !r.mariadb; });
    setAll("[data-mdb-status]", r.mariadb ? "MariaDB " + r.mariadb : "");
    setAll("[data-ver-banner]", r.mariadb ? "Server version: " + r.mariadb + "-MariaDB mariadb.org binary distribution\n" : "");
    setAll("[data-date]", r.published ? new Date(r.published).toLocaleDateString(de ? "de-AT" : "en-GB", { day: "numeric", month: "long", year: "numeric" }) : "–");
    ["setup", "portable"].forEach(function (k) {
      var f = r.files[k];
      if (!f) return;
      setAll('[data-file="' + k + '"]', f.name);
      setAll('[data-size="' + k + '"]', mb(f.size));
      $$('[data-hash="' + k + '"]').forEach(function (b) {
        if (!f.sha256) return;
        b.hidden = false;
        b.__hash = f.sha256;
        $("span", b).textContent = f.sha256;
      });
    });
  }
  function loadRelease() {
    var cached = store.get("em-rel");
    if (cached) { try { release = JSON.parse(cached); renderRelease(); } catch (e) {} }
    if (!window.fetch) return;
    fetch("/api/release", { headers: { accept: "application/json" } })
      .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
      .then(function (r) { release = r; store.set("em-rel", JSON.stringify(r)); renderRelease(); })
      .catch(function () {});
  }

  /* ---------- Kopieren ---------- */
  function copyText(text, done) {
    function fallback() {
      var t = document.createElement("textarea");
      t.value = text; t.setAttribute("readonly", ""); t.className = "sr-copy";
      document.body.appendChild(t); t.select();
      try { document.execCommand("copy"); } catch (e) {}
      document.body.removeChild(t); done();
    }
    if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(text).then(done, fallback); else fallback();
  }
  document.addEventListener("click", function (e) {
    var c = e.target.closest && e.target.closest("[data-copy]");
    var h = e.target.closest && e.target.closest("[data-hash]");
    var el = c || h;
    if (!el) return;
    var text = c ? c.getAttribute("data-copy") : h.__hash;
    if (!text) return;
    copyText(text, function () {
      var target = c || $("b", h), old = target.innerHTML;
      target.textContent = lang() === "de" ? "Kopiert" : "Copied";
      setTimeout(function () { target.innerHTML = old; }, 1400);
    });
  });

  /* ---------- Registerkarten (Funktionen und Fenster-Nachbau) ---------- */
  function tabs(list, tabSel, paneOf) {
    if (!list) return;
    var items = $$(tabSel, list);
    function select(tab, focus) {
      items.forEach(function (t) {
        var on = t === tab;
        t.setAttribute("aria-selected", on ? "true" : "false");
        t.tabIndex = on ? 0 : -1;
        var p = paneOf(t);
        if (p) { p.hidden = !on; p.classList.toggle("on", on); }
      });
      if (focus) tab.focus();
    }
    items.forEach(function (t, i) {
      t.addEventListener("click", function () { select(t); });
      t.addEventListener("keydown", function (e) {
        var n = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
        if (e.key === "Home") n = -i;
        if (e.key === "End") n = items.length - 1 - i;
        if (n === undefined) return;
        e.preventDefault();
        select(items[(i + n + items.length) % items.length], true);
      });
    });
    select(items.filter(function (t) { return t.getAttribute("aria-selected") === "true"; })[0] || items[0]);
  }
  tabs($(".feat-nav"), ".nav-i", function (t) { return document.getElementById(t.getAttribute("aria-controls")); });
  tabs($(".w-tabs"), ".w-tab", function (t) { return document.getElementById(t.getAttribute("data-pane")); });

  /* ---------- aktiver Abschnitt ---------- */
  var secs = $$("main .sec");
  function mark(id) {
    $$(".activity a").forEach(function (a) { a.classList.toggle("on", a.getAttribute("data-sec") === id); });
    $$(".menus a").forEach(function (a) { a.classList.toggle("on", a.getAttribute("href") === "#" + id); });
  }
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) mark(e.target.id); });
    }, { rootMargin: "-35% 0px -60% 0px" });
    secs.forEach(function (s) { io.observe(s); });
  }
  mark("start");

  applyLang();
  loadRelease();
})();
