use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "provision::temporal::update";

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
            arr.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        }
        _ => {}
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 008_01: Update temporal entity ───────────────────────────────────

    // 008_01_01: Update a temporal representation of an entity with simple temporal properties
    trials.push(trial(
        &format!("{PREFIX}::008_01_01::update_temporal_entity_simple_properties"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:008_01_01:{}", uuid::Uuid::new_v4());
            ctx.track_temporal_entity(&id);

            // Setup: create the initial temporal entity
            let entity = fixtures::load_fixture("temporal_entities/vehicle-create-temporal-representation.jsonld", &id);
            let resp = ctx.client.create_temporal_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Update: POST the update payload to the same endpoint
            let update_body = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation-update.jsonld", &id);
            let resp = ctx.client.create_temporal_entity(&update_body).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");
            assert!(resp.body_bytes().is_empty(), "response body should be empty");

            // Verify: retrieve and compare against expectation
            let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
            let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
            assert_eq!(resp.status, StatusCode::OK);

            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_fixture("temporal_entities/expectations/vehicle-temporal-representation-update.jsonld", &id);
            normalize(&mut actual);
            normalize(&mut expected);
            assert_eq!(expected, actual);
        },
    ));
}
