// Neue Beziehungen anlegen (Dialog und SQL).

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum LinkKind {
    OneToMany,
    OneToOne,
    ManyToMany,
}

pub(super) struct LinkDlg {
    pub(super) kind: LinkKind,
    pub(super) table: String,
    pub(super) column: String,
    pub(super) target: String,
    pub(super) on_delete: String,
    pub(super) on_update: String,
    /// n:m: zweite Tabelle und Name der Zwischentabelle (leer = automatisch)
    pub(super) other: String,
    pub(super) junction: String,
}

pub(super) const RULES: &[&str] = &["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"];

/// SQL fuer eine neue Beziehung. Err("") = noch unvollstaendig, Err(text) = Problem.
pub(super) fn link_sql(db: &str, schema: &Schema, d: &LinkDlg) -> Result<String, String> {
    if d.kind == LinkKind::ManyToMany {
        if d.table.is_empty() || d.other.is_empty() {
            return Err(String::new());
        }
        let key = |name: &str| -> Result<(String, String), String> {
            let t = schema.table(name).ok_or_else(String::new)?;
            let pk = t.pk_columns();
            if pk.len() != 1 {
                return Err(crate::tr_format!("„{name}“ braucht einen Primärschlüssel aus genau einer Spalte (z. B. id).", "“{name}” needs a primary key consisting of exactly one column, such as id."));
            }
            let ty = t.columns.iter().find(|c| c.name == pk[0]).map(|c| c.col_type.clone()).unwrap_or_else(|| "INT".into());
            Ok((pk[0].clone(), ty))
        };
        let (pa, ta) = key(&d.table)?;
        let (pb, tb) = key(&d.other)?;
        let junction = if d.junction.trim().is_empty() { format!("{}_{}", d.table, d.other) } else { d.junction.trim().to_string() };
        if schema.table(&junction).is_some() {
            return Err(crate::tr_format!("Eine Tabelle „{junction}“ gibt es schon – bitte einen anderen Namen wählen.", "A table named “{junction}” already exists — please choose another name."));
        }
        let ca = format!("{}_{}", d.table, pa);
        let mut cb = format!("{}_{}", d.other, pb);
        if cb == ca {
            cb.push('2');
        }
        return Ok(format!(
            "CREATE TABLE {db}.{j} (\n  {ca} {ta} NOT NULL,\n  {cb} {tb} NOT NULL,\n  PRIMARY KEY ({ca}, {cb}),\n  \
             CONSTRAINT {fa} FOREIGN KEY ({ca}) REFERENCES {db}.{a} ({pa}) ON DELETE CASCADE ON UPDATE CASCADE,\n  \
             CONSTRAINT {fb} FOREIGN KEY ({cb}) REFERENCES {db}.{b} ({pb}) ON DELETE CASCADE ON UPDATE CASCADE\n) ENGINE=InnoDB",
            db = q(db),
            j = q(&junction),
            ca = q(&ca),
            cb = q(&cb),
            fa = q(&format!("fk_{junction}_{ca}")),
            fb = q(&format!("fk_{junction}_{cb}")),
            a = q(&d.table),
            b = q(&d.other),
            pa = q(&pa),
            pb = q(&pb),
        ));
    }
    let Some((rt, rc)) = d.target.split_once('.') else { return Err(String::new()) };
    if d.table.is_empty() || d.column.is_empty() {
        return Err(String::new());
    }
    let already_unique = schema.unique_keys.iter().any(|(t, k)| *t == d.table && k.len() == 1 && k[0] == d.column);
    let unique = if d.kind == LinkKind::OneToOne && !already_unique {
        format!("ADD UNIQUE KEY {} ({}), ", q(&format!("uq_{}_{}", d.table, d.column)), q(&d.column))
    } else {
        String::new()
    };
    Ok(format!(
        "ALTER TABLE {}.{} {unique}ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}.{} ({}) ON DELETE {} ON UPDATE {}",
        q(db),
        q(&d.table),
        q(&format!("fk_{}_{}", d.table, d.column)),
        q(&d.column),
        q(db),
        q(rt),
        q(rc),
        d.on_delete,
        d.on_update
    ))
}

