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

#[derive(Clone, Default)]
pub struct Stand {
    pub text: String,
    pub sitzung: Option<String>,
    pub fehler: Option<String>,
    pub fertig: bool,
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

fn argumente(ziel: Target, sitzung: Option<&str>, schreiben: bool) -> Vec<String> {
    let mut args: Vec<String> = match ziel {
        Target::Claude => {
            let werkzeuge = if schreiben { "Read,Glob,Grep,Edit,Write" } else { "Read,Glob,Grep" };
            let freigaben = format!("{werkzeuge},mcp__easymysql__*");
            ["-p", "--output-format", "stream-json", "--verbose", "--tools", werkzeuge, "--allowedTools", &freigaben, "--permission-mode", "dontAsk"]
                .into_iter()
                .map(String::from)
                .collect()
        }
        Target::Codex => vec![
            "-c".into(),
            format!("sandbox_mode=\"{}\"", if schreiben { "workspace-write" } else { "read-only" }),
            "-c".into(),
            "approval_policy=\"never\"".into(),
            "exec".into(),
        ],
    };
    if let Some(id) = sitzung {
        match ziel {
            Target::Claude => args.extend(["--resume".into(), id.into()]),
            Target::Codex => args.extend(["resume".into(), id.into()]),
        }
    }
    if ziel == Target::Codex {
        args.extend(["--json".into(), "--skip-git-repo-check".into(), "-".into()]);
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
        (Target::Codex, "item.completed") if v["item"]["type"] == "agent_message" => {
            if let Some(text) = v["item"]["text"].as_str() {
                anfuegen(&mut stand.text, &format!("{text}\n\n"));
            }
        }
        (Target::Claude, "assistant") if v["parent_tool_use_id"].is_null() => {
            if let Some(inhalt) = v["message"]["content"].as_array() {
                for teil in inhalt.iter().filter(|t| t["type"] == "text") {
                    if let Some(text) = teil["text"].as_str() {
                        anfuegen(&mut stand.text, &format!("{text}\n\n"));
                    }
                }
            }
        }
        (Target::Claude, "result") => {
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

    pub fn starten(ctx: &egui::Context, ziel: Target, ordner: &Path, sitzung: Option<&str>, schreiben: bool, frage: Option<String>) -> Result<Self, String> {
        let programm = ziel.program().ok_or_else(|| crate::i18n::tr("Bitte zuerst das Plugin installieren.", "Please install the plugin first.").to_string())?;
        let login = frage.is_none();
        let args = if login {
            match ziel {
                Target::Claude => vec!["auth".into(), "login".into()],
                Target::Codex => vec!["login".into()],
            }
        } else {
            argumente(ziel, sitzung, schreiben)
        };
        let mut cmd = Command::new(programm);
        cmd.args(args).current_dir(ordner).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(path) = crate::dev::process::tool_path(&crate::dev::tools()) {
            cmd.env("PATH", path);
        }
        crate::server::hidden(&mut cmd);
        Self::ausfuehren(ctx, ziel, cmd, frage)
    }

    fn ausfuehren(ctx: &egui::Context, ziel: Target, mut cmd: Command, frage: Option<String>) -> Result<Self, String> {
        let login = frage.is_none();
        // Abbrechen beendet unter Unix auch die von der KI gestarteten Unterprozesse.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut prozess = cmd.spawn().map_err(|e| e.to_string())?;
        let eingabe = prozess.stdin.take();
        std::thread::spawn(move || {
            if let (Some(mut stdin), Some(frage)) = (eingabe, frage) {
                let _ = stdin.write_all(frage.as_bytes());
            }
        });
        let mut auftrag = Self { stand: Arc::default(), abbruch: Arc::default(), arbeiter: None };
        let fehlerausgabe = Arc::new(Mutex::new(String::new()));
        let mut leser = Vec::new();
        for (stderr, stream) in [
            (false, prozess.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
            (true, prozess.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
        ] {
            let Some(stream) = stream else { continue };
            let (stand, fehlerausgabe, ctx) = (auftrag.stand.clone(), fehlerausgabe.clone(), ctx.clone());
            leser.push(std::thread::spawn(move || {
                for zeile in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
                    if login {
                        anfuegen(&mut stand.lock().unwrap().text, &format!("{zeile}\n"));
                    } else if stderr {
                        anfuegen(&mut fehlerausgabe.lock().unwrap(), &format!("{zeile}\n"));
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
                if abbruch.load(Ordering::Relaxed) {
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
                    s.fehler = Some(crate::i18n::tr("Abgebrochen.", "Cancelled.").into());
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
            if !login && s.text.trim().is_empty() && s.fehler.is_none() {
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
        for ziel in [Target::Claude, Target::Codex] {
            for schreiben in [false, true] {
                let args = argumente(ziel, Some("abc-123"), schreiben);
                assert!(args.iter().any(|a| a == "abc-123"));
                assert!(!args.iter().any(|a| a.contains("bypass") || a.contains("danger")));
                if ziel == Target::Codex {
                    assert_eq!(args[1], if schreiben { "sandbox_mode=\"workspace-write\"" } else { "sandbox_mode=\"read-only\"" });
                    assert_eq!(args.last().unwrap(), "-");
                } else {
                    assert_eq!(args[5].contains("Edit"), schreiben);
                    assert!(!args[5].contains("Bash"));
                    assert!(args[7].ends_with(",mcp__easymysql__*"));
                }
            }
        }
    }
}
