//! Self-signed certificate provisioning for the HTTP/3 stack, which serves TLS 1.3
//! itself because QUIC has no cleartext mode. The key material is never committed,
//! so it is generated on demand before the stack comes up.

use crate::error::XtaskError;
use std::{fs, path::Path, process::Command};

/// Directory that holds the generated leaf certificate and key, relative to the root.
const CERTS_DIR: &str = "docker/compose/test/http3/certs";
/// Leaf certificate path, relative to the repository root.
const CERT_PATH: &str = "docker/compose/test/http3/certs/broker.crt";
/// Private key path, relative to the repository root.
const KEY_PATH: &str = "docker/compose/test/http3/certs/broker.key";

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
pub(crate) fn ensure(repository_root: &Path) -> Result<(), XtaskError> {
    if repository_root.join(CERT_PATH).exists() && repository_root.join(KEY_PATH).exists() {
        return Ok(());
    }
    fs::create_dir_all(repository_root.join(CERTS_DIR))?;
    let status = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "365",
            "-keyout",
            KEY_PATH,
            "-out",
            CERT_PATH,
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
        ])
        .current_dir(repository_root)
        .status()?;
    if status.success() { Ok(()) } else { Err(XtaskError::CertGeneration) }
}
