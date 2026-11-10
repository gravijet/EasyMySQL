// Offizielles App-Server-Protokoll: ein Prozess pro Chat, echte Modelle, Skills, Werkzeuge und Komprimierung.
use super::*;

/// Eingebaute Befehle, die die Oberflaeche selbst ausfuehrt (wie in der Codex-Konsole)
const BEFEHLE: [(&str, &str, &str); 3] = [
    ("/new", "Neuen Chat beginnen", "Start a new chat"),
    ("/compact", "Kontext zusammenfassen, um Platz zu sparen", "Summarize the context to save space"),
    ("/model", "Modell und Aufwand wählen", "Choose model and effort"),
];

pub(super) struct Protokoll {
    ordner: String,
    sitzung: Option<String>,
    schreiben: bool,
    auswahl: Auswahl,
    zug: Option<String>,
    /// Bestaetigter Thread (erst nach thread/start bzw. thread/resume)
    thread: Option<String>,
    /// Vor dem Thread-Start gesendete Nachricht
    wartend: Option<(Anfrage, Vec<Befehl>)>,
    naechste_id: u64,
}

impl Protokoll {
    pub fn new(ordner: &Path, sitzung: Option<&str>, schreiben: bool, auswahl: &Auswahl) -> Self {
        Self { ordner: ordner.display().to_string(), sitzung: sitzung.map(str::to_string), schreiben, auswahl: auswahl.clone(), zug: None, thread: None, wartend: None, naechste_id: 10 }
    }

