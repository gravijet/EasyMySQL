// Updates: nach neuen Versionen suchen, im Hintergrund herunterladen und installieren, ohne dass
// das Programm geschlossen werden muss. Danach erscheint eine Meldung mit "Neu starten".
//
// Quelle ist die Website von EasyMySQL: /api/release liefert Version und Pruefsummen,
// /download/app die kleine Update-Datei (nur EasyMySQL.exe als ZIP), /download/setup das Setup.
//
// Installation: Die laufende EXE laesst sich unter Windows umbenennen. Die neue Datei wird an
// ihren Platz kopiert; die alte bleibt als `.old` liegen und wird beim naechsten Start geloescht.
// Darf das Programm nicht in seinen Ordner schreiben (Programme), fragt Windows einmal nach
// der Freigabe und ein kurzer Helfer (`--apply-update`) tauscht die Datei aus.

use eframe::egui;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const SITE: &str = "https://mysql.benjaminberger.at";
pub const DOWNLOAD_PAGE: &str = "https://mysql.benjaminberger.at/#download";
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_APP: u64 = 256 * 1024 * 1024;
/// Umgebungsvariable: der neue Prozess wartet, bis dieser Prozess beendet ist (Neustart nach dem Update)
pub const WAIT_PID: &str = "EASYMYSQL_WAIT_PID";

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    url: String,
    size: Option<u64>,
    sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct Files {
    app: Option<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
struct Release {
    version: String,
    files: Files,
}

fn safe_url(a: &Asset) -> bool {
    a.url.starts_with("/download/") && !a.url.contains(['?', '#', '\\']) && !a.url.contains("..")
}

impl Release {
    fn version(&self) -> Result<Version, String> {
        Version::parse(&self.version).map_err(|_| crate::i18n::text("Die Versionsnummer des Updates ist ungültig.").into())
    }

    fn newer_than(&self, current: &str) -> Result<bool, String> {
        let version = self.version()?;
        let current = Version::parse(current).map_err(|e| e.to_string())?;
        Ok(version.pre.is_empty() && version.cmp_precedence(&current).is_gt())
    }

    /// Die kleine Update-Datei (nur das Programm), mit der im Hintergrund aktualisiert wird
    fn app(&self) -> Result<Asset, String> {
        let version = self.version()?;
        let name = format!("EasyMySQL-app-{version}.zip");
        self.files
            .app
            .iter()
            .find(|a| a.name == name && safe_url(a))
            .cloned()
            .ok_or_else(|| crate::i18n::text("Für dieses Release gibt es noch kein Update. Bitte später erneut prüfen.").into())
    }
}

/// Basisadresse; zum Testen ist in Debug-Builds ein eigener Server moeglich (EASYMYSQL_UPDATE_SITE)
fn site() -> String {
    if cfg!(debug_assertions)
        && let Ok(s) = std::env::var("EASYMYSQL_UPDATE_SITE")
    {
        return s.trim_end_matches('/').to_string();
    }
    SITE.to_string()
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(site().starts_with("https://"))
        .user_agent(concat!("EasyMySQL/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn check() -> Result<Option<Release>, String> {
    let response = agent(Duration::from_secs(20)).get(format!("{}/api/release", site())).header("Accept", "application/json").call();
    let mut response = match response {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(ureq::Error::StatusCode(403 | 429 | 502 | 503)) => return Err(crate::i18n::text("Der Update-Server ist gerade ausgelastet. Bitte später erneut versuchen.").into()),
        Err(e) => return Err(crate::tr_format!("Update-Server nicht erreichbar: {e}", "Could not reach the update server: {e}")),
    };
    let release: Release = response
        .body_mut()
        .read_json()
        .map_err(|e| crate::tr_format!("Antwort des Update-Servers konnte nicht gelesen werden: {e}", "Could not read the update server response: {e}"))?;
    if release.newer_than(env!("CARGO_PKG_VERSION"))? { Ok(Some(release)) } else { Ok(None) }
}

fn verify_download(size: u64, hash: &str, asset: &Asset) -> Result<(), String> {
    if size == 0 || asset.size != Some(size) {
        return Err(crate::i18n::text("Der Download ist unvollständig. Bitte erneut herunterladen.").into());
    }
    let expected = asset
        .sha256
        .as_deref()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(crate::i18n::text("Für das Update liegt keine gültige SHA-256-Prüfsumme vor."))?;
    if !hash.eq_ignore_ascii_case(expected) {
        return Err(crate::i18n::text("Die Prüfsumme des Downloads stimmt nicht. Das Update wird nicht installiert.").into());
    }
    Ok(())
}

/// Fortschritt des Downloads (fuer die Anzeige)
#[derive(Default)]
struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

/// Datei herunterladen, Groesse und SHA-256 pruefen. Liefert den Pfad in einem eigenen, privaten Ordner.
fn download(asset: &Asset, progress: &Progress) -> Result<PathBuf, String> {
    let expected_size = asset.size.unwrap_or(0);
    if expected_size == 0 || expected_size > MAX_APP {
        return Err(crate::i18n::text("Die Größe des Updates ist ungültig.").into());
    }
    progress.total.store(expected_size, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "EasyMySQL-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
    ));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(&asset.name);
    let result = (|| {
        let mut response = agent(Duration::from_secs(15 * 60)).get(format!("{}{}", site(), asset.url)).call().map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader().take(expected_size + 1);
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
            hash.update(&buffer[..count]);
            size += count as u64;
            progress.done.store(size, Ordering::Relaxed);
        }
        verify_download(size, &format!("{:x}", hash.finalize()), asset)?;
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Name der Programmdatei im Update-Archiv
fn app_file() -> &'static str {
    if cfg!(windows) { "EasyMySQL.exe" } else { "easymysql" }
}

/// Programmdatei aus dem ZIP in `dir` entpacken. Nur genau diese eine Datei wird gelesen.
fn extract_app(zip_path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let wanted = app_file().to_lowercase();
    let index = (0..zip.len())
        .find(|&i| zip.by_index(i).ok().is_some_and(|f| !f.is_dir() && f.name().ok().is_some_and(|n| n.rsplit(['/', '\\']).next().is_some_and(|n| n.to_lowercase() == wanted))))
        .ok_or_else(|| crate::i18n::text("Das Update enthält keine Programmdatei.").to_string())?;
    let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
    if entry.size() == 0 || entry.size() > MAX_APP {
        return Err(crate::i18n::text("Die Größe des Updates ist ungültig.").into());
    }
    let out = dir.join(app_file());
    let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
    }
    Ok(out)
}

/// Sieht die Datei wie ein Programm aus? (Windows: "MZ", Linux: ELF)
fn looks_like_program(p: &Path) -> bool {
    let mut head = [0u8; 4];
    let ok = std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok();
    ok && (head.starts_with(b"MZ") || head == *b"\x7fELF")
}

fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// Laufende Programmdatei austauschen: die alte wird umbenannt (das geht auch, solange sie laeuft),
/// die neue an ihren Platz kopiert. Schlaegt das Kopieren fehl, kommt die alte zurueck.
pub fn replace_program(new: &Path, target: &Path) -> std::io::Result<()> {
    if !looks_like_program(new) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a program file"));
    }
    let old = old_path(target);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(target, &old)?;
    if let Err(e) = std::fs::copy(new, target) {
        let _ = std::fs::remove_file(target);
        let _ = std::fs::rename(&old, target);
        return Err(e);
    }
    Ok(())
}

/// Beim Start: die Datei der vorigen Version (`.old`) aufraeumen
pub fn cleanup_old() {
    let _ = std::fs::remove_file(old_path(&exe_path()));
}

/// Pfad der Programmdatei, wie beim Start (nach dem Austausch zeigt `current_exe` unter Windows auf die `.old`-Datei)
pub fn exe_path() -> PathBuf {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::current_exe().unwrap_or_default()).clone()
}

