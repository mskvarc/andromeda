//! Snapshot-scoped temporal writes on a broker whose live branch defers its
//! temporal history.
//!
//! Clause 5.5.15 permits the Temporal API on a Snapshot, so a Snapshot-scoped
//! write is part of that branch's Temporal Evolution the moment it answers.
//! `snapshot-deferred` shares the primary database but queues its live-branch
//! history for a background drain; nothing drains a Snapshot branch's queue, so
//! it records a branch's history in the write's own transaction. The native
//! `snapshot-primary`, which reads history straight from the branch tables and
//! overlays no queued intent, is what proves the rows are there.

use crate::support::{broker_url, clean_database, client, create_snapshot_raw, wait_for_snapshot_terminal};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::{env, time::Duration};

/// The broker configured with `temporal.append_mode = "deferred"`.
const DEFERRED_BASE: &str = "http://127.0.0.1:1088/ngsi-ld/v1";

async fn snapshot_database() -> PgPool {
    PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("Snapshot database connection")
}

/// How many rows `relation` of branch `schema` holds. The schema name is
/// derived from a surrogate key, never from input.
async fn branch_rows(pool: &PgPool, schema: &str, relation: &str) -> i64 {
    let sql = format!("SELECT count(*) FROM {schema}.{relation}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await
        .expect("branch relation count")
}

/// Whether branch `schema` has a relation named `relation` at all.
async fn branch_has(pool: &PgPool, schema: &str, relation: &str) -> bool {
    sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(format!("{schema}.{relation}"))
        .fetch_one(pool)
        .await
        .expect("relation lookup")
}

/// The `(value, observedAt)` of every instance a temporal attribute holds, in
/// the order it holds them. A single instance may be rendered as the instance
/// itself rather than a one-element array.
fn observations(attribute: &Value) -> Vec<(Value, Value)> {
    let instances = match attribute {
        Value::Array(instances) => instances.iter().collect(),
        instance @ Value::Object(_) => vec![instance],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    instances
        .into_iter()
        .map(|instance| (instance["value"].clone(), instance["observedAt"].clone())) // clone: the pair owns what the rendered instance holds
        .collect()
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_scoped_temporal_write_under_deferred_history_is_recorded_in_the_branch_at_once() {
    let _serialized_environment = clean_database().await;
    let pool = snapshot_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:deferred-history";
    let entity_id = "urn:ngsi-ld:TemporalVehicle:deferred-history";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": snapshot, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    let prepared = wait_for_snapshot_terminal(&client(), snapshot, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(prepared["snapshotStatus"], "success", "the empty Snapshot publishes its branch: {prepared}");
    let key: i64 = sqlx::query_scalar("SELECT snapshot_key FROM snapshots WHERE id = $1")
        .bind(snapshot)
        .fetch_one(&pool)
        .await
        .expect("snapshot key");
    let schema = format!("snap_{key}");

    let created = client()
        .request(Method::POST, format!("{DEFERRED_BASE}/temporal/entities"))
        .header("NGSILD-Snapshot", snapshot)
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "id": entity_id,
            "type": "https://example.org/TemporalVehicle",
            "speed": [{"type": "Property", "value": 42, "observedAt": "2025-01-01T00:00:00Z"}],
            "@context": CORE_CONTEXT.as_str()
        }))
        .send()
        .await
        .expect("Snapshot-scoped temporal create on the deferred broker");
    pretty_assertions::assert_eq!(created.status(), StatusCode::CREATED);

    assert!(
        !branch_has(&pool, &schema, "temporal_append_intents").await,
        "a Snapshot branch has no intent queue: only the live branch defers its history"
    );
    assert!(
        branch_rows(&pool, &schema, "attribute_history").await > 0,
        "the branch's history holds the observation as soon as the write answers"
    );

    let read = client()
        .get(broker_url(&format!("/temporal/entities/{entity_id}")))
        .header("NGSILD-Snapshot", snapshot)
        .send()
        .await
        .expect("Snapshot-scoped temporal read on the native broker");
    pretty_assertions::assert_eq!(read.status(), StatusCode::OK);
    let entity = read.json::<Value>().await.expect("branch temporal entity");
    pretty_assertions::assert_eq!(
        observations(&entity["speed"]),
        vec![(json!(42), json!("2025-01-01T00:00:00Z"))],
        "the native broker reads the one observation from the branch history: {entity}"
    );
    pool.close().await;
}
