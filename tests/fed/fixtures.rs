//! Shared real-broker fixtures for the federation acceptance suite.

use andromeda_test_client::{
    broker_endpoint::{BrokerDnsOverride, BrokerEndpoint},
    header::jsonld_link_header,
    response::TestResponse,
    tenant_name::TenantName,
    test_client::TestClient,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::{
    multi_broker::{
        MultiBrokerTopology,
        broker_handle::BrokerHandle,
        node_spec::{BrokerEnvironmentKey, BrokerNodeDefaults, BrokerNodeSpec},
    },
    readiness::ReadinessPolicy,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};

/// Roles in the five-broker federation topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FedRole {
    /// Federation boundary broker.
    Hub,
    /// Madrid domain broker.
    Madrid,
    /// Berlin domain broker.
    Berlin,
    /// Nordic domain broker.
    Nordic,
    /// North sub-domain broker.
    North,
}

/// Machine-checked federation requirement inventory.
pub const FEDERATION_REQUIREMENTS: &[&str] = &[
    "REQ-FED-ARCH-1",
    "REQ-FED-ARCH-2",
    "REQ-FED-ARCH-3",
    "REQ-FED-ARCH-4",
    "REQ-FED-ARCH-5",
    "REQ-FED-ARCH-6",
    "REQ-FED-ARCH-7",
    "REQ-FED-ARCH-8",
    "REQ-FED-ARCH-9",
    "REQ-FED-ARCH-10",
    "REQ-FED-ARCH-11",
    "REQ-FED-API-1",
    "REQ-FED-API-2",
    "REQ-FED-API-3",
    "REQ-FED-API-4",
];

