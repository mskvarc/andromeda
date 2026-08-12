//! Verification matrix row "Backfill freeze".
//!
//! Every relation the rebuild bulk-loaded was written inside one transaction,
//! so without the closing `VACUUM (FREEZE, ANALYZE)` of migrations 0053 and
//! 0059 each tuple would still carry a live xmin: the first anti-wraparound
//! vacuum would rewrite the whole history hypertable at an arbitrary later
//! moment, and the first reads would pay a hint-bit write storm on tuples no
//! reader had visited.
//!
//! The freeze is checked as the catalogue state it leaves behind — a young
//! frozen-xid age and computed statistics — rather than as a timing
//! measurement.

use crate::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use sqlx::{PgPool, Row};

/// The relations migrations 0053 and 0059 name in their freeze statements.
const FROZEN_RELATIONS: &[&str] = &[
    "attribute_history",
    "scope_history",
    "temporal_entities",
    "temporal_attribute_streams",
    "write_records",
    "entities",
    "current_attribute_values",
];

/// The hypertables among them.
///
/// A hypertable root holds no tuples of its own, so its statistics live on its
/// chunks and the chunk catalogue is where the analyse has to be observed.
const HYPERTABLES: &[&str] = &["attribute_history", "scope_history"];

/// The share of the anti-wraparound horizon a freshly frozen relation may have
/// consumed.
///
/// A freeze sets the relation's frozen xid to the current transaction, so its
/// age afterwards is only what the fleet has transacted since. Anything
/// approaching the horizon would mean the freeze did not happen, not that the
/// database is busy.
const HORIZON_SHARE: i64 = 10;

/// The server's configured anti-wraparound threshold.
async fn freeze_horizon(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, String>("SELECT current_setting('autovacuum_freeze_max_age')")
        .fetch_one(pool)
        .await
        .expect("the freeze horizon must be readable")
        .parse()
        .expect("the freeze horizon is a number")
}

#[tokio::test]
async fn every_rebuilt_relation_carries_a_young_frozen_xid_rather_than_a_mass_of_live_tuples() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let horizon = freeze_horizon(pool).await;

    for relation in FROZEN_RELATIONS {
        let age: i32 = sqlx::query_scalar("SELECT age(relfrozenxid) FROM pg_class WHERE oid = $1::regclass")
            .bind(relation)
            .fetch_one(pool)
            .await
            .expect("the relation must exist and report its frozen xid");
        assert!(
            i64::from(age) < horizon / HORIZON_SHARE,
            "{relation} has a frozen-xid age of {age}, which is not the state a freeze leaves (horizon {horizon})"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// A hypertable's freeze has to have reached its chunks: the root relation
/// holds no tuples, so freezing it alone would leave every history row unfrozen.
#[tokio::test]
async fn every_history_chunk_was_reached_by_the_freeze() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let horizon = freeze_horizon(pool).await;

    let chunks = sqlx::query(
        "SELECT c.hypertable_name, c.chunk_schema, c.chunk_name, \
                age(k.relfrozenxid) AS frozen_age \
         FROM timescaledb_information.chunks c \
         JOIN pg_class k ON k.oid = format('%I.%I', c.chunk_schema, c.chunk_name)::regclass \
         WHERE c.hypertable_name IN ('attribute_history', 'scope_history')",
    )
    .fetch_all(pool)
    .await
    .expect("the chunk catalogue must be readable");

    for chunk in &chunks {
        let hypertable: String = chunk.get("hypertable_name");
        let name: String = chunk.get("chunk_name");
        let age: i32 = chunk.get("frozen_age");
        assert!(
            i64::from(age) < horizon / HORIZON_SHARE,
            "chunk {name} of {hypertable} has a frozen-xid age of {age}, so the freeze did not reach it (horizon {horizon})"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// The `ANALYZE` half of the same statement is equally load-bearing: a rebuilt
/// relation with no statistics makes the first query plan against defaults.
///
/// The evidence is the cumulative statistics view rather than `pg_class`. A
/// `reltuples` estimate says what the planner believes right now, and `TRUNCATE`
/// resets it to the "never analysed" sentinel, so every suite that clears the
/// tenant would erase the record of a migration that did run. `last_analyze`
/// records that the analyse happened and survives truncation, which is exactly
/// the fact the freeze statement is being held to.
#[tokio::test]
async fn every_rebuilt_relation_carries_the_statistics_the_freeze_statement_computed() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    for relation in FROZEN_RELATIONS.iter().filter(|relation| !HYPERTABLES.contains(relation)) {
        let analysed: Option<bool> = sqlx::query_scalar(
            "SELECT last_analyze IS NOT NULL OR last_autoanalyze IS NOT NULL \
             FROM pg_stat_all_tables WHERE relid = $1::regclass",
        )
        .bind(relation)
        .fetch_optional(pool)
        .await
        .expect("the relation must exist and report its statistics activity")
        .flatten();
        assert_eq!(analysed, Some(true), "{relation} has never been analysed, so it plans against defaults");
    }

    // Only the chunks the freeze could have reached. A chunk cut by ingest
    // after the migration ran is the autovacuum daemon's to analyse on its own
    // schedule, and holding migration 0053 responsible for it would make this
    // assertion fail for a reason that has nothing to do with the backfill.
    let unanalysed_chunks: Vec<String> = sqlx::query_scalar(
        "SELECT c.chunk_name FROM timescaledb_information.chunks c \
         JOIN pg_stat_all_tables s ON s.relid = format('%I.%I', c.chunk_schema, c.chunk_name)::regclass \
         WHERE c.hypertable_name = ANY($1) \
           AND c.chunk_creation_time <= (SELECT installed_on FROM _sqlx_migrations WHERE version = 53) \
           AND s.last_analyze IS NULL AND s.last_autoanalyze IS NULL",
    )
    .bind(HYPERTABLES)
    .fetch_all(pool)
    .await
    .expect("the chunk catalogue must be readable");
    assert!(
        unanalysed_chunks.is_empty(),
        "history chunks were never analysed, so the first temporal query plans against defaults: {unanalysed_chunks:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
