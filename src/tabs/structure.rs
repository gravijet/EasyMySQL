// Struktur einer Tabelle: Spalten, Indizes, Fremdschluessel bearbeiten.

use super::designer::{ref_field, type_field};
use super::{Action, Ctx, TabView};
use crate::db::{ColDef, ForeignKey, IndexInfo, TableInfo, q};
use crate::style;
use eframe::egui::{self, RichText};

enum Dlg {
    Column { orig: Option<String>, def: ColDef, after: String },
    Index { name: String, cols: String, unique: bool },
    Fk { col: String, target: String, on_delete: String, on_update: String },
}

pub struct StructureTab {
    pub db: String,
    pub table: String,
    info: Option<TableInfo>,
    indexes: Vec<IndexInfo>,
    fks_out: Vec<ForeignKey>,
    fks_in: Vec<ForeignKey>,
    create: String,
    loaded: bool,
    schema_wait: bool,
    selected: Option<usize>,
    dlg: Option<Dlg>,
    targets: Vec<String>,
}

const RULES: &[&str] = &["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"];

impl StructureTab {
    pub fn new(db: String, table: String) -> Self {
        Self {
            db,
            table,
            info: None,
            indexes: Vec::new(),
            fks_out: Vec::new(),
            fks_in: Vec::new(),
            create: String::new(),
            loaded: false,
            schema_wait: false,
            selected: None,
            dlg: None,
            targets: Vec::new(),
        }
    }

    fn full(&self) -> String {
        format!("{}.{}", q(&self.db), q(&self.table))
    }

    fn reload(&mut self, cx: &mut Ctx) {
        self.loaded = true;
        let Some(dbc) = cx.db else { return };
        self.schema_wait = true;
        self.apply_schema(cx);
        self.indexes = dbc.indexes(&self.db, &self.table).unwrap_or_default();
        self.create = dbc.create_statement(&self.db, &self.table).unwrap_or_default();
    }

    /// Tabellen-, Fremdschluessel- und Spaltenlisten uebernehmen, sobald die Struktur gelesen ist.
    fn apply_schema(&mut self, cx: &mut Ctx) {
        if let Some(s) = cx.schema(&self.db) {
            self.schema_wait = false;
            self.info = s.table(&self.table).cloned();
            self.fks_out = s.fks.iter().filter(|f| f.table == self.table).cloned().collect();
            self.fks_in = s.fks.iter().filter(|f| f.ref_table == self.table).cloned().collect();
            self.targets = s
                .tables
                .iter()
                .filter(|t| !t.is_view)
                .flat_map(|t| t.columns.iter().map(move |c| format!("{}.{}", t.name, c.name)))
                .collect();
        } else if !cx.schema_loading(&self.db) {
            self.schema_wait = false;
        }
    }

