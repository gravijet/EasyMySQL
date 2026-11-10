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
    Claude,
    Codex,
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
    Coffee,
    Package,
    Minus,
    Bug,
    Flask,
    Braces,
    Script,
    StepOver,
    StepInto,
    StepOut,
    Stop,
    GitBranch,
    GitCommit,
    GitPullRequest,
    Github,
    CloudUpload,
    CloudDownload,
    GitMerge,
    CircleDot,
    RotateCcw,
    Code,
    Scissors,
    Clipboard,
    FolderInput,
    Power,
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
            Icon::Claude => "claude",
            Icon::Codex => "codex",
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
            Icon::Coffee => "coffee",
            Icon::Package => "package",
            Icon::Minus => "minus",
            Icon::Bug => "bug",
            Icon::Flask => "flask-conical",
            Icon::Braces => "braces",
            Icon::Script => "file-code-2",
            Icon::StepOver => "redo-dot",
            Icon::StepInto => "arrow-down-to-dot",
            Icon::StepOut => "arrow-up-from-dot",
            Icon::Stop => "square",
            Icon::GitBranch => "git-branch",
            Icon::GitCommit => "git-commit-horizontal",
            Icon::GitPullRequest => "git-pull-request",
            Icon::Github => "github",
            Icon::CloudUpload => "cloud-upload",
            Icon::CloudDownload => "cloud-download",
            Icon::GitMerge => "git-merge",
            Icon::CircleDot => "circle-dot",
            Icon::RotateCcw => "rotate-ccw",
            Icon::Code => "code-xml",
            Icon::Scissors => "scissors",
            Icon::Clipboard => "clipboard-paste",
            Icon::FolderInput => "folder-input",
            Icon::Power => "power",
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
    if matches!(icon, Icon::Claude | Icon::Codex) {
        let id = egui::Id::new(("marke", icon.name()));
        let texture = p.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)).unwrap_or_else(|| {
                let bytes: &[u8] = if icon == Icon::Claude { include_bytes!("../../assets/brands/claude.png") } else { include_bytes!("../../assets/brands/codex.png") };
                let img = image::load_from_memory(bytes).expect("eingebettetes Markensymbol").to_rgba8();
                let texture = p.ctx().load_texture(icon.name(), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR);
                p.ctx().data_mut(|d| d.insert_temp(id, texture.clone()));
                texture
        });
        // Markenzeichen in Originalfarbe; nur die Deckkraft folgt der Anfrage (deaktiviert, Puls).
        p.image(texture.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::from_white_alpha(color.a()));
        return;
    }
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

/// Text genau mittig um `center` (nach den sichtbaren Zeichen, nicht nach der Zeilenhoehe der Schrift:
/// sonst sitzen Grossbuchstaben zu tief).
pub fn text_centered(p: &egui::Painter, center: Pos2, text: &str, font: egui::FontId, color: Color32) {
    let g = p.layout_no_wrap(text.to_string(), font, color);
    let b = g.mesh_bounds;
    let pos = center - b.center().to_vec2();
    p.galley(pos2(pos.x.round(), pos.y.round()), g, color);
}

/// Farbiges Kaestchen mit Kuerzel (Projekte, Dateiarten)
pub fn badge(p: &egui::Painter, r: Rect, text: &str, fill: Color32, fg: Color32, radius: f32) {
    p.rect_filled(r, radius, fill);
    let size = (r.height() * if text.chars().count() > 1 { 0.5 } else { 0.62 }).round();
    text_centered(p, r.center(), text, egui::FontId::proportional(size), fg);
    // Etwas kraeftiger (es gibt nur den normalen Schnitt)
    text_centered(p, r.center() + vec2(0.35, 0.0), text, egui::FontId::proportional(size), fg);
}

