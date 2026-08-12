//! The write's own idempotency bookkeeping record.
//!
//! A `write_records` row is persisted once per write, inside the transaction
//! that produces the write's durable effects. It persists `recorded_at`
//! because the hypertable rows the write may still produce deduplicate on a
//! key containing the partition column: a replay that stamped a fresh instant
//! would land beside the rows it is meant to collide with and duplicate them,
//! so a replay reads the stored instant back and reuses it. `accepted_at` is
//! when the record itself was written, which a replay does not change.
//!
//! The upsert is pure bookkeeping, not a gate. Deduplication of the write's
//! actual effects happens at the row level, on the `uq_attribute_history_write`
//! and `uq_scope_history_write` constraints those rows insert under with `ON
//! CONFLICT ... DO NOTHING`: a caller persists this record and then always
//! appends every entry, never skipping an intent because its record was
//! already present.

use andromeda_context_commit::write_record::WriteRecord;
use chrono::Utc;
use sqlx::{Postgres, Transaction};

/// Persist the bookkeeping record of one write inside the caller's transaction.
///
/// Idempotent: a write replayed under its original identity re-runs this
/// exactly as before and changes nothing.
///
/// # Errors
///
/// Returns the underlying [`sqlx::Error`] when the statement fails.
pub(crate) async fn persist_write_record(tx: &mut Transaction<'_, Postgres>, record: &WriteRecord) -> Result<(), sqlx::Error> {
    sqlx::query(PERSIST_WRITE_RECORD_SQL)
        .bind(record.write_id().as_uuid())
        .bind(record.recorded_at())
        .bind(Utc::now())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

const PERSIST_WRITE_RECORD_SQL: &str = "INSERT INTO write_records (write_id, recorded_at, accepted_at) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING";

#[cfg(test)]
mod tests {
    use crate::write_record_persist::PERSIST_WRITE_RECORD_SQL;

    #[test]
    fn a_repeated_write_is_recognised_rather_than_recorded_twice() {
        assert!(PERSIST_WRITE_RECORD_SQL.contains("ON CONFLICT DO NOTHING"));
    }

    #[test]
    fn the_record_persists_the_instant_a_replay_has_to_reuse() {
        assert!(PERSIST_WRITE_RECORD_SQL.contains("(write_id, recorded_at, accepted_at)"));
    }

    #[test]
    fn the_record_replaces_the_legacy_receipt_relation() {
        assert!(PERSIST_WRITE_RECORD_SQL.contains("INTO write_records"));
        assert!(!PERSIST_WRITE_RECORD_SQL.contains("temporal_history_sink_receipts"));
    }
}