    fn alter(&mut self, cx: &mut Ctx, sql: String) -> bool {
        let Some(dbc) = cx.db else { return false };
        match dbc.exec(&sql, ()) {
            Ok(_) => {
                cx.status(crate::i18n::text("Struktur geändert."));
                cx.actions.push(Action::SchemaChanged(self.db.clone()));
                true
            }
            Err(e) => {
                cx.error(format!("{e}\n\nSQL: {sql}"));
                false
            }
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(dlg) = self.dlg.as_mut() else { return };
        let mut ok = false;
        let mut close = false;
        let title = match dlg {
            Dlg::Column { orig: None, .. } => crate::i18n::text("Spalte hinzufügen").to_string(),
            Dlg::Column { orig: Some(o), .. } => crate::tr_format!("Spalte {o} ändern", "Edit column {o}"),
            Dlg::Index { .. } => crate::i18n::text("Index hinzufügen").into(),
            Dlg::Fk { .. } => crate::i18n::text("Fremdschlüssel (Beziehung) hinzufügen").into(),
        };
        let cols: Vec<String> = self
            .info
            .as_ref()
            .map(|i| i.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let targets = self.targets.clone();
        egui::Window::new(title)
            .id(egui::Id::new(("structdlg", &self.db, &self.table)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("dlggrid").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| match dlg {
                    Dlg::Column { orig, def, after } => {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut def.name);
                        ui.end_row();
                        ui.label(crate::i18n::text("Datentyp:"));
                        type_field(ui, "coltype", &mut def.col_type);
                        ui.end_row();
                        ui.label("");
                        crate::widgets::check(ui, &mut def.not_null, crate::i18n::text("Nicht NULL"));
                        ui.end_row();
                        ui.label("");
                        crate::widgets::check(ui, &mut def.auto_inc, "AUTO_INCREMENT");
                        ui.end_row();
                        if orig.is_none() {
                            ui.label("");
                            crate::widgets::check(ui, &mut def.primary, crate::i18n::text("Primärschlüssel"));
                            ui.end_row();
                        }
                        ui.label("");
                        crate::widgets::check(ui, &mut def.unique, crate::i18n::text("Eindeutig (UNIQUE)"));
                        ui.end_row();
                        ui.label(crate::i18n::text("Standardwert:"));
                        ui.text_edit_singleline(&mut def.default);
                        ui.end_row();
                        ui.label(crate::i18n::text("Kommentar:"));
                        ui.text_edit_singleline(&mut def.comment);
                        ui.end_row();
                        if orig.is_none() {
                            ui.label("Position:");
                            let mut items = vec![crate::i18n::text("(am Ende)").to_string(), crate::i18n::text("(am Anfang)").to_string()];
                            items.extend(cols.iter().map(|c| crate::tr_format!("nach {c}", "after {c}")));
                            super::str_combo(ui, "colpos", &items, after, 160.0);
                            ui.end_row();
                            ui.label(crate::i18n::text("Verweist auf:"));
                            ref_field(ui, "colref", &targets, &mut def.references);
                            ui.end_row();
                        }
                    }
                    Dlg::Index { name, cols: c, unique } => {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                        ui.end_row();
                        ui.label(crate::i18n::text("Spalten:"));
                        ui.add(egui::TextEdit::singleline(c).hint_text(crate::i18n::text("spalte1, spalte2")));
                        ui.end_row();
                        ui.label("");
                        ui.horizontal_wrapped(|ui| {
                            for col in &cols {
                                if ui.button(col).clicked() {
                                    if !c.trim().is_empty() {
                                        c.push_str(", ");
                                    }
                                    c.push_str(col);
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        crate::widgets::check(ui, unique, crate::i18n::text("Eindeutig (UNIQUE)"));
                        ui.end_row();
                    }
                    Dlg::Fk { col, target, on_delete, on_update } => {
                        ui.label(crate::i18n::text("Spalte:"));
                        super::str_combo(ui, "fkcol", &cols, col, 160.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("Verweist auf:"));
                        ref_field(ui, "fkref", &targets, target);
                        ui.end_row();
                        let rules: Vec<String> = RULES.iter().map(|s| s.to_string()).collect();
                        ui.label(crate::i18n::text("Beim Löschen:"));
                        super::str_combo(ui, "fkdel", &rules, on_delete, 120.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("Beim Ändern:"));
                        super::str_combo(ui, "fkupd", &rules, on_update, 120.0);
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        ok = true;
                    }
                    if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                        close = true;
                    }
                });
            });
        if ok {
            let full = self.full();
            let sql = match self.dlg.as_ref().unwrap() {
                Dlg::Column { orig: None, def, after } => {
                    let pos = if after == crate::i18n::text("(am Anfang)") {
                        " FIRST".to_string()
                    } else if let Some(c) = after.strip_prefix(crate::i18n::text("nach ")) {
                        format!(" AFTER {}", q(c))
                    } else {
                        String::new()
                    };
                    let mut s = format!("ALTER TABLE {full} ADD COLUMN {}{pos}", def.sql());
                    if def.primary {
                        s.push_str(&format!(", ADD PRIMARY KEY ({})", q(&def.name)));
                    }
                    if def.unique && !def.primary {
                        s.push_str(&format!(", ADD UNIQUE ({})", q(&def.name)));
                    }
                    if let Some((t, c)) = def.fk_target() {
                        s.push_str(&format!(
                            ", ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
                            q(&format!("fk_{}_{}", self.table, def.name)),
                            q(&def.name),
                            q(&t),
                            q(&c)
                        ));
                    }
                    s
                }
                Dlg::Column { orig: Some(o), def, .. } => {
                    let mut d = def.clone();
                    d.primary = false;
                    let mut s = format!("ALTER TABLE {full} CHANGE COLUMN {} {}", q(o), d.sql());
                    let was_unique = self
                        .info
                        .as_ref()
                        .and_then(|i| i.columns.iter().find(|c| &c.name == o))
                        .map(|c| c.key == "UNI")
                        .unwrap_or(false);
                    if def.unique && !was_unique {
                        s.push_str(&format!(", ADD UNIQUE ({})", q(&def.name)));
                    }
                    s
                }
                Dlg::Index { name, cols, unique } => {
                    let list: Vec<String> = cols
                        .split(',')
                        .map(|c| c.trim())
                        .filter(|c| !c.is_empty())
                        .map(q)
                        .collect();
                    format!(
                        "ALTER TABLE {full} ADD {}INDEX {} ({})",
                        if *unique { "UNIQUE " } else { "" },
                        q(name.trim()),
                        list.join(", ")
                    )
                }
                Dlg::Fk { col, target, on_delete, on_update } => {
                    let (t, c) = target.split_once('.').unwrap_or(("", ""));
                    format!(
                        "ALTER TABLE {full} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
                        q(&format!("fk_{}_{}", self.table, col)),
                        q(col),
                        q(t),
                        q(c),
                        on_delete,
                        on_update
                    )
                }
            };
            if self.alter(cx, sql) {
                self.dlg = None;
            }
        } else if close {
            self.dlg = None;
        }
    }
}

impl StructureTab {
    /// Spaltenliste als Tabelle (gleiche Optik wie die Datentabelle); Klick waehlt, Doppelklick aendert
    fn columns_table(&mut self, ui: &mut egui::Ui, info: &TableInfo) {
        use egui_extras::{Column, TableBuilder};
        const PAD: f32 = 10.0;
        let pal = style::pal();
        let mut clicked = None;
        let mut edit = None;
        style::sunken_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let head = |ui: &mut egui::Ui, t: &str| {
                ui.painter().rect_filled(ui.max_rect().expand2(egui::vec2(4.0, 2.0)), 0.0, pal.face_light);
                ui.add_space(PAD);
                ui.label(RichText::new(t).strong());
            };
            TableBuilder::new(ui)
                .id_salt(("cols", &self.table))
                .striped(true)
                .vscroll(false)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(48.0))
                .column(Column::initial(140.0).at_least(60.0).clip(true))
                .column(Column::initial(110.0).at_least(60.0).clip(true))
                .column(Column::exact(60.0))
                .column(Column::initial(150.0).at_least(60.0).clip(true))
                .column(Column::initial(90.0).at_least(60.0).clip(true))
                .column(Column::initial(110.0).at_least(60.0).clip(true))
                .column(Column::remainder().at_least(80.0).clip(true))
                .header(30.0, |mut h| {
                    for t in ["#", "Name", crate::i18n::text("Datentyp"), "NULL", crate::i18n::text("Schlüssel"), crate::i18n::text("Standard"), "Extra", crate::i18n::text("Kommentar")] {
                        h.col(|ui| head(ui, t));
                    }
                })
                .body(|body| {
                    body.rows(26.0, info.columns.len(), |mut row| {
                        let i = row.index();
                        let c = &info.columns[i];
                        row.set_selected(self.selected == Some(i));
                        row.col(|ui| {
                            ui.add_space(PAD);
                            ui.label(RichText::new((i + 1).to_string()).color(pal.text_weak));
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            let mut name = RichText::new(&c.name);
                            if c.is_pk() {
                                name = name.strong();
                            }
                            ui.add(egui::Label::new(name).selectable(false).truncate());
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            ui.add(egui::Label::new(&c.col_type).selectable(false).truncate());
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            ui.label(if c.nullable { crate::i18n::text("ja") } else { crate::i18n::text("nein") });
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            let key = match c.key.as_str() {
                                "PRI" => crate::i18n::text("Primärschlüssel"),
                                "UNI" => crate::i18n::text("Eindeutig"),
                                "MUL" => "Index",
                                _ => "",
                            };
                            let text = match self.fks_out.iter().find(|f| f.columns.contains(&c.name)) {
                                Some(f) => format!("{key} → {}.{}", f.ref_table, f.ref_columns.join(",")),
                                None => key.to_string(),
                            };
                            ui.add(egui::Label::new(text).selectable(false).truncate());
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            match &c.default {
                                Some(d) => ui.add(egui::Label::new(d).selectable(false).truncate()),
                                None => ui.label(RichText::new("NULL").italics().color(pal.null_text)),
                            };
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            ui.add(egui::Label::new(&c.extra).selectable(false).truncate());
                        });
                        row.col(|ui| {
                            ui.add_space(PAD);
                            ui.add(egui::Label::new(&c.comment).selectable(false).truncate());
                        });
                        let r = row.response();
                        if r.clicked() {
                            clicked = Some(i);
                        }
                        if r.double_clicked() {
                            edit = Some(i);
                        }
                    });
                });
        });
        if let Some(i) = clicked {
            self.selected = Some(i);
        }
        if let Some(i) = edit
            && !info.is_view
        {
            self.dlg = Some(Dlg::Column { orig: Some(info.columns[i].name.clone()), def: ColDef::from_info(&info.columns[i]), after: String::new() });
        }
    }
}

