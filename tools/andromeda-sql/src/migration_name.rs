//! The name of a migration file and the naming rule it follows.
//!
//! A migration is named `NNNN_snake_case.sql`: four version digits, an
//! underscore, and a description of lowercase ASCII words joined by single
//! underscores, the first word starting with a letter.

use crate::migration_version::{MigrationVersion, MigrationVersionError};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::str::FromStr;

/// The extension every migration file carries.
const SQL_EXTENSION: &str = ".sql";

/// The snake-case description that follows the version in a file name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display, derive_more::AsRef)]
#[as_ref(str)]
pub struct MigrationDescription(String);

impl MigrationDescription {
    /// Accept `written` when it is lowercase ASCII words joined by single
    /// underscores, the first word starting with a letter.
    fn parse(written: &str) -> Option<MigrationDescription> {
        let starts_with_letter = written.bytes().next().is_some_and(|byte| byte.is_ascii_lowercase());
        let words_are_well_formed = written
            .split('_')
            .all(|word| !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
        (starts_with_letter && words_are_well_formed).then(|| MigrationDescription(written.to_owned()))
    }
}

/// A file name that follows the naming rule, `NNNN_snake_case.sql`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display, SerializeDisplay, DeserializeFromStr)]
#[display("{version}_{description}.sql")]
pub struct MigrationName {
    /// The version the name starts with.
    version: MigrationVersion,
    /// The description it continues with.
    description: MigrationDescription,
}

impl MigrationName {
    /// The version the name starts with.
    #[must_use]
    pub const fn version(&self) -> MigrationVersion {
        self.version
    }
}

impl FromStr for MigrationName {
    type Err = MigrationNameError;

    /// Read a file name that follows the naming rule.
    fn from_str(name: &str) -> Result<MigrationName, MigrationNameError> {
        let stem = name.strip_suffix(SQL_EXTENSION).ok_or(MigrationNameError::NotSql)?;
        let (version, description) = stem.split_once('_').ok_or(MigrationNameError::NoDescription)?;
        Ok(MigrationName {
            version: version.parse().map_err(MigrationNameError::Version)?,
            description: MigrationDescription::parse(description).ok_or(MigrationNameError::Description)?,
        })
    }
}

/// Which part of the naming rule a file name breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MigrationNameError {
    /// The name is not valid UTF-8.
    #[error("the name is not valid UTF-8")]
    NotUtf8,
    /// The name does not end in `.sql`.
    #[error("the name does not end in .sql")]
    NotSql,
    /// No underscore separates a version from a description.
    #[error("no underscore separates the version from the description")]
    NoDescription,
    /// The part before the first underscore is not a version.
    #[error(transparent)]
    Version(MigrationVersionError),
    /// The part after the first underscore is not snake case.
    #[error("the description is not lowercase words joined by single underscores")]
    Description,
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_name::{MigrationName, MigrationNameError},
        migration_version::MigrationVersionError,
    };
    use pretty_assertions::assert_eq;

    fn reason(name: &str) -> Option<MigrationNameError> {
        name.parse::<MigrationName>().err()
    }

    #[test]
    fn a_conforming_name_reads_back_as_written() {
        let name = "0029_subscription_notified_registrations.sql".parse::<MigrationName>();
        assert_eq!(
            name.map(|name| (name.version().get(), name.to_string())),
            Ok((29, "0029_subscription_notified_registrations.sql".to_owned()))
        );
    }

    #[test]
    fn each_broken_part_of_the_name_is_named() {
        assert_eq!(reason("0032_entity_maps.txt"), Some(MigrationNameError::NotSql));
        assert_eq!(reason("0032.sql"), Some(MigrationNameError::NoDescription));
        assert_eq!(
            reason("032_entity_maps.sql"),
            Some(MigrationNameError::Version(MigrationVersionError::NotFourDigits))
        );
        assert_eq!(
            reason("0032-entity_maps.sql"),
            Some(MigrationNameError::Version(MigrationVersionError::NotFourDigits))
        );
        assert_eq!(reason("0032_Entity_maps.sql"), Some(MigrationNameError::Description));
        assert_eq!(reason("0032_entity__maps.sql"), Some(MigrationNameError::Description));
        assert_eq!(reason("0032_entity_maps_.sql"), Some(MigrationNameError::Description));
        assert_eq!(reason("0032_1st_entity_maps.sql"), Some(MigrationNameError::Description));
        assert_eq!(reason("0032_entity-maps.sql"), Some(MigrationNameError::Description));
        assert_eq!(reason("0032_entity_maps_v2.sql"), None);
    }
}
