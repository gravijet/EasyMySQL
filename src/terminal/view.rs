// Zeichnet den Bildschirm einer Terminal-Sitzung und uebersetzt Tastatur und Maus.

use super::{Session, set_focused};
use eframe::egui::{self, Color32, Event, EventFilter, FontId, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

/// Farben 0-15 der Terminal-Palette (wie in Visual Studio Code)
const ANSI_DARK: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00), (0xCD, 0x31, 0x31), (0x0D, 0xBC, 0x79), (0xE5, 0xE5, 0x10), (0x24, 0x72, 0xC8), (0xBC, 0x3F, 0xBC), (0x11, 0xA8, 0xCD), (0xE5, 0xE5, 0xE5),
    (0x66, 0x66, 0x66), (0xF1, 0x4C, 0x4C), (0x23, 0xD1, 0x8B), (0xF5, 0xF5, 0x43), (0x3B, 0x8E, 0xEA), (0xD6, 0x70, 0xD6), (0x29, 0xB8, 0xDB), (0xFF, 0xFF, 0xFF),
];
const ANSI_LIGHT: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00), (0xCD, 0x31, 0x31), (0x00, 0xBC, 0x00), (0x94, 0x98, 0x00), (0x04, 0x51, 0xA5), (0xBC, 0x05, 0xBC), (0x05, 0x98, 0xBC), (0x55, 0x55, 0x55),
    (0x66, 0x66, 0x66), (0xCD, 0x31, 0x31), (0x14, 0xCE, 0x14), (0xB5, 0xBA, 0x00), (0x04, 0x51, 0xA5), (0xBC, 0x05, 0xBC), (0x05, 0x98, 0xBC), (0xA5, 0xA5, 0xA5),
];

fn indexed(i: u8, dark: bool) -> Color32 {
    let rgb = |(r, g, b): (u8, u8, u8)| Color32::from_rgb(r, g, b);
    match i {
        0..=15 => rgb(if dark { ANSI_DARK[i as usize] } else { ANSI_LIGHT[i as usize] }),
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Color32::from_rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            let v = 8 + 10 * (i - 232);
            Color32::from_rgb(v, v, v)
        }
    }
}