impl TabView for StructureTab {
    fn icon(&self) -> Option<crate::icons::Icon> {
        Some(crate::icons::Icon::Columns)
    }

    fn title(&self) -> String {
        crate::tr_format!("{} (Struktur)", "{} (Structure)", self.table)
    }

    fn key(&self) -> Option<String> {
        Some(format!("struct:{}.{}", self.db, self.table))
    }

    fn session(&self) -> Option<String> {
        Some(format!("struct\t{}\t{}", self.db, self.table))
    }

    fn execute(&mut self, cx: &mut Ctx) {
        self.reload(cx);
    }

    fn schema_changed(&mut self, db: &str, cx: &mut Ctx) {
        if db == self.db {
            self.reload(cx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        if !self.loaded {
            self.reload(cx);
        } else if self.schema_wait {
            self.apply_schema(cx);
        }
        let Some(info) = self.info.clone() else {
            if self.schema_wait {
                ui.label(RichText::new(crate::i18n::text("Wird gelesen …")).color(style::pal().text_weak));
                return;
            }
            ui.label(RichText::new(crate::tr_format!("Tabelle {}.{} nicht gefunden.", "Table {}.{} was not found.", self.db, self.table)).color(style::pal().error_text));
            if ui.button(crate::i18n::text("Aktualisieren")).clicked() {
                self.reload(cx);
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}.{}", self.db, self.table)).size(15.0).strong());
            let mut desc = String::from(if info.is_view { crate::i18n::text("Sicht") } else { crate::i18n::text("Tabelle") });
            if !info.engine.is_empty() {
                desc.push_str(&format!(" · {}", info.engine));
            }
            desc.push_str(&crate::tr_format!(" · ca. {} Zeilen", " · approx. {} rows", info.rows.unwrap_or(0)));
            if !info.comment.is_empty() {
                desc.push_str(&format!(" · {}", info.comment));
            }
            ui.add_space(6.0);
            ui.label(RichText::new(desc).color(style::pal().text_weak));
        });
        ui.add_space(2.0);
        let view = info.is_view;
        let sel = self.selected.and_then(|i| info.columns.get(i)).cloned();
        let sel_i = self.selected.filter(|i| *i < info.columns.len());
        let mut mv = None;
        ui.horizontal(|ui| {
            use crate::icons::{self, Icon};
            ui.add_enabled_ui(!view, |ui| {
                if icons::text_button(ui, Icon::Plus, crate::i18n::text("Spalte")).on_hover_text(crate::i18n::tr("Neue Spalte am Ende", "New column at the end")).clicked() {
                    self.dlg = Some(Dlg::Column {
                        orig: None,
                        def: ColDef::new("", "VARCHAR(255)"),
                        after: crate::i18n::text("(am Ende)").into(),
                    });
                }
            });
            // Aktionen fuer die gewaehlte Spalte
            ui.add_enabled_ui(sel.is_some() && !view, |ui| {
                if icons::button(ui, Icon::Edit, crate::i18n::text("Spalte ändern")).clicked()
                    && let Some(c) = sel.clone()
                {
                    self.dlg = Some(Dlg::Column { orig: Some(c.name.clone()), def: ColDef::from_info(&c), after: String::new() });
                }
                if icons::button(ui, Icon::Trash, crate::i18n::text("Spalte löschen")).clicked()
                    && let Some(c) = sel.clone()
                {
                    cx.actions.push(Action::Confirm {
                        text: crate::tr_format!("Spalte \"{}\" wirklich löschen? Alle Werte darin gehen verloren.", "Really delete column \"{}\"? All its values will be lost.", c.name),
                        db: None,
                        sql: format!("ALTER TABLE {} DROP COLUMN {}", self.full(), q(&c.name)),
                    });
                }
                if icons::button(ui, Icon::ArrowUp, crate::i18n::tr("Spalte nach oben", "Move column up")).clicked() {
                    mv = Some(crate::db::Move::Left);
                }
                if icons::button(ui, Icon::ArrowDown, crate::i18n::tr("Spalte nach unten", "Move column down")).clicked() {
                    mv = Some(crate::db::Move::Right);
                }
            });
            // Rechts: Index, Fremdschluessel, Daten
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::text_button(ui, Icon::Table, crate::i18n::text("Daten anzeigen")).clicked() {
                    cx.actions.push(Action::OpenData { db: self.db.clone(), table: self.table.clone() });
                }
                ui.add_enabled_ui(!view, |ui| {
                    if icons::text_button(ui, Icon::Plus, crate::i18n::text("Fremdschlüssel")).clicked() {
                        self.dlg = Some(Dlg::Fk {
                            col: info.columns.first().map(|c| c.name.clone()).unwrap_or_default(),
                            target: String::new(),
                            on_delete: "RESTRICT".into(),
                            on_update: "CASCADE".into(),
                        });
                    }
                    if icons::text_button(ui, Icon::Plus, "Index").clicked() {
                        self.dlg = Some(Dlg::Index { name: format!("idx_{}", self.table), cols: String::new(), unique: false });
                    }
                });
            });
        });
        if let (Some(m), Some(i)) = (mv, sel_i)
            && let Some(sql) = crate::db::move_column(&self.full(), &info, &info.columns[i].name, m)
            && self.alter(cx, sql)
        {
            self.selected = Some(if m == crate::db::Move::Left { i - 1 } else { i + 1 });
        }
        ui.add_space(2.0);

