// MariaDB in bestehenden Installationen aktualisieren, ohne Administratorrechte: die neue Version kommt in
// den Programmdatenordner (mariadb/<version>/), der Server wechselt dorthin. Vorher wird alles gesichert
// (Sicherung aller Datenbanken und eine Kopie des Datenordners); klappt der Start nicht, kehrt EasyMySQL
// zur bisherigen Version und zum gesicherten Datenordner zurueck.

use crate::dev::install::{Checksum, Progress, download, extract, get_json};
use crate::server::{Server, app_data_dir, find_tool};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Welche Versionen in Frage kommen
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    /// Neueste stabile Version mit Langzeitunterstuetzung (empfohlen)
    Lts,
    /// Neueste stabile Version (auch ohne Langzeitunterstuetzung)
    Stable,
}

impl Track {
    pub fn from_id(s: &str) -> Track {
        if s == "stable" { Track::Stable } else { Track::Lts }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file: String,
    pub sha512: String,
}

/// Version der MariaDB-Programme in `bin` ("mariadbd  Ver 12.3.3-MariaDB …" -> "12.3.3")
pub fn installed_version(bin: &Path) -> Option<String> {
    let out = crate::dev::probe(&find_tool(bin, "mariadbd"), &["--version"])?;
    let after = out.split(" Ver ").nth(1)?;
    let v: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    (!v.is_empty()).then_some(v)
}

fn file_pattern() -> Option<String> {
    if cfg!(windows) && cfg!(target_arch = "x86_64") {
        Some("-winx64.zip".into())
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("-linux-systemd-x86_64.tar.gz".into())
    } else {
        None
    }
}

/// Neueste passende Version laut downloads.mariadb.org
pub fn latest(track: Track) -> Result<Release, String> {
    let pattern = file_pattern().ok_or_else(|| crate::i18n::tr("Für dieses System gibt es keine fertigen MariaDB-Pakete.", "There are no ready-made MariaDB packages for this system.").to_string())?;
    let list = get_json("https://downloads.mariadb.org/rest-api/mariadb/")?;
    let majors = list["major_releases"].as_array().cloned().unwrap_or_default();
    let series = majors
        .iter()
        .filter(|m| m["release_status"].as_str() == Some("Stable"))
        .filter(|m| track == Track::Stable || m["release_support_type"].as_str() == Some("Long Term Support"))
        .filter_map(|m| m["release_id"].as_str().map(str::to_string))
        .max_by(|a, b| crate::dev::version_key(a).cmp(&crate::dev::version_key(b)))
        .ok_or("MariaDB: series")?;
    let rel = get_json(&format!("https://downloads.mariadb.org/rest-api/mariadb/{series}/latest/"))?;
    let (version, info) = rel["releases"].as_object().and_then(|o| o.iter().next()).map(|(k, v)| (k.clone(), v.clone())).ok_or("MariaDB: releases")?;
    let files = info["files"].as_array().cloned().unwrap_or_default();
    let f = files.iter().find(|f| f["file_name"].as_str().is_some_and(|n| n.ends_with(&pattern) && !n.contains("debug"))).ok_or("MariaDB: file")?;
    let file = f["file_name"].as_str().unwrap_or("").to_string();
    let url = f["file_download_url"].as_str().unwrap_or("").replacen("http://", "https://", 1);
    let sha512 = f["checksum"]["sha512sum"].as_str().unwrap_or("").to_ascii_lowercase();
    if url.is_empty() || sha512.len() != 128 {
        return Err("MariaDB: download".into());
    }
    Ok(Release { version, url, file, sha512 })
}

/// Ordner fuer heruntergeladene MariaDB-Versionen
pub fn own_dir() -> PathBuf {
    app_data_dir().join("mariadb")
}

/// Stand des Updates fuer die Anzeige
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub busy: bool,
    pub stage: String,
    pub fraction: Option<f32>,
    pub error: Option<String>,
    pub done: Option<String>,
    /// Gefundene neuere Version (nach "Suchen")
    pub available: Option<Release>,
    pub checked: bool,
}

static STATE: Mutex<Option<Status>> = Mutex::new(None);
/// Neuer Programmordner nach dem Update (die App uebernimmt ihn in die Einstellungen)
static APPLIED: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

pub fn take_applied() -> Option<Option<PathBuf>> {
    APPLIED.lock().unwrap().take()
}

fn stamp() -> PathBuf {
    app_data_dir().join("mariadb-letzte-pruefung.txt")
}

/// Einmal am Tag nach neueren Versionen sehen (je nach Einstellung)
pub fn daily_check(server: &Server, mode: &str, track: Track, ctx: &eframe::egui::Context) {
    static DONE: AtomicBool = AtomicBool::new(false);
    if mode == "aus" || DONE.swap(true, std::sync::atomic::Ordering::Relaxed) || file_pattern().is_none() {
        return;
    }
    let due = std::fs::metadata(stamp()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|a| a > std::time::Duration::from_secs(24 * 3600));
    if due {
        let _ = crate::workspace::atomic_write(&stamp(), &crate::server::timestamp());
        check(server, track, ctx);
    }
}
static PROGRESS: Mutex<Option<Arc<Progress>>> = Mutex::new(None);

