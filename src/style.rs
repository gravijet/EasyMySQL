// Aussehen "Registratur": helles (Standard) oder dunkles Design; flache Flaechen mit feinen Trennlinien.

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
    /// Hauptaktion (Ausfuehren, Anlegen): Ordnergruen in beiden Designs
    pub primary: Color32,
    pub on_primary: Color32,
    /// Titelleiste: Flaeche, Schrift, schwache Schrift, Flaeche unter der Maus
    pub chrome: Color32,
    pub chrome_text: Color32,
    pub chrome_weak: Color32,
    pub chrome_hover: Color32,
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

// "Registratur": die App als Aktenregistratur. Jede Datenbank ist ein Ordner mit eigener Rueckenfarbe,
// Reiter stecken wie Haengeregister in der Farbe ihrer Datenbank, Ergebnisse werden gestempelt.
// Hell (Standard): weisses Papier, graue Aktendeckel, Titelleiste in Tinte. Dunkel: dieselbe Registratur bei Nacht.
pub static DARK_PAL: Pal = Pal {
    dark: true,
    face: rgb(0x13, 0x15, 0x1A),
    bg: rgb(0x19, 0x1C, 0x22),
    face_light: rgb(0x20, 0x24, 0x2C),
    text: rgb(0xEC, 0xEE, 0xF2),
    text_weak: rgb(0x9C, 0xA3, 0xB0),
    border: rgb(0x2B, 0x30, 0x3A),
    accent: rgb(0x8E, 0xA4, 0xFF),
    on_accent: rgb(0x0B, 0x12, 0x40),
    primary: rgb(0x16, 0x8A, 0x5E),
    on_primary: rgb(0xFF, 0xFF, 0xFF),
    chrome: rgb(0x0F, 0x11, 0x15),
    chrome_text: rgb(0xEC, 0xEE, 0xF2),
    chrome_weak: rgb(0x9C, 0xA3, 0xB0),
    chrome_hover: rgb(0x23, 0x27, 0x30),
    null_text: rgb(0x6C, 0x73, 0x81),
    error_text: rgb(0xFF, 0x8A, 0x8F),
    warn_text: rgb(0xF4, 0xA5, 0x5A),
    ok_text: rgb(0x7F, 0xD6, 0xA6),
    blue: rgb(0x8E, 0xA4, 0xFF),
    green: rgb(0x7F, 0xD6, 0xA6),
    orange: rgb(0xF4, 0xA5, 0x5A),
    coral: rgb(0xFF, 0x8A, 0x8F),
    pink: rgb(0xF2, 0x8D, 0xBF),
    lilac: rgb(0xB7, 0x9C, 0xFF),
    teal: rgb(0x6F, 0xD0, 0xDC),
    activity_fg: rgb(0x80, 0x87, 0x94),
    sidebar_bg: rgb(0x16, 0x19, 0x1E),
    current_line: rgb(0x1F, 0x23, 0x2B),
    exec_bg: rgb(0x47, 0x3C, 0x12),
    exec_fg: rgb(0xFF, 0xC6, 0x29),
    breakpoint_fg: rgb(0xFF, 0x6B, 0x72),
    diff_add_bg: rgb(0x17, 0x38, 0x2A),
    diff_del_bg: rgb(0x47, 0x21, 0x25),
    selection: rgb(0x2C, 0x3A, 0x6B),
    find_match: rgb(0x5E, 0x4C, 0x14),
    bracket: rgb(0x26, 0x44, 0x37),
    hover: rgb(0x25, 0x29, 0x32),
    er_canvas: rgb(0x15, 0x18, 0x1D),
    er_box: rgb(0x1F, 0x23, 0x2B),
    er_line: rgb(0x8E, 0xA4, 0xFF),
    er_line_dim: rgb(0x34, 0x3A, 0x46),
    er_type: rgb(0x9C, 0xA3, 0xB0),
    syn_keyword: rgb(0x8E, 0xA4, 0xFF),
    syn_function: rgb(0xF2, 0x8D, 0xBF),
    syn_string: rgb(0x7F, 0xD6, 0xA6),
    syn_number: rgb(0xF4, 0xA5, 0x5A),
    syn_comment: rgb(0x70, 0x77, 0x85),
    syn_ident: rgb(0xDC, 0xE0, 0xE8),
    syn_type: rgb(0x6F, 0xD0, 0xDC),
    syn_const: rgb(0xFF, 0xC6, 0x29),
    syn_variable: rgb(0xFF, 0x8A, 0x8F),
    syn_op: rgb(0x9C, 0xA3, 0xB0),
    syn_text: rgb(0xE6, 0xE9, 0xEF),
    syn_gutter: rgb(0x58, 0x5F, 0x6D),
};

