//! A leaf certificate and the private key it certifies.

use rcgen::KeyPair;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A leaf certificate with its PKCS#8 private key, in the encodings a listener
/// reads from disk and a client presents in memory.
pub struct TestIdentity {
    /// The DER leaf certificate.
    certificate: CertificateDer<'static>,
    /// The PEM leaf certificate.
    certificate_pem: String,
    /// The PEM PKCS#8 private key.
    private_key_pem: String,
    /// The DER PKCS#8 private key.
    private_key_der: Vec<u8>,
}

impl TestIdentity {
    /// Package an issued certificate with the key it certifies.
    pub(crate) fn new(certificate: CertificateDer<'static>, certificate_pem: String, key: &KeyPair) -> TestIdentity {
        TestIdentity {
            certificate,
            certificate_pem,
            private_key_pem: key.serialize_pem(),
            private_key_der: key.serialize_der(),
        }
    }

    /// The DER leaf certificate.
    #[must_use]
    pub const fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }

    /// The PEM leaf certificate.
    #[must_use]
    pub fn certificate_pem(&self) -> &str {
        &self.certificate_pem
    }

    /// The PEM PKCS#8 private key.
    #[must_use]
    pub fn private_key_pem(&self) -> &str {
        &self.private_key_pem
    }

    /// The certificate followed by the key, the single PEM bundle an HTTP
    /// client identity is built from.
    #[must_use]
    pub fn identity_pem(&self) -> String {
        format!("{}{}", self.certificate_pem, self.private_key_pem)
    }

    /// The DER private key, owned for a TLS configuration to consume.
    #[must_use]
    pub fn private_key(&self) -> PrivateKeyDer<'static> {
        // A rustls configuration takes its key by value, so the bytes are
        // copied out for each configuration built from this identity.
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.private_key_der.clone()))
    }
}
