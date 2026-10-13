// Windows-spezifisches: Infobereich-Symbol (Tray), Fenster verstecken/zeigen,
// nur eine laufende Instanz. Auf anderen Systemen (Entwicklung) leer.

use eframe::egui;
use std::path::PathBuf;
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
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SMTO_ABORTIFHUNG, SW_HIDE, SW_RESTORE, SW_SHOW,
        SendMessageTimeoutW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
    };

    /// Kennung der Nachricht, mit der eine zweite Instanz Dateien an die laufende schickt
    const COPY_MAGIC: usize = 0x454D_5346;

    /// Von anderen Instanzen geschickte Dateien (werden im naechsten Bild geoeffnet)
    static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static OPEN_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

    pub fn install_open_hook(f: Box<dyn Fn() + Send + Sync>) {
        let _ = OPEN_HOOK.set(f);
    }

    pub fn take_opened() -> Vec<PathBuf> {
        OPENED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }

    /// Dateien aus der Kommandozeile an die laufende Instanz schicken.
    fn forward_files(h: HWND, files: &[PathBuf]) {
        if files.is_empty() {
            return;
        }
        let text = files.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
        let wide: Vec<u16> = text.encode_utf16().collect();
        let data = COPYDATASTRUCT { dwData: COPY_MAGIC, cbData: (wide.len() * 2) as u32, lpData: wide.as_ptr() as *mut core::ffi::c_void };
        let mut result: usize = 0;
        unsafe {
            SendMessageTimeoutW(h, WM_COPYDATA, 0, &data as *const COPYDATASTRUCT as LPARAM, SMTO_ABORTIFHUNG, 3000, &mut result);
        }
    }

    /// EasyMySQL fuer .sql-Dateien eintragen (nur fuer den aktuellen Benutzer, ohne Administratorrechte).
    pub fn register_sql_association() -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();
        let run = |key: &str, name: Option<&str>, data: &str| -> Result<(), String> {
            let mut c = std::process::Command::new("reg");
            c.args(["add", key]);
            match name {
                Some(n) => c.args(["/v", n]),
                None => c.arg("/ve"),
            };
            c.args(["/t", "REG_SZ", "/d", data, "/f"]).creation_flags(0x0800_0000);
            let out = c.output().map_err(|e| e.to_string())?;
            if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
        };
        run(r"HKCU\Software\Classes\EasyMySQL.sql", None, "SQL")?;
        run(r"HKCU\Software\Classes\EasyMySQL.sql\DefaultIcon", None, &format!("\"{exe}\",0"))?;
        run(r"HKCU\Software\Classes\EasyMySQL.sql\shell\open\command", None, &format!("\"{exe}\" \"%1\""))?;
        run(r"HKCU\Software\Classes\.sql", None, "EasyMySQL.sql")?;
        run(r"HKCU\Software\Classes\.sql\OpenWithProgids", Some("EasyMySQL.sql"), "")?;
        unsafe {
            windows_sys::Win32::UI::Shell::SHChangeNotify(
                windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
                windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
        Ok(())
    }

    /// Windows-Einstellungen "Standard-Apps" oeffnen
    pub fn open_default_apps() {
        let _ = std::process::Command::new("explorer").arg("ms-settings:defaultapps").spawn();
    }

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
    pub fn single_instance(files: &[PathBuf]) -> bool {
        unsafe {
            let name = wide("Local\\EasyMySQL_Einzelinstanz");
            let _m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let title = wide("EasyMySQL");
                let h = FindWindowW(std::ptr::null(), title.as_ptr());
                if !h.is_null() {
                    forward_files(h, files);
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
                    let reason = wide(crate::i18n::text("EasyMySQL beendet die Datenbank sicher ..."));
                    ShutdownBlockReasonCreate(hwnd, reason.as_ptr());
                    1
                }
                WM_COPYDATA => {
                    let data = lparam as *const COPYDATASTRUCT;
                    if !data.is_null() && (*data).dwData == COPY_MAGIC && !(*data).lpData.is_null() {
                        let units = std::slice::from_raw_parts((*data).lpData as *const u16, (*data).cbData as usize / 2);
                        let text = String::from_utf16_lossy(units);
                        if let Ok(mut queue) = OPENED.lock() {
                            queue.extend(text.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
                        }
                        if let Some(f) = OPEN_HOOK.get() {
                            f();
                        }
                        return 1;
                    }
                    DefSubclassProc(hwnd, msg, wparam, lparam)
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
        let open = MenuItem::new(crate::i18n::text("EasyMySQL öffnen"), true, None);
        let restart = MenuItem::new(crate::i18n::text("Server neu starten"), true, None);
        let quit = MenuItem::new(crate::i18n::text("Beenden (Server stoppen)"), true, None);
        menu.append(&open).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&restart).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let icon = Icon::from_rgba(crate::icon::icon_rgba(32), 32, 32).ok()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip(crate::i18n::text("EasyMySQL – MariaDB läuft"))
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

/// true = diese Instanz darf laufen. Sonst wurden `files` an die laufende Instanz weitergegeben.
pub fn single_instance(files: &[PathBuf]) -> bool {
    #[cfg(windows)]
    {
        imp::single_instance(files)
    }
    #[cfg(not(windows))]
    {
        let _ = files;
        true
    }
}

/// Dateien, die eine zweite Instanz (Doppelklick im Explorer) an diese geschickt hat
pub fn take_opened_files() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        imp::take_opened()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// `f` aufrufen, sobald Dateien von einer zweiten Instanz eintreffen (fuer ein neues Bild)
pub fn install_open_hook(f: Box<dyn Fn() + Send + Sync>) {
    #[cfg(windows)]
    imp::install_open_hook(f);
    #[cfg(not(windows))]
    let _ = f;
}

/// Ist die Verknuepfung mit .sql-Dateien in dieser Version moeglich?
pub const CAN_ASSOCIATE: bool = cfg!(windows);

/// EasyMySQL als Programm fuer .sql-Dateien eintragen und die Windows-Einstellung dazu zeigen.
pub fn register_sql_association() -> Result<(), String> {
    #[cfg(windows)]
    {
        imp::register_sql_association()?;
        imp::open_default_apps();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(crate::i18n::text("Nur unter Windows verfügbar.").into())
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
