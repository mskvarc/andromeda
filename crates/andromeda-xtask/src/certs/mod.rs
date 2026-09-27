//! Test certificate material a stack needs before it comes up.
//!
//! The material is committed test material. A file of it that is missing is
//! generated with the `openssl` CLI (OpenSSL 3.0 or newer, for `req -CA`), so
//! the runner needs no cryptography of its own.

mod http3_leaf;
mod openssl;
mod tls_authority;

use crate::error::XtaskError;
use std::path::Path;

/// Which certificate material a stack's services mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StackMaterial {
    /// The stack terminates no TLS, or ships what it needs.
    None,
    /// One self-signed leaf the HTTP/3 broker presents and the suite trusts directly.
    Http3Leaf,
    /// A test authority, the leaves it issues, and client identities for the
    /// native-TLS and reverse-proxy brokers of the TLS stack.
    TlsAuthority,
}

impl StackMaterial {
    /// Generate the material this stack mounts.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::Io`] when a directory or file cannot be written and
    /// [`XtaskError::CertGeneration`] when `openssl` exits non-zero.
    pub(crate) fn provision(self, repository_root: &Path) -> Result<(), XtaskError> {
        match self {
            StackMaterial::None => Ok(()),
            StackMaterial::Http3Leaf => http3_leaf::ensure(repository_root),
            StackMaterial::TlsAuthority => tls_authority::ensure(repository_root),
        }
    }
}
