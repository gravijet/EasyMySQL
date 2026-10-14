// Abfrage-Assistent: beliebige SELECT-Abfragen grafisch zusammenstellen – mit Verknuepfungen,
// Unterabfragen, WITH, UNION/EXCEPT/INTERSECT, Gruppierung, Fensterfunktionen usw.

mod dml;
pub mod model;

use super::{Action, Ctx, TabView};
use crate::db::{self, ResultSet, Schema};
use crate::grid::{self, GridState};
use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, RichText};
use dml::{Dml, Mode};
use model::*;
use std::sync::mpsc::{Receiver, channel};

/// Was im Assistenten sichtbar ist: Tabellen, CTEs und Spalten (auch aus aeusseren Abfragen)
#[derive(Clone)]
struct Env<'a> {
    schema: Option<&'a Schema>,
    tables: Vec<String>,
    /// (Name, Spalten) der sichtbaren WITH-Abfragen
    ctes: Vec<(String, Vec<String>)>,
    /// (Bezeichner, Spalten) der Tabellen dieser und aeusserer Abfragen
    scope: Vec<(String, Vec<String>)>,
    depth: usize,
}

impl Env<'_> {
    fn first_column(&self) -> Expr {
        self.scope
            .iter()
            .find_map(|(s, cols)| cols.first().map(|c| Expr::column(s, c)))
            .unwrap_or(Expr::Value("1".into()))
    }

    fn source_cols(&self, src: &Source) -> Vec<String> {
        match &src.from {
            From::Table(t) => self.schema.and_then(|s| s.table(t)).map(|t| t.columns.iter().map(|c| c.name.clone()).collect()).unwrap_or_default(),
            From::Cte(n) => self.ctes.iter().find(|c| c.0 == *n).map(|c| c.1.clone()).unwrap_or_default(),
            From::Sub(qr) => self.output_cols(qr),
        }
    }

    /// Spaltennamen des Ergebnisses einer Abfrage
    fn output_cols(&self, qr: &Query) -> Vec<String> {
        let env = self.with_ctes(qr);
        let s = &qr.parts[0].select;
        if s.items.is_empty() {
            return s.sources.iter().flat_map(|src| env.source_cols(src)).collect();
        }
        let mut out = Vec::new();
        for it in &s.items {
            if !it.alias.trim().is_empty() {
                out.push(it.alias.trim().to_string());
                continue;
            }
            match &it.expr {
                Expr::Column { src, col } if col == "*" => {
                    if let Some(so) = s.sources.iter().find(|x| x.reference() == *src) {
                        out.extend(env.source_cols(so));
                    }
                }
                Expr::All => out.extend(s.sources.iter().flat_map(|src| env.source_cols(src))),
                e => out.push(e.label()),
            }
        }
        out
    }

    /// Umgebung innerhalb einer Abfrage (mit ihren WITH-Definitionen)
    fn with_ctes(&self, qr: &Query) -> Env<'_> {
        let mut env = self.clone();
        for c in &qr.with {
            if c.name.trim().is_empty() {
                continue;
            }
            let cols = if !c.columns.trim().is_empty() {
                c.columns.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
            } else if c.query.with.iter().any(|x| x.name == c.name) || depth_guard(&env) {
                Vec::new()
            } else {
                let mut e2 = env.clone();
                e2.depth += 1;
                // rekursive CTE verweist auf sich selbst: erste Teilabfrage genuegt
                let mut first = c.query.clone();
                first.parts.truncate(1);
                e2.output_cols(&first)
            };
            env.ctes.retain(|x| x.0 != c.name);
            env.ctes.push((c.name.trim().to_string(), cols));
        }
        env
    }

    /// Umgebung innerhalb eines SELECT (Spalten der eigenen Tabellen zuerst)
    fn inside(&self, s: &Select) -> Env<'_> {
        let mut env = self.clone();
        let mut own: Vec<(String, Vec<String>)> = s.sources.iter().map(|src| (src.reference(), self.source_cols(src))).collect();
        own.append(&mut env.scope);
        env.scope = own;
        env.depth += 1;
        env
    }
}

fn depth_guard(env: &Env) -> bool {
    env.depth > 12
}

/// Rahmen fuer eine verschachtelte Abfrage (farbiger Rand links)
fn nested_frame<R>(ui: &mut egui::Ui, depth: usize, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let colors = [Color32::from_rgb(0x3B, 0x8E, 0xD0), Color32::from_rgb(0xC0, 0x7A, 0x2E), Color32::from_rgb(0x5A, 0xA8, 0x5A), Color32::from_rgb(0xA0, 0x60, 0xC0)];
    let c = colors[depth % colors.len()];
    let r = egui::Frame::new()
        .inner_margin(egui::Margin { left: 10, right: 6, top: 4, bottom: 4 })
        .fill(style::pal().face_light.gamma_multiply(0.6))
        .show(ui, |ui| ui.vertical(add).inner);
    let rect = r.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, c);
    r.inner
}

fn keyword(ui: &mut egui::Ui, kw: &str) {
    ui.label(RichText::new(kw).monospace().strong().color(style::pal().syn_keyword));
}

fn remove_button(ui: &mut egui::Ui) -> bool {
    icons::button_sized(ui, Icon::Close, crate::i18n::text("Entfernen"), 18.0).clicked()
}

/// Abschnitt mit Ueberschrift (einklappbar)
fn section(ui: &mut egui::Ui, id: egui::Id, kw: &str, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    section_ex(ui, id, kw, title, open, "", false, add);
}

/// Abschnitt als Karte: Schluesselwort als Marke, Titel, bei zugeklapptem Abschnitt eine Kurzfassung des Inhalts.
/// `clearable`: Papierkorb in der Kopfzeile; gibt true zurueck, wenn er geklickt wurde.
#[allow(clippy::too_many_arguments)]
/// Karte für die Bereiche unten (SQL, Ergebnis): eigener Rahmen, füllt den freien Platz
fn pane_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let pal = style::pal();
    egui::Frame::new()
        .fill(pal.bg)
        .stroke(egui::Stroke::new(1.0, pal.border))
        .corner_radius(style::RADIUS + 1)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui);
        });
}

fn section_ex(ui: &mut egui::Ui, id: egui::Id, kw: &str, title: &str, open: bool, summary: &str, clearable: bool, add: impl FnOnce(&mut egui::Ui)) -> bool {
    use egui::{FontId, Sense, Stroke, pos2, vec2};
    let ctx = ui.ctx().clone();
    let pal = style::pal();
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, open);
    let mut cleared = false;
    egui::Frame::new()
        .fill(pal.face_light.gamma_multiply(if pal.dark { 0.7 } else { 0.9 }))
        .stroke(Stroke::new(1.0, pal.border.gamma_multiply(0.55)))
        .corner_radius(style::RADIUS as f32 + 2.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            let painter = ui.painter();
            if resp.hovered() {
                painter.rect_filled(rect, style::RADIUS as f32, pal.hover);
            }
            let is_open = state.is_open();
            let chevron = egui::Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(14.0, 14.0));
            icons::paint(painter, chevron, if is_open { Icon::ChevronDown } else { Icon::ChevronRight }, pal.text_weak);
            let mut x = rect.left() + 28.0;
            if !kw.is_empty() {
                let g = painter.layout_no_wrap(kw.to_string(), FontId::monospace(12.5), pal.syn_keyword);
                let pill = egui::Rect::from_min_size(pos2(x, rect.center().y - 11.0), vec2(g.size().x + 14.0, 22.0));
                painter.rect_filled(pill, 6.0, pal.syn_keyword.gamma_multiply(if pal.dark { 0.16 } else { 0.12 }));
                painter.galley(pos2(pill.left() + 7.0, rect.center().y - g.size().y / 2.0), g, pal.syn_keyword);
                x = pill.right() + 10.0;
            }
            let g = painter.layout_no_wrap(title.to_string(), FontId::proportional(12.5), pal.text_weak);
            painter.galley(pos2(x, rect.center().y - g.size().y / 2.0), g.clone(), pal.text_weak);
            x += g.size().x + 14.0;
            // Kurzfassung, solange der Abschnitt zu ist (offen steht der Inhalt ja da)
            let right = rect.right() - if clearable { 30.0 } else { 8.0 };
            if !is_open && !summary.is_empty() && right - x > 40.0 {
                let mut text = summary.to_string();
                loop {
                    let g = painter.layout_no_wrap(text.clone(), FontId::proportional(12.5), pal.text);
                    if g.size().x <= right - x || text.chars().count() <= 3 {
                        painter.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, pal.text.gamma_multiply(0.85));
                        break;
                    }
                    text = text.chars().take(text.chars().count() - 2).collect::<String>().trim_end().to_string() + "…";
                    if text.ends_with("……") {
                        text.pop();
                    }
                }
            }
            let mut toggle = resp.clicked();
            if clearable {
                let r = egui::Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(22.0, 22.0));
                let clear = ui.interact(r, id.with("clear"), Sense::click());
                if resp.hovered() || clear.hovered() {
                    if clear.hovered() {
                        ui.painter().rect_filled(r, 5.0, pal.hover.gamma_multiply(1.6));
                    }
                    icons::paint(ui.painter(), r.shrink(4.0), Icon::Trash, if clear.hovered() { pal.text } else { pal.text_weak });
                }
                let clear = clear.on_hover_text(crate::i18n::text("Leeren"));
                if clear.clicked() {
                    cleared = true;
                    toggle = false;
                }
            }
            if toggle {
                state.toggle(ui);
            }
            state.show_body_unindented(ui, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width() - 4.0);
                        add(ui);
                    });
                });
                ui.add_space(4.0);
            });
        });
    state.store(&ctx);
    cleared
}

