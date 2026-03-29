use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::batch::update";

fn get_entity_ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 005_01: Basic batch update ──────────────────────────────────────

    // 005_01_01: Batch update entities with simple properties (append)
    trials.push(trial(&format!("{PREFIX}::005_01_01::with_properties"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:005_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:005_01_01b");

        // Setup: create entities with simple attributes
        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        // Update: relationship entities add locatedAt
        let u1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let u2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let updates = serde_json::json!([u1, u2]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "batch update should return 204");

        // Verify: new attribute added, old ones preserved
        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 005_01_02: Batch update entities with relationships
    trials.push(trial(&format!("{PREFIX}::005_01_02::with_relationships"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:005_01_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:005_01_02b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        let u1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let u2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let updates = serde_json::json!([u1, u2]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    }));

    // 005_01_03: Batch update entities with relationship-of-property
    trials.push(trial(&format!("{PREFIX}::005_01_03::with_rel_of_property"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:005_01_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:005_01_03b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        let u1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id1);
        let u2 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id2);
        let updates = serde_json::json!([u1, u2]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // ── 005_02: Batch update with noOverwrite option ────────────────────

    // 005_02_01: Batch update with noOverwrite — existing attrs should not be overwritten → 207
    trials.push(trial(&format!("{PREFIX}::005_02_01::no_overwrite_existing"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:005_02_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Try to overwrite existing "name" with noOverwrite — use same fixture (same name value)
        let u1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let updates = serde_json::json!([u1]);

        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/update?options=noOverwrite", Some(&updates), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "existing attr with noOverwrite should return 207");

        // Verify original value preserved
        let resp = ctx.client.get_entity(&id, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 005_02_02: Batch update with noOverwrite — new attr only → 204
    trials.push(trial(&format!("{PREFIX}::005_02_02::no_overwrite_new"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:005_02_02");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Add new attribute with noOverwrite — should succeed (entity is minimal, relationship is new)
        let u1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id);
        let updates = serde_json::json!([u1]);

        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/update?options=noOverwrite", Some(&updates), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "new attr with noOverwrite should return 204");

        let resp = ctx.client.get_entity(&id, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    }));

    // 005_02_03: Batch update with noOverwrite — mix of new and existing attrs → 207
    trials.push(trial(&format!("{PREFIX}::005_02_03::no_overwrite_mixed"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:005_02_03");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        // Mix: overwrite existing name (should fail) + add new locatedAt (should succeed)
        let u1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id);
        let updates = serde_json::json!([u1]);

        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/update?options=noOverwrite", Some(&updates), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS);
    }));

    // ── 005_03: Mixed existing and non-existing ─────────────────────────

    // 005_03_01: Batch update with non-existing and existing entities → 207
    trials.push(trial(&format!("{PREFIX}::005_03_01::mixed_existing_nonexisting"), |ctx| async move {
        let existing_id1 = ctx.entity_id("urn:ngsi-ld:Building:005_03_01a");
        let existing_id2 = ctx.entity_id("urn:ngsi-ld:Building:005_03_01b");
        let nonexistent_id = ctx.entity_id("urn:ngsi-ld:Building:005_03_01c");

        for id in [&existing_id1, &existing_id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        let u1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id1);
        let u2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id2);
        let u3 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &nonexistent_id);
        let updates = serde_json::json!([u1, u2, u3]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success");
        let errors = body["errors"].as_array().expect("should have errors");
        assert_eq!(success.len(), 2);
        assert_eq!(errors.len(), 1);
    }));

    // ── 005_04: Invalid update requests ─────────────────────────────────

    // 005_04_01: Batch update with invalid (malformed) JSON → 400
    trials.push(trial(&format!("{PREFIX}::005_04_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(
                Method::POST,
                "/entityOperations/update",
                b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
                "application/ld+json",
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 005_04_02: Batch update with invalid JSON-LD → 207
    trials.push(trial(&format!("{PREFIX}::005_04_02::invalid_json_ld"), |ctx| async move {
        // Valid JSON but invalid JSON-LD (missing type, invalid structure)
        let updates = serde_json::json!([{
            "id": "urn:ngsi-ld:Building:invalid_ld",
            "invalidField": "not a valid NGSI-LD structure",
            "@context": fixtures::test_suite_context_url()
        }]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "invalid JSON-LD should return 207");
    }));

    // ── 005_05: Update entity types ─────────────────────────────────────

    // 005_05_01: Batch update to add new type to entity
    trials.push(trial(&format!("{PREFIX}::005_05_01::entity_type"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:005_05_01");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);
        ctx.client.create_entity(&entity).await;

        let u1 = fixtures::load_fixture("entities/building-minimal-with-new-types.jsonld", &id);
        let updates = serde_json::json!([u1]);

        let resp = ctx.client.batch_update(&updates).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));
}
