#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entity_attributes::append";

pub fn tests(trials: &mut Vec<Trial>) {
    // 010_01_01: Append a new attribute to an existing entity
    trials.push(trial(&format!("{PREFIX}::010_01::append_new_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_01_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-new-attribute-fragment.jsonld");

        let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());

        let resp = ctx
            .client
            .get_entity(
                &id,
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["attribute_to_be_added"]["type"], "Property");
        pretty_assertions::assert_eq!(body["attribute_to_be_added"]["value"], 23);
        assert!(body.get("speed").is_some());
    }));

    // 010_01_02: Append attribute with different datasetId
    trials.push(trial(
        &format!("{PREFIX}::010_01::append_attribute_with_different_dataset_id"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_01_02");

            let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-different-datasetid-fragment.jsonld");

            let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
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
            assert!(body.get("speed").is_some());
        },
    ));

    // 010_02_01: Append with empty entity ID returns 400
    trials.push(trial(&format!("{PREFIX}::010_02::append_empty_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.request(Method::POST, "/entities//attrs", Some(&fragment), Default::default()).await;
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::NOT_FOUND || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "expected error status, got {}",
            resp.status
        );
    }));

    // 010_02_02: Append with invalid URI entity ID returns 400
    trials.push(trial(&format!("{PREFIX}::010_02::append_invalid_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::POST, "/entities/not-a-valid-uri/attrs", Some(&fragment), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some());
    }));

    // 010_03_01: Append to non-existent entity returns 404
    trials.push(trial(&format!("{PREFIX}::010_03::append_to_nonexistent_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_03_01");
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some());
    }));

    // 010_04_01: Append with noOverwrite on existing attribute returns 207
    trials.push(trial(&format!("{PREFIX}::010_04::append_no_overwrite_existing_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_04_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-attribute-to-add-fragment.jsonld");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs?options=noOverwrite"),
                Some(&fragment),
                Default::default(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        assert!(body.get("notUpdated").is_some() || body.get("updated").is_some());
    }));

    // 010_04_02: Append new multi-attribute instance with noOverwrite returns 204
    trials.push(trial(&format!("{PREFIX}::010_04::append_no_overwrite_new_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_04_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-different-datasetid-fragment.jsonld");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs?options=noOverwrite"),
                Some(&fragment),
                Default::default(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 010_05_01: Append with invalid JSON fragment returns 400
    trials.push(trial(&format!("{PREFIX}::010_05::append_invalid_fragment"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_05_01");

        let entity = fixtures::load_fixture("entities/vehicle-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request_raw(Method::POST, &format!("/entities/{id}/attrs"), b"{invalid json", "application/ld+json")
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 010_06_01: Append LanguageProperty attribute
    trials.push(trial(&format!("{PREFIX}::010_06::append_language_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_06_01");

        let entity = fixtures::load_fixture("entities/vehicle-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-new-language-property-fragment.jsonld");

        let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
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
        pretty_assertions::assert_eq!(body["street"]["type"], "LanguageProperty");
        pretty_assertions::assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
    }));

    // 010_07_01: Append scope with overwrite
    trials.push(trial(&format!("{PREFIX}::010_07::append_scope_with_overwrite"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:010_07_01");

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

        let fragment = fixtures::load_fragment("entities/fragment_entities/one-scope-fragment.json");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs"),
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
        pretty_assertions::assert_eq!(body["scope"], "/CompanyA/OrganizationB/UnitC");
    }));

    // 010_07_02: Append scope with noOverwrite
    trials.push(trial(&format!("{PREFIX}::010_07::append_scope_no_overwrite"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:010_07_02");

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

        let fragment = fixtures::load_fragment("entities/fragment_entities/one-scope-fragment.json");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs?options=noOverwrite"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert!(resp.status == StatusCode::NO_CONTENT || resp.status == StatusCode::MULTI_STATUS);
    }));

    // 010_08: Append observationSpace GeoProperty
    trials.push(trial(&format!("{PREFIX}::010_08::append_observation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:010_08");

        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/observation-space-fragment.json");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs"),
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
        pretty_assertions::assert_eq!(body["observationSpace"]["type"], "GeoProperty");
    }));

    // 010_09: Append operationSpace GeoProperty
    trials.push(trial(&format!("{PREFIX}::010_09::append_operation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:010_09");

        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/operation-space-fragment.json");

        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{id}/attrs"),
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
        pretty_assertions::assert_eq!(body["operationSpace"]["type"], "GeoProperty");
    }));

    // 010_10_01: Append JsonProperty attribute
    trials.push(trial(&format!("{PREFIX}::010_10::append_json_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_10_01");

        let entity = fixtures::load_fixture("entities/vehicle-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-new-json-property-fragment.jsonld");

        let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
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
        pretty_assertions::assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
        assert!(body["jsonProperty"]["json"].is_object());
    }));

    // 010_11_01: Append VocabProperty attribute
    trials.push(trial(&format!("{PREFIX}::010_11::append_vocab_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:010_11_01");

        let entity = fixtures::load_fixture("entities/vehicle-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-new-vocab-property-fragment.jsonld");

        let resp = ctx.client.append_entity_attrs(&id, &fragment).await;
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
        pretty_assertions::assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
    }));
}
