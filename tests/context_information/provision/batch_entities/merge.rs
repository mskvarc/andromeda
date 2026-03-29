use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::batch::merge";

fn get_entity_ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 057_01: Basic batch merge ───────────────────────────────────────

    // 057_01_01: Batch merge with empty fragment — entities unchanged
    trials.push(trial(&format!("{PREFIX}::057_01_01::empty_fragment"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:057_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:057_01_01b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        let e1 = fixtures::load_fixture("entities/building-minimal.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-minimal.jsonld", &id2);
        let fragments = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "batch merge should return 204");

        // Verify unchanged
        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
    }));

    // 057_01_02: Batch merge simple properties — values updated
    trials.push(trial(&format!("{PREFIX}::057_01_02::properties"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:057_01_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:057_01_02b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        // Merge with relationship (adds locatedAt)
        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let fragments = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
        // Unmodified attrs should remain
        assert_eq!(body["subCategory"]["value"], "tourism");
    }));

    // 057_01_03: Batch merge adds relationship to entities
    trials.push(trial(&format!("{PREFIX}::057_01_03::relationship"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:057_01_03a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:057_01_03b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-minimal.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        let e1 = fixtures::load_fixture("entities/building-relationship.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship.jsonld", &id2);
        let fragments = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    }));

    // 057_01_04: Batch merge partial property data — unmentioned sub-attributes preserved
    trials.push(trial(&format!("{PREFIX}::057_01_04::partial"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:057_01_04a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:057_01_04b");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        // Merge with relationship-of-property (updates airQualityLevel, adds hasWeatherObserved)
        let e1 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-relationship-of-property.jsonld", &id2);
        let fragments = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity(&id1, &Default::default(), get_entity_ctx_headers()).await;
        let body: serde_json::Value = resp.json();
        assert_eq!(body["airQualityLevel"]["value"], 4);
        assert_eq!(body["airQualityLevel"]["unitCode"], "C62", "unitCode should be preserved");
        assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
    }));

    // ── 057_02: Mixed existing and non-existing ─────────────────────────

    // 057_02_01: Batch merge with existing and non-existing entities → 207
    trials.push(trial(&format!("{PREFIX}::057_02_01::mixed_existing_nonexisting"), |ctx| async move {
        let existing_id1 = ctx.entity_id("urn:ngsi-ld:Building:057_02_01a");
        let existing_id2 = ctx.entity_id("urn:ngsi-ld:Building:057_02_01b");
        let nonexistent_id = ctx.entity_id("urn:ngsi-ld:Building:057_02_01c");

        for id in [&existing_id1, &existing_id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            ctx.client.create_entity(&entity).await;
        }

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id2);
        let e3 = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &nonexistent_id);
        let fragments = serde_json::json!([e1, e2, e3]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success");
        let errors = body["errors"].as_array().expect("should have errors");
        assert_eq!(success.len(), 2);
        assert_eq!(errors.len(), 1);
    }));

    // ── 057_03: Invalid merge requests ──────────────────────────────────

    // 057_03_01: Batch merge with invalid (malformed) JSON → 400
    trials.push(trial(&format!("{PREFIX}::057_03_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(
                Method::POST,
                "/entityOperations/merge",
                b"[{\"id\":\"urn:ngsi-ld:Building:x\",,}]",
                "application/ld+json",
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 057_03_02: Batch merge with invalid JSON-LD → 207
    trials.push(trial(&format!("{PREFIX}::057_03_02::invalid_json_ld"), |ctx| async move {
        let fragments = serde_json::json!([{
            "id": "urn:ngsi-ld:Building:invalid_ld",
            "invalidField": "not a valid NGSI-LD structure",
            "@context": fixtures::test_suite_context_url()
        }]);

        let resp = ctx.client.batch_merge(&fragments).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "invalid JSON-LD should return 207");
    }));
}
