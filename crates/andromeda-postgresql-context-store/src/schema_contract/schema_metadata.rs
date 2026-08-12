//! The `schema_metadata` singleton: stamping it, reading it, and verifying it.

use crate::schema_contract::{
    contract::SchemaContract,
    error::SchemaContractError,
    migrations_digest::MigrationsDigest,
    schema_version::SchemaVersion,
    server_requirements::verify_server_requirements,
};
use sqlx::{PgPool, Row};

/// Refuse a tenant whose schema is not the one this binary was built against,
/// stamping a database that has never carried a stamp and re-stamping one
/// this call itself confirms was legitimately upgraded to it.
///
/// The stamp is read first, before anything is written, because a stamp that
/// is already present is evidence about a database this process did not just
/// create: overwriting it on sight would turn real drift — a fleet running
/// two binaries, a tenant restored from an older backup, manual surgery —
/// into a silently repaired row instead of a refusal. Three cases follow from
/// that reading:
///
/// * No stamp at all: this is the one moment the migrated schema and the
///   binary's contract are known to agree, so this is the only other path
///   that writes one.
/// * A stamp at or ahead of the contract: compared as-is by
///   [`refuse_on_mismatch`], since the caller's migrator has already brought
///   the schema forward and a stamp this call did not just write can only
///   agree or be ahead of what it should be.
/// * A stamp behind the contract: [`reconcile_upgrade`] decides whether the
///   tenant was demonstrably carried forward by an earlier binary — in which
///   case the stamp is advanced — or the stamp is drift the migrator's own
///   checksum check could not see, in which case it is refused exactly like
///   any other mismatch.
///
/// Reading the stamp back after every write is not ceremony: it is what turns
/// a database whose `schema_metadata` row cannot be written or does not
/// survive into a refusal here rather than into a failure on the first query
/// that depends on the new shape.
pub async fn ensure_schema_contract(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    verify_server_requirements(pool).await?;

    let Some(stamped) = read_stamp(pool).await? else {
        stamp(pool, contract).await?;
        let restamped = read_stamp(pool).await?.ok_or(SchemaContractError::Unstamped {
            expected_version: contract.version(),
            expected_digest: contract.digest(),
        })?;
        return refuse_on_mismatch(&restamped, contract);
    };
    if stamped.version < contract.version() {
        return reconcile_upgrade(pool, &stamped, contract).await;
    }
    refuse_on_mismatch(&stamped, contract)
}

/// Advance a stamp that is behind the contract, once the tenant's own applied
/// migration history proves it is a completed upgrade rather than drift.
///
/// By the time this runs, [`migrate_tenant_db`] has already applied every
/// migration the binary embeds, so the tenant's schema itself is at
/// `contract`'s version; what is still unproven is whether the *stamp* left
/// behind by an earlier binary is trustworthy. It is trustworthy exactly when
/// the migrations actually recorded as applied, up to and including the
/// stamped version, digest to the value the stamp claims — proof that the
/// earlier binary's stamp described this tenant's real history rather than
/// one inherited from an unrelated migration path that happens to name the
/// same version.
///
/// `_sqlx_migrations` is queried rather than recomputed from the embedded
/// migrator's own list: a migration file the current binary has since deleted
/// would be invisible to a binary-only recomputation, which would silently
/// turn a diverged history into an accepted upgrade.
///
/// [`migrate_tenant_db`]: crate::tenant_database::migrate_tenant_db
async fn reconcile_upgrade(pool: &PgPool, stamped: &StampedContract, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    let applied_digest = applied_migrations_digest_up_to(pool, stamped.version).await?;
    if applied_digest != stamped.digest {
        return Err(SchemaContractError::MigrationsDigestMismatch {
            expected: applied_digest,
            found: stamped.digest,
        });
    }
    restamp(pool, contract).await?;
    let restamped = read_stamp(pool).await?.ok_or(SchemaContractError::Unstamped {
        expected_version: contract.version(),
        expected_digest: contract.digest(),
    })?;
    refuse_on_mismatch(&restamped, contract)
}

