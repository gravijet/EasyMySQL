// Kastengroessen und Linienverlauf: einmal berechnet, danach nur noch gezeichnet.

use super::*;

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

/// Kurzform eines Datentyps fuer die Tabellenkarte: lange Aufzaehlungen werden zu "enum(7)",
/// andere sehr lange Typen gekuerzt. Der volle Typ steht im Tooltip der Spalte.
pub(super) fn type_label(t: &str) -> std::borrow::Cow<'_, str> {
    const MAX: usize = 20;
    if t.chars().count() <= MAX {
        return t.into();
    }
    let lower = t.to_ascii_lowercase();
    for kind in ["enum", "set"] {
        if lower.starts_with(kind) && lower[kind.len()..].starts_with('(') {
            let values = t.matches("','").count() + 1;
            return format!("{kind}({values})").into();
        }
    }
    let cut: String = t.chars().take(MAX - 1).collect();
    format!("{cut}…").into()
}

pub(super) fn box_size(t: &TableInfo, show_types: bool) -> Vec2 {
    let mut chars = t.name.chars().count() + 4;
    for c in &t.columns {
        let mut n = c.name.chars().count() + 5;
        if show_types {
            n += type_label(&c.col_type).chars().count() + 2;
        }
        chars = chars.max(n);
    }
    let w = (chars as f32 * CHAR_W + 22.0).clamp(130.0, 460.0);
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

    /// Linienverlauf aller Beziehungen (Weltkoordinaten), in der Reihenfolge von `schema.fks`: rechtwinklig,
    /// mit eigenen Spuren in den freien Streifen zwischen den Tabellen (siehe `ortho`).
    pub(super) fn all_paths(&self, schema: &Schema) -> Vec<Option<Path>> {
        let index: HashMap<&str, usize> = schema.tables.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();
        let rects: Vec<Rect> = schema.tables.iter().map(|t| self.table_rect_world(t)).collect();
        let mut edges: Vec<ortho::EdgeIn> = Vec::with_capacity(schema.fks.len());
        let mut edge_of: Vec<Option<usize>> = vec![None; schema.fks.len()];
        for (i, fk) in schema.fks.iter().enumerate() {
            let (Some(&c), Some(&p)) = (index.get(fk.table.as_str()), index.get(fk.ref_table.as_str())) else { continue };
            let (Some(cc), Some(pc)) = (fk.columns.first(), fk.ref_columns.first()) else { continue };
            let lanes = self.routes.get(&fk_key(fk)).map(|l| l.iter().map(|q| q.y).collect()).unwrap_or_default();
            edge_of[i] = Some(edges.len());
            edges.push(ortho::EdgeIn {
                child: c,
                parent: p,
                child_y: rects[c].top() + row_y(&schema.tables[c], cc),
                parent_y: rects[p].top() + row_y(&schema.tables[p], pc),
                lanes,
            });
        }
        let routed = ortho::route(&rects, &edges);
        edge_of.iter().map(|e| e.map(|k| rounded_path(&routed[k].points, 7.0)).filter(|p| !p.is_empty())).collect()
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
