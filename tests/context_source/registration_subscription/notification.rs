use std::time::Duration;

use crate::harness::{client::RequestHeaders, fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "context_source::registration_subscription::notification";

fn set_endpoint_uri(sub: &mut serde_json::Value, uri: &str) {
    sub["notification"]["endpoint"]["uri"] = serde_json::Value::String(uri.to_string());
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 047_01: Receive cSourceNotification periodically (timeInterval) ─

    // 047_01_01: Subscription with timeInterval sends periodic notifications (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_01_01::periodic_notification"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-timeInterval.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // timeInterval is 10s in fixture; wait up to 15s for periodic notification
        let notif = receiver.wait_for_notification(Duration::from_secs(15)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 047_02: Notification on subscription creation and CSR change ────

    // 047_02_01: Initial notification + notification on CSR update with triggerReason (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_02_01::notification_on_change"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_02_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_02_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Initial notification on subscription creation
        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");

        // Update CSR to trigger second notification
        let fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-update.json");
        let resp = ctx
            .client
            .request(
                reqwest::Method::PATCH,
                &format!("/csourceRegistrations/{csr_id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "updated");
    }));

    // ── 047_03: Notification with relevant information only ─────────────

    // 047_03_01: Only matching RegistrationInfo elements in notification (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_03_01::relevant_information"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // CSR with Building AND Bus info — subscription only filters Building
        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");

        // Notification data should contain only Building info, not Bus
        let data = notif.body["data"].as_array().expect("data should be an array");
        assert!(!data.is_empty(), "data should not be empty");
        let info = data[0]["information"].as_array().expect("information should be an array");
        // The original CSR has Building and Bus info blocks; only Building should remain
        assert_eq!(info.len(), 1, "only one RegistrationInfo element should match");
        for info_entry in info {
            let entities = info_entry["entities"].as_array().expect("entities should be an array");
            for entity in entities {
                let type_val = entity["type"].as_str().expect("type should be a string");
                // Entity type is stored in expanded URI form
                assert!(
                    type_val == "Building" || type_val.ends_with("#Building"),
                    "only Building entities should be in notification, got: {type_val}"
                );
            }
        }
    }));

    // ── 047_04: Notification with compliant structure (5.3.2) ───────────

    // 047_04_01: cSourceNotification structure per clause 5.3.2 (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_04_01::compliant_structure"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert!(notif.body["notifiedAt"].is_string(), "notifiedAt should be present");
        let notified_at = notif.body["notifiedAt"].as_str().unwrap();
        assert!(
            chrono::DateTime::parse_from_rfc3339(notified_at).is_ok(),
            "notifiedAt should be a valid ISO date: {notified_at}"
        );
        assert!(notif.body["triggerReason"].is_string(), "triggerReason should be present");
        assert!(notif.body["data"].is_array(), "data should be an array");
        let data = notif.body["data"].as_array().unwrap();
        assert!(!data.is_empty(), "data should not be empty");
    }));

    // ── 047_05: Successful notification updates subscription metadata ───

    // 047_05_01: timesSent, lastSuccess, lastNotification updated, status="ok" (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_05_01::notification_updated_on_success"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let _notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        // Small delay to let mark_success complete
        tokio::time::sleep(Duration::from_millis(500)).await;

        // options=sysAttrs is required to see notification metadata (timesSent, lastSuccess, etc.)
        let resp = ctx
            .client
            .request(
                reqwest::Method::GET,
                &format!("/csourceSubscriptions/{sub_id}?options=sysAttrs"),
                None,
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["notification"]["timesSent"], 1);
        assert_eq!(body["notification"]["status"], "ok");

        let last_notif = body["notification"]["lastNotification"].as_str().expect("lastNotification should be present");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_notif).is_ok(),
            "lastNotification should be a valid ISO date: {last_notif}"
        );

        let last_success = body["notification"]["lastSuccess"].as_str().expect("lastSuccess should be present");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_success).is_ok(),
            "lastSuccess should be a valid ISO date: {last_success}"
        );
    }));

    // ── 047_06: Failed notification updates subscription metadata ───────

    // 047_06_01: timesSent, lastFailure, lastNotification updated, status="failed" (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_06_01::notification_updated_on_failure"), |ctx| async move {
        let sub_id = format!("urn:ngsi-ld:Subscription:047_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        // Unreachable endpoint — notification delivery will fail
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-unreachable-endpoint.jsonld", &sub_id);
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Wait for broker to attempt and fail delivery
        tokio::time::sleep(Duration::from_secs(3)).await;

        // options=sysAttrs is required to see notification metadata (lastFailure, etc.)
        let resp = ctx
            .client
            .request(
                reqwest::Method::GET,
                &format!("/csourceSubscriptions/{sub_id}?options=sysAttrs"),
                None,
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["notification"]["status"], "failed");

        let last_failure = body["notification"]["lastFailure"].as_str().expect("lastFailure should be present");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_failure).is_ok(),
            "lastFailure should be a valid ISO date: {last_failure}"
        );

        let last_notif = body["notification"]["lastNotification"].as_str().expect("lastNotification should be present");
        assert!(
            chrono::DateTime::parse_from_rfc3339(last_notif).is_ok(),
            "lastNotification should be a valid ISO date: {last_notif}"
        );
    }));

    // ── 047_07: No notification when subscription is not active ─────────

    // 047_07_01: No notification when subscription is paused (isActive=false) (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_07_01::paused_subscription"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Pause subscription
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-isActive-update.json");
        let resp = ctx
            .client
            .update_csource_subscription(&sub_id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        receiver.wait_for_no_notification(Duration::from_secs(5)).await;
    }));

    // 047_07_02: No notification when subscription is expired (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_07_02::expired_subscription"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_07_02:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Expire subscription
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-expiresAt-update.json");
        let resp = ctx
            .client
            .update_csource_subscription(&sub_id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_07_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        receiver.wait_for_no_notification(Duration::from_secs(5)).await;
    }));

    // ── 047_08: Notification for matching CSR providing latest info ──────

    // 047_08_01: Without temporalQ, notifications from CSRs providing latest info (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_08_01::matching_latest_information"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_08_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_08_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Initial notification
        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");

        // Update CSR
        let fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-update.json");
        let resp = ctx
            .client
            .request(
                reqwest::Method::PATCH,
                &format!("/csourceRegistrations/{csr_id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "updated");
    }));

    // ── 047_09: Notification for no longer matching CSR ──────────────────

    // 047_09_01: CSR info changes to no longer match → triggerReason=noLongerMatching (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_09_01::no_longer_matching"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_09_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-building-and-bus-entities.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_09_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Consume initial notification
        let _notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        // Update CSR information so it no longer matches Building subscription
        let fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-update-information.json");
        let resp = ctx
            .client
            .request(
                reqwest::Method::PATCH,
                &format!("/csourceRegistrations/{csr_id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "noLongerMatching");
    }));

    // ── 047_10: Notification matching on observationInterval ─────────────

    // 047_10_01: temporalQ with observedAt matches CSR observationInterval (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_10_01::observation_interval"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_10_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-temporalQ-observedAt.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_10_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-observationInterval.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_11: Notification matching on managementInterval ──────────────

    // 047_11_01: temporalQ with createdAt matches CSR managementInterval (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_11_01::management_interval_created_at"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_11_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-temporalQ-createdAt.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_11_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-managementInterval.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // 047_11_02: temporalQ with modifiedAt matches CSR managementInterval (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_11_02::management_interval_modified_at"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_11_02:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-temporalQ-modifiedAt.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_11_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-managementInterval.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_12: Notification matching on watchedAttributes ──────────────

    // 047_12_01: Subscription with entities + watchedAttributes filter (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_12_01::watched_attributes"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_12_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-watchedAttributes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_12_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-detailed-information.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_13: Notification matching on any attribute (no watchedAttrs) ─

    // 047_13_01: Without watchedAttributes, all attributes match (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_13_01::any_watched_attribute"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_13_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_13_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-detailed-information.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_14: Notification matching on location (geoQ) ────────────────

    // 047_14_01: geoQ with geoproperty matches CSR location (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_14_01::matching_location"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_14_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-geoQ.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_14_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-location.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_15: Notification matching on location as default geoproperty ─

    // 047_15_01: geoQ without geoproperty defaults to "location" (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_15_01::location_as_default"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:047_15_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-geoQ-without-geoproperty.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_15_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_id);
        let csr = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-location.jsonld", &csr_id);
        let resp = ctx.client.create_registration(&csr).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
    }));

    // ── 047_16: Notification for newly matching CSRs after sub update ────

    // 047_16_01: Update subscription to Vehicle → matches first CSR only (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_16_01::newly_matching_first_csr"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        // Create two CSRs with different entity types
        let csr_vehicle_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_01_v:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_vehicle_id);
        let csr_vehicle = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-vehicle-entities.jsonld", &csr_vehicle_id);
        let resp = ctx.client.create_registration(&csr_vehicle).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_bus_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_01_b:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_bus_id);
        let csr_bus = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-bus-entities.jsonld", &csr_bus_id);
        let resp = ctx.client.create_registration(&csr_bus).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create subscription filtering Building (won't match either CSR initially)
        let sub_id = format!("urn:ngsi-ld:Subscription:047_16_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Update subscription to filter Vehicle entities
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-vehicle-entities.json");
        let resp = ctx
            .client
            .update_csource_subscription(&sub_id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
        let data = notif.body["data"].as_array().expect("data should be an array");
        assert_eq!(data.len(), 1, "should match exactly one CSR");
    }));

    // 047_16_02: Update subscription to Bus → matches second CSR only (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_16_02::newly_matching_second_csr"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_vehicle_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_02_v:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_vehicle_id);
        let csr_vehicle = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-vehicle-entities.jsonld", &csr_vehicle_id);
        let resp = ctx.client.create_registration(&csr_vehicle).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_bus_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_02_b:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_bus_id);
        let csr_bus = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-bus-entities.jsonld", &csr_bus_id);
        let resp = ctx.client.create_registration(&csr_bus).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_16_02:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Update subscription to filter Bus entities
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-bus-entities.json");
        let resp = ctx
            .client
            .update_csource_subscription(&sub_id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
        let data = notif.body["data"].as_array().expect("data should be an array");
        assert_eq!(data.len(), 1, "should match exactly one CSR");
    }));

    // 047_16_03: Update subscription to Vehicle+Bus → matches both CSRs (5.11.7)
    trials.push(trial(&format!("{PREFIX}::047_16_03::newly_matching_both_csrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let csr_vehicle_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_03_v:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_vehicle_id);
        let csr_vehicle = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-vehicle-entities.jsonld", &csr_vehicle_id);
        let resp = ctx.client.create_registration(&csr_vehicle).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let csr_bus_id = format!("urn:ngsi-ld:ContextSourceRegistration:047_16_03_b:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&csr_bus_id);
        let csr_bus = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-bus-entities.jsonld", &csr_bus_id);
        let resp = ctx.client.create_registration(&csr_bus).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:047_16_03:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&sub_id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Update subscription to filter both Vehicle and Bus entities
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-vehicle-and-bus-entities.json");
        let resp = ctx
            .client
            .update_csource_subscription(&sub_id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        assert_eq!(notif.body["triggerReason"], "newlyMatching");
        let data = notif.body["data"].as_array().expect("data should be an array");
        assert_eq!(data.len(), 2, "should match both CSRs");
    }));
}
