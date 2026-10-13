// Export als SVG.

use super::*;

impl ErTab {
    pub(super) fn to_svg(&self) -> String {
        let Some(schema) = &self.schema else { return String::new() };
        let Some(b) = self.bounds() else { return String::new() };
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let pad = 30.0;
        let mut o = String::new();
        let _ = writeln!(
            o,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" font-family="Segoe UI, Arial, sans-serif" font-size="12">"#,
            b.width() + 2.0 * pad,
            b.height() + 2.0 * pad
        );
        let _ = writeln!(o, r#"<rect width="100%" height="100%" fill="white"/><g transform="translate({},{})">"#, pad - b.min.x, pad - b.min.y);
        let raw = self.raw.as_deref().unwrap_or(schema);
        let mut tags: Vec<(Pos2, String)> = Vec::new();
        let mut texts: Vec<(Pos2, String, Align2, ())> = Vec::new();
        let all_paths = self.all_paths(schema);
        for (fk, path) in schema.fks.iter().zip(all_paths) {
            let Some(path) = path else { continue };
            let mut d = format!("M{} {}", path[0][0].x, path[0][0].y);
            for s in &path {
                let _ = write!(d, " C{} {} {} {} {} {}", s[1].x, s[1].y, s[2].x, s[2].y, s[3].x, s[3].y);
            }
            let _ = writeln!(o, r##"<path d="{d}" fill="none" stroke="#305090" stroke-width="1.4"/>"##);
            let rel = self.rel(schema, fk);
            for prim in markers(&path, rel, self.notation, self.rel_labels) {
                match prim {
                    Prim::Line(p, q) => {
                        let _ = writeln!(o, r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#305090" stroke-width="1.4"/>"##, p.x, p.y, q.x, q.y);
                    }
                    Prim::Circle(c, r) => {
                        let _ = writeln!(o, r##"<circle cx="{}" cy="{}" r="{r}" fill="white" stroke="#305090" stroke-width="1.4"/>"##, c.x, c.y);
                    }
                    Prim::Text(p, t, align) => texts.push((p, t, align, ())),
                    Prim::Tag(p, t) => tags.push((p, t)),
                    Prim::Diamond(c, r) => {
                        let w = r * 1.4;
                        let _ = writeln!(
                            o,
                            r##"<path d="M{} {} L{} {} L{} {} L{} {} Z" fill="white" stroke="#305090" stroke-width="1.4"/>"##,
                            c.x - w, c.y, c.x, c.y - r, c.x + w, c.y, c.x, c.y + r
                        );
                    }
                }
            }
        }
        for (p, t, align, _) in place_texts(texts) {
            let anchor = if align == Align2::LEFT_BOTTOM { "start" } else { "end" };
            let _ = writeln!(o, r##"<text x="{}" y="{}" font-size="10.5" text-anchor="{anchor}" fill="#305090">{}</text>"##, p.x, p.y - 2.0, esc(&t));
        }
        for (p, t) in tags {
            let w = t.chars().count() as f32 * 6.5 + 8.0;
            let _ = writeln!(
                o,
                r##"<rect x="{}" y="{}" width="{w}" height="15" rx="3" fill="white" stroke="#305090"/><text x="{}" y="{}" font-size="10.5" text-anchor="middle" fill="#305090">{}</text>"##,
                p.x - w / 2.0,
                p.y - 7.5,
                p.x,
                p.y + 4.0,
                esc(&t)
            );
        }
        for t in &schema.tables {
            let r = self.table_rect_world(t);
            let (p, s) = (r.min, r.size());
            let head = if t.is_view { "#E8F3E0" } else { "#DCE8F5" };
            let _ = writeln!(o, r##"<rect x="{}" y="{}" width="{}" height="{}" fill="white" stroke="#606060"/>"##, p.x, p.y, s.x, s.y);
            let _ = writeln!(o, r##"<rect x="{}" y="{}" width="{}" height="{HEADER_H}" fill="{head}" stroke="#606060"/>"##, p.x, p.y, s.x);
            let _ = writeln!(o, r#"<text x="{}" y="{}" font-weight="bold">{}</text>"#, p.x + 6.0, p.y + 15.0, esc(&t.name));
            for (i, c) in t.columns.iter().enumerate() {
                let y = p.y + HEADER_H + 3.0 + ROW_H * i as f32 + 12.5;
                if c.is_pk() {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" font-size="9" fill="#B08000">PK</text>"##, p.x + 5.0);
                } else if raw.is_fk_column(&t.name, &c.name) {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" font-size="9" fill="#2060C0">FK</text>"##, p.x + 5.0);
                }
                let w = if c.is_pk() { " font-weight=\"bold\"" } else { "" };
                let _ = writeln!(o, r#"<text x="{}" y="{y}"{w}>{}</text>"#, p.x + 27.0, esc(&c.name));
                if self.show_types {
                    let _ = writeln!(o, r##"<text x="{}" y="{y}" text-anchor="end" fill="#707070">{}</text>"##, p.x + s.x - 16.0, esc(&c.col_type));
                }
            }
        }
        o.push_str("</g></svg>\n");
        o
    }
}
