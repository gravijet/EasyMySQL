// Werkzeugfenster wie in IntelliJ IDEA. Explorer, Suchen, Datenbanken, Verlauf, Git, Claude, Codex und Terminal
// stehen als Symbole in Leisten: links oben (Werkzeuge links und in eigenen Fenstern), links unten (Werkzeuge unten)
// und rechts. Ein Symbol oder die Kopfzeile eines offenen Werkzeugs laesst sich ziehen:
// - in eine Leiste: genau zwischen zwei Symbole, die anderen ruecken zur Seite (Reihenfolge wird gemerkt);
// - an einen Rand des Fensters: eine Vorschau zeigt die Flaeche, in der das Werkzeug erscheint;
// - aus dem Hauptfenster hinaus: eigenes Fenster an dieser Stelle. Aus dem eigenen Fenster geht es genauso zurueck.
// Mitten im Editor losgelassen oder mit Escape bleibt alles, wie es war. Platz und Reihenfolge werden gemerkt
// (dock_<name>=<platz>, tool_order=...). Die Leisten gleiten beim Oeffnen und Schliessen herein und hinaus.

use super::*;
use crate::icons::{self, Icon};
use eframe::egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

/// Hoehe eines Symbols in der Leiste, Abstand dazwischen, Breite der Leiste
pub const TILE: f32 = 46.0;
const GAP: f32 = 4.0;
const STEP: f32 = TILE + GAP;
pub const STRIPE_W: f32 = 60.0;

/// Platz eines Werkzeugfensters
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Place {
    Left,
    Right,
    Bottom,
    Window,
}

impl Place {
    pub const ALL: [Place; 4] = [Place::Left, Place::Right, Place::Bottom, Place::Window];

    pub fn id(self) -> &'static str {
        match self {
            Place::Left => "links",
            Place::Right => "rechts",
            Place::Bottom => "unten",
            Place::Window => "fenster",
        }
    }

    pub fn from_id(s: &str) -> Option<Place> {
        Place::ALL.into_iter().find(|p| p.id() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Place::Left => crate::i18n::tr("Links", "Left"),
            Place::Right => crate::i18n::tr("Rechts", "Right"),
            Place::Bottom => crate::i18n::tr("Unten", "Bottom"),
            Place::Window => crate::i18n::tr("Eigenes Fenster", "Separate window"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Place::Left => Icon::ChevronLeft,
            Place::Right => Icon::ChevronRight,
            Place::Bottom => Icon::ChevronDown,
            Place::Window => Icon::External,
        }
    }

    /// Leiste, in der das Symbol steht
    pub fn stripe(self) -> Stripe {
        match self {
            Place::Left | Place::Window => Stripe::LeftTop,
            Place::Bottom => Stripe::LeftBottom,
            Place::Right => Stripe::Right,
        }
    }
}

/// Gruppe von Symbolen in einer Leiste
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stripe {
    LeftTop,
    LeftBottom,
    Right,
}

impl Stripe {
    fn place(self) -> Place {
        match self {
            Stripe::LeftTop => Place::Left,
            Stripe::LeftBottom => Place::Bottom,
            Stripe::Right => Place::Right,
        }
    }
}

/// Wohin ein gezogenes Werkzeug beim Loslassen kommt
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Target {
    /// Nichts aendern (Mitte des Editors, Escape)
    #[default]
    Cancel,
    /// In eine Leiste an Stelle n
    Slot(Stripe, usize),
    /// An einen Rand (ans Ende seiner Leiste)
    Edge(Place),
    /// Ausserhalb des Hauptfensters: eigenes Fenster
    Float,
}

impl Target {
    fn place(self) -> Option<Place> {
        match self {
            Target::Cancel => None,
            Target::Slot(s, _) => Some(s.place()),
            Target::Edge(p) => Some(p),
            Target::Float => Some(Place::Window),
        }
    }
}

/// Alle Werkzeuge in der Grundreihenfolge
pub const TOOLS: [SideView; 8] = [
    SideView::Explorer,
    SideView::Search,
    SideView::Databases,
    SideView::History,
    SideView::Git,
    SideView::Claude,
    SideView::Codex,
    SideView::Terminal,
];

/// Voreinstellung: Terminal unten, alles andere links
pub fn default_place(v: SideView) -> Place {
    if v == SideView::Terminal { Place::Bottom } else { Place::Left }
}

/// Gemerkte Reihenfolge; unbekannte Namen fallen weg, fehlende Werkzeuge kommen in der Grundreihenfolge dazu
pub fn parse_order(s: &str) -> Vec<SideView> {
    let mut out: Vec<SideView> = Vec::with_capacity(TOOLS.len());
    for n in s.split(',').map(str::trim) {
        if let Some(v) = TOOLS.iter().find(|v| v.name() == n)
            && !out.contains(v)
        {
            out.push(*v);
        }
    }
    for v in TOOLS {
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// `v` in einer Leiste an Stelle `index` einsortieren. `group`: die Werkzeuge dieser Leiste ohne `v`, in Reihenfolge.
pub fn reorder(order: &[SideView], group: &[SideView], v: SideView, index: usize) -> Vec<SideView> {
    let mut out: Vec<SideView> = order.iter().copied().filter(|t| *t != v).collect();
    let pos = match group.get(index) {
        Some(before) => out.iter().position(|t| t == before).unwrap_or(out.len()),
        None => group.last().and_then(|last| out.iter().position(|t| t == last)).map_or(out.len(), |p| p + 1),
    };
    out.insert(pos, v);
    out
}

pub fn tool_icon(v: SideView) -> Icon {
    match v {
        SideView::Explorer => Icon::Files,
        SideView::Search => Icon::Search,
        SideView::Databases => Icon::Database,
        SideView::History => Icon::History,
        SideView::Git => Icon::GitBranch,
        SideView::Claude => Icon::Claude,
        SideView::Codex => Icon::Codex,
        SideView::Terminal => Icon::Terminal,
    }
}

/// Farbe eines Werkzeugs (Symbol, offenes Symbol in der Leiste)
pub fn tool_hue(v: SideView) -> Color32 {
    let p = crate::style::pal();
    match v {
        SideView::Explorer => p.blue,
        SideView::Search => p.lilac,
        SideView::Databases => p.teal,
        SideView::History => p.orange,
        SideView::Git => p.coral,
        SideView::Claude => p.orange,
        SideView::Codex => p.lilac,
        SideView::Terminal => p.green,
    }
}

/// Kurze Beschriftung unter dem Symbol in der Leiste
pub fn tool_caption(v: SideView) -> &'static str {
    match v {
        SideView::Explorer => crate::i18n::tr("Dateien", "Files"),
        SideView::Search => crate::i18n::tr("Suchen", "Search"),
        SideView::Databases => crate::i18n::tr("Daten", "Data"),
        SideView::History => crate::i18n::tr("Verlauf", "History"),
        SideView::Git => "Git",
        SideView::Claude => "Claude",
        SideView::Codex => "Codex",
        SideView::Terminal => "Terminal",
    }
}

pub fn tool_title(v: SideView) -> &'static str {
    match v {
        SideView::Explorer => "Explorer",
        SideView::Search => crate::i18n::tr("Suchen", "Search"),
        SideView::Databases => crate::i18n::tr("Datenbanken", "Databases"),
        SideView::History => crate::i18n::tr("Verlauf", "History"),
        SideView::Git => "Git",
        SideView::Claude => "Claude Code",
        SideView::Codex => "Codex",
        SideView::Terminal => "Terminal",
    }
}

