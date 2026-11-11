(function () {
  "use strict";
  var wurzel = document.documentElement;
  var $ = function (s) {
    return document.querySelector(s);
  };
  var $$ = function (s) {
    return Array.from(document.querySelectorAll(s));
  };
  var sprache = wurzel.lang === "en" ? "en" : "de";
  var text = function (de, en) {
    return sprache === "de" ? de : en;
  };
  var texte = $$("[data-en]").map(function (el) {
    return [el, el.textContent];
  });
  var attribute = ["alt", "label"]
    .map(function (art) {
      return $$("[data-en-" + art + "]").map(function (el) {
        var name = art === "label" ? "aria-label" : art;
        return [
          el,
          name,
          el.getAttribute(name),
          el.getAttribute("data-en-" + art),
        ];
      });
    })
    .flat();
  var release = null;
  var ladefehler = false;

  // Betriebssystemauswahl gilt fuer den kompakten Installationsbefehl oben.
  var systeme = $$("[data-install-os]");
  function waehleSystem(knopf, fokus) {
    var windows = knopf.dataset.installOs === "win";
    systeme.forEach(function (el) {
      el.setAttribute("aria-checked", String(el === knopf));
      el.tabIndex = el === knopf ? 0 : -1;
    });
    $("#hero-cmd").textContent = windows
      ? "irm https://mysql.benjaminberger.at/install | iex"
      : "curl -fsSL https://mysql.benjaminberger.at/install | sh";
    $("#terminal-shell").textContent = windows ? "PowerShell" : "Terminal · sh";
    if (fokus) knopf.focus();
  }
  systeme.forEach(function (knopf, index) {
    knopf.addEventListener("click", function () {
      waehleSystem(knopf, false);
    });
    knopf.addEventListener("keydown", function (e) {
      var ziel =
        e.key === "ArrowRight" || e.key === "ArrowDown"
          ? (index + 1) % systeme.length
          : e.key === "ArrowLeft" || e.key === "ArrowUp"
            ? (index + systeme.length - 1) % systeme.length
            : e.key === "Home"
              ? 0
              : e.key === "End"
                ? systeme.length - 1
                : -1;
      if (ziel !== -1) {
        e.preventDefault();
        waehleSystem(systeme[ziel], true);
      }
    });
  });
  var plattform =
    (navigator.userAgentData && navigator.userAgentData.platform) ||
    navigator.platform ||
    "";
  waehleSystem(
    systeme[/mac/i.test(plattform) ? 1 : /linux/i.test(plattform) ? 2 : 0],
    false,
  );

  function groesse(n) {
    return (
      new Intl.NumberFormat(sprache, { maximumFractionDigits: 1 }).format(
        n / 1000000,
      ) + " MB"
    );
  }

  function zeigeRelease() {
    if (!release) return;
    $$("[data-release-version]").forEach(function (el) {
      el.textContent = "v" + release.version;
    });
    $$("[data-dl]").forEach(function (a) {
      var datei = release.files[a.dataset.dl];
      var vorhanden = !!(datei && datei.url === "/download/" + a.dataset.dl);
      if (vorhanden) a.setAttribute("href", datei.url);
      else a.removeAttribute("href");
      a.setAttribute("aria-disabled", String(!vorhanden));
      a.title = vorhanden
        ? (datei.name || "EasyMySQL") +
          (datei.size ? " · " + groesse(datei.size) : "")
        : text(
            "Kein Paket im aktuellen Release",
            "No package in the current release",
          );
      // Fehlende Dateien mit Begruendung auffindbar halten.
      if (!vorhanden) {
        a.setAttribute("role", "link");
        a.tabIndex = 0;
      } else {
        a.removeAttribute("role");
        a.removeAttribute("tabindex");
      }
    });
    $$("[data-file-state]").forEach(function (el) {
      var datei = release.files[el.dataset.fileState];
      el.textContent =
        datei && datei.url === "/download/" + el.dataset.fileState
          ? datei.size
            ? groesse(datei.size)
            : text("Direkter Download", "Direct download")
          : text(
              "Noch kein Paket im aktuellen Release.",
              "No package in the current release yet.",
            );
    });
    var fehlend = [
      ["setup", "Windows"],
      ["macos", "macOS"],
      ["linux", "Linux"],
    ]
      .filter(function (p) {
        return !release.files[p[0]];
      })
      .map(function (p) {
        return p[1];
      });
    var hinweis = $("#download-notice");
    hinweis.hidden = !fehlend.length;
    hinweis.textContent = fehlend.length
      ? text("Aktuell kein Download für ", "Currently no download for ") +
        fehlend.join(", ") +
        text(" im Release.", " in this release.")
      : "";
  }

  function zeigeFehler() {
    var el = $("#release-error");
    el.hidden = !ladefehler;
    el.replaceChildren();
    if (!ladefehler) return;
    el.append(
      document.createTextNode(
        text(
          "Release-Status nicht erreichbar. ",
          "Release status unavailable. ",
        ),
      ),
    );
    var link = document.createElement("a");
    link.href = "https://github.com/gravijet/EasyMySQL/releases/latest";
    link.textContent = text(
      "Downloads auf GitHub öffnen",
      "Open downloads on GitHub",
    );
    el.append(link);
  }

  function setzeSprache(neu) {
    sprache = neu;
    wurzel.lang = neu;
    texte.forEach(function (p) {
      p[0].textContent = neu === "en" ? p[0].dataset.en : p[1];
    });
    attribute.forEach(function (p) {
      p[0].setAttribute(p[1], neu === "en" ? p[3] : p[2]);
    });
    var andere = neu === "en" ? "de" : "en";
    $("#lang").textContent = andere.toUpperCase();
    $("#lang").lang = andere;
    $("#lang").setAttribute(
      "aria-label",
      andere === "en" ? "English" : "Deutsch",
    );
    document.title = text(
      "EasyMySQL – Datenbank, SQL & Java",
      "EasyMySQL – Database, SQL & Java",
    );
    $('meta[name="description"]').content = text(
      "Datenbank, SQL-Editor, ER-Diagramme und Java in einer App. Kostenlos für Windows, macOS und Linux.",
      "Database, SQL editor, ER diagrams and Java in one app. Free for Windows, macOS and Linux.",
    );
    teileWoerter();
    zeigeRelease();
    zeigeFehler();
  }
  // Ueberschrift in Woerter teilen, damit sie einzeln einblenden
  function teileWoerter() {
    $$("[data-split]").forEach(function (el) {
      var woerter = el.textContent.trim().split(/\s+/);
      el.replaceChildren();
      woerter.forEach(function (wort, i) {
        var span = document.createElement("span");
        span.className = "w";
        span.textContent = wort;
        span.style.setProperty("--i", i);
        // der farbige Schlusssatz (letzte vier Woerter) beginnt in einer neuen Zeile
        if (i === woerter.length - 4) el.append(document.createElement("br"));
        el.append(span, " ");
      });
    });
  }
  $("#lang").addEventListener("click", function () {
    var neu = sprache === "de" ? "en" : "de";
    try {
      localStorage.setItem("em-lang", neu);
    } catch (_) {}
    setzeSprache(neu);
  });
  setzeSprache(sprache);

  // Downloadlinks funktionieren auch ohne JavaScript und vor der Release-Abfrage.
  var abbruch = new AbortController();
  var zeitlimit = setTimeout(function () {
    abbruch.abort();
  }, 10000);
  fetch("/api/release", {
    headers: { accept: "application/json" },
    signal: abbruch.signal,
  })
    .then(function (antwort) {
      if (!antwort.ok) throw new Error("release");
      return antwort.json();
    })
    .then(function (daten) {
      if (
        !/^\d+\.\d+\.\d+$/.test(daten.version) ||
        !daten.files ||
        typeof daten.files !== "object"
      )
        throw new Error("release");
      release = daten;
      zeigeRelease();
    })
    .catch(function () {
      ladefehler = true;
      zeigeFehler();
    })
    .finally(function () {
      clearTimeout(zeitlimit);
    });

  var ruhig = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Tabs: Pfeiltasten, Home/End, eine Tabposition; die farbige Flaeche gleitet zum gewaehlten Tab.
  var tabs = $$("[role=tab]");
  var gleiter = $(".glider");
  function setzeGleiter() {
    var tab = $("[role=tab][aria-selected=true]");
    gleiter.style.width = tab.offsetWidth + "px";
    gleiter.style.translate = tab.offsetLeft + "px 0";
  }
  function waehleTab(tab, fokus) {
    tabs.forEach(function (el) {
      var aktiv = el === tab;
      el.setAttribute("aria-selected", String(aktiv));
      el.tabIndex = aktiv ? 0 : -1;
      document.getElementById(el.getAttribute("aria-controls")).hidden = !aktiv;
    });
    setzeGleiter();
    if (fokus) tab.focus({ preventScroll: true });
    var leiste = $(".tabs-scroll");
    leiste.scrollTo({
      left: tab.offsetLeft - (leiste.clientWidth - tab.offsetWidth) / 2,
      behavior: ruhig.matches ? "auto" : "smooth",
    });
  }
  tabs.forEach(function (tab, index) {
    tab.tabIndex = index === 0 ? 0 : -1;
    tab.addEventListener("click", function () {
      waehleTab(tab, false);
    });
    tab.addEventListener("keydown", function (e) {
      var ziel =
        e.key === "ArrowRight"
          ? (index + 1) % tabs.length
          : e.key === "ArrowLeft"
            ? (index + tabs.length - 1) % tabs.length
            : e.key === "Home"
              ? 0
              : e.key === "End"
                ? tabs.length - 1
                : -1;
      if (ziel !== -1) {
        e.preventDefault();
        waehleTab(tabs[ziel], true);
      }
    });
  });
  setzeGleiter();
  addEventListener("resize", setzeGleiter, { passive: true });
  if (document.fonts) document.fonts.ready.then(setzeGleiter);

  // Zahlen zaehlen hoch, sobald sie sichtbar werden.
  function zaehle(el) {
    var ziel = parseFloat(el.dataset.count);
    var stellen = +el.dataset.dec;
    var anhang = el.textContent.replace(/^[\d.,]+/, "");
    var beginn = performance.now();
    (function schritt(jetzt) {
      var t = Math.min(1, (jetzt - beginn) / 1600);
      var wert = t === 1 ? ziel : ziel * (1 - Math.pow(2, -10 * t));
      el.textContent =
        new Intl.NumberFormat(sprache, {
          minimumFractionDigits: stellen,
          maximumFractionDigits: stellen,
        }).format(wert) + anhang;
      if (t < 1) requestAnimationFrame(schritt);
    })(beginn);
  }

  // Einblenden beim Scrollen: Geschwister nacheinander, Balken und Zaehler starten mit.
  function zeige(el) {
    el.classList.add("in");
    $$("[data-w]").forEach(function (balken) {
      if (el.contains(balken)) balken.style.width = balken.dataset.w + "%";
    });
    if (!ruhig.matches)
      $$("[data-count]").forEach(function (zahl) {
        if (el.contains(zahl)) zaehle(zahl);
      });
  }
  var ziele = $$("[data-reveal], .tile, .stat, tbody tr, tfoot tr");
  if ("IntersectionObserver" in window) {
    var beobachter = new IntersectionObserver(
      function (eintraege) {
        var reihe = 0;
        eintraege.forEach(function (e) {
          if (!e.isIntersecting) return;
          e.target.style.setProperty("--d", reihe++);
          zeige(e.target);
          beobachter.unobserve(e.target);
        });
      },
      { threshold: 0.2, rootMargin: "0px 0px -6% 0px" },
    );
    ziele.forEach(function (el) {
      beobachter.observe(el);
    });
  } else ziele.forEach(zeige);

  // Scrollen: Fortschrittsbalken, schmalere Kopfzeile, das Fenster richtet sich auf.
  var fenster = $("#hero-window");
  var mausX = 0,
    mausY = 0,
    kippX = 0,
    kippY = 0,
    bild = 0;
  function zeichne() {
    bild = 0;
    var hoch = document.documentElement.scrollHeight - innerHeight;
    wurzel.style.setProperty("--scroll", hoch > 0 ? (scrollY / hoch).toFixed(4) : 0);
    $(".nav").classList.toggle("small", scrollY > 60);
    if (ruhig.matches) return;
    var r = fenster.parentElement.getBoundingClientRect();
    // 1 = Fenster noch unten im Bild, 0 = oben angekommen
    var k = Math.min(1, Math.max(0, (r.top - innerHeight * 0.18) / (innerHeight * 0.62)));
    kippX += (mausX - kippX) * 0.08;
    kippY += (mausY - kippY) * 0.08;
    fenster.style.setProperty("--rx", (k * 22 - kippY * 3).toFixed(2) + "deg");
    fenster.style.setProperty("--ry", (kippX * 4).toFixed(2) + "deg");
    fenster.style.setProperty("--s", (1 - k * 0.1).toFixed(4));
    if (Math.abs(mausX - kippX) + Math.abs(mausY - kippY) > 0.002) anfordern();
  }
  function anfordern() {
    if (!bild) bild = requestAnimationFrame(zeichne);
  }
  addEventListener("scroll", anfordern, { passive: true });
  addEventListener("resize", anfordern, { passive: true });
  addEventListener(
    "pointermove",
    function (e) {
      if (e.pointerType !== "mouse" || ruhig.matches) return;
      mausX = e.clientX / innerWidth - 0.5;
      mausY = e.clientY / innerHeight - 0.5;
      anfordern();
    },
    { passive: true },
  );
  anfordern();

  // Lichtfleck folgt der Maus (Kacheln, KI-Flaeche); der grosse Knopf zieht leicht zur Maus.
  $$(".tile, [data-spot]").forEach(function (el) {
    el.addEventListener("pointermove", function (e) {
      var r = el.getBoundingClientRect();
      el.style.setProperty("--mx", e.clientX - r.left + "px");
      el.style.setProperty("--my", e.clientY - r.top + "px");
    });
  });
  $$("[data-magnet]").forEach(function (el) {
    el.addEventListener("pointermove", function (e) {
      if (e.pointerType !== "mouse" || ruhig.matches) return;
      var r = el.getBoundingClientRect();
      el.style.setProperty("--tx", ((e.clientX - r.left - r.width / 2) * 0.18).toFixed(1) + "px");
      el.style.setProperty("--ty", ((e.clientY - r.top - r.height / 2) * 0.3).toFixed(1) + "px");
    });
    el.addEventListener("pointerleave", function () {
      el.style.setProperty("--tx", "0px");
      el.style.setProperty("--ty", "0px");
    });
  });

  var meldung;
  function zeigeMeldung(inhalt) {
    if (!meldung) {
      meldung = document.createElement("div");
      meldung.className = "toast";
      meldung.setAttribute("role", "status");
      document.body.append(meldung);
    }
    meldung.textContent = inhalt;
    meldung.hidden = false;
    clearTimeout(meldung.zeit);
    meldung.zeit = setTimeout(function () {
      meldung.hidden = true;
    }, 2200);
  }
  $$("[data-copy-from]").forEach(function (knopf) {
    knopf.addEventListener("click", function () {
      var inhalt = $(knopf.dataset.copyFrom).textContent.trim();
      if (navigator.clipboard)
        navigator.clipboard.writeText(inhalt).then(
          function () {
            zeigeMeldung(text("Befehl kopiert", "Command copied"));
          },
          function () {
            zeigeMeldung(
              text(
                "Bitte den Befehl markieren und kopieren.",
                "Select and copy the command.",
              ),
            );
          },
        );
      else
        zeigeMeldung(
          text(
            "Bitte den Befehl markieren und kopieren.",
            "Select and copy the command.",
          ),
        );
    });
  });
})();
