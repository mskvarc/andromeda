//! What a Context Source Registration Subscription carries that its subscriber
//! did not write.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.12-1 gives a Subscription an optional
//! `notificationTrigger` and says "If not present, the default is the
//! combination 'attributeCreated' and 'attributeUpdated'". The triggers it
//! defines are entity-shaped -- `entityCreated`, `entityUpdated`,
//! `entityDeleted`, `attributeCreated`, `attributeUpdated`,
//! `attributeDeleted` -- and they select which changes to a subscribed *Entity*
//! notify.
//!
//! Clause 5.11.2.3 takes that same data type for a Context Source Registration
//! Subscription, but clause 5.11.2.4 gives the operation its own notification
//! rule that names no trigger at all: "initially on subscription and whenever
//! there is a change of a matching Context Source Registration (creation,
//! update, deletion), implementations shall post a new `CsourceNotification` to
//! the endpoint specified in the notification parameters informing about this
//! change by providing the Context Source Registration(s) together with the
//! appropriate trigger reason in the triggerReason member". Clause 5.3.2's
//! `triggerReason` is what reports the change; no notification trigger selects
//! one. Defaulting the member there asserted a selection the operation does not
//! make.
//!
//! The broker materialised the default at creation, for both kinds. It is now
//! stored as written and supplied where clause 5.2.12 defines it, which is the
//! Subscription of clause 5.8 -- so an entity subscription still reads back the
//! default the table names.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use urlencoding::encode;

/// A subscription body both resources accept, carrying whatever triggers the
/// caller names.
fn subscription(id: &str, triggers: Option<Value>) -> Value {
    let mut body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    });
    if let Some(triggers) = triggers {
        body["notificationTrigger"] = triggers;
    }
    body
}

/// Create one Context Source Registration Subscription.
async fn create_csource_subscription(client: &TestClient, body: &Value) -> TestResponse {
    client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create context source subscription")
}

/// Read one Context Source Registration Subscription back.
async fn retrieve_csource_subscription(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .raw(Method::GET, &format!("csourceSubscriptions/{}", encode(id)))
        .send()
        .await
        .expect("retrieve context source subscription");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a retrieved subscription is JSON")
}

/// Clause 5.11.2.4 notifies on every change to a matching registration and
/// reports which one in `triggerReason`, so a Context Source Registration
/// Subscription that named no `notificationTrigger` reads back without one.
#[tokio::test]
async fn regression_csource_subscription_no_notification_trigger_is_invented() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:csource-no-trigger");

    let created = create_csource_subscription(client, &subscription(&id, None)).await;
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let stored = retrieve_csource_subscription(client, &id).await;
    assert!(
        stored.get("notificationTrigger").is_none(),
        "clause 5.11.2.4 selects no trigger, so none is stored: {stored}"
    );

    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&id)))
        .send()
        .await
        .expect("delete context source subscription");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1 gives the member cardinality 0..1 on the data type clause
/// 5.11.2.3 takes, so a subscriber that does name triggers reads them back.
#[tokio::test]
async fn regression_csource_subscription_a_written_notification_trigger_survives() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:csource-written-trigger");

    let created = create_csource_subscription(client, &subscription(&id, Some(json!(["entityDeleted"])))).await;
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let stored = retrieve_csource_subscription(client, &id).await;
    assert_eq!(
        stored["notificationTrigger"],
        json!(["entityDeleted"]),
        "what was written is what is served: {stored}"
    );

    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(&id)))
        .send()
        .await
        .expect("delete context source subscription");
    scope.finish().await.expect("cleanup failed");
}

/// The Subscription of clause 5.8 is where Table 5.2.12-1's default belongs,
/// and it still reads back: the two resources are told apart, not levelled.
#[tokio::test]
async fn regression_csource_subscription_an_entity_subscription_still_reads_the_default() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:entity-default-trigger");

    let created = client.create_subscription(&subscription(&id, None)).await.expect("create subscription");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let stored = client
        .retrieve_subscription(&id, Some(CORE_CONTEXT))
        .await
        .expect("retrieve subscription")
        .json_value()
        .expect("a retrieved subscription is JSON");
    assert_eq!(
        stored["notificationTrigger"],
        json!(["attributeCreated", "attributeUpdated"]),
        "Table 5.2.12-1 names the default combination: {stored}"
    );

    client.delete_subscription(&id).await.expect("delete subscription");
    scope.finish().await.expect("cleanup failed");
}
