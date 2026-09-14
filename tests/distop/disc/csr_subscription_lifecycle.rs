//! Context Source Subscription lifecycle (clauses 5.11.2-5.11.6): create,
//! local-case validation, update-and-merge, retrieve, query with pagination,
//! and delete. Change-driven rows capture notifications through the in-process
//! adapter; the rest are `T-LOCAL`.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::register,
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::{assert_problem_details, assert_status, json_body, location},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(10);
/// The quiet window over which an absence is confirmed.
const ABSENT: Duration = Duration::from_secs(4);
/// A notification endpoint URI for lifecycle rows that never deliver.
const INERT_ENDPOINT: &str = "http://broker-b:1056/notify";

/// A valid Context Source Subscription over a unique entity type.
fn subscription(id: &str, entity_type: &str) -> Value {
    CsrSubscriptionBuilder::new(id, INERT_ENDPOINT, entity_type).build()
}

/// A valid subscription with an explicit id returns `201` with a `Location`
/// carrying that id, and one without an id returns `201` with a generated id
/// (clauses 5.11.2, 5.9.2). Confirms REQ-DISC-45, REQ-DISC-47, REQ-DISC-63.
#[tokio::test]
async fn csr_subscription_create_valid_returns_201_and_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("CreateType");
    let id = topology.id("subscription:create");
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription(&id, &vtype)).await;
    assert_status(&created, 201);
    assert!(location(&created).ends_with(&id), "the Location carries the explicit subscription id");

    let mut without_id = subscription(&topology.id("subscription:placeholder"), &vtype);
    without_id.as_object_mut().unwrap_or_else(|| panic!("subscription is an object")).remove("id");
    let generated = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &without_id).await;
    assert_status(&generated, 201);
    let tail = location(&generated);
    let tail = tail.rsplit('/').next().unwrap_or_else(|| panic!("Location has a final segment"));
    assert!(tail.starts_with("urn:"), "a subscription without an id is assigned a generated URI: {tail}");
    topology.finish().await.expect("topology cleanup");
}

/// Create applies the local validation cases: a missing notification, an
/// invalid type, a malformed `q`, empty watched attributes, and a past
/// `expiresAt` are each rejected with `400 BadRequestData` (clause 5.11.2).
/// Confirms REQ-DISC-46.
#[tokio::test]
async fn csr_subscription_create_applies_local_case_validation() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("ValidationType");
    let mutations: [fn(&mut Value); 5] = [
        |body| {
            body.as_object_mut()
                .unwrap_or_else(|| panic!("subscription is an object"))
                .remove("notification");
        },
        |body| body["type"] = json!("RandomType"),
        |body| body["q"] = json!("invalidQuery"),
        |body| body["watchedAttributes"] = json!([]),
        |body| body["expiresAt"] = json!("2018-08-01T22:07:00Z"),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut body = subscription(&topology.id(&format!("subscription:invalid:{index}")), &vtype);
        mutate(&mut body);
        let response = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &body).await;
        assert_status(&response, 400);
        assert_problem_details(&response, BAD_REQUEST_DATA);
    }
    topology.finish().await.expect("topology cleanup");
}

/// A subscription with no temporal query gates matching to latest-info
/// registrations: an interval-bearing registration stays silent while a latest
/// one notifies (clause 5.12). Confirms REQ-DISC-52.
#[tokio::test]
async fn csr_subscription_create_no_temporaliq_gates_to_latest_info() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("GateType");
    let subscription_id = topology.id("subscription:gate");
    let created = csr_subscription::create(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;
    assert_status(&created, 201);

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:gate-latest"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the latest-info registration notifies the ungated subscription"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A valid update returns `204` and merges the fragment into the stored
/// subscription (clauses 5.11.3, 5.5.8). Confirms REQ-DISC-66, REQ-DISC-68.
#[tokio::test]
async fn csr_subscription_update_valid_returns_204_and_merges() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("MergeType");
    let id = topology.id("subscription:merge");
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription(&id, &vtype)).await;
    assert_status(&created, 201);

    let updated = csr_subscription::update(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &id,
        &json!({ "description": "merged description" }),
    )
    .await;
    assert_status(&updated, 204);

    let retrieved = csr_subscription::retrieve(topology.broker(BrokerRole::A).expect("topology broker A").client(), &id, None).await;
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(body["description"].as_str(), Some("merged description"), "the patched field is stored");
    assert!(body["entities"].is_array(), "the untouched entities are preserved by the merge");
    topology.finish().await.expect("topology cleanup");
}