/// Anzahl der einzelnen Bedingungen (ohne leere Gruppen)
fn cond_count(c: &Cond) -> usize {
    match c {
        Cond::Group { items, .. } => items.iter().map(cond_count).sum(),
        Cond::Raw(s) => !s.trim().is_empty() as usize,
        _ => 1,
    }
}

/// Aufzaehlung fuer die Kurzfassung eines zugeklappten Abschnitts
fn list_summary(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

// ---------------------------------------------------------------------------
// Ausdruecke

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Column,
    Value,
    Func,
    Op,
    Case,
    Cast,
    Sub,
    Raw,
    All,
}

const KINDS: [(Kind, &str); 9] = [
    (Kind::Column, "Spalte"),
    (Kind::Value, "Wert"),
    (Kind::Func, "Funktion"),
    (Kind::Op, "Rechnung"),
    (Kind::Case, "CASE"),
    (Kind::Cast, "CAST"),
    (Kind::Sub, "Unterabfrage"),
    (Kind::Raw, "SQL"),
    (Kind::All, "*"),
];

fn kind_of(e: &Expr) -> Kind {
    match e {
        Expr::Column { .. } => Kind::Column,
        Expr::All => Kind::All,
        Expr::Value(_) => Kind::Value,
        Expr::Func { .. } => Kind::Func,
        Expr::Op { .. } => Kind::Op,
        Expr::Case { .. } => Kind::Case,
        Expr::Cast { .. } => Kind::Cast,
        Expr::Sub(_) => Kind::Sub,
        Expr::Raw(_) => Kind::Raw,
    }
}

fn new_query(env: &Env) -> Query {
    let mut qr = Query::default();
    if let Some(t) = env.tables.first() {
        qr.parts[0].select.sources.push(Source::table(t));
    }
    qr
}

fn convert(e: &Expr, k: Kind, env: &Env) -> Expr {
    let cur = e.clone();
    match k {
        Kind::Column => env.first_column(),
        Kind::Value => Expr::Value(String::new()),
        Kind::Func => match cur {
            // aus einer Spalte wird COUNT(spalte), die Funktion laesst sich dann aendern
            Expr::Column { .. } => Expr::Func { name: "COUNT".into(), args: vec![cur], distinct: false, over: None },
            _ => Expr::func("COUNT", None),
        },
        Kind::Op => Expr::Op { op: "+".into(), left: Box::new(cur), right: Box::new(Expr::Value("1".into())) },
        Kind::Case => Expr::Case {
            whens: vec![(Cond::Cmp { left: cur, op: CmpOp::Eq, right: vec![Expr::Value(String::new())], quant: Quant::None, sub: None }, Expr::Value(String::new()))],
            otherwise: Some(Box::new(Expr::Value(String::new()))),
        },
        Kind::Cast => Expr::Cast { expr: Box::new(cur), ty: "CHAR".into() },
        Kind::Sub => Expr::Sub(Box::new(new_query(env))),
        Kind::Raw => Expr::Raw(cur.sql()),
        Kind::All => Expr::All,
    }
}

fn column_picker(ui: &mut egui::Ui, id: egui::Id, src: &mut String, col: &mut String, env: &Env, stars: bool) {
    let text = if src.is_empty() { col.clone() } else { format!("{src}.{col}") };
    crate::widgets::combo(id).selected_text(RichText::new(text).monospace()).width(150.0).height(400.0).show_ui(ui, |ui| {
        for (s, cols) in &env.scope {
            ui.label(RichText::new(s).small().strong().color(style::pal().text_weak));
            if stars && ui.selectable_label(src == s && col == "*", format!("  {s}.*")).clicked() {
                *src = s.clone();
                *col = "*".into();
            }
            for c in cols {
                if ui.selectable_label(src == s && col == c, format!("  {c}")).clicked() {
                    *src = s.clone();
                    *col = c.clone();
                }
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(crate::widgets::edit(col).desired_width(90.0));
            if ui.button(crate::i18n::text("ohne Tabelle")).clicked() {
                src.clear();
            }
        });
    });
}

