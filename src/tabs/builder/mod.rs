// Abfrage-Assistent: beliebige SELECT-Abfragen grafisch zusammenstellen – mit Verknuepfungen,
// Unterabfragen, WITH, UNION/EXCEPT/INTERSECT, Gruppierung, Fensterfunktionen usw.

pub mod model;

use super::{Action, Ctx, TabView};
use crate::db::{self, ResultSet, Schema};
use crate::grid::{self, GridState};
use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, RichText};
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
        own.extend(env.scope.drain(..));
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
    icons::button_sized(ui, Icon::Close, "Entfernen", 18.0).clicked()
}

/// Abschnitt mit Ueberschrift (einklappbar)
fn section(ui: &mut egui::Ui, id: egui::Id, kw: &str, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    let st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    st.show_header(ui, |ui| {
        keyword(ui, kw);
        ui.label(RichText::new(title).small().color(style::pal().text_weak));
    })
    .body(|ui| add(ui));
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
    egui::ComboBox::from_id_salt(id).selected_text(RichText::new(text).monospace()).width(150.0).height(400.0).show_ui(ui, |ui| {
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
            ui.add(egui::TextEdit::singleline(col).desired_width(90.0)).on_hover_text("z. B. ein Alias aus SELECT");
            if ui.small_button("ohne Tabelle").clicked() {
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
            ui.menu_button(g, |ui| {
                for fd in FUNCS.iter().filter(|f| f.group == g) {
                    if ui.button(fd.name).on_hover_text(fd.hint).clicked() {
                        *name = fd.name.to_string();
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("andere:");
            if ui.add(egui::TextEdit::singleline(name).desired_width(110.0)).changed() {
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
        egui::ComboBox::from_id_salt(id.with("kind"))
            .selected_text(RichText::new(KINDS.iter().find(|x| x.0 == k).map(|x| x.1).unwrap_or("")).small())
            .width(78.0)
            .show_ui(ui, |ui| {
                for (kk, label) in KINDS {
                    ui.selectable_value(&mut nk, kk, label);
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
                ui.add(egui::TextEdit::singleline(v).desired_width(100.0).hint_text("Wert"));
            }
            Expr::Raw(r) => {
                ui.add(egui::TextEdit::singleline(r).desired_width(180.0).font(egui::TextStyle::Monospace).hint_text("SQL"));
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
                    ui.checkbox(distinct, RichText::new("DISTINCT").small()).on_hover_text("doppelte Werte nur einmal");
                }
                let mut remove = None;
                for (i, a) in args.iter_mut().enumerate() {
                    if i > 0 {
                        ui.label(",");
                    }
                    ui.push_id(i, |ui| {
                        expr_ui(ui, a, env, id.with(("arg", i)));
                        if icons::button_sized(ui, Icon::Close, "Argument entfernen", 14.0).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    args.remove(i);
                }
                if icons::button_sized(ui, Icon::Plus, "Argument hinzufügen", 16.0).clicked() {
                    args.push(env.first_column());
                }
                ui.label(")");
                if func_def(name).is_none_or(|d| d.window || d.aggregate) {
                    let mut on = over.is_some();
                    if ui.checkbox(&mut on, RichText::new("OVER").small()).on_hover_text("als Fensterfunktion").changed() {
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
                egui::ComboBox::from_id_salt(id.with("op")).selected_text(RichText::new(op.as_str()).monospace()).width(46.0).show_ui(ui, |ui| {
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
                        if ui.small_button("+ WHEN").clicked() {
                            whens.push((Cond::Cmp { left: env.first_column(), op: CmpOp::Eq, right: vec![Expr::Value(String::new())], quant: Quant::None, sub: None }, Expr::Value(String::new())));
                        }
                        let mut has_else = otherwise.is_some();
                        if ui.checkbox(&mut has_else, "ELSE").changed() {
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
                ui.label(RichText::new("Rahmen").small());
                ui.add(egui::TextEdit::singleline(&mut w.frame).desired_width(260.0).hint_text("z. B. ROWS BETWEEN 2 PRECEDING AND CURRENT ROW"));
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
    if icons::button_sized(ui, Icon::Plus, "Hinzufügen", 18.0).clicked() {
        list.push(env.first_column());
    }
}

fn order_list_ui(ui: &mut egui::Ui, list: &mut Vec<OrderItem>, env: &Env, id: egui::Id) {
    let mut remove = None;
    for (i, o) in list.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            expr_ui(ui, &mut o.expr, env, id.with(i));
            egui::ComboBox::from_id_salt(id.with(("dir", i))).selected_text(if o.desc { "absteigend" } else { "aufsteigend" }).width(92.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut o.desc, false, "aufsteigend (ASC)");
                ui.selectable_value(&mut o.desc, true, "absteigend (DESC)");
            });
            if remove_button(ui) {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if icons::button_sized(ui, Icon::Plus, "Sortierung hinzufügen", 18.0).clicked() {
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
            ui.add(egui::TextEdit::singleline(r).desired_width(260.0).font(egui::TextStyle::Monospace).hint_text("Bedingung in SQL"));
        }
        Cond::Exists { not, query } => {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(id.with("ex")).selected_text(if *not { "NOT EXISTS" } else { "EXISTS" }).width(100.0).show_ui(ui, |ui| {
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
                egui::ComboBox::from_id_salt(id.with("op")).selected_text(RichText::new(op.sql()).monospace()).width(96.0).height(420.0).show_ui(ui, |ui| {
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
                    let mut use_sub = sub.is_some();
                    if ui.toggle_value(&mut use_sub, RichText::new("Unterabfrage").small()).changed() {
                        *sub = use_sub.then(|| Box::new(new_query(env)));
                    }
                }
                match (op.arity(), sub.is_some()) {
                    (_, true) => {
                        if !matches!(op, CmpOp::In | CmpOp::NotIn) {
                            egui::ComboBox::from_id_salt(id.with("q"))
                                .selected_text(match quant {
                                    Quant::None => "–",
                                    Quant::Any => "ANY",
                                    Quant::All => "ALL",
                                })
                                .width(50.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(quant, Quant::None, "– (ein Wert)");
                                    ui.selectable_value(quant, Quant::Any, "ANY (mindestens einer)");
                                    ui.selectable_value(quant, Quant::All, "ALL (alle)");
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
                        if icons::button_sized(ui, Icon::Plus, "Wert hinzufügen", 16.0).clicked() {
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
            egui::ComboBox::from_id_salt(id.with("any")).selected_text(if *any { "ODER – eine muss gelten" } else { "UND – alle müssen gelten" }).width(190.0).show_ui(ui, |ui| {
                ui.selectable_value(any, false, "UND – alle müssen gelten");
                ui.selectable_value(any, true, "ODER – eine muss gelten");
            });
            ui.checkbox(not, "NICHT");
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
        if ui.small_button("+ Bedingung").clicked() {
            items.push(new_cmp(env));
        }
        if ui.small_button("+ Gruppe (…)").on_hover_text("Klammer mit UND/ODER").clicked() {
            items.push(Cond::Group { any: !*any, not: false, items: vec![new_cmp(env)] });
        }
        if ui.small_button("+ EXISTS").clicked() {
            items.push(Cond::Exists { not: false, query: Box::new(new_query(env)) });
        }
        if ui.small_button("+ SQL").clicked() {
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
                    egui::ComboBox::from_id_salt(id.with(("join", i))).selected_text(RichText::new(src.join.sql()).monospace().color(style::pal().syn_keyword)).width(150.0).show_ui(ui, |ui| {
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
                egui::ComboBox::from_id_salt(id.with(("kind", i)))
                    .selected_text(["Tabelle", "WITH", "Unterabfrage"][kind])
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut nk, 0, "Tabelle");
                        if !env.ctes.is_empty() {
                            ui.selectable_value(&mut nk, 1, "WITH-Abfrage");
                        }
                        ui.selectable_value(&mut nk, 2, "Unterabfrage");
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
                ui.add(egui::TextEdit::singleline(&mut src.alias).desired_width(70.0).hint_text("Alias"));
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
                                if src.on.is_empty() && ui.small_button("USING …").on_hover_text("gleichnamige Spalten").clicked() {
                                    src.using = "id".into();
                                }
                            });
                            cond_group_ui(ui, &mut src.on, &inner_env, id.with(("on", i)));
                        } else {
                            ui.horizontal(|ui| {
                                keyword(ui, "USING");
                                ui.add(egui::TextEdit::singleline(&mut src.using).desired_width(160.0).hint_text("spalte1, spalte2"));
                                if ui.small_button("ON").clicked() {
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
        ui.menu_button("+ Tabelle", |ui| {
            if !joins.is_empty() {
                ui.label(RichText::new("Über Beziehungen").small().color(style::pal().text_weak));
                for (label, src) in &joins {
                    if ui.button(label).clicked() {
                        s.sources.push(src.clone());
                        ui.close();
                    }
                }
                ui.separator();
            }
            ui.menu_button("Tabelle", |ui| {
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
                ui.menu_button("WITH-Abfrage", |ui| {
                    for (c, _) in &env.ctes {
                        if ui.button(c).clicked() {
                            s.sources.push(Source { from: From::Cte(c.clone()), ..Source::table(c) });
                            ui.close();
                        }
                    }
                });
            }
            if ui.button("Unterabfrage").clicked() {
                s.sources.push(Source { from: From::Sub(Box::new(new_query(env))), alias: format!("u{}", s.sources.len() + 1), ..Source::table("") });
                ui.close();
            }
        });
    });
}

fn items_ui(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.distinct, "DISTINCT").on_hover_text("doppelte Zeilen nur einmal");
        if s.items.is_empty() {
            ui.label(RichText::new("alle Spalten (*)").color(style::pal().text_weak));
        }
    });
    let mut remove = None;
    let mut swap = None;
    let n = s.items.len();
    for (i, it) in s.items.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(i > 0, |ui| {
                    if icons::button_sized(ui, Icon::Up, "nach oben", 16.0).clicked() {
                        swap = Some((i - 1, i));
                    }
                });
                ui.add_enabled_ui(i + 1 < n, |ui| {
                    if icons::button_sized(ui, Icon::Down, "nach unten", 16.0).clicked() {
                        swap = Some((i, i + 1));
                    }
                });
                expr_ui(ui, &mut it.expr, env, id.with(("item", i)));
                keyword(ui, "AS");
                ui.add(egui::TextEdit::singleline(&mut it.alias).desired_width(90.0).hint_text("Name"));
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
        ui.menu_button("+ Spalte", |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (src, cols) in env.scope.iter().take(s.sources.len()) {
                    ui.label(RichText::new(src).small().strong().color(style::pal().text_weak));
                    if ui.button(format!("{src}.*")).clicked() {
                        s.items.push(Item { expr: Expr::column(src, "*"), alias: String::new() });
                        ui.close();
                    }
                    for c in cols {
                        if ui.button(c).clicked() {
                            s.items.push(Item { expr: Expr::column(src, c), alias: String::new() });
                            ui.close();
                        }
                    }
                }
            });
        });
        ui.menu_button("+ Funktion", |ui| {
            let mut groups: Vec<&str> = FUNCS.iter().map(|f| f.group).collect();
            groups.dedup();
            for g in groups {
                ui.menu_button(g, |ui| {
                    for fd in FUNCS.iter().filter(|f| f.group == g) {
                        if ui.button(fd.name).on_hover_text(fd.hint).clicked() {
                            s.items.push(Item { expr: Expr::func(fd.name, Some(env.first_column())), alias: fd.name.to_lowercase() });
                            ui.close();
                        }
                    }
                });
            }
        });
        if ui.small_button("+ Ausdruck").clicked() {
            s.items.push(Item { expr: Expr::Op { op: "*".into(), left: Box::new(env.first_column()), right: Box::new(Expr::Value("1".into())) }, alias: String::new() });
        }
        if ui.small_button("+ CASE").clicked() {
            s.items.push(Item { expr: convert(&env.first_column(), Kind::Case, env), alias: String::new() });
        }
        if ui.small_button("+ Unterabfrage").clicked() {
            s.items.push(Item { expr: Expr::Sub(Box::new(new_query(env))), alias: String::new() });
        }
        if !s.items.is_empty() && ui.small_button("Alle entfernen").clicked() {
            s.items.clear();
        }
    });
}

fn select_ui(ui: &mut egui::Ui, s: &mut Select, env: &Env, id: egui::Id) {
    let inner = env.inside(s);
    section(ui, id.with("from"), "FROM", "Tabellen", true, |ui| sources_ui(ui, s, env, id.with("src")));
    section(ui, id.with("sel"), "SELECT", "Spalten", true, |ui| items_ui(ui, s, &inner, id.with("items")));
    section(ui, id.with("where"), "WHERE", "Bedingungen", true, |ui| cond_group_ui(ui, &mut s.filter, &inner, id.with("filter")));
    let group_open = !s.group.is_empty() || !s.having.is_empty() || s.items.iter().any(|i| i.expr.is_aggregate());
    section(ui, id.with("group"), "GROUP BY", "Gruppieren", group_open, |ui| {
        ui.horizontal_wrapped(|ui| {
            expr_list_ui(ui, &mut s.group, &inner, id.with("g"));
            ui.checkbox(&mut s.rollup, "WITH ROLLUP").on_hover_text("zusätzliche Zwischensummen");
        });
        let needed = s.needed_group();
        if !needed.is_empty() && needed.iter().any(|e| !s.group.contains(e)) {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Spalten ohne Zusammenfassung fehlen in GROUP BY.").small().color(style::pal().syn_function));
                if ui.small_button("Übernehmen").clicked() {
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
            ui.label(RichText::new("Bedingungen für Gruppen").small().color(style::pal().text_weak));
        });
        cond_group_ui(ui, &mut s.having, &inner, id.with("having"));
    });
    let mut order_env = inner.clone();
    let aliases: Vec<String> = s.items.iter().map(|i| i.alias.trim().to_string()).filter(|a| !a.is_empty()).collect();
    if !aliases.is_empty() {
        order_env.scope.insert(0, (String::new(), aliases));
    }
    section(ui, id.with("order"), "ORDER BY", "Sortieren", !s.order.is_empty(), |ui| {
        ui.horizontal_wrapped(|ui| order_list_ui(ui, &mut s.order, &order_env, id.with("ord")));
    });
    section(ui, id.with("limit"), "LIMIT", "Begrenzen", !s.limit.is_empty(), |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.limit).desired_width(60.0).hint_text("Zeilen"));
            keyword(ui, "OFFSET");
            ui.add(egui::TextEdit::singleline(&mut s.offset).desired_width(60.0).hint_text("0"));
        });
    });
}

fn query_ui(ui: &mut egui::Ui, qr: &mut Query, env: &Env, id: egui::Id) {
    if env.depth > 10 {
        ui.label("Zu tief verschachtelt.");
        return;
    }
    // WITH
    if !qr.with.is_empty() {
        ui.horizontal(|ui| {
            keyword(ui, "WITH");
            ui.checkbox(&mut qr.recursive, "RECURSIVE").on_hover_text("WITH-Abfragen dürfen sich selbst verwenden (z. B. Bäume, Zahlenreihen)");
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
                    ui.add(egui::TextEdit::singleline(&mut cte.name).desired_width(120.0).hint_text("Name"));
                    ui.add(egui::TextEdit::singleline(&mut cte.columns).desired_width(120.0).hint_text("(Spalten)"));
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
                    egui::ComboBox::from_id_salt(id.with(("setop", i))).selected_text(RichText::new(part.op.sql()).monospace().strong().color(style::pal().syn_keyword)).width(130.0).show_ui(ui, |ui| {
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
        if ui.small_button("+ WITH").on_hover_text("benannte Hilfsabfrage (CTE)").clicked() {
            let k = qr.with.len() + 1;
            qr.with.push(Cte { name: format!("hilf{k}"), columns: String::new(), query: new_query(&env) });
        }
    });
    if qr.parts.len() > 1 {
        let cols = env.output_cols(&Query { parts: vec![qr.parts[0].clone()], ..Default::default() });
        let mut oenv = env.clone();
        oenv.scope = vec![(String::new(), cols)];
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Gesamtergebnis").small().color(style::pal().text_weak));
            keyword(ui, "ORDER BY");
            order_list_ui(ui, &mut qr.order, &oenv, id.with("qord"));
            keyword(ui, "LIMIT");
            ui.add(egui::TextEdit::singleline(&mut qr.limit).desired_width(50.0));
            keyword(ui, "OFFSET");
            ui.add(egui::TextEdit::singleline(&mut qr.offset).desired_width(50.0));
        });
    }
}

// ---------------------------------------------------------------------------
// Registerkarte

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    db: String,
    query: Query,
}

fn saved_path(id: &str) -> std::path::PathBuf {
    crate::workspace::internal_dir().join("assistent").join(format!("{id}.json"))
}

type Job = Receiver<Result<ResultSet, String>>;

pub struct BuilderTab {
    id: String,
    pub db: String,
    query: Query,
    schema: Option<Schema>,
    loaded_for: String,
    result: Option<ResultSet>,
    error: Option<String>,
    grid: GridState,
    job: Option<Job>,
    saved_json: String,
    last_save: std::time::Instant,
    view_name: Option<String>,
    initialized: bool,
}

impl BuilderTab {
    pub fn new(db: Option<String>) -> Self {
        let id = crate::server::timestamp() + &format!("-{}", std::process::id() % 1000);
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
        }
    }

    /// Gespeicherten Assistenten aus der letzten Sitzung laden
    pub fn restore(id: &str, db: &str) -> Self {
        let mut t = Self::new(Some(db.to_string()));
        t.id = id.to_string();
        if let Some(s) = std::fs::read_to_string(saved_path(id)).ok().and_then(|j| serde_json::from_str::<Saved>(&j).ok()) {
            t.db = s.db;
            t.query = s.query;
            t.loaded_for = t.db.clone();
            t.initialized = true;
        }
        t.saved_json = serde_json::to_string(&Saved { db: t.db.clone(), query: t.query.clone() }).unwrap_or_default();
        t
    }

    fn save(&mut self) {
        let j = serde_json::to_string(&Saved { db: self.db.clone(), query: self.query.clone() }).unwrap_or_default();
        if j != self.saved_json {
            let _ = crate::workspace::atomic_write(&saved_path(&self.id), &j);
            self.saved_json = j;
        }
        self.last_save = std::time::Instant::now();
    }

    fn run(&mut self, cx: &mut Ctx) {
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
        let sql = self.query.sql();
        let db = self.db.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            use mysql::prelude::*;
            let mut conn = conn;
            let res = (|| {
                if !db.is_empty() {
                    conn.query_drop(format!("USE {}", db::q(&db))).map_err(db::err_text)?;
                }
                let mut out = db::run_script(&mut conn, &sql);
                if let Some(e) = out.error.take() {
                    return Err(e);
                }
                Ok(out.sets.into_iter().find(|s| s.has_table()).unwrap_or_default())
            })();
            let _ = tx.send(res);
        });
        self.job = Some(rx);
        let entry = crate::qhistory::Entry { time: crate::server::timestamp(), db: self.db.clone(), millis: 0, error: false, sql: self.query.pretty() };
        cx.actions.push(Action::History(vec![entry]));
    }

    fn poll(&mut self) {
        let Some(rx) = &self.job else { return };
        let Ok(r) = rx.try_recv() else { return };
        self.job = None;
        self.grid.reset();
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
    fn title(&self) -> String {
        if self.db.is_empty() { "Abfrage-Assistent".into() } else { format!("Assistent: {}", self.db) }
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
        self.poll();
        if self.job.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        if self.db.is_empty() {
            if let Some(d) = cx.databases.iter().find(|d| !db::SYSTEM_DATABASES.contains(&d.as_str())) {
                self.db = d.clone();
            }
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
        if !self.initialized {
            if let Some(t) = self.schema.as_ref().and_then(|s| s.tables.first()) {
                if self.query == Query::default() {
                    self.query.parts[0].select.sources = vec![Source::table(&t.name)];
                }
                self.initialized = true;
            }
        }
        if self.last_save.elapsed().as_secs() >= 1 {
            self.save();
        }
        let pal = style::pal();
        let sql = self.query.pretty();

        ui.horizontal(|ui| {
            let mut d = self.db.clone();
            if super::db_combo(ui, "bdb", cx.databases, &mut d) {
                self.db = d;
            }
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                if icons::text_button(ui, Icon::Play, "Ausführen").on_hover_text(crate::keymap::text(crate::keymap::Cmd::RunAll)).clicked() {
                    self.run(cx);
                }
            });
            if ui.button("Im Editor öffnen").clicked() {
                cx.actions.push(Action::OpenSql { db: Some(self.db.clone()), sql: format!("{sql};\n"), run: false });
            }
            if ui.button("Kopieren").clicked() {
                ui.ctx().copy_text(sql.clone());
            }
            if ui.button("Als Sicht speichern …").on_hover_text("CREATE VIEW").clicked() {
                self.view_name = Some(String::new());
            }
            if ui.button("Neu").on_hover_text("Leere Abfrage").clicked() {
                self.query = Query::default();
                if let Some(t) = self.schema.as_ref().and_then(|s| s.tables.first()) {
                    self.query.parts[0].select.sources = vec![Source::table(&t.name)];
                }
                self.result = None;
                self.error = None;
            }
        });

        // Ergebnis und SQL unten
        egui::Panel::bottom(egui::Id::new(("builder-bottom", &self.id)))
            .resizable(true)
            .default_size(ui.available_height() * 0.42)
            .min_size(90.0)
            .frame(egui::Frame::new().fill(pal.face).inner_margin(egui::Margin::symmetric(4, 4)))
            .show(ui, |ui| {
                egui::Panel::left(egui::Id::new(("builder-sql", &self.id))).resizable(true).default_size(ui.available_width() * 0.4).show(ui, |ui| {
                    egui::ScrollArea::both().id_salt("bsql").auto_shrink([false, false]).show(ui, |ui| {
                        let mut job = crate::sqledit::highlight(&sql, egui::FontId::monospace(12.5));
                        job.wrap.max_width = f32::INFINITY;
                        ui.add(egui::Label::new(job).selectable(true).wrap_mode(egui::TextWrapMode::Extend));
                    });
                });
                if let Some(e) = &self.error {
                    ui.label(RichText::new(e).color(pal.error_text));
                }
                if self.job.is_some() {
                    ui.spinner();
                }
                if let Some(rs) = &self.result {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} Zeile(n)", rs.rows.len())).color(pal.ok_text));
                        grid::export_menu(ui, "ergebnis", &rs.columns, &rs.rows);
                    });
                    grid::show(ui, ("bgrid", &self.id), &rs.columns, &rs.rows, false, &mut self.grid);
                }
            });

        let tables: Vec<String> = self.schema.as_ref().map(|s| s.tables.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
        let env = Env { schema: self.schema.as_ref(), tables, ctes: Vec::new(), scope: Vec::new(), depth: 0 };
        egui::ScrollArea::both().id_salt(("builder-edit", &self.id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            query_ui(ui, &mut self.query, &env, egui::Id::new(("bq", &self.id)));
        });

        // Als Sicht speichern
        let mut close = false;
        if let Some(name) = self.view_name.as_mut() {
            egui::Window::new("Als Sicht speichern").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
                let r = ui.add(egui::TextEdit::singleline(name).hint_text("Name der Sicht").desired_width(240.0));
                r.request_focus();
                ui.horizontal(|ui| {
                    let ok = ui.button("Speichern").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if ok && !name.trim().is_empty() {
                        if let Some(dbc) = cx.db {
                            let create = format!("CREATE OR REPLACE VIEW {} AS {}", db::q(name.trim()), self.query.sql());
                            match dbc.exec_in(&self.db, &create) {
                                Ok(_) => {
                                    cx.status(format!("Sicht {} gespeichert.", name.trim()));
                                    cx.actions.push(Action::SchemaChanged(self.db.clone()));
                                    close = true;
                                }
                                Err(e) => cx.error(e),
                            }
                        }
                    }
                    if ui.button("Abbrechen").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
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
