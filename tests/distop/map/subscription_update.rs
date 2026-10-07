//! Subscription update and delete: validation, partial-merge semantics, the
//! `isActive`/`expiresAt` interplay that decides `status`, and the success codes
//! (clauses 5.8.1, 5.5.8, 5.2.12). All `T-LOCAL`.

use crate::{
    map::subscription_document::subscription,
    oracle::{assert_problem_details, assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, LD_CONTEXT_NOT_AVAILABLE, RESOURCE_NOT_FOUND},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A never-contacted notification sink for validation rows.
const INERT_ENDPOINT: &str = "http://127.0.0.1:9/notify";

/// Create a baseline subscription and return its id.
async fn create_baseline(topology: &MultiBrokerTopology<BrokerRole>, local: &str) -> String {
    let id = topology.id(local);
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_subscription(&subscription(&id, INERT_ENDPOINT))
        .await
        .expect("create baseline subscription");
    assert_status(&created, 201);
    id
}

/// The stored representation of a subscription retrieved under the core context.
async fn stored(topology: &MultiBrokerTopology<BrokerRole>, id: &str) -> Value {
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_subscription(id, None)
        .await
        .expect("retrieve subscription");
    assert_status(&retrieved, 200);
    json_body(&retrieved)
}

/// Updating an invalid subscription id is rejected `400 BadRequestData`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription("not a uri", &json!({ "description": "x" }), None)
        .await
        .expect("update subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Updating an unknown subscription is rejected `404 ResourceNotFound`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_unknown_target_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:unknown-update");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "description": "x" }), None)
        .await
        .expect("update subscription");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// A well-formed fragment passes JSON-LD validation and updates successfully
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_performs_jsonld_validation() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:jsonld-valid").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "q": "speed>10" }), Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("update subscription");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A fragment violating the data model is rejected `400 BadRequestData`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_fragment_violating_data_model_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:bad-model").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "entities": "not-an-array" }), None)
        .await
        .expect("update subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Update expands attribute terms, so a compound-context `watchedAttributes`
/// term is stored as an IRI (clause 5.8.1; term expansion per clause 5.5.7).
/// The read-back deliberately names no context: under the core context alone
/// the stored IRI cannot compact back to `speed`, which is what proves it was
/// expanded on update.
#[tokio::test]
async fn sub_update_expands_attribute_terms() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:expand-update").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "watchedAttributes": ["speed"] }), Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("update subscription");
    assert_status(&response, 204);
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .without_default_context()
        .retrieve_subscription(&id, None)
        .await
        .expect("retrieve subscription under the core context");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    let watched = body["watchedAttributes"].as_array().unwrap_or_else(|| panic!("watchedAttributes present"));
    assert!(
        watched.iter().filter_map(Value::as_str).any(|term| term.starts_with("http")),
        "updated watchedAttributes stored expanded: {watched:?}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// An unreachable `@context` in an update is rejected `LdContextNotAvailable`,
/// which the HTTP binding maps to `504` (clause 5.8.1; Table 6.3.2-1 maps
/// `LdContextNotAvailable` to 504).
#[tokio::test]
async fn sub_update_unreachable_jsonld_context_returns_504() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:unreachable-update").await;
    let body = json!({ "watchedAttributes": ["speed"], "@context": "http://127.0.0.1:9/context.jsonld" });
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/subscriptions/{}", urlencoding::encode(&id)))
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("update subscription");
    assert_status(&response, 504);
    assert_problem_details(&response, LD_CONTEXT_NOT_AVAILABLE);
    topology.finish().await.expect("topology cleanup");
}

/// A structurally invalid `@context` in an update is rejected `400 BadRequestData`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_invalid_jsonld_context_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:invalid-update").await;
    let body = json!({ "watchedAttributes": ["speed"], "@context": 42 });
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/subscriptions/{}", urlencoding::encode(&id)))
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("update subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A partial update merges: an unmentioned member survives (clause 5.5.8).
#[tokio::test]
async fn sub_update_partial_merge_per_558() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:partial-merge");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["description"] = json!("original");
    body["q"] = json!("speed>10");
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_subscription(&body)
        .await
        .expect("create subscription");
    assert_status(&created, 201);
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "description": "changed" }), None)
        .await
        .expect("update subscription");
    assert_status(&response, 204);
    let merged = stored(&topology, &id).await;
    assert_eq!(merged["description"].as_str(), Some("changed"), "the provided member is updated");
    assert_eq!(merged["q"].as_str(), Some("speed>10"), "the unmentioned member survives the partial update");
    topology.finish().await.expect("topology cleanup");
}

/// `isActive` true with no `expiresAt` yields `status` `active` (clause 5.2.12).
#[tokio::test]
async fn sub_update_is_active_true_no_expires_at_status_active_unless_expired() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:reactivate-active").await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "isActive": false }), None)
            .await
            .expect("pause"),
        204,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "isActive": true }), None)
            .await
            .expect("activate"),
        204,
    );
    assert_eq!(
        stored(&topology, &id).await["status"].as_str(),
        Some("active"),
        "isActive:true with no expiry is active"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `isActive` true with a future `expiresAt` yields `status` `active`
/// (clause 5.2.12).
#[tokio::test]
async fn sub_update_is_active_true_future_expires_at_status_active() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:future-active").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "isActive": true, "expiresAt": "2999-01-01T00:00:00Z" }), None)
        .await
        .expect("update");
    assert_status(&response, 204);
    assert_eq!(
        stored(&topology, &id).await["status"].as_str(),
        Some("active"),
        "isActive:true with a future expiry is active"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `isActive` false with no `expiresAt` yields `status` `paused` (clause 5.2.12).
#[tokio::test]
async fn sub_update_is_active_false_no_expires_at_status_paused_unless_expired() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:pause").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "isActive": false }), None)
        .await
        .expect("update");
    assert_status(&response, 204);
    assert_eq!(
        stored(&topology, &id).await["status"].as_str(),
        Some("paused"),
        "isActive:false with no expiry is paused"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Setting a future `expiresAt` on an already-active subscription keeps it
/// active (clause 5.2.12).
#[tokio::test]
async fn sub_update_future_expires_at_only_reactivates_expired() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:future-expiry").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "expiresAt": "2999-01-01T00:00:00Z" }), None)
        .await
        .expect("update");
    assert_status(&response, 204);
    assert_eq!(
        stored(&topology, &id).await["status"].as_str(),
        Some("active"),
        "a future expiry leaves an active subscription active"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A past `expiresAt` in an update is rejected `400 BadRequestData`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_update_past_expires_at_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:past-update").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "expiresAt": "2000-01-01T00:00:00Z" }), None)
        .await
        .expect("update");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A successful update returns `204 No Content` (clause 5.8.1).
#[tokio::test]
async fn sub_update_success_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:update-204").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .update_subscription(&id, &json!({ "description": "updated" }), None)
        .await
        .expect("update");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an invalid subscription id is rejected `400 BadRequestData`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_delete_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_subscription("not a uri")
        .await
        .expect("delete subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an unknown subscription is rejected `404 ResourceNotFound`
/// (clause 5.8.1).
#[tokio::test]
async fn sub_delete_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:unknown-delete");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_subscription(&id)
        .await
        .expect("delete subscription");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// A successful delete returns `204 No Content` (clause 5.8.1).
#[tokio::test]
async fn sub_delete_success_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = create_baseline(&topology, "Subscription:delete-204").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_subscription(&id)
        .await
        .expect("delete subscription");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}
