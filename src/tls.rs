// Verschluesselte Verbindungen (TLS) zum eigenen Server: eine eigene Zertifizierungsstelle (CA) und ein
// damit signiertes Serverzertifikat fuer diesen PC. Andere Rechner und Programme vertrauen der CA, indem sie
// ca.pem kennen (Einstellungen → Server → „CA-Zertifikat speichern“).

use rcgen::{BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose, SanType};
use std::path::PathBuf;

/// Wie der eigene Server verschluesselt
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Wie MariaDB es selbst macht (ab 11.4 mit einem bei jedem Start neu erzeugten Zertifikat)
    Default,
    /// Mit dem eigenen, dauerhaften Zertifikat; unverschluesselte Verbindungen bleiben moeglich
    On,
    /// Nur verschluesselte Verbindungen (ueber TCP; require_secure_transport)
    Required,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Default, Mode::On, Mode::Required];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Default => "standard",
            Mode::On => "an",
            Mode::Required => "pflicht",
        }
    }

    pub fn from_id(s: &str) -> Mode {
        Mode::ALL.into_iter().find(|m| m.id() == s).unwrap_or(Mode::Default)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Default => crate::i18n::tr("Standard", "Default"),
            Mode::On => crate::i18n::tr("Eigenes Zertifikat", "Own certificate"),
            Mode::Required => crate::i18n::tr("Nur verschlüsselt", "Encrypted only"),
        }
    }
}

pub struct Files {
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    /// Namen im Zertifikat (fuer "neu erzeugen, wenn sie sich aendern")
    names: PathBuf,
}

pub fn dir() -> PathBuf {
    crate::server::app_data_dir().join("tls")
}

pub fn files() -> Files {
    let d = dir();
    Files { ca: d.join("ca.pem"), cert: d.join("server-cert.pem"), key: d.join("server-key.pem"), names: d.join("namen.txt") }
}

/// Name dieses Rechners (fuer das Zertifikat)
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Alle Namen und Adressen, fuer die das Zertifikat gilt
pub fn names(extra: &str) -> Vec<String> {
    let mut v = vec!["localhost".to_string(), "127.0.0.1".into(), "::1".into()];
    if let Some(h) = host_name() {
        v.push(h.to_lowercase());
    }
    for n in extra.split([',', ' ', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            v.push(n.to_string());
        }
    }
    v
}

fn year_now() -> i32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

fn san(name: &str) -> Result<SanType, String> {
    match name.parse::<std::net::IpAddr>() {
        Ok(ip) => Ok(SanType::IpAddress(ip)),
        Err(_) => Ok(SanType::DnsName(name.to_string().try_into().map_err(|e: rcgen::Error| e.to_string())?)),
    }
}

/// CA und Serverzertifikat anlegen (vorhandene bleiben, solange die Namen gleich sind)
pub fn ensure(extra: &str) -> Result<Files, String> {
    let f = files();
    let wanted = names(extra).join("\n");
    let fresh = f.ca.is_file() && f.cert.is_file() && f.key.is_file() && std::fs::read_to_string(&f.names).is_ok_and(|n| n == wanted);
    if fresh {
        return Ok(f);
    }
    generate(extra)
}

/// Neue CA und neues Serverzertifikat (z. B. wenn ein Schluessel bekannt geworden ist)
pub fn generate(extra: &str) -> Result<Files, String> {
    let f = files();
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let y = year_now();
    let err = |e: rcgen::Error| e.to_string();

    let mut ca_params = CertificateParams::new(Vec::<String>::new()).map_err(err)?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("EasyMySQL CA ({})", host_name().unwrap_or_else(|| "localhost".into())));
    dn.push(DnType::OrganizationName, "EasyMySQL");
    ca_params.distinguished_name = dn;
    ca_params.not_before = rcgen::date_time_ymd(y - 1, 1, 1);
    ca_params.not_after = rcgen::date_time_ymd(y + 20, 1, 1);
    let ca_key = KeyPair::generate().map_err(err)?;
    let ca_cert = ca_params.self_signed(&ca_key).map_err(err)?;
    let issuer = rcgen::Issuer::new(ca_params, ca_key);

    let list = names(extra);
    let mut params = CertificateParams::new(Vec::<String>::new()).map_err(err)?;
    params.subject_alt_names = list.iter().map(|n| san(n)).collect::<Result<_, _>>()?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, host_name().unwrap_or_else(|| "localhost".into()));
    params.distinguished_name = dn;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = rcgen::date_time_ymd(y - 1, 1, 1);
    params.not_after = rcgen::date_time_ymd(y + 5, 1, 1);
    let key = KeyPair::generate().map_err(err)?;
    let cert = params.signed_by(&key, &issuer).map_err(err)?;

    let write = |p: &PathBuf, s: &str| crate::workspace::atomic_write(p, s).map(|_| ()).map_err(|e| format!("{}: {e}", p.display()));
    write(&f.ca, &ca_cert.pem())?;
    write(&f.cert, &cert.pem())?;
    write(&f.key, &key.serialize_pem())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f.key, std::fs::Permissions::from_mode(0o600));
    }
    write(&f.names, &list.join("\n"))?;
    Ok(f)
}

/// Zeilen fuer [mysqld] in my.ini
pub fn ini_lines(mode: Mode, extra: &str) -> Result<Vec<String>, String> {
    let slash = |p: &PathBuf| p.to_string_lossy().replace('\\', "/");
    match mode {
        Mode::Default => Ok(Vec::new()),
        Mode::On | Mode::Required => {
            let f = ensure(extra)?;
            let mut v = vec![format!("ssl-ca={}", slash(&f.ca)), format!("ssl-cert={}", slash(&f.cert)), format!("ssl-key={}", slash(&f.key))];
            if mode == Mode::Required {
                v.push("require_secure_transport=ON".into());
            }
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificates_are_created_and_chain_to_the_ca() {
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(ca_params, ca_key);
        let mut p = CertificateParams::new(Vec::<String>::new()).unwrap();
        p.subject_alt_names = names("db.local 192.168.1.5").iter().map(|n| san(n).unwrap()).collect();
        let key = KeyPair::generate().unwrap();
        let cert = p.signed_by(&key, &issuer).unwrap();
        assert!(ca.pem().starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(cert.pem().contains("BEGIN CERTIFICATE") && key.serialize_pem().contains("PRIVATE KEY"));
        let n = names("db.local, 192.168.1.5");
        assert!(n.contains(&"127.0.0.1".to_string()) && n.contains(&"db.local".to_string()) && n.contains(&"192.168.1.5".to_string()));
        assert!(matches!(san("::1").unwrap(), SanType::IpAddress(_)));
    }
}
