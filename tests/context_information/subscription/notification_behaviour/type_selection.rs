use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::type_selection";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_16_01: Matching entity type ─────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_16_01::matching_type"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:046_16_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-type-selection.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_16_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 046_16_02: Non-matching entity type ─────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_16_02::non_matching_type"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:046_16_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-type-selection.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:046_16_02");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        receiver.wait_for_no_notification(Duration::from_secs(3)).await;
    }));
}
