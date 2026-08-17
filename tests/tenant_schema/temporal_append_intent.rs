//! Schema contract checks for the durable temporal append queue.

use crate::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;

/// Migration 0022 aligns the deployed queue column with every runtime
/// statement that inserts, acknowledges and retries an intent. The rename must
/// also leave the three-part idempotency constraint attached to that identity.
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
    assert_eq!(legacy_column, 0, "the pre-0022 column name must not remain");

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

    scope.finish().await.expect("cleanup failed");
}
