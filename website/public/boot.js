// Vor dem ersten Zeichnen: Sprache setzen, damit nichts aufblitzt.
(function () {
  var d = document.documentElement, l;
  try { l = localStorage.getItem("em-lang"); } catch (e) {}
  if (!l) l = /^de\b/i.test(navigator.language || "") ? "de" : "en";
  d.setAttribute("lang", l === "en" ? "en" : "de");
  d.className += " js";
})();
