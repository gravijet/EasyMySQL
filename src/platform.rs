// Windows-spezifisches: Infobereich-Symbol (Tray), Fenster verstecken/zeigen,
// nur eine laufende Instanz. Auf anderen Systemen (Entwicklung) leer.

use eframe::egui;
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum TrayCmd {
    Show,
    RestartServer,
    Quit,
}

#[cfg(windows)]
mod imp {
    use super::TrayCmd;
    use eframe::egui;
    use std::sync::mpsc::{Receiver, channel};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW,
        SetForegroundWindow, ShowWindow,
    };

    const ERROR_ALREADY_EXISTS: u32 = 183;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn show_window(hwnd: isize) {
        let h = hwnd as HWND;
        unsafe {
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            ShowWindow(h, SW_SHOW);
            SetForegroundWindow(h);
        }
    }

    pub fn hide_window(hwnd: isize) {
        unsafe {
            ShowWindow(hwnd as HWND, SW_HIDE);
        }
    }

    /// true = diese Instanz darf laufen. Sonst wird das vorhandene Fenster gezeigt.
    pub fn single_instance() -> bool {
        unsafe {
            let name = wide("Local\\EasyMySQL_Einzelinstanz");
            let _m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let title = wide("EasyMySQL");
                let h = FindWindowW(std::ptr::null(), title.as_ptr());
                if !h.is_null() {
                    show_window(h as isize);
                }
                return false;
            }
            // Mutex bleibt bis Prozessende offen (Handle wird absichtlich nicht geschlossen)
        }
        true
    }

    pub struct Tray {
        _icon: TrayIcon,
    }

    // --- Windows herunterfahren / abmelden: Datenbank vorher sauber beenden ---
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

    static SHUTDOWN_HOOK: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        unsafe {
            match msg {
                WM_QUERYENDSESSION => {
                    let reason = wide("EasyMySQL beendet die Datenbank sicher ...");
                    ShutdownBlockReasonCreate(hwnd, reason.as_ptr());
                    1
                }
                WM_ENDSESSION => {
                    if wparam != 0 {
                        if let Some(f) = SHUTDOWN_HOOK.get() {
                            f();
                        }
                    }
                    ShutdownBlockReasonDestroy(hwnd);
                    0
                }
                _ => DefSubclassProc(hwnd, msg, wparam, lparam),
            }
        }
    }

    pub fn install_shutdown_hook(hwnd: isize, f: Box<dyn Fn() + Send + Sync>) {
        let _ = SHUTDOWN_HOOK.set(f);
        unsafe {
            SetWindowSubclass(hwnd as HWND, Some(subclass_proc), 0x454D, 0);
        }
    }

    pub fn create_tray(ctx: egui::Context, hwnd: isize) -> Option<(Tray, Receiver<TrayCmd>)> {
        let (tx, rx) = channel();
        let menu = Menu::new();
        let open = MenuItem::new("EasyMySQL öffnen", true, None);
        let restart = MenuItem::new("Server neu starten", true, None);
        let quit = MenuItem::new("Beenden (Server stoppen)", true, None);
        menu.append(&open).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&restart).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let icon = Icon::from_rgba(crate::icon::icon_rgba(32), 32, 32).ok()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("EasyMySQL – MariaDB läuft")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;

        let (open_id, restart_id, quit_id) = (open.id().clone(), restart.id().clone(), quit.id().clone());
        let tx_menu = tx.clone();
        let ctx_menu = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let cmd = if e.id == open_id {
                show_window(hwnd);
                TrayCmd::Show
            } else if e.id == restart_id {
                TrayCmd::RestartServer
            } else if e.id == quit_id {
                TrayCmd::Quit
            } else {
                return;
            };
            let _ = tx_menu.send(cmd);
            ctx_menu.request_repaint();
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            let show = matches!(
                e,
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                    | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
            );
            if show {
                show_window(hwnd);
                let _ = tx.send(TrayCmd::Show);
                ctx.request_repaint();
            }
        }));
        Some((Tray { _icon: tray }, rx))
    }
}

#[cfg(windows)]
pub use imp::Tray;

#[cfg(not(windows))]
pub struct Tray;

pub fn single_instance() -> bool {
    #[cfg(windows)]
    {
        imp::single_instance()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Beim Herunterfahren/Abmelden von Windows `f` ausfuehren (Datenbank sauber beenden).
pub fn install_shutdown_hook(hwnd: Option<isize>, f: Box<dyn Fn() + Send + Sync>) {
    #[cfg(windows)]
    if let Some(h) = hwnd {
        imp::install_shutdown_hook(h, f);
    }
    #[cfg(not(windows))]
    let _ = (hwnd, f);
}

pub fn window_handle(cc: &eframe::CreationContext) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

pub fn create_tray(ctx: &egui::Context, hwnd: Option<isize>) -> Option<(Tray, Receiver<TrayCmd>)> {
    #[cfg(windows)]
    {
        imp::create_tray(ctx.clone(), hwnd?)
    }
    #[cfg(not(windows))]
    {
        let _ = (ctx, hwnd);
        None
    }
}

pub fn hide_window(hwnd: isize) {
    #[cfg(windows)]
    imp::hide_window(hwnd);
    #[cfg(not(windows))]
    let _ = hwnd;
}
