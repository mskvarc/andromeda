use crate::harness::{client::QueryParams, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::batch::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // 006_01_01: Batch delete existing entities
    trials.push(trial(&format!("{PREFIX}::006_01_01::delete_entities"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:006_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:006_01_01b");

        // Create entities first
        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        // Batch delete
        let ids = serde_json::json!([&id1, &id2]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "batch delete should return 204");

        // Verify deleted
        let resp = ctx.client.get_entity(&id1, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let resp = ctx.client.get_entity(&id2, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 006_02_01: Batch delete with mix of existing and non-existing entities
    trials.push(trial(&format!("{PREFIX}::006_02_01::mixed_existing_nonexisting"), |ctx| async move {
        let existing_id = ctx.entity_id("urn:ngsi-ld:Building:006_02_01a");
        let nonexistent_id = ctx.entity_id("urn:ngsi-ld:Building:006_02_01b");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &existing_id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let ids = serde_json::json!([&existing_id, &nonexistent_id]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "mixed should return 207");

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success array");
        let errors = body["errors"].as_array().expect("should have errors array");
        assert_eq!(success.len(), 1);
        assert_eq!(errors.len(), 1);

        // Existing should be deleted
        let resp = ctx.client.get_entity(&existing_id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 006_03_01: Batch delete with invalid (malformed) JSON
    trials.push(trial(&format!("{PREFIX}::006_03_01::invalid_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::POST, "/entityOperations/delete", b"[\"urn:ngsi-ld:Building:x\",,]", "application/json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 006_03_02: Batch delete with empty JSON array
    trials.push(trial(&format!("{PREFIX}::006_03_02::empty_json"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::POST, "/entityOperations/delete", b"[]", "application/json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 006_04_01: Batch delete with same entity ID twice → 207
    trials.push(trial(&format!("{PREFIX}::006_04_01::duplicate_ids"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:006_04_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Delete same ID twice
        let ids = serde_json::json!([&id, &id]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::MULTI_STATUS, "duplicate IDs should return 207");

        let body: serde_json::Value = resp.json();
        let success = body["success"].as_array().expect("should have success array");
        let errors = body["errors"].as_array().expect("should have errors array");
        assert_eq!(success.len(), 1, "first delete should succeed");
        assert_eq!(errors.len(), 1, "second delete should fail");
    }));
}