    pub fn initialisieren() -> Value {
        json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"easymysql","title":"EasyMySQL","version":env!("CARGO_PKG_VERSION")}}})
    }

    pub fn senden(&mut self, anfrage: Anfrage, befehle: &[Befehl], leitung: &mut Leitung) -> Result<(), String> {
        let Some(thread) = self.thread.clone() else {
            self.wartend = Some((anfrage, befehle.to_vec()));
            return Ok(());
        };
        let id = self.naechste_id;
        self.naechste_id += 1;
        match anfrage {
            Anfrage::Kompakt => leitung.senden(&json!({"id":id,"method":"thread/compact/start","params":{"threadId":thread}})),
            Anfrage::Text(text) => {
                let mut eingabe = vec![json!({"type":"text","text":text,"text_elements":[]})];
                // `$skill` im Text als Skill anhaengen, wie es die Codex-Konsole tut
                for b in befehle.iter().filter(|b| b.name.starts_with('$') && text.split_whitespace().any(|w| w == b.name)) {
                    if let Some(pfad) = &b.pfad {
                        eingabe.push(json!({"type":"skill","name":&b.name[1..],"path":pfad}));
                    }
                }
                let mut params = json!({"threadId":thread,"input":eingabe});
                if !self.auswahl.aufwand.is_empty() {
                    params["effort"] = json!(self.auswahl.aufwand);
                }
                leitung.senden(&json!({"id":id,"method":"turn/start","params":params}))
            }
        }
    }

    pub fn unterbrechen(&mut self, leitung: &mut Leitung) -> Result<(), String> {
        let (Some(thread), Some(zug)) = (&self.thread, &self.zug) else { return Ok(()) };
        leitung.senden(&json!({"id":9,"method":"turn/interrupt","params":{"threadId":thread,"turnId":zug}}))
    }

    fn weiter(&mut self, v: &Value, s: &mut Stand, leitung: &mut Leitung) -> Result<(), String> {
        if let Some(error) = v.get("error").filter(|e| !e.is_null())
            && (v.get("id").is_some() || v["params"]["willRetry"] != true)
        {
            return Err(error["message"].as_str().unwrap_or("Codex error").into());
        }
        if let Some(id) = v.get("id") {
            if v.get("method").is_some() {
                // Unerwartete Serveranfragen nie unbemerkt erlauben oder unbeantwortet lassen.
                leitung.senden(&json!({"id":id,"error":{"code":-32601,"message":"Interactive requests are not supported; use the configured permissions."}}))?;
                return Err(
                    crate::i18n::tr("Codex benötigt eine interaktive Freigabe. Bitte Zugriffsmodus prüfen.", "Codex needs interactive approval. Check the access mode.").into()
                );
            }
            match id.as_u64() {
                Some(1) => {
                    leitung.senden(&json!({"method":"initialized","params":{}}))?;
                    leitung.senden(&json!({"id":2,"method":"model/list","params":{"limit":100,"includeHidden":true}}))?;
                    leitung.senden(&json!({"id":5,"method":"skills/list","params":{"cwds":[self.ordner]}}))?;
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
                    leitung.senden(&json!({"id":3,"method":method,"params":params}))?;
                }
                Some(2) => {
                    for m in v["result"]["data"].as_array().into_iter().flatten() {
                        if let (Some(id), Some(name)) = (m["model"].as_str(), m["displayName"].as_str()) {
                            s.modelle.push(Modell {
                                id: id.into(),
                                name: name.into(),
                                beschreibung: m["description"].as_str().unwrap_or("").into(),
                                aufwand: m["supportedReasoningEfforts"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|e| e["reasoningEffort"].as_str().map(str::to_string))
                                    .collect(),
                            });
                        }
                    }
                    if let Some(cursor) = v["result"]["nextCursor"].as_str() {
                        leitung.senden(&json!({"id":2,"method":"model/list","params":{"limit":100,"includeHidden":true,"cursor":cursor}}))?;
                    }
                }
                Some(5) => {
                    s.befehle.retain(|b| !b.name.starts_with('$'));
                    for skill in v["result"]["data"].as_array().into_iter().flatten().flat_map(|e| e["skills"].as_array().into_iter().flatten()) {
                        if let (Some(name), Some(pfad), true) = (skill["name"].as_str(), skill["path"].as_str(), skill["enabled"] != false) {
                            s.befehle.push(Befehl {
                                name: format!("${name}"),
                                hinweis: String::new(),
                                beschreibung: skill["shortDescription"].as_str().or(skill["description"].as_str()).unwrap_or("").into(),
                                pfad: Some(pfad.into()),
                            });
                        }
                    }
                }
                Some(3) => {
                    let id = v["result"]["thread"]["id"].as_str().ok_or("Codex returned no thread id")?;
                    self.thread = Some(id.into());
                    s.sitzung = Some(id.into());
                    s.modell = v["result"]["model"].as_str().map(str::to_string);
                    if s.befehle.iter().all(|b| b.name.starts_with('$')) {
                        s.befehle.splice(0..0, BEFEHLE.iter().map(|(n, d, e)| Befehl { name: (*n).into(), beschreibung: crate::i18n::tr(d, e).into(), ..Default::default() }));
                    }
                    s.bereit = true;
                    if let Some((anfrage, befehle)) = self.wartend.take() {
                        self.senden(anfrage, &befehle, leitung)?;
                    }
                }
                Some(n) if n >= 10 => {
                    if let Some(zug) = v["result"]["turn"]["id"].as_str() {
                        self.zug = Some(zug.into());
                    }
                }
                _ => {}
            }
        }
        let p = &v["params"];
        match v["method"].as_str().unwrap_or("") {
            "turn/started" => self.zug = p["turn"]["id"].as_str().map(str::to_string),
            "item/agentMessage/delta" => {
                if let (Some(id), Some(text)) = (p["itemId"].as_str(), p["delta"].as_str()) {
                    s.denkt = false;
                    anfuegen(&mut s.nachricht(id, Rolle::Assistent, "").text, text);
                }
            }
            "item/started" | "item/completed" => {
                let abgeschlossen = v["method"] == "item/completed";
                let item = &p["item"];
                let typ = item["type"].as_str().unwrap_or("");
                let id = item["id"].as_str().unwrap_or("");
                match typ {
                    "reasoning" => s.denkt = !abgeschlossen,
                    "agentMessage" => {
                        let n = s.nachricht(id, Rolle::Assistent, "");
                        if let Some(text) = item["text"].as_str() {
                            n.text = text.into();
                        }
                        n.fertig = abgeschlossen;
                    }
                    "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" => {
                        let titel = match typ {
                            "commandExecution" => format!("Bash({})", item["command"].as_str().unwrap_or("")),
                            "fileChange" => {
                                let pfade = item["changes"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect::<Vec<_>>().join(", ");
                                format!("Edit({pfade})")
                            }
                            "mcpToolCall" => werkzeug_titel(&format!("{} · {}", item["server"].as_str().unwrap_or("mcp"), item["tool"].as_str().unwrap_or("")), &item["arguments"]),
                            _ => format!("Search({})", item["query"].as_str().unwrap_or("")),
                        };
                        let n = s.nachricht(id, Rolle::Werkzeug, &titel);
                        n.text = item["aggregatedOutput"].as_str().map(str::to_string).unwrap_or_else(|| serde_json::to_string_pretty(item).unwrap_or_default());
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
                        s.zug_ende();
                    }
                    _ => {}
                }
            }
            "thread/tokenUsage/updated" => {
                s.tokens = p["tokenUsage"]["last"]["totalTokens"].as_u64();
                s.kontext = p["tokenUsage"]["modelContextWindow"].as_u64();
            }
            "turn/completed" => {
                self.zug = None;
                if p["turn"]["status"] == "failed" {
                    s.fehler = Some(p["turn"]["error"]["message"].as_str().unwrap_or("Codex turn failed").into());
                }
                s.zug_ende();
            }
            "error" if p["willRetry"] != true => return Err(p["error"]["message"].as_str().unwrap_or("Codex error").into()),
            _ => {}
        }
        Ok(())
    }

    pub fn ereignis(&mut self, zeile: &str, s: &mut Stand, leitung: &mut Leitung) {
        let Ok(v) = serde_json::from_str::<Value>(zeile) else { return };
        if let Err(e) = self.weiter(&v, s, leitung) {
            s.fehler = Some(e);
            if s.laeuft {
                s.zug_ende();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anfragen(prozess: Child) -> Vec<Value> {
        let ausgabe = prozess.wait_with_output().unwrap();
        String::from_utf8(ausgabe.stdout).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn handshake_einmal_danach_nur_noch_zuege_und_compact() {
        let mut prozess = Command::new("sh").args(["-c", "cat"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut leitung = Leitung(prozess.stdin.take());
        let auswahl = Auswahl { modell: "test-model".into(), aufwand: "high".into() };
        let mut p = Protokoll::new(Path::new("/projekt"), Some("thread-1"), false, &auswahl);
        let mut s = Stand::default();
        let skill = Befehl { name: "$review".into(), pfad: Some("/skills/review".into()), ..Default::default() };
        // Vor dem Thread-Start gesendet: wird nachgeholt
        p.senden(Anfrage::Text("Hallo $(exit 9) $review".into()), std::slice::from_ref(&skill), &mut leitung).unwrap();
        for v in [
            json!({"id":1,"result":{}}),
            json!({"id":2,"result":{"data":[{"model":"test-model","displayName":"Test","supportedReasoningEfforts":[{"reasoningEffort":"high"}]}]}}),
            json!({"id":5,"result":{"data":[{"cwd":"/projekt","skills":[{"name":"review","path":"/skills/review","description":"Prüft","enabled":true}],"errors":[]}]}}),
            json!({"id":3,"result":{"thread":{"id":"thread-1"},"model":"test-model"}}),
            json!({"id":10,"result":{"turn":{"id":"zug-1"}}}),
        ] {
            p.weiter(&v, &mut s, &mut leitung).unwrap();
        }
        p.unterbrechen(&mut leitung).unwrap();
        p.senden(Anfrage::Kompakt, &[], &mut leitung).unwrap();
        drop(leitung);
        let a = anfragen(prozess);
        let methoden = a.iter().map(|v| v["method"].as_str().unwrap_or("")).collect::<Vec<_>>();
        assert_eq!(methoden, ["initialized", "model/list", "skills/list", "thread/resume", "turn/start", "turn/interrupt", "thread/compact/start"]);
        assert_eq!(a[1]["params"]["includeHidden"], true);
        assert_eq!(a[3]["params"]["sandbox"], "read-only");
        assert_eq!(a[3]["params"]["approvalPolicy"], "never");
        assert_eq!(a[3]["params"]["model"], "test-model");
        assert_eq!(a[4]["params"]["input"][0]["text"], "Hallo $(exit 9) $review");
        assert_eq!(a[4]["params"]["input"][1]["path"], "/skills/review");
        assert_eq!(a[4]["params"]["effort"], "high");
        assert_eq!(a[5]["params"]["turnId"], "zug-1");
        assert_eq!(s.modelle[0].aufwand, ["high"]);
        assert!(s.bereit && s.befehle.iter().any(|b| b.name == "/compact") && s.befehle.iter().any(|b| b.name == "$review"));
    }

    #[test]
    fn nachrichten_deltas_werkzeuge_und_komprimierung() {
        let mut p = Protokoll::new(Path::new("."), Some("thread"), false, &Auswahl::default());
        let mut s = Stand { laeuft: true, ..Default::default() };
        let mut leitung = Leitung::default();
        for v in [
            json!({"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"Hal"}}),
            json!({"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"lo"}}),
            json!({"method":"item/completed","params":{"item":{"type":"agentMessage","id":"a","text":"Hallo!"}}}),
            json!({"method":"item/completed","params":{"item":{"type":"commandExecution","id":"b","command":"ls","aggregatedOutput":"a.txt"}}}),
        ] {
            p.ereignis(&v.to_string(), &mut s, &mut leitung);
        }
        assert_eq!(s.nachrichten.len(), 2);
        assert_eq!(s.nachrichten[0].text, "Hallo!");
        assert_eq!((s.nachrichten[1].titel.as_str(), s.nachrichten[1].text.as_str()), ("Bash(ls)", "a.txt"));
        assert!(s.laeuft);
        p.ereignis(r#"{"method":"item/completed","params":{"item":{"id":"c","type":"contextCompaction"}}}"#, &mut s, &mut leitung);
        assert!(s.komprimiert && !s.laeuft && s.zug_fertig);
        p.ereignis(r#"{"id":11,"error":{"message":"Limit erreicht"}}"#, &mut s, &mut leitung);
        assert_eq!(s.fehler.as_deref(), Some("Limit erreicht"));
    }
}
