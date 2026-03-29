use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::join";

/// Helper to set up linked entity test: creates City entity, Building entity with
/// locatedAt→City relationship, and subscription with join mode.
/// Returns (entity_id, sub_id).
async fn setup_join_test(ctx: &crate::harness::TestCtx, test_id: &str, sub_fixture: &str, receiver: &NotificationReceiver) -> (String, String) {
    // Create the City entity with the ID that building-relationship.jsonld references
    let city_id = "urn:ngsi-ld:City:Paris";
    ctx.track_entity(city_id);
    let city = fixtures::load_fixture("entities/city-minimal.jsonld", city_id);
    let resp = ctx.client.create_entity(&city).await;
    assert_eq!(resp.status, StatusCode::CREATED);

    // Create Building entity with locatedAt→City:Paris relationship
    let entity_id = ctx.entity_id(&format!("urn:ngsi-ld:Building:{test_id}"));
    let entity = fixtures::load_fixture("entities/building-relationship.jsonld", &entity_id);
    let resp = ctx.client.create_entity(&entity).await;
    assert_eq!(resp.status, StatusCode::CREATED);

    let sub_id = format!("urn:ngsi-ld:Subscription:{test_id}:{}", uuid::Uuid::new_v4());
    ctx.track_subscription(&sub_id);
    let mut sub = fixtures::load_fixture(sub_fixture, &sub_id);
    set_endpoint_uri(&mut sub, &receiver.endpoint_url());
    let resp = ctx.client.create_subscription(&sub).await;
    assert_eq!(resp.status, StatusCode::CREATED);

    (entity_id, sub_id)
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_23: Join flat ───────────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_23::join_flat"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(&ctx, "046_23", "subscriptions/subscription-building-entities-join-flat.jsonld", &receiver).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        // Flat join: both Building and City should be separate entries in data
        assert!(data.len() >= 2, "flat join should include linked entity, got {} entries", data.len());
    }));

    // ── 046_24: Join flat + sysAttrs ────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_24::join_flat_sys_attrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_24",
            "subscriptions/subscription-building-entities-join-flat-sysAttrs.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(data.len() >= 2, "flat join should include linked entity");
        // Verify sysAttrs present
        assert!(data[0].get("createdAt").is_some(), "should contain createdAt");
    }));

    // ── 046_25: Join flat + simplified ──────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_25::join_flat_simplified"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_25",
            "subscriptions/subscription-building-entities-join-flat-simplified.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(data.len() >= 2, "flat join should include linked entity");
    }));

    // ── 046_26: Join flat + selected attributes ─────────────────────
    trials.push(trial(&format!("{PREFIX}::046_26::join_flat_attributes"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_26",
            "subscriptions/subscription-building-entities-join-flat-attributes.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 046_27: Join inline ─────────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_27::join_inline"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(&ctx, "046_27", "subscriptions/subscription-building-entities-join-inline.jsonld", &receiver).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        // Inline join: City entity should be embedded within the locatedAt relationship
        assert_eq!(data[0]["id"], entity_id);
    }));

    // ── 046_28: Join inline + sysAttrs ──────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_28::join_inline_sys_attrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_28",
            "subscriptions/subscription-building-entities-join-inline-sysAttrs.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0].get("createdAt").is_some(), "should contain createdAt");
    }));

    // ── 046_29: Join inline + simplified ────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_29::join_inline_simplified"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_29",
            "subscriptions/subscription-building-entities-join-inline-simplified.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 046_30: Join inline + selected attributes ───────────────────
    trials.push(trial(&format!("{PREFIX}::046_30::join_inline_attributes"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;
        let (entity_id, sub_id) = setup_join_test(
            &ctx,
            "046_30",
            "subscriptions/subscription-building-entities-join-inline-attributes.jsonld",
            &receiver,
        )
        .await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));
}
