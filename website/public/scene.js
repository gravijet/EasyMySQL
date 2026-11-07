/* Vollbild-zu-Vorschau und Feature-Fortschritt aus der eigenen WinMorph-Vorlage.
   Quelle: github.com/gravijet/WinMorph, website/public/app.js (07.11.2026).
   Keine automatische Bildrotation; Scrollen und Tasten steuern dieselbe Ansicht. */
(function () {
  "use strict";
  var wurzel = document.documentElement;
  var szene = document.querySelector(".morph");
  if (!szene) return;
  var pin = szene.querySelector(".morph-pin");
  var held = document.getElementById("hero");
  var tabs = Array.from(szene.querySelectorAll("[role=tab]"));
  var bilder = Array.from(szene.querySelectorAll(".screenshot-link"));
  var weniger = matchMedia("(prefers-reduced-motion: reduce)");
  var flach = matchMedia("(max-height: 500px) and (min-aspect-ratio: 1/1)");
  var farben = [
    "#537bdf",
    "#337e7c",
    "#9a6e39",
    "#745ea8",
    "#467a55",
    "#446682",
  ];
  var index = 0,
    rahmen = 0,
    x = 0,
    y = 0,
    sx = 0,
    sy = 0;
  function begrenze(wert) {
    return Math.min(1, Math.max(0, wert));
  }
  function weich(t) {
    return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
  }
  function grenzen() {
    return {
      hoehe: innerHeight,
      vorlauf: innerHeight * 0.65,
      strecke: Math.max(1, szene.offsetHeight - innerHeight),
    };
  }
  function zeichne() {
    rahmen = 0;
    document.querySelector(".nav").classList.toggle("solid", scrollY > 80);
    if (flach.matches) {
      held.inert = false;
      bilder.forEach(function (bild) {
        bild.style.removeProperty("pointer-events");
      });
      return;
    }
    var r = szene.getBoundingClientRect(),
      g = grenzen();
    if (r.bottom < 0 || r.top > innerHeight) return;
    var weg = Math.max(0, -r.top),
      k = begrenze(weg / g.vorlauf),
      e = weniger.matches ? (k > 0.5 ? 1 : 0) : weich(k);
    var aktuell = bilder.find(function (bild) {
      return !bild.closest("figure").hidden;
    });
    var breite = aktuell.offsetWidth,
      hoehe = breite / 1.6;
    var deckung = Math.max(innerWidth / breite, innerHeight / hoehe) * 1.04;
    var skalierung = deckung + (1 - deckung) * e;
    var mitte = aktuell.offsetTop;
    var dy = (innerHeight * 0.5 - mitte) * (1 - e);
    sx += (x - sx) * 0.12;
    sy += (y - sy) * 0.12;
    pin.style.setProperty("--scale", skalierung.toFixed(5));
    pin.style.setProperty("--dy", dy.toFixed(2) + "px");
    pin.style.setProperty("--rx", (-sy * 3 * e).toFixed(2) + "deg");
    pin.style.setProperty("--ry", (sx * 4 * e).toFixed(2) + "deg");
    pin.style.setProperty("--veil", (1 - e).toFixed(4));
    pin.style.setProperty(
      "--after",
      (weniger.matches ? e : begrenze((k - 0.65) / 0.35)).toFixed(4),
    );
    held.style.opacity = (
      weniger.matches ? 1 - e : 1 - begrenze(k * 1.8)
    ).toFixed(4);
    held.style.transform =
      "translateY(" + (weniger.matches ? 0 : -k * 60).toFixed(2) + "px)";
    held.inert = weniger.matches ? e === 1 : k > 0.55;
    var p = begrenze((weg - g.vorlauf) / (g.strecke - g.vorlauf));
    var fortschritt = Math.min(tabs.length - 0.00001, p * tabs.length),
      neu = Math.floor(fortschritt);
    tabs.forEach(function (tab, i) {
      tab.style.setProperty(
        "--p",
        i < neu ? 1 : i === neu ? fortschritt - i : 0,
      );
    });
    if (neu !== index) {
      index = neu;
      wurzel.style.setProperty("--scene", farben[index]);
      document.dispatchEvent(
        new CustomEvent("easymysql-scene-feature", { detail: index }),
      );
    }
    bilder.forEach(function (bild) {
      bild.style.pointerEvents = k > 0.85 ? "" : "none";
    });
    if (Math.abs(x - sx) + Math.abs(y - sy) > 0.001) anfordern();
  }
  function anfordern() {
    if (!rahmen) rahmen = requestAnimationFrame(zeichne);
  }
  document.addEventListener("easymysql-feature", function (ereignis) {
    if (flach.matches) return;
    index = ereignis.detail;
    wurzel.style.setProperty("--scene", farben[index]);
    var g = grenzen();
    scrollTo({
      top:
        szene.offsetTop +
        g.vorlauf +
        ((index + 0.15) / tabs.length) * (g.strecke - g.vorlauf),
      behavior: "instant",
    });
    anfordern();
  });
  document.querySelectorAll('a[href="#features"]').forEach(function (link) {
    link.addEventListener("click", function (ereignis) {
      if (weniger.matches || flach.matches) return;
      ereignis.preventDefault();
      var g = grenzen();
      scrollTo({ top: szene.offsetTop + g.vorlauf + 10, behavior: "smooth" });
    });
  });
  addEventListener("scroll", anfordern, { passive: true });
  addEventListener("resize", anfordern, { passive: true });
  addEventListener(
    "pointermove",
    function (ereignis) {
      if (ereignis.pointerType !== "mouse" || weniger.matches) return;
      x = ereignis.clientX / innerWidth - 0.5;
      y = ereignis.clientY / innerHeight - 0.5;
      anfordern();
    },
    { passive: true },
  );
  weniger.addEventListener("change", function () {
    if (weniger.matches) {
      held.style.removeProperty("opacity");
      held.style.removeProperty("transform");
      held.inert = false;
      bilder.forEach(function (bild) {
        bild.style.removeProperty("pointer-events");
      });
    }
    anfordern();
  });
  document.fonts.ready.then(anfordern);
  anfordern();
})();
