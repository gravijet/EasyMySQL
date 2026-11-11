/* Hintergrund der Seite: ziehende Farbwolken und drei Scheiben als echtes 3D-Objekt
   (Raymarching). Gestapelt sind sie das Datenbank-Symbol, nebeneinander und verschieden hoch
   sind sie die Saeulen des Vergleichs. app.js gibt ueber window.easyScene vor, wo jede Scheibe
   steht und wie hoch sie ist; hier laeuft alles weich hinterher. */
(function () {
  "use strict";
  var leinwand = document.getElementById("gl");
  if (!leinwand) return;
  var gl = leinwand.getContext("webgl", { antialias: false, alpha: false, powerPreference: "high-performance" });
  if (!gl) {
    document.documentElement.classList.add("no-gl");
    return;
  }
  var ruhig = matchMedia("(prefers-reduced-motion: reduce)");

  var VS = "attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}";
  var FS = `
precision highp float;
uniform vec2 R;      // Groesse in Pixeln
uniform float T;     // Zeit
uniform vec2 M;      // Maus, -0.5 bis 0.5
uniform float V;     // Objekt sichtbar, 0 bis 1
uniform float K;     // Objektgroesse
uniform vec2 O;      // Objektmitte in Bildkoordinaten
uniform float C;     // 0 = Symbol (lebendig, geneigt), 1 = Diagramm (ruhig)
uniform float G;     // Scheibe 2 und 3 grau, 0 bis 1
uniform float W;     // Radius des Bereichs, in dem das Objekt liegt
uniform vec3 D0;     // je Scheibe: x, y, halbe Hoehe
uniform vec3 D1;
uniform vec3 D2;
uniform vec3 A;      // Wolkenfarbe 1
uniform vec3 B;      // Wolkenfarbe 2

float h(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float n(vec2 p){
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3. - 2. * f);
  return mix(mix(h(i), h(i + vec2(1, 0)), f.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), f.x), f.y);
}
float fbm(vec2 p){
  float a = .5, s = 0.;
  for (int i = 0; i < 5; i++) { s += a * n(p); p = p * 2.03 + vec2(1.7, 9.2); a *= .5; }
  return s;
}
vec3 wolken(vec2 uv){
  vec2 p = uv * 1.25 + vec2(T * .012, -T * .008);
  float w = fbm(p + 1.7 * vec2(fbm(p * 1.4 + T * .02), fbm(p * 1.4 - T * .017 + 4.)));
  vec3 c = mix(vec3(.010, .014, .032), A, smoothstep(.22, .86, w) * .9);
  c = mix(c, B, smoothstep(.48, .95, fbm(p * .8 + 11. + w)) * .6);
  c *= 1. - .5 * dot(uv * .52, uv * .52);
  vec2 z = floor(gl_FragCoord.xy / 2.);
  float stern = step(.9978, h(z));
  c += stern * .22 * (.6 + .4 * sin(T * 1.7 + h(z + 3.) * 40.));
  return c;
}
mat2 dreh(float a){ float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }
float scheibe(vec3 p, float hh){
  vec2 d = vec2(length(p.xz) - .8, abs(p.y) - max(hh - .1, .01));
  return min(max(d.x, d.y), 0.) + length(max(d, 0.)) - .1;
}
vec3 lage(vec3 p){
  float l = 1. - C;
  p.yz *= dreh(mix(-.62, -.42, C) + (M.y * .35 + sin(T * .31) * .06) * l + M.y * .12 * C);
  p.xy *= dreh((.18 + M.x * .3 + sin(T * .23) * .08) * l);
  p.xz *= dreh(M.x * .35 * C);
  return p;
}
vec2 feld(vec3 p){
  p = lage(p);
  float d = scheibe(p - vec3(D0.xy, 0.), D0.z), id = 0.;
  float e = scheibe(p - vec3(D1.xy, 0.), D1.z);
  if (e < d) { d = e; id = 1.; }
  e = scheibe(p - vec3(D2.xy, 0.), D2.z);
  if (e < d) { d = e; id = 2.; }
  return vec2(d, id);
}
void main(){
  vec2 uv = (gl_FragCoord.xy * 2. - R) / R.y;
  vec3 c = wolken(uv);
  vec2 q = (uv - O) / K;
  if (V > .01 && dot(q, q) < W * W) {
    vec3 ro = vec3(0., 0., 5.), rd = normalize(vec3(q, -3.1));
    float t = 0., nah = 1e9;
    vec2 m = vec2(0.);
    bool treffer = false;
    for (int i = 0; i < 72; i++) {
      m = feld(ro + rd * t);
      nah = min(nah, m.x);
      if (m.x < .003) { treffer = true; break; }
      t += m.x;
      if (t > 14.) break;
    }
    if (treffer) {
      vec3 p = ro + rd * t;
      vec2 e = vec2(.003, 0.);
      vec3 nn = normalize(vec3(feld(p + e.xyy).x - feld(p - e.xyy).x, feld(p + e.yxy).x - feld(p - e.yxy).x, feld(p + e.yyx).x - feld(p - e.yyx).x));
      vec3 l = normalize(vec3(-.55, .75, .65));
      float hell = max(dot(nn, l), 0.);
      float glanz = pow(max(dot(reflect(rd, nn), l), 0.), 46.);
      float rand = pow(1. - max(dot(nn, -rd), 0.), 3.);
      float deckel = smoothstep(.55, .95, lage(nn).y);
      vec3 blau = mix(vec3(.07, .20, .56), vec3(.50, .70, 1.), deckel * .85);
      vec3 grau = mix(vec3(.27, .30, .40), vec3(.70, .74, .84), deckel * .85);
      vec3 farbe = mix(blau, grau, G * step(.5, m.y));
      vec3 o = farbe * (.22 + .95 * hell) + glanz * .9 + rand * (A * 1.6 + .12);
      c = mix(c, o, V);
    } else {
      c += A * smoothstep(.09, 0., nah) * .5 * V;
    }
  }
  c += (h(gl_FragCoord.xy + fract(T) * 61.) - .5) * .03;
  gl_FragColor = vec4(pow(max(c, 0.), vec3(.92)), 1.);
}`;

  function shader(art, quelle) {
    var s = gl.createShader(art);
    gl.shaderSource(s, quelle);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  }
  var programm = gl.createProgram();
  try {
    gl.attachShader(programm, shader(gl.VERTEX_SHADER, VS));
    gl.attachShader(programm, shader(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(programm);
    if (!gl.getProgramParameter(programm, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(programm));
  } catch (fehler) {
    document.documentElement.classList.add("no-gl");
    return;
  }
  gl.useProgram(programm);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  var ort = gl.getAttribLocation(programm, "p");
  gl.enableVertexAttribArray(ort);
  gl.vertexAttribPointer(ort, 2, gl.FLOAT, false, 0, 0);
  var u = {};
  ["R", "T", "M", "V", "K", "O", "C", "G", "W", "D0", "D1", "D2", "A", "B"].forEach(function (name) {
    u[name] = gl.getUniformLocation(programm, name);
  });

  // Ziel kommt von app.js; `jetzt` laeuft ihm weich hinterher. `fest` wird ohne Nachlauf uebernommen.
  var ziel = (window.easyScene = window.easyScene || {
    V: 1,
    K: 0.82,
    O: [0.9, 0],
    C: 0,
    G: 0,
    W: 2.2,
    D: [
      [0, 0.5, 0.15],
      [0, 0, 0.15],
      [0, -0.5, 0.15],
    ],
    A: [0.09, 0.2, 0.62],
    B: [0.3, 0.1, 0.52],
  });
  var jetzt = JSON.parse(JSON.stringify(ziel));
  var maus = [0, 0];
  var mausWeich = [0, 0];
  function groesse() {
    // unter voller Aufloesung rechnen: Wolken sind ohnehin weich, das spart viel Rechenzeit
    var dichte = Math.min(devicePixelRatio || 1, 2);
    var faktor = Math.min(1, 1500 / (innerWidth * dichte)) * dichte * 0.7;
    leinwand.width = Math.max(2, Math.round(innerWidth * faktor));
    leinwand.height = Math.max(2, Math.round(innerHeight * faktor));
    gl.viewport(0, 0, leinwand.width, leinwand.height);
  }
  function naeher(a, b, k) {
    return a + (b - a) * k;
  }
  var start = performance.now();
  var sichtbar = true;
  function bild(zeit) {
    var k = ruhig.matches ? 1 : 0.09;
    // Ort folgt der Seite sofort, sonst wuerde das Objekt beim Scrollen nachziehen
    jetzt.O = ziel.O.slice();
    ["V", "K", "C", "G", "W"].forEach(function (n) {
      jetzt[n] = naeher(jetzt[n], ziel[n], k);
    });
    ["A", "B"].forEach(function (n) {
      for (var i = 0; i < 3; i++) jetzt[n][i] = naeher(jetzt[n][i], ziel[n][i], 0.05);
    });
    for (var s = 0; s < 3; s++) for (var j = 0; j < 3; j++) jetzt.D[s][j] = naeher(jetzt.D[s][j], ziel.D[s][j], k);
    mausWeich[0] = naeher(mausWeich[0], maus[0], 0.05);
    mausWeich[1] = naeher(mausWeich[1], maus[1], 0.05);
    gl.uniform2f(u.R, leinwand.width, leinwand.height);
    gl.uniform1f(u.T, ruhig.matches ? 12 : (zeit - start) / 1000);
    gl.uniform2f(u.M, mausWeich[0], mausWeich[1]);
    gl.uniform1f(u.V, jetzt.V);
    gl.uniform1f(u.K, jetzt.K);
    gl.uniform2f(u.O, jetzt.O[0], jetzt.O[1]);
    gl.uniform1f(u.C, jetzt.C);
    gl.uniform1f(u.G, jetzt.G);
    gl.uniform1f(u.W, jetzt.W);
    gl.uniform3f(u.D0, jetzt.D[0][0], jetzt.D[0][1], jetzt.D[0][2]);
    gl.uniform3f(u.D1, jetzt.D[1][0], jetzt.D[1][1], jetzt.D[1][2]);
    gl.uniform3f(u.D2, jetzt.D[2][0], jetzt.D[2][1], jetzt.D[2][2]);
    gl.uniform3f(u.A, jetzt.A[0], jetzt.A[1], jetzt.A[2]);
    gl.uniform3f(u.B, jetzt.B[0], jetzt.B[1], jetzt.B[2]);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (!ruhig.matches && sichtbar) requestAnimationFrame(bild);
  }
  function neu() {
    requestAnimationFrame(bild);
  }
  groesse();
  addEventListener("resize", function () {
    groesse();
    if (ruhig.matches) neu();
  });
  // ohne Bewegung: nur neu zeichnen, wenn sich etwas aendert
  addEventListener(
    "scroll",
    function () {
      if (ruhig.matches) neu();
    },
    { passive: true },
  );
  document.addEventListener("easymysql-scene", function () {
    if (ruhig.matches) neu();
  });
  addEventListener(
    "pointermove",
    function (e) {
      if (e.pointerType === "mouse") maus = [e.clientX / innerWidth - 0.5, 0.5 - e.clientY / innerHeight];
    },
    { passive: true },
  );
  document.addEventListener("visibilitychange", function () {
    sichtbar = !document.hidden;
    if (sichtbar) neu();
  });
  neu();
})();
