// Symbole aus dem Lucide-Satz (ISC-Lizenz, siehe data.rs), als Linien gezeichnet. Sonderzeichen wie ▾ oder ✖
// fehlen in manchen Windows-Schriften und erscheinen dann als leeres Viereck.

use crate::style;
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, pos2, vec2};
use std::collections::HashMap;
use std::sync::OnceLock;

mod data;
mod path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Files,
    Search,
    Database,
    History,
    Diagram,
    Wizard,
    Close,
    Plus,
    Refresh,
    NewFile,
    NewFolder,
    ChevronDown,
    ChevronRight,
    ChevronLeft,
    Play,
    More,
    Trash,
    Up,
    Down,
    Collapse,
    ZoomIn,
    ZoomOut,
    Fit,
    External,
    Folder,
    FolderOpen,
    File,
    FileText,
    FileCode,
    FileImage,
    FileSheet,
    FileJson,
    Table,
    Columns,
    Key,
    Link,
    Settings,
    Book,
    Terminal,
    Shield,
    Save,
    Download,
    Alert,
    Check,
    Info,
    Sparkles,
    Layers,
    ArrowLeft,
    ArrowRight,
    Undo,
    Redo,
    Explain,
    Upload,
    Users,
    ArrowUp,
    ArrowDown,
    Lock,
    Edit,
    Copy,
}

impl Icon {
    /// Name des Lucide-Symbols (siehe data.rs)
    fn name(self) -> &'static str {
        match self {
            Icon::Files => "files",
            Icon::Search => "search",
            Icon::Database => "database",
            Icon::History => "clock",
            Icon::Diagram => "network",
            Icon::Wizard => "wand-sparkles",
            Icon::Close => "x",
            Icon::Plus => "plus",
            Icon::Refresh => "refresh-cw",
            Icon::NewFile => "file-plus",
            Icon::NewFolder => "folder-plus",
            Icon::ChevronDown | Icon::Down => "chevron-down",
            Icon::ChevronRight => "chevron-right",
            Icon::ChevronLeft => "chevron-left",
            Icon::Up => "chevron-up",
            Icon::Play => "play",
            Icon::More => "ellipsis",
            Icon::Trash => "trash",
            Icon::Collapse => "chevrons-down-up",
            Icon::ZoomIn => "zoom-in",
            Icon::ZoomOut => "zoom-out",
            Icon::Fit => "maximize",
            Icon::External => "external-link",
            Icon::Folder => "folder",
            Icon::FolderOpen => "folder-open",
            Icon::File => "file",
            Icon::FileText => "file-text",
            Icon::FileCode => "file-code",
            Icon::FileImage => "file-image",
            Icon::FileSheet => "file-spreadsheet",
            Icon::FileJson => "file-braces",
            Icon::Table => "table",
            Icon::Columns => "columns-3",
            Icon::Key => "key-round",
            Icon::Link => "link",
            Icon::Settings => "settings",
            Icon::Book => "book-open",
            Icon::Terminal => "terminal",
            Icon::Shield => "shield-check",
            Icon::Save => "save",
            Icon::Download => "download",
            Icon::Alert => "circle-alert",
            Icon::Check => "circle-check",
            Icon::Info => "info",
            Icon::Sparkles => "sparkles",
            Icon::Layers => "layers",
            Icon::ArrowLeft => "arrow-left",
            Icon::ArrowRight => "arrow-right",
            Icon::Undo => "undo-2",
            Icon::Redo => "redo-2",
            Icon::Explain => "scan-search",
            Icon::Upload => "upload",
            Icon::Users => "users",
            Icon::ArrowUp => "arrow-up",
            Icon::ArrowDown => "arrow-down",
            Icon::Lock => "lock",
            Icon::Edit => "pencil",
            Icon::Copy => "copy",
        }
    }

    /// Flaechig gefuellt statt nur umrissen
    fn filled(self) -> bool {
        matches!(self, Icon::Play)
    }
}

