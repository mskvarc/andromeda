//! ETSI GS CIM 009 v1.9.1 clause 5.5.11.0: "Implementations shall treat the entity instances as if they had all arrived in separate requests", and clauses 5.6.8.4, 5.6.9.4 and 5.6.20.4 run the single-entity behaviour of Upsert, Update and Merge for each entity of a batch. A batch entry is therefore exactly as safe beside a concurrent request as the single-entity operation it runs, and several broker replicas sharing one database are a supported deployment.
//!
//! Nothing acknowledged may be missing: a write that loses its compare-and-set on every attempt the broker makes is answered as a conflict and leaves nothing behind, which a sustained race on one entity across two replicas occasionally produces, on the single-entity paths as much as on the batch ones.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::large_entity_replicas::second_replica;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use futures::{StreamExt, future::BoxFuture, stream};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Map, Value, json};

/// The core context every body names.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
/// How many writes each side of a race on one entity sends.
const WRITES_PER_SIDE: usize = 150;
/// How many identifiers a create race runs over.
const CREATE_RACES: usize = 60;
/// How many requests are in flight at once.
const IN_FLIGHT: usize = 64;
/// The share of a race's writes, one in this many, that may at most be answered as a conflict.
const MAX_CONFLICT_SHARE: usize = 10;
/// The problem type of a write that conflicts with the current state.
const CONFLICT: &str = "https://uri.etsi.org/ngsi-ld/errors/Conflict";

/// Send `body` as JSON-LD to `path` through `client`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("the request is answered")
}

/// One Property `name` holding `value`, with the core context.
fn fragment(name: &str, value: usize) -> Value {
    json!({name: {"type": "Property", "value": value}, "@context": CORE_CONTEXT})
}

/// The entity `id` carrying the Property `name`, as a batch entry.
fn batch_entry(id: &str, name: &str, value: usize) -> Value {
    let mut entry = fragment(name, value);
    entry["id"] = json!(id);
    entry["type"] = json!("Probe");
    entry
}

