//! Temporal Evolution deletion (clause 5.6.16).
//!
//! Deleting the temporal evolution of an entity removes recorded history, the
//! streams that grouped it and the temporal entity itself. Nothing in the
//! database sequences that: the hypertables carry no foreign key back to the
//! catalogue, because a referential action on the broker's hottest write path
//! costs a lookup per inserted row, and no trigger cascades anything. The order
//! is therefore stated here, inside one transaction, and
//! [`audit_orphaned_temporal_rows`](TemporalRepository::audit_orphaned_temporal_rows)
//! is the check that the arrangement held.
//!
//! Core Entity deletion (clause 5.6.6) is a different operation and does not
//! reach this module: it leaves temporal state standing and records the
//! tombstones the specification requires instead.

use crate::{error::TemporalStoreError, repo::TemporalRepository, write_record_release::release_named};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::collections::BTreeSet;
use uuid::Uuid;

/// What one Temporal Evolution deletion removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalEvolutionDeletion {
    /// The temporal entity the deletion resolved and removed.
    pub entity_key: TemporalEntityKey,
    /// Attribute history rows removed.
    pub attribute_history_rows: u64,
    /// Scope history rows removed.
    pub scope_history_rows: u64,
    /// Attribute streams removed.
    pub streams: u64,
    /// Idempotency records released because they no longer protect an effect.
    pub released_write_records: u64,
}

/// Temporal rows whose owning row no longer exists.
///
/// Every count is expected to be zero. A non-zero count means a deletion did
/// not run its full sequence, which leaves history no query can reach and no
/// deletion can address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrphanedTemporalRows {
    /// Attribute history rows whose `stream_id` names no stream.
    pub attribute_history_rows: i64,
    /// Scope history rows whose `temporal_entity_key` names no temporal entity.
    pub scope_history_rows: i64,
    /// Streams whose `temporal_entity_key` names no temporal entity.
    pub streams: i64,
}

impl TemporalRepository {
    /// Delete the whole temporal evolution of one entity (clause 5.6.16).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NotFound`] when no temporal entity carries
    /// the identifier, [`TemporalStoreError::Database`] when a statement fails,
    /// and [`TemporalStoreError::Decode`] when the stored temporal-entity key is
    /// outside the range the database assigns.
    pub async fn delete_temporal_evolution(&self, entity_id: &Uri) -> Result<TemporalEvolutionDeletion, TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let entity_key = locate_temporal_entity(&mut tx, entity_id).await?;
        let attribute_writes = delete_returning_write_ids(&mut tx, DELETE_ATTRIBUTE_HISTORY_SQL, entity_key).await?;
        let scope_writes = delete_returning_write_ids(&mut tx, DELETE_SCOPE_HISTORY_SQL, entity_key).await?;
        let streams = sqlx::query(DELETE_ATTRIBUTE_STREAMS_SQL)
            .bind(entity_key.get())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query(DELETE_TEMPORAL_ENTITY_SQL).bind(entity_key.get()).execute(&mut *tx).await?;
        let touched_writes = attribute_writes.iter().chain(scope_writes.iter()).copied().collect::<BTreeSet<Uuid>>();
        let touched_writes = touched_writes.into_iter().collect::<Vec<Uuid>>();
        let released_write_records = release_named(&mut tx, &touched_writes).await?;
        tx.commit().await?;
        Ok(TemporalEvolutionDeletion {
            entity_key,
            attribute_history_rows: attribute_writes.len() as u64,
            scope_history_rows: scope_writes.len() as u64,
            streams,
            released_write_records,
        })
    }

    /// Report temporal rows left behind by an incomplete deletion sequence.
    ///
    /// This is the audit that stands in for the foreign keys the hypertables do
    /// not carry. An empty report — every count zero — is the expected outcome.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the audit statement fails
    /// and [`TemporalStoreError::Decode`] when a returned count does not carry
    /// the expected type.
    pub async fn audit_orphaned_temporal_rows(&self) -> Result<OrphanedTemporalRows, TemporalStoreError> {
        let row = sqlx::query(AUDIT_ORPHANED_TEMPORAL_ROWS_SQL).fetch_one(&self.pool).await?;
        Ok(OrphanedTemporalRows {
            attribute_history_rows: count(&row, "orphaned_attribute_history_rows")?,
            scope_history_rows: count(&row, "orphaned_scope_history_rows")?,
            streams: count(&row, "orphaned_streams")?,
        })
    }
}

/// Read one audit count, naming the column that failed to decode.
fn count(row: &PgRow, column: &str) -> Result<i64, TemporalStoreError> {
    row.try_get(column)
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal orphan audit count {column}: {error}")))
}

/// Resolve and lock the temporal entity one deletion addresses.
async fn locate_temporal_entity(tx: &mut Transaction<'_, Postgres>, entity_id: &Uri) -> Result<TemporalEntityKey, TemporalStoreError> {
    let located = sqlx::query(LOCK_TEMPORAL_ENTITY_SQL).bind(entity_id.as_str()).fetch_optional(&mut **tx).await?;
    let Some(row) = located else {
        return Err(TemporalStoreError::NotFound(format!("Temporal evolution of {entity_id} not found")));
    };
    let stored: i64 = row
        .try_get("entity_key")
        .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.entity_key: {error}")))?;
    TemporalEntityKey::try_from(stored).map_err(|error| TemporalStoreError::Decode(error.to_string()))
}

