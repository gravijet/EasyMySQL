// Kardinalitaets-Notationen, Marken am Linienende und Bezier-Hilfen.

use super::*;

/// Eine Linie als Folge kubischer Bezier-Segmente (Weltkoordinaten).
pub(super) type Path = Vec<[Pos2; 4]>;

/// Darstellung der Kardinalitaeten
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Notation {
    /// Kraehenfuss (Information Engineering), Standard
    Crow,
    /// Chen: 1 / n / m an den Linienenden
    Chen,
    /// (min,max)-Notation
    MinMax,
}

impl Notation {
    pub(super) const ALL: [Notation; 3] = [Notation::Crow, Notation::Chen, Notation::MinMax];

    pub(super) fn label(self) -> &'static str {
        match self {
            Notation::Crow => crate::i18n::text("Krähenfuß"),
            Notation::Chen => "Chen (1, n, m)",
            Notation::MinMax => "(min,max)",
        }
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Notation::Crow => "crow",
            Notation::Chen => "chen",
            Notation::MinMax => "minmax",
        }
    }
}

/// Art einer gezeichneten Beziehung. Linie laeuft immer vom Kind (Anfang) zum Elternteil (Ende),
/// bei n:m von der ersten zur zweiten Tabelle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Rel {
    /// 1:n (sonst 1:1)
    pub(super) many: bool,
    /// n:m ueber eine (ausgeblendete) Zwischentabelle
    pub(super) nm: bool,
    /// Fremdschluessel darf NULL sein (Kind muss keinen Elternteil haben)
    pub(super) optional: bool,
}

impl Rel {
    pub(super) fn label(self) -> &'static str {
        if self.nm {
            "n:m"
        } else if self.many {
            "1:n"
        } else {
            "1:1"
        }
    }
}

/// Zeichenelement einer Beziehungsmarkierung (Weltkoordinaten)
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Prim {
    Line(Pos2, Pos2),
    Circle(Pos2, f32),
    /// kleine Beschriftung am Linienende, oberhalb der Linie
    Text(Pos2, String, Align2),
    /// umrahmte Beschriftung in der Linienmitte
    Tag(Pos2, String),
    /// Raute (Chen) in der Linienmitte
    Diamond(Pos2, f32),
}

/// Mittelpunkt eines Linienverlaufs
pub(super) fn path_mid(path: &Path) -> Pos2 {
    if path.len() % 2 == 1 {
        sample(&path[path.len() / 2], 0.5)
    } else {
        path[path.len() / 2][0]
    }
}

/// Markierungen fuer eine Beziehung: Anfang = Kind (bzw. erste n:m-Tabelle), Ende = Elternteil.
pub(super) fn markers(path: &Path, rel: Rel, notation: Notation, labels: bool) -> Vec<Prim> {
    let first = path[0];
    let last = path[path.len() - 1];
    let a = first[0];
    let da = (first[1].x - first[0].x).signum();
    let b = last[3];
    let db = (last[2].x - last[3].x).signum();
    let at = |p: Pos2, d: f32, k: f32| p + vec2(d * k, 0.0);
    let bar = |p: Pos2, d: f32, k: f32| {
        let c = at(p, d, k);
        Prim::Line(c + vec2(0.0, -5.0), c + vec2(0.0, 5.0))
    };
    let foot = |p: Pos2, d: f32| {
        let f = at(p, d, 11.0);
        vec![Prim::Line(f, p + vec2(0.0, -6.0)), Prim::Line(f, p + vec2(0.0, 6.0))]
    };
    let text = |p: Pos2, d: f32, s: &str| {
        let align = if d >= 0.0 { Align2::LEFT_BOTTOM } else { Align2::RIGHT_BOTTOM };
        Prim::Text(at(p, d, 6.0) + vec2(0.0, -3.0), s.to_string(), align)
    };
    let mut out = Vec::new();
    match notation {
        Notation::Crow => {
            // Anfang (Kind): viele = Kraehenfuss, eins = Strich
            if rel.many || rel.nm {
                out.extend(foot(a, da));
            } else {
                out.push(bar(a, da, 8.0));
            }
            // Ende (Elternteil): genau eins = zwei Striche, keins oder eins = Strich + Kreis
            if rel.nm {
                out.extend(foot(b, db));
            } else if rel.optional {
                out.push(bar(b, db, 6.0));
                out.push(Prim::Circle(at(b, db, 14.0), 3.5));
            } else {
                out.push(bar(b, db, 6.0));
                out.push(bar(b, db, 10.0));
            }
            if labels {
                out.push(Prim::Tag(path_mid(path), rel.label().into()));
            }
        }
        Notation::Chen => {
            let (sa, sb) = if rel.nm { ("n", "m") } else if rel.many { ("n", "1") } else { ("1", "1") };
            out.push(text(a, da, sa));
            out.push(text(b, db, sb));
            out.push(Prim::Diamond(path_mid(path), 7.0));
        }
        Notation::MinMax => {
            // (min,max): an wie vielen Beziehungen nimmt ein Datensatz dieser Tabelle teil?
            let (sa, sb) = if rel.nm {
                ("(0,n)", "(0,n)")
            } else {
                (if rel.optional { "(0,1)" } else { "(1,1)" }, if rel.many { "(0,n)" } else { "(0,1)" })
            };
            out.push(text(a, da, sa));
            out.push(text(b, db, sb));
        }
    }
    out
}

