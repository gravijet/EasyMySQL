// PDF-Ansicht: Die Seiten werden von hayro (reines Rust) im Hintergrund gezeichnet, und zwar nur die, die gerade
// sichtbar sind. Seiten untereinander mit Bildlauf, Zoom mit Strg + Mausrad oder Knoepfen.

use crate::icons::{self, Icon};
use crate::style;
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, pos2, vec2};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

/// 1 Punkt der PDF-Seite = 4/3 Bildschirmpunkte bei 100 %
const POINT: f32 = 96.0 / 72.0;
/// Laengste Seite einer gezeichneten Seite in Pixeln
const MAX_PIXELS: f32 = 4096.0;
/// So viele Seitenbilder bleiben im Speicher
const KEEP: usize = 8;
const GAP: f32 = 12.0;

enum Req {
    Render { page: usize, scale: f32 },
}

enum Resp {
    Opened(Vec<(f32, f32)>),
    Failed(String),
    Page { page: usize, scale: f32, image: ColorImage },
}

pub struct PdfView {
    tx: Sender<Req>,
    rx: Receiver<Resp>,
    pages: Vec<(f32, f32)>,
    error: Option<String>,
    loading: bool,
    zoom: f32,
    fit: bool,
    textures: HashMap<usize, (TextureHandle, f32)>,
    pending: HashMap<usize, f32>,
    current: usize,
    goto: Option<usize>,
    /// Scrollstand nach einer Zoomaenderung (damit die Stelle stehen bleibt)
    set_offset: Option<f32>,
    scroll_y: f32,
}

fn worker(path: std::path::PathBuf, rx: Receiver<Req>, tx: Sender<Resp>, ctx: egui::Context) {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    let run = std::panic::AssertUnwindSafe(|| -> Result<(), String> {
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        let pdf = Pdf::new(data).map_err(|e| {
            let text = format!("{e:?}");
            if text.contains("ncrypt") { crate::i18n::text("Passwortgeschützte PDF-Dateien werden nicht unterstützt.").to_string() } else { crate::tr_format!("Die PDF-Datei konnte nicht gelesen werden ({text}).", "The PDF file could not be read ({text}).") }
        })?;
        let pages = pdf.pages();
        let sizes: Vec<(f32, f32)> = pages.iter().map(|p| p.render_dimensions()).collect();
        let _ = tx.send(Resp::Opened(sizes));
        ctx.request_repaint();
        let cache = RenderCache::new();
        let settings = InterpreterSettings::default();
        while let Ok(Req::Render { page, scale }) = rx.recv() {
            let Some(p) = pages.get(page) else { continue };
            // Mehrere Anfragen fuer dieselbe Seite: nur die neueste zeichnen
            let (mut page, mut scale, mut p) = (page, scale, p);
            while let Ok(Req::Render { page: np, scale: ns }) = rx.try_recv() {
                if let Some(q) = pages.get(np) {
                    (page, scale, p) = (np, ns, q);
                }
            }
            let pixmap = render(p, &cache, &settings, &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
            let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
            let image = ColorImage::from_rgba_premultiplied([w, h], pixmap.data_as_u8_slice());
            if tx.send(Resp::Page { page, scale, image }).is_err() {
                break;
            }
            ctx.request_repaint();
        }
        Ok(())
    });
    match std::panic::catch_unwind(run) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = tx.send(Resp::Failed(e));
        }
        Err(_) => {
            let _ = tx.send(Resp::Failed(crate::i18n::text("Die PDF-Datei lässt sich nicht darstellen.").to_string()));
        }
    }
    ctx.request_repaint();
}

impl PdfView {
    pub fn new(ctx: &egui::Context, path: &Path) -> Self {
        let (req_tx, req_rx) = channel();
        let (resp_tx, resp_rx) = channel();
        let (p, c) = (path.to_path_buf(), ctx.clone());
        std::thread::spawn(move || worker(p, req_rx, resp_tx, c));
        Self {
            tx: req_tx,
            rx: resp_rx,
            pages: Vec::new(),
            error: None,
            loading: true,
            zoom: 1.0,
            fit: true,
            textures: HashMap::new(),
            pending: HashMap::new(),
            current: 0,
            goto: None,
            set_offset: None,
            scroll_y: 0.0,
        }
    }

