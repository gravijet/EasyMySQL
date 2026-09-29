// Programmatisch gezeichnetes Programmsymbol (Datenbank-Zylinder).
// Wird sowohl von build.rs (fuer die .ico-Datei) als auch vom Programm
// selbst (Fenster- und Tray-Symbol) verwendet.

/// Liefert ein RGBA-Bild (size x size) des Symbols.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4; // Supersampling fuer weiche Kanten
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];

    let cx = s / 2.0;
    let rx = s * 0.38;
    let ry = s * 0.12;
    let top = s * 0.18;
    let bottom = s * 0.82;
    let line = (s / 32.0).max(1.0);

    for py in 0..size {
        for px in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    let c = sample(x, y, cx, rx, ry, top, bottom, line);
                    let a = c[3] as f32 / 255.0;
                    acc[0] += c[0] as f32 * a;
                    acc[1] += c[1] as f32 * a;
                    acc[2] += c[2] as f32 * a;
                    acc[3] += a;
                }
            }
            let n = (SS * SS) as f32;
            let i = ((py * size + px) * 4) as usize;
            if acc[3] > 0.0 {
                out[i] = (acc[0] / acc[3]) as u8;
                out[i + 1] = (acc[1] / acc[3]) as u8;
                out[i + 2] = (acc[2] / acc[3]) as u8;
                out[i + 3] = (acc[3] / n * 255.0) as u8;
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn sample(x: f32, y: f32, cx: f32, rx: f32, ry: f32, top: f32, bottom: f32, line: f32) -> [u8; 4] {
    let outline = [0x10, 0x20, 0x50, 255];
    let body = [0x2a, 0x5d, 0xb0, 255];
    let body_light = [0x5a, 0x8d, 0xe0, 255];
    let lid = [0x9c, 0xc0, 0xf4, 255];
    let band = [0x18, 0x3c, 0x80, 255];

    let dx = (x - cx) / rx;
    let in_x = dx.abs() <= 1.0;
    let e = |yc: f32, y: f32| -> f32 {
        // Abstand in "Ellipsen-Einheiten" zum Ellipsenrand bei Mitte yc
        let dy = (y - yc) / ry;
        dx * dx + dy * dy
    };
    let edge = |v: f32, r: f32| -> bool {
        // v ist normierter Radius^2, r die Linienstaerke relativ
        v <= 1.0 && v >= (1.0 - r).powi(2)
    };
    let rel = line / rx.min(ry) * 1.2;

    // Deckel (obere Ellipse)
    let et = e(top, y);
    if et <= 1.0 {
        if edge(et, rel) {
            return outline;
        }
        return lid;
    }
    // Koerper
    if in_x && y >= top && y <= bottom {
        // Rand links/rechts
        if dx.abs() >= 1.0 - line / rx {
            return outline;
        }
        // Baender (untere Ellipsenhaelften)
        for k in [1.0f32, 2.0] {
            let yc = top + (bottom - top) * k / 3.0;
            let eb = e(yc, y);
            if y > yc && eb <= 1.0 && eb >= (1.0 - rel).powi(2) {
                return band;
            }
        }
        if dx < -0.35 && dx > -0.75 {
            return body_light;
        }
        return body;
    }
    // Boden (untere Ellipsenhaelfte)
    let eb = e(bottom, y);
    if eb <= 1.0 && y >= bottom {
        if edge(eb, rel) {
            return outline;
        }
        if dx < -0.35 && dx > -0.75 {
            return body_light;
        }
        return body;
    }
    [0, 0, 0, 0]
}
