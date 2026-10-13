// Kastengroessen und Linienverlauf: einmal berechnet, danach nur noch gezeichnet.

use super::*;

/// Anfang, Richtung am Anfang, Ende, Richtung am Ende und ggf. gespeicherte Fahrspur einer Beziehung.
pub(super) type Ends<'a> = (Pos2, f32, Pos2, f32, Option<&'a Vec<Pos2>>);

/// Anschlusspunkt (x, y als Bitmuster) und Richtung: Linien mit gleichem Schluessel werden aufgefaechert.
type PortKey = (u32, u32, i32);

/// Alles, was sich nur mit Struktur, Positionen oder Notation aendert (nicht mit Zoom/Versatz):
/// wird einmal berechnet und dann in jedem Bild nur noch gezeichnet.
pub(super) struct RouteGeometry {
    pub(super) paths: Vec<Option<Path>>,
    pub(super) polylines: Vec<Vec<Pos2>>,
    /// Umrandung jeder Linie (Weltkoordinaten) zum schnellen Aussortieren ausserhalb des Bildes
    pub(super) bounds: Vec<Rect>,
    /// Schluessel (`fk_key`) jeder Beziehung
    pub(super) keys: Vec<String>,
    /// Linien, Kreise und Rauten der Kardinalitaetsmarken je Beziehung
    pub(super) shapes: Vec<Vec<Prim>>,
    /// Umrahmte Beschriftungen in der Linienmitte: (Beziehung, Ort, Text)
    pub(super) tags: Vec<(usize, Pos2, String)>,
    /// Beschriftungen an den Linienenden, schon ueberlappungsfrei: (Beziehung, Ort, Text, Ausrichtung)
    pub(super) texts: Vec<(usize, Pos2, String, Align2)>,
}

pub(super) struct RouteCache {
    pub(super) schema: Arc<Schema>,
    pub(super) positions: HashMap<String, Pos2>,
    pub(super) routes: HashMap<String, Vec<Pos2>>,
    pub(super) show_types: bool,
    pub(super) notation: Notation,
    pub(super) rel_labels: bool,
    pub(super) geometry: Arc<RouteGeometry>,
}

/// Kastengroessen aller Tabellen; sie zu berechnen kostet pro Aufruf alle Spaltennamen zu zaehlen.
pub(super) struct BoxCache {
    pub(super) schema: Arc<Schema>,
    pub(super) show_types: bool,
    pub(super) sizes: HashMap<String, Vec2>,
}

pub(super) fn box_size(t: &TableInfo, show_types: bool) -> Vec2 {
    let mut chars = t.name.chars().count() + 4;
    for c in &t.columns {
        let mut n = c.name.chars().count() + 5;
        if show_types {
            n += c.col_type.chars().count() + 2;
        }
        chars = chars.max(n);
    }
    let w = (chars as f32 * CHAR_W + 22.0).clamp(130.0, 420.0);
    let h = HEADER_H + ROW_H * t.columns.len().max(1) as f32 + 6.0;
    vec2(w, h)
}

pub(super) fn fk_key(fk: &ForeignKey) -> String {
    format!("{}|{}", fk.table, fk.name)
}

/// Mitte (y) einer Spaltenzeile relativ zur Kastenoberkante.
pub(super) fn row_y(t: &TableInfo, col: &str) -> f32 {
    let i = t.columns.iter().position(|c| c.name == col).unwrap_or(0);
    HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0
}

impl ErTab {
    /// Groesse der Tabellenkarte, aus dem Zwischenspeicher (siehe `ensure_boxes`)
    pub(super) fn box_of(&self, t: &TableInfo) -> Vec2 {
        if let (Some(c), Some(s)) = (&self.box_cache, &self.schema)
            && c.show_types == self.show_types && Arc::ptr_eq(&c.schema, s)
                && let Some(v) = c.sizes.get(&t.name) {
                    return *v;
                }
        box_size(t, self.show_types)
    }

