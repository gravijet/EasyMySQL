// Zugriff ueber SSH: ein Tunnel (ssh -L) vom eigenen Rechner zum Datenbankserver.
// Benutzt den OpenSSH-Client des Systems (bei Windows 10/11 und macOS/Linux vorhanden).

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct SshConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Pfad zur privaten Schluesseldatei (leer = ssh-Agent und Standardschluessel)
    pub key: String,
    pub password: String,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self { enabled: false, host: String::new(), port: 22, user: String::new(), key: String::new(), password: String::new() }
    }
}

/// Laufender Tunnel; wird beim Loslassen beendet.
pub struct Tunnel {
    child: Child,
    pub local_port: u16,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Umgebungsvariable, ueber die ssh das Passwort von diesem Programm erfragt (SSH_ASKPASS)
pub const ASKPASS_VAR: &str = "EASYMYSQL_ASKPASS";

/// Wird dieses Programm von ssh als Passwortabfrage gestartet, das Passwort ausgeben und beenden.
pub fn answer_askpass() -> bool {
    match std::env::var(ASKPASS_VAR) {
        Ok(p) => {
            println!("{p}");
            true
        }
        Err(_) => false,
    }
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

pub fn args(cfg: &SshConfig, local_port: u16, remote_host: &str, remote_port: u16) -> Vec<String> {
    let mut a: Vec<String> = ["-N", "-T", "-o", "ExitOnForwardFailure=yes", "-o", "ServerAliveInterval=30", "-o", "ConnectTimeout=12", "-o", "StrictHostKeyChecking=accept-new"].iter().map(|s| s.to_string()).collect();
    if cfg.password.is_empty() {
        // Ohne Passwort darf ssh nie nachfragen (kein Terminal vorhanden)
        a.extend(["-o".into(), "BatchMode=yes".into()]);
    } else {
        a.extend(["-o".into(), "PreferredAuthentications=password,keyboard-interactive".into()]);
    }
    if !cfg.key.trim().is_empty() {
        a.extend(["-i".into(), cfg.key.trim().to_string(), "-o".into(), "IdentitiesOnly=yes".into()]);
    }
    a.extend(["-p".into(), cfg.port.to_string(), "-L".into(), format!("127.0.0.1:{local_port}:{remote_host}:{remote_port}")]);
    let target = if cfg.user.trim().is_empty() { cfg.host.trim().to_string() } else { format!("{}@{}", cfg.user.trim(), cfg.host.trim()) };
    a.push(target);
    a
}

/// Meldung von ssh auf das Wesentliche kuerzen
fn short_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("Warning: Permanently added")).collect();
    match lines.last() {
        Some(l) => l.to_string(),
        None => crate::i18n::text("Der SSH-Tunnel konnte nicht aufgebaut werden.").to_string(),
    }
}

/// Tunnel aufbauen und warten, bis er bereit ist. `remote_host`/`remote_port`: Datenbankserver aus Sicht des SSH-Servers.
pub fn open(cfg: &SshConfig, remote_host: &str, remote_port: u16) -> Result<Tunnel, String> {
    if cfg.host.trim().is_empty() {
        return Err(crate::i18n::text("Kein SSH-Server angegeben.").into());
    }
    let local_port = free_port()?;
    let mut cmd = Command::new("ssh");
    cmd.args(args(cfg, local_port, remote_host, remote_port)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    if !cfg.password.is_empty() {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        cmd.env("SSH_ASKPASS", me).env("SSH_ASKPASS_REQUIRE", "force").env(ASKPASS_VAR, &cfg.password);
        if std::env::var_os("DISPLAY").is_none() {
            cmd.env("DISPLAY", ":0");
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            crate::i18n::text("ssh wurde nicht gefunden. Der OpenSSH-Client muss installiert sein.").to_string()
        } else {
            e.to_string()
        }
    })?;
    let start = Instant::now();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, local_port));
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            let mut err = String::new();
            if let Some(mut s) = child.stderr.take() {
                let _ = s.read_to_string(&mut err);
            }
            return Err(short_error(&err));
        }
        if TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok() {
            return Ok(Tunnel { child, local_port });
        }
        if start.elapsed() > Duration::from_secs(25) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::i18n::text("Zeitüberschreitung beim Aufbau des SSH-Tunnels.").into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line() {
        let mut c = SshConfig { enabled: true, host: "srv.example.org".into(), port: 2200, user: "anna".into(), key: "C:\\k\\id".into(), password: String::new() };
        let a = args(&c, 40000, "127.0.0.1", 3306);
        assert!(a.windows(2).any(|w| w == ["-L", "127.0.0.1:40000:127.0.0.1:3306"]));
        assert!(a.windows(2).any(|w| w == ["-p", "2200"]) && a.windows(2).any(|w| w == ["-i", "C:\\k\\id"]));
        assert!(a.contains(&"BatchMode=yes".to_string()));
        assert_eq!(a.last().unwrap(), "anna@srv.example.org");
        c.password = "x".into();
        c.user.clear();
        let a = args(&c, 1, "db", 3307);
        assert!(!a.contains(&"BatchMode=yes".to_string()));
        assert_eq!(a.last().unwrap(), "srv.example.org");
        assert_eq!(short_error("Warning: Permanently added 'x' to the list\nanna@h: Permission denied (publickey).\n"), "anna@h: Permission denied (publickey).");
    }

    /// Braucht sshd auf 127.0.0.1:2222 (Benutzer tester, Schluessel /tmp/sshd/id, Passwort pw123) und MariaDB auf 3306.
    #[test]
    #[ignore]
    fn tunnel_to_database() {
        use crate::db::{ConnInfo, Db};
        let mut cfg = SshConfig { enabled: true, host: "127.0.0.1".into(), port: 2222, user: "tester".into(), key: "/tmp/sshd/id".into(), password: String::new() };
        let t = open(&cfg, "127.0.0.1", 3306).unwrap();
        let db = Db::connect(&ConnInfo { host: "127.0.0.1".into(), port: t.local_port, ..ConnInfo::default() }).unwrap();
        assert!(db.version.contains("MariaDB"));
        drop(db);
        drop(t);
        // falscher Schluessel
        cfg.key = "/tmp/sshd/hostkey".into();
        let e = open(&cfg, "127.0.0.1", 3306).err().unwrap();
        assert!(e.contains("denied") || e.contains("Permission"), "{e}");
    }
}