/// Art einer Datei nach Name und Endung: Symbol bzw. Kuerzel und Farbe (wie die Dateisymbole von IntelliJ IDEA)
pub enum FileGlyph {
    Sym(Icon, Color32),
    Badge(&'static str, Color32, Color32),
}

pub fn file_glyph(name: &str) -> FileGlyph {
    use FileGlyph::*;
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let white = Color32::WHITE;
    match lower.as_str() {
        "package.json" | "package-lock.json" => return Sym(Icon::Package, Color32::from_rgb(0x8B, 0xC3, 0x4A)),
        "requirements.txt" | "pyproject.toml" | "pom.xml" | "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => return Sym(Icon::Package, Color32::from_rgb(0xB0, 0x8B, 0x5B)),
        ".gitignore" | ".gitattributes" => return Sym(Icon::GitBranch, Color32::from_rgb(0xE8, 0x6A, 0x4A)),
        "dockerfile" => return Sym(Icon::Package, Color32::from_rgb(0x2D, 0x9C, 0xDB)),
        _ => {}
    }
    match ext {
        "sql" => Sym(Icon::Database, Color32::from_rgb(0xE3, 0x8C, 0x3B)),
        "java" => Badge("J", Color32::from_rgb(0xD9, 0x6B, 0x3E), white),
        "class" | "jar" => Sym(Icon::Package, Color32::from_rgb(0xB0, 0x8B, 0x5B)),
        "py" | "pyw" => Badge("Py", Color32::from_rgb(0x35, 0x72, 0xA5), Color32::from_rgb(0xFF, 0xDA, 0x4A)),
        "js" | "mjs" | "cjs" => Badge("JS", Color32::from_rgb(0xE8, 0xC9, 0x3A), Color32::from_rgb(0x2A, 0x2A, 0x2A)),
        "jsx" | "tsx" => Badge("R", Color32::from_rgb(0x2B, 0xA9, 0xC9), white),
        "ts" | "mts" | "cts" => Badge("TS", Color32::from_rgb(0x31, 0x78, 0xC6), white),
        "kt" | "kts" => Badge("K", Color32::from_rgb(0x8E, 0x5B, 0xE8), white),
        "html" | "htm" => Sym(Icon::Code, Color32::from_rgb(0xE4, 0x6F, 0x3C)),
        "css" | "scss" => Badge("#", Color32::from_rgb(0x3D, 0x8F, 0xD6), white),
        "properties" | "ini" | "cfg" | "conf" | "toml" | "env" => Sym(Icon::Settings, Color32::from_rgb(0x8A, 0x93, 0x9E)),
        "md" | "markdown" => Sym(Icon::FileText, Color32::from_rgb(0x51, 0x9A, 0xBA)),
        "txt" | "log" => Sym(Icon::FileText, Color32::from_rgb(0x8A, 0x93, 0x9E)),
        "csv" | "tsv" | "xlsx" | "xls" | "ods" => Sym(Icon::FileSheet, Color32::from_rgb(0x4C, 0xAF, 0x50)),
        "json" | "jsonc" => Sym(Icon::Braces, Color32::from_rgb(0xD6, 0xB8, 0x4E)),
        "xml" | "yml" | "yaml" => Sym(Icon::Code, Color32::from_rgb(0xC0, 0x8F, 0x4E)),
        "png" | "jpg" | "jpeg" | "jfif" | "gif" | "bmp" | "webp" | "ico" | "svg" => Sym(Icon::FileImage, Color32::from_rgb(0xA0, 0x7C, 0xD6)),
        "pdf" => Badge("P", Color32::from_rgb(0xD9, 0x4B, 0x43), white),
        "sh" | "bat" | "cmd" | "ps1" => Sym(Icon::Terminal, Color32::from_rgb(0x6C, 0xBE, 0x6A)),
        "zip" | "gz" | "tar" | "7z" => Sym(Icon::Package, Color32::from_rgb(0x8A, 0x93, 0x9E)),
        _ => Sym(Icon::File, style::pal().text_weak),
    }
}

/// Dateisymbol in `r` (16 x 16) zeichnen
pub fn paint_file(p: &egui::Painter, r: Rect, name: &str) {
    match file_glyph(name) {
        FileGlyph::Sym(icon, c) => paint(p, r, icon, c),
        FileGlyph::Badge(t, bg, fg) => badge(p, r.shrink(1.0), t, bg, fg, 3.0),
    }
}

/// Farbe eines Ordners: Quellen blau, Tests gruen, Ressourcen gelb (wie in IntelliJ IDEA), sonst neutral
pub fn folder_color(name: &str) -> Color32 {
    match name {
        "src" | "java" | "main" => Color32::from_rgb(0x5C, 0x9D, 0xE8),
        "test" | "tests" | "__tests__" | "spec" => Color32::from_rgb(0x5F, 0xB8, 0x65),
        "resources" | "assets" | "static" | "public" => Color32::from_rgb(0xD6, 0xB8, 0x4E),
        "lib" | "libs" | "target" | "build" | "bin" | "out" | "dist" => Color32::from_rgb(0xA0, 0x8A, 0x70),
        _ => Color32::from_rgb(0xDC, 0xB6, 0x7A),
    }
}

/// Menueeintrag mit Symbol (Platz bleibt auch ohne Symbol frei, damit alle Texte buendig stehen) und Tastenkuerzel
pub fn menu_item(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, keys: &str) -> Response {
    menu_item_colored(ui, icon.map(|i| (i, None)), text, keys)
}

pub fn menu_item_colored(ui: &mut egui::Ui, icon: Option<(Icon, Option<Color32>)>, text: &str, keys: &str) -> Response {
    menu_row(ui, icon, text, keys, false)
}

/// Untermenue mit Symbol und Pfeil, buendig mit den anderen Eintraegen
pub fn submenu<R>(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> Response {
    use egui::containers::menu::{MenuState, SubMenu};
    let id = ui.next_auto_id();
    let open = MenuState::from_ui(ui, |st, _| st.open_item == Some(SubMenu::id_from_widget_id(id)));
    let resp = menu_row(ui, icon.map(|i| (i, None)), text, "", open);
    let c = if open && style::pal().dark { Color32::WHITE } else { style::pal().text_weak };
    paint(ui.painter(), Rect::from_center_size(pos2(resp.rect.right() - 14.0, resp.rect.center().y), vec2(13.0, 13.0)), Icon::ChevronRight, c);
    SubMenu::new().show(ui, &resp, |ui| {
        style::menu_item_look(ui);
        content(ui)
    });
    resp
}

fn menu_row(ui: &mut egui::Ui, icon: Option<(Icon, Option<Color32>)>, text: &str, keys: &str, force_hot: bool) -> Response {
    let pal = style::pal();
    let enabled = ui.is_enabled();
    let font = egui::FontId::proportional(13.0);
    let g = ui.painter().layout_no_wrap(text.to_string(), font, pal.text);
    let kg = (!keys.is_empty()).then(|| ui.painter().layout_no_wrap(keys.to_string(), egui::FontId::proportional(12.0), pal.text_weak));
    let w = 34.0 + g.size().x + kg.as_ref().map(|k| k.size().x + 28.0).unwrap_or(0.0) + 24.0;
    let resp = ui.allocate_response(vec2(w, 26.0), Sense::click());
    let r = resp.rect;
    let hot = enabled && (resp.hovered() || resp.has_focus() || force_hot);
    if hot {
        ui.painter().rect_filled(r, style::RADIUS as f32, pal.selection);
    }
    let fg = if !enabled { pal.text_weak.gamma_multiply(0.6) } else if hot && pal.dark { Color32::WHITE } else { pal.text };
    if let Some((i, tint)) = icon {
        let c = if !enabled { fg } else { tint.unwrap_or(if hot { fg } else { pal.text_weak }) };
        paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 17.0, r.center().y), vec2(15.0, 15.0)), i, c);
    }
    ui.painter().galley_with_override_text_color(pos2(r.left() + 34.0, r.center().y - g.size().y / 2.0), g, fg);
    if let Some(k) = kg {
        let kc = if hot && pal.dark { Color32::from_white_alpha(170) } else { pal.text_weak };
        ui.painter().galley_with_override_text_color(pos2(r.right() - 10.0 - k.size().x, r.center().y - k.size().y / 2.0), k, kc);
    }
    resp
}

