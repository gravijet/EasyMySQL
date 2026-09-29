// Ergebnis-/Datentabelle mit Gitterlinien und optionaler Zellbearbeitung.

use crate::db::Row;
use crate::style;
use eframe::egui::{self, Color32, Key, RichText, Sense, Stroke};
use egui_extras::{Column, TableBuilder};

#[derive(Default)]
pub struct GridState {
    pub selected: Option<usize>,
    editing: Option<(usize, usize, String, bool)>,
}

impl GridState {
    pub fn reset(&mut self) {
        self.selected = None;
        self.editing = None;
    }
}

pub enum GridEvent {
    /// Zelle geaendert (Zeile, Spalte, neuer Wert)
    Edit(usize, usize, Option<String>),
    DeleteRow(usize),
    /// Doppelklick in nicht bearbeitbarer Tabelle
    Activate,
}

fn display(v: &str) -> String {
    let first = v.lines().next().unwrap_or("");
    let mut s: String = first.chars().take(200).collect();
    if s.len() < v.len() {
        s.push('…');
    }
    s
}

fn cell_lines(ui: &egui::Ui) {
    let r = ui.max_rect();
    let p = ui.painter();
    let st = Stroke::new(1.0, style::GRID_LINE);
    p.hline(r.left()..=r.right() + 4.0, r.bottom() + 1.5, st);
    p.vline(r.right() + 3.5, r.top() - 2.0..=r.bottom() + 2.0, st);
}

