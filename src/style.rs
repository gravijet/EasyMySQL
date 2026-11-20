// Aussehen: dunkles (Standard, "Tafel") oder helles Design ("Heft"); flache Flaechen mit feinen Trennlinien.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Visuals,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Farbpalette der Oberflaeche.
pub struct Pal {
    pub dark: bool,
    /// Grundflaeche (Seitenleiste, Dialoge)
    pub face: Color32,
    /// Inhaltsflaeche (Editor, Tabellen)
    pub bg: Color32,
    /// Leicht abgesetzte Flaeche (Tabellenkopf, Zeilennummern)
    pub face_light: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub border: Color32,
    pub accent: Color32,
    /// Schrift auf einer Flaeche in Akzentfarbe
    pub on_accent: Color32,
    pub null_text: Color32,
    pub error_text: Color32,
    pub warn_text: Color32,
    pub ok_text: Color32,
    /// Farben fuer Kategorien (Symbole, Kennzeichen); als Schrift auf `bg` lesbar
    pub blue: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub coral: Color32,
    pub pink: Color32,
    pub lilac: Color32,
    pub teal: Color32,
    pub activity_fg: Color32,
    pub activity_active: Color32,
    pub sidebar_bg: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub find_match: Color32,
    pub bracket: Color32,
    /// Debugger: aktuelle Zeile (Hintergrund und Pfeil) und Haltepunkte
    pub exec_bg: Color32,
    pub exec_fg: Color32,
    pub breakpoint_fg: Color32,
    /// Git: Hintergrund hinzugefuegter und entfernter Zeilen
    pub diff_add_bg: Color32,
    pub diff_del_bg: Color32,
    pub hover: Color32,
    // ER-Diagramm
    pub er_canvas: Color32,
    pub er_box: Color32,
    pub er_header: Color32,
    pub er_view_header: Color32,
    pub er_line: Color32,
    pub er_line_dim: Color32,
    pub er_type: Color32,
    // Syntax
    pub syn_keyword: Color32,
    pub syn_function: Color32,
    pub syn_string: Color32,
    pub syn_number: Color32,
    pub syn_comment: Color32,
    pub syn_ident: Color32,
    pub syn_type: Color32,
    pub syn_const: Color32,
    pub syn_variable: Color32,
    pub syn_op: Color32,
    pub syn_text: Color32,
    pub syn_gutter: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// Zwei Welten aus dem Ort, an dem SQL gelernt und benutzt wird:
// dunkel = "Tafel" (Schiefer, Kreideweiss, farbige Kreiden), hell = "Heft" (Papier, Tinte, Textmarker).
pub static DARK_PAL: Pal = Pal {
    dark: true,
    face: rgb(0x0F, 0x15, 0x13),
    bg: rgb(0x16, 0x1E, 0x1B),
    face_light: rgb(0x1D, 0x26, 0x22),
    text: rgb(0xE9, 0xEC, 0xE4),
    text_weak: rgb(0x95, 0xA2, 0x9B),
    border: rgb(0x26, 0x32, 0x2D),
    // Gelbe Kreide: Hauptaktion, Fokus, aktive Markierung
    accent: rgb(0xF2, 0xD1, 0x5C),
    on_accent: rgb(0x1A, 0x17, 0x08),
    null_text: rgb(0x6B, 0x79, 0x73),
    error_text: rgb(0xF2, 0x8B, 0x7E),
    warn_text: rgb(0xF0, 0xB0, 0x60),
    ok_text: rgb(0x93, 0xD5, 0x8A),
    blue: rgb(0x86, 0xBE, 0xEB),
    green: rgb(0x93, 0xD5, 0x8A),
    orange: rgb(0xF0, 0xB0, 0x60),
    coral: rgb(0xF2, 0x8B, 0x7E),
    pink: rgb(0xEE, 0x9A, 0xC4),
    lilac: rgb(0xB7, 0xA6, 0xEE),
    teal: rgb(0x6F, 0xD3, 0xC4),
    activity_fg: rgb(0x7F, 0x8C, 0x86),
    activity_active: rgb(0xF2, 0xF4, 0xEE),
    sidebar_bg: rgb(0x12, 0x19, 0x17),
    current_line: rgb(0x1B, 0x24, 0x20),
    exec_bg: rgb(0x47, 0x40, 0x17),
    exec_fg: rgb(0xF2, 0xD1, 0x5C),
    breakpoint_fg: rgb(0xF0, 0x6A, 0x6A),
    diff_add_bg: rgb(0x1C, 0x3A, 0x28),
    diff_del_bg: rgb(0x45, 0x23, 0x1F),
    selection: rgb(0x2A, 0x42, 0x56),
    find_match: rgb(0x5C, 0x4E, 0x1C),
    bracket: rgb(0x2E, 0x4A, 0x3A),
    hover: rgb(0x23, 0x2E, 0x2A),
    er_canvas: rgb(0x13, 0x1A, 0x17),
    er_box: rgb(0x1C, 0x25, 0x21),
    er_header: rgb(0x22, 0x37, 0x48),
    er_view_header: rgb(0x25, 0x3E, 0x2E),
    er_line: rgb(0x86, 0xBE, 0xEB),
    er_line_dim: rgb(0x33, 0x42, 0x3C),
    er_type: rgb(0x95, 0xA2, 0x9B),
    syn_keyword: rgb(0x86, 0xBE, 0xEB),
    syn_function: rgb(0xEE, 0x9A, 0xC4),
    syn_string: rgb(0x9C, 0xD6, 0x8F),
    syn_number: rgb(0xF0, 0xB0, 0x60),
    syn_comment: rgb(0x74, 0x85, 0x7D),
    syn_ident: rgb(0xD9, 0xE6, 0xDF),
    syn_type: rgb(0xB7, 0xA6, 0xEE),
    syn_const: rgb(0xF2, 0xD1, 0x5C),
    syn_variable: rgb(0xF2, 0x8B, 0x7E),
    syn_op: rgb(0x9A, 0xA7, 0xA0),
    syn_text: rgb(0xE6, 0xE9, 0xE1),
    syn_gutter: rgb(0x5A, 0x67, 0x61),
};

pub static LIGHT_PAL: Pal = Pal {
    dark: false,
    face: rgb(0xEE, 0xF2, 0xF0),
    bg: rgb(0xFF, 0xFF, 0xFF),
    face_light: rgb(0xF7, 0xF9, 0xF8),
    text: rgb(0x18, 0x20, 0x1C),
    text_weak: rgb(0x56, 0x62, 0x5C),
    border: rgb(0xD9, 0xE0, 0xDC),
    // Koenigsblaue Tinte
    accent: rgb(0x23, 0x40, 0xB8),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    null_text: rgb(0x80, 0x8B, 0x85),
    error_text: rgb(0xC2, 0x36, 0x2B),
    warn_text: rgb(0x99, 0x58, 0x00),
    ok_text: rgb(0x1D, 0x7A, 0x3A),
    blue: rgb(0x1F, 0x5F, 0xC4),
    green: rgb(0x1D, 0x7A, 0x3A),
    orange: rgb(0xB0, 0x4A, 0x00),
    coral: rgb(0xC2, 0x36, 0x2B),
    pink: rgb(0xA3, 0x24, 0x7A),
    lilac: rgb(0x5E, 0x45, 0xB5),
    teal: rgb(0x0A, 0x74, 0x72),
    activity_fg: rgb(0x6A, 0x75, 0x70),
    activity_active: rgb(0x18, 0x20, 0x1C),
    sidebar_bg: rgb(0xF5, 0xF7, 0xF6),
    current_line: rgb(0xF4, 0xF7, 0xF5),
    exec_bg: rgb(0xFF, 0xF1, 0xB8),
    exec_fg: rgb(0xA6, 0x74, 0x00),
    breakpoint_fg: rgb(0xD3, 0x34, 0x3A),
    diff_add_bg: rgb(0xE1, 0xF5, 0xE5),
    diff_del_bg: rgb(0xFC, 0xE6, 0xE3),
    selection: rgb(0xD5, 0xDE, 0xFA),
    // Textmarker
    find_match: rgb(0xFF, 0xE5, 0x8A),
    bracket: rgb(0xD3, 0xEE, 0xD9),
    hover: rgb(0xE7, 0xEC, 0xE9),
    er_canvas: rgb(0xF8, 0xFA, 0xF9),
    er_box: rgb(0xFF, 0xFF, 0xFF),
    er_header: rgb(0xDF, 0xE6, 0xFB),
    er_view_header: rgb(0xDD, 0xF2, 0xE2),
    er_line: rgb(0x23, 0x40, 0xB8),
    er_line_dim: rgb(0xC6, 0xCF, 0xCA),
    er_type: rgb(0x5E, 0x6A, 0x64),
    syn_keyword: rgb(0x23, 0x40, 0xB8),
    syn_function: rgb(0xA3, 0x24, 0x7A),
    syn_string: rgb(0x1E, 0x7A, 0x3C),
    syn_number: rgb(0xB0, 0x4A, 0x00),
    syn_comment: rgb(0x6E, 0x79, 0x73),
    syn_ident: rgb(0x18, 0x20, 0x1C),
    syn_type: rgb(0x0A, 0x74, 0x72),
    syn_const: rgb(0x8A, 0x5A, 0x00),
    syn_variable: rgb(0xB3, 0x36, 0x1F),
    syn_op: rgb(0x4D, 0x56, 0x52),
    syn_text: rgb(0x18, 0x20, 0x1C),
    syn_gutter: rgb(0x9C, 0xA6, 0xA1),
};

/// Grundradius der Bedienelemente (Schaltflaechen, Eingabefelder, Listeneintraege)
pub const RADIUS: u8 = 4;
/// Radius der grossen Flaechen (Seitenleiste, Inhalt, Karten)
pub const PANEL_RADIUS: u8 = 6;

static DARK: AtomicBool = AtomicBool::new(true);

pub fn pal() -> &'static Pal {
    if DARK.load(Ordering::Relaxed) { &DARK_PAL } else { &LIGHT_PAL }
}

