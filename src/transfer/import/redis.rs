// Redis lesen (eigener, kleiner RESP-Client): Schluessel nach Typ in Tabellen aufteilen.
//   redis_strings(key, value, ttl)   redis_hashes(key, field, value)   redis_lists(key, idx, value)
//   redis_sets(key, member)          redis_zsets(key, member, score)

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct RedisConf {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub db: u32,
}

impl Default for RedisConf {
    fn default() -> Self {
        RedisConf { host: "127.0.0.1".into(), port: 6379, user: String::new(), password: String::new(), db: 0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Reply {
    Nil,
    Int(i64),
    Str(Vec<u8>),
    Arr(Vec<Reply>),
}

impl Reply {
    fn bytes(&self) -> Vec<u8> {
        match self {
            Reply::Str(b) => b.clone(),
            Reply::Int(i) => i.to_string().into_bytes(),
            _ => Vec::new(),
        }
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }
    fn int(&self) -> i64 {
        match self {
            Reply::Int(i) => *i,
            Reply::Str(b) => String::from_utf8_lossy(b).trim().parse().unwrap_or(0),
            _ => 0,
        }
    }
    fn list(self) -> Vec<Reply> {
        match self {
            Reply::Arr(a) => a,
            _ => Vec::new(),
        }
    }
}

struct Client {
    r: BufReader<TcpStream>,
}

impl Client {
    fn connect(c: &RedisConf) -> Result<Client, String> {
        let addr = (c.host.as_str(), c.port).to_socket_addrs().map_err(|e| format!("{}:{}: {e}", c.host, c.port))?.next().ok_or_else(|| format!("{}: unbekannter Server", c.host))?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{}:{}: {e}", c.host, c.port))?;
        s.set_read_timeout(Some(Duration::from_secs(30))).ok();
        s.set_nodelay(true).ok();
        let mut cl = Client { r: BufReader::new(s) };
        if !c.password.is_empty() {
            let mut args: Vec<&[u8]> = vec![b"AUTH"];
            if !c.user.is_empty() {
                args.push(c.user.as_bytes());
            }
            args.push(c.password.as_bytes());
            cl.cmd(&args)?;
        }
        if c.db != 0 {
            cl.cmd(&[b"SELECT", c.db.to_string().as_bytes()])?;
        }
        Ok(cl)
    }

    fn send(&mut self, args: &[&[u8]]) -> Result<(), String> {
        let mut buf = format!("*{}\r\n", args.len()).into_bytes();
        for a in args {
            buf.extend(format!("${}\r\n", a.len()).as_bytes());
            buf.extend(*a);
            buf.extend(b"\r\n");
        }
        self.r.get_mut().write_all(&buf).map_err(|e| e.to_string())
    }

    fn cmd(&mut self, args: &[&[u8]]) -> Result<Reply, String> {
        self.send(args)?;
        self.reply()
    }

    /// Mehrere Befehle auf einmal senden und alle Antworten lesen (viel schneller als einzeln)
    fn pipeline(&mut self, cmds: &[Vec<Vec<u8>>]) -> Result<Vec<Reply>, String> {
        let mut buf = Vec::new();
        for c in cmds {
            buf.extend(format!("*{}\r\n", c.len()).as_bytes());
            for a in c {
                buf.extend(format!("${}\r\n", a.len()).as_bytes());
                buf.extend(a);
                buf.extend(b"\r\n");
            }
        }
        self.r.get_mut().write_all(&buf).map_err(|e| e.to_string())?;
        cmds.iter().map(|_| self.reply()).collect()
    }

    fn line(&mut self) -> Result<String, String> {
        let mut l = String::new();
        if self.r.read_line(&mut l).map_err(|e| e.to_string())? == 0 {
            return Err(crate::i18n::text("Redis hat die Verbindung beendet.").into());
        }
        Ok(l.trim_end_matches(['\r', '\n']).to_string())
    }

    fn reply(&mut self) -> Result<Reply, String> {
        let l = self.line()?;
        let (kind, rest) = l.split_at(1.min(l.len()));
        match kind {
            "+" => Ok(Reply::Str(rest.as_bytes().to_vec())),
            "-" => Err(format!("Redis: {rest}")),
            ":" => Ok(Reply::Int(rest.parse().unwrap_or(0))),
            "$" => {
                let n: i64 = rest.parse().map_err(|_| "Redis: ungültige Antwort".to_string())?;
                if n < 0 {
                    return Ok(Reply::Nil);
                }
                let mut b = vec![0u8; n as usize + 2];
                self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
                b.truncate(n as usize);
                Ok(Reply::Str(b))
            }
            "*" => {
                let n: i64 = rest.parse().map_err(|_| "Redis: ungültige Antwort".to_string())?;
                if n < 0 {
                    return Ok(Reply::Nil);
                }
                (0..n).map(|_| self.reply()).collect::<Result<_, _>>().map(Reply::Arr)
            }
            _ => Err(format!("Redis: unerwartete Antwort „{l}“")),
        }
    }
}

const KINDS: [(&str, &str); 5] = [("string", "redis_strings"), ("hash", "redis_hashes"), ("list", "redis_lists"), ("set", "redis_sets"), ("zset", "redis_zsets")];

fn cmd(parts: &[&[u8]]) -> Vec<Vec<u8>> {
    parts.iter().map(|p| p.to_vec()).collect()
}

/// Alle Schluessel mit ihrem Typ
fn keys(cl: &mut Client, cancel: &AtomicBool) -> Result<Vec<(Vec<u8>, String)>, String> {
    let mut out = Vec::new();
    let mut cursor = b"0".to_vec();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("abgebrochen".into());
        }
        let r = cl.cmd(&[b"SCAN", &cursor, b"COUNT", b"1000"])?.list();
        let [next, found] = <[Reply; 2]>::try_from(r).map_err(|_| "Redis: ungültige Antwort auf SCAN".to_string())?;
        let found: Vec<Vec<u8>> = found.list().iter().map(Reply::bytes).collect();
        let cmds: Vec<_> = found.iter().map(|k| cmd(&[b"TYPE", k])).collect();
        for (k, t) in found.into_iter().zip(cl.pipeline(&cmds)?) {
            out.push((k, t.text()));
        }
        cursor = next.bytes();
        if cursor == b"0" {
            break;
        }
    }
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    Ok(out)
}

fn text(b: &[u8]) -> Val {
    Val::Text(String::from_utf8_lossy(b).into_owned())
}

fn layout(table: &str) -> Vec<(&'static str, ColType)> {
    let long = ColType::LongText;
    match table {
        "redis_strings" => vec![("key", ColType::Varchar(255)), ("value", long), ("ttl", ColType::BigInt)],
        "redis_hashes" => vec![("key", ColType::Varchar(255)), ("field", ColType::Varchar(255)), ("value", long)],
        "redis_lists" => vec![("key", ColType::Varchar(255)), ("idx", ColType::Int), ("value", long)],
        "redis_sets" => vec![("key", ColType::Varchar(255)), ("member", ColType::Varchar(255))],
        _ => vec![("key", ColType::Varchar(255)), ("member", ColType::Varchar(255)), ("score", ColType::Double)],
    }
}

pub fn scan(c: &RedisConf, cancel: &AtomicBool) -> Result<Vec<TableScan>, String> {
    let mut cl = Client::connect(c)?;
    let all = keys(&mut cl, cancel)?;
    let mut out = Vec::new();
    for (kind, table) in KINDS {
        let ks: Vec<&Vec<u8>> = all.iter().filter(|(_, t)| t == kind).map(|(k, _)| k).collect();
        if ks.is_empty() {
            continue;
        }
        // Zeilen zaehlen: Strings 1, sonst Groesse des Werts
        let mut rows = 0u64;
        if kind == "string" {
            rows = ks.len() as u64;
        } else {
            let len_cmd: &[u8] = match kind {
                "hash" => b"HLEN",
                "list" => b"LLEN",
                "set" => b"SCARD",
                _ => b"ZCARD",
            };
            for chunk in ks.chunks(500) {
                let cmds: Vec<_> = chunk.iter().map(|k| cmd(&[len_cmd, k])).collect();
                rows += cl.pipeline(&cmds)?.iter().map(Reply::int).sum::<i64>().max(0) as u64;
            }
        }
        // Laengster Schluessel/Feldname bestimmt die Spaltenlaenge nicht: alle sind VARCHAR(255) oder LONGTEXT
        let columns = layout(table).into_iter().map(|(n, t)| Col { name: n.into(), ty: t }).collect();
        let pk = if kind == "string" { vec!["key".to_string()] } else { vec![] };
        out.push(TableScan { name: table.into(), origin: kind.into(), columns, rows, pk, auto_inc: false, fks: vec![] });
    }
    Ok(out)
}

pub fn read(c: &RedisConf, t: &TableScan, f: &mut dyn FnMut(Vec<Val>) -> Result<bool, String>) -> Result<(), String> {
    let mut cl = Client::connect(c)?;
    let never = AtomicBool::new(false);
    let kind = t.origin.as_str();
    let keys: Vec<Vec<u8>> = keys(&mut cl, &never)?.into_iter().filter(|(_, k)| k == kind).map(|(k, _)| k).collect();
    for chunk in keys.chunks(200) {
        let cmds: Vec<_> = chunk
            .iter()
            .map(|k| match kind {
                "string" => cmd(&[b"GET", k]),
                "hash" => cmd(&[b"HGETALL", k]),
                "list" => cmd(&[b"LRANGE", k, b"0", b"-1"]),
                "set" => cmd(&[b"SMEMBERS", k]),
                _ => cmd(&[b"ZRANGE", k, b"0", b"-1", b"WITHSCORES"]),
            })
            .collect();
        let replies = cl.pipeline(&cmds)?;
        let ttls: Vec<Reply> = if kind == "string" { cl.pipeline(&chunk.iter().map(|k| cmd(&[b"TTL", k])).collect::<Vec<_>>())? } else { vec![] };
        for (i, (k, r)) in chunk.iter().zip(replies).enumerate() {
            let key = text(k);
            let go = match kind {
                "string" => {
                    let ttl = ttls[i].int();
                    f(vec![key, if r == Reply::Nil { Val::Null } else { text(&r.bytes()) }, if ttl >= 0 { Val::Int(ttl) } else { Val::Null }])?
                }
                "hash" => {
                    let flat = r.list();
                    let mut go = true;
                    for pair in flat.chunks(2) {
                        if let [fld, v] = pair {
                            go = f(vec![key.clone(), text(&fld.bytes()), text(&v.bytes())])?;
                            if !go {
                                break;
                            }
                        }
                    }
                    go
                }
                "list" => {
                    let mut go = true;
                    for (n, v) in r.list().iter().enumerate() {
                        go = f(vec![key.clone(), Val::Int(n as i64), text(&v.bytes())])?;
                        if !go {
                            break;
                        }
                    }
                    go
                }
                "set" => {
                    let mut go = true;
                    for v in r.list() {
                        go = f(vec![key.clone(), text(&v.bytes())])?;
                        if !go {
                            break;
                        }
                    }
                    go
                }
                _ => {
                    let flat = r.list();
                    let mut go = true;
                    for pair in flat.chunks(2) {
                        if let [m, s] = pair {
                            go = f(vec![key.clone(), text(&m.bytes()), s.text().parse().map(Val::Real).unwrap_or(Val::Null)])?;
                            if !go {
                                break;
                            }
                        }
                    }
                    go
                }
            };
            if !go {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Befehl ausfuehren (nur fuer Tests, die Daten anlegen)
#[cfg(test)]
pub fn exec(c: &RedisConf, args: &[&str]) {
    let mut cl = Client::connect(c).unwrap();
    let a: Vec<&[u8]> = args.iter().map(|x| x.as_bytes()).collect();
    cl.cmd(&a).unwrap();
}
