use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::entity_deleted";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_21_01: entityDeleted trigger ────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_21_01::entity_deleted"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_21_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_21_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-entityDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], entity_id);
        assert!(data[0].get("deletedAt").is_some(), "should contain deletedAt");
    }));

    // ── 046_21_02: entityDeleted with sysAttrs ──────────────────────
    trials.push(trial(&format!("{PREFIX}::046_21_02::entity_deleted_sys_attrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_21_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_21_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-entityDeleted-sysAttrs.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], entity_id);
        assert!(data[0].get("createdAt").is_some(), "should contain createdAt");
        assert!(data[0].get("deletedAt").is_some(), "should contain deletedAt");
    }));
}
