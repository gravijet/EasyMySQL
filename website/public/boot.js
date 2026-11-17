// Vor dem ersten Zeichnen: Sprache setzen, damit nichts aufblitzt.
(function () {
  var d = document.documentElement, l;
  // /en/ ist immer Englisch, / ohne gespeicherte Wahl immer Deutsch (so sehen es auch Suchmaschinen)
  var p = location.pathname;
  try { l = localStorage.getItem("em-lang"); } catch (e) {}
  if (/^\/en(\/|$)/.test(p)) l = "en";
  else if (!l) l = p === "/" ? "de" : /^de\b/i.test(navigator.language || "") ? "de" : "en";
  d.setAttribute("lang", l === "en" ? "en" : "de");
  d.className += " js";
})();
