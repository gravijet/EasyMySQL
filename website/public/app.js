(function () {
  "use strict";
  var root = document.documentElement;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var clamp = function (v, a, b) { return v < a ? a : v > b ? b : v; };
  var still = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---- Sprache -------------------------------------------------------------
  var lang = root.lang === "en" ? "en" : "de";
  var texts = $$("[data-en]").map(function (el) { return [el, el.textContent]; });
  var alts = $$("[data-en-alt]").map(function (el) { return [el, el.alt]; });
  var labels = $$("[data-en-label]").map(function (el) { return [el, el.getAttribute("aria-label")]; });
  var plain = $$("[data-split]:not([data-en])").map(function (el) { return [el, el.textContent]; });
  var desc = {
    de: "MariaDB-Server und Datenbank-Werkzeug für Windows: SQL-Editor, Tabellen, Diagramme, Import und Export, Java, Python und JavaScript.",
    en: "MariaDB server and database tool for Windows: SQL editor, tables, diagrams, import and export, Java, Python and JavaScript.",
  };
  var rel = null;

  // Woerter einzeln einpacken: im Satz zum Aufleuchten, sonst zum Aufsteigen
  function split(el, text) {
    var say = el.classList.contains("say");
    el.textContent = "";
    text.split(/\s+/).filter(Boolean).forEach(function (w, i) {
      if (i) el.appendChild(document.createTextNode(" "));
      var s = document.createElement("span");
      s.textContent = w;
      if (say) { s.className = "w"; el.appendChild(s); return; }
      s.className = "wi";
      s.style.setProperty("--i", i);
      var l = document.createElement("span");
      l.className = "wl";
      l.appendChild(s);
      el.appendChild(l);
    });
    if (say) el.setAttribute("aria-label", text);
  }

  function applyLang(l) {
    lang = l;
    root.lang = l;
    texts.forEach(function (p) {
      var t = l === "en" ? p[0].getAttribute("data-en") : p[1];
      if (p[0].hasAttribute("data-split")) split(p[0], t); else p[0].textContent = t;
    });
    plain.forEach(function (p) { split(p[0], p[1]); });
    alts.forEach(function (p) { p[0].alt = l === "en" ? p[0].getAttribute("data-en-alt") : p[1]; });
    labels.forEach(function (p) { p[0].setAttribute("aria-label", l === "en" ? p[0].getAttribute("data-en-label") : p[1]); });
    $$(".lang button").forEach(function (b) { b.setAttribute("aria-pressed", String(b.dataset.lang === l)); });
    $('meta[name="description"]').content = desc[l];
    words = $$(".say .w");
    lit = -1;
    if (rel) render(rel);
    tick();
  }
  $$(".lang button").forEach(function (b) {
    b.addEventListener("click", function () {
      try { localStorage.setItem("em-lang", b.dataset.lang); } catch (e) {}
      applyLang(b.dataset.lang);
    });
  });

  // ---- Neueste Version -----------------------------------------------------
  function mb(n) {
    var v = n / 1048576;
    return (v >= 100 ? Math.round(v) : v.toFixed(1)).toString().replace(".", lang === "de" ? "," : ".") + " MB";
  }
  function render(r) {
    var f = r.files || {}, s = f.setup;
    var parts = ["Version " + r.version];
    if (r.published) {
      var d = new Date(r.published);
      if (!isNaN(d)) parts.push(d.toLocaleDateString(lang === "de" ? "de-AT" : "en-GB", { day: "numeric", month: "long", year: "numeric" }));
    }
    if (s && s.size) parts.push(mb(s.size));
    $("#dl-info").textContent = parts.join(" · ");
    $$("[data-dl]").forEach(function (a) { var x = f[a.dataset.dl]; if (x && x.url) a.href = x.url; });
    var sha = $("#sha");
    sha.hidden = !(s && s.sha256);
    sha.dataset.sum = (s && s.sha256) || "";
  }
  try { var c = JSON.parse(localStorage.getItem("em-rel") || "null"); if (c && c.version) { rel = c; render(c); } } catch (e) {}
  fetch("/api/release", { headers: { accept: "application/json" } })
    .then(function (r) { if (!r.ok) throw 0; return r.json(); })
    .then(function (r) {
      rel = r; render(r);
      try { localStorage.setItem("em-rel", JSON.stringify(r)); } catch (e) {}
    })
    .catch(function () {});

  var toast;
  $("#sha").addEventListener("click", function () {
    var sum = this.dataset.sum;
    var done = function () {
      if (!toast) { toast = document.createElement("div"); toast.className = "toast"; toast.setAttribute("role", "status"); document.body.appendChild(toast); }
      toast.textContent = lang === "de" ? "SHA-256 kopiert" : "SHA-256 copied";
      toast.classList.add("on");
      clearTimeout(toast.t);
      toast.t = setTimeout(function () { toast.classList.remove("on"); }, 2200);
    };
    if (navigator.clipboard) navigator.clipboard.writeText(sum).then(done, function () { prompt("SHA-256", sum); });
    else prompt("SHA-256", sum);
  });

  // ---- Erscheinen beim Scrollen -------------------------------------------
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -12% 0px" });
    $$("[data-reveal], .formats h2").forEach(function (el) { io.observe(el); });
  } else {
    $$("[data-reveal], .formats h2").forEach(function (el) { el.classList.add("in"); });
  }

  // ---- Scrollen --------------------------------------------------------------
  var bar = $(".bar"), scrub = $(".scrub"), tour = $(".tour"), tilt = $(".tilt");
  var chs = $$(".ch"), shots = $$(".shot"), steps = $$(".steps button"), now = $(".count .now");
  var rows = $$(".row"), rowsBox = $(".rows"), get = $(".get");
  var n = chs.length, cur = 0, words = [], lit = -1, lastY = 0, m = {};
  var px = 0, py = 0, tx = 0, ty = 0, fine = matchMedia("(pointer: fine)").matches;

  function measure() {
    var y = scrollY, top = function (el) { return el.getBoundingClientRect().top + y; };
    m.vh = innerHeight; m.vw = innerWidth;
    m.scrub = top(scrub); m.scrubH = scrub.offsetHeight;
    m.tour = top(tour); m.tourH = tour.offsetHeight;
    m.rows = top(rowsBox); m.rowsH = rowsBox.offsetHeight;
    m.get = top(get);
    m.rowW = rows.map(function (r) { return r.scrollWidth; });
  }

  function show(i) {
    if (i === cur) return;
    tour.dataset.dir = i > cur ? "down" : "up";
    shots.forEach(function (s, k) { s.classList.toggle("was", k === cur); s.classList.toggle("on", k === i); });
    chs.forEach(function (c, k) { c.classList.toggle("on", k === i); });
    now.textContent = "0" + (i + 1);
    cur = i;
  }

  // Wie weit der Himmel in welcher Stimmung ist (0 Start, 1 Rundgang, 2 Formate, 3 Download)
  var scene = 0;
  function sceneAt(y) {
    var a = [0, m.tour - m.vh * .3, m.tour + m.tourH - m.vh * .6, m.get - m.vh * .4];
    for (var i = a.length - 1; i > 0; i--) if (y >= a[i - 1]) return i - 1 + clamp((y - a[i - 1]) / Math.max(1, a[i] - a[i - 1]), 0, 1);
    return 0;
  }

  var queued = false;
  function tick() { if (!queued) { queued = true; requestAnimationFrame(frame); } }
  function frame() {
    queued = false;
    var y = scrollY, vh = m.vh;

    if (y > lastY + 6 && y > vh * .6) bar.classList.add("away");
    else if (y < lastY - 6 || y < vh * .6) bar.classList.remove("away");
    lastY = y;

    var p = clamp((y - m.scrub) / (m.scrubH - vh), 0, 1);
    var k = Math.round(p * 1.2 * words.length);
    if (k !== lit) { words.forEach(function (w, i) { w.classList.toggle("lit", i < k); }); lit = k; }

    // Das Fenster kommt flach gekippt herein und richtet sich auf
    var e = clamp(1 - (m.tour - y) / vh, 0, 1);
    e = 1 - Math.pow(1 - e, 3);
    px += (tx - px) * .08; py += (ty - py) * .08;
    var rx = (1 - e) * 34 - py * 3, ry = px * 4;
    tilt.style.transform = "translate3d(0," + ((1 - e) * 22).toFixed(2) + "vh,0) scale(" + (.8 + .2 * e).toFixed(4) + ") rotateX(" + rx.toFixed(2) + "deg) rotateY(" + ry.toFixed(2) + "deg)";

    var q = clamp((y - m.tour) / (m.tourH - vh), 0, 1) * n;
    show(Math.min(n - 1, Math.floor(q)));
    steps.forEach(function (s, i) { s.style.setProperty("--p", clamp(q - i, 0, 1).toFixed(3)); });

    var f = clamp((y + vh - m.rows) / (vh + m.rowsH), 0, 1);
    rows.forEach(function (r, i) {
      var span = Math.max(0, m.rowW[i] - m.vw);
      r.style.transform = "translate3d(" + (-(i ? 1 - f : f) * span).toFixed(1) + "px,0,0)";
    });

    scene = sceneAt(y);
    if (still && sky) sky.draw(0);
    if (Math.abs(tx - px) + Math.abs(ty - py) > .002) tick();
  }

  steps.forEach(function (s, i) {
    s.addEventListener("click", function () {
      scrollTo({ top: m.tour + (i + .05) / n * (m.tourH - m.vh), behavior: still ? "auto" : "smooth" });
    });
  });
  if (fine) addEventListener("pointermove", function (ev) { tx = ev.clientX / m.vw - .5; ty = ev.clientY / m.vh - .5; tick(); }, { passive: true });
  addEventListener("scroll", tick, { passive: true });
  addEventListener("resize", function () { measure(); tick(); });
  if ("ResizeObserver" in window) new ResizeObserver(function () { measure(); tick(); }).observe(document.body);

  measure();
  applyLang(lang);

  // ---- Himmel (WebGL) --------------------------------------------------------
  // Je Stimmung: Grund, Wolke, Licht, Helligkeit
  var moods = [
    [[.012, .02, .05], [.05, .16, .45], [.3, .6, 1], 1],
    [[.008, .012, .03], [.03, .08, .24], [.2, .4, .9], .7],
    [[.004, .03, .035], [.02, .26, .3], [.25, 1, .85], .85],
    [[.03, .014, .004], [.42, .2, .02], [1, .72, .25], .9],
  ];
  var sky = null;
  function startSky() {
    var cv = $(".sky"), gl = cv.getContext("webgl", { antialias: false, alpha: false, depth: false, powerPreference: "low-power" });
    if (!gl) return;
    var vs = "attribute vec2 a;void main(){gl_Position=vec4(a,0.,1.);}";
    var fs = [
      "precision highp float;uniform vec2 R,P;uniform float T,K,O,S,Y;uniform vec3 A,B,C;uniform mat3 W;",
      "float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}",
      "float n(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);return mix(mix(h(i),h(i+vec2(1,0)),f.x),mix(h(i+vec2(0,1)),h(i+1.),f.x),f.y);}",
      "float fb(vec2 p){return .5*n(p)+.3*n(p*2.1+7.)+.2*n(p*4.3+3.);}",
      // Drei abgerundete Scheiben uebereinander, wie das Logo
      "float disc(vec3 p){vec2 d=vec2(length(p.xz)-.56,abs(p.y)-.04);return min(max(d.x,d.y),0.)+length(max(d,0.))-.1;}",
      "vec3 tw(vec3 p){return W*p;}",
      "float map(vec3 p){p=W*p;vec3 o=vec3(p.x,abs(p.y)-Y,p.z);return min(disc(p),disc(o));}",
      "vec3 nrm(vec3 p){vec2 e=vec2(.002,-.002);return normalize(e.xyy*map(p+e.xyy)+e.yyx*map(p+e.yyx)+e.yxy*map(p+e.yxy)+e.xxx*map(p+e.xxx));}",
      "vec3 env(vec3 d){vec2 k=d.xy-vec2(.65,.4),j=d.xy+vec2(.8,.05);return mix(vec3(1.),C,.35)*(smoothstep(.2,1.,d.y)*.1+exp(-pow((d.x+d.y*.5-.15)*9.,2.))*smoothstep(0.,.5,d.y)*1.4)+vec3(1.)*(exp(-dot(k,k)*45.)*3.+exp(-dot(k,k)*8.)*.5+exp(-dot(j,j)*10.)*.4);}",
      "vec3 sky(vec2 p){float t=T*.035;vec2 q=vec2(fb(p*.9+t),fb(p*.9-t+5.2));float f=fb(p*.8+q*1.4+vec2(t,-t*.6));",
      "vec3 c=mix(A,B,smoothstep(.28,.85,f));c+=C*pow(smoothstep(.55,1.,f),2.)*.35;return c*K*(1.-.45*length(p*vec2(.6,1.)));}",
      "void main(){vec2 p=(gl_FragCoord.xy-.5*R)/R.y;vec3 c=sky(p);",
      "if(O>.001){vec2 q=(p-P)/S;vec3 ro=vec3(0,0,3.4),rd=normalize(vec3(q,-1.9));",
      "float bb=dot(ro,rd);float hh=bb*bb-dot(ro,ro)+1.05;",
      "if(hh>0.){float t=-bb-sqrt(hh),d=1.;for(int i=0;i<56;i++){d=map(ro+rd*t);if(d<.0015||t>5.)break;t+=d;}",
      "if(d<.004){vec3 x=ro+rd*t,nn=nrm(x),v=-rd;float ct=max(dot(nn,v),0.),fr=pow(1.-ct,2.6);",
      "vec3 film=.5+.5*cos(6.2832*(vec3(.58,.68,.8)+.9*ct+.05*T));",
      "vec3 side=mix(vec3(1.,.16,.3),C,smoothstep(-.6,.6,tw(nn).x));",
      "vec3 o=vec3(.006,.008,.02)+side*pow(1.-ct,1.4)*.55+film*fr*1.25+env(reflect(rd,nn))*(.1+fr*.9);",
      "c=mix(c,o,O);}}}",
      "c+=(h(gl_FragCoord.xy+fract(T)*61.)-.5)*.018;gl_FragColor=vec4(max(c,0.),1.);}",
    ].join("\n");
    function sh(t, src) { var s = gl.createShader(t); gl.shaderSource(s, src); gl.compileShader(s); return s; }
    var pr = gl.createProgram();
    gl.attachShader(pr, sh(gl.VERTEX_SHADER, vs));
    gl.attachShader(pr, sh(gl.FRAGMENT_SHADER, fs));
    gl.linkProgram(pr);
    if (!gl.getProgramParameter(pr, gl.LINK_STATUS)) { console.error(gl.getProgramInfoLog(pr)); return; }
    gl.useProgram(pr);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    var u = {};
    ["R", "P", "T", "K", "O", "S", "Y", "A", "B", "C", "W"].forEach(function (k) { u[k] = gl.getUniformLocation(pr, k); });

    // Unter voller Aufloesung: der Himmel ist weich, nur die Scheiben brauchen Schaerfe
    var ar = 1, qual = 1;
    function size() {
      var d = devicePixelRatio || 1, s = qual * Math.min(d > 1 ? .75 * Math.min(d, 2) : 1, 1920 / innerWidth);
      cv.width = Math.round(innerWidth * s); cv.height = Math.round(innerHeight * s);
      ar = innerWidth / innerHeight;
      gl.viewport(0, 0, cv.width, cv.height);
      gl.uniform2f(u.R, cv.width, cv.height);
    }
    size();
    addEventListener("resize", size);

    var cs = scene, mx = 0, my = 0, v = [0, 0, 0], W = new Float32Array(9);
    // Drehung um x (a), dann z (b), dann y (c), Zeilen wie in der Mathematik; WebGL will Spalten
    function rotm(a, b, c) {
      var ca = Math.cos(a), sa = Math.sin(a), cb = Math.cos(b), sb = Math.sin(b), cc = Math.cos(c), sc = Math.sin(c);
      var m = mul([cc, 0, -sc, 0, 1, 0, sc, 0, cc], mul([cb, -sb, 0, sb, cb, 0, 0, 0, 1], [1, 0, 0, 0, ca, -sa, 0, sa, ca]));
      for (var i = 0; i < 3; i++) for (var j = 0; j < 3; j++) W[j * 3 + i] = m[i * 3 + j];
    }
    function mul(a, b) {
      var r = [];
      for (var i = 0; i < 3; i++) for (var j = 0; j < 3; j++) r[i * 3 + j] = a[i * 3] * b[j] + a[i * 3 + 1] * b[3 + j] + a[i * 3 + 2] * b[6 + j];
      return r;
    }
    function lerp3(a, b, w) { for (var j = 0; j < 3; j++) v[j] = a[j] + (b[j] - a[j]) * w; return v; }
    // Uebergaenge nach Zeit, nicht nach Bildern: auf langsamen Geraeten gleich schnell
    var prev = 0;
    function draw(t) {
      var k = still || !prev ? 1 : 1 - Math.exp(-Math.min(t - prev, 250) / 300);
      prev = t;
      cs += (scene - cs) * k;
      mx += (px - mx) * k * .7; my += (py - my) * k * .7;
      var i = Math.min(2, Math.floor(cs)), w = cs - i, a = moods[i], b = moods[i + 1];
      gl.uniform3fv(u.A, lerp3(a[0], b[0], w));
      gl.uniform3fv(u.B, lerp3(a[1], b[1], w));
      gl.uniform3fv(u.C, lerp3(a[2], b[2], w));
      gl.uniform1f(u.K, a[3] + (b[3] - a[3]) * w);
      // Die Scheiben stehen rechts neben dem Schriftzug (am Handy darueber) und steigen beim Scrollen davon
      var off = clamp(scrollY / m.vh, 0, 1.4), port = ar < 1;
      gl.uniform1f(u.O, clamp(1.25 - off, 0, 1));
      gl.uniform1f(u.S, port ? .28 : .46);
      gl.uniform2f(u.P, port ? .06 : ar * .5 - .6 - .05 * off, (port ? .08 : .02) + off * .6);
      solid = off < 1.25;
      // Neigung zur Maus, Wiegen und Drehen: einmal je Bild statt in jedem Pixel
      var T = 20 + t / 1000;
      rotm(-.5 + my * .35 + .06 * Math.sin(T * .5), -.16 + mx * .3 + .05 * Math.sin(T * .37), T * .15);
      gl.uniformMatrix3fv(u.W, false, W);
      gl.uniform1f(u.Y, .36 + .045 * Math.sin(T * .9));
      gl.uniform1f(u.T, T);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    }
    sky = { draw: draw };
    // Zu langsame Bilder: Aufloesung senken, ganz unten bleibt der Himmel stehen (dann zeichnet nur noch das Scrollen)
    var run = false, solid = true, last = 0, slow = 0, odd = false;
    function loop(t) {
      if (!run) return;
      var dt = last ? t - last : 16;
      last = t;
      slow = slow * .9 + (dt > 28 ? .1 : 0);
      if (slow > .6) {
        slow = 0;
        if (qual > .45) { qual *= .75; size(); } else { run = false; still = true; return; }
      }
      // Ohne Scheiben reichen dem langsamen Himmel halb so viele Bilder
      odd = !odd;
      if (solid || odd) draw(t);
      requestAnimationFrame(loop);
    }
    function go() {
      if (still) { draw(0); return; }
      var on = !document.hidden;
      if (on && !run) { run = true; requestAnimationFrame(loop); } else if (!on) run = false;
    }
    document.addEventListener("visibilitychange", go);
    go();
    requestAnimationFrame(function () { cv.classList.add("on"); });
  }
  (window.requestIdleCallback || function (f) { setTimeout(f, 200); })(startSky, { timeout: 1200 });
})();
