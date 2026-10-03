//! Self-signed host certificate and fingerprint pinning.

use std::path::Path;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("certificate error: {0}")]
    Cert(String),
}

pub struct HostIdentity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl HostIdentity {
    /// Load the host certificate from `dir`, generating one on first use.
    pub fn load_or_create(dir: &Path) -> Result<Self, TlsError> {
        let cert_p = dir.join("host-cert.der");
        let key_p = dir.join("host-key.der");
        if let (Ok(c), Ok(k)) = (std::fs::read(&cert_p), std::fs::read(&key_p)) {
            return Ok(Self {
                cert_der: c,
                key_der: k,
            });
        }
        std::fs::create_dir_all(dir)?;
        let mut params = rcgen::CertificateParams::new(vec![
            "conductor-host".to_string(),
            "localhost".to_string(),
        ])
        .map_err(|e| TlsError::Cert(e.to_string()))?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Conductor Host");
        params
            .subject_alt_names
            .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
                std::net::Ipv4Addr::LOCALHOST,
            )));
        let key = rcgen::KeyPair::generate().map_err(|e| TlsError::Cert(e.to_string()))?;
        let cert = params
            .self_signed(&key)
            .map_err(|e| TlsError::Cert(e.to_string()))?;
        let id = Self {
            cert_der: cert.der().to_vec(),
            key_der: key.serialize_der(),
        };
        std::fs::write(&cert_p, &id.cert_der)?;
        write_private(&key_p, &id.key_der)?;
        Ok(id)
    }

    /// SHA-256 fingerprint shown to the user during pairing, formatted
    /// `AB:CD:...`.
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert_der)
    }

    pub fn server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| TlsError::Cert(e.to_string()))?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert_der.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone())),
            )
            .map_err(|e| TlsError::Cert(e.to_string()))
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Client-side verifier that accepts exactly one certificate: the pinned
/// fingerprint. Hostname and CA are irrelevant because trust comes from the
/// out-of-band pairing step.
#[derive(Debug)]
pub struct PinnedVerifier {
    expected: String,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl PinnedVerifier {
    pub fn new(expected_fingerprint: &str) -> Self {
        Self {
            expected: expected_fingerprint.trim().to_ascii_uppercase(),
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _i: &[CertificateDer<'_>],
        _n: &ServerName<'_>,
        _o: &[u8],
        _t: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity.as_ref()) == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "host certificate does not match the paired fingerprint".into(),
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn pinned_client_config(fingerprint: &str) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier::new(fingerprint)))
        .with_no_client_auth()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_persists_and_fingerprint_is_stable() {
        let d = tempfile::tempdir().unwrap();
        let a = HostIdentity::load_or_create(d.path()).unwrap();
        let b = HostIdentity::load_or_create(d.path()).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 32 * 3 - 1);
        a.server_config().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert_ne!(
            HostIdentity::load_or_create(other.path())
                .unwrap()
                .fingerprint(),
            a.fingerprint()
        );
    }
}
