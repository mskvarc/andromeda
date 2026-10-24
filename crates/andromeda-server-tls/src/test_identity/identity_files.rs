//! A server identity written to disk, where a listener and its watcher read it.

use crate::test_identity::{authority::TestAuthority, error::TestIdentityError, identity::TestIdentity};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// The certificate and key files of one server identity, in a temporary
/// directory that lives as long as this value.
pub struct IdentityFiles {
    /// The PEM certificate chain file.
    certificate_path: PathBuf,
    /// The PEM private key file.
    private_key_path: PathBuf,
    /// The directory holding both files, removed on drop.
    directory: TempDir,
}

impl IdentityFiles {
    /// Write `identity` into a fresh temporary directory.
    ///
    /// # Errors
    ///
    /// Returns [`TestIdentityError::Io`] when the directory or a file cannot be
    /// written.
    pub fn write(identity: &TestIdentity) -> Result<IdentityFiles, TestIdentityError> {
        let directory = TempDir::new()?;
        let files = IdentityFiles {
            certificate_path: directory.path().join("broker.crt"),
            private_key_path: directory.path().join("broker.key"),
            directory,
        };
        files.replace(identity)?;
        Ok(files)
    }

    /// Replace both files with `identity`.
    ///
    /// Each file is written beside its target and renamed over it, the way a
    /// certificate manager rotates material, so a watcher never reads a
    /// half-written file.
    ///
    /// # Errors
    ///
    /// Returns [`TestIdentityError::Io`] when a file cannot be written or renamed.
    pub fn replace(&self, identity: &TestIdentity) -> Result<(), TestIdentityError> {
        replace_atomically(&self.certificate_path, identity.certificate_pem())?;
        replace_atomically(&self.private_key_path, identity.private_key_pem())
    }

    /// Write `authority`'s certificate as a client CA bundle and return its path.
    ///
    /// # Errors
    ///
    /// Returns [`TestIdentityError::Io`] when the bundle cannot be written.
    pub fn write_client_ca(&self, authority: &TestAuthority) -> Result<PathBuf, TestIdentityError> {
        let path = self.directory.path().join("client-ca.pem");
        replace_atomically(&path, &authority.certificate_pem())?;
        Ok(path)
    }

    /// The directory holding the files.
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// The PEM certificate chain file.
    #[must_use]
    pub fn certificate_path(&self) -> &Path {
        &self.certificate_path
    }

    /// The PEM private key file.
    #[must_use]
    pub fn private_key_path(&self) -> &Path {
        &self.private_key_path
    }
}

/// Write `contents` to a sibling of `target` and rename it over `target`.
fn replace_atomically(target: &Path, contents: &str) -> Result<(), TestIdentityError> {
    let staging = target.with_extension("staging");
    fs::write(&staging, contents)?;
    fs::rename(&staging, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_identity::{authority::TestAuthority, identity_files::IdentityFiles};
    use std::{error::Error, fs};

    #[test]
    fn replacing_the_files_swaps_both_certificate_and_key() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let first = authority.issue_server_identity()?;
        let second = authority.issue_server_identity()?;
        let files = IdentityFiles::write(&first)?;
        pretty_assertions::assert_eq!(fs::read_to_string(files.certificate_path())?, first.certificate_pem());
        files.replace(&second)?;
        pretty_assertions::assert_eq!(fs::read_to_string(files.certificate_path())?, second.certificate_pem());
        pretty_assertions::assert_eq!(fs::read_to_string(files.private_key_path())?, second.private_key_pem());
        Ok(())
    }

    #[test]
    fn a_client_ca_bundle_is_written_beside_the_identity() -> Result<(), Box<dyn Error>> {
        let authority = TestAuthority::generate()?;
        let files = IdentityFiles::write(&authority.issue_server_identity()?)?;
        let bundle = files.write_client_ca(&authority)?;
        assert!(bundle.starts_with(files.directory()));
        pretty_assertions::assert_eq!(fs::read_to_string(bundle)?, authority.certificate_pem());
        Ok(())
    }
}