fn func_picker(ui: &mut egui::Ui, name: &mut String) -> bool {
    let mut changed = false;
    ui.menu_button(RichText::new(name.to_ascii_uppercase()).monospace().color(style::pal().syn_function), |ui| {
        let mut groups: Vec<&str> = FUNCS.iter().map(|f| f.group).collect();
        groups.dedup();
        for g in groups {
            ui.menu_button(crate::i18n::text(g), |ui| {
                for fd in FUNCS.iter().filter(|f| f.group == g) {
                    if ui.button(fd.name).on_hover_text(crate::i18n::text(fd.hint)).clicked() {
                        *name = fd.name.to_string();
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(crate::i18n::text("andere:"));
            if ui.add(crate::widgets::edit(name).desired_width(110.0)).changed() {
                changed = true;
            }
        });
    });
    changed
}

fn expr_ui(ui: &mut egui::Ui, e: &mut Expr, env: &Env, id: egui::Id) {
    ui.horizontal_wrapped(|ui| {
        let k = kind_of(e);
        let mut nk = k;
        crate::widgets::combo(id.with("kind"))
            .selected_text(RichText::new(KINDS.iter().find(|x| x.0 == k).map(|x| crate::i18n::text(x.1)).unwrap_or("")).small())
            .width(78.0)
            .show_ui(ui, |ui| {
                for (kk, label) in KINDS {
                    ui.selectable_value(&mut nk, kk, crate::i18n::text(label));
                }
            });
        if nk != k {
            *e = convert(e, nk, env);
        }
        match e {
            Expr::Column { src, col } => column_picker(ui, id.with("col"), src, col, env, true),
            Expr::All => {
                ui.label(RichText::new("*").monospace());
            }
            Expr::Value(v) => {
                ui.add(crate::widgets::edit(v).desired_width(100.0).hint_text(crate::i18n::text("Wert")));
            }
            Expr::Raw(r) => {
                ui.add(crate::widgets::edit(r).desired_width(180.0).font(egui::TextStyle::Monospace).hint_text("SQL"));
            }
            Expr::Func { name, args, distinct, over } => {
                if func_picker(ui, name) {
                    let first = args.first().cloned().filter(|a| !matches!(a, Expr::All));
                    let fresh = Expr::func(name, first.or_else(|| Some(env.first_column())));
                    if let Expr::Func { args: a, over: o, .. } = fresh {
                        *args = a;
                        *over = o.or(over.take().filter(|_| func_def(name).is_some_and(|d| d.window)));
                    }
                }
                ui.label("(");
                if func_def(name).is_some_and(|d| d.aggregate) {
                    crate::widgets::check(ui, distinct, RichText::new("DISTINCT").small());
                }
                let mut remove = None;
                for (i, a) in args.iter_mut().enumerate() {
                    if i > 0 {
                        ui.label(",");
                    }
                    ui.push_id(i, |ui| {
                        expr_ui(ui, a, env, id.with(("arg", i)));
                        if icons::button_sized(ui, Icon::Close, crate::i18n::text("Argument entfernen"), 14.0).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    args.remove(i);
                }
                if icons::button_sized(ui, Icon::Plus, crate::i18n::text("Argument hinzufügen"), 16.0).clicked() {
                    args.push(env.first_column());
                }
                ui.label(")");
                if func_def(name).is_none_or(|d| d.window || d.aggregate) {
                    let mut on = over.is_some();
                    if crate::widgets::check(ui, &mut on, RichText::new("OVER").small()).changed() {
                        *over = on.then(Window::default);
                    }
                }
                if let Some(w) = over {
                    window_ui(ui, w, env, id.with("over"));
                }
            }
            Expr::Op { op, left, right } => {
                ui.label("(");
                expr_ui(ui, left, env, id.with("l"));
                crate::widgets::combo(id.with("op")).selected_text(RichText::new(op.as_str()).monospace()).width(46.0).show_ui(ui, |ui| {
                    for o in OPS {
                        ui.selectable_value(op, o.to_string(), *o);
                    }
                });
                expr_ui(ui, right, env, id.with("r"));
                ui.label(")");
            }
            Expr::Cast { expr, ty } => {
                ui.label("(");
                expr_ui(ui, expr, env, id.with("c"));
                keyword(ui, "AS");
                let items: Vec<String> = TYPES.iter().map(|s| s.to_string()).collect();
                super::str_combo(ui, id.with("ty"), &items, ty, 110.0);
                ui.label(")");
            }
            Expr::Case { whens, otherwise } => {
                ui.vertical(|ui| {
                    let mut remove = None;
                    for (i, (c, v)) in whens.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                keyword(ui, "WHEN");
                                cond_item_ui(ui, c, env, id.with(("w", i)));
                                keyword(ui, "THEN");
                                expr_ui(ui, v, env, id.with(("t", i)));
                                if whens_len_gt1(i) && remove_button(ui) {
                                    remove = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = remove {
                        whens.remove(i);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("+ WHEN").clicked() {
                            whens.push((Cond::Cmp { left: env.first_column(), op: CmpOp::Eq, right: vec![Expr::Value(String::new())], quant: Quant::None, sub: None }, Expr::Value(String::new())));
                        }
                        let mut has_else = otherwise.is_some();
                        if crate::widgets::check(ui, &mut has_else, "ELSE").changed() {
                            *otherwise = has_else.then(|| Box::new(Expr::Value(String::new())));
                        }
                        if let Some(o) = otherwise {
                            expr_ui(ui, o, env, id.with("else"));
                        }
                        keyword(ui, "END");
                    });
                });
            }
            Expr::Sub(qr) => {
                ui.vertical(|ui| {
                    nested_frame(ui, env.depth, |ui| query_ui(ui, qr, env, id.with("sub")));
                });
            }
        }
    });
}

/// Nur die erste WHEN-Zeile bleibt ohne Entfernen-Knopf
fn whens_len_gt1(i: usize) -> bool {
    i > 0
}

fn window_ui(ui: &mut egui::Ui, w: &mut Window, env: &Env, id: egui::Id) {
    ui.vertical(|ui| {
        nested_frame(ui, env.depth + 1, |ui| {
            ui.horizontal_wrapped(|ui| {
                keyword(ui, "PARTITION BY");
                expr_list_ui(ui, &mut w.partition, env, id.with("p"));
            });
            ui.horizontal_wrapped(|ui| {
                keyword(ui, "ORDER BY");
                order_list_ui(ui, &mut w.order, env, id.with("o"));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::text("Rahmen")).small());
                ui.add(crate::widgets::edit(&mut w.frame).desired_width(260.0).hint_text(crate::i18n::text("z. B. ROWS BETWEEN 2 PRECEDING AND CURRENT ROW")));
            });
        });
    });
}

fn expr_list_ui(ui: &mut egui::Ui, list: &mut Vec<Expr>, env: &Env, id: egui::Id) {
    let mut remove = None;
    for (i, e) in list.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            expr_ui(ui, e, env, id.with(i));
            if remove_button(ui) {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if icons::button_sized(ui, Icon::Plus, crate::i18n::text("Hinzufügen"), 18.0).clicked() {
        list.push(env.first_column());
    }
}

fn order_list_ui(ui: &mut egui::Ui, list: &mut Vec<OrderItem>, env: &Env, id: egui::Id) {
    let mut remove = None;
    for (i, o) in list.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            expr_ui(ui, &mut o.expr, env, id.with(i));
            crate::widgets::combo(id.with(("dir", i))).selected_text(if o.desc { crate::i18n::text("absteigend") } else { crate::i18n::text("aufsteigend") }).width(92.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut o.desc, false, crate::i18n::text("aufsteigend (ASC)"));
                ui.selectable_value(&mut o.desc, true, crate::i18n::text("absteigend (DESC)"));
            });
            if remove_button(ui) {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if icons::button_sized(ui, Icon::Plus, crate::i18n::text("Sortierung hinzufügen"), 18.0).clicked() {
        list.push(OrderItem { expr: env.first_column(), desc: false });
    }
}

// ---------------------------------------------------------------------------
// Bedingungen

fn new_cmp(env: &Env) -> Cond {
    Cond::Cmp { left: env.first_column(), op: CmpOp::Eq, right: vec![Expr::Value(String::new())], quant: Quant::None, sub: None }
}

/// Eine Bedingung (Vergleich, EXISTS, SQL oder Gruppe)
fn cond_item_ui(ui: &mut egui::Ui, c: &mut Cond, env: &Env, id: egui::Id) {
    match c {
        Cond::Group { .. } => {
            ui.vertical(|ui| cond_group_ui(ui, c, env, id));
        }
        Cond::Raw(r) => {
            ui.add(crate::widgets::edit(r).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text(crate::i18n::text("Bedingung in SQL")));
        }
        Cond::Exists { not, query } => {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    crate::widgets::combo(id.with("ex")).selected_text(if *not { "NOT EXISTS" } else { "EXISTS" }).width(100.0).show_ui(ui, |ui| {
                        ui.selectable_value(not, false, "EXISTS");
                        ui.selectable_value(not, true, "NOT EXISTS");
                    });
                });
                nested_frame(ui, env.depth, |ui| query_ui(ui, query, env, id.with("exq")));
            });
        }
        Cond::Cmp { left, op, right, quant, sub } => {
            ui.horizontal_wrapped(|ui| {
                expr_ui(ui, left, env, id.with("left"));
                let before = *op;
                crate::widgets::combo(id.with("op")).selected_text(RichText::new(op.sql()).monospace()).width(96.0).height(420.0).show_ui(ui, |ui| {
                    for o in CmpOp::ALL {
                        ui.selectable_value(op, o, o.sql());
                    }
                });
                if *op != before {
                    if !op.allows_sub() {
                        *sub = None;
                    }
                    if let Some(n) = op.arity() {
                        right.resize_with(n, || Expr::Value(String::new()));
                    } else if right.is_empty() {
                        right.push(Expr::Value(String::new()));
                    }
                }
                if op.allows_sub() {
                    let use_sub = sub.is_some();
                    if ui
                        .add(egui::Button::new(RichText::new(crate::i18n::text("Unterabfrage")).small()).selected(use_sub))
                        .clicked()
                    {
                        *sub = (!use_sub).then(|| Box::new(new_query(env)));
                    }
                }
                match (op.arity(), sub.is_some()) {
                    (_, true) => {
                        if !matches!(op, CmpOp::In | CmpOp::NotIn) {
                            crate::widgets::combo(id.with("q"))
                                .selected_text(match quant {
                                    Quant::None => "–",
                                    Quant::Any => "ANY",
                                    Quant::All => "ALL",
                                })
                                .width(50.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(quant, Quant::None, crate::i18n::text("– (ein Wert)"));
                                    ui.selectable_value(quant, Quant::Any, crate::i18n::text("ANY (mindestens einer)"));
                                    ui.selectable_value(quant, Quant::All, crate::i18n::text("ALL (alle)"));
                                });
                        }
                    }
                    (Some(0), _) => {}
                    (Some(2), _) => {
                        right.resize_with(2, || Expr::Value(String::new()));
                        expr_ui(ui, &mut right[0], env, id.with("r0"));
                        keyword(ui, "AND");
                        expr_ui(ui, &mut right[1], env, id.with("r1"));
                    }
                    (Some(_), _) => {
                        if right.is_empty() {
                            right.push(Expr::Value(String::new()));
                        }
                        expr_ui(ui, &mut right[0], env, id.with("r0"));
                    }
                    (None, _) => {
                        ui.label("(");
                        let mut remove = None;
                        for (i, r) in right.iter_mut().enumerate() {
                            ui.push_id(i, |ui| {
                                expr_ui(ui, r, env, id.with(("in", i)));
                                if icons::button_sized(ui, Icon::Close, "", 14.0).clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            right.remove(i);
                        }
                        if icons::button_sized(ui, Icon::Plus, crate::i18n::text("Wert hinzufügen"), 16.0).clicked() {
                            right.push(Expr::Value(String::new()));
                        }
                        ui.label(")");
                    }
                }
            });
            if let Some(sq) = sub {
                nested_frame(ui, env.depth, |ui| query_ui(ui, sq, env, id.with("cmpsub")));
            }
        }
    }
}

