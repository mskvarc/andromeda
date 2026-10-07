//! Batch Merge forwarding (clause 5.6.20.4).
//!
//! Merge forwards each registration only the entities carrying an attribute it
//! owns (attribute-less entities are dropped); a source advertising the batch
//! operation is handed the whole batch and its result merged keyed by
//! `registrationId`; otherwise the broker drives per-entity Merge Entity. An
//! redirect registration advertising no viable operation fails each owned entity
//! with `409 Conflict`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, attr_present, entity_with_id, forwarded_entities, request_path},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A single-`speed` entity of an explicit per-test type.
fn speed_vehicle(id: &str, entity_type: &str, speed: i64) -> Value {
    entity(id, entity_type, &[("speed", property(json!(speed)))])
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
/// an attribute-less entity is dropped from its copy.
#[tokio::test]
async fn batch_merge_per_csr_filtering_drops_attrless() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:batch-merge-filter-v1");
    let v2 = topology.id("Vehicle:batch-merge-filter-v2");
    let reg_speed = topology.id("registration:batch-merge-filter-speed");
    let reg_gear = topology.id("registration:batch-merge-filter-gear");

    let speed_csr = CsrBuilder::new(&reg_speed, &mock.url("/s1"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["mergeBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&speed_csr).await.expect("create speed registration"), 201);
    let gear_csr = CsrBuilder::new(&reg_gear, &mock.url("/s2"))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["gear"])
        .operations(&["mergeBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&gear_csr).await.expect("create gear registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/merge", 204).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/merge", 204).await;

    let batch = json!([
        speed_vehicle(&v1, &vtype, 42),
        entity(&v2, &vtype, &[("brandName", property(json!("Mercedes")))])
    ]);
    let dispatched = client.batch_merge(&batch).await.expect("batch merge");
    assert!(dispatched.status.as_u16() < 500, "the batch merge is dispatched: {}", dispatched.status);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).contains("/s1"),
        "only the speed source is forwarded to: {}",
        request_path(&forwarded)
    );
    let body = forwarded.json::<Value>().expect("forwarded merge batch is JSON");
    let entities = forwarded_entities(&body);
    let v1_copy = entity_with_id(&entities, &v1).expect("V1 carrying speed is forwarded");
    assert!(attr_present(v1_copy, "speed"), "the owned attribute is present in the copy: {v1_copy}");
    assert!(entity_with_id(&entities, &v2).is_none(), "V2 with no owned attribute is dropped: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source advertising the batch operation is handed the whole batch and its
/// result merged keyed by `registrationId`.
#[tokio::test]
async fn batch_merge_supported_forwards_and_merges() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:batch-merge-merge-v1");
    let v2 = topology.id("Vehicle:batch-merge-merge-v2");
    let reg_id = topology.id("registration:batch-merge-merge");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["mergeBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let source_result = json!({
        "success": [v1.as_str()],
        "errors": [ { "entityId": v2.as_str(), "error": { "status": 404, "type": "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound", "title": "missing" } } ]
    });
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/merge", 207, &source_result)
        .await;

    let merged = client
        .batch_merge(&json!([speed_vehicle(&v1, &vtype, 1), speed_vehicle(&v2, &vtype, 2)]))
        .await
        .expect("batch merge");
    assert_status(&merged, 207);
    assert!(
        error_keyed(&json_body(&merged), &reg_id, 404),
        "the source error is merged keyed by registrationId: {}",
        json_body(&merged)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without the batch operation, a merge-capable source is driven per entity via
/// Merge Entity.
#[tokio::test]
async fn batch_merge_fallback_per_entity_merge() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:batch-merge-fb-v1");
    let v2 = topology.id("Vehicle:batch-merge-fb-v2");
    let reg_id = topology.id("registration:batch-merge-fb");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .operations(&["mergeEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    mock.set_stub(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}"), 204).await;
    mock.set_stub(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v2}"), 204).await;

    let _ = client
        .batch_merge(&json!([speed_vehicle(&v1, &vtype, 1), speed_vehicle(&v2, &vtype, 2)]))
        .await
        .expect("batch merge");
    assert_eq!(
        mock.get_count(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v1}")),
        1,
        "V1 is merged individually"
    );
    assert_eq!(
        mock.get_count(Method::PATCH, &format!("/ngsi-ld/v1/entities/{v2}")),
        1,
        "V2 is merged individually"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration advertising no viable operation fails every owned
/// entity with a `409 Conflict` keyed by `registrationId`.
#[tokio::test]
async fn batch_merge_redirect_neither_op_conflict_per_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:batch-merge-noviable-v1");
    let v2 = topology.id("Vehicle:batch-merge-noviable-v2");
    let reg_id = topology.id("registration:batch-merge-noviable");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type(&vtype)
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let merged = client
        .batch_merge(&json!([speed_vehicle(&v1, &vtype, 1), speed_vehicle(&v2, &vtype, 2)]))
        .await
        .expect("batch merge");
    assert_status(&merged, 207);
    let body = json_body(&merged);
    let conflicts = body["errors"]
        .as_array()
        .expect("errors array")
        .iter()
        .filter(|entry| entry["registrationId"].as_str() == Some(reg_id.as_str()) && entry["error"]["status"].as_u64() == Some(409))
        .count();
    assert_eq!(conflicts, 2, "both owned entities are rejected with a keyed conflict: {body}");
    assert_eq!(mock.get_total_count(), 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local phase merges into a locally held entity and, since every entity
/// existed and was merged, answers with no content (clause 5.6.20.5: "none (if
/// all Entities already existed and are successfully merged)"), while an
/// auxiliary registration suppresses forwarding.
#[tokio::test]
async fn batch_merge_local_phase_populates_results() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let v1 = topology.id("Vehicle:batch-merge-local-v1");
    let reg_id = topology.id("registration:batch-merge-local");

    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(&entity(&v1, &vtype, &[("brandName", property(json!("Mercedes")))]))
        .send()
        .await
        .expect("seed local");
    assert_status(&seeded, 201);

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .entity_type(&vtype)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let merged = client.batch_merge(&json!([speed_vehicle(&v1, &vtype, 99)])).await.expect("batch merge");
    assert_status(&merged, 204);

    let local = client
        .get_entity(&v1, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 200);
    assert_eq!(json_body(&local)["speed"]["value"].as_i64(), Some(99), "the local merge added speed");
    assert_eq!(mock.get_total_count(), 0, "an auxiliary registration does not forward");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
