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
uniform float O;         // Relief wandert mit seinem Abschnitt (in Bildhoehen)
uniform float S;         // gescrollte Strecke in Bildhoehen
uniform vec4 G;          // Massstab der gespeicherten Steigung (Textur vorher: xy, jetzt: zw) je Einheit der Texturkoordinate

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
// Text: der alte schmilzt fleckig weg, der neue steigt auf. Die Texturen enthalten in R den weichgezeichneten
// Text und in G/B die Steigung der Hoehe (smootherstep von .12 bis .7 des Textwerts), einmal in JavaScript mit Gleitkomma-Genauigkeit
// berechnet (siehe schreibe). Frueher entstand die Steigung hier aus Differenzen benachbarter 8-Bit-Werte;
// deren Stufen zeigten sich an Rand und Flanken als Treppen und Krumen.
// f = Textwert, g = Steigung je Einheit von uv
void schrift(vec2 uv, out float f, out vec2 g){
  vec2 st = vec2(uv.x * R.y / R.x, uv.y) * .5 + .5;
  st.y -= O;
  float fleck = n(st * 7. + T * .15);
  float k = smoothstep(fleck - .3, fleck + .3, Q * 1.6 - .3);
  vec4 b = texture2D(X1, st);
  // 128 steht genau fuer 0: so kippt keine Rundung die ganze Flaeche
  vec2 gs = (b.gb * 255. - 128.) / 127. * G.zw;
  f = b.r;
  if (k < .999) {
    vec4 a = texture2D(X0, st);
    f = mix(a.r, f, k);
    gs = mix((a.gb * 255. - 128.) / 127. * G.xy, gs, k);
  }
  // Texturkoordinate -> uv: x mit R.y/R.x * .5, y mit .5
  g = gs * vec2(.5 * R.y / R.x, .5);
}
// Wasseroberflaeche und Wellen (ohne Text): niederfrequent, die Steigung darf numerisch entstehen
float meer(vec2 uv){
  vec2 ww = uv - vec2(0., S * .6);
  float see = fbm(ww * .7 + vec2(T * .035, -T * .025) + 1.1 * fbm(ww * .9 - T * .04));
  float hh = see * .42;
  for (int i = 0; i < 6; i++) {
    float alt = T - W[i].z;
    if (alt > 0. && alt < 3.2) {
      float d = length(uv - W[i].xy);
      hh += sin(30. * d - alt * 8.) * exp(-d * 4.5) * exp(-alt * 1.5) * smoothstep(0., .12, alt) * .045;
    }
  }
  return hh;
}
// Farbe an einem Punkt: Text-Relief aus der Textur, Wasserflaeche als schon berechnete Steigung
vec3 farbe(vec2 uv, vec2 meerSteig){
  float fs;
  vec2 gt;
  schrift(uv, fs, gt);
  vec2 steig = meerSteig + gt * .5 * H;
  vec3 nn = normalize(vec3(-steig * .42, 1.));
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
  // volle Glanzlichter nur am Relief: die freie Flaeche bleibt ruhig, damit Text darauf lesbar ist
  float nah = smoothstep(.02, .3, fs) * H;
  float kraft = mix(.46, 1., nah);
  float zum = dot(nn, normalize(vec3(-.5 + M.x * .5, .62 + M.y * .35, .6)));
  float hin = max(zum, 0.);
  c += kraft * (spiegel * schraeg * (.5 + 2.4 * hin) + mix(film, vec3(1.), .3) * schraeg * schraeg * 2.6 * hin);
  c += kraft * (pow(max(zum - .6, 0.) * 2.5, 2.) * 1.6 * (A * .4 + .6) + glanz);
  c *= 1. - schraeg * (1. - hin) * .75;
  // Die Buchstaben bestehen aus derselben Fluessigkeit: innen etwas heller und nass,
  // ein Lichtstreifen wandert langsam darueber und folgt der Maus.
  float oben = smoothstep(.5, .7, fs) * H;
  float streif = smoothstep(.2, 0., abs(fract((uv.x + uv.y * .45) * .3 - T * .07 - M.x * .25) - .5) - .06);
  c += oben * (vec3(.055, .06, .085) + (A * .55 + .45) * (.1 + .42 * streif));
  c *= 1. - .38 * dot(uv * .5, uv * .5);
  c += (h(gl_FragCoord.xy + fract(T) * 61.) - .5) * .018;
  // weich begrenzen, damit helle Kanten nicht zu weissen Flaechen ausbrennen
  c = 1. - exp(-max(c, 0.) * 1.5);
  // abseits des Reliefs bleibt die Flaeche dunkel: heller Text hat darauf immer genug Kontrast
  c = mix(.2 * (1. - exp(-c / .2)), c, nah);
  return pow(c, vec3(.92));
}
void main(){
  vec2 uv = (gl_FragCoord.xy * 2. - R) / R.y;
  float e = 3. / R.y;
  float c0 = meer(uv);
  vec2 ms = vec2(meer(uv + vec2(e, 0.)) - c0, meer(uv + vec2(0., e)) - c0) / e;
  // Am Relief vier Punkte je Pixel (gedrehtes Raster) und mitteln: die Glanzkante zwischen Rand und
  // Buchstabenflaeche ist schmaler als ein Pixel und wurde sonst treppig. Die freie Flaeche bleibt bei einem.
  vec2 st = vec2(uv.x * R.y / R.x, uv.y) * .5 + .5;
  st.y -= O;
  float t = max(texture2D(X1, st).r, Q < .999 ? texture2D(X0, st).r : 0.);
  if (t * H < .01) {
    gl_FragColor = vec4(farbe(uv, ms), 1.);
    return;
  }
  float p = 2. / R.y;
  vec3 c = farbe(uv + vec2(.125, .375) * p, ms) + farbe(uv + vec2(.375, -.125) * p, ms)
         + farbe(uv + vec2(-.125, -.375) * p, ms) + farbe(uv + vec2(-.375, .125) * p, ms);
  gl_FragColor = vec4(c * .25, 1.);
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
  ["R", "T", "M", "X0", "X1", "Q", "H", "A", "B", "W", "O", "S", "G"].forEach(function (name) {
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
  // Zeilen werden von unten nach oben selbst geschrieben (siehe relief)
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
  var blatt = document.createElement("canvas");
  var stift = blatt.getContext("2d", { willReadFrequently: true });

  var texGrenze = Math.min(2048, gl.getParameter(gl.MAX_TEXTURE_SIZE) || 2048);
  var massstab = [
    [0, 0],
    [0, 0],
  ]; // Steigungsmassstab je Textur (je Einheit der Texturkoordinate)
  var vorrat = []; // die letzten berechneten Reliefs: beim Scrollen wechselt der Text hin und her

  // Zeichnet den Text weichgezeichnet (so entsteht die Hoehe) und berechnet daraus in Gleitkomma die Steigung.
  // Ergebnis: RGBA mit R = Text, G/B = Steigung der Hoehe je Texel (128 = 0).
  function relief(spez, zeilen, w, h) {
    blatt.width = w;
    blatt.height = h;
    stift.fillStyle = "#000";
    stift.fillRect(0, 0, w, h);
    var px = spez.h * h;
    stift.font = "780 " + px + 'px "Inter Tight", system-ui, sans-serif';
    if ("letterSpacing" in stift) stift.letterSpacing = -0.03 * px + "px";
    var laengste = Math.max.apply(
      null,
      zeilen.map(function (z) {
        return stift.measureText(z).width;
      }),
    );
    // passt die laengste Zeile nicht in die Breite, wird alles kleiner
    px *= Math.min(1, (spez.breit * w) / laengste);
    stift.font = "780 " + px + 'px "Inter Tight", system-ui, sans-serif';
    if ("letterSpacing" in stift) stift.letterSpacing = -0.03 * px + "px";
    stift.textBaseline = "middle";
    stift.fillStyle = "#fff";
    stift.shadowColor = "#fff";
    stift.shadowBlur = px * 0.055;
    zeilen.forEach(function (zeile, i) {
      var y = spez.y * h + (i - (zeilen.length - 1) / 2) * px * 0.9;
      // zweimal: der Schatten macht die Kante weich, die Flaeche bleibt voll
      stift.fillText(zeile, spez.x * w, y);
      stift.fillText(zeile, spez.x * w, y);
    });
    stift.shadowBlur = 0;
    var roh = stift.getImageData(0, 0, w, h).data;
    var n = w * h;
    var f = new Float32Array(n);
    var t = new Float32Array(n);
    for (var i = 0; i < n; i++) f[i] = roh[i * 4] / 255;
    // Dreimal einen Kastenfilter (ueber laufende Summen) je Richtung, etwa eine Gaussglocke mit 2,5 Texeln
    // (bei 2048 Texeln Breite; der Radius waechst mit der Textur). Das nimmt die 8-Bit-Stufen heraus und rundet
    // den Knick an der Buchstabenkante, wo der volle Text auf den weichen Schatten trifft.
    var r = Math.max(1, Math.round((2 * w) / 2048));
    var teiler = 1 / (2 * r + 1);
    // waagrecht: je Zeile eine laufende Summe
    function waagrecht(von, nach) {
      for (var y = 0; y < h; y++) {
        var basis = y * w;
        var summe = 0;
        for (var j = -r - 1; j < r; j++) summe += von[basis + Math.min(Math.max(j, 0), w - 1)];
        for (j = 0; j < w; j++) {
          var rein = j + r < w ? j + r : w - 1;
          var raus = j - r - 1 > 0 ? j - r - 1 : 0;
          summe += von[basis + rein] - von[basis + raus];
          nach[basis + j] = summe * teiler;
        }
      }
    }
    // senkrecht: laufende Summen aller Spalten zugleich, Zeile fuer Zeile (schonend fuer den Speicher-Cache)
    var summen = new Float32Array(w);
    function senkrecht(von, nach) {
      summen.fill(0);
      for (var j = -r - 1; j < r; j++) {
        var zeile = Math.min(Math.max(j, 0), h - 1) * w;
        for (var x = 0; x < w; x++) summen[x] += von[zeile + x];
      }
      for (var y = 0; y < h; y++) {
        var rein = (y + r < h ? y + r : h - 1) * w;
        var raus = (y - r - 1 > 0 ? y - r - 1 : 0) * w;
        var ziel = y * w;
        for (x = 0; x < w; x++) {
          summen[x] += von[rein + x] - von[raus + x];
          nach[ziel + x] = summen[x] * teiler;
        }
      }
    }
    for (var runde = 0; runde < 3; runde++) {
      waagrecht(f, t);
      senkrecht(t, f);
    }
    // Steigung der Hoehe je Texel (t: waagrecht, gy: senkrecht; y zeigt in der Textur nach oben).
    // Hoehe = smootherstep (6v^5 - 15v^4 + 10v^3) von .12 bis .7 des Textwerts: ihre Steigung laeuft oben und
    // unten quadratisch aus. Mit smoothstep fiel sie linear auf null, und der gesaettigte helle Rand kippte in
    // weniger als einem Pixel in die Buchstabenflaeche.
    var gy = new Float32Array(n);
    var gross = 1e-6;
    for (var y = 0; y < h; y++) {
      var oben = y > 0 ? -w : 0,
        unten = y < h - 1 ? w : 0;
      for (var x = 0; x < w; x++) {
        var o = y * w + x;
        var fo = f[o];
        if (fo <= 0.12 || fo >= 0.7) {
          t[o] = 0;
          gy[o] = 0;
          continue;
        }
        var v = (fo - 0.12) / 0.58;
        var s = (30 * v * v * (1 - v) * (1 - v)) / 0.58;
        var ax = (s * (f[o + (x < w - 1 ? 1 : 0)] - f[o - (x > 0 ? 1 : 0)])) / 2;
        var ay = (-s * (f[o + unten] - f[o + oben])) / 2;
        t[o] = ax;
        gy[o] = ay;
        if (ax > gross) gross = ax;
        else if (-ax > gross) gross = -ax;
        if (ay > gross) gross = ay;
        else if (-ay > gross) gross = -ay;
      }
    }
    var daten = new Uint8Array(n * 4);
    var teil = 127 / gross;
    for (y = 0; y < h; y++) {
      var zeile = (h - 1 - y) * w; // unterste Zeile zuerst
      for (x = 0; x < w; x++) {
        o = y * w + x;
        var z = (zeile + x) * 4;
        daten[z] = Math.round(f[o] * 255);
        daten[z + 1] = 128 + Math.round(t[o] * teil);
        daten[z + 2] = 128 + Math.round(gy[o] * teil);
        daten[z + 3] = 255;
      }
    }
    return { daten: daten, w: w, h: h, k: gross };
  }

  // Laedt den Text (oder nichts) als neue Textur; der alte bleibt fuer den Uebergang erhalten.
  function schreibe(spez) {
    var zeilen = (spez && spez.zeilen) || [];
    vorn = 1 - vorn;
    gl.activeTexture(gl.TEXTURE0 + vorn);
    gl.bindTexture(gl.TEXTURE_2D, texturen[vorn]);
    if (!zeilen.length) {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 128, 128, 255]));
      massstab[vorn] = [0, 0];
      return;
    }
    // etwa so dicht wie die Zeichenflaeche (hoechstens 2048 breit): Steigung und Text werden zwischen den
    // Texeln linear gelesen und bleiben dabei glatt, auch wenn die Textur etwas vergroessert wird
    var breit = Math.max(480, leinwand.width);
    var hoch = Math.round((breit * innerHeight) / innerWidth);
    var m = Math.min(1, texGrenze / breit, texGrenze / Math.max(1, hoch));
    var w = Math.max(2, Math.round(breit * m));
    var h = Math.max(2, Math.round(hoch * m));
    var schluessel = JSON.stringify([zeilen, spez.h, spez.x, spez.y, spez.breit, w, h, document.fonts ? document.fonts.status : ""]);
    var fertig = null;
    for (var i = 0; i < vorrat.length; i++) if (vorrat[i].schluessel === schluessel) fertig = vorrat[i];
    if (!fertig) {
      fertig = relief(spez, zeilen, w, h);
      fertig.schluessel = schluessel;
      vorrat.unshift(fertig);
      vorrat.length = Math.min(vorrat.length, 2);
    }
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, fertig.w, fertig.h, 0, gl.RGBA, gl.UNSIGNED_BYTE, fertig.daten);
    massstab[vorn] = [fertig.k * fertig.w, fertig.k * fertig.h];
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
    // so scharf wie der Bildschirm, hoechstens rund 5 Mio. Bildpunkte (vorher 2,4 Mio.: auf 1440p- und
    // 4K-Bildschirmen wurde das Bild bis fast aufs Doppelte hochgezogen und wirkte pixelig); `guete`
    // sinkt, wenn das Geraet dabei nicht fluessig bleibt (siehe bild)
    var dichte = Math.min(devicePixelRatio || 1, 2.5);
    var faktor = Math.min(dichte, Math.sqrt(5000000 / (innerWidth * innerHeight))) * guete;
    leinwand.width = Math.max(2, Math.round(innerWidth * faktor));
    leinwand.height = Math.max(2, Math.round(innerHeight * faktor));
    gl.viewport(0, 0, leinwand.width, leinwand.height);
    stand.n = -1;
  }
  var sichtbar = true;
  var letztesBild = performance.now();
  var guete = 1; // Anteil der vollen Aufloesung
  var gemessen = 0;
  var dauer = 0;
  function bild() {
    if (stand.n !== ziel.n) {
      stand.n = ziel.n;
      schreibe(ziel.text);
      stand.q = ruhig.matches || stand.hoch < 0.08 ? 1 : 0;
    }
    // alles nach vergangener Zeit bewegen, nicht nach Bildern: auf langsamen Geraeten gleich schnell
    var jetztMs = performance.now();
    var dt = Math.min(0.25, (jetztMs - letztesBild) / 1000);
    letztesBild = jetztMs;
    // laeuft es ueber 40 Bilder langsamer als etwa 22 Bilder je Sekunde, wird groeber gerechnet
    if (++gemessen > 10) dauer += dt;
    if (gemessen === 50) {
      if (dauer / 40 > 0.045 && guete > 0.45) {
        guete *= 0.75;
        groesse();
      }
      gemessen = 0;
      dauer = 0;
    }
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
    gl.uniform1f(u.O, ziel.versatz || 0);
    gl.uniform4f(u.G, massstab[1 - vorn][0], massstab[1 - vorn][1], massstab[vorn][0], massstab[vorn][1]);
    gl.uniform1f(u.S, ruhig.matches ? 0 : ziel.lauf || 0);
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
    gemessen = 0;
    dauer = 0;
    letztesBild = performance.now();
    if (sichtbar) neu();
  });
  neu();
})();
