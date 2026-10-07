// EasyMySQL – MariaDB-Server und grafische Datenbankverwaltung fuer Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod backup;
mod repair;
mod db;
mod grid;
mod icon;
mod i18n;
mod icons;
mod keymap;
mod platform;
mod qhistory;
mod server;
mod settings;
mod sqledit;
mod sqlfmt;
mod style;
mod updates;
mod tabs;
mod vscode;
mod workspace;

use eframe::egui;

fn main() -> eframe::Result {
    if !platform::single_instance() {
        return Ok(());
    }
    // Die Oberfläche braucht keine Hochleistungs-Grafikkarte: Die integrierte GPU startet
    // schneller und spart Strom. Wer WGPU_POWER_PREF selbst setzt, behält seine Wahl.
    #[cfg(windows)]
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        // SAFETY: Es läuft noch kein weiterer Thread.
        unsafe { std::env::set_var("WGPU_POWER_PREF", "low") };
    }
    let icon = egui::IconData {
        rgba: icon::icon_rgba(64),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EasyMySQL")
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(icon),
        ..Default::default()
    };
    #[cfg(windows)]
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..options
    };
    eframe::run_native(
        "EasyMySQL",
        options,
        Box::new(|cc| Ok(Box::new(app::EasyApp::new(cc)))),
    )
}
