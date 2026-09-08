//! Simulating an out-of-band tenant schema drop and recreate.
//!
//! An operator can drop and recreate a tenant's schema directly against the
//! database while the broker keeps the tenant's runtime -- and its in-memory
//! `NameCache` -- alive. A recreate resets the dictionary's `IDENTITY`
//! sequences, so a cached name-to-key mapping now points at a dictionary row
//! that no longer exists. This helper reproduces that exact signature on a
//! tenant's own database pool: it resets the dictionary and re-stamps the
//! schema, which is what the broker's schema-epoch check detects.
//!
//! The global reset SQL lives here in the harness rather than inline in an
//! integration test, so the test-infrastructure boundary that forbids ad-hoc
//! `TRUNCATE` in `tests/` stays intact.

use crate::database_reset::DatabaseResetError;
use sqlx::PgPool;

/// Reset the attribute-name dictionary and advance the schema stamp on a
/// tenant's database, the signature an out-of-band schema recreate leaves.
///
/// `TRUNCATE ... RESTART IDENTITY CASCADE` empties the dictionary and restarts
/// its `IDENTITY`, cascading to the projection and temporal rows that reference
/// it, so a re-minted name takes a fresh key; advancing `schema_metadata`'s
/// `applied_at` to the current instant is the stamp change the broker reads as
/// a recreate.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when a statement fails.
pub async fn simulate_out_of_band_recreate(pool: &PgPool) -> Result<(), DatabaseResetError> {
    sqlx::query("TRUNCATE attribute_names RESTART IDENTITY CASCADE")
        .execute(pool)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    sqlx::query("UPDATE schema_metadata SET applied_at = clock_timestamp()")
        .execute(pool)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    Ok(())
}
