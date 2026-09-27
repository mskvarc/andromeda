//! The self-signed leaf the HTTP/3 stack's broker presents, generated once.

use crate::{
    certs::openssl::{Signer, issue},
    error::XtaskError,
};
use std::{fs, path::Path};

/// Directory that holds the generated leaf certificate and key, relative to the root.
const CERTS_DIR: &str = "docker/compose/test/http3/certs";
/// Leaf certificate file name.
const CERTIFICATE: &str = "broker.crt";
/// Private key file name.
const PRIVATE_KEY: &str = "broker.key";

/// Generate the HTTP/3 leaf certificate and key if either is missing.
///
/// The SANs cover the names and address the client verifies against, and the leaf
/// is marked `CA:FALSE`: a client that trusts the self-signed leaf directly rejects
/// a CA-marked certificate presented as an end entity.
///
/// # Errors
///
/// Returns [`XtaskError::Io`] when the certificate directory cannot be created and
/// [`XtaskError::CertGeneration`] when `openssl` exits non-zero.
pub(super) fn ensure(repository_root: &Path) -> Result<(), XtaskError> {
    let directory = repository_root.join(CERTS_DIR);
    if directory.join(CERTIFICATE).exists() && directory.join(PRIVATE_KEY).exists() {
        return Ok(());
    }
    fs::create_dir_all(&directory)?;
    issue(
        &directory,
        "/CN=localhost",
        &["subjectAltName=DNS:localhost,IP:127.0.0.1", "basicConstraints=critical,CA:FALSE"],
        &Signer::SelfSigned,
        CERTIFICATE,
        PRIVATE_KEY,
    )
}
