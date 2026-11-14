/* Die Buehne der Seite: eine dunkle, schillernde Fluessigkeit. Grosse Woerter heben sich als glaenzendes
   Relief aus ihr und schmelzen beim Wechsel ineinander. Die Maus wirft Wellen. app.js gibt ueber
   window.easyScene vor, welcher Text gerade steht (text, n) und welche Farben sich spiegeln (A, B);
   `hoch` regelt, wie stark das Relief ist.

   Zwei Durchgaenge je Bild: Die Wasserflaeche aendert sich nur langsam ueber den Bildschirm. Ihre Steigung
   wird deshalb in einer kleinen Textur berechnet (hoechstens 240 Zeilen hoch) und im zweiten Durchgang glatt
   vergroessert. Nur das Licht und das Relief der Schrift entstehen in voller Aufloesung. */
(function () {
  "use strict";
  var leinwand = document.getElementById("gl");
  if (!leinwand) return;
  var ruhig = matchMedia("(prefers-reduced-motion: reduce)");
  var gl = leinwand.getContext("webgl", { antialias: false, alpha: false, depth: false, stencil: false, powerPreference: "high-performance" });
  function ohneGl() {
    document.documentElement.classList.add("no-gl");
  }
  if (!gl) return ohneGl();

  var VS = "attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}";
  // gemeinsam: Rauschen ohne sin() (bleibt auch bei grossen Koordinaten und auf Handy-GPUs gleichmaessig)
  var RAUSCHEN = `
precision highp float;
float h(vec2 p){
  vec3 q = fract(vec3(p.xyx) * .1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}
float n(vec2 p){
  vec2 i = floor(p), f = fract(p);
  f = f * f * (3. - 2. * f);
  return mix(mix(h(i), h(i + vec2(1, 0)), f.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), f.x), f.y);
}`;

  // 1. Durchgang: Steigung der Wasserflaeche (langsame Duenung und Wellen der Maus), klein gerechnet.
  // Ausgabe: .5 + Steigung / 8 in R und G.
  var FS_MEER =
    RAUSCHEN +
    `
uniform vec2 R;          // Groesse der kleinen Textur
uniform float T;         // Zeit
uniform vec3 W[6];       // Wellen: x, y, Startzeit
uniform float S;         // gescrollte Strecke in Bildhoehen
float fbm(vec2 p){
  float a = .5, s = 0.;
  for (int i = 0; i < 3; i++) { s += a * n(p); p = p * 2.03 + vec2(1.7, 9.2); a *= .5; }
  return s;
}
float meer(vec2 uv){
  vec2 ww = uv - vec2(0., S * .6);
  float hh = fbm(ww * .7 + vec2(T * .035, -T * .025) + 1.1 * fbm(ww * .9 - T * .04)) * .42;
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
  float e = .006;
  float c0 = meer(uv);
  vec2 g = vec2(meer(uv + vec2(e, 0.)) - c0, meer(uv + vec2(0., e)) - c0) / e;
  gl_FragColor = vec4(clamp(.5 + g / 8., 0., 1.), 0., 1.);
}`;

  // Licht und Spiegelung, gemeinsam fuer die kleine Farbflaeche und das Relief in voller Groesse
  var LICHT =
    RAUSCHEN +
    `
uniform vec2 R;          // Groesse in Pixeln
uniform float T;         // Zeit
uniform vec2 M;          // Maus in Bildkoordinaten (begrenzt)
uniform vec3 L;          // Richtung zum Licht fuer die Kanten (immer schraeg von oben, die Maus dreht sie nur)
uniform vec3 K;          // Richtung des Glanzlichts
uniform sampler2D X0;    // Text vorher (weichgezeichnet)
uniform sampler2D X1;    // Text jetzt
uniform sampler2D Y;     // Steigung der Wasserflaeche (1. Durchgang)
uniform float Q;         // Uebergang 0 bis 1
uniform float H;         // Staerke des Reliefs
uniform vec3 A;          // Spiegelfarbe 1
uniform vec3 B;          // Spiegelfarbe 2
uniform float O;         // Relief wandert mit seinem Abschnitt (in Bildhoehen)
uniform vec4 G;          // Massstab der gespeicherten Steigung (Textur vorher: xy, jetzt: zw) je Einheit der Texturkoordinate

// Text: der alte schmilzt fleckig weg, der neue steigt auf. Die Texturen enthalten in R den weichgezeichneten
// Text und in G/B die Steigung der Hoehe, einmal in JavaScript mit Gleitkomma-Genauigkeit berechnet (siehe relief).
// f = Textwert, g = Steigung je Einheit von uv
void schrift(vec2 uv, out float f, out vec2 g){
  vec2 st = vec2(uv.x * R.y / R.x, uv.y) * .5 + .5;
  st.y -= O;
  vec4 b = texture2D(X1, st);
  // 128 steht genau fuer 0: so kippt keine Rundung die ganze Flaeche
  vec2 gs = (b.gb * 255. - 128.) / 127. * G.zw;
  f = b.r;
  if (Q < .999) {
    float fleck = n(st * 7. + T * .15);
    float k = smoothstep(fleck - .3, fleck + .3, Q * 1.6 - .3);
    vec4 a = texture2D(X0, st);
    f = mix(a.r, f, k);
    gs = mix((a.gb * 255. - 128.) / 127. * G.xy, gs, k);
  }
  g = gs * vec2(.5 * R.y / R.x, .5);
}
// Farbe an einem Punkt aus der Steigung der Oberflaeche (Wasser und Schrift zusammen) und dem Textwert fs.
// Die Teile, die nur am Relief wirken, werden auf der freien Flaeche uebersprungen.
vec3 licht(vec2 uv, vec2 steig, float fs){
  vec3 nn = normalize(vec3(-steig * .42, 1.));
  vec3 r = reflect(vec3(0., 0., -1.), nn);
  float schraeg = 1. - nn.z;
  // was sich spiegelt: zwei Farbflaechen und ein heller Streifen, der der Maus folgt
  float band = r.y * .6 + r.x * .3 + .5;
  vec3 spiegel = mix(B, A, smoothstep(.15, .85, band)) * 1.5;
  spiegel += smoothstep(.16, 0., abs(r.y - .2 - M.y * .25)) * .35;
  // Grund: fast schwarz mit einem weiten, langsam ziehenden Schimmer der beiden Farben
  float zug = smoothstep(-1.1, 1.1, uv.y + uv.x * .4 + .25 * sin(T * .05 + uv.x * .6));
  vec3 c = vec3(.006, .008, .02) + mix(B, A, zug) * .06;
  // Kanten zum Licht hin leuchten farbig, abgewandte bleiben dunkel: so wirkt das Relief erhaben.
  // Volle Glanzlichter nur am Relief; die freie Flaeche schimmert nur in den Spiegelfarben, ohne Weiss.
  float nah = smoothstep(.02, .3, fs) * H;
  float zum = dot(nn, L);
  float hin = max(zum, 0.);
  vec3 weiss = spiegel * .55;
  vec3 hell = vec3(0.);
  if (nah > 0.) {
    // duenner Film wie Oel: die Farbe wandert mit der Neigung
    vec3 film = .5 + .5 * cos(6.2831 * (schraeg * 3.4 + vec3(0., .33, .67)) + T * .25);
    weiss = mix(weiss, mix(film, vec3(1.), .3), nah);
    hell = nah * (pow(max(zum - .6, 0.) * 2.5, 2.) * 1.6 * (A * .4 + .6) + pow(max(dot(r, K), 0.), 26.) * .55);
  }
  c += mix(.7, 1., nah) * (spiegel * schraeg * (.5 + 2.4 * hin) + weiss * schraeg * schraeg * 2.6 * hin) + hell;
  c *= 1. - schraeg * (1. - hin) * .75;
  // Die Buchstaben bestehen aus derselben Fluessigkeit: innen etwas heller und nass,
  // ein Lichtstreifen wandert langsam darueber und folgt der Maus.
  float oben = smoothstep(.5, .7, fs) * H;
  if (oben > 0.) {
    float streif = smoothstep(.2, 0., abs(fract((uv.x + uv.y * .45) * .3 - T * .07 - M.x * .25) - .5) - .06);
    c += oben * (vec3(.055, .06, .085) + (A * .55 + .45) * (.1 + .42 * streif));
  }
  c *= 1. - .38 * dot(uv * .5, uv * .5);
  // weich begrenzen, damit helle Kanten nicht zu weissen Flaechen ausbrennen
  c = 1. - exp(-max(c, 0.) * 1.5);
  // abseits des Reliefs bleibt die Flaeche dunkel: heller Text hat darauf immer genug Kontrast
  c = mix(.2 * (1. - exp(-c / .2)), c, nah);
  return pow(c, vec3(.92));
}
vec3 relief(vec2 uv, vec2 ms){
  float fs;
  vec2 gt;
  schrift(uv, fs, gt);
  return licht(uv, ms + gt * .5 * H, fs);
}
`;

  // 2. Durchgang: die Farbe der freien Flaeche, ebenfalls klein (sie aendert sich nur langsam ueber den Bildschirm)
  var FS_FLAECHE =
    LICHT +
    `
void main(){
  vec2 uv = (gl_FragCoord.xy * 2. - R) / R.y;
  vec2 ms = (texture2D(Y, gl_FragCoord.xy / R).rg - .5) * 8.;
  gl_FragColor = vec4(licht(uv, ms, 0.), 1.);
}`;

  // 3. Durchgang: das Relief der Schrift in voller Aufloesung, sonst die vergroesserte Flaeche und feines Korn
  var FS =
    LICHT +
    `
uniform sampler2D Z;     // Farbe der freien Flaeche (2. Durchgang)
void main(){
  vec2 uv = (gl_FragCoord.xy * 2. - R) / R.y;
  vec2 st = vec2(uv.x * R.y / R.x, uv.y) * .5 + .5;
  st.y -= O;
  float t = max(texture2D(X1, st).r, Q < .999 ? texture2D(X0, st).r : 0.);
  vec3 c;
  // freie Flaeche: schon im 2. Durchgang klein berechnet, hier nur glatt vergroessert
  if (t * H < .01) c = texture2D(Z, gl_FragCoord.xy / R).rgb;
  else {
    vec2 ms = (texture2D(Y, gl_FragCoord.xy / R).rg - .5) * 8.;
    // Am Relief vier Punkte je Pixel (gedrehtes Raster) und mitteln: die Glanzkante zwischen Rand und
    // Buchstabenflaeche ist schmaler als ein Pixel und wurde sonst treppig.
    float p = 2. / R.y;
    c = (relief(uv + vec2(.125, .375) * p, ms) + relief(uv + vec2(.375, -.125) * p, ms)
       + relief(uv + vec2(-.125, -.375) * p, ms) + relief(uv + vec2(-.375, .125) * p, ms)) * .25;
  }
  // feines Korn gegen Farbstufen im Dunkeln (Rauschen nach Jimenez, gleichmaessig auch bei 4K)
  float korn = fract(52.9829189 * fract(dot(gl_FragCoord.xy + fract(T * 7.31) * 97., vec2(.06711056, .00583715))));
  gl_FragColor = vec4(c + (korn - .5) * .014, 1.);
}`;

  // ---------- Einrichten (auch nach Verlust des Grafikkontexts) ----------
  var um, uf, u, texturen, meerTex, meerFbo, farbTex, farbFbo, meerW = 0, meerH = 0;
  var vorn = 1; // Index der Textur mit dem aktuellen Text
  var massstab = [
    [0, 0],
    [0, 0],
  ]; // Steigungsmassstab je Textur (je Einheit der Texturkoordinate)
  var texGrenze = 2048;

  function shader(art, quelle) {
    var s = gl.createShader(art);
    gl.shaderSource(s, quelle);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  }
  function programm(fs, namen) {
    var p = gl.createProgram();
    gl.attachShader(p, shader(gl.VERTEX_SHADER, VS));
    gl.attachShader(p, shader(gl.FRAGMENT_SHADER, fs));
    gl.bindAttribLocation(p, 0, "p");
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    var orte = { p: p };
    namen.forEach(function (name) {
      orte[name] = gl.getUniformLocation(p, name);
    });
    return orte;
  }
  function textur(einheit) {
    var t = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0 + einheit);
    gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0, 128, 128, 255]));
    return t;
  }
  function einrichten() {
    try {
      um = programm(FS_MEER, ["R", "T", "W", "S"]);
      uf = programm(FS_FLAECHE, ["R", "T", "M", "L", "K", "Y", "H", "A", "B"]);
      u = programm(FS, ["R", "T", "M", "L", "K", "X0", "X1", "Y", "Z", "Q", "H", "A", "B", "O", "G"]);
    } catch (fehler) {
      return false;
    }
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    // Zeilen werden von unten nach oben selbst geschrieben (siehe relief)
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    texturen = [textur(0), textur(1)];
    massstab = [
      [0, 0],
      [0, 0],
    ];
    meerTex = textur(2);
    meerFbo = gl.createFramebuffer();
    farbTex = textur(3);
    farbFbo = gl.createFramebuffer();
    meerW = meerH = 0;
    texGrenze = Math.min(2048, gl.getParameter(gl.MAX_TEXTURE_SIZE) || 2048);
    gl.useProgram(uf.p);
    gl.uniform1i(uf.Y, 2);
    gl.useProgram(u.p);
    gl.uniform1i(u.Y, 2);
    gl.uniform1i(u.Z, 3);
    return true;
  }
  if (!einrichten()) return ohneGl();

  // ---------- Relief der Schrift ----------
  var blatt = document.createElement("canvas");
  var stift = blatt.getContext("2d", { willReadFrequently: true });
  var vorrat = []; // die letzten berechneten Reliefs: beim Scrollen wechselt der Text hin und her

  // Zeichnet den Text weichgezeichnet (so entsteht die Hoehe) und berechnet daraus in Gleitkomma die Steigung.
  // Ergebnis: RGBA mit R = Text, G/B = Steigung der Hoehe je Texel (128 = 0). Gerechnet wird nur im Rechteck um
  // die Schrift; der Rest der Textur ist leer (das spart den Grossteil der Rechenzeit beim Laden).
  function relief(spez, zeilen, w, h) {
    blatt.width = w;
    blatt.height = h;
    var px = spez.h * h;
    function schrift() {
      stift.font = "780 " + px + 'px "Inter Tight", system-ui, sans-serif';
      if ("letterSpacing" in stift) stift.letterSpacing = -0.03 * px + "px";
    }
    schrift();
    var laengste = Math.max.apply(
      null,
      zeilen.map(function (z) {
        return stift.measureText(z).width;
      }),
    );
    // passt die laengste Zeile nicht in die Breite, wird alles kleiner
    px *= Math.min(1, (spez.breit * w) / laengste);
    schrift();
    stift.textBaseline = "middle";
    stift.fillStyle = "#fff";
    stift.shadowColor = "#fff";
    stift.shadowBlur = px * 0.055;
    // Dreimal einen Kastenfilter je Richtung, etwa eine Gaussglocke mit 2,5 Texeln bei 2048 Texeln Breite.
    // Das nimmt die 8-Bit-Stufen heraus und rundet den Knick an der Buchstabenkante.
    var r = Math.max(1, Math.round((2 * w) / 2048));
    var rand = Math.ceil(px * 0.055 * 2 + 3 * r + 6);
    var x0 = w,
      x1 = 0,
      y0 = h,
      y1 = 0;
    zeilen.forEach(function (zeile, i) {
      var y = spez.y * h + (i - (zeilen.length - 1) / 2) * px * 0.9;
      // zweimal: der Schatten macht die Kante weich, die Flaeche bleibt voll
      stift.fillText(zeile, spez.x * w, y);
      stift.fillText(zeile, spez.x * w, y);
      var mass = stift.measureText(zeile);
      var links = mass.actualBoundingBoxLeft || 0,
        rechts = mass.actualBoundingBoxRight || mass.width,
        auf = mass.actualBoundingBoxAscent || px * 0.6,
        ab = mass.actualBoundingBoxDescent || px * 0.6;
      x0 = Math.min(x0, spez.x * w - links);
      x1 = Math.max(x1, spez.x * w + rechts);
      y0 = Math.min(y0, y - auf);
      y1 = Math.max(y1, y + ab);
    });
    stift.shadowBlur = 0;
    var daten = new Uint8Array(w * h * 4);
    new Uint32Array(daten.buffer).fill(new Uint32Array(new Uint8Array([0, 128, 128, 255]).buffer)[0]);
    var bx = Math.max(0, Math.floor(x0 - rand)),
      by = Math.max(0, Math.floor(y0 - rand));
    var bw = Math.min(w, Math.ceil(x1 + rand)) - bx,
      bh = Math.min(h, Math.ceil(y1 + rand)) - by;
    if (bw < 3 || bh < 3) return { daten: daten, w: w, h: h, k: 0 };
    var roh = stift.getImageData(bx, by, bw, bh).data;
    var n = bw * bh;
    var f = new Float32Array(n);
    var t = new Float32Array(n);
    // Deckkraft statt Farbe: die Flaeche ist durchsichtig, weisse Schrift und Schatten decken sie ab
    for (var i = 0; i < n; i++) f[i] = roh[i * 4 + 3] / 255;
    var teiler = 1 / (2 * r + 1);
    // waagrecht: je Zeile eine laufende Summe
    function waagrecht(von, nach) {
      for (var y = 0; y < bh; y++) {
        var basis = y * bw;
        var summe = 0;
        for (var j = -r - 1; j < r; j++) summe += von[basis + Math.min(Math.max(j, 0), bw - 1)];
        for (j = 0; j < bw; j++) {
          var rein = j + r < bw ? j + r : bw - 1;
          var raus = j - r - 1 > 0 ? j - r - 1 : 0;
          summe += von[basis + rein] - von[basis + raus];
          nach[basis + j] = summe * teiler;
        }
      }
    }
    // senkrecht: laufende Summen aller Spalten zugleich, Zeile fuer Zeile (schonend fuer den Speicher-Cache)
    var summen = new Float32Array(bw);
    function senkrecht(von, nach) {
      summen.fill(0);
      for (var j = -r - 1; j < r; j++) {
        var zeile = Math.min(Math.max(j, 0), bh - 1) * bw;
        for (var x = 0; x < bw; x++) summen[x] += von[zeile + x];
      }
      for (var y = 0; y < bh; y++) {
        var rein = (y + r < bh ? y + r : bh - 1) * bw;
        var raus = (y - r - 1 > 0 ? y - r - 1 : 0) * bw;
        var ziel = y * bw;
        for (x = 0; x < bw; x++) {
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
    // unten quadratisch aus, so kippt der helle Rand nicht in weniger als einem Pixel in die Buchstabenflaeche.
    var gy = new Float32Array(n);
    var gross = 1e-6;
    for (var y = 0; y < bh; y++) {
      var oben = y > 0 ? -bw : 0,
        unten = y < bh - 1 ? bw : 0;
      for (var x = 0; x < bw; x++) {
        var o = y * bw + x;
        var fo = f[o];
        if (fo <= 0.12 || fo >= 0.7) {
          t[o] = 0;
          gy[o] = 0;
          continue;
        }
        var v = (fo - 0.12) / 0.58;
        var s = (30 * v * v * (1 - v) * (1 - v)) / 0.58;
        var ax = (s * (f[o + (x < bw - 1 ? 1 : 0)] - f[o - (x > 0 ? 1 : 0)])) / 2;
        var ay = (-s * (f[o + unten] - f[o + oben])) / 2;
        t[o] = ax;
        gy[o] = ay;
        if (ax > gross) gross = ax;
        else if (-ax > gross) gross = -ax;
        if (ay > gross) gross = ay;
        else if (-ay > gross) gross = -ay;
      }
    }
    var teil = 127 / gross;
    for (y = 0; y < bh; y++) {
      var zeile = (h - 1 - (by + y)) * w + bx; // unterste Zeile zuerst
      for (x = 0; x < bw; x++) {
        o = y * bw + x;
        var z = (zeile + x) * 4;
        daten[z] = Math.round(f[o] * 255);
        daten[z + 1] = 128 + Math.round(t[o] * teil);
        daten[z + 2] = 128 + Math.round(gy[o] * teil);
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

  // ---------- Ablauf ----------
  var ziel = (window.easyScene = window.easyScene || { n: 0, text: null, hoch: 1, A: [0.2, 0.35, 1], B: [0.6, 0.2, 0.9] });
  var stand = { n: -1, hoch: 0, A: ziel.A.slice(), B: ziel.B.slice(), q: 1, zeilen: "", versatz: NaN };
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
  var guete = 1; // Anteil der vollen Aufloesung
  function groesse() {
    // so scharf wie der Bildschirm, hoechstens rund 5 Mio. Bildpunkte; `guete` sinkt, wenn das Geraet dabei
    // nicht fluessig bleibt (siehe bild)
    var dichte = Math.min(devicePixelRatio || 1, 2.5);
    var faktor = Math.min(dichte, Math.sqrt(5000000 / (innerWidth * innerHeight))) * guete;
    leinwand.width = Math.max(2, Math.round(innerWidth * faktor));
    leinwand.height = Math.max(2, Math.round(innerHeight * faktor));
    // die Wasserflaeche: hoechstens 240 Zeilen, gleiches Seitenverhaeltnis
    var klein = Math.min(1, 240 / leinwand.height);
    var w = Math.max(2, Math.round(leinwand.width * klein));
    var h = Math.max(2, Math.round(leinwand.height * klein));
    if (w !== meerW || h !== meerH) {
      meerW = w;
      meerH = h;
      [
        [2, meerTex, meerFbo],
        [3, farbTex, farbFbo],
      ].forEach(function (t) {
        gl.activeTexture(gl.TEXTURE0 + t[0]);
        gl.bindTexture(gl.TEXTURE_2D, t[1]);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
        gl.bindFramebuffer(gl.FRAMEBUFFER, t[2]);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, t[1], 0);
      });
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    }
    stand.n = -1;
  }
  var sichtbar = !document.hidden;
  var verloren = false;
  var wartet = false; // ein Bild ist angefordert
  var letztesBild = performance.now();
  var letzteZeit = 0; // wann zuletzt gezeichnet wurde (fuer die Begrenzung auf etwa 60 Bilder je Sekunde)
  var ruheBis = 0; // waehrend der Groessenaenderung den Text noch nicht neu berechnen
  var gemessen = 0;
  var dauer = 0;
  function bild(jetztMs) {
    wartet = false;
    if (verloren) return;
    jetztMs = jetztMs || performance.now();
    var weiter = !ruhig.matches && sichtbar;
    // Bildschirme mit 120 Hz und mehr: die Fluessigkeit bewegt sich langsam, 60 Bilder je Sekunde genuegen.
    // Nur waehrend das sichtbare Relief mitscrollt, wird jedes Bild gezeichnet, damit es nicht ruckelt.
    var reliefZieht = stand.hoch > 0.01 && (ziel.versatz || 0) < 1.2 && ziel.versatz !== stand.versatz;
    if (weiter && !reliefZieht && stand.n === ziel.n && jetztMs - letzteZeit < 12.5) {
      anfordern();
      return;
    }
    letzteZeit = jetztMs;
    stand.versatz = ziel.versatz;
    if (stand.n !== ziel.n && schriftDa && (jetztMs >= ruheBis || stand.n === -2)) {
      stand.n = ziel.n;
      var zeilen = ziel.text && ziel.text.zeilen ? ziel.text.zeilen.join("\n") : "";
      schreibe(ziel.text);
      // gleicher Text (nur neue Groesse): sofort tauschen statt erneut zu schmelzen
      stand.q = ruhig.matches || stand.hoch < 0.08 || zeilen === stand.zeilen ? 1 : 0;
      stand.zeilen = zeilen;
    } else if (stand.n !== ziel.n && !weiter && schriftDa) {
      setTimeout(neu, ruheBis - jetztMs + 5);
    }
    // alles nach vergangener Zeit bewegen, nicht nach Bildern: auf langsamen Geraeten gleich schnell
    var dt = Math.min(0.25, (jetztMs - letztesBild) / 1000);
    letztesBild = jetztMs;
    // laeuft es ueber 40 Bilder langsamer als etwa 22 Bilder je Sekunde, wird groeber gerechnet
    if (weiter) {
      if (++gemessen > 10) dauer += dt;
      if (gemessen === 50) {
        if (dauer / 40 > 0.045 && guete > 0.45) {
          guete *= 0.75;
          groesse();
        }
        gemessen = 0;
        dauer = 0;
      }
    }
    stand.q = Math.min(1, stand.q + dt / 1.1);
    var k = ruhig.matches ? 1 : 1 - Math.exp(-dt * 4.5);
    stand.hoch += (ziel.hoch - stand.hoch) * k;
    for (var i = 0; i < 3; i++) {
      stand.A[i] += (ziel.A[i] - stand.A[i]) * k;
      stand.B[i] += (ziel.B[i] - stand.B[i]) * k;
    }
    var km = ruhig.matches ? 1 : 1 - Math.exp(-dt * 3.7);
    mausWeich[0] += (maus[0] - mausWeich[0]) * km;
    mausWeich[1] += (maus[1] - mausWeich[1]) * km;
    var weichQ = stand.q * stand.q * (3 - 2 * stand.q);
    var zeit = ruhig.matches ? 12 : sekunden();

    // 1. Wasserflaeche klein
    gl.bindFramebuffer(gl.FRAMEBUFFER, meerFbo);
    gl.viewport(0, 0, meerW, meerH);
    gl.useProgram(um.p);
    gl.uniform2f(um.R, meerW, meerH);
    gl.uniform1f(um.T, zeit);
    gl.uniform1f(um.S, ruhig.matches ? 0 : ziel.lauf || 0);
    gl.uniform3fv(um.W, wellen);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    // Licht: immer schraeg von oben, die Maus dreht nur seine Richtung. Frueher kippte es mit der Maus nach
    // unten rechts bis fast auf den Betrachter, und die ganze ebene Flaeche wurde grau.
    var mx = Math.min(1.6, Math.max(-1.6, mausWeich[0])),
      my = Math.min(1, Math.max(-1, mausWeich[1]));
    function lichtWerte(p, w, h) {
      gl.useProgram(p.p);
      gl.uniform2f(p.R, w, h);
      gl.uniform1f(p.T, zeit);
      gl.uniform2f(p.M, mx, my);
      richtung(p.L, -0.5 + mx * 0.35, 0.7 + my * 0.25, 0.8, 0.6);
      richtung(p.K, mx * 0.7 - 0.25, my * 0.5 + 0.75, 0.62, 0.78);
      gl.uniform1f(p.H, stand.hoch);
      gl.uniform3f(p.A, stand.A[0], stand.A[1], stand.A[2]);
      gl.uniform3f(p.B, stand.B[0], stand.B[1], stand.B[2]);
    }
    // 2. Farbe der freien Flaeche klein
    gl.bindFramebuffer(gl.FRAMEBUFFER, farbFbo);
    lichtWerte(uf, meerW, meerH);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    // 3. Relief und Korn in voller Groesse
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, leinwand.width, leinwand.height);
    lichtWerte(u, leinwand.width, leinwand.height);
    gl.uniform1i(u.X0, 1 - vorn);
    gl.uniform1i(u.X1, vorn);
    gl.uniform1f(u.Q, weichQ);
    gl.uniform1f(u.O, ziel.versatz || 0);
    gl.uniform4f(u.G, massstab[1 - vorn][0], massstab[1 - vorn][1], massstab[vorn][0], massstab[vorn][1]);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    if (weiter) anfordern();
  }
  // Einheitsvektor: waagrechter Anteil (x, y) auf die Laenge `flach` gebracht, senkrecht `steil`
  function richtung(ort, x, y, flach, steil) {
    var l = Math.hypot(x, y) || 1;
    x = (x / l) * flach;
    y = (y / l) * flach;
    l = Math.hypot(x, y, steil);
    gl.uniform3f(ort, x / l, y / l, steil / l);
  }
  function anfordern() {
    if (!wartet) {
      wartet = true;
      requestAnimationFrame(bild);
    }
  }
  function neu() {
    anfordern();
  }
  function ort2(e) {
    return [((e.clientX / innerWidth) * 2 - 1) * (innerWidth / innerHeight), 1 - (e.clientY / innerHeight) * 2];
  }
  groesse();
  stand.n = -2; // das erste Relief sofort schreiben, sobald die Schrift geladen ist
  // Mit der Ersatzschrift waere das Relief erst falsch und wuerde nach dem Laden ein zweites Mal berechnet
  var schriftDa = !document.fonts || document.fonts.status === "loaded";
  function schriftGeladen() {
    if (schriftDa) return;
    schriftDa = true;
    stand.n = -2;
    neu();
  }
  setTimeout(schriftGeladen, 1500);
  addEventListener("resize", function () {
    if (verloren) return;
    groesse();
    // Beim Ziehen am Fenster und wenn auf dem Handy die Adressleiste ein- und ausfaehrt, kommen viele
    // Groessenaenderungen kurz hintereinander: das Relief erst neu berechnen, wenn es eine Weile ruhig ist.
    ruheBis = performance.now() + 220;
    neu();
  });
  document.addEventListener("easymysql-scene", function () {
    if (ruhig.matches) neu();
  });
  if (document.fonts)
    document.fonts.ready.then(function () {
      if (schriftDa) stand.n = -2;
      schriftGeladen();
      neu();
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
  if (ruhig.addEventListener) ruhig.addEventListener("change", neu);
  // Grafiktreiber neu gestartet (z. B. Wechsel der Grafikkarte am Laptop): bis zur Rueckkehr der ruhige Verlauf,
  // danach alles neu einrichten
  leinwand.addEventListener("webglcontextlost", function (e) {
    e.preventDefault();
    verloren = true;
    leinwand.style.visibility = "hidden";
    ohneGl();
  });
  leinwand.addEventListener("webglcontextrestored", function () {
    if (!einrichten()) return;
    verloren = false;
    vorrat = [];
    leinwand.style.visibility = "";
    document.documentElement.classList.remove("no-gl");
    groesse();
    stand.n = -2;
    neu();
  });
  neu();
})();
