use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::structure";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_07_01: Notification structure (type, subscriptionId, notifiedAt) ─
    trials.push(trial(&format!("{PREFIX}::046_07_01::notification_structure_fields"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_07_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["type"], "Notification");
        assert_eq!(notif.body["subscriptionId"], sub_id);
        // notifiedAt should be a valid ISO date string
        let notified_at = notif.body["notifiedAt"].as_str().expect("notifiedAt should be a string");
        assert!(
            chrono::DateTime::parse_from_rfc3339(notified_at).is_ok(),
            "notifiedAt should be a valid ISO date: {notified_at}"
        );
        assert!(notif.body["data"].is_array(), "data should be an array");
    }));

    // ── 046_07_02: Only specified notification.attributes ───────────
    trials.push(trial(&format!("{PREFIX}::046_07_02::notification_attributes_filter"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_07_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_07_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active-notificationAttributes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        let entity_data = &data[0];
        // Should have airQualityLevel (the specified attribute)
        assert!(entity_data.get("airQualityLevel").is_some(), "should contain airQualityLevel");
        // Should NOT have name or subCategory (not in notification.attributes)
        assert!(entity_data.get("name").is_none(), "should not contain name");
        assert!(entity_data.get("subCategory").is_none(), "should not contain subCategory");
    }));

    // ── 046_07_03: URI expansion observed ───────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_07_03::uri_expansion"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_07_03");
        // Create entity with the test suite context so the entity has the right types
        let entity = fixtures::load_fixture_original_context("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_07_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        // This fixture uses core context only with expanded URIs
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active-default-context.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        let entity_data = &data[0];
        // With core context only, the notification should use expanded URIs
        assert!(
            entity_data.get("https://ngsi-ld-test-suite/context#airQualityLevel").is_some(),
            "notification should use expanded URIs, got keys: {:?}",
            entity_data.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
    }));

    // ── 046_08_01: All attributes when notification.attributes not set ─
    trials.push(trial(&format!("{PREFIX}::046_08_01::all_attributes_included"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_08_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_08_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        let entity_data = &data[0];
        // All entity attributes should be present
        assert!(entity_data.get("airQualityLevel").is_some(), "should contain airQualityLevel");
        assert!(entity_data.get("name").is_some(), "should contain name");
    }));

    // ── 046_08_02: Simplified (keyValues) format ────────────────────
    trials.push(trial(&format!("{PREFIX}::046_08_02::simplified_format"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:046_08_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        // entityCreated subscription has format=keyValues
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-entityCreated.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_08_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        let entity_data = &data[0];
        // In keyValues format, name should be a string, not a Property object
        assert!(
            entity_data["name"].is_string(),
            "name should be a string in keyValues format, got: {}",
            entity_data["name"]
        );
    }));

    // ── 046_09: receiverInfo custom headers ─────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_09::receiver_info_headers"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_09");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_09:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active-endpoint-info.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        // Verify custom header from receiverInfo
        assert!(
            notif.headers.get("X-Additional-Key").is_some(),
            "notification should contain X-Additional-Key header"
        );
        assert_eq!(notif.headers.get("X-Additional-Key").unwrap().to_str().unwrap(), "Value");
    }));

    // ── 046_10: JSON format (not JSON-LD) by default ────────────────
    trials.push(trial(&format!("{PREFIX}::046_10::json_format_default"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_10");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_10:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-default.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        // Should have Link header with context
        assert!(notif.headers.get("Link").is_some(), "notification should have Link header for JSON format");
        // Data entities should NOT have @context
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0].get("@context").is_none(), "data entities should not contain @context in JSON format");
    }));
}
