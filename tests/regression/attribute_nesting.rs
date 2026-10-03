//! An attribute nesting deeper than the broker reads back what it stores, on
//! every Core write (R60).
//!
//! ETSI GS CIM 009 v1.9.1 bounds no nesting. The broker stores an attribute as
//! its JSON-LD expansion, about two containers per level, and reads every
//! stored document back with a reader that refuses 128. It admitted every
//! attribute the request reader took, about 125 levels, so an attribute of 61
//! levels answered 201 and the entity then answered 500 on every read, PATCH
//! and merge for good. A chain of 33 sub-attributes overflowed the native stack
//! on the read and one of 40 on the write itself, aborting the broker.
//!
//! Each attribute is now held to a nesting limit (32 by default) at admission
//! and refused with `BadRequestData`: clause 6.3.2 keeps 413 for too many
//! bytes, and clauses 5.7.1 and 5.7.2 answer a too deep query with
//! `BadRequestData`. Every attribute at the limit round-trips.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    attribute_nesting_fixture::{LIMIT, assert_absent, assert_refusal_detail, assert_refused, object_chain, probe, read, shapes, sub_attribute_chain},
    notification_adapter::NotificationAdapter,
};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// Create one entity, asserting clause 5.6.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let created = client.post_entity(body).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create {}: {:?}", body["id"], created.body);
}

/// Create `id` holding `p` as one level of nesting, the value every refused
/// write must leave in place.
async fn create_shallow(client: &TestClient, id: &str) {
    create(client, &probe(id, &json!({"type": "Property", "value": object_chain(1)}))).await;
}

/// Assert that `p` of `id` still nests the one level [`create_shallow`] wrote.
async fn assert_shallow(client: &TestClient, id: &str) {
    let entity = read(client, id, "").await;
    assert_eq!(entity["p"]["value"], object_chain(1), "{entity}");
}

/// Send `body` under `application/ld+json` to `path`.
async fn send(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("request")
}

/// A Property nesting `levels` objects, as an Attribute Fragment.
fn fragment(levels: usize) -> Value {
    json!({"type": "Property", "value": object_chain(levels), "@context": [CORE_CONTEXT]})
}

