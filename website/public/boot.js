// Vor dem ersten Zeichnen: Design und Sprache setzen, damit nichts aufblitzt.
(function () {
  var d = document.documentElement, t = "dark", l;
  try { t = localStorage.getItem("em-theme") || t; l = localStorage.getItem("em-lang"); } catch (e) {}
  if (!l) l = /^de\b/i.test(navigator.language || "") ? "de" : "en";
  d.setAttribute("data-theme", t === "light" ? "light" : "dark");
  d.setAttribute("lang", l === "en" ? "en" : "de");
  d.className += " js";
})();
