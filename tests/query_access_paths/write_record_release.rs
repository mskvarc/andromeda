//! Write-record release plans: releasing a write record seeks one index entry
//! per history chunk.
//!
//! A `write_records` row may be released only once no attribute history row,
//! no scope history row and no queued append intent references its write. Both
//! history relations are hypertables whose only index carrying `write_id` is
//! `uq_*_write (recorded_at, write_id, write_ordinal)`, so the release
//! correlates each history probe on the write's instant as well as its
//! identity. That is what lets every Temporal Evolution deletion (clause
//! 5.6.16) and every retention sweep seek one index entry per chunk instead of
//! scanning every chunk the history spans.

use crate::seeded_corpus::{PlannerPaths, SEED_PREFIX, dictionary_key, explain_rolled_back, type_dictionary_key};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use sqlx::PgPool;
use uuid::Uuid;

/// Weekly chunks the seeded history spans; the hypertables chunk on seven days
/// of `recorded_at`.
const SEEDED_WEEKS: i64 = 6;

/// Writes recorded per seeded week.
const WRITES_PER_WEEK: i64 = 2_000;

/// Writes whose release is planned.
const RELEASED_WRITES: i64 = 3;

/// Type of the temporal Entity the seeded history belongs to.
const HISTORIC_TYPE: &str = "https://example.test/query-access-paths/Historic";

/// Expanded IRI of the attribute the seeded history records.
const RELEASE_ATTRIBUTE: &str = "https://example.test/query-access-paths/release-reading";

/// Write one attribute history row and one scope history row per write, spread
/// over [`SEEDED_WEEKS`] distinct weekly chunks, with the `write_records` row
/// each write owns, and analyze every relation the release reads.
///
/// Every history row carries the `recorded_at` of its write's record, which is
/// the invariant the committer keeps and the release correlation relies on.
async fn seed_released_history(pool: &PgPool, prefix: &str) {
    let type_key = type_dictionary_key(pool, HISTORIC_TYPE).await;
    let entity_key: i64 = sqlx::query_scalar(
        "INSERT INTO temporal_entities (created_at, modified_at, id, entity_type_name_keys) \
         VALUES (now(), now(), $1::text || ':release', ARRAY[$2::integer]) RETURNING temporal_entity_key",
    )
    .bind(prefix)
    .bind(type_key)
    .fetch_one(pool)
    .await
    .expect("temporal catalogue insert must succeed");

    let attribute_name_key = dictionary_key(pool, RELEASE_ATTRIBUTE).await;
    let stream_key: i64 =
        sqlx::query_scalar("INSERT INTO temporal_attribute_streams (temporal_entity_key, attribute_name_key) VALUES ($1, $2) RETURNING stream_key")
            .bind(entity_key)
            .bind(attribute_name_key)
            .fetch_one(pool)
            .await
            .expect("stream insert must succeed");

    sqlx::query(
        "INSERT INTO write_records (recorded_at, accepted_at, write_id) \
         SELECT TIMESTAMPTZ '2026-01-05 00:00:00Z' + (g % $1) * INTERVAL '7 days' + (g / $1) * INTERVAL '1 minute', \
                now(), gen_random_uuid() \
         FROM generate_series(1, $1 * $2) AS g",
    )
    .bind(SEEDED_WEEKS)
    .bind(WRITES_PER_WEEK)
    .execute(pool)
    .await
    .expect("write record corpus must be writable");

    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, \
          observed_at, generated_instance_id, value_number) \
         SELECT w.recorded_at, w.write_id, $1, 0, 1, 1, w.recorded_at, gen_random_uuid(), 1 \
         FROM write_records AS w",
    )
    .bind(stream_key)
    .execute(pool)
    .await
    .expect("attribute history corpus must be writable");

    sqlx::query(
        "INSERT INTO scope_history \
         (recorded_at, temporal_entity_key, write_id, write_ordinal, value_kind, observed_at, generated_instance_id, scope_path, verbatim_scope_path) \
         SELECT w.recorded_at, $1, w.write_id, 0, 1, w.recorded_at, gen_random_uuid(), 'seed.release'::ltree, '/seed/release' \
         FROM write_records AS w",
    )
    .bind(entity_key)
    .execute(pool)
    .await
    .expect("scope history corpus must be writable");

    for statement in [
        "ANALYZE write_records",
        "ANALYZE attribute_history",
        "ANALYZE scope_history",
        "ANALYZE temporal_append_intents",
    ] {
        sqlx::query(statement).execute(pool).await.expect("analyze must succeed");
    }
}

/// The number of distinct weekly chunks one history hypertable was split into.
async fn chunk_count(pool: &PgPool, hypertable: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM timescaledb_information.chunks WHERE hypertable_name = $1")
        .bind(hypertable)
        .fetch_one(pool)
        .await
        .expect("the chunk catalogue must be readable")
}

#[tokio::test]
async fn write_record_release_seeks_the_history_write_index_instead_of_scanning_every_chunk() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed_released_history(pool, SEED_PREFIX).await;
    for hypertable in ["attribute_history", "scope_history"] {
        assert!(
            chunk_count(pool, hypertable).await >= SEEDED_WEEKS,
            "the seeded {hypertable} must span one chunk per week for the plan to mean anything"
        );
    }

    let released: Vec<Uuid> = sqlx::query_scalar("SELECT write_id FROM write_records ORDER BY write_id LIMIT $1")
        .bind(RELEASED_WRITES)
        .fetch_all(pool)
        .await
        .expect("the released writes must be readable");
    let released = released.iter().map(|write_id| format!("'{write_id}'")).collect::<Vec<String>>().join(", ");

    // The statement the release issues for a clause 5.6.16 deletion, with its
    // bound write identities inlined so the plan reads them as constants.
    let statement = format!(
        "DELETE FROM write_records \
         WHERE NOT EXISTS (SELECT 1 FROM attribute_history AS ah \
                           WHERE ah.recorded_at = write_records.recorded_at AND ah.write_id = write_records.write_id) \
           AND NOT EXISTS (SELECT 1 FROM scope_history AS sh \
                           WHERE sh.recorded_at = write_records.recorded_at AND sh.write_id = write_records.write_id) \
           AND NOT EXISTS (SELECT 1 FROM temporal_append_intents AS ti WHERE ti.write_id = write_records.write_id) \
           AND write_records.write_id = ANY (ARRAY[{released}]::uuid[])"
    );
    // Every seeded record is protected, so the delete removes nothing; the
    // rollback keeps that true whatever the plan does.
    let plan = explain_rolled_back(pool, PlannerPaths::Unrestricted, &statement).await;
    for index in ["uq_attribute_history_write_identity", "uq_scope_history_write_identity"] {
        assert!(plan.contains(index), "the release must seek {index} in each chunk: {plan}");
    }
    assert!(!plan.contains("Seq Scan on _hyper_"), "the release must not scan a history chunk: {plan}");

    scope.finish().await.expect("cleanup failed");
}
