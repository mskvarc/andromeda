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
use andromeda_postgresql_schema::queue::WriteRecords;
use chrono::{DateTime, Utc};
use sea_query::{Expr, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// Persist the bookkeeping record of one write inside the caller's transaction.
///
/// Idempotent: a write replayed under its original identity re-runs this
/// exactly as before and changes nothing.
///
/// # Errors
///
/// Returns the underlying [`sqlx::Error`] when the statement fails.
pub(crate) async fn persist_write_record(tx: &mut Transaction<'_, Postgres>, record: &WriteRecord) -> Result<(), sqlx::Error> {
    let statement = persist_write_record_statement(record.write_id().as_uuid(), record.recorded_at(), Utc::now())
        .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(&mut **tx).await?;
    Ok(())
}

/// The idempotent insert of one write's bookkeeping record.
///
/// The conflict is on the `write_id` primary key with no update, so a replay of
/// an already-recorded write changes nothing.
fn persist_write_record_statement(write_id: Uuid, recorded_at: DateTime<Utc>, accepted_at: DateTime<Utc>) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(WriteRecords::Table)
        .columns([WriteRecords::WriteId, WriteRecords::RecordedAt, WriteRecords::AcceptedAt])
        .values([Expr::val(write_id), recorded_at.into(), accepted_at.into()])?
        .on_conflict(OnConflict::new().do_nothing().to_owned());
    Ok(statement.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::write_record_persist::persist_write_record_statement;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn rendered() -> String {
        persist_write_record_statement(Uuid::nil(), Utc::now(), Utc::now())
            .expect("arity")
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_repeated_write_is_recognised_rather_than_recorded_twice() {
        let sql = rendered();
        assert!(sql.contains("ON CONFLICT") && sql.contains("DO NOTHING"), "{sql}");
    }

    #[test]
    fn the_record_persists_the_instant_a_replay_has_to_reuse() {
        let sql = rendered();
        assert!(
            sql.contains("\"write_id\"") && sql.contains("\"recorded_at\"") && sql.contains("\"accepted_at\""),
            "{sql}"
        );
    }

    #[test]
    fn the_record_replaces_the_legacy_receipt_relation() {
        let sql = rendered();
        assert!(sql.contains("INTO \"write_records\""), "{sql}");
        assert!(!sql.contains("temporal_history_sink_receipts"), "{sql}");
    }
}
