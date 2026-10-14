// Sammlungen eines MongoDB-Servers lesen (auch ueber SSH-Tunnel und TLS).
// Eigener, kleiner Client: OP_MSG, Anmeldung mit SCRAM-SHA-256, find/getMore. Verbindungen ueber
// "mongodb+srv" (DNS-Abfrage) gibt es nicht; dafuer Server und Port direkt eintragen.

use super::docs::{self, Doc, base64};
use super::*;
use crate::transfer::server::{Link, wrap_tls};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

// ---------------------------------------------------------------- BSON schreiben

/// Dokument im BSON-Format aufbauen
#[derive(Default)]
struct B(Vec<u8>);

impl B {
    fn key(&mut self, ty: u8, k: &str) {
        self.0.push(ty);
        self.0.extend(k.as_bytes());
        self.0.push(0);
    }
    fn str(mut self, k: &str, v: &str) -> Self {
        self.key(0x02, k);
        self.0.extend(((v.len() + 1) as i32).to_le_bytes());
        self.0.extend(v.as_bytes());
        self.0.push(0);
        self
    }
    fn i32(mut self, k: &str, v: i32) -> Self {
        self.key(0x10, k);
        self.0.extend(v.to_le_bytes());
        self
    }
    fn i64(mut self, k: &str, v: i64) -> Self {
        self.key(0x12, k);
        self.0.extend(v.to_le_bytes());
        self
    }
    fn bool(mut self, k: &str, v: bool) -> Self {
        self.key(0x08, k);
        self.0.push(v as u8);
        self
    }
    fn bin(mut self, k: &str, v: &[u8]) -> Self {
        self.key(0x05, k);
        self.0.extend((v.len() as i32).to_le_bytes());
        self.0.push(0);
        self.0.extend(v);
        self
    }
    fn doc(mut self, k: &str, v: B) -> Self {
        self.key(0x03, k);
        self.0.extend(v.finish());
        self
    }
    fn finish(self) -> Vec<u8> {
        let mut out = ((self.0.len() + 5) as i32).to_le_bytes().to_vec();
        out.extend(self.0);
        out.push(0);
        out
    }
}

// ---------------------------------------------------------------- Kleine Krypto-Helfer (SCRAM)

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut s = salt.to_vec();
    s.extend(1u32.to_be_bytes());
    let mut u = hmac(password, &s);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac(password, &u);
        for (o, x) in out.iter_mut().zip(u) {
            *o ^= x;
        }
    }
    out
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// SCRAM-SHA-256: Antwort des Clients (ohne Anfangsteil) und die erwartete Signatur des Servers
fn scram_final(password: &str, client_first_bare: &str, server_first: &str, nonce: &str) -> Result<(String, Vec<u8>), String> {
    let field = |k: &str| server_first.split(',').find_map(|p| p.strip_prefix(k)).map(str::to_string);
    let (r, s, i) = (field("r="), field("s="), field("i="));
    let (Some(r), Some(s), Some(i)) = (r, s, i) else { return Err(crate::i18n::text("Unerwartete Antwort bei der Anmeldung.").into()) };
    if !r.starts_with(nonce) {
        return Err(crate::i18n::text("Der Server hat die Anmeldung nicht bestätigt.").into());
    }
    let iterations: u32 = i.parse().map_err(|_| "SCRAM".to_string())?;
    let salted = pbkdf2(password.as_bytes(), &base64_decode(&s), iterations);
    let client_key = hmac(&salted, b"Client Key");
    let stored = Sha256::digest(client_key);
    let without_proof = format!("c=biws,r={r}");
    let auth = format!("{client_first_bare},{server_first},{without_proof}");
    let sig = hmac(&stored, auth.as_bytes());
    let proof: Vec<u8> = client_key.iter().zip(sig).map(|(a, b)| a ^ b).collect();
    let server_key = hmac(&salted, b"Server Key");
    let server_sig = hmac(&server_key, auth.as_bytes());
    Ok((format!("{without_proof},p={}", base64(&proof)), server_sig.to_vec()))
}

// ---------------------------------------------------------------- Verbindung

