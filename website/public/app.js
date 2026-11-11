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
    baueAlleZiffern();
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

  // Tabs: Pfeiltasten, Home/End, eine Tabposition. Die gelbe Flaeche gleitet zum gewaehlten Tab,
  // die Bilder liegen uebereinander und gleiten weich aneinander vorbei.
  var tabs = $$("[role=tab]");
  var felder = tabs.map(function (tab) {
    return document.getElementById(tab.getAttribute("aria-controls"));
  });
  var gleiter = $(".glider");
  function setzeGleiter() {
    var tab = $("[role=tab][aria-selected=true]");
    gleiter.style.width = tab.offsetWidth + "px";
    gleiter.style.translate = tab.offsetLeft + "px 0";
  }
  function waehleTab(tab, fokus) {
    var jetzt = tabs.indexOf(tab);
    tabs.forEach(function (el, i) {
      var aktiv = el === tab;
      el.setAttribute("aria-selected", String(aktiv));
      el.tabIndex = aktiv ? 0 : -1;
      felder[i].hidden = false;
      felder[i].inert = !aktiv;
      felder[i].setAttribute("aria-hidden", String(!aktiv));
      felder[i].classList.toggle("on", aktiv);
      felder[i].classList.toggle("before", i < jetzt);
      felder[i].classList.toggle("after", i > jetzt);
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
  waehleTab(tabs[0], false);
  addEventListener("resize", setzeGleiter, { passive: true });
  if (document.fonts) document.fonts.ready.then(setzeGleiter);

  // Rollende Ziffern: jede Stelle ist eine Trommel mit zehn Flaechen.
  function baueZiffern(el) {
    // Ausgangstext merken: nach dem Aufbau besteht der Inhalt aus allen Trommelflaechen
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
  // Einblenden beim Scrollen: Geschwister nacheinander; Balken wachsen, Ziffern rollen.
  function zeige(el) {
    el.classList.add("in");
    el.querySelectorAll("[data-w]").forEach(function (balken, i) {
      setTimeout(function () {
        balken.style.setProperty("--w", balken.dataset.w);
      }, 350 + i * 160);
    });
    el.querySelectorAll("[data-roll]").forEach(rolle);
    if (el.classList.contains("tile"))
      setTimeout(function () {
        el.classList.add("done");
      }, 1700);
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

  // Scrollen und Maus, alles weich nachgefuehrt: die dunkle Buehne waechst aus einer runden Karte,
  // das App-Fenster richtet sich auf, die Diagramme drehen sich leicht mit.
  var flaeche = $(".stage");
  var buehne = $(".feature-panels");
  var szenen = $$(".stat");
  var maus = [0, 0];
  var weich = { x: 0, y: 0, k: 1, g: 0 };
  var bild = 0;
  function grenze(wert) {
    return Math.min(1, Math.max(0, wert));
  }
  function zeichne() {
    bild = 0;
    if (ruhig.matches) return;
    var hoch = innerHeight;
    // g: 0 = Buehne kommt gerade ins Bild, 1 = fuellt die Breite
    var g = grenze((hoch - flaeche.getBoundingClientRect().top) / (hoch * 0.75));
    // k: 1 = Fenster kommt unten ins Bild, 0 = oben angekommen
    var k = grenze((buehne.getBoundingClientRect().top - hoch * 0.2) / (hoch * 0.7));
    weich.x += (maus[0] - weich.x) * 0.07;
    weich.y += (maus[1] - weich.y) * 0.07;
    weich.k += (k - weich.k) * 0.12;
    weich.g += (g - weich.g) * 0.12;
    flaeche.style.setProperty("--grow", weich.g.toFixed(4));
    buehne.style.setProperty("--rx", (weich.k * 22 - weich.y * 3).toFixed(2) + "deg");
    buehne.style.setProperty("--ry", (weich.x * 5).toFixed(2) + "deg");
    buehne.style.setProperty("--s", (1 - weich.k * 0.1).toFixed(4));
    szenen.forEach(function (szene) {
      if (!szene.classList.contains("in")) return;
      var s = szene.getBoundingClientRect();
      var mitte = (s.top + s.height / 2) / hoch - 0.5;
      var diagramm = szene.querySelector(".chart3d");
      diagramm.style.setProperty("--cx", (-8 - mitte * 10 - weich.y * 8).toFixed(2) + "deg");
      diagramm.style.setProperty("--cy", (12 + weich.x * 22).toFixed(2) + "deg");
    });
    if (
      Math.abs(maus[0] - weich.x) + Math.abs(maus[1] - weich.y) + Math.abs(k - weich.k) + Math.abs(g - weich.g) >
      0.002
    )
      anfordern();
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
      el.style.setProperty("--ty", (((e.clientX - r.left) / r.width - 0.5) * 16).toFixed(1) + "deg");
      el.style.setProperty("--tx", ((0.5 - (e.clientY - r.top) / r.height) * 16).toFixed(1) + "deg");
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
