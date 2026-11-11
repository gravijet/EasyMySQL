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
  var bereit = false;
  var release = null;
  var ladefehler = false;

  var ruhig = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Terminal: Befehl je Betriebssystem, steht sofort da.
  var systeme = $$("[data-install-os]");
  var system = "win";
  function zeigeBefehl() {
    var windows = system === "win";
    var befehl = windows
      ? "irm https://mysql.benjaminberger.at/install | iex"
      : "curl -fsSL https://mysql.benjaminberger.at/install | sh";
    var ziel = $("#hero-cmd");
    ziel.dataset.full = befehl;
    $("#prompt").textContent = windows ? "PS>" : "$";
    ziel.textContent = befehl;
  }
  function waehleSystem(knopf, fokus) {
    system = knopf.dataset.installOs;
    systeme.forEach(function (el) {
      el.setAttribute("aria-checked", String(el === knopf));
      el.tabIndex = el === knopf ? 0 : -1;
    });
    zeigeBefehl();
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
    (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  system = /mac/i.test(plattform) ? "mac" : /linux/i.test(plattform) ? "linux" : "win";
  systeme.forEach(function (el) {
    el.setAttribute("aria-checked", String(el.dataset.installOs === system));
    el.tabIndex = el.dataset.installOs === system ? 0 : -1;
  });

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
    el.textContent = ladefehler ? text("Downloads gerade nicht erreichbar.", "Downloads are unavailable right now.") : "";
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
      "Datenbank, SQL-Editor, ER-Diagramme und Java in einer App. Für Windows, macOS und Linux.",
      "Database, SQL editor, ER diagrams and Java in one app. For Windows, macOS and Linux.",
    );
    teileWoerter();
    if (bereit) male();
    zeigeBefehl();
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

  // ---------- Buehne: welcher Text als Relief in der Fluessigkeit steht ----------
  // Spiegelfarben je Abschnitt (zwei Toene, Werte 0 bis 1)
  var FARBEN = {
    start: [[0.25, 0.45, 1], [0.7, 0.25, 0.95]],
    vergleich: [[1, 0.35, 0.55], [0.3, 0.35, 1]],
    tabelle: [[0.12, 0.18, 0.4], [0.1, 0.25, 0.3]],
    ende: [[0.2, 0.85, 0.7], [0.3, 0.45, 1]],
  };
  var FUNKTIONEN = [
    { de: "SQL-Editor", en: "SQL editor", bild: "sql", f: [[0.25, 0.5, 1], [0.1, 0.8, 0.8]] },
    { de: "Tabellen", en: "Tables", bild: "data", f: [[0.1, 0.8, 0.75], [0.2, 0.4, 1]] },
    { de: "ER-Diagramme", en: "ER diagrams", bild: "er", f: [[1, 0.6, 0.15], [0.9, 0.2, 0.45]] },
    { de: "Query Builder", en: "Query builder", bild: "builder", f: [[0.65, 0.35, 1], [0.25, 0.4, 1]] },
    { de: "Java", en: "Java", bild: "code", f: [[0.25, 0.9, 0.5], [0.1, 0.6, 0.8]] },
    { de: "Git", en: "Git", bild: "git", f: [[0.4, 0.6, 0.9], [0.8, 0.4, 0.65]] },
  ];
  // Sekunden bzw. MB fuer EasyMySQL, XAMPP, Workbench; null = geht dort nicht.
  var FEHLT = ["nicht vorhanden", "not available"];
  var MESSUNGEN = [
    { de: "Programm starten", en: "Starting the app", w: [0.47, 2.7, 5.7], e: "s" },
    { de: "Tabelle öffnen", en: "Opening a table", w: [0.17, 0.33, 2.1], e: "s" },
    { de: "In 1 Mio. Zeilen suchen", en: "Searching 1M rows", w: [0.2, 0.34, 0.25], e: "s" },
    { de: "ER-Diagramm zeichnen", en: "Drawing an ER diagram", w: [0.22, 0.6, null], e: "s", g: [null, null, FEHLT] },
    { de: "1 Mio. Zeilen importieren", en: "Importing 1M rows", w: [7.7, null, null], e: "s", g: [null, ["Datei zu groß", "file too large"], FEHLT] },
    { de: "Arbeitsspeicher", en: "Memory", w: [133, 252, 266], e: "MB" },
    { de: "Platz auf der Festplatte", en: "Disk space", w: [260, 929, 870], e: "MB" },
    { de: "Download", en: "Download", w: [77, 200, 273], e: "MB" },
  ];
  var szene = (window.easyScene = { n: 0, text: null, hoch: 1, A: FARBEN.start[0], B: FARBEN.start[1] });
  var letzterText = "";
  function zahl(wert, einheit) {
    var stellen = einheit === "MB" ? 0 : wert < 1 ? 2 : 1;
    return (
      new Intl.NumberFormat(sprache, { minimumFractionDigits: stellen, maximumFractionDigits: stellen }).format(wert) +
      " " +
      einheit
    );
  }

  var abschnitte = ["top", "compare", "features", "table", "ai"].map(function (id) {
    return document.getElementById(id);
  });
  var aktiv = 0;
  var messung = 0;
  var funktion = 0;

  // Legt fest, was die Buehne zeigt: Text, Lage, Groesse und Farben.
  function stelle() {
    var schmal = innerWidth < innerHeight;
    var rand = Math.min(60, Math.max(20, innerWidth * 0.04)) / innerWidth;
    var spez = null;
    var farbe = FARBEN.start;
    if (aktiv === 0) {
      spez = schmal
        ? { zeilen: ["Easy", "MySQL"], h: 0.17, y: 0.3 }
        : { zeilen: ["EasyMySQL"], h: 0.36, y: 0.4 };
    } else if (aktiv === 1) {
      var m = MESSUNGEN[messung];
      spez = { zeilen: [zahl(m.w[0], m.e)], h: schmal ? 0.2 : 0.46, y: schmal ? 0.4 : 0.46 };
      farbe = FARBEN.vergleich;
    } else if (aktiv === 2) {
      var f = FUNKTIONEN[funktion];
      spez = { zeilen: [text(f.de, f.en)], h: schmal ? 0.085 : 0.2, y: schmal ? 0.17 : 0.2, breit: schmal ? 1 - 2 * rand : 0.6 };
      farbe = f.f;
    } else if (aktiv === 3) {
      farbe = FARBEN.tabelle;
    } else {
      spez = { zeilen: ["Claude Code", "& Codex"], h: schmal ? 0.11 : 0.24, y: 0.42 };
      farbe = FARBEN.ende;
    }
    if (spez) {
      spez.x = rand;
      if (!spez.breit) spez.breit = 1 - 2 * rand;
    }
    szene.A = farbe[0];
    szene.B = farbe[1];
    szene.hoch = spez ? 1 : 0;
    var kennung = JSON.stringify(spez) + innerWidth + "x" + innerHeight;
    if (kennung !== letzterText) {
      letzterText = kennung;
      szene.text = spez;
      szene.n++;
    }
    document.dispatchEvent(new Event("easymysql-scene"));
  }

  // ---------- Vergleich: laeuft von allein durch ----------
  var mName = $("#m-name");
  var mUns = $("#m-us");
  var gegner = $$(".rival").map(function (el) {
    return { el: el, wert: el.querySelector("b") };
  });
  function zeigeMessung() {
    var m = MESSUNGEN[messung];
    var groesste = Math.max.apply(null, m.w.filter(Boolean));
    mName.textContent = text(m.de, m.en);
    mName.classList.remove("swap");
    void mName.offsetWidth; // Animation neu starten
    mName.classList.add("swap");
    mUns.textContent = text(m.de, m.en) + ": EasyMySQL " + zahl(m.w[0], m.e);
    gegner.forEach(function (g, i) {
      var w = m.w[i];
      g.el.classList.toggle("na", w === null);
      g.el.style.setProperty("--f", w === null ? 0 : ((w / groesste) * 100).toFixed(1));
      if (g.wert) g.wert.textContent = w === null ? text(m.g[i][0], m.g[i][1]) : zahl(w, m.e);
    });
  }

  // ---------- Funktionen: Bild aus Lamellen, die sich der Reihe nach drehen ----------
  var LAMELLEN = 14;
  var blende = $("#blinds");
  var lamellen = [];
  var gedreht = false;
  var gross = innerWidth > 820 ? "2000" : "1000";
  function bildAdresse(i) {
    return "url(/shots/" + FUNKTIONEN[i].bild + "-" + gross + ".webp)";
  }
  blende.replaceChildren();
  for (var l = 0; l < LAMELLEN; l++) {
    var lamelle = document.createElement("div");
    lamelle.className = "slat";
    lamelle.style.setProperty("--i", l);
    lamelle.style.setProperty("--count", LAMELLEN);
    var vorn = document.createElement("s");
    var hinten = document.createElement("s");
    vorn.style.backgroundImage = bildAdresse(0);
    lamelle.append(vorn, hinten);
    blende.append(lamelle);
    lamellen.push([lamelle, vorn, hinten]);
  }
  // alle Bilder vorab laden, damit beim Drehen nichts fehlt
  FUNKTIONEN.forEach(function (f) {
    new Image().src = "/shots/" + f.bild + "-" + gross + ".webp";
  });
  function zeigeFunktion() {
    // das neue Bild kommt auf die Rueckseite, dann drehen sich alle Lamellen
    lamellen.forEach(function (teile) {
      teile[gedreht ? 1 : 2].style.backgroundImage = bildAdresse(funktion);
      teile[0].classList.toggle("flip", !gedreht);
    });
    gedreht = !gedreht;
    $("#f-name").textContent = text(FUNKTIONEN[funktion].de, FUNKTIONEN[funktion].en);
  }

  // Alle 3,6 s geht der sichtbare Abschnitt einen Schritt weiter; niemand muss etwas anklicken.
  setInterval(function () {
    if (document.hidden) return;
    if (aktiv === 1) {
      messung = (messung + 1) % MESSUNGEN.length;
      zeigeMessung();
      stelle();
    } else if (aktiv === 2) {
      funktion = (funktion + 1) % FUNKTIONEN.length;
      zeigeFunktion();
      stelle();
    }
  }, 3600);

  // ---------- Scrollen, Maus, eigener Zeiger ----------
  var zeiger = $("#cursor");
  var eigenerZeiger = matchMedia("(hover: hover) and (pointer: fine)").matches && !ruhig.matches;
  if (eigenerZeiger) wurzel.classList.add("cur");
  var maus = [innerWidth / 2, innerHeight / 2];
  var folgt = maus.slice();
  var neigung = [0, 0];
  var bild = 0;
  function zeichne() {
    bild = 0;
    folgt[0] += (maus[0] - folgt[0]) * 0.22;
    folgt[1] += (maus[1] - folgt[1]) * 0.22;
    zeiger.style.setProperty("--x", folgt[0].toFixed(1) + "px");
    zeiger.style.setProperty("--y", folgt[1].toFixed(1) + "px");
    // das App-Bild neigt sich leicht zur Maus
    neigung[0] += (maus[0] / innerWidth - 0.5 - neigung[0]) * 0.06;
    neigung[1] += (maus[1] / innerHeight - 0.5 - neigung[1]) * 0.06;
    blende.style.setProperty("--ry", (-12 + neigung[0] * 16).toFixed(2) + "deg");
    blende.style.setProperty("--rx", (6 - neigung[1] * 10).toFixed(2) + "deg");
    if (Math.abs(maus[0] - folgt[0]) + Math.abs(maus[1] - folgt[1]) > 0.3) anfordern();
  }
  function anfordern() {
    if (!bild && !ruhig.matches) bild = requestAnimationFrame(zeichne);
  }
  addEventListener(
    "pointermove",
    function (e) {
      if (e.pointerType !== "mouse") return;
      maus = [e.clientX, e.clientY];
      var ziel = e.target.closest ? e.target.closest("[data-cursor]") : null;
      zeiger.classList.toggle("on", !!ziel);
      if (ziel) {
        var wort = text(ziel.dataset.cursor, ziel.dataset.cursorEn || ziel.dataset.cursor);
        zeiger.firstElementChild.textContent = wort;
        zeiger.style.setProperty("--w", wort.length * 8 + 34 + "px");
      } else zeiger.style.setProperty("--w", e.target.closest && e.target.closest("a, button") ? "34px" : "18px");
      anfordern();
    },
    { passive: true },
  );
  function pruefeAbschnitt() {
    var mitte = innerHeight / 2;
    var neu = aktiv;
    abschnitte.forEach(function (el, i) {
      var r = el.getBoundingClientRect();
      if (r.top <= mitte && r.bottom > mitte) neu = i;
    });
    if (neu !== aktiv) {
      aktiv = neu;
      stelle();
    }
  }
  addEventListener("scroll", pruefeAbschnitt, { passive: true });
  addEventListener("resize", function () {
    pruefeAbschnitt();
    stelle();
  });
  // nach einem Sprachwechsel alles neu schreiben
  function male() {
    zeigeMessung();
    $("#f-name").textContent = text(FUNKTIONEN[funktion].de, FUNKTIONEN[funktion].en);
    stelle();
  }
  bereit = true;
  pruefeAbschnitt();
  male();

  // Tabelle: Zeilen blenden nacheinander ein.
  var ziele = $$(".table h2, tbody tr, tfoot tr");
  if ("IntersectionObserver" in window) {
    var beobachter = new IntersectionObserver(
      function (eintraege) {
        var reihe = 0;
        eintraege.forEach(function (e) {
          if (!e.isIntersecting) return;
          e.target.style.setProperty("--d", reihe++);
          e.target.classList.add("in");
          beobachter.unobserve(e.target);
        });
      },
      { threshold: 0.1, rootMargin: "0px 0px -6% 0px" },
    );
    ziele.forEach(function (el) {
      beobachter.observe(el);
    });
  } else
    ziele.forEach(function (el) {
      el.classList.add("in");
    });

  zeigeBefehl();

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
      var quelle = $(knopf.dataset.copyFrom);
      var inhalt = (quelle.dataset.full || quelle.textContent).trim();
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