/// Create the entity `id` through `client`, carrying nothing but its type.
async fn create_probe(client: &TestClient, id: &str) {
    let created = send(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": id, "type": "Probe", "@context": CORE_CONTEXT}),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// Every request of both sides, interleaved and kept in flight together,
/// answered with its label.
async fn race<'a>(first: Vec<(String, BoxFuture<'a, TestResponse>)>, second: Vec<(String, BoxFuture<'a, TestResponse>)>) -> Vec<(String, TestResponse)> {
    let mut interleaved = Vec::with_capacity(first.len() + second.len());
    let mut second = second.into_iter();
    for request in first {
        interleaved.push(request);
        interleaved.extend(second.next());
    }
    interleaved.extend(second);
    stream::iter(interleaved.into_iter().map(|(label, response)| async move { (label, response.await) }))
        .buffer_unordered(IN_FLIGHT)
        .collect()
        .await
}

/// The members the entity `id` holds, read through `client` as key-values.
async fn stored_members(client: &TestClient, id: &str) -> Map<String, Value> {
    let read = client
        .get_entity(id, "options=keyValues", "application/json", None)
        .await
        .expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    serde_json::from_value(read.json_value().expect("the entity is JSON")).expect("an entity is an object")
}

/// Whether `response` answers a write that lost to concurrent writers on every attempt the broker made: `409 Conflict` for a single-entity write, and a `207` whose one error is that Conflict for a single-entry batch (Table 6.3.2-1, "The operation conflicts with the current state of the system").
fn is_conflict(response: &TestResponse) -> bool {
    let Ok(body) = response.json_value() else {
        return false;
    };
    match response.status {
        StatusCode::CONFLICT => body["type"] == CONFLICT,
        StatusCode::MULTI_STATUS => {
            body["success"] == json!([])
                && body["errors"]
                    .as_array()
                    .is_some_and(|errors| errors.len() == 1 && errors[0]["error"]["type"] == CONFLICT)
        }
        _ => false,
    }
}

/// Assert no write was lost: every request was either acknowledged with 204, and then the attribute it wrote is held by the entity `id`, or answered as a conflict, and then nothing of it landed.
///
/// A conflict is the broker's designed answer once a write lost its compare-and-set on every attempt; it is only allowed to stay rare, which is what keeps this test from passing on a broker that refuses everything.
async fn assert_no_write_is_lost(client: &TestClient, id: &str, answered: &[(String, TestResponse)]) {
    let unexpected: Vec<String> = answered
        .iter()
        .filter(|(_, response)| response.status != StatusCode::NO_CONTENT && !is_conflict(response))
        .map(|(label, response)| format!("{label}: {} {:?}", response.status, response.body))
        .collect();
    assert_eq!(unexpected, Vec::<String>::new(), "every write is acknowledged or answered as a conflict");
    let members = stored_members(client, id).await;
    let missing: Vec<&str> = answered
        .iter()
        .filter(|(label, response)| response.status == StatusCode::NO_CONTENT && !members.contains_key(label))
        .map(|(label, _)| label.as_str())
        .collect();
    assert_eq!(missing, Vec::<&str>::new(), "acknowledged but missing from {id}");
    let conflicts: Vec<&str> = answered
        .iter()
        .filter(|(_, response)| response.status != StatusCode::NO_CONTENT)
        .map(|(label, _)| label.as_str())
        .collect();
    let landed_anyway: Vec<&&str> = conflicts.iter().filter(|label| members.contains_key(**label)).collect();
    assert_eq!(landed_anyway, Vec::<&&str>::new(), "answered as a conflict but landed in {id}");
    assert!(
        conflicts.len() * MAX_CONFLICT_SHARE <= answered.len(),
        "more than one write in {MAX_CONFLICT_SHARE} conflicted: {conflicts:?}"
    );
}

/// The single-entity writes the second replica races with: one Merge Entity
/// per attribute `p<i>`.
fn merges_through<'a>(client: &'a TestClient, id: &'a str) -> Vec<(String, BoxFuture<'a, TestResponse>)> {
    (0..WRITES_PER_SIDE)
        .map(|index| {
            let name = format!("p{index}");
            let request: BoxFuture<'a, TestResponse> = Box::pin(async move {
                send(
                    client,
                    Method::PATCH,
                    &format!("/ngsi-ld/v1/entities/{id}"),
                    &fragment(&format!("p{index}"), index),
                )
                .await
            });
            (name, request)
        })
        .collect()
}

/// The batch writes the first replica races with: one single-entry batch to
/// `path` per attribute `b<i>`.
fn batches_through<'a>(client: &'a TestClient, id: &'a str, path: &'static str) -> Vec<(String, BoxFuture<'a, TestResponse>)> {
    (0..WRITES_PER_SIDE)
        .map(|index| {
            let name = format!("b{index}");
            let request: BoxFuture<'a, TestResponse> =
                Box::pin(async move { send(client, Method::POST, path, &json!([batch_entry(id, &format!("b{index}"), index)])).await });
            (name, request)
        })
        .collect()
}

/// Clause 5.6.20.4: each Batch Entity Merge entry runs Merge Entity, so a Merge Entity another replica commits meanwhile is kept, and so is the batch's.
#[tokio::test]
async fn regression_batch_cas_batch_merge_keeps_a_concurrent_merge_from_the_other_replica() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let id = scope.id("Probe:batch-merge");
    create_probe(first, &id).await;

    let answered = race(batches_through(first, &id, "/ngsi-ld/v1/entityOperations/merge"), merges_through(&second, &id)).await;

    assert_no_write_is_lost(first, &id, &answered).await;
    scope.finish().await.expect("cleanup");
}

/// Clause 5.6.9.4: each Batch Entity Update entry runs the attribute update of its entity, so an Append Attributes another replica commits meanwhile is kept.
#[tokio::test]
async fn regression_batch_cas_batch_update_keeps_a_concurrent_append_from_the_other_replica() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let id = scope.id("Probe:batch-update");
    create_probe(first, &id).await;

    let appends = (0..WRITES_PER_SIDE)
        .map(|index| {
            let request: BoxFuture<'_, TestResponse> = Box::pin({
                let (second, id) = (&second, &id);
                async move {
                    send(
                        second,
                        Method::POST,
                        &format!("/ngsi-ld/v1/entities/{id}/attrs"),
                        &fragment(&format!("p{index}"), index),
                    )
                    .await
                }
            });
            (format!("p{index}"), request)
        })
        .collect();
    let answered = race(batches_through(first, &id, "/ngsi-ld/v1/entityOperations/update"), appends).await;

    assert_no_write_is_lost(first, &id, &answered).await;
    scope.finish().await.expect("cleanup");
}

