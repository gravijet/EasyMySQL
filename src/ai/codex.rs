// Offizielles App-Server-Protokoll: echte Modelle, Nachrichten, Werkzeuge und Komprimierung.
use super::*;
use serde_json::json;
use std::process::ChildStdin;

pub(super) struct Protokoll {
    eingabe: Option<ChildStdin>,
    ordner: String,
    sitzung: Option<String>,
    schreiben: bool,
    auswahl: Auswahl,
    anfrage: Anfrage,
}

impl Protokoll {
    pub fn new(ordner: &Path, sitzung: Option<&str>, schreiben: bool, auswahl: &Auswahl, anfrage: Anfrage) -> Self {
        Self { eingabe: None, ordner: ordner.display().to_string(), sitzung: sitzung.map(str::to_string), schreiben, auswahl: auswahl.clone(), anfrage }
    }

    fn senden(&mut self, v: Value) -> Result<(), String> {
        let eingabe = self.eingabe.as_mut().ok_or("Codex-Verbindung geschlossen")?;
        writeln!(eingabe, "{v}").and_then(|_| eingabe.flush()).map_err(|e| e.to_string())
    }

    pub fn initialisieren(&mut self, eingabe: ChildStdin) -> Result<(), String> {
        self.eingabe = Some(eingabe);
        self.senden(json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"easymysql","title":"EasyMySQL","version":env!("CARGO_PKG_VERSION")}}}))
    }

    fn weiter(&mut self, v: &Value, s: &mut Stand) -> Result<bool, String> {
        if let Some(error) = v.get("error").filter(|e| !e.is_null())
            && (v.get("id").is_some() || v["params"]["willRetry"] != true)
        {
            return Err(error["message"].as_str().unwrap_or("Codex error").into());
        }
        if let Some(id) = v.get("id") {
            if v.get("method").is_some() {
                // Unerwartete Serveranfragen nie unbemerkt erlauben oder unbeantwortet lassen.
                self.senden(json!({"id":id,"error":{"code":-32601,"message":"Interactive requests are not supported; use the configured permissions."}}))?;
                return Err(
                    crate::i18n::tr("Codex benötigt eine interaktive Freigabe. Bitte Zugriffsmodus prüfen.", "Codex needs interactive approval. Check the access mode.").into()
                );
            }
            match id.as_u64() {
                Some(1) => {
                    self.senden(json!({"method":"initialized","params":{}}))?;
                    self.senden(json!({"id":2,"method":"model/list","params":{"limit":100,"includeHidden":false}}))?;
                }
                Some(2) => {
                    if let Some(modelle) = v["result"]["data"].as_array() {
                        for m in modelle {
                            if let (Some(id), Some(name)) = (m["model"].as_str(), m["displayName"].as_str()) {
                                s.modelle.push(Modell {
                                    id: id.into(),
                                    name: name.into(),
                                    aufwand: m["supportedReasoningEfforts"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|e| e["reasoningEffort"].as_str().map(str::to_string))
                                        .collect(),
                                });
                            }
                        }
                    }
                    if let Some(cursor) = v["result"]["nextCursor"].as_str() {
                        self.senden(json!({"id":2,"method":"model/list","params":{"limit":100,"cursor":cursor}}))?;
                        return Ok(false);
                    }
                    if matches!(self.anfrage, Anfrage::Modelle) {
                        return Ok(true);
                    }
                    let mut params = json!({"cwd":self.ordner,"approvalPolicy":"never","sandbox":if self.schreiben {"workspace-write"} else {"read-only"}});
                    if !self.auswahl.modell.is_empty() {
                        params["model"] = json!(self.auswahl.modell);
                    }
                    let method = if let Some(id) = &self.sitzung {
                        params["threadId"] = json!(id);
                        "thread/resume"
                    } else {
                        "thread/start"
                    };
                    self.senden(json!({"id":3,"method":method,"params":params}))?;
                }
                Some(3) => {
                    let id = v["result"]["thread"]["id"].as_str().ok_or("Codex returned no thread id")?;
                    s.sitzung = Some(id.into());
                    s.modell = v["result"]["model"].as_str().map(str::to_string);
                    if matches!(self.anfrage, Anfrage::Kompakt) {
                        self.senden(json!({"id":4,"method":"thread/compact/start","params":{"threadId":id}}))?;
                    } else if let Anfrage::Text(text) = &self.anfrage {
                        let mut params = json!({"threadId":id,"input":[{"type":"text","text":text,"text_elements":[]}]});
                        if !self.auswahl.aufwand.is_empty() {
                            params["effort"] = json!(self.auswahl.aufwand);
                        }
                        self.senden(json!({"id":4,"method":"turn/start","params":params}))?;
                    }
                }
                _ => {}
            }
        }
        let p = &v["params"];
        match v["method"].as_str().unwrap_or("") {
            "item/agentMessage/delta" => {
                if let (Some(id), Some(text)) = (p["itemId"].as_str(), p["delta"].as_str()) {
                    anfuegen(&mut s.nachricht(id, Rolle::Assistent, "").text, text);
                }
            }
            "item/started" | "item/completed" => {
                let abgeschlossen = v["method"] == "item/completed";
                let item = &p["item"];
                let typ = item["type"].as_str().unwrap_or("");
                let id = item["id"].as_str().unwrap_or("");
                match typ {
                    "agentMessage" => {
                        let n = s.nachricht(id, Rolle::Assistent, "");
                        if let Some(text) = item["text"].as_str() {
                            n.text = text.into();
                        }
                        n.fertig = abgeschlossen;
                    }
                    "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" => {
                        let titel = item["command"].as_str().or(item["tool"].as_str()).unwrap_or(typ);
                        let n = s.nachricht(id, Rolle::Werkzeug, titel);
                        n.text = serde_json::to_string_pretty(item).unwrap_or_default();
                        if n.text.len() > 64 * 1024 {
                            let mut ende = 64 * 1024;
                            while !n.text.is_char_boundary(ende) {
                                ende -= 1;
                            }
                            n.text.truncate(ende);
                        }
                        n.fertig = abgeschlossen;
                    }
                    "contextCompaction" if abgeschlossen => {
                        s.komprimiert = true;
                        if matches!(self.anfrage, Anfrage::Kompakt) {
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }
            "thread/tokenUsage/updated" => {
                s.tokens = p["tokenUsage"]["last"]["totalTokens"].as_u64();
                s.kontext = p["tokenUsage"]["modelContextWindow"].as_u64();
            }
            "turn/completed" => {
                if p["turn"]["status"] == "failed" {
                    return Err(p["turn"]["error"]["message"].as_str().unwrap_or("Codex turn failed").into());
                }
                return Ok(true);
            }
            "error" if p["willRetry"] != true => return Err(p["error"]["message"].as_str().unwrap_or("Codex error").into()),
            _ => {}
        }
        Ok(false)
    }

    pub fn ereignis(&mut self, zeile: &str, s: &mut Stand) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(zeile) else { return false };
        match self.weiter(&v, s) {
            Ok(fertig) => fertig,
            Err(e) => {
                s.fehler = Some(e);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn handshake_fortsetzen_modell_rechte_und_compact_sind_echte_rpc_anfragen() {
        for kompakt in [false, true] {
            let mut prozess = Command::new("sh").args(["-c", "cat"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
            let auswahl = Auswahl { modell: "test-model".into(), aufwand: "high".into() };
            let anfrage = if kompakt { Anfrage::Kompakt } else { Anfrage::Text("Hallo $(exit 9)".into()) };
            let mut p = Protokoll::new(Path::new("/projekt"), Some("thread-1"), false, &auswahl, anfrage);
            p.initialisieren(prozess.stdin.take().unwrap()).unwrap();
            let mut s = Stand::default();
            for v in [
                json!({"id":1,"result":{}}),
                json!({"id":2,"result":{"data":[{"model":"test-model","displayName":"Test","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]}}),
                json!({"id":3,"result":{"thread":{"id":"thread-1"},"model":"test-model"}}),
            ] {
                p.weiter(&v, &mut s).unwrap();
            }
            drop(p);
            let ausgabe = prozess.wait_with_output().unwrap();
            let anfragen = String::from_utf8(ausgabe.stdout).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>();
            assert_eq!(anfragen[0]["method"], "initialize");
            assert_eq!(anfragen[1]["method"], "initialized");
            assert_eq!(anfragen[3]["method"], "thread/resume");
            assert_eq!(anfragen[3]["params"]["sandbox"], "read-only");
            assert_eq!(anfragen[3]["params"]["approvalPolicy"], "never");
            assert_eq!(anfragen[3]["params"]["model"], "test-model");
            assert_eq!(anfragen[4]["method"], if kompakt { "thread/compact/start" } else { "turn/start" });
            assert_eq!(anfragen[4]["params"]["threadId"], "thread-1");
            if !kompakt {
                assert_eq!(anfragen[4]["params"]["input"][0]["text"], "Hallo $(exit 9)");
                assert_eq!(anfragen[4]["params"]["effort"], "high");
            }
            assert_eq!(s.modelle[0].aufwand, ["high"]);
        }
    }
    #[test]
    fn nachrichten_deltas_werkzeuge_und_komprimierung() {
        let mut p = Protokoll::new(Path::new("."), Some("thread"), false, &Auswahl::default(), Anfrage::Kompakt);
        let mut s = Stand::default();
        for v in [
            json!({"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"Hal"}}),
            json!({"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"lo"}}),
            json!({"method":"item/completed","params":{"item":{"type":"agentMessage","id":"a","text":"Hallo!"}}}),
            json!({"method":"item/completed","params":{"item":{"type":"mcpToolCall","id":"b","tool":"get_schema","status":"completed"}}}),
        ] {
            assert!(!p.ereignis(&v.to_string(), &mut s));
        }
        assert_eq!(s.nachrichten.len(), 2);
        assert_eq!(s.nachrichten[0].text, "Hallo!");
        assert!(s.nachrichten[0].fertig && s.nachrichten[1].fertig);
        assert!(p.ereignis(r#"{"method":"item/completed","params":{"item":{"id":"c","type":"contextCompaction"}}}"#, &mut s));
        assert!(s.komprimiert);
        assert!(p.ereignis(r#"{"id":4,"error":{"message":"Limit erreicht"}}"#, &mut s));
        assert_eq!(s.fehler.as_deref(), Some("Limit erreicht"));
    }
}