/// Clause 5.6.1: an attribute of every shape at the limit is created and reads
/// back as written.
#[tokio::test]
async fn regression_r60_every_shape_at_the_limit_is_created_and_reads_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (case, p) in shapes(LIMIT) {
        let id = scope.id(&format!("NestingProbe:at-limit-{case}"));
        create(client, &probe(&id, &p)).await;
        let entity = read(client, &id, "").await;
        assert_eq!(entity["p"], p, "{case}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// An attribute of 61 levels was admitted, and its entity could never be read
/// again: its stored expansion nests past the reader. It is refused, and
/// nothing is stored.
#[tokio::test]
async fn regression_r60_an_attribute_past_the_old_read_bound_is_refused_not_stored() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:past-read-bound");

    let response = client
        .post_entity(&probe(&id, &json!({"type": "Property", "value": object_chain(61)})))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_refused(&response, "61 levels");
    assert_absent(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Every shape one level past the limit is refused, and nothing is stored.
#[tokio::test]
async fn regression_r60_every_shape_past_the_limit_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (case, p) in shapes(LIMIT + 1) {
        let id = scope.id(&format!("NestingProbe:past-limit-{case}"));
        let response = client.post_entity(&probe(&id, &p)).with_ld_json().send().await.expect("create entity request");
        assert_refused(&response, case);
        assert_absent(client, &id).await;
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.18, 5.6.2, 5.6.3, 5.6.4, 5.6.19 and 5.6.17: every write of an
/// existing entity refuses an attribute past the limit and leaves the entity
/// as it was.
#[tokio::test]
async fn regression_r60_every_write_of_an_existing_entity_refuses_an_attribute_past_the_limit() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:existing");
    create_shallow(client, &id).await;
    let entity_path = format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&id));
    let deep = json!({"p": {"type": "Property", "value": object_chain(LIMIT + 1)}, "@context": [CORE_CONTEXT]});

    let replaced = client
        .replace_entity(
            &id,
            &probe(&id, &json!({"type": "Property", "value": object_chain(LIMIT + 1)})),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace entity request");
    assert_refused(&replaced, "replace entity");
    let updated = client.update_entity_attrs(&id, &deep).await.expect("update attributes request");
    assert_refused(&updated, "update attributes");
    let appended = client.append_entity_attrs(&id, &deep).await.expect("append attributes request");
    assert_refused(&appended, "append attributes");
    let partial = client
        .partial_update_attr(&id, "p", &json!({"value": object_chain(LIMIT + 1), "@context": [CORE_CONTEXT]}))
        .await
        .expect("partial attribute update request");
    assert_refused(&partial, "partial attribute update");
    let replaced_attribute = send(client, Method::PUT, &format!("{entity_path}/attrs/p"), &fragment(LIMIT + 1)).await;
    assert_refused(&replaced_attribute, "replace attribute");
    let merged = client.merge_entity(&id, &deep, None).await.expect("merge entity request");
    assert_refused(&merged, "merge entity");
    assert_shallow(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// The same writes at the limit deepen the held attribute to the limit.
#[tokio::test]
async fn regression_r60_every_write_of_an_existing_entity_admits_an_attribute_at_the_limit() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:existing-at-limit");
    create_shallow(client, &id).await;
    let entity_path = format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&id));
    let at_limit = json!({"p": {"type": "Property", "value": object_chain(LIMIT)}, "@context": [CORE_CONTEXT]});

    let updated = client.update_entity_attrs(&id, &at_limit).await.expect("update attributes request");
    assert_deepened(client, &id, &updated, "update attributes").await;
    let appended = client.append_entity_attrs(&id, &at_limit).await.expect("append attributes request");
    assert_deepened(client, &id, &appended, "append attributes").await;
    let partial = client
        .partial_update_attr(&id, "p", &json!({"value": object_chain(LIMIT), "@context": [CORE_CONTEXT]}))
        .await
        .expect("partial attribute update request");
    assert_deepened(client, &id, &partial, "partial attribute update").await;
    let replaced_attribute = send(client, Method::PUT, &format!("{entity_path}/attrs/p"), &fragment(LIMIT)).await;
    assert_deepened(client, &id, &replaced_attribute, "replace attribute").await;
    let merged = client.merge_entity(&id, &at_limit, None).await.expect("merge entity request");
    assert_deepened(client, &id, &merged, "merge entity").await;

    scope.finish().await.expect("environment cleanup");
}

/// Assert `answer` wrote `p` of `id` at the limit, then set it back to one
/// level for the next write.
async fn assert_deepened(client: &TestClient, id: &str, answer: &TestResponse, case: &str) {
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{case}: {:?}", answer.body);
    let entity = read(client, id, "").await;
    assert_eq!(entity["p"]["value"], object_chain(LIMIT), "{case}: {entity}");
    let reset = client
        .update_entity_attrs(id, &json!({"p": {"type": "Property", "value": object_chain(1)}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("reset request");
    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{case}: {:?}", reset.body);
}

/// Clauses 5.6.7, 5.6.8, 5.6.9 and 5.6.20: a batch refuses the entity nesting
/// too deeply against its own identifier, and writes its neighbour.
#[tokio::test]
async fn regression_r60_a_batch_refuses_only_the_entity_nesting_too_deeply() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let deep = json!({"type": "Property", "value": object_chain(LIMIT + 1)});
    let at_limit = json!({"type": "Property", "value": object_chain(LIMIT)});

    let refused_id = scope.id("NestingProbe:batch-create-refused");
    let created_id = scope.id("NestingProbe:batch-create-created");
    let created = client
        .batch_create(&json!([probe(&refused_id, &deep), probe(&created_id, &at_limit)]))
        .await
        .expect("batch create request");
    assert_batch_refused(&created, &refused_id, &created_id, "batch create");
    assert_absent(client, &refused_id).await;
    assert_eq!(read(client, &created_id, "").await["p"], at_limit);

    for (case, path) in [("batch upsert", "upsert"), ("batch update", "update"), ("batch merge", "merge")] {
        let refused_id = scope.id(&format!("NestingProbe:{path}-refused"));
        let written_id = scope.id(&format!("NestingProbe:{path}-written"));
        create_shallow(client, &refused_id).await;
        create_shallow(client, &written_id).await;
        let answer = send(
            client,
            Method::POST,
            &format!("/ngsi-ld/v1/entityOperations/{path}"),
            &json!([probe(&refused_id, &deep), probe(&written_id, &at_limit)]),
        )
        .await;
        assert_batch_refused(&answer, &refused_id, &written_id, case);
        assert_shallow(client, &refused_id).await;
        assert_eq!(read(client, &written_id, "").await["p"]["value"], object_chain(LIMIT), "{case}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Assert a batch answered 207, refusing `refused` alone with the nesting
/// refusal and writing `written`.
fn assert_batch_refused(answer: &TestResponse, refused: &str, written: &str, case: &str) {
    assert_eq!(answer.status, StatusCode::MULTI_STATUS, "{case}: {:?}", answer.body);
    let result = answer.json_value().expect("a batch operation result");
    assert_eq!(result["success"], json!([written]), "{case}: {result}");
    let errors = result["errors"].as_array().expect("the errors");
    assert_eq!(errors.len(), 1, "{case}: {result}");
    assert_eq!(errors[0]["entityId"], json!(refused), "{case}: {result}");
    assert_refusal_detail(&errors[0]["error"], case);
}

/// The deepest admitted entity is served by every read: a notification, a
/// query page, the simplified representation, and a PATCH of another
/// attribute beside it.
#[tokio::test]
async fn regression_r60_an_entity_at_the_limit_is_served_by_every_read() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let id = scope.id("NestingProbe:served");
    let subscription_id = scope.id("Subscription:nesting");
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": id, "type": "NestingProbe"}],
            "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);

    let p = sub_attribute_chain(LIMIT);
    create(client, &probe(&id, &p)).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(notification.body["data"][0]["p"], p, "{}", notification.body);

    let page = client
        .query_entities(
            &format!("type=NestingProbe&id={}", urlencoding::encode(&id)),
            "application/json",
            Some(CORE_CONTEXT),
        )
        .await
        .expect("query entities request");
    assert_eq!(page.status, StatusCode::OK, "{:?}", page.body);
    assert_eq!(page.json_value().expect("a query answer")[0]["p"], p);
    // Clause 4.5.4: the simplified representation keeps the value alone.
    let simplified = read(client, &id, "options=keyValues").await;
    assert_eq!(simplified["p"], json!(1), "{simplified}");
    let patched = client
        .update_entity_attrs(&id, &json!({"q": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("update attributes request");
    assert_eq!(patched.status, StatusCode::NO_CONTENT, "{:?}", patched.body);
    assert_eq!(read(client, &id, "").await["p"], p);

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    scope.finish().await.expect("environment cleanup");
}

/// A chain of 124 sub-attributes, the longest the request reader admits in
/// one attribute, overflowed the stack of the request's worker and aborted the
/// broker. It is refused like any attribute past the limit, and the broker
/// answers on.
#[tokio::test]
async fn regression_r60_a_chain_of_sub_attributes_as_long_as_the_reader_admits_is_refused_without_aborting() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:reader-limit");

    let response = client
        .post_entity(&probe(&id, &sub_attribute_chain(124)))
        .with_ld_json()
        .send()
        .await
        .expect("the broker answers the create");
    assert_refused(&response, "124 sub-attributes");
    assert_absent(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// A text nesting past the request reader's 127 containers is no JSON the
/// reader takes, and stays the reader's `InvalidRequest`.
#[tokio::test]
async fn regression_r60_a_text_past_the_request_reader_is_an_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:past-reader");

    let response = client
        .post_entity(&probe(&id, &json!({"type": "Property", "value": object_chain(126)})))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    let problem = response.json_value().expect("a problem detail");
    assert_eq!(problem["type"], json!("https://uri.etsi.org/ngsi-ld/errors/InvalidRequest"), "{problem}");
    assert_absent(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}
