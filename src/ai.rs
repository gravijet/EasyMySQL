// Native KI-Chats: die offiziellen Programme laufen pro Chat dauerhaft im Hintergrund und liefern
// JSON; die App zeichnet den Verlauf. So antwortet jede weitere Nachricht ohne Neustart.

use crate::mcp::setup::Target;
use eframe::egui;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod codex;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Rolle {
    Assistent,
    Werkzeug,
    Status,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Nachricht {
    pub id: String,
    pub rolle: Rolle,
    pub titel: String,
    pub text: String,
    pub fertig: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Modell {
    pub id: String,
    pub name: String,
    pub beschreibung: String,
    pub aufwand: Vec<String>,
}

/// Befehl fuer die Tab-Vervollstaendigung: `/name` (Claude, Codex) oder `$name` (Codex-Skill)
#[derive(Clone, Debug, Default)]
pub struct Befehl {
    pub name: String,
    pub hinweis: String,
    pub beschreibung: String,
    pub pfad: Option<String>,
}

#[derive(Clone)]
pub enum Anfrage {
    Text(String),
    Kompakt,
}

#[derive(Clone, Default, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Auswahl {
    pub modell: String,
    pub aufwand: String,
}

#[derive(Clone, Default)]
pub struct Stand {
    /// Ausgabe einmaliger Auftraege (Installieren, Anmelden)
    pub text: String,
    pub sitzung: Option<String>,
    pub fehler: Option<String>,
    /// Einmaliger Auftrag beendet
    pub fertig: bool,
    /// Sitzung hat Modelle und Befehle geliefert
    pub bereit: bool,
    /// Eine Antwort laeuft gerade
    pub laeuft: bool,
    /// Eine Antwort ist fertig; die Oberflaeche uebernimmt `nachrichten` in den Verlauf
    pub zug_fertig: bool,
    /// Prozess beendet
    pub beendet: bool,
    pub denkt: bool,
    pub nachrichten: Vec<Nachricht>,
    pub modelle: Vec<Modell>,
    pub befehle: Vec<Befehl>,
    pub tokens: Option<u64>,
    pub kontext: Option<u64>,
    pub modell: Option<String>,
    pub komprimiert: bool,
}

impl Stand {
    fn nachricht(&mut self, id: &str, rolle: Rolle, titel: &str) -> &mut Nachricht {
        if let Some(i) = self.nachrichten.iter().position(|n| n.id == id) {
            return &mut self.nachrichten[i];
        }
        self.nachrichten.push(Nachricht { id: id.into(), rolle, titel: titel.into(), text: String::new(), fertig: false });
        self.nachrichten.last_mut().unwrap()
    }

    fn zug_ende(&mut self) {
        self.laeuft = false;
        self.denkt = false;
        self.zug_fertig = true;
    }
}

fn anfuegen(text: &mut String, zusatz: &str) {
    // shortcut: Ausgabe auf 1 MiB begrenzt, bei Bedarf einen virtuellen Verlauf einfuehren.
    if text.len() < 1024 * 1024 {
        text.push_str(zusatz);
    }
}

/// Kurztitel wie im Terminal: `Read(src/main.rs)`
pub fn werkzeug_titel(name: &str, eingabe: &Value) -> String {
    let arg = ["file_path", "path", "pattern", "command", "query", "url", "description", "sql"]
        .iter()
        .find_map(|k| eingabe[*k].as_str())
        .map(|s| s.lines().next().unwrap_or("").chars().take(120).collect::<String>());
    let name = name.strip_prefix("mcp__").map(|n| n.replace("__", " · ")).unwrap_or_else(|| name.into());
    match arg {
        Some(a) if !a.is_empty() => format!("{name}({a})"),
        _ => name,
    }
}

fn claude_argumente(sitzung: Option<&str>, schreiben: bool, auswahl: &Auswahl) -> Vec<String> {
    let werkzeuge = if schreiben { "Read,Glob,Grep,Edit,Write" } else { "Read,Glob,Grep" };
    let freigaben = format!("{werkzeuge},mcp__easymysql__*");
    let mut args = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--tools",
        werkzeuge,
        "--allowedTools",
        &freigaben,
        "--permission-mode",
        "dontAsk",
        "--include-partial-messages",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    if let Some(id) = sitzung {
        args.extend(["--resume".into(), id.into()]);
    }
    if !auswahl.modell.is_empty() {
        args.extend(["--model".into(), auswahl.modell.clone()]);
    }
    if !auswahl.aufwand.is_empty() {
        args.extend(["--effort".into(), auswahl.aufwand.clone()]);
    }
    args
}

fn claude_initialisieren(v: &Value, stand: &mut Stand) {
    let r = &v["response"]["response"];
    stand.modelle = r["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(Modell {
                id: m["value"].as_str()?.into(),
                name: m["displayName"].as_str()?.into(),
                beschreibung: m["description"].as_str().unwrap_or("").into(),
                aufwand: m["supportedEffortLevels"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(String::from)).collect(),
            })
        })
        // "default" ist die leere Auswahl der Oberflaeche
        .filter(|m| m.id != "default")
        .collect();
    stand.befehle = r["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Befehl {
                name: format!("/{}", c["name"].as_str()?),
                hinweis: c["argumentHint"].as_str().unwrap_or("").into(),
                beschreibung: c["description"].as_str().unwrap_or("").into(),
                pfad: None,
            })
        })
        .collect();
    stand.bereit = true;
}

