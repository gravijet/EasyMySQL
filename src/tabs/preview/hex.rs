// Hexdarstellung fuer Binaerdateien (die ersten Megabyte), mit Erkennung bekannter Dateiarten.

use crate::style;
use eframe::egui::{self, FontId, RichText};
use std::io::Read;
use std::path::Path;

/// So viel der Datei wird angezeigt
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const PER_ROW: usize = 16;

pub struct HexView {
    data: Vec<u8>,
    total: u64,
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

impl HexView {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let total = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut data = Vec::new();
        file.take(MAX_BYTES).read_to_end(&mut data).map_err(|e| e.to_string())?;
        let kind = guess_kind(&data);
        Ok(Self { data, total, kind })
    }

    pub fn info(&self) -> String {
        let kind = self.kind.map(|k| crate::i18n::text(k).to_string()).unwrap_or_else(|| crate::i18n::text("Binärdatei").to_string());
        if self.total > self.data.len() as u64 {
            crate::tr_format!("{kind}  ·  die ersten {} MB", "{kind}  ·  first {} MB", MAX_BYTES / (1024 * 1024))
        } else {
            kind
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, path: &Path) {
        let pal = style::pal();
        ui.label(RichText::new(crate::tr_format!("„{}“ ist keine Textdatei und lässt sich hier nur als Bytes ansehen.", "“{}” is not a text file and can only be viewed as bytes here.", crate::workspace::dir_name(path))).color(pal.text_weak));
        ui.add_space(4.0);
        let font = FontId::monospace(crate::sqledit::font_size());
        let row_h = ui.ctx().fonts_mut(|f| f.row_height(&font));
        let rows = self.data.len().div_ceil(PER_ROW);
        crate::tabs::page_keys(ui);
        style::sunken_frame().show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, row_h, rows, |ui, range| {
                for r in range {
                    let from = r * PER_ROW;
                    let chunk = &self.data[from..(from + PER_ROW).min(self.data.len())];
                    let hex: Vec<String> = (0..PER_ROW).map(|i| chunk.get(i).map_or("  ".to_string(), |b| format!("{b:02X}"))).collect();
                    let ascii: String = chunk.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '·' }).collect();
                    let line = format!("{from:08X}  {}  {}  {ascii}", hex[..8].join(" "), hex[8..].join(" "));
                    ui.label(RichText::new(line).font(font.clone()).color(pal.text));
                }
            });
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