pub fn status() -> Status {
    let mut st = STATE.lock().unwrap().clone().unwrap_or_default();
    if let Some(p) = PROGRESS.lock().unwrap().as_ref() {
        let total = p.total.load(std::sync::atomic::Ordering::Relaxed);
        let done = p.done.load(std::sync::atomic::Ordering::Relaxed);
        if total > 0 && st.busy {
            st.fraction = Some(done as f32 / total as f32);
        }
    }
    st
}

fn set(f: impl FnOnce(&mut Status)) {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(Status::default));
}

/// Im Hintergrund nach einer neueren Version sehen
pub fn check(server: &Server, track: Track, ctx: &eframe::egui::Context) {
    let current = server.paths().and_then(|p| installed_version(&p.bin));
    set(|s| {
        s.busy = true;
        s.error = None;
        s.stage = crate::i18n::tr("Suche nach neueren MariaDB-Versionen …", "Looking for newer MariaDB versions …").into();
    });
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let r = latest(track);
        set(|s| {
            s.busy = false;
            s.checked = true;
            s.stage.clear();
            match r {
                Ok(rel) if current.as_ref().is_none_or(|c| crate::dev::is_newer(&rel.version, c)) => s.available = Some(rel),
                Ok(_) => {
                    s.available = None;
                    s.done = Some(crate::i18n::tr("MariaDB ist auf dem neuesten Stand.", "MariaDB is up to date.").into());
                }
                Err(e) => s.error = Some(e),
            }
        });
        ctx.request_repaint();
    });
}