    pub(super) fn ensure_boxes(&mut self) {
        let Some(schema) = &self.schema else { return };
        if self.box_cache.as_ref().is_some_and(|c| c.show_types == self.show_types && Arc::ptr_eq(&c.schema, schema)) {
            return;
        }
        let sizes = schema.tables.iter().map(|t| (t.name.clone(), box_size(t, self.show_types))).collect();
        self.box_cache = Some(BoxCache { schema: Arc::clone(schema), show_types: self.show_types, sizes });
    }

    pub(super) fn table_rect_world(&self, t: &TableInfo) -> Rect {
        let p = self.pos.get(&t.name).copied().unwrap_or_default();
        Rect::from_min_size(p, self.box_of(t))
    }

    /// Endpunkte einer Beziehung: (Anfang am Kind, Richtung, Ende am Elternteil, Richtung, Fahrspuren).
    /// Richtung = +1 nach rechts aus der Tabelle heraus, -1 nach links.
    pub(super) fn endpoints(&self, schema: &Schema, fk: &ForeignKey) -> Option<Ends<'_>> {
        let src = schema.table(&fk.table)?;
        let dst = schema.table(&fk.ref_table)?;
        let sr = self.table_rect_world(src);
        let dr = self.table_rect_world(dst);
        let sy = sr.top() + row_y(src, fk.columns.first()?);
        let dy = dr.top() + row_y(dst, fk.ref_columns.first()?);
        if src.name == dst.name {
            return Some((pos2(sr.right(), sy), 1.0, pos2(dr.right(), dy), 1.0, None));
        }
        // Mit Fahrspuren, wenn die Anordnung noch passt (Kind rechts vom Elternteil)
        if let Some(lanes) = self.routes.get(&fk_key(fk))
            && !lanes.is_empty() && dr.right() < lanes.last()?.x + 1.0 && sr.left() > lanes[0].x - 1.0 {
                return Some((pos2(sr.left(), sy), -1.0, pos2(dr.right(), dy), 1.0, Some(lanes)));
            }
        // Direkt: passende Seiten waehlen
        Some(if sr.left() > dr.right() + 20.0 {
            (pos2(sr.left(), sy), -1.0, pos2(dr.right(), dy), 1.0, None)
        } else if dr.left() > sr.right() + 20.0 {
            (pos2(sr.right(), sy), 1.0, pos2(dr.left(), dy), -1.0, None)
        } else if sr.center().x <= dr.center().x {
            (pos2(sr.left(), sy), -1.0, pos2(dr.left(), dy), -1.0, None)
        } else {
            (pos2(sr.right(), sy), 1.0, pos2(dr.right(), dy), 1.0, None)
        })
    }

    /// Linienverlauf aller Beziehungen (Weltkoordinaten), in der Reihenfolge von `schema.fks`.
    ///
    /// Enden mehrere Linien am selben Punkt (z. B. alle Verweise auf kunde.id), werden sie leicht
    /// aufgefaechert – sortiert nach der Hoehe des anderen Endes, damit sie sich nicht kreuzen.
    /// Die Endpunkte werden dafuer einmal berechnet und gruppiert (statt je Linie alle anderen
    /// zu durchsuchen), sonst waere der Aufwand bei tausenden Beziehungen quadratisch.
    pub(super) fn all_paths(&self, schema: &Schema) -> Vec<Option<Path>> {
        let ends: Vec<_> = schema.fks.iter().map(|fk| self.endpoints(schema, fk)).collect();
        // Gruppe = gleicher Punkt und gleiche Richtung
        let mut groups: HashMap<PortKey, Vec<(f32, usize, bool)>> = HashMap::new();
        let group_key = |p: Pos2, d: f32| (p.x.to_bits(), p.y.to_bits(), d as i32);
        for (i, e) in ends.iter().enumerate() {
            let Some((a, da, b, db, lanes)) = e else { continue };
            let (na, nb) = match lanes {
                Some(l) => (l[0], l[l.len() - 1]),
                None => (*b, *a),
            };
            groups.entry(group_key(*a, *da)).or_default().push((na.y, i, true));
            groups.entry(group_key(*b, *db)).or_default().push((nb.y, i, false));
        }
        let mut start_off = vec![0.0f32; ends.len()];
        let mut end_off = vec![0.0f32; ends.len()];
        for group in groups.values_mut() {
            let n = group.len();
            if n <= 1 {
                continue;
            }
            group.sort_by(|x, y| x.0.total_cmp(&y.0).then_with(|| (x.1, x.2).cmp(&(y.1, y.2))));
            let step = (12.0 / (n - 1) as f32).min(5.0);
            for (idx, &(_, i, at_start)) in group.iter().enumerate() {
                let off = (idx as f32 - (n - 1) as f32 / 2.0) * step;
                if at_start {
                    start_off[i] = off;
                } else {
                    end_off[i] = off;
                }
            }
        }
        schema
            .fks
            .iter()
            .zip(ends)
            .enumerate()
            .map(|(i, (fk, e))| {
                let (mut a, da, mut b, db, lanes) = e?;
                a.y += start_off[i];
                b.y += end_off[i];
                if fk.table == fk.ref_table {
                    let bend = 40.0 + (a.y - b.y).abs() * 0.2;
                    return Some(vec![[a, a + vec2(bend, 0.0), b + vec2(bend, 0.0), b]]);
                }
                if let Some(lanes) = lanes {
                    let mut pts = vec![a];
                    pts.extend(lanes.iter().copied());
                    pts.push(b);
                    let mut path = Vec::new();
                    for w in pts.windows(2) {
                        if (w[0].y - w[1].y).abs() < 0.5 {
                            path.push([w[0], w[0].lerp(w[1], 0.33), w[0].lerp(w[1], 0.66), w[1]]);
                        } else {
                            path.push(bezier_between(w[0], w[1], -1.0, 1.0));
                        }
                    }
                    return Some(path);
                }
                Some(vec![bezier_between(a, b, da, db)])
            })
            .collect()
    }

    pub(super) fn geometry(&mut self, schema: &Arc<Schema>) -> Arc<RouteGeometry> {
        if let Some(cache) = &self.route_cache
            && Arc::ptr_eq(schema, &cache.schema) && cache.positions == self.pos
                && cache.routes == self.routes && cache.show_types == self.show_types
                && cache.notation == self.notation && cache.rel_labels == self.rel_labels {
                return Arc::clone(&cache.geometry);
            }
        self.ensure_boxes();
        let paths = self.all_paths(schema);
        let polylines: Vec<Vec<Pos2>> = paths.iter().map(|path| path.as_ref().map(|segments| {
            segments.iter().flat_map(|segment| (0..=12).map(move |i| sample(segment, i as f32 / 12.0))).collect()
        }).unwrap_or_default()).collect();
        let bounds = paths.iter().map(|path| {
            let mut r = Rect::NOTHING;
            for seg in path.iter().flatten() {
                for p in seg {
                    r.extend_with(*p);
                }
            }
            r
        }).collect();
        let keys = schema.fks.iter().map(fk_key).collect();
        let rels: Vec<Rel> = schema.fks.iter().map(|fk| self.rel(schema, fk)).collect();
        let mut shapes = vec![Vec::new(); paths.len()];
        let mut tags = Vec::new();
        let mut texts = Vec::new();
        let mut text_edge = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            let Some(path) = path else { continue };
            for prim in markers(path, rels[i], self.notation, self.rel_labels) {
                match prim {
                    Prim::Text(p, s, align) => {
                        texts.push((p, s, align, ()));
                        text_edge.push(i);
                    }
                    Prim::Tag(p, s) => tags.push((i, p, s)),
                    other => shapes[i].push(other),
                }
            }
        }
        // Beschriftungen ueberlappungsfrei setzen; die Zuordnung zur Linie bleibt erhalten
        let texts = place_texts(texts.into_iter().zip(text_edge).map(|((p, s, a, _), e)| (p, s, a, e)).collect())
            .into_iter()
            .map(|(p, s, a, e)| (e, p, s, a))
            .collect();
        let geometry = Arc::new(RouteGeometry { paths, polylines, bounds, keys, shapes, tags, texts });
        self.route_cache = Some(RouteCache {
            schema: Arc::clone(schema), positions: self.pos.clone(), routes: self.routes.clone(),
            show_types: self.show_types, notation: self.notation, rel_labels: self.rel_labels,
            geometry: Arc::clone(&geometry),
        });
        geometry
    }
}
