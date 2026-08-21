//! Cascade limiting, `Via` propagation, loop prevention, and per-tenant
//! separation of federated requests (ETSI GS CIM 009 v1.9.1, clauses 4.3.6.4,
//! 6.3.18). Header capture on `T-MOCK`; real cascades on `T-PAIR`; loops on
//! `T-LOOP`; tenancy on `T-TENANT`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, forwarded_header, has_warning, via_pseudonyms},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use andromeda_test_client::tenant_name::TenantName;
use axum::http::Method;
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// The request a broker forwards to a registered endpoint signals cascade
/// limiting: its `Via` chain carries the forwarding broker's own alias (clause
/// 6.3.18). Confirms REQ-FW-30.
#[tokio::test]
async fn distop_entity_via_local_scope_signaled_to_registered_endpoint() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:via-signal"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("create registration"),
        201,
    );

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let pseudonyms = via_pseudonyms(&forwarded);
    assert!(
        pseudonyms.iter().any(|alias| alias == topology.front().alias()),
        "the forwarded Via carries the broker's alias: {pseudonyms:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `Via` header of a forwarded request carries the encountered source
/// aliases, letting a downstream source detect a loop (clause 6.3.18). Confirms
/// REQ-FW-31.
#[tokio::test]
async fn distop_entity_via_header_carries_encountered_source_aliases() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:via-carries"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .alias("cs-broker-A")
        .build();
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("create registration"),
        201,
    );

    let response = topology
        .front()
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let pseudonyms = via_pseudonyms(&forwarded);
    assert!(
        pseudonyms.iter().any(|alias| alias == topology.front().alias()),
        "the forwarded Via lists the forwarding broker's alias: {pseudonyms:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A query whose incoming `Via` already lists a source's alias is not forwarded
/// to that source: the registration is skipped to prevent a loop (clause
/// 6.3.18). Confirms REQ-FW-31.
#[tokio::test]
async fn distop_entity_via_csr_alias_in_header_skips_csr() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:via-skip"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .alias("cs-broker-A")
        .build();
    assert_status(
        &topology.front().client().create_registration(&registration).await.expect("create registration"),
        201,
    );

    let response = topology
        .front()
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("Via", "1.1 cs-broker-A")
        .send()
        .await
        .expect("query with Via");
    assert_status(&response, 200);
    assert_eq!(mock.get_total_count().await, 0, "a source already listed in the incoming Via is skipped");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A two-hop cascade appends each broker's alias to the `Via` chain, so the
/// final source sees both aliases and can exclude an already-listed hop (clause
/// 6.3.18). Confirms REQ-FW-31, REQ-FW-30.
#[tokio::test]
async fn distop_entity_via_chain_appends_alias_and_excludes_loop() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let middle = topology.broker(BrokerRole::B);
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;

    let endpoint = mock.url("");
    let middle_registration = CsrBuilder::new(&topology.id("registration:chain-middle"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &middle.client().create_registration(&middle_registration).await.expect("middle registration"),
        201,
    );

    let front_registration = CsrBuilder::new(&topology.id("registration:chain-front"), middle.peer_url())
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&front_registration).await.expect("front registration"), 201);

    let response = front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let pseudonyms = via_pseudonyms(&forwarded);
    assert!(
        pseudonyms.iter().any(|alias| alias == front.alias()),
        "the front broker's alias is in the chain: {pseudonyms:?}"
    );
    assert!(
        pseudonyms.iter().any(|alias| alias == middle.alias()),
        "the middle broker's alias is appended: {pseudonyms:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A single exclusive registration on each of two brokers pointing at the other
/// for the same id forms a loop. For a distributed GET clause 6.3.17 reports the
/// loop as `NGSILD-Warning` 199 rather than 508, which is reserved for the HTTP
/// unsafe methods. Confirms REQ-FW-31.
#[tokio::test]
async fn distop_entity_loop_single_exclusive_returns_199_warning() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);
    let entity_id = topology.id("Vehicle:loop-508");

    let front_registration = CsrBuilder::new(&topology.id("registration:loop-front"), peer.peer_url())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&front_registration).await.expect("front registration"), 201);

    let peer_registration = CsrBuilder::new(&topology.id("registration:loop-peer"), front.peer_url())
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(&peer.client().create_registration(&peer_registration).await.expect("peer registration"), 201);

    let retrieved = front.client().get_entity(&entity_id, "", "application/ld+json", None).await.expect("retrieve");
    assert!(
        retrieved.status.as_u16() < 500,
        "a GET loop is a warning, not a 5xx: got {}",
        retrieved.status.as_u16()
    );
    assert!(has_warning(&retrieved, 199), "the loop is reported as NGSILD-Warning 199");

    topology.finish().await.expect("topology cleanup");
}

/// A loop among multiple inclusive sources does not fail the request: it returns
/// 200 with a 199 warning rather than 508 (clause 6.3.18). Confirms REQ-FW-31.
#[tokio::test]
async fn distop_entity_loop_multi_source_returns_199_warning() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);

    assert_status(
        &front
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:loop-local-a"), 10))
            .with_ld_json()
            .send()
            .await
            .expect("front entity"),
        201,
    );
    assert_status(
        &peer
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:loop-local-b"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("peer entity"),
        201,
    );

    let front_registration = CsrBuilder::new(&topology.id("registration:loop-multi-front"), peer.peer_url())
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(&front.client().create_registration(&front_registration).await.expect("front registration"), 201);

    let peer_registration = CsrBuilder::new(&topology.id("registration:loop-multi-peer"), front.peer_url())
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(&peer.client().create_registration(&peer_registration).await.expect("peer registration"), 201);

    let response = front.client().query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);
    assert!(has_warning(&response, 199), "a multi-source loop yields a 199 warning, not a 508");

    topology.finish().await.expect("topology cleanup");
}

/// Each tenant is federated separately: a query under a tenant forwards with
/// that tenant's `NGSILD-Tenant` header, matching per (source, tenant) (clause
/// 4.3.6). Confirms REQ-FW-32.
#[tokio::test]
async fn distop_entity_via_multi_tenant_each_tenant_considered_separately() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &serde_json::json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:tenant-federation"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();

    let first_tenant = TenantName::parse("distopTenantAlpha").expect("valid tenant name");
    let second_tenant = TenantName::parse("distopTenantBeta").expect("valid tenant name");
    let first_client = topology.front().tenant_client(first_tenant);
    let second_client = topology.front().tenant_client(second_tenant);

    assert_status(&first_client.create_registration(&registration).await.expect("first tenant registration"), 201);
    assert_status(
        &second_client.create_registration(&registration).await.expect("second tenant registration"),
        201,
    );

    let first_query = first_client
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("first tenant query");
    assert_status(&first_query, 200);
    let first_forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded_header(&first_forwarded, "ngsild-tenant").as_deref(),
        Some("distopTenantAlpha"),
        "the first tenant's query carries its own NGSILD-Tenant"
    );

    let second_query = second_client
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("second tenant query");
    assert_status(&second_query, 200);
    let second_forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded_header(&second_forwarded, "ngsild-tenant").as_deref(),
        Some("distopTenantBeta"),
        "the second tenant's query carries its own NGSILD-Tenant"
    );

    // Tenant-scoped registrations are reclaimed explicitly: prefix cleanup runs on the default tenant only.
    let first_id = topology.id("registration:tenant-federation");
    assert_status(&first_client.delete_registration(&first_id).await.expect("first tenant cleanup"), 204);
    assert_status(&second_client.delete_registration(&first_id).await.expect("second tenant cleanup"), 204);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
