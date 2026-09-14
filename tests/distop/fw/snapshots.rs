//! Snapshot locality and lifecycle: a snapshot captures data, its operations are
//! implicitly local, it is orthogonal to tenancy, and low-resource eviction
//! follows priority (ETSI GS CIM 009 v1.9.1, clause 5.16). Snapshot creation
//! returns before its queries run (clause 5.16.1.4: "The request returns,
//! confirming the creation of the snapshot ... The following is executed in the
//! background"), so every row that scopes an operation to a snapshot first polls
//! its status out of `preparing`. The eviction rows lack a deterministic
//! trigger and are ignored.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, json_body, location},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// How long a snapshot may stay in `preparation` before the test gives up.
const PREPARATION_BUDGET: Duration = Duration::from_secs(30);

/// The pause between two status polls.
const PREPARATION_POLL: Duration = Duration::from_millis(200);

/// A minimal valid snapshot creation body: a Snapshot resource with one query
/// and a bounded lifetime (clause 5.16), carrying its `@context` inline because
/// it is sent as `application/ld+json` (clause 6.3.5).
fn snapshot_body() -> Value {
    json!({ "type": "Snapshot", "snapshotLifetime": "PT1H", "snapshotQueries": [{ "type": "Query" }], "@context": COMPOUND_CONTEXT.as_str() })
}

/// Create a snapshot through the given client and return its identifier once its
/// background preparation has finished.
async fn create_snapshot(client: &TestClient) -> String {
    create_prepared_snapshot(client, &snapshot_body()).await
}

/// Create a snapshot from `body` and poll `GET /snapshots/{id}` until its
/// `snapshotStatus` has left the pending state, returning the snapshot
/// identifier. Table 5.2.41-2 names the initial status `preparing`; the
/// `preparation` spelling in clause 5.16.1.4 is an editorial error reconciled
/// by the Snapshot conformance register and is never accepted or emitted.
pub async fn create_prepared_snapshot(client: &TestClient, body: &Value) -> String {
    let created = client.create_snapshot(body).await.expect("create snapshot");
    assert_status(&created, 201);
    let snapshot_id = location(&created)
        .rsplit('/')
        .next()
        .filter(|identifier| !identifier.is_empty())
        .expect("Snapshot Location ends in an identifier")
        .to_owned();
    let started = Instant::now();
    loop {
        let status = client.retrieve_snapshot(&snapshot_id, None).await.expect("retrieve snapshot status");
        assert_status(&status, 200);
        let snapshot_status = json_body(&status)["snapshotStatus"].as_str().map(str::to_owned).unwrap_or_default();
        if snapshot_status != "preparing" {
            return snapshot_id;
        }
        assert!(
            started.elapsed() < PREPARATION_BUDGET,
            "snapshot {snapshot_id} is still in preparation after {PREPARATION_BUDGET:?}"
        );
        sleep(PREPARATION_POLL).await;
    }
}

/// Register an inclusive query CSR at the mock and assert acceptance.
async fn register_inclusive_query(topology: &MultiBrokerTopology<BrokerRole>, endpoint: &str, name: &str) {
    let registration = CsrBuilder::new(&topology.id(name), endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );
}

/// Snapshot creation returns a usable identifier and location (clause 5.16).
/// Confirms REQ-FW-84.
#[tokio::test]
async fn distop_entity_snapshot_creation_returns_identifier() {
    let topology = topology::single().await.expect("single-broker topology");
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_snapshot(&snapshot_body())
        .await
        .expect("create snapshot");
    assert_status(&created, 201);
    assert!(created.body.is_empty(), "Snapshot creation returns an empty 201 response");
    let created_location = location(&created);
    assert!(created_location.contains("/snapshots/"), "snapshot creation returns a location");
    assert!(!created_location.ends_with('/'), "Snapshot Location ends in the generated identifier");
    topology.finish().await.expect("topology cleanup");
}

/// A snapshot operation is scoped to the captured information and does not see a
/// later mutation (clause 5.16). Confirms REQ-FW-80.
#[tokio::test]
async fn distop_entity_snapshot_op_scoped_to_snapshot_information() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:snapshot-scoped");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 10))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .replace_entity(&entity_id, &vehicle(&entity_id, 99), "application/ld+json", None)
            .await
            .expect("mutate"),
        204,
    );

    let scoped = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
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

