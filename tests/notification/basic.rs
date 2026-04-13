#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::time::Duration;

use reqwest::StatusCode;

use crate::helpers::{
    CORE_CONTEXT,
    NotificationReceiver,
    append_entity_attrs,
    building_minimal_entity,
    building_multi_instances_entity,
    building_simple_entity,
    client,
    create_entity,
    create_subscription,
    delete_entity,
    fragment_air_quality_level,
    fragment_building_name_dataset,
    retrieve_subscription,
    set_endpoint_uri,
    subscription_with_endpoint,
    unique_entity_id,
    unique_subscription_id,
    vehicle_simple_entity,
    with_dataset_id,
    with_endpoint_accept,
    with_entity_type_selector,
    with_is_active,
    with_jsonld_context,
    with_notification_attributes,
    with_notification_format,
    with_notification_trigger,
    with_query_filter,
    with_receiver_info,
    with_sys_attrs,
    with_time_interval,
    with_watched_attributes,
};

fn notification_contains_entity(notification: &serde_json::Value, entity_id: &str) -> bool {
    notification["data"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["id"].as_str() == Some(entity_id)))
}

#[serial_test::serial]
#[tokio::test]
async fn notification_active_subscription_on_attribute_append_delivers_notification() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_01");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("046_01");
    let mut subscription = with_is_active(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), true);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(notification_contains_entity(&notification.body, &entity_id));
}

#[serial_test::serial]
#[tokio::test]
async fn notification_time_interval_subscription_delivers_notification_without_entity_change() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_02");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("046_02");
    let mut subscription = with_time_interval(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), 10);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(15)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(notification_contains_entity(&notification.body, &entity_id));
}

#[serial_test::serial]
#[tokio::test]
async fn notification_default_subscription_on_attribute_append_delivers_notification() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_03");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("046_03");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    subscription["notification"].as_object_mut().expect("notification object").remove("format");
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_watched_attributes_subscription_delivers_notification_for_matching_attribute() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_04");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("046_04");
    let mut subscription = with_watched_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["airQualityLevel"]);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_query_filter_matching_entity_delivers_notification() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_05");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("046_05");
    let mut subscription = with_query_filter(
        with_watched_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["airQualityLevel"]),
        "name==\"Eiffel%20Tower\"",
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_entity_created_trigger_delivers_notification_on_create() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("046_06");
    let mut subscription = with_notification_trigger(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
        &["entityCreated"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());

    let (status, _, _) = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_06");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert!(notification_contains_entity(&notification.body, &entity_id));
}

#[serial_test::serial]
#[tokio::test]
async fn notification_payload_contains_type_subscription_id_notified_at_and_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_07_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_07_01");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["type"], "Notification");
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    let notified_at = notification.body["notifiedAt"].as_str().expect("notifiedAt string");
    assert!(chrono::DateTime::parse_from_rfc3339(notified_at).is_ok());
    assert!(notification.body["data"].is_array());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_notification_attributes_filter_only_includes_configured_attributes() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_07_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_07_02");
    let mut subscription = with_notification_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["airQualityLevel"]);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("array");
    assert!(data[0].get("airQualityLevel").is_some());
    assert!(data[0].get("name").is_none());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_core_context_subscription_uses_expanded_attribute_terms() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_07_03");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_07_03");
    let mut subscription = with_notification_attributes(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), &["airQualityLevel"]);
    subscription["entities"] = serde_json::json!([{"type": "Building"}]);
    subscription["@context"] = serde_json::json!([CORE_CONTEXT]);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("array");
    let has_expanded_key = data[0]
        .as_object()
        .expect("notification entity object")
        .keys()
        .any(|key| key.contains("airQualityLevel"));
    assert!(has_expanded_key);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_without_notification_attributes_includes_all_entity_attributes() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_08_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_08_01");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("array");
    assert!(data[0].get("airQualityLevel").is_some());
    assert!(data[0].get("name").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_key_values_format_sends_scalar_property_values() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("046_08_02");
    let mut subscription = with_notification_trigger(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
        &["entityCreated"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_08_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("array");
    let name = &data[0]["name"];
    assert!(name.is_string() || name.get("value").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_receiver_info_adds_custom_headers_to_notification_request() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_09");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_09");
    let mut subscription = with_receiver_info(
        subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()),
        "X-Additional-Key",
        "Value",
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(
        notification.headers.get("x-additional-key").and_then(|header| header.to_str().ok()),
        Some("Value")
    );
}

#[serial_test::serial]
#[tokio::test]
async fn notification_default_json_format_includes_link_header_and_omits_entity_context() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_10");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_10");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    subscription["notification"].as_object_mut().expect("notification object").remove("format");
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.headers.get("link").is_some());
    assert!(notification.body["data"][0].get("@context").is_none());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_after_successful_delivery_subscription_times_sent_is_incremented() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_11");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_11");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let _ = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;

    let (status, body) = retrieve_subscription(&http_client, &subscription_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["timesSent"], 1);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_after_successful_delivery_subscription_status_is_ok_and_timestamps_are_set() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_12");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_12");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let _ = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (status, body) = retrieve_subscription(&http_client, &subscription_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["status"], "ok");
    assert!(chrono::DateTime::parse_from_rfc3339(body["notification"]["lastNotification"].as_str().expect("lastNotification string")).is_ok());
    assert!(chrono::DateTime::parse_from_rfc3339(body["notification"]["lastSuccess"].as_str().expect("lastSuccess string")).is_ok());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_failed_delivery_sets_failed_status_and_last_failure_timestamp() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_13");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_13");
    let unreachable_endpoint = "http://host.docker.internal:19999/notify";
    let subscription = subscription_with_endpoint(&subscription_id, unreachable_endpoint);
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    for _ in 0..10 {
        let (status, body) = retrieve_subscription(&http_client, &subscription_id).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK);

        if body["notification"]["status"] == "failed" {
            assert!(chrono::DateTime::parse_from_rfc3339(body["notification"]["lastFailure"].as_str().expect("lastFailure string")).is_ok());
            return;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    unreachable!("subscription notification status did not become failed within retry window");
}

