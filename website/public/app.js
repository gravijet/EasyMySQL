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

  // Schalter: der helle Block gleitet unter das gewaehlte Wort (Lage als --l und --r).
  function gleite(schalter) {
    var knopf = schalter.querySelector('[aria-checked="true"]');
    if (!knopf) return;
    var links = knopf.offsetLeft;
    var alt = parseFloat(schalter.style.getPropertyValue("--l"));
    if (!isNaN(alt) && alt !== links) schalter.dataset.dir = links > alt ? "r" : "l";
    schalter.style.setProperty("--l", links + "px");
    schalter.style.setProperty("--r", schalter.clientWidth - links - knopf.offsetWidth + "px");
    // erst nach der ersten Lage sichtbar und beweglich, sonst faehrt er beim Laden herein
    requestAnimationFrame(function () {
      schalter.classList.add("ready");
    });
  }
  function gleiteAlle() {
    $$(".switch").forEach(gleite);
  }
  addEventListener("resize", gleiteAlle);
  if (document.fonts) document.fonts.ready.then(gleiteAlle);

  // Terminal: Befehl je Betriebssystem, steht sofort da.
  var systeme = $$("[data-install-os]");
  var system = "win";
  function zeigeBefehl() {
    var windows = system === "win";
    var befehl = windows
      ? 'powershell -c "irm https://mysql.benjaminberger.at/install | iex"'
      : "curl -fsSL https://mysql.benjaminberger.at/install | sh";
    var feld = $("#hero-cmd");
    if (feld.dataset.full && feld.dataset.full !== befehl && feld.animate && !ruhig.matches)
      feld.animate(
        [
          { opacity: 0, filter: "blur(6px)", transform: "translateY(0.4em)" },
          { opacity: 1, filter: "blur(0)", transform: "none" },
        ],
        { duration: 520, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
      );
    feld.dataset.full = befehl;
    $$(".files li").forEach(function (li) {
      li.classList.toggle("match", li.dataset.os === system);
    });
    $$("[data-prompt]").forEach(function (el) {
      el.textContent = windows ? ">" : "$";
    });
    $$("[data-cmd]").forEach(function (el) {
      el.textContent = befehl;
    });
  }
  function waehleSystem(knopf, fokus) {
    system = knopf.dataset.installOs;
    systeme.forEach(function (el) {
      el.setAttribute("aria-checked", String(el === knopf));
      el.tabIndex = el === knopf ? 0 : -1;
    });
    zeigeBefehl();
    gleite(knopf.parentElement);
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
    $$("[data-size]").forEach(function (el) {
      var datei = release.files[el.dataset.size];
      el.textContent = datei && datei.size ? " · " + groesse(datei.size) : "";
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
    $$("[data-lang]").forEach(function (el) {
      el.setAttribute("aria-checked", String(el.dataset.lang === neu));
      el.tabIndex = el.dataset.lang === neu ? 0 : -1;
    });
    gleiteAlle();
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
  var laeuft = false;
  // Sprachwechsel: wo der Browser es kann, loest sich die alte Seite weich in die neue auf
  function wechsle(neu) {
    if (laeuft || neu === sprache) return;
    try {
      localStorage.setItem("em-lang", neu);
    } catch (_) {}
    if (ruhig.matches || !document.body.animate) return setzeSprache(neu);
    // Der Schalter springt sofort um; jeder sichtbare Text verschwimmt, tauscht sich und taucht
    // wieder auf, als Welle, die vom Schalter aus ueber die Seite laeuft.
    laeuft = true;
    $$("[data-lang]").forEach(function (el) {
      el.setAttribute("aria-checked", String(el.dataset.lang === neu));
    });
    gleite($("#lang"));
    var s = $("#lang").getBoundingClientRect();
    var fertig = texte
      .filter(function (p) {
        var r = p[0].getBoundingClientRect();
        return r.width > 2 && r.bottom > 0 && r.top < innerHeight;
      })
      .map(function (p) {
        var el = p[0];
        var r = el.getBoundingClientRect();
        var verzug = Math.min(520, Math.hypot(r.right - s.right, r.top - s.top) * 0.4);
        var weg = el.animate(
          [
            { opacity: 1, filter: "blur(0)", transform: "none" },
            { opacity: 0, filter: "blur(8px)", transform: "translateY(-0.3em) scale(0.98)" },
          ],
          { duration: 260, delay: verzug, easing: "cubic-bezier(0.5, 0, 0.75, 0)", fill: "forwards" },
        );
        return weg.finished.then(function () {
          el.textContent = neu === "en" ? el.dataset.en : p[1];
          el.animate(
            [
              { opacity: 0, filter: "blur(8px)", transform: "translateY(0.35em) scale(0.98)" },
              { opacity: 1, filter: "blur(0)", transform: "none" },
            ],
            { duration: 640, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
          );
          weg.cancel();
        });
      });
    Promise.all(fertig).then(function () {
      laeuft = false;
      setzeSprache(neu);
    });
  }
  $$("[data-lang]").forEach(function (knopf) {
    knopf.addEventListener("click", function () {
      wechsle(knopf.dataset.lang);
    });
    knopf.addEventListener("keydown", function (e) {
      if (!/^Arrow/.test(e.key)) return;
      e.preventDefault();
      var andere = $('[data-lang]:not([data-lang="' + sprache + '"])');
      wechsle(andere.dataset.lang);
      andere.focus();
    });
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

  // ---------- Buehne: was als Relief in der Fluessigkeit steht und welche Farben sich spiegeln ----------
  var FARBEN = {
    start: [[0.25, 0.45, 1], [0.7, 0.25, 0.95]],
    ruhig: [[0.14, 0.2, 0.45], [0.1, 0.26, 0.34]],
    funktionen: [
      [[0.25, 0.5, 1], [0.1, 0.8, 0.8]],
      [[0.1, 0.8, 0.75], [0.2, 0.4, 1]],
      [[1, 0.6, 0.15], [0.9, 0.2, 0.45]],
      [[0.65, 0.35, 1], [0.25, 0.4, 1]],
      [[0.25, 0.9, 0.5], [0.1, 0.6, 0.8]],
      [[0.4, 0.6, 0.9], [0.8, 0.4, 0.65]],
    ],
  };
  var szene = (window.easyScene = { n: 0, text: null, hoch: 1, A: FARBEN.start[0], B: FARBEN.start[1] });
  var letzterText = "";
  var traeger = null; // Abschnitt, an dem das Relief haengt
  var tabelle = $(".compare table");
  var reliefs = $$("[data-relief]");
  var funktionen = $$(".feature");
  var bilder = funktionen.map(function (el) {
    return { el: el.querySelector("img"), k: 1 };
  });

  // Text, Lage und Groesse des Reliefs fuer den Abschnitt in der Bildmitte.
  function relief(art) {
    var schmal = innerWidth < innerHeight;
    if (art === "start")
      return schmal ? { zeilen: ["Easy", "MySQL"], h: 0.14, y: 0.27, anteil: 0.8 } : { zeilen: ["EasyMySQL"], h: 0.36, y: 0.36, anteil: 0.7 };
    return null;
  }
  function stelle() {
    var mitte = innerHeight / 2;
    var art = null;
    reliefs.forEach(function (el) {
      var r = el.getBoundingClientRect();
      if (r.top <= mitte && r.bottom > mitte) {
        art = el.dataset.relief;
        traeger = el;
      }
    });
    var farbe = art ? FARBEN[art] : FARBEN.ruhig;
    funktionen.forEach(function (el) {
      var r = el.getBoundingClientRect();
      if (r.top <= mitte && r.bottom > mitte) farbe = FARBEN.funktionen[+el.dataset.farbe];
    });
    var spez = relief(art);
    if (spez) {
      spez.x = Math.min(60, Math.max(20, innerWidth * 0.04)) / innerWidth;
      spez.breit = (1 - 2 * spez.x) * spez.anteil;
    }
    szene.A = farbe[0];
    szene.B = farbe[1];
    szene.hoch = spez ? 1 : 0;
    // das Relief scrollt mit seinem Abschnitt mit, die Fluessigkeit zieht langsamer nach
    szene.versatz = traeger ? -traeger.getBoundingClientRect().top / innerHeight : 0;
    szene.lauf = scrollY / innerHeight;
    if (tabelle) {
      var lage = tabelle.getBoundingClientRect();
      tabelle.classList.toggle("stuck", lage.top <= 68 && lage.bottom > 68 + tabelle.tHead.offsetHeight);
    }
    // ohne Relief bleibt der letzte Text stehen und versinkt nur (hoch = 0)
    var kennung = spez ? JSON.stringify(spez) + innerWidth + "x" + innerHeight : letzterText;
    if (kennung !== letzterText) {
      letzterText = kennung;
      szene.text = spez;
      szene.n++;
    }
    document.dispatchEvent(new Event("easymysql-scene"));
  }

  // App-Bilder richten sich beim Hereinscrollen auf, weich nachgefuehrt.
  var bild = 0;
  function zeichne() {
    bild = 0;
    var hoch = innerHeight;
    var unruhig = false;
    bilder.forEach(function (b) {
      var r = b.el.parentElement.getBoundingClientRect();
      // 1 = Bild kommt unten ins Sichtfeld, 0 = steht in der Mitte
      var ziel = ruhig.matches ? 0 : Math.min(1, Math.max(0, (r.top - hoch * 0.22) / (hoch * 0.75)));
      b.k += (ziel - b.k) * 0.12;
      if (Math.abs(ziel - b.k) > 0.002) unruhig = true;
      b.el.style.setProperty("--k", b.k.toFixed(4));
    });
    if (unruhig) anfordern();
  }
  function anfordern() {
    if (!bild) bild = requestAnimationFrame(zeichne);
  }
  addEventListener(
    "scroll",
    function () {
      stelle();
      anfordern();
    },
    { passive: true },
  );
  addEventListener("resize", function () {
    stelle();
    anfordern();
  });

  // Zahlen zaehlen hoch, sobald sie sichtbar werden.
  function zaehle(el) {
    var ziel = parseFloat(el.dataset.count);
    var stellen = +el.dataset.dec;
    var ende = el.textContent;
    var anhang = ende.replace(/^[\d.,]+/, "");
    var beginn = performance.now();
    (function schritt(jetzt) {
      var t = Math.min(1, (jetzt - beginn) / 1700);
      if (t === 1) {
        el.textContent = ende;
        return;
      }
      el.textContent =
        new Intl.NumberFormat(sprache, { minimumFractionDigits: stellen, maximumFractionDigits: stellen }).format(
          ziel * (1 - Math.pow(1 - t, 4)),
        ) + anhang;
      requestAnimationFrame(schritt);
    })(beginn);
  }
  function zeige(el) {
    el.classList.add("in");
    el.querySelectorAll("[data-w]").forEach(function (balken) {
      balken.style.setProperty("--w", balken.dataset.w);
    });
    if (!ruhig.matches) el.querySelectorAll("[data-count]").forEach(zaehle);
  }
  var ziele = $$(".rv, .feature > div, tbody tr, tfoot tr");
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
      { threshold: 0.12, rootMargin: "0px 0px -6% 0px" },
    );
    ziele.forEach(function (el) {
      beobachter.observe(el);
    });
  } else ziele.forEach(zeige);

  // nach einem Sprachwechsel das Relief neu schreiben
  function male() {
    stelle();
  }
  bereit = true;
  stelle();
  anfordern();

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
      var befehl = knopf.querySelector("code");
      if (befehl && window.getSelection) getSelection().selectAllChildren(befehl);
      if (navigator.clipboard)
        navigator.clipboard.writeText(inhalt).then(
          function () {
            var schild = knopf.querySelector(".copy span");
            knopf.classList.add("done");
            schild.textContent = text("Kopiert", "Copied");
            clearTimeout(knopf.zeit);
            knopf.zeit = setTimeout(function () {
              knopf.classList.remove("done");
              schild.textContent = text("Kopieren", "Copy");
            }, 1900);
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