/// Update durchfuehren. `copy_data`: Datenordner vorher zusaetzlich kopieren.
/// `apply_bin` wird mit dem neuen Programmordner (bzw. bei Fehlern dem alten) aufgerufen, damit die
/// Einstellungen ihn speichern.
pub fn run(server: Server, conn: crate::db::ConnInfo, rel: Release, copy_data: bool, ctx: &eframe::egui::Context) {
    let apply_bin = |b: Option<PathBuf>| *APPLIED.lock().unwrap() = Some(b);
    let progress = Arc::new(Progress::default());
    *PROGRESS.lock().unwrap() = Some(Arc::clone(&progress));
    set(|s| {
        s.busy = true;
        s.error = None;
        s.done = None;
    });
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let stage = |t: String| {
            set(|s| s.stage = t.clone());
            server.log(t);
            ctx.request_repaint();
        };
        let result = (|| -> Result<String, String> {
            let paths = server.paths().ok_or("MariaDB")?;
            let old_bin = server.config().bin_dir.clone();
            // 1. Laden und entpacken (der Server laeuft weiter)
            let dir = own_dir();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let archive = dir.join(&rel.file);
            stage(crate::tr_format!("MariaDB {} wird geladen …", "Downloading MariaDB {} …", rel.version));
            download(&rel.url, &archive, &Checksum::Sha512(rel.sha512.clone()), &progress, &AtomicBool::new(false))?;
            stage(crate::tr_format!("MariaDB {} wird entpackt …", "Unpacking MariaDB {} …", rel.version));
            let tmp = dir.join(format!(".neu-{}", rel.version));
            let top = extract(&archive, &tmp)?;
            let target = dir.join(format!("mariadb-{}", rel.version));
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&top, &target).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(&tmp);
            let _ = std::fs::remove_file(&archive);
            let new_bin = target.join("bin");
            if !find_tool(&new_bin, "mariadbd").is_file() {
                return Err(crate::i18n::tr("Im Paket fehlt mariadbd.", "mariadbd is missing in the package.").into());
            }
            // Laeuft die neue Version hier ueberhaupt? (fehlende Bibliotheken unter Linux)
            if installed_version(&new_bin).is_none() {
                return Err(crate::i18n::tr("Die neue MariaDB-Version startet auf diesem System nicht (fehlende Bibliotheken?).", "The new MariaDB version does not run on this system (missing libraries?).").into());
            }
            // 2. Sicherung aller Datenbanken
            if server.state().is_running() {
                stage(crate::i18n::tr("Alle Datenbanken werden gesichert …", "Backing up all databases …").into());
                if let Some(env) = crate::backup::env() {
                    crate::backup::create(&env, None, "vor-mariadb-update", &|l| server.log(l))?;
                }
            }
            // 3. Server beenden, Datenordner kopieren
            stage(crate::i18n::tr("Server wird beendet …", "Stopping the server …").into());
            server.stop_blocking(&conn);
            let copy = paths.data.with_file_name(format!("{}-vor-{}", paths.data.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into()), rel.version));
            if copy_data {
                stage(crate::i18n::tr("Datenordner wird kopiert …", "Copying the data folder …").into());
                let _ = std::fs::remove_dir_all(&copy);
                if let Err(e) = crate::server::copy_dir(&paths.data, &copy) {
                    // Meist zu wenig Platz: die halbe Kopie entfernen und mit der bisherigen Version weiterlaufen
                    let _ = std::fs::remove_dir_all(&copy);
                    let _ = server.start_blocking(&|| ctx.request_repaint());
                    return Err(crate::tr_format!(
                        "Der Datenordner konnte nicht kopiert werden ({e}). Das Update wurde nicht durchgeführt; die bisherige Version läuft weiter.",
                        "The data folder could not be copied ({e}). The update was not applied; the previous version keeps running."
                    ));
                }
            }
            // 4. Mit der neuen Version starten (mariadb-upgrade laeuft beim Start)
            stage(crate::tr_format!("MariaDB {} wird gestartet …", "Starting MariaDB {} …", rel.version));
            let mut cfg = server.config();
            cfg.bin_dir = Some(new_bin.clone());
            server.configure(cfg.clone());
            match server.start_blocking(&|| ctx.request_repaint()) {
                Ok(()) => {
                    apply_bin(Some(new_bin));
                    // Aeltere eigene Versionen entfernen (die bisherige bleibt fuer den Notfall)
                    if let Ok(rd) = std::fs::read_dir(&dir) {
                        for e in rd.flatten() {
                            let p = e.path();
                            if p != target && old_bin.as_ref().is_none_or(|o| !o.starts_with(&p)) && e.file_name().to_string_lossy().starts_with("mariadb-") {
                                let _ = std::fs::remove_dir_all(&p);
                            }
                        }
                    }
                    Ok(crate::tr_format!("MariaDB {} läuft.", "MariaDB {} is running.", rel.version))
                }
                Err(e) => {
                    // Zurueck zur bisherigen Version mit dem gesicherten Datenordner
                    stage(crate::i18n::tr("Start fehlgeschlagen – zurück zur bisherigen Version …", "Start failed – going back to the previous version …").into());
                    server.stop_blocking(&conn);
                    let mut note = String::new();
                    if copy_data && copy.is_dir() {
                        let broken = paths.data.with_file_name(format!("{}-fehlgeschlagen-{}", paths.data.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), crate::server::timestamp().replace([':', ' '], "-")));
                        // Nur wenn die veraenderten Daten zur Seite liegen, die Kopie zurueckspielen - sonst wuerden beide Staende vermischt
                        match std::fs::rename(&paths.data, &broken) {
                            Ok(()) => {
                                if let Err(e) = crate::server::copy_dir(&copy, &paths.data) {
                                    let _ = std::fs::remove_dir_all(&paths.data);
                                    let _ = std::fs::rename(&broken, &paths.data);
                                    note = crate::tr_format!(" Die Sicherungskopie ließ sich nicht zurückspielen ({e}); sie liegt unter {}.", " The backup copy could not be restored ({e}); it is at {}.", copy.display());
                                }
                            }
                            Err(e) => {
                                note = crate::tr_format!(" Die Daten blieben unverändert, weil sie nicht beiseitegelegt werden konnten ({e}); die Kopie liegt unter {}.", " The data was left as it is because it could not be moved aside ({e}); the copy is at {}.", copy.display());
                            }
                        }
                    }
                    cfg.bin_dir = old_bin.clone();
                    server.configure(cfg);
                    apply_bin(old_bin);
                    let _ = server.start_blocking(&|| ctx.request_repaint());
                    Err(format!("{}{note}", crate::tr_format!("MariaDB {} ließ sich nicht starten ({e}). Die bisherige Version läuft wieder.", "MariaDB {} could not be started ({e}). The previous version is running again.", rel.version)))
                }
            }
        })();
        *PROGRESS.lock().unwrap() = None;
        set(|s| {
            s.busy = false;
            s.stage.clear();
            match result {
                Ok(m) => {
                    s.done = Some(m);
                    s.available = None;
                }
                Err(e) => s.error = Some(e),
            }
        });
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_of_the_installed_server() {
        // Mit dem MariaDB des Systems (falls vorhanden)
        if let Some(bin) = ["/usr/sbin", "/usr/bin"].iter().map(Path::new).find(|b| b.join("mariadbd").is_file()) {
            let v = installed_version(bin).unwrap();
            assert!(crate::dev::version_key(&v).len() >= 3, "{v}");
        }
        assert_eq!(Track::from_id("stable"), Track::Stable);
        assert_eq!(Track::from_id(""), Track::Lts);
    }

    /// `cargo test mariadb_release -- --ignored`
    #[test]
    #[ignore = "braucht Internet"]
    fn mariadb_release_is_found() {
        let r = latest(Track::Lts).unwrap();
        assert!(r.url.starts_with("https://") && crate::dev::is_stable(&r.version), "{r:?}");
    }
}
