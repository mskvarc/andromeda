//! Loading the broker's identity and client trust anchors from PEM files.

use crate::crypto_provider::broker_crypto_provider;
use andromeda_configuration::schema::tls_material_sources::{ClientTrust, TlsMaterialSources};
use rustls::{RootCertStore, sign::CertifiedKey};
use rustls_pki_types::{
    CertificateDer,
    PrivateKeyDer,
    pem::{Error as PemError, PemObject},
};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;

/// PEM material that cannot form a server identity or a client trust store.
#[derive(Debug, Error)]
pub enum TlsMaterialError {
    /// A certificate, key or CA file could not be read.
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
        source: PemError,
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
    /// A client CA bundle contained no certificates.
    #[error("no client CA certificates found in {path}")]
    NoClientRoots {
        /// The bundle that was empty of certificates.
        path: PathBuf,
    },
    /// A client CA certificate is not a usable trust anchor.
    #[error("client CA certificate in {path} is not a usable trust anchor: {source}")]
    ClientRoot {
        /// The bundle holding the rejected certificate.
        path: PathBuf,
        /// Why the certificate was rejected.
        source: rustls::Error,
    },
    /// The key cannot be used, or does not certify the leaf certificate.
    #[error("certificate and key are not a valid server identity: {0}")]
    Identity(#[source] rustls::Error),
}

/// How a listener treats client certificates, with the roots it verifies them against.
#[derive(Debug, Clone)]
pub enum ClientVerification {
    /// No client certificate is requested.
    Anonymous,
    /// A presented certificate must chain to these roots; none may be presented.
    Optional(Arc<RootCertStore>),
    /// A certificate chaining to these roots must be presented.
    Required(Arc<RootCertStore>),
}

impl ClientVerification {
    /// The roots client certificates are verified against, when any are requested.
    fn roots(&self) -> Option<&RootCertStore> {
        match self {
            ClientVerification::Anonymous => None,
            ClientVerification::Optional(roots) | ClientVerification::Required(roots) => Some(roots),
        }
    }
}

/// Whether freshly loaded material differs from what is being served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialChange {
    /// The same chain, key and client roots: nothing to republish.
    Unchanged,
    /// Something a handshake would present or verify against changed.
    Changed,
}

/// A loaded, internally consistent server identity and client trust.
///
/// The key is proven to certify the leaf when the material is loaded, so a
/// mismatched pair is rejected before any listener is handed it.
#[derive(Debug, Clone)]
pub struct TlsMaterial {
    /// The certificate chain and the signing key for its leaf.
    certified_key: Arc<CertifiedKey>,
    /// Which client certificates are requested, and what they must chain to.
    client_verification: ClientVerification,
}

impl TlsMaterial {
    /// Load the identity and client trust the sources name.
    ///
    /// # Errors
    ///
    /// Returns a [`TlsMaterialError`] when a file cannot be read, its PEM
    /// cannot be parsed, it holds no usable item, a client CA is not a usable
    /// trust anchor, or the key does not certify the leaf.
    pub fn load(sources: &TlsMaterialSources<'_>) -> Result<TlsMaterial, TlsMaterialError> {
        let chain = load_certificate_chain(sources.certificate_path())?;
        let key = load_private_key(sources.private_key_path())?;
        let certified_key = CertifiedKey::from_der(chain, key, &broker_crypto_provider()).map_err(TlsMaterialError::Identity)?;
        let client_verification = match sources.client_trust() {
            ClientTrust::Anonymous => ClientVerification::Anonymous,
            ClientTrust::Optional(bundle) => ClientVerification::Optional(Arc::new(load_client_roots(bundle)?)),
            ClientTrust::Required(bundle) => ClientVerification::Required(Arc::new(load_client_roots(bundle)?)),
        };
        Ok(TlsMaterial {
            certified_key: Arc::new(certified_key),
            client_verification,
        })
    }

    /// The certificate chain and the signing key for its leaf.
    #[must_use]
    pub fn certified_key(&self) -> Arc<CertifiedKey> {
        Arc::clone(&self.certified_key)
    }

    /// The leaf certificate the broker presents.
    #[must_use]
    pub fn leaf(&self) -> Option<&CertificateDer<'static>> {
        self.certified_key.cert.first()
    }

    /// Which client certificates are requested, and what they must chain to.
    #[must_use]
    pub const fn client_verification(&self) -> &ClientVerification {
        &self.client_verification
    }

    /// Whether this material presents or verifies anything `previous` does not.
    #[must_use]
    pub fn change_from(&self, previous: &TlsMaterial) -> MaterialChange {
        let same_chain = self.certified_key.cert == previous.certified_key.cert;
        let same_key = self.certified_key.key.public_key() == previous.certified_key.key.public_key();
        let same_roots = self.client_verification.roots().map(|roots| &roots.roots) == previous.client_verification.roots().map(|roots| &roots.roots);
        if same_chain && same_key && same_roots {
            MaterialChange::Unchanged
        } else {
            MaterialChange::Changed
        }
    }
}