fn cond_group_ui(ui: &mut egui::Ui, c: &mut Cond, env: &Env, id: egui::Id) {
    let Cond::Group { any, not, items } = c else { return };
    if items.len() > 1 || *not {
        ui.horizontal(|ui| {
            crate::widgets::combo(id.with("any")).selected_text(if *any { crate::i18n::text("ODER – eine muss gelten") } else { crate::i18n::text("UND – alle müssen gelten") }).width(190.0).show_ui(ui, |ui| {
                ui.selectable_value(any, false, crate::i18n::text("UND – alle müssen gelten"));
                ui.selectable_value(any, true, crate::i18n::text("ODER – eine muss gelten"));
            });
            crate::widgets::check(ui, not, "NICHT");
        });
    }
    let mut remove = None;
    for (i, it) in items.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                if i > 0 {
                    ui.label(RichText::new(if *any { "OR" } else { "AND" }).monospace().small().color(style::pal().syn_keyword));
                }
                if matches!(it, Cond::Group { .. }) {
                    nested_frame(ui, env.depth + 1, |ui| cond_item_ui(ui, it, env, id.with(i)));
                } else {
                    cond_item_ui(ui, it, env, id.with(i));
                }
                if remove_button(ui) {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        items.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::text("+ Bedingung")).clicked() {
            items.push(new_cmp(env));
        }
        if ui.button(crate::i18n::text("+ Gruppe (…)")).clicked() {
            items.push(Cond::Group { any: !*any, not: false, items: vec![new_cmp(env)] });
        }
        if ui.button("+ EXISTS").clicked() {
            items.push(Cond::Exists { not: false, query: Box::new(new_query(env)) });
        }
        if ui.button("+ SQL").clicked() {
            items.push(Cond::Raw(String::new()));
        }
    });
}

// ---------------------------------------------------------------------------
// SELECT und Abfrage

/// Verknuepfungen ueber Fremdschluessel zu den schon verwendeten Tabellen
fn fk_joins(s: &Select, env: &Env) -> Vec<(String, Source)> {
    let Some(schema) = env.schema else { return Vec::new() };
    let mut out = Vec::new();
    for fk in &schema.fks {
        for src in &s.sources {
            let From::Table(t) = &src.from else { continue };
            let r = src.reference();
            let (other, cond_pairs, label) = if *t == fk.table {
                (fk.ref_table.clone(), fk.columns.iter().zip(&fk.ref_columns).map(|(a, b)| ((r.clone(), a.clone()), (fk.ref_table.clone(), b.clone()))).collect::<Vec<_>>(), format!("{} → {}", t, fk.ref_table))
            } else if *t == fk.ref_table {
                (fk.table.clone(), fk.ref_columns.iter().zip(&fk.columns).map(|(a, b)| ((r.clone(), a.clone()), (fk.table.clone(), b.clone()))).collect::<Vec<_>>(), format!("{} ← {}", t, fk.table))
            } else {
                continue;
            };
            // Name fuer die neue Tabelle (Alias, falls schon verwendet)
            let used: Vec<String> = s.sources.iter().map(|x| x.reference()).collect();
            let mut alias = String::new();
            if used.contains(&other) {
                let mut k = 2;
                while used.contains(&format!("{other}{k}")) {
                    k += 1;
                }
                alias = format!("{other}{k}");
            }
            let refname = if alias.is_empty() { other.clone() } else { alias.clone() };
            let items: Vec<Cond> = cond_pairs
                .into_iter()
                .map(|((sa, ca), (_, cb))| Cond::Cmp { left: Expr::column(&refname, &cb), op: CmpOp::Eq, right: vec![Expr::column(&sa, &ca)], quant: Quant::None, sub: None })
                .collect();
            let mut src_new = Source::table(&other);
            src_new.alias = alias;
            src_new.on = Cond::Group { any: false, not: false, items };
            let cols: Vec<String> = fk.columns.clone();
            out.push((format!("{label}  ({})", cols.join(", ")), src_new));
        }
    }
    out
}

fn sources_ui(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    let inner_env = env.inside(s);
    let mut remove = None;
    let n = s.sources.len();
    for (i, src) in s.sources.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal_wrapped(|ui| {
                if i == 0 {
                    keyword(ui, "FROM");
                } else {
                    crate::widgets::combo(id.with(("join", i))).selected_text(RichText::new(src.join.sql()).monospace().color(style::pal().syn_keyword)).width(150.0).show_ui(ui, |ui| {
                        for j in JoinKind::ALL {
                            ui.selectable_value(&mut src.join, j, j.sql()).on_hover_text(j.hint());
                        }
                    });
                }
                // Art der Quelle
                let kind = match &src.from {
                    From::Table(_) => 0,
                    From::Cte(_) => 1,
                    From::Sub(_) => 2,
                };
                let mut nk = kind;
                crate::widgets::combo(id.with(("kind", i)))
                    .selected_text([crate::i18n::text("Tabelle"), "WITH", crate::i18n::text("Unterabfrage")][kind])
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut nk, 0, crate::i18n::text("Tabelle"));
                        if !env.ctes.is_empty() {
                            ui.selectable_value(&mut nk, 1, crate::i18n::text("WITH-Abfrage"));
                        }
                        ui.selectable_value(&mut nk, 2, crate::i18n::text("Unterabfrage"));
                    });
                if nk != kind {
                    src.from = match nk {
                        0 => From::Table(env.tables.first().cloned().unwrap_or_default()),
                        1 => From::Cte(env.ctes.first().map(|c| c.0.clone()).unwrap_or_default()),
                        _ => {
                            if src.alias.is_empty() {
                                src.alias = format!("u{}", i + 1);
                            }
                            From::Sub(Box::new(new_query(env)))
                        }
                    };
                }
                match &mut src.from {
                    From::Table(t) => {
                        super::str_combo(ui, id.with(("t", i)), &env.tables, t, 150.0);
                    }
                    From::Cte(c) => {
                        let names: Vec<String> = env.ctes.iter().map(|x| x.0.clone()).collect();
                        super::str_combo(ui, id.with(("c", i)), &names, c, 150.0);
                    }
                    From::Sub(_) => {}
                }
                keyword(ui, "AS");
                ui.add(crate::widgets::edit(&mut src.alias).desired_width(70.0).hint_text("Alias"));
                if n > 1 && remove_button(ui) {
                    remove = Some(i);
                }
            });
            if let From::Sub(qr) = &mut src.from {
                nested_frame(ui, env.depth, |ui| query_ui(ui, qr, env, id.with(("sub", i))));
            }
            if i > 0 && src.join.has_condition() {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        if src.using.trim().is_empty() {
                            ui.horizontal(|ui| {
                                keyword(ui, "ON");
                                if src.on.is_empty() && ui.button("USING …").clicked() {
                                    src.using = "id".into();
                                }
                            });
                            cond_group_ui(ui, &mut src.on, &inner_env, id.with(("on", i)));
                        } else {
                            ui.horizontal(|ui| {
                                keyword(ui, "USING");
                                ui.add(crate::widgets::edit(&mut src.using).desired_width(160.0).hint_text(crate::i18n::text("spalte1, spalte2")));
                                if ui.button("ON").clicked() {
                                    src.using.clear();
                                }
                            });
                        }
                    });
                });
            }
        });
    }
    if let Some(i) = remove {
        s.sources.remove(i);
    }
    ui.horizontal(|ui| {
        let joins = fk_joins(s, env);
        ui.menu_button(crate::i18n::text("+ Tabelle"), |ui| {
            if !joins.is_empty() {
                ui.label(RichText::new(crate::i18n::text("Über Beziehungen")).small().color(style::pal().text_weak));
                for (label, src) in &joins {
                    if ui.button(label).clicked() {
                        s.sources.push(src.clone());
                        ui.close();
                    }
                }
                ui.separator();
            }
            ui.menu_button(crate::i18n::text("Tabelle"), |ui| {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    for t in &env.tables {
                        if ui.button(t).clicked() {
                            let mut src = Source::table(t);
                            if s.sources.iter().any(|x| x.reference() == *t) {
                                src.alias = format!("{t}2");
                            }
                            if s.sources.is_empty() {
                                s.sources.push(src);
                            } else {
                                src.join = JoinKind::Inner;
                                s.sources.push(src);
                            }
                            ui.close();
                        }
                    }
                });
            });
            if !env.ctes.is_empty() {
                ui.menu_button(crate::i18n::text("WITH-Abfrage"), |ui| {
                    for (c, _) in &env.ctes {
                        if ui.button(c).clicked() {
                            s.sources.push(Source { from: From::Cte(c.clone()), ..Source::table(c) });
                            ui.close();
                        }
                    }
                });
            }
            if ui.button(crate::i18n::text("Unterabfrage")).clicked() {
                s.sources.push(Source { from: From::Sub(Box::new(new_query(env))), alias: format!("u{}", s.sources.len() + 1), ..Source::table("") });
                ui.close();
            }
        });
    });
}