fn shortcut(v: SideView) -> Option<Cmd> {
    match v {
        SideView::Explorer => Some(Cmd::ShowExplorer),
        SideView::Search => Some(Cmd::ShowSearch),
        SideView::Databases => Some(Cmd::ShowDatabases),
        SideView::History => Some(Cmd::ShowHistory),
        SideView::Git => Some(Cmd::ShowGit),
        SideView::Terminal => Some(Cmd::ToggleTerminal),
        SideView::Claude | SideView::Codex => None,
    }
}

/// Lage der Leisten im Hauptfenster (vom letzten Bild), fuer die Trefferpruefung beim Ziehen
#[derive(Clone)]
pub struct Geo {
    pub left: Rect,
    pub right: Option<Rect>,
    /// Gruppen: Flaeche, in der ein Symbol dort landet, und die Mitten der vorhandenen Symbole (ohne das gezogene)
    pub groups: Vec<(Stripe, Rect, Vec<f32>)>,
}

impl Default for Geo {
    fn default() -> Self {
        Geo { left: Rect::NOTHING, right: None, groups: Vec::new() }
    }
}

/// Flaeche zwischen den Leisten (Seitenleisten, Editor, unten)
fn work_area(geo: &Geo, window: Rect) -> Rect {
    let right = geo.right.map_or(window.right(), |r| r.left());
    Rect::from_min_max(pos2(geo.left.right(), geo.left.top()), pos2(right, window.bottom()))
}

/// Unsichtbare rechte Leiste beim Ziehen, solange rechts noch nichts steht
fn ghost_right(window: Rect, area: Rect) -> Rect {
    Rect::from_min_max(pos2(window.right() - STRIPE_W, area.top()), pos2(window.right(), window.bottom()))
}

/// Ziel unter dem Zeiger (Koordinaten des Hauptfensters; `None`: Zeiger ausserhalb)
pub fn target_at(geo: &Geo, window: Rect, p: Option<Pos2>) -> Target {
    let Some(p) = p.filter(|p| window.contains(*p)) else { return Target::Float };
    for (s, region, centers) in &geo.groups {
        if region.contains(p) {
            return Target::Slot(*s, centers.iter().filter(|c| **c < p.y).count());
        }
    }
    let a = work_area(geo, window);
    if geo.right.is_none() && ghost_right(window, a).contains(p) {
        return Target::Slot(Stripe::Right, 0);
    }
    if !a.contains(p) {
        return Target::Cancel;
    }
    let side = (a.width() * 0.2).clamp(70.0, 260.0);
    if p.x < a.left() + side {
        Target::Edge(Place::Left)
    } else if p.x > a.right() - side {
        Target::Edge(Place::Right)
    } else if p.y > a.bottom() - (a.height() * 0.3).clamp(80.0, 300.0) {
        Target::Edge(Place::Bottom)
    } else {
        Target::Cancel
    }
}

/// Flaeche, in der das Werkzeug nach dem Loslassen erscheint (Vorschau)
fn preview_rect(geo: &Geo, window: Rect, target: Target) -> Option<Rect> {
    let a = work_area(geo, window);
    let side = (a.width() * 0.24).clamp(220.0, 360.0).min(a.width() * 0.5);
    let bottom = (a.height() * 0.32).clamp(140.0, 340.0).min(a.height() * 0.6);
    match target.place()? {
        Place::Left => Some(Rect::from_min_max(a.min, pos2(a.left() + side, a.bottom()))),
        Place::Right => Some(Rect::from_min_max(pos2(a.right() - side, a.top()), a.max)),
        Place::Bottom => Some(Rect::from_min_max(pos2(a.left(), a.bottom() - bottom), a.max)),
        Place::Window => None,
    }
}

/// Gerade gezogenes Werkzeug
pub struct Drag {
    pub view: SideView,
    /// Fenster, in dem gezogen wird
    viewport: egui::ViewportId,
    /// Zeiger (Bildschirmkoordinaten, oder im Hauptfenster, wenn das System keine Fensterlage meldet)
    pointer: Option<Pos2>,
    /// Zeiger hat das Fenster verlassen
    gone: bool,
    /// Lage des Zeigers in Bildschirmkoordinaten bekannt
    screen: bool,
    released: bool,
}

/// Offene Werkzeugfenster ausser links (links: `side_view` und `settings.sidebar`) und der Zustand beim Ziehen
#[derive(Default)]
pub struct Dock {
    pub right: Option<SideView>,
    pub bottom: Option<SideView>,
    pub windows: Vec<SideView>,
    pub drag: Option<Drag>,
    /// Ziel unter dem Zeiger (vom letzten Bild; die Leisten oeffnen dort eine Luecke)
    target: Target,
    geo: Geo,
    /// Vorschau: aktuelle (gleitende) Flaeche und Deckkraft
    preview: Option<Rect>,
    preview_alpha: f32,
    /// Werkzeug der Vorschau (bleibt beim Ausblenden nach dem Loslassen stehen)
    previewed: Option<SideView>,
    /// Zuletzt gezeigtes Werkzeug je Platz und Art (fuer das Hinausgleiten nach dem Schliessen)
    last: std::collections::HashMap<(Place, bool), SideView>,
    /// Wo ein neues eigenes Fenster erscheinen soll
    window_at: std::collections::HashMap<&'static str, Pos2>,
}