        super::page_keys(ui);
        egui::ScrollArea::vertical().id_salt(("structscroll", &self.table)).auto_shrink([false, false]).show(ui, |ui| {
            super::section_title(ui, crate::i18n::text("Spalten"));
            self.columns_table(ui, &info);

            super::section_title(ui, crate::i18n::text("Indizes"));
            style::sunken_frame().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.indexes.is_empty() {
                    ui.label(RichText::new(crate::i18n::text("keine")).color(style::pal().null_text));
                }
                let mut drop = None;
                egui::Grid::new(("idx", &self.table)).striped(false).spacing([24.0, 8.0]).show(ui, |ui| {
                    for ix in &self.indexes {
                        ui.label(RichText::new(&ix.name).strong());
                        ui.label(RichText::new(if ix.name == "PRIMARY" { crate::i18n::text("Primärschlüssel") } else if ix.unique { crate::i18n::text("Eindeutig") } else { "Index" }).color(style::pal().text_weak));
                        ui.label(ix.columns.join(", "));
                        if !info.is_view && crate::icons::button(ui, crate::icons::Icon::Trash, crate::i18n::text("löschen")).clicked() {
                            drop = Some(ix.name.clone());
                        }
                        ui.end_row();
                    }
                });
                if let Some(n) = drop {
                    let sql = if n == "PRIMARY" {
                        format!("ALTER TABLE {} DROP PRIMARY KEY", self.full())
                    } else {
                        format!("ALTER TABLE {} DROP INDEX {}", self.full(), q(&n))
                    };
                    cx.actions.push(Action::Confirm { text: crate::tr_format!("Index \"{n}\" löschen?", "Delete index \"{n}\"?"), db: None, sql });
                }
            });

