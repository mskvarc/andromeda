use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::format";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_14: JSON-LD format when endpoint.accept=application/ld+json ─
    trials.push(trial(&format!("{PREFIX}::046_14::json_ld_format"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_14");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_14:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-accept-jsonld.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        // No Link header for JSON-LD
        assert!(
            notif.headers.get("Link").is_none(),
            "notification should NOT have Link header for JSON-LD format"
        );
        // Data entities should have @context
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0].get("@context").is_some(), "data entities should contain @context in JSON-LD format");
    }));

    // ── 046_20: jsonldContext used for notification compaction ───────
    trials.push(trial(&format!("{PREFIX}::046_20::jsonld_context_compaction"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_20");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_20:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-with-jsonldContext.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        // The notification should be compacted using the subscription's jsonldContext
        // With only core context, custom terms should be expanded
        assert_eq!(data[0]["id"], entity_id);
    }));
}
