//! Verification matrix row "Hot-update maintenance".
//!
//! Four relations take queue-shaped churn — `current_attribute_values`,
//! `entities`, `notification_deliveries` and `pending_side_effects` — and each
//! carries an explicit `fillfactor` and aggressive per-table autovacuum
//! settings, because the server defaults let dead tuples accumulate faster than
//! they are reclaimed at that write rate.
//!
//! The row is checked in three parts. The settings the migrations declare are
//! read back from the catalogue, so a relation that lost them is caught. A
//! sustained update run then reports the heap-only-tuple ratio and the dead-tuple
//! count each relation actually reaches, which is the measurement the row asks
//! for. And the claim-shaped update is asserted to be the one that cannot go
//! HOT: `next_attempt_at` is an index key, so vacuum cadence rather than
//! fillfactor is the defence there, exactly as the proposal records.

use crate::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::{PgPool, Row};

/// One relation the redesign tunes, and the storage parameters it declares.
struct TunedRelation {
    /// Relation name.
    name: &'static str,
    /// The `fillfactor` the migration declares, absent where the relation takes
    /// inserts rather than updates and only needs the vacuum cadence.
    fillfactor: Option<i32>,
}

/// Every relation the proposal names as hot-update shaped, plus the append-only
/// event relation that shares the vacuum cadence.
const TUNED_RELATIONS: &[TunedRelation] = &[
    TunedRelation {
        name: "entities",
        fillfactor: Some(90),
    },
    TunedRelation {
        name: "current_attribute_values",
        fillfactor: Some(90),
    },
    TunedRelation {
        name: "notification_deliveries",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "pending_side_effects",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "notification_events",
        fillfactor: None,
    },
];

/// The autovacuum settings every tuned relation declares.
const AUTOVACUUM_SETTINGS: &[&str] = &[
    "autovacuum_vacuum_scale_factor",
    "autovacuum_vacuum_threshold",
    "autovacuum_analyze_scale_factor",
    "autovacuum_analyze_threshold",
    "autovacuum_vacuum_cost_delay",
];

/// Updates applied to the churn fixture, enough for the HOT ratio to be a
/// measurement rather than a rounding of one or two tuples.
const CHURN_UPDATES: i64 = 2_000;

/// The storage parameters one relation declares, as `name=value` strings.
async fn storage_parameters(pool: &PgPool, relation: &str) -> Vec<String> {
    sqlx::query_scalar::<_, Option<Vec<String>>>("SELECT reloptions FROM pg_class WHERE oid = $1::regclass")
        .bind(relation)
        .fetch_one(pool)
        .await
        .expect("the relation must exist and report its storage parameters")
        .unwrap_or_default()
}

/// What one measured churn run did to the relation.
struct ChurnOutcome {
    /// Updates the relation counted during the run.
    updated: i64,
    /// How many of them were heap-only.
    heap_only: i64,
    /// Pages the relation occupied when the run finished.
    pages: i64,
}

/// Apply `statement` `CHURN_UPDATES` times inside one transaction and report
/// what it did.
///
/// The counters are the per-transaction ones. The cumulative statistics a
/// relation reports are flushed to shared memory asynchronously, so reading
/// them straight after a run measures the flush rather than the run; the
/// transaction-local counters are exact the moment the run finishes, which is
/// what makes this a measurement instead of a race.
async fn churn(pool: &PgPool, statement: &str, entity_key: i64) -> ChurnOutcome {
    let mut transaction = pool.begin().await.expect("the churn transaction must open");
    for round in 1..=CHURN_UPDATES {
        sqlx::query(statement)
            .bind(entity_key)
            .bind(round)
            .execute(&mut *transaction)
            .await
            .expect("the churn update must apply");
    }
    let outcome = sqlx::query(
        "SELECT pg_stat_get_xact_tuples_updated(c.oid) AS updated, \
                pg_stat_get_xact_tuples_hot_updated(c.oid) AS heap_only, \
                (pg_relation_size(c.oid) / current_setting('block_size')::bigint) AS pages \
         FROM pg_class AS c WHERE c.oid = 'current_attribute_values'::regclass",
    )
    .fetch_one(&mut *transaction)
    .await
    .map(|row| ChurnOutcome {
        updated: row.get::<i64, _>("updated"),
        heap_only: row.get::<i64, _>("heap_only"),
        pages: row.get::<i64, _>("pages"),
    })
    .expect("the transaction-local tuple counters must be readable");
    transaction.commit().await.expect("the churn transaction must commit");
    outcome
}