impl ErTab {
    pub(super) fn open_link(&mut self, table: String, column: String, target: String) {
        // Ist die Spalte schon eindeutig, ist 1:1 die naheliegende Art
        let unique = self.raw.as_ref().is_some_and(|s| {
            s.unique_keys.iter().any(|(t, k)| *t == table && k.len() == 1 && k[0] == column)
        });
        self.link = Some(LinkDlg {
            kind: if unique { LinkKind::OneToOne } else { LinkKind::OneToMany },
            table,
            column,
            target,
            on_delete: "RESTRICT".into(),
            on_update: "CASCADE".into(),
            other: String::new(),
            junction: String::new(),
        });
    }

    /// Rueckfrage zum Loeschen einer Beziehung (bei n:m die Zwischentabelle)
    pub(super) fn delete_action(&self, fk: &ForeignKey) -> Action {
        match self.nm.get(&fk_key(fk)) {
            Some(j) => Action::Confirm {
                text: crate::tr_format!("n:m-Beziehung {} – {} löschen? Die Zwischentabelle „{j}“ wird mit allen Zuordnungen gelöscht.", "Delete n:m relationship {} – {}? Junction table “{j}” and all its associations will be deleted.", fk.table, fk.ref_table),
                db: Some(self.db.clone()),
                sql: format!("DROP TABLE {}", q(j)),
            },
            None => Action::Confirm {
                text: crate::tr_format!("Beziehung {}.{} → {}.{} löschen?", "Delete relationship {}.{} → {}.{}?", fk.table, fk.columns.join(","), fk.ref_table, fk.ref_columns.join(",")),
                db: Some(self.db.clone()),
                sql: format!("ALTER TABLE {} DROP FOREIGN KEY {}", q(&fk.table), q(&fk.name)),
            },
        }
    }