/// Kleine Ueberschrift in einem Menue (z. B. "Zuletzt geoeffnet")
pub fn menu_heading(ui: &mut egui::Ui, text: &str) {
    let pal = style::pal();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(text.to_uppercase()).size(10.5).color(pal.text_weak));
    });
    ui.add_space(1.0);
}

/// Kleiner Knopf nur mit Symbol (ohne Rahmen, Hervorhebung bei Maus darueber).
pub fn button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> Response {
    button_sized(ui, icon, tip, 22.0)
}

pub fn button_sized(ui: &mut egui::Ui, icon: Icon, tip: &str, size: f32) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let pal = style::pal();
    if resp.hovered() {
        ui.painter().rect_filled(r, style::RADIUS as f32, pal.hover);
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

/// Ausfuehren wie in IntelliJ IDEA: ruhiger Knopf mit gruenem Startsymbol
pub fn run_button(ui: &mut egui::Ui, text: &str) -> Response {
    labelled_button_tinted(ui, Icon::Play, text, false, false, Some(style::pal().ok_text))
}

/// Hervorgehobene Hauptaktion (blau gefuellt)
pub fn primary_button(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    labelled_button(ui, icon, text, true, false)
}

fn labelled_button(ui: &mut egui::Ui, icon: Icon, text: &str, primary: bool, icon_right: bool) -> Response {
    labelled_button_tinted(ui, icon, text, primary, icon_right, None)
}

/// `tint`: eigene Farbe des Symbols (nur wenn der Knopf bedienbar ist)
fn labelled_button_tinted(ui: &mut egui::Ui, icon: Icon, text: &str, primary: bool, icon_right: bool, tint: Option<Color32>) -> Response {
    let pal = style::pal();
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), pal.text);
    // Innenabstand links und rechts je 10, Symbol 14, Abstand Symbol–Text 7
    let size = vec2(galley.size().x + 41.0, ui.spacing().interact_size.y.max(24.0));
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let visuals = *ui.style().interact(&resp);
    let (fill, fg) = if primary && enabled {
        let fill = if resp.is_pointer_button_down_on() { pal.accent.gamma_multiply(0.8) } else if resp.hovered() { pal.accent.lerp_to_gamma(Color32::WHITE, 0.14) } else { pal.accent };
        (fill, Color32::WHITE)
    } else {
        (visuals.weak_bg_fill, if enabled { visuals.fg_stroke.color } else { pal.text_weak })
    };
    ui.painter().rect(r, style::RADIUS as f32, fill, if primary { Stroke::NONE } else { visuals.bg_stroke }, egui::StrokeKind::Inside);
    let (icon_x, text_x) = if icon_right { (r.right() - 17.0, r.left() + 10.0) } else { (r.left() + 17.0, r.left() + 31.0) };
    let icon_color = tint.filter(|_| enabled).unwrap_or(fg);
    paint(ui.painter(), Rect::from_center_size(pos2(icon_x, r.center().y), vec2(14.0, 14.0)), icon, icon_color);
    ui.painter().galley_with_override_text_color(pos2(text_x, r.center().y - galley.size().y / 2.0), galley, fg);
    if resp.has_focus() {
        ui.painter().rect_stroke(r, style::RADIUS as f32, Stroke::new(1.0, pal.accent), egui::StrokeKind::Outside);
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
            Sparkles, Layers, ArrowLeft, ArrowRight, Undo, Redo, Explain, Upload, Users, ArrowUp, ArrowDown, Lock, Edit, Copy, Coffee, Package, Minus,
            Bug, Flask, Braces, Script, StepOver, StepInto, StepOut, Stop, GitBranch, GitCommit, GitPullRequest, Github, CloudUpload, CloudDownload, GitMerge, CircleDot, RotateCcw,
            Code, Scissors, Clipboard, FolderInput, Power,
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
            Sparkles, Layers, ArrowLeft, ArrowRight, Undo, Redo, Explain, Upload, Users, ArrowUp, ArrowDown, Lock, Edit, Copy, Coffee, Package, Minus,
            Bug, Flask, Braces, Script, StepOver, StepInto, StepOut, Stop, GitBranch, GitCommit, GitPullRequest, Github, CloudUpload, CloudDownload, GitMerge, CircleDot, RotateCcw,
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
