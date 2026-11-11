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
    $("#terminal-shell").textContent = windows
      ? text("PowerShell öffnen, einfügen, Enter drücken.", "Open PowerShell, paste, press Enter.")
      : text("Terminal öffnen, einfügen, Enter drücken.", "Open a terminal, paste, press Enter.");
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

  // ---------- Szenen: alles haengt am Scrollen und wird weich nachgefuehrt ----------
  function grenze(wert) {
    return Math.min(1, Math.max(0, wert));
  }
  function weich(t) {
    return t * t * (3 - 2 * t);
  }
  function setze(liste, index) {
    liste.forEach(function (el, i) {
      el.classList.toggle("on", i === index);
      el.classList.toggle("before", i < index);
      el.classList.toggle("after", i > index);
    });
  }
  // Farben der Wolken je Abschnitt (zwei Toene, Werte 0 bis 1)
  var FARBEN = {
    start: [[0.09, 0.2, 0.62], [0.3, 0.1, 0.52]],
    funktionen: [
      [[0.08, 0.22, 0.66], [0.04, 0.36, 0.52]],
      [[0.03, 0.36, 0.4], [0.05, 0.2, 0.5]],
      [[0.5, 0.26, 0.06], [0.42, 0.1, 0.22]],
      [[0.34, 0.14, 0.6], [0.1, 0.16, 0.56]],
      [[0.1, 0.4, 0.22], [0.05, 0.26, 0.38]],
      [[0.14, 0.24, 0.42], [0.36, 0.16, 0.3]],
    ],
    vergleich: [[[0.46, 0.1, 0.3], [0.12, 0.14, 0.52]], [[0.1, 0.3, 0.6], [0.4, 0.12, 0.4]]],
    tabelle: [[0.05, 0.08, 0.18], [0.04, 0.1, 0.14]],
    ki: [[0.06, 0.34, 0.3], [0.1, 0.18, 0.58]],
  };
  var szene = (window.easyScene = { S: 0, V: 1, K: 1, O: [0.9, 0], A: FARBEN.start[0].slice(), B: FARBEN.start[1].slice() });

  // Funktionen: Titel, Text und App-Fenster wechseln gemeinsam.
  var fAbschnitt = $("#features");
  var fTitel = $$("#features .titles h3");
  var fTexte = $$("#features .caps p");
  var fBilder = $$("#features .screen picture");
  var fKnoepfe = $$("#features .rail button");
  var fBuehne = $("#features .screen");
  var fIndex = -1;

  // Vergleich: Sekunden bzw. MB fuer EasyMySQL, XAMPP, Workbench; null = geht dort nicht.
  var FEHLT = ["nicht vorhanden", "not available"];
  var MESSUNGEN = [
    { w: [0.47, 2.7, 5.7], e: "s" },
    { w: [0.17, 0.33, 2.1], e: "s" },
    { w: [0.2, 0.34, 0.25], e: "s" },
    { w: [0.22, 0.6, null], e: "s", g: [null, null, FEHLT] },
    { w: [7.7, null, null], e: "s", g: [null, ["Datei zu groß", "file too large"], FEHLT] },
    { w: [133, 252, 266], e: "MB" },
    { w: [260, 929, 870], e: "MB" },
    { w: [77, 200, 273], e: "MB" },
  ];
  var vAbschnitt = $("#compare");
  var vTitel = $$("#compare .titles h3");
  var vKnoepfe = $$("#compare .rail button");
  var bahnen = $$(".lane").map(function (el) {
    return { el: el, balken: el.querySelector("i"), wert: el.querySelector("b") };
  });
  var vIndex = -1;
  function zahl(wert, einheit) {
    var stellen = einheit === "MB" ? 0 : wert < 1 ? 2 : 1;
    return (
      new Intl.NumberFormat(sprache, { minimumFractionDigits: stellen, maximumFractionDigits: stellen }).format(wert) +
      " " +
      einheit
    );
  }
  // Stand einer Messung: u laeuft von 0 (Start) bis 1 (alle fertig).
  function maleMessung(index, u) {
    var m = MESSUNGEN[index];
    var groesste = Math.max.apply(
      null,
      m.w.filter(function (w) {
        return w !== null;
      }),
    );
    bahnen.forEach(function (bahn, i) {
      var w = m.w[i];
      bahn.el.classList.toggle("na", w === null);
      if (w === null) {
        bahn.balken.style.setProperty("--f", 0);
        bahn.wert.textContent = text(m.g[i][0], m.g[i][1]);
        return;
      }
      // Zeiten laufen wie eine Stoppuhr bis zum eigenen Wert, Groessen wachsen gemeinsam
      var stand = m.e === "s" ? Math.min(w, u * groesste) : w * weich(u);
      bahn.balken.style.setProperty("--f", ((stand / groesste) * 100).toFixed(2));
      bahn.wert.textContent = zahl(u >= 1 ? w : Math.max(stand, m.e === "s" ? 0.01 : 1), m.e);
    });
  }

  // KI-Satz in Woerter teilen; sie werden beim Scrollen nacheinander hell.
  var licht = [];
  function teileLicht() {
    licht = [];
    $$("[data-light]").forEach(function (el) {
      var woerter = el.textContent.trim().split(/\s+/);
      el.replaceChildren();
      woerter.forEach(function (wort) {
        var span = document.createElement("span");
        span.className = "w";
        span.textContent = wort;
        el.append(span, " ");
        licht.push(span);
      });
    });
  }

  var maus = [0, 0];
  var nach = { f: 0, v: 0, ein: 1, mx: 0, my: 0 };
  var bild = 0;
  function fortschritt(abschnitt) {
    var r = abschnitt.getBoundingClientRect();
    return grenze(-r.top / Math.max(1, r.height - innerHeight));
  }
  function zeichne() {
    bild = 0;
    var hoch = innerHeight;
    var seite = innerWidth / hoch;
    var schmal = seite < 1;
    var k = ruhig.matches ? 1 : 0.14;

    // --- Funktionen ---
    var fRect = fAbschnitt.getBoundingClientRect();
    var fZiel = fortschritt(fAbschnitt);
    nach.f += (fZiel - nach.f) * k;
    nach.ein += (grenze(fRect.top / hoch) - nach.ein) * k;
    var fStand = Math.min(fTitel.length - 0.0001, nach.f * fTitel.length);
    var fNeu = Math.floor(fStand);
    if (fNeu !== fIndex) {
      fIndex = fNeu;
      setze(fTitel, fIndex);
      setze(fTexte, fIndex);
      setze(fBilder, fIndex);
      fKnoepfe.forEach(function (knopf, i) {
        knopf.setAttribute("aria-selected", String(i === fIndex));
        knopf.tabIndex = i === fIndex ? 0 : -1;
      });
      fBilder.forEach(function (b, i) {
        b.setAttribute("aria-hidden", String(i !== fIndex));
      });
      zeigeInLeiste(fKnoepfe[fIndex]);
    }
    fKnoepfe.forEach(function (knopf, i) {
      knopf.style.setProperty("--p", i < fIndex ? 1 : i === fIndex ? (fStand - i).toFixed(3) : 0);
    });
    nach.mx += (maus[0] - nach.mx) * 0.08;
    nach.my += (maus[1] - nach.my) * 0.08;
    // das Fenster kommt gekippt von unten und richtet sich auf
    fBuehne.style.setProperty("--rx", (nach.ein * 46 - nach.my * 5).toFixed(2) + "deg");
    fBuehne.style.setProperty("--ry", (nach.mx * 7).toFixed(2) + "deg");
    fBuehne.style.setProperty("--sc", (1 - nach.ein * 0.22).toFixed(4));

    // --- Vergleich ---
    var vZiel = fortschritt(vAbschnitt);
    nach.v += (vZiel - nach.v) * k;
    var vStand = Math.min(MESSUNGEN.length - 0.0001, nach.v * MESSUNGEN.length);
    var vNeu = Math.floor(vStand);
    if (vNeu !== vIndex) {
      vIndex = vNeu;
      setze(vTitel, vIndex);
      vKnoepfe.forEach(function (knopf, i) {
        knopf.setAttribute("aria-pressed", String(i === vIndex));
      });
      zeigeInLeiste(vKnoepfe[vIndex]);
    }
    vKnoepfe.forEach(function (knopf, i) {
      knopf.style.setProperty("--p", i < vIndex ? 1 : i === vIndex ? (vStand - i).toFixed(3) : 0);
    });
    // erste 70 % eines Schritts laeuft die Messung, danach bleibt das Ergebnis stehen
    maleMessung(vIndex, ruhig.matches ? 1 : grenze((vStand - vIndex) / 0.7));

    // --- KI-Satz ---
    var aRect = $("#ai").getBoundingClientRect();
    var aStand = grenze((hoch * 0.85 - aRect.top) / (aRect.height * 0.62));
    licht.forEach(function (wort, i) {
      wort.classList.toggle("lit", i < aStand * (licht.length + 1));
    });

    // --- Wolken und 3D-Objekt ---
    var oben = grenze(scrollY / hoch);
    var vRect = vAbschnitt.getBoundingClientRect();
    var tRect = $("#table").getBoundingClientRect();
    var farbe;
    if (aRect.top < hoch * 0.7) farbe = FARBEN.ki;
    else if (tRect.top < hoch * 0.6) farbe = FARBEN.tabelle;
    else if (vRect.top < hoch * 0.5) farbe = FARBEN.vergleich[vIndex % 2];
    else if (fRect.top < hoch * 0.6) farbe = FARBEN.funktionen[Math.max(0, fIndex)];
    else farbe = FARBEN.start;
    szene.A = farbe[0];
    szene.B = farbe[1];
    var amEnde = grenze((hoch - aRect.top) / hoch);
    if (amEnde > 0) {
      // am Schluss kommt das Symbol zurueck und setzt sich wieder zusammen
      szene.V = weich(amEnde);
      szene.S = 1 - weich(aStand);
      szene.K = schmal ? 0.46 : 0.8;
      szene.O = schmal ? [seite * 0.3, -0.6] : [seite * 0.64, -0.05];
    } else {
      // am Anfang schwebt es neben der Ueberschrift und zerfaellt beim Wegscrollen
      szene.V = 1 - weich(grenze((oben - 0.45) / 0.5));
      szene.S = weich(oben);
      szene.K = schmal ? 0.5 : 0.82;
      szene.O = schmal ? [seite * 0.32, 0.56 + oben * 0.5] : [seite * 0.56, 0.04 + oben * 0.7];
    }

    if (
      !ruhig.matches &&
      Math.abs(fZiel - nach.f) + Math.abs(vZiel - nach.v) + Math.abs(maus[0] - nach.mx) + Math.abs(maus[1] - nach.my) > 0.0006
    )
      anfordern();
  }
  function anfordern() {
    if (!bild) bild = requestAnimationFrame(zeichne);
  }
  // gewaehlten Knopf in der Leiste sichtbar halten, ohne die Seite zu bewegen
  function zeigeInLeiste(knopf) {
    var leiste = knopf.parentElement;
    if (leiste.scrollWidth <= leiste.clientWidth) return;
    leiste.scrollTo({ left: knopf.offsetLeft - (leiste.clientWidth - knopf.offsetWidth) / 2, behavior: "smooth" });
  }
  // Klick oder Pfeiltaste in der Leiste springt an die passende Stelle der Szene.
  function verbinde(abschnitt, knoepfe) {
    function springe(i, fokus) {
      var strecke = abschnitt.offsetHeight - innerHeight;
      scrollTo({ top: abschnitt.offsetTop + ((i + 0.4) / knoepfe.length) * strecke, behavior: ruhig.matches ? "auto" : "smooth" });
      if (fokus) knoepfe[i].focus({ preventScroll: true });
    }
    knoepfe.forEach(function (knopf, i) {
      knopf.addEventListener("click", function () {
        springe(i, false);
      });
      knopf.addEventListener("keydown", function (e) {
        var ziel =
          e.key === "ArrowRight"
            ? (i + 1) % knoepfe.length
            : e.key === "ArrowLeft"
              ? (i + knoepfe.length - 1) % knoepfe.length
              : e.key === "Home"
                ? 0
                : e.key === "End"
                  ? knoepfe.length - 1
                  : -1;
        if (ziel !== -1) {
          e.preventDefault();
          springe(ziel, true);
        }
      });
    });
  }
  verbinde(fAbschnitt, fKnoepfe);
  verbinde(vAbschnitt, vKnoepfe);
  addEventListener("scroll", anfordern, { passive: true });
  addEventListener("resize", anfordern, { passive: true });
  addEventListener(
    "pointermove",
    function (e) {
      if (e.pointerType !== "mouse" || ruhig.matches) return;
      maus = [e.clientX / innerWidth - 0.5, e.clientY / innerHeight - 0.5];
      anfordern();
    },
    { passive: true },
  );
  // nach einem Sprachwechsel: Woerter neu teilen, Werte neu schreiben
  function male() {
    teileLicht();
    vIndex = -1;
    anfordern();
  }
  bereit = true;
  male();

  // Tabelle: Zeilen blenden nacheinander ein.
  var ziele = $$("[data-reveal], tbody tr, tfoot tr");
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
