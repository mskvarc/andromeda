use std::time::Duration;

use crate::harness::{client::QueryParams, fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::attribute_deleted";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_22_01: attributeDeleted keyValues ───────────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_01::attr_deleted_key_values"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-attributeDeleted-keyValues.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "name").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);
    }));

    // ── 046_22_02: attributeDeleted specific attribute keyValues ────
    trials.push(trial(&format!("{PREFIX}::046_22_02::specific_attr_key_values"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-keyValues.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "name").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], entity_id);
    }));

    // ── 046_22_03: Non-watched attribute → no notification ──────────
    trials.push(trial(&format!("{PREFIX}::046_22_03::non_watched_no_notification"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        // watches "name" only
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-keyValues.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Delete airQualityLevel (not watched)
        let resp = ctx.client.delete_entity_attr(&entity_id, "airQualityLevel").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        receiver.wait_for_no_notification(Duration::from_secs(3)).await;
    }));

    // ── 046_22_04: attributeDeleted normalized ──────────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_04::attr_deleted_normalized"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_04");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_04:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "name").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);
        // Without sysAttrs, the deleted attribute is nullified but has no deletedAt
        assert_eq!(data[0]["name"]["value"], "urn:ngsi-ld:null", "name should be nullified");
    }));

    // ── 046_22_05: attributeDeleted normalized + sysAttrs ───────────
    trials.push(trial(&format!("{PREFIX}::046_22_05::attr_deleted_normalized_sys_attrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_05");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_05:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized-sysAttrs.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "name").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        let name_attr = &data[0]["name"];
        assert!(name_attr.get("deletedAt").is_some(), "name should have deletedAt");
        assert!(name_attr.get("createdAt").is_some(), "name should have createdAt with sysAttrs");
    }));

    // ── 046_22_06: Multi-instance datasetId deletion ────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_06::multi_instance_dataset_id"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_06");
        let entity = fixtures::load_fixture("entities/building-multi-instances-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_06:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .delete_entity_attr_with_params(&entity_id, "name", &QueryParams::new().dataset_id("urn:ngsi-ld:Dataset:french-name"))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);
    }));

    // ── 046_22_07: attributeDeleted keyValues datasetId ─────────────
    trials.push(trial(&format!("{PREFIX}::046_22_07::key_values_dataset_id"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_07");
        let entity = fixtures::load_fixture("entities/building-multi-instances-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_07:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-keyValues.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .delete_entity_attr_with_params(&entity_id, "name", &QueryParams::new().dataset_id("urn:ngsi-ld:Dataset:french-name"))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 046_22_08: deleteAll=true ───────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_08::delete_all"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_08");
        let entity = fixtures::load_fixture("entities/building-multi-instances-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_08:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-attributeDeleted-keyValues.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .delete_entity_attr_with_params(&entity_id, "name", &QueryParams::new().delete_all(true))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
    }));

    // ── 046_22_09_01: NGSI-LD null via merge ────────────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_09_01::null_via_merge"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_09_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_09_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let null_fragment = fixtures::load_fixture("entities/fragment_entities/ngsild_null/building-null-property.jsonld", &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &null_fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["name"]["value"], "urn:ngsi-ld:null", "name should be nullified");
    }));

    // ── 046_22_09_02: NGSI-LD null via update_entity_attrs ──────────
    trials.push(trial(&format!("{PREFIX}::046_22_09_02::null_via_update_attrs"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_09_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_09_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property.jsonld");
        let resp = ctx.client.update_entity_attrs(&entity_id, &null_fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["name"]["value"], "urn:ngsi-ld:null", "name should be nullified");
    }));

    // ── 046_22_09_03: NGSI-LD null via partial_update ───────────────
    trials.push(trial(&format!("{PREFIX}::046_22_09_03::null_via_partial_update"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_09_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_09_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-fragment.jsonld");
        let resp = ctx.client.partial_update_attr(&entity_id, "name", &null_fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["name"]["value"], "urn:ngsi-ld:null", "name should be nullified");
    }));

    // ── 046_22_10_01: Relationship null via merge ───────────────────
    trials.push(trial(&format!("{PREFIX}::046_22_10_01::relationship_null_via_merge"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_10_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_10_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-relationship-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let null_fragment = fixtures::load_fixture("entities/fragment_entities/ngsild_null/building-null-relationship.jsonld", &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &null_fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["locatedAt"]["object"], "urn:ngsi-ld:null", "locatedAt should be nullified");
    }));

    // ── 046_22_10_02: Relationship null via update_entity_attrs ─────
    trials.push(trial(
        &format!("{PREFIX}::046_22_10_02::relationship_null_via_update_attrs"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_10_02");
            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_22_10_02:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture(
                "subscriptions/subscription-building-entities-attributeDeleted-relationship-normalized.jsonld",
                &sub_id,
            );
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship.jsonld");
            let resp = ctx.client.update_entity_attrs(&entity_id, &null_fragment).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            assert_eq!(notif.body["subscriptionId"], sub_id);
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            assert_eq!(data[0]["locatedAt"]["object"], "urn:ngsi-ld:null", "locatedAt should be nullified");
        },
    ));

    // ── 046_22_10_03: Relationship null via partial_update ──────────
    trials.push(trial(
        &format!("{PREFIX}::046_22_10_03::relationship_null_via_partial_update"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_10_03");
            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_22_10_03:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture(
                "subscriptions/subscription-building-entities-attributeDeleted-relationship-normalized.jsonld",
                &sub_id,
            );
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-fragment.jsonld");
            let resp = ctx.client.partial_update_attr(&entity_id, "locatedAt", &null_fragment).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            assert_eq!(notif.body["subscriptionId"], sub_id);
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            assert_eq!(data[0]["locatedAt"]["object"], "urn:ngsi-ld:null", "locatedAt should be nullified");
        },
    ));

    // ── 046_22_11_01: LanguageProperty null via merge ───────────────
    trials.push(trial(&format!("{PREFIX}::046_22_11_01::language_property_null_via_merge"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_11_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_11_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-languageproperty-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let null_fragment = fixtures::load_fixture("entities/fragment_entities/ngsild_null/building-null-languageproperty.jsonld", &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &null_fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(
            data[0]["street"]["languageMap"],
            serde_json::json!({"@none": "urn:ngsi-ld:null"}),
            "street should be nullified"
        );
    }));

    // ── 046_22_11_02: LanguageProperty null via update_entity_attrs ─
    trials.push(trial(
        &format!("{PREFIX}::046_22_11_02::language_property_null_via_update_attrs"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_11_02");
            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_22_11_02:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture(
                "subscriptions/subscription-building-entities-attributeDeleted-languageproperty-normalized.jsonld",
                &sub_id,
            );
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty.jsonld");
            let resp = ctx.client.update_entity_attrs(&entity_id, &null_fragment).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            assert_eq!(notif.body["subscriptionId"], sub_id);
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            assert_eq!(
                data[0]["street"]["languageMap"],
                serde_json::json!({"@none": "urn:ngsi-ld:null"}),
                "street should be nullified"
            );
        },
    ));

    // ── 046_22_11_03: LanguageProperty null via partial_update ──────
    trials.push(trial(
        &format!("{PREFIX}::046_22_11_03::language_property_null_via_partial_update"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_11_03");
            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_22_11_03:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture(
                "subscriptions/subscription-building-entities-attributeDeleted-languageproperty-normalized.jsonld",
                &sub_id,
            );
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let null_fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-fragment.jsonld");
            let resp = ctx.client.partial_update_attr(&entity_id, "street", &null_fragment).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            assert_eq!(notif.body["subscriptionId"], sub_id);
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            assert_eq!(
                data[0]["street"]["languageMap"],
                serde_json::json!({"@none": "urn:ngsi-ld:null"}),
                "street should be nullified"
            );
        },
    ));

    // ── 046_22_12: attributeDeleted on entity deletion ──────────────
    trials.push(trial(&format!("{PREFIX}::046_22_12::attr_deleted_on_entity_deletion"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_12");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_22_12:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture(
            "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized.jsonld",
            &sub_id,
        );
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["subscriptionId"], sub_id);
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);
        assert_eq!(data[0]["name"]["value"], "urn:ngsi-ld:null", "name should be nullified");
    }));

    // ── 046_22_13: attributeDeleted on entity deletion + sysAttrs ───
    trials.push(trial(
        &format!("{PREFIX}::046_22_13::attr_deleted_on_entity_deletion_sys_attrs"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_22_13");
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_22_13:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture(
                "subscriptions/subscription-building-entities-attributeDeleted-specific-attribute-normalized-sysAttrs.jsonld",
                &sub_id,
            );
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.delete_entity(&entity_id).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            assert_eq!(notif.body["subscriptionId"], sub_id);
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            let name_attr = &data[0]["name"];
            assert!(name_attr.get("deletedAt").is_some(), "name should have deletedAt");
            assert!(name_attr.get("createdAt").is_some(), "name should have createdAt with sysAttrs");
        },
    ));
}
