#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::temporal_attributes::add";

/// Recursively strip `instanceId` and `@context`, then sort arrays for order-independent comparison.
fn normalize(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Object(map) => {
            map.remove("instanceId");
            map.remove("@context");
            for v in map.values_mut() {
                normalize(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize(v);
            }
            arr.sort_by_key(|a| a.to_string());
        }
        _ => {}
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 014_01: Add attribute to temporal entity ─────────────────────────

    // 014_01_01: Add an attribute to a temporal entity with simple temporal properties
    trials.push(trial(&format!("{PREFIX}::014_01_01::add_temporal_attrs"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:014_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        // Setup: create the temporal entity
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Add attribute
        let fragment = fixtures::load_fragment("temporal_entities/fragments/vehicle-temporal-representation-fragment.jsonld");
        let resp = ctx.client.add_temporal_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "add attrs should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        // Verify
        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-added-attribute.jsonld", &id);
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 014_02: Invalid entity ID ────────────────────────────────────────

    // 014_02_01: Add an attribute to a temporal representation of an entity with an empty entity ID
    trials.push(trial(&format!("{PREFIX}::014_02_01::add_temporal_attrs_empty_entity_id"), |ctx| async move {
        let fragment = fixtures::load_fragment("temporal_entities/fragments/vehicle-temporal-representation-fragment.jsonld");
        let resp = ctx
            .client
            .request(Method::POST, "/temporal/entities//attrs", Some(&fragment), Default::default())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("BadRequestData"),
            "error type should be BadRequestData"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // 014_02_02: Add an attribute to a temporal representation of an entity with an invalid entity ID
    trials.push(trial(&format!("{PREFIX}::014_02_02::add_temporal_attrs_invalid_entity_id"), |ctx| async move {
        let fragment = fixtures::load_fragment("temporal_entities/fragments/vehicle-temporal-representation-fragment.jsonld");
        let resp = ctx.client.add_temporal_attrs("invalidId", &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("BadRequestData"),
            "error type should be BadRequestData"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // ── 014_03: Entity not found ─────────────────────────────────────────

    // 014_03_01: Add attribute to a non-existent temporal entity
    trials.push(trial(&format!("{PREFIX}::014_03_01::add_temporal_attrs_not_found"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:014_03_01:{}", uuid::Uuid::new_v4());
        let fragment = fixtures::load_fragment("temporal_entities/fragments/vehicle-temporal-representation-fragment.jsonld");
        let resp = ctx.client.add_temporal_attrs(&id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // ── 014_04: Invalid/empty content ────────────────────────────────────

    // 014_04_01: Add an attribute to a temporal representation of an entity with invalid content
    trials.push(trial(&format!("{PREFIX}::014_04_01::add_temporal_attrs_invalid_json"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:014_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        // Setup: create entity
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let raw = fixtures::load_fixture_raw("temporal_entities/fragments/vehicle-temporal-representation-invalid-json-fragment.jsonld");
        let resp = ctx
            .client
            .request_raw(Method::POST, &format!("/temporal/entities/{id}/attrs"), raw.as_bytes(), "application/ld+json")
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("InvalidRequest"),
            "error type should be InvalidRequest"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));

    // 014_04_02: Add an attribute to a temporal representation of an entity with empty content
    trials.push(trial(&format!("{PREFIX}::014_04_02::add_temporal_attrs_empty_json"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:014_04_02:{}", uuid::Uuid::new_v4());
        ctx.track_temporal_entity(&id);

        // Setup: create entity
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let raw = fixtures::load_fixture_raw("temporal_entities/fragments/vehicle-temporal-representation-empty-json-fragment.jsonld");
        let resp = ctx
            .client
            .request_raw(Method::POST, &format!("/temporal/entities/{id}/attrs"), raw.as_bytes(), "application/ld+json")
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("InvalidRequest"),
            "error type should be InvalidRequest"
        );
        assert!(body["title"].as_str().is_some(), "should contain error title");
    }));
}
