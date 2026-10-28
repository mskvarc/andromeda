//! The version a migration file carries in the first four characters of its name.

use std::str::FromStr;

/// The number of digits every migration version is written with.
const VERSION_DIGITS: usize = 4;

/// A migration version, written as exactly four decimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0:04}")]
pub struct MigrationVersion(u16);

impl MigrationVersion {
    /// The number the version stands for.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl FromStr for MigrationVersion {
    type Err = MigrationVersionError;

    /// Read a version written as exactly four ASCII digits, leading zeros included.
    fn from_str(written: &str) -> Result<MigrationVersion, MigrationVersionError> {
        if written.len() != VERSION_DIGITS || !written.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(MigrationVersionError::NotFourDigits);
        }
        written.parse().map(MigrationVersion).map_err(|_| MigrationVersionError::NotFourDigits)
    }
}

/// Why a version could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MigrationVersionError {
    /// The text is not exactly four ASCII digits.
    #[error("a migration version is exactly four decimal digits")]
    NotFourDigits,
}

#[cfg(test)]
mod tests {
    use crate::migration_version::{MigrationVersion, MigrationVersionError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_four_digit_version_keeps_its_leading_zeros_when_written() {
        let version = "0031".parse::<MigrationVersion>();
        assert_eq!(version.map(|version| (version.get(), version.to_string())), Ok((31, "0031".to_owned())));
    }

    #[test]
    fn a_version_not_written_as_four_digits_is_refused() {
        for written in ["31", "00031", "003a", "+031", " 031", ""] {
            assert_eq!(written.parse::<MigrationVersion>(), Err(MigrationVersionError::NotFourDigits), "{written:?}");
        }
    }
}