/// Linienzuege der Symbole, einmal aus den SVG-Pfaden berechnet (Entwurfsraster 24 x 24).
fn shapes(icon: Icon) -> &'static [path::Poly] {
    static CACHE: OnceLock<HashMap<&'static str, Vec<path::Poly>>> = OnceLock::new();
    let map = CACHE.get_or_init(|| data::SHAPES.iter().map(|(name, ds)| (*name, ds.iter().flat_map(|d| path::parse(d)).collect())).collect());
    map.get(icon.name()).map(Vec::as_slice).unwrap_or(&[])
}

/// Zeichnet `icon` in `r` (die Groesse passt sich dem Rechteck an, Entwurf fuer 24 px).
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    let k = r.width().min(r.height()) / 24.0;
    let width = (1.75 * k).clamp(1.1, 3.0);
    let stroke = Stroke::new(width, color);
    let c = r.center();
    let map = |q: Pos2| c + (q.to_vec2() - vec2(12.0, 12.0)) * k;
    for poly in shapes(icon) {
        let pts: Vec<Pos2> = poly.pts.iter().map(|&q| map(q)).collect();
        if pts.len() < 2 {
            // Punkt (z. B. Auslassungspunkte): runder Tupfer
            if let Some(&q) = pts.first() {
                p.circle_filled(q, width / 2.0, color);
            }
            continue;
        }
        if icon.filled() && poly.closed {
            p.add(Shape::convex_polygon(pts.clone(), color, stroke));
        } else if poly.closed {
            p.add(Shape::closed_line(pts.clone(), stroke));
        } else {
            p.add(Shape::line(pts.clone(), stroke));
        }
        // Runde Enden
        if !poly.closed {
            let (a, b) = (pts[0], pts[pts.len() - 1]);
            p.circle_filled(a, width / 2.0, color);
            p.circle_filled(b, width / 2.0, color);
        }
    }
}

/// Kleiner Knopf nur mit Symbol (ohne Rahmen, Hervorhebung bei Maus darueber).
pub fn button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> Response {
    button_sized(ui, icon, tip, 22.0)
}

pub fn button_sized(ui: &mut egui::Ui, icon: Icon, tip: &str, size: f32) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let pal = style::pal();
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, pal.hover);
    }
    let color = if ui.is_enabled() { if resp.hovered() { pal.text } else { pal.text_weak } } else { pal.text_weak.gamma_multiply(0.5) };
    paint(ui.painter(), r.shrink(size * 0.15), icon, color);
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

/// Knopf mit Symbol und Text (Symbol links)
pub fn text_button(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    labelled_button(ui, icon, text, false, false)
}

/// Knopf mit Text und Symbol rechts davon (z. B. "Weiter ->")
pub fn text_button_right(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    labelled_button(ui, icon, text, false, true)
}

/// Hervorgehobene Hauptaktion (blau gefuellt, z. B. Ausfuehren)
pub fn primary_button(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    labelled_button(ui, icon, text, true, false)
}