/// Zeichnet die Tabelle. `editable`: Doppelklick bearbeitet eine Zelle.
pub fn show(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    columns: &[String],
    rows: &[Row],
    editable: bool,
    state: &mut GridState,
) -> Vec<GridEvent> {
    let mut events = Vec::new();
    if columns.is_empty() {
        return events;
    }
    let row_h = 20.0;
    let mut commit: Option<(usize, usize, Option<String>)> = None;
    let mut cancel = false;

    style::sunken_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        let mut tb = TableBuilder::new(ui)
            .id_salt(id)
            .striped(true)
            .resizable(true)
            .auto_shrink([false, false])
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(44.0));
        for (ci, c) in columns.iter().enumerate() {
            let mut chars = c.chars().count();
            for r in rows.iter().take(100) {
                if let Some(Some(v)) = r.get(ci) {
                    chars = chars.max(v.lines().next().unwrap_or("").chars().count().min(40));
                }
            }
            let w = (chars as f32 * 7.5 + 20.0).clamp(50.0, 320.0);
            tb = tb.column(Column::initial(w).at_least(30.0).clip(true).resizable(true));
        }
        tb.header(22.0, |mut header| {
            header.col(|ui| {
                ui.painter()
                    .rect_filled(ui.max_rect().expand(3.0), 0.0, style::FACE);
                cell_lines(ui);
                ui.label("#");
            });
            for c in columns {
                header.col(|ui| {
                    ui.painter()
                        .rect_filled(ui.max_rect().expand(3.0), 0.0, style::FACE);
                    cell_lines(ui);
                    ui.label(RichText::new(c).strong());
                });
            }
        })
        .body(|body| {
            body.rows(row_h, rows.len(), |mut row| {
                let r = row.index();
                let data = &rows[r];
                row.set_selected(state.selected == Some(r));
                row.col(|ui| {
                    ui.painter()
                        .rect_filled(ui.max_rect().expand(3.0), 0.0, style::FACE);
                    cell_lines(ui);
                    let resp = ui.add(
                        egui::Label::new(RichText::new((r + 1).to_string()).color(Color32::DARK_GRAY))
                            .sense(Sense::click()),
                    );
                    if resp.clicked() {
                        state.selected = Some(r);
                    }
                });
                for (c, val) in data.iter().enumerate() {
                    row.col(|ui| {
                        cell_lines(ui);
                        if let Some((er, ec, text, focused)) = state.editing.as_mut() {
                            if *er == r && *ec == c {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(text)
                                        .desired_width(f32::INFINITY)
                                        .margin(egui::vec2(2.0, 0.0)),
                                );
                                if !*focused {
                                    resp.request_focus();
                                    *focused = true;
                                }
                                if ui.input(|i| i.key_pressed(Key::Escape)) {
                                    cancel = true;
                                } else if resp.lost_focus() {
                                    commit = Some((r, c, Some(text.clone())));
                                }
                                return;
                            }
                        }
                        let resp = match val {
                            None => ui.add(
                                egui::Label::new(RichText::new("NULL").italics().color(style::NULL_TEXT))
                                    .selectable(false)
                                    .sense(Sense::click()),
                            ),
                            Some(v) => ui.add(
                                egui::Label::new(display(v))
                                    .selectable(false)
                                    .truncate()
                                    .sense(Sense::click()),
                            ),
                        };
                        if resp.clicked() {
                            state.selected = Some(r);
                        }
                        if resp.double_clicked() {
                            if editable {
                                state.editing =
                                    Some((r, c, val.clone().unwrap_or_default(), false));
                            } else {
                                events.push(GridEvent::Activate);
                            }
                        }
                        if let Some(v) = val {
                            if v.len() > 40 || v.contains('\n') {
                                resp.clone().on_hover_text(v.chars().take(2000).collect::<String>());
                            }
                        }
                        resp.context_menu(|ui| {
                            state.selected = Some(r);
                            if ui.button("Wert kopieren").clicked() {
                                ui.ctx().copy_text(val.clone().unwrap_or("NULL".into()));
                                ui.close();
                            }
                            if ui.button("Zeile kopieren (Tab-getrennt)").clicked() {
                                let line = data
                                    .iter()
                                    .map(|v| v.clone().unwrap_or("NULL".into()))
                                    .collect::<Vec<_>>()
                                    .join("\t");
                                ui.ctx().copy_text(line);
                                ui.close();
                            }
                            if editable {
                                ui.separator();
                                if ui.button("Bearbeiten").clicked() {
                                    state.editing =
                                        Some((r, c, val.clone().unwrap_or_default(), false));
                                    ui.close();
                                }
                                if ui.button("Auf NULL setzen").clicked() {
                                    events.push(GridEvent::Edit(r, c, None));
                                    ui.close();
                                }
                                if ui.button("Zeile löschen").clicked() {
                                    events.push(GridEvent::DeleteRow(r));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            });
        });
    });

    if cancel {
        state.editing = None;
    } else if let Some((r, c, v)) = commit {
        state.editing = None;
        if rows.get(r).and_then(|row| row.get(c)) != Some(&v) {
            events.push(GridEvent::Edit(r, c, v));
        }
    }
    events
}

/// Kopiert eine Ergebnismenge als Tab-getrennten Text (fuer Excel).
pub fn to_tsv(columns: &[String], rows: &[Row]) -> String {
    let mut s = columns.join("\t");
    s.push('\n');
    for r in rows {
        let line: Vec<String> = r
            .iter()
            .map(|v| v.clone().unwrap_or("NULL".into()).replace(['\t', '\n'], " "))
            .collect();
        s.push_str(&line.join("\t"));
        s.push('\n');
    }
    s
}

/// CSV (Semikolon-getrennt, wie im deutschen Excel)
pub fn to_csv(columns: &[String], rows: &[Row]) -> String {
    let esc = |v: &str| -> String {
        if v.contains([';', '"', '\n', '\r']) {
            format!("\"{}\"", v.replace('"', "\"\""))
        } else {
            v.to_string()
        }
    };
    let mut s = columns.iter().map(|c| esc(c)).collect::<Vec<_>>().join(";");
    s.push_str("\r\n");
    for r in rows {
        let line: Vec<String> = r
            .iter()
            .map(|v| v.as_deref().map(esc).unwrap_or_default())
            .collect();
        s.push_str(&line.join(";"));
        s.push_str("\r\n");
    }
    s
}
