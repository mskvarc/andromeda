//! TLS 1.3 assembly for the QUIC endpoint.
//!
//! QUIC carries TLS 1.3 as its only handshake (RFC 9001); there is no cleartext
//! or TLS 1.2 mode. The endpoint therefore terminates TLS in process from a
//! certificate chain and private key on disk, advertising the HTTP/3 ALPN token
//! `h3` (RFC 9114 clause 3.1) so a client distinguishes it from an HTTP/2-over-
//! QUIC listener. The finished [`rustls::ServerConfig`] is handed to quinn,
//! which wraps it in its own QUIC crypto configuration.

use crate::config::Http3Activation;
use quinn::crypto::rustls::{NoInitialCipherSuite, QuicServerConfig};
use rustls::{ServerConfig, crypto::aws_lc_rs::default_provider};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

/// The ALPN protocol identifier for HTTP/3 over QUIC (RFC 9114 clause 3.1).
const H3_ALPN: &[u8] = b"h3";

/// Failure to assemble the QUIC TLS configuration.
#[derive(Debug, Error)]
pub enum Http3TlsError {
    /// A certificate or key file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        source: io::Error,
    },
    /// A PEM file could not be parsed into DER items.
    #[error("cannot parse PEM in {path}: {source}")]
    Pem {
        /// The file whose PEM content was rejected.
        path: PathBuf,
        /// The underlying parse failure.
        source: io::Error,
    },
    /// A certificate file contained no certificates.
    #[error("no certificates found in {path}")]
    NoCertificates {
        /// The certificate file that was empty of certificates.
        path: PathBuf,
    },
    /// A key file contained no PKCS#8, SEC1, or PKCS#1 private key.
    #[error("no private key found in {path}")]
    NoPrivateKey {
        /// The key file that was empty of private keys.
        path: PathBuf,
    },
    /// The rustls crypto provider rejected the safe default protocol versions.
    #[error("cannot enable TLS protocol versions: {0}")]
    ProtocolVersions(#[source] rustls::Error),
    /// The certificate and key were not a usable server identity.
    #[error("certificate and key are not a valid server identity: {0}")]
    Identity(#[source] rustls::Error),
    /// The TLS configuration produced no QUIC-compatible initial cipher suite.
    #[error("TLS configuration is not QUIC-compatible: {0}")]
    QuicIncompatible(#[source] NoInitialCipherSuite),
}

/// Read and PEM-decode a certificate chain, leaf first.
fn load_certificate_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>, Http3TlsError> {
    let bytes = fs::read(path).map_err(|source| Http3TlsError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = bytes.as_slice();
    let chain = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| Http3TlsError::Pem {
            path: path.to_path_buf(),
            source,
        })?;
    if chain.is_empty() {
        return Err(Http3TlsError::NoCertificates { path: path.to_path_buf() });
    }
    Ok(chain)
}

/// Read and PEM-decode a single private key, accepting PKCS#8, SEC1, or PKCS#1.
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, Http3TlsError> {
    let bytes = fs::read(path).map_err(|source| Http3TlsError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = bytes.as_slice();
    rustls_pemfile::private_key(&mut reader)
        .map_err(|source| Http3TlsError::Pem {
            path: path.to_path_buf(),
            source,
        })?
        .ok_or_else(|| Http3TlsError::NoPrivateKey { path: path.to_path_buf() })
}

/// Assemble the QUIC server crypto configuration for an active HTTP/3 listener.
///
/// The returned [`QuicServerConfig`] terminates TLS 1.3 with the activation's
/// certificate chain and advertises the `h3` ALPN token. It requires no client
/// certificate, matching the NGSI-LD edge where authentication, when present,
/// is an application-layer concern.
///
/// The rustls configuration is built against an explicit `aws-lc-rs` provider
/// rather than the process default, so it is unaffected by whichever provider
/// another workspace crate may have installed.
///
/// # Errors
///
/// Returns an [`Http3TlsError`] when a file cannot be read, its PEM cannot be
/// parsed, it holds no usable item, the certificate and key do not form a valid
/// identity, or the result is not QUIC-compatible.
pub fn quic_server_crypto(activation: Http3Activation<'_>) -> Result<QuicServerConfig, Http3TlsError> {
    let certificate_chain = load_certificate_chain(activation.certificate_path())?;
    let private_key = load_private_key(activation.private_key_path())?;
    let mut server_config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(Http3TlsError::ProtocolVersions)?
        .with_no_client_auth()
        .with_single_cert(certificate_chain, private_key)
        .map_err(Http3TlsError::Identity)?;
    server_config.alpn_protocols = vec![H3_ALPN.to_vec()];
    QuicServerConfig::try_from(server_config).map_err(Http3TlsError::QuicIncompatible)
}

#[cfg(test)]
mod tests {
    use crate::{
        test_support::SelfSignedListener,
        tls::{H3_ALPN, Http3TlsError, load_certificate_chain, load_private_key, quic_server_crypto},
    };
    use std::{error::Error, fs::File, io::Write, path::Path};
    use tempfile::TempDir;

    #[test]
    fn the_alpn_token_is_the_registered_http3_identifier() {
        assert_eq!(H3_ALPN, b"h3");
    }

    #[test]
    fn a_self_signed_key_pair_assembles_a_quic_configuration() -> Result<(), Box<dyn Error>> {
        let listener = SelfSignedListener::generate()?;
        let activation = listener.config().activation()?.expect("enabled config yields activation");
        quic_server_crypto(activation)?;
        Ok(())
    }

    #[test]
    fn a_missing_certificate_file_is_reported_with_its_path() {
        let error = load_certificate_chain(Path::new("/nonexistent/broker.crt")).expect_err("missing file fails");
        assert!(matches!(error, Http3TlsError::Read { .. }), "{error:?}");
    }

    #[test]
    fn a_certificate_file_without_certificates_is_rejected() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let empty = directory.path().join("empty.crt");
        File::create(&empty)?.write_all(b"not a certificate\n")?;
        let error = load_certificate_chain(&empty).expect_err("no certificates fails");
        assert!(matches!(error, Http3TlsError::NoCertificates { .. }), "{error:?}");
        Ok(())
    }

    #[test]
    fn a_key_file_without_a_key_is_rejected() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let empty = directory.path().join("empty.key");
        File::create(&empty)?.write_all(b"not a key\n")?;
        let error = load_private_key(&empty).expect_err("no key fails");
        assert!(matches!(error, Http3TlsError::NoPrivateKey { .. }), "{error:?}");
        Ok(())
    }
}