/// The digest of a tenant's actually-applied migrations, up to and including
/// `up_to_version`, as `_sqlx_migrations` itself records them.
async fn applied_migrations_digest_up_to(pool: &PgPool, up_to_version: SchemaVersion) -> Result<MigrationsDigest, SchemaContractError> {
    let rows = sqlx::query(
        r#"
        SELECT version, description, checksum
        FROM _sqlx_migrations
        WHERE success AND version <= $1
        ORDER BY version
        "#,
    )
    .bind(i64::from(up_to_version.get()))
    .fetch_all(pool)
    .await?;
    let records = rows
        .iter()
        .map(|row| {
            (
                row.get::<i64, _>("version"),
                row.get::<String, _>("description"),
                row.get::<Vec<u8>, _>("checksum"),
            )
        })
        .collect::<Vec<_>>();
    Ok(MigrationsDigest::of_records(
        records
            .iter()
            .map(|(version, description, checksum)| (*version, description.as_str(), checksum.as_slice())),
    ))
}

/// Refuse a tenant whose schema is not the one this binary was built against.
///
/// Unlike [`ensure_schema_contract`], this never writes: an absent stamp is
/// refused rather than filled in.
pub async fn verify_schema_contract(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    verify_server_requirements(pool).await?;

    let Some(stamped) = read_stamp(pool).await? else {
        return Err(SchemaContractError::Unstamped {
            expected_version: contract.version(),
            expected_digest: contract.digest(),
        });
    };
    refuse_on_mismatch(&stamped, contract)
}

/// Compare a stamp actually read from a tenant against what this binary
/// requires, refusing on the first mismatch.
fn refuse_on_mismatch(stamped: &StampedContract, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    if stamped.version != contract.version() {
        return Err(SchemaContractError::SchemaVersionMismatch {
            expected: contract.version(),
            found: stamped.version,
        });
    }
    if stamped.digest != contract.digest() {
        return Err(SchemaContractError::MigrationsDigestMismatch {
            expected: contract.digest(),
            found: stamped.digest,
        });
    }
    Ok(())
}

