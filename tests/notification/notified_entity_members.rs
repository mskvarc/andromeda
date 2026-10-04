//! What a notification carries of the notified entity's own members when the
//! subscription lists its `notification.attributes` (clause 5.8.6, Table
//! 5.2.14.1-1).
//!
//! The list narrows the Attributes, and as "a synonym for pick" that cannot
//! name `scope` it leaves `scope` out; it never touches the `deletedAt` every
//! entity deletion carries, nor the `createdAt` and `modifiedAt` `sysAttrs`
//! asks for.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    helpers::{
        append_entity_attrs,
        client,
        create_entity,
        create_subscription,
        delete_entity,
        fragment_name,
        set_endpoint_uri,
        subscription_with_endpoint,
        with_join,
        with_notification_attributes,
        with_notification_trigger,
        with_sys_attrs,
    },
    notification_adapter::NotificationAdapter as NotificationReceiver,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// A Building with a name, a scope, an expiry and a Relationship to `city_id`.
fn scoped_building(entity_id: &str, city_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "locatedAt": {"type": "Relationship", "object": city_id},
        "scope": "/Paris/Tourism",
        "expiresAt": "2099-01-01T00:00:00Z",
        "@context": [CORE_CONTEXT]
    })
}

/// A City with nothing but its identity.
fn city(city_id: &str) -> Value {
    json!({"id": city_id, "type": "City", "@context": [CORE_CONTEXT]})
}

/// Create the Building `case` names, and the City it is located at.
async fn create_building(http_client: &reqwest::Client, scope: &TestEnvironment, case: &str) -> (String, String) {
    let city_id = scope.id(&format!("City:{case}"));
    let (status, _, _) = create_entity(http_client, &city(&city_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let entity_id = scope.id(&format!("Building:{case}"));
    let (status, _, _) = create_entity(http_client, &scoped_building(&entity_id, &city_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    (entity_id, city_id)
}

/// Create `subscription` with its endpoint at `receiver`.
async fn subscribe(http_client: &reqwest::Client, scope: &TestEnvironment, receiver: &NotificationReceiver, mut subscription: Value) {
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, scope, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

/// Clause 5.8.6: "If the notification was triggered by the deletion of an
/// Entity and the notification.showChanges member is not set to true, only
/// the deletedAt system property shall be provided" -- whatever Attributes
/// the subscription lists.
#[tokio::test]
async fn notification_entity_deletion_with_listed_attributes_carries_deleted_at() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_building(&http_client, &scope, "r09_01").await;

    let subscription_id = scope.id("Subscription:r09_01");
    let subscription = with_notification_trigger(
        with_notification_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["name"]),
        &["entityDeleted"],
    );
    subscribe(&http_client, &scope, &receiver, subscription).await;
    receiver.clear_notifications().await;

    let (status, _) = delete_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    pretty_assertions::assert_eq!(entity["id"], json!(entity_id));
    assert!(entity.get("deletedAt").is_some(), "the deletion carries deletedAt: {entity}");
    for absent in ["name", "createdAt", "modifiedAt", "scope", "expiresAt"] {
        assert!(entity.get(absent).is_none(), "{absent} is not provided: {entity}");
    }
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6: "In case notification.sysAttrs is set to true, also
/// createdAt and modifiedAt shall be provided", with a listed Attribute too.
#[tokio::test]
async fn notification_entity_deletion_with_listed_attributes_and_sys_attrs_carries_the_entity_system_times() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_building(&http_client, &scope, "r09_02").await;

    let subscription_id = scope.id("Subscription:r09_02");
    let subscription = with_notification_trigger(
        with_sys_attrs(
            with_notification_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["name"]),
            true,
        ),
        &["entityDeleted"],
    );
    subscribe(&http_client, &scope, &receiver, subscription).await;
    receiver.clear_notifications().await;

    let (status, _) = delete_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    for present in ["deletedAt", "createdAt", "modifiedAt"] {
        assert!(entity.get(present).is_some(), "{present} is provided: {entity}");
    }
    assert!(entity.get("name").is_none(), "{entity}");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.14.1-1: `sysAttrs` includes the entity's `createdAt` and
/// `modifiedAt`; `notification.attributes`, a `pick` that cannot name
/// `scope`, keeps the listed Attribute and leaves `scope` and the unlisted
/// Attributes out.
#[tokio::test]
async fn notification_modification_with_listed_attributes_and_sys_attrs_carries_the_entity_system_times() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_building(&http_client, &scope, "r09_03").await;

    let subscription_id = scope.id("Subscription:r09_03");
    let subscription = with_sys_attrs(
        with_notification_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["name"]),
        true,
    );
    subscribe(&http_client, &scope, &receiver, subscription).await;
    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    pretty_assertions::assert_eq!(entity["name"]["value"], json!("Tour Eiffel"));
    for present in ["createdAt", "modifiedAt"] {
        assert!(entity.get(present).is_some(), "{present} is provided: {entity}");
    }
    for absent in ["scope", "subCategory", "locatedAt", "deletedAt"] {
        assert!(entity.get(absent).is_none(), "{absent} is not provided: {entity}");
    }
    scope.finish().await.expect("cleanup failed");
}

/// The entity's `expiresAt` is no member a `pick` decides, so a listed
/// Attribute leaves it exactly as a subscription listing none carries it.
#[tokio::test]
async fn notification_listed_attributes_keep_the_entity_expiry() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_building(&http_client, &scope, "r09_04").await;

    let listed_id = scope.id("Subscription:r09_04_listed");
    let whole_id = scope.id("Subscription:r09_04_whole");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        with_notification_attributes(subscription_with_endpoint(&listed_id, &receiver.endpoint_url()), &["name"]),
    )
    .await;
    subscribe(&http_client, &scope, &receiver, subscription_with_endpoint(&whole_id, &receiver.endpoint_url())).await;
    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let listed = receiver.wait_for_subscription_notification(&listed_id, Duration::from_secs(5)).await;
    let whole = receiver.wait_for_subscription_notification(&whole_id, Duration::from_secs(5)).await;
    let (listed, whole) = (&listed.body["data"][0], &whole.body["data"][0]);
    assert!(whole.get("expiresAt").is_some(), "the whole entity carries its expiry: {whole}");
    pretty_assertions::assert_eq!(listed.get("expiresAt"), whole.get("expiresAt"));
    assert!(listed.get("scope").is_none() && whole.get("scope").is_some(), "{listed} / {whole}");
    scope.finish().await.expect("cleanup failed");
}

/// An entity deletion provides "only the deletedAt system property" (clause
/// 5.8.6), so a `join` adds no Linked Entity to it.
#[tokio::test]
async fn notification_entity_deletion_with_a_flat_join_carries_only_the_deleted_entity() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_building(&http_client, &scope, "r09_05").await;

    let subscription_id = scope.id("Subscription:r09_05");
    let subscription = with_notification_trigger(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "flat", 1),
        &["entityDeleted"],
    );
    subscribe(&http_client, &scope, &receiver, subscription).await;
    receiver.clear_notifications().await;

    let (status, _) = delete_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("a data array");
    pretty_assertions::assert_eq!(data.len(), 1, "{data:?}");
    pretty_assertions::assert_eq!(data[0]["id"], json!(entity_id));
    assert!(data[0].get("deletedAt").is_some(), "{data:?}");
    scope.finish().await.expect("cleanup failed");
}
