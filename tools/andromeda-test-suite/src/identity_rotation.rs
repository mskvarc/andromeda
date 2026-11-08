//! Rewriting the TLS identity a broker reads, `tls.crt` and `tls.key` in the directory the stack names for it, the way a
//! certificate manager does: each file is written beside its target and renamed into place, so the broker never reads
//! a half-written file. The broker reloads its identity when the directory changes; what a new connection is shown
//! tells a test which identity is in service.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    eventually::retried,
    presented_certificate::PresentedCertificate,
    tls_connection::{Connection, TlsError, read},
};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;
use std::{fs, io, path::PathBuf, time::Duration};

/// How long a broker of the stack takes at most to act on a rewritten identity: ten times the 500 ms its
/// `tls.reload_settle_after` waits for the files to settle.
pub const RELOAD_WINDOW: Duration = Duration::from_secs(5);

/// The file a broker reads its certificate chain from.
const CERTIFICATE: &str = "tls.crt";

/// The file a broker reads its private key from.
const KEY: &str = "tls.key";

/// Why an identity could not be rewritten.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The stack does not name the broker's identity directory or its own directory.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A file of the stack's identities cannot be read.
    #[error(transparent)]
    Source(#[from] TlsError),
    /// A file of the broker's identity cannot be written or renamed into place.
    #[error("{0} cannot be written")]
    Write(PathBuf, #[source] io::Error),
}

/// The identity files of one broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityFiles {
    /// The directory the broker reads them from.
    directory: PathBuf,
}

impl IdentityFiles {
    /// The identity files of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Config`] when the stack names no identity directory for the broker.
    pub fn of(role: BrokerRole) -> Result<IdentityFiles, IdentityError> {
        let config = SuiteConfig::get().map_err(|error| IdentityError::Config(error.clone()))?;
        Ok(IdentityFiles {
            directory: config.identity_dir(role)?.to_owned(),
        })
    }

    /// Puts the certificate and key in the PEM files `certificate` and `key` beneath the stack's directory in service,
    /// the key first.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError`] when a file cannot be read or written.
    pub fn install(&self, certificate: &str, key: &str) -> Result<(), IdentityError> {
        let config = SuiteConfig::get().map_err(|error| IdentityError::Config(error.clone()))?;
        let stack = config.stack_dir()?;
        self.replace(KEY, &read(&stack.join(key))?)?;
        self.replace(CERTIFICATE, &read(&stack.join(certificate))?)
    }

    /// Replaces the certificate file with `contents`, leaving the key as it is.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Write`] when the file cannot be written.
    pub fn replace_certificate(&self, contents: &[u8]) -> Result<(), IdentityError> {
        self.replace(CERTIFICATE, contents)
    }

    /// Writes `contents` beside the file `name` and renames it into place.
    fn replace(&self, name: &str, contents: &[u8]) -> Result<(), IdentityError> {
        let target = self.directory.join(name);
        let staging = target.with_added_extension("staging");
        // clone: the error owns a copy, as the staging path is still renamed into place
        fs::write(&staging, contents).map_err(|error| IdentityError::Write(staging.clone(), error))?;
        fs::rename(&staging, &target).map_err(|error| IdentityError::Write(target, error))
    }
}

/// The certificate the broker of `role` presents to a connection opened now, made the way `connection` describes.
///
/// # Errors
///
/// Returns the failure of a client that cannot be built, a request that is not answered, or an answer that carries no
/// certificate.
pub async fn presented_to_a_new_connection(connection: &Connection, role: BrokerRole) -> Result<PresentedCertificate> {
    let client = connection.client(role).await?;
    let headed = client.with_heads().with_accept(Accept::any()).source_identity().send().await?;
    Ok(PresentedCertificate::of(headed.head.extensions())?)
}

/// Waits until a connection opened to the broker of `role` is shown the certificate with `serial_number`.
///
/// # Errors
///
/// Returns the last failure when no new connection is shown it within the delivery deadline.
pub async fn await_presented(connection: &Connection, role: BrokerRole, serial_number: &str) -> Result<()> {
    retried(async || {
        let presented = presented_to_a_new_connection(connection, role).await?;
        verify_that!(presented.serial_number(), eq(serial_number))
    })
    .await
}

#[cfg(test)]
mod tests {
    use crate::identity_rotation::IdentityFiles;
    use googletest::prelude::*;
    use std::{env, fs, path::PathBuf};
    use uuid::Uuid;

    #[gtest]
    fn a_replaced_certificate_is_renamed_into_place_without_a_staging_file_left() -> Result<()> {
        let directory = env::temp_dir().join(format!("identity-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory)?;
        let files = IdentityFiles {
            directory: PathBuf::from(&directory),
        };
        files.replace_certificate(b"first")?;
        files.replace_certificate(b"second")?;
        expect_that!(fs::read(directory.join("tls.crt"))?, eq(b"second"));
        expect_that!(directory.join(".tls.crt.staging").exists(), eq(false));
        fs::remove_dir_all(&directory)?;
        Ok(())
    }
}