fn ereignis(zeile: &str, stand: &mut Stand) {
    let Ok(v) = serde_json::from_str::<Value>(zeile) else { return };
    let typ = v["type"].as_str().unwrap_or("");
    if let Some(id) = v["session_id"].as_str().filter(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
        stand.sitzung = Some(id.into());
    }
    match typ {
        "control_response" if v["response"]["request_id"] == "init" => claude_initialisieren(&v, stand),
        "system" if v["subtype"] == "init" => {
            stand.modell = v["model"].as_str().map(str::to_string);
        }
        "system" if v["subtype"] == "compact_boundary" => {
            stand.komprimiert = true;
            stand.tokens = None;
        }
        "stream_event" if v["parent_tool_use_id"].is_null() => {
            let e = &v["event"];
            match e["type"].as_str().unwrap_or("") {
                "message_start" => {
                    // Die Stream-Blocknummer ist nur innerhalb einer Nachricht eindeutig.
                    let id = e["message"]["id"].as_str().unwrap_or("stream");
                    stand.nachricht(id, Rolle::Assistent, "");
                }
                "content_block_start" => stand.denkt = e["content_block"]["type"] == "thinking",
                "content_block_delta" if e["delta"]["type"] == "text_delta" => {
                    stand.denkt = false;
                    if let Some(n) = stand.nachrichten.iter_mut().rev().find(|n| n.rolle == Rolle::Assistent) {
                        anfuegen(&mut n.text, e["delta"]["text"].as_str().unwrap_or(""));
                    }
                }
                _ => {}
            }
        }
        "assistant" if v["parent_tool_use_id"].is_null() => {
            if let Some(modell) = v["message"]["model"].as_str().filter(|m| !m.starts_with('<')) {
                stand.modell = Some(modell.into());
            }
            if let Some(usage) = v["message"]["usage"].as_object() {
                stand.tokens = Some(["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().filter_map(|k| usage.get(*k).and_then(Value::as_u64)).sum());
            }
            let id = v["message"]["id"].as_str().unwrap_or("assistant");
            if let Some(inhalt) = v["message"]["content"].as_array() {
                let text = inhalt.iter().filter(|t| t["type"] == "text").filter_map(|t| t["text"].as_str()).collect::<Vec<_>>().join("\n\n");
                if !text.is_empty() {
                    let n = stand.nachricht(id, Rolle::Assistent, "");
                    n.text = text;
                    n.fertig = true;
                }
                for teil in inhalt.iter().filter(|t| t["type"] == "tool_use") {
                    if let (Some(id), Some(name)) = (teil["id"].as_str(), teil["name"].as_str()) {
                        let titel = werkzeug_titel(name, &teil["input"]);
                        let n = stand.nachricht(id, Rolle::Werkzeug, &titel);
                        n.text = serde_json::to_string_pretty(&teil["input"]).unwrap_or_default();
                    }
                }
            }
        }
        "user" => {
            if let Some(inhalt) = v["message"]["content"].as_array() {
                for teil in inhalt.iter().filter(|t| t["type"] == "tool_result") {
                    if let Some(n) = stand.nachrichten.iter_mut().find(|n| Some(n.id.as_str()) == teil["tool_use_id"].as_str()) {
                        n.fertig = true;
                        let ergebnis = match &teil["content"] {
                            Value::String(s) => s.clone(),
                            Value::Array(a) => a.iter().filter_map(|t| t["text"].as_str()).collect::<Vec<_>>().join("\n"),
                            v => v.to_string(),
                        };
                        anfuegen(&mut n.text, &format!("\n\n{ergebnis}"));
                    }
                }
            }
        }
        "result" => {
            if let Some(usage) = v["modelUsage"].as_object().and_then(|u| stand.modell.as_ref().and_then(|m| u.get(m)).or_else(|| u.values().next())) {
                stand.kontext = usage["contextWindow"].as_u64();
            }
            if v["is_error"] == true && v["subtype"] != "error_during_execution" {
                let fehler = v["result"].as_str().map(String::from).unwrap_or_else(|| v["errors"].to_string());
                // Claude schickt den Fehler zusaetzlich als Antworttext; nur einmal anzeigen
                stand.nachrichten.retain(|n| n.rolle != Rolle::Assistent || n.text.trim() != fehler.trim());
                stand.fehler = Some(fehler);
            } else if stand.nachrichten.is_empty()
                && let Some(text) = v["result"].as_str().filter(|t| !t.trim().is_empty())
            {
                let n = stand.nachricht("ergebnis", Rolle::Assistent, "");
                n.text = text.into();
                n.fertig = true;
            }
            if v["permission_denials"].as_array().is_some_and(|a| !a.is_empty()) {
                stand.fehler = Some(
                    crate::i18n::tr(
                        "Ein Werkzeug wurde nicht freigegeben. Prüfen Sie die Dateifreigabe und die MCP-Einstellungen.",
                        "A tool was denied. Check file access and MCP settings.",
                    )
                    .into(),
                );
            }
            stand.zug_ende();
        }
        "error" => {
            stand.fehler = Some(v["error"]["message"].as_str().or(v["message"].as_str()).unwrap_or(zeile).into());
            stand.zug_ende();
        }
        _ => {}
    }
}

/// Schreibkanal zum Prozess (auch aus dem Lesefaden fuer Codex-Antworten)
#[derive(Default)]
pub(crate) struct Leitung(Option<ChildStdin>);

impl Leitung {
    fn senden(&mut self, v: &Value) -> Result<(), String> {
        let eingabe = self.0.as_mut().ok_or_else(|| crate::i18n::tr("Verbindung zum Assistenten geschlossen.", "Connection to the assistant is closed.").to_string())?;
        writeln!(eingabe, "{v}").and_then(|_| eingabe.flush()).map_err(|e| e.to_string())
    }
}

fn befehl(programm: &Path, args: Vec<String>, ordner: &Path) -> Command {
    let mut cmd = Command::new(programm);
    cmd.args(args).current_dir(ordner).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(path) = crate::dev::process::tool_path(&crate::dev::tools()) {
        cmd.env("PATH", path);
    }
    crate::server::hidden(&mut cmd);
    // Abbrechen beendet unter Unix auch die von der KI gestarteten Unterprozesse.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

fn beenden(prozess: &mut Child) {
    #[cfg(windows)]
    {
        let _ = crate::server::hidden(Command::new("taskkill").args(["/F", "/T", "/PID", &prozess.id().to_string()])).status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", prozess.id())]).status();
    }
    let _ = prozess.kill();
    let _ = prozess.wait();
}

/// Was eine laufende Sitzung festlegt; aendert es sich, startet die Sitzung neu (mit Fortsetzung).
#[derive(Clone, PartialEq, Debug)]
pub struct Konfig {
    pub ordner: PathBuf,
    pub schreiben: bool,
    pub auswahl: Auswahl,
}

enum Protokoll {
    Claude,
    Codex(Box<codex::Protokoll>),
}

/// Dauerhafter Chat-Prozess
pub struct Sitzung {
    pub stand: Arc<Mutex<Stand>>,
    pub konfig: Konfig,
    leitung: Arc<Mutex<Leitung>>,
    protokoll: Arc<Mutex<Protokoll>>,
    prozess: Child,
    leser: Vec<std::thread::JoinHandle<()>>,
    gestoppt: Arc<AtomicBool>,
}

impl Drop for Sitzung {
    fn drop(&mut self) {
        self.gestoppt.store(true, Ordering::Relaxed);
        self.leitung.lock().unwrap().0 = None;
        beenden(&mut self.prozess);
        for leser in self.leser.drain(..) {
            let _ = leser.join();
        }
    }
}

impl Sitzung {
    pub fn starten(ctx: &egui::Context, ziel: Target, konfig: Konfig, sitzung: Option<&str>) -> Result<Self, String> {
        let programm = ziel.program().ok_or_else(|| crate::i18n::tr("Bitte zuerst das Plugin installieren.", "Please install the plugin first.").to_string())?;
        let args = match ziel {
            Target::Claude => claude_argumente(sitzung, konfig.schreiben, &konfig.auswahl),
            Target::Codex => vec!["app-server".into()],
        };
        let cmd = befehl(&programm, args, &konfig.ordner);
        let protokoll = match ziel {
            Target::Claude => Protokoll::Claude,
            Target::Codex => Protokoll::Codex(Box::new(codex::Protokoll::new(&konfig.ordner, sitzung, konfig.schreiben, &konfig.auswahl))),
        };
        Self::mit_prozess(ctx, cmd, protokoll, konfig)
    }

    fn mit_prozess(ctx: &egui::Context, mut cmd: Command, protokoll: Protokoll, konfig: Konfig) -> Result<Self, String> {
        let mut prozess = cmd.spawn().map_err(|e| e.to_string())?;
        let leitung = Arc::new(Mutex::new(Leitung(prozess.stdin.take())));
        let start = match &protokoll {
            Protokoll::Claude => json!({"type":"control_request","request_id":"init","request":{"subtype":"initialize"}}),
            Protokoll::Codex(_) => codex::Protokoll::initialisieren(),
        };
        if let Err(e) = leitung.lock().unwrap().senden(&start) {
            beenden(&mut prozess);
            return Err(e);
        }
        let stand = Arc::new(Mutex::new(Stand { laeuft: false, ..Default::default() }));
        let protokoll = Arc::new(Mutex::new(protokoll));
        let gestoppt = Arc::new(AtomicBool::new(false));
        let fehlerausgabe = Arc::new(Mutex::new(String::new()));
        let mut leser = Vec::new();
        if let Some(stderr) = prozess.stderr.take() {
            let fehlerausgabe = fehlerausgabe.clone();
            leser.push(std::thread::spawn(move || {
                for zeile in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut f = fehlerausgabe.lock().unwrap();
                    // Nur das Ende ist fuer Fehlermeldungen interessant.
                    if f.len() > 16 * 1024 {
                        f.clear();
                    }
                    f.push_str(&zeile);
                    f.push('\n');
                }
            }));
        }
        if let Some(stdout) = prozess.stdout.take() {
            let (stand, protokoll, leitung, gestoppt, ctx) = (stand.clone(), protokoll.clone(), leitung.clone(), gestoppt.clone(), ctx.clone());
            leser.push(std::thread::spawn(move || {
                for zeile in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                    let mut p = protokoll.lock().unwrap();
                    let mut s = stand.lock().unwrap();
                    match &mut *p {
                        Protokoll::Claude => ereignis(&zeile, &mut s),
                        Protokoll::Codex(c) => c.ereignis(&zeile, &mut s, &mut leitung.lock().unwrap()),
                    }
                    drop((s, p));
                    ctx.request_repaint();
                }
                let mut s = stand.lock().unwrap();
                s.beendet = true;
                if !gestoppt.load(Ordering::Relaxed) && s.fehler.is_none() {
                    // stderr kurz nachlaufen lassen, damit die Meldung vollstaendig ist
                    std::thread::sleep(Duration::from_millis(100));
                    let err = fehlerausgabe.lock().unwrap();
                    s.fehler = Some(if err.trim().is_empty() {
                        crate::i18n::tr(
                            "Der Assistent wurde beendet. Bitte Anmeldung prüfen und erneut senden.",
                            "The assistant exited. Please check your login and send again.",
                        )
                        .into()
                    } else {
                        err.trim().chars().rev().take(4000).collect::<Vec<_>>().into_iter().rev().collect()
                    });
                }
                if s.laeuft {
                    s.zug_ende();
                }
                ctx.request_repaint();
            }));
        }
        Ok(Self { stand, konfig, leitung, protokoll, prozess, leser, gestoppt })
    }

    pub fn senden(&self, anfrage: Anfrage, befehle: &[Befehl]) -> Result<(), String> {
        let mut p = self.protokoll.lock().unwrap();
        let mut leitung = self.leitung.lock().unwrap();
        {
            let mut s = self.stand.lock().unwrap();
            if s.beendet {
                return Err(s.fehler.clone().unwrap_or_else(|| crate::i18n::tr("Der Assistent wurde beendet.", "The assistant exited.").into()));
            }
            s.fehler = None;
            s.laeuft = true;
            s.zug_fertig = false;
            s.komprimiert = false;
        }
        let ergebnis = match &mut *p {
            Protokoll::Claude => {
                let text = match anfrage {
                    Anfrage::Text(t) => t,
                    Anfrage::Kompakt => "/compact".into(),
                };
                leitung.senden(&json!({"type":"user","message":{"role":"user","content":text},"parent_tool_use_id":null}))
            }
            Protokoll::Codex(c) => c.senden(anfrage, befehle, &mut leitung),
        };
        if let Err(e) = &ergebnis {
            let mut s = self.stand.lock().unwrap();
            s.fehler = Some(e.clone());
            s.laeuft = false;
        }
        ergebnis
    }

    /// Bricht die laufende Antwort ab, die Sitzung bleibt bestehen.
    pub fn unterbrechen(&self) {
        let mut p = self.protokoll.lock().unwrap();
        let mut leitung = self.leitung.lock().unwrap();
        let ergebnis = match &mut *p {
            Protokoll::Claude => leitung.senden(&json!({"type":"control_request","request_id":"stop","request":{"subtype":"interrupt"}})),
            Protokoll::Codex(c) => c.unterbrechen(&mut leitung),
        };
        let mut s = self.stand.lock().unwrap();
        if let Err(e) = ergebnis {
            s.fehler = Some(e);
        }
        s.zug_ende();
        s.nachrichten.push(Nachricht { id: "stop".into(), rolle: Rolle::Status, titel: String::new(), text: crate::i18n::tr("Abgebrochen", "Interrupted").into(), fertig: true });
    }
}

/// Einmaliger Auftrag: Installieren oder Anmelden
pub struct Auftrag {
    pub stand: Arc<Mutex<Stand>>,
    pub abbruch: Arc<AtomicBool>,
    arbeiter: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Auftrag {
    fn drop(&mut self) {
        self.abbruch.store(true, Ordering::Relaxed);
        if let Some(arbeiter) = self.arbeiter.take() {
            let _ = arbeiter.join();
        }
    }
}

impl Auftrag {
    pub fn installieren(ctx: &egui::Context, ziel: Target) -> Self {
        let auftrag = Self { stand: Arc::default(), abbruch: Arc::default(), arbeiter: None };
        let (stand, abbruch, ctx) = (auftrag.stand.clone(), auftrag.abbruch.clone(), ctx.clone());
        std::thread::spawn(move || {
            let ergebnis = crate::dev::install::install_ai(ziel, &abbruch);
            let mut s = stand.lock().unwrap();
            match ergebnis {
                Ok(_) => s.text = crate::tr_format!("{} ist bereit.", "{} is ready.", ziel.label()),
                Err(e) => s.fehler = Some(e),
            }
            s.fertig = true;
            ctx.request_repaint();
        });
        auftrag
    }

    pub fn anmelden(ctx: &egui::Context, ziel: Target, ordner: &Path) -> Result<Self, String> {
        let programm = ziel.program().ok_or_else(|| crate::i18n::tr("Bitte zuerst das Plugin installieren.", "Please install the plugin first.").to_string())?;
        let args = match ziel {
            Target::Claude => vec!["auth".into(), "login".into()],
            Target::Codex => vec!["login".into()],
        };
        Self::ausfuehren(ctx, befehl(&programm, args, ordner))
    }

    /// Fuehrt `cmd` aus und sammelt die Ausgabe zeilenweise in `stand.text`.
    fn ausfuehren(ctx: &egui::Context, mut cmd: Command) -> Result<Self, String> {
        let mut prozess = cmd.spawn().map_err(|e| e.to_string())?;
        drop(prozess.stdin.take());
        let mut auftrag = Self { stand: Arc::default(), abbruch: Arc::default(), arbeiter: None };
        let mut leser = Vec::new();
        for stream in [prozess.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), prozess.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)]
            .into_iter()
            .flatten()
        {
            let (stand, ctx) = (auftrag.stand.clone(), ctx.clone());
            leser.push(std::thread::spawn(move || {
                for zeile in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                    anfuegen(&mut stand.lock().unwrap().text, &format!("{zeile}\n"));
                    ctx.request_repaint();
                }
            }));
        }
        let (stand, abbruch, ctx) = (auftrag.stand.clone(), auftrag.abbruch.clone(), ctx.clone());
        auftrag.arbeiter = Some(std::thread::spawn(move || {
            let status = loop {
                if abbruch.load(Ordering::Relaxed) {
                    beenden(&mut prozess);
                    let mut s = stand.lock().unwrap();
                    s.fehler = Some(crate::i18n::tr("Abgebrochen.", "Cancelled.").into());
                    s.fertig = true;
                    ctx.request_repaint();
                    return;
                }
                match prozess.try_wait() {
                    Ok(Some(status)) if leser.iter().all(|l| l.is_finished()) => break Ok(status),
                    Err(e) => {
                        beenden(&mut prozess);
                        break Err(e);
                    }
                    Ok(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            };
            for leser in leser {
                let _ = leser.join();
            }
            let mut s = stand.lock().unwrap();
            if !status.is_ok_and(|s| s.success()) && s.fehler.is_none() {
                s.fehler = Some(crate::i18n::tr("Der Vorgang ist fehlgeschlagen.", "The operation failed.").into());
            }
            s.fertig = true;
            ctx.request_repaint();
        }));
        Ok(auftrag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warte(stand: &Arc<Mutex<Stand>>, bis: impl Fn(&Stand) -> bool) {
        let start = std::time::Instant::now();
        while !bis(&stand.lock().unwrap()) {
            assert!(start.elapsed() < Duration::from_secs(60), "Zeitlimit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "startet die installierten Programme (Claude und Codex), keine Modellanfrage"]
    fn e2e_modelle_und_befehle() {
        for ziel in [Target::Claude, Target::Codex] {
            let konfig = Konfig { ordner: std::env::temp_dir(), schreiben: false, auswahl: Auswahl::default() };
            let s = Sitzung::starten(&egui::Context::default(), ziel, konfig, None).unwrap();
            warte(&s.stand, |s| s.bereit || s.beendet);
            let s = s.stand.lock().unwrap();
            assert!(s.fehler.is_none(), "{:?}", s.fehler);
            assert!(!s.modelle.is_empty() && !s.befehle.is_empty());
            eprintln!("{}: {} Modelle, {} Befehle", ziel.label(), s.modelle.len(), s.befehle.len());
        }
    }

    #[test]
    fn claude_init_liefert_modelle_aufwand_und_befehle() {
        let mut s = Stand::default();
        ereignis(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{"models":[{"value":"default","displayName":"Default"},{"value":"opus","displayName":"Opus 5.5","description":"Komplex","supportedEffortLevels":["low","max"]},{"value":"haiku","displayName":"Haiku"}],"commands":[{"name":"compact","description":"Kontext","argumentHint":"<anweisung>"}]}}}"#,
            &mut s,
        );
        assert!(s.bereit);
        assert_eq!(s.modelle.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["opus", "haiku"]);
        assert_eq!(s.modelle[0].aufwand, ["low", "max"]);
        assert_eq!(s.befehle[0].name, "/compact");
        assert_eq!(s.befehle[0].hinweis, "<anweisung>");
    }

    #[test]
    fn claude_streaming_wird_durch_die_echte_endnachricht_ersetzt() {
        let mut s = Stand { laeuft: true, ..Default::default() };
        for zeile in [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"thinking"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hal"}}}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Hallo"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"Main.java"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"class Main {}"}]}]}}"#,
            r#"{"type":"system","subtype":"compact_boundary","session_id":"s1"}"#,
        ] {
            ereignis(zeile, &mut s);
        }
        assert_eq!(s.nachrichten.len(), 2);
        assert_eq!(s.nachrichten[0].text, "Hallo");
        assert!(!s.denkt);
        assert_eq!(s.nachrichten[1].titel, "Read(Main.java)");
        assert!(s.nachrichten[1].fertig && s.nachrichten[1].text.contains("class Main {}"));
        assert!(s.komprimiert && s.laeuft);
        ereignis(r#"{"type":"result","session_id":"def-456","result":"Hallo","is_error":false}"#, &mut s);
        assert!(!s.laeuft && s.zug_fertig);
        assert_eq!(s.sitzung.as_deref(), Some("def-456"));
        ereignis(r#"{"type":"result","is_error":true,"subtype":"error_during_execution","result":""}"#, &mut s);
        assert!(s.fehler.is_none(), "Abbruch durch den Nutzer ist kein Fehler");
        s.nachrichten.push(Nachricht { id: "x".into(), rolle: Rolle::Assistent, titel: String::new(), text: "Limit erreicht".into(), fertig: true });
        ereignis(r#"{"type":"result","is_error":true,"result":"Limit erreicht"}"#, &mut s);
        assert!(s.nachrichten.iter().all(|n| n.text != "Limit erreicht"), "Fehler nicht doppelt");
        assert_eq!(s.fehler.as_deref(), Some("Limit erreicht"));
        ereignis("kein JSON", &mut s);
    }

    #[cfg(unix)]
    #[test]
    fn sitzung_bleibt_offen_und_beantwortet_mehrere_nachrichten() {
        // Ein Mini-"Claude": beantwortet jede Zeile mit einem Ergebnis, ohne neu zu starten.
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            r#"read init; n=0; while read zeile; do n=$((n+1)); printf '{"type":"result","session_id":"s-1","result":"Antwort %s","is_error":false}\n' $n; done"#,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let konfig = Konfig { ordner: ".".into(), schreiben: false, auswahl: Auswahl::default() };
        let s = Sitzung::mit_prozess(&egui::Context::default(), cmd, Protokoll::Claude, konfig).unwrap();
        for n in 1..=2 {
            s.senden(Anfrage::Text("Hallo $(exit 9)".into()), &[]).unwrap();
            warte(&s.stand, |s| s.zug_fertig);
            let mut st = s.stand.lock().unwrap();
            assert_eq!(st.nachrichten.pop().unwrap().text, format!("Antwort {n}"));
            assert!(!st.beendet && st.fehler.is_none());
        }
        let stand = s.stand.clone();
        let start = std::time::Instant::now();
        drop(s);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(stand.lock().unwrap().beendet);
    }

    #[test]
    fn claude_rechte_ohne_bypass_und_ohne_shell() {
        for schreiben in [false, true] {
            let args = claude_argumente(Some("abc-123"), schreiben, &Auswahl { modell: "opus".into(), aufwand: "high".into() });
            assert!(args.iter().any(|a| a == "abc-123"));
            assert!(!args.iter().any(|a| a.contains("bypass") || a.contains("danger")));
            let werkzeuge = &args[args.iter().position(|a| a == "--tools").unwrap() + 1];
            assert_eq!(werkzeuge.contains("Edit"), schreiben);
            assert!(!werkzeuge.contains("Bash"));
            assert!(args.windows(2).any(|w| w == ["--model", "opus"]) && args.windows(2).any(|w| w == ["--effort", "high"]));
        }
    }
}
