use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::batch::create";

fn get_entity_ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 003_01: Basic batch create ──────────────────────────────────────

    // 003_01_01: Batch create minimal entities
    trials.push(trial(&format!("{PREFIX}::003_01_01::minimal_entities"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_01_01b");

        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED, "batch create should return 201");

        let body: serde_json::Value = resp.json();
        let uris = body.as_array().expect("response should be array of URIs");
        assert_eq!(uris.len(), 2);

        // Verify entities exist
        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx.client.get_entity(&id2, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    // 003_01_02: Batch create entities with simple properties
    trials.push(trial(&format!("{PREFIX}::003_01_02::with_properties"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_01_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_01_02b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 003_01_03: Batch create entities with relationships
    trials.push(trial(&format!("{PREFIX}::003_01_03::with_relationships"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_01_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_01_03b");

        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["type"], "Relationship");
    }));

    // 003_01_04: Batch create entities with relationship-of-property
    trials.push(trial(&format!("{PREFIX}::003_01_04::with_relationship_of_property"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_01_04a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_01_04b");

        let e1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);
    }));

    // ── 003_02: Mixed success/failure ───────────────────────────────────

    // 003_02_01: Batch create with one duplicate entity returns 207
    trials.push(trial(&format!("{PREFIX}::003_02_01::with_existing_entity"), |ctx| async move {
        // Pre-create one entity
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:003_02_01_exist");
        let pre = fixtures::load_fixture("entities/building-minimal.jsonld", &existing_id);
        let resp = ctx.client.create_entity(&pre).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Batch create: 2 new + 1 duplicate
        let new_id1 = ctx.entity_id("urn:ngsi-ld:Building:003_02_01a");
        let new_id2 = ctx.entity_id("urn:ngsi-ld:Building:003_02_01b");
        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &new_id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &new_id2);
        let e3 = fixtures::load_fixture("entities/building-minimal.jsonld", &existing_id);
        let entities = serde_json::json!([e1, e2, e3]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "mixed result should return 207");

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success array");
        let errors = body["errors"].as_array().expect("should have errors array");
        assert_eq!(success.len(), 2, "2 entities should succeed");
        assert_eq!(errors.len(), 1, "1 entity should fail");
    }));

    // ── 003_03: Invalid requests ────────────────────────────────────────

    // 003_03_01: Batch create with invalid (malformed) JSON
    trials.push(trial(&format!("{PREFIX}::003_03_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(
                Method::POST,
                "/entityOperations/create",
                b"[{\"id\":\"urn:ngsi-ld:Building:invalid\",,\"type\":\"Building\"}]",
                "application/ld+json",
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some(), "should contain error type");
    }));

    // 003_03_02: Batch create with empty JSON
    trials.push(trial(&format!("{PREFIX}::003_03_02::empty_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::POST, "/entityOperations/create", b"[]", "application/ld+json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── 003_04: Link header with JSON content type ──────────────────────

    // 003_04_01: Batch create with Link header and JSON content type
    trials.push(trial(&format!("{PREFIX}::003_04_01::json_with_link_header"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_04_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_04_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.json", &id2);
        let entities = serde_json::json!([e1, e2]);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);
    }));

    // 003_04_02: Batch create with Link header, verify context in retrieval
    trials.push(trial(&format!("{PREFIX}::003_04_02::json_with_link_header_retrieval"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_04_02");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // ── 003_05: Default context with JSON content type ──────────────────

    // 003_05_01: Batch create with JSON content type, no Link header — default context
    trials.push(trial(&format!("{PREFIX}::003_05_01::default_context"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_05_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_05_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.json", &id2);
        let entities = serde_json::json!([e1, e2]);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    // 003_05_02: Batch create with default context, verify compaction on retrieval
    trials.push(trial(&format!("{PREFIX}::003_05_02::default_context_retrieval"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_05_02");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // ── 003_06: @context in body with application/json ──────────────────

    // 003_06_01: Batch create with @context in body + application/json → per-entity error
    trials.push(trial(&format!("{PREFIX}::003_06_01::json_with_context_in_body"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_06_01");
        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new().content_type("application/json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        // Batch operations return 207 for per-entity errors (not 400)
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "@context with JSON CT should cause per-entity error");
    }));

    // ── 003_07: application/ld+json with @context ───────────────────────

    // 003_07_01: Batch create with application/ld+json and @context in payload
    trials.push(trial(&format!("{PREFIX}::003_07_01::ld_json_with_context"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_07_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_07_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);
    }));

    // 003_07_02: Batch create with ld+json, verify context in retrieval
    trials.push(trial(&format!("{PREFIX}::003_07_02::ld_json_with_context_retrieval"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_07_02");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
        assert_eq!(body["subCategory"]["value"], "tourism");
    }));

    // ── 003_08: application/ld+json without @context ────────────────────

    // 003_08_01: Batch create with ld+json but missing @context → per-entity error
    trials.push(trial(&format!("{PREFIX}::003_08_01::ld_json_without_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_08_01");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new().content_type("application/ld+json");

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "missing @context with ld+json should fail per-entity");
    }));

    // ── 003_09: Link header with application/ld+json ────────────────────

    // 003_09_01: Batch create with Link header + ld+json → 400
    trials.push(trial(&format!("{PREFIX}::003_09_01::ld_json_with_link_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:003_09_01");
        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let headers = RequestHeaders::new()
            .content_type("application/ld+json")
            .link_context(fixtures::test_suite_context_url());

        let resp = ctx.client.request(Method::POST, "/entityOperations/create", Some(&entities), headers).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "Link + ld+json should return 400");
    }));

    // ── 003_10: Duplicate IDs in batch ──────────────────────────────────

    // 003_10_01: Batch create with duplicate IDs in same batch → 207
    trials.push(trial(&format!("{PREFIX}::003_10_01::duplicate_ids_in_batch"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:003_10_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:003_10_01b");

        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        let e3 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        // id1 appears twice
        let entities = serde_json::json!([e1, e2, e3]);

        let resp = ctx.client.batch_create(&entities).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "duplicate IDs should return 207");

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success array");
        assert_eq!(success.len(), 2, "2 unique entities should succeed");
    }));
}