/// Write the singleton, if nothing has claimed it yet.
///
/// `DO NOTHING` rather than `DO UPDATE` on purpose: the caller has already
/// established the row was absent, but nothing stops a concurrent tenant boot
/// from winning the race between that read and this write. Losing the race
/// leaves the other writer's stamp in place, which the caller's read-back then
/// verifies like any other pre-existing stamp — a race resolves to a refusal
/// on mismatch, never to two stamps clobbering each other.
async fn stamp(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    sqlx::query(
        r#"
        INSERT INTO schema_metadata (applied_at, schema_version, singleton, migrations_sha256)
        VALUES (NOW(), $1, TRUE, $2)
        ON CONFLICT (singleton) DO NOTHING
        "#,
    )
    .bind(contract.version().get())
    .bind(contract.digest().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Advance an existing stamp to the contract the migrator just brought the
/// database up to.
///
/// Unlike [`stamp`], this always writes: the caller has already confirmed the
/// existing stamp's digest matches the tenant's true applied history, so this
/// is not a race between two first-boot writers deciding who claims an absent
/// row. Two concurrent upgraders reconciling the same tenant both compute the
/// same target version and digest from their own (identical) binary, so a
/// lost race here converges on the same values rather than clobbering a
/// competing write with a different one.
async fn restamp(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    sqlx::query(
        r#"
        UPDATE schema_metadata
        SET applied_at = NOW(), schema_version = $1, migrations_sha256 = $2
        WHERE singleton
        "#,
    )
    .bind(contract.version().get())
    .bind(contract.digest().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Read the singleton, if the tenant carries one.
async fn read_stamp(pool: &PgPool) -> Result<Option<StampedContract>, SchemaContractError> {
    let Some(row) = sqlx::query("SELECT schema_version, migrations_sha256 FROM schema_metadata WHERE singleton")
        .fetch_optional(pool)
        .await?
    else {
        return Ok(None);
    };
    let version = SchemaVersion::try_from(i64::from(row.get::<i32, _>("schema_version")))?;
    let digest = MigrationsDigest::try_from(row.get::<String, _>("migrations_sha256").as_str())?;
    Ok(Some(StampedContract { version, digest }))
}

/// What one tenant's `schema_metadata` row claims about its schema.
///
/// Distinct from [`SchemaContract`] on purpose: that type is what the binary
/// requires and is always well-formed, while this one is read back from a
/// database that may have been stamped by another binary entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StampedContract {
    /// The version the row carries.
    version: SchemaVersion,
    /// The digest the row carries.
    digest: MigrationsDigest,
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::{
        contract::SchemaContract,
        error::SchemaContractError,
        migrations_digest::MigrationsDigest,
        schema_metadata::{StampedContract, refuse_on_mismatch},
    };
    use sqlx::migrate::{Migration, MigrationType, Migrator};
    use std::borrow::Cow;

    /// Build one resolved migration with the shape `migrate!()` produces.
    fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
        Migration::new(version, Cow::Borrowed(description), MigrationType::ReversibleUp, Cow::Borrowed(sql), false)
    }

    /// The contract a migration set expresses, built the way `migrate!()`
    /// would build it but from an in-memory set instead of a filesystem walk.
    fn contract_of(migrations: Vec<Migration>) -> SchemaContract {
        let migrator = Migrator {
            migrations: Cow::Owned(migrations),
            ..Migrator::DEFAULT
        };
        match SchemaContract::of(&migrator) {
            Ok(contract) => contract,
            Err(error) => unreachable!("the fixture set has a contract: {error}"),
        }
    }

    /// A stamp that agrees with `contract` on both version and digest.
    fn matching_stamp(contract: &SchemaContract) -> StampedContract {
        StampedContract {
            version: contract.version(),
            digest: contract.digest(),
        }
    }

    #[test]
    fn a_stamp_that_matches_the_binarys_contract_on_version_and_digest_proceeds() {
        let contract = contract_of(vec![migration(5, "entities", "CREATE TABLE entities ()")]);
        let stamped = matching_stamp(&contract);

        assert!(refuse_on_mismatch(&stamped, &contract).is_ok(), "an agreeing stamp must not be refused");
    }

    #[test]
    fn a_stamp_with_a_different_version_is_refused_naming_both_versions() {
        let contract = contract_of(vec![migration(5, "entities", "CREATE TABLE entities ()")]);
        let behind_contract = contract_of(vec![migration(4, "entities", "CREATE TABLE entities ()")]);
        let stamped = StampedContract {
            version: behind_contract.version(),
            digest: contract.digest(),
        };

        match refuse_on_mismatch(&stamped, &contract) {
            Err(SchemaContractError::SchemaVersionMismatch { expected, found }) => {
                assert_eq!(expected, contract.version(), "the refusal must name what the binary requires");
                assert_eq!(found, behind_contract.version(), "the refusal must name what the tenant carries");
            }
            other => unreachable!("a version mismatch must refuse naming both versions: {other:?}"),
        }
    }

    #[test]
    fn a_stamp_with_the_right_version_but_a_different_digest_is_refused_naming_both_digests() {
        let contract = contract_of(vec![migration(5, "entities", "CREATE TABLE entities ()")]);
        let other_contract = contract_of(vec![migration(5, "entities", "CREATE TABLE entities (id UUID)")]);
        assert_eq!(
            other_contract.version(),
            contract.version(),
            "the fixture must isolate the digest as the only differing field"
        );
        let stamped = StampedContract {
            version: contract.version(),
            digest: other_contract.digest(),
        };

        match refuse_on_mismatch(&stamped, &contract) {
            Err(SchemaContractError::MigrationsDigestMismatch { expected, found }) => {
                assert_eq!(expected, contract.digest(), "the refusal must name the digest the binary requires");
                assert_eq!(found, other_contract.digest(), "the refusal must name the digest the tenant carries");
            }
            other => unreachable!("a digest mismatch must refuse naming both digests: {other:?}"),
        }
    }

    #[test]
    fn version_is_checked_before_digest_so_a_stamp_wrong_in_both_reports_the_version_mismatch() {
        let contract = contract_of(vec![migration(5, "entities", "CREATE TABLE entities ()")]);
        let behind = vec![migration(4, "entities", "CREATE TABLE entities ()")];
        let stamped = StampedContract {
            version: contract_of(behind.clone()).version(),
            digest: MigrationsDigest::of(&behind),
        };

        assert!(
            matches!(refuse_on_mismatch(&stamped, &contract), Err(SchemaContractError::SchemaVersionMismatch { .. })),
            "the version check must run first"
        );
    }
}
