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
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE, HWND};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, EnumWindows, GetPropW, GetWindowThreadProcessId, IsIconic, SMTO_ABORTIFHUNG,
        SW_HIDE, SW_RESTORE, SW_SHOW, SendMessageTimeoutW, SetForegroundWindow, SetPropW, ShowWindow, WM_COPYDATA,
    };

    /// Kennung der Weckmeldung, mit der eine zweite Instanz die laufende aufweckt
    const COPY_MAGIC: usize = 0x454D_5346;
    /// Fenstereigenschaft, an der die zweite Instanz das Hauptfenster erkennt (der Titel allein
    /// passt auch auf einen Explorer-Ordner namens "EasyMySQL")
    const MAIN_PROP: &str = "EasyMySQL.Hauptfenster";

    static WAKE: AtomicBool = AtomicBool::new(false);
    static OPEN_HOOK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

    pub fn install_open_hook(f: Box<dyn Fn() + Send + Sync>) {
        let _ = OPEN_HOOK.set(f);
    }

    /// Wurde die Instanz von einer zweiten (oder durch Fokus) aufgeweckt, kommen die
    /// uebergebenen Dateien zurueck (ggf. leer).
    pub fn take_opened() -> Option<Vec<PathBuf>> {
        WAKE.swap(false, Ordering::Relaxed).then(super::take_queue)
    }

    pub fn wake() {
        WAKE.store(true, Ordering::Relaxed);
    }

    extern "system" fn find_main(h: HWND, out: isize) -> BOOL {
        let prop = wide(MAIN_PROP);
        if unsafe { GetPropW(h, prop.as_ptr()) }.is_null() {
            return 1;
        }
        unsafe { *(out as *mut HWND) = h };
        0
    }

    fn main_window() -> Option<HWND> {
        let mut found: HWND = std::ptr::null_mut();
        unsafe { EnumWindows(Some(find_main), &mut found as *mut HWND as isize) };
        (!found.is_null()).then_some(found)
    }

    /// Laufende Instanz aufwecken (die Dateien stehen in der Warteschlangendatei).
    fn wake_remote(h: HWND) {
        let data = COPYDATASTRUCT { dwData: COPY_MAGIC, cbData: 0, lpData: std::ptr::null_mut() };
        let mut result: usize = 0;
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            AllowSetForegroundWindow(pid);
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
        // Auch in der Liste "Standard-Apps" erscheinen und unter "Öffnen mit" angeboten werden
        run(r"HKCU\Software\Classes\Applications\EasyMySQL.exe\shell\open\command", None, &format!("\"{exe}\" \"%1\""))?;
        run(r"HKCU\Software\EasyMySQL\Capabilities", Some("ApplicationName"), "EasyMySQL")?;
        run(r"HKCU\Software\EasyMySQL\Capabilities", Some("ApplicationDescription"), crate::i18n::text("SQL-Editor und MariaDB-Verwaltung"))?;
        run(r"HKCU\Software\EasyMySQL\Capabilities\FileAssociations", Some(".sql"), "EasyMySQL.sql")?;
        run(r"HKCU\Software\RegisteredApplications", Some("EasyMySQL"), r"Software\EasyMySQL\Capabilities")?;
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

    /// Windows-Einstellungen "Standard-Apps" direkt bei EasyMySQL oeffnen
    pub fn open_default_apps() {
        let _ = std::process::Command::new("explorer").arg("ms-settings:defaultapps?registeredAppUser=EasyMySQL").spawn();
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

    /// Mauszeiger im Fenster (in Punkten). Waehrend Dateien von aussen ueber das Fenster gezogen werden,
/// meldet das Fenster keine Mausbewegung; unter Windows wird der Zeiger deshalb direkt abgefragt.
pub fn cursor_pos(hwnd: Option<isize>, pixels_per_point: f32) -> Option<egui::Pos2> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{HWND, POINT};
        use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let h = hwnd? as HWND;
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut p) == 0 || ScreenToClient(h, &mut p) == 0 {
                return None;
            }
        }
        Some(egui::pos2(p.x as f32, p.y as f32) / pixels_per_point)
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, pixels_per_point);
        None
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
                super::enqueue(files);
                // Die laufende Instanz startet evtl. gerade erst: kurz auf ihr Fenster warten
                for _ in 0..25 {
                    if let Some(h) = main_window() {
                        wake_remote(h);
                        show_window(h as isize);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
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
                    if !data.is_null() && (*data).dwData == COPY_MAGIC {
                        wake();
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
            SetPropW(hwnd as HWND, wide(MAIN_PROP).as_ptr(), 1 as HANDLE);
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

/// Warteschlange fuer Dateien, die eine zweite Instanz (Doppelklick im Explorer) uebergibt:
/// eine Datei statt einer Fensternachricht, damit nichts verloren geht, wenn die laufende
/// Instanz gerade nicht antwortet.
fn queue_file() -> PathBuf {
    crate::server::app_data_dir().join("oeffnen.txt")
}

#[cfg_attr(not(windows), allow(dead_code))]
fn enqueue(files: &[PathBuf]) {
    use std::io::Write;
    if files.is_empty() {
        return;
    }
    let path = queue_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        for p in files {
            let _ = writeln!(f, "{}", p.display());
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn take_queue() -> Vec<PathBuf> {
    let path = queue_file();
    let taken = path.with_extension("lesen");
    if std::fs::rename(&path, &taken).is_err() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&taken).unwrap_or_default();
    let _ = std::fs::remove_file(&taken);
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.is_file()).collect()
}

/// `Some(Dateien)`, wenn eine zweite Instanz diese aufgeweckt hat (Fenster nach vorn holen;
/// die Liste kann leer sein).
pub fn take_opened_files() -> Option<Vec<PathBuf>> {
    #[cfg(windows)]
    {
        imp::take_opened()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Die Warteschlange beim naechsten Bild pruefen (z. B. wenn das Fenster den Fokus bekommt)
pub fn check_queue() {
    #[cfg(windows)]
    imp::wake();
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

/// Mauszeiger im Fenster (in Punkten). Waehrend Dateien von aussen ueber das Fenster gezogen werden,
/// meldet das Fenster keine Mausbewegung; unter Windows wird der Zeiger deshalb direkt abgefragt.
pub fn cursor_pos(hwnd: Option<isize>, pixels_per_point: f32) -> Option<egui::Pos2> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{HWND, POINT};
        use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let h = hwnd? as HWND;
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut p) == 0 || ScreenToClient(h, &mut p) == 0 {
                return None;
            }
        }
        Some(egui::pos2(p.x as f32, p.y as f32) / pixels_per_point)
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, pixels_per_point);
        None
    }
}

pub fn hide_window(hwnd: isize) {
    #[cfg(windows)]
    imp::hide_window(hwnd);
    #[cfg(not(windows))]
    let _ = hwnd;
}
