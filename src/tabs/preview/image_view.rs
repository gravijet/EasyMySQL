// Bildansicht: Dekodieren im Hintergrund, Zoom mit Mausrad oder Knoepfen, Verschieben durch Ziehen.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

/// Laengste Seite der Textur (groessere Bilder werden fuer die Anzeige verkleinert)
const MAX_SIDE: u32 = 4096;
/// Kleine Vorschauversion fuer glatte Verkleinerung (Texturen werden ohne Mipmaps gezeichnet)
const THUMB_SIDE: u32 = 2048;

struct Decoded {
    orig: [u32; 2],
    full: ColorImage,
    thumb: Option<ColorImage>,
    alpha: bool,
    format: String,
}

pub struct ImageView {
    rx: Option<Receiver<Result<Decoded, String>>>,
    full: Option<TextureHandle>,
    thumb: Option<TextureHandle>,
    checker: Option<TextureHandle>,
    orig: Vec2,
    alpha: bool,
    format: String,
    zoom: f32,
    fit: bool,
    pan: Vec2,
    error: Option<String>,
    path: PathBuf,
}

fn decode(path: &Path) -> Result<Decoded, String> {
    use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};
    let reader = ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let format = reader.format().map(|f| format!("{f:?}").to_uppercase()).unwrap_or_default();
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let alpha = img.color().has_alpha();
    let orig = [img.width(), img.height()];
    let to_color = |i: &DynamicImage| {
        let rgba = i.to_rgba8();
        ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
    };
    let long = orig[0].max(orig[1]);
    let full_img = if long > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle) } else { img };
    let thumb = (full_img.width().max(full_img.height()) > THUMB_SIDE * 13 / 10).then(|| to_color(&full_img.resize(THUMB_SIDE, THUMB_SIDE, FilterType::Triangle)));
    Ok(Decoded { orig, full: to_color(&full_img), thumb, alpha, format })
}

impl ImageView {
    pub fn new(ctx: &egui::Context, path: &Path) -> Self {
        let (tx, rx) = channel();
        let p = path.to_path_buf();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(|| decode(&p)).unwrap_or_else(|_| Err(crate::i18n::text("Das Bild konnte nicht gelesen werden.").to_string()));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        Self { rx: Some(rx), full: None, thumb: None, checker: None, orig: Vec2::ZERO, alpha: false, format: String::new(), zoom: 1.0, fit: true, pan: Vec2::ZERO, error: None, path: path.to_path_buf() }
    }