/// "+ Spalte": Spalten aller Tabellen der Abfrage ankreuzen (mehrere auf einmal), mit Suchfeld.
/// Das Menue bleibt offen, bis man daneben klickt.
fn column_menu(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    use egui::containers::menu::{MenuButton, MenuConfig};
    let cfg = MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    MenuButton::from_button(egui::Button::new(crate::i18n::text("+ Spalte"))).config(cfg).ui(ui, |ui| {
        ui.set_min_width(250.0);
        let mut filter: String = ui.data_mut(|d| d.get_temp(id.with("filter")).unwrap_or_default());
        let r = ui.add(crate::widgets::edit(&mut filter).hint_text(crate::i18n::text("Suchen …")).desired_width(250.0));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id.with("filter"), filter.clone()));
        }
        let needle = filter.trim().to_lowercase();
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (src, cols) in env.scope.iter().take(s.sources.len()) {
                ui.label(RichText::new(src).small().strong().color(style::pal().text_weak));
                let mut entries: Vec<&str> = vec!["*"];
                entries.extend(cols.iter().map(|c| c.as_str()));
                for c in entries {
                    if !needle.is_empty() && !c.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let pos = s.items.iter().position(|i| matches!(&i.expr, Expr::Column { src: a, col: b } if a == src && b == c));
                    let mut on = pos.is_some();
                    let label = if c == "*" { crate::i18n::text("alle Spalten (*)").to_string() } else { c.to_string() };
                    if crate::widgets::check(ui, &mut on, label).changed() {
                        match pos {
                            Some(i) if !on => {
                                s.items.remove(i);
                            }
                            None if on => s.items.push(Item { expr: Expr::column(src, c), alias: String::new() }),
                            _ => {}
                        }
                    }
                }
                ui.add_space(4.0);
            }
        });
    });
}

fn items_ui(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    ui.horizontal(|ui| {
        crate::widgets::check(ui, &mut s.distinct, "DISTINCT");
        if s.items.is_empty() {
            ui.label(RichText::new(crate::i18n::text("alle Spalten (*)")).color(style::pal().text_weak));
        }
    });
    let mut remove = None;
    let mut swap = None;
    let n = s.items.len();
    for (i, it) in s.items.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(i > 0, |ui| {
                    if icons::button_sized(ui, Icon::Up, crate::i18n::text("nach oben"), 16.0).clicked() {
                        swap = Some((i - 1, i));
                    }
                });
                ui.add_enabled_ui(i + 1 < n, |ui| {
                    if icons::button_sized(ui, Icon::Down, crate::i18n::text("nach unten"), 16.0).clicked() {
                        swap = Some((i, i + 1));
                    }
                });
                expr_ui(ui, &mut it.expr, env, id.with(("item", i)));
                keyword(ui, "AS");
                ui.add(crate::widgets::edit(&mut it.alias).desired_width(90.0).hint_text("Name"));
                if remove_button(ui) {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some((a, b)) = swap {
        s.items.swap(a, b);
    }
    if let Some(i) = remove {
        s.items.remove(i);
    }
    ui.horizontal(|ui| {
        column_menu(ui, s, env, id.with("cols"));
        ui.menu_button(crate::i18n::text("+ Funktion"), |ui| {
            let mut groups: Vec<&str> = FUNCS.iter().map(|f| f.group).collect();
            groups.dedup();
            for g in groups {
                ui.menu_button(crate::i18n::text(g), |ui| {
                    for fd in FUNCS.iter().filter(|f| f.group == g) {
                        if ui.button(fd.name).on_hover_text(crate::i18n::text(fd.hint)).clicked() {
                            s.items.push(Item { expr: Expr::func(fd.name, Some(env.first_column())), alias: fd.name.to_lowercase() });
                            ui.close();
                        }
                    }
                });
            }
        });
        if ui.button(crate::i18n::text("+ Ausdruck")).clicked() {
            s.items.push(Item { expr: Expr::Op { op: "*".into(), left: Box::new(env.first_column()), right: Box::new(Expr::Value("1".into())) }, alias: String::new() });
        }
        if ui.button("+ CASE").clicked() {
            s.items.push(Item { expr: convert(&env.first_column(), Kind::Case, env), alias: String::new() });
        }
        if ui.button(crate::i18n::text("+ Unterabfrage")).clicked() {
            s.items.push(Item { expr: Expr::Sub(Box::new(new_query(env))), alias: String::new() });
        }
        if !s.items.is_empty() && ui.button(crate::i18n::text("Alle entfernen")).clicked() {
            s.items.clear();
        }
    });
}

fn select_ui(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    let inner = env.inside(s);
    let from_sum = list_summary(s.sources.iter().map(|x| x.reference()));
    section_ex(ui, id.with("from"), "FROM", crate::i18n::text("Tabellen"), true, &from_sum, false, |ui| sources_ui(ui, s, env, id.with("src")));
    let sel_sum = if s.items.is_empty() { "*".to_string() } else { list_summary(s.items.iter().map(|i| if i.alias.trim().is_empty() { i.expr.label() } else { i.alias.trim().to_string() })) };
    if section_ex(ui, id.with("sel"), "SELECT", crate::i18n::text("Spalten"), true, &sel_sum, !s.items.is_empty(), |ui| items_ui(ui, s, &inner, id.with("items"))) {
        s.items.clear();
    }
    let n_where = cond_count(&s.filter);
    let where_sum = if n_where == 0 { String::new() } else { crate::tr_format!("{} Bedingung(en)", "{} condition(s)", n_where) };
    if section_ex(ui, id.with("where"), "WHERE", crate::i18n::text("Bedingungen"), true, &where_sum, n_where > 0, |ui| cond_group_ui(ui, &mut s.filter, &inner, id.with("filter"))) {
        s.filter = Cond::group();
    }
    let group_open = !s.group.is_empty() || !s.having.is_empty() || s.items.iter().any(|i| i.expr.is_aggregate());
    let group_sum = list_summary(s.group.iter().map(|e| e.label()));
    let has_group = !s.group.is_empty() || !s.having.is_empty();
    if section_ex(ui, id.with("group"), "GROUP BY", crate::i18n::text("Gruppieren"), group_open, &group_sum, has_group, |ui| {
        ui.horizontal_wrapped(|ui| {
            expr_list_ui(ui, &mut s.group, &inner, id.with("g"));
            crate::widgets::check(ui, &mut s.rollup, "WITH ROLLUP");
        });
        let needed = s.needed_group();
        if !needed.is_empty() && needed.iter().any(|e| !s.group.contains(e)) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::text("Spalten ohne Zusammenfassung fehlen in GROUP BY.")).small().color(style::pal().syn_function));
                if ui.button(crate::i18n::text("Übernehmen")).clicked() {
                    for e in needed {
                        if !s.group.contains(&e) {
                            s.group.push(e);
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            keyword(ui, "HAVING");
            ui.label(RichText::new(crate::i18n::text("Bedingungen für Gruppen")).small().color(style::pal().text_weak));
        });
        cond_group_ui(ui, &mut s.having, &inner, id.with("having"));
    }) {
        s.group.clear();
        s.rollup = false;
        s.having = Cond::group();
    }
    let mut order_env = inner.clone();
    let aliases: Vec<String> = s.items.iter().map(|i| i.alias.trim().to_string()).filter(|a| !a.is_empty()).collect();
    if !aliases.is_empty() {
        order_env.scope.insert(0, (String::new(), aliases));
    }
    let order_sum = list_summary(s.order.iter().map(|o| format!("{} {}", o.expr.label(), if o.desc { "↓" } else { "↑" })));
    if section_ex(ui, id.with("order"), "ORDER BY", crate::i18n::text("Sortieren"), !s.order.is_empty(), &order_sum, !s.order.is_empty(), |ui| {
        ui.horizontal_wrapped(|ui| order_list_ui(ui, &mut s.order, &order_env, id.with("ord")));
    }) {
        s.order.clear();
    }
    let limit_sum = if s.limit.trim().is_empty() { String::new() } else if s.offset.trim().is_empty() { s.limit.trim().to_string() } else { format!("{} OFFSET {}", s.limit.trim(), s.offset.trim()) };
    let has_limit = !s.limit.trim().is_empty() || !s.offset.trim().is_empty();
    if section_ex(ui, id.with("limit"), "LIMIT", crate::i18n::text("Begrenzen"), has_limit, &limit_sum, has_limit, |ui| {
        ui.horizontal(|ui| {
            ui.add(crate::widgets::edit(&mut s.limit).desired_width(60.0).hint_text(crate::i18n::text("Zeilen")));
            keyword(ui, "OFFSET");
            ui.add(crate::widgets::edit(&mut s.offset).desired_width(60.0).hint_text("0"));
        });
    }) {
        s.limit.clear();
        s.offset.clear();
    }
}

