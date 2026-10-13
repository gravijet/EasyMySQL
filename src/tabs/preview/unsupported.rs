// Dateien, die sich nicht anzeigen lassen (kein Text, Bild oder PDF): nur ein Hinweis mit der erkannten Dateiart.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, RichText};
use std::io::Read;
use std::path::Path;

pub struct Unsupported {
    kind: Option<&'static str>,
}

/// Bekannte Dateianfaenge
const SIGNATURES: &[(&[u8], &str)] = &[
    (b"PK\x03\x04", "ZIP-Archiv (auch Word, Excel, JAR)"),
    (b"MZ", "Windows-Programm oder Bibliothek"),
    (b"\x7fELF", "ELF-Programm"),
    (b"7z\xBC\xAF\x27\x1C", "7-Zip-Archiv"),
    (b"Rar!", "RAR-Archiv"),
    (&[0x1F, 0x8B], "gzip-Archiv"),
    (b"SQLite format 3\0", "SQLite-Datenbank"),
    (&[0xD0, 0xCF, 0x11, 0xE0], "Altes Office-Dokument (doc, xls)"),
    (b"ID3", "MP3-Audio"),
    (b"OggS", "Ogg-Audio/Video"),
    (b"fLaC", "FLAC-Audio"),
    (b"\xFF\xFB", "MP3-Audio"),
    (b"MThd", "MIDI-Datei"),
];

fn guess_kind(head: &[u8]) -> Option<&'static str> {
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        return Some("WAV-Audio");
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return Some("MP4/MOV-Video");
    }
    SIGNATURES.iter().find(|(m, _)| head.starts_with(m)).map(|(_, n)| *n)
}

impl Unsupported {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut head = Vec::new();
        file.take(16).read_to_end(&mut head).map_err(|e| e.to_string())?;
        Ok(Self { kind: guess_kind(&head) })
    }

    pub fn info(&self) -> String {
        self.kind.map(|k| crate::i18n::text(k).to_string()).unwrap_or_else(|| crate::i18n::text("Binärdatei").to_string())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, path: &Path) {
        let pal = style::pal();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(24.0));
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
            icons::paint(ui.painter(), r, Icon::File, pal.text_weak);
            ui.add_space(8.0);
            ui.label(RichText::new(crate::workspace::dir_name(path)).strong());
            ui.label(RichText::new(crate::i18n::text("Diese Datei kann nicht angezeigt werden.")).color(pal.text_weak));
            ui.add_space(8.0);
            if icons::text_button(ui, Icon::External, crate::i18n::text("Mit dem Standardprogramm öffnen")).clicked() {
                crate::app::open_path(path);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(guess_kind(b"PK\x03\x04abc"), Some("ZIP-Archiv (auch Word, Excel, JAR)"));
        assert_eq!(guess_kind(b"RIFF\0\0\0\0WAVEfmt "), Some("WAV-Audio"));
        assert_eq!(guess_kind(b"\0\0\0\x18ftypmp42"), Some("MP4/MOV-Video"));
        assert_eq!(guess_kind(b"zzzz"), None);
    }
}