    pub fn info(&self) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        if self.rx.is_some() {
            return crate::i18n::text("Wird gelesen …").to_string();
        }
        format!("{} × {} px  ·  {}", self.orig.x, self.orig.y, self.format)
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(Ok(d)) => {
                // Kleine Bilder (Symbole, Pixelgrafik) ohne Glaettung zeigen
                let opts = if d.orig[0].max(d.orig[1]) <= 256 { TextureOptions::NEAREST } else { TextureOptions::LINEAR };
                self.full = Some(ctx.load_texture(format!("img-{}", self.path.display()), d.full, opts));
                self.thumb = d.thumb.map(|t| ctx.load_texture(format!("img-thumb-{}", self.path.display()), t, TextureOptions::LINEAR));
                self.orig = vec2(d.orig[0] as f32, d.orig[1] as f32);
                self.alpha = d.alpha;
                self.format = d.format;
                self.rx = None;
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(60)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.error = Some(crate::i18n::text("Das Bild konnte nicht gelesen werden.").to_string());
                self.rx = None;
            }
        }
    }

    /// Faktor 0 = 100 %
    pub fn zoom_by(&mut self, factor: f32) {
        self.fit = false;
        if factor == 0.0 {
            self.zoom = 1.0;
            self.pan = Vec2::ZERO;
        } else {
            self.zoom = (self.zoom * factor).clamp(0.02, 32.0);
        }
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        if icons::button(ui, Icon::ZoomIn, crate::i18n::text("Vergrößern")).clicked() {
            self.zoom_by(1.25);
        }
        ui.label(egui::RichText::new(format!("{:.0} %", self.zoom * 100.0)).color(style::pal().text_weak));
        if icons::button(ui, Icon::ZoomOut, crate::i18n::text("Verkleinern")).clicked() {
            self.zoom_by(0.8);
        }
        if icons::button(ui, Icon::Fit, crate::i18n::text("Einpassen")).clicked() {
            self.fit = true;
            self.pan = Vec2::ZERO;
        }
        if ui.small_button("1:1").on_hover_text(crate::i18n::text("Originalgröße")).clicked() {
            self.zoom_by(0.0);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let pal = style::pal();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, if pal.dark { Color32::from_rgb(0x1A, 0x1A, 0x1A) } else { Color32::from_rgb(0xE8, 0xE8, 0xE8) });
        if let Some(e) = &self.error {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), pal.error_text);
            return;
        }
        let Some(full) = self.full.clone() else {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, crate::i18n::text("Wird gelesen …"), egui::FontId::proportional(14.0), pal.text_weak);
            return;
        };
        // Einpassen: nie groesser als das Original
        let fit_zoom = ((rect.width() - 24.0) / self.orig.x).min((rect.height() - 24.0) / self.orig.y).clamp(0.001, 1.0);
        if self.fit {
            self.zoom = fit_zoom;
            self.pan = Vec2::ZERO;
        }
        // Mausrad (und Strg + Mausrad): um den Mauszeiger zoomen
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.0015).exp();
            if (factor - 1.0).abs() > 1e-4
                && let Some(p) = ctx.pointer_hover_pos()
            {
                let old = self.zoom;
                self.zoom = (old * factor).clamp(0.02, 32.0);
                self.fit = false;
                let center = rect.center() + self.pan;
                let new_center = p - (p - center) * (self.zoom / old);
                self.pan = new_center - rect.center();
            }
        }
        if resp.dragged() {
            self.pan += resp.drag_delta();
            self.fit = false;
        }
        if resp.double_clicked() {
            if self.fit {
                self.zoom_by(0.0);
            } else {
                self.fit = true;
            }
        }
        // Das Bild darf nicht ganz aus dem Fenster verschwinden
        let size = self.orig * self.zoom;
        let limit = (rect.size() + size) / 2.0 - vec2(40.0, 40.0);
        self.pan = self.pan.clamp(-limit.max(Vec2::ZERO), limit.max(Vec2::ZERO));
        let img_rect = Rect::from_center_size(rect.center() + self.pan, size);
        if self.alpha {
            let checker = self.checker.get_or_insert_with(|| {
                let (a, b) = if pal.dark { (Color32::from_gray(0x55), Color32::from_gray(0x44)) } else { (Color32::from_gray(0xFF), Color32::from_gray(0xE0)) };
                ctx.load_texture("checker", ColorImage::new([2, 2], vec![a, b, b, a]), TextureOptions::NEAREST_REPEAT)
            });
            let visible = img_rect.intersect(rect);
            // Jedes Feld ist 8 Punkte gross
            let uv = Rect::from_min_max(
                pos2((visible.left() - img_rect.left()) / 16.0, (visible.top() - img_rect.top()) / 16.0),
                pos2((visible.right() - img_rect.left()) / 16.0, (visible.bottom() - img_rect.top()) / 16.0),
            );
            let mut mesh = egui::Mesh::with_texture(checker.id());
            mesh.add_rect_with_uv(visible, uv, Color32::WHITE);
            painter.add(mesh);
        }
        // Verkleinert gezeichnet: die vorgefilterte kleine Fassung nehmen (glatter als die grosse)
        let shown_long = size.x.max(size.y) * ctx.pixels_per_point();
        let tex = match &self.thumb {
            Some(t) if shown_long <= THUMB_SIDE as f32 => t,
            _ => &full,
        };
        painter.image(tex.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        painter.rect_stroke(img_rect, 0.0, egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Outside);
        if resp.hovered() {
            ctx.set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_png_and_downsizes_huge_images() {
        let dir = std::env::temp_dir().join(format!("easymysql-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 128])).save(&small).unwrap();
        let d = decode(&small).unwrap();
        assert_eq!(d.orig, [3, 2]);
        assert!(d.alpha && d.thumb.is_none());
        assert_eq!(d.full.size, [3, 2]);
        let big = dir.join("big.png");
        image::RgbImage::from_pixel(6000, 100, image::Rgb([1, 2, 3])).save(&big).unwrap();
        let d = decode(&big).unwrap();
        assert_eq!(d.orig, [6000, 100]);
        assert_eq!(d.full.size[0], MAX_SIDE as usize);
        assert!(d.thumb.is_some_and(|t| t.size[0] == THUMB_SIDE as usize));
        assert!(decode(&dir.join("missing.png")).is_err());
        std::fs::write(dir.join("broken.png"), b"\x89PNG\r\n\x1a\nnot really").unwrap();
        assert!(decode(&dir.join("broken.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