struct Client {
    s: Box<dyn Stream>,
    id: i32,
}

fn err_of(d: &Doc) -> String {
    d.get("errmsg").and_then(Value::as_str).unwrap_or("MongoDB").to_string()
}

fn is_ok(d: &Doc) -> bool {
    d.get("ok").and_then(Value::as_f64).is_some_and(|v| v >= 1.0)
}

impl Client {
    fn open(link: &Link) -> Result<Client, String> {
        let (host, port) = (link.host.as_str(), link.port);
        let addr = (host, port).to_socket_addrs().map_err(|e| format!("{host}:{port}: {e}"))?.next().ok_or_else(|| format!("{host}: unbekannter Server"))?;
        let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| format!("{host}:{port}: {e}"))?;
        tcp.set_read_timeout(Some(Duration::from_secs(60))).ok();
        tcp.set_nodelay(true).ok();
        let s: Box<dyn Stream> = if link.conf.tls.on() { Box::new(wrap_tls(&link.conf.host, tcp, link.conf.tls)?) } else { Box::new(tcp) };
        let mut c = Client { s, id: 0 };
        let hello = c.command("admin", B::default().i32("hello", 1).bool("helloOk", true))?;
        if !is_ok(&hello) {
            return Err(err_of(&hello));
        }
        let conf = &link.conf;
        if !conf.user.is_empty() {
            // Zuerst in "admin" anmelden, dann in der gewaehlten Datenbank (dort angelegte Konten)
            let first = c.scram(&conf.user, &conf.password, "admin");
            if let Err(e) = first {
                if conf.database.is_empty() || conf.database == "admin" {
                    return Err(e);
                }
                c.scram(&conf.user, &conf.password, &conf.database).map_err(|_| e)?;
            }
        }
        Ok(c)
    }

    fn command(&mut self, db: &str, body: B) -> Result<Doc, String> {
        let doc = body.str("$db", db).finish();
        self.id += 1;
        let mut msg = Vec::with_capacity(doc.len() + 21);
        msg.extend(((doc.len() + 21) as i32).to_le_bytes());
        msg.extend(self.id.to_le_bytes());
        msg.extend(0i32.to_le_bytes());
        msg.extend(2013i32.to_le_bytes());
        msg.extend(0u32.to_le_bytes());
        msg.push(0);
        msg.extend(doc);
        self.s.write_all(&msg).map_err(|e| e.to_string())?;
        let mut head = [0u8; 16];
        self.s.read_exact(&mut head).map_err(|e| e.to_string())?;
        let len = i32::from_le_bytes(head[0..4].try_into().unwrap());
        let op = i32::from_le_bytes(head[12..16].try_into().unwrap());
        if !(21..=64 * 1024 * 1024).contains(&len) || op != 2013 {
            return Err(crate::i18n::text("Unerwartete Antwort von MongoDB.").into());
        }
        let mut body = vec![0u8; len as usize - 16];
        self.s.read_exact(&mut body).map_err(|e| e.to_string())?;
        let flags = u32::from_le_bytes(body[0..4].try_into().unwrap());
        let end = if flags & 1 != 0 { body.len() - 4 } else { body.len() };
        if body.get(4) != Some(&0) {
            return Err(crate::i18n::text("Unerwartete Antwort von MongoDB.").into());
        }
        let mut i = 5;
        match docs::bson_doc(&body[..end], &mut i, false) {
            Some(Value::Object(d)) => Ok(d),
            _ => Err(crate::i18n::text("Unerwartete Antwort von MongoDB.").into()),
        }
    }

    fn scram(&mut self, user: &str, password: &str, db: &str) -> Result<(), String> {
        let mut h = Sha256::new();
        for _ in 0..3 {
            use std::hash::{BuildHasher, Hasher};
            let mut x = std::collections::hash_map::RandomState::new().build_hasher();
            x.write_u64(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0));
            h.update(x.finish().to_le_bytes());
        }
        let nonce = base64(&h.finalize()[..18]);
        let bare = format!("n={},r={nonce}", user.replace('=', "=3D").replace(',', "=2C"));
        let start = self.command(db, B::default().i32("saslStart", 1).str("mechanism", "SCRAM-SHA-256").bin("payload", format!("n,,{bare}").as_bytes()).bool("autoAuthorize", true))?;
        if !is_ok(&start) {
            return Err(crate::i18n::text("Anmeldung fehlgeschlagen – Benutzer oder Passwort falsch.").to_string());
        }
        let payload = |d: &Doc| -> Vec<u8> { d.get("payload").and_then(|p| p.get("$binary")).and_then(|b| b.get("base64")).and_then(Value::as_str).map(base64_decode).unwrap_or_default() };
        let server_first = String::from_utf8_lossy(&payload(&start)).into_owned();
        let conv = start.get("conversationId").and_then(Value::as_i64).unwrap_or(1) as i32;
        let (client_final, server_sig) = scram_final(password, &bare, &server_first, &nonce)?;
        let cont = self.command(db, B::default().i32("saslContinue", 1).i32("conversationId", conv).bin("payload", client_final.as_bytes()))?;
        if !is_ok(&cont) {
            return Err(crate::i18n::text("Anmeldung fehlgeschlagen – Benutzer oder Passwort falsch.").to_string());
        }
        let server_final = String::from_utf8_lossy(&payload(&cont)).into_owned();
        if server_final.strip_prefix("v=").map(base64_decode).as_deref() != Some(server_sig.as_slice()) {
            return Err(crate::i18n::text("Der Server hat die Anmeldung nicht bestätigt.").into());
        }
        if cont.get("done").and_then(Value::as_bool) != Some(true) {
            let fin = self.command(db, B::default().i32("saslContinue", 1).i32("conversationId", conv).bin("payload", b""))?;
            if !is_ok(&fin) {
                return Err(err_of(&fin));
            }
        }
        Ok(())
    }

    /// Alle Dokumente einer Abfrage (find) nacheinander
    fn find(&mut self, db: &str, coll: &str, f: &mut dyn FnMut(Doc) -> Result<bool, String>) -> Result<(), String> {
        let mut r = self.command(db, B::default().str("find", coll).i32("batchSize", 1000))?;
        loop {
            if !is_ok(&r) {
                return Err(err_of(&r));
            }
            let cursor = r.get("cursor").and_then(Value::as_object).cloned().unwrap_or_default();
            let id = cursor.get("id").and_then(Value::as_i64).unwrap_or(0);
            let batch = cursor.get("firstBatch").or_else(|| cursor.get("nextBatch")).and_then(Value::as_array).cloned().unwrap_or_default();
            for d in batch {
                if let Value::Object(o) = d
                    && !f(o)?
                {
                    return Ok(());
                }
            }
            if id == 0 {
                return Ok(());
            }
            r = self.command(db, B::default().i64("getMore", id).str("collection", coll).i32("batchSize", 1000))?;
        }
    }
}