/// Ziel des Austauschs (zum Testen in Debug-Builds ueberschreibbar)
fn install_target() -> PathBuf {
    if cfg!(debug_assertions)
        && let Some(p) = std::env::var_os("EASYMYSQL_UPDATE_TARGET")
    {
        return PathBuf::from(p);
    }
    exe_path()
}

/// Kann das Programm sich selbst aktualisieren? (Windows; zum Testen auch mit eigenem Ziel)
fn can_self_update() -> bool {
    cfg!(windows) || (cfg!(debug_assertions) && std::env::var_os("EASYMYSQL_UPDATE_TARGET").is_some())
}

/// Neue Programmdatei an die Stelle der laufenden setzen; braucht der Ordner Administratorrechte, fragt Windows danach.
fn install(new: &Path) -> Result<(), String> {
    let target = install_target();
    match replace_program(new, &target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => install_elevated(new, &target),
        Err(e) => Err(crate::tr_format!("Das Update konnte nicht installiert werden: {e}", "The update could not be installed: {e}")),
    }
}

#[cfg(windows)]
fn install_elevated(new: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let exe = wide(exe_path().as_os_str());
    let verb = wide("runas".as_ref());
    let params = wide(format!("--apply-update \"{}\" \"{}\"", new.display(), target.display()).as_ref());
    // SAFETY: Die Puffer leben bis zum Ende des Aufrufs; die Struktur wird vollstaendig initialisiert.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err(crate::i18n::text("Die Installation wurde nicht freigegeben (Administratorfreigabe abgebrochen).").into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code != 0 {
            return Err(crate::i18n::text("Das Update konnte nicht installiert werden.").into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn install_elevated(_new: &Path, _target: &Path) -> Result<(), String> {
    Err(crate::i18n::text("Das Programm darf seine Datei nicht ersetzen. Bitte die Download-Seite öffnen.").into())
}

/// Mit `--apply-update <neu> <ziel>` gestartet (erhoehter Helfer): Datei tauschen und beenden.
pub fn run_helper(args: &[std::ffi::OsString]) -> Option<i32> {
    if args.first().is_some_and(|a| a == "--apply-update") && args.len() == 3 {
        return Some(if replace_program(Path::new(&args[1]), Path::new(&args[2])).is_ok() { 0 } else { 1 });
    }
    None
}

/// Neuen Prozess starten, der auf das Ende dieses Prozesses wartet (Neustart nach dem Update)
pub fn spawn_restart() -> Result<(), String> {
    let mut cmd = std::process::Command::new(install_target());
    cmd.env(WAIT_PID, std::process::id().to_string());
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Am Programmstart: Wurde dieser Prozess fuer einen Neustart gestartet, auf das Ende des alten warten.
pub fn wait_for_previous() {
    let Some(pid) = std::env::var(WAIT_PID).ok().and_then(|v| v.parse::<u32>().ok()) else { return };
    // SAFETY: Es laeuft noch kein weiterer Thread.
    unsafe { std::env::remove_var(WAIT_PID) };
    wait_for_exit(pid, Duration::from_secs(40));
}

#[cfg(windows)]
fn wait_for_exit(pid: u32, limit: Duration) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    // SAFETY: Das Handle wird nach dem Warten geschlossen.
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, limit.as_millis() as u32);
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn wait_for_exit(pid: u32, limit: Duration) {
    let start = Instant::now();
    while Path::new(&format!("/proc/{pid}")).exists() && start.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// Zustand und Oberflaeche

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Checking,
    /// Neue Version gefunden, wartet auf die Zustimmung
    Available,
    Downloading,
    /// Installiert; zum Benutzen muss das Programm neu gestartet werden
    Installed,
    UpToDate,
    Failed,
}

/// Was die Einstellungen vom Updater wissen muessen
#[derive(Clone, Debug)]
pub struct Status {
    pub phase: Phase,
    pub text: String,
    /// 0..1 waehrend des Downloads
    pub progress: f32,
}

impl Default for Status {
    fn default() -> Self {
        Self { phase: Phase::Idle, text: String::new(), progress: 0.0 }
    }
}

/// Wunsch der Oberflaeche an die App
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Restart,
}

type InstallResult = Result<(), String>;

#[derive(Default)]
pub struct Updater {
    last_check: Option<Instant>,
    check_job: Option<Receiver<Result<Option<Release>, String>>>,
    install_job: Option<(Receiver<InstallResult>, Arc<Progress>)>,
    release: Option<Release>,
    phase: Option<Phase>,
    text: String,
    /// Der Nutzer hat die Pruefung selbst angestossen (Ergebnis immer melden)
    manual: bool,
    /// Meldung "Update installiert" wurde weggeklickt
    card_hidden: bool,
    /// Hinweis auf diese Version wurde mit "Spaeter" weggeklickt
    dismissed: Option<String>,
}

impl Updater {
    fn phase(&self) -> Phase {
        self.phase.unwrap_or(Phase::Idle)
    }

    pub fn status(&self) -> Status {
        let progress = self
            .install_job
            .as_ref()
            .map(|(_, p)| {
                let t = p.total.load(Ordering::Relaxed).max(1);
                (p.done.load(Ordering::Relaxed) as f32 / t as f32).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        Status { phase: self.phase(), text: self.text.clone(), progress }
    }

    /// Nach Updates suchen. `manual`: das Ergebnis immer melden (auch "aktuell").
    pub fn check_now(&mut self, ctx: &egui::Context, manual: bool) {
        self.manual |= manual;
        if self.check_job.is_some() || self.install_job.is_some() || self.phase() == Phase::Installed {
            return;
        }
        self.last_check = Some(Instant::now());
        self.phase = Some(Phase::Checking);
        self.text = crate::i18n::text("Suche nach Updates …").into();
        let (tx, rx) = mpsc::channel();
        self.check_job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check());
            ctx.request_repaint();
        });
    }

    /// Herunterladen und installieren (im Hintergrund)
    pub fn install_now(&mut self, ctx: &egui::Context) {
        let Some(release) = self.release.clone() else { return };
        if self.install_job.is_some() {
            return;
        }
        if !can_self_update() {
            self.phase = Some(Phase::Failed);
            self.text = crate::i18n::text("Das automatische Update ist nur unter Windows verfügbar. Bitte die Download-Seite öffnen.").into();
            return;
        }
        let asset = match release.app() {
            Ok(a) => a,
            Err(e) => {
                self.phase = Some(Phase::Failed);
                self.text = e;
                return;
            }
        };
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let (p, ctx) = (progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = (|| {
                let zip = download(&asset, &p)?;
                let dir = zip.parent().map(Path::to_path_buf).unwrap_or_default();
                let exe = extract_app(&zip, &dir)?;
                let r = install(&exe);
                let _ = std::fs::remove_dir_all(&dir);
                r
            })();
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.install_job = Some((rx, progress));
        self.phase = Some(Phase::Downloading);
        self.text = crate::tr_format!("EasyMySQL {} wird im Hintergrund geladen und installiert …", "Downloading and installing EasyMySQL {} in the background …", release.version);
        self.card_hidden = false;
    }

    /// Regelmaessig pruefen; bei eingeschaltetem automatischem Update gleich herunterladen und installieren.
    pub fn poll(&mut self, ctx: &egui::Context, auto_install: bool) {
        if self.last_check.is_none_or(|t| t.elapsed() >= INTERVAL) && self.phase() != Phase::Installed {
            self.check_now(ctx, false);
        }
        ctx.request_repaint_after(Duration::from_secs(60));
        if let Some(result) = self.check_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.check_job = None;
            let manual = std::mem::take(&mut self.manual);
            match result {
                Ok(Some(release)) => {
                    self.text = crate::tr_format!("EasyMySQL {} ist verfügbar.", "EasyMySQL {} is available.", release.version);
                    let is_new = self.release.as_ref().map(|r| &r.version) != Some(&release.version);
                    self.release = Some(release);
                    self.phase = Some(Phase::Available);
                    if is_new || manual {
                        self.dismissed = None;
                    }
                    if auto_install {
                        self.install_now(ctx);
                    }
                }
                Ok(None) => {
                    self.release = None;
                    self.phase = Some(if manual { Phase::UpToDate } else { Phase::Idle });
                    self.text = crate::i18n::text("EasyMySQL ist auf dem neuesten Stand.").into();
                }
                Err(e) => {
                    // Fehler beim stillen Pruefen sind keine Meldung wert
                    self.phase = Some(if manual { Phase::Failed } else { Phase::Idle });
                    self.text = e;
                }
            }
        }
        if let Some((rx, _)) = &self.install_job {
            ctx.request_repaint_after(Duration::from_millis(200));
            if let Ok(result) = rx.try_recv() {
                self.install_job = None;
                match result {
                    Ok(()) => {
                        self.phase = Some(Phase::Installed);
                        self.text = crate::tr_format!(
                            "EasyMySQL {} wurde installiert. Starten Sie das Programm neu, um es zu verwenden.",
                            "EasyMySQL {} has been installed. Restart the app to start using it.",
                            self.release.as_ref().map(|r| r.version.as_str()).unwrap_or("")
                        );
                        self.card_hidden = false;
                    }
                    Err(e) => {
                        self.phase = Some(Phase::Failed);
                        self.text = crate::tr_format!("Update fehlgeschlagen: {e}", "Update failed: {e}");
                    }
                }
            }
        }
    }

    /// Installierte, aber noch nicht aktive Version
    pub fn installed_version(&self) -> Option<&str> {
        if self.phase() == Phase::Installed { self.release.as_ref().map(|r| r.version.as_str()) } else { None }
    }

    /// Meldung oben rechts: neue Version, Fortschritt, "installiert – neu starten", Fehler.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Event> {
        #[derive(Clone, Copy)]
        enum Card {
            Ask,
            Busy,
            Done,
            Error,
        }
        let pal = crate::style::pal();
        let version = self.release.as_ref().map(|r| r.version.clone()).unwrap_or_default();
        let card = match self.phase() {
            Phase::Available if self.dismissed.as_deref() != Some(&version) => Card::Ask,
            Phase::Downloading => Card::Busy,
            Phase::Installed if !self.card_hidden => Card::Done,
            Phase::Failed if !self.card_hidden => Card::Error,
            _ => return None,
        };
        let mut event = None;
        let mut install = false;
        let mut hide = false;
        let mut open_page = false;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("update-card"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.right() - 16.0 - 340.0, screen.top() + crate::app::TITLE_H + 12.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(pal.face_light)
                    .stroke(egui::Stroke::new(1.0, pal.border))
                    .corner_radius(crate::style::PANEL_RADIUS - 2)
                    .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: egui::Color32::from_black_alpha(if pal.dark { 140 } else { 40 }) })
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(308.0);
                        let (title, color) = match card {
                            Card::Ask => (crate::tr_format!("Update verfügbar: {version}", "Update available: {version}"), pal.accent),
                            Card::Busy => (crate::i18n::text("Update wird installiert").to_string(), pal.accent),
                            Card::Done => (crate::i18n::text("Update installiert").to_string(), pal.ok_text),
                            Card::Error => (crate::i18n::text("Update fehlgeschlagen").to_string(), pal.error_text),
                        };
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.0, color);
                            ui.label(egui::RichText::new(title).strong());
                        });
                        ui.add_space(6.0);
                        let body = match card {
                            Card::Ask => crate::i18n::text("Die neue Version wird im Hintergrund geladen und installiert. EasyMySQL muss dafür nicht geschlossen werden.").to_string(),
                            _ => self.text.clone(),
                        };
                        ui.label(egui::RichText::new(body).color(pal.text_weak));
                        if matches!(card, Card::Busy) {
                            ui.add_space(10.0);
                            let p = self.status().progress;
                            ui.add(egui::ProgressBar::new(p).desired_height(8.0).corner_radius(4));
                        }
                        ui.add_space(12.0);
                        ui.horizontal(|ui| match card {
                            Card::Ask => {
                                if crate::widgets::primary(ui, crate::i18n::text("Installieren")).clicked() {
                                    install = true;
                                }
                                if ui.button(crate::i18n::text("Später")).clicked() {
                                    hide = true;
                                }
                            }
                            Card::Busy => {}
                            Card::Done => {
                                if crate::widgets::primary(ui, crate::i18n::text("Jetzt neu starten")).clicked() {
                                    event = Some(Event::Restart);
                                }
                                if ui.button(crate::i18n::text("Später")).clicked() {
                                    hide = true;
                                }
                            }
                            Card::Error => {
                                if ui.button(crate::i18n::text("Download-Seite")).clicked() {
                                    open_page = true;
                                }
                                if ui.button(crate::i18n::text("Schließen")).clicked() {
                                    hide = true;
                                }
                            }
                        });
                    });
            });
        if install {
            self.install_now(ctx);
        }
        if open_page {
            crate::app::open_path(Path::new(DOWNLOAD_PAGE));
        }
        if hide {
            match card {
                Card::Ask => self.dismissed = Some(version),
                Card::Done | Card::Error => self.card_hidden = true,
                Card::Busy => {}
            }
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        serde_json::from_value(serde_json::json!({"version":tag,"files":{}})).unwrap()
    }

    fn asset(name: &str, url: &str) -> Asset {
        Asset { name: name.into(), url: url.into(), size: Some(3), sha256: None }
    }

    #[test]
    fn versions_are_compared_numerically_and_only_stable_releases_update() {
        assert!(release("3.10.0").newer_than("3.9.0").unwrap());
        assert!(release("4.0.0").newer_than("3.5.0").unwrap());
        assert!(!release("3.0.1").newer_than("3.0.1").unwrap());
        assert!(!release("2.9.0").newer_than("3.0.1").unwrap());
        assert!(!release("4.0.0-beta.1").newer_than("3.0.1").unwrap());
        assert!(!release("3.0.1+build.2").newer_than("3.0.1").unwrap());
        assert!(release("latest").newer_than("3.0.1").is_err());
    }

    #[test]
    fn update_file_requires_exact_name_and_download_path() {
        let mut r = release("4.0.0");
        assert!(r.app().is_err());
        r.files.app = Some(asset("EasyMySQL-app-4.0.0.zip", "/download/app"));
        assert!(r.app().is_ok());
        for bad in ["https://evil.test/download/app", "//evil.test/download/app", "/download/../x", "/other", "/download/app?x=1"] {
            r.files.app = Some(asset("EasyMySQL-app-4.0.0.zip", bad));
            assert!(r.app().is_err(), "{bad}");
        }
        r.files.app = Some(asset("EasyMySQL-app-9.9.9.zip", "/download/app"));
        assert!(r.app().is_err());
    }

    #[test]
    fn release_json_from_the_site_is_understood() {
        let r: Release = serde_json::from_str(
            r#"{"version":"4.0.0","published":null,"mariadb":null,"files":{"setup":{"name":"EasyMySQL-Setup-4.0.0.exe","url":"/download/setup","size":10,"sha256":null},"app":{"name":"EasyMySQL-app-4.0.0.zip","url":"/download/app","size":1,"sha256":null},"portable":{"name":"x","url":"/download/portable","size":1,"sha256":null}}}"#,
        )
        .unwrap();
        assert!(r.newer_than("3.5.0").unwrap());
        assert!(r.app().is_ok());
    }

    #[test]
    fn incomplete_or_modified_downloads_cannot_be_installed() {
        let hash = format!("{:x}", Sha256::digest(b"abc"));
        let mut a = Asset { name: String::new(), url: String::new(), size: Some(3), sha256: Some(hash.clone()) };
        assert!(verify_download(3, &hash, &a).is_ok());
        assert!(verify_download(2, &hash, &a).is_err());
        assert!(verify_download(3, &"0".repeat(64), &a).is_err());
        a.sha256 = None;
        assert!(verify_download(3, &hash, &a).is_err());
        a.sha256 = Some(hash.clone());
        a.size = None;
        assert!(verify_download(3, &hash, &a).is_err());
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("easymysql-upd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_program(version: u8) -> Vec<u8> {
        let mut v = b"\x7fELF".to_vec();
        v.extend(std::iter::repeat_n(version, 64));
        v
    }

    #[test]
    fn running_program_is_replaced_and_old_one_kept_until_cleanup() {
        let dir = temp("replace");
        let target = dir.join("easymysql");
        let new = dir.join("new");
        std::fs::write(&target, fake_program(1)).unwrap();
        std::fs::write(&new, fake_program(2)).unwrap();
        replace_program(&new, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), fake_program(2));
        assert_eq!(std::fs::read(old_path(&target)).unwrap(), fake_program(1));
        // zweites Update ueberschreibt die alte Sicherung
        std::fs::write(&new, fake_program(3)).unwrap();
        replace_program(&new, &target).unwrap();
        assert_eq!(std::fs::read(old_path(&target)).unwrap(), fake_program(2));
        // keine Programmdatei: nichts wird veraendert
        std::fs::write(&new, b"hello").unwrap();
        assert!(replace_program(&new, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), fake_program(3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn program_is_extracted_from_the_update_archive() {
        let dir = temp("zip");
        let zip_path = dir.join("update.zip");
        {
            let mut zw = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("README.txt", opts).unwrap();
            zw.write_all(b"hi").unwrap();
            zw.start_file(format!("EasyMySQL/{}", app_file()), opts).unwrap();
            zw.write_all(&fake_program(7)).unwrap();
            zw.finish().unwrap();
        }
        let out = extract_app(&zip_path, &dir).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), fake_program(7));
        let empty = dir.join("empty.zip");
        zip::ZipWriter::new(std::fs::File::create(&empty).unwrap()).finish().unwrap();
        assert!(extract_app(&empty, &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn helper_arguments() {
        assert_eq!(run_helper(&[]), None);
        assert_eq!(run_helper(&["--other".into()]), None);
        assert_eq!(run_helper(&["--apply-update".into(), "/nonexistent/a".into(), "/nonexistent/b".into()]), Some(1));
    }
}
