//! Clause 4.9 Query Terms answered by the in-memory evaluator rather than by
//! the PostgreSQL pushdown.
//!
//! A subscription's `q` is evaluated against the expanded notification payload,
//! never against a stored projection, so it is the one request that exercises
//! the in-memory reading of clause 4.9's target values end to end. The two list
//! families are the reason this suite exists: JSON-LD expansion wraps an ordered
//! `valueList` and `objectList` in a single `@list` node, an evaluator reading
//! one level too high resolved no target at all, and a subscription filtering on
//! either family therefore never fired — in both polarities, since a target that
//! resolves to nothing satisfies no operator.
//!
//! Every assertion states what ETSI GS CIM 009 v1.9.1 requires rather than what
//! the broker happened to do.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

/// How long the first notifications are waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a further notification is waited for once every expected one has
/// arrived, so a subscription that must not fire has had its chance to.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

/// How many deliveries are read before giving up on the ones under test.
const DELIVERIES_READ: usize = 12;

/// A `Building` carrying one target of every family whose clause 4.9 target
/// value is an array.
fn building_with_array_targets(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "roomTags": {"type": "ListProperty", "valueList": ["residential", "commercial"]},
        "roomLinks": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]},
        "roomVocab": {"type": "VocabProperty", "vocab": ["http://example.org/Male", "http://example.org/Other"]},
        "roomName": {"type": "Property", "value": "atrium"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one subscription over `Building` entities whose `q` is `filter`.
async fn create_filtered_subscription(scope: &TestEnvironment, receiver: &NotificationServer, local_id: &str, filter: &str) -> String {
    let id = scope.id(local_id);
    let subscription = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notificationTrigger": ["entityCreated", "attributeUpdated"],
        "q": filter,
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/json"
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);
    id
}

/// Every subscription that delivered a notification, read until `expected` of
/// them have arrived and then once more so a subscription that must not fire has
/// had its chance to.
async fn delivering_subscriptions(receiver: &NotificationServer, expected: usize) -> BTreeSet<String> {
    let mut delivered = BTreeSet::new();
    for _ in 0..DELIVERIES_READ {
        let timeout = if delivered.len() < expected { DELIVERY_TIMEOUT } else { SETTLE_TIMEOUT };
        let Ok(captured) = receiver.next_request(timeout).await else {
            break;
        };
        let body: Value = captured.json().expect("a notification body is JSON");
        let id = body["subscriptionId"].as_str().expect("a notification names its subscription");
        delivered.insert(id.to_owned());
    }
    delivered
}

/// Clause 4.9: "If the target element is a `ListProperty`, the target value is
/// defined as the valueList array associated to such a `ListProperty`"; "If the
/// target element is a `ListRelationship`, the target object is defined as the
/// array of objects (represented as URIs) associated to such
/// `ListRelationship`"; and "If the target element is a `VocabProperty`, the
/// target value shall be expanded according to the @context". The `equal`
/// operator matches each of them when the Query Term value "is included in the
/// target value, if the latter is an array".
///
/// A subscription is the request that puts those definitions through the
/// in-memory evaluator, and one filtering on either list family fired for no
/// change at all: the expanded `@list` wrapper meant the evaluator resolved no
/// target, and an unresolved target satisfies no operator.
#[tokio::test]
async fn regression_query_evaluator_a_subscription_filtering_on_a_list_family_fires() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let value_list = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-value-list", "roomTags==\"commercial\"").await;
    let object_list = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-object-list", "roomLinks==\"urn:ngsi-ld:Room:2\"").await;
    let vocab = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-vocab", "roomVocab==\"http://example.org/Other\"").await;
    let control = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-control", "roomName==\"atrium\"").await;
    let absent = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-absent", "roomTags==\"industrial\"").await;

    let id = scope.id("Building:q-eval-array-targets");
    let created = scope
        .client()
        .post_entity(&building_with_array_targets(&id))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let delivered = delivering_subscriptions(&receiver, 4).await;

    assert!(delivered.contains(&value_list), "a ListProperty's valueList is a target value: {delivered:?}");
    assert!(
        delivered.contains(&object_list),
        "a ListRelationship's objectList is a target object: {delivered:?}"
    );
    assert!(delivered.contains(&vocab), "every term of a VocabProperty is a target value: {delivered:?}");
    assert!(delivered.contains(&control), "the scalar filter is the control: {delivered:?}");
    // Clause 4.9: `equal` holds only when the Query Term value is included in
    // the target value, so a member no list holds selects nothing.
    assert!(!delivered.contains(&absent), "a value no list holds must not match: {delivered:?}");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9, `unequal` operator: "Is not included in the target value, if the
/// latter is an array (e.g. matches `["blue","black","green"]`, but not
/// `["blue","red","green"]`)."
///
/// The negated polarity is the half an unresolved target hides: with no target
/// resolved neither `==` nor `!=` ever held, so the operator looked "safe" while
/// being wrong in both directions. Here the array does hold `"commercial"`, so
/// `!=` must not fire, and the array does not hold `"industrial"`, so it must.
#[tokio::test]
async fn regression_query_evaluator_a_negated_list_filter_is_the_complement_of_membership() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let outside = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-not-in-list", "roomTags!=\"industrial\"").await;
    let inside = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-in-list", "roomTags!=\"commercial\"").await;
    let object_outside = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-not-in-object-list", "roomLinks!=\"urn:ngsi-ld:Room:9\"").await;
    let object_inside = create_filtered_subscription(&scope, &receiver, "Subscription:q-eval-in-object-list", "roomLinks!=\"urn:ngsi-ld:Room:1\"").await;

    let id = scope.id("Building:q-eval-negated-targets");
    let created = scope
        .client()
        .post_entity(&building_with_array_targets(&id))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let delivered = delivering_subscriptions(&receiver, 2).await;

    assert!(
        delivered.contains(&outside),
        "a value the valueList does not hold is unequal to it: {delivered:?}"
    );
    assert!(
        delivered.contains(&object_outside),
        "a URI the objectList does not hold is unequal to it: {delivered:?}"
    );
    assert!(
        !delivered.contains(&inside),
        "a value the valueList does hold is not unequal to it: {delivered:?}"
    );
    assert!(
        !delivered.contains(&object_inside),
        "a URI the objectList does hold is not unequal to it: {delivered:?}"
    );

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
