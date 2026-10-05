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
    Rename(String),
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
        if let Some(s) = cx.schema(&self.db) {
            self.info = s.table(&self.table).cloned();
            self.fks_out = s.fks.iter().filter(|f| f.table == self.table).cloned().collect();
            self.fks_in = s.fks.iter().filter(|f| f.ref_table == self.table).cloned().collect();
            self.targets = s
                .tables
                .iter()
                .filter(|t| !t.is_view)
                .flat_map(|t| t.columns.iter().map(move |c| format!("{}.{}", t.name, c.name)))
                .collect();
        }
        self.indexes = dbc.indexes(&self.db, &self.table).unwrap_or_default();
        self.create = dbc.create_statement(&self.db, &self.table).unwrap_or_default();
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
            Dlg::Rename(_) => crate::i18n::text("Tabelle umbenennen").into(),
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
                        ui.checkbox(&mut def.not_null, crate::i18n::text("Nicht NULL"));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut def.auto_inc, "AUTO_INCREMENT");
                        ui.end_row();
                        if orig.is_none() {
                            ui.label("");
                            ui.checkbox(&mut def.primary, crate::i18n::text("Primärschlüssel"));
                            ui.end_row();
                        }
                        ui.label("");
                        ui.checkbox(&mut def.unique, crate::i18n::text("Eindeutig (UNIQUE)"));
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
                                if ui.small_button(col).clicked() {
                                    if !c.trim().is_empty() {
                                        c.push_str(", ");
                                    }
                                    c.push_str(col);
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(unique, crate::i18n::text("Eindeutig (UNIQUE)"));
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
                    Dlg::Rename(n) => {
                        ui.label(crate::i18n::text("Neuer Name:"));
                        ui.text_edit_singleline(n);
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
                Dlg::Rename(n) => format!("RENAME TABLE {full} TO {}.{}", q(&self.db), q(n.trim())),
            };
            let rename = match self.dlg.as_ref().unwrap() {
                Dlg::Rename(n) => Some(n.trim().to_string()),
                _ => None,
            };
            if self.alter(cx, sql) {
                self.dlg = None;
                if let Some(n) = rename {
                    self.table = n;
                }
            }
        } else if close {
            self.dlg = None;
        }
    }
}

fn header_cell(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).strong());
}

