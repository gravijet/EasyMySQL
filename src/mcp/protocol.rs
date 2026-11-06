// JSON-RPC 2.0 nach dem Model Context Protocol (MCP), Transport "stdio": eine JSON-Nachricht je Zeile.
// Unabhaengig von der Datenbank, damit es sich mit einem Ersatz-Handler testen laesst.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// Unterstuetzte Protokollversionen, neueste zuerst
pub const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Ergebnis eines Werkzeugaufrufs
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: false }
    }

    pub fn json(v: &Value) -> Self {
        Self::ok(serde_json::to_string_pretty(v).unwrap_or_default())
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: true }
    }
}

pub trait Handler {
    /// Beschreibung der Werkzeuge (Name, Beschreibung, inputSchema, annotations)
    fn tools(&self) -> Vec<Value>;
    fn call(&mut self, name: &str, args: &Value) -> ToolResult;
    /// Hinweise fuer die KI, die beim Verbinden mitgeschickt werden
    fn instructions(&self) -> String;
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Eine eingehende Nachricht beantworten. `None` fuer Benachrichtigungen (ohne `id`) und Antworten.
pub fn handle(h: &mut dyn Handler, msg: &Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else { return Some(error(Value::Null, -32600, "Invalid Request")) };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // Antworten des Clients (z. B. auf ping) brauchen keine Antwort
        return if obj.contains_key("result") || obj.contains_key("error") { None } else { Some(error(id.unwrap_or(Value::Null), -32600, "Invalid Request")) };
    };
    let id = id?;
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(VERSIONS[0]);
            let version = VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "easymysql", "title": "EasyMySQL", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": h.instructions(),
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": h.tools() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else { return Some(error(id, -32602, "Missing tool name")) };
            if !h.tools().iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name)) {
                return Some(error(id, -32602, &format!("Unknown tool: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let r = h.call(name, &args);
            result(id, json!({ "content": [{ "type": "text", "text": r.text }], "isError": r.is_error }))
        }
        // Keine Ressourcen und Vorlagen: leere Listen statt Fehler, manche Clients fragen trotzdem
        "resources/list" => result(id, json!({ "resources": [] })),
        "resources/templates/list" => result(id, json!({ "resourceTemplates": [] })),
        "prompts/list" => result(id, json!({ "prompts": [] })),
        _ => error(id, -32601, &format!("Method not found: {method}")),
    })
}

/// Nachrichten lesen und beantworten, bis die Eingabe endet.
pub fn serve(input: impl BufRead, mut output: impl Write, h: &mut dyn Handler) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let replies: Vec<Value> = match serde_json::from_str::<Value>(&line) {
            Err(_) => vec![error(Value::Null, -32700, "Parse error")],
            // Aeltere Protokollversionen erlauben Stapel (Arrays)
            Ok(Value::Array(items)) => items.iter().filter_map(|m| handle(h, m)).collect(),
            Ok(msg) => handle(h, &msg).into_iter().collect(),
        };
        for r in replies {
            // serde_json schreibt keine Zeilenumbrueche in die Nachricht: eine Zeile je Antwort
            writeln!(output, "{r}")?;
        }
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl Handler for Echo {
        fn tools(&self) -> Vec<Value> {
            vec![json!({ "name": "echo", "description": "x", "inputSchema": { "type": "object" } })]
        }
        fn call(&mut self, _name: &str, args: &Value) -> ToolResult {
            match args.get("fail") {
                Some(_) => ToolResult::err("kaputt"),
                None => ToolResult::ok(args.to_string()),
            }
        }
        fn instructions(&self) -> String {
            "hallo".into()
        }
    }

    fn talk(lines: &[&str]) -> Vec<Value> {
        let mut out = Vec::new();
        serve(lines.join("\n").as_bytes(), &mut out, &mut Echo).unwrap();
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn handshake_and_tools() {
        let r = talk(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":"drei","method":"tools/call","params":{"name":"echo","arguments":{"a":1}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"echo","arguments":{"fail":true}}}"#,
        ]);
        assert_eq!(r.len(), 4, "Benachrichtigungen bekommen keine Antwort");
        assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r[0]["result"]["serverInfo"]["name"], "easymysql");
        assert_eq!(r[0]["result"]["instructions"], "hallo");
        assert_eq!(r[1]["result"]["tools"][0]["name"], "echo");
        assert_eq!(r[2]["id"], "drei");
        assert_eq!(r[2]["result"]["content"][0]["text"], r#"{"a":1}"#);
        assert_eq!(r[2]["result"]["isError"], false);
        assert_eq!(r[3]["result"]["isError"], true);
    }

    #[test]
    fn unknown_version_gets_newest() {
        let r = talk(&[r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#]);
        assert_eq!(r[0]["result"]["protocolVersion"], VERSIONS[0]);
    }

    #[test]
    fn errors() {
        let r = talk(&[
            "kein json",
            r#"{"jsonrpc":"2.0","id":1,"method":"gibtsnicht"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fehlt"}}"#,
            r#"[{"jsonrpc":"2.0","id":3,"method":"ping"},{"jsonrpc":"2.0","id":4,"method":"ping"}]"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
        ]);
        assert_eq!(r[0]["error"]["code"], -32700);
        assert_eq!(r[1]["error"]["code"], -32601);
        assert_eq!(r[2]["error"]["code"], -32602);
        assert_eq!((r[3]["id"].clone(), r[4]["id"].clone()), (json!(3), json!(4)));
        assert_eq!(r.len(), 5, "Antworten des Clients werden nicht beantwortet");
    }
}
