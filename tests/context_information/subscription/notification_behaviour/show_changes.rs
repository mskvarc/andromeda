use std::time::Duration;

use crate::harness::{fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

use super::set_endpoint_uri;

const PREFIX: &str = "subscription::notification_behaviour::show_changes";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 046_31_01: showChanges - Property update previousValue ──────
    trials.push(trial(&format!("{PREFIX}::046_31_01::previous_value_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_31_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_31_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["name"].get("previousValue").is_some(), "name should have previousValue");
    }));

    // ── 046_31_02: showChanges - Relationship update previousObject ─
    trials.push(trial(&format!("{PREFIX}::046_31_02::previous_value_relationship"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_31_02");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_31_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/locatedAt-fragment.jsonld");
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["locatedAt"].get("previousObject").is_some(), "locatedAt should have previousObject");
    }));

    // ── 046_31_03: showChanges - GeoProperty update previousValue ───
    trials.push(trial(&format!("{PREFIX}::046_31_03::previous_value_geoproperty"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_31_03");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_31_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/location-fragment.jsonld");
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["location"].get("previousValue").is_some(), "location should have previousValue");
    }));

    // ── 046_31_04: showChanges - LanguageProperty update previousLanguageMap
    trials.push(trial(&format!("{PREFIX}::046_31_04::previous_value_language_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_31_04");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_31_04:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/street-language-property-fragment.jsonld");
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["street"].get("previousLanguageMap").is_some(), "street should have previousLanguageMap");
    }));

    // ── 046_32: showChanges - JsonProperty update ───────────────────
    trials.push(trial(&format!("{PREFIX}::046_32::previous_value_json_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_32");
        let entity = fixtures::load_fixture("entities/building-json-property-object.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_32:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-json-property-fragment.jsonld");
        let resp = ctx.client.partial_update_attr(&entity_id, "jsonProperty", &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["jsonProperty"].get("previousJson").is_some(), "jsonProperty should have previousJson");
    }));

    // ── 046_33: showChanges - VocabProperty update ──────────────────
    trials.push(trial(&format!("{PREFIX}::046_33::previous_value_vocab_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_33");
        let entity = fixtures::load_fixture("entities/building-vocab-property-string.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_33:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-vocab-property-fragment.jsonld");
        let resp = ctx.client.partial_update_attr(&entity_id, "vocabProperty", &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["vocabProperty"].get("previousVocab").is_some(),
            "vocabProperty should have previousVocab"
        );
    }));

    // ── 046_34_01: showChanges - Property deletion previousValue ────
    trials.push(trial(&format!("{PREFIX}::046_34_01::deletion_previous_value_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_34_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_34_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "name").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(data[0]["name"].get("previousValue").is_some(), "name should have previousValue on deletion");
    }));

    // ── 046_34_02: showChanges - Relationship deletion previousObject
    trials.push(trial(&format!("{PREFIX}::046_34_02::deletion_previous_value_relationship"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_34_02");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_34_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "locatedAt").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["locatedAt"].get("previousObject").is_some(),
            "locatedAt should have previousObject on deletion"
        );
    }));

    // ── 046_34_03: showChanges - GeoProperty deletion previousValue ─
    trials.push(trial(&format!("{PREFIX}::046_34_03::deletion_previous_value_geoproperty"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_34_03");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_34_03:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "location").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["location"].get("previousValue").is_some(),
            "location should have previousValue on deletion"
        );
    }));

    // ── 046_34_04: showChanges - LanguageProperty deletion ──────────
    trials.push(trial(
        &format!("{PREFIX}::046_34_04::deletion_previous_value_language_property"),
        |ctx| async move {
            let receiver = NotificationReceiver::start().await;

            let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_34_04");
            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let sub_id = format!("urn:ngsi-ld:Subscription:046_34_04:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&sub_id);
            let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
            set_endpoint_uri(&mut sub, &receiver.endpoint_url());
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.delete_entity_attr(&entity_id, "street").await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
            let data = notif.body["data"].as_array().expect("data should be array");
            assert!(!data.is_empty());
            assert!(
                data[0]["street"].get("previousLanguageMap").is_some(),
                "street should have previousLanguageMap on deletion"
            );
        },
    ));

    // ── 046_35: showChanges - JsonProperty deletion ─────────────────
    trials.push(trial(&format!("{PREFIX}::046_35::deletion_previous_json_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_35");
        let entity = fixtures::load_fixture("entities/building-json-property-object.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_35:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "jsonProperty").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["jsonProperty"].get("previousJson").is_some(),
            "jsonProperty should have previousJson on deletion"
        );
    }));

    // ── 046_36: showChanges - VocabProperty deletion ────────────────
    trials.push(trial(&format!("{PREFIX}::046_36::deletion_previous_vocab_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_36");
        let entity = fixtures::load_fixture("entities/building-vocab-property-string.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_36:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attributeDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity_attr(&entity_id, "vocabProperty").await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["vocabProperty"].get("previousVocab").is_some(),
            "vocabProperty should have previousVocab on deletion"
        );
    }));

    // ── 046_37: showChanges - entity deletion all attributes ────────
    trials.push(trial(&format!("{PREFIX}::046_37::entity_deletion_all_previous_values"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_37");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_37:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-entityDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert_eq!(data[0]["id"], entity_id);
        // All attribute types should have their previous values
        assert!(data[0]["name"].get("previousValue").is_some(), "name should have previousValue");
        assert!(data[0]["locatedAt"].get("previousObject").is_some(), "locatedAt should have previousObject");
    }));

    // ── 046_38: showChanges - entity deletion JsonProperty ──────────
    trials.push(trial(&format!("{PREFIX}::046_38::entity_deletion_json_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_38");
        let entity = fixtures::load_fixture("entities/building-json-property-object.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_38:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-entityDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["jsonProperty"].get("previousJson").is_some(),
            "jsonProperty should have previousJson on entity deletion"
        );
    }));

    // ── 046_39: showChanges - entity deletion VocabProperty ─────────
    trials.push(trial(&format!("{PREFIX}::046_39::entity_deletion_vocab_property"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_39");
        let entity = fixtures::load_fixture("entities/building-vocab-property-string.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_39:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-entityDeleted.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&entity_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        assert!(
            data[0]["vocabProperty"].get("previousVocab").is_some(),
            "vocabProperty should have previousVocab on entity deletion"
        );
    }));

    // ── 046_40: showChanges - no previousValue on entityCreated ─────
    trials.push(trial(&format!("{PREFIX}::046_40::no_previous_value_on_entity_created"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:046_40:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-entity-created.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_40");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        // No previousValue should exist for entityCreated notifications
        let body_str = serde_json::to_string(&notif.body).unwrap();
        assert!(
            !body_str.contains("previousValue"),
            "entityCreated notification should NOT contain previousValue"
        );
        assert!(
            !body_str.contains("previousObject"),
            "entityCreated notification should NOT contain previousObject"
        );
        assert!(
            !body_str.contains("previousLanguageMap"),
            "entityCreated notification should NOT contain previousLanguageMap"
        );
    }));

    // ── 046_41: showChanges - no previousValue on attributeCreated ──
    trials.push(trial(&format!("{PREFIX}::046_41::no_previous_value_on_attribute_created"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let entity_id = ctx.entity_id("urn:ngsi-ld:Building:046_41");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let sub_id = format!("urn:ngsi-ld:Subscription:046_41:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let mut sub = fixtures::load_fixture("subscriptions/subscription-building-entities-show-changes-attribute-created.jsonld", &sub_id);
        set_endpoint_uri(&mut sub, &receiver.endpoint_url());
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Append a new attribute (name doesn't exist yet on building-minimal)
        let fragment = fixtures::load_fragment("entities/fragment_entities/name-fragment.jsonld");
        let resp = ctx.client.append_entity_attrs(&entity_id, &fragment).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        let data = notif.body["data"].as_array().expect("data should be array");
        assert!(!data.is_empty());
        // No previousValue should exist for attributeCreated notifications
        let body_str = serde_json::to_string(&notif.body).unwrap();
        assert!(
            !body_str.contains("previousValue"),
            "attributeCreated notification should NOT contain previousValue"
        );
        assert!(
            !body_str.contains("previousObject"),
            "attributeCreated notification should NOT contain previousObject"
        );
    }));
}
