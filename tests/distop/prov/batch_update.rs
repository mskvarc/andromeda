//! Batch Update forwarding — REQ-PROV-67..72 (clause 5.6.9.4).
//!
//! Update forwards each registration only the entities that actually carry an
//! attribute it owns (attribute-less entities are dropped); a source advertising
//! the batch operation is handed the whole batch and its result merged keyed by
//! `registrationId`; otherwise the broker drives per-entity Update Attributes
//! (overwrite) or Append (no-overwrite). An exclusive registration advertising
//! no viable operation fails each owned entity with `409 Conflict`.

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

/// A registration owning `speed` receives only the entities that carry `speed`;
/// an attribute-less entity is dropped from its copy. Confirms REQ-PROV-67.
#[tokio::test]
async fn batch_update_per_csr_filtering_drops_attrless_entities() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-filter-v1");
    let v2 = topology.id("Vehicle:batch-update-filter-v2");
    let reg_speed = topology.id("registration:batch-update-filter-speed");
    let reg_gear = topology.id("registration:batch-update-filter-gear");

    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s1"))
        .mode("exclusive")
        .property_names(&["speed"])
        .operations(&["updateAttrs", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    let gear_csr = CsrBuilder::new(&reg_gear, &mock.url("/s2"))
        .mode("exclusive")
        .property_names(&["gear"])
        .operations(&["updateAttrs", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&gear_csr).await.expect("create gear registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/update", 204).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/update", 204).await;

    let batch = json!([speed_vehicle(&v1, 42), entity(&v2, "Vehicle", &[("brandName", property(json!("Mercedes")))])]);
    let dispatched = client.batch_update(&batch).await.expect("batch update");
    assert!(dispatched.status.as_u16() < 500, "the batch update is dispatched: {}", dispatched.status);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).contains("/s1"),
        "only the speed source is forwarded to: {}",
        request_path(&forwarded)
    );
    let body = forwarded.json::<Value>().expect("forwarded update batch is JSON");
    let entities = forwarded_entities(&body);
    assert!(entity_with_id(&entities, &v1).is_some(), "V1 carrying speed is forwarded: {body}");
    assert!(entity_with_id(&entities, &v2).is_none(), "V2 with no owned attribute is dropped: {body}");
    let v1_copy = entity_with_id(&entities, &v1).expect("V1 forwarded copy");
    assert!(attr_present(v1_copy, "speed"), "the owned attribute is present in the copy: {v1_copy}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source advertising the batch operation is handed the whole batch and its
/// result merged keyed by `registrationId`. Confirms REQ-PROV-68.
#[tokio::test]
async fn batch_update_supported_forwards_and_merges() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-merge-v1");
    let v2 = topology.id("Vehicle:batch-update-merge-v2");
    let reg_id = topology.id("registration:batch-update-merge");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["updateAttrs", "queryBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let source_result = json!({
        "success": [v1.as_str()],
        "errors": [ { "entityId": v2.as_str(), "error": { "status": 404, "type": "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound", "title": "missing" } } ]
    });
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/update", 207, &source_result)
        .await;

    let updated = client
        .batch_update(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch update");
    assert_status(&updated, 207);
    assert!(
        error_keyed(&json_body(&updated), &reg_id, 404),
        "the source error is merged keyed by registrationId: {}",
        json_body(&updated)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without the batch operation, an update-capable source is driven per entity via
/// Update Attributes. Confirms REQ-PROV-69.
#[tokio::test]
async fn batch_update_fallback_per_entity_update_overwrite() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-fbupdate-v1");
    let v2 = topology.id("Vehicle:batch-update-fbupdate-v2");
    let reg_id = topology.id("registration:batch-update-fbupdate");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}/attrs"), 204).await;
    mock.set_stub(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v2}/attrs"), 204).await;

    let _ = client
        .batch_update(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch update");
    assert_eq!(
        mock.get_count(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}/attrs")).await,
        1,
        "V1 is updated individually"
    );
    assert_eq!(
        mock.get_count(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v2}/attrs")).await,
        1,
        "V2 is updated individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With `noOverwrite`, an append-capable source is driven per entity via Append
/// Attributes rather than Update. Confirms REQ-PROV-70.
#[tokio::test]
async fn batch_update_fallback_per_entity_append_no_overwrite() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-fbappend-v1");
    let v2 = topology.id("Vehicle:batch-update-fbappend-v2");
    let reg_id = topology.id("registration:batch-update-fbappend");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .operations(&["appendAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::POST, &format!("/ngsi-ld/v1/entities/{v1}/attrs"), 204).await;
    mock.set_stub(Method::POST, &format!("/ngsi-ld/v1/entities/{v2}/attrs"), 204).await;

    let _ = client
        .batch_update(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch update");
    assert_eq!(
        mock.get_count(Method::POST, &format!("/ngsi-ld/v1/entities/{v1}/attrs")).await,
        1,
        "V1 attrs are appended individually"
    );
    assert_eq!(
        mock.get_count(Method::POST, &format!("/ngsi-ld/v1/entities/{v2}/attrs")).await,
        1,
        "V2 attrs are appended individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration advertising no viable operation fails every owned
/// entity with a `409 Conflict` keyed by `registrationId`. Confirms REQ-PROV-71.
#[tokio::test]
async fn batch_update_exclusive_no_viable_op_conflict_per_entity() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-noviable-v1");
    let v2 = topology.id("Vehicle:batch-update-noviable-v2");
    let reg_id = topology.id("registration:batch-update-noviable");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let updated = client
        .batch_update(&json!([speed_vehicle(&v1, 1), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch update");
    assert_status(&updated, 207);
    let body = json_body(&updated);
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

/// The local phase applies to a locally held entity and reports the absent one
/// as `404`, while an auxiliary registration suppresses forwarding.
/// Confirms REQ-PROV-72.
#[tokio::test]
async fn batch_update_local_phase_populates_results() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:batch-update-local-v1");
    let v2 = topology.id("Vehicle:batch-update-local-v2");
    let reg_id = topology.id("registration:batch-update-local");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&speed_vehicle(&v1, 1))
        .send()
        .await
        .expect("seed local");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .operations(&["updateAttrs", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let updated = client
        .batch_update(&json!([speed_vehicle(&v1, 99), speed_vehicle(&v2, 2)]))
        .await
        .expect("batch update");
    assert_status(&updated, 207);
    let body = json_body(&updated);
    assert!(success_contains(&body, &v1), "the present entity is updated: {body}");
    assert!(
        body["errors"].as_array().is_some_and(|entries| entries
            .iter()
            .any(|entry| entry["entityId"].as_str() == Some(v2.as_str()) && entry["error"]["status"].as_u64() == Some(404))),
        "the absent entity is reported as 404: {body}"
    );
    assert_eq!(mock.get_total_count().await, 0, "an auxiliary registration does not forward");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
