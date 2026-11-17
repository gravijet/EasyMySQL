// Lokale KI-Vervollstaendigung ueber Ollama (kostenlos, laeuft offline auf diesem Rechner). Ist Ollama nicht
// installiert oder das Modell nicht geladen, passiert nichts; die Einstellungen zeigen, wie man es einrichtet.
// Anfragen laufen in einem eigenen Thread; das Ergebnis gilt nur, solange Text und Cursor unveraendert sind.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const URL: &str = "http://127.0.0.1:11434";
/// Klein (1 GB), schnell genug ohne Grafikkarte und fuer Code mit Lueckenfuellen (fill in the middle) trainiert
pub const DEFAULT_MODEL: &str = "qwen2.5-coder:1.5b";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Unknown,
    /// Ollama laeuft nicht
    Off,
    /// Ollama laeuft, das Modell fehlt
    NoModel,
    Ready,
}

static STATUS: Mutex<(Status, Option<Instant>, String)> = Mutex::new((Status::Unknown, None, String::new()));

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

/// Zustand (wird hoechstens alle 30 s im Hintergrund neu geprueft)
pub fn status(model: &str) -> Status {
    let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    let stale = s.1.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) || s.2 != model;
    if stale {
        s.1 = Some(Instant::now());
        s.2 = model.to_string();
        let model = model.to_string();
        std::thread::spawn(move || {
            let st = check(&model);
            let mut s = STATUS.lock().unwrap_or_else(|e| e.into_inner());
            if s.2 == model {
                s.0 = st;
            }
        });
    }
    s.0
}

fn check(model: &str) -> Status {
    let Ok(mut r) = agent(Duration::from_millis(800)).get(&format!("{URL}/api/tags")).call() else { return Status::Off };
    let Ok(v) = r.body_mut().read_json::<serde_json::Value>() else { return Status::Off };
    let names: Vec<&str> = v["models"].as_array().map(|a| a.iter().filter_map(|m| m["name"].as_str()).collect()).unwrap_or_default();
    if names.iter().any(|n| model_matches(n, model)) { Status::Ready } else { Status::NoModel }
}

fn model_matches(installed: &str, wanted: &str) -> bool {
    installed == wanted || (!wanted.contains(':') && installed.split(':').next() == Some(wanted))
}

/// Ergebnis einer Anfrage: (Nummer der Anfrage, Cursorstelle, Vorschlag)
pub type Slot = Arc<Mutex<Option<(u64, usize, String)>>>;

/// Vorschlag fuer die Stelle zwischen `prefix` und `suffix` anfordern
pub fn request(ctx: eframe::egui::Context, model: String, prefix: String, suffix: String, at: usize, id: u64, slot: Slot) {
    std::thread::spawn(move || {
        let body = serde_json::json!({
            "model": model,
            "prompt": prefix,
            "suffix": suffix,
            "stream": false,
            "keep_alive": "30m",
            "options": { "temperature": 0.2, "num_predict": 160, "stop": ["\n\n\n"] }
        });
        let Ok(mut r) = agent(Duration::from_secs(20)).post(&format!("{URL}/api/generate")).send_json(&body) else { return };
        let Ok(v) = r.body_mut().read_json::<serde_json::Value>() else { return };
        if let Some(text) = v["response"].as_str().and_then(|t| clean(t, &suffix)) {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, at, text));
            ctx.request_repaint();
        }
    });
}

/// Antwort kuerzen: hoechstens 15 Zeilen, nichts, was schon hinter dem Cursor steht
pub fn clean(t: &str, suffix: &str) -> Option<String> {
    let t = t.replace("\r\n", "\n");
    let mut lines: Vec<&str> = t.lines().take(15).collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let out = lines.join("\n");
    let rest = suffix.trim_start();
    if out.trim().is_empty() || (!rest.is_empty() && rest.starts_with(out.trim())) {
        return None;
    }
    Some(out)
}

/// Kurze Beschreibung der Datenbank fuer die KI (als SQL-Kommentar vor dem Text)
pub fn schema_hint(schema: Option<&crate::db::Schema>, comment: &str) -> String {
    let Some(s) = schema else { return String::new() };
    let mut out = format!("{comment} MariaDB, Datenbank {}\n", s.name);
    for t in s.tables.iter().take(40) {
        let cols: Vec<String> = t.columns.iter().map(|c| format!("{} {}{}", c.name, c.col_type, if c.is_pk() { " PK" } else { "" })).collect();
        out.push_str(&format!("{comment} {}({})\n", t.name, cols.join(", ")));
    }
    for f in s.fks.iter().take(60) {
        out.push_str(&format!("{comment} {}.{} -> {}.{}\n", f.table, f.columns.join(","), f.ref_table, f.ref_columns.join(",")));
    }
    out
}

static PULLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Wird das Modell gerade geladen?
pub fn pulling() -> bool {
    PULLING.load(std::sync::atomic::Ordering::Relaxed)
}

/// Modell ueber Ollama laden (einmalig, etwa 1 GB); danach wird der Zustand neu geprueft
pub fn pull(model: &str) {
    if PULLING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let model = model.to_string();
    std::thread::spawn(move || {
        let body = serde_json::json!({ "model": model, "stream": false });
        let _ = agent(Duration::from_secs(3600)).post(&format!("{URL}/api/pull")).send_json(&body);
        STATUS.lock().unwrap_or_else(|e| e.into_inner()).1 = None;
        PULLING.store(false, std::sync::atomic::Ordering::Relaxed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_trimmed() {
        assert_eq!(clean("SELECT *\nFROM t;\n\n", "").as_deref(), Some("SELECT *\nFROM t;"));
        assert_eq!(clean("  \n", ""), None);
        assert_eq!(clean(")", ");"), None);
        assert!(model_matches("qwen2.5-coder:1.5b", DEFAULT_MODEL));
        assert!(model_matches("qwen2.5-coder:latest", "qwen2.5-coder"));
        assert!(!model_matches("llama3:8b", DEFAULT_MODEL));
    }
}