impl Dock {
    /// Ziehen beginnen (im Fenster `viewport`)
    pub fn start_drag(&mut self, v: SideView, viewport: egui::ViewportId) {
        self.drag = Some(Drag { view: v, viewport, pointer: None, gone: false, screen: false, released: false });
        self.target = Target::Cancel;
    }

    pub fn dragging(&self) -> Option<SideView> {
        self.drag.as_ref().map(|d| d.view)
    }

    /// Zeiger und Loslassen in dem Fenster verfolgen, in dem gezogen wird
    pub fn track(&mut self, ctx: &egui::Context) {
        let Some(d) = self.drag.as_mut() else { return };
        if d.viewport != ctx.viewport_id() {
            return;
        }
        let (pos, down, origin, esc) =
            ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.viewport().inner_rect.map(|r| r.min), i.key_pressed(egui::Key::Escape)));
        if esc {
            self.drag = None;
            self.target = Target::Cancel;
        } else {
            d.gone = pos.is_none();
            match (pos, origin) {
                (Some(p), Some(o)) => {
                    d.pointer = Some(p + o.to_vec2());
                    d.screen = true;
                }
                // Ohne Fensterlage (z. B. Wayland) nur im Hauptfenster: dort reichen eigene Koordinaten
                (Some(p), None) if d.viewport == egui::ViewportId::ROOT => {
                    d.pointer = Some(p);
                    d.screen = false;
                }
                (Some(_), None) => d.pointer = None,
                (None, _) => {}
            }
            d.released |= !down;
        }
        ctx.request_repaint();
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Platz laut Einstellungen
fn place_in(settings: &crate::settings::Settings, v: SideView) -> Place {
    settings.dock.get(v.name()).and_then(|p| Place::from_id(p)).unwrap_or_else(|| default_place(v))
}

fn is_chat(v: SideView) -> bool {
    matches!(v, SideView::Claude | SideView::Codex)
}

impl Dock {
    /// Beim Start: zuletzt offene Werkzeuge rechts, unten und in eigenen Fenstern (Terminals entstehen erst bei Bedarf)
    pub fn restore(settings: &crate::settings::Settings) -> Self {
        let mut d = Dock::default();
        for v in settings.dock_open.split(',').filter(|n| !n.is_empty() && *n != "terminal").map(SideView::from_name) {
            match place_in(settings, v) {
                Place::Right => d.right = Some(v),
                Place::Bottom => d.bottom = Some(v),
                Place::Window if !d.windows.contains(&v) => d.windows.push(v),
                _ => {}
            }
        }
        d
    }

    fn open_names(&self) -> String {
        self.right.iter().chain(self.bottom.iter()).chain(self.windows.iter()).map(|v| v.name()).collect::<Vec<_>>().join(",")
    }
}

impl EasyApp {
    pub(super) fn place_of(&self, v: SideView) -> Place {
        place_in(&self.settings, v)
    }

    fn tool_order(&self) -> Vec<SideView> {
        parse_order(&self.settings.tool_order)
    }

    /// Werkzeuge einer Leiste in Reihenfolge
    fn group(&self, stripe: Stripe) -> Vec<SideView> {
        self.tool_order().into_iter().filter(|v| self.place_of(*v).stripe() == stripe).collect()
    }

    /// Offene Werkzeuge fuer den naechsten Start merken
    fn remember_open(&mut self) {
        let names = self.dock.open_names();
        if self.settings.dock_open != names {
            self.settings.dock_open = names;
            self.settings.save();
        }
    }

    pub(super) fn tool_open(&self, v: SideView) -> bool {
        match self.place_of(v) {
            Place::Left => self.settings.sidebar && self.side_view == v,
            Place::Right => self.dock.right == Some(v),
            Place::Bottom => self.dock.bottom == Some(v),
            Place::Window => self.dock.windows.contains(&v),
        }
    }

    /// Zeigen, wo es steht; ein anderes Werkzeug am selben Platz (links, rechts, unten) weicht
    pub(super) fn show_tool(&mut self, v: SideView) {
        match self.place_of(v) {
            Place::Left => {
                self.side_view = v;
                if !self.settings.sidebar {
                    self.settings.sidebar = true;
                    self.settings.save();
                }
            }
            Place::Right => self.dock.right = Some(v),
            Place::Bottom => self.dock.bottom = Some(v),
            Place::Window => {
                if !self.dock.windows.contains(&v) {
                    self.dock.windows.push(v);
                }
            }
        }
        if v == SideView::Terminal {
            self.terminal.focus = true;
        }
        self.remember_open();
    }

    pub(super) fn hide_tool(&mut self, v: SideView) {
        match self.place_of(v) {
            Place::Left if self.side_view == v && self.settings.sidebar => {
                self.settings.sidebar = false;
                self.settings.save();
            }
            Place::Left => {}
            Place::Right if self.dock.right == Some(v) => self.dock.right = None,
            Place::Bottom if self.dock.bottom == Some(v) => self.dock.bottom = None,
            Place::Right | Place::Bottom => {}
            Place::Window => self.dock.windows.retain(|w| *w != v),
        }
        self.remember_open();
    }

    pub(super) fn toggle_tool(&mut self, v: SideView) {
        if self.tool_open(v) {
            self.hide_tool(v);
        } else if v == SideView::Terminal && self.terminal_empty() {
            let ctx = CTX.lock_ok().clone();
            if let Some(ctx) = ctx {
                self.new_terminal(&ctx, None);
            }
        } else {
            self.show_tool(v);
        }
    }

    /// An einen anderen Platz verschieben und dort zeigen
    pub(super) fn move_tool(&mut self, v: SideView, to: Place) {
        self.hide_tool(v);
        if to == default_place(v) {
            self.settings.dock.remove(v.name());
        } else {
            self.settings.dock.insert(v.name().to_string(), to.id().to_string());
        }
        self.settings.save();
        // Nach dem Verschieben sieht man es am neuen Platz
        if v == SideView::Terminal && self.terminal_empty() {
            let ctx = CTX.lock_ok().clone();
            if let Some(ctx) = ctx {
                self.new_terminal(&ctx, None);
                return;
            }
        }
        self.show_tool(v);
    }