    pub fn info(&self) -> String {
        if let Some(e) = &self.error {
            return e.clone();
        }
        if self.loading {
            return crate::i18n::text("Wird gelesen …").to_string();
        }
        crate::tr_format!("{} Seite(n)", "{} page(s)", self.pages.len())
    }

    /// Faktor 0 = Seitenbreite einpassen
    pub fn zoom_by(&mut self, factor: f32) {
        let old = self.zoom;
        if factor == 0.0 {
            self.fit = true;
            return;
        }
        self.fit = false;
        self.zoom = (self.zoom * factor).clamp(0.1, 8.0);
        self.set_offset = Some(self.scroll_y * self.zoom / old);
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        if icons::button(ui, Icon::ZoomIn, crate::i18n::text("Vergrößern")).clicked() {
            self.zoom_by(1.25);
        }
        ui.label(egui::RichText::new(format!("{:.0} %", self.zoom * 100.0)).color(style::pal().text_weak));
        if icons::button(ui, Icon::ZoomOut, crate::i18n::text("Verkleinern")).clicked() {
            self.zoom_by(0.8);
        }
        if icons::button(ui, Icon::Fit, crate::i18n::text("Seitenbreite")).clicked() {
            self.zoom_by(0.0);
        }
        ui.separator();
        let n = self.pages.len();
        if icons::button(ui, Icon::Down, crate::i18n::text("Nächste Seite")).clicked() && self.current + 1 < n {
            self.goto = Some(self.current + 1);
        }
        ui.label(egui::RichText::new(format!("{} / {n}", self.current + 1)).color(style::pal().text_weak));
        if icons::button(ui, Icon::Up, crate::i18n::text("Vorherige Seite")).clicked() && self.current > 0 {
            self.goto = Some(self.current - 1);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Resp::Opened(p) => {
                    self.pages = p;
                    self.loading = false;
                }
                Resp::Failed(e) => {
                    self.error = Some(e);
                    self.loading = false;
                }
                Resp::Page { page, scale, image } => {
                    self.pending.remove(&page);
                    let tex = ctx.load_texture(format!("pdf-page-{page}"), image, TextureOptions::LINEAR);
                    self.textures.insert(page, (tex, scale));
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let pal = style::pal();
        let avail = ui.available_rect_before_wrap();
        if let Some(e) = &self.error {
            ui.painter().text(avail.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), pal.error_text);
            return;
        }
        if self.pages.is_empty() {
            ui.painter().text(avail.center(), egui::Align2::CENTER_CENTER, crate::i18n::text("Wird gelesen …"), egui::FontId::proportional(14.0), pal.text_weak);
            return;
        }
        let max_w = self.pages.iter().map(|p| p.0).fold(1.0, f32::max);
        if self.fit {
            self.zoom = ((avail.width() - 2.0 * GAP - 16.0) / (max_w * POINT)).clamp(0.1, 8.0);
        }
        // Strg + Mausrad zoomt, normales Mausrad blaettert
        if ui.rect_contains_pointer(avail) {
            let pinch = ui.input(|i| i.zoom_delta());
            if (pinch - 1.0).abs() > 1e-3 {
                self.zoom_by(pinch);
            }
        }
        // Tastatur
        if !ctx.egui_wants_keyboard_input() && ui.rect_contains_pointer(avail) {
            let n = egui::Modifiers::NONE;
            let (home, end) = ctx.input_mut(|i| (i.consume_key(n, egui::Key::Home), i.consume_key(n, egui::Key::End)));
            if home {
                self.goto = Some(0);
            }
            if end {
                self.goto = Some(self.pages.len() - 1);
            }
            crate::tabs::page_keys(ui);
        }
        let z = self.zoom * POINT;
        let heights: Vec<f32> = self.pages.iter().map(|p| p.1 * z).collect();
        let total_h = heights.iter().sum::<f32>() + GAP * (heights.len() as f32 + 1.0);
        let content_w = (max_w * z + 2.0 * GAP).max(avail.width() - 2.0);
        let mut area = egui::ScrollArea::both().auto_shrink([false, false]).id_salt("pdf-scroll");
        if let Some(y) = self.set_offset.take() {
            area = area.vertical_scroll_offset(y);
        }
        let ppp = ctx.pixels_per_point();
        let mut wanted: Vec<(usize, f32)> = Vec::new();
        let out = area.show_viewport(ui, |ui, viewport| {
            let (space, _) = ui.allocate_exact_size(vec2(content_w, total_h), egui::Sense::hover());
            let origin = space.min;
            let mut y = GAP;
            let near = viewport.expand2(vec2(0.0, viewport.height() * 0.5));
            for (i, &(w, _)) in self.pages.iter().enumerate() {
                let size = vec2(w * z, heights[i]);
                let local = Rect::from_min_size(pos2((content_w - size.x) / 2.0, y), size);
                let r = local.translate(origin.to_vec2());
                if self.goto == Some(i) {
                    ui.scroll_to_rect(r.expand(GAP), Some(egui::Align::TOP));
                    self.goto = None;
                }
                if local.intersects(viewport) {
                    let center = viewport.center().y;
                    if local.top() <= center && center <= local.bottom() + GAP {
                        self.current = i;
                    }
                }
                if local.intersects(near) {
                    let painter = ui.painter();
                    painter.rect_filled(r.translate(vec2(2.0, 2.0)), 0.0, Color32::from_black_alpha(60));
                    match self.textures.get(&i) {
                        Some((tex, _)) => {
                            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            painter.rect_filled(r, 0.0, Color32::WHITE);
                            painter.text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(18.0), Color32::from_gray(0xB0));
                        }
                    }
                    let mut scale = z * ppp;
                    let long = (w.max(self.pages[i].1)) * scale;
                    if long > MAX_PIXELS {
                        scale *= MAX_PIXELS / long;
                    }
                    wanted.push((i, scale));
                }
                y += size.y + GAP;
            }
        });
        self.scroll_y = out.state.offset.y;
        // Fehlende oder unscharfe Seiten anfordern, je Seite nur eine Anfrage zugleich
        for (i, scale) in wanted {
            let stale = match self.textures.get(&i) {
                None => true,
                Some((_, s)) => scale / s > 1.25 || scale / s < 0.55,
            };
            if stale && !self.pending.contains_key(&i) {
                self.pending.insert(i, scale);
                let _ = self.tx.send(Req::Render { page: i, scale });
            }
        }
        // Speicher begrenzen: weit entfernte Seiten verwerfen
        if self.textures.len() > KEEP {
            let cur = self.current as i64;
            let mut keys: Vec<usize> = self.textures.keys().copied().collect();
            keys.sort_by_key(|k| (*k as i64 - cur).abs());
            for k in keys.into_iter().skip(KEEP) {
                self.textures.remove(&k);
            }
        }
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kleinste gueltige PDF-Datei mit einer Seite und etwas Text; die Querverweistabelle wird berechnet.
    fn minimal_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            {
                let stream = "BT /F1 24 Tf 20 40 Td (Hallo) Tj ET";
                format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())
            },
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }

    #[test]
    fn renders_a_page_in_the_background() {
        let dir = std::env::temp_dir().join(format!("easymysql-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.pdf");
        std::fs::write(&file, minimal_pdf()).unwrap();
        let ctx = egui::Context::default();
        let mut v = PdfView::new(&ctx, &file);
        let mut ok = false;
        for _ in 0..200 {
            v.poll(&ctx);
            if v.error.is_some() {
                break;
            }
            if !v.pages.is_empty() && v.pending.is_empty() && v.textures.is_empty() {
                v.pending.insert(0, 1.0);
                v.tx.send(Req::Render { page: 0, scale: 1.0 }).unwrap();
            }
            if v.textures.contains_key(&0) {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(v.error.is_none(), "{:?}", v.error);
        assert!(ok, "Seite wurde nicht gezeichnet");
        assert_eq!(v.pages, vec![(200.0, 100.0)]);
        let (tex, _) = &v.textures[&0];
        assert_eq!(tex.size(), [200, 100]);
        // Nicht lesbare Datei: Fehler statt Absturz
        std::fs::write(dir.join("kaputt.pdf"), b"%PDF-1.4 nothing useful").unwrap();
        let mut bad = PdfView::new(&ctx, &dir.join("kaputt.pdf"));
        for _ in 0..200 {
            bad.poll(&ctx);
            if bad.error.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(bad.error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
