use crate::harness::{client::QueryParams, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::temporal::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 009_01: Delete temporal entity ───────────────────────────────────

    // 009_01_01: Delete a temporal representation of an entity with simple temporal properties
    trials.push(trial(&format!("{PREFIX}::009_01_01::delete_temporal_entity"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:009_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        // Setup: create the temporal entity
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Delete
        let resp = ctx.client.delete_temporal_entity(&id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        // Verify: entity no longer exists
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "deleted entity should return 404");
    }));

    // ── 009_02: Delete with invalid entity ID ────────────────────────────

    // 009_02_01: Delete a temporal entity with an empty entity ID
    trials.push(trial(&format!("{PREFIX}::009_02_01::delete_temporal_entity_empty_id"), |ctx| async move {
        let resp = ctx.client.request(Method::DELETE, "/temporal/entities/", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED, "empty ID should return 405");
    }));

    // 009_02_02: Delete a temporal entity with an invalid entity ID
    trials.push(trial(&format!("{PREFIX}::009_02_02::delete_temporal_entity_invalid_id"), |ctx| async move {
        let resp = ctx.client.delete_temporal_entity("invalidId").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("BadRequestData"),
            "error type should be BadRequestData"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // ── 009_03: Delete non-existent entity ───────────────────────────────

    // 009_03_01: Delete a temporal entity with an unknown entity ID
    trials.push(trial(&format!("{PREFIX}::009_03_01::delete_temporal_entity_unknown_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:009_03_01:{}", uuid::Uuid::new_v4());

        let resp = ctx.client.delete_temporal_entity(&id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "unknown ID should return 404");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));
}
