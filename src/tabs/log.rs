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
        Self { filter: Filter::All, search: String::new(), hide_internal: true, show_old: false, cleared_at: None, cache: Vec::new(), cache_count: None }
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
        "Server-Log".into()
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
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.filter, Filter::All, "Alles");
            ui.selectable_value(&mut self.filter, Filter::Server, "Server");
            ui.selectable_value(&mut self.filter, Filter::Queries, "Anweisungen");
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Filtern").desired_width(180.0));
            ui.separator();
            let mut on = cx.settings.query_log;
            if ui
                .checkbox(&mut on, "Alle Anweisungen mitschreiben")
                .on_hover_text("Schreibt jede Anweisung an den Server mit – auch aus der Eingabeaufforderung (mysql) und anderen Programmen.")
                .changed()
            {
                cx.settings.query_log = on;
                cx.actions.push(Action::SettingsChanged);
                if on {
                    self.filter = Filter::Queries;
                }
            }
            ui.checkbox(&mut self.hide_internal, "Interne ausblenden")
                .on_hover_text("Anweisungen ausblenden, die EasyMySQL selbst sendet (Struktur lesen, Verbindungen)");
            ui.checkbox(&mut self.show_old, "Frühere Sitzungen");
            ui.separator();
            if ui.button("Kopieren").clicked() {
                let text: Vec<String> = self.cache.iter().filter(|l| self.visible(l)).map(|l| format!("{}  {}", l.time, l.text)).collect();
                ui.ctx().copy_text(text.join("\n"));
            }
            if ui.button("Leeren").on_hover_text("Nur die Anzeige leeren; die Datei bleibt erhalten").clicked() {
                self.cleared_at = Some(self.cache.len());
            }
            ui.menu_button("Dateien", |ui| {
                if ui.button("server.log öffnen").clicked() {
                    crate::app::open_path(&crate::server::log_file());
                    ui.close();
                }
                if ui.button("abfragen.log öffnen").clicked() {
                    crate::app::open_path(&crate::server::query_log_file());
                    ui.close();
                }
                if let Some(p) = &cx.server.paths {
                    ui.separator();
                    if ui.button("Datenordner öffnen").clicked() {
                        crate::app::open_path(&p.data);
                        ui.close();
                    }
                }
            });
        });
        if cx.server.paths.is_none() {
            ui.label(RichText::new("MariaDB wurde nicht gefunden.").color(pal.error_text));
        }
        let start = self.cleared_at.unwrap_or(0).min(self.cache.len());
        let lines: Vec<&LogLine> = self.cache[start..].iter().filter(|l| self.visible(l)).collect();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        style::sunken_frame().show(ui, |ui| {
            egui::ScrollArea::both().stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_h, lines.len(), |ui, range| {
                for l in &lines[range] {
                    let low = l.text.to_lowercase();
                    let color = if low.contains("[error]") || low.contains("fehler") || low.contains("error ") {
                        pal.error_text
                    } else if low.contains("[warning]") || low.contains("hinweis") {
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