/// Requirement coverage for every executable federation case.
pub const FEDERATION_CASE_COVERAGE: &[(&str, &[&str])] = &[
    (
        "fed_topology_reports_distinct_identities_and_isolated_stores",
        &["REQ-FED-ARCH-2", "REQ-FED-ARCH-7"],
    ),
    ("fed_registration_lifecycle_controls_domain_routing", &["REQ-FED-API-2", "REQ-FED-API-4"]),
    (
        "fed_coarse_scope_selects_matching_domain",
        &["REQ-FED-ARCH-2", "REQ-FED-ARCH-3", "REQ-FED-ARCH-4", "REQ-FED-ARCH-5"],
    ),
    (
        "fed_geographic_coverage_selects_matching_domain",
        &["REQ-FED-ARCH-3", "REQ-FED-ARCH-4", "REQ-FED-ARCH-5"],
    ),
    (
        "fed_query_merges_multi_domain_entities_and_dataset_instances",
        &[
            "REQ-FED-ARCH-1",
            "REQ-FED-ARCH-5",
            "REQ-FED-ARCH-6",
            "REQ-FED-ARCH-11",
            "REQ-FED-API-1",
            "REQ-FED-API-3",
        ],
    ),
    (
        "fed_query_is_transparent_to_consumer",
        &["REQ-FED-ARCH-1", "REQ-FED-ARCH-6", "REQ-FED-ARCH-11", "REQ-FED-API-1"],
    ),
    ("fed_centralized_domain_serves_local_state", &["REQ-FED-ARCH-2", "REQ-FED-ARCH-7"]),
    (
        "fed_distributed_domain_resolves_downstream_source",
        &["REQ-FED-ARCH-5", "REQ-FED-ARCH-8", "REQ-FED-API-3"],
    ),
    (
        "fed_nested_federation_excludes_back_edge",
        &["REQ-FED-ARCH-6", "REQ-FED-ARCH-9", "REQ-FED-API-3"],
    ),
    ("fed_direct_discovery_contacts_exact_domain_endpoint", &["REQ-FED-ARCH-10", "REQ-FED-API-2"]),
    (
        "fed_single_access_point_combines_local_and_three_domains",
        &["REQ-FED-ARCH-1", "REQ-FED-ARCH-4", "REQ-FED-ARCH-5", "REQ-FED-ARCH-6", "REQ-FED-ARCH-11"],
    ),
    ("fed_split_entity_filters_after_cross_domain_merge", &["REQ-FED-API-3"]),
    (
        "fed_partial_read_failure_preserves_healthy_domain_results",
        &["REQ-FED-ARCH-5", "REQ-FED-ARCH-6", "REQ-FED-API-3"],
    ),
    ("fed_entity_map_stabilizes_cross_domain_pagination", &["REQ-FED-API-3"]),
    (
        "fed_available_types_and_attributes_union_domain_metadata",
        &["REQ-FED-ARCH-4", "REQ-FED-ARCH-6", "REQ-FED-ARCH-11"],
    ),
    ("fed_redirected_entity_crud_lifecycle_reaches_domain", &["REQ-FED-API-1", "REQ-FED-API-4"]),
    ("fed_batch_provision_routes_entities_to_three_domains", &["REQ-FED-ARCH-4", "REQ-FED-ARCH-5"]),
    ("fed_batch_write_reports_partial_domain_failure", &["REQ-FED-API-3"]),
    (
        "fed_subscription_two_domain_lifecycle_delivers_original_id",
        &[
            "REQ-FED-ARCH-1",
            "REQ-FED-ARCH-4",
            "REQ-FED-ARCH-5",
            "REQ-FED-ARCH-6",
            "REQ-FED-ARCH-11",
            "REQ-FED-API-1",
            "REQ-FED-API-3",
            "REQ-FED-API-4",
        ],
    ),
    (
        "fed_subscription_discovers_domain_registered_after_creation",
        &["REQ-FED-ARCH-4", "REQ-FED-ARCH-5"],
    ),
    (
        "fed_nested_subscription_delivers_from_subdomain_and_cleans_up",
        &["REQ-FED-ARCH-8", "REQ-FED-ARCH-9", "REQ-FED-API-3"],
    ),
    (
        "fed_temporal_query_selects_domains_by_operation_and_interval",
        &[
            "REQ-FED-ARCH-3",
            "REQ-FED-ARCH-5",
            "REQ-FED-ARCH-6",
            "REQ-FED-ARCH-11",
            "REQ-FED-API-1",
            "REQ-FED-API-3",
            "REQ-FED-API-4",
        ],
    ),
    ("fed_temporal_fragments_merge_and_apply_last_n", &["REQ-FED-ARCH-6"]),
    (
        "fed_nested_temporal_query_reaches_subdomain_without_backtracking",
        &["REQ-FED-ARCH-8", "REQ-FED-ARCH-9", "REQ-FED-API-3"],
    ),
    ("fed_tenant_isolation_preserves_domain_boundaries", &["REQ-FED-API-3"]),
    (
        "fed_stress_current_query_4000_entities_64_clients",
        &["REQ-FED-ARCH-1", "REQ-FED-ARCH-5", "REQ-FED-ARCH-6", "REQ-FED-API-3"],
    ),
    (
        "fed_stress_temporal_20000_observations_64_clients",
        &["REQ-FED-ARCH-5", "REQ-FED-ARCH-6", "REQ-FED-API-3"],
    ),
    (
        "fed_stress_64_cross_domain_subscriptions",
        &[
            "REQ-FED-ARCH-1",
            "REQ-FED-ARCH-4",
            "REQ-FED-ARCH-5",
            "REQ-FED-ARCH-6",
            "REQ-FED-ARCH-11",
            "REQ-FED-API-3",
            "REQ-FED-API-4",
        ],
    ),
];

/// Assert that the executable case catalog covers every federation requirement.
pub fn assert_complete_catalog() {
    for requirement in FEDERATION_REQUIREMENTS {
        assert!(
            FEDERATION_CASE_COVERAGE.iter().any(|(_, covered)| covered.contains(requirement)),
            "{requirement} has no federation case"
        );
    }
    assert_eq!(FEDERATION_CASE_COVERAGE.len(), 28);
}

