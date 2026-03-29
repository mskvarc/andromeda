use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::batch::upsert";

fn get_entity_ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 004_01: Upsert non-existing entities (create) ───────────────────

    // 004_01_01: Batch upsert new entities with properties
    trials.push(trial(&format!("{PREFIX}::004_01_01::new_entities_with_properties"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_01_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED, "upsert of new entities should return 201");

        let body: serde_json::Value = resp.json();
        let uris = body.as_array().expect("response should be array of URIs");
        assert_eq!(uris.len(), 2);
    }));

    // 004_01_02: Batch upsert new entities with relationships
    trials.push(trial(&format!("{PREFIX}::004_01_02::new_entities_with_relationships"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_01_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_01_02b");

        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);
    }));

    // 004_01_03: Batch upsert new entities with relationship-of-property
    trials.push(trial(&format!("{PREFIX}::004_01_03::new_entities_with_rel_of_property"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_01_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_01_03b");

        let e1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);
    }));

    // ── 004_02: Upsert mixed new and existing ───────────────────────────

    // 004_02_01: Batch upsert with mix of new and existing entities
    trials.push(trial(&format!("{PREFIX}::004_02_01::mixed_new_and_existing"), |ctx| async move {
        // Pre-create one entity
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_01_exist");
        let pre = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        let resp = ctx.client.create_entity(&pre).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Upsert: existing (replace) + new (create)
        let new_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_01_new");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &new_id);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED, "should return 201 for new entities");

        // Verify existing was replaced
        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");

        // Verify new was created
        let resp = ctx.client.get_entity(&new_id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    // 004_02_02: Batch upsert mixed new and existing, verify retrieval with ld+json
    trials.push(trial(&format!("{PREFIX}::004_02_02::mixed_new_and_existing_retrieval"), |ctx| async move {
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_02_exist");
        let pre = fixtures::load_fixture("entities/building-minimal.jsonld", &existing_id);
        let resp = ctx.client.create_entity(&pre).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let new_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_02_new");
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &new_id);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
        assert_eq!(body["subCategory"]["value"], "tourism");

        let resp = ctx.client.get_entity(&new_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 004_02_03: Batch upsert mixed with relationship entities
    trials.push(trial(&format!("{PREFIX}::004_02_03::mixed_with_relationships"), |ctx| async move {
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_03_exist");
        let pre = fixtures::load_fixture("entities/building-minimal.jsonld", &existing_id);
        let resp = ctx.client.create_entity(&pre).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let new_id = ctx.entity_id("urn:ngsi-ld:Building:004_02_03_new");
        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &existing_id);
        let e2 = fixtures::load_fixture("entities/building-relationship.jsonld", &new_id);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["type"], "Relationship");
    }));

    // ── 004_03: Upsert existing entities (replace) ─────────────────────

    // 004_03_01: Batch upsert existing entities replaces properties
    trials.push(trial(&format!("{PREFIX}::004_03_01::existing_replace_properties"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_03_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_03_01b");

        // Create initial entities with simple attributes (has name, subCategory, etc.)
        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        // Upsert (replace) with minimal — old attrs should be gone
        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "upsert of all existing should return 204");

        // Verify replacement
        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("name").is_none(), "old attribute should be gone after replace");
        assert!(body.get("subCategory").is_none(), "old attribute should be gone after replace");
    }));

    // 004_03_02: Batch upsert existing entities replaces relationships
    trials.push(trial(&format!("{PREFIX}::004_03_02::existing_replace_relationships"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_03_02");
        let entity = fixtures::load_fixture("entities/building-relationship.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Upsert with relationship-of-property (replaces old relationship)
        let e1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
    }));

    // 004_03_03: Batch upsert existing replaces relationship-of-property
    trials.push(trial(&format!("{PREFIX}::004_03_03::existing_replace_rel_of_property"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_03_03");
        let entity = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Upsert with simple attributes (replaces old rel-of-prop)
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 004_03_04: Batch upsert existing entity replaces scope
    trials.push(trial(&format!("{PREFIX}::004_03_04::existing_replace_scope"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_03_04");
        let entity = fixtures::load_fixture("entities/building-with-one-scope.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Upsert with minimal (no scope) — scope should be gone
        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("scope").is_none(), "scope should be gone after replace");
    }));

    // 004_03_05: Batch upsert existing entity replaces types
    trials.push(trial(&format!("{PREFIX}::004_03_05::existing_replace_types"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_03_05");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let e1 = fixtures::load_fixture("entities/building-with-two-types.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // ── 004_04: Upsert with update option ───────────────────────────────

    // 004_04_01: Batch upsert with options=update merges instead of replaces
    trials.push(trial(&format!("{PREFIX}::004_04_01::with_update_option"), |ctx| async move {
        // Pre-create entity with simple attributes
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:004_04_01_exist");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        ctx.client.create_entity(&entity).await;

        let new_id = ctx.entity_id("urn:ngsi-ld:Building:004_04_01_new");
        // Upsert existing with relationship (merge), plus a new entity
        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &existing_id);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &new_id);
        let entities = serde_json::json!([e1, e2]);

        // Use options=update for merge behavior
        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/upsert?options=update", Some(&entities), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED, "should return 201 for new entity");

        // Verify existing was merged (not replaced): subCategory should still be there
        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["subCategory"]["value"], "tourism", "subCategory should be preserved (merge, not replace)");
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    }));

    // 004_04_02: Batch upsert with update option, verify retrieval
    trials.push(trial(&format!("{PREFIX}::004_04_02::with_update_option_retrieval"), |ctx| async move {
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:004_04_02_exist");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        ctx.client.create_entity(&entity).await;

        // Merge with relationship-of-property
        let e1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &existing_id);
        let entities = serde_json::json!([e1]);

        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/upsert?options=update", Some(&entities), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), get_entity_ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
    }));

    // 004_04_03: Batch upsert with update option preserves existing scope
    trials.push(trial(&format!("{PREFIX}::004_04_03::with_update_option_preserves_scope"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_04_03");
        let entity = fixtures::load_fixture("entities/building-with-one-scope.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Merge with simple attributes (scope should be preserved)
        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/upsert?options=update", Some(&entities), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert!(body.get("scope").is_some(), "scope should be preserved after merge");
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // ── 004_05: Upsert with invalid entity in batch ────────────────────

    // 004_05_01: Batch upsert with one invalid entity → 207
    trials.push(trial(&format!("{PREFIX}::004_05_01::with_invalid_entity"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_05_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_05_01b");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:004_05_01c");

        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        // Third entity has no type → invalid
        let entities = serde_json::json!([
            e1,
            e2,
            { "id": &id3, "@context": fixtures::test_suite_context_url() }
        ]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success");
        let errors = body["errors"].as_array().expect("should have errors");
        assert_eq!(success.len(), 2);
        assert_eq!(errors.len(), 1);
    }));

    // ── 004_06: Invalid upsert requests ─────────────────────────────────

    // 004_06_01: Batch upsert with invalid (malformed) JSON
    trials.push(trial(&format!("{PREFIX}::004_06_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(
                Method::POST,
                "/entityOperations/upsert",
                b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
                "application/ld+json",
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 004_06_02: Batch upsert with empty JSON array
    trials.push(trial(&format!("{PREFIX}::004_06_02::empty_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::POST, "/entityOperations/upsert", b"[]", "application/ld+json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── 004_07: Upsert with duplicate IDs ───────────────────────────────

    // 004_07_01: Batch upsert with duplicate IDs → deduplication
    trials.push(trial(&format!("{PREFIX}::004_07_01::duplicate_ids"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:004_07_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:004_07_01b");

        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        let e3 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id1);
        // id1 appears twice
        let entities = serde_json::json!([e1, e2, e3]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let uris = body.as_array().expect("should be array of URIs");
        // Should contain at most 2 unique entity IDs
        assert!(uris.len() <= 2, "should deduplicate IDs");
    }));

    // ── 004_08: Upsert with new types ───────────────────────────────────

    // 004_08_01: Batch upsert existing entities with new types
    trials.push(trial(&format!("{PREFIX}::004_08_01::with_new_types"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:004_08_01");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let e1 = fixtures::load_fixture("entities/building-minimal-with-new-types.jsonld", &id);
        let entities = serde_json::json!([e1]);

        let resp = ctx.client.batch_upsert(&entities).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));
}
