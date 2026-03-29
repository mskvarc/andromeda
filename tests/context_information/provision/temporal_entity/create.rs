use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::temporal::create";

/// Recursively strip `instanceId` and `@context` from a JSON value for comparison.
fn strip_ignored_keys(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            map.remove("instanceId");
            map.remove("@context");
            for v in map.values_mut() {
                strip_ignored_keys(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_ignored_keys(v);
            }
        }
        _ => {}
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 007_01: Create temporal entity with valid payload ────────────────

    // 007_01_01: Create a temporal representation of an entity
    trials.push(trial(&format!("{PREFIX}::007_01_01::create_temporal_entity"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:007_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-create-temporal-representation.jsonld", &id);

        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED, "create should return 201");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-create.jsonld", &id);
        strip_ignored_keys(&mut actual);
        strip_ignored_keys(&mut expected);
        assert_eq!(expected, actual);
    }));

    // 007_01_02: Create a temporal entity with no context
    trials.push(trial(&format!("{PREFIX}::007_01_02::create_temporal_entity_no_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:007_01_02:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        let entity = fixtures::load_fixture("temporal_entities/vehicle-create-temporal-representation-without-context.jsonld", &id);

        let headers = RequestHeaders::new().content_type("application/json");
        let resp = ctx.client.request(Method::POST, "/temporal/entities", Some(&entity), headers).await;
        assert_eq!(resp.status, StatusCode::CREATED, "create should return 201");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        // Retrieve without context — the expectation file has no @context
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-temporal-representation-create-with-no-context.jsonld",
            &id,
        );
        strip_ignored_keys(&mut actual);
        strip_ignored_keys(&mut expected);
        assert_eq!(expected, actual);
    }));

    // ── 007_02: Invalid/empty JSON ───────────────────────────────────────

    // 007_02_01: Create a temporal entity with invalid JSON
    trials.push(trial(&format!("{PREFIX}::007_02_01::create_temporal_entity_invalid_json"), |ctx| async move {
        let raw = fixtures::load_fixture_raw("temporal_entities/vehicle-temporal-representation-invalid-json.jsonld");

        let resp = ctx
            .client
            .request_raw(Method::POST, "/temporal/entities", raw.as_bytes(), "application/ld+json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("InvalidRequest"),
            "error type should be InvalidRequest"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // 007_02_02: Create a temporal entity with empty JSON
    trials.push(trial(&format!("{PREFIX}::007_02_02::create_temporal_entity_empty_json"), |ctx| async move {
        let resp = ctx.client.request_raw(Method::POST, "/temporal/entities", b"", "application/ld+json").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("InvalidRequest"),
            "error type should be InvalidRequest"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // ── 007_03: Missing/invalid entity ID ────────────────────────────────

    // 007_03_01: Create a temporal entity with missing ID
    trials.push(trial(&format!("{PREFIX}::007_03_01::create_temporal_entity_missing_id"), |ctx| async move {
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation-without-id.jsonld", "");

        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "missing ID should return 400");
    }));

    // 007_03_02: Create a temporal entity with invalid URI
    trials.push(trial(&format!("{PREFIX}::007_03_02::create_temporal_entity_invalid_uri"), |ctx| async move {
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", "invalidId");

        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
    }));
}