    /// Losgelassen: Platz und Stelle in der Leiste uebernehmen
    fn drop_tool(&mut self, v: SideView, target: Target) {
        let Some(to) = target.place() else { return };
        if let Target::Slot(stripe, index) = target {
            let order = self.tool_order();
            let group: Vec<SideView> = order.iter().copied().filter(|t| *t != v && self.place_of(*t).stripe() == stripe).collect();
            let new = reorder(&order, &group, v, index);
            if new != order {
                self.settings.tool_order = new.iter().map(|t| t.name()).collect::<Vec<_>>().join(",");
                self.settings.save();
            }
        }
        // Ein eigenes Fenster in die linke Leiste gezogen wird wieder links angedockt
        if to != self.place_of(v) {
            self.move_tool(v, to);
        } else if !matches!(target, Target::Slot(..)) && !self.tool_open(v) {
            self.show_tool(v);
        }
    }

    /// Menue "Verschieben nach" (Rechtsklick auf ein Symbol oder die Kopfzeile)
    pub(super) fn tool_place_menu(&mut self, ui: &mut egui::Ui, v: SideView) {
        crate::style::menu_item_look(ui);
        ui.label(egui::RichText::new(crate::tr_format!("{} verschieben nach", "Move {} to", tool_title(v))).small().color(style::pal().text_weak));
        let here = self.place_of(v);
        for p in Place::ALL {
            let label = if p == here { format!("{}  ✓", p.label()) } else { p.label().to_string() };
            if icons::menu_item(ui, Some(p.icon()), &label, "").clicked() {
                if p != here {
                    self.move_tool(v, p);
                }
                ui.close();
            }
        }
        if self.tool_open(v) && icons::menu_item(ui, Some(Icon::Close), crate::i18n::tr("Ausblenden", "Hide"), "").clicked() {
            self.hide_tool(v);
            ui.close();
        }
    }