/// Beschriftungen an Linienenden ohne Ueberlappung setzen (Weltkoordinaten): gleiche Texte
/// am selben Ort werden zusammengefasst, verschiedene nach aussen weggeschoben.
pub(super) fn place_texts<T: Clone>(items: Vec<(Pos2, String, Align2, T)>) -> Vec<(Pos2, String, Align2, T)> {
    // Gitter aus Zellen, damit nur benachbarte Beschriftungen verglichen werden (nicht alle)
    pub(super) const CELL: f32 = 64.0;
    let cells_of = |r: Rect| {
        let (x0, x1) = ((r.min.x / CELL).floor() as i32, (r.max.x / CELL).floor() as i32);
        let (y0, y1) = ((r.min.y / CELL).floor() as i32, (r.max.y / CELL).floor() as i32);
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    };
    let mut placed: Vec<(Rect, String)> = Vec::new();
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let mut out = Vec::new();
    'items: for (mut p, s, align, extra) in items {
        let w = s.chars().count() as f32 * 6.0 + 2.0;
        let left = align == Align2::LEFT_BOTTOM;
        let rect_at = |p: Pos2| {
            if left {
                Rect::from_min_max(pos2(p.x, p.y - 11.0), pos2(p.x + w, p.y))
            } else {
                Rect::from_min_max(pos2(p.x - w, p.y - 11.0), pos2(p.x, p.y))
            }
        };
        for _ in 0..8 {
            let r = rect_at(p);
            // zuerst gesetzte Beschriftung, die sich mit r ueberlappt
            let hit = cells_of(r)
                .filter_map(|c| grid.get(&c))
                .flatten()
                .copied()
                .filter(|&k| placed[k].0.intersects(r))
                .min();
            let Some(k) = hit else { break };
            let (o, os) = &placed[k];
            if *os == s {
                continue 'items;
            }
            p.x += if left { o.max.x + 3.0 - r.min.x } else { o.min.x - 3.0 - r.max.x };
        }
        let r = rect_at(p);
        for c in cells_of(r) {
            grid.entry(c).or_default().push(placed.len());
        }
        placed.push((r, s.clone()));
        out.push((p, s, align, extra));
    }
    out
}

pub(super) fn bezier_between(a: Pos2, b: Pos2, da: f32, db: f32) -> [Pos2; 4] {
    let bend = ((b.x - a.x).abs() * 0.5).max(30.0);
    [a, a + vec2(da * bend, 0.0), b + vec2(db * bend, 0.0), b]
}

pub(super) fn sample(seg: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let p = seg[0].to_vec2() * (u * u * u)
        + seg[1].to_vec2() * (3.0 * u * u * t)
        + seg[2].to_vec2() * (3.0 * u * t * t)
        + seg[3].to_vec2() * (t * t * t);
    p.to_pos2()
}

pub(super) fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}
