// Werkzeugfenster (Explorer, Suche, Datenbanken, Verlauf, Git, Claude, Codex, Terminal) stehen links, rechts,
// unten oder in einem eigenen Fenster. Verschoben wird wie in VS Code durch Ziehen: an der Kopfzeile des
// Werkzeugs oder an seinem Symbol in der Leiste. Beim Ziehen erscheinen die Andockzonen; ausserhalb des
// Hauptfensters losgelassen entsteht ein eigenes Fenster an dieser Stelle. Aus einem eigenen Fenster laesst sich
// das Werkzeug genauso zurueck ins Hauptfenster ziehen. Der Platz wird gemerkt (dock_<name>=<platz>).

use super::*;
use crate::icons::{self, Icon};
use eframe::egui::{Align2, Color32, FontId, Rect, Stroke, pos2, vec2};

/// Platz eines Werkzeugfensters
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
}

/// Voreinstellung: Terminal unten, alles andere links
pub fn default_place(v: SideView) -> Place {
    if v == SideView::Terminal { Place::Bottom } else { Place::Left }
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

/// Registerfarbe eines Werkzeugs (Kachel in der Registerleiste und Kopfband der Seitenleiste)
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

/// Kurze Beschriftung unter dem Symbol in der Registerleiste
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

/// Zielzone beim Ziehen eines Symbols: Rechtecke im Fenster und der Platz, der dort entsteht
fn zones(r: Rect) -> [(Place, Rect); 4] {
    let side = (r.width() * 0.22).clamp(120.0, 360.0);
    let bottom = (r.height() * 0.28).clamp(100.0, 320.0);
    [
        (Place::Left, Rect::from_min_max(r.min, pos2(r.left() + side, r.bottom()))),
        (Place::Right, Rect::from_min_max(pos2(r.right() - side, r.top()), r.max)),
        (Place::Bottom, Rect::from_min_max(pos2(r.left() + side, r.bottom() - bottom), pos2(r.right() - side, r.bottom()))),
        (Place::Window, Rect::from_min_max(pos2(r.left() + side, r.top()), pos2(r.right() - side, r.bottom() - bottom))),
    ]
}

/// Platz unter dem Zeiger (ausserhalb des Fensters: eigenes Fenster)
pub fn zone_at(r: Rect, p: egui::Pos2) -> Place {
    if !r.contains(p) {
        return Place::Window;
    }
    zones(r).into_iter().find(|(_, z)| z.contains(p)).map_or(Place::Window, |(p, _)| p)
}

/// Offene Werkzeugfenster ausser links (links: `side_view` und `settings.sidebar`)
#[derive(Default)]
pub struct Dock {
    pub right: Option<SideView>,
    pub bottom: Option<SideView>,
    pub windows: Vec<SideView>,
    /// Gerade gezogenes Werkzeug, das Fenster, in dem gezogen wird, und der Zeiger in Bildschirmkoordinaten
    pub drag: Option<SideView>,
    drag_viewport: Option<egui::ViewportId>,
    pointer: Option<egui::Pos2>,
    /// Losgelassen (Bildschirmkoordinaten); ausgewertet vom Hauptfenster
    dropped: Option<egui::Pos2>,
    /// Wo ein neues eigenes Fenster erscheinen soll
    window_at: std::collections::HashMap<&'static str, egui::Pos2>,
}

impl Dock {
    /// Ziehen beginnen (im Fenster `viewport`)
    pub fn start_drag(&mut self, v: SideView, viewport: egui::ViewportId) {
        self.drag = Some(v);
        self.drag_viewport = Some(viewport);
        self.pointer = None;
        self.dropped = None;
    }

    /// Zeiger und Loslassen in dem Fenster verfolgen, in dem gezogen wird
    pub fn track(&mut self, ctx: &egui::Context) {
        if self.drag.is_none() || self.drag_viewport != Some(ctx.viewport_id()) {
            return;
        }
        let (pos, down, origin) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.any_down(), i.viewport().inner_rect.map(|r| r.min)));
        if let (Some(p), Some(o)) = (pos, origin) {
            self.pointer = Some(p + o.to_vec2());
        }
        if !down {
            self.dropped = self.pointer.or(Some(egui::Pos2::ZERO));
        }
        ctx.request_repaint();
    }
}