impl TabView for StructureTab {
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
        }
        let Some(info) = self.info.clone() else {
            ui.label(RichText::new(crate::tr_format!("Tabelle {}.{} nicht gefunden.", "Table {}.{} was not found.", self.db, self.table)).color(style::pal().error_text));
            if ui.button(crate::i18n::text("Aktualisieren")).clicked() {
                self.reload(cx);
            }
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}.{}", self.db, self.table)).strong());
            let mut desc = format!("– {}", if info.is_view { crate::i18n::text("Sicht") } else { crate::i18n::text("Tabelle") });
            if !info.comment.is_empty() {
                desc.push_str(&format!(" \"{}\"", info.comment));
            }
            if !info.engine.is_empty() {
                desc.push_str(&format!(", Engine {}", info.engine));
            }
            desc.push_str(&crate::tr_format!(", ca. {} Zeilen", ", approx. {} rows", info.rows.unwrap_or(0)));
            ui.label(desc);
        });
        ui.horizontal_wrapped(|ui| {
            let view = info.is_view;
            if ui.add_enabled(!view, egui::Button::new(crate::i18n::text("+ Spalte"))).clicked() {
                self.dlg = Some(Dlg::Column {
                    orig: None,
                    def: ColDef::new("", "VARCHAR(255)"),
                    after: crate::i18n::text("(am Ende)").into(),
                });
            }
            let sel = self.selected.and_then(|i| info.columns.get(i)).cloned();
            if ui.add_enabled(sel.is_some() && !view, egui::Button::new(crate::i18n::text("Spalte ändern"))).clicked() {
                let c = sel.clone().unwrap();
                self.dlg = Some(Dlg::Column {
                    orig: Some(c.name.clone()),
                    def: ColDef::from_info(&c),
                    after: String::new(),
                });
            }
            if ui.add_enabled(sel.is_some() && !view, egui::Button::new(crate::i18n::text("Spalte löschen"))).clicked() {
                let c = sel.unwrap();
                cx.actions.push(Action::Confirm {
                    text: crate::tr_format!("Spalte \"{}\" wirklich löschen? Alle Werte darin gehen verloren.", "Really delete column \"{}\"? All its values will be lost.", c.name),
                    db: None,
                    sql: format!("ALTER TABLE {} DROP COLUMN {}", self.full(), q(&c.name)),
                });
            }
            ui.separator();
            if ui.add_enabled(!view, egui::Button::new("+ Index")).clicked() {
                self.dlg = Some(Dlg::Index {
                    name: format!("idx_{}", self.table),
                    cols: String::new(),
                    unique: false,
                });
            }
            if ui.add_enabled(!view, egui::Button::new(crate::i18n::text("+ Fremdschlüssel"))).clicked() {
                self.dlg = Some(Dlg::Fk {
                    col: info.columns.first().map(|c| c.name.clone()).unwrap_or_default(),
                    target: String::new(),
                    on_delete: "RESTRICT".into(),
                    on_update: "CASCADE".into(),
                });
            }
            ui.separator();
            if ui.button(crate::i18n::text("Umbenennen")).clicked() {
                self.dlg = Some(Dlg::Rename(self.table.clone()));
            }
            if ui.button(crate::i18n::text("Daten anzeigen")).clicked() {
                cx.actions.push(Action::OpenData { db: self.db.clone(), table: self.table.clone() });
            }
            if ui.button(crate::i18n::text("Aktualisieren")).clicked() {
                self.reload(cx);
            }
        });
        ui.add_space(4.0);

        egui::ScrollArea::vertical().id_salt(("structscroll", &self.table)).show(ui, |ui| {
            ui.label(RichText::new(crate::i18n::text("Spalten")).strong());
            style::sunken_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::Grid::new(("cols", &self.table)).striped(true).num_columns(8).spacing([14.0, 3.0]).show(ui, |ui| {
                    for h in ["#", "Name", crate::i18n::text("Datentyp"), "NULL", crate::i18n::text("Schlüssel"), crate::i18n::text("Standard"), "Extra", crate::i18n::text("Kommentar")] {
                        header_cell(ui, h);
                    }
                    ui.end_row();
                    for (i, c) in info.columns.iter().enumerate() {
                        let sel = self.selected == Some(i);
                        if ui.selectable_label(sel, format!("{}", i + 1)).clicked() {
                            self.selected = Some(i);
                        }
                        let mut name = RichText::new(&c.name);
                        if c.is_pk() {
                            name = name.strong();
                        }
                        let r = ui.selectable_label(sel, name);
                        if r.clicked() {
                            self.selected = Some(i);
                        }
                        if r.double_clicked() && !info.is_view {
                            self.dlg = Some(Dlg::Column { orig: Some(c.name.clone()), def: ColDef::from_info(c), after: String::new() });
                        }
                        ui.label(&c.col_type);
                        ui.label(if c.nullable { "ja" } else { crate::i18n::text("nein") });
                        let key = match c.key.as_str() {
                            "PRI" => crate::i18n::text("Primärschlüssel"),
                            "UNI" => crate::i18n::text("Eindeutig"),
                            "MUL" => "Index",
                            _ => "",
                        };
                        let fk = self.fks_out.iter().find(|f| f.columns.contains(&c.name));
                        match fk {
                            Some(f) => ui.label(format!("{key} → {}.{}", f.ref_table, f.ref_columns.join(","))),
                            None => ui.label(key),
                        };
                        match &c.default {
                            Some(d) => ui.label(d),
                            None => ui.label(RichText::new("NULL").italics().color(style::pal().null_text)),
                        };
                        ui.label(&c.extra);
                        ui.label(&c.comment);
                        ui.end_row();
                    }
                });
            });

            ui.add_space(8.0);
            ui.label(RichText::new(crate::i18n::text("Indizes")).strong());
            style::sunken_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.indexes.is_empty() {
                    ui.label(RichText::new(crate::i18n::text("keine")).color(style::pal().null_text));
                }
                let mut drop = None;
                egui::Grid::new(("idx", &self.table)).striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                    for ix in &self.indexes {
                        ui.label(&ix.name);
                        ui.label(if ix.name == "PRIMARY" { crate::i18n::text("Primärschlüssel") } else if ix.unique { crate::i18n::text("Eindeutig") } else { "Index" });
                        ui.label(ix.columns.join(", "));
                        if !info.is_view && ui.small_button(crate::i18n::text("löschen")).clicked() {
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

            ui.add_space(8.0);
            ui.label(RichText::new(crate::i18n::text("Beziehungen (Fremdschlüssel)")).strong());
            style::sunken_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.fks_out.is_empty() && self.fks_in.is_empty() {
                    ui.label(RichText::new(crate::i18n::text("keine")).color(style::pal().null_text));
                }
                let mut drop = None;
                egui::Grid::new(("fks", &self.table)).striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                    for f in &self.fks_out {
                        ui.label(&f.name);
                        ui.label(format!("{}.({}) → {}.({})", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
                        ui.label(format!("ON DELETE {} / ON UPDATE {}", f.on_delete, f.on_update));
                        if ui.small_button(crate::i18n::text("löschen")).clicked() {
                            drop = Some(f.name.clone());
                        }
                        ui.end_row();
                    }
                    for f in &self.fks_in {
                        ui.label(&f.name);
                        let r = ui.link(format!("{}.({}) → {}.({})", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
                        if r.clicked() {
                            cx.actions.push(Action::OpenStructure { db: self.db.clone(), table: f.table.clone() });
                        }
                        ui.label(crate::i18n::text("(eingehend)"));
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

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::text("CREATE-Anweisung")).strong());
                if ui.small_button(crate::i18n::text("Kopieren")).clicked() {
                    ui.ctx().copy_text(self.create.clone());
                }
            });
            style::sunken_frame().show(ui, |ui| {
                let mut t = self.create.clone();
                ui.add(egui::TextEdit::multiline(&mut t).code_editor().frame(egui::Frame::NONE).desired_width(f32::INFINITY));
            });
        });

        let ctx = ui.ctx().clone();
        self.dialog_ui(&ctx, cx);
    }
}