/// Build the isolated, five-broker federation topology.
pub async fn topology() -> MultiBrokerTopology<FedRole> {
    let specs = [
        (
            FedRole::Hub,
            "HUB",
            "http://127.0.0.1:1071/ngsi-ld/v1/",
            "http://fed-hub:1071",
            "postgres://postgres:postgres@127.0.0.1:5451/fed_hub",
            "fed-hub",
            1071,
        ),
        (
            FedRole::Madrid,
            "MADRID",
            "http://127.0.0.1:1072/ngsi-ld/v1/",
            "http://fed-domain-madrid:1072",
            "postgres://postgres:postgres@127.0.0.1:5452/fed_madrid",
            "fed-domain-madrid",
            1072,
        ),
        (
            FedRole::Berlin,
            "BERLIN",
            "http://127.0.0.1:1073/ngsi-ld/v1/",
            "http://fed-domain-berlin:1073",
            "postgres://postgres:postgres@127.0.0.1:5453/fed_berlin",
            "fed-domain-berlin",
            1073,
        ),
        (
            FedRole::Nordic,
            "NORDIC",
            "http://127.0.0.1:1074/ngsi-ld/v1/",
            "http://fed-domain-nordic:1074",
            "postgres://postgres:postgres@127.0.0.1:5454/fed_nordic",
            "fed-domain-nordic",
            1074,
        ),
        (
            FedRole::North,
            "NORTH",
            "http://127.0.0.1:1075/ngsi-ld/v1/",
            "http://fed-domain-north:1075",
            "postgres://postgres:postgres@127.0.0.1:5455/fed_north",
            "fed-domain-north",
            1075,
        ),
    ];
    let specs = specs
        .into_iter()
        .map(|(role, token, client, peer, database, alias, port)| {
            BrokerNodeSpec::from_environment(
                role,
                role,
                BrokerEnvironmentKey::new("FED", token),
                BrokerNodeDefaults::new(client, peer, database, alias),
                Some(port),
            )
            .expect("federation broker specification is valid")
        })
        .collect();
    let context = jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("compound context is a valid link");
    MultiBrokerTopology::from_specs(
        "fed",
        specs,
        Some(context),
        ReadinessPolicy::new(Duration::from_secs(90), Duration::from_millis(250), Duration::from_secs(2)),
    )
    .await
    .expect("federation stack is healthy")
}

/// Create a simple entity with one scalar property.
pub fn entity(id: &str, entity_type: &str, value: i64) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": {"type": "Property", "value": value},
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Create an entity carrying one hierarchical scope.
pub fn entity_with_scope(id: &str, entity_type: &str, value: i64, scope: &str) -> Value {
    let mut value = entity(id, entity_type, value);
    value["scope"] = json!([scope]);
    value
}

/// Create a registration for one domain footprint, held by the domain broker
/// under `tenant`.
///
/// Clause 4.14: "When contacting the respective Context Sources, the Tenant
/// information from the Context Source Registration has to be used. If no
/// Tenant information is present in the Context Source Registration, no Tenant
/// information is to be used and thus the default Tenant is targeted". Every
/// federation registration points at a broker of the topology, so it names the
/// tenant the test's data lives in there.
pub fn registration(tenant: &str, id: &str, endpoint: &str, entity_type: &str, scope: Option<&str>, mode: &str, operations: Option<&[&str]>) -> Value {
    let mut body = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "tenant": tenant,
        "information": [{"entities": [{"type": entity_type}]}],
        "mode": mode,
        "@context": COMPOUND_CONTEXT.as_str()
    });
    if let Some(scope) = scope {
        body["scope"] = json!([scope]);
    }
    if let Some(operations) = operations {
        body["operations"] = json!(operations);
    }
    body
}

/// Create a registration constrained to one exact entity id.
pub fn registration_for_entity(tenant: &str, id: &str, endpoint: &str, entity_id: &str, entity_type: &str, mode: &str, operations: Option<&[&str]>) -> Value {
    let mut body = registration(tenant, id, endpoint, entity_type, None, mode, operations);
    body["information"][0]["entities"][0]["id"] = json!(entity_id);
    body
}

/// Build a direct client from an unchanged discovered endpoint, running under
/// the tenant the discovery ran under (clause 4.3.6.4: each tenant is federated
/// separately, so the discovered source holds the data under that tenant).
pub fn discovered_client(endpoint: &str, port: u16, tenant: Option<&TenantName>) -> TestClient {
    let endpoint = BrokerEndpoint::parse(endpoint).expect("discovered endpoint is a valid broker endpoint");
    let host = endpoint.url().host_str().expect("discovered endpoint has a hostname");
    let dns = BrokerDnsOverride::new(host, SocketAddr::from(([127, 0, 0, 1], port)));
    TestClient::new_with_dns_overrides(endpoint, tenant.cloned(), &[dns]) // clone: the client owns the tenant it runs under
        .expect("DNS override client builds")
        .with_default_context(jsonld_link_header(COMPOUND_CONTEXT.as_str()).expect("compound context link"))
}

/// Return a broker handle or fail the test with a useful role name.
pub fn broker(topology: &MultiBrokerTopology<FedRole>, role: FedRole) -> &BrokerHandle<FedRole> {
    topology.broker(role).expect("topology broker for role")
}

/// Assert an HTTP status code and include the response body on failure.
pub fn assert_status(response: &TestResponse, status: u16) {
    assert_eq!(response.status.as_u16(), status, "unexpected response body: {:?}", response.body);
}

/// Decode a response body as JSON for structural assertions.
pub fn body(response: &TestResponse) -> Value {
    response.json_value().expect("response body is JSON")
}