fn names(d: &Doc, list: &str) -> Vec<String> {
    let arr = if list == "databases" { d.get("databases") } else { d.get("cursor").and_then(|c| c.get("firstBatch")) };
    arr.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string)).collect()).unwrap_or_default()
}

/// Datenbanken des Servers ohne die Systemdatenbanken
pub fn databases(link: &Link) -> Result<Vec<String>, String> {
    let mut c = Client::open(link)?;
    let r = c.command("admin", B::default().i32("listDatabases", 1).bool("nameOnly", true))?;
    if !is_ok(&r) {
        return Err(err_of(&r));
    }
    Ok(names(&r, "databases").into_iter().filter(|d| !matches!(d.as_str(), "admin" | "local" | "config")).collect())
}

pub fn scan(link: &Link, cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let db = link.conf.database.clone();
    if db.is_empty() {
        return Err(crate::i18n::text("Bitte eine Datenbank wählen.").into());
    }
    let mut c = Client::open(link)?;
    let r = c.command(&db, B::default().i32("listCollections", 1).bool("nameOnly", true).doc("filter", B::default().str("type", "collection")))?;
    if !is_ok(&r) {
        return Err(err_of(&r));
    }
    let mut colls = names(&r, "cursor");
    colls.retain(|n| !n.starts_with("system."));
    colls.sort();
    let mut out = Vec::new();
    for coll in colls {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let origin = format!("mongodb\u{0}{coll}");
        out.push(docs::scan_collection(coll.clone(), origin, &mut |f| c.find(&db, &coll, f), cancel)?);
    }
    Ok(out)
}

