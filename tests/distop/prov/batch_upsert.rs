//! Batch Upsert forwarding — REQ-PROV-57..66 (clause 5.6.8.4).
//!
//! Each matching registration receives a filtered copy of the batch input; a
//! source that advertises the batch operation is handed the whole batch and its
//! `BatchOperationResult` is merged back keyed by `registrationId`, while a
//! source that does not falls back to per-entity Create/Replace/Update. When the
//! source already holds an entity, the upsert mode selects the follow-up path
//! (replace or update), and an exclusive registration advertising no viable
//! operation yields a per-entity `409 Conflict`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present, entity_with_id, forwarded_entities, request_path},
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying `brandName`, `speed` and `location`, used to prove
/// per-registration attribute filtering.
fn tri_attr_vehicle(id: &str) -> Value {
    entity(
        id,
        "Vehicle",
        &[
            ("brandName", property(json!("Mercedes"))),
            ("speed", property(json!(42))),
            ("location", property(json!("home"))),
        ],
    )
}

/// A `Vehicle` carrying only `speed`.
fn speed_vehicle(id: &str, speed: i64) -> Value {
    entity(id, "Vehicle", &[("speed", property(json!(speed)))])
}

/// Whether a `BatchOperationResult` reports `id` under `success`.
fn success_contains(body: &Value, id: &str) -> bool {
    body["success"]
        .as_array()
        .is_some_and(|entries| entries.iter().any(|entry| entry.as_str() == Some(id) || entry["entityId"].as_str() == Some(id)))
}

/// Whether a `BatchOperationResult` carries an error keyed by `registration` with the given status.
fn error_keyed(body: &Value, registration: &str, status: u64) -> bool {
    body["errors"].as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|entry| entry["registrationId"].as_str() == Some(registration) && entry["error"]["status"].as_u64() == Some(status))
    })
}

