//! Verification matrix row "Hot-update maintenance".
//!
//! Four relations take queue-shaped churn — `current_attribute_values`,
//! `entity_documents`, `notification_deliveries` and `pending_side_effects` —
//! and each carries an explicit `fillfactor` and aggressive per-table autovacuum
//! settings, because the server defaults let dead tuples accumulate faster than
//! they are reclaimed at that write rate. `entities` and `entity_search` share
//! the vacuum cadence: the former is updated only for an expiry change, the
//! latter only when a projected search column moves.
//!
//! The row is checked in four parts. The settings the migrations declare are
//! read back from the catalogue, so a relation that lost them is caught. A
//! sustained update run then reports the heap-only-tuple ratio and the dead-tuple
//! count each relation actually reaches, which is the measurement the row asks
//! for. The split of the entity into a document row that every write touches
//! and a search row that only a projection change touches is verified through
//! the API: a value-only write leaves the search row's update counter where it
//! was, a type change advances it. And the two queue claims are told apart: the
//! delivery claim writes the inputs of its claimable-expression index, so vacuum
//! cadence rather than fillfactor is the defence there, exactly as the proposal
//! records; the side-effect claim writes no indexed column, so its fillfactor
//! of 50 must keep it heap-only.

use crate::SUITE;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tokio::time::sleep;

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
        fillfactor: None,
    },
    TunedRelation {
        name: "entity_documents",
        fillfactor: Some(70),
    },
    TunedRelation {
        name: "entity_search",
        fillfactor: None,
    },
    TunedRelation {
        name: "current_attribute_values",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "notification_deliveries",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "pending_side_effects",
        fillfactor: Some(50),
    },
    TunedRelation {
        name: "notification_events",
        fillfactor: None,
    },
    TunedRelation {
        name: "subscription_delivery_state",
        fillfactor: Some(50),
    },
    TunedRelation {
        name: "temporal_append_intents",
        fillfactor: Some(70),
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

/// The pages `relation` occupies right now.
async fn relation_pages(pool: &PgPool, relation: &str) -> i64 {
    sqlx::query_scalar("SELECT (pg_relation_size($1::regclass) / current_setting('block_size')::bigint)")
        .bind(relation)
        .fetch_one(pool)
        .await
        .expect("the relation must report its size")
}

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
/// what it did to `relation`.
///
/// The counters are the per-transaction ones. The cumulative statistics a
/// relation reports are flushed to shared memory asynchronously, so reading
/// them straight after a run measures the flush rather than the run; the
/// transaction-local counters are exact the moment the run finishes, which is
/// what makes this a measurement instead of a race.
async fn churn(pool: &PgPool, relation: &str, statement: &str, entity_key: i64) -> ChurnOutcome {
    let mut transaction = pool.begin().await.expect("the churn transaction must open");
    for round in 1..=CHURN_UPDATES {
        sqlx::query(sqlx::AssertSqlSafe(statement))
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
         FROM pg_class AS c WHERE c.oid = $1::regclass",
    )
    .bind(relation)
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
/// relation can take.
///
/// Through the broker a write that restates an instance without changing its
/// value touches no row at all: the upsert's `IS DISTINCT FROM` guard suppresses
/// it. The first run here is therefore a page-space probe rather than a
/// production shape: it rewrites a column no index key contains
/// (`unit_code_id`), which is the only update the reserved `fillfactor` space
/// can keep heap-only, and it measures that the reservation does so. A write
/// that changes the value touches `value_number`, which `idx_cav_attr_number`
/// contains, so it cannot: every typed-value index disqualifies HOT for its own
/// column, which is why the proposal makes vacuum cadence rather than fillfactor
/// the primary defence. Both are measured so the tuning is judged on what it
/// can reach.
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

    let entity_key = churn_entity(pool, prefix).await;

    sqlx::query(
        "INSERT INTO current_attribute_values (entity_key, attribute_name_id, attribute_kind, value_kind, value_number) \
         VALUES ($1, $2, 1, 1, 0::numeric)",
    )
    .bind(entity_key)
    .bind(attribute_name_id)
    .execute(pool)
    .await
    .expect("the churn projection row must be writable");

    // `$2` is bound but never changes the row's value: it only keeps the two
    // runs on one churn driver. `unit_code_id` is set to NULL, which it already
    // is, so the row version differs from its predecessor in nothing an index
    // carries.
    let restatement = churn(
        pool,
        "current_attribute_values",
        "UPDATE current_attribute_values SET unit_code_id = NULL WHERE entity_key = $1 AND $2 > 0",
        entity_key,
    )
    .await;

    assert_eq!(restatement.updated, CHURN_UPDATES, "every value-preserving update must be counted");
    // The row is small, no touched column is an index key, and the relation
    // reserves twenty percent of every page, so the overwhelming majority of
    // the run must stay on its original page. A threshold rather than a ratio
    // of one: the first update of a filled page still has to move.
    assert!(
        restatement.heap_only * 10 >= restatement.updated * 9,
        "a value-preserving restatement must stay heap-only: {} of {} updates were HOT",
        restatement.heap_only,
        restatement.updated
    );

    let revaluation = churn(
        pool,
        "current_attribute_values",
        "UPDATE current_attribute_values SET value_number = $2::numeric WHERE entity_key = $1",
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

/// Insert one bare entity in all three current-state relations under `prefix`
/// and return its surrogate key.
async fn churn_entity(pool: &PgPool, prefix: &str) -> i64 {
    sqlx::query_scalar(
        "WITH type_name AS ( \
             INSERT INTO entity_type_names (name) VALUES ('https://example.test/tenant-schema/Churn') \
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id), \
         ins AS (INSERT INTO entities (id) VALUES ($1::text || ':churn') RETURNING entity_key), \
         doc AS (INSERT INTO entity_documents (entity_key, payload) SELECT entity_key, '[]'::jsonb FROM ins) \
         INSERT INTO entity_search (entity_key, type_ids, attribute_name_ids) \
         SELECT ins.entity_key, ARRAY[type_name.id], ARRAY[]::integer[] FROM ins, type_name \
         RETURNING entity_key",
    )
    .bind(prefix)
    .fetch_one(pool)
    .await
    .expect("the churn entity must be writable")
}

/// A sustained payload rewrite over the document row.
///
/// The document row is the one every attribute write rewrites, and it carries
/// no index other than its key, so a payload rewrite touches no index key at
/// all: with the free space `fillfactor` reserves the run must stay heap-only,
/// which is the property the split of the entity into three relations exists
/// to buy.
#[tokio::test]
async fn sustained_document_churn_stays_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let entity_key = churn_entity(pool, scope.id_prefix()).await;
    let pages_before = relation_pages(pool, "entity_documents").await;

    let rewrite = churn(
        pool,
        "entity_documents",
        "UPDATE entity_documents SET payload = jsonb_build_array(jsonb_build_object('round', $2::bigint)), modified_at = now() WHERE entity_key = $1",
        entity_key,
    )
    .await;

    assert_eq!(rewrite.updated, CHURN_UPDATES, "every payload rewrite must be counted");
    assert!(
        rewrite.heap_only * 10 >= rewrite.updated * 9,
        "a payload rewrite must stay heap-only: {} of {} updates were HOT",
        rewrite.heap_only,
        rewrite.updated
    );
    // The relation is shared with every other test of the suite and carries
    // whatever dead space earlier suites left, so growth is measured against
    // the size the run started from.
    assert!(
        rewrite.pages <= pages_before + 1 + CHURN_UPDATES / 8,
        "sustained payload churn must not grow the relation without bound: {} pages after, {pages_before} before",
        rewrite.pages
    );

    scope.finish().await.expect("cleanup failed");
}

/// The cumulative update counter of `relation`, from the statistics collector.
async fn tuples_updated(pool: &PgPool, relation: &str) -> i64 {
    sqlx::query_scalar::<_, Option<i64>>("SELECT n_tup_upd FROM pg_stat_user_tables WHERE relname = $1")
        .bind(relation)
        .fetch_one(pool)
        .await
        .expect("the relation must report its statistics")
        .unwrap_or_default()
}

/// Wait until `relation` reports more than `before` updates, or give up.
///
/// A backend flushes its table statistics some time after each transaction,
/// so a counter is read by polling rather than once; the flush carries every
/// relation the transaction touched together, which is what lets a counter
/// that did not move be read as evidence.
async fn wait_for_update(pool: &PgPool, relation: &str, before: i64) -> i64 {
    for _ in 0..100 {
        let now = tuples_updated(pool, relation).await;
        if now > before {
            return now;
        }
        sleep(Duration::from_millis(50)).await;
    }
    tuples_updated(pool, relation).await
}

/// Through the API: a write that changes only an attribute value rewrites the
/// document row and leaves the search row untouched, and a write that adds a
/// type rewrites the search row too.
#[tokio::test]
async fn a_value_only_write_leaves_the_search_row_untouched_while_a_type_change_rewrites_it() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let pool = scope.database_pool();
    let id = scope.id("Sensor:search-row");

    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Sensor",
            "temperature": {"type": "Property", "value": 1},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create request failed");
    assert_eq!(created.status, StatusCode::CREATED);

    let documents_before = tuples_updated(pool, "entity_documents").await;
    let search_before = tuples_updated(pool, "entity_search").await;

    let value_only = client
        .update_entity_attrs(&id, &json!({"temperature": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("attribute update request failed");
    assert_eq!(value_only.status, StatusCode::NO_CONTENT);
    let documents_after_value = wait_for_update(pool, "entity_documents", documents_before).await;
    assert!(documents_after_value > documents_before, "the value write must rewrite the document row");
    assert_eq!(
        tuples_updated(pool, "entity_search").await,
        search_before,
        "a value-only write must leave the search row where it was"
    );

    let retyped = client
        .replace_entity(
            &id,
            &json!({
                "id": id,
                "type": ["Sensor", "Thermometer"],
                "temperature": {"type": "Property", "value": 2},
                "@context": [CORE_CONTEXT]
            }),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace request failed");
    assert_eq!(retyped.status, StatusCode::NO_CONTENT);
    let search_after_type = wait_for_update(pool, "entity_search", search_before).await;
    assert!(search_after_type > search_before, "a type change must rewrite the search row");

    scope.finish().await.expect("cleanup failed");
}

/// The claim-shaped update is the one that cannot go HOT: `next_attempt_at`
/// and `lease_until` are the two inputs of the claim index's expression, and
/// `delivered_at`/`exhausted_at` are its predicate, so every claim, retry and
/// acknowledgement writes new index tuples whatever the fillfactor is. The
/// index definition is the fact that makes it true, and it is also the fact
/// that makes the claim one bounded range scan: the expression is the single
/// key the claim orders and filters on.
#[tokio::test]
async fn the_delivery_claim_index_is_the_claimable_expression_so_vacuum_cadence_is_its_defence() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    let claim_index: String = sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = $1")
        .bind("idx_notification_deliveries_claimable")
        .fetch_one(scope.database_pool())
        .await
        .expect("the claim index must exist");

    assert!(
        claim_index.contains("GREATEST(next_attempt_at, COALESCE(lease_until,"),
        "the claim index must be led by the claimable-instant expression: {claim_index}"
    );
    assert!(claim_index.contains(", id)"), "the claim index must break ties on id: {claim_index}");
    for excluded in ["delivered_at IS NULL", "exhausted_at IS NULL"] {
        assert!(
            claim_index.contains(excluded),
            "the claim index must exclude rows where not {excluded}: {claim_index}"
        );
    }
    assert!(
        !claim_index.contains("attempt_count"),
        "the claim index must not repeat the attempt ceiling: {claim_index}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The side-effect claim writes only `locked_until` and `locked_by`, neither
/// of which any index of `pending_side_effects` carries (`idx_pse_claim_candidates`
/// is led by `next_attempt_at`, `idx_pse_dead` by `created_at`), so with the
/// fillfactor of 50 the migration declares the claim must stay heap-only.
#[tokio::test]
async fn sustained_side_effect_claims_stay_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    sqlx::query(
        "INSERT INTO pending_side_effects (id, write_id, recorded_at, consequence_base, payload) \
         VALUES (gen_random_uuid(), gen_random_uuid(), NOW(), 0, '{\"temporal_entities\": [], \"change_events\": []}'::jsonb)",
    )
    .execute(pool)
    .await
    .expect("the churn job must be seeded");

    let claims = churn(
        pool,
        "pending_side_effects",
        "UPDATE pending_side_effects SET locked_until = NOW() + make_interval(secs => $2), locked_by = 'worker-' || $2::text \
         WHERE consequence_base = $1::integer",
        0,
    )
    .await;

    assert_eq!(claims.updated, CHURN_UPDATES, "every claim must be counted");
    assert!(
        claims.heap_only * 10 >= claims.updated * 9,
        "the side-effect claim must be heap-only at least 90% of the time: {} of {}",
        claims.heap_only,
        claims.updated
    );

    sqlx::query("DELETE FROM pending_side_effects")
        .execute(pool)
        .await
        .expect("the churn job must be removable");
    scope.finish().await.expect("cleanup failed");
}

/// The delivery counters of a subscription live on `subscription_delivery_state`
/// so that a notification never rewrites the wide subscription row. That row
/// carries no index but its primary key and half of every page is kept free,
/// so the claim (`times_sent + 1`, `last_notification`) and the outcome marks
/// (`last_success` / `last_failure`) that every delivery writes must stay
/// heap-only.
#[tokio::test]
async fn sustained_delivery_state_churn_stays_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let subscription_key: i64 =
        sqlx::query_scalar("INSERT INTO subscriptions (id, kind, notification) VALUES ($1, 'entity', '{}'::jsonb) RETURNING subscription_key")
            .bind(scope.id("urn:ngsi-ld:Subscription:delivery-state-churn"))
            .fetch_one(pool)
            .await
            .expect("the churn subscription must be seeded");
    sqlx::query("INSERT INTO subscription_delivery_state (subscription_key) VALUES ($1)")
        .bind(subscription_key)
        .execute(pool)
        .await
        .expect("the churn state row must be seeded");

    let claims = churn(
        pool,
        "subscription_delivery_state",
        "UPDATE subscription_delivery_state SET times_sent = times_sent + 1, last_notification = NOW(), \
         last_success = CASE WHEN $2 % 2 = 0 THEN NOW() ELSE last_success END, \
         last_failure = CASE WHEN $2 % 2 = 1 THEN NOW() ELSE last_failure END \
         WHERE subscription_key = $1",
        subscription_key,
    )
    .await;

    assert_eq!(claims.updated, CHURN_UPDATES, "every claim must be counted");
    assert!(
        claims.heap_only * 10 >= claims.updated * 9,
        "the delivery-state write must be heap-only at least 90% of the time: {} of {}",
        claims.heap_only,
        claims.updated
    );

    scope.finish().await.expect("cleanup failed");
}
