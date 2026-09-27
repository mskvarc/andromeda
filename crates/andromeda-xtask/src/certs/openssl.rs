//! Generating keys and certificates through the `openssl` CLI.

use crate::error::XtaskError;
use std::{path::Path, process::Command};

/// The key options every generated key pair shares: a P-256 key, unencrypted,
/// for a certificate that stays valid for as long as the committed material is
/// expected to be kept.
const KEY_OPTIONS: [&str; 7] = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "3650"];

/// What signs a generated certificate.
pub(crate) enum Signer<'files> {
    /// The certificate signs itself.
    SelfSigned,
    /// An authority's certificate and key sign it.
    Authority {
        /// The authority's PEM certificate.
        certificate: &'files Path,
        /// The authority's PEM private key.
        private_key: &'files Path,
    },
}

/// Generate a fresh key pair and a certificate for `subject`, carrying
/// `extensions` and signed by `signer`, into `certificate` and `private_key`.
///
/// # Errors
///
/// Returns [`XtaskError::Io`] when `openssl` cannot be spawned and
/// [`XtaskError::CertGeneration`] when it exits non-zero.
pub(crate) fn issue(directory: &Path, subject: &str, extensions: &[&str], signer: &Signer<'_>, certificate: &str, private_key: &str) -> Result<(), XtaskError> {
    let mut command = Command::new("openssl");
    command
        .args(["req", "-x509"])
        .args(KEY_OPTIONS)
        .args(["-keyout", private_key, "-out", certificate, "-subj", subject]);
    for extension in extensions {
        command.args(["-addext", extension]);
    }
    if let Signer::Authority {
        certificate: authority_certificate,
        private_key: authority_key,
    } = signer
    {
        command.arg("-CA").arg(authority_certificate).arg("-CAkey").arg(authority_key);
    }
    let status = command.current_dir(directory).status()?;
    if status.success() { Ok(()) } else { Err(XtaskError::CertGeneration) }
}