/// Clause 5.6.8.4: an update-mode Batch Entity Upsert entry runs clause 5.6.2 on an entity that exists, so a Merge Entity another replica commits meanwhile is kept.
#[tokio::test]
async fn regression_batch_cas_upsert_update_keeps_a_concurrent_merge_from_the_other_replica() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let id = scope.id("Probe:upsert-update");
    create_probe(first, &id).await;

    let answered = race(
        batches_through(first, &id, "/ngsi-ld/v1/entityOperations/upsert?options=update"),
        merges_through(&second, &id),
    )
    .await;

    assert_no_write_is_lost(first, &id, &answered).await;
    scope.finish().await.expect("cleanup");
}

/// Race, per identifier, an update-mode upsert of `u` through `upserting` against a Create Entity of `c` through `creating`, and assert each pair ends as one of the two serial orders would: the create first leaves `c` and `u`, the upsert first leaves `u` and answers the create 409.
async fn assert_upsert_and_create_both_take_effect(scope: &TenantScopedEnvironment, upserting: &TestClient, creating: &TestClient, label: &str) {
    create_probe(upserting, &scope.id(&format!("Probe:{label}-tenant"))).await;
    let ids: Vec<String> = (0..CREATE_RACES).map(|index| scope.id(&format!("Probe:{label}-{index}"))).collect();
    let upserts = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let request: BoxFuture<'_, TestResponse> = Box::pin(async move {
                send(
                    upserting,
                    Method::POST,
                    "/ngsi-ld/v1/entityOperations/upsert?options=update",
                    &json!([batch_entry(id, "u", index)]),
                )
                .await
            });
            (format!("upsert {id}"), request)
        })
        .collect();
    let creates = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let request: BoxFuture<'_, TestResponse> =
                Box::pin(async move { send(creating, Method::POST, "/ngsi-ld/v1/entities", &batch_entry(id, "c", index)).await });
            (format!("create {id}"), request)
        })
        .collect();

    let answered = race(upserts, creates).await;

    let mut wrong = Vec::new();
    for id in &ids {
        let answer = |kind: &str| {
            answered
                .iter()
                .find(|(label, _)| *label == format!("{kind} {id}"))
                .map(|(_, response)| response)
                .expect("every request is answered")
        };
        let (upsert, create) = (answer("upsert"), answer("create"));
        let members = stored_members(upserting, id).await;
        let upsert_took_effect = matches!(upsert.status, StatusCode::CREATED | StatusCode::NO_CONTENT) && members.contains_key("u");
        let create_consistent = match create.status {
            StatusCode::CREATED => members.contains_key("c"),
            StatusCode::CONFLICT => !members.contains_key("c"),
            _ => false,
        };
        if !(upsert_took_effect && create_consistent) {
            wrong.push(format!(
                "{id}: upsert {} {:?}, create {} -> {:?}",
                upsert.status,
                upsert.body,
                create.status,
                members.keys().collect::<Vec<_>>()
            ));
        }
    }
    assert_eq!(wrong, Vec::<String>::new(), "no serial order of the two requests leaves these");
}

/// Clause 5.6.8.4: an update-mode upsert creates an Entity that does not exist and updates one that does. Losing the race to a create on the other replica makes it the second case, not a success that stored nothing.
#[tokio::test]
async fn regression_batch_cas_upsert_update_racing_a_create_keeps_both() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let second = second_replica(&scope);

    assert_upsert_and_create_both_take_effect(&scope, scope.client(), &second, "two-replicas").await;
    scope.finish().await.expect("cleanup");
}

/// The same race on one broker: Create Entity takes no per-identifier guard, so the store's own condition is all that orders the two.
#[tokio::test]
async fn regression_batch_cas_upsert_update_racing_a_create_on_one_broker_keeps_both() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");

    assert_upsert_and_create_both_take_effect(&scope, scope.client(), scope.client(), "one-broker").await;
    scope.finish().await.expect("cleanup");
}
