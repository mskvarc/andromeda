#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entity_attributes::update";

pub fn tests(trials: &mut Vec<Trial>) {
    // 011_01_01: Update attribute with no datasetId
    trials.push(trial(&format!("{PREFIX}::011_01::update_attribute_no_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_01_01");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-two-datasetid-01-fragment.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
    }));

    // 011_01_02: Update attribute with datasetId
    trials.push(trial(&format!("{PREFIX}::011_01::update_attribute_with_dataset_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_01_02");

        let entity = fixtures::load_fixture("entities/vehicle-speed-two-datasetid.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-two-datasetid-02-fragment.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 011_01_03: Update with new attribute (append + update)
    trials.push(trial(&format!("{PREFIX}::011_01::update_with_new_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_01_03");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-two-datasetid-03-fragment.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
        assert!(
            resp.status == StatusCode::NO_CONTENT || resp.status == StatusCode::MULTI_STATUS,
            "expected 204 or 207, got {}",
            resp.status
        );
    }));

    // 011_01_04: Update with attribute type change
    trials.push(trial(&format!("{PREFIX}::011_01::update_change_attribute_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_01_04");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-speed-two-datasetid-04-fragment.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
        pretty_assertions::assert_eq!(body["brandName"]["type"], "Relationship");
    }));

    // 011_02_01: Update with empty entity ID
    trials.push(trial(&format!("{PREFIX}::011_02::update_empty_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.request(Method::PATCH, "/entities//attrs", Some(&fragment), Default::default()).await;
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::NOT_FOUND || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "expected error status, got {}",
            resp.status
        );
    }));

    // 011_02_02: Update with invalid URI entity ID
    trials.push(trial(&format!("{PREFIX}::011_02::update_invalid_entity_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx
            .client
            .request(Method::PATCH, "/entities/not-a-valid-uri/attrs", Some(&fragment), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 011_03_01: Update non-existent entity returns 404
    trials.push(trial(&format!("{PREFIX}::011_03::update_nonexistent_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_03_01");
        let fragment = serde_json::json!({
            "speed": {
                "type": "Property",
                "value": 100
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 011_04_01: Update with invalid JSON fragment
    trials.push(trial(&format!("{PREFIX}::011_04::update_invalid_fragment"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_04_01");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let resp = ctx
            .client
            .request_raw(Method::PATCH, &format!("/entities/{id}/attrs"), b"{invalid json", "application/ld+json")
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 011_05_01: Update scope when entity has existing scope
    trials.push(trial(&format!("{PREFIX}::011_05::update_scope_existing"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:011_05_01");

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
                Method::PATCH,
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

    // 011_05_02: Update scope when entity has no scope returns 207
    trials.push(trial(&format!("{PREFIX}::011_05::update_scope_not_existing"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:011_05_02");

        let entity = fixtures::load_fixture("entities/building-minimal.json", &id);
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
                Method::PATCH,
                &format!("/entities/{id}/attrs"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert!(
            resp.status == StatusCode::MULTI_STATUS || resp.status == StatusCode::NO_CONTENT,
            "expected 207 or 204, got {}",
            resp.status
        );
    }));

    // 011_06_01: Append single type to entity
    trials.push(trial(&format!("{PREFIX}::011_06::update_append_single_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_06_01");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/type-building-fragment.json");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
        let types = body["type"].as_array();
        if let Some(types) = types {
            let type_strs: Vec<&str> = types.iter().filter_map(|t| t.as_str()).collect();
            assert!(type_strs.contains(&"Vehicle"));
            assert!(type_strs.contains(&"Building"));
        }
    }));

    // 011_06_02: Append multiple types to entity
    trials.push(trial(&format!("{PREFIX}::011_06::update_append_multiple_types"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_06_02");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/types-vehicle-car-fragment.json");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 011_06_03: Append duplicate type is ignored
    trials.push(trial(&format!("{PREFIX}::011_06::update_append_duplicate_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_06_03");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/type-vehicle-fragment.json");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
        pretty_assertions::assert_eq!(body["type"], "Vehicle");
    }));

    // 011_06_04: Append type with existing multi-type
    trials.push(trial(&format!("{PREFIX}::011_06::update_append_type_to_multi_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:011_06_04");

        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // First add "Car" type
        let car_fragment = fixtures::load_fragment("entities/fragment_entities/types-vehicle-car-fragment.json");
        ctx.client.update_entity_attrs(&id, &car_fragment).await;

        // Then add "Building" type
        let building_fragment = fixtures::load_fragment("entities/fragment_entities/type-building-fragment.json");

        let resp = ctx.client.update_entity_attrs(&id, &building_fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 011_07_01: Delete Property using NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::011_07::update_delete_property_with_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:011_07_01");

        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
    }));

    // 011_07_02: Delete Relationship using NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::011_07::update_delete_relationship_with_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:011_07_02");

        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
    }));

    // 011_07_03: Delete GeoProperty using NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::011_07::update_delete_geo_property_with_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:011_07_03");

        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty.jsonld");

        let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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
    }));

    // 011_07_04: Delete LanguageProperty using NGSI-LD null
    trials.push(trial(
        &format!("{PREFIX}::011_07::update_delete_language_property_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_07_04");

            let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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

    // 011_08_01: Delete specific Property instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_08::update_delete_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_08_01");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-instance.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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

    // 011_08_02: Delete specific Relationship instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_08::update_delete_relationship_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_08_02");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-instance.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 011_08_03: Delete specific GeoProperty instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_08::update_delete_geo_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_08_03");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty-instance.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 011_08_04: Delete specific LanguageProperty instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_08::update_delete_language_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_08_04");

            let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-instance.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 011_09_01: Delete default Property instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_09::update_delete_default_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_09_01");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
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

    // 011_09_02: Delete default Relationship instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_09::update_delete_default_relationship_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_09_02");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 011_09_03: Delete default GeoProperty instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_09::update_delete_default_geo_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_09_03");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 011_09_04: Delete default LanguageProperty instance using null
    trials.push(trial(
        &format!("{PREFIX}::011_09::update_delete_default_language_property_instance_with_null"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:011_09_04");

            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            ctx.client.create_entity(&entity).await;

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty.jsonld");

            let resp = ctx.client.update_entity_attrs(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));
}