fn query_ui(ui: &mut egui::Ui, qr: &mut Query, env: &Env, id: egui::Id) {
    if env.depth > 10 {
        ui.label(crate::i18n::text("Zu tief verschachtelt."));
        return;
    }
    // WITH
    if !qr.with.is_empty() {
        ui.horizontal(|ui| {
            keyword(ui, "WITH");
            crate::widgets::check(ui, &mut qr.recursive, "RECURSIVE");
        });
        let mut remove = None;
        for i in 0..qr.with.len() {
            // jede CTE sieht die vorherigen (bei RECURSIVE auch sich selbst)
            let mut partial = qr.clone();
            partial.with.truncate(if qr.recursive { i + 1 } else { i });
            let cenv = env.with_ctes(&partial);
            let cte = &mut qr.with[i];
            ui.push_id(("cte", i), |ui| {
                ui.horizontal(|ui| {
                    ui.add(crate::widgets::edit(&mut cte.name).desired_width(120.0).hint_text("Name"));
                    ui.add(crate::widgets::edit(&mut cte.columns).desired_width(120.0).hint_text(crate::i18n::text("(Spalten)")));
                    keyword(ui, "AS");
                    if remove_button(ui) {
                        remove = Some(i);
                    }
                });
                let mut e2 = cenv.clone();
                e2.depth += 1;
                nested_frame(ui, env.depth, |ui| query_ui(ui, &mut cte.query, &e2, id.with(("cteq", i))));
            });
        }
        if let Some(i) = remove {
            qr.with.remove(i);
        }
    }
    let env = env.with_ctes(qr);
    let mut remove = None;
    let n = qr.parts.len();
    for (i, part) in qr.parts.iter_mut().enumerate() {
        ui.push_id(("part", i), |ui| {
            if i > 0 {
                ui.horizontal(|ui| {
                    crate::widgets::combo(id.with(("setop", i))).selected_text(RichText::new(part.op.sql()).monospace().strong().color(style::pal().syn_keyword)).width(130.0).show_ui(ui, |ui| {
                        for op in SetOp::ALL {
                            ui.selectable_value(&mut part.op, op, op.sql()).on_hover_text(op.hint());
                        }
                    });
                    ui.label(RichText::new(part.op.hint()).small().color(style::pal().text_weak));
                    if remove_button(ui) {
                        remove = Some(i);
                    }
                });
            }
            if n > 1 {
                nested_frame(ui, env.depth + i, |ui| select_ui(ui, &mut part.select, &env, id.with(("sel", i))));
            } else {
                select_ui(ui, &mut part.select, &env, id.with(("sel", i)));
            }
        });
    }
    if let Some(i) = remove {
        qr.parts.remove(i);
    }
    ui.horizontal(|ui| {
        ui.menu_button("+ UNION / EXCEPT / INTERSECT", |ui| {
            for op in SetOp::ALL {
                if ui.button(op.sql()).on_hover_text(op.hint()).clicked() {
                    // zweiter Teil mit gleicher Spaltenzahl beginnen
                    let mut sel = qr.parts[0].select.clone();
                    sel.order.clear();
                    sel.limit.clear();
                    qr.parts.push(Part { op, select: sel });
                    ui.close();
                }
            }
        });
        if ui.button("+ WITH").clicked() {
            let k = qr.with.len() + 1;
            qr.with.push(Cte { name: format!("hilf{k}"), columns: String::new(), query: new_query(&env) });
        }
    });
    if qr.parts.len() > 1 {
        let cols = env.output_cols(&Query { parts: vec![qr.parts[0].clone()], ..Default::default() });
        let mut oenv = env.clone();
        oenv.scope = vec![(String::new(), cols)];
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(crate::i18n::text("Gesamtergebnis")).small().color(style::pal().text_weak));
            keyword(ui, "ORDER BY");
            order_list_ui(ui, &mut qr.order, &oenv, id.with("qord"));
            keyword(ui, "LIMIT");
            ui.add(crate::widgets::edit(&mut qr.limit).desired_width(50.0));
            keyword(ui, "OFFSET");
            ui.add(crate::widgets::edit(&mut qr.offset).desired_width(50.0));
        });
    }
}

// ---------------------------------------------------------------------------
// Registerkarte

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    db: String,
    query: Query,
    #[serde(default)]
    mode: Mode,
    #[serde(default)]
    dml: Dml,
}

/// Stand der Eingaben fuer Rueckgaengig/Wiederholen
type Snap = (Query, Mode, Dml);

fn saved_path(id: &str) -> std::path::PathBuf {
    crate::workspace::internal_dir().join("assistent").join(format!("{id}.json"))
}

type Job = Receiver<Result<ResultSet, String>>;

/// Laufende Ausfuehrung: Start, SQL (fuer den Verlauf)
struct Running {
    start: std::time::Instant,
    sql: String,
}

pub struct BuilderTab {
    id: String,
    pub db: String,
    query: Query,
    schema: Option<std::sync::Arc<Schema>>,
    loaded_for: String,
    result: Option<ResultSet>,
    error: Option<String>,
    grid: GridState,
    job: Option<Job>,
    saved_json: String,
    last_save: std::time::Instant,
    view_name: Option<String>,
    initialized: bool,
    running: Option<Running>,
    /// Start der laufenden Abfrage und Dauer der letzten
    started: Option<std::time::Instant>,
    took: Option<std::time::Duration>,
    pretty_for: Option<Snap>,
    pretty_sql: String,
    layout_cache: crate::sqledit::SqlLayoutCache,
    mode: Mode,
    dml: Dml,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// Letzter Stand und Zeitpunkt der letzten Sicherung fuer Rueckgaengig
    last_snap: Snap,
    snap_at: std::time::Instant,
    /// Ergebnis waehrend der Eingabe automatisch zeigen (nur Abfragen, auf wenige Zeilen begrenzt)
    live: bool,
    live_done: Option<Snap>,
    live_changed: std::time::Instant,
}