/// Platz laut Einstellungen
fn place_in(settings: &crate::settings::Settings, v: SideView) -> Place {
    settings.dock.get(v.name()).and_then(|p| Place::from_id(p)).unwrap_or_else(|| default_place(v))
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
        self.show_tool(v);
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

    /// Inhalt eines Werkzeugfensters (gleich, wo es steht). Die Kopfzeile ist der Griff zum Verschieben: Knoepfe
    /// darin liegen darueber und bleiben klickbar, auf der freien Flaeche beginnt das Ziehen.
    pub(super) fn tool_ui(&mut self, ui: &mut egui::Ui, v: SideView, actions: &mut Vec<Action>) {
        let head = Rect::from_min_size(ui.max_rect().min, vec2(ui.max_rect().width(), 30.0));
        let grip = ui.interact(head, ui.id().with(("tool-grip", v.name())), egui::Sense::drag());
        if grip.drag_started() {
            self.dock.start_drag(v, ui.ctx().viewport_id());
        }
        if grip.hovered() && self.dock.drag.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        egui::Popup::context_menu(&grip).show(|ui| self.tool_place_menu(ui, v));
        // Kopfband in der Registerfarbe: verbindet die Seitenleiste mit ihrer Kachel in der Registerleiste
        if v != SideView::Terminal {
            let outer = ui.clip_rect();
            let band = Rect::from_min_max(outer.min, egui::pos2(outer.right(), outer.top() + 36.0));
            let hue = tool_hue(v);
            ui.painter().rect_filled(band, 0.0, crate::style::soft(hue));
            ui.painter().rect_filled(Rect::from_min_max(band.min, egui::pos2(band.right(), band.top() + 3.0)), 0.0, hue);
        }
        if v == SideView::Terminal {
            self.terminal_view(ui);
        } else {
            self.side_panel_for(ui, v, actions);
        }
    }

    /// Rechte und untere Leiste (vor dem Editor anlegen) samt Werkzeugen in eigenen Fenstern
    pub(super) fn dock_panels(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ctx = ui.ctx().clone();
        let pal = style::pal();
        let flat = |fill: Color32| egui::Frame::new().fill(fill);
        let w = ctx.content_rect().width();
        // Steht ein Werkzeug inzwischen woanders, ist es hier nicht mehr offen
        if self.dock.right.is_some_and(|v| self.place_of(v) != Place::Right) {
            self.dock.right = None;
        }
        if self.dock.bottom.is_some_and(|v| self.place_of(v) != Place::Bottom) {
            self.dock.bottom = None;
        }
        if let Some(v) = self.dock.right {
            let chat = matches!(v, SideView::Claude | SideView::Codex);
            egui::Panel::right(if chat { "dock-right-chat" } else { "dock-right" })
                .resizable(true)
                .show_separator_line(true)
                .default_size(if chat { 360.0 } else { (w * 0.22).clamp(200.0, 300.0) })
                .min_size(if chat { 250.0 } else { 170.0 })
                .max_size((w * if chat { 0.7 } else { 0.45 }).max(250.0))
                .frame(flat(pal.sidebar_bg))
                .show(ui, |ui| {
                    style::panel_card(ui, pal.sidebar_bg, egui::Margin { left: 4, right: 4, top: 2, bottom: 6 }, |ui| {
                        self.tool_ui(ui, v, actions);
                    });
                });
        }
        match self.dock.bottom {
            Some(SideView::Terminal) => self.terminal_panel(ui),
            Some(v) => {
                egui::Panel::bottom(egui::Id::new(("dock-bottom", v.name())))
                    .resizable(true)
                    .show_separator_line(true)
                    .default_size(280.0)
                    .min_size(110.0)
                    .frame(flat(pal.sidebar_bg))
                    .show(ui, |ui| {
                        style::panel_card(ui, pal.sidebar_bg, egui::Margin::symmetric(8, 6), |ui| {
                            self.tool_ui(ui, v, actions);
                        });
                    });
            }
            None => {}
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

    /// Beim Ziehen die Andockzonen im Hauptfenster zeigen; beim Loslassen dorthin verschieben (ausserhalb des
    /// Hauptfensters: eigenes Fenster an dieser Stelle)
    pub(super) fn dock_drag_overlay(&mut self, ctx: &egui::Context) {
        let Some(v) = self.dock.drag else { return };
        self.dock.track(ctx);
        let r = ctx.content_rect();
        let origin = ctx.input(|i| i.viewport().inner_rect.map(|r| r.min)).unwrap_or(egui::Pos2::ZERO);
        // Zeiger relativ zum Hauptfenster
        let local = |p: egui::Pos2| p - origin.to_vec2();
        if let Some(at) = self.dock.dropped.take() {
            self.dock.drag = None;
            self.dock.drag_viewport = None;
            let to = zone_at(r, local(at));
            if to == Place::Window && at != egui::Pos2::ZERO {
                self.dock.window_at.insert(v.name(), at);
            }
            if to != self.place_of(v) {
                self.move_tool(v, to);
            }
            return;
        }
        let target = self.dock.pointer.map(|p| zone_at(r, local(p)));
        let pal = style::pal();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-zones")));
        painter.rect_filled(r, 0.0, Color32::from_black_alpha(if pal.dark { 70 } else { 30 }));
        for (place, z) in zones(r) {
            let z = z.shrink(10.0);
            let hot = target == Some(place);
            let fill = if hot { pal.accent.gamma_multiply(0.30) } else { pal.face.gamma_multiply(0.85) };
            painter.rect(z, style::RADIUS as f32 + 2.0, fill, Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { pal.accent } else { pal.border }), egui::StrokeKind::Inside);
            let ir = Rect::from_center_size(z.center() - vec2(0.0, 14.0), vec2(22.0, 22.0));
            icons::paint(&painter, ir, place.icon(), if hot { pal.accent } else { pal.text_weak });
            painter.text(z.center() + vec2(0.0, 12.0), Align2::CENTER_CENTER, place.label(), FontId::proportional(14.0), if hot { pal.text } else { pal.text_weak });
        }
        if let Some(p) = self.dock.pointer.map(local).filter(|p| r.contains(*p)) {
            let ir = Rect::from_center_size(p + vec2(14.0, 14.0), vec2(20.0, 20.0));
            painter.rect_filled(ir.expand(4.0), style::RADIUS as f32, pal.face_light);
            icons::paint(&painter, ir, tool_icon(v), icons::tint(tool_icon(v)));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_roundtrip_and_zones() {
        for p in Place::ALL {
            assert_eq!(Place::from_id(p.id()), Some(p));
        }
        assert_eq!(Place::from_id("x"), None);
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0));
        assert_eq!(zone_at(r, pos2(20.0, 400.0)), Place::Left);
        assert_eq!(zone_at(r, pos2(980.0, 100.0)), Place::Right);
        assert_eq!(zone_at(r, pos2(500.0, 780.0)), Place::Bottom);
        assert_eq!(zone_at(r, pos2(500.0, 300.0)), Place::Window);
        assert_eq!(zone_at(r, pos2(-5.0, 300.0)), Place::Window);
        assert_eq!(default_place(SideView::Terminal), Place::Bottom);
        assert_eq!(default_place(SideView::Claude), Place::Left);
    }
}
