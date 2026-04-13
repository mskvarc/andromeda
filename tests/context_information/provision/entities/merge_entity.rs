#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entities::merge";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 056_01: Merge entity scenarios ──────────────────────────────────

    // 056_01_01: Merge empty fragment — entity should remain unchanged
    trials.push(trial(&format!("{PREFIX}::056_01_01::empty_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_01_01");
        let entity = fixtures::load_fixture("entities/merge/building-merge-data.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "merge should return 204");

        // Verify entity unchanged
        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["almostFull"]["value"], false);
    }));

    // 056_01_02: Merge simple properties — values should be updated
    trials.push(trial(&format!("{PREFIX}::056_01_02::simple_properties"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_01_02");
        let entity = fixtures::load_fixture("entities/merge/building-merge-data.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture("entities/building-simple-attributes-second.jsonld", &id);
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["name"]["value"], "Pisa Tower");
        pretty_assertions::assert_eq!(body["almostFull"]["value"], true);
        pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 6);
    }));

    // 056_01_03: Merge a new relationship into an existing entity
    trials.push(trial(&format!("{PREFIX}::056_01_03::simple_relationship"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_01_03");
        let entity = fixtures::load_fixture("entities/merge/building-merge-data.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture("entities/building-relationship.jsonld", &id);
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["locatedAt"]["type"], "Relationship");
        pretty_assertions::assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
        // Original properties should still be present
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 056_01_04: Merge partial property data — unmentioned sub-attributes preserved
    trials.push(trial(&format!("{PREFIX}::056_01_04::partial_properties"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_01_04");
        let entity = fixtures::load_fixture("entities/merge/building-merge-data.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture("entities/merge/building-only-airQualityLevel-value.jsonld", &id);
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 1);
        let observed = body["airQualityLevel"]["observedAt"].as_str().unwrap();
        assert!(observed.starts_with("2024-01-01T00:00:00"), "observedAt should match: {observed}");
        // unitCode should be preserved from original
        pretty_assertions::assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
        // Other properties unchanged
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // ── 056_02: Merge entity error cases ────────────────────────────────

    // 056_02_01: Merge entity with invalid ID
    trials.push(trial(&format!("{PREFIX}::056_02_01::invalid_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.merge_entity("invalidUri", &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");
    }));

    // 056_02_02: Merge entity without providing an ID (PATCH on collection)
    trials.push(trial(&format!("{PREFIX}::056_02_02::missing_id"), |ctx| async move {
        let fragment = serde_json::json!({
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.request(Method::PATCH, "/entities", Some(&fragment), Default::default()).await;
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "missing ID should return 400 or 405, got {}",
            resp.status
        );
    }));

    // 056_02_03: Merge entity with non-existent ID
    trials.push(trial(&format!("{PREFIX}::056_02_03::not_found"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_02_03");
        let fragment = serde_json::json!({
            "id": &id,
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    }));

    // ── 056_03: Delete attributes via NGSI-LD null ──────────────────────

    // 056_03_01: Delete all instances of a Property via NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::056_03_01::delete_property_via_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_03_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("name").is_none(), "name should be deleted");
        // Other attributes should remain
        assert!(body.get("locatedAt").is_some(), "locatedAt should remain");
        assert!(body.get("location").is_some(), "location should remain");
        assert!(body.get("street").is_some(), "street should remain");
    }));

    // 056_03_02: Delete all instances of a Relationship via NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::056_03_02::delete_relationship_via_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_03_02");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("locatedAt").is_none(), "locatedAt should be deleted");
        assert!(body.get("name").is_some(), "name should remain");
    }));

    // 056_03_03: Delete all instances of a GeoProperty via NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::056_03_03::delete_geo_property_via_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_03_03");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("location").is_none(), "location should be deleted");
        assert!(body.get("name").is_some(), "name should remain");
    }));

    // 056_03_04: Delete all instances of a LanguageProperty via NGSI-LD null
    trials.push(trial(&format!("{PREFIX}::056_03_04::delete_language_property_via_null"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_03_04");
        let entity = fixtures::load_fixture("entities/building-different-attributes-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("street").is_none(), "street should be deleted");
        assert!(body.get("name").is_some(), "name should remain");
    }));

    // ── 056_04: Delete specific attribute instance via NGSI-LD null ─────

    // 056_04_01: Delete a specific Property instance by datasetId
    trials.push(trial(&format!("{PREFIX}::056_04_01::delete_property_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_04_01");
        let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property-instance.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        // Dataset:01 instance should be gone, Dataset:02 should remain
        let name = &body["name"];
        if name.is_array() {
            let names = name.as_array().unwrap();
            pretty_assertions::assert_eq!(names.len(), 1);
            pretty_assertions::assert_eq!(names[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
        } else {
            pretty_assertions::assert_eq!(name["datasetId"], "urn:ngsi-ld:Dataset:02");
        }
    }));

    // 056_04_02: Delete a specific Relationship instance by datasetId
    trials.push(trial(&format!("{PREFIX}::056_04_02::delete_relationship_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_04_02");
        let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship-instance.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        let located = &body["locatedAt"];
        if located.is_array() {
            let items = located.as_array().unwrap();
            pretty_assertions::assert_eq!(items.len(), 1);
            pretty_assertions::assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
        } else {
            pretty_assertions::assert_eq!(located["datasetId"], "urn:ngsi-ld:Dataset:02");
        }
    }));

    // 056_04_03: Delete a specific GeoProperty instance by datasetId
    trials.push(trial(&format!("{PREFIX}::056_04_03::delete_geo_property_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_04_03");
        let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty-instance.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        let loc = &body["location"];
        if loc.is_array() {
            let items = loc.as_array().unwrap();
            pretty_assertions::assert_eq!(items.len(), 1);
            pretty_assertions::assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
        } else {
            pretty_assertions::assert_eq!(loc["datasetId"], "urn:ngsi-ld:Dataset:02");
        }
    }));

    // 056_04_04: Delete a specific LanguageProperty instance by datasetId
    trials.push(trial(&format!("{PREFIX}::056_04_04::delete_language_property_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_04_04");
        let entity = fixtures::load_fixture("entities/building-different-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty-instance.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        let street = &body["street"];
        if street.is_array() {
            let items = street.as_array().unwrap();
            pretty_assertions::assert_eq!(items.len(), 1);
            pretty_assertions::assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
        } else {
            pretty_assertions::assert_eq!(street["datasetId"], "urn:ngsi-ld:Dataset:02");
        }
    }));

    // ── 056_05: Delete all attribute instances via NGSI-LD null (entity with defaults) ──────

    // 056_05_01: Null sentinel without datasetId removes ALL instances (§5.6.1)
    trials.push(trial(&format!("{PREFIX}::056_05_01::delete_default_property_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_05_01");
        let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-property.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        // Per §5.6.1: null without datasetId removes ALL instances
        assert!(body.get("name").is_none(), "name should be fully deleted");
    }));

    // 056_05_02: Null sentinel without datasetId removes ALL Relationship instances
    trials.push(trial(&format!("{PREFIX}::056_05_02::delete_default_relationship_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_05_02");
        let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-relationship.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("locatedAt").is_none(), "locatedAt should be fully deleted");
    }));

    // 056_05_03: Null sentinel without datasetId removes ALL GeoProperty instances
    trials.push(trial(&format!("{PREFIX}::056_05_03::delete_default_geo_property_instance"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:056_05_03");
        let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-geoproperty.jsonld");
        let resp = ctx.client.merge_entity(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("location").is_none(), "location should be fully deleted");
    }));

    // 056_05_04: Null sentinel without datasetId removes ALL LanguageProperty instances
    trials.push(trial(
        &format!("{PREFIX}::056_05_04::delete_default_language_property_instance"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:056_05_04");
            let entity = fixtures::load_fixture("entities/building-different-default-attributes-instances-and-types.jsonld", &id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let fragment = fixtures::load_fragment("entities/fragment_entities/ngsild_null/null-languageproperty.jsonld");
            let resp = ctx.client.merge_entity(&id, &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
            let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
            let body: serde_json::Value = resp.json();
            assert!(body.get("street").is_none(), "street should be fully deleted");
        },
    ));
}