fn resolve(c: vt100::Color, default: Color32, dark: bool) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => indexed(i, dark),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Bytes fuer eine Taste (None = die Taste sendet nichts oder kommt als Text)
pub(super) fn key_bytes(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let code = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let seq = |letter: char| -> Vec<u8> {
        if code > 1 {
            format!("\x1b[1;{code}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if code > 1 { format!("\x1b[{n};{code}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    Some(match key {
        Key::Enter => b"\r".to_vec(),
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Backspace if mods.ctrl => vec![0x08],
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => seq('A'),
        Key::ArrowDown => seq('B'),
        Key::ArrowRight => seq('C'),
        Key::ArrowLeft => seq('D'),
        Key::Home => seq('H'),
        Key::End => seq('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        Key::Space if mods.ctrl => vec![0],
        // Strg + Buchstabe: Steuerzeichen (Strg+C, Strg+V und Strg+X kommen als Kopieren/Einfuegen/Ausschneiden)
        k if mods.ctrl && !mods.alt => {
            let name = k.name();
            let c = name.chars().next().filter(|_| name.chars().count() == 1)?;
            match c {
                'A'..='Z' => vec![c as u8 - b'A' + 1],
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                _ => return None,
            }
        }
        // Alt + Buchstabe: ESC davor, der Buchstabe selbst kommt als Text
        k if mods.alt && !mods.ctrl && k.name().chars().count() == 1 => vec![0x1b],
        _ => return None,
    })
}

/// Text der Auswahl
fn selected_text(s: &Session) -> Option<String> {
    let ((r1, c1), (r2, c2)) = ordered(s.selection?);
    let p = s.parser.lock().ok()?;
    let t = p.screen().contents_between(r1, c1, r2, c2.saturating_add(1));
    (!t.trim().is_empty()).then_some(t)
}

fn ordered(sel: ((u16, u16), (u16, u16))) -> ((u16, u16), (u16, u16)) {
    let (a, b) = sel;
    if a <= b { (a, b) } else { (b, a) }
}

/// Zeichnet die Sitzung in den verfuegbaren Platz und verarbeitet die Eingabe.
/// `want_focus`: Tastaturfokus jetzt auf dieses Terminal setzen.
pub fn show(ui: &mut egui::Ui, id: egui::Id, s: &mut Session, want_focus: bool) {
    let pal = crate::style::pal();
    let font = FontId::monospace(crate::sqledit::font_size());
    let ctx = ui.ctx().clone();
    let (cw, ch) = ctx.fonts_mut(|f| (f.glyph_width(&font, 'M').max(1.0), f.row_height(&font).max(1.0)));
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(avail, Sense::hover());
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let inner = rect.shrink2(vec2(6.0, 2.0));
    let cols = (inner.width() / cw).floor().max(10.0) as u16;
    let rows = (inner.height() / ch).floor().max(2.0) as u16;
    s.resize(rows, cols);

    if want_focus || resp.clicked() || resp.drag_started() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let focused = ctx.memory(|m| m.has_focus(id));
    if focused {
        // Tab, Pfeiltasten und Esc gehoeren der Shell (egui wuerde sonst den Fokus weiterreichen)
        ctx.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        set_focused(true);
    }

    let cell_at = |p: egui::Pos2| -> (u16, u16) {
        let r = ((p.y - inner.top()) / ch).floor().clamp(0.0, (rows - 1) as f32) as u16;
        let c = ((p.x - inner.left()) / cw).floor().clamp(0.0, (cols - 1) as f32) as u16;
        (r, c)
    };

    // Maus: Auswahl, Mausrad blaettert durch die Ausgabe
    if resp.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let c = cell_at(p);
        s.selection = Some((c, c));
    } else if resp.dragged()
        && let (Some(sel), Some(p)) = (s.selection, ctx.pointer_interact_pos())
    {
        s.selection = Some((sel.0, cell_at(p)));
    } else if resp.clicked() {
        s.selection = None;
    }
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let lines = (dy / ch).round() as i64;
            if let Ok(mut p) = s.parser.lock() {
                let now = p.screen().scrollback() as i64;
                p.screen_mut().set_scrollback((now + lines).max(0) as usize);
            }
        }
    }

    // Tastatur
    if focused {
        let events = ui.input(|i| i.events.clone());
        let (app_cursor, bracketed) = {
            let p = s.parser.lock().unwrap();
            (p.screen().application_cursor(), p.screen().bracketed_paste())
        };
        let mut typed = false;
        for e in events {
            if s.exited() {
                if matches!(e, Event::Key { key: Key::Enter, pressed: true, .. }) {
                    s.restart(&ctx);
                }
                continue;
            }
            match e {
                Event::Text(t) => {
                    s.write(t.as_bytes());
                    typed = true;
                }
                Event::Paste(t) => {
                    if bracketed {
                        s.write(b"\x1b[200~");
                        s.write(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                        s.write(b"\x1b[201~");
                    } else {
                        s.write(t.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
                    }
                    typed = true;
                }
                // Strg+C: mit Auswahl kopieren, sonst unterbrechen
                Event::Copy => match selected_text(s) {
                    Some(t) => {
                        ctx.copy_text(t);
                        s.selection = None;
                    }
                    None => {
                        s.write(&[0x03]);
                        typed = true;
                    }
                },
                Event::Cut => {
                    s.write(&[0x18]);
                    typed = true;
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    // AltGr (Strg+Alt) liefert Zeichen als Text
                    if modifiers.ctrl && modifiers.alt {
                        continue;
                    }
                    // Umschalt+Einfg fuegt ein, Strg+Umschalt+C kopiert
                    if key == Key::C && modifiers.ctrl && modifiers.shift {
                        if let Some(t) = selected_text(s) {
                            ctx.copy_text(t);
                        }
                        continue;
                    }
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        s.write(&bytes);
                        typed = true;
                    }
                }
                _ => {}
            }
        }
        if typed && let Ok(mut p) = s.parser.lock() {
            p.screen_mut().set_scrollback(0);
        }
    }

    // Zeichnen
    let painter = ui.painter_at(rect);
    let (default_fg, default_bg) = if pal.dark { (Color32::from_rgb(0xCC, 0xCC, 0xCC), pal.bg) } else { (Color32::from_rgb(0x33, 0x33, 0x33), pal.bg) };
    painter.rect_filled(rect, 0.0, default_bg);
    let guard = s.parser.lock().unwrap();
    let screen = guard.screen();
    let sel = s.selection.map(ordered);
    for row in 0..rows {
        let y = inner.top() + row as f32 * ch;
        let mut run = String::new();
        let mut run_start = 0u16;
        let mut run_style: Option<(Color32, bool, bool)> = None;
        let mut bg_run: Option<(u16, Color32)> = None;
        let flush_text = |run: &mut String, start: u16, style: Option<(Color32, bool, bool)>| {
            if let Some((fg, bold, underline)) = style
                && !run.trim().is_empty()
            {
                let pos = pos2(inner.left() + start as f32 * cw, y);
                painter.text(pos, egui::Align2::LEFT_TOP, run.as_str(), font.clone(), fg);
                if bold {
                    painter.text(pos + vec2(0.7, 0.0), egui::Align2::LEFT_TOP, run.as_str(), font.clone(), fg);
                }
                if underline {
                    let w = run.chars().count() as f32 * cw;
                    painter.hline(pos.x..=pos.x + w, y + ch - 2.0, Stroke::new(1.0, fg));
                }
            }
            run.clear();
        };
        let flush_bg = |bg: &mut Option<(u16, Color32)>, end: u16| {
            if let Some((start, color)) = bg.take() {
                let r = Rect::from_min_max(pos2(inner.left() + start as f32 * cw, y), pos2(inner.left() + end as f32 * cw, y + ch));
                painter.rect_filled(r, 0.0, color);
            }
        };
        let mut col = 0u16;
        while col < cols {
            let Some(cell) = screen.cell(row, col) else { break };
            if cell.is_wide_continuation() {
                col += 1;
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fgcolor(), default_fg, pal.dark), resolve(cell.bgcolor(), default_bg, pal.dark));
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = fg.gamma_multiply(0.65);
            }
            // Hintergrund
            if bg != default_bg {
                match bg_run {
                    Some((_, c)) if c == bg => {}
                    _ => {
                        flush_bg(&mut bg_run, col);
                        bg_run = Some((col, bg));
                    }
                }
            } else {
                flush_bg(&mut bg_run, col);
            }
            // Text
            let style = Some((fg, cell.bold(), cell.underline()));
            if run_style != style {
                flush_text(&mut run, run_start, run_style);
                run_start = col;
                run_style = style;
            }
            run.push_str(if cell.has_contents() { cell.contents() } else { " " });
            col += if cell.is_wide() { 2 } else { 1 };
        }
        flush_bg(&mut bg_run, col);
        flush_text(&mut run, run_start, run_style);
        // Auswahl
        if let Some(((r1, c1), (r2, c2))) = sel
            && row >= r1
            && row <= r2
        {
            let from = if row == r1 { c1 } else { 0 };
            let to = if row == r2 { c2 + 1 } else { cols };
            if to > from {
                let r = Rect::from_min_max(pos2(inner.left() + from as f32 * cw, y), pos2(inner.left() + to as f32 * cw, y + ch));
                painter.rect_filled(r, 0.0, pal.selection.gamma_multiply(0.85));
            }
        }
    }
    // Cursor
    if !screen.hide_cursor() && screen.scrollback() == 0 {
        let (cr, cc) = screen.cursor_position();
        let r = Rect::from_min_size(pos2(inner.left() + cc as f32 * cw, inner.top() + cr as f32 * ch), vec2(cw, ch));
        if focused {
            painter.rect_filled(r, 0.0, default_fg.gamma_multiply(0.75));
        } else {
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, default_fg.gamma_multiply(0.7)), StrokeKind::Inside);
        }
    }
    drop(guard);
    // Beendet: Hinweis am unteren Rand
    if s.exited() {
        let msg = match &s.error {
            Some(e) => format!("{e}  –  {}", crate::i18n::text("Enter versucht es erneut")),
            None => crate::i18n::text("Der Prozess wurde beendet. Enter startet ihn neu.").to_string(),
        };
        painter.text(pos2(inner.left(), rect.bottom() - 4.0), egui::Align2::LEFT_BOTTOM, msg, FontId::proportional(12.0), pal.warn_text);
    }
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    // Rechtsklick: Kopieren/Einfuegen
    resp.context_menu(|ui| {
        if ui.add_enabled(s.selection.is_some(), egui::Button::new(crate::i18n::text("Kopieren"))).clicked() {
            if let Some(t) = selected_text(s) {
                ui.ctx().copy_text(t);
            }
            ui.close();
        }
        if ui.button(crate::i18n::text("Aus Zwischenablage einfügen")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_terminal_sequences() {
        let n = Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, n, false).unwrap(), b"\r");
        assert_eq!(key_bytes(Key::ArrowUp, n, false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(Key::ArrowUp, n, true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::CTRL, false).unwrap(), b"\x1b[1;5D");
        assert_eq!(key_bytes(Key::Delete, n, false).unwrap(), b"\x1b[3~");
        assert_eq!(key_bytes(Key::Backspace, n, false).unwrap(), vec![0x7f]);
        assert_eq!(key_bytes(Key::D, Modifiers::CTRL, false).unwrap(), vec![4]);
        assert_eq!(key_bytes(Key::L, Modifiers::CTRL, false).unwrap(), vec![12]);
        assert_eq!(key_bytes(Key::Tab, Modifiers::SHIFT, false).unwrap(), b"\x1b[Z");
        // Normale Buchstaben kommen als Text
        assert!(key_bytes(Key::A, n, false).is_none());
        // Alt+Buchstabe: nur das ESC davor
        assert_eq!(key_bytes(Key::B, Modifiers::ALT, false).unwrap(), vec![0x1b]);
    }

    #[test]
    fn colors() {
        assert_eq!(indexed(1, true), Color32::from_rgb(0xCD, 0x31, 0x31));
        assert_eq!(indexed(16, true), Color32::from_rgb(0, 0, 0));
        assert_eq!(indexed(196, true), Color32::from_rgb(255, 0, 0));
        assert_eq!(indexed(232, true), Color32::from_rgb(8, 8, 8));
        assert_eq!(resolve(vt100::Color::Rgb(1, 2, 3), Color32::WHITE, true), Color32::from_rgb(1, 2, 3));
    }

    #[test]
    fn selection_is_ordered() {
        assert_eq!(ordered(((3, 4), (1, 2))), ((1, 2), (3, 4)));
    }
}
