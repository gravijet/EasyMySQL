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

  // ---------- 3D-Szene: drei Scheiben, erst Symbol, dann Saeulen des Vergleichs ----------
  function grenze(wert) {
    return Math.min(1, Math.max(0, wert));
  }
  function weich(t) {
    return t * t * (3 - 2 * t);
  }
  function misch(a, b, t) {
    return a + (b - a) * t;
  }
  // Farben der Wolken je Abschnitt (zwei Toene, Werte 0 bis 1)
  var FARBEN = {
    start: [[0.09, 0.2, 0.62], [0.3, 0.1, 0.52]],
    vergleich: [[0.44, 0.1, 0.32], [0.1, 0.16, 0.56]],
    funktionen: [
      [[0.08, 0.22, 0.66], [0.04, 0.36, 0.52]],
      [[0.03, 0.36, 0.4], [0.05, 0.2, 0.5]],
      [[0.5, 0.26, 0.06], [0.42, 0.1, 0.22]],
      [[0.34, 0.14, 0.6], [0.1, 0.16, 0.56]],
      [[0.1, 0.4, 0.22], [0.05, 0.26, 0.38]],
      [[0.14, 0.24, 0.42], [0.36, 0.16, 0.3]],
    ],
    tabelle: [[0.05, 0.08, 0.18], [0.04, 0.1, 0.14]],
    ki: [[0.06, 0.34, 0.3], [0.1, 0.18, 0.58]],
  };
  var STAPEL = [
    [0, 0.5, 0.15],
    [0, 0, 0.15],
    [0, -0.5, 0.15],
  ];
  var szene = (window.easyScene = {
    V: 1,
    K: 0.82,
    O: [0.9, 0],
    C: 0,
    G: 0,
    W: 2.2,
    D: JSON.parse(JSON.stringify(STAPEL)),
    A: FARBEN.start[0],
    B: FARBEN.start[1],
  });

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
  var ABSTAND = 2.3; // Abstand der Saeulen im Raum des Objekts
  var BODEN = -1.1; // Standflaeche der Saeulen
  var NEIGUNG = -0.42; // wie in gl.js (lage) fuer das Diagramm
  var vAbschnitt = $("#compare");
  var wahl = $$(".picks button");
  var spalten = $$(".col").map(function (el) {
    return { el: el, wert: el.querySelector("b") };
  });
  var messung = 0;
  var selbstGewaehlt = false;
  function zahl(wert, einheit) {
    var stellen = einheit === "MB" ? 0 : wert < 1 ? 2 : 1;
    return (
      new Intl.NumberFormat(sprache, { minimumFractionDigits: stellen, maximumFractionDigits: stellen }).format(wert) +
      " " +
      einheit
    );
  }
  // halbe Hoehe der Saeule i fuer die gewaehlte Messung
  function hoehe(i) {
    var m = MESSUNGEN[messung];
    if (m.w[i] === null) return 0.11;
    return 0.13 + 0.82 * (m.w[i] / Math.max.apply(null, m.w.filter(Boolean)));
  }
  var zaehler = 0;
  function waehleMessung(index, weichZaehlen) {
    var vorher = MESSUNGEN[messung];
    messung = index;
    var m = MESSUNGEN[index];
    wahl.forEach(function (knopf, i) {
      knopf.setAttribute("aria-pressed", String(i === index));
    });
    var meiner = ++zaehler;
    var beginn = performance.now();
    (function schritt(jetzt) {
      if (meiner !== zaehler) return;
      var t = weichZaehlen && !ruhig.matches ? grenze((jetzt - beginn) / 700) : 1;
      spalten.forEach(function (spalte, i) {
        var w = m.w[i];
        spalte.el.classList.toggle("na", w === null);
        if (w === null) {
          spalte.wert.textContent = text(m.g[i][0], m.g[i][1]);
          return;
        }
        // von der alten zur neuen Zahl zaehlen, wenn die Einheit gleich bleibt
        var von = vorher.e === m.e && vorher.w[i] !== null ? vorher.w[i] : 0;
        spalte.wert.textContent = zahl(t === 1 ? w : misch(von, w, weich(t)), m.e);
      });
      if (t < 1) requestAnimationFrame(schritt);
    })(beginn);
    document.dispatchEvent(new Event("easymysql-scene"));
    anfordern();
  }
  wahl.forEach(function (knopf, i) {
    knopf.addEventListener("click", function () {
      selbstGewaehlt = true;
      waehleMessung(i, true);
    });
  });

  // Funktionen: Stapel der App-Fenster.
  var fAbschnitt = $("#features");
  var tabs = $$("[role=tab]");
  var blaetter = tabs.map(function (tab) {
    return document.getElementById(tab.getAttribute("aria-controls"));
  });
  var blatt = 0;
  var selbstGeblaettert = false;
  function waehleBlatt(index, fokus) {
    blatt = index;
    tabs.forEach(function (tab, i) {
      tab.setAttribute("aria-selected", String(i === index));
      tab.tabIndex = i === index ? 0 : -1;
      blaetter[i].classList.toggle("on", i === index);
      blaetter[i].setAttribute("aria-hidden", String(i !== index));
    });
    if (fokus) tabs[index].focus({ preventScroll: true });
    anfordern();
  }
  tabs.forEach(function (tab, i) {
    function nimm() {
      selbstGeblaettert = true;
      waehleBlatt(i, false);
    }
    tab.addEventListener("click", nimm);
    tab.addEventListener("pointerenter", function (e) {
      if (e.pointerType === "mouse") nimm();
    });
    blaetter[i].addEventListener("click", nimm);
    tab.addEventListener("keydown", function (e) {
      var ziel =
        e.key === "ArrowDown" || e.key === "ArrowRight"
          ? (i + 1) % tabs.length
          : e.key === "ArrowUp" || e.key === "ArrowLeft"
            ? (i + tabs.length - 1) % tabs.length
            : e.key === "Home"
              ? 0
              : e.key === "End"
                ? tabs.length - 1
                : -1;
      if (ziel !== -1) {
        e.preventDefault();
        selbstGeblaettert = true;
        waehleBlatt(ziel, true);
      }
    });
  });
  waehleBlatt(0, false);

  // Solange niemand selbst waehlt, laufen Messungen und Fenster von allein weiter.
  function imBild(el) {
    var r = el.getBoundingClientRect();
    return r.top < innerHeight * 0.6 && r.bottom > innerHeight * 0.4;
  }
  setInterval(function () {
    if (ruhig.matches || document.hidden) return;
    if (!selbstGewaehlt && imBild(vAbschnitt)) waehleMessung((messung + 1) % MESSUNGEN.length, true);
    if (!selbstGeblaettert && imBild(fAbschnitt)) waehleBlatt((blatt + 1) % tabs.length, false);
  }, 3400);

  // Klick auf das Symbol oben laesst die Scheiben kurz auseinanderspringen.
  var stoss = 0;
  $("#top").addEventListener("pointerdown", function (e) {
    if (e.target.closest("a, button, code")) return;
    stoss = 1;
    anfordern();
  });

  var bild = 0;
  function zeichne() {
    bild = 0;
    var hoch = innerHeight;
    var seite = innerWidth / hoch;
    var schmal = seite < 1;
    var vRect = vAbschnitt.getBoundingClientRect();
    var fRect = fAbschnitt.getBoundingClientRect();
    var tRect = $("#table").getBoundingClientRect();
    var aRect = $("#ai").getBoundingClientRect();

    // c: 0 = Symbol oben, 1 = Saeulen im Vergleich
    var c = weich(grenze(1 - vRect.top / hoch));
    stoss *= 0.93;
    // Ankerpunkte auf der Seite (in Pixeln ab Seitenanfang), dazwischen gleitet das Objekt
    var obenY = hoch * 0.47;
    var saeulenY = scrollY + vRect.top + vRect.height * (schmal ? 0.6 : 0.58);
    var obenX = schmal ? seite * 0.32 : seite * 0.56;
    var saeulenX = schmal ? 0 : seite * 0.2;
    var obenK = schmal ? 0.5 : 0.82;
    var saeulenK = Math.min(0.4, seite * 0.38);
    var amEnde = weich(grenze((hoch * 0.9 - aRect.top) / (hoch * 0.6)));
    var k, x, yPx;
    if (amEnde > 0) {
      // ganz unten kommt das Symbol zurueck
      c = 0;
      k = schmal ? 0.42 : 0.7;
      x = schmal ? 0 : seite * 0.5;
      yPx = scrollY + aRect.top + aRect.height * (schmal ? 0.74 : 0.5);
      szene.V = amEnde;
    } else {
      k = misch(obenK, saeulenK, c);
      x = misch(obenX, saeulenX, c);
      yPx = misch(obenY, saeulenY, c);
      // hinter dem Vergleich verschwindet es
      szene.V = grenze(vRect.bottom / (hoch * 0.55));
    }
    szene.K = k;
    szene.C = c;
    szene.G = c;
    szene.W = misch(2.2, 5.4, c);
    szene.O = [x, 1 - (2 * (yPx - scrollY)) / hoch];
    for (var i = 0; i < 3; i++) {
      var h = hoehe(i);
      szene.D[i][0] = misch(0, (i - 1) * ABSTAND, c);
      szene.D[i][1] = misch(STAPEL[i][1] * (1 + stoss * 1.6), BODEN + h, c);
      szene.D[i][2] = misch(0.15, h, c);
    }

    // Beschriftung unter jeder Saeule: gleiche Projektion wie in gl.js
    var wy = BODEN * Math.cos(NEIGUNG);
    var wz = -BODEN * Math.sin(NEIGUNG);
    var p = 3.1 / (5 - wz);
    spalten.forEach(function (spalte, i) {
      var px = innerWidth / 2 + (saeulenX + saeulenK * (i - 1) * ABSTAND * p) * (hoch / 2);
      // Unterkante der Saeule: Standflaeche plus der nach vorn geneigte Rand
      var py = vRect.height * (schmal ? 0.6 : 0.58) - saeulenK * (wy - 0.9 * Math.sin(-NEIGUNG)) * p * (hoch / 2);
      spalte.el.style.setProperty("--x", px.toFixed(1) + "px");
      spalte.el.style.setProperty("--y", (py + hoch * 0.035).toFixed(1) + "px");
      spalte.el.style.setProperty("--o", grenze((c - 0.8) / 0.2).toFixed(3));
    });

    var farbe;
    if (aRect.top < hoch * 0.8) farbe = FARBEN.ki;
    else if (tRect.top < hoch * 0.6) farbe = FARBEN.tabelle;
    else if (fRect.top < hoch * 0.55) farbe = FARBEN.funktionen[blatt];
    else if (vRect.top < hoch * 0.55) farbe = FARBEN.vergleich;
    else farbe = FARBEN.start;
    szene.A = farbe[0];
    szene.B = farbe[1];
    if (stoss > 0.01 && !ruhig.matches) anfordern();
  }
  function anfordern() {
    if (!bild) bild = requestAnimationFrame(zeichne);
  }
  addEventListener("scroll", anfordern, { passive: true });
  addEventListener("resize", anfordern, { passive: true });
  // nach einem Sprachwechsel die Werte neu schreiben
  function male() {
    waehleMessung(messung, false);
  }
  bereit = true;
  male();

  // Tabelle und Schlusssatz blenden ein.
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
