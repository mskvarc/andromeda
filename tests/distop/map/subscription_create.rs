//! Subscription creation: validation, defaults, and stored members over a valid
//! baseline (clauses 5.8.1, 5.2.12). All `T-LOCAL`; the notification-context row
//! additionally starts a notification receiver to own a resolvable endpoint.

use crate::{
    map::subscription_document::{set_watched_attributes, subscription},
    notification_adapter::NotificationAdapter,
    oracle::{assert_problem_details, assert_status, json_body, location},
    topology::DistributedTopology,
};
use andromeda_test_fixtures::{
    context::COMPOUND_CONTEXT,
    problem_detail::{ALREADY_EXISTS, BAD_REQUEST_DATA, LD_CONTEXT_NOT_AVAILABLE},
};
use serde_json::json;

/// A never-contacted notification sink for creation rows that only assert on the
/// stored subscription, never on delivery.
const INERT_ENDPOINT: &str = "http://127.0.0.1:9/notify";

/// A body missing the mandatory `notification` member is rejected with
/// `400 BadRequestData` (clause 5.8.1). Confirms REQ-MAP-1.
#[tokio::test]
async fn sub_create_malformed_body_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:malformed");
    let body = json!({ "id": id, "type": "Subscription", "@context": COMPOUND_CONTEXT.as_str() });
    let response = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Re-creating an existing subscription id is rejected `409 AlreadyExists`
/// (clause 5.8.1). Confirms REQ-MAP-2.
#[tokio::test]
async fn sub_create_duplicate_id_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:duplicate");
    let body = subscription(&id, INERT_ENDPOINT);
    let first = topology.front().client().create_subscription(&body).await.expect("first create");
    assert_status(&first, 201);
    let second = topology.front().client().create_subscription(&body).await.expect("second create");
    assert_status(&second, 409);
    assert_problem_details(&second, ALREADY_EXISTS);
    topology.finish().await.expect("topology cleanup");
}

/// A subscription created without an `id` is assigned a generated URI returned in
/// `Location` (clause 5.8.1). Confirms REQ-MAP-3, REQ-MAP-36.
#[tokio::test]
async fn sub_create_without_id_returns_generated_uri() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mut body = subscription(&topology.id("Subscription:generated"), INERT_ENDPOINT);
    body.as_object_mut()
        .unwrap_or_else(|| unreachable!("subscription baseline is an object"))
        .remove("id");
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let generated = location(&created)
        .rsplit('/')
        .next()
        .unwrap_or_else(|| unreachable!("Location has a final segment"))
        .to_owned();
    assert!(generated.starts_with("urn:"), "generated subscription id is a URI: {generated}");
    topology
        .front()
        .client()
        .delete_subscription(&generated)
        .await
        .expect("delete generated subscription");
    topology.finish().await.expect("topology cleanup");
}

/// A future `expiresAt` is accepted and stored (clause 5.2.12). Confirms REQ-MAP-5.
#[tokio::test]
async fn sub_create_with_expires_at_stores_expiration() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:expires");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["expiresAt"] = json!("2999-01-01T00:00:00Z");
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology
        .front()
        .client()
        .without_default_context()
        .retrieve_subscription(&id, None)
        .await
        .expect("retrieve subscription under the core context");
    assert_eq!(
        json_body(&retrieved)["expiresAt"].as_str(),
        Some("2999-01-01T00:00:00Z"),
        "expiresAt stored verbatim"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A past `expiresAt` is rejected with `400 BadRequestData` (clause 5.8.1).
/// Confirms REQ-MAP-6.
#[tokio::test]
async fn sub_create_past_expires_at_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:past-expires");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["expiresAt"] = json!("2000-01-01T00:00:00Z");
    let response = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A subscription without `expiresAt` is perpetual: no expiration is stored
/// (clause 5.2.12). Confirms REQ-MAP-7.
#[tokio::test]
async fn sub_create_without_expires_at_is_perpetual() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:perpetual");
    let body = subscription(&id, INERT_ENDPOINT);
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_subscription(&id, None).await.expect("retrieve subscription");
    assert!(json_body(&retrieved)["expiresAt"].is_null(), "a perpetual subscription stores no expiresAt");
    topology.finish().await.expect("topology cleanup");
}

