//! Snapshot locality and lifecycle: a snapshot captures data, its operations are
//! implicitly local, it is orthogonal to tenancy, and low-resource eviction
//! follows priority (ETSI GS CIM 009 v1.9.1, clause 5.16). `UNKNOWN` status,
//! probing the snapshot resources first; the eviction rows lack a deterministic
//! trigger and are ignored.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, json_body, location},
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient};
use axum::http::Method;
use serde_json::{Value, json};

/// A minimal valid snapshot creation body: a Snapshot resource with one query
/// and a bounded lifetime (clause 5.16).
fn snapshot_body() -> Value {
    json!({ "type": "Snapshot", "snapshotLifetime": "PT1H", "snapshotQueries": [{ "type": "Query" }] })
}

/// Create a snapshot through the given client and return its identifier.
async fn create_snapshot(client: &TestClient) -> String {
    let created = client
        .raw(Method::POST, "/ngsi-ld/v1/snapshots")
        .json(&snapshot_body())
        .send()
        .await
        .expect("create snapshot");
    assert_status(&created, 201);
    json_body(&created)["id"].as_str().expect("snapshot has an id").to_owned()
}

/// Register an inclusive query CSR at the mock and assert acceptance.
async fn register_inclusive_query(topology: &DistributedTopology, endpoint: &str, name: &str) {
    let registration = CsrBuilder::new(&topology.id(name), endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("create registration"),
        201,
    );
}

/// Snapshot creation returns a usable identifier and location (clause 5.16).
/// Confirms REQ-FW-84.
#[tokio::test]
async fn distop_entity_snapshot_creation_returns_identifier() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let created = topology
        .front()
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/snapshots")
        .json(&snapshot_body())
        .send()
        .await
        .expect("create snapshot");
    assert_status(&created, 201);
    assert!(json_body(&created)["id"].as_str().is_some(), "snapshot creation returns an identifier");
    assert!(location(&created).contains("/snapshots/"), "snapshot creation returns a location");
    topology.finish().await.expect("topology cleanup");
}

/// A snapshot operation is scoped to the captured information and does not see a
/// later mutation (clause 5.16). Confirms REQ-FW-80.
#[tokio::test]
async fn distop_entity_snapshot_op_scoped_to_snapshot_information() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:snapshot-scoped");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 10))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let snapshot_id = create_snapshot(topology.front().client()).await;
    assert_status(
        &topology
            .front()
            .client()
            .replace_entity(&entity_id, &vehicle(&entity_id, 99), "application/ld+json", None)
            .await
            .expect("mutate"),
        204,
    );

    let scoped = topology
        .front()
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type=Vehicle&id={entity_id}"))
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot-scoped query");
    assert_status(&scoped, 200);
    let reflects_mutation = json_body(&scoped)
        .as_array()
        .into_iter()
        .flatten()
        .any(|entity| entity["id"].as_str() == Some(entity_id.as_str()) && entity["speed"]["value"].as_i64() == Some(99));
    assert!(!reflects_mutation, "a snapshot operation does not reflect a mutation made after capture");

    topology.finish().await.expect("topology cleanup");
}

/// A snapshot permits Core (and Temporal) operations while other operations are
/// rejected (clause 5.16). Confirms REQ-FW-81.
#[tokio::test]
async fn distop_entity_snapshot_permitted_ops_core_plus_temporal() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let snapshot_id = create_snapshot(topology.front().client()).await;

    let query = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&query, 200);

    let write = topology
        .front()
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/entities")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("content-type", "application/ld+json")
        .json(&vehicle(&topology.id("Vehicle:snapshot-write"), 10))
        .send()
        .await
        .expect("snapshot write");
    assert!(write.status.as_u16() >= 400, "a write against a snapshot is rejected: {}", write.status);

    topology.finish().await.expect("topology cleanup");
}

