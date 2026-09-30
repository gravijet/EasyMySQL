// Registerkarte "Hilfe": Handbuch mit Inhaltsverzeichnis und Suche.

use super::{Ctx, TabView};
use eframe::egui;

pub struct HelpTab {
    goto: Option<String>,
}

impl HelpTab {
    pub fn new() -> Self {
        Self { goto: None }
    }
}

impl TabView for HelpTab {
    fn title(&self) -> String {
        "Hilfe".into()
    }
    fn key(&self) -> Option<String> {
        Some("help".into())
    }
    fn session(&self) -> Option<String> {
        Some("help".into())
    }
    fn show_section(&mut self, id: &str) {
        self.goto = Some(id.into());
    }
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Ctx) {
        ui.label("Hilfe");
    }
}
