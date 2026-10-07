// Nach neuen Versionen suchen und das Windows-Setup im Hintergrund herunterladen.
// Quelle ist die Website von EasyMySQL: /api/release liefert Version und Prüfsumme,
// /download/setup das Setup.

use eframe::egui;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const SITE: &str = "https://mysql.benjaminberger.at";
const API: &str = "https://mysql.benjaminberger.at/api/release";
pub const DOWNLOAD_PAGE: &str = "https://mysql.benjaminberger.at/#download";
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_SETUP: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    url: String,
    size: Option<u64>,
    sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Files {
    setup: Option<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
struct Release {
    version: String,
    files: Files,
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

    fn setup(&self) -> Result<Asset, String> {
        let version = self.version()?;
        let name = format!("EasyMySQL-Setup-{version}.exe");
        self.files
            .setup
            .iter()
            .find(|a| a.name == name && a.url.starts_with("/download/") && !a.url.contains(['?', '#', '\\']) && !a.url.contains(".."))
            .cloned()
            .ok_or_else(|| crate::i18n::text("Für dieses Release ist noch kein Windows-Setup verfügbar. Bitte später erneut prüfen.").into())
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .user_agent(concat!("EasyMySQL/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn check() -> Result<Option<Release>, String> {
    let response = agent(Duration::from_secs(20)).get(API).header("Accept", "application/json").call();
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
    if release.newer_than(env!("CARGO_PKG_VERSION"))? {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn verify_download(size: u64, hash: &str, asset: &Asset) -> Result<(), String> {
    if size == 0 || asset.size != Some(size) {
        return Err(crate::i18n::text("Der Download ist unvollständig. Bitte erneut herunterladen.").into());
    }
    let expected = asset
        .sha256
        .as_deref()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(crate::i18n::text("Für das Setup liegt keine gültige SHA-256-Prüfsumme vor."))?;
    if !hash.eq_ignore_ascii_case(expected) {
        return Err(crate::i18n::text("Die Prüfsumme des Downloads stimmt nicht. Das Setup wird nicht gestartet.").into());
    }
    Ok(())
}

fn download(release: &Release) -> Result<PathBuf, String> {
    let asset = release.setup()?;
    let expected_size = asset.size.unwrap_or(0);
    if expected_size == 0 || expected_size > MAX_SETUP {
        return Err(crate::i18n::text("Die Größe des Windows-Setups ist ungültig.").into());
    }
    // Privater, pro Download eindeutiger Ordner statt einer gemeinsam benutzten EXE.
    let dir = std::env::temp_dir().join(format!(
        "EasyMySQL-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let partial = dir.join("setup.part");
    let target = dir.join(&asset.name);
    let result = (|| {
        let mut response = agent(Duration::from_secs(15 * 60))
            .get(format!("{SITE}{}", asset.url))
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader().take(expected_size + 1);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|e| e.to_string())?;
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
        }
        verify_download(size, &format!("{:x}", hash.finalize()), &asset)?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

#[derive(Default)]
pub struct Updater {
    last_check: Option<Instant>,
    check_job: Option<Receiver<Result<Option<Release>, String>>>,
    download_job: Option<Receiver<Result<PathBuf, String>>>,
    available: Option<Release>,
    ready: Option<PathBuf>,
    status: String,
    pub open: bool,
}

impl Updater {
    pub fn check_now(&mut self, ctx: &egui::Context, manual: bool) {
        self.open |= manual;
        if self.check_job.is_some() || self.download_job.is_some() {
            return;
        }
        self.last_check = Some(Instant::now());
        self.status = crate::i18n::text("Suche nach Updates …").into();
        let (tx, rx) = mpsc::channel();
        self.check_job = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check());
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self, ctx: &egui::Context, auto: bool) {
        if auto && self.last_check.is_none_or(|t| t.elapsed() >= INTERVAL) {
            self.check_now(ctx, false);
        }
        if auto {
            ctx.request_repaint_after(Duration::from_secs(60));
        }
        if let Some(result) = self.check_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.check_job = None;
            match result {
                Ok(Some(release)) => {
                    self.status = crate::tr_format!("EasyMySQL {} ist verfügbar.", "EasyMySQL {} is available.", release.version);
                    if self.available.as_ref().map(|r| &r.version) != Some(&release.version) {
                        self.ready = None;
                        self.open = true;
                    }
                    self.available = Some(release);
                }
                Ok(None) => {
                    self.available = None;
                    self.ready = None;
                    self.status = crate::i18n::text("EasyMySQL ist auf dem neuesten Stand.").into();
                }
                Err(e) => self.status = e,
            }
        }
        if let Some(result) = self.download_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.download_job = None;
            self.open = true;
            match result {
                Ok(path) => {
                    self.ready = Some(path);
                    self.status = crate::i18n::text("Das Update ist heruntergeladen und geprüft.").into();
                }
                Err(e) => self.status = crate::tr_format!("Download fehlgeschlagen: {e}", "Download failed: {e}"),
            }
        }
    }

    /// true: Nutzer hat die Installation angefordert.
    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        let mut open = self.open;
        let mut install = false;
        egui::Window::new(crate::i18n::text("EasyMySQL aktualisieren"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(crate::tr_format!("Installierte Version: {}", "Installed version: {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label(&self.status);
                if self.check_job.is_some() || self.download_job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if self.download_job.is_some() {
                            crate::i18n::text("Setup wird heruntergeladen …")
                        } else {
                            crate::i18n::text("Update-Server wird geprüft …")
                        });
                    });
                } else if self.ready.is_some() && cfg!(windows) {
                    ui.label(
                        crate::i18n::text("Zum Installieren werden Ihre Dateien gespeichert und der Datenbankserver sauber beendet. Anschließend öffnet sich das Windows-Setup."),
                    );
                    install = ui.button(crate::i18n::text("Update installieren")).clicked();
                } else if let Some(release) = self.available.clone() {
                    if cfg!(windows) {
                        match release.setup() {
                            Ok(_) => {
                                if ui.button(crate::i18n::text("Update herunterladen")).clicked() {
                                    let (tx, rx) = mpsc::channel();
                                    self.download_job = Some(rx);
                                    let ctx = ctx.clone();
                                    std::thread::spawn(move || {
                                        let _ = tx.send(download(&release));
                                        ctx.request_repaint();
                                    });
                                }
                            }
                            Err(e) => {
                                ui.label(e);
                            }
                        }
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.check_job.is_none() && self.download_job.is_none(), egui::Button::new(crate::i18n::text("Erneut prüfen")))
                        .clicked()
                    {
                        self.check_now(ctx, true);
                    }
                    ui.hyperlink_to(crate::i18n::text("Download-Seite öffnen"), DOWNLOAD_PAGE);
                });
            });
        self.open = open;
        install
    }

    pub fn launch(&self) -> Result<(), String> {
        let path = self.ready.as_deref().ok_or(crate::i18n::text("Es wurde noch kein Setup heruntergeladen."))?;
        launch_setup(path)
    }
}

#[cfg(windows)]
fn launch_setup(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let parameters: Vec<u16> = "/NORESTART\0".encode_utf16().collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(crate::i18n::text("Das Windows-Setup wurde nicht gestartet (möglicherweise wurde die Administratorfreigabe abgebrochen).").into())
    }
}

#[cfg(not(windows))]
fn launch_setup(_path: &Path) -> Result<(), String> {
    Err(crate::i18n::text("Das automatische Setup ist nur unter Windows verfügbar. Bitte die Download-Seite öffnen.").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        serde_json::from_value(serde_json::json!({"version":tag,"files":{}})).unwrap()
    }

    fn asset(url: &str) -> Asset {
        Asset { name: "EasyMySQL-Setup-3.1.0.exe".into(), url: url.into(), size: Some(3), sha256: None }
    }

    #[test]
    fn versions_are_compared_numerically_and_only_stable_releases_update() {
        assert!(release("3.10.0").newer_than("3.9.0").unwrap());
        assert!(!release("3.0.1").newer_than("3.0.1").unwrap());
        assert!(!release("2.9.0").newer_than("3.0.1").unwrap());
        assert!(!release("4.0.0-beta.1").newer_than("3.0.1").unwrap());
        assert!(!release("3.0.1+build.2").newer_than("3.0.1").unwrap());
        assert!(release("latest").newer_than("3.0.1").is_err());
    }

    #[test]
    fn setup_requires_exact_name_and_download_path() {
        let mut r = release("3.1.0");
        assert!(r.setup().is_err());
        r.files.setup = Some(asset("/download/setup"));
        assert!(r.setup().is_ok());
        for bad in ["https://evil.test/download/setup", "//evil.test/download/setup", "/download/../x", "/other", "/download/setup?x=1"] {
            r.files.setup = Some(asset(bad));
            assert!(r.setup().is_err(), "{bad}");
        }
        r.files.setup = Some(Asset { name: "EasyMySQL-Setup-9.9.9.exe".into(), ..asset("/download/setup") });
        assert!(r.setup().is_err());
    }

    #[test]
    fn release_json_from_the_site_is_understood() {
        let r: Release = serde_json::from_str(
            r#"{"version":"3.2.0","published":null,"mariadb":null,"files":{"setup":{"name":"EasyMySQL-Setup-3.2.0.exe","url":"/download/setup","size":10,"sha256":null},"portable":{"name":"x","url":"/download/portable","size":1,"sha256":null}}}"#,
        )
        .unwrap();
        assert!(r.newer_than("3.1.0").unwrap());
        assert!(r.setup().is_ok());
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
}