    /// Inhalt eines Werkzeugfensters (gleich, wo es steht). Die Kopfzeile ist der Griff zum Verschieben. Der Griff
    /// zum Ziehen liegt ueber dem Inhalt (sonst schlucken Ueberschriften oder auswaehlbarer Text das Ziehen); Knoepfe
    /// darin bleiben klickbar, weil er nur auf Ziehen reagiert. Rechtsklick auf die Kopfzeile zeigt die Plaetze.
    pub(super) fn tool_ui(&mut self, ui: &mut egui::Ui, v: SideView, actions: &mut Vec<Action>) {
        let head = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 30.0));
        let menu = ui.interact(head, egui::Id::new(("tool-menu", v.name())), Sense::click());
        egui::Popup::context_menu(&menu).show(|ui| self.tool_place_menu(ui, v));
        if v == SideView::Terminal {
            self.terminal_view(ui);
        } else {
            self.side_panel_for(ui, v, actions);
        }
        let grip = ui.interact(head, egui::Id::new(("tool-grip", v.name())), Sense::drag());
        if grip.drag_started() {
            self.dock.start_drag(v, ui.ctx().viewport_id());
        }
        if grip.hovered() && self.dock.drag.is_none() && ui.ctx().dragged_id().is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    // ------------------------------------------------------------------
    // Leisten mit den Symbolen

    /// Linke Leiste: oben die Werkzeuge links und in eigenen Fenstern, darunter die Werkzeuge unten, ganz unten die Optionen
    pub(super) fn stripe_left(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let pal = style::pal();
        let options = Rect::from_min_max(pos2(full.left(), full.bottom() - TILE), full.max);
        let top = self.stripe_group(ui, Stripe::LeftTop, full.x_range(), full.top(), false);
        let bottom = self.stripe_group(ui, Stripe::LeftBottom, full.x_range(), options.top() - 14.0, true);
        // Trefferflaechen: oben bis zur Mitte zwischen den Gruppen, unten bis zu den Optionen
        let split = (top.1 + bottom.0) / 2.0;
        let x = full.left()..=full.right() + 16.0;
        self.dock.geo.left = full;
        self.dock.geo.groups.retain(|(s, ..)| *s == Stripe::Right);
        self.dock.geo.groups.push((Stripe::LeftTop, Rect::from_x_y_ranges(x.clone(), full.top() - 8.0..=split), top.2));
        self.dock.geo.groups.push((Stripe::LeftBottom, Rect::from_x_y_ranges(x, split..=options.top() - 4.0), bottom.2));
        // Optionen
        let open = self.tabs.get(self.active).is_some_and(|t| t.icon() == Some(Icon::Settings));
        let k = keymap::text(Cmd::Settings);
        let tip = if k.is_empty() { Cmd::Settings.label().to_string() } else { format!("{} ({k})", Cmd::Settings.label()) };
        let resp = ui.interact(options, egui::Id::new("stripe-options"), Sense::click());
        paint_tile(ui, options, &resp, Icon::Settings, pal.text_weak, crate::i18n::tr("Optionen", "Options"), open, false);
        if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            let ctx = ui.ctx().clone();
            self.run_cmd(&ctx, Cmd::Settings);
        }
    }

    /// Rechte Leiste (nur, wenn dort Werkzeuge stehen; gleitet herein und hinaus)
    pub(super) fn stripe_right(&mut self, ui: &mut egui::Ui) {
        let pal = style::pal();
        let has = !self.group(Stripe::Right).is_empty();
        let w = ui.ctx().animate_value_with_time(egui::Id::new("stripe-right-w"), if has { STRIPE_W } else { 0.0 }, 0.18);
        if w < 0.5 {
            self.dock.geo.right = None;
            self.dock.geo.groups.retain(|(s, ..)| *s != Stripe::Right);
            return;
        }
        egui::Panel::right("stripe-right").exact_size(w).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(pal.face).inner_margin(egui::Margin::symmetric(0, 8))).show(ui, |ui| {
            let full = ui.max_rect();
            // Beim Hinausgleiten haengt der Inhalt an der rechten Kante
            let x = egui::Rangef::new(full.right() - STRIPE_W, full.right());
            let g = self.stripe_group(ui, Stripe::Right, x, full.top(), false);
            self.dock.geo.right = Some(full);
            self.dock.geo.groups.retain(|(s, ..)| *s != Stripe::Right);
            self.dock.geo.groups.push((Stripe::Right, Rect::from_x_y_ranges((full.left() - 16.0)..=x.max, full.top() - 8.0..=full.bottom()), g.2));
        });
    }

    /// Eine Gruppe von Symbolen ab `anchor` (nach unten, oder mit `up` nach oben wachsend). Beim Ziehen oeffnet sich
    /// dort, wo das Werkzeug landen wuerde, eine Luecke; die anderen Symbole gleiten zur Seite.
    /// Rueckgabe: obere und untere Kante (ohne Luecke) und die Mitten der Symbole fuer die Trefferpruefung.
    fn stripe_group(&mut self, ui: &mut egui::Ui, stripe: Stripe, x: egui::Rangef, anchor: f32, up: bool) -> (f32, f32, Vec<f32>) {
        let pal = style::pal();
        let ctx = ui.ctx().clone();
        let dragged = self.dock.dragging();
        let all = self.group(stripe);
        // Stelle des gezogenen Werkzeugs, wenn es aus dieser Leiste kommt (an einem Rand losgelassen bleibt es dort)
        let own = all.iter().position(|v| Some(*v) == dragged);
        let tools: Vec<SideView> = all.into_iter().filter(|v| Some(*v) != dragged).collect();
        let gap = match (dragged, self.dock.target) {
            (Some(_), Target::Slot(s, k)) if s == stripe => Some(k.min(tools.len())),
            (Some(_), Target::Edge(p)) if p.stripe() == stripe => Some(own.unwrap_or(tools.len())),
            _ => None,
        };
        let n = tools.len() as f32;
        let slots = n + if gap.is_some() { 1.0 } else { 0.0 };
        // Obere Kante ohne und mit Luecke
        let (y0, y0_gap) = if up { (anchor - n * STEP + GAP, anchor - slots * STEP + GAP) } else { (anchor, anchor) };
        let centers: Vec<f32> = (0..tools.len()).map(|i| y0 + i as f32 * STEP + TILE / 2.0).collect();
        let tile_x = x.min + 5.0..=x.max - 5.0;
        for (i, v) in tools.iter().enumerate() {
            let slot = i + usize::from(gap.is_some_and(|k| i >= k));
            let id = egui::Id::new(("stripe-tile", v.name()));
            let y = ctx.animate_value_with_time(id.with("y"), y0_gap + slot as f32 * STEP, 0.16);
            let r = Rect::from_x_y_ranges(tile_x.clone(), y..=y + TILE);
            let resp = ui.interact(r, id, Sense::click_and_drag());
            let open = self.tool_open(*v);
            let floating = self.place_of(*v) == Place::Window;
            paint_tile(ui, r, &resp, tool_icon(*v), tool_hue(*v), tool_caption(*v), open, floating);
            let mut tip = match shortcut(*v) {
                Some(cmd) => match keymap::text(cmd) {
                    k if k.is_empty() => cmd.label().to_string(),
                    k => format!("{} ({k})", cmd.label()),
                },
                None => tool_title(*v).to_string(),
            };
            if floating {
                tip.push_str(&format!(" · {}", Place::Window.label()));
            }
            tip.push_str(crate::i18n::tr("\nZiehen: in eine Leiste, an einen Rand oder aus dem Fenster", "\nDrag into a bar, to an edge or out of the window"));
            let resp = if dragged.is_none() { resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
            if resp.drag_started() {
                self.dock.start_drag(*v, ctx.viewport_id());
            }
            if resp.clicked() {
                let was_open = self.tool_open(*v);
                self.toggle_tool(*v);
                if !was_open && *v == SideView::Search {
                    self.search.focus = true;
                }
            }
            egui::Popup::context_menu(&resp).show(|ui| self.tool_place_menu(ui, *v));
        }
        // Das gezogene Symbol bleibt als (unsichtbares) Element erhalten, damit das Ziehen nicht abreisst
        if let Some(v) = dragged
            && self.place_of(v).stripe() == stripe
        {
            let _ = ui.interact(Rect::from_x_y_ranges(tile_x.clone(), anchor..=anchor + 1.0), egui::Id::new(("stripe-tile", v.name())), Sense::click_and_drag());
        }
        // Luecke: gestrichelte Stelle, an der das Werkzeug landet
        let show = ctx.animate_bool_with_time(egui::Id::new(("stripe-gap", stripe as u8)), gap.is_some(), 0.14);
        if let Some(k) = gap {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(("stripe-gap-at", stripe as u8)), k));
        }
        if show > 0.0 {
            let k = gap.or_else(|| ctx.data(|d| d.get_temp::<usize>(egui::Id::new(("stripe-gap-at", stripe as u8))))).unwrap_or(0);
            let gy = ctx.animate_value_with_time(egui::Id::new(("stripe-gap-y", stripe as u8)), y0_gap + k as f32 * STEP, 0.16);
            let r = Rect::from_x_y_ranges(tile_x, gy..=gy + TILE).shrink(2.0 + 4.0 * (1.0 - show));
            let p = ui.painter();
            p.rect_filled(r, 8.0, pal.accent.gamma_multiply(0.12 * show));
            dashed_rect(p, r, Stroke::new(1.5, pal.accent.gamma_multiply(0.85 * show)));
            if let Some(v) = dragged {
                icons::paint(p, Rect::from_center_size(r.center(), vec2(18.0, 18.0)), tool_icon(v), pal.accent.gamma_multiply(show));
            }
        }
        (y0, y0 + n * STEP - GAP, centers)
    }

    // ------------------------------------------------------------------
    // Leisten mit den Werkzeugen

    /// Linke, rechte und untere Leiste (vor dem Editor anlegen). Jede gleitet beim Oeffnen herein und beim
    /// Schliessen hinaus; an der Kante gezogen aendert sich die Breite, ganz zugezogen schliesst sie.
    pub(super) fn dock_panels(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        // Steht ein Werkzeug inzwischen woanders, ist es hier nicht mehr offen
        if self.dock.right.is_some_and(|v| self.place_of(v) != Place::Right) {
            self.dock.right = None;
        }
        if self.dock.bottom.is_some_and(|v| self.place_of(v) != Place::Bottom) {
            self.dock.bottom = None;
        }
        let left = (self.settings.sidebar && self.place_of(self.side_view) == Place::Left).then_some(self.side_view);
        self.dock_panel(ui, Place::Right, self.dock.right, actions);
        self.dock_panel(ui, Place::Bottom, self.dock.bottom, actions);
        self.dock_panel(ui, Place::Left, left, actions);
    }

    fn dock_panel(&mut self, ui: &mut egui::Ui, place: Place, open: Option<SideView>, actions: &mut Vec<Action>) {
        let pal = style::pal();
        let w = ui.ctx().content_rect().width();
        // Zwei Leisten je Platz: Chats sind breiter und merken sich ihre eigene Breite
        for chat in [false, true] {
            let mut expanded = open.is_some_and(|v| is_chat(v) == chat);
            if let Some(v) = open.filter(|_| expanded) {
                self.dock.last.insert((place, chat), v);
            }
            let content = self.dock.last.get(&(place, chat)).copied();
            let (size, min, max) = match place {
                Place::Bottom => (280.0, 110.0, f32::INFINITY),
                _ if chat => (360.0, 250.0, (w * 0.7).max(250.0)),
                Place::Left => ((w * 0.22).clamp(200.0, 280.0), 170.0, (w * 0.36).max(250.0)),
                _ => ((w * 0.22).clamp(200.0, 300.0), 170.0, (w * 0.45).max(250.0)),
            };
            let id = match (place, chat) {
                (Place::Left, false) => "sidebar",
                (Place::Left, true) => "sidebar-chat",
                (Place::Right, false) => "dock-right",
                (Place::Right, true) => "dock-right-chat",
                (_, false) => "dock-bottom",
                (_, true) => "dock-bottom-chat",
            };
            let panel = match place {
                Place::Left => egui::Panel::left(id),
                Place::Right => egui::Panel::right(id),
                _ => egui::Panel::bottom(id),
            };
            let fill = if content == Some(SideView::Terminal) { pal.bg } else { pal.sidebar_bg };
            let panel = panel.resizable(true).show_separator_line(true).default_size(size).min_size(min).max_size(max).frame(egui::Frame::new().fill(fill));
            let was = expanded;
            panel.show_collapsible(ui, &mut expanded, |ui| {
                let Some(v) = content else { return };
                if v == SideView::Terminal {
                    self.tool_ui(ui, v, actions);
                } else {
                    let m = if place == Place::Bottom { egui::Margin::symmetric(8, 6) } else { egui::Margin { left: 4, right: 4, top: 2, bottom: 6 } };
                    style::panel_card(ui, pal.sidebar_bg, m, |ui| self.tool_ui(ui, v, actions));
                }
            });
            // Ganz zugezogen: ausblenden
            if was
                && !expanded
                && let Some(v) = open
            {
                self.hide_tool(v);
            }
        }
    }

    /// Werkzeuge in eigenen Fenstern (nach dem Hauptfenster zeichnen)
    pub(super) fn dock_windows(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let pal = style::pal();
        for v in self.dock.windows.clone() {
            if self.place_of(v) != Place::Window {
                self.dock.windows.retain(|w| *w != v);
                continue;
            }
            let id = egui::ViewportId::from_hash_of(("easymysql-tool", v.name()));
            let size = if v == SideView::Terminal { [760.0, 420.0] } else { [420.0, 680.0] };
            let mut builder = egui::ViewportBuilder::default().with_title(format!("{} – EasyMySQL", tool_title(v))).with_inner_size(size).with_min_inner_size([260.0, 180.0]);
            // Neues Fenster dort, wo das Werkzeug losgelassen wurde (Kopfzeile unter dem Zeiger)
            if let Some(p) = self.dock.window_at.remove(v.name()) {
                builder = builder.with_position(p - vec2(60.0, 16.0));
            }
            let mut closed = false;
            ctx.show_viewport_immediate(id, builder, |ui, _class| {
                self.dock.track(ui.ctx());
                let frame = egui::Frame::new().fill(if v == SideView::Terminal { pal.bg } else { pal.sidebar_bg }).inner_margin(egui::Margin::symmetric(4, 4));
                egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                    self.tool_ui(ui, v, actions);
                });
                closed = ui.ctx().input(|i| i.viewport().close_requested());
            });
            if closed {
                self.dock.windows.retain(|w| *w != v);
                self.remember_open();
            }
        }
    }

    /// Beim Ziehen: Ziel bestimmen, Vorschau der Flaeche und das mitgezogene Symbol zeichnen; beim Loslassen dorthin
    /// verschieben (ausserhalb des Hauptfensters: eigenes Fenster an dieser Stelle)
    pub(super) fn dock_drag_overlay(&mut self, ctx: &egui::Context) {
        self.dock.track(ctx);
        let window = ctx.content_rect();
        let pal = style::pal();
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-drag")));
        let mut target_rect = None;
        if let Some(d) = &self.dock.drag {
            let v = d.view;
            // Zeiger im Hauptfenster
            let origin = if d.screen { ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)) } else { Some(Pos2::ZERO) };
            let local = match (d.pointer, origin) {
                (Some(p), Some(o)) if !(d.gone && d.viewport == egui::ViewportId::ROOT) => Some(p - o.to_vec2()),
                _ => None,
            };
            // Ohne bekannte Lage (Wayland, Zeiger in einem anderen Fenster) laesst sich nichts zuordnen
            let target = if d.pointer.is_none() || (local.is_none() && !d.screen && d.viewport != egui::ViewportId::ROOT) {
                Target::Cancel
            } else {
                let t = target_at(&self.dock.geo, window, local);
                // Ein eigenes Fenster bleibt eines, wenn es ausserhalb losgelassen wird
                if t == Target::Float && self.place_of(v) == Place::Window && d.viewport != egui::ViewportId::ROOT { Target::Cancel } else { t }
            };
            if d.released {
                let screen = d.screen.then_some(d.pointer).flatten();
                self.dock.drag = None;
                self.dock.target = Target::Cancel;
                if target == Target::Float
                    && let Some(at) = screen
                {
                    self.dock.window_at.insert(v.name(), at);
                }
                self.drop_tool(v, target);
                ctx.request_repaint();
            } else {
                self.dock.target = target;
                target_rect = preview_rect(&self.dock.geo, window, target);
                // Unsichtbare rechte Leiste, solange rechts nichts steht
                let area = work_area(&self.dock.geo, window);
                if self.dock.geo.right.is_none() {
                    let hot = matches!(target, Target::Slot(Stripe::Right, _));
                    let a = ctx.animate_bool_with_time(egui::Id::new("ghost-right"), true, 0.18);
                    let r = ghost_right(window, area);
                    painter.rect_filled(r, 0.0, pal.face.gamma_multiply(0.92 * a));
                    painter.vline(r.left(), r.y_range(), Stroke::new(1.0, pal.border.gamma_multiply(a)));
                    let slot = Rect::from_min_size(pos2(r.left() + 5.0, r.top() + 8.0), vec2(STRIPE_W - 10.0, TILE)).shrink(2.0);
                    painter.rect_filled(slot, 8.0, pal.accent.gamma_multiply(if hot { 0.16 } else { 0.06 } * a));
                    dashed_rect(&painter, slot, Stroke::new(1.5, pal.accent.gamma_multiply(if hot { 0.9 } else { 0.4 } * a)));
                }
                // Mitgezogenes Symbol mit Namen am Zeiger; ausserhalb des Fensters ein Hinweis am Rand
                let appear = ctx.animate_bool_with_time(egui::Id::new("dock-ghost"), true, 0.14);
                let chip_at = match local {
                    Some(p) if target != Target::Float => Some((p + vec2(16.0, 14.0), tool_title(v).to_string(), tool_icon(v))),
                    _ if target == Target::Float => d.pointer.zip(origin).map(|(p, o)| {
                        let p = p - o.to_vec2();
                        (pos2(p.x.clamp(window.left() + 12.0, window.right() - 220.0), p.y.clamp(window.top() + 12.0, window.bottom() - 40.0)), crate::tr_format!("{} als eigenes Fenster", "{} as separate window", tool_title(v)), Icon::External)
                    }),
                    _ => None,
                };
                if let Some((at, text, icon)) = chip_at {
                    let font = style::medium(13.0);
                    let g = painter.layout_no_wrap(text, font, pal.text);
                    // Am rechten oder unteren Rand auf die andere Seite des Zeigers
                    let size = vec2(g.size().x + 40.0, 30.0);
                    let at = pos2(if at.x + size.x > window.right() - 6.0 { at.x - size.x - 28.0 } else { at.x }, at.y.min(window.bottom() - size.y - 6.0));
                    let r = Rect::from_min_size(at, size);
                    let r = Rect::from_center_size(r.center(), r.size() * egui::lerp(0.92..=1.0, appear));
                    let weak = target == Target::Cancel;
                    painter.add(egui::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha((if pal.dark { 120.0 } else { 40.0 } * appear) as u8) }.as_shape(r, 8));
                    painter.rect(r, 8.0, pal.face_light.gamma_multiply(appear), Stroke::new(1.0, pal.border.gamma_multiply(appear)), egui::StrokeKind::Inside);
                    let ic = if icon == Icon::External { pal.accent } else { tool_hue(v) };
                    icons::paint(&painter, Rect::from_center_size(pos2(r.left() + 17.0, r.center().y), vec2(16.0, 16.0)), icon, ic.gamma_multiply(appear * if weak { 0.6 } else { 1.0 }));
                    painter.galley(pos2(r.left() + 32.0, r.center().y - g.size().y / 2.0), g, if weak { pal.text_weak } else { pal.text }.gamma_multiply(appear));
                }
                ctx.set_cursor_icon(if target == Target::Cancel { egui::CursorIcon::NotAllowed } else { egui::CursorIcon::Grabbing });
                ctx.request_repaint();
            }
        } else {
            ctx.animate_bool_with_time(egui::Id::new("dock-ghost"), false, 0.0);
            ctx.animate_bool_with_time(egui::Id::new("ghost-right"), false, 0.0);
        }
        // Vorschau gleitet zur neuen Flaeche und blendet weich ein und aus
        let k = 1.0 - (-dt * 22.0).exp();
        match target_rect {
            Some(t) => {
                let cur = self.dock.preview.filter(|_| self.dock.preview_alpha > 0.01).unwrap_or_else(|| shrink_toward_edge(t, window));
                self.dock.preview = Some(Rect::from_min_max(cur.min + (t.min - cur.min) * k, cur.max + (t.max - cur.max) * k));
                self.dock.preview_alpha += (1.0 - self.dock.preview_alpha) * k;
            }
            None => self.dock.preview_alpha -= self.dock.preview_alpha * k,
        }
        if self.dock.preview_alpha < 0.03 {
            self.dock.preview_alpha = 0.0;
        } else if let Some(r) = self.dock.preview {
            let a = self.dock.preview_alpha;
            let r = r.shrink(4.0);
            painter.rect_filled(r, 8.0, pal.accent.gamma_multiply(if pal.dark { 0.20 } else { 0.10 } * a));
            painter.rect_stroke(r, 8.0, Stroke::new(2.0, pal.accent.gamma_multiply(0.9 * a)), egui::StrokeKind::Inside);
            if let Some(v) = self.dock.dragging().or(self.dock.previewed) {
                let c = r.center();
                icons::paint(&painter, Rect::from_center_size(c - vec2(0.0, 13.0), vec2(22.0, 22.0)), tool_icon(v), pal.accent.gamma_multiply(a));
                painter.text(c + vec2(0.0, 14.0), Align2::CENTER_CENTER, tool_title(v), style::medium(13.5), pal.accent.gamma_multiply(a));
            }
            if let Some(v) = self.dock.dragging() {
                self.dock.previewed = Some(v);
            }
            ctx.request_repaint();
        }
    }
}

