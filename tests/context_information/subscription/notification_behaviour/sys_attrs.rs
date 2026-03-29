use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::sys_attrs";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_17: sysAttrs=true → createdAt/modifiedAt included ──────
    trials.push(trial(&format!("{PREFIX}::046_17::sys_attrs_true"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_17");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_17:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-with-true-sysAttrs-parameter.jsonld", &sub_id);
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
        assert!(entity_data.get("createdAt").is_some(), "should contain createdAt when sysAttrs=true");
        assert!(entity_data.get("modifiedAt").is_some(), "should contain modifiedAt when sysAttrs=true");
    }));

    // ── 046_18: sysAttrs=false → no createdAt/modifiedAt ───────────
    trials.push(trial(&format!("{PREFIX}::046_18::sys_attrs_false"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_18");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_18:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-with-false-sysAttrs-parameter.jsonld", &sub_id);
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
        assert!(entity_data.get("createdAt").is_none(), "should NOT contain createdAt when sysAttrs=false");
        assert!(entity_data.get("modifiedAt").is_none(), "should NOT contain modifiedAt when sysAttrs=false");
    }));
}
