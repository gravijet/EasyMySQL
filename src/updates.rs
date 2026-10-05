// GitHub Releases pruefen und das Windows-Setup im Hintergrund herunterladen.

use eframe::egui;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const API: &str = "https://api.github.com/repos/gravijet/EasyMySQL/releases/latest";
pub const RELEASES: &str = "https://github.com/gravijet/EasyMySQL/releases";
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_SETUP: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

impl Release {
    fn version(&self) -> Result<Version, String> {
        Version::parse(self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name)).map_err(|_| "Das GitHub Release hat keine gültige Versionsnummer.".into())
    }

    fn newer_than(&self, current: &str) -> Result<bool, String> {
        let version = self.version()?;
        let current = Version::parse(current).map_err(|e| e.to_string())?;
        Ok(!self.draft && !self.prerelease && version.pre.is_empty() && version.cmp_precedence(&current).is_gt())
    }

    fn setup(&self) -> Result<Asset, String> {
        let version = self.version()?;
        let name = format!("EasyMySQL-Setup-{version}.exe");
        self.assets
            .iter()
            .find(|a| a.name == name && trusted_download(&a.browser_download_url))
            .cloned()
            .ok_or_else(|| "Für dieses Release ist noch kein Windows-Setup verfügbar. Bitte später erneut prüfen.".into())
    }
}

fn trusted_download(url: &str) -> bool {
    url.starts_with("https://github.com/gravijet/EasyMySQL/releases/download/")
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
    let response = agent(Duration::from_secs(20))
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call();
    let mut response = match response {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(ureq::Error::StatusCode(403 | 429)) => return Err("GitHub begrenzt gerade die Update-Anfragen. Bitte später erneut versuchen.".into()),
        Err(e) => return Err(format!("GitHub ist nicht erreichbar: {e}")),
    };
    let release: Release = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GitHub-Antwort konnte nicht gelesen werden: {e}"))?;
    if release.newer_than(env!("CARGO_PKG_VERSION"))? {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn verify_download(size: u64, hash: &str, asset: &Asset) -> Result<(), String> {
    if size != asset.size || size == 0 {
        return Err("Der Download ist unvollständig. Bitte erneut herunterladen.".into());
    }
    let expected = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("GitHub liefert keine gültige SHA-256-Prüfsumme für das Setup.")?;
    if !hash.eq_ignore_ascii_case(expected) {
        return Err("Die Prüfsumme des Downloads stimmt nicht. Das Setup wird nicht gestartet.".into());
    }
    Ok(())
}

fn download(release: &Release) -> Result<PathBuf, String> {
    let asset = release.setup()?;
    if asset.size == 0 || asset.size > MAX_SETUP {
        return Err("Die Größe des Windows-Setups ist ungültig.".into());
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
            .get(&asset.browser_download_url)
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().as_reader().take(asset.size + 1);
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
        self.status = "Suche nach Updates auf GitHub …".into();
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
                    self.status = format!("EasyMySQL {} ist verfügbar.", release.tag_name);
                    if self.available.as_ref().map(|r| &r.tag_name) != Some(&release.tag_name) {
                        self.ready = None;
                        self.open = true;
                    }
                    self.available = Some(release);
                }
                Ok(None) => {
                    self.available = None;
                    self.ready = None;
                    self.status = "EasyMySQL ist auf dem neuesten Stand.".into();
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
                    self.status = "Das Update ist heruntergeladen und geprüft.".into();
                }
                Err(e) => self.status = format!("Download fehlgeschlagen: {e}"),
            }
        }
    }

    /// true: Nutzer hat die Installation angefordert.
    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        let mut open = self.open;
        let mut install = false;
        egui::Window::new("EasyMySQL aktualisieren")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label(format!("Installierte Version: {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label(&self.status);
                if self.check_job.is_some() || self.download_job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if self.download_job.is_some() {
                            "Setup wird heruntergeladen …"
                        } else {
                            "GitHub wird geprüft …"
                        });
                    });
                } else if self.ready.is_some() && cfg!(windows) {
                    ui.label(
                        "Zum Installieren werden Ihre Dateien gespeichert und der Datenbankserver sauber beendet. Anschließend öffnet sich das Windows-Setup.",
                    );
                    install = ui.button("Update installieren").clicked();
                } else if let Some(release) = self.available.clone() {
                    if cfg!(windows) {
                        match release.setup() {
                            Ok(_) => {
                                if ui.button("Update herunterladen").clicked() {
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
                        .add_enabled(self.check_job.is_none() && self.download_job.is_none(), egui::Button::new("Erneut prüfen"))
                        .clicked()
                    {
                        self.check_now(ctx, true);
                    }
                    ui.hyperlink_to("GitHub Releases öffnen", RELEASES);
                });
            });
        self.open = open;
        install
    }

    pub fn launch(&self) -> Result<(), String> {
        let path = self.ready.as_deref().ok_or("Es wurde noch kein Setup heruntergeladen.")?;
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
        Err("Das Windows-Setup wurde nicht gestartet (möglicherweise wurde die Administratorfreigabe abgebrochen).".into())
    }
}

#[cfg(not(windows))]
fn launch_setup(_path: &Path) -> Result<(), String> {
    Err("Das automatische Setup ist nur unter Windows verfügbar. Bitte GitHub Releases öffnen.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        serde_json::from_value(serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false,"assets":[]})).unwrap()
    }

    #[test]
    fn versions_are_compared_numerically_and_only_stable_releases_update() {
        assert!(release("v3.10.0").newer_than("3.9.0").unwrap());
        assert!(!release("3.0.1").newer_than("3.0.1").unwrap());
        assert!(!release("v2.9.0").newer_than("3.0.1").unwrap());
        assert!(!release("v4.0.0-beta.1").newer_than("3.0.1").unwrap());
        assert!(!release("v3.0.1+build.2").newer_than("3.0.1").unwrap());
        assert!(release("latest").newer_than("3.0.1").is_err());
        let mut r = release("v4.0.0");
        r.draft = true;
        assert!(!r.newer_than("3.0.1").unwrap());
        r.draft = false;
        r.prerelease = true;
        assert!(!r.newer_than("3.0.1").unwrap());
    }

    #[test]
    fn setup_requires_exact_asset_and_repository() {
        let mut r = release("v3.1.0");
        assert!(r.setup().is_err());
        r.assets.push(Asset {
            name: "EasyMySQL-Setup-3.1.0.exe".into(),
            browser_download_url: "https://github.com/gravijet/EasyMySQL/releases/download/v3.1.0/setup.exe".into(),
            size: 3,
            digest: None,
        });
        assert!(r.setup().is_ok());
        r.assets[0].browser_download_url = "https://github.com.evil.test/gravijet/EasyMySQL/releases/download/x".into();
        assert!(r.setup().is_err());
    }

    #[test]
    fn incomplete_or_modified_downloads_cannot_be_installed() {
        let hash = format!("{:x}", Sha256::digest(b"abc"));
        let mut a = Asset {
            name: String::new(),
            browser_download_url: String::new(),
            size: 3,
            digest: Some(format!("sha256:{hash}")),
        };
        assert!(verify_download(3, &hash, &a).is_ok());
        assert!(verify_download(2, &hash, &a).is_err());
        assert!(verify_download(3, &"0".repeat(64), &a).is_err());
        a.digest = None;
        assert!(verify_download(3, &hash, &a).is_err());
    }
}