/// An absent `isActive` yields `status` `active` (clause 5.2.12). Confirms REQ-MAP-8.
#[tokio::test]
async fn sub_create_is_active_absent_or_true_status_active() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:active");
    let body = subscription(&id, INERT_ENDPOINT);
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_subscription(&id, None).await.expect("retrieve subscription");
    assert_eq!(
        json_body(&retrieved)["status"].as_str(),
        Some("active"),
        "an active subscription reports status active"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `isActive` false yields `status` `paused` (clause 5.2.12). Confirms REQ-MAP-9.
#[tokio::test]
async fn sub_create_is_active_false_status_paused() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:paused");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["isActive"] = json!(false);
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_subscription(&id, None).await.expect("retrieve subscription");
    assert_eq!(
        json_body(&retrieved)["status"].as_str(),
        Some("paused"),
        "an isActive:false subscription reports status paused"
    );
    topology.finish().await.expect("topology cleanup");
}

/// `watchedAttributes` terms are expanded and stored as IRIs (clause 5.8.1).
/// Retrieving under the core context alone (clause 6.3.5: a GET without a
/// `Link` header uses the default context, so the read-back must not carry
/// the topology client's compound `Link`) leaves the compound-context term
/// uncompacted, so its expanded IRI is visible. Confirms REQ-MAP-11.
#[tokio::test]
async fn sub_create_watched_attributes_expanded_and_stored() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:watched");
    let mut body = subscription(&id, INERT_ENDPOINT);
    set_watched_attributes(&mut body, &["speed"]);
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology
        .front()
        .client()
        .without_default_context()
        .retrieve_subscription(&id, None)
        .await
        .expect("retrieve subscription");

    let watched = json_body(&retrieved)["watchedAttributes"]
        .as_array()
        .unwrap_or_else(|| unreachable!("watchedAttributes round-trips"))
        .clone();
    assert!(
        watched.iter().filter_map(|value| value.as_str()).any(|term| term.starts_with("http")),
        "watchedAttributes are stored expanded: {watched:?}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A subscription whose `jsonldContext` resolves is accepted and stores that
/// context (clause 5.8.1). Confirms REQ-MAP-14.
#[tokio::test]
async fn sub_create_notification_context_resolves_from_jsonld_context() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:notif-context");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["jsonldContext"] = json!(COMPOUND_CONTEXT.as_str());
    let created = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_subscription(&id, None).await.expect("retrieve subscription");
    assert_eq!(
        json_body(&retrieved)["jsonldContext"].as_str(),
        Some(COMPOUND_CONTEXT.as_str()),
        "the resolvable notification context is stored"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An unreachable `jsonldContext` is rejected `LdContextNotAvailable`, mapped to `504` by Table 6.3.2-1,
/// (clause 5.8.1). Confirms REQ-MAP-15.
#[tokio::test]
async fn sub_create_unreachable_jsonld_context_returns_504() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:unreachable-context");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["jsonldContext"] = json!("http://127.0.0.1:9/context.jsonld");
    let response = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&response, 504);
    assert_problem_details(&response, LD_CONTEXT_NOT_AVAILABLE);
    topology.finish().await.expect("topology cleanup");
}

/// A structurally invalid `jsonldContext` is rejected `400 BadRequestData`
/// (clause 5.8.1). Confirms REQ-MAP-16.
#[tokio::test]
async fn sub_create_invalid_jsonld_context_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("Subscription:invalid-context");
    let mut body = subscription(&id, INERT_ENDPOINT);
    body["jsonldContext"] = json!(42);
    let response = topology.front().client().create_subscription(&body).await.expect("create subscription");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}