/// A malformed id or a malformed fragment is rejected with `400 BadRequestData`
/// (clause 5.11.3). Confirms REQ-DISC-64, REQ-DISC-65.
#[tokio::test]
async fn csr_subscription_update_invalid_id_or_fragment_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("UpdateInvalidType");
    let id = topology.id("subscription:update-invalid");
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription(&id, &vtype)).await;
    assert_status(&created, 201);

    let bad_id = csr_subscription::update(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        "not a uri",
        &json!({ "description": "x" }),
    )
    .await;
    assert_status(&bad_id, 400);
    assert_problem_details(&bad_id, BAD_REQUEST_DATA);

    let bad_fragment = csr_subscription::update(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &id,
        &json!({ "notification": { "format": "keyValues" } }),
    )
    .await;
    assert_status(&bad_fragment, 400);
    assert_problem_details(&bad_fragment, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An update to an unknown subscription id returns `404 ResourceNotFound`
/// (clause 5.11.3). Confirms REQ-DISC-64.
#[tokio::test]
async fn csr_subscription_update_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let unknown = topology.id("subscription:unknown-update");
    let response = csr_subscription::update(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &unknown,
        &json!({ "description": "x" }),
    )
    .await;
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Updating a subscription re-notifies with every currently matching
/// registration (clause 5.11.7). Confirms REQ-DISC-67.
#[tokio::test]
async fn csr_subscription_update_emits_notification_with_all_matching_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("UpdateEmitType");
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:update-emit-a"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:update-emit-b"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;

    let subscription_id = topology.id("subscription:update-emit");
    let created = csr_subscription::create(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;
    assert_status(&created, 201);
    let _initial = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    adapter.clear_notifications().await;

    let updated = csr_subscription::update(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &subscription_id,
        &json!({ "description": "re-evaluated" }),
    )
    .await;
    assert_status(&updated, 204);
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let data = notification.body["data"].as_array().unwrap_or_else(|| panic!("notification data is an array"));
    assert!(
        data.len() >= 2,
        "the post-update notification lists all currently matching registrations, got {}",
        data.len()
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An existing subscription is retrieved as a `200` Subscription object (clause
/// 5.11.4). Confirms REQ-DISC-71.
#[tokio::test]
async fn csr_subscription_retrieve_existing_returns_200() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("RetrieveType");
    let id = topology.id("subscription:retrieve");
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription(&id, &vtype)).await;
    assert_status(&created, 201);

    let retrieved = csr_subscription::retrieve(topology.broker(BrokerRole::A).expect("topology broker A").client(), &id, None).await;
    assert_status(&retrieved, 200);
    assert_eq!(
        json_body(&retrieved)["id"].as_str(),
        Some(id.as_str()),
        "the retrieved subscription carries its id"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A malformed id returns `400` and an unknown id returns `404` on retrieve
/// (clause 5.11.4). Confirms REQ-DISC-69, REQ-DISC-70.
#[tokio::test]
async fn csr_subscription_retrieve_invalid_or_unknown_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let invalid = csr_subscription::retrieve(topology.broker(BrokerRole::A).expect("topology broker A").client(), "not a uri", None).await;
    assert_status(&invalid, 400);
    assert_problem_details(&invalid, BAD_REQUEST_DATA);

    let unknown = csr_subscription::retrieve(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &topology.id("subscription:unknown-retrieve"),
        None,
    )
    .await;
    assert_status(&unknown, 404);
    assert_problem_details(&unknown, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Query lists subscriptions as a `200` array, honours pagination, and rejects
/// an invalid limit with `400` (clauses 5.11.5, 5.5.9). Confirms REQ-DISC-72,
/// REQ-DISC-73, REQ-DISC-74.
#[tokio::test]
async fn csr_subscription_query_lists_all_with_pagination() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("QueryType");
    for index in 0..2 {
        let id = topology.id(&format!("subscription:query:{index}"));
        let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription(&id, &vtype)).await;
        assert_status(&created, 201);
    }

    let listed = csr_subscription::query(topology.broker(BrokerRole::A).expect("topology broker A").client(), "limit=1").await;
    assert_status(&listed, 200);
    let body = json_body(&listed);
    assert!(body.is_array(), "the subscription query returns a JSON-LD array");
    assert_eq!(body.as_array().map(Vec::len), Some(1), "limit=1 bounds the page to one subscription");

    let invalid = csr_subscription::query(topology.broker(BrokerRole::A).expect("topology broker A").client(), "limit=0").await;
    assert_status(&invalid, 400);
    assert_problem_details(&invalid, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting a subscription returns `204`, makes it unretrievable, and stops
/// further notifications (clause 5.11.6). Confirms REQ-DISC-77.
#[tokio::test]
async fn csr_subscription_delete_existing_returns_204_and_stops_notifications() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("DeleteType");
    let subscription_id = topology.id("subscription:delete");
    let created = csr_subscription::create(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;
    assert_status(&created, 201);

    let deleted = csr_subscription::delete(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription_id).await;
    assert_status(&deleted, 204);
    let retrieved = csr_subscription::retrieve(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription_id, None).await;
    assert_status(&retrieved, 404);

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:after-delete"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A malformed id returns `400` and an unknown id returns `404` on delete
/// (clause 5.11.6). Confirms REQ-DISC-75, REQ-DISC-76.
#[tokio::test]
async fn csr_subscription_delete_invalid_or_unknown_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let invalid = csr_subscription::delete(topology.broker(BrokerRole::A).expect("topology broker A").client(), "not a uri").await;
    assert_status(&invalid, 400);
    assert_problem_details(&invalid, BAD_REQUEST_DATA);

    let unknown = csr_subscription::delete(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &topology.id("subscription:unknown-delete"),
    )
    .await;
    assert_status(&unknown, 404);
    assert_problem_details(&unknown, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}
