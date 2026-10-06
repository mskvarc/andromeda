//! A `null` in a Subscription update fragment deletes the member it names.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.2.4 modifies the target Subscription "as
//! mandated by clause 5.5.8", whose Partial Update Patch Behaviour reads: "For
//! each member of the Fragment, whose value is an NGSI-LD Null, contained by the
//! target, the target member is deleted." Table 5.2.12-1 gives `expiresAt`
//! cardinality `0..1`, so deleting it leaves a subscription that never expires.
//!
//! Before this suite existed the broker read a `null` `expiresAt` (and every
//! other scalar member) as an absent one, so `PATCH {"expiresAt": null}` was
//! answered `204 No Content` and the expiry silently stayed.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// An `expiresAt` far enough ahead that the expiry sweep never reaches it.
const FAR_FUTURE: &str = "2099-01-01T00:00:00Z";

/// A subscription that expires at [`FAR_FUTURE`].
fn expiring_subscription(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "subscriptionName": "expiring",
        "entities": [{"type": entity_type}],
        "expiresAt": FAR_FUTURE,
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Clause 5.5.8: a `null` `expiresAt` deletes the expiry, so the subscription
/// stays active and a retrieval no longer carries the member, while the members
/// the fragment does not name are kept.
#[tokio::test]
async fn regression_subscription_a_null_expires_at_makes_the_subscription_perpetual() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:null-expires-at");

    let created = client
        .create_subscription(&expiring_subscription(&id, &scope.entity_type("ExpiryProbe")))
        .await
        .expect("create subscription request");
    let before = client.retrieve_subscription(&id, None).await.expect("retrieve subscription request");
    let updated = client
        .update_subscription(&id, &json!({"expiresAt": null}), Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("update subscription request");
    let after = client.retrieve_subscription(&id, None).await.expect("retrieve subscription request");
    let deleted = client.delete_subscription(&id).await.expect("delete subscription request");

    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let before = before.json_value().expect("a retrieved subscription is JSON");
    assert_eq!(before["expiresAt"], json!(FAR_FUTURE), "the expiry is stored: {before}");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "update: {:?}", updated.body);
    let after = after.json_value().expect("a retrieved subscription is JSON");
    assert!(after.get("expiresAt").is_none(), "the expiry is deleted: {after}");
    assert_eq!(after["status"], json!("active"), "a subscription without an expiry is active: {after}");
    assert_eq!(
        after["subscriptionName"],
        json!("expiring"),
        "a member the fragment does not name is kept: {after}"
    );
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1 gives `notification` cardinality `1`, so it cannot be
/// deleted: a `null` for it is `BadRequestData` (clause 5.5.4) and the stored
/// notification is untouched.
#[tokio::test]
async fn regression_subscription_a_null_notification_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:null-notification");

    let created = client
        .create_subscription(&expiring_subscription(&id, &scope.entity_type("ExpiryProbe")))
        .await
        .expect("create subscription request");
    let refused = client
        .update_subscription(&id, &json!({"notification": null}), Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("update subscription request");
    let after = client.retrieve_subscription(&id, None).await.expect("retrieve subscription request");
    let deleted = client.delete_subscription(&id).await.expect("delete subscription request");

    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "refused: {:?}", refused.body);
    let problem = refused.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");
    let after = after.json_value().expect("a retrieved subscription is JSON");
    assert_eq!(after["notification"]["endpoint"]["uri"], json!("http://127.0.0.1:9/notify"), "{after}");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);
    scope.finish().await.expect("cleanup failed");
}
