//! Positive registration: defaults, id generation, stored optional fields, and
//! the 201/Location output (clauses 5.2.9, 5.9.2). All `T-LOCAL` except the
//! tenant-scoping row.

use crate::{
    oracle::{assert_status, json_body, location},
    reg::PEER_ENDPOINT,
    registration::csr_basic_typed,
    topology::{self, BrokerRole},
};
use andromeda_test_client::tenant_name::TenantName;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A baseline registration whose covered entity type is derived from the
/// per-test topology prefix. The default mode is `inclusive` over `federationOps`,
/// so a bare `Vehicle` type would let this registration match another concurrent
/// test's `Vehicle` writes and discovery on the shared broker; the derived type
/// confines it to this test.
fn typed_basic(topology: &MultiBrokerTopology<BrokerRole>, id: &str) -> Value {
    csr_basic_typed(id, PEER_ENDPOINT, &topology.id("Vehicle"))
}

/// Create the registration and return its retrieved representation.
async fn create_and_retrieve(topology: &MultiBrokerTopology<BrokerRole>, id: &str, body: &Value) -> Value {
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(body)
        .await
        .expect("create registration");
    assert_status(&created, 201);
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(id, None)
        .await
        .expect("retrieve registration");
    assert_status(&retrieved, 200);
    json_body(&retrieved)
}

/// Whether a JSON value (string, or array of strings) contains the token.
fn contains_token(value: &Value, token: &str) -> bool {
    match value {
        Value::String(text) => text == token,
        Value::Array(items) => items.iter().any(|item| item.as_str() == Some(token)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
    }
}

/// An omitted `mode` defaults to `inclusive` (clause 5.2.9). Confirms REQ-REG-20.
#[tokio::test]
async fn reg_mode_omitted_defaults_inclusive() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:mode-default");
    let body = typed_basic(&topology, &id);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["mode"].as_str(), Some("inclusive"), "an omitted mode defaults to inclusive");
    topology.finish().await.expect("topology cleanup");
}

/// Omitted `operations` default to the `federationOps` group (clause 5.2.9).
/// Confirms REQ-REG-24.
#[tokio::test]
async fn reg_operations_omitted_defaults_federationops() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:ops-default");
    let body = typed_basic(&topology, &id);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert!(
        contains_token(&stored["operations"], "federationOps"),
        "omitted operations default to federationOps: {}",
        stored["operations"]
    );
    topology.finish().await.expect("topology cleanup");
}

/// A valid ISO 8601 `refreshRate` duration is accepted and stored verbatim.
/// Confirms REQ-REG-26.
#[tokio::test]
async fn reg_refreshrate_valid_duration_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:refreshrate");
    let mut body = typed_basic(&topology, &id);
    body["refreshRate"] = json!("PT5M");
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["refreshRate"].as_str(), Some("PT5M"), "refreshRate stored verbatim");
    topology.finish().await.expect("topology cleanup");
}

/// A valid `management.cacheDuration` is accepted and stored. Confirms REQ-REG-39.
#[tokio::test]
async fn reg_management_cacheduration_valid_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:cacheduration");
    let mut body = typed_basic(&topology, &id);
    body["management"] = json!({ "cacheDuration": "PT10S" });
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["management"]["cacheDuration"].as_str(), Some("PT10S"), "cacheDuration stored");
    topology.finish().await.expect("topology cleanup");
}

/// `datasetId` accepts both URIs and the `@none` token. Confirms REQ-REG-14.
#[tokio::test]
async fn reg_datasetid_valid_uris_and_none_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:datasetid");
    let mut body = typed_basic(&topology, &id);
    body["datasetId"] = json!(["urn:ngsi-ld:ds:1", "@none"]);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert!(contains_token(&stored["datasetId"], "urn:ngsi-ld:ds:1"), "explicit datasetId stored");
    assert!(contains_token(&stored["datasetId"], "@none"), "@none datasetId preserved");
    topology.finish().await.expect("topology cleanup");
}

/// `contextSourceInfo` round-trips. Confirms REQ-REG-11.
#[tokio::test]
async fn reg_contextsourceinfo_stored() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:csi");
    let mut body = typed_basic(&topology, &id);
    body["contextSourceInfo"] = json!([ { "key": "accept", "value": "application/ld+json" } ]);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    let info = stored["contextSourceInfo"]
        .as_array()
        .unwrap_or_else(|| panic!("contextSourceInfo round-trips"));
    assert!(
        info.iter()
            .any(|entry| entry["key"] == json!("accept") && entry["value"] == json!("application/ld+json")),
        "contextSourceInfo entry round-trips: {info:?}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A non-empty `contextSourceAlias` is stored. Confirms REQ-REG-12.
#[tokio::test]
async fn reg_contextsourcealias_nonempty_stored() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:alias");
    let mut body = typed_basic(&topology, &id);
    body["contextSourceAlias"] = json!("broker-alias-x");
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["contextSourceAlias"].as_str(), Some("broker-alias-x"), "alias stored");
    topology.finish().await.expect("topology cleanup");
}

