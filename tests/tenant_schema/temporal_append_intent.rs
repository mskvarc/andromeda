//! Schema contract checks for the durable temporal append queue.

use crate::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;

/// The queue column must carry the same `write_id` name every runtime
/// statement that inserts, acknowledges and retries an intent uses, and the
/// three-part idempotency constraint must be attached to that identity. The
/// column was once created as `intent_id` and renamed by a later migration;
/// with the schema edited in place that name must not exist at all. The queue
/// has no dead-letter state -- an intent is the durable record of accepted
/// history and stays claimable however often it fails -- so no terminal
/// column is expected here either; the failure summary it keeps is bounded by
/// the `last_error` length check.
#[tokio::test]
async fn temporal_append_intents_exposes_write_identity_and_preserves_its_deduplication_key() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let write_id: (String, String) = sqlx::query_as(
        "SELECT data_type, is_nullable FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'temporal_append_intents' AND column_name = 'write_id'",
    )
    .fetch_one(pool)
    .await
    .expect("the temporal append queue must expose write_id");
    assert_eq!(write_id, ("uuid".to_owned(), "NO".to_owned()));

    let legacy_column: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'temporal_append_intents' AND column_name = 'intent_id'",
    )
    .fetch_one(pool)
    .await
    .expect("the temporal append queue columns must be readable");
    assert_eq!(legacy_column, 0, "the superseded intent_id column name must not exist");

    let constraint_columns: Vec<String> = sqlx::query_scalar(
        "SELECT array_agg(attribute.attname ORDER BY key.ordinality)::text[] \
         FROM pg_constraint AS constraint_definition \
         CROSS JOIN LATERAL unnest(constraint_definition.conkey) WITH ORDINALITY AS key(attnum, ordinality) \
         JOIN pg_attribute AS attribute ON attribute.attrelid = constraint_definition.conrelid AND attribute.attnum = key.attnum \
         WHERE constraint_definition.conrelid = 'temporal_append_intents'::regclass \
           AND constraint_definition.conname = 'uq_temporal_append_intents_write' \
         GROUP BY constraint_definition.oid",
    )
    .fetch_one(pool)
    .await
    .expect("the temporal append intent uniqueness constraint must exist");
    assert_eq!(constraint_columns, ["write_id", "entity_id", "base"]);

    let bounded_error: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_constraint \
         WHERE conrelid = 'temporal_append_intents'::regclass AND conname = 'chk_temporal_append_intents_last_error_length'",
    )
    .fetch_one(pool)
    .await
    .expect("the queue constraints must be readable");
    assert_eq!(bounded_error, 1, "the failure summary the queue keeps must be length-bounded");

    let terminal_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'temporal_append_intents' \
           AND column_name IN ('dead', 'exhausted_at', 'dead_lettered_at')",
    )
    .fetch_one(pool)
    .await
    .expect("the temporal append queue columns must be readable");
    assert_eq!(
        terminal_columns, 0,
        "an intent has no dead-letter state: it stays claimable however often it fails"
    );

    scope.finish().await.expect("cleanup failed");
}
