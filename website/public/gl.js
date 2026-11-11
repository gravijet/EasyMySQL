/* Die Buehne der Seite: eine dunkle, schillernde Fluessigkeit. Grosse Woerter und Zahlen heben
   sich als glaenzendes Relief aus ihr und schmelzen beim Wechsel ineinander. Die Maus wirft
   Wellen. app.js gibt ueber window.easyScene vor, welcher Text gerade steht (text, n) und
   welche Farben sich spiegeln (A, B); `hoch` regelt, wie stark das Relief ist. */
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
uniform vec2 R;          // Groesse in Pixeln
uniform float T;         // Zeit
uniform vec2 M;          // Maus in Bildkoordinaten
uniform sampler2D X0;    // Text vorher (weichgezeichnet)
uniform sampler2D X1;    // Text jetzt
uniform float Q;         // Uebergang 0 bis 1
uniform float H;         // Staerke des Reliefs
uniform vec3 A;          // Spiegelfarbe 1
uniform vec3 B;          // Spiegelfarbe 2
uniform vec3 W[6];       // Wellen: x, y, Startzeit

float h(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float n(vec2 p){
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3. - 2. * f);
  return mix(mix(h(i), h(i + vec2(1, 0)), f.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), f.x), f.y);
}
float fbm(vec2 p){
  float a = .5, s = 0.;
  for (int i = 0; i < 3; i++) { s += a * n(p); p = p * 2.03 + vec2(1.7, 9.2); a *= .5; }
  return s;
}
// Text: der alte schmilzt fleckig weg, der neue steigt auf
float schrift(vec2 uv){
  vec2 st = vec2(uv.x * R.y / R.x, uv.y) * .5 + .5;
  float fleck = n(st * 7. + T * .15);
  float k = smoothstep(fleck - .3, fleck + .3, Q * 1.6 - .3);
  return mix(texture2D(X0, st).r, texture2D(X1, st).r, k);
}
float hoehe(vec2 uv){
  float see = fbm(uv * .85 + vec2(T * .04, -T * .03) + 1.1 * fbm(uv * 1.1 - T * .045));
  float hh = see * .5 + smoothstep(.12, .7, schrift(uv)) * .5 * H;
  for (int i = 0; i < 6; i++) {
    float alt = T - W[i].z;
    if (alt > 0. && alt < 3.2) {
      float d = length(uv - W[i].xy);
      hh += sin(30. * d - alt * 8.) * exp(-d * 4.5) * exp(-alt * 1.5) * smoothstep(0., .12, alt) * .045;
    }
  }
  return hh;
}
void main(){
  vec2 uv = (gl_FragCoord.xy * 2. - R) / R.y;
  float e = 3. / R.y;
  float c0 = hoehe(uv);
  vec3 nn = normalize(vec3((c0 - hoehe(uv + vec2(e, 0.))) / e * .42, (c0 - hoehe(uv + vec2(0., e))) / e * .42, 1.));
  vec3 r = reflect(vec3(0., 0., -1.), nn);
  float schraeg = 1. - nn.z;
  // was sich spiegelt: zwei Farbflaechen und ein heller Streifen, der der Maus folgt
  float band = r.y * .6 + r.x * .3 + .5;
  vec3 spiegel = mix(B, A, smoothstep(.15, .85, band)) * 1.5;
  vec3 licht = normalize(vec3(M.x * .7 - .25, M.y * .7 + .45, .75));
  float glanz = pow(max(dot(r, licht), 0.), 26.) * .55;
  spiegel += smoothstep(.16, 0., abs(r.y - .2 - M.y * .25)) * .35;
  // duenner Film wie Oel: die Farbe wandert mit der Neigung
  vec3 film = .5 + .5 * cos(6.2831 * (schraeg * 3.4 + vec3(0., .33, .67)) + T * .25);
  vec3 c = vec3(.008, .010, .024) + mix(B, A, smoothstep(-.9, .9, uv.y + uv.x * .4)) * .07;
  // Licht von links oben (wandert mit der Maus): Kanten zum Licht hin leuchten farbig,
  // abgewandte Kanten bleiben dunkel. So wirkt das Relief wirklich erhaben.
  float zum = dot(nn, normalize(vec3(-.5 + M.x * .5, .62 + M.y * .35, .6)));
  float hin = max(zum, 0.);
  c += spiegel * schraeg * (.5 + 2.4 * hin) + mix(film, vec3(1.), .3) * schraeg * schraeg * 2.6 * hin;
  c += pow(max(zum - .6, 0.) * 2.5, 2.) * 1.6 * (A * .4 + .6) + glanz;
  c *= 1. - schraeg * (1. - hin) * .75;
  // Die Buchstaben bestehen aus derselben Fluessigkeit: innen etwas heller und nass,
  // ein Lichtstreifen wandert langsam darueber und folgt der Maus.
  float oben = smoothstep(.5, .7, schrift(uv)) * H;
  float streif = smoothstep(.2, 0., abs(fract((uv.x + uv.y * .45) * .3 - T * .07 - M.x * .25) - .5) - .06);
  c += oben * (vec3(.055, .06, .085) + (A * .55 + .45) * (.1 + .42 * streif));
  c *= 1. - .38 * dot(uv * .5, uv * .5);
  c += (h(gl_FragCoord.xy + fract(T) * 61.) - .5) * .028;
  // weich begrenzen, damit helle Kanten nicht zu weissen Flaechen ausbrennen
  c = 1. - exp(-max(c, 0.) * 1.5);
  gl_FragColor = vec4(pow(c, vec3(.92)), 1.);
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
  ["R", "T", "M", "X0", "X1", "Q", "H", "A", "B", "W"].forEach(function (name) {
    u[name] = gl.getUniformLocation(programm, name);
  });

  // Zwei Texturen: der Text vorher und der Text jetzt.
  function textur(einheit) {
    var t = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0 + einheit);
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 0, 0, 255]));
    return t;
  }
  var texturen = [textur(0), textur(1)];
  var vorn = 1; // Index der Textur mit dem aktuellen Text
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
  var blatt = document.createElement("canvas");
  var stift = blatt.getContext("2d");

  // Zeichnet den Text weichgezeichnet (so entsteht die Hoehe) und laedt ihn als neue Textur.
  function schreibe(spez) {
    var breit = Math.min(1400, Math.max(480, innerWidth));
    blatt.width = breit;
    blatt.height = Math.round((breit * innerHeight) / innerWidth);
    stift.fillStyle = "#000";
    stift.fillRect(0, 0, blatt.width, blatt.height);
    var zeilen = (spez && spez.zeilen) || [];
    if (zeilen.length) {
      var px = spez.h * blatt.height;
      stift.font = "780 " + px + 'px "Inter Tight", system-ui, sans-serif';
      if ("letterSpacing" in stift) stift.letterSpacing = -0.03 * px + "px";
      var laengste = Math.max.apply(
        null,
        zeilen.map(function (z) {
          return stift.measureText(z).width;
        }),
      );
      // passt die laengste Zeile nicht in die Breite, wird alles kleiner
      var mass = Math.min(1, (spez.breit * blatt.width) / laengste);
      px *= mass;
      stift.font = "780 " + px + 'px "Inter Tight", system-ui, sans-serif';
      if ("letterSpacing" in stift) stift.letterSpacing = -0.03 * px + "px";
      stift.textBaseline = "middle";
      stift.fillStyle = "#fff";
      stift.shadowColor = "#fff";
      stift.shadowBlur = px * 0.07;
      zeilen.forEach(function (zeile, i) {
        var y = spez.y * blatt.height + (i - (zeilen.length - 1) / 2) * px * 0.9;
        // zweimal: der Schatten macht die Kante weich, die Flaeche bleibt voll
        stift.fillText(zeile, spez.x * blatt.width, y);
        stift.fillText(zeile, spez.x * blatt.width, y);
      });
      stift.shadowBlur = 0;
    }
    vorn = 1 - vorn;
    gl.activeTexture(gl.TEXTURE0 + vorn);
    gl.bindTexture(gl.TEXTURE_2D, texturen[vorn]);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, blatt);
  }

  var ziel = (window.easyScene = window.easyScene || { n: 0, text: null, hoch: 1, A: [0.2, 0.35, 1], B: [0.6, 0.2, 0.9] });
  var stand = { n: -1, hoch: 0, A: ziel.A.slice(), B: ziel.B.slice(), q: 1 };
  var maus = [0.3, 0.2];
  var mausWeich = [0.3, 0.2];
  var wellen = new Float32Array(18).fill(-9);
  var naechste = 0;
  var letzteWelle = 0;
  var start = performance.now();
  function sekunden() {
    return (performance.now() - start) / 1000;
  }
  function welle(x, y) {
    wellen[naechste * 3] = x;
    wellen[naechste * 3 + 1] = y;
    wellen[naechste * 3 + 2] = sekunden();
    naechste = (naechste + 1) % 6;
  }
  function groesse() {
    // unter voller Aufloesung rechnen: die Flaeche ist weich, das spart viel Rechenzeit
    var dichte = Math.min(devicePixelRatio || 1, 2);
    var faktor = Math.min(1, 1500 / (innerWidth * dichte)) * dichte * 0.75;
    leinwand.width = Math.max(2, Math.round(innerWidth * faktor));
    leinwand.height = Math.max(2, Math.round(innerHeight * faktor));
    gl.viewport(0, 0, leinwand.width, leinwand.height);
    stand.n = -1;
  }
  var sichtbar = true;
  var letztesBild = performance.now();
  function bild() {
    if (stand.n !== ziel.n) {
      stand.n = ziel.n;
      schreibe(ziel.text);
      stand.q = ruhig.matches ? 1 : 0;
    }
    // alles nach vergangener Zeit bewegen, nicht nach Bildern: auf langsamen Geraeten gleich schnell
    var jetztMs = performance.now();
    var dt = Math.min(0.25, (jetztMs - letztesBild) / 1000);
    letztesBild = jetztMs;
    stand.q = Math.min(1, stand.q + dt / 1.1);
    var k = ruhig.matches ? 1 : 1 - Math.exp(-dt * 4.5);
    stand.hoch += (ziel.hoch - stand.hoch) * k;
    for (var i = 0; i < 3; i++) {
      stand.A[i] += (ziel.A[i] - stand.A[i]) * k;
      stand.B[i] += (ziel.B[i] - stand.B[i]) * k;
    }
    mausWeich[0] += (maus[0] - mausWeich[0]) * 0.06;
    mausWeich[1] += (maus[1] - mausWeich[1]) * 0.06;
    var weichQ = stand.q * stand.q * (3 - 2 * stand.q);
    gl.uniform2f(u.R, leinwand.width, leinwand.height);
    gl.uniform1f(u.T, ruhig.matches ? 12 : sekunden());
    gl.uniform2f(u.M, mausWeich[0], mausWeich[1]);
    gl.uniform1i(u.X0, 1 - vorn);
    gl.uniform1i(u.X1, vorn);
    gl.uniform1f(u.Q, weichQ);
    gl.uniform1f(u.H, stand.hoch);
    gl.uniform3f(u.A, stand.A[0], stand.A[1], stand.A[2]);
    gl.uniform3f(u.B, stand.B[0], stand.B[1], stand.B[2]);
    gl.uniform3fv(u.W, wellen);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (!ruhig.matches && sichtbar) requestAnimationFrame(bild);
  }
  function neu() {
    requestAnimationFrame(bild);
  }
  function ort2(e) {
    return [((e.clientX / innerWidth) * 2 - 1) * (innerWidth / innerHeight), 1 - (e.clientY / innerHeight) * 2];
  }
  groesse();
  addEventListener("resize", function () {
    groesse();
    if (ruhig.matches) neu();
  });
  document.addEventListener("easymysql-scene", function () {
    if (ruhig.matches) neu();
  });
  if (document.fonts)
    document.fonts.ready.then(function () {
      stand.n = -1;
      if (ruhig.matches) neu();
    });
  addEventListener(
    "pointermove",
    function (e) {
      maus = ort2(e);
      var jetzt = performance.now();
      if (jetzt - letzteWelle > 260 && !ruhig.matches) {
        letzteWelle = jetzt;
        welle(maus[0], maus[1]);
      }
    },
    { passive: true },
  );
  addEventListener(
    "pointerdown",
    function (e) {
      var p = ort2(e);
      if (!ruhig.matches) welle(p[0], p[1]);
    },
    { passive: true },
  );
  document.addEventListener("visibilitychange", function () {
    sichtbar = !document.hidden;
    if (sichtbar) neu();
  });
  neu();
})();