/// Startflaeche der Vorschau: schmal an der Kante, an der sie haengt
fn shrink_toward_edge(r: Rect, window: Rect) -> Rect {
    if (r.bottom() - window.bottom()).abs() < 1.0 && r.width() > r.height() * 1.5 {
        Rect::from_min_max(pos2(r.left(), r.bottom() - r.height() * 0.4), r.max)
    } else if (r.left() - window.left()).abs() < r.width() {
        Rect::from_min_max(r.min, pos2(r.left() + r.width() * 0.4, r.bottom()))
    } else {
        Rect::from_min_max(pos2(r.right() - r.width() * 0.4, r.top()), r.max)
    }
}

/// Gestrichelter, abgerundeter Rahmen (Platzhalter in einer Leiste)
fn dashed_rect(p: &egui::Painter, r: Rect, s: Stroke) {
    let rad = 8.0;
    let mut pts = Vec::with_capacity(48);
    for (c, a0) in [(pos2(r.right() - rad, r.top() + rad), -90.0f32), (pos2(r.right() - rad, r.bottom() - rad), 0.0), (pos2(r.left() + rad, r.bottom() - rad), 90.0), (pos2(r.left() + rad, r.top() + rad), 180.0)] {
        for k in 0..=6 {
            let a = (a0 + k as f32 * 15.0).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * rad);
        }
    }
    pts.push(pts[0]);
    p.extend(egui::Shape::dashed_line(&pts, s, 4.0, 3.0));
}