fn load_font(candidates: &[&str]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    // Eingebettete Schriften (Onest in drei Staerken und JetBrains Mono NL ohne Ligaturen, damit != nicht als Zeichen
    // erscheint; SIL OFL, Lizenzen in assets/fonts): auf jedem System gleich. Systemschriften folgen als Reserve.
    let mut fonts = FontDefinitions::default();
    let embedded: [(&str, &'static [u8]); 4] = [
        ("onest", include_bytes!("../assets/fonts/Onest-Regular.ttf")),
        ("onest-medium", include_bytes!("../assets/fonts/Onest-Medium.ttf")),
        ("onest-semi", include_bytes!("../assets/fonts/Onest-SemiBold.ttf")),
        ("jetbrains-mono", include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf")),
    ];
    for (name, bytes) in embedded {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "onest".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "jetbrains-mono".into());
    let prop = load_font(&["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\tahoma.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]);
    let mono = load_font(&["C:\\Windows\\Fonts\\consola.ttf", "C:\\Windows\\Fonts\\cour.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"]);
    if let Some(data) = prop {
        fonts.font_data.insert("sys".into(), Arc::new(FontData::from_owned(data)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(1, "sys".into());
    }
    if let Some(data) = mono {
        fonts.font_data.insert("sysmono".into(), Arc::new(FontData::from_owned(data)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(1, "sysmono".into());
    }
    // Kraeftigere Staerken mit denselben Reserven fuer fehlende Zeichen
    let reserve: Vec<String> = fonts.families[&FontFamily::Proportional].iter().skip(1).cloned().collect();
    for (family, first) in [(MEDIUM, "onest-medium"), (SEMI, "onest-semi")] {
        let mut list = vec![first.to_string()];
        list.extend(reserve.iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
    set_theme(ctx, dark);
}

const MEDIUM: &str = "medium";
const SEMI: &str = "semi";

/// Schrift mittlerer Staerke (Knoepfe, Reiter)
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

/// Halbfette Schrift (Ueberschriften, Hervorhebungen)
pub fn semi(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMI.into()))
}

/// Hervorhebung in echter halbfetter Schrift statt nur in hellerer Farbe
pub trait Semi {
    fn semi(self) -> egui::RichText;
}

impl<T: Into<egui::RichText>> Semi for T {
    fn semi(self) -> egui::RichText {
        self.into().strong().family(FontFamily::Name(SEMI.into()))
    }
}

pub fn set_theme(ctx: &egui::Context, dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            // Klein ist nur wenig kleiner als normal: alles bleibt gut lesbar und wirkt aus einem Guss
            (TextStyle::Small, FontId::proportional(12.5)),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Name(MEDIUM.into()))),
            (TextStyle::Heading, FontId::new(17.0, FontFamily::Name(SEMI.into()))),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(8.0, 6.0);
        sp.button_padding = egui::vec2(12.0, 4.0);
        sp.interact_size.y = 28.0;
        sp.icon_width = 16.0;
        sp.icon_width_inner = 8.0;
        sp.icon_spacing = 8.0;
        sp.slider_width = 200.0;
        sp.slider_rail_height = 4.0;
        sp.window_margin = Margin::same(16);
        sp.menu_margin = Margin::same(5);
        sp.menu_spacing = 4.0;
        // Schlanke Bildlaufleisten wie in IntelliJ IDEA: dezent, ueber dem Inhalt, beim Darueberfahren breiter
        sp.scroll = egui::style::ScrollStyle {
            floating: true,
            bar_width: 10.0,
            floating_width: 7.0,
            floating_allocated_width: 7.0,
            handle_min_length: 28.0,
            foreground_color: true,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.0,
            interact_background_opacity: 0.08,
            dormant_handle_opacity: 0.28,
            active_handle_opacity: 0.45,
            interact_handle_opacity: 0.7,
            ..egui::style::ScrollStyle::solid()
        };
        sp.combo_width = 160.0;
        // Kurze Uebergaenge (Auf-/Zuklappen, Schalter) statt harter Spruenge
        style.animation_time = 0.12;
        style.visuals = visuals(pal());
    });
}

fn visuals(p: &Pal) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(RADIUS);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    // Schatten im Farbton der Flaeche statt grauem Nebel
    let shade = if p.dark { Color32::from_rgba_premultiplied(2, 6, 4, 150) } else { Color32::from_rgba_premultiplied(10, 30, 20, 34) };
    v.window_shadow = Shadow { offset: [0, 12], blur: 36, spread: 0, color: shade };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: shade.gamma_multiply(0.75) };
    v.window_fill = p.face_light;
    v.panel_fill = p.face;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.bg;
    // Eingabefelder: im dunklen Design eine vertiefte Schieferflaeche, im hellen weisses Papier
    v.text_edit_bg_color = Some(field_fill(p));
    v.faint_bg_color = if p.dark { rgb(0x1A, 0x23, 0x1F) } else { rgb(0xF4, 0xF7, 0xF5) };
    v.code_bg_color = p.bg;
    v.hyperlink_color = if p.dark { p.blue } else { p.accent };
    v.selection.bg_fill = p.selection;
    // Fokusrahmen von Eingabefeldern und Schaltflaechen
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.window_highlight_topmost = false;
    v.striped = false;
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.text_weak);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);

    let w = &mut v.widgets;
    // Knoepfe: weiche Flaeche ohne harte Kante; die feine Linie zeichnet nur Eingabefelder sichtbar nach
    let (btn, btn_hover, btn_active) = button_fills(p);
    let edge = Stroke::new(1.0, if p.dark { rgb(0x2A, 0x36, 0x31) } else { rgb(0xCF, 0xD7, 0xD3) });
    let edge_hover = Stroke::new(1.0, if p.dark { rgb(0x3A, 0x48, 0x42) } else { rgb(0xB3, 0xBE, 0xB8) });
    w.noninteractive.bg_fill = p.face;
    w.noninteractive.weak_bg_fill = p.face;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.corner_radius = r;
    for (ws, fill, stroke) in [
        (&mut w.inactive, btn, edge),
        (&mut w.hovered, btn_hover, edge_hover),
        (&mut w.active, btn_active, Stroke::new(1.0, p.accent)),
        (&mut w.open, btn_hover, Stroke::new(1.0, p.accent)),
    ] {
        ws.bg_fill = fill;
        ws.weak_bg_fill = fill;
        ws.bg_stroke = stroke;
        ws.fg_stroke = Stroke::new(1.0, p.text);
        ws.corner_radius = r;
        ws.expansion = 0.0;
    }
    v
}

