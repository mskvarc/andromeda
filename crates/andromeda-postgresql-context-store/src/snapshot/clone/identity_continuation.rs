//! Continuing a clone's identity sequences from where its source left off.
//!
//! A clone copies every identity value verbatim through `OVERRIDING SYSTEM
//! VALUE`, and PostgreSQL never advances an identity sequence for a value it
//! was given. The target branch is built fresh by the branch migrations, so its
//! sequences would still start at 1 and the first key a write to the clone mints
//! would collide with a copied row. Each identity sequence of the target is
//! therefore set, in the clone's copy transaction, to the larger of two
//! positions: the source sequence's, so the clone never mints again a key its
//! source already handed out and freed, and the largest value copied, so a
//! source whose sequence lags its own rows cannot pass that lag on. When both
//! are absent -- an empty relation whose source never minted -- the sequence is
//! left as the migrations made it.

use crate::error::ContextStoreError;
use sqlx::{Executor, PgConnection};

/// Advance the target's sequence behind one identity column of `table` past
/// everything the clone copied into it and everything its source minted.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the sequence position cannot be read or
/// set.
pub(crate) async fn continue_identity(
    conn: &mut PgConnection,
    source_schema: &str,
    target_schema: &str,
    table: &str,
    column: &str,
) -> Result<(), ContextStoreError> {
    let sql = continuation_sql(target_schema, table, column);
    conn.execute(
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(format!("{source_schema}.{table}"))
            .bind(format!("{target_schema}.{table}"))
            .bind(column),
    )
    .await?;
    Ok(())
}

/// The statement setting the target's sequence, binding the source relation
/// as `$1`, the target relation as `$2` and the column as `$3`.
///
/// `pg_sequences.last_value` may run ahead of the last value handed out when
/// the sequence caches, which only moves the continuation further on; it is
/// null for a sequence never called, which `GREATEST` ignores.
fn continuation_sql(target_schema: &str, table: &str, column: &str) -> String {
    format!(
        "SELECT setval(pg_get_serial_sequence($2, $3)::regclass, continuation.position) \
         FROM (SELECT GREATEST(\
         (SELECT source.last_value FROM pg_sequences AS source \
         WHERE format('%I.%I', source.schemaname, source.sequencename)::regclass = pg_get_serial_sequence($1, $3)::regclass), \
         (SELECT max(\"{column}\") FROM {target_schema}.{table})\
         ) AS position) AS continuation \
         WHERE continuation.position IS NOT NULL"
    )
}

#[cfg(test)]
mod tests {
    use crate::snapshot::clone::identity_continuation::continuation_sql;

    #[test]
    fn the_target_sequence_is_set_from_the_larger_of_the_source_position_and_the_copied_maximum() {
        let sql = continuation_sql("snap_3_stg", "entities", "entity_key");
        assert!(
            sql.starts_with("SELECT setval(pg_get_serial_sequence($2, $3)::regclass, continuation.position)"),
            "{sql}"
        );
        assert!(sql.contains("GREATEST("), "{sql}");
        assert!(
            sql.contains(
                "FROM pg_sequences AS source WHERE format('%I.%I', source.schemaname, source.sequencename)::regclass = pg_get_serial_sequence($1, $3)::regclass"
            ),
            "the source position is read from the source relation's own sequence: {sql}"
        );
        assert!(
            sql.contains("(SELECT max(\"entity_key\") FROM snap_3_stg.entities)"),
            "the copied maximum is read from the target: {sql}"
        );
    }

    #[test]
    fn a_relation_with_nothing_copied_and_nothing_minted_keeps_its_fresh_sequence() {
        let sql = continuation_sql("snap_3_stg", "temporal_entities", "temporal_entity_key");
        assert!(sql.ends_with("WHERE continuation.position IS NOT NULL"), "{sql}");
    }
}
