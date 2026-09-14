//! Self-signed TLS material shared across the HTTP/3 test modules.
//!
//! HTTP/3 cannot be exercised without a certificate, so every test that binds
//! or dials a QUIC endpoint needs a throwaway key pair. Generating it once here
//! keeps that setup out of the individual test modules and gives the dialling
//! side the exact certificate to trust as its only root.

use crate::config::{Http3Config, Http3Mode};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls_pki_types::CertificateDer;
use std::{error::Error, fs::File, io::Write};
use tempfile::TempDir;

/// A generated self-signed identity written to disk, with the paths wired into
/// an enabled [`Http3Config`] and the leaf certificate kept for client trust.
pub(crate) struct SelfSignedListener {
    /// Keeps the certificate and key files alive for the listener's lifetime.
    _directory: TempDir,
    /// An enabled configuration pointing at the written key pair.
    config: Http3Config,
    /// The leaf certificate, for a dialling client's root store.
    certificate: CertificateDer<'static>,
}

impl SelfSignedListener {
    /// Generate a `localhost` identity and write it to a fresh temporary directory.
    pub(crate) fn generate() -> Result<SelfSignedListener, Box<dyn Error>> {
        let directory = TempDir::new()?;
        let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".to_owned()])?;
        let certificate_path = directory.path().join("broker.crt");
        let private_key_path = directory.path().join("broker.key");
        File::create(&certificate_path)?.write_all(cert.pem().as_bytes())?;
        File::create(&private_key_path)?.write_all(key_pair.serialize_pem().as_bytes())?;
        let config = Http3Config {
            mode: Http3Mode::Enabled,
            certificate_path,
            private_key_path,
            ..Http3Config::default()
        };
        Ok(SelfSignedListener {
            _directory: directory,
            config,
            certificate: cert.der().clone(),
        })
    }

    /// The enabled configuration referencing the written key pair.
    pub(crate) fn config(&self) -> &Http3Config {
        &self.config
    }

    /// The leaf certificate a dialling client must trust as its sole root.
    pub(crate) fn certificate(&self) -> CertificateDer<'static> {
        self.certificate.clone()
    }
}
