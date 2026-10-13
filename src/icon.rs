// Programmsymbol: abgerundetes Quadrat mit blauem Verlauf und einer weissen Datenbank (gestapelte Scheiben).
// Wird sowohl von build.rs (fuer die .ico-Datei) als auch vom Programm selbst (Fenster- und Tray-Symbol)
// verwendet und braucht deshalb nur die Standardbibliothek.

/// Liefert ein RGBA-Bild (size x size) des Symbols.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4; // Supersampling fuer weiche Kanten
    let s = size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (px as f32 + (sx as f32 + 0.5) / SS as f32) / s;
                    let v = (py as f32 + (sy as f32 + 0.5) / SS as f32) / s;
                    let c = sample(u, v);
                    // Vormultipliziert summieren, damit Kanten nicht dunkel ausfransen
                    acc[0] += c[0] * c[3];
                    acc[1] += c[1] * c[3];
                    acc[2] += c[2] * c[3];
                    acc[3] += c[3];
                }
            }
            let n = (SS * SS) as f32;
            let i = ((py * size + px) * 4) as usize;
            if acc[3] > 0.0 {
                out[i] = (acc[0] / acc[3] * 255.0).round() as u8;
                out[i + 1] = (acc[1] / acc[3] * 255.0).round() as u8;
                out[i + 2] = (acc[2] / acc[3] * 255.0).round() as u8;
                out[i + 3] = (acc[3] / n * 255.0).round() as u8;
            }
        }
    }
    out
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Abstand zum Rand eines abgerundeten Rechtecks (negativ = innen)
fn round_rect(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half - radius);
    let qy = y.abs() - (half - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// Farbe an Stelle (u, v) im Einheitsquadrat: (r, g, b, a) von 0 bis 1
fn sample(u: f32, v: f32) -> [f32; 4] {
    // Grundflaeche
    let d = round_rect(u - 0.5, v - 0.5, 0.465, 0.21);
    if d > 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let rgb = |r: u8, g: u8, b: u8| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
    // Verlauf von links oben (hell) nach rechts unten (dunkel)
    let t = ((u + v) / 2.0).clamp(0.0, 1.0);
    let mut c = mix(rgb(0x3D, 0x8B, 0xFF), rgb(0x1B, 0x4B, 0xC9), t);
    // Sanfter Glanz im oberen Teil
    let gloss = (1.0 - v / 0.55).clamp(0.0, 1.0) * 0.14;
    c = mix(c, [1.0, 1.0, 1.0], gloss);

    // Datenbank-Zylinder
    let (cx, rx, ry) = (0.5f32, 0.235f32, 0.075f32);
    let (top, bottom) = (0.27f32, 0.73f32);
    let dx = (u - cx) / rx;
    let ell = |yc: f32| dx * dx + ((v - yc) / ry).powi(2);
    let top_face = ell(top) <= 1.0;
    let body = dx.abs() <= 1.0 && v >= top && v <= bottom;
    let base = ell(bottom) <= 1.0 && v > bottom;
    if top_face {
        // Deckel: reines Weiss mit feinem Blauschimmer zum Rand
        let edge = ell(top);
        return mix_a(mix([1.0, 1.0, 1.0], rgb(0xD6, 0xE6, 0xFF), edge.powi(3) * 0.8), 1.0);
    }
    if body || base {
        // Trennlinien: untere Haelften der Ellipsen bei 1/3 und 2/3
        for k in [1.0f32, 2.0] {
            let yc = top + (bottom - top) * k / 3.0;
            let edge = yc + ry * (1.0 - dx * dx).max(0.0).sqrt();
            if dx.abs() <= 1.0 && (v - edge).abs() < 0.0105 && v >= yc {
                return mix_a(mix(c, rgb(0x1B, 0x4B, 0xC9), 0.35), 1.0);
            }
        }
        // Koerper: Weiss, links etwas heller als rechts (Rundung)
        let shade = 0.86 + 0.14 * (1.0 - ((dx + 0.35).abs() / 1.35).min(1.0));
        return mix_a(mix(rgb(0xB9, 0xD2, 0xFF), [1.0, 1.0, 1.0], shade), 1.0);
    }
    // Kantenglaettung der Flaeche kommt vom Supersampling
    mix_a(c, 1.0)
}

fn mix_a(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_rounded_corners_and_content() {
        for size in [16u32, 32, 64, 256] {
            let img = icon_rgba(size);
            assert_eq!(img.len(), (size * size * 4) as usize);
            let px = |x: u32, y: u32| &img[((y * size + x) * 4) as usize..][..4];
            assert_eq!(px(0, 0)[3], 0, "Ecke ist durchsichtig ({size})");
            assert_eq!(px(size / 2, size - 2 - size / 16)[3], 255, "unten innen ist gefuellt ({size})");
            // Mitte des Deckels ist weiss
            let top = px(size / 2, (size as f32 * 0.27) as u32);
            assert!(top[0] > 240 && top[1] > 240 && top[2] > 240, "{top:?}");
        }
    }
}
