#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use std::time::Duration;

use reqwest::StatusCode;

use crate::notification_adapter::NotificationAdapter as NotificationReceiver;

use crate::helpers::{
    base_subscription,
    building_different_attribute_types_entity,
    building_multi_instances_entity,
    building_simple_entity,
    client,
    create_entity,
    create_subscription,
    delete_entity,
    delete_entity_attr,
    merge_entity,
    null_language_property_fragment,
    null_language_property_partial_fragment,
    null_property_fragment,
    null_property_partial_fragment,
    null_relationship_fragment,
    null_relationship_partial_fragment,
    partial_update_attr,
    set_endpoint_uri,
    update_entity_attrs,
    with_notification_format,
    with_notification_trigger,
    with_show_changes,
    with_sys_attrs,
    with_watched_attributes,
};
use andromeda_test_harness::environment::TestEnvironment;

async fn create_attr_deleted_subscription(
    http_client: &reqwest::Client,
    scope: &TestEnvironment,
    case_id: &str,
    receiver: &NotificationReceiver,
    format: &str,
    watched_attributes: Option<&[&str]>,
    sys_attrs: Option<bool>,
) -> String {
    let subscription_id = scope.id(&format!("Subscription:{case_id}"));
    let mut subscription = with_notification_trigger(base_subscription(&subscription_id), &["attributeDeleted"]);
    subscription = with_notification_format(subscription, format);

    if let Some(watched_attributes) = watched_attributes {
        subscription = with_watched_attributes(subscription, watched_attributes);
    }

    if let Some(sys_attrs) = sys_attrs {
        subscription = with_sys_attrs(subscription, sys_attrs);
    }

    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, scope, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;
    subscription_id
}

#[tokio::test]
async fn notification_attribute_deleted_key_values_delivers_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_01", &receiver, "keyValues", None, None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_specific_watched_attribute_key_values_delivers_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_02", &receiver, "keyValues", Some(&["name"]), None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_deleting_non_watched_attribute_does_not_deliver_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_03", &receiver, "keyValues", Some(&["name"]), None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "airQualityLevel", None, None).await;
    receiver.wait_for_no_subscription_notification(&subscription_id, Duration::from_secs(3)).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_normalized_nullifies_deleted_attribute() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_04");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_04", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["name"]["value"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_normalized_sys_attrs_adds_deleted_at_and_created_at() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_05");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_05", &receiver, "normalized", Some(&["name"]), Some(true)).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["name"].get("deletedAt").is_some());
    assert!(notification.body["data"][0]["name"].get("createdAt").is_some());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_with_dataset_id_delivers_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_06");
    let _ = create_entity(&http_client, &building_multi_instances_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_06", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", Some("urn:ngsi-ld:Dataset:french-name"), None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_key_values_with_dataset_id_delivers_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_07");
    let _ = create_entity(&http_client, &building_multi_instances_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_07", &receiver, "keyValues", Some(&["name"]), None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", Some("urn:ngsi-ld:Dataset:french-name"), None).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["subscriptionId"].is_string());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_attribute_deleted_with_delete_all_delivers_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_08");
    let _ = create_entity(&http_client, &building_multi_instances_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_08", &receiver, "keyValues", None, None).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, Some(true)).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["subscriptionId"].is_string());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_property_via_merge_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_09_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_09_01", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = merge_entity(&http_client, &entity_id, &null_property_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["name"]["value"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_property_via_update_attrs_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_09_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_09_02", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &null_property_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["name"]["value"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_property_via_partial_update_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_09_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_09_03", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = partial_update_attr(&http_client, &entity_id, "name", &null_property_partial_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["name"]["value"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_relationship_via_merge_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_10_01");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_10_01", &receiver, "normalized", Some(&["locatedAt"]), None).await;

    let _ = merge_entity(&http_client, &entity_id, &null_relationship_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["locatedAt"]["object"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_relationship_via_update_attrs_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_10_02");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_10_02", &receiver, "normalized", Some(&["locatedAt"]), None).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &null_relationship_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["locatedAt"]["object"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_relationship_via_partial_update_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_10_03");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_10_03", &receiver, "normalized", Some(&["locatedAt"]), None).await;

    let _ = partial_update_attr(&http_client, &entity_id, "locatedAt", &null_relationship_partial_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["locatedAt"]["object"], "urn:ngsi-ld:null");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_language_property_via_merge_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_11_01");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_11_01", &receiver, "normalized", Some(&["street"]), None).await;

    let _ = merge_entity(&http_client, &entity_id, &null_language_property_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(
        notification.body["data"][0]["street"]["languageMap"],
        serde_json::json!({"@none": "urn:ngsi-ld:null"})
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_language_property_via_update_attrs_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_11_02");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_11_02", &receiver, "normalized", Some(&["street"]), None).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &null_language_property_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(
        notification.body["data"][0]["street"]["languageMap"],
        serde_json::json!({"@none": "urn:ngsi-ld:null"})
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_ngsi_ld_null_language_property_via_partial_update_triggers_attribute_deleted_notification() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_11_03");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_11_03", &receiver, "normalized", Some(&["street"]), None).await;

    let _ = partial_update_attr(&http_client, &entity_id, "street", &null_language_property_partial_fragment()).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(
        notification.body["data"][0]["street"]["languageMap"],
        serde_json::json!({"@none": "urn:ngsi-ld:null"})
    );
    scope.finish().await.expect("cleanup failed");
}

// Clause 5.8.6: "If the notification was triggered by the deletion of an Entity
// and the notification.showChanges member is not set to true, only the deletedAt
// system property shall be provided." Deleting the Entity is what triggers this
// notification, so the payload carries the entity-level `deletedAt` and none of
// the attributes the subscription watches -- the per-attribute nullification of
// the same clause applies to the deletion of an Attribute, not of an Entity.
#[tokio::test]
async fn notification_entity_deletion_with_attribute_deleted_trigger_carries_only_deleted_at() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_12");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_12", &receiver, "normalized", Some(&["name"]), None).await;

    let _ = delete_entity(&http_client, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    pretty_assertions::assert_eq!(entity["id"], entity_id);
    assert!(entity.get("deletedAt").is_some());
    assert!(entity.get("name").is_none());
    scope.finish().await.expect("cleanup failed");
}

// Clause 5.8.6: on the deletion of an Entity "only the deletedAt system property
// shall be provided. In case notification.sysAttrs is set to true, also createdAt
// and modifiedAt shall be provided." All three are members of the deleted Entity,
// not of any Attribute of it.
#[tokio::test]
async fn notification_entity_deletion_with_attribute_deleted_and_sys_attrs_includes_deleted_at() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_13");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = create_attr_deleted_subscription(&http_client, &scope, "046_22_13", &receiver, "normalized", Some(&["name"]), Some(true)).await;

    let _ = delete_entity(&http_client, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    pretty_assertions::assert_eq!(entity["id"], entity_id);
    assert!(entity.get("deletedAt").is_some());
    assert!(entity.get("createdAt").is_some());
    assert!(entity.get("modifiedAt").is_some());
    assert!(entity.get("name").is_none());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_extension_attribute_deleted_show_changes_combines_with_previous_value_fields() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:046_22_extension");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = scope.id("Subscription:046_22_extension");
    let mut subscription = with_show_changes(with_notification_trigger(base_subscription(&subscription_id), &["attributeDeleted"]));
    subscription = with_watched_attributes(subscription, &["name"]);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &scope, &subscription).await;

    receiver.clear_notifications().await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["name"].get("previousValue").is_some());
    scope.finish().await.expect("cleanup failed");
}
