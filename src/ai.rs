// Native KI-Chats: die offiziellen Programme liefern JSON, die App zeichnet den Verlauf.

use crate::mcp::setup::Target;
use eframe::egui;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Stdio};
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

#[derive(Clone, Debug)]
pub struct Modell {
    pub id: String,
    pub name: String,
    pub aufwand: Vec<String>,
}

#[derive(Clone)]
pub enum Anfrage {
    Text(String),
    Kompakt,
    Modelle,
    Anmelden,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Auswahl {
    pub modell: String,
    pub aufwand: String,
}

#[derive(Clone, Default)]
pub struct Stand {
    pub text: String,
    pub sitzung: Option<String>,
    pub fehler: Option<String>,
    pub fertig: bool,
    pub nachrichten: Vec<Nachricht>,
    pub modelle: Vec<Modell>,
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
}

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

fn argumente(sitzung: Option<&str>, schreiben: bool) -> Vec<String> {
    let werkzeuge = if schreiben { "Read,Glob,Grep,Edit,Write" } else { "Read,Glob,Grep" };
    let freigaben = format!("{werkzeuge},mcp__easymysql__*");
    let mut args =
        ["-p", "--output-format", "stream-json", "--verbose", "--tools", werkzeuge, "--allowedTools", &freigaben, "--permission-mode", "dontAsk", "--include-partial-messages"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
    if let Some(id) = sitzung {
        args.extend(["--resume".into(), id.into()]);
    }
    args
}

fn anfuegen(text: &mut String, zusatz: &str) {
    // shortcut: Ausgabe auf 1 MiB begrenzt, bei Bedarf einen virtuellen Verlauf einfuehren.
    if text.len() < 1024 * 1024 {
        text.push_str(zusatz);
    }
}

fn ereignis(ziel: Target, zeile: &str, stand: &mut Stand) {
    let Ok(v) = serde_json::from_str::<Value>(zeile) else { return };
    let typ = v["type"].as_str().unwrap_or("");
    let id = if ziel == Target::Codex { v["thread_id"].as_str() } else { v["session_id"].as_str() };
    if let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')) {
        stand.sitzung = Some(id.into());
    }
    match (ziel, typ) {
        (Target::Claude, "system") if v["subtype"] == "init" => {
            stand.modell = v["model"].as_str().map(str::to_string);
        }
        (Target::Claude, "system") if v["subtype"] == "compact_boundary" => {
            stand.komprimiert = true;
            stand.tokens = None;
        }
        (Target::Claude, "stream_event") if v["parent_tool_use_id"].is_null() => {
            let e = &v["event"];
            match e["type"].as_str().unwrap_or("") {
                "message_start" => {
                    // Die Stream-Blocknummer ist nur innerhalb einer Nachricht eindeutig.
                    let id = e["message"]["id"].as_str().unwrap_or("stream");
                    stand.nachricht(id, Rolle::Assistent, "");
                }
                "content_block_delta" if e["delta"]["type"] == "text_delta" => {
                    if let Some(n) = stand.nachrichten.last_mut().filter(|n| n.rolle == Rolle::Assistent) {
                        anfuegen(&mut n.text, e["delta"]["text"].as_str().unwrap_or(""));
                    }
                }
                _ => {}
            }
        }
        (Target::Codex, "item.completed") if v["item"]["type"] == "agent_message" => {
            if let Some(text) = v["item"]["text"].as_str() {
                anfuegen(&mut stand.text, &format!("{text}\n\n"));
            }
        }
        (Target::Claude, "assistant") if v["parent_tool_use_id"].is_null() => {
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
                    n.text = text.clone();
                    n.fertig = true;
                    anfuegen(&mut stand.text, &format!("{text}\n\n"));
                }
                for teil in inhalt.iter().filter(|t| t["type"] == "tool_use") {
                    if let (Some(id), Some(name)) = (teil["id"].as_str(), teil["name"].as_str()) {
                        let n = stand.nachricht(id, Rolle::Werkzeug, name);
                        n.text = serde_json::to_string_pretty(&teil["input"]).unwrap_or_default();
                    }
                }
            }
        }
        (Target::Claude, "user") => {
            if let Some(inhalt) = v["message"]["content"].as_array() {
                for teil in inhalt.iter().filter(|t| t["type"] == "tool_result") {
                    if let Some(n) = stand.nachrichten.iter_mut().find(|n| Some(n.id.as_str()) == teil["tool_use_id"].as_str()) {
                        n.fertig = true;
                        anfuegen(&mut n.text, &format!("\n\n{}", teil["content"].as_str().map(str::to_string).unwrap_or_else(|| teil["content"].to_string())));
                    }
                }
            }
        }
        (Target::Claude, "result") => {
            if let Some(usage) = v["modelUsage"].as_object().and_then(|u| stand.modell.as_ref().and_then(|m| u.get(m)).or_else(|| u.values().next())) {
                stand.kontext = usage["contextWindow"].as_u64();
            }
            if v["is_error"] == true {
                stand.fehler = Some(v["result"].as_str().map(String::from).unwrap_or_else(|| v["errors"].to_string()));
            } else if stand.text.is_empty()
                && let Some(text) = v["result"].as_str()
            {
                anfuegen(&mut stand.text, text);
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
        }
        (Target::Codex, "turn.completed") => stand.fehler = None,
        (_, "error" | "turn.failed") => {
            stand.fehler = Some(v["error"]["message"].as_str().or(v["message"].as_str()).unwrap_or(zeile).into());
        }
        _ => {}
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

    pub fn starten(ctx: &egui::Context, ziel: Target, ordner: &Path, sitzung: Option<&str>, schreiben: bool, auswahl: &Auswahl, anfrage: Anfrage) -> Result<Self, String> {
        if matches!(anfrage, Anfrage::Kompakt) && sitzung.is_none() {
            return Err(crate::i18n::tr("Bitte zuerst einen Chat beginnen.", "Please start a conversation first.").into());
        }
        let programm = ziel.program().ok_or_else(|| crate::i18n::tr("Bitte zuerst das Plugin installieren.", "Please install the plugin first.").to_string())?;
        let login = matches!(anfrage, Anfrage::Anmelden);
        let args = if login {
            match ziel {
                Target::Claude => vec!["auth".into(), "login".into()],
                Target::Codex => vec!["login".into()],
            }
        } else if ziel == Target::Codex {
            vec!["app-server".into()]
        } else {
            let mut args = argumente(sitzung, schreiben);
            if !auswahl.modell.is_empty() {
                args.extend(["--model".into(), auswahl.modell.clone()]);
            }
            args
        };
        let mut cmd = Command::new(programm);
        cmd.args(args).current_dir(ordner).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(path) = crate::dev::process::tool_path(&crate::dev::tools()) {
            cmd.env("PATH", path);
        }
        crate::server::hidden(&mut cmd);
        let protokoll = (ziel == Target::Codex && !login).then(|| codex::Protokoll::new(ordner, sitzung, schreiben, auswahl, anfrage.clone()));
        let frage = match anfrage {
            Anfrage::Text(text) => Some(text),
            Anfrage::Kompakt => Some("/compact".into()),
            Anfrage::Modelle => Some(String::new()),
            Anfrage::Anmelden => None,
        };
        Self::ausfuehren_mit_protokoll(ctx, ziel, cmd, frage, protokoll)
    }

    #[cfg(test)]
    fn ausfuehren(ctx: &egui::Context, ziel: Target, cmd: Command, frage: Option<String>) -> Result<Self, String> {
        Self::ausfuehren_mit_protokoll(ctx, ziel, cmd, frage, None)
    }

    fn ausfuehren_mit_protokoll(ctx: &egui::Context, ziel: Target, mut cmd: Command, frage: Option<String>, mut protokoll: Option<codex::Protokoll>) -> Result<Self, String> {
        let login = frage.is_none();
        // Abbrechen beendet unter Unix auch die von der KI gestarteten Unterprozesse.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut prozess = cmd.spawn().map_err(|e| e.to_string())?;
        let mut eingabe = prozess.stdin.take();
        if let Some(p) = &mut protokoll
            && let Err(e) = p.initialisieren(eingabe.take().unwrap())
        {
            let _ = prozess.kill();
            let _ = prozess.wait();
            return Err(e);
        }
        std::thread::spawn(move || {
            if let (Some(mut stdin), Some(frage)) = (eingabe, frage) {
                let _ = stdin.write_all(frage.as_bytes());
            }
        });
        let mut auftrag = Self { stand: Arc::default(), abbruch: Arc::default(), arbeiter: None };
        let fehlerausgabe = Arc::new(Mutex::new(String::new()));
        let protokoll_ende = Arc::new(AtomicBool::new(false));
        let mut leser = Vec::new();
        for (stderr, stream) in [
            (false, prozess.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
            (true, prozess.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
        ] {
            let Some(stream) = stream else { continue };
            let (stand, fehlerausgabe, ctx, ende) = (auftrag.stand.clone(), fehlerausgabe.clone(), ctx.clone(), protokoll_ende.clone());
            let mut protokoll = if !stderr { protokoll.take() } else { None };
            leser.push(std::thread::spawn(move || {
                for zeile in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                    if login {
                        anfuegen(&mut stand.lock().unwrap().text, &format!("{zeile}\n"));
                    } else if stderr {
                        anfuegen(&mut fehlerausgabe.lock().unwrap(), &format!("{zeile}\n"));
                    } else if let Some(p) = &mut protokoll {
                        if p.ereignis(&zeile, &mut stand.lock().unwrap()) {
                            ende.store(true, Ordering::Relaxed);
                        }
                    } else {
                        ereignis(ziel, &zeile, &mut stand.lock().unwrap());
                    }
                    ctx.request_repaint();
                }
            }));
        }
        let (stand, abbruch, ctx) = (auftrag.stand.clone(), auftrag.abbruch.clone(), ctx.clone());
        auftrag.arbeiter = Some(std::thread::spawn(move || {
            let status = loop {
                if abbruch.load(Ordering::Relaxed) || protokoll_ende.load(Ordering::Relaxed) {
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
                    let mut s = stand.lock().unwrap();
                    if abbruch.load(Ordering::Relaxed) {
                        s.fehler = Some(crate::i18n::tr("Abgebrochen.", "Cancelled.").into());
                    }
                    s.fertig = true;
                    ctx.request_repaint();
                    return;
                }
                match prozess.try_wait() {
                    Ok(Some(status)) if leser.iter().all(|l| l.is_finished()) => break Ok(status),
                    Err(e) => {
                        let _ = prozess.kill();
                        let _ = prozess.wait();
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
                let err = fehlerausgabe.lock().unwrap();
                s.fehler = Some(if err.trim().is_empty() {
                    crate::i18n::tr(
                        "Der Assistent konnte die Anfrage nicht abschließen. Bitte Anmeldung prüfen.",
                        "The assistant could not complete the request. Please check your login.",
                    )
                    .into()
                } else {
                    err.trim().to_string()
                });
            }
            if !login && s.text.trim().is_empty() && s.nachrichten.is_empty() && !s.komprimiert && s.fehler.is_none() {
                s.fehler = Some(crate::i18n::tr("Der Assistent hat keine Antwort geliefert.", "The assistant returned no response.").into());
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

    #[test]
    #[ignore = "liest die Modellliste des installierten Codex App Servers, keine Modellanfrage"]
    fn e2e_codex_modelle() {
        let auftrag = Auftrag::starten(&egui::Context::default(), Target::Codex, &std::env::temp_dir(), None, false, &Auswahl::default(), Anfrage::Modelle).unwrap();
        let start = std::time::Instant::now();
        while !auftrag.stand.lock().unwrap().fertig {
            assert!(start.elapsed() < Duration::from_secs(40));
            std::thread::sleep(Duration::from_millis(50));
        }
        let s = auftrag.stand.lock().unwrap();
        assert!(s.fehler.is_none(), "{:?}", s.fehler);
        assert!(!s.modelle.is_empty());
        assert!(s.sitzung.is_none() && s.nachrichten.is_empty());
        eprintln!("{} Modelle vom offiziellen App Server", s.modelle.len());
    }

    #[test]
    fn chat_protokolle_trennen_text_fehler_und_sitzungen() {
        let mut s = Stand::default();
        ereignis(Target::Codex, r#"{"type":"thread.started","thread_id":"abc-123"}"#, &mut s);
        ereignis(Target::Codex, r#"{"type":"item.completed","item":{"type":"reasoning","text":"privat"}}"#, &mut s);
        ereignis(Target::Codex, r#"{"type":"item.completed","item":{"type":"agent_message","text":"Hallo"}}"#, &mut s);
        assert_eq!(s.sitzung.as_deref(), Some("abc-123"));
        assert_eq!(s.text.trim(), "Hallo");
        ereignis(Target::Codex, r#"{"type":"turn.failed","error":{"message":"Login fehlt"}}"#, &mut s);
        assert_eq!(s.fehler.as_deref(), Some("Login fehlt"));
        let mut s = Stand::default();
        ereignis(Target::Claude, r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"privat"},{"type":"text","text":"Hallo"}]}}"#, &mut s);
        ereignis(Target::Claude, r#"{"type":"result","session_id":"def-456","result":"Hallo","is_error":false}"#, &mut s);
        assert_eq!(s.text.trim(), "Hallo");
        assert_eq!(s.sitzung.as_deref(), Some("def-456"));
        ereignis(Target::Claude, r#"{"type":"result","is_error":true,"result":"Limit erreicht"}"#, &mut s);
        assert_eq!(s.fehler.as_deref(), Some("Limit erreicht"));
        ereignis(Target::Claude, "kein JSON", &mut s);
    }

    #[test]
    fn claude_streaming_wird_durch_die_echte_endnachricht_ersetzt() {
        let mut s = Stand::default();
        for zeile in [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hal"}}}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Hallo"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"Main.java"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"class Main {}"}]}}"#,
            r#"{"type":"system","subtype":"compact_boundary","session_id":"s1"}"#,
        ] {
            ereignis(Target::Claude, zeile, &mut s);
        }
        assert_eq!(s.nachrichten.len(), 2);
        assert_eq!(s.nachrichten[0].text, "Hallo");
        assert_eq!(s.nachrichten[1].rolle, Rolle::Werkzeug);
        assert!(s.nachrichten[1].fertig && s.nachrichten[1].text.contains("class Main {}"));
        assert!(s.komprimiert);
    }

    #[cfg(unix)]
    #[test]
    fn hintergrundprozess_liest_stdin_und_laesst_sich_abbrechen() {
        let ctx = egui::Context::default();
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "read frage; test \"$frage\" = 'Hallo $(exit 9)' || exit 4; printf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Antwort\"}}'",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let auftrag = Auftrag::ausfuehren(&ctx, Target::Codex, cmd, Some("Hallo $(exit 9)\n".into())).unwrap();
        let start = std::time::Instant::now();
        while !auftrag.stand.lock().unwrap().fertig {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = auftrag.stand.lock().unwrap().clone();
        assert_eq!(s.text.trim(), "Antwort");
        assert!(s.fehler.is_none(), "{:?}", s.fehler);

        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 60 & exit 0"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let auftrag = Auftrag::ausfuehren(&ctx, Target::Codex, cmd, Some(String::new())).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let stand = auftrag.stand.clone();
        let start = std::time::Instant::now();
        drop(auftrag);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(stand.lock().unwrap().fertig);
        assert!(stand.lock().unwrap().fehler.is_some());
    }

    #[test]
    fn fortsetzen_behaelt_rechte_ohne_bypass_und_ohne_shell_prompt() {
        for schreiben in [false, true] {
            let args = argumente(Some("abc-123"), schreiben);
            assert!(args.iter().any(|a| a == "abc-123"));
            assert!(!args.iter().any(|a| a.contains("bypass") || a.contains("danger")));
            assert_eq!(args[5].contains("Edit"), schreiben);
            assert!(!args[5].contains("Bash"));
            assert!(args[7].ends_with(",mcp__easymysql__*"));
            assert!(args.iter().any(|a| a == "--include-partial-messages"));
        }
    }
}
