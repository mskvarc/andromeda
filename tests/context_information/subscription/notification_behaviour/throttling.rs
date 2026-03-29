use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::throttling";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_15: Throttling ──────────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_15::throttling_window"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_15");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_15:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        // throttling=15 in this fixture
        let mut sub = fixtures::load_fixture("subscriptions/subscription-valid-throttling.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // First update → should trigger notification
        let fragment = fixtures::load_fragment("entities/fragment_entities/airQualityLevel-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let _notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        // Second update immediately → should NOT trigger (within throttling window)
        let fragment2 = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment2).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        receiver.wait_for_no_notification(Duration::from_secs(5)).await;
    }));
}
