//! Server-generated CSR health fields (clause 5.2.9, `status` and counters).
//!
//! The static half is `IMPL`: client-supplied health values are ignored and a
//! fresh registration exposes only server-owned counters. The dynamic half —
//! flipping `status` and incrementing counters on a real forward — is driven
//! through a mock context source (`T-MOCK`).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::{CsrBuilder, csr_basic},
    topology::DistributedTopology,
};
use reqwest::Method;
use serde_json::json;
use urlencoding::encode;

/// Client-supplied `status`, `timesSent`, `timesFailed` and `lastSuccess` are
/// server-owned and must be discarded, not stored (clause 5.2.9). Confirms
/// REQ-REG-30, REQ-REG-31, REQ-REG-32.
#[tokio::test]
async fn reg_client_supplied_health_fields_ignored() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:health-ignored");
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["status"] = json!("failed");
    body["timesSent"] = json!(99);
    body["timesFailed"] = json!(42);
    body["lastSuccess"] = json!("2000-01-01T00:00:00Z");

    let created = topology.front().client().create_registration(&body).await.expect("create registration");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_registration(&id, None).await.expect("retrieve registration");
    assert_status(&retrieved, 200);
    let stored = json_body(&retrieved);
    assert_ne!(stored["status"], json!("failed"), "server owns status; the client-forced value is discarded");
    assert_ne!(stored["timesSent"], json!(99), "server owns timesSent; the client value is discarded");
    assert_ne!(stored["timesFailed"], json!(42), "server owns timesFailed; the client value is discarded");

    topology.finish().await.expect("topology cleanup");
}

/// A freshly created registration reports non-negative delivery counters and, if
/// present, a `status` drawn from the `CsrStatus` tokens (clause 5.2.9). Confirms
/// REQ-REG-31, REQ-REG-32.
#[tokio::test]
async fn reg_fresh_registration_counters_nonnegative() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let id = topology.id("registration:health-fresh");
    let created = topology
        .front()
        .client()
        .create_registration(&csr_basic(&id, PEER_ENDPOINT))
        .await
        .expect("create registration");
    assert_status(&created, 201);
    let retrieved = topology.front().client().retrieve_registration(&id, None).await.expect("retrieve registration");
    assert_status(&retrieved, 200);
    let stored = json_body(&retrieved);
    if let Some(times_sent) = stored["timesSent"].as_i64() {
        assert!(times_sent >= 0, "timesSent is non-negative on a fresh registration, got {times_sent}");
    }
    if let Some(times_failed) = stored["timesFailed"].as_i64() {
        assert!(times_failed >= 0, "timesFailed is non-negative on a fresh registration, got {times_failed}");
    }
    if let Some(status) = stored["status"].as_str() {
        assert!(matches!(status, "ok" | "failed"), "status is one of the CsrStatus tokens, got {status}");
    }

    topology.finish().await.expect("topology cleanup");
}

/// A forward to an endpoint that always fails flips the registration `status` to
/// `failed` (clause 5.2.9, dynamic). Confirms REQ-REG-31 (dynamic).
#[tokio::test]
async fn reg_health_status_transitions_to_failed_on_forward_error() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:health-fail");
    let registration_id = topology.id("registration:health-fail");
    adapter
        .set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{}", encode(&entity_id)), 502)
        .await;

    let registration = CsrBuilder::new(&registration_id, &adapter.url("/ngsi-ld/v1"))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
        .build();
    let registered = topology
        .front()
        .client()
        .create_registration(&registration)
        .await
        .expect("register mock source");
    assert_status(&registered, 201);

    let _forward = topology
        .front()
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("forwarded retrieve");

    let retrieved = topology
        .front()
        .client()
        .retrieve_registration(&registration_id, None)
        .await
        .expect("retrieve registration");
    assert_status(&retrieved, 200);
    assert_eq!(
        json_body(&retrieved)["status"].as_str(),
        Some("failed"),
        "a failed forward flips CSR status to failed"
    );

    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Every forward attempt increments the delivery counters and stamps the matching
/// success/failure timestamp (clause 5.2.9, dynamic). Confirms REQ-REG-32 (dynamic).
#[tokio::test]
async fn reg_health_counters_increment_on_forward_attempt() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = ContextSourceAdapter::start().await;
    let ok_entity = topology.id("Vehicle:health-ok");
    let fail_entity = topology.id("Vehicle:health-bad");
    let registration_id = topology.id("registration:health-counters");
    adapter
        .set_stub_with_body(
            Method::GET,
            &format!("/ngsi-ld/v1/entities/{}", encode(&ok_entity)),
            200,
            &vehicle(&ok_entity, 10),
        )
        .await;
    adapter
        .set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{}", encode(&fail_entity)), 502)
        .await;

    let registration = CsrBuilder::new(&registration_id, &adapter.url("/ngsi-ld/v1"))
        .mode("inclusive")
        .id_pattern(&format!("{}.*", topology.id("Vehicle:health")))
        .operations(&["retrieveEntity"])
        .build();
    let registered = topology
        .front()
        .client()
        .create_registration(&registration)
        .await
        .expect("register mock source");
    assert_status(&registered, 201);

    let _ok = topology
        .front()
        .client()
        .get_entity(&ok_entity, "", "application/ld+json", None)
        .await
        .expect("forwarded success");
    let _bad = topology
        .front()
        .client()
        .get_entity(&fail_entity, "", "application/ld+json", None)
        .await
        .expect("forwarded failure");

    let retrieved = topology
        .front()
        .client()
        .retrieve_registration(&registration_id, None)
        .await
        .expect("retrieve registration");
    assert_status(&retrieved, 200);
    let stored = json_body(&retrieved);
    assert!(
        stored["timesSent"].as_i64().unwrap_or(0) >= 2,
        "both forwards are counted in timesSent: {}",
        stored["timesSent"]
    );
    assert!(
        stored["timesFailed"].as_i64().unwrap_or(0) >= 1,
        "the failing forward is counted in timesFailed: {}",
        stored["timesFailed"]
    );
    assert!(stored["lastSuccess"].is_string(), "lastSuccess is set after a successful forward");
    assert!(stored["lastFailure"].is_string(), "lastFailure is set after a failed forward");

    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