impl BuilderTab {
    pub fn new(db: Option<String>) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("{}-{}-{n}", crate::server::timestamp(), std::process::id() % 1000);
        Self {
            id,
            db: db.unwrap_or_default(),
            query: Query::default(),
            schema: None,
            loaded_for: "\u{0}".into(),
            result: None,
            error: None,
            grid: GridState::default(),
            job: None,
            saved_json: String::new(),
            last_save: std::time::Instant::now(),
            view_name: None,
            initialized: false,
            running: None,
            started: None,
            took: None,
            pretty_for: None,
            pretty_sql: String::new(),
            layout_cache: crate::sqledit::SqlLayoutCache::default(),
            mode: Mode::Select,
            dml: Dml::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_snap: (Query::default(), Mode::Select, Dml::default()),
            snap_at: std::time::Instant::now(),
            live: false,
            live_done: None,
            live_changed: std::time::Instant::now(),
        }
    }

    /// Gespeicherten Assistenten aus der letzten Sitzung laden
    pub fn restore(id: &str, db: &str) -> Self {
        let mut t = Self::new(Some(db.to_string()));
        t.id = id.to_string();
        if let Some(s) = std::fs::read_to_string(saved_path(id)).ok().and_then(|j| serde_json::from_str::<Saved>(&j).ok()) {
            t.db = s.db;
            t.query = s.query;
            t.mode = s.mode;
            t.dml = s.dml;
            t.loaded_for = t.db.clone();
            t.initialized = true;
        }
        t.saved_json = serde_json::to_string(&Saved { db: t.db.clone(), query: t.query.clone(), mode: t.mode, dml: t.dml.clone() }).unwrap_or_default();
        t.last_snap = t.snapshot();
        t
    }

    fn snapshot(&self) -> Snap {
        (self.query.clone(), self.mode, self.dml.clone())
    }

    fn restore_snap(&mut self, s: Snap) {
        (self.query, self.mode, self.dml) = s;
    }

    /// Einzeiliges SQL der aktuellen Eingabe (je nach Betriebsart)
    fn current_sql(&self) -> String {
        match self.mode {
            Mode::Select => self.query.sql(),
            m => self.dml.sql(m, &self.query),
        }
    }

    /// Aenderungen merken (zusammengehoerende Eingaben innerhalb einer knappen Zeit gelten als ein Schritt)
    fn track_changes(&mut self) {
        let now = self.snapshot();
        if now != self.last_snap {
            if self.snap_at.elapsed().as_millis() > 700 {
                self.undo.push(self.last_snap.clone());
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
                self.snap_at = std::time::Instant::now();
            }
            self.redo.clear();
            self.last_snap = now;
            self.live_changed = std::time::Instant::now();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore_snap(prev);
            self.last_snap = self.snapshot();
            self.snap_at = std::time::Instant::now();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore_snap(next);
            self.last_snap = self.snapshot();
            self.snap_at = std::time::Instant::now();
        }
    }

    fn save(&mut self) {
        let j = serde_json::to_string(&Saved { db: self.db.clone(), query: self.query.clone(), mode: self.mode, dml: self.dml.clone() }).unwrap_or_default();
        if j != self.saved_json {
            let _ = crate::workspace::atomic_write(&saved_path(&self.id), &j);
            self.saved_json = j;
        }
        self.last_save = std::time::Instant::now();
    }

    /// Ausfuehren: Abfragen laufen sofort, Aenderungen der Daten erst nach Rueckfrage
    fn run(&mut self, cx: &mut Ctx) {
        if self.mode == Mode::Select {
            let sql = self.query.sql();
            let history = self.query.pretty();
            self.run_sql(cx, sql, Some(history));
            return;
        }
        let sql = self.dml.sql(self.mode, &self.query);
        if sql.is_empty() {
            self.error = Some(crate::i18n::text("Es fehlen noch Angaben, z. B. die Spalten oder Werte.").into());
            return;
        }
        self.error = None;
        cx.actions.push(Action::Confirm { text: self.dml.question(self.mode), db: Some(self.db.clone()), sql });
    }

    /// Ausfuehrungsplan der Abfrage
    fn explain(&mut self, cx: &mut Ctx) {
        let sql = format!("EXPLAIN {}", self.query.sql());
        let history = format!("EXPLAIN {}", self.query.pretty());
        self.run_sql(cx, sql, Some(history));
    }

    /// Anzeige waehrend der Eingabe: hoechstens 200 Zeilen, nicht im Verlauf
    fn preview(&mut self, cx: &mut Ctx) {
        let mut q = self.query.clone();
        if q.limit.trim().is_empty() && q.parts.len() > 1 || q.parts.len() == 1 && q.parts[0].select.limit.trim().is_empty() {
            if q.parts.len() > 1 {
                q.limit = "200".into();
            } else {
                q.parts[0].select.limit = "200".into();
            }
        }
        self.live_done = Some(self.snapshot());
        self.run_sql(cx, q.sql(), None);
    }

    fn run_sql(&mut self, cx: &mut Ctx, sql: String, history: Option<String>) {
        if self.job.is_some() {
            return;
        }
        let Some(dbc) = cx.db else { return };
        let conn = match dbc.new_conn() {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let db = self.db.clone();
        let info = dbc.info.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            use mysql::prelude::*;
            let mut conn = conn;
            let res = (|| {
                if !db.is_empty() {
                    conn.query_drop(format!("USE {}", db::q(&db))).map_err(db::err_text)?;
                }
                let id = conn.connection_id();
                let mut out = db::run_script_with(&mut conn, &sql, Some(&|| db::kill_query(&info, id)));
                if let Some(e) = out.error.take() {
                    return Err(e);
                }
                Ok(out.sets.into_iter().find(|s| s.has_table()).unwrap_or_default())
            })();
            let _ = tx.send(res);
        });
        self.job = Some(rx);
        self.started = Some(std::time::Instant::now());
        self.running = history.map(|sql| Running { start: std::time::Instant::now(), sql });
    }

    fn poll(&mut self, cx: &mut Ctx) {
        let Some(rx) = &self.job else { return };
        let Ok(r) = rx.try_recv() else { return };
        self.job = None;
        self.took = self.started.take().map(|t| t.elapsed());
        self.grid.reset();
        if let Some(run) = self.running.take() {
            let entry = crate::qhistory::Entry {
                time: crate::server::timestamp(),
                db: self.db.clone(),
                millis: run.start.elapsed().as_millis() as u64,
                error: r.is_err(),
                sql: run.sql,
            };
            cx.actions.push(Action::History(vec![entry]));
        }
        match r {
            Ok(rs) => {
                self.result = Some(rs);
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.result = None;
            }
        }
    }
}

