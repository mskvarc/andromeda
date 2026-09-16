//! The digest of the migration set a broker binary embeds.

use sha2::{Digest, Sha256};
use sqlx::migrate::Migration;

/// Separates the fields of one migration's digest record.
const FIELD_SEPARATOR: u8 = 0x1f;
/// Separates two migrations' digest records.
const RECORD_SEPARATOR: u8 = 0x1e;

/// The SHA-256 over the tenant migration set the running binary embeds.
///
/// Two binaries agree on this value exactly when they carry the same
/// migrations, with the same descriptions, in the same order, with the same
/// bodies — sqlx's own per-migration checksum covers the body. Framing each
/// record with the two ASCII separators is what keeps the digest injective: a
/// description ending in digits could otherwise be confused with the next
/// record's version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{}", hex::encode(_0))]
pub struct MigrationsDigest([u8; 32]);

impl MigrationsDigest {
    /// Digest a migration set.
    ///
    /// Down migrations are excluded because they never run against a tenant,
    /// and the set is ordered by version so the digest does not depend on the
    /// order the source happened to resolve.
    pub fn of<'migration>(migrations: impl IntoIterator<Item = &'migration Migration>) -> MigrationsDigest {
        MigrationsDigest::of_records(
            migrations
                .into_iter()
                .filter(|migration| !migration.migration_type.is_down_migration())
                .map(|migration| (migration.version, migration.description.as_ref(), migration.checksum.as_ref())),
        )
    }

    /// Digest a set of `(version, description, checksum)` records directly,
    /// bypassing the embedded [`Migration`] type.
    ///
    /// Shared by [`MigrationsDigest::of`], which digests the migrations the
    /// running binary embeds, and by the schema-contract reconciliation path,
    /// which digests rows read back from a tenant's own `_sqlx_migrations`
    /// table: the two must use the identical framing for a digest computed
    /// from one source to ever equal a digest computed from the other.
    pub(crate) fn of_records<'record>(records: impl IntoIterator<Item = (i64, &'record str, &'record [u8])>) -> MigrationsDigest {
        let mut ordered: Vec<(i64, &str, &[u8])> = records.into_iter().collect();
        ordered.sort_unstable_by_key(|(version, _, _)| *version);
        let ordered = ordered;

        let mut hasher = Sha256::new();
        for (version, description, checksum) in ordered {
            hasher.update(version.to_string().as_bytes());
            hasher.update([FIELD_SEPARATOR]);
            hasher.update(description.as_bytes());
            hasher.update([FIELD_SEPARATOR]);
            hasher.update(checksum);
            hasher.update([RECORD_SEPARATOR]);
        }
        MigrationsDigest(hasher.finalize().into())
    }
}

impl TryFrom<&str> for MigrationsDigest {
    type Error = MigrationsDigestError;

    /// Read back what `schema_metadata.migrations_sha256` stores.
    ///
    /// Only the canonical lowercase-hex form is accepted: the column's check
    /// constraint writes no other, and accepting a second spelling would make
    /// two different texts stamp the same contract.
    fn try_from(value: &str) -> Result<MigrationsDigest, MigrationsDigestError> {
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(|error| MigrationsDigestError::Unreadable {
            found: value.to_owned(),
            detail: error.to_string(),
        })?;
        if hex::encode(bytes) != value {
            return Err(MigrationsDigestError::Unreadable {
                found: value.to_owned(),
                detail: "a migrations digest is stored as lowercase hexadecimal".to_owned(),
            });
        }
        Ok(MigrationsDigest(bytes))
    }
}

/// What refuses a stored value the digest domain cannot hold.
#[derive(Debug, thiserror::Error)]
pub enum MigrationsDigestError {
    /// The stored text is not 64 lowercase hexadecimal digits.
    #[error("migrations digest '{found}' is not 64 lowercase hexadecimal digits: {detail}")]
    Unreadable {
        /// The text the database holds.
        found: String,
        /// Why it could not be read.
        detail: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::migrations_digest::{MigrationsDigest, MigrationsDigestError};
    use sqlx::migrate::{Migration, MigrationType};
    use std::borrow::Cow;

    /// Build one resolved migration with the shape `migrate!()` produces.
    fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
        Migration::new(
            version,
            Cow::Borrowed(description),
            MigrationType::ReversibleUp,
            sqlx::SqlStr::from_static(sql),
            false,
        )
    }

