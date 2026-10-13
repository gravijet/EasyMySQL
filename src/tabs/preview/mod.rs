// Vorschau fuer Dateien, die kein Text sind: Bilder, PDF-Dokumente und andere Dateien (nur ein Hinweis).

mod image_view;
mod pdf;
mod unsupported;

use super::{Ctx, TabView};
use crate::icons::{self, Icon};
use crate::keymap::Cmd;
use crate::style;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Image,
    Pdf,
    Binary,
}

/// Dateiart anhand des Dateianfangs (Magic Bytes); ohne Nullbytes gilt eine Datei als Text.
pub fn detect(path: &Path, head: &[u8]) -> Kind {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if head.starts_with(b"%PDF") {
        return Kind::Pdf;
    }
    let image = head.starts_with(b"\x89PNG\r\n\x1a\n")
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || (head.len() > 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
        || (head.starts_with(b"BM") && matches!(ext.as_str(), "bmp" | "dib"))
        || (head.starts_with(&[0, 0, 1, 0]) && matches!(ext.as_str(), "ico" | "cur"));
    if image {
        return Kind::Image;
    }
    // Text in UTF-16 (Windows-PowerShell-Ausgaben) enthaelt Nullbytes, hat aber eine Kennung
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return Kind::Text;
    }
    if head.contains(&0) { Kind::Binary } else { Kind::Text }
}

/// Dateien, die in der Schnellsuche angeboten werden sollen, obwohl sie kein Text sind
pub fn previewable_name(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "jfif" | "gif" | "bmp" | "webp" | "ico" | "pdf")
}

enum Body {
    Image(image_view::ImageView),
    Pdf(pdf::PdfView),
    Other(unsupported::Unsupported),
}

pub struct PreviewTab {
    path: PathBuf,
    body: Body,
    size: u64,
}

impl PreviewTab {
    pub fn open(ctx: &egui::Context, path: &Path, kind: Kind) -> Result<Self, String> {
        let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
        let body = match kind {
            Kind::Image => Body::Image(image_view::ImageView::new(ctx, path)),
            Kind::Pdf => Body::Pdf(pdf::PdfView::new(ctx, path)),
            _ => Body::Other(unsupported::Unsupported::load(path)?),
        };
        Ok(Self { path: path.to_path_buf(), body, size })
    }

    fn name(&self) -> String {
        crate::workspace::dir_name(&self.path)
    }
}

/// Dateigroesse lesbar
fn human_size(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

impl TabView for PreviewTab {
    fn title(&self) -> String {
        self.name()
    }

    fn key(&self) -> Option<String> {
        Some(format!("file:{}", self.path.to_string_lossy().to_lowercase()))
    }

    fn session(&self) -> Option<String> {
        Some(format!("preview\t{}", self.path.display()))
    }

    fn file(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn icon(&self) -> Option<Icon> {
        Some(match self.body {
            Body::Image(_) => Icon::FileImage,
            Body::Pdf(_) => Icon::FileText,
            Body::Other(_) => Icon::File,
        })
    }

    fn file_renamed(&mut self, old: &Path, new: &Path) {
        if let Ok(rest) = self.path.strip_prefix(old) {
            self.path = if rest.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rest) };
        }
    }

    /// Strg + / Strg - zoomen das Bild bzw. die Seite statt der Editorschrift
    fn command(&mut self, cmd: Cmd, _cx: &mut Ctx) -> bool {
        let factor = match cmd {
            Cmd::FontLarger => 1.25,
            Cmd::FontSmaller => 0.8,
            Cmd::FontReset => 0.0,
            _ => return false,
        };
        match &mut self.body {
            Body::Image(v) => v.zoom_by(factor),
            Body::Pdf(v) => v.zoom_by(factor),
            Body::Other(_) => return false,
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Ctx) {
        let pal = style::pal();
        // Kopfzeile: Angaben, Zoom, Oeffnen
        ui.horizontal(|ui| {
            let info = match &self.body {
                Body::Image(v) => v.info(),
                Body::Pdf(v) => v.info(),
                Body::Other(v) => v.info(),
            };
            ui.label(RichText::new(format!("{}  ·  {info}", human_size(self.size))).color(pal.text_weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Folder, crate::i18n::text("Im Datei-Explorer zeigen")).clicked()
                    && let Some(d) = self.path.parent()
                {
                    crate::app::open_path(d);
                }
                if icons::button(ui, Icon::External, crate::i18n::text("Mit dem Standardprogramm öffnen")).clicked() {
                    crate::app::open_path(&self.path);
                }
                ui.separator();
                match &mut self.body {
                    Body::Image(v) => v.toolbar(ui),
                    Body::Pdf(v) => v.toolbar(ui),
                    Body::Other(_) => {}
                }
            });
        });
        ui.add_space(2.0);
        match &mut self.body {
            Body::Image(v) => v.ui(ui),
            Body::Pdf(v) => v.ui(ui),
            Body::Other(v) => v.ui(ui, &self.path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_magic_bytes() {
        let p = Path::new("x");
        assert_eq!(detect(p, b"%PDF-1.7 ..."), Kind::Pdf);
        assert_eq!(detect(p, b"\x89PNG\r\n\x1a\n\0\0"), Kind::Image);
        assert_eq!(detect(p, &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]), Kind::Image);
        assert_eq!(detect(p, b"GIF89a\x01\x00"), Kind::Image);
        assert_eq!(detect(p, b"RIFF\x00\x00\x00\x00WEBPVP8 "), Kind::Image);
        // BMP und ICO brauchen die passende Endung, sonst koennte "BM..." ein Text sein
        assert_eq!(detect(p, b"BMW ist ein Auto"), Kind::Text);
        assert_eq!(detect(Path::new("a.bmp"), b"BM\x36\0\0\0"), Kind::Image);
        assert_eq!(detect(Path::new("a.ico"), &[0, 0, 1, 0, 1, 0]), Kind::Image);
        assert_eq!(detect(p, b"SELECT 1;\n"), Kind::Text);
        assert_eq!(detect(p, b"PK\x03\x04\0\0"), Kind::Binary);
        assert_eq!(detect(p, &[0xFF, 0xFE, b'S', 0]), Kind::Text);
    }

    #[test]
    fn sizes_and_names() {
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert!(previewable_name("Foto.JPG") && previewable_name("a.pdf") && !previewable_name("a.sql"));
    }
}