/// Read a file, reporting a failure against its path.
fn read(path: &Path) -> Result<Vec<u8>, TlsMaterialError> {
    fs::read(path).map_err(|source| TlsMaterialError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// PEM-decode every certificate in a file.
fn certificates_in(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsMaterialError> {
    CertificateDer::pem_slice_iter(&read(path)?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsMaterialError::Pem {
            path: path.to_path_buf(),
            source,
        })
}

/// Read and PEM-decode a certificate chain, leaf first.
fn load_certificate_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsMaterialError> {
    let chain = certificates_in(path)?;
    if chain.is_empty() {
        return Err(TlsMaterialError::NoCertificates { path: path.to_path_buf() });
    }
    Ok(chain)
}

/// Read and PEM-decode a single private key, accepting PKCS#8, SEC1, or PKCS#1.
fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsMaterialError> {
    // `PrivateKeyDer::from_pem_slice` reports a file without a key as
    // `NoItemsFound`; every other `pem::Error` is a genuine parse failure. The
    // enum is `#[non_exhaustive]`, so the one variant treated specially is
    // tested for rather than matched exhaustively.
    PrivateKeyDer::from_pem_slice(&read(path)?).map_err(|source| {
        if matches!(source, PemError::NoItemsFound) {
            TlsMaterialError::NoPrivateKey { path: path.to_path_buf() }
        } else {
            TlsMaterialError::Pem {
                path: path.to_path_buf(),
                source,
            }
        }
    })
}

/// Read a PEM bundle of client CA certificates into a trust store.
fn load_client_roots(path: &Path) -> Result<RootCertStore, TlsMaterialError> {
    let certificates = certificates_in(path)?;
    if certificates.is_empty() {
        return Err(TlsMaterialError::NoClientRoots { path: path.to_path_buf() });
    }
    let mut roots = RootCertStore::empty();
    certificates.into_iter().try_for_each(|certificate| {
        roots.add(certificate).map_err(|source| TlsMaterialError::ClientRoot {
            path: path.to_path_buf(),
            source,
        })
    })?;
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use crate::{
        pem_material::{ClientVerification, MaterialChange, TlsMaterial, TlsMaterialError, load_certificate_chain, load_client_roots, load_private_key},
        test_identity::{authority::TestAuthority, identity_files::IdentityFiles},
    };
    use andromeda_configuration::schema::tls::{ClientAuthenticationConfig, ClientAuthenticationMode, TlsConfig};
    use std::{error::Error, fs, path::Path};
    use tempfile::TempDir;

    /// A `[tls]` section naming the key pair in `files`, requiring client
    /// certificates chained to `client_ca` when one is given.
    fn section_of(files: &IdentityFiles, client_ca: Option<&Path>) -> TlsConfig {
        TlsConfig {
            certificate: Some(files.certificate_path().to_path_buf()),
            private_key: Some(files.private_key_path().to_path_buf()),
            client_authentication: ClientAuthenticationConfig {
                mode: client_ca.map_or(ClientAuthenticationMode::Disabled, |_| ClientAuthenticationMode::Required),
                ca: client_ca.map(Path::to_path_buf),
            },
            ..TlsConfig::default()
        }
    }

    #[test]
    fn a_missing_certificate_file_is_reported_with_its_path() {
        let error = load_certificate_chain(Path::new("/nonexistent/broker.crt")).map(|_| ());
        assert!(
            matches!(&error, Err(TlsMaterialError::Read { path, .. }) if path == Path::new("/nonexistent/broker.crt")),
            "{error:?}"
        );
    }

    #[test]
    fn a_certificate_file_without_certificates_is_rejected() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let empty = directory.path().join("empty.crt");
        fs::write(&empty, b"not a certificate\n")?;
        let error = load_certificate_chain(&empty).map(|_| ());
        assert!(matches!(error, Err(TlsMaterialError::NoCertificates { .. })), "{error:?}");
        Ok(())
    }

    #[test]
    fn a_key_file_without_a_key_is_rejected() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let empty = directory.path().join("empty.key");
        fs::write(&empty, b"not a key\n")?;
        let error = load_private_key(&empty).map(|_| ());
        assert!(matches!(error, Err(TlsMaterialError::NoPrivateKey { .. })), "{error:?}");
        Ok(())
    }

    #[test]
    fn a_self_signed_pair_loads() -> Result<(), Box<dyn Error>> {
        let identity = TestAuthority::generate()?.issue_server_identity()?;
        let files = IdentityFiles::write(&identity)?;
        let material = TlsMaterial::load(&section_of(&files, None).material_sources()?)?;
        pretty_assertions::assert_eq!(material.leaf(), Some(identity.certificate()));
        assert!(matches!(material.client_verification(), ClientVerification::Anonymous));
        Ok(())
    }

    #[test]
    fn a_key_not_matching_the_leaf_is_rejected() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let files = IdentityFiles::write(&first)?;
        fs::write(files.private_key_path(), second.private_key_pem())?;
        let error = TlsMaterial::load(&section_of(&files, None).material_sources()?).map(|_| ());
        assert!(matches!(error, Err(TlsMaterialError::Identity(_))), "{error:?}");
        Ok(())
    }

    #[test]
    fn a_client_ca_bundle_without_certificates_is_rejected() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let bundle = directory.path().join("clients.pem");
        fs::write(&bundle, b"no certificates here\n")?;
        let error = load_client_roots(&bundle).map(|_| ());
        assert!(matches!(error, Err(TlsMaterialError::NoClientRoots { .. })), "{error:?}");
        Ok(())
    }

    #[test]
    fn a_required_client_ca_bundle_is_loaded_into_the_verification() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let bundle = files.write_client_ca(&authority)?;
        let material = TlsMaterial::load(&section_of(&files, Some(&bundle)).material_sources()?)?;
        assert!(matches!(material.client_verification(), ClientVerification::Required(roots) if roots.len() == 1));
        Ok(())
    }

    #[test]
    fn reloading_the_same_files_is_unchanged_and_a_new_pair_is_a_change() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let section = section_of(&files, None);
        let sources = section.material_sources()?;
        let first = TlsMaterial::load(&sources)?;
        pretty_assertions::assert_eq!(TlsMaterial::load(&sources)?.change_from(&first), MaterialChange::Unchanged);
        files.replace(&authority.issue_server_identity()?)?;
        pretty_assertions::assert_eq!(TlsMaterial::load(&sources)?.change_from(&first), MaterialChange::Changed);
        Ok(())
    }
}
