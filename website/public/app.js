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
  // deutscher Ausgangstext der rollenden Zahlen, bevor die Sprache gesetzt wird
  $$("[data-roll]").forEach(function (el) {
    el.dataset.de = el.textContent.trim();
  });
  var bereit = false;
  var release = null;
  var ladefehler = false;

  var ruhig = window.matchMedia("(prefers-reduced-motion: reduce)");

  // Terminal: Befehl je Betriebssystem, wird beim Wechsel neu getippt.
  var systeme = $$("[data-install-os]");
  var system = "win";
  var tippen = 0;
  function zeigeBefehl(getippt) {
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
    clearInterval(tippen);
    if (!getippt || ruhig.matches) {
      ziel.textContent = befehl;
      return;
    }
    var n = 0;
    ziel.textContent = "";
    tippen = setInterval(function () {
      ziel.textContent = befehl.slice(0, ++n);
      if (n >= befehl.length) clearInterval(tippen);
    }, 22);
  }
  function waehleSystem(knopf, fokus, getippt) {
    system = knopf.dataset.installOs;
    systeme.forEach(function (el) {
      el.setAttribute("aria-checked", String(el === knopf));
      el.tabIndex = el === knopf ? 0 : -1;
    });
    zeigeBefehl(getippt);
    if (fokus) knopf.focus();
  }
  systeme.forEach(function (knopf, index) {
    knopf.addEventListener("click", function () {
      waehleSystem(knopf, false, true);
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
        waehleSystem(systeme[ziel], true, true);
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
    if (bereit) {
      baueAlleZiffern();
      baueRennen();
    }
    zeigeBefehl(false);
    zeigeRelease();
    zeigeFehler();
  }
  function baueAlleZiffern() {
    $$("[data-roll]").forEach(baueZiffern);
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

  // Runde Auswahlleisten: die blaue Flaeche gleitet zum gewaehlten Knopf.
  function setzeGleiter(leiste) {
    var knopf = leiste.querySelector("[aria-selected=true], [aria-pressed=true]");
    var gleiter = leiste.querySelector(".glider");
    if (!knopf) return;
    gleiter.style.width = knopf.offsetWidth + "px";
    gleiter.style.translate = knopf.offsetLeft + "px 0";
  }
  function zeigeKnopf(knopf) {
    var leiste = knopf.closest(".tabs-scroll");
    leiste.scrollTo({
      left: knopf.offsetLeft - (leiste.clientWidth - knopf.offsetWidth) / 2,
      behavior: ruhig.matches ? "auto" : "smooth",
    });
  }
  function alleGleiter() {
    $$(".pills").forEach(setzeGleiter);
  }
  addEventListener("resize", alleGleiter, { passive: true });
  if (document.fonts) document.fonts.ready.then(alleGleiter);

  // Funktionen: die App-Fenster stehen als Karussell im Raum, das gewaehlte vorn in der Mitte.
  var tabs = $$("[role=tab]");
  var fenster = tabs.map(function (tab) {
    return document.getElementById(tab.getAttribute("aria-controls"));
  });
  function waehleTab(tab, fokus) {
    var mitte = tabs.indexOf(tab);
    tabs.forEach(function (el, i) {
      var d = i - mitte;
      var weit = Math.abs(d);
      el.setAttribute("aria-selected", String(d === 0));
      el.tabIndex = d === 0 ? 0 : -1;
      fenster[i].style.setProperty("--d", d);
      fenster[i].style.setProperty("--a", weit);
      fenster[i].style.zIndex = 10 - weit;
      fenster[i].classList.toggle("on", d === 0);
      fenster[i].classList.toggle("far", weit > 2);
      fenster[i].setAttribute("aria-hidden", String(d !== 0));
    });
    setzeGleiter(tab.parentElement);
    if (fokus) tab.focus({ preventScroll: true });
    zeigeKnopf(tab);
  }
  tabs.forEach(function (tab, index) {
    tab.addEventListener("click", function () {
      waehleTab(tab, false);
    });
    fenster[index].addEventListener("click", function () {
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
  waehleTab(tabs[0], false);

  // Rollende Ziffern: jede Stelle ist eine Trommel mit zehn Flaechen.
  function baueZiffern(el) {
    var inhalt = sprache === "en" && el.dataset.en ? el.dataset.en : el.dataset.de;
    el.setAttribute("aria-label", inhalt);
    el.replaceChildren();
    Array.from(inhalt).forEach(function (zeichen, stelle) {
      var span = document.createElement("span");
      span.setAttribute("aria-hidden", "true");
      if (/\d/.test(zeichen)) {
        span.className = "digit";
        var trommel = document.createElement("span");
        trommel.className = "drum";
        for (var n = 0; n < 10; n++) {
          var flaeche = document.createElement("i");
          flaeche.textContent = n;
          flaeche.style.setProperty("--n", n);
          trommel.append(flaeche);
        }
        // jede Stelle dreht eine Runde mehr als die davor
        trommel.dataset.ziel = +zeichen + 10 * (stelle + 1);
        if (el.dataset.rolled || ruhig.matches) trommel.style.setProperty("--v", trommel.dataset.ziel);
        span.append(trommel);
      } else span.textContent = zeichen;
      el.append(span);
    });
  }
  function rolle(el) {
    el.dataset.rolled = "1";
    el.querySelectorAll(".drum").forEach(function (trommel) {
      trommel.style.setProperty("--v", trommel.dataset.ziel);
    });
  }

  // Rennen: Messwerte in Sekunden fuer EasyMySQL, XAMPP, Workbench; null = geht dort nicht.
  var rennen = [
    { de: "Programm starten", en: "Start the app", t: [0.47, 2.7, 5.7] },
    { de: "Tabelle öffnen", en: "Open a table", t: [0.17, 0.33, 2.1] },
    { de: "In 1 Mio. Zeilen suchen", en: "Search 1M rows", t: [0.2, 0.34, 0.25] },
    {
      de: "ER-Diagramm zeichnen",
      en: "Draw an ER diagram",
      t: [0.22, 0.6, null],
      grund: [null, null, ["nicht vorhanden", "not available"]],
    },
    {
      de: "1 Mio. Zeilen importieren",
      en: "Import 1M rows",
      t: [7.7, null, null],
      grund: [null, ["Datei zu groß", "file too large"], ["nicht vorhanden", "not available"]],
    },
  ];
  var bahnen = $$(".lane");
  var lauf = 0;
  var gewaehlt = 0;
  var rennKnoepfe = [];
  bahnen.forEach(function (bahn) {
    var scheibe = bahn.querySelector(".puck");
    for (var z = 0; z < 10; z++) {
      var lage = document.createElement("s");
      lage.style.setProperty("--z", z);
      scheibe.append(lage);
    }
  });
  function sekunden(wert) {
    return (
      new Intl.NumberFormat(sprache, {
        minimumFractionDigits: wert < 1 ? 2 : 1,
        maximumFractionDigits: wert < 1 ? 2 : 1,
      }).format(wert) + " s"
    );
  }
  // Stellt die Bahnen auf den Stand `anteil` (0 bis 1) des laengsten Laufs.
  function stelleRennen(anteil) {
    var r = rennen[gewaehlt];
    var laengste = Math.max.apply(
      null,
      r.t.filter(function (t) {
        return t !== null;
      }),
    );
    bahnen.forEach(function (bahn, i) {
      var zeit = r.t[i];
      var scheibe = bahn.querySelector(".puck");
      var anzeige = bahn.querySelector(".time");
      if (zeit === null) {
        // bleibt nach dem Start liegen
        var weg = Math.min(1, anteil / 0.14);
        scheibe.style.setProperty("--p", (weg * 0.14).toFixed(4));
        bahn.classList.toggle("fail", weg === 1);
        anzeige.textContent = weg === 1 ? text(r.grund[i][0], r.grund[i][1]) : "";
        return;
      }
      var p = Math.min(1, (anteil * laengste) / zeit);
      bahn.classList.remove("fail");
      scheibe.style.setProperty("--p", p.toFixed(4));
      anzeige.textContent = sekunden(p === 1 ? zeit : Math.max(0.01, zeit * p));
    });
  }
  function starteRennen() {
    var meiner = ++lauf;
    if (ruhig.matches) {
      stelleRennen(1);
      return;
    }
    var beginn = performance.now();
    (function schritt(jetzt) {
      if (meiner !== lauf) return;
      var anteil = Math.min(1, (jetzt - beginn) / 3200);
      stelleRennen(anteil);
      if (anteil < 1) requestAnimationFrame(schritt);
    })(beginn);
  }
  function waehleRennen(index, starten) {
    gewaehlt = index;
    rennKnoepfe.forEach(function (knopf, i) {
      knopf.setAttribute("aria-pressed", String(i === index));
    });
    setzeGleiter($("#race-tabs"));
    if (rennKnoepfe[index]) zeigeKnopf(rennKnoepfe[index]);
    if (starten) starteRennen();
    else {
      lauf++;
      stelleRennen(1);
    }
  }
  // Baut die Knoepfe in der aktuellen Sprache und zeigt das Endergebnis.
  function baueRennen() {
    if (!bahnen.length || !rennKnoepfe) return;
    var leiste = $("#race-tabs");
    rennKnoepfe.forEach(function (knopf) {
      knopf.remove();
    });
    rennKnoepfe = rennen.map(function (r, i) {
      var knopf = document.createElement("button");
      knopf.type = "button";
      knopf.textContent = text(r.de, r.en);
      knopf.addEventListener("click", function () {
        waehleRennen(i, true);
      });
      leiste.append(knopf);
      return knopf;
    });
    waehleRennen(gewaehlt, false);
  }
  $("#replay").addEventListener("click", starteRennen);
  bereit = true;
  baueAlleZiffern();
  baueRennen();

  // Einblenden beim Scrollen: Geschwister nacheinander; Zylinder wachsen, Ziffern rollen, das Rennen startet.
  function zeige(el) {
    el.classList.add("in");
    el.querySelectorAll("[data-h]").forEach(function (zylinder, i) {
      setTimeout(function () {
        zylinder.style.setProperty("--h", zylinder.dataset.h);
      }, 300 + i * 180);
    });
    el.querySelectorAll("[data-roll]").forEach(rolle);
    if (el.id === "race") setTimeout(starteRennen, 500);
    if (el.classList.contains("tile"))
      setTimeout(function () {
        el.classList.add("done");
      }, 2000);
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
      { threshold: 0.08, rootMargin: "0px 0px -8% 0px" },
    );
    ziele.forEach(function (el) {
      beobachter.observe(el);
    });
  } else ziele.forEach(zeige);

  // Scrollen und Maus, weich nachgefuehrt: das Karussell richtet sich auf, die Rennplatte neigt sich mit.
  var karussell = $("#flow");
  var platte = $(".board");
  var maus = [0, 0];
  var weich = { x: 0, y: 0, k: 1 };
  var bild = 0;
  function zeichne() {
    bild = 0;
    $(".nav").classList.toggle("small", scrollY > 60);
    if (ruhig.matches) return;
    var hoch = innerHeight;
    // k: 1 = Karussell kommt unten ins Bild, 0 = oben angekommen
    var k = Math.min(1, Math.max(0, (karussell.getBoundingClientRect().top - hoch * 0.22) / (hoch * 0.7)));
    weich.x += (maus[0] - weich.x) * 0.07;
    weich.y += (maus[1] - weich.y) * 0.07;
    weich.k += (k - weich.k) * 0.12;
    karussell.style.setProperty("--rx", (weich.k * 24 - weich.y * 4).toFixed(2) + "deg");
    karussell.style.setProperty("--ry", (weich.x * 6).toFixed(2) + "deg");
    // auf dem Handy flacher, sonst verdecken die Scheiben die Zeiten
    platte.style.setProperty("--bx", ((innerWidth < 640 ? 16 : 30) - weich.y * 10).toFixed(2) + "deg");
    platte.style.setProperty("--by", (weich.x * 10).toFixed(2) + "deg");
    if (Math.abs(maus[0] - weich.x) + Math.abs(maus[1] - weich.y) + Math.abs(k - weich.k) > 0.002) anfordern();
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
      maus = [e.clientX / innerWidth - 0.5, e.clientY / innerHeight - 0.5];
      anfordern();
    },
    { passive: true },
  );
  anfordern();

  // Kacheln neigen sich zur Maus.
  $$(".tile").forEach(function (el) {
    el.addEventListener("pointermove", function (e) {
      if (e.pointerType !== "mouse" || ruhig.matches) return;
      var r = el.getBoundingClientRect();
      el.style.setProperty("--ty", (((e.clientX - r.left) / r.width - 0.5) * 18).toFixed(1) + "deg");
      el.style.setProperty("--tx", ((0.5 - (e.clientY - r.top) / r.height) * 18).toFixed(1) + "deg");
    });
    el.addEventListener("pointerleave", function () {
      el.style.setProperty("--tx", "0deg");
      el.style.setProperty("--ty", "0deg");
    });
  });

  zeigeBefehl(true);

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