pub static LIGHT_PAL: Pal = Pal {
    dark: false,
    // Aktendeckel: kuehles Grau fuer Leisten, Papierweiss fuer den Inhalt
    face: rgb(0xEB, 0xED, 0xF1),
    bg: rgb(0xFF, 0xFF, 0xFF),
    face_light: rgb(0xF5, 0xF6, 0xF9),
    text: rgb(0x14, 0x16, 0x1B),
    text_weak: rgb(0x56, 0x5C, 0x68),
    border: rgb(0xD6, 0xDA, 0xE2),
    // Kobalt: Fokus, Auswahl, Verweise
    accent: rgb(0x27, 0x47, 0xD6),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    // Ordnergruen: Hauptaktion (Ausfuehren, Anlegen); Tinte: Titelleiste
    primary: rgb(0x12, 0x7A, 0x53),
    on_primary: rgb(0xFF, 0xFF, 0xFF),
    chrome: rgb(0x1D, 0x22, 0x30),
    chrome_text: rgb(0xF1, 0xF3, 0xF7),
    chrome_weak: rgb(0xA9, 0xB0, 0xBF),
    chrome_hover: rgb(0x2E, 0x35, 0x47),
    null_text: rgb(0x8A, 0x90, 0x9B),
    error_text: rgb(0xC4, 0x1E, 0x33),
    warn_text: rgb(0xA0, 0x52, 0x00),
    ok_text: rgb(0x0B, 0x7A, 0x50),
    blue: rgb(0x27, 0x47, 0xD6),
    green: rgb(0x0F, 0x7F, 0x55),
    orange: rgb(0xC2, 0x5A, 0x06),
    coral: rgb(0xC9, 0x28, 0x3A),
    pink: rgb(0xC2, 0x24, 0x6E),
    lilac: rgb(0x63, 0x40, 0xCF),
    teal: rgb(0x0A, 0x75, 0x84),
    activity_fg: rgb(0x64, 0x6B, 0x78),
    sidebar_bg: rgb(0xF5, 0xF6, 0xF9),
    current_line: rgb(0xF4, 0xF6, 0xFC),
    exec_bg: rgb(0xFF, 0xF1, 0xB8),
    exec_fg: rgb(0x8B, 0x69, 0x00),
    breakpoint_fg: rgb(0xD2, 0x28, 0x3C),
    diff_add_bg: rgb(0xDD, 0xF6, 0xE8),
    diff_del_bg: rgb(0xFF, 0xE4, 0xE2),
    selection: rgb(0xDC, 0xE4, 0xFF),
    // Textmarker
    find_match: rgb(0xFF, 0xE5, 0x8A),
    bracket: rgb(0xD4, 0xF1, 0xE2),
    hover: rgb(0xE2, 0xE5, 0xEC),
    er_canvas: rgb(0xF7, 0xF8, 0xFA),
    er_box: rgb(0xFF, 0xFF, 0xFF),
    er_line: rgb(0x27, 0x47, 0xD6),
    er_line_dim: rgb(0xC9, 0xCE, 0xD8),
    er_type: rgb(0x5E, 0x64, 0x70),
    syn_keyword: rgb(0x27, 0x47, 0xD6),
    syn_function: rgb(0xC2, 0x24, 0x6E),
    syn_string: rgb(0x0F, 0x7F, 0x55),
    syn_number: rgb(0xC2, 0x5A, 0x06),
    syn_comment: rgb(0x7A, 0x80, 0x8C),
    syn_ident: rgb(0x14, 0x16, 0x1B),
    syn_type: rgb(0x0A, 0x75, 0x84),
    syn_const: rgb(0x8B, 0x69, 0x00),
    syn_variable: rgb(0xC9, 0x28, 0x3A),
    syn_op: rgb(0x4D, 0x53, 0x5E),
    syn_text: rgb(0x14, 0x16, 0x1B),
    syn_gutter: rgb(0xA0, 0xA6, 0xB1),
};

static DB_FARBEN: std::sync::Mutex<Vec<(String, usize)>> = std::sync::Mutex::new(Vec::new());

fn db_hash(name: &str) -> usize {
    name.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193)) as usize
}

