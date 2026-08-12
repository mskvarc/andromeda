//! When one write's idempotency record may be removed.
//!
//! A `write_records` row persists the instant a replayed write must reuse, so
//! that the replay collides with the rows it would otherwise duplicate on
//! `UNIQUE (recorded_at, write_id, write_ordinal)`. It may therefore only go
//! once nothing it protects is left: no attribute history row, no scope history
//! row, and no queued append intent that could still be delivered under its
//! identity. Both callers that release records — Temporal Evolution deletion
//! (clause 5.6.16) and the operator retention sweep — apply that one rule, so
//! it is written once here.

use crate::error::TemporalStoreError;
use chrono::{DateTime, Utc};
use const_format::concatcp;
use sqlx::{Postgres, Transaction, postgres::PgPool};
use uuid::Uuid;

/// Release the named idempotency records that no longer protect an effect.
///
/// Runs inside the caller's transaction so that the records go exactly when the
/// effects they protected do.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn release_named(tx: &mut Transaction<'_, Postgres>, write_ids: &[Uuid]) -> Result<u64, TemporalStoreError> {
    Ok(sqlx::query(RELEASE_NAMED_WRITE_RECORDS_SQL)
        .bind(write_ids)
        .execute(&mut **tx)
        .await?
        .rows_affected())
}

/// Release every unprotected idempotency record recorded before a cutoff.
///
/// The cutoff is what keeps the sweep off records of writes that are still in
/// flight: a record younger than the horizon is left alone whether or not it
/// currently protects anything.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn release_behind_horizon(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, TemporalStoreError> {
    Ok(sqlx::query(RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL)
        .bind(cutoff)
        .execute(pool)
        .await?
        .rows_affected())
}

/// The three effects a write record outlives nothing without.
const UNPROTECTED_BY_ANY_EFFECT: &str = r#"
  AND NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.write_id = wr.write_id)
  AND NOT EXISTS (SELECT 1 FROM scope_history sh WHERE sh.write_id = wr.write_id)
  AND NOT EXISTS (SELECT 1 FROM temporal_append_intents ti WHERE ti.write_id = wr.write_id)
"#;

const RELEASE_NAMED_WRITE_RECORDS_SQL: &str = concatcp!("DELETE FROM write_records wr WHERE wr.write_id = ANY($1)", UNPROTECTED_BY_ANY_EFFECT);

const RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL: &str = concatcp!("DELETE FROM write_records wr WHERE wr.recorded_at < $1", UNPROTECTED_BY_ANY_EFFECT);

#[cfg(test)]
mod tests {
    use crate::write_record_release::{RELEASE_NAMED_WRITE_RECORDS_SQL, RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL};

    #[test]
    fn both_releases_apply_the_same_three_protections() {
        for statement in [RELEASE_NAMED_WRITE_RECORDS_SQL, RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL] {
            assert!(statement.contains("FROM attribute_history ah WHERE ah.write_id = wr.write_id"));
            assert!(statement.contains("FROM scope_history sh WHERE sh.write_id = wr.write_id"));
            assert!(statement.contains("FROM temporal_append_intents ti WHERE ti.write_id = wr.write_id"));
            assert_eq!(statement.matches("NOT EXISTS").count(), 3);
        }
    }

    #[test]
    fn the_named_release_is_confined_to_the_records_it_was_given() {
        assert!(RELEASE_NAMED_WRITE_RECORDS_SQL.contains("wr.write_id = ANY($1)"));
        assert!(!RELEASE_NAMED_WRITE_RECORDS_SQL.contains("recorded_at"));
    }

    #[test]
    fn the_horizon_release_is_confined_to_records_behind_the_cutoff() {
        assert!(RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL.contains("wr.recorded_at < $1"));
        assert!(!RELEASE_WRITE_RECORDS_BEHIND_HORIZON_SQL.contains("ANY("));
    }
}