fn labelled_button(ui: &mut egui::Ui, icon: Icon, text: &str, primary: bool, icon_right: bool) -> Response {
    let pal = style::pal();
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), pal.text);
    let size = vec2(galley.size().x + 38.0, ui.spacing().interact_size.y.max(24.0));
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let visuals = *ui.style().interact(&resp);
    let (fill, fg) = if primary && enabled {
        let fill = if resp.is_pointer_button_down_on() { pal.accent.gamma_multiply(0.8) } else if resp.hovered() { pal.accent.lerp_to_gamma(Color32::WHITE, 0.14) } else { pal.accent };
        (fill, Color32::WHITE)
    } else {
        (visuals.weak_bg_fill, if enabled { visuals.fg_stroke.color } else { pal.text_weak })
    };
    ui.painter().rect(r, 3.0, fill, if primary { Stroke::NONE } else { visuals.bg_stroke }, egui::StrokeKind::Inside);
    let (icon_x, text_x) = if icon_right { (r.right() - 15.0, r.left() + 11.0) } else { (r.left() + 15.0, r.left() + 28.0) };
    paint(ui.painter(), Rect::from_center_size(pos2(icon_x, r.center().y), vec2(14.0, 14.0)), icon, fg);
    ui.painter().galley_with_override_text_color(pos2(text_x, r.center().y - galley.size().y / 2.0), galley, fg);
    if resp.has_focus() {
        ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, pal.accent), egui::StrokeKind::Outside);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jedes Symbol hat Linien innerhalb des 24er-Rasters.
    #[test]
    fn all_icons_have_shapes_within_grid() {
        use Icon::*;
        for icon in [
            Files, Search, Database, History, Diagram, Wizard, Close, Plus, Refresh, NewFile, NewFolder, ChevronDown, ChevronRight, ChevronLeft, Play,
            More, Trash, Up, Down, Collapse, ZoomIn, ZoomOut, Fit, External, Folder, FolderOpen, File, FileText, FileCode, FileImage, FileSheet,
            FileJson, Table, Columns, Key, Link, Settings, Book, Terminal, Shield, Save, Download, Alert, Check, Info,
            Sparkles, Layers, ArrowLeft, ArrowRight, Undo, Redo, Explain, Upload, Users, ArrowUp, ArrowDown, Lock, Edit, Copy,
        ] {
            let polys = shapes(icon);
            assert!(!polys.is_empty(), "{icon:?}");
            for poly in polys {
                assert!(!poly.pts.is_empty(), "{icon:?}");
                for q in &poly.pts {
                    assert!((-0.5..=24.5).contains(&q.x) && (-0.5..=24.5).contains(&q.y), "{icon:?}: {q:?}");
                }
            }
        }
    }

    /// Zeichnet alle Symbole als Bogen in eine PNG-Datei (Sichtpruefung):
    /// `ICON_SHEET=/pfad/blatt.png cargo test icon_sheet -- --ignored`
    #[test]
    #[ignore = "manuelle Sichtpruefung"]
    fn icon_sheet() {
        use Icon::*;
        let Ok(out) = std::env::var("ICON_SHEET") else { return };
        let all = [
            Files, Search, Database, History, Diagram, Wizard, Close, Plus, Refresh, NewFile, NewFolder, ChevronDown, ChevronRight, ChevronLeft, Play,
            More, Trash, Up, Down, Collapse, ZoomIn, ZoomOut, Fit, External, Folder, FolderOpen, File, FileText, FileCode, FileImage, FileSheet,
            FileJson, Table, Columns, Key, Link, Settings, Book, Terminal, Shield, Save, Download, Alert, Check, Info,
            Sparkles, Layers, ArrowLeft, ArrowRight, Undo, Redo, Explain, Upload, Users, ArrowUp, ArrowDown, Lock, Edit, Copy,
        ];
        const CELL: usize = 96;
        const COLS: usize = 10;
        let rows = all.len().div_ceil(COLS);
        let mut img = image::RgbImage::from_pixel((COLS * CELL) as u32, (rows * CELL) as u32, image::Rgb([30, 30, 30]));
        for (n, icon) in all.iter().enumerate() {
            let (ox, oy) = ((n % COLS) * CELL, (n / COLS) * CELL);
            let k = (CELL - 24) as f32 / 24.0;
            let half = 1.75 * k / 2.0;
            let pt = |q: Pos2| (12.0 + q.x * k, 12.0 + q.y * k);
            let mut segs: Vec<((f32, f32), (f32, f32))> = Vec::new();
            for poly in shapes(*icon) {
                let pts: Vec<_> = poly.pts.iter().map(|&q| pt(q)).collect();
                for w in pts.windows(2) {
                    segs.push((w[0], w[1]));
                }
                if poly.closed && pts.len() > 2 {
                    segs.push((pts[pts.len() - 1], pts[0]));
                }
                if pts.len() == 1 {
                    segs.push((pts[0], pts[0]));
                }
            }
            for y in 0..CELL {
                for x in 0..CELL {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let mut best = f32::MAX;
                    for &((ax, ay), (bx, by)) in &segs {
                        let (dx, dy) = (bx - ax, by - ay);
                        let l2 = dx * dx + dy * dy;
                        let t = if l2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0) };
                        let (cx, cy) = (ax + t * dx, ay + t * dy);
                        best = best.min(((px - cx).powi(2) + (py - cy).powi(2)).sqrt());
                    }
                    let a = (half + 0.5 - best).clamp(0.0, 1.0);
                    if a > 0.0 {
                        let v = (30.0 + 225.0 * a) as u8;
                        img.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgb([v, v, v]));
                    }
                }
            }
        }
        img.save(out).unwrap();
    }
}
