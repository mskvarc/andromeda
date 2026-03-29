/// Feature 1: Notification joinLevel (spec 5.2.14.1)
///
/// Verifies that subscription notification.joinLevel is honoured instead of
/// being hardcoded to 1.
use crate::harness::{self, fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use reqwest::StatusCode;
use std::time::Duration;

const PREFIX: &str = "spec_features::notification_join_level";

fn set_endpoint_uri(sub: &mut serde_json::Value, uri: &str) {
    sub["notification"]["endpoint"]["uri"] = serde_json::Value::String(uri.to_string());
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── joinLevel=3 delivers 3 levels of linked entities ──────────
    trials.push(trial(&format!("{PREFIX}::join_level_3_delivers_deep_links"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        // Create chain: Building → City → Country
        let country_id = "urn:ngsi-ld:Country:France";
        ctx.track_entity(country_id);
        let country = serde_json::json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "France"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&country).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let city_id = "urn:ngsi-ld:City:Paris";
        ctx.track_entity(city_id);
        let city = serde_json::json!({
            "id": city_id,
            "type": "City",
            "name": {"type": "Property", "value": "Paris"},
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&city).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let building_id = ctx.entity_id("urn:ngsi-ld:Building:jl3");
        let building = serde_json::json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&building).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create subscription with joinLevel=3
        let sub_id = format!("urn:ngsi-ld:Subscription:jl3:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = serde_json::json!({
            "id": sub_id,
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "endpoint": {"uri": "http://placeholder"},
                "join": "flat",
                "joinLevel": 3
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Trigger notification by updating the building
        let fragment = serde_json::json!({
            "name": {"type": "Property", "value": "Tower"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.append_entity_attrs(&building_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Wait for notification
        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");

        // Should include building + city + country = at least 3
        assert!(
            data.len() >= 3,
            "joinLevel=3 should include building, city, and country, got {} entries: {:?}",
            data.len(),
            data.iter().map(|d| d["id"].as_str().unwrap_or("?")).collect::<Vec<_>>()
        );

        // Verify all three entities are present
        let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
        assert!(ids.contains(&building_id.as_str()), "should contain building");
        assert!(ids.contains(&city_id), "should contain city");
        assert!(ids.contains(&country_id), "should contain country");
    }));

    // ── joinLevel defaults to 1 ──────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::join_level_default_is_1"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        // Create chain: Building → City → Country
        let country_id = "urn:ngsi-ld:Country:Germany";
        ctx.track_entity(country_id);
        let country = serde_json::json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Germany"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&country).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let city_id = "urn:ngsi-ld:City:Berlin";
        ctx.track_entity(city_id);
        let city = serde_json::json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&city).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let building_id = ctx.entity_id("urn:ngsi-ld:Building:jldef");
        let building = serde_json::json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&building).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Sub without explicit joinLevel → defaults to 1
        let sub_id = format!("urn:ngsi-ld:Subscription:jldef:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = serde_json::json!({
            "id": sub_id,
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "endpoint": {"uri": "http://placeholder"},
                "join": "flat"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = serde_json::json!({
            "name": {"type": "Property", "value": "Haus"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.append_entity_attrs(&building_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");

        // Default joinLevel=1 → building + city only (not country)
        assert_eq!(data.len(), 2, "default joinLevel=1 should return building + 1 level linked");
        let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
        assert!(ids.contains(&building_id.as_str()));
        assert!(ids.contains(&city_id));
        assert!(!ids.contains(&country_id), "country should not be included at joinLevel=1");
    }));
}
