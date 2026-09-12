//! Which `@context` a subscription's notifications carry when the subscription
//! named no `jsonldContext` of its own.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.2.4: "The @context to be used for sending
//! Notifications related to this Subscription shall be the one specified in the
//! jsonldContext field. If not present, the jsonldContext field shall be
//! initialized with the @context applicable for the Subscription (if @context
//! is also not present in the Subscription, see clause 5.5.5)." So the
//! precedence is `jsonldContext` member, then the creation `@context`, then the
//! core context; a subscription created with a `Link`-header `@context` and no
//! `jsonldContext` member must notify under that creation context, not core.
//! Andromeda read the field only from an explicit `jsonldContext` member and
//! fell straight to the core context, so its notifications advertised the wrong
//! `@context` and expanded terms.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::{CORE_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// A Building carrying one attribute, created under the test-suite context.
fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Tour Eiffel"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A subscription body with no `@context` member and no `jsonldContext` member,
/// so its creation context can only come from the request `Link` header.
fn subscription(id: &str, entity_id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": "Building"}],
        "notification": {"endpoint": {"uri": endpoint, "accept": "application/json"}}
    })
}

/// Create the subscription, supplying its `@context` only through the `Link`
/// header (clause 6.3.5, the `application/json` case).
async fn subscribe_with_link_context(client: &TestClient, body: &Value) {
    let created = client
        .raw(Method::POST, "/ngsi-ld/v1/subscriptions")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .json(body)
        .send()
        .await
        .expect("create subscription request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Clause 5.8.2.4: the subscription's `jsonldContext` is initialized with the
/// creation `@context` when the request named none, so a later retrieval shows
/// the creation context, not core.
#[tokio::test]
async fn regression_notification_creation_context_initializes_jsonld_context_from_the_link_header() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:creation-context-init");
    let subscription_id = scope.id("Subscription:creation-context-init");

    let create_entity = client.post_entity(&building(&entity_id)).send().await.expect("create entity request");
    assert_eq!(create_entity.status, StatusCode::CREATED, "create entity: {:?}", create_entity.body);
    subscribe_with_link_context(client, &subscription(&subscription_id, &entity_id, &receiver.endpoint_url())).await;

    let retrieved = client
        .retrieve_subscription(&subscription_id, Some(CORE_CONTEXT))
        .await
        .expect("retrieve subscription");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve subscription: {:?}", retrieved.body);
    let subscription = retrieved.json_value().expect("a subscription retrieval answers JSON");
    assert_eq!(
        subscription["jsonldContext"],
        json!(TEST_SUITE_CONTEXT.as_str()),
        "jsonldContext is initialized with the creation context: {subscription}"
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// The same subscription's notifications advertise the creation context in the
/// `Link` header (clause 5.8.6, "the one specified in the jsonldContext
/// field"), not the core context.
#[tokio::test]
async fn regression_notification_creation_context_notifies_under_the_creation_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:creation-context-notify");
    let subscription_id = scope.id("Subscription:creation-context-notify");

    let create_entity = client.post_entity(&building(&entity_id)).send().await.expect("create entity request");
    assert_eq!(create_entity.status, StatusCode::CREATED, "create entity: {:?}", create_entity.body);
    subscribe_with_link_context(client, &subscription(&subscription_id, &entity_id, &receiver.endpoint_url())).await;

    let updated = client
        .append_entity_attrs(
            &entity_id,
            &json!({"name": {"type": "Property", "value": "Le Tour Eiffel"}, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("append attributes");
    assert!(updated.status.is_success(), "append: {:?}", updated.body);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let link = notification
        .headers
        .get("link")
        .expect("an application/json notification carries its @context in the Link header")
        .to_str()
        .expect("the Link header is valid text");
    assert!(
        link.contains(TEST_SUITE_CONTEXT.as_str()),
        "the notification advertises the creation context, not core alone: {link}"
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
