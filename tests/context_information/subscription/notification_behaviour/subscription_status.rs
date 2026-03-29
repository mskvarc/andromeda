use std::time::Duration;

use crate::harness::{client::RequestHeaders, fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::status";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_11: timesSent incremented ───────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_11::times_sent_incremented"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_11");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_11:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let _notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        let resp = ctx.client.retrieve_subscription(&sub_id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["notification"]["timesSent"], 1);
    }));

    // ── 046_12: lastNotification and lastSuccess updated ────────────
    trials.push(trial(&format!("{PREFIX}::046_12::last_notification_updated"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_12");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_12:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let _notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        // Allow time for the broker to record delivery outcome after the HTTP response
        tokio::time::sleep(Duration::from_millis(200)).await;

        let resp = ctx.client.retrieve_subscription(&sub_id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["notification"]["status"], "ok");

        let last_notif = body["notification"]["lastNotification"].as_str().expect("lastNotification should be a string");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_notif).is_ok(),
            "lastNotification should be a valid ISO date: {last_notif}"
        );

        let last_success = body["notification"]["lastSuccess"].as_str().expect("lastSuccess should be a string");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_success).is_ok(),
            "lastSuccess should be a valid ISO date: {last_success}"
        );
    }));

    // ── 046_13: lastFailure on delivery failure ─────────────────────
    trials.push(trial(&format!("{PREFIX}::046_13::last_failure_on_error"), |ctx| async move {
        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_13");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_13:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-active.jsonld", &sub_id);
        // Point to an unreachable endpoint (no receiver started)
        sub["notification"]["endpoint"]["uri"] = serde_json::Value::String("http://host.docker.internal:19999/notify".to_string());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Wait for the broker to attempt and fail delivery
        tokio::time::sleep(Duration::from_secs(3)).await;

        let resp = ctx.client.retrieve_subscription(&sub_id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["notification"]["status"], "failed");

        let last_failure = body["notification"]["lastFailure"].as_str().expect("lastFailure should be a string");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_failure).is_ok(),
            "lastFailure should be a valid ISO date: {last_failure}"
        );
    }));
}
