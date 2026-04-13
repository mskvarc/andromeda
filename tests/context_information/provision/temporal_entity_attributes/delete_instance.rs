#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    TestCtx,
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::temporal_attributes::delete_instance";

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

fn extract_instance_id(entity: &serde_json::Value, attr: &str) -> String {
    entity[attr][0]["instanceId"].as_str().expect("instanceId not found").to_string()
}

/// Create a temporal entity and return its ID and the instanceId of the first `speed` attribute.
async fn setup_entity_with_instance_id(ctx: &TestCtx, test_id: &str) -> (String, String) {
    let id = format!("urn:ngsi-ld:Vehicle:{test_id}:{}", uuid::Uuid::new_v4());
    ctx.track_temporal_entity(&id);

    let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);
    let resp = ctx.client.create_temporal_entity(&entity).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

    let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
    let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
    pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
    let body: serde_json::Value = resp.json();
    let instance_id = extract_instance_id(&body, "speed");

    (id, instance_id)
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 017_01: Delete attribute instance ────────────────────────────────

    // 017_01_01: Delete an attribute instance in temporal representation of an entity
    trials.push(trial(&format!("{PREFIX}::017_01_01::delete_temporal_attr_instance"), |ctx| async move {
        let (id, instance_id) = setup_entity_with_instance_id(&ctx, "017_01_01").await;

        let resp = ctx.client.delete_temporal_attr_instance(&id, "speed", &instance_id).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete instance should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        // Verify
        let headers = RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.get_temporal_entity(&id, &QueryParams::new(), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_fixture(
            "temporal_entities/expectations/vehicle-temporal-representation-delete-speed-instanceid.jsonld",
            &id,
        );
        normalize(&mut actual);
        normalize(&mut expected);
        pretty_assertions::assert_eq!(expected, actual);
    }));

    // ── 017_02: Invalid entity/attribute/instance IDs ────────────────────

    // 017_02_01: Delete attribute instance with an invalid entity ID
    trials.push(trial(
        &format!("{PREFIX}::017_02_01::delete_temporal_attr_instance_invalid_entity_id"),
        |ctx| async move {
            let (_id, instance_id) = setup_entity_with_instance_id(&ctx, "017_02_01").await;

            let resp = ctx.client.delete_temporal_attr_instance("invalidId", "speed", &instance_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("BadRequestData"),
                "error type should be BadRequestData"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 017_02_02: Delete attribute instance with an empty entity ID
    trials.push(trial(
        &format!("{PREFIX}::017_02_02::delete_temporal_attr_instance_empty_entity_id"),
        |ctx| async move {
            let (_id, instance_id) = setup_entity_with_instance_id(&ctx, "017_02_02").await;

            let resp = ctx
                .client
                .request(
                    Method::DELETE,
                    &format!("/temporal/entities//attrs/speed/{instance_id}"),
                    None,
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");
        },
    ));

    // 017_02_03: Delete attribute instance with an invalid instance ID
    trials.push(trial(
        &format!("{PREFIX}::017_02_03::delete_temporal_attr_instance_invalid_instance_id"),
        |ctx| async move {
            let (_id, _instance_id) = setup_entity_with_instance_id(&ctx, "017_02_03").await;

            let resp = ctx.client.delete_temporal_attr_instance(&_id, "speed", "invalidId").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid instance ID should return 400");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("BadRequestData"),
                "error type should be BadRequestData"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 017_02_04: Delete attribute instance with an invalid attribute name
    trials.push(trial(
        &format!("{PREFIX}::017_02_04::delete_temporal_attr_instance_invalid_attr_name"),
        |ctx| async move {
            let (id, instance_id) = setup_entity_with_instance_id(&ctx, "017_02_04").await;

            let resp = ctx.client.delete_temporal_attr_instance(&id, "invalid(Name", &instance_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid attr name should return 400");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("BadRequestData"),
                "error type should be BadRequestData"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 017_02_05: Delete attribute instance with an empty attribute name
    trials.push(trial(
        &format!("{PREFIX}::017_02_05::delete_temporal_attr_instance_empty_attr_name"),
        |ctx| async move {
            let (id, instance_id) = setup_entity_with_instance_id(&ctx, "017_02_05").await;

            let resp = ctx
                .client
                .request(
                    Method::DELETE,
                    &format!("/temporal/entities/{id}/attrs//{instance_id}"),
                    None,
                    Default::default(),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty attr name should return 400");
        },
    ));

    // ── 017_03: Entity/attribute/instance not found ──────────────────────

    // 017_03_01: Delete attribute instance if the entity with given ID is not found
    trials.push(trial(
        &format!("{PREFIX}::017_03_01::delete_temporal_attr_instance_entity_not_found"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Vehicle:017_03_01:{}", uuid::Uuid::new_v4());
            let resp = ctx.client.delete_temporal_attr_instance(&id, "speed", "urn:ngsi-ld:Instance:nonexistent").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
                "error type should be ResourceNotFound"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 017_03_02: Delete attribute instance if the target attribute is not found
    trials.push(trial(
        &format!("{PREFIX}::017_03_02::delete_temporal_attr_instance_attr_not_found"),
        |ctx| async move {
            let (id, instance_id) = setup_entity_with_instance_id(&ctx, "017_03_02").await;

            let resp = ctx.client.delete_temporal_attr_instance(&id, "speed2", &instance_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent attr should return 404");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
                "error type should be ResourceNotFound"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));

    // 017_03_03: Delete attribute instance if the target attribute instance is not found
    trials.push(trial(
        &format!("{PREFIX}::017_03_03::delete_temporal_attr_instance_instance_not_found"),
        |ctx| async move {
            let (id, _instance_id) = setup_entity_with_instance_id(&ctx, "017_03_03").await;

            let resp = ctx.client.delete_temporal_attr_instance(&id, "speed", "urn:ngsi-ld:01234567890123456789").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent instance should return 404");

            let body: serde_json::Value = resp.json();
            assert!(
                body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
                "error type should be ResourceNotFound"
            );
            assert!(body["title"].as_str().is_some(), "should contain error title");
        },
    ));
}