/// A snapshot permits both Core and Temporal operations against its isolated
/// local branch (clause 5.16). Confirms REQ-FW-81.
#[tokio::test]
async fn distop_entity_snapshot_permitted_ops_core_plus_temporal() {
    let topology = topology::single().await.expect("single-broker topology");
    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;

    let query = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&query, 200);

    let write = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/entities")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("content-type", "application/ld+json")
        .json(&vehicle(&topology.id("Vehicle:snapshot-write"), 10))
        .send()
        .await
        .expect("snapshot write");
    assert_status(&write, 201);

    topology.finish().await.expect("topology cleanup");
}

/// Snapshot operations are implicitly local: no source is contacted even without
/// a `local` parameter (clause 5.16). Confirms REQ-FW-82.
#[tokio::test]
async fn distop_entity_snapshot_ops_implicitly_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-local").await;
    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;
    let forwarded_during_capture = mock.get_total_count();

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(
        mock.get_total_count(),
        forwarded_during_capture,
        "a scoped operation contacts no source after capture"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A snapshot operation's implicit locality skips the registry entirely (clause
/// 5.16). Confirms REQ-FW-83.
#[tokio::test]
async fn distop_entity_snapshot_implicit_local_skips_registry_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-skip-a").await;
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-skip-b").await;
    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;
    let forwarded_during_capture = mock.get_total_count();

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(
        mock.get_total_count(),
        forwarded_during_capture,
        "no registration is consulted for a scoped operation after capture"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An explicit `local=false` is ignored for a snapshot operation: still no
/// forwarding (clause 5.16). Confirms REQ-FW-70.
#[tokio::test]
async fn distop_entity_snapshot_local_false_ignored() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:snapshot-localfalse").await;
    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;
    let forwarded_during_capture = mock.get_total_count();

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle&local=false")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("snapshot query");
    assert_status(&response, 200);
    assert_eq!(
        mock.get_total_count(),
        forwarded_during_capture,
        "local=false is ignored, so the scoped operation stays local after capture"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A tenant snapshot resolves only with both the snapshot id and its tenant: the
/// same id is unknown under the default tenant (clause 5.16, 4.14). The tenant
/// is brought into existence by a Create Entity first, since clause 5.5.10
/// lists the operations that implicitly create a Tenant ("Create Entity (clause
/// 5.6.1). Batch Entity Creation (clause 5.6.7). Create or Update Temporal
/// Evolution of an Entity (clause 5.6.11). Create Subscription (clause 5.8.1).
/// Register Context Source (clause 5.9.2). Create Context Source Registration
/// (clause 5.11.2)") and snapshot creation is not among them. Confirms
/// REQ-FW-85.
#[tokio::test]
async fn distop_entity_snapshot_orthogonal_to_tenant_requires_both() {
    let topology = topology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantSnapshot").expect("valid tenant name");
    let tenant_client = topology.broker(BrokerRole::A).expect("topology broker A").tenant_client(tenant);
    assert_status(
        &tenant_client
            .post_entity(&vehicle(&topology.id("Vehicle:snapshot-tenant"), 10))
            .with_ld_json()
            .send()
            .await
            .expect("create tenant entity"),
        201,
    );
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
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("NGSILD-Snapshot", snapshot_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("default snapshot query");
    assert_status(&under_default, 404);

    assert_status(&tenant_client.delete_snapshot(&snapshot_id).await.expect("tenant snapshot cleanup"), 204);
    topology.finish().await.expect("topology cleanup");
}

/// Under low resources a broker MAY delete a snapshot (clause 5.16). Confirms
/// REQ-FW-86.
#[ignore = "non-deterministic REQ-FW-86: requires induced resource pressure with no deterministic trigger"]
#[tokio::test]
async fn distop_entity_snapshot_low_resources_may_delete() {
    let topology = topology::single().await.expect("single-broker topology");
    let snapshot_id = create_snapshot(topology.broker(BrokerRole::A).expect("topology broker A").client()).await;
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_snapshot(&snapshot_id, None)
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
    let topology = topology::single().await.expect("single-broker topology");
    let mut low_priority = snapshot_body();
    low_priority["snapshotPriority"] = json!(1);
    let mut high_priority = snapshot_body();
    high_priority["snapshotPriority"] = json!(10);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_snapshot(&low_priority)
            .await
            .expect("low priority"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_snapshot(&high_priority)
            .await
            .expect("high priority"),
        201,
    );
    topology.finish().await.expect("topology cleanup");
}
