//! The stamped schema version.

/// The schema version a tenant database is stamped with: the highest version
/// among the migrations the running binary embeds.
///
/// `schema_metadata.schema_version` is a positive `INTEGER`, so a version that
/// does not fit that domain is refused here rather than by the column's check
/// constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub struct SchemaVersion(i32);

impl SchemaVersion {
    /// The version as the `INTEGER` the column stores.
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl TryFrom<i64> for SchemaVersion {
    type Error = SchemaVersionError;

    /// Narrow a migration version — sqlx models them as `i64` — into the
    /// positive `INTEGER` domain the column declares.
    fn try_from(value: i64) -> Result<SchemaVersion, SchemaVersionError> {
        let narrowed = i32::try_from(value).map_err(|_| SchemaVersionError::OutOfRange { value })?;
        if narrowed <= 0 {
            return Err(SchemaVersionError::NotPositive { value });
        }
        Ok(SchemaVersion(narrowed))
    }
}

/// What refuses a value the schema-version domain cannot hold.
#[derive(Debug, thiserror::Error)]
pub enum SchemaVersionError {
    /// The value does not fit the `INTEGER` the column declares.
    #[error("schema version {value} does not fit the INTEGER column")]
    OutOfRange {
        /// The rejected value.
        value: i64,
    },
    /// The value is zero or negative, which the column's check constraint forbids.
    #[error("schema version {value} is not positive")]
    NotPositive {
        /// The rejected value.
        value: i64,
    },
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::schema_version::{SchemaVersion, SchemaVersionError};

    #[test]
    fn a_positive_migration_version_narrows_into_the_integer_column_domain() {
        let version = match SchemaVersion::try_from(39_i64) {
            Ok(value) => value,
            Err(error) => unreachable!("39 is a schema version: {error}"),
        };
        pretty_assertions::assert_eq!(version.get(), 39);
        pretty_assertions::assert_eq!(version.to_string(), "39");
    }

    #[test]
    fn a_version_beyond_the_integer_column_is_refused_rather_than_truncated() {
        let error = SchemaVersion::try_from(i64::from(i32::MAX) + 1);
        assert!(matches!(error, Err(SchemaVersionError::OutOfRange { .. })));
    }

    #[test]
    fn a_non_positive_version_is_refused_before_the_check_constraint_sees_it() {
        assert!(matches!(SchemaVersion::try_from(0), Err(SchemaVersionError::NotPositive { .. })));
        assert!(matches!(SchemaVersion::try_from(-1), Err(SchemaVersionError::NotPositive { .. })));
    }
}