/// Symbol in der Leiste: ruhend neutral, unter der Maus in seiner Farbe, offen auf einer Flaeche seiner Farbe
/// (dunkel: kraeftige Flaeche mit dunklem Symbol; hell: zarte Flaeche mit farbigem Symbol)
#[allow(clippy::too_many_arguments)]
fn paint_tile(ui: &egui::Ui, r: Rect, resp: &egui::Response, icon: Icon, hue: Color32, caption: &str, active: bool, floating: bool) {
    let pal = style::pal();
    let t = ui.ctx().animate_bool_with_time(resp.id.with("offen"), active, 0.2);
    let hot = ui.ctx().animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
    let down = resp.is_pointer_button_down_on() && !resp.dragged();
    let r = if down { Rect::from_center_size(r.center(), r.size() * 0.96) } else { r };
    let p = ui.painter();
    if hot > 0.0 && t < 1.0 {
        p.rect_filled(r, 8.0, pal.hover.gamma_multiply(hot * (1.0 - t)));
    }
    let (tile, on) = if pal.dark { (hue, pal.face) } else { (style::soft(hue).lerp_to_gamma(hue, 0.04), hue) };
    if t > 0.0 {
        let tr = Rect::from_center_size(r.center(), r.size() * egui::lerp(0.86..=1.0, t));
        p.rect_filled(tr, 8.0, tile.gamma_multiply(t));
    }
    let idle = if pal.dark { hue.lerp_to_gamma(pal.activity_fg, 0.3 * (1.0 - hot)) } else { pal.activity_fg.lerp_to_gamma(hue, hot) };
    let fg = idle.lerp_to_gamma(on, t);
    let ir = Rect::from_center_size(pos2(r.center().x, r.top() + 17.0), vec2(19.0, 19.0));
    icons::paint(p, ir, icon, fg);
    if floating {
        // Kennzeichen "eigenes Fenster" rechts oben am Symbol
        let b = Rect::from_center_size(ir.right_top() + vec2(3.0, 1.0), vec2(11.0, 11.0));
        icons::paint(p, b, Icon::External, fg);
    }
    let text_c = pal.text_weak.lerp_to_gamma(pal.text, hot).lerp_to_gamma(on, t);
    p.text(pos2(r.center().x, r.bottom() - 9.5), Align2::CENTER_CENTER, caption, style::medium(10.5), text_c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_roundtrip() {
        for p in Place::ALL {
            assert_eq!(Place::from_id(p.id()), Some(p));
        }
        assert_eq!(Place::from_id("x"), None);
        assert_eq!(default_place(SideView::Terminal), Place::Bottom);
        assert_eq!(default_place(SideView::Claude), Place::Left);
    }

    #[test]
    fn order_is_complete_and_reorders_within_a_stripe() {
        let o = parse_order("git,unbekannt,git,explorer");
        assert_eq!(o.len(), TOOLS.len());
        assert_eq!(&o[..3], &[SideView::Git, SideView::Explorer, SideView::Search]);
        // Terminal in der Gruppe [Explorer, Git] an Stelle 1 (zwischen beide)
        let order = parse_order("");
        let group = [SideView::Explorer, SideView::Git];
        let new = reorder(&order, &group, SideView::Terminal, 1);
        let pos = |v| new.iter().position(|t| *t == v).unwrap();
        assert!(pos(SideView::Explorer) < pos(SideView::Terminal) && pos(SideView::Terminal) < pos(SideView::Git));
        // Ans Ende einer Gruppe: direkt hinter ihr letztes Werkzeug
        let new = reorder(&order, &group, SideView::Terminal, 2);
        assert_eq!(new.iter().position(|t| *t == SideView::Terminal), Some(pos_of(&new, SideView::Git) + 1));
        // Leere Gruppe: ans Ende
        assert_eq!(reorder(&order, &[], SideView::Explorer, 0).last(), Some(&SideView::Explorer));
    }

    fn pos_of(o: &[SideView], v: SideView) -> usize {
        o.iter().position(|t| *t == v).unwrap()
    }

    #[test]
    fn targets_follow_the_pointer() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0));
        let left = Rect::from_min_max(pos2(0.0, 40.0), pos2(60.0, 800.0));
        let geo = Geo {
            left,
            right: None,
            groups: vec![
                (Stripe::LeftTop, Rect::from_min_max(pos2(0.0, 40.0), pos2(76.0, 500.0)), vec![70.0, 120.0, 170.0]),
                (Stripe::LeftBottom, Rect::from_min_max(pos2(0.0, 500.0), pos2(76.0, 740.0)), vec![700.0]),
            ],
        };
        let t = |x: f32, y: f32| target_at(&geo, window, Some(pos2(x, y)));
        assert_eq!(t(30.0, 50.0), Target::Slot(Stripe::LeftTop, 0));
        assert_eq!(t(30.0, 130.0), Target::Slot(Stripe::LeftTop, 2));
        assert_eq!(t(30.0, 720.0), Target::Slot(Stripe::LeftBottom, 1));
        assert_eq!(t(1180.0, 300.0), Target::Slot(Stripe::Right, 0));
        assert_eq!(t(150.0, 300.0), Target::Edge(Place::Left));
        assert_eq!(t(1050.0, 300.0), Target::Edge(Place::Right));
        assert_eq!(t(600.0, 750.0), Target::Edge(Place::Bottom));
        assert_eq!(t(600.0, 300.0), Target::Cancel);
        assert_eq!(target_at(&geo, window, Some(pos2(-10.0, 300.0))), Target::Float);
        assert_eq!(target_at(&geo, window, None), Target::Float);
        assert_eq!(preview_rect(&geo, window, Target::Edge(Place::Left)).map(|r| r.left()), Some(60.0));
        assert_eq!(preview_rect(&geo, window, Target::Cancel), None);
    }
}