/// Farben fuer die bekannten Datenbanken verteilen: jede nimmt die Farbe ihres Namens, ist die schon
/// vergeben, die naechste freie (solange es freie gibt). So haben wenige Datenbanken nie dieselbe Farbe.
pub fn set_databases(names: &[String]) {
    let mut sorted: Vec<&String> = names.iter().filter(|n| !crate::tabs::is_system_db(n)).collect();
    sorted.sort();
    let mut used = [false; DB_HUES];
    let mut out = Vec::with_capacity(sorted.len());
    for n in sorted {
        let mut i = db_hash(n) % DB_HUES;
        if used.iter().any(|u| !u) {
            while used[i] {
                i = (i + 1) % DB_HUES;
            }
        }
        used[i] = true;
        out.push((n.clone(), i));
    }
    if let Ok(mut m) = DB_FARBEN.lock() {
        *m = out;
    }
}

const DB_HUES: usize = 7;

/// Rueckenfarbe einer Datenbank: bleibt je Name gleich; Systemdatenbanken sind grau.
pub fn db_color(name: &str) -> Color32 {
    let p = pal();
    if name.is_empty() || crate::tabs::is_system_db(name) {
        return p.text_weak;
    }
    let hues: [Color32; DB_HUES] = [p.blue, p.green, p.orange, p.pink, p.teal, p.lilac, p.exec_fg];
    let known = DB_FARBEN.lock().ok().and_then(|m| m.iter().find(|(n, _)| n == name).map(|(_, i)| *i));
    hues[known.unwrap_or(db_hash(name) % DB_HUES)]
}

/// Weiche Flaeche in einer Farbe (Hintergrund von Reitern, Kennzeichen, Kopfbaendern)
pub fn soft(c: Color32) -> Color32 {
    let p = pal();
    p.bg.lerp_to_gamma(c, if p.dark { 0.22 } else { 0.13 })
}

/// Grundradius der Bedienelemente (Schaltflaechen, Eingabefelder, Listeneintraege)
pub const RADIUS: u8 = 4;
/// Radius der grossen Flaechen (Seitenleiste, Inhalt, Karten)
pub const PANEL_RADIUS: u8 = 6;

static DARK: AtomicBool = AtomicBool::new(false);

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
    v.faint_bg_color = if p.dark { rgb(0x1C, 0x20, 0x27) } else { rgb(0xF6, 0xF7, 0xFA) };
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
    let edge = Stroke::new(1.0, if p.dark { rgb(0x2D, 0x32, 0x3C) } else { rgb(0xCF, 0xD4, 0xDD) });
    let edge_hover = Stroke::new(1.0, if p.dark { rgb(0x3C, 0x43, 0x50) } else { rgb(0xB2, 0xB9, 0xC5) });
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
    if p.dark { rgb(0x10, 0x12, 0x17) } else { Color32::WHITE }
}

/// Flaechen normaler Knoepfe: ruhend, unter der Maus, gedrueckt
pub fn button_fills(p: &Pal) -> (Color32, Color32, Color32) {
    if p.dark {
        (rgb(0x24, 0x28, 0x31), rgb(0x2C, 0x31, 0x3C), rgb(0x35, 0x3B, 0x47))
    } else {
        (rgb(0xFF, 0xFF, 0xFF), rgb(0xF1, 0xF3, 0xF7), rgb(0xE4, 0xE7, 0xED))
    }
}

/// Kante der Hauptaktion (gefuellt, daher ohne Kante)
pub fn primary_edge() -> Stroke {
    Stroke::NONE
}

/// Flaeche der Hauptaktion je nach Zustand (gedrueckt etwas dunkler, unter der Maus etwas heller)
pub fn accent_fill(hot: f32, down: bool) -> Color32 {
    let p = pal();
    if down {
        p.primary.lerp_to_gamma(Color32::BLACK, 0.10)
    } else {
        p.primary.lerp_to_gamma(Color32::WHITE, 0.12 * hot)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Wenige Datenbanken bekommen nie dieselbe Rueckenfarbe, und die Farbe haengt nicht an der Reihenfolge
    #[test]
    fn database_colours_are_distinct_and_stable() {
        let names: Vec<String> = ["heiltest", "schule1", "shop", "kunden", "lager", "mysql"].iter().map(|s| s.to_string()).collect();
        set_databases(&names);
        let farben: Vec<Color32> = names[..5].iter().map(|n| db_color(n)).collect();
        for (i, a) in farben.iter().enumerate() {
            assert!(farben[i + 1..].iter().all(|b| b != a), "{farben:?}");
        }
        assert_eq!(db_color("mysql"), pal().text_weak);
        let mut rev = names.clone();
        rev.reverse();
        set_databases(&rev);
        assert_eq!(names[..5].iter().map(|n| db_color(n)).collect::<Vec<_>>(), farben);
    }
}
