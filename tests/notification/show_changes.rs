use std::time::Duration;

use reqwest::StatusCode;

use crate::helpers::{
    NotificationReceiver,
    append_entity_attrs,
    base_subscription,
    building_different_attribute_types_entity,
    building_json_property_entity,
    building_minimal_entity,
    building_simple_entity,
    building_vocab_property_entity,
    client,
    create_entity,
    create_subscription,
    delete_entity,
    delete_entity_attr,
    fragment_json_property_partial,
    fragment_located_at,
    fragment_location,
    fragment_name,
    fragment_street_language,
    fragment_vocab_property_partial,
    partial_update_attr,
    set_endpoint_uri,
    unique_entity_id,
    unique_subscription_id,
    update_entity_attrs,
    with_notification_trigger,
    with_show_changes,
};

async fn create_show_changes_subscription(http_client: &reqwest::Client, case_id: &str, receiver: &NotificationReceiver) -> String {
    let subscription_id = unique_subscription_id(case_id);
    let mut subscription = with_show_changes(base_subscription(&subscription_id));
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, &subscription).await;
    assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;
    subscription_id
}

async fn create_show_changes_trigger_subscription(http_client: &reqwest::Client, case_id: &str, trigger: &str, receiver: &NotificationReceiver) -> String {
    let subscription_id = unique_subscription_id(case_id);
    let mut subscription = with_notification_trigger(with_show_changes(base_subscription(&subscription_id)), &[trigger]);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, &subscription).await;
    assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;
    subscription_id
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_property_update_contains_previous_value() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_31_01");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_31_01", &receiver).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["name"].get("previousValue").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_relationship_update_contains_previous_object() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_31_02");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_31_02", &receiver).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &fragment_located_at()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["locatedAt"].get("previousObject").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_geoproperty_update_contains_previous_value() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_31_03");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_31_03", &receiver).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &fragment_location()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["location"].get("previousValue").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_language_property_update_contains_previous_language_map() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_31_04");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_31_04", &receiver).await;

    let _ = update_entity_attrs(&http_client, &entity_id, &fragment_street_language()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["street"].get("previousLanguageMap").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_json_property_partial_update_contains_previous_json() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_32");
    let _ = create_entity(&http_client, &building_json_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_32", &receiver).await;

    let _ = partial_update_attr(&http_client, &entity_id, "jsonProperty", &fragment_json_property_partial()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["jsonProperty"].get("previousJson").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_vocab_property_partial_update_contains_previous_vocab() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_33");
    let _ = create_entity(&http_client, &building_vocab_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_subscription(&http_client, "046_33", &receiver).await;

    let _ = partial_update_attr(&http_client, &entity_id, "vocabProperty", &fragment_vocab_property_partial()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["vocabProperty"].get("previousVocab").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_property_contains_previous_value() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_34_01");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_34_01", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "name", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["name"].get("previousValue").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_relationship_contains_previous_object() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_34_02");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_34_02", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "locatedAt", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["locatedAt"].get("previousObject").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_geoproperty_contains_previous_value() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_34_03");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_34_03", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "location", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["location"].get("previousValue").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_language_property_contains_previous_language_map() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_34_04");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_34_04", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "street", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["street"].get("previousLanguageMap").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_json_property_contains_previous_json() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_35");
    let _ = create_entity(&http_client, &building_json_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_35", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "jsonProperty", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["jsonProperty"].get("previousJson").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_deleted_vocab_property_contains_previous_vocab() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_36");
    let _ = create_entity(&http_client, &building_vocab_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_36", "attributeDeleted", &receiver).await;

    let _ = delete_entity_attr(&http_client, &entity_id, "vocabProperty", None, None).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["vocabProperty"].get("previousVocab").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_entity_deleted_contains_previous_values_for_multiple_attribute_types() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_37");
    let _ = create_entity(&http_client, &building_different_attribute_types_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_37", "entityDeleted", &receiver).await;

    let _ = delete_entity(&http_client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert_eq!(notification.body["data"][0]["id"], entity_id);
    assert!(notification.body["data"][0]["name"].get("previousValue").is_some());
    assert!(notification.body["data"][0]["locatedAt"].get("previousObject").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_entity_deleted_json_property_contains_previous_json() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_38");
    let _ = create_entity(&http_client, &building_json_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_38", "entityDeleted", &receiver).await;

    let _ = delete_entity(&http_client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["jsonProperty"].get("previousJson").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_entity_deleted_vocab_property_contains_previous_vocab() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_39");
    let _ = create_entity(&http_client, &building_vocab_property_entity(&entity_id)).await;
    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_39", "entityDeleted", &receiver).await;

    let _ = delete_entity(&http_client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.body["data"][0]["vocabProperty"].get("previousVocab").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_entity_created_does_not_include_previous_fields() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_40", "entityCreated", &receiver).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_40");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let payload = serde_json::to_string(&notification.body).expect("serialize notification");
    assert!(!payload.contains("previousValue"));
    assert!(!payload.contains("previousObject"));
    assert!(!payload.contains("previousLanguageMap"));
}

#[serial_test::serial]
#[tokio::test]
async fn notification_show_changes_attribute_created_does_not_include_previous_fields() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_41");
    let _ = create_entity(&http_client, &building_minimal_entity(&entity_id)).await;

    let subscription_id = create_show_changes_trigger_subscription(&http_client, "046_41", "attributeCreated", &receiver).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let payload = serde_json::to_string(&notification.body).expect("serialize notification");
    assert!(!payload.contains("previousValue"));
    assert!(!payload.contains("previousObject"));
}
