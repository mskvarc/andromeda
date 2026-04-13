#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entity_attributes::replace";

pub fn tests(trials: &mut Vec<Trial>) {
    // 055_01_01: Replace existing attribute
    trials.push(trial(&format!("{PREFIX}::055_01::replace_existing_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_01_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-multi-instances.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/vehicle-isParked-attribute.json");

        let resp = ctx
            .client
            .request(
                Method::PUT,
                &format!("/entities/{id}/attrs/isParked"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["isParked"]["type"], "Relationship");
        pretty_assertions::assert_eq!(body["isParked"]["object"], "urn:ngsi-ld:OffStreetParking:Downtown2");
    }));

    // 055_01_02: Replace attribute with new type
    trials.push(trial(&format!("{PREFIX}::055_01::replace_attribute_with_new_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_01_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-multi-instances.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/vehicle-isParked-new-type-attribute.json");

        let resp = ctx
            .client
            .request(
                Method::PUT,
                &format!("/entities/{id}/attrs/isParked"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["isParked"]["type"], "Property");
    }));

    // 055_02_01: Replace default instance of multi-instance attribute
    trials.push(trial(&format!("{PREFIX}::055_02::replace_default_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_02_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-no-datasetid-fragment.jsonld");

        let resp = ctx.client.replace_entity_attr(&id, "speed", &fragment).await;
        // vehicle-speed-two-datasetid has no default instance (both have datasetId), so this returns 404
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 055_02_02: Replace specific instance by datasetId
    trials.push(trial(&format!("{PREFIX}::055_02::replace_specific_instance_by_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_02_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-equal-datasetid-fragment.jsonld");

        let resp = ctx
            .client
            .request(
                Method::PUT,
                &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Property:speedometer"),
                Some(&fragment),
                Default::default(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 055_03_01: Replace with invalid entity ID returns 400
    trials.push(trial(&format!("{PREFIX}::055_03::replace_invalid_entity_id"), |ctx| async move {
        let replacement = serde_json::json!({
            "type": "Property",
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::PUT, "/entities/not-a-valid-uri/attrs/speed", Some(&replacement), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 055_03_02: Replace on non-existent entity returns 404
    trials.push(trial(&format!("{PREFIX}::055_03::replace_nonexistent_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_03_02");
        let replacement = serde_json::json!({
            "type": "Property",
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.replace_entity_attr(&id, "speed", &replacement).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 055_03_03: Replace with invalid attribute name returns 400
    trials.push(trial(&format!("{PREFIX}::055_03::replace_invalid_attribute_name"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_03_03");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let replacement = serde_json::json!({
            "type": "Property",
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::PUT, &format!("/entities/{id}/attrs/@invalid"), Some(&replacement), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 055_03_04: Replace non-existent attribute returns 404
    trials.push(trial(&format!("{PREFIX}::055_03::replace_nonexistent_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:055_03_04");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let replacement = serde_json::json!({
            "type": "Property",
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.replace_entity_attr(&id, "nonExistentAttr", &replacement).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 055_03_05: Replace scope attribute returns 400
    trials.push(trial(&format!("{PREFIX}::055_03::replace_scope_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:055_03_05");

        let entity = fixtures::load_fixture("entities/building-minimal-with-one-scope.json", &id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities",
                Some(&entity),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED, "create entity should return 201");

        let replacement = serde_json::json!({
            "type": "Property",
            "value": "/Barcelona",
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.replace_entity_attr(&id, "scope", &replacement).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));
}