    #[test]
    fn the_same_migration_set_digests_to_the_same_value_whatever_order_it_arrives_in() {
        let ascending = vec![
            migration(1, "extensions", "CREATE EXTENSION postgis"),
            migration(2, "entities", "CREATE TABLE entities ()"),
        ];
        let descending = vec![
            migration(2, "entities", "CREATE TABLE entities ()"),
            migration(1, "extensions", "CREATE EXTENSION postgis"),
        ];

        pretty_assertions::assert_eq!(MigrationsDigest::of(&ascending), MigrationsDigest::of(&descending));
    }

    #[test]
    fn changing_a_migration_body_description_or_version_changes_the_digest() {
        let baseline = vec![migration(1, "extensions", "CREATE EXTENSION postgis")];
        let rebodied = vec![migration(1, "extensions", "CREATE EXTENSION timescaledb")];
        let redescribed = vec![migration(1, "extension", "CREATE EXTENSION postgis")];
        let renumbered = vec![migration(2, "extensions", "CREATE EXTENSION postgis")];

        let digest = MigrationsDigest::of(&baseline);
        assert_ne!(digest, MigrationsDigest::of(&rebodied));
        assert_ne!(digest, MigrationsDigest::of(&redescribed));
        assert_ne!(digest, MigrationsDigest::of(&renumbered));
    }

    #[test]
    fn a_down_migration_does_not_take_part_in_the_digest() {
        let up_only = vec![migration(1, "extensions", "CREATE EXTENSION postgis")];
        let mut with_down = up_only.clone(); // clone: the two sets differ only by the appended down migration
        with_down.push(Migration::new(
            1,
            Cow::Borrowed("extensions"),
            MigrationType::ReversibleDown,
            sqlx::SqlStr::from_static("DROP EXTENSION postgis"),
            false,
        ));

        pretty_assertions::assert_eq!(MigrationsDigest::of(&up_only), MigrationsDigest::of(&with_down));
    }

    #[test]
    fn a_digest_round_trips_through_its_stored_lowercase_hexadecimal_form() {
        let digest = MigrationsDigest::of(&vec![migration(1, "extensions", "CREATE EXTENSION postgis")]);
        let stored = digest.to_string();
        pretty_assertions::assert_eq!(stored.len(), 64);

        let parsed = match MigrationsDigest::try_from(stored.as_str()) {
            Ok(value) => value,
            Err(error) => panic!("a rendered digest parses: {error}"),
        };
        pretty_assertions::assert_eq!(parsed, digest);
    }

    #[test]
    fn of_records_agrees_with_of_over_the_same_migrations() {
        let migrations = vec![
            migration(1, "extensions", "CREATE EXTENSION postgis"),
            migration(2, "entities", "CREATE TABLE entities ()"),
        ];
        let records = migrations
            .iter()
            .map(|migration| (migration.version, migration.description.as_ref(), migration.checksum.as_ref()));

        pretty_assertions::assert_eq!(MigrationsDigest::of_records(records), MigrationsDigest::of(&migrations));
    }

    #[test]
    fn of_records_changes_when_a_records_checksum_changes() {
        let baseline = MigrationsDigest::of_records([(1_i64, "extensions", [1_u8, 2, 3].as_slice())]);
        let rechecksummed = MigrationsDigest::of_records([(1_i64, "extensions", [4_u8, 5, 6].as_slice())]);

        assert_ne!(baseline, rechecksummed);
    }

    #[test]
    fn a_stored_digest_that_is_not_canonical_lowercase_hexadecimal_is_refused() {
        for candidate in ["", "abc", &"A".repeat(64), &"z".repeat(64)] {
            let parsed = MigrationsDigest::try_from(candidate);
            assert!(matches!(parsed, Err(MigrationsDigestError::Unreadable { .. })), "{candidate:?} must be refused");
        }
    }
}
