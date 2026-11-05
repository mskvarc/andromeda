//! The `pg_repack` version a tenant database runs, and whether the controller
//! drives it.
//!
//! The controller runs the extension's protocol itself, against statements
//! the extension generates and functions whose arguments and effects belong
//! to one release line. A run is therefore only started against the line the
//! protocol was written for; any other version is a missing capability, not
//! a run that could fail halfway.

use andromeda_postgresql_context_store::schema_contract::server_requirements::{ExtensionVersion, ExtensionVersionError};
use thiserror::Error;

/// The release line whose protocol the controller drives.
const SUPPORTED_LINE: (u32, u32) = (1, 5);

/// What `repack.version()` reports before the version number.
const VERSION_PREFIX: &str = "pg_repack ";

/// The version of the `pg_repack` library a database runs.
#[derive(Clone, Copy, Debug, derive_more::Display, Eq, PartialEq)]
#[display("pg_repack {_0}")]
pub(crate) struct RepackVersion(ExtensionVersion);

/// Whether the controller drives a version's protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepackSupport {
    /// The version is on the supported line.
    Supported,
    /// The version is on another line.
    Unsupported,
}

/// What refuses a `repack.version()` report.
#[derive(Debug, Error)]
pub enum RepackVersionError {
    /// The report does not start with the library's name.
    #[error("the pg_repack version report does not name pg_repack")]
    MissingName,
    /// The number after the name is not a version.
    #[error("the pg_repack version report holds no version: {0}")]
    Version(#[from] ExtensionVersionError),
}

impl RepackVersion {
    /// Whether the controller drives this version.
    pub(crate) const fn support(self) -> RepackSupport {
        if self.0.major == SUPPORTED_LINE.0 && self.0.minor == SUPPORTED_LINE.1 {
            RepackSupport::Supported
        } else {
            RepackSupport::Unsupported
        }
    }
}

impl TryFrom<&str> for RepackVersion {
    type Error = RepackVersionError;

    /// Read what `repack.version()` reports: `pg_repack 1.5.3`.
    fn try_from(report: &str) -> Result<RepackVersion, RepackVersionError> {
        let number = report.strip_prefix(VERSION_PREFIX).ok_or(RepackVersionError::MissingName)?;
        Ok(RepackVersion(ExtensionVersion::try_from(number)?))
    }
}

#[cfg(test)]
mod tests {
    use crate::repack::version::{RepackSupport, RepackVersion, RepackVersionError};
    use andromeda_postgresql_context_store::schema_contract::server_requirements::ExtensionVersion;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_shipped_release_is_supported() -> Result<(), RepackVersionError> {
        let version = RepackVersion::try_from("pg_repack 1.5.3")?;
        assert_eq!(version, RepackVersion(ExtensionVersion::new(1, 5, 3)));
        assert_eq!(version.support(), RepackSupport::Supported);
        assert_eq!(version.to_string(), "pg_repack 1.5.3");
        Ok(())
    }

    #[test]
    fn another_release_line_is_unsupported() -> Result<(), RepackVersionError> {
        assert_eq!(RepackVersion::try_from("pg_repack 1.4.8")?.support(), RepackSupport::Unsupported);
        assert_eq!(RepackVersion::try_from("pg_repack 2.0.0")?.support(), RepackSupport::Unsupported);
        Ok(())
    }

    #[test]
    fn a_report_without_a_version_is_refused() {
        assert!(matches!(RepackVersion::try_from("pg_repack"), Err(RepackVersionError::MissingName)));
        assert!(matches!(RepackVersion::try_from(""), Err(RepackVersionError::MissingName)));
        assert!(matches!(RepackVersion::try_from("pg_repack x.y"), Err(RepackVersionError::Version(_))));
    }
}
