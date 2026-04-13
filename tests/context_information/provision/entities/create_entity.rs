#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::{trial, trial_ignored},
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entities::create";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 001_01: Create entity with valid payload ────────────────────────

    // 001_01_01: Create a minimal entity (id + type only)
    trials.push(trial(&format!("{PREFIX}::001_01_01::minimal_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_01");
        let entity = fixtures::load_fixture("entities/building-minimal.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED, "create should return 201");
        assert!(resp.location().is_some(), "should return Location header");

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["type"], "Building");
    }));

    // 001_01_02: Create entity with simple properties
    trials.push(trial(&format!("{PREFIX}::001_01_02::with_properties"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["name"]["type"], "Property");
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["subCategory"]["type"], "Property");
        pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
        pretty_assertions::assert_eq!(body["airQualityLevel"]["type"], "Property");
        pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 4);
        pretty_assertions::assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
        pretty_assertions::assert_eq!(body["almostFull"]["type"], "Property");
        pretty_assertions::assert_eq!(body["almostFull"]["value"], false);
    }));

    // 001_01_03: Create entity with relationship properties
    trials.push(trial(&format!("{PREFIX}::001_01_03::with_relationship"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_03");
        let entity = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["airQualityLevel"]["type"], "Property");
        pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 4);
        pretty_assertions::assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
        pretty_assertions::assert_eq!(
            body["airQualityLevel"]["hasWeatherObserved"]["object"],
            "urn:ngsi-ld:Sensor:43a1af27-4aa7-44fe-bee9-fe76d56978a4"
        );
    }));

    // 001_01_04: Create entity with location GeoProperty
    trials.push(trial(&format!("{PREFIX}::001_01_04::with_location"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_04");
        let entity = fixtures::load_fixture("entities/building-location-attribute.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["location"]["type"], "GeoProperty");
        pretty_assertions::assert_eq!(body["location"]["value"]["type"], "Point");
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
    }));

    // 001_01_05: Create entity with non-core GeoProperty
    trials.push(trial(&format!("{PREFIX}::001_01_05::non_core_geo_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_05");
        let entity = fixtures::load_fixture("entities/building-non-core-geoproperty-attribute.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["customLocation"]["type"], "GeoProperty");
        pretty_assertions::assert_eq!(body["customLocation"]["value"]["type"], "Point");
    }));

    // ── 001_02: Invalid entity creation requests ────────────────────────

    // 001_02_01: Create entity with invalid (malformed) JSON
    trials.push(trial(&format!("{PREFIX}::001_02_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(
                Method::POST,
                "/entities",
                b"{\"id\":\"urn:ngsi-ld:Building:invalid\",,\"type\":\"Building\"}",
                "application/ld+json",
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "malformed JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some(), "should contain error type");
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // 001_02_02: Create entity with empty JSON array
    trials.push(trial(&format!("{PREFIX}::001_02_02::empty_json"), |ctx| async move {
        let resp = ctx.client.request_raw(Method::POST, "/entities", b"[]", "application/ld+json").await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty array should return 400");

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some(), "should contain error type");
    }));

    // 001_02_03: Create entity without @context (bare JSON, no Link header) -> 400
    // NOTE: andromeda uses a default context when none is provided, returning 201 instead of 400.
    trials.push(trial_ignored(
        &format!("{PREFIX}::001_02_03::no_context"),
        "andromeda accepts entities without explicit context (uses default)",
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:001_02_03");
            let entity = fixtures::load_fixture("entities/building-minimal.json", &id);

            // Send as application/json WITHOUT a Link header — no context provided at all
            let headers = RequestHeaders::new().content_type("application/json");

            let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "no context should return 400");

            let body: serde_json::Value = resp.json();
            assert!(body["type"].as_str().is_some(), "should contain error type");
        },
    ));

    // 001_02_04: Create entity with invalid type value
    trials.push(trial(&format!("{PREFIX}::001_02_04::invalid_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_02_04");
        let entity = serde_json::json!({
            "id": &id,
            "type": "type",
            "@context": fixtures::test_suite_context_url()
        });

        let headers = RequestHeaders::new().content_type("application/ld+json");
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid type should return 400");
    }));

    // ── 001_03: Duplicate entity ID ─────────────────────────────────────

    // 001_03_01: Creating entity with duplicate ID returns 409
    trials.push(trial(&format!("{PREFIX}::001_03_01::duplicate_id"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_03_01");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CONFLICT, "duplicate should return 409");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("AlreadyExists"),
            "error type should be AlreadyExists"
        );
    }));

    // ── 001_04: Link header with JSON content type ──────────────────────

    // 001_04_01: Create entity with JSON content type and Link header
    trials.push(trial(&format!("{PREFIX}::001_04_01::json_with_link_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_04_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        assert!(resp.location().is_some());

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
        pretty_assertions::assert_eq!(body["almostFull"]["value"], false);
    }));

    // 001_04_02: Create entity with JSON content type, Link header, no @context in retrieval
    trials.push(trial(&format!("{PREFIX}::001_04_02::json_with_link_header_no_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_04_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Retrieve without context — attributes should appear as expanded URIs
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
    }));

    // ── 001_05: Default context with JSON content type ──────────────────

    // 001_05_01: Create entity with JSON content type, no Link header, no @context
    trials.push(trial(&format!("{PREFIX}::001_05_01::default_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_05_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["id"], id);
        pretty_assertions::assert_eq!(body["type"], "Building");
    }));

    // 001_05_02: Create entity with JSON content type, verify default context compaction
    trials.push(trial(&format!("{PREFIX}::001_05_02::default_context_retrieval"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_05_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Retrieve with test-suite context — attributes should be compacted
        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // ── 001_06: @context in body with application/json ──────────────────

    // 001_06_01: @context in body with JSON content type should be rejected
    trials.push(trial(&format!("{PREFIX}::001_06_01::json_with_context_in_body"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_06_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "@context with application/json should return 400");
    }));

    // ── 001_07: @context from payload with application/ld+json ──────────

    // 001_07_01: Create entity with application/ld+json and @context in body
    trials.push(trial(&format!("{PREFIX}::001_07_01::ld_json_with_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_07_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        assert!(resp.location().is_some());

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["subCategory"]["value"], "tourism");
        pretty_assertions::assert_eq!(body["almostFull"]["value"], false);
    }));

    // 001_07_02: Create entity with ld+json, verify context from payload is used
    trials.push(trial(&format!("{PREFIX}::001_07_02::ld_json_with_context_retrieval"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_07_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
        pretty_assertions::assert_eq!(body["airQualityLevel"]["value"], 4);
        pretty_assertions::assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
    }));

    // ── 001_08: Missing @context with application/ld+json ───────────────

    // 001_08_01: application/ld+json without @context in body should be rejected
    trials.push(trial(&format!("{PREFIX}::001_08_01::ld_json_without_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_08_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "ld+json without @context should return 400");
    }));

    // ── 001_09: Link header with application/ld+json ────────────────────

    // 001_09_01: Link header with application/ld+json should be rejected
    trials.push(trial(&format!("{PREFIX}::001_09_01::ld_json_with_link_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_09_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let headers = RequestHeaders::new()
            .content_type("application/ld+json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "Link header with ld+json should return 400");
    }));

    // ── 001_10: LanguageProperty ────────────────────────────────────────

    // 001_10_01: Create entity with LanguageProperty
    trials.push(trial(&format!("{PREFIX}::001_10_01::language_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_10_01");
        let entity = fixtures::load_fixture("entities/building-language-property.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        assert!(resp.location().is_some());

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["street"]["type"], "LanguageProperty");
        pretty_assertions::assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
        pretty_assertions::assert_eq!(body["street"]["languageMap"]["nl"], "Grote Markt");
    }));

    // ── 001_11: Scope property ──────────────────────────────────────────

    // 001_11_01: Create entity with single scope
    trials.push(trial(&format!("{PREFIX}::001_11_01::one_scope"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_11_01");
        let entity = fixtures::load_fixture("entities/building-minimal-with-one-scope.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["scope"], "/Madrid/Gardens/ParqueNorte");
    }));

    // 001_11_02: Create entity with multiple scopes
    trials.push(trial(&format!("{PREFIX}::001_11_02::many_scopes"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_11_02");
        let entity = fixtures::load_fixture("entities/building-minimal-with-many-scopes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        let scopes = body["scope"].as_array().expect("scope should be an array");
        pretty_assertions::assert_eq!(scopes.len(), 2);
    }));

    // ── 001_12: observationSpace GeoProperty ────────────────────────────

    // 001_12: Create entity with observationSpace GeoProperty
    trials.push(trial(&format!("{PREFIX}::001_12::observation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_12");
        let entity = fixtures::load_fixture("entities/building-observation-space-geoproperty.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["observationSpace"]["type"], "GeoProperty");
    }));

    // ── 001_13: operationSpace GeoProperty ──────────────────────────────

    // 001_13: Create entity with operationSpace GeoProperty
    trials.push(trial(&format!("{PREFIX}::001_13::operation_space"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_13");
        let entity = fixtures::load_fixture("entities/building-operation-space-geoproperty.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["operationSpace"]["type"], "GeoProperty");
        pretty_assertions::assert_eq!(body["operationSpace"]["value"]["type"], "Polygon");
    }));

    // ── 001_14: JsonProperty ────────────────────────────────────────────

    // 001_14_01: Create entity with JsonProperty (object value)
    trials.push(trial(&format!("{PREFIX}::001_14_01::json_property_object"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_14_01");
        let entity = fixtures::load_fixture("entities/building-json-property-object.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
        assert!(body["jsonProperty"]["json"].is_object());
    }));

    // 001_14_02: Create entity with JsonProperty (array value)
    trials.push(trial(&format!("{PREFIX}::001_14_02::json_property_array"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_14_02");
        let entity = fixtures::load_fixture("entities/building-json-property-array.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
        assert!(body["jsonProperty"]["json"].is_array());
        pretty_assertions::assert_eq!(body["jsonProperty"]["json"].as_array().unwrap().len(), 2);
    }));

    // ── 001_15: VocabProperty ───────────────────────────────────────────

    // 001_15_01: Create entity with VocabProperty (string value)
    trials.push(trial(&format!("{PREFIX}::001_15_01::vocab_property_string"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_15_01");
        let entity = fixtures::load_fixture("entities/building-vocab-property-string.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
    }));

    // 001_15_02: Create entity with VocabProperty (array value)
    trials.push(trial(&format!("{PREFIX}::001_15_02::vocab_property_array"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_15_02");
        let entity = fixtures::load_fixture("entities/building-vocab-property-array.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
    }));
}
