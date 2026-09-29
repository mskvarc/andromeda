//! Writing to a cloned Snapshot (clause 5.16.2.4).
//!
//! A clone copies "All Entity and Temporal Evolution of Entity data" of its
//! source, with the source's keys, into a branch of its own, and a published
//! Snapshot branch accepts writes through `NGSILD-Snapshot`. Every key a write
//! to the clone mints must therefore lie past every key the clone copied: the
//! clone's identity sequences continue from its source's rather than starting
//! over. The Snapshot-scoped subscriptions of the source and their notification
//! queue are not Entity data and stay behind.

use crate::{
    notification_adapter::NotificationAdapter,
    support::{clean_database, client, clone_snapshot_raw, create_entity, create_snapshot_raw, published_branch_schema, scoped, wait_for_snapshot_terminal},
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const CAR: &str = "https://example.org/clone/Car";
const SPEED: &str = "https://example.org/clone/speed";
const COLOUR: &str = "https://example.org/clone/colour";

/// The identity columns of the relations a clone copies, each with the
/// relation holding it.
const COPIED_IDENTITIES: [(&str, &str); 3] = [
    ("entities", "entity_key"),
    ("temporal_entities", "temporal_entity_key"),
    ("temporal_attribute_streams", "stream_key"),
];

/// The relations holding Snapshot-scoped subscriptions and their notification
/// queue, which a clone leaves empty.
const SUBSCRIPTION_RELATIONS: [&str; 4] = ["subscriptions", "subscription_delivery_state", "notification_events", "notification_deliveries"];

fn car(id: &str, speed: i64) -> Value {
    json!({"id": id, "type": CAR, SPEED: {"type": "Property", "value": speed}, "@context": CORE_CONTEXT})
}

fn colour(value: &str) -> Value {
    json!({COLOUR: {"type": "Property", "value": value}, "@context": CORE_CONTEXT})
}

fn subscription(id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": CAR}],
        "notification": {"format": "normalized", "endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": CORE_CONTEXT
    })
}

/// Capture every live car into a new Snapshot and wait for it to succeed.
async fn capture_cars(snapshot: &str) {
    let body = json!({
        "id": snapshot,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": CAR}]}]
    });
    let (status, _, problem) = create_snapshot_raw(&client(), &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), snapshot, Duration::from_secs(20)).await["snapshotStatus"],
        "success"
    );
}

/// Clone `source` into `target` and wait for the clone to succeed.
async fn clone_into(source: &str, target: &str) {
    let (status, _, problem) = clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), target, Duration::from_secs(20)).await["snapshotStatus"],
        "success"
    );
}

/// The status of one write scoped to `snapshot`, with the problem it answered.
async fn write(method: Method, path: &str, snapshot: &str, body: &Value) -> (StatusCode, Value) {
    let response = scoped(method, path, snapshot, Some(body)).await;
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

/// The largest key the relation holds and the position of the sequence behind
/// it, in `schema`.
async fn key_and_sequence(pool: &PgPool, schema: &str, table: &str, column: &str) -> (Option<i64>, Option<i64>) {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT (SELECT max({column}) FROM {schema}.{table}), \
         (SELECT last_value FROM pg_sequences \
         WHERE format('%I.%I', schemaname, sequencename)::regclass = pg_get_serial_sequence($1, $2)::regclass)"
    )))
    .bind(format!("{schema}.{table}"))
    .bind(column)
    .fetch_one(pool)
    .await
    .expect("the key and the sequence position")
}

/// Every identity sequence of the relations a clone copies stands at or past
/// the largest key it holds, so the next key it mints is new.
async fn assert_sequences_continue_past_the_copied_keys(pool: &PgPool, schema: &str) {
    for (table, column) in COPIED_IDENTITIES {
        let (largest, position) = key_and_sequence(pool, schema, table, column).await;
        assert!(largest.is_some(), "{schema}.{table} holds copied rows");
        assert!(
            position >= largest,
            "{schema}.{table}.{column}: the sequence stands at {position:?}, behind the copied key {largest:?}"
        );
    }
}

