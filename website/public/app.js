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
    $("#spec-ver").textContent = release.version;
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
      "EasyMySQL – Datenbank, SQL & KI",
      "EasyMySQL – Database, SQL & AI",
    );
    $('meta[name="description"]').content = text(
      "MariaDB-Server, SQL-Editor, ER-Diagramme und KI-Anbindung in einer nativen App. Kostenlos. Direkte Downloads für Windows, macOS und Linux.",
      "MariaDB server, SQL editor, ER diagrams and AI in one native app. Free. Direct downloads for Windows, macOS and Linux.",
    );
    zeigeRelease();
    zeigeFehler();
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

  // Tabs: Pfeiltasten, Home/End und eine einzige Tabposition pro Gruppe.
  var tabs = $$("[role=tab]");
  function waehleTab(tab, fokus) {
    tabs.forEach(function (el) {
      var aktiv = el === tab;
      el.setAttribute("aria-selected", String(aktiv));
      el.tabIndex = aktiv ? 0 : -1;
      document.getElementById(el.getAttribute("aria-controls")).hidden = !aktiv;
    });
    if (fokus) {
      tab.focus({ preventScroll: true });
      tab.scrollIntoView({
        block: "nearest",
        inline: "nearest",
        behavior: "instant",
      });
    }
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
