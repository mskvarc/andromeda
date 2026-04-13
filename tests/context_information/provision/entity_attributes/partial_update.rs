#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entity_attributes::partial_update";

pub fn tests(trials: &mut Vec<Trial>) {
    // 012_01_01: Partial update attribute value (no datasetId)
    trials.push(trial(&format!("{PREFIX}::012_01::partial_update_attribute_no_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_01_01");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-isparked-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "isParked", &fragment).await;
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
        pretty_assertions::assert_eq!(body["isParked"]["providedBy"]["object"], "urn:ngsi-ld:Person:JohnDoe");
    }));

    // 012_01_02: Partial update attribute with specific datasetId
    trials.push(trial(
        &format!("{PREFIX}::012_01::partial_update_attribute_with_dataset_id"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_01_02");

            let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-equal-datasetid-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Property:speedometer"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_02_01: Partial update with empty entity ID
    trials.push(trial(&format!("{PREFIX}::012_02::partial_update_empty_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::PATCH, "/entities//attrs/speed", Some(&fragment), Default::default())
            .await;
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::NOT_FOUND || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "expected error status, got {}",
            resp.status
        );
    }));

    // 012_02_02: Partial update with invalid URI entity ID
    trials.push(trial(&format!("{PREFIX}::012_02::partial_update_invalid_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::PATCH, "/entities/not-a-valid-uri/attrs/speed", Some(&fragment), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 012_02_03: Partial update with mismatched attribute type
    trials.push(trial(
        &format!("{PREFIX}::012_02::partial_update_mismatched_attribute_type"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_02_03");

            let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-equal-datasetid-different-type-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Property:speedometer"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        },
    ));

    // 012_02_04: Partial update with empty fragment
    trials.push(trial(&format!("{PREFIX}::012_02::partial_update_empty_fragment"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_02_04");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request_raw(Method::PATCH, &format!("/entities/{id}/attrs/speed"), b"{}", "application/ld+json")
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 012_03_01: Partial update non-existent entity
    trials.push(trial(&format!("{PREFIX}::012_03::partial_update_nonexistent_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_03_01");
        let fragment = serde_json::json!({
            "value": 100,
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.partial_update_attr(&id, "speed", &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 012_03_02: Partial update with no default instance (no datasetId)
    trials.push(trial(&format!("{PREFIX}::012_03::partial_update_no_default_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_03_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-no-datasetid-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "speed", &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 012_03_03: Partial update with non-existent datasetId
    trials.push(trial(&format!("{PREFIX}::012_03::partial_update_nonexistent_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_03_03");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-unknown-datasetid-fragment.jsonld");

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/entities/{id}/attrs/speed?datasetId=urn:ngsi-ld:Property:unknownDatasetId"),
                Some(&fragment),
                Default::default(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 012_03_04: Partial update non-existent attribute name
    trials.push(trial(&format!("{PREFIX}::012_03::partial_update_nonexistent_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:012_03_04");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-no-datasetid-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "nonExistentAttr", &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 012_04_01: Partial update LanguageProperty
    trials.push(trial(&format!("{PREFIX}::012_04::partial_update_language_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:012_04_01");

        let entity = fixtures::load_fixture("entities/building-language-property-sub-property.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-language-property-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "street", &fragment).await;
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
        pretty_assertions::assert_eq!(body["street"]["languageMap"]["es"], "Gran Lugar");
    }));

    // 012_05_01: Delete Property via null in partial update
    trials.push(trial(
        &format!("{PREFIX}::012_05::partial_update_delete_property_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_05_01");

            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "name", &fragment).await;
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
            assert!(body.get("name").is_none(), "name should be deleted");
        },
    ));

    // 012_05_02: Delete Relationship via null in partial update
    trials.push(trial(
        &format!("{PREFIX}::012_05::partial_update_delete_relationship_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_05_02");

            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "locatedAt", &fragment).await;
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
            assert!(body.get("locatedAt").is_none(), "locatedAt should be deleted");
        },
    ));

    // 012_05_03: Delete GeoProperty via null in partial update
    trials.push(trial(
        &format!("{PREFIX}::012_05::partial_update_delete_geo_property_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_05_03");

            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "location", &fragment).await;
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
            assert!(body.get("location").is_none(), "location should be deleted");
        },
    ));

    // 012_05_04: Delete LanguageProperty via null in partial update
    trials.push(trial(
        &format!("{PREFIX}::012_05::partial_update_delete_language_property_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_05_04");

            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "street", &fragment).await;
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
            assert!(body.get("street").is_none(), "street should be deleted");
        },
    ));

    // 012_06_01: Delete specific Property instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_06::partial_update_delete_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_06_01");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-instance-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/name?datasetId=urn:ngsi-ld:Dataset:01"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_06_02: Delete specific Relationship instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_06::partial_update_delete_relationship_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_06_02");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-instance-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/locatedAt?datasetId=urn:ngsi-ld:Dataset:01"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_06_03: Delete specific GeoProperty instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_06::partial_update_delete_geo_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_06_03");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty-instance-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/location?datasetId=urn:ngsi-ld:Dataset:01"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_06_04: Delete specific LanguageProperty instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_06::partial_update_delete_language_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_06_04");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-instance-fragment.jsonld");

            let resp = ctx
                .client
                .request(
                    Method::PATCH,
                    &format!("/entities/{id}/attrs/street?datasetId=urn:ngsi-ld:Dataset:01"),
                    Some(&fragment),
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_07_01: Delete default Property instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_07::partial_update_delete_default_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_07_01");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "name", &fragment).await;
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
            assert!(body.get("name").is_some());
        },
    ));

    // 012_07_02: Delete default Relationship instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_07::partial_update_delete_default_relationship_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_07_02");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "locatedAt", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_07_03: Delete default GeoProperty instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_07::partial_update_delete_default_geo_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_07_03");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "location", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_07_04: Delete default LanguageProperty instance via null
    trials.push(trial(
        &format!("{PREFIX}::012_07::partial_update_delete_default_language_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:012_07_04");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-fragment.jsonld");

            let resp = ctx.client.partial_update_attr(&id, "street", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 012_08_01: Partial update JsonProperty
    trials.push(trial(&format!("{PREFIX}::012_08::partial_update_json_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:012_08_01");

        let entity = fixtures::load_fixture("entities/building-json-property-object-sub-property.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-json-property-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "jsonProperty", &fragment).await;
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
        pretty_assertions::assert_eq!(body["jsonProperty"]["json"]["id"], "another-id");
    }));

    // 012_09_01: Partial update VocabProperty
    trials.push(trial(&format!("{PREFIX}::012_09::partial_update_vocab_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:012_09_01");

        let entity = fixtures::load_fixture("entities/building-vocab-property-sub-property.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/building-vocab-property-fragment.jsonld");

        let resp = ctx.client.partial_update_attr(&id, "vocabProperty", &fragment).await;
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