/// A source holding captured cars, a car created in its branch and a new
/// attribute on a captured car, so every copied identity relation has rows.
async fn source_with_branch_writes(source: &str) {
    for n in 1..=3 {
        pretty_assertions::assert_eq!(
            create_entity(&client(), &car(&format!("urn:ngsi-ld:CloneCar:captured-{n}"), n)).await,
            StatusCode::CREATED
        );
    }
    capture_cars(source).await;
    let (status, problem) = write(Method::POST, "/entities", source, &car("urn:ngsi-ld:CloneCar:source-write", 10)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    let (status, problem) = write(Method::POST, "/entities/urn:ngsi-ld:CloneCar:captured-1/attrs", source, &colour("red")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{problem}");
}

/// Clause 5.16.2.4 copies the source's Entity and Temporal Evolution data with
/// its keys; a write to the clone afterwards creates an Entity, a batch of
/// Entities, a new Attribute on a copied Entity and their temporal evolution,
/// each under a key the clone mints past every copied one.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_writes_after_the_copy_mint_keys_past_every_copied_key() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-writes-source";
    let target = "urn:ngsi-ld:Snapshot:clone-writes-target";
    source_with_branch_writes(source).await;

    clone_into(source, target).await;
    assert_sequences_continue_past_the_copied_keys(pool, &published_branch_schema(pool, target).await).await;

    let (status, problem) = write(Method::POST, "/entities", target, &car("urn:ngsi-ld:CloneCar:clone-write", 20)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "a create in the clone: {problem}");
    let batch = json!([car("urn:ngsi-ld:CloneCar:clone-batch-1", 21), car("urn:ngsi-ld:CloneCar:clone-batch-2", 22)]);
    let (status, problem) = write(Method::POST, "/entityOperations/create", target, &batch).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "a batch create in the clone: {problem}");
    let (status, problem) = write(Method::POST, "/entities/urn:ngsi-ld:CloneCar:captured-2/attrs", target, &colour("blue")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "a new Attribute on a copied Entity: {problem}");

    for id in [
        "urn:ngsi-ld:CloneCar:clone-write",
        "urn:ngsi-ld:CloneCar:clone-batch-2",
        "urn:ngsi-ld:CloneCar:source-write",
    ] {
        pretty_assertions::assert_eq!(
            scoped(Method::GET, &format!("/entities/{id}"), target, None).await.status(),
            StatusCode::OK,
            "{id}"
        );
    }
    let history = scoped(Method::GET, "/temporal/entities/urn:ngsi-ld:CloneCar:clone-write", target, None).await;
    pretty_assertions::assert_eq!(history.status(), StatusCode::OK);
    let history: Value = history.json().await.expect("the clone's temporal evolution");
    pretty_assertions::assert_eq!(history["id"], "urn:ngsi-ld:CloneCar:clone-write");
    assert!(
        history[SPEED].to_string().contains("20"),
        "the clone records the new Entity's evolution: {history}"
    );
    pretty_assertions::assert_eq!(
        scoped(Method::GET, "/entities/urn:ngsi-ld:CloneCar:clone-write", source, None).await.status(),
        StatusCode::NOT_FOUND,
        "the source never sees a write to its clone"
    );

    environment.reset_and_finish().await.expect("reset");
}

/// The clone of a clone continues from the first clone's keys, including the
/// ones written to it after its own copy.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_of_a_written_clone_continues_its_keys() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-chain-source";
    let first = "urn:ngsi-ld:Snapshot:clone-chain-first";
    let second = "urn:ngsi-ld:Snapshot:clone-chain-second";
    source_with_branch_writes(source).await;

    clone_into(source, first).await;
    let (status, problem) = write(Method::POST, "/entities", first, &car("urn:ngsi-ld:CloneCar:first-write", 30)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");

    clone_into(first, second).await;
    let second_schema = published_branch_schema(pool, second).await;
    assert_sequences_continue_past_the_copied_keys(pool, &second_schema).await;
    let (status, problem) = write(Method::POST, "/entities", second, &car("urn:ngsi-ld:CloneCar:second-write", 31)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    let (status, problem) = write(Method::POST, "/entities/urn:ngsi-ld:CloneCar:first-write/attrs", second, &colour("green")).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{problem}");

    environment.reset_and_finish().await.expect("reset");
}

/// Clause 5.16.2.4 copies Entity and Temporal Evolution data only. The
/// source's Snapshot-scoped subscriptions, their delivery state and their
/// pending notifications stay behind: the clone lists no subscription, a write
/// to it notifies none of the source's subscribers, and a subscription under
/// the source's identifier can be created in it.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_copies_no_subscription_and_no_pending_notification() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let receiver = NotificationAdapter::start().await;
    let source = "urn:ngsi-ld:Snapshot:clone-subscriptions-source";
    let target = "urn:ngsi-ld:Snapshot:clone-subscriptions-target";
    let delivered = "urn:ngsi-ld:Subscription:clone-source-delivered";
    let pending = "urn:ngsi-ld:Subscription:clone-source-pending";
    source_with_branch_writes(source).await;

    let (status, problem) = write(Method::POST, "/subscriptions", source, &subscription(delivered, &receiver.endpoint_url())).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    let (status, problem) = write(
        Method::POST,
        "/subscriptions",
        source,
        &subscription(pending, "http://host.docker.internal:1/undeliverable"),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    let (status, problem) = write(Method::POST, "/entities", source, &car("urn:ngsi-ld:CloneCar:notified", 40)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    receiver.wait_for_subscription_notification(delivered, Duration::from_secs(10)).await;
    let source_schema = published_branch_schema(pool, source).await;
    let queued: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {source_schema}.notification_deliveries")))
        .fetch_one(pool)
        .await
        .expect("the source's delivery queue");
    assert!(queued > 0, "the source's queue holds its deliveries, which the retention reap has not reached");

    clone_into(source, target).await;
    let target_schema = published_branch_schema(pool, target).await;
    for relation in SUBSCRIPTION_RELATIONS {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {target_schema}.{relation}")))
            .fetch_one(pool)
            .await
            .expect("the clone's subscription relations");
        pretty_assertions::assert_eq!(rows, 0, "{target_schema}.{relation} is not copied");
    }
    let listed = scoped(Method::GET, "/subscriptions", target, None).await;
    pretty_assertions::assert_eq!(listed.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(listed.json::<Value>().await.expect("the clone's subscriptions"), json!([]));

    receiver.clear_notifications().await;
    let (status, problem) = write(Method::POST, "/entities", target, &car("urn:ngsi-ld:CloneCar:unnotified", 41)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    receiver.wait_for_no_subscription_notification(delivered, Duration::from_millis(750)).await;

    let (status, problem) = write(Method::POST, "/subscriptions", target, &subscription(delivered, &receiver.endpoint_url())).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "the source's identifier is free in the clone: {problem}");

    receiver.shutdown().await;
    environment.reset_and_finish().await.expect("reset");
}