impl TabView for BuilderTab {
    fn database(&self) -> Option<String> {
        Some(self.db.clone()).filter(|d| !d.is_empty())
    }

    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Wizard)
    }

    fn title(&self) -> String {
        if self.db.is_empty() { crate::i18n::text("Abfrage-Assistent").into() } else { crate::tr_format!("Assistent: {}", "Builder: {}", self.db) }
    }

    fn session(&self) -> Option<String> {
        Some(format!("builder\t{}\t{}", self.id, self.db))
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.run(cx);
    }

    fn save_now(&mut self) {
        self.save();
    }

    fn on_close(&mut self) -> bool {
        let _ = std::fs::remove_file(saved_path(&self.id));
        true
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db {
            self.schema = cx.schema(db);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        self.poll(cx);
        if self.job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.db.is_empty()
            && let Some(d) = cx.databases.iter().find(|d| !db::SYSTEM_DATABASES.contains(&d.as_str())) {
                self.db = d.clone();
            }
        if self.loaded_for != self.db {
            if self.loaded_for != "\u{0}" {
                // andere Datenbank: neu anfangen
                self.query = Query::default();
                self.result = None;
                self.initialized = false;
            }
            self.schema = cx.schema(&self.db);
            self.loaded_for = self.db.clone();
        }
        if self.schema.is_none() {
            self.schema = cx.schema(&self.db);
        }
        // Neue Abfrage: mit der ersten Tabelle beginnen
        if !self.initialized
            && let Some(t) = self.schema.as_ref().and_then(|s| s.tables.first()) {
                if self.query == Query::default() {
                    self.query.parts[0].select.sources = vec![Source::table(&t.name)];
                }
                self.initialized = true;
            }
        if self.last_save.elapsed().as_secs() >= 1 {
            self.save();
        }
        let pal = style::pal();
        let snap = self.snapshot();
        if self.pretty_for.as_ref() != Some(&snap) {
            self.pretty_sql = crate::sqlfmt::format_sql(&self.current_sql()).trim_end().to_string();
            self.pretty_for = Some(snap);
        }
        let sql = self.pretty_sql.clone();
        // Strg+Z / Strg+Y, solange kein Textfeld getippt wird
        if !ui.ctx().egui_wants_keyboard_input() {
            let (z, y) = ui.ctx().input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z), i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) || i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)));
            if z {
                self.undo();
            } else if y {
                self.redo();
            }
        }

        ui.horizontal(|ui| {
            let mut d = self.db.clone();
            if super::db_combo(ui, "bdb", cx.databases, &mut d) {
                self.db = d;
            }
            ui.add_space(4.0);
            let items: Vec<(Mode, &str)> = Mode::ALL.iter().map(|m| (*m, m.label())).collect();
            if crate::widgets::segmented(ui, &mut self.mode, &items) {
                self.result = None;
                self.error = None;
            }
            // Rechts: Ausfuehren, Rueckgaengig, weitere Aktionen
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    if icons::primary_button(ui, Icon::Play, crate::i18n::text("Ausführen")).on_hover_text(crate::keymap::text(crate::keymap::Cmd::RunAll)).clicked() {
                        self.run(cx);
                    }
                });
                if self.mode == Mode::Select
                    && ui.add_enabled_ui(self.job.is_none(), |ui| icons::button(ui, Icon::Explain, crate::i18n::text("Ausführungsplan (EXPLAIN)"))).inner.clicked()
                {
                    self.explain(cx);
                }
                let more = icons::button(ui, Icon::More, crate::i18n::text("Weitere Aktionen"));
                egui::Popup::menu(&more).show(|ui| {
                    if ui.button(crate::i18n::text("Im Editor öffnen")).clicked() {
                        if !sql.is_empty() {
                            cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql: format!("{sql};\n"), run: false });
                        }
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Kopieren")).clicked() {
                        ui.ctx().copy_text(sql.clone());
                        ui.close();
                    }
                    if ui.button(crate::i18n::text("Als Java-Code kopieren")).clicked() {
                        if !sql.is_empty() {
                            let target = crate::javagen::Target::new(&cx.settings.conn, &self.db);
                            ui.ctx().copy_text(crate::javagen::query_class(&self.current_sql(), &target));
                            cx.status(crate::i18n::text("Java-Code kopiert."));
                        }
                        ui.close();
                    }
                    if self.mode == Mode::Select && ui.button(crate::i18n::text("Als Sicht speichern …")).clicked() {
                        self.view_name = Some(String::new());
                        ui.close();
                    }
                    if self.mode == Mode::Select {
                        crate::widgets::check(ui, &mut self.live, crate::i18n::text("Live-Vorschau")).on_hover_text(crate::i18n::text("Zeigt das Ergebnis (höchstens 200 Zeilen) schon während der Eingabe"));
                    }
                    ui.separator();
                    if ui.button(crate::i18n::text("Neu")).clicked() {
                        self.query = Query::default();
                        self.dml = Dml::default();
                        if let Some(t) = self.schema.as_ref().and_then(|s| s.tables.first()) {
                            self.query.parts[0].select.sources = vec![Source::table(&t.name)];
                        }
                        self.result = None;
                        self.error = None;
                        ui.close();
                    }
                });
                if ui.add_enabled_ui(!self.redo.is_empty(), |ui| icons::button(ui, Icon::Redo, crate::i18n::text("Wiederholen (Strg+Y)"))).inner.clicked() {
                    self.redo();
                }
                if ui.add_enabled_ui(!self.undo.is_empty(), |ui| icons::button(ui, Icon::Undo, crate::i18n::text("Rückgängig (Strg+Z)"))).inner.clicked() {
                    self.undo();
                }
            });
        });
        ui.add_space(4.0);

        // Ergebnis und SQL unten
        egui::Panel::bottom(egui::Id::new(("builder-bottom", &self.id)))
            .resizable(true)
            .default_size(ui.available_height() * 0.42)
            .min_size(90.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 0 }))
            .show(ui, |ui| {
                egui::Panel::left(egui::Id::new(("builder-sql", &self.id))).resizable(true).default_size(ui.available_width() * 0.4).frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 4, top: 0, bottom: 0 })).show(ui, |ui| pane_card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SQL").small().strong().color(pal.text_weak));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::button(ui, Icon::Copy, crate::i18n::text("Kopieren")).clicked() {
                                ui.ctx().copy_text(sql.clone());
                                cx.status(crate::i18n::text("SQL kopiert."));
                            }
                            let lines = sql.lines().count();
                            ui.label(RichText::new(crate::tr_format!("{} Zeile(n)", "{} line(s)", lines)).small().color(pal.text_weak));
                        });
                    });
                    egui::ScrollArea::both().id_salt("bsql").auto_shrink([false, false]).show(ui, |ui| {
                        let galley = self.layout_cache.layout(ui, &sql, &egui::FontId::monospace(12.5));
                        ui.add(egui::Label::new(galley).selectable(true).wrap_mode(egui::TextWrapMode::Extend));
                    });
                }));
                pane_card(ui, |ui| {
                if let Some(e) = &self.error {
                    egui::Frame::new().fill(pal.error_text.gamma_multiply(0.12)).corner_radius(style::RADIUS as f32).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new(e).color(pal.error_text));
                    });
                }
                if self.job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(crate::i18n::text("Läuft …")).small().color(pal.text_weak));
                    });
                }
                if let Some(rs) = &self.result {
                    ui.horizontal(|ui| {
                        let took = self.took.map(|t| if t.as_millis() < 1000 { format!(" · {} ms", t.as_millis()) } else { format!(" · {:.1} s", t.as_secs_f32()) }).unwrap_or_default();
                        ui.label(RichText::new(format!("{}{took}", crate::tr_format!("{} Zeile(n)", "{} row(s)", rs.rows.len()))).color(pal.ok_text));
                        grid::export_menu(ui, "ergebnis", &rs.columns, &rs.rows);
                    });
                    grid::show(ui, ("bgrid", &self.id), &rs.columns, &rs.rows, grid::Edit::View, &mut self.grid);
                } else if self.job.is_none() && self.error.is_none() {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(crate::i18n::text("Noch kein Ergebnis. „Ausführen“ zeigt die Zeilen der Abfrage.")).color(pal.text_weak));
                    });
                }
                });
            });

        let tables: Vec<String> = self.schema.as_ref().map(|s| s.tables.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
        let env = Env { schema: self.schema.as_deref(), tables, ctes: Vec::new(), scope: Vec::new(), depth: 0 };
        crate::tabs::page_keys(ui);
        egui::ScrollArea::both().id_salt(("builder-edit", &self.id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            let id = egui::Id::new(("bq", &self.id));
            match self.mode {
                Mode::Select => query_ui(ui, &mut self.query, &env, id),
                m => dml::ui(ui, &mut self.dml, m, &mut self.query, &env, id),
            }
        });
        self.track_changes();
        // Live-Vorschau: kurz nach der letzten Aenderung die Abfrage mit wenigen Zeilen ausfuehren
        if self.live && self.mode == Mode::Select && self.job.is_none() && self.live_done.as_ref() != Some(&self.last_snap) {
            let wait = std::time::Duration::from_millis(600);
            if self.live_changed.elapsed() >= wait {
                self.preview(cx);
            } else {
                ui.ctx().request_repaint_after(wait - self.live_changed.elapsed());
            }
        }

        // Als Sicht speichern
        let mut close = false;
        if let Some(name) = self.view_name.as_mut() {
            egui::Window::new(crate::i18n::text("Als Sicht speichern")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
                let r = ui.add(crate::widgets::edit(name).hint_text(crate::i18n::text("Name der Sicht")).desired_width(240.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    let ok = ui.button(crate::i18n::text("Speichern")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if ok && !name.trim().is_empty()
                        && let Some(dbc) = cx.db {
                            let create = format!("CREATE OR REPLACE VIEW {} AS {}", db::q(name.trim()), self.query.sql());
                            match dbc.exec_in(&self.db, &create) {
                                Ok(_) => {
                                    cx.status(crate::tr_format!("Sicht {} gespeichert.", "View {} saved.", name.trim()));
                                    cx.actions.push(Action::SchemaChanged(self.db.clone()));
                                    close = true;
                                }
                                Err(e) => cx.error(e),
                            }
                        }
                    if ui.button(crate::i18n::text("Abbrechen")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });
        }
        if close {
            self.view_name = None;
        }
    }
}
