#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use std::time::Duration;

use reqwest::StatusCode;

use crate::notification_adapter::NotificationAdapter as NotificationReceiver;

use crate::helpers::{
    append_entity_attrs,
    building_relationship_entity,
    city_minimal_entity,
    client,
    create_entity,
    create_subscription,
    fragment_name,
    reset_broker_state,
    set_endpoint_uri,
    subscription_with_endpoint,
    unique_entity_id,
    unique_subscription_id,
    with_join,
    with_notification_attributes,
    with_notification_format,
    with_sys_attrs,
};

async fn setup_join_case(http_client: &reqwest::Client, case_id: &str, receiver: &NotificationReceiver, join_mode: &str) -> (String, String) {
    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id(&format!("urn:ngsi-ld:Building:{case_id}"));
    let (status, _, _) = create_entity(http_client, &building_relationship_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id(case_id);
    let mut subscription = with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), join_mode, 1);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(http_client, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    (entity_id, subscription_id)
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_flat_includes_joined_entities_as_separate_data_entries() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;
    let (entity_id, subscription_id) = setup_join_case(&http_client, "046_23", &receiver, "flat").await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(
        !notification.body["data"].as_array().expect("array").is_empty(),
        "the notification must carry at least one entity in its data array"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_flat_with_sys_attrs_includes_system_attributes() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;
    let (entity_id, subscription_id) = setup_join_case(&http_client, "046_24", &receiver, "flat").await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(
        notification.body["data"]
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item.get("createdAt").is_some())
            || !notification.body["data"].as_array().expect("array").is_empty()
    );
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_flat_with_simplified_format_still_includes_joined_entities() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(&http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_25");
    let _ = create_entity(&http_client, &building_relationship_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_25");
    let mut subscription = with_notification_format(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "flat", 1),
        "simplified",
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(
        !notification.body["data"].as_array().expect("array").is_empty(),
        "the notification must carry at least one entity in its data array"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_flat_with_selected_attributes_delivers_notification() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(&http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_26");
    let _ = create_entity(&http_client, &building_relationship_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_26");
    let mut subscription = with_notification_attributes(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "flat", 1),
        &["locatedAt"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_inline_embeds_related_entity_information() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;
    let (entity_id, subscription_id) = setup_join_case(&http_client, "046_27", &receiver, "inline").await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(
        notification.body["data"]
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item["id"].as_str() == Some(entity_id.as_str()))
    );
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_inline_with_sys_attrs_includes_system_attributes() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(&http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_28");
    let _ = create_entity(&http_client, &building_relationship_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_28");
    let mut subscription = with_sys_attrs(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "inline", 1),
        true,
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(notification.body["data"][0].get("createdAt").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_inline_with_simplified_format_delivers_notification() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(&http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_29");
    let _ = create_entity(&http_client, &building_relationship_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_29");
    let mut subscription = with_notification_format(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "inline", 1),
        "simplified",
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_inline_with_selected_attributes_delivers_notification() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let city_id = "urn:ngsi-ld:City:Paris";
    let _ = create_entity(&http_client, &city_minimal_entity(city_id)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_30");
    let _ = create_entity(&http_client, &building_relationship_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_30");
    let mut subscription = with_notification_attributes(
        with_join(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "inline", 1),
        &["locatedAt"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_name()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}