/// The whole `management` object round-trips. Confirms REQ-REG-17, REQ-REG-41.
#[tokio::test]
async fn reg_management_object_stored() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:management");
    let mut body = typed_basic(&topology, &id);
    body["management"] = json!({ "localOnly": true, "timeout": 3000, "cooldown": 500 });
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["management"]["localOnly"], json!(true), "localOnly stored");
    assert_eq!(stored["management"]["timeout"], json!(3000), "timeout stored");
    assert_eq!(stored["management"]["cooldown"], json!(500), "cooldown stored");
    topology.finish().await.expect("topology cleanup");
}

/// `scope` round-trips. Confirms REQ-REG-28.
#[tokio::test]
async fn reg_scope_stored() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:scope");
    let mut body = typed_basic(&topology, &id);
    body["scope"] = json!(["/Madrid/Gardens", "/Madrid/Parks"]);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert!(contains_token(&stored["scope"], "/Madrid/Gardens"), "first scope stored");
    assert!(contains_token(&stored["scope"], "/Madrid/Parks"), "second scope stored");
    topology.finish().await.expect("topology cleanup");
}

/// A `managementInterval` with only `startAt` is accepted as open-ended.
/// Confirms REQ-REG-38.
#[tokio::test]
async fn reg_timeinterval_startat_only_open_ended_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:startat-only");
    let mut body = typed_basic(&topology, &id);
    body["managementInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z" });
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);
    topology.finish().await.expect("topology cleanup");
}

/// A registration with no `id` is assigned a generated URI, returned in
/// `Location` and retrievable there. Confirms REQ-REG-6, REQ-REG-63.
#[tokio::test]
async fn reg_missing_id_generates_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let placeholder = topology.id("registration:generated");
    let mut body = typed_basic(&topology, &placeholder);
    body.as_object_mut().unwrap_or_else(|| panic!("baseline object")).remove("id");

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);
    let location = location(&created);
    let generated = location.rsplit('/').next().unwrap_or_else(|| panic!("Location has a final segment"));
    assert!(generated.starts_with("urn:"), "generated registration id is a URI: {generated}");

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(generated, None)
        .await
        .expect("retrieve generated id");
    assert_status(&retrieved, 200);
    assert_eq!(json_body(&retrieved)["id"].as_str(), Some(generated), "retrieved id matches the generated URI");

    topology.finish().await.expect("topology cleanup");
}

/// A valid CSR with an explicit id returns 201 and carries that id.
/// Confirms REQ-REG-64.
#[tokio::test]
async fn reg_valid_registration_returns_201_with_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:explicit-id");
    let body = typed_basic(&topology, &id);
    let stored = create_and_retrieve(&topology, &id, &body).await;
    assert_eq!(stored["id"].as_str(), Some(id.as_str()), "explicit id carried");
    topology.finish().await.expect("topology cleanup");
}

/// The 201 `Location` ends with the CSR URI. Confirms REQ-REG-65.
#[tokio::test]
async fn reg_success_returns_location_with_csr_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:location");
    let body = typed_basic(&topology, &id);
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);
    let location = location(&created);
    assert!(
        location.contains("/csourceRegistrations/"),
        "Location targets the registry resource: {location}"
    );
    assert!(location.ends_with(&id), "Location's final segment is the CSR URI: {location}");
    topology.finish().await.expect("topology cleanup");
}

/// A registration is scoped to the tenant that created it: visible under that
/// tenant, absent under the default tenant, and the same id can be created
/// independently under the default tenant (clause 5.2.9, multi-tenancy 4.14).
/// Confirms REQ-REG-29.
#[tokio::test]
async fn reg_tenant_scopes_registration() {
    let topology = topology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantA").expect("valid tenant name");
    let tenant_client = topology.broker(BrokerRole::A).expect("topology broker A").tenant_client(tenant);
    let id = topology.id("registration:tenant-scoped");
    let body = typed_basic(&topology, &id);

    let created = tenant_client.create_registration(&body).await.expect("tenant create");
    assert_status(&created, 201);

    let under_tenant = tenant_client.retrieve_registration(&id, None).await.expect("tenant retrieve");
    assert_status(&under_tenant, 200);

    let under_default = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("default retrieve");
    assert_status(&under_default, 404);

    let created_default = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("default create");
    assert_status(&created_default, 201);

    // Clean the tenant-scoped copy explicitly: prefix cleanup runs on the default tenant only.
    let deleted = tenant_client.delete_registration(&id).await.expect("tenant delete");
    assert_status(&deleted, 204);
    topology.finish().await.expect("topology cleanup");
}