/// Flaeche von Eingabefeldern
pub fn field_fill(p: &Pal) -> Color32 {
    if p.dark { rgb(0x0E, 0x14, 0x12) } else { Color32::WHITE }
}

/// Flaechen normaler Knoepfe: ruhend, unter der Maus, gedrueckt
pub fn button_fills(p: &Pal) -> (Color32, Color32, Color32) {
    if p.dark {
        (rgb(0x22, 0x2C, 0x28), rgb(0x2B, 0x37, 0x32), rgb(0x33, 0x41, 0x3B))
    } else {
        (rgb(0xED, 0xF1, 0xEF), rgb(0xE3, 0xE9, 0xE6), rgb(0xD8, 0xE0, 0xDC))
    }
}

/// Flaeche der Hauptaktion je nach Zustand (gedrueckt etwas dunkler, unter der Maus etwas heller)
pub fn accent_fill(hot: f32, down: bool) -> Color32 {
    let p = pal();
    if down {
        p.accent.lerp_to_gamma(Color32::BLACK, 0.12)
    } else {
        p.accent.lerp_to_gamma(Color32::WHITE, if p.dark { 0.16 } else { 0.10 } * hot)
    }
}

/// Einträge in Menüs: mehr Innenabstand, beim Darüberfahren eine abgerundete Fläche in der Auswahlfarbe
pub fn menu_item_look(ui: &mut egui::Ui) {
    let p = pal();
    // Kompakte Eintraege wie in IntelliJ IDEA (26 px hoch, kaum Abstand dazwischen)
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.spacing_mut().interact_size.y = 26.0;
    ui.spacing_mut().item_spacing.y = 1.0;
    let w = &mut ui.visuals_mut().widgets;
    w.hovered.weak_bg_fill = p.selection;
    w.hovered.bg_fill = p.selection;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, if p.dark { Color32::WHITE } else { p.text });
    w.active.weak_bg_fill = p.selection;
    w.active.bg_fill = p.selection;
    w.active.bg_stroke = Stroke::NONE;
    w.open.weak_bg_fill = p.selection;
    w.open.bg_fill = p.selection;
    w.open.bg_stroke = Stroke::NONE;
    w.inactive.bg_stroke = Stroke::NONE;
}

