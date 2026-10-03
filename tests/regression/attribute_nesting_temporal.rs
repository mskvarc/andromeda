//! An attribute nesting deeper than the broker reads back what it stores, on
//! every Temporal write (R60).
//!
//! A Temporal Evolution records each observation's value and metadata in its
//! history and its last state in the entity it extends (clauses 5.6.11,
//! 5.6.12, 5.6.14), so a Temporal write is held to the same nesting limit as a
//! Core write. Before, an observation of 61 levels answered 201 and its entity
//! then answered 500 on every Core read, and from 63 levels on every temporal
//! read too.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::attribute_nesting_fixture::{LIMIT, assert_absent, assert_refused, object_chain, read, shapes, sub_attribute_chain};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The instant every observation of this module is made at.
const OBSERVED_AT: &str = "2024-01-01T00:00:00Z";

/// `attribute` as one observation made at [`OBSERVED_AT`].
fn observation(attribute: &Value) -> Value {
    let mut observed = attribute.clone(); // clone: the caller keeps the attribute to compare against
    observed["observedAt"] = json!(OBSERVED_AT);
    json!([observed])
}

/// A Temporal Evolution of `id` observing `p` once.
fn evolution(id: &str, p: &Value) -> Value {
    json!({"id": id, "type": "NestingProbe", "p": observation(p), "@context": CORE_CONTEXT})
}

/// Create the Temporal Evolution `body`.
async fn create(client: &TestClient, body: &Value) -> TestResponse {
    client.create_temporal_entity(body).await.expect("create temporal entity request")
}

/// The Temporal Evolution of `id`.
async fn temporal(client: &TestClient, id: &str) -> TestResponse {
    client
        .get_temporal_entity(id, WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request")
}

/// Clause 5.6.11: an observation of every shape at the limit is recorded, and
/// both the Temporal Evolution and the entity read it back as written.
#[tokio::test]
async fn regression_r60_every_shape_at_the_limit_is_recorded_and_reads_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (case, p) in shapes(LIMIT) {
        let id = scope.id(&format!("NestingProbe:temporal-at-limit-{case}"));
        let created = create(client, &evolution(&id, &p)).await;
        assert_eq!(created.status, StatusCode::CREATED, "{case}: {:?}", created.body);
        let history = temporal(client, &id).await;
        assert_eq!(history.status, StatusCode::OK, "{case}: {:?}", history.body);
        let history = history.json_value().expect("a temporal answer");
        for member in ["value", "valueList", "json", "s"] {
            assert_eq!(history["p"][0][member], p[member], "{case} {member}: {history}");
        }
        let entity = read(client, &id, "").await;
        assert_eq!(entity["p"]["value"], p["value"], "{case}: {entity}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// An observation of 61 levels was recorded, and its entity could never be
/// read again. It is refused, and nothing is recorded.
#[tokio::test]
async fn regression_r60_an_observation_past_the_old_read_bound_is_refused_not_recorded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:temporal-past-read-bound");

    let created = create(client, &evolution(&id, &json!({"type": "Property", "value": object_chain(61)}))).await;
    assert_refused(&created, "61 levels");
    assert_eq!(temporal(client, &id).await.status, StatusCode::NOT_FOUND);
    assert_absent(client, &id).await;

    scope.finish().await.expect("environment cleanup");
}

/// Every shape one level past the limit is refused, and nothing is recorded.
#[tokio::test]
async fn regression_r60_every_shape_past_the_limit_is_refused_by_a_temporal_create() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (case, p) in shapes(LIMIT + 1) {
        let id = scope.id(&format!("NestingProbe:temporal-past-limit-{case}"));
        let created = create(client, &evolution(&id, &p)).await;
        assert_refused(&created, case);
        assert_eq!(temporal(client, &id).await.status, StatusCode::NOT_FOUND, "{case}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.12 and 5.6.14: adding an instance past the limit, or replacing
/// an instance with one, is refused, and the evolution keeps what it held.
#[tokio::test]
async fn regression_r60_adding_or_modifying_an_instance_past_the_limit_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:temporal-existing");
    let shallow = json!({"type": "Property", "value": object_chain(1)});
    let created = create(client, &evolution(&id, &shallow)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let encoded = urlencoding::encode(&id);

    let deep = json!({"type": "Property", "value": object_chain(LIMIT + 1)});
    let added = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entities/{encoded}/attrs"))
        .header("content-type", "application/ld+json")
        .json(&json!({"p": observation(&deep), "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("temporal add attributes request");
    assert_refused(&added, "add attributes");

    let history = temporal(client, &id).await.json_value().expect("a temporal answer");
    assert_eq!(history["p"].as_array().map(Vec::len), Some(1), "{history}");
    let instance = history["p"][0]["instanceId"].as_str().expect("the instance identifier").to_owned();
    let modified = client
        .raw(
            Method::PATCH,
            &format!("/ngsi-ld/v1/temporal/entities/{encoded}/attrs/p/{}", urlencoding::encode(&instance)),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({"p": [deep], "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("modify attribute instance request");
    assert_refused(&modified, "modify attribute instance");
    let history = temporal(client, &id).await.json_value().expect("a temporal answer");
    assert_eq!(history["p"][0]["value"], object_chain(1), "{history}");

    scope.finish().await.expect("environment cleanup");
}

/// A chain of sub-attributes at the limit is recorded as observation metadata
/// and reads back.
#[tokio::test]
async fn regression_r60_a_chain_of_sub_attributes_at_the_limit_is_recorded_and_reads_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("NestingProbe:temporal-sub-chain");
    let p = sub_attribute_chain(LIMIT);

    let created = create(client, &evolution(&id, &p)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let history = temporal(client, &id).await.json_value().expect("a temporal answer");
    assert_eq!(history["p"][0]["s"], p["s"], "{history}");

    scope.finish().await.expect("environment cleanup");
}
