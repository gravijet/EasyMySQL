// Verbindung zu einem anderen Datenbankserver (Quelle oder Ziel): Adresse, Anmeldung, TLS und
// auf Wunsch ein SSH-Tunnel. Wird fuer MariaDB/MySQL, PostgreSQL, MongoDB und Redis benutzt.

use crate::ssh::{self, SshConfig, Tunnel};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tls {
    #[default]
    Off,
    /// Verschluesselt, Zertifikat wird geprueft
    On,
    /// Verschluesselt, Zertifikat wird nicht geprueft (z. B. selbst signiert)
    NoVerify,
}

impl Tls {
    pub fn on(self) -> bool {
        self != Tls::Off
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerConf {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Datenbank bzw. Schema (leer = erst auswaehlen)
    pub database: String,
    pub tls: Tls,
    pub ssh: SshConfig,
}

impl ServerConf {
    pub fn new(port: u16) -> Self {
        Self { host: "127.0.0.1".into(), port, user: String::new(), password: String::new(), database: String::new(), tls: Tls::Off, ssh: SshConfig::default() }
    }

    /// Verbindung vorbereiten: bei SSH den Tunnel aufbauen. Der Tunnel lebt, solange der `Link` (oder eine Kopie) lebt.
    pub fn open(&self) -> Result<Link, String> {
        if self.host.trim().is_empty() {
            return Err(crate::i18n::text("Kein Server angegeben.").into());
        }
        if self.ssh.enabled {
            let t = ssh::open(&self.ssh, self.host.trim(), self.port)?;
            Ok(Link { host: "127.0.0.1".into(), port: t.local_port, conf: self.clone(), _tunnel: Some(Arc::new(t)) })
        } else {
            Ok(Link { host: self.host.trim().to_string(), port: self.port, conf: self.clone(), _tunnel: None })
        }
    }
}

/// Offene Verbindung zum Server: Adresse (bei SSH die des Tunnels) und Anmeldedaten
#[derive(Clone)]
pub struct Link {
    pub host: String,
    pub port: u16,
    pub conf: ServerConf,
    _tunnel: Option<Arc<Tunnel>>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Link({}:{})", self.host, self.port)
    }
}

// ---------------------------------------------------------------------------
// TLS

mod verify {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};

    /// Nimmt jedes Zertifikat an (nur fuer "Zertifikat nicht pruefen")
    #[derive(Debug)]
    pub struct AcceptAll(pub std::sync::Arc<rustls::crypto::CryptoProvider>);

    impl ServerCertVerifier for AcceptAll {
        fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, s: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls12_signature(m, c, s, &self.0.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, s: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
            rustls::crypto::verify_tls13_signature(m, c, s, &self.0.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

pub fn tls_config(tls: Tls) -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().expect("TLS-Protokolle");
    let cfg = if tls == Tls::NoVerify {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(verify::AcceptAll(provider))).with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    Arc::new(cfg)
}

/// Verschluesselte Verbindung ueber einen schon offenen TCP-Strom (z. B. fuer Redis)
pub fn wrap_tls(host: &str, tcp: std::net::TcpStream, tls: Tls) -> Result<rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>, String> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let conn = rustls::ClientConnection::new(tls_config(tls), name).map_err(|e| e.to_string())?;
    Ok(rustls::StreamOwned::new(conn, tcp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_link_keeps_address_and_tls_modes() {
        let mut c = ServerConf::new(5432);
        c.host = " db.example.org ".into();
        let l = c.open().unwrap();
        assert_eq!((l.host.as_str(), l.port), ("db.example.org", 5432));
        assert!(Tls::On.on() && Tls::NoVerify.on() && !Tls::Off.on());
        c.host.clear();
        assert!(c.open().is_err());
        // beide TLS-Konfigurationen lassen sich bauen
        let _ = tls_config(Tls::On);
        let _ = tls_config(Tls::NoVerify);
    }
}
