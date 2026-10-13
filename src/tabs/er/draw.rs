// Zeichnen und Mausbedienung der Zeichenflaeche.

use super::*;

impl ErTab {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, style::pal().er_canvas);
        let origin = rect.min;
        let Some(schema) = self.schema.clone() else { return };
        if self.fit_pending && rect.width() > 50.0 {
            self.fit_pending = false;
            self.fit(rect);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));

        crate::app::block_autoscroll(rect);
        // Zoom mit Mausrad (unabhaengig von der eingestellten Scrollgeschwindigkeit)
        if resp.hovered() {
            let speed = cx.settings.scroll_speed.max(0.25);
            let scroll = ui.input(|i| i.smooth_scroll_delta.y / speed + i.zoom_delta().ln() * 200.0);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(MIN_ZOOM, MAX_ZOOM);
                if let Some(mouse) = hover {
                    let world = (mouse - origin - self.offset) / old;
                    self.offset = mouse - origin - world * self.zoom;
                }
            }
        }

        // Cache routes and hit-test polylines in world coordinates. Panning and zooming reuse them.
        let geometry = self.geometry(&schema);
        let polylines = &geometry.polylines;
        let (off, zm) = (self.offset, self.zoom);

        let hit_table = |this: &Self, p: Pos2| -> Option<String> {
            let w = this.to_world(origin, p);
            schema.tables.iter().rev().find(|t| this.table_rect_world(t).contains(w)).map(|t| t.name.clone())
        };
        let hit_fk = |p: Pos2| -> Option<usize> {
            let p = ((p - origin - off) / zm).to_pos2();
            let mut best = None;
            let mut best_d = 6.0 / zm;
            for (i, pl) in polylines.iter().enumerate() {
                if !geometry.bounds[i].expand(best_d).contains(p) {
                    continue;
                }
                for w in pl.windows(2) {
                    let d = dist_to_segment(p, w[0], w[1]);
                    if d < best_d {
                        best_d = d;
                        best = Some(i);
                    }
                }
            }
            best
        };
        let hover_table = hover.and_then(|p| hit_table(self, p));
        let hover_fk = if hover_table.is_none() { hover.and_then(hit_fk) } else { None };
        // Anfasser (Punkt rechts neben der Spalte) unter der Maus?
        let handle_at = |this: &Self, p: Pos2| -> Option<(String, String)> {
            let w = this.to_world(origin, p);
            for t in schema.tables.iter().rev().filter(|t| !t.is_view) {
                let r = this.table_rect_world(t);
                let tol = (9.0 / this.zoom).max(6.0);
                if (w.x - (r.right() - 7.0)).abs() <= tol && w.y > r.top() + HEADER_H && w.y < r.bottom() {
                    let i = ((w.y - r.top() - HEADER_H - 3.0) / ROW_H) as usize;
                    if let Some(c) = t.columns.get(i) {
                        return Some((t.name.clone(), c.name.clone()));
                    }
                }
            }
            None
        };

        // Maus: ziehen
        if resp.drag_started() {
            let origin_press = ui.input(|i| i.pointer.press_origin()).or(pointer);
            let middle = ui.input(|i| i.pointer.middle_down());
            self.drag = match origin_press {
                Some(_) if middle => Drag::Pan,
                Some(p) => {
                    if let Some((t, c)) = handle_at(self, p) {
                        Drag::Link(t, c)
                    } else if let Some(t) = hit_table(self, p) {
                        Drag::Table(t)
                    } else {
                        Drag::Pan
                    }
                }
                None => Drag::None,
            };
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match self.drag.clone() {
                Drag::Table(name) => {
                    if let Some(p) = self.pos.get_mut(&name) {
                        *p += d / self.zoom;
                    }
                    self.remove_routes_of(&name);
                }
                Drag::Pan => self.offset += d,
                _ => {}
            }
        }
        if resp.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Table(_) => self.save_layout(),
                Drag::Link(t, c) => {
                    if let Some(p) = hover.or(pointer) {
                        let w = self.to_world(origin, p);
                        if let Some((rt, rc)) = self.column_at(&schema, w) {
                            let rc = rc.or_else(|| schema.table(&rt).and_then(|ti| ti.pk_columns().first().cloned()));
                            if let Some(rc) = rc
                                && !(rt == t && rc == c) {
                                    self.open_link(t, c, format!("{rt}.{rc}"));
                                }
                        }
                    }
                }
                _ => {}
            }
        }
        if resp.clicked() {
            self.selected = pointer.filter(|p| hit_table(self, *p).is_none()).and_then(hit_fk).map(|i| geometry.keys[i].clone());
        }
        // Entf: ausgewaehlte Beziehung loeschen
        let editing_text = ui.ctx().egui_wants_keyboard_input();
        if !editing_text && self.link.is_none() && ui.input(|i| i.key_pressed(egui::Key::Delete))
            && let Some(fk) = self.selected.as_ref().and_then(|k| schema.fks.iter().find(|f| fk_key(f) == *k)) {
                cx.actions.push(self.delete_action(fk));
            }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
        }
        if resp.double_clicked()
            && let Some(t) = pointer.and_then(|p| hit_table(self, p)) {
                cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: t });
            }
        if resp.secondary_clicked() {
            self.menu = Some(match pointer {
                Some(p) => {
                    if let Some(t) = hit_table(self, p) {
                        Menu::Table(t)
                    } else if let Some(i) = hit_fk(p) {
                        Menu::Fk(i)
                    } else {
                        Menu::Background
                    }
                }
                None => Menu::Background,
            });
        }

        let painter = painter.with_clip_rect(rect);
        // Nach den Maus-Aktionen mit aktuellem Zoom/Versatz zeichnen
        let z = self.zoom;
        let (off, zm) = (self.offset, self.zoom);
        let screen = move |p: Pos2| origin + off + p.to_vec2() * zm;
        let geometry = self.geometry(&schema);
        let paths = &geometry.paths;

        // Hintergrundraster (dezent)
        let grid = 40.0 * z;
        if grid > 12.0 {
            let dot = if style::pal().dark { Color32::from_gray(0x33) } else { Color32::from_gray(0xE4) };
            let start = origin + vec2(self.offset.x.rem_euclid(grid), self.offset.y.rem_euclid(grid));
            let mut x = start.x;
            while x < rect.right() {
                let mut y = start.y;
                while y < rect.bottom() {
                    painter.circle_filled(pos2(x, y), 1.0, dot);
                    y += grid;
                }
                x += grid;
            }
        }

        // Beziehungslinien: nur die im Bild sichtbaren, in jeder Groesse als Kurven
        let focus_table = match &self.drag {
            Drag::Table(t) => Some(t.clone()),
            _ => hover_table.clone(),
        };
        let raw = self.raw.clone().unwrap_or_else(|| schema.clone());
        let view = Rect::from_min_max(self.to_world(origin, rect.min), self.to_world(origin, rect.max)).expand(24.0);
        let detail = z >= 0.3;
        let style_of = |i: usize, fk: &ForeignKey| -> (Color32, f32) {
            let related = focus_table.as_ref().map(|t| *t == fk.table || *t == fk.ref_table);
            let is_hover = hover_fk == Some(i) || matches!(self.menu, Some(Menu::Fk(j)) if j == i);
            let is_sel = self.selected.as_deref() == Some(geometry.keys[i].as_str());
            if is_sel {
                (LINE_HI, 3.2)
            } else if is_hover || related == Some(true) {
                (LINE_HI, 2.2)
            } else if related == Some(false) {
                (style::pal().er_line_dim, 1.2)
            } else {
                (style::pal().er_line, 1.4)
            }
        };
        for (i, fk) in schema.fks.iter().enumerate() {
            let Some(path) = &paths[i] else { continue };
            let bounds = geometry.bounds[i];
            if !bounds.intersects(view) || (bounds.width() * z < 1.0 && bounds.height() * z < 1.0) {
                continue;
            }
            let (color, width) = style_of(i, fk);
            let stroke = Stroke::new((width * z).max(1.0), color);
            for seg in path {
                let s = seg.map(screen);
                painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(s, false, Color32::TRANSPARENT, stroke)));
            }
            // Kardinalitaeten (1:1, 1:n, n:m, optional) in der gewaehlten Notation
            if detail {
                for prim in &geometry.shapes[i] {
                    match prim {
                        Prim::Line(p, q) => {
                            painter.line_segment([screen(*p), screen(*q)], stroke);
                        }
                        Prim::Circle(c, r) => {
                            painter.circle(screen(*c), *r * z, style::pal().er_canvas, stroke);
                        }
                        Prim::Diamond(c, r) => {
                            let c = screen(*c);
                            let (w, h) = (*r * 1.4 * z, *r * z);
                            let pts = vec![c + vec2(-w, 0.0), c + vec2(0.0, -h), c + vec2(w, 0.0), c + vec2(0.0, h)];
                            painter.add(Shape::convex_polygon(pts, style::pal().er_canvas, stroke));
                        }
                        Prim::Text(..) | Prim::Tag(..) => {}
                    }
                }
            }
        }
        if detail {
            // Beschriftungen ueber alle Linien
            let small = FontId::proportional(qsize((10.5 * z).max(6.0)));
            for (i, p, s, align) in &geometry.texts {
                if view.contains(*p) {
                    painter.text(screen(*p), *align, s, small.clone(), style_of(*i, &schema.fks[*i]).0);
                }
            }
        }
        if z > 0.35 {
            let font = FontId::proportional(qsize(10.5 * z));
            for (i, p, s) in &geometry.tags {
                if !view.contains(*p) {
                    continue;
                }
                let color = style_of(*i, &schema.fks[*i]).0;
                let galley = painter.layout_no_wrap(s.clone(), font.clone(), color);
                let r = Rect::from_center_size(screen(*p), galley.size() + vec2(8.0, 2.0) * z);
                painter.rect(r, 3.0 * z, style::pal().er_canvas, Stroke::new(1.0, color), StrokeKind::Inside);
                painter.galley(r.center() - galley.size() / 2.0, galley, color);
            }
        }

        // Tabellen. Schriftgroessen werden gestuft: jede neue Groesse legt Zeichen im Schrift-
        // Atlas an, den egui nie verkleinert (stufenloses Zoomen liesse den Speicher anwachsen).
        let pal = style::pal();
        let font = FontId::proportional(qsize(12.0 * z));
        let bold = FontId::proportional(qsize(12.5 * z));
        let small = FontId::proportional(qsize(9.5 * z));
        let radius = (6.0 * z).clamp(1.0, 8.0) as u8;
        let round = egui::CornerRadius::same(radius);
        let round_top = egui::CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 };
        let columns = 12.0 * z >= 7.0;
        let title_inside = 12.5 * z >= 8.5;
        let mut unlabeled: Vec<(usize, Rect)> = Vec::new();
        for (ti, t) in schema.tables.iter().enumerate() {
            let r = Rect::from_min_size(screen(self.table_rect_world(t).min), self.box_of(t) * z);
            if !r.intersects(rect) {
                continue;
            }
            let focused = focus_table.as_deref() == Some(t.name.as_str());
            let head_color = if t.is_view { pal.er_view_header } else { pal.er_header };
            if z >= 0.3 {
                painter.rect_filled(r.translate(vec2(0.0, 2.0) * z).expand(1.0 * z), round, Color32::from_black_alpha(if pal.dark { 60 } else { 26 }));
            }
            painter.rect_filled(r, round, pal.er_box);
            let head = Rect::from_min_size(r.min, vec2(r.width(), (HEADER_H * z).max(2.0)));
            painter.rect_filled(head, round_top, head_color);
            let outline = if focused { Stroke::new(1.8, LINE_HI) } else { Stroke::new(1.0, pal.border) };
            if !title_inside {
                // Zu klein fuer Text: Name spaeter ueber Kollisionspruefung setzen
                painter.rect_stroke(r, round, outline, StrokeKind::Inside);
                unlabeled.push((ti, r));
                continue;
            }
            let clip = painter.with_clip_rect(r.intersect(rect));
            let title = if t.is_view { crate::tr_format!("{} (Sicht)", "{} (View)", t.name) } else { t.name.clone() };
            clip.text(head.left_center() + vec2(8.0 * z, 0.0), Align2::LEFT_CENTER, title, bold.clone(), pal.text);
            if columns {
                let type_x = r.right() - 14.0 * z;
                for (i, c) in t.columns.iter().enumerate() {
                    let y = r.top() + (HEADER_H + 3.0 + ROW_H * i as f32 + ROW_H / 2.0) * z;
                    if y < rect.top() - 10.0 || y > rect.bottom() + 10.0 {
                        continue;
                    }
                    let is_fk = raw.is_fk_column(&t.name, &c.name);
                    let x0 = r.left() + 8.0 * z;
                    if c.is_pk() {
                        clip.text(pos2(x0, y), Align2::LEFT_CENTER, "PK", small.clone(), PK_COLOR);
                    } else if is_fk {
                        clip.text(pos2(x0, y), Align2::LEFT_CENTER, "FK", small.clone(), FK_COLOR);
                    }
                    let mut label = c.name.clone();
                    if !c.nullable && !c.is_pk() {
                        label.push_str(" *");
                    }
                    let nf = if c.is_pk() { bold.clone() } else { font.clone() };
                    clip.text(pos2(x0 + 22.0 * z, y), Align2::LEFT_CENTER, label, nf, pal.text);
                    if self.show_types {
                        clip.text(pos2(type_x, y), Align2::RIGHT_CENTER, type_label(&c.col_type), font.clone(), pal.er_type);
                    }
                    // Anfasser zum Verknuepfen (nur bei Maus ueber der Tabelle)
                    if focused && !t.is_view && matches!(self.drag, Drag::None) {
                        painter.circle(pos2(r.right() - 7.0 * z, y), 3.5 * z, pal.er_box, Stroke::new(1.2, LINE_HI));
                    }
                }
            }
            painter.rect_stroke(r, round, outline, StrokeKind::Inside);
        }
        // Namen herausgezoomter Tabellen: die mit den meisten Beziehungen zuerst, ohne dass sich
        // Beschriftungen ueberdecken (wie Ortsnamen auf einer Karte)
        if !unlabeled.is_empty() {
            let lookup: HashMap<usize, Rect> = unlabeled.into_iter().collect();
            let label_font = FontId::proportional(10.0);
            const CELL: f32 = 96.0;
            let mut placed: Vec<Rect> = Vec::new();
            let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
            let cells = |r: Rect| {
                let (x0, x1) = ((r.min.x / CELL).floor() as i32, (r.max.x / CELL).floor() as i32);
                let (y0, y1) = ((r.min.y / CELL).floor() as i32, (r.max.y / CELL).floor() as i32);
                (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
            };
            let mut drawn = 0;
            for &ti in &geometry.label_order {
                let Some(r) = lookup.get(&ti) else { continue };
                let galley = painter.layout_no_wrap(schema.tables[ti].name.clone(), label_font.clone(), pal.text);
                let size = galley.size() + vec2(8.0, 3.0);
                let lr = Rect::from_min_size(pos2(r.left().max(rect.left() + 2.0), r.top().max(rect.top() + 2.0)), size);
                if lr.right() > rect.right() || lr.bottom() > rect.bottom() {
                    continue;
                }
                if cells(lr).filter_map(|c| grid.get(&c)).flatten().any(|&k| placed[k].expand(1.0).intersects(lr)) {
                    continue;
                }
                for c in cells(lr) {
                    grid.entry(c).or_default().push(placed.len());
                }
                placed.push(lr);
                painter.rect_filled(lr, 3.0, pal.er_canvas.gamma_multiply(0.88));
                painter.galley(lr.min + vec2(4.0, 1.5), galley, pal.text);
                drawn += 1;
                if drawn >= 800 {
                    break;
                }
            }
        }

        // Neue Beziehung: gestrichelte Linie zur Maus
        if let Drag::Link(t, c) = &self.drag
            && let (Some(ti), Some(p)) = (schema.table(t), hover.or(pointer)) {
                let r = self.table_rect_world(ti);
                let a = screen(pos2(r.right() - 7.0, r.top() + row_y(ti, c)));
                painter.add(Shape::dashed_line(&[a, p], Stroke::new(2.0, LINE_HI), 6.0, 4.0));
                painter.circle_filled(a, 4.0, LINE_HI);
                let w = self.to_world(origin, p);
                let tip = match self.column_at(&schema, w) {
                    Some((rt, Some(rc))) => format!("{t}.{c}  →  {rt}.{rc}"),
                    Some((rt, None)) => crate::tr_format!("{t}.{c}  →  {rt} (Primärschlüssel)", "{t}.{c}  →  {rt} (primary key)"),
                    None => crate::i18n::text("Auf die Zielspalte ziehen (meist id)").into(),
                };
                painter.text(p + vec2(12.0, 12.0), Align2::LEFT_TOP, tip, FontId::proportional(12.0), LINE_HI);
            }

        // Tooltip fuer Linie
        if let (Some(i), Drag::None) = (hover_fk, &self.drag) {
            resp.clone().on_hover_text(self.rel_text(&schema, &schema.fks[i]));
        }
        // Tooltip fuer eine Spalte, deren Typ gekuerzt ist oder die einen Kommentar hat
        if let (Some(p), Drag::None, true) = (hover, &self.drag, z >= 0.58)
            && let Some((tn, Some(cn))) = self.column_at(&schema, self.to_world(origin, p))
            && let Some(c) = schema.table(&tn).and_then(|t| t.columns.iter().find(|c| c.name == cn))
            && (type_label(&c.col_type) != c.col_type || !c.comment.is_empty())
        {
            let mut text = format!("{}  {}", c.name, c.col_type);
            if !c.comment.is_empty() {
                text.push_str(&format!("\n{}", c.comment));
            }
            resp.clone().on_hover_text(text);
        }

        if schema.tables.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::text("Diese Datenbank enthält noch keine Tabellen."), FontId::proportional(14.0), style::pal().text_weak);
        }
        painter.rect_stroke(rect, style::RADIUS + 3, Stroke::new(1.0, style::pal().border), StrokeKind::Inside);

        // Kontextmenue
        let menu = self.menu.clone();
        let db = self.db.clone();
        let mut close_menu = false;
        resp.context_menu(|ui| match &menu {
            Some(Menu::Table(t)) => {
                ui.label(RichText::new(t).strong());
                if ui.button(crate::i18n::text("Daten anzeigen")).clicked() {
                    cx.actions.push(Action::OpenData { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button(crate::i18n::text("Struktur bearbeiten")).clicked() {
                    cx.actions.push(Action::OpenStructure { db: db.clone(), table: t.clone() });
                    ui.close();
                }
                if ui.button(crate::i18n::text("SELECT-Abfrage erzeugen")).clicked() {
                    cx.actions.push(Action::OpenSql { db: Some(db.clone()), sql: format!("SELECT * FROM {} LIMIT 100;", q(t)), run: true });
                    ui.close();
                }
                if ui.button(crate::i18n::text("Beziehung von hier anlegen …")).clicked() {
                    let col = schema.table(t).and_then(|ti| ti.columns.iter().find(|c| !c.is_pk()).or(ti.columns.first())).map(|c| c.name.clone()).unwrap_or_default();
                    self.open_link(t.clone(), col, String::new());
                    ui.close();
                }
            }
            Some(Menu::Fk(i)) if schema.fks.get(*i).is_some_and(|fk| self.nm.contains_key(&fk_key(fk))) => {
                let fk = &schema.fks[*i];
                let j = self.nm[&fk_key(fk)].clone();
                ui.label(RichText::new(crate::tr_format!("{} ↔ {}  (n:m über {j})", "{} ↔ {}  (n:m through {j})", fk.table, fk.ref_table)).strong());
                if ui.button(crate::i18n::text("Daten der Zwischentabelle")).clicked() {
                    cx.actions.push(Action::OpenData { db: db.clone(), table: j.clone() });
                    ui.close();
                }
                if ui.button(crate::i18n::text("Struktur der Zwischentabelle")).clicked() {
                    cx.actions.push(Action::OpenStructure { db: db.clone(), table: j.clone() });
                    ui.close();
                }
                if ui.button(crate::i18n::text("Beziehung löschen …")).clicked() {
                    cx.actions.push(self.delete_action(fk));
                    close_menu = true;
                    ui.close();
                }
            }
            Some(Menu::Fk(i)) => {
                if let Some(fk) = schema.fks.get(*i) {
                    ui.label(RichText::new(format!("{}.{} → {}.{}", fk.table, fk.columns.join(","), fk.ref_table, fk.ref_columns.join(","))).strong());
                    if ui.button(crate::i18n::text("Beziehung löschen …")).clicked() {
                        cx.actions.push(self.delete_action(fk));
                        close_menu = true;
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Struktur von ").to_string() + &fk.table).clicked() {
                        cx.actions.push(Action::OpenStructure { db: db.clone(), table: fk.table.clone() });
                        ui.close();
                    }
                }
            }
            _ => {
                if ui.button(crate::i18n::text("Automatisch anordnen")).clicked() {
                    self.auto_layout();
                    self.fit_pending = true;
                    self.save_layout();
                    ui.close();
                }
                if ui.button(crate::i18n::text("Alles anzeigen")).clicked() {
                    self.fit_pending = true;
                    ui.close();
                }
                if ui.button(crate::i18n::text("Neue Tabelle …")).clicked() {
                    cx.actions.push(Action::NewTable(db.clone()));
                    ui.close();
                }
                if ui.button(crate::i18n::text("Neue Beziehung …")).clicked() {
                    self.open_link(String::new(), String::new(), String::new());
                    ui.close();
                }
            }
        });
        if close_menu {
            self.menu = None;
        }
    }
}