/// Run one deletion statement, collecting the write identity of every removed row.
async fn delete_returning_write_ids(
    tx: &mut Transaction<'_, Postgres>,
    statement: &str,
    entity_key: TemporalEntityKey,
) -> Result<Vec<Uuid>, TemporalStoreError> {
    let rows = sqlx::query(statement).bind(entity_key.get()).fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            row.try_get("write_id")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid history write_id: {error}")))
        })
        .collect()
}

const LOCK_TEMPORAL_ENTITY_SQL: &str = r#"
SELECT entity_key FROM temporal_entities WHERE id = $1 FOR UPDATE
"#;

const DELETE_ATTRIBUTE_HISTORY_SQL: &str = r#"
DELETE FROM attribute_history
WHERE stream_id IN (SELECT s.id FROM temporal_attribute_streams s WHERE s.temporal_entity_key = $1)
RETURNING write_id
"#;

const DELETE_SCOPE_HISTORY_SQL: &str = r#"
DELETE FROM scope_history WHERE temporal_entity_key = $1 RETURNING write_id
"#;

// The stream's foreign key cascades from `temporal_entities`, so this statement
// is not what keeps the catalogue consistent. It is stated anyway to keep the
// order of the sequence explicit and to report what the deletion removed.
const DELETE_ATTRIBUTE_STREAMS_SQL: &str = r#"
DELETE FROM temporal_attribute_streams WHERE temporal_entity_key = $1
"#;

const DELETE_TEMPORAL_ENTITY_SQL: &str = r#"
DELETE FROM temporal_entities WHERE entity_key = $1
"#;

const AUDIT_ORPHANED_TEMPORAL_ROWS_SQL: &str = r#"
SELECT
    (SELECT count(*) FROM attribute_history ah
        LEFT JOIN temporal_attribute_streams s ON s.id = ah.stream_id
        WHERE s.id IS NULL) AS orphaned_attribute_history_rows,
    (SELECT count(*) FROM scope_history sh
        LEFT JOIN temporal_entities te ON te.entity_key = sh.temporal_entity_key
        WHERE te.entity_key IS NULL) AS orphaned_scope_history_rows,
    (SELECT count(*) FROM temporal_attribute_streams s
        LEFT JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
        WHERE te.entity_key IS NULL) AS orphaned_streams
"#;

#[cfg(test)]
mod tests {
    use crate::temporal_entity_deletion::{
        AUDIT_ORPHANED_TEMPORAL_ROWS_SQL,
        DELETE_ATTRIBUTE_HISTORY_SQL,
        DELETE_ATTRIBUTE_STREAMS_SQL,
        DELETE_SCOPE_HISTORY_SQL,
        DELETE_TEMPORAL_ENTITY_SQL,
        LOCK_TEMPORAL_ENTITY_SQL,
    };

    #[test]
    fn the_addressed_temporal_entity_is_locked_before_anything_is_removed() {
        assert!(LOCK_TEMPORAL_ENTITY_SQL.contains("FROM temporal_entities WHERE id = $1"));
        assert!(LOCK_TEMPORAL_ENTITY_SQL.contains("FOR UPDATE"));
    }

    #[test]
    fn history_removal_reports_the_write_identity_of_every_removed_row() {
        assert!(DELETE_ATTRIBUTE_HISTORY_SQL.contains("RETURNING write_id"));
        assert!(DELETE_SCOPE_HISTORY_SQL.contains("RETURNING write_id"));
    }

    #[test]
    fn the_sequence_reaches_every_relation_the_evolution_occupies() {
        assert!(DELETE_ATTRIBUTE_HISTORY_SQL.contains("DELETE FROM attribute_history"));
        assert!(DELETE_SCOPE_HISTORY_SQL.contains("DELETE FROM scope_history"));
        assert!(DELETE_ATTRIBUTE_STREAMS_SQL.contains("DELETE FROM temporal_attribute_streams"));
        assert!(DELETE_TEMPORAL_ENTITY_SQL.contains("DELETE FROM temporal_entities"));
    }

    #[test]
    fn the_audit_covers_both_references_no_foreign_key_enforces() {
        assert!(AUDIT_ORPHANED_TEMPORAL_ROWS_SQL.contains("orphaned_attribute_history_rows"));
        assert!(AUDIT_ORPHANED_TEMPORAL_ROWS_SQL.contains("orphaned_scope_history_rows"));
        assert!(AUDIT_ORPHANED_TEMPORAL_ROWS_SQL.contains("orphaned_streams"));
    }

    #[test]
    fn no_statement_records_a_tombstone_for_a_temporal_evolution_deletion() {
        for statement in [
            DELETE_ATTRIBUTE_HISTORY_SQL,
            DELETE_SCOPE_HISTORY_SQL,
            DELETE_ATTRIBUTE_STREAMS_SQL,
            DELETE_TEMPORAL_ENTITY_SQL,
        ] {
            assert!(!statement.contains("INSERT"));
            assert!(!statement.contains("deleted_at"));
        }
    }
}
