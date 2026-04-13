#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::entity::retrieve";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

fn accept_geojson_headers() -> RequestHeaders {
    RequestHeaders::new().accept_geo_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 018_01: Retrieve entity by id ────────────────────────────────────

    // 018_01_01: Get an entity by id
    trials.push(trial(&format!("{PREFIX}::018_01_01::by_id"), |ctx| async move {
        let compound = fixtures::compound_context_url();

        let id = ctx.entity_id("urn:ngsi-ld:Building:018_01_01");
        let mut entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        entity["@context"] = serde_json::json!([compound]);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(compound);
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("entities/expectations/building-simple-attributes.jsonld");
        expected["id"] = serde_json::Value::String(id.clone());
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["type"], expected["type"]);
        pretty_assertions::assert_eq!(body["name"], expected["name"]);
        pretty_assertions::assert_eq!(body["airQualityLevel"], expected["airQualityLevel"]);
    }));

    // 018_01_02: Query some attributes from an entity
    trials.push(trial(&format!("{PREFIX}::018_01_02::with_attrs_filter"), |ctx| async move {
        let compound = fixtures::compound_context_url();

        let id = ctx.entity_id("urn:ngsi-ld:Building:018_01_02");
        let mut entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        entity["@context"] = serde_json::json!([compound]);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().attrs([
            "https://ngsi-ld-test-suite/context#airQualityLevel",
            "https://ngsi-ld-test-suite/context#subCategory",
        ]);
        let headers = RequestHeaders::new().accept_ld_json().link_context(compound);
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("airQualityLevel").is_some(), "airQualityLevel should be present");
        assert!(body.get("subCategory").is_some(), "subCategory should be present");
        assert!(body.get("name").is_none(), "name should not be returned");
    }));

    // ── 018_02: Invalid entity id ────────────────────────────────────────

    // 018_02_01: Get an entity if the entity id is not a valid URI
    trials.push(trial(&format!("{PREFIX}::018_02_01::invalid_id"), |ctx| async move {
        let resp = ctx.client.get_entity("thisisaninvaliduri", &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("BadRequestData"), "error type should be BadRequestData");
    }));

    // ── 018_03: Entity not found ─────────────────────────────────────────

    // 018_03_01: Get an entity if the entity id is not known to the system
    trials.push(trial(&format!("{PREFIX}::018_03_01::not_found"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_03_01");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap().contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
    }));

    // 018_03_02: Get an entity if an attribute is not known to the system
    trials.push(trial(&format!("{PREFIX}::018_03_02::unknown_attribute"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_03_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().attrs(["property_not_found"]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap().contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
    }));

    // ── 018_04: Simplified representation ────────────────────────────────

    // 018_04_01: Get an entity in a simplified representation
    trials.push(trial(&format!("{PREFIX}::018_04_01::key_values"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_04_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "name should be a simple string in keyValues representation");
    }));

    // ── 018_05: GeoJSON representation ───────────────────────────────────

    // 018_05_01: Retrieve entity in simplified GeoJSON
    trials.push(trial(&format!("{PREFIX}::018_05_01::geojson_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_05_01");
        let entity = fixtures::load_fixture("entities/building-two-geometry-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, accept_geojson_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["type"], "Feature");
        assert!(body.get("geometry").is_some(), "should have geometry field");
    }));

    // 018_05_02: Retrieve entity in normalized GeoJSON
    trials.push(trial(&format!("{PREFIX}::018_05_02::geojson_normalized"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_05_02");
        let entity = fixtures::load_fixture("entities/building-two-geometry-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), accept_geojson_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["type"], "Feature");
        assert!(body.get("geometry").is_some(), "should have geometry field");
        assert!(body.get("properties").is_some(), "should have properties field");
    }));

    // 018_05_03: Retrieve entity in GeoJSON with geometryProperty
    trials.push(trial(&format!("{PREFIX}::018_05_03::geojson_geometry_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_05_03");
        let entity = fixtures::load_fixture("entities/building-two-geometry-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().geometry_property("observationSpace");
        let resp = ctx.client.get_entity(&id, &params, accept_geojson_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["type"], "Feature");
        assert!(body.get("geometry").is_some(), "should have geometry from observationSpace");
    }));

    // 018_05_04: Retrieve entity in GeoJSON with nonexistent geometryProperty
    trials.push(trial(
        &format!("{PREFIX}::018_05_04::geojson_nonexistent_geometry_property"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:018_05_04");
            let entity = fixtures::load_fixture("entities/building-two-geometry-attributes.jsonld", &id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().geometry_property("operationSpace");
            let resp = ctx.client.get_entity(&id, &params, accept_geojson_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            pretty_assertions::assert_eq!(body["type"], "Feature");
            assert!(body["geometry"].is_null(), "geometry should be null for nonexistent geometryProperty");
        },
    ));

    // ── 018_06: JSON-LD context resolution ───────────────────────────────

    // 018_06_01: Retrieve entity with empty JSON-LD context (expanded terms)
    trials.push(trial(&format!("{PREFIX}::018_06_01::json_empty_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_06_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept("application/json");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(
            body.get("https://ngsi-ld-test-suite/context#name").is_some() || body.get("name").is_some(),
            "should contain name attribute (expanded or compacted)"
        );
    }));

    // 018_06_02: Retrieve entity with creation-time JSON-LD context (compacted terms)
    trials.push(trial(&format!("{PREFIX}::018_06_02::json_creation_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_06_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new()
            .accept("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("name").is_some(), "name should be compacted with context");
    }));

    // ── 018_07: Language filter ──────────────────────────────────────────

    // 018_07_01: Retrieve entity using a specific natural language (fr)
    trials.push(trial(&format!("{PREFIX}::018_07_01::lang_fr"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_01");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // 018_07_02: Retrieve entity using multiple natural languages with no ranked preference
    trials.push(trial(&format!("{PREFIX}::018_07_02::lang_multi_no_ranked"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_02");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr-CH,fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // 018_07_03: Retrieve entity with any supported language
    trials.push(trial(&format!("{PREFIX}::018_07_03::lang_any"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_03");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("*");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // 018_07_04: Retrieve entity using multiple natural languages with ranked preferences
    trials.push(trial(&format!("{PREFIX}::018_07_04::lang_ranked_preferences"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_04");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // 018_07_05: Retrieve entity using a specific natural language with simplified representation
    trials.push(trial(&format!("{PREFIX}::018_07_05::lang_fr_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_05");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr").options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // 018_07_06: Retrieve entity with any supported language with simplified representation
    trials.push(trial(&format!("{PREFIX}::018_07_06::lang_any_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_07_06");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("*").options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // ── 018_08: observationSpace geoProperty ─────────────────────────────

    // 018_08_01: Retrieve entity with observationSpace geospatial property (simplified)
    trials.push(trial(&format!("{PREFIX}::018_08_01::observation_space_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_08_01");
        let entity = fixtures::load_fixture("entities/building-observation-space-geoproperty.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("observationSpace").is_some(), "observationSpace should be present");
    }));

    // 018_08_02: Retrieve entity with observationSpace geospatial property (normalized)
    trials.push(trial(&format!("{PREFIX}::018_08_02::observation_space_normalized"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_08_02");
        let entity = fixtures::load_fixture("entities/building-observation-space-geoproperty.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("observationSpace").is_some(), "observationSpace should be present");
        pretty_assertions::assert_eq!(body["observationSpace"]["type"], "GeoProperty");
    }));

    // ── 018_09: operationSpace geoProperty ───────────────────────────────

    // 018_09_01: Retrieve entity with operationSpace geospatial property (simplified)
    trials.push(trial(&format!("{PREFIX}::018_09_01::operation_space_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_09_01");
        let entity = fixtures::load_fixture("entities/building-operation-space-geoproperty.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("operationSpace").is_some(), "operationSpace should be present");
    }));

    // 018_09_02: Retrieve entity with operationSpace geospatial property (normalized)
    trials.push(trial(&format!("{PREFIX}::018_09_02::operation_space_normalized"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_09_02");
        let entity = fixtures::load_fixture("entities/building-operation-space-geoproperty.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body.get("operationSpace").is_some(), "operationSpace should be present");
        pretty_assertions::assert_eq!(body["operationSpace"]["type"], "GeoProperty");
    }));

    // ── 018_10: One-level linked entities ────────────────────────────────

    // 018_10_01: Retrieve entity with inline linked entity (normalized)
    trials.push(trial(&format!("{PREFIX}::018_10_01::inline_linked"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_10_01");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_10_01");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(1);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
        assert!(body["locatedAt"].get("entity").is_some(), "linked entity should be inline");
    }));

    // 018_10_02: Retrieve entity with flat linked entity (normalized)
    trials.push(trial(&format!("{PREFIX}::018_10_02::flat_linked"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_10_02");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_10_02");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(1);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join should return an array");
        assert!(arr.len() >= 2, "should contain building and city");
    }));

    // 018_10_03: Retrieve entity with inline linked entity (simplified)
    trials.push(trial(&format!("{PREFIX}::018_10_03::inline_linked_simplified"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_10_03");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_10_03");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(1).options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
    }));

    // 018_10_04: Retrieve entity with flat linked entity (simplified)
    trials.push(trial(&format!("{PREFIX}::018_10_04::flat_linked_simplified"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_10_04");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_10_04");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(1).options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join should return an array");
        assert!(arr.len() >= 2, "should contain building and city");
    }));

    // ── 018_11: Inline linked entity with sysAttrs ──────────────────────

    // 018_11_01: Retrieve entity with inline linked entity and sysAttrs
    trials.push(trial(&format!("{PREFIX}::018_11_01::inline_linked_sys_attrs"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_11_01");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_11_01");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(1).options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
        assert!(
            body["locatedAt"]["entity"].get("createdAt").is_some(),
            "linked entity should have createdAt with sysAttrs"
        );
    }));

    // ── 018_12: Flat linked entity with sysAttrs ─────────────────────────

    // 018_12_01: Retrieve entity with flat linked entity and sysAttrs
    trials.push(trial(&format!("{PREFIX}::018_12_01::flat_linked_sys_attrs"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_12_01");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_12_01");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let city = fixtures::load_fixture("entities/city-minimal.jsonld", &city_id);
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(1).options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join should return array");
        for entity in arr {
            assert!(entity.get("createdAt").is_some(), "all entities should have createdAt with sysAttrs");
        }
    }));

    // ── 018_13: Two-level linked entities ────────────────────────────────

    // 018_13_01: Retrieve entity with inline two-level linked entities (normalized)
    trials.push(trial(&format!("{PREFIX}::018_13_01::inline_two_level"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_13_01");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_13_01");
        let country_id = ctx.entity_id("urn:ngsi-ld:Country:018_13_01");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut city = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id);
        city["locatedAt"]["object"] = serde_json::Value::String(country_id.clone());
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let country = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id);
        let resp = ctx.client.create_entity(&country).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(2);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
    }));

    // 018_13_02: Retrieve entity with flat two-level linked entities (normalized)
    trials.push(trial(&format!("{PREFIX}::018_13_02::flat_two_level"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_13_02");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_13_02");
        let country_id = ctx.entity_id("urn:ngsi-ld:Country:018_13_02");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut city = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id);
        city["locatedAt"]["object"] = serde_json::Value::String(country_id.clone());
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let country = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id);
        let resp = ctx.client.create_entity(&country).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(2);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join should return array");
        assert!(arr.len() >= 3, "should contain building, city, and country");
    }));

    // 018_13_03: Retrieve entity with inline two-level linked entities (simplified)
    trials.push(trial(&format!("{PREFIX}::018_13_03::inline_two_level_simplified"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_13_03");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_13_03");
        let country_id = ctx.entity_id("urn:ngsi-ld:Country:018_13_03");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut city = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id);
        city["locatedAt"]["object"] = serde_json::Value::String(country_id.clone());
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let country = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id);
        let resp = ctx.client.create_entity(&country).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(2).options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
    }));

    // 018_13_04: Retrieve entity with flat two-level linked entities (simplified)
    trials.push(trial(&format!("{PREFIX}::018_13_04::flat_two_level_simplified"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_13_04");
        let city_id = ctx.entity_id("urn:ngsi-ld:City:018_13_04");
        let country_id = ctx.entity_id("urn:ngsi-ld:Country:018_13_04");

        let mut building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        building["locatedAt"]["object"] = serde_json::Value::String(city_id.clone());
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut city = fixtures::load_fixture("entities/city-relationship.jsonld", &city_id);
        city["locatedAt"]["object"] = serde_json::Value::String(country_id.clone());
        let resp = ctx.client.create_entity(&city).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let country = fixtures::load_fixture("entities/country-minimal.jsonld", &country_id);
        let resp = ctx.client.create_entity(&country).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(2).options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join should return array");
        assert!(arr.len() >= 3, "should contain building, city, and country");
    }));

    // ── 018_14: Non-existent linked entity ───────────────────────────────

    // 018_14_01: Retrieve entity with inline join when linked entity does not exist
    trials.push(trial(&format!("{PREFIX}::018_14_01::inline_nonexistent_linked"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_14_01");
        let building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("inline").join_level(1);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], building_id);
    }));

    // 018_14_02: Retrieve entity with flat join when linked entity does not exist
    trials.push(trial(&format!("{PREFIX}::018_14_02::flat_nonexistent_linked"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:018_14_02");
        let building = fixtures::load_fixture("entities/building-relationship.jsonld", &building_id);
        let resp = ctx.client.create_entity(&building).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().join("flat").join_level(1);
        let resp = ctx.client.get_entity(&building_id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
    }));

    // ── 018_15: Format and options parameters ────────────────────────────

    // 018_15_01: Retrieve entity with format=keyValues
    trials.push(trial(&format!("{PREFIX}::018_15_01::format_key_values"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().format("keyValues");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "name should be simplified");
    }));

    // 018_15_02: Retrieve entity with format=simplified
    trials.push(trial(&format!("{PREFIX}::018_15_02::format_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().format("simplified");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "name should be simplified");
    }));

    // 018_15_03: Retrieve entity with format=normalized
    trials.push(trial(&format!("{PREFIX}::018_15_03::format_normalized"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().format("normalized");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["name"]["type"], "Property", "name should be normalized");
    }));

    // 018_15_04: Retrieve entity with options=keyValues
    trials.push(trial(&format!("{PREFIX}::018_15_04::options_key_values"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_04");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "name should be simplified");
    }));

    // 018_15_05: Retrieve entity with options=simplified
    trials.push(trial(&format!("{PREFIX}::018_15_05::options_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_05");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?options=simplified"), None, ctx_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "name should be simplified");
    }));

    // 018_15_06: Retrieve entity with options=normalized
    trials.push(trial(&format!("{PREFIX}::018_15_06::options_normalized"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_06");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?options=normalized"), None, ctx_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["name"]["type"], "Property", "name should be normalized");
    }));

    // 018_15_07: Retrieve entity with format=keyValues overriding options=normalized
    trials.push(trial(&format!("{PREFIX}::018_15_07::format_overrides_options"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_15_07");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?format=keyValues&options=normalized"), None, ctx_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        assert!(body["name"].is_string(), "format=keyValues should override options=normalized");
    }));

    // ── 018_16: Invalid format/options ───────────────────────────────────

    // 018_16_01: Retrieve entity with invalid format value
    trials.push(trial(&format!("{PREFIX}::018_16_01::invalid_format"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_16_01");

        let headers = RequestHeaders::new().accept_ld_json();
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?format=invalidFormat"), None, headers)
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("InvalidRequest"), "error type should be InvalidRequest");
    }));

    // 018_16_02: Retrieve entity with invalid options value
    trials.push(trial(&format!("{PREFIX}::018_16_02::invalid_options"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_16_02");

        let headers = RequestHeaders::new().accept_ld_json();
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?options=invalidOptions"), None, headers)
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("InvalidRequest"), "error type should be InvalidRequest");
    }));

    // 018_16_03: Retrieve entity with at least one invalid option
    trials.push(trial(&format!("{PREFIX}::018_16_03::mixed_invalid_options"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_16_03");

        let headers = RequestHeaders::new().accept_ld_json();
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?options=keyValues,invalid"), None, headers)
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("InvalidRequest"), "error type should be InvalidRequest");
    }));

    // 018_16_04: Retrieve entity with valid format but invalid options
    trials.push(trial(&format!("{PREFIX}::018_16_04::invalid_format_with_options"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:018_16_04");

        let headers = RequestHeaders::new().accept_ld_json();
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}?format=keyValues&options=invalid"), None, headers)
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().unwrap().contains("InvalidRequest"), "error type should be InvalidRequest");
    }));
}
