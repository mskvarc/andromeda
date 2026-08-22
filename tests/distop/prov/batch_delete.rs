//! Batch Delete forwarding — REQ-PROV-73..77 (clause 5.6.10.4).
//!
//! Each matching registration is forwarded only the ids it owns; a source
//! advertising the batch delete is forwarded the batch and its results merge
//! keyed by `registrationId`; otherwise the broker falls back to one delete per
//! id, and a redirect source advertising neither yields a per-entity
//! `409 Conflict`. The local phase removes the matched ids and reports them in
//! `success`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_status, json_body},
    prov::forwarding::{FORWARD_WAIT, request_path},
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use axum::http::Method;
use serde_json::{Value, json};

/// Per-CSR filtering forwards each redirect source only the id it owns.
/// Confirms REQ-PROV-73.
#[tokio::test]
async fn batch_delete_per_csr_filtering() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:prov-batch-delete-filter-v1");
    let v2 = topology.id("Vehicle:prov-batch-delete-filter-v2");
    let reg_v1 = topology.id("registration:prov-batch-delete-filter-v1");
    let reg_v2 = topology.id("registration:prov-batch-delete-filter-v2");

    let v1_csr = CsrBuilder::new(&reg_v1, &mock.url("/s1"))
        .mode("redirect")
        .entity_id(&v1)
        .operations(&["deleteBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&v1_csr).await.expect("create v1 registration"), 201);
    let v2_csr = CsrBuilder::new(&reg_v2, &mock.url("/s2"))
        .mode("redirect")
        .entity_id(&v2)
        .operations(&["deleteBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&v2_csr).await.expect("create v2 registration"), 201);
    mock.set_stub(Method::POST, "/s1/ngsi-ld/v1/entityOperations/delete", 204).await;
    mock.set_stub(Method::POST, "/s2/ngsi-ld/v1/entityOperations/delete", 204).await;

    client.batch_delete(&json!([v1, v2])).await.expect("batch delete");

    let first = mock.next_request(FORWARD_WAIT).await;
    let second = mock.next_request(FORWARD_WAIT).await;
    let (s1_request, s2_request) = if request_path(&first).contains("/s1") {
        (first, second)
    } else {
        (second, first)
    };
    let s1_ids = s1_request.json::<Value>().expect("s1 delete body is JSON");
    let s2_ids = s2_request.json::<Value>().expect("s2 delete body is JSON");
    let s1_ids = s1_ids.as_array().expect("s1 body is an id array");
    let s2_ids = s2_ids.as_array().expect("s2 body is an id array");
    assert!(s1_ids.iter().any(|value| value == &json!(v1)), "s1 owns v1: {s1_ids:?}");
    assert!(!s1_ids.iter().any(|value| value == &json!(v2)), "s1 does not own v2: {s1_ids:?}");
    assert!(s2_ids.iter().any(|value| value == &json!(v2)), "s2 owns v2: {s2_ids:?}");
    assert!(!s2_ids.iter().any(|value| value == &json!(v1)), "s2 does not own v1: {s2_ids:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source advertising the batch delete is forwarded the batch and its results
/// merge keyed by `registrationId`. Confirms REQ-PROV-74.
#[tokio::test]
async fn batch_delete_supported_forwards_and_merges() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:prov-batch-delete-merge-v1");
    let v2 = topology.id("Vehicle:prov-batch-delete-merge-v2");
    let reg_id = topology.id("registration:prov-batch-delete-merge");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type("Vehicle")
        .operations(&["deleteBatch", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let remote_result = json!({
        "success": [v1],
        "errors": [ { "entityId": v2, "error": { "status": 404, "type": "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound", "title": "absent" } } ]
    });
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/delete", 207, &remote_result)
        .await;

    let response = client.batch_delete(&json!([v1, v2])).await.expect("batch delete");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    assert!(
        errors.iter().any(|entry| entry["registrationId"] == json!(reg_id)),
        "the merged remote error is keyed by registrationId: {body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without the batch operation the broker falls back to one delete per id.
/// Confirms REQ-PROV-75.
#[tokio::test]
async fn batch_delete_fallback_per_entity_delete() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:prov-batch-delete-fallback-v1");
    let v2 = topology.id("Vehicle:prov-batch-delete-fallback-v2");
    let reg_id = topology.id("registration:prov-batch-delete-fallback");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type("Vehicle")
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let v1_path = format!("/ngsi-ld/v1/entities/{v1}");
    let v2_path = format!("/ngsi-ld/v1/entities/{v2}");
    mock.set_stub(Method::DELETE, &v1_path, 204).await;
    mock.set_stub(Method::DELETE, &v2_path, 204).await;

    client.batch_delete(&json!([v1, v2])).await.expect("batch delete");

    assert_eq!(mock.get_count(Method::DELETE, &v1_path).await, 1, "v1 is deleted per id");
    assert_eq!(mock.get_count(Method::DELETE, &v2_path).await, 1, "v2 is deleted per id");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect source advertising neither the batch nor the per-entity delete
/// yields a per-entity `409 Conflict`, with no forwarding. The registration
/// precedes the local seeds (clause 5.9.2.4 redirect restriction). Confirms
/// REQ-PROV-76.
#[tokio::test]
async fn batch_delete_redirect_neither_op_conflict_per_entity() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:prov-batch-delete-neither-v1");
    let v2 = topology.id("Vehicle:prov-batch-delete-neither-v2");
    let reg_id = topology.id("registration:prov-batch-delete-neither");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_type("Vehicle")
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    for id in [&v1, &v2] {
        let seeded = client
            .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
            .header("content-type", "application/ld+json")
            .json(&entity(id, "Vehicle", &[("speed", property(json!(1)))]))
            .send()
            .await
            .expect("seed local entity");
        assert_status(&seeded, 201);
    }

    let response = client.batch_delete(&json!([v1, v2])).await.expect("batch delete");
    assert_status(&response, 207);
    let body = json_body(&response);
    let errors = body["errors"].as_array().expect("multi-status carries an errors array");
    let conflicts = errors
        .iter()
        .filter(|entry| entry["error"]["status"] == json!(409) && entry["registrationId"] == json!(reg_id))
        .count();
    assert_eq!(conflicts, 2, "each id surfaces a keyed conflict: {body}");
    assert_eq!(mock.get_total_count().await, 0, "no forwarding on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local phase removes the matched ids and reports them in `success`, with
/// the auxiliary registration forwarding nothing. Confirms REQ-PROV-77.
#[tokio::test]
async fn batch_delete_local_phase_removes_ids() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let v1 = topology.id("Vehicle:prov-batch-delete-local-v1");
    let v2 = topology.id("Vehicle:prov-batch-delete-local-v2");
    let reg_id = topology.id("registration:prov-batch-delete-local");

    for id in [&v1, &v2] {
        let seeded = client
            .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
            .header("content-type", "application/ld+json")
            .json(&entity(id, "Vehicle", &[("speed", property(json!(1)))]))
            .send()
            .await
            .expect("seed local entity");
        assert_status(&seeded, 201);
    }

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("auxiliary")
        .entity_type("Vehicle")
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);

    let response = client.batch_delete(&json!([v1, v2])).await.expect("batch delete");
    let status = response.status.as_u16();
    assert!(matches!(status, 200 | 204), "a wholly successful batch delete succeeds, got {status}");
    if status == 200 {
        let body = json_body(&response);
        for id in [&v1, &v2] {
            assert!(
                body.as_array().is_some_and(|success| success.iter().any(|value| value == &json!(id)))
                    || body["success"]
                        .as_array()
                        .is_some_and(|success| success.iter().any(|value| value == &json!(id))),
                "{id} is reported as deleted: {body}"
            );
        }
    }
    for id in [&v1, &v2] {
        let local = client.get_entity(id, "local=true", "application/ld+json", None).await.expect("local read-back");
        assert_status(&local, 404);
    }
    assert_eq!(
        mock.get_total_count().await,
        0,
        "the auxiliary registration does not forward provision operations"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