pub fn read(link: &Link, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let coll = t.origin.split('\u{1}').next().unwrap_or("").split('\u{0}').nth(1).unwrap_or("").to_string();
    let db = link.conf.database.clone();
    let mut c = Client::open(link)?;
    docs::read_with(t, &mut |g| c.find(&db, &coll, g), f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_helpers_match_test_vectors() {
        // RFC 6070 (angepasst auf SHA-256): PBKDF2-HMAC-SHA256("password", "salt", 1)
        let k = pbkdf2(b"password", b"salt", 1);
        assert_eq!(k.iter().map(|b| format!("{b:02x}")).collect::<String>(), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        // RFC 4231 Test Case 2
        let m = hmac(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(m.iter().map(|b| format!("{b:02x}")).collect::<String>(), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert_eq!(base64_decode("TWFu"), b"Man");
        assert_eq!(base64_decode("TWE="), b"Ma");
    }

    #[test]
    fn scram_sha256_matches_rfc7677() {
        let bare = "n=user,r=rOprNGfwEbeRWgbNEkqO";
        let server_first = "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        let (fin, sig) = scram_final("pencil", bare, server_first, "rOprNGfwEbeRWgbNEkqO").unwrap();
        assert_eq!(fin, "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=");
        assert_eq!(base64(&sig), "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=");
        assert!(scram_final("pencil", bare, "r=other,s=AA==,i=1", "rOprNGfwEbeRWgbNEkqO").is_err());
    }

    #[test]
    fn bson_command_is_well_formed() {
        let d = B::default().i32("find", 1).str("x", "ab").bool("t", true).bin("b", &[1, 2]).doc("d", B::default().i64("n", 5)).finish();
        let mut i = 0;
        let v = docs::bson_doc(&d, &mut i, false).unwrap();
        assert_eq!(i, d.len());
        assert_eq!(v["find"], 1);
        assert_eq!(v["x"], "ab");
        assert_eq!(v["t"], true);
        assert_eq!(v["d"]["n"], 5);
        assert_eq!(v["b"]["$binary"]["base64"], "AQI=");
    }
}

#[cfg(test)]
mod live {
    use super::*;
    use crate::transfer::server::ServerConf;

    /// Braucht MongoDB auf 127.0.0.1:27117 mit der Datenbank "shopx" (Sammlungen kunden und orders mit 2500 Dokumenten);
    /// mit der Umgebungsvariable MONGO_USER/MONGO_PASSWORD auch mit Anmeldung (SCRAM-SHA-256).
    #[test]
    #[ignore]
    fn read_from_a_real_server() {
        let mut conf = ServerConf::new(27117);
        conf.user = std::env::var("MONGO_USER").unwrap_or_default();
        conf.password = std::env::var("MONGO_PASSWORD").unwrap_or_default();
        let link = conf.open().unwrap();
        let dbs = databases(&link).unwrap();
        assert!(dbs.contains(&"shopx".to_string()), "{dbs:?}");
        let mut l = link.clone();
        l.conf.database = "shopx".into();
        let tables = scan(&l, &AtomicBool::new(false)).unwrap();
        assert_eq!(tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["kunden", "orders"]);
        let orders = tables.iter().find(|t| t.name == "orders").unwrap();
        assert_eq!(orders.rows, 2500, "getMore muss alle Dokumente liefern");
        let kunden = tables.iter().find(|t| t.name == "kunden").unwrap();
        let mut rows = Vec::new();
        read(&l, kunden, &mut |r| {
            rows.push(r);
            Ok(true)
        })
        .unwrap();
        assert_eq!(rows.len(), 2);
        let names: Vec<&str> = kunden.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names[0], "_id");
        let seit = names.iter().position(|n| *n == "seit").unwrap();
        assert_eq!(rows[0][seit], Val::Text("2024-05-17 10:20:30.000".into()));
    }
}
