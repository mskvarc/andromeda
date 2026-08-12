//! The `schema_metadata` singleton: stamping it, reading it, and verifying it.

use crate::schema_contract::{
    contract::SchemaContract,
    error::SchemaContractError,
    migrations_digest::MigrationsDigest,
    schema_version::SchemaVersion,
    server_requirements::verify_server_requirements,
};
use sqlx::{PgPool, Row};

/// Stamp a freshly migrated tenant and prove the stamp took.
///
/// The migrator has just brought this database to the binary's migration set,
/// so writing the stamp is the only moment the two are known to agree. Reading
/// it back is not ceremony: it is what turns a database whose `schema_metadata`
/// row cannot be written or does not survive into a refusal here rather than
/// into a failure on the first query that depends on the new shape.
pub async fn ensure_schema_contract(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    stamp(pool, contract).await?;
    verify_schema_contract(pool, contract).await
}

/// Refuse a tenant whose schema is not the one this binary was built against.
pub async fn verify_schema_contract(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    verify_server_requirements(pool).await?;

    let Some(stamped) = read_stamp(pool).await? else {
        return Err(SchemaContractError::Unstamped {
            expected_version: contract.version(),
            expected_digest: contract.digest(),
        });
    };
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

/// Write the singleton, replacing whatever it held.
async fn stamp(pool: &PgPool, contract: &SchemaContract) -> Result<(), SchemaContractError> {
    sqlx::query(
        r#"
        INSERT INTO schema_metadata (applied_at, schema_version, singleton, migrations_sha256)
        VALUES (NOW(), $1, TRUE, $2)
        ON CONFLICT (singleton) DO UPDATE
        SET applied_at = EXCLUDED.applied_at,
            schema_version = EXCLUDED.schema_version,
            migrations_sha256 = EXCLUDED.migrations_sha256
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