    pub(super) fn link_dialog(&mut self, ctx: &egui::Context, cx: &mut Ctx) {
        let Some(schema) = self.raw.clone() else { return };
        let db = self.db.clone();
        let Some(dlg) = self.link.as_mut() else { return };
        let mut ok = false;
        let mut cancel = false;
        let tables: Vec<String> = schema.tables.iter().filter(|t| !t.is_view).map(|t| t.name.clone()).collect();
        let cols: Vec<String> = schema
            .table(&dlg.table)
            .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let targets: Vec<String> = schema
            .tables
            .iter()
            .filter(|t| !t.is_view)
            .flat_map(|t| t.columns.iter().filter(|c| c.is_pk() || c.key == "UNI").map(move |c| format!("{}.{}", t.name, c.name)))
            .collect();
        egui::Window::new(crate::i18n::text("Beziehung anlegen"))
            .id(egui::Id::new(("erlink", &self.db)))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::text("Art:"));
                    let before = dlg.kind;
                    ui.radio_value(&mut dlg.kind, LinkKind::OneToMany, "1:n");
                    ui.radio_value(&mut dlg.kind, LinkKind::OneToOne, "1:1");
                    ui.radio_value(&mut dlg.kind, LinkKind::ManyToMany, "n:m");
                    // Beim Wechsel zu n:m die Zieltabelle als zweite Tabelle uebernehmen
                    if dlg.kind == LinkKind::ManyToMany && before != LinkKind::ManyToMany && dlg.other.is_empty() {
                        if let Some((rt, _)) = dlg.target.split_once('.') {
                            dlg.other = rt.to_string();
                        }
                    }
                });
                ui.add_space(4.0);
                egui::Grid::new("erlinkgrid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    if dlg.kind == LinkKind::ManyToMany {
                        ui.label(crate::i18n::text("Tabelle A:"));
                        crate::tabs::str_combo(ui, "erl_t", &tables, &mut dlg.table, 180.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("Tabelle B:"));
                        crate::tabs::str_combo(ui, "erl_o", &tables, &mut dlg.other, 180.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("Zwischentabelle:"));
                        let auto = format!("{}_{}", dlg.table, dlg.other);
                        ui.add(egui::TextEdit::singleline(&mut dlg.junction).hint_text(auto).desired_width(180.0));
                        ui.end_row();
                    } else {
                        ui.label(if dlg.kind == LinkKind::OneToOne { crate::i18n::text("Tabelle (abhängig):") } else { crate::i18n::text("Tabelle (viele):") });
                        if crate::tabs::str_combo(ui, "erl_t", &tables, &mut dlg.table, 180.0) {
                            dlg.column.clear();
                        }
                        ui.end_row();
                        ui.label(crate::i18n::text("Spalte:"));
                        crate::tabs::str_combo(ui, "erl_c", &cols, &mut dlg.column, 180.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("verweist auf (eins):"));
                        crate::tabs::str_combo(ui, "erl_r", &targets, &mut dlg.target, 180.0);
                        ui.end_row();
                        let rules: Vec<String> = RULES.iter().map(|s| s.to_string()).collect();
                        ui.label(crate::i18n::text("Beim Löschen:"));
                        crate::tabs::str_combo(ui, "erl_d", &rules, &mut dlg.on_delete, 120.0);
                        ui.end_row();
                        ui.label(crate::i18n::text("Beim Ändern:"));
                        crate::tabs::str_combo(ui, "erl_u", &rules, &mut dlg.on_update, 120.0);
                        ui.end_row();
                    }
                });
                if dlg.kind != LinkKind::ManyToMany {
                    // Typen vergleichen
                    let ty = |t: &str, c: &str| {
                        schema.table(t).and_then(|ti| ti.columns.iter().find(|x| x.name == c)).map(|x| x.col_type.clone())
                    };
                    if let (Some(a), Some((rt, rc))) = (ty(&dlg.table, &dlg.column), dlg.target.split_once('.')) {
                        if let Some(b) = ty(rt, rc) {
                            if a != b {
                                ui.label(RichText::new(crate::tr_format!("Hinweis: Datentypen unterscheiden sich ({a} / {b}).", "Hint: Data types differ ({a} / {b}).")).color(style::pal().error_text).small());
                            }
                        }
                    }
                }
                let sql = link_sql(&db, &schema, dlg);
                ui.separator();
                match &sql {
                    Ok(sql) => {
                        egui::CollapsingHeader::new(RichText::new(crate::i18n::text("SQL anzeigen")).small()).id_salt("erlinksql").show(ui, |ui| {
                            ui.label(RichText::new(sql).monospace().small());
                        });
                    }
                    Err(e) if !e.is_empty() => {
                        ui.label(RichText::new(e).small().color(style::pal().error_text));
                    }
                    Err(_) => {}
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(sql.is_ok(), egui::Button::new(crate::i18n::text("Anlegen"))).clicked() {
                        ok = true;
                    }
                    if ui.button(crate::i18n::text("Abbrechen")).clicked() {
                        cancel = true;
                    }
                });
            });
        if ok {
            let d = self.link.take().unwrap();
            let Ok(sql) = link_sql(&self.db, &schema, &d) else { return };
            if let Some(dbc) = cx.db {
                match dbc.exec(&sql, ()) {
                    Ok(_) => {
                        let what = match d.kind {
                            LinkKind::ManyToMany => crate::tr_format!("n:m-Beziehung {} ↔ {} angelegt.", "n:m relationship {} ↔ {} created.", d.table, d.other),
                            LinkKind::OneToOne => crate::tr_format!("1:1-Beziehung {}.{} → {} angelegt.", "1:1 relationship {}.{} → {} created.", d.table, d.column, d.target),
                            LinkKind::OneToMany => crate::tr_format!("1:n-Beziehung {}.{} → {} angelegt.", "1:n relationship {}.{} → {} created.", d.table, d.column, d.target),
                        };
                        cx.status(what);
                        cx.actions.push(Action::SchemaChanged(self.db.clone()));
                    }
                    Err(e) => {
                        let tip = match d.kind {
                            LinkKind::ManyToMany => crate::i18n::text("Tipp: Gibt es die Zwischentabelle schon? Dann einen anderen Namen wählen."),
                            LinkKind::OneToOne => crate::i18n::text("Tipp: Für 1:1 darf jeder Wert in der Spalte nur einmal vorkommen. Beide Spalten brauchen den gleichen Datentyp."),
                            LinkKind::OneToMany => crate::i18n::text("Tipp: Beide Spalten brauchen den gleichen Datentyp, und vorhandene Werte müssen in der Zieltabelle existieren."),
                        };
                        cx.error(format!("{e}\n\n{tip}\n\nSQL: {sql}"));
                        self.link = Some(d);
                    }
                }
            }
        } else if cancel {
            self.link = None;
        }
    }
}
