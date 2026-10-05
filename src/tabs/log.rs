// Registerkarte "Server-Log": Meldungen von EasyMySQL und MariaDB, auf Wunsch alle Anweisungen.
// Das Log wird immer mitgeschrieben (auch wenn diese Registerkarte geschlossen ist).

use super::{Action, Ctx, TabView};
use crate::server::{LogKind, LogLine};
use crate::style;
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Server,
    Queries,
}

pub struct LogTab {
    filter: Filter,
    search: String,
    hide_internal: bool,
    show_old: bool,
    /// Zeilen bis zu diesem Zaehlerstand ausblenden ("Leeren")
    cleared_at: Option<usize>,
    cache: Vec<LogLine>,
    cache_count: Option<usize>,
    visible_rows: Vec<usize>,
    visible_for: Option<(usize, Filter, String, bool, bool, Option<usize>)>,
}

/// Anweisungen, die EasyMySQL selbst im Hintergrund sendet (Struktur lesen usw.)
fn internal(text: &str) -> bool {
    let u = text.to_ascii_uppercase();
    [
        "INFORMATION_SCHEMA",
        "SELECT DATABASE()",
        "SELECT VERSION()",
        "SELECT @@",
        "SHOW DATABASES",
        "SHOW CREATE TABLE",
        "SHOW VARIABLES",
        "SET NAMES",
        "SET GLOBAL GENERAL_LOG",
        "] CONNECT ",
        "] QUIT",
        "] INIT DB",
        "] PREPARE ",
        "] CLOSE STMT",
    ]
    .iter()
    .any(|k| u.contains(k))
}

impl LogTab {
    pub fn new() -> Self {
        Self { filter: Filter::All, search: String::new(), hide_internal: true, show_old: false, cleared_at: None, cache: Vec::new(), cache_count: None, visible_rows: Vec::new(), visible_for: None }
    }

    fn visible(&self, l: &LogLine) -> bool {
        let kind_ok = match self.filter {
            Filter::All => true,
            Filter::Server => l.kind != LogKind::Query,
            Filter::Queries => l.kind == LogKind::Query,
        };
        kind_ok
            && (self.show_old || !l.old)
            && !(self.hide_internal && l.kind == LogKind::Query && internal(&l.text))
            && (self.search.is_empty() || l.text.to_lowercase().contains(&self.search.to_lowercase()))
    }
}

impl TabView for LogTab {
    fn title(&self) -> String {
        crate::i18n::text("Server-Log").into()
    }

    fn key(&self) -> Option<String> {
        Some("log".into())
    }

    fn session(&self) -> Option<String> {
        Some("log".into())
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Ctx) {
        let pal = style::pal();
        let count = cx.server.log_count();
        if self.cache_count != Some(count) {
            self.cache = cx.server.log_lines();
            self.cache_count = Some(count);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(700));
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.filter, Filter::All, crate::i18n::text("Alles"));
            ui.selectable_value(&mut self.filter, Filter::Server, "Server");
            ui.selectable_value(&mut self.filter, Filter::Queries, crate::i18n::text("Anweisungen"));
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(crate::i18n::text("Filtern")).desired_width(180.0));
            ui.separator();
            let mut on = cx.settings.query_log;
            if ui
                .checkbox(&mut on, crate::i18n::text("Alle Anweisungen mitschreiben"))
                .on_hover_text(crate::i18n::text("Schreibt jede Anweisung an den Server mit – auch aus der Eingabeaufforderung (mysql) und anderen Programmen."))
                .changed()
            {
                cx.settings.query_log = on;
                cx.actions.push(Action::SettingsChanged);
                if on {
                    self.filter = Filter::Queries;
                }
            }
            ui.checkbox(&mut self.hide_internal, crate::i18n::text("Interne ausblenden"))
                .on_hover_text(crate::i18n::text("Anweisungen ausblenden, die EasyMySQL selbst sendet (Struktur lesen, Verbindungen)"));
            ui.checkbox(&mut self.show_old, crate::i18n::text("Frühere Sitzungen")).on_hover_text(crate::i18n::text("Meldungen vor dem letzten Start (aus server.log)"));
            ui.separator();
            if ui.button(crate::i18n::text("Kopieren")).clicked() {
                let text: Vec<String> = self.cache.iter().filter(|l| self.visible(l)).map(|l| format!("{}  {}", l.time, l.text)).collect();
                ui.ctx().copy_text(text.join("\n"));
            }
            if ui.button(crate::i18n::text("Leeren")).on_hover_text(crate::i18n::text("Nur die Anzeige leeren; die Datei bleibt erhalten")).clicked() {
                self.cleared_at = Some(self.cache.len());
            }
            ui.menu_button(crate::i18n::text("Dateien"), |ui| {
                if ui.button(crate::i18n::text("server.log öffnen")).clicked() {
                    crate::app::open_path(&crate::server::log_file());
                    ui.close();
                }
                if ui.button(crate::i18n::text("abfragen.log öffnen")).clicked() {
                    crate::app::open_path(&crate::server::query_log_file());
                    ui.close();
                }
                if let Some(p) = &cx.server.paths {
                    ui.separator();
                    if ui.button(crate::i18n::text("Datenordner öffnen")).clicked() {
                        crate::app::open_path(&p.data);
                        ui.close();
                    }
                }
            });
        });
        if cx.server.paths.is_none() {
            ui.label(RichText::new(crate::i18n::text("MariaDB wurde nicht gefunden.")).color(pal.error_text));
        }
        let start = self.cleared_at.unwrap_or(0).min(self.cache.len());
        let filter_key = (count, self.filter, self.search.clone(), self.hide_internal, self.show_old, self.cleared_at);
        if self.visible_for.as_ref() != Some(&filter_key) {
            self.visible_rows = (start..self.cache.len()).filter(|&index| self.visible(&self.cache[index])).collect();
            self.visible_for = Some(filter_key);
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        style::sunken_frame().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::both().stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_h, self.visible_rows.len(), |ui, range| {
                for &index in &self.visible_rows[range] {
                    let l = &self.cache[index];
                    let low = l.text.to_lowercase();
                    let color = if low.contains("[error]") || low.contains("fehler") || low.contains("error ") || low.contains("error:") {
                        pal.error_text
                    } else if low.contains("[warning]") || low.contains("hinweis") || low.contains("hint:") {
                        style::pal().syn_function
                    } else if l.kind == LogKind::Query {
                        pal.syn_keyword
                    } else if l.old {
                        pal.text_weak
                    } else {
                        pal.text
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&l.time[l.time.len().saturating_sub(8)..]).monospace().color(pal.text_weak))
                            .on_hover_text(&l.time);
                        ui.label(RichText::new(&l.text).monospace().color(color));
                    });
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_queries() {
        assert!(internal("[5] SELECT TABLE_NAME FROM information_schema.TABLES"));
        assert!(internal("[5] Connect root@localhost on  using TCP/IP"));
        assert!(!internal("[5] SELECT * FROM kunde"));
    }
}