/// Umrandeter Inhaltsbereich (wie ein Listenfeld).
pub fn sunken_frame() -> egui::Frame {
    egui::Frame::new().fill(pal().bg).stroke(Stroke::new(1.0, pal().border)).corner_radius(CornerRadius::same(RADIUS)).inner_margin(Margin::same(3))
}

/// Flache Flaeche, die genau den freien Platz fuellt – unabhaengig davon, wie gross der Inhalt
/// ist (zu breiter Inhalt wird abgeschnitten statt die Flaeche zu verbreitern).
pub fn panel_card(ui: &mut egui::Ui, fill: Color32, margin: Margin, add: impl FnOnce(&mut egui::Ui)) {
    let outer = ui.available_rect_before_wrap();
    ui.painter().rect_filled(outer, 0.0, fill);
    let inner = egui::Rect::from_min_max(
        outer.min + egui::vec2(margin.left as f32, margin.top as f32),
        outer.max - egui::vec2(margin.right as f32, margin.bottom as f32),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(outer.intersect(ui.clip_rect()));
    add(&mut child);
    ui.advance_cursor_after_rect(outer);
}

/// Gruppenrahmen (Karte).
pub fn group_frame() -> egui::Frame {
    crate::widgets::card()
}

/// Navigation mit einheitlichem Einzug, Hover-Feedback und aktiver Kategorie.
pub fn nav_item(ui: &mut egui::Ui, title: &str, active: bool, indent: f32) -> egui::Response {
    let p = pal();
    let width = ui.available_width();
    let color = if active && !p.dark { p.accent } else { p.text };
    let text = ui.painter().layout(title.to_string(), FontId::proportional(13.0), color, (width - indent - 24.0).max(40.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, text.size().y + 14.0), egui::Sense::click());
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, RADIUS as f32, if active { p.selection } else { p.hover });
    }
    ui.painter().galley(rect.min + egui::vec2(indent + 14.0, 7.0), text, color);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), active, title));
    response
}