            super::section_title(ui, crate::i18n::text("Beziehungen (Fremdschlüssel)"));
            style::sunken_frame().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.fks_out.is_empty() && self.fks_in.is_empty() {
                    ui.label(RichText::new(crate::i18n::text("keine")).color(style::pal().null_text));
                }
                let mut drop = None;
                egui::Grid::new(("fks", &self.table)).striped(false).spacing([24.0, 8.0]).show(ui, |ui| {
                    for f in &self.fks_out {
                        ui.label(RichText::new(&f.name).strong());
                        ui.label(format!("{}.({}) → {}.({})", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
                        ui.label(RichText::new(format!("ON DELETE {} / ON UPDATE {}", f.on_delete, f.on_update)).color(style::pal().text_weak));
                        if crate::icons::button(ui, crate::icons::Icon::Trash, crate::i18n::text("löschen")).clicked() {
                            drop = Some(f.name.clone());
                        }
                        ui.end_row();
                    }
                    for f in &self.fks_in {
                        ui.label(RichText::new(&f.name).strong());
                        let r = ui.link(format!("{}.({}) → {}.({})", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
                        if r.clicked() {
                            cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: f.table.clone() });
                        }
                        ui.label(RichText::new(crate::i18n::text("(eingehend)")).color(style::pal().text_weak));
                        ui.label("");
                        ui.end_row();
                    }
                });
                if let Some(n) = drop {
                    cx.actions.push(Action::Confirm {
                        text: crate::tr_format!("Fremdschlüssel \"{n}\" löschen?", "Delete foreign key \"{n}\"?"),
                        db: None,
                        sql: format!("ALTER TABLE {} DROP FOREIGN KEY {}", self.full(), q(&n)),
                    });
                }
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::text("CREATE-Anweisung")).size(12.5).strong().color(style::pal().text_weak));
                if crate::icons::button(ui, crate::icons::Icon::Copy, crate::i18n::text("Kopieren")).clicked() {
                    ui.ctx().copy_text(self.create.clone());
                }
            });
            style::sunken_frame().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                let mut t = self.create.clone();
                ui.add(egui::TextEdit::multiline(&mut t).code_editor().frame(egui::Frame::NONE).desired_width(f32::INFINITY));
            });
        });

        let ctx = ui.ctx().clone();
        self.dialog_ui(&ctx, cx);
    }
}
