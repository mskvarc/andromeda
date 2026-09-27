//! A failed delivery queued on a Snapshot branch keeps its dormant tenant due:
//! the delivery drain claims from every published Snapshot branch as from the
//! live one, so the wake loop has to see that branch's retry too.
//!
//! Every attempt fails, so the retries keep coming at the stack's backoff for
//! longer than any eviction can take; the test requires one made after the
//! tenant was seen dormant. Nothing on the
//! live branch is due, so a wake that read only the live branch's queue would
//! never load the tenant again, and no attempt would follow.

use crate::{assert_retried_after_dormancy, attempts_after_dormancy, client, control_pool, evict_until_dormant, fail_every_attempt, first_attempt, reset};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{endpoints, tenant_load};
use andromeda_test_mocks::{callback_path::CallbackPath, callback_server::CallbackServer, server_config::MockServerConfig};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::{slice, time::Duration};
use tokio::time::{Instant, sleep};

/// A Snapshot-scoped request for `tenant` on the branch `snapshot`.
fn scoped(client: &Client, method: Method, url: String, tenant: &str, snapshot: &str) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .header("NGSILD-Tenant", tenant)
        .header("NGSILD-Snapshot", snapshot)
        .header("Content-Type", "application/ld+json")
}

/// Create an empty Snapshot of `tenant` and wait for its branch to publish.
async fn published_branch(client: &Client, broker: &str, tenant: &str, snapshot: &str) {
    let created = client
        .post(format!("{broker}snapshots"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/json")
        .json(&json!({"id": snapshot, "type": "Snapshot", "snapshotQueries": []}))
        .send()
        .await
        .expect("create Snapshot request");
    assert_eq!(created.status(), StatusCode::CREATED, "the Snapshot is accepted");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status: Value = client
            .get(format!("{broker}snapshots/{}", urlencoding::encode(snapshot)))
            .header("NGSILD-Tenant", tenant)
            .send()
            .await
            .expect("retrieve Snapshot request")
            .json()
            .await
            .expect("Snapshot body");
        if status["snapshotStatus"] != "preparing" {
            assert_eq!(status["snapshotStatus"], "success", "the empty Snapshot publishes its branch: {status}");
            return;
        }
        assert!(Instant::now() < deadline, "the Snapshot finishes preparing before the deadline");
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[serial_test::serial]
async fn a_failed_snapshot_branch_delivery_is_retried_after_eviction() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = CallbackServer::start(MockServerConfig::from_environment().expect("mock config"))
        .await
        .expect("callback server");
    let path = CallbackPath::parse("/dormant-branch-retry").expect("callback path");
    fail_every_attempt(&server, &path).await;
    let callback = server.endpoint(&path).expect("callback endpoint").to_string();

    let tenant = tenant_load::tenant_names("pgbbranch", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("every tenant is provisioned");
    let snapshot = format!("urn:ngsi-ld:Snapshot:dormant-retry-{tenant}");
    published_branch(&client, &broker, &tenant, &snapshot).await;
    let subscribed = scoped(&client, Method::POST, format!("{broker}subscriptions"), &tenant, &snapshot)
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "BranchDevice"}],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("create Snapshot-scoped subscription");
    assert_eq!(subscribed.status(), StatusCode::CREATED, "the branch subscription is created");

    // A change on the branch: its first delivery attempt gets a 500.
    let created = scoped(&client, Method::POST, format!("{broker}entities"), &tenant, &snapshot)
        .json(&json!({
            "id": format!("urn:ngsi-ld:BranchDevice:{tenant}"),
            "type": "BranchDevice",
            "value": {"type": "Property", "value": 1},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("create branch entity");
    assert_eq!(created.status(), StatusCode::CREATED, "the branch entity is created");

    let first = first_attempt(&server).await;
    let dormancy = evict_until_dormant(&client, &broker, &tenant, &server).await;
    let after = attempts_after_dormancy(&server, first, &dormancy).await;
    server.shutdown().await.expect("shutdown");
    assert_retried_after_dormancy("a failed Snapshot-branch delivery", &dormancy, after);
    reset(&client, &pool).await;
    pool.close().await;
}