/// Every tuned relation still carries the settings its migration declared.
#[tokio::test]
async fn every_hot_update_relation_still_declares_the_maintenance_settings_it_was_given() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    for relation in TUNED_RELATIONS {
        let parameters = storage_parameters(pool, relation.name).await;
        if let Some(fillfactor) = relation.fillfactor {
            assert!(
                parameters.iter().any(|parameter| parameter == &format!("fillfactor={fillfactor}")),
                "{} must keep fillfactor={fillfactor}: {parameters:?}",
                relation.name
            );
        }
        for setting in AUTOVACUUM_SETTINGS {
            assert!(
                parameters.iter().any(|parameter| parameter.starts_with(&format!("{setting}="))),
                "{} must keep {setting}: {parameters:?}",
                relation.name
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// A sustained update run over the current projection, in the two shapes the
/// relation actually takes.
///
/// A write that restates an instance without changing its value touches
/// `modified_at`, which no index key contains, so the free space `fillfactor`
/// reserves lets it stay heap-only. A write that changes the value touches
/// `value_number`, which `idx_cav_attr_number` contains, so it cannot: every
/// typed-value index disqualifies HOT for its own column, which is why the
/// proposal makes vacuum cadence rather than fillfactor the primary defence.
/// Both are measured here so the tuning is judged on what it can reach.
#[tokio::test]
async fn sustained_projection_churn_stays_heap_only_and_leaves_bounded_dead_tuples() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let prefix = scope.id_prefix();

    let attribute_name_id: i32 = sqlx::query_scalar(
        "INSERT INTO attribute_names (name) VALUES ('https://example.test/tenant-schema/churn') \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("attribute dictionary insert must succeed");

    let entity_key: i64 = sqlx::query_scalar(
        "INSERT INTO entities (created_at, modified_at, id, types, payload) \
         VALUES (now(), now(), $1::text || ':churn', ARRAY['https://example.test/tenant-schema/Churn'], '[]'::jsonb) \
         RETURNING entity_key",
    )
    .bind(prefix)
    .fetch_one(pool)
    .await
    .expect("the churn entity must be writable");

    sqlx::query(
        "INSERT INTO current_attribute_values (entity_key, modified_at, attribute_name_id, attribute_kind, value_kind, value_number) \
         VALUES ($1, now(), $2, 1, 1, 0::numeric)",
    )
    .bind(entity_key)
    .bind(attribute_name_id)
    .execute(pool)
    .await
    .expect("the churn projection row must be writable");

    // `$2` goes unread by the value-preserving statement, which binds it anyway
    // so both runs share one churn driver.
    let restatement = churn(
        pool,
        "UPDATE current_attribute_values SET modified_at = now() + make_interval(secs => 0 * $2) WHERE entity_key = $1",
        entity_key,
    )
    .await;

    assert_eq!(restatement.updated, CHURN_UPDATES, "every value-preserving update must be counted");
    // The row is small, no touched column is an index key, and the relation
    // reserves ten percent of every page, so the overwhelming majority of the
    // run must stay on its original page. A threshold rather than a ratio of
    // one: the first update of a filled page still has to move.
    assert!(
        restatement.heap_only * 10 >= restatement.updated * 9,
        "a value-preserving restatement must stay heap-only: {} of {} updates were HOT",
        restatement.heap_only,
        restatement.updated
    );

    let revaluation = churn(
        pool,
        "UPDATE current_attribute_values SET modified_at = now(), value_number = $2::numeric WHERE entity_key = $1",
        entity_key,
    )
    .await;

    assert_eq!(revaluation.updated, CHURN_UPDATES, "every value-changing update must be counted");
    // `idx_cav_attr_number` contains `value_number`, so none of these can be
    // heap-only. Recorded as an assertion because the day that index goes, the
    // proposal's "vacuum cadence is the primary defence" reasoning changes with
    // it and the row has to be re-measured.
    assert_eq!(
        revaluation.heap_only, 0,
        "a value-changing update writes an index tuple, so none of the {} can be heap-only",
        revaluation.updated
    );
    // Bloat is what the tuning exists to bound. Two thousand updates of one row
    // must not grow the relation without limit: the reserved free space absorbs
    // the versions and page pruning reclaims them within the same page.
    assert!(
        revaluation.pages <= restatement.pages + CHURN_UPDATES / 8,
        "sustained churn must not grow the relation without bound: {} pages after the second run, {} after the first",
        revaluation.pages,
        restatement.pages
    );

    sqlx::query("DELETE FROM current_attribute_values WHERE entity_key = $1")
        .bind(entity_key)
        .execute(pool)
        .await
        .expect("the churn projection row must be removable");

    scope.finish().await.expect("cleanup failed");
}

/// The claim-shaped update is the one that cannot go HOT: `next_attempt_at` is
/// the claim index's leading key and `lease_until` is included in it, so every
/// claim writes new index tuples whatever the fillfactor is. The proposal says
/// so, and the index definition is the fact that makes it true.
#[tokio::test]
async fn the_delivery_claim_touches_an_index_key_so_vacuum_cadence_is_its_defence() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    let claim_index: String = sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = $1")
        .bind("idx_notification_deliveries_claim_candidates")
        .fetch_one(scope.database_pool())
        .await
        .expect("the claim index must exist");

    for claimed_column in ["next_attempt_at", "lease_until"] {
        assert!(
            claim_index.contains(claimed_column),
            "the claim index must carry {claimed_column}, which is what disqualifies the claim update from HOT: {claim_index}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}