/// Each registration receives only the attributes it owns from the upsert input.
/// Confirms REQ-PROV-57.
#[tokio::test]
async fn batch_upsert_per_csr_filtering() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-filter-v1");
    let reg_speed = topology.id("registration:batch-upsert-filter-speed");
    let reg_location = topology.id("registration:batch-upsert-filter-location");

    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s1"))
        .mode("exclusive")
        .property_names(&["speed"])
        .operations(&["createEntity", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    let location_csr = CsrBuilder::new(&reg_location, &mock.url("/s2"))
        .mode("exclusive")
        .property_names(&["location"])
        .operations(&["createEntity", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&location_csr).await.expect("create location registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/upsert", 204).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/upsert", 204).await;

    let dispatched = client.batch_upsert(&json!([tri_attr_vehicle(&v1)])).await.expect("batch upsert");
    assert!(dispatched.status.as_u16() < 500, "the batch upsert is dispatched: {}", dispatched.status);

    let first = mock.next_request(FORWARD_WAIT).await;
    let second = mock.next_request(FORWARD_WAIT).await;
    let (speed_req, location_req) = if request_path(&first).contains("/s1") {
        (first, second)
    } else {
        (second, first)
    };

    let speed_body = speed_req.json::<Value>().expect("forwarded speed batch is JSON");
    let speed_entities = forwarded_entities(&speed_body);
    let speed_copy = entity_with_id(&speed_entities, &v1).expect("the speed source receives V1");
    assert!(attr_present(speed_copy, "speed"), "the speed source receives speed: {speed_copy}");
    assert!(
        !attr_present(speed_copy, "location"),
        "the speed source does not receive location: {speed_copy}"
    );
    assert!(
        !attr_present(speed_copy, "brandName"),
        "the speed source does not receive brandName: {speed_copy}"
    );

    let location_body = location_req.json::<Value>().expect("forwarded location batch is JSON");
    let location_entities = forwarded_entities(&location_body);
    let location_copy = entity_with_id(&location_entities, &v1).expect("the location source receives V1");
    assert!(
        attr_present(location_copy, "location"),
        "the location source receives location: {location_copy}"
    );
    assert!(
        !attr_present(location_copy, "speed"),
        "the location source does not receive speed: {location_copy}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source advertising the batch operation is handed the whole batch and its
/// per-entity result is merged back keyed by `registrationId`. Confirms REQ-PROV-58.
#[tokio::test]
async fn batch_upsert_supported_forwards_and_merges() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-merge-v1");
    let v2 = topology.id("Vehicle:batch-upsert-merge-v2");
    let reg_id = topology.id("registration:batch-upsert-merge");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["createEntity", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let source_result = json!({
        "success": [v1.as_str()],
        "errors": [ { "entityId": v2.as_str(), "error": { "status": 409, "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists" } } ]
    });
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/upsert", 207, &source_result)
        .await;

    let upserted = client
        .batch_upsert(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch upsert");
    assert_status(&upserted, 207);
    assert!(
        error_keyed(&json_body(&upserted), &reg_id, 409),
        "the source error is merged keyed by registrationId: {}",
        json_body(&upserted)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without the batch operation, a create-capable source is driven per entity via
/// Create Entity. Confirms REQ-PROV-59.
#[tokio::test]
async fn batch_upsert_fallback_per_entity_create() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-fbcreate-v1");
    let v2 = topology.id("Vehicle:batch-upsert-fbcreate-v2");
    let reg_id = topology.id("registration:batch-upsert-fbcreate");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

    assert_status(
        &client
            .batch_upsert(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
            .await
            .expect("batch upsert"),
        207,
    );
    assert_eq!(
        mock.get_count(Method::POST, "/ngsi-ld/v1/entities").await,
        2,
        "each entity is created individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// When the source reports the entity already exists and the mode is replace,
/// the broker follows up with a per-entity Replace Entity. Confirms REQ-PROV-60.
#[tokio::test]
async fn batch_upsert_already_exists_replace_mode_forwards_replace() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-replace-v1");
    let reg_id = topology.id("registration:batch-upsert-replace");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["createEntity", "replaceEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;
    mock.set_stub(Method::PUT, &format!("/ngsi-ld/v1/entities/{v1}"), 204).await;

    let upserted = client.batch_upsert(&json!([speed_vehicle(&v1, 1)])).await.expect("batch upsert");
    assert!(
        upserted.status.as_u16() < 500,
        "the upsert resolves without a server error: {}",
        upserted.status
    );
    assert!(
        mock.get_count(Method::PUT, &format!("/ngsi-ld/v1/entities/{v1}")).await >= 1,
        "replace mode drives a Replace Entity"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// When the source reports the entity already exists and the mode is update, the
/// broker follows up with a per-entity Update Attributes. Confirms REQ-PROV-61.
#[tokio::test]
async fn batch_upsert_already_exists_update_mode_forwards_update() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-updmode-v1");
    let reg_id = topology.id("registration:batch-upsert-updmode");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["createEntity", "updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;
    mock.set_stub(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}/attrs"), 204).await;

    let upserted = client.batch_upsert(&json!([speed_vehicle(&v1, 1)])).await.expect("batch upsert");
    assert!(
        upserted.status.as_u16() < 500,
        "the upsert resolves without a server error: {}",
        upserted.status
    );
    assert!(
        mock.get_count(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}/attrs")).await >= 1,
        "update mode drives an Update Attributes"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// When the source reports the entity already exists but no follow-up operation
/// is advertised, the entity fails with `422 OperationNotSupported` keyed by
/// `registrationId`. Confirms REQ-PROV-62.
#[tokio::test]
async fn batch_upsert_already_exists_no_path_operation_not_supported() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-nopath-v1");
    let reg_id = topology.id("registration:batch-upsert-nopath");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;

    let upserted = client.batch_upsert(&json!([speed_vehicle(&v1, 1)])).await.expect("batch upsert");
    assert_status(&upserted, 207);
    assert!(
        error_keyed(&json_body(&upserted), &reg_id, 422),
        "the missing follow-up surfaces 422: {}",
        json_body(&upserted)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A replace-only source without the batch operation is driven per entity via
/// Replace Entity. Confirms REQ-PROV-63.
#[tokio::test]
async fn batch_upsert_fallback_per_entity_replace() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-fbreplace-v1");
    let v2 = topology.id("Vehicle:batch-upsert-fbreplace-v2");
    let reg_id = topology.id("registration:batch-upsert-fbreplace");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["replaceEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PUT, &format!("/ngsi-ld/v1/entities/{v1}"), 204).await;
    mock.set_stub(Method::PUT, &format!("/ngsi-ld/v1/entities/{v2}"), 204).await;

    let _ = client
        .batch_upsert(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch upsert");
    assert_eq!(
        mock.get_count(Method::PUT, &format!("/ngsi-ld/v1/entities/{v1}")).await,
        1,
        "V1 is replaced individually"
    );
    assert_eq!(
        mock.get_count(Method::PUT, &format!("/ngsi-ld/v1/entities/{v2}")).await,
        1,
        "V2 is replaced individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An update-only source without the batch operation is driven per entity via
/// Update Attributes. Confirms REQ-PROV-64.
#[tokio::test]
async fn batch_upsert_fallback_per_entity_update() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-fbupdate-v1");
    let v2 = topology.id("Vehicle:batch-upsert-fbupdate-v2");
    let reg_id = topology.id("registration:batch-upsert-fbupdate");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, &format!("/ngsi-ld/v1/entities/{v1}/attrs"), 204).await;
    mock.set_stub(Method::POST, &format!("/ngsi-ld/v1/entities/{v2}/attrs"), 204).await;

    let _ = client
        .batch_upsert(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch upsert");
    assert_eq!(
        mock.get_count(Method::POST, &format!("/ngsi-ld/v1/entities/{v1}/attrs")).await,
        1,
        "V1 attrs are posted individually"
    );
    assert_eq!(
        mock.get_count(Method::POST, &format!("/ngsi-ld/v1/entities/{v2}/attrs")).await,
        1,
        "V2 attrs are posted individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration advertising no viable operation fails every owned
/// entity with a `409 Conflict` keyed by `registrationId`. Confirms REQ-PROV-65.
#[tokio::test]
async fn batch_upsert_exclusive_no_viable_op_conflict_per_entity() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-noviable-v1");
    let v2 = topology.id("Vehicle:batch-upsert-noviable-v2");
    let reg_id = topology.id("registration:batch-upsert-noviable");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let upserted = client
        .batch_upsert(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch upsert");
    assert_status(&upserted, 207);
    let body = json_body(&upserted);
    let conflicts = body["errors"]
        .as_array()
        .expect("errors array")
        .iter()
        .filter(|entry| entry["registrationId"].as_str() == Some(reg_id.as_str()) && entry["error"]["status"].as_u64() == Some(409))
        .count();
    assert_eq!(conflicts, 2, "both owned entities are rejected with a keyed conflict: {body}");
    assert_eq!(mock.get_total_count().await, 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local phase applies the upsert to a locally held entity, populating the
/// result, while an auxiliary registration suppresses forwarding.
/// Confirms REQ-PROV-66.
#[tokio::test]
async fn batch_upsert_local_phase_per_mode() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-upsert-local-v1");
    let reg_id = topology.id("registration:batch-upsert-local");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&v1, "Vehicle", &[("brandName", property(json!("Mercedes")))]))
        .send()
        .await
        .expect("seed local");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let replacement = entity(&v1, "Vehicle", &[("brandName", property(json!("BMW")))]);
    let upserted = client.batch_upsert(&json!([replacement])).await.expect("batch upsert");
    assert!(upserted.status.as_u16() < 400, "the local upsert succeeds: {}", upserted.status);
    assert!(
        success_contains(&json_body(&upserted), &v1),
        "the locally upserted entity is reported: {}",
        json_body(&upserted)
    );

    let local = client
        .get_entity(&v1, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["brandName"]["value"].as_str(), Some("BMW"), "the local replace took effect");
    assert_eq!(mock.get_total_count().await, 0, "an auxiliary registration does not forward");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
