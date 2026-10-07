// Weiche, dunkle Glaskugeln im Hintergrund des Starts. Reine 2D-Rechnung auf einem
// kleinen Bild, das vom Browser hochskaliert wird. Steht still, wenn der Start nicht
// sichtbar ist oder Bewegung reduziert werden soll.
(function () {
  "use strict";
  var cv = document.getElementById("orbs");
  if (!cv || !cv.getContext) return;
  var ctx = cv.getContext("2d", { alpha: false });
  var reduce = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
  var hero = cv.parentNode;

  // Position (Anteil der Breite/Höhe), Radius (Anteil der Höhe), Bewegung
  var balls = [
    { x: .53, y: .36, r: .165, ax: .035, ay: .05, sp: .23, ph: 0.0 },
    { x: .66, y: .58, r: .115, ax: .05, ay: .035, sp: .19, ph: 1.7 },
    { x: .38, y: .24, r: .07, ax: .04, ay: .05, sp: .31, ph: 3.1 },
    { x: .15, y: .50, r: .105, ax: .03, ay: .04, sp: .17, ph: 4.4 },
    { x: .85, y: .30, r: .085, ax: .045, ay: .03, sp: .27, ph: 2.2 },
    { x: .30, y: .66, r: .05, ax: .05, ay: .05, sp: .35, ph: 5.2 }
  ];
  var W = 0, H = 0, img, buf, pos = [], t0 = performance.now(), run = false, raf = 0, hue = 36;

  function size() {
    var r = cv.getBoundingClientRect();
    if (!r.width || !r.height) return false;
    var s = Math.min(1, 460 / r.width);
    W = Math.max(8, Math.round(r.width * s));
    H = Math.max(8, Math.round(r.height * s));
    cv.width = W; cv.height = H;
    img = ctx.createImageData(W, H);
    buf = img.data;
    return true;
  }

  function hsl(h, s, l) {
    h = ((h % 360) + 360) % 360 / 360;
    var a = s * Math.min(l, 1 - l);
    function f(n) {
      var k = (n + h * 12) % 12;
      return 255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
    }
    return [f(0), f(8), f(4)];
  }

  function frame(now) {
    var t = (now - t0) / 1000;
    var asp = W / H, i, n = balls.length, rs = Math.min(1, asp * 0.85);
    for (i = 0; i < n; i++) {
      var b = balls[i];
      pos[i] = [
        (b.x + Math.sin(t * b.sp + b.ph) * b.ax) * asp,
        b.y + Math.cos(t * b.sp * 1.3 + b.ph) * b.ay,
        b.r * b.r * rs * rs
      ];
    }
    var hi = hsl(hue, .9, .82), rim = hsl(hue + 150, .8, .55), rim2 = hsl(hue + 230, .8, .5);
    var o = 0;
    for (var py = 0; py < H; py++) {
      var v = py / H;
      for (var px = 0; px < W; px++, o += 4) {
        var u = px / H, F = 0, gx = 0, gy = 0;
        for (i = 0; i < n; i++) {
          var p = pos[i], dx = u - p[0], dy = v - p[1];
          var d2 = dx * dx + dy * dy + 1e-5, q = p[2] / d2;
          F += q; var k = q / d2; gx -= 2 * dx * k; gy -= 2 * dy * k;
        }
        // Hintergrund mit leichter Aufhellung zum Rand der Kugeln
        var R = 9, G = 11, B = 18;
        if (F > 0.25) {
          var glow = Math.min(1, (F - 0.25) / 0.75); glow = glow * glow * glow;
          R += 8 * glow; G += 10 * glow; B += 22 * glow;
        }
        if (F > 0.9) {
          var inside = Math.min(1, (F - 0.9) / 0.14); inside = inside * inside * (3 - 2 * inside);
          var gl = Math.sqrt(gx * gx + gy * gy) + 1e-6;
          var nx = -gx / gl, ny = -gy / gl;          // nach außen
          var h = Math.sqrt(Math.max(0, 1 - 1 / F)); // 0 am Rand, 1 in der Mitte
          var e = Math.sqrt(Math.max(0, 1 - h * h));
          var sx = nx * e, sy = ny * e, sz = h;
          // Grundfarbe: fast schwarz, warm links, kühl rechts
          var warm = Math.pow(Math.max(0, -sx), 1.4) * (0.35 + 0.65 * e);
          var cool = Math.pow(Math.max(0, sx), 1.4) * (0.35 + 0.65 * e);
          var cr = 6 + 70 * warm + 8 * cool, cg = 7 + 38 * warm + 24 * cool, cb = 12 + 6 * warm + 74 * cool;
          // dünner schillernder Rand
          var rr = Math.pow(1 - h, 14), mixr = 0.5 + 0.5 * Math.sin(Math.atan2(ny, nx) * 1.2 + t * 0.4);
          cr += rr * (rim[0] * mixr + rim2[0] * (1 - mixr)) * 0.75;
          cg += rr * (rim[1] * mixr + rim2[1] * (1 - mixr)) * 0.75;
          cb += rr * (rim[2] * mixr + rim2[2] * (1 - mixr)) * 0.75;
          // Glanzlicht oben links
          var dot = sx * -0.38 + sy * -0.78 + sz * 0.5;
          var sp = Math.pow(Math.max(0, dot), 38);
          var sp2 = Math.pow(Math.max(0, dot), 6) * 0.12;
          cr += (hi[0] * sp) + hi[0] * sp2 * 0.5; cg += (hi[1] * sp) + hi[1] * sp2 * 0.5; cb += (hi[2] * sp) + hi[2] * sp2 * 0.5;
          R += (cr - R) * inside; G += (cg - G) * inside; B += (cb - B) * inside;
        }
        buf[o] = R > 255 ? 255 : R; buf[o + 1] = G > 255 ? 255 : G; buf[o + 2] = B > 255 ? 255 : B; buf[o + 3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
  }

  function loop(now) {
    if (!run) return;
    var y = window.scrollY || 0;
    hue = 36 + Math.min(1, y / (window.innerHeight * 1.2)) * 300;
    frame(now);
    raf = requestAnimationFrame(loop);
  }
  function start() { if (run || reduce) return; run = true; raf = requestAnimationFrame(loop); }
  function stop() { run = false; cancelAnimationFrame(raf); }

  if (!size()) return;
  frame(performance.now() + 2000);
  if (reduce) return;
  var visible = true;
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(function (es) {
      visible = es[0].isIntersecting;
      if (visible && !document.hidden) start(); else stop();
    }).observe(hero);
  }
  document.addEventListener("visibilitychange", function () { if (document.hidden) stop(); else if (visible) start(); });
  var rt;
  window.addEventListener("resize", function () {
    clearTimeout(rt);
    rt = setTimeout(function () { if (size()) frame(performance.now()); }, 150);
  });
  start();
})();
