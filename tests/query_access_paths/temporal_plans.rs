//! Verification-matrix row "Temporal access-path plans".
//!
//! Stage 1 of a temporal query selects `temporal_entities` by the selector's
//! `idPattern` (clause 5.2.33), compiled to a POSIX `~` on the identifier. An
//! anchored pattern range-scans `uq_temporal_entities_id`; an unanchored one
//! has no anchor to range over and must reach the trigram index instead of
//! scanning the whole catalogue.

use crate::seeded_corpus::{SEED_PREFIX, explain, type_dictionary_id};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use sqlx::PgPool;

/// Temporal Entities written per seeded catalogue.
const SEEDED_TEMPORAL_ENTITIES: i64 = 20_000;

/// Type every seeded temporal Entity carries.
const TEMPORAL_TYPE: &str = "https://example.test/query-access-paths/Instrument";

/// Write the temporal catalogue and analyze it.
///
/// Every 1000th temporal Entity is a `sensor`, every other one a `device`, so
/// a pattern naming the sensors is selective enough that an index path is the
/// cheaper plan.
async fn seed_temporal_entities(pool: &PgPool, prefix: &str) {
    let type_id = type_dictionary_id(pool, TEMPORAL_TYPE).await;
    sqlx::query(
        "INSERT INTO temporal_entities (created_at, modified_at, id, type_ids) \
         SELECT now(), now(), \
                $1::text || ':' || CASE WHEN g % 1000 = 0 THEN 'sensor' ELSE 'device' END || ':' || lpad(g::text, 6, '0'), \
                ARRAY[$2::integer] \
         FROM generate_series(1, $3::bigint) AS g",
    )
    .bind(prefix)
    .bind(type_id)
    .bind(SEEDED_TEMPORAL_ENTITIES)
    .execute(pool)
    .await
    .expect("temporal catalogue corpus must be writable");

    sqlx::query("ANALYZE temporal_entities").execute(pool).await.expect("analyze must succeed");
}

#[tokio::test]
async fn temporal_plans_an_unanchored_id_pattern_uses_the_trigram_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_temporal_entities(pool, SEED_PREFIX).await;

    // The stage 1 shape: the selector's pattern against the `e` alias the
    // temporal query selects `temporal_entities` under.
    let plan = explain(pool, "SELECT e.entity_key, e.id FROM temporal_entities AS e WHERE e.id ~ '.*sensor.*'").await;
    assert!(
        plan.contains("idx_temporal_entities_id_trgm"),
        "an unanchored temporal idPattern must use the trigram index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