/// Snapshot operations are implicitly local: no source is contacted even without
/// a `local` parameter (clause 5.16). Confirms REQ-FW-82.
#[tokio::test]
async fn distop_entity_snapshot_ops_implicitly_local() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-local").await;
    let snapshot_id = create_snapshot(topology.front().client()).await;

    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count().await, 0, "a snapshot operation contacts no source");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A snapshot operation's implicit locality skips the registry entirely (clause
/// 5.16). Confirms REQ-FW-83.
#[tokio::test]
async fn distop_entity_snapshot_implicit_local_skips_registry_csrs() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-skip-a").await;
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-skip-b").await;
    let snapshot_id = create_snapshot(topology.front().client()).await;

    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count().await, 0, "no registration is consulted for a snapshot operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An explicit `local=false` is ignored for a snapshot operation: still no
/// forwarding (clause 5.16). Confirms REQ-FW-70.
#[tokio::test]
async fn distop_entity_snapshot_local_false_ignored() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-localfalse").await;
    let snapshot_id = create_snapshot(topology.front().client()).await;

    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle&local=false")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count().await, 0, "local=false is ignored, a snapshot operation stays local");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A tenant snapshot resolves only with both the snapshot id and its tenant: the
/// same id is unknown under the default tenant (clause 5.16, 4.14). Confirms
/// REQ-FW-85.
#[tokio::test]
async fn distop_entity_snapshot_orthogonal_to_tenant_requires_both() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantSnapshot").expect("valid tenant name");
    let tenant_client = topology.front().tenant_client(tenant);
    let snapshot_id = create_snapshot(&tenant_client).await;

    let under_tenant = tenant_client
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("tenant snapshot query");
    assert_status(&under_tenant, 200);

    let under_default = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("default snapshot query");
    assert_status(&under_default, 404);

    assert_status(
        &tenant_client
            .raw(Method::DELETE, &format!("/ngsi-ld/v1/snapshots/{snapshot_id}"))
            .send()
            .await
            .expect("tenant snapshot cleanup"),
        204,
    );
    topology.finish().await.expect("topology cleanup");
}

/// Under low resources a broker MAY delete a snapshot (clause 5.16). Confirms
/// REQ-FW-86.
#[ignore = "non-deterministic REQ-FW-86: requires induced resource pressure with no deterministic trigger"]
#[tokio::test]
async fn distop_entity_snapshot_low_resources_may_delete() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let snapshot_id = create_snapshot(topology.front().client()).await;
    let retrieved = topology
        .front()
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/snapshots/{snapshot_id}"))
        .send()
        .await
        .expect("retrieve snapshot");
    assert_status(&retrieved, 200);
    topology.finish().await.expect("topology cleanup");
}

/// Under eviction the lowest `snapshotPriority` is deleted first, ties broken by
/// `expiresAt`/`lastUsedAt` (clause 5.16). Confirms REQ-FW-87.
#[ignore = "non-deterministic REQ-FW-87: eviction trigger is non-deterministic"]
#[tokio::test]
async fn distop_entity_snapshot_deletion_order_by_priority() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let low_priority = json!({ "type": "Snapshot", "snapshotLifetime": "PT1H", "snapshotPriority": 1, "snapshotQueries": [{ "type": "Query" }] });
    let high_priority = json!({ "type": "Snapshot", "snapshotLifetime": "PT1H", "snapshotPriority": 10, "snapshotQueries": [{ "type": "Query" }] });
    assert_status(
        &topology
            .front()
            .client()
            .raw(Method::POST, "/ngsi-ld/v1/snapshots")
            .json(&low_priority)
            .send()
            .await
            .expect("low priority"),
        201,
    );
    assert_status(
        &topology
            .front()
            .client()
            .raw(Method::POST, "/ngsi-ld/v1/snapshots")
            .json(&high_priority)
            .send()
            .await
            .expect("high priority"),
        201,
    );
    topology.finish().await.expect("topology cleanup");
}