#[serial_test::serial]
#[tokio::test]
async fn notification_jsonld_accept_on_endpoint_omits_link_header_and_includes_entity_context() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_14");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_14");
    let mut subscription = with_endpoint_accept(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "application/ld+json");
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    assert!(notification.headers.get("link").is_none());
    assert!(notification.body["data"][0].get("@context").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_throttling_only_emits_one_notification_within_window() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_15");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_15");
    let mut subscription = subscription_with_endpoint(&subscription_id, &receiver.endpoint_url());
    subscription["throttling"] = serde_json::json!(15);
    subscription["notification"]["format"] = serde_json::json!("keyValues");
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    let _ = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;

    let _ = append_entity_attrs(&http_client, &entity_id, &crate::helpers::fragment_name()).await;
    receiver.wait_for_no_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_entity_created_trigger_with_matching_type_selector_delivers_notification() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("046_16_01");
    let mut subscription = with_notification_trigger(
        with_entity_type_selector(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "(Building|Tower)"),
        &["entityCreated"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_16_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_entity_created_trigger_with_non_matching_type_selector_delays_notification() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("046_16_02");
    let mut subscription = with_notification_trigger(
        with_entity_type_selector(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "(Building|Tower)"),
        &["entityCreated"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:046_16_02");
    let _ = create_entity(&http_client, &vehicle_simple_entity(&entity_id)).await;

    receiver.wait_for_no_subscription_notification(&subscription_id, Duration::from_secs(3)).await;
}

#[serial_test::serial]
#[tokio::test]
async fn notification_sys_attrs_true_includes_created_and_modified_timestamps() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_17");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_17");
    let mut subscription = with_sys_attrs(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
        true,
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    assert!(entity.get("createdAt").is_some());
    assert!(entity.get("modifiedAt").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_sys_attrs_false_omits_created_and_modified_timestamps() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_18");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_18");
    let mut subscription = with_sys_attrs(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
        false,
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;
    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    assert!(entity.get("createdAt").is_none());
    assert!(entity.get("modifiedAt").is_none());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_dataset_id_filter_emits_notification_for_matching_dataset_instance() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_19");
    let _ = create_entity(&http_client, &building_multi_instances_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_19");
    let mut subscription = with_dataset_id(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "normalized"),
        &["urn:ngsi-ld:Dataset:french-name"],
    );
    subscription["notification"]["endpoint"]["accept"] = serde_json::json!("application/ld+json");
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_building_name_dataset()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_subscription_jsonld_context_compacts_notification_payload() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_20");
    let _ = create_entity(&http_client, &building_minimal_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_20");
    let mut subscription = with_jsonld_context(
        with_endpoint_accept(
            with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "normalized"),
            "application/ld+json",
        ),
        CORE_CONTEXT,
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let _ = append_entity_attrs(&http_client, &entity_id, &fragment_building_name_dataset()).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id);
}

#[serial_test::serial]
#[tokio::test]
async fn notification_entity_deleted_trigger_emits_deleted_entity_payload() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_21_01");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_21_01");
    let mut subscription = with_notification_trigger(
        with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
        &["entityDeleted"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let (status, _) = delete_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id);
    assert!(notification.body["data"][0].get("deletedAt").is_some());
}

#[serial_test::serial]
#[tokio::test]
async fn notification_entity_deleted_with_sys_attrs_includes_created_and_deleted_timestamps() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:046_21_02");
    let _ = create_entity(&http_client, &building_simple_entity(&entity_id)).await;

    let subscription_id = unique_subscription_id("046_21_02");
    let mut subscription = with_notification_trigger(
        with_sys_attrs(
            with_notification_format(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), "keyValues"),
            true,
        ),
        &["entityDeleted"],
    );
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let _ = create_subscription(&http_client, &subscription).await;

    receiver.clear_notifications().await;

    let (status, _) = delete_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let entity = &notification.body["data"][0];
    pretty_assertions::assert_eq!(entity["id"], entity_id);
    assert!(entity.get("createdAt").is_some());
    assert!(entity.get("deletedAt").is_some());
}
