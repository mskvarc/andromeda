//! `contextSourceAlias` identifies a Tenant, not just a Context Source.
//!
//! Table 5.2.40-1 makes the alias "a unique id for a Context Source which can
//! be used to identify loops" and adds that "in the multi-tenancy use case (see
//! clause 4.14), this id shall be identifying a specific Tenant within a
//! registered Context Source". Clause 4.3.6.4 states the consequence for
//! distributed operations: "in the case of multi-tenancy (see clause 4.14) each
//! Tenant found within each registered Context Source shall be considered
//! separately."
//!
//! One alias served to every tenant would leave two tenants of one broker
//! indistinguishable in a `Via` chain, and clause 5.12 would then drop from the
//! match set every registration naming either of them as soon as a request had
//! traversed one: "no registration shall match if the
//! `CSourceRegistration` `contextSourceAlias` can be found within the listing of
//! previously encountered Context Sources".

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{tenant_name::TenantName, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::TestRunConfiguration, environment::TestEnvironment};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// A client addressing `tenant` on the broker under test.
///
/// `GET /info/sourceIdentity` resolves no tenant state, so naming a tenant here
/// provisions nothing and leaves nothing to clean up.
fn tenant_client(tenant: &str) -> TestClient {
    let configuration = TestRunConfiguration::from_environment().expect("test run configuration");
    TestClient::new(
        configuration.broker().clone(),
        Some(TenantName::parse(tenant).expect("a valid tenant header value")),
    )
}

/// The `contextSourceAlias` `tenant` observes, and the whole identity document.
async fn identity_of(tenant: Option<&str>) -> (String, Value) {
    let response = if let Some(tenant) = tenant {
        tenant_client(tenant).source_identity(None).await
    } else {
        let configuration = TestRunConfiguration::from_environment().expect("test run configuration");
        TestClient::new(configuration.broker().clone(), None).source_identity(None).await
    }
    .expect("source identity request failed");
    assert_eq!(response.status, StatusCode::OK, "source identity: {:?}", response.body);
    let body = response.json_value().expect("source identity body must be valid JSON");
    assert!(
        body["contextSourceAlias"].is_string(),
        "Table 5.2.40-1 makes contextSourceAlias a mandatory string: {body}"
    );
    let alias = body["contextSourceAlias"].as_str().expect("asserted to be a string above").to_owned();
    (alias, body)
}

/// A registration naming `alias` as the alias of the context source it points at.
fn registration_naming(id: &str, alias: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://context-source.invalid:1026/",
        "contextSourceAlias": alias,
        "@context": [CORE_CONTEXT]
    })
}

/// The registration identifiers a discovery answered with, under the `Via`
/// chain naming the context sources the request has already traversed.
async fn discovered_with_via(client: &TestClient, via: Option<&str>) -> Vec<String> {
    let mut request = client.raw(Method::GET, "csourceRegistrations?type=Vehicle&limit=1000");
    if let Some(via) = via {
        request = request.header("Via", via);
    }
    let response = request.send().await.expect("registration discovery failed");
    assert_eq!(response.status, StatusCode::OK, "discovery: {:?}", response.body);
    response
        .json_value()
        .expect("discovery body must be valid JSON")
        .as_array()
        .expect("a registration discovery answers with an array")
        .iter()
        .filter_map(|registration| registration["id"].as_str().map(str::to_owned))
        .collect()
}

/// Table 5.2.40-1: the alias "shall be identifying a specific Tenant within a
/// registered Context Source", so two tenants of one broker never observe the
/// same one.
#[tokio::test]
async fn regression_source_identity_gives_each_tenant_a_distinct_alias() {
    let (default_alias, _) = identity_of(None).await;
    let (alpha_alias, _) = identity_of(Some("regressionaliasalpha")).await;
    let (beta_alias, _) = identity_of(Some("regressionaliasbeta")).await;

    assert_ne!(default_alias, alpha_alias, "a named tenant must not share the default tenant's alias");
    assert_ne!(default_alias, beta_alias, "a named tenant must not share the default tenant's alias");
    assert_ne!(alpha_alias, beta_alias, "two named tenants must not share an alias");
    for alias in [&default_alias, &alpha_alias, &beta_alias] {
        assert!(!alias.is_empty(), "Table 5.2.40-1 restricts the alias to a non-empty string");
        assert!(
            !alias.chars().any(|character| character.is_whitespace() || character.is_control()),
            "the alias must stay a single RFC 7230 pseudonym token: {alias}"
        );
    }
}

/// The default tenant's alias is the operator's configured value, unchanged.
/// Peer brokers already carry it in their registrations, and clause 4.14 makes
/// the default tenant one that "always exists" rather than a new one, so
/// re-spelling its alias would invalidate every registration naming this broker.
#[tokio::test]
async fn regression_the_default_tenant_alias_is_the_configured_value() {
    let (default_alias, _) = identity_of(None).await;
    let (alpha_alias, _) = identity_of(Some("regressionaliasalpha")).await;

    assert!(
        alpha_alias.starts_with(&default_alias),
        "a tenant alias is derived from the configured one, which stays the default tenant's: {default_alias} / {alpha_alias}"
    );
    assert_eq!(
        alpha_alias.len(),
        default_alias.len() + "~regressionaliasalpha".len(),
        "the derivation appends exactly the separator and the tenant: {alpha_alias}"
    );
}

/// Clause 5.2.40 reads `id` and `contextSourceAlias` together: the type
/// "represents the data uniquely identifying a Context Source, and if the
/// Context Source supports multi-tenancy (see clause 4.14) uniquely identifying
/// a Tenant within that Context Source". Table 5.2.40-1 makes only the alias
/// tenant-specific, so the identifier names the context source for every tenant.
#[tokio::test]
async fn regression_the_context_source_identifier_does_not_move_with_the_tenant() {
    let (_, default_body) = identity_of(None).await;
    let (_, alpha_body) = identity_of(Some("regressionaliasalpha")).await;

    assert_eq!(default_body["id"], alpha_body["id"], "the Context Source ID is not tenant-specific");
    assert_eq!(default_body["type"], Value::from("ContextSourceIdentity"));
    assert_eq!(alpha_body["type"], Value::from("ContextSourceIdentity"));
    for member in ["contextSourceUptime", "contextSourceTimeAt"] {
        assert!(alpha_body[member].is_string(), "Table 5.2.40-1 makes {member} mandatory: {alpha_body}");
    }
}

/// Clause 6.3.14: "If the HTTP header `NGSILD-Tenant` is present in the HTTP
/// request, it shall also be present in HTTP response." The identity endpoint
/// reads that header, so it also echoes it.
#[tokio::test]
async fn regression_source_identity_echoes_the_tenant_it_answered_for() {
    let response = tenant_client("regressionaliasalpha")
        .source_identity(None)
        .await
        .expect("source identity request failed");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers.get("ngsild-tenant").and_then(|value| value.to_str().ok()),
        Some("regressionaliasalpha")
    );
}

/// Clause 4.3.6.4: "each Tenant found within each registered Context Source
/// shall be considered separately". A registration naming one tenant of a
/// context source must survive a request that traversed a different tenant of
/// the same context source, and must be excluded only by its own tenant.
///
/// Were every tenant to share one alias, the two `Via` chains below would be the
/// same value and the registration would disappear from both discoveries.
#[tokio::test]
async fn regression_a_hop_through_one_tenant_does_not_exclude_another_tenants_registration() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let (default_alias, _) = identity_of(None).await;
    let (alpha_alias, _) = identity_of(Some("regressionaliasalpha")).await;

    // A registration in the default tenant that points at the `alpha` tenant of
    // this same context source -- the shape clause 4.14's last paragraph
    // describes, "integrating local Tenants in a federated system using a
    // different Tenant".
    let alpha_registration = scope.id("ContextSourceRegistration:alias-alpha");
    let created = client
        .create_registration(&registration_naming(&alpha_registration, &alpha_alias))
        .await
        .expect("create registration failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let untraversed = discovered_with_via(client, None).await;
    assert!(
        untraversed.contains(&alpha_registration),
        "a request that traversed nothing must see the registration: {untraversed:?}"
    );

    let after_default_hop = discovered_with_via(client, Some(&format!("1.1 {default_alias}"))).await;
    assert!(
        after_default_hop.contains(&alpha_registration),
        "clause 4.3.6.4 considers each Tenant separately, so a hop through the default tenant \
         must not exclude a registration naming the `alpha` tenant: {after_default_hop:?}"
    );

    let after_alpha_hop = discovered_with_via(client, Some(&format!("1.1 {alpha_alias}"))).await;
    assert!(
        !after_alpha_hop.contains(&alpha_registration),
        "clause 5.12 excludes a registration whose alias is in the encountered listing: {after_alpha_hop:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The exclusion is exact, not a prefix match: a derived tenant alias shares its
/// leading characters with the configured one, so a `contains`-style comparison
/// would silently drop every tenant of a broker whose default tenant was
/// traversed.
#[tokio::test]
async fn regression_the_encountered_listing_matches_an_alias_whole_rather_than_by_prefix() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let (default_alias, _) = identity_of(None).await;

    let derived = format!("{default_alias}~regressionaliasgamma");
    let registration = scope.id("ContextSourceRegistration:alias-prefix");
    let created = client
        .create_registration(&registration_naming(&registration, &derived))
        .await
        .expect("create registration failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let after_prefix_hop = discovered_with_via(client, Some(&format!("1.1 {default_alias}"))).await;
    assert!(
        after_prefix_hop.contains(&registration),
        "an alias that merely starts with an encountered one is a different context source: {after_prefix_hop:?}"
    );

    let after_exact_hop = discovered_with_via(client, Some(&format!("1.1 {derived}"))).await;
    assert!(
        !after_exact_hop.contains(&registration),
        "the exact alias must still be excluded: {after_exact_hop:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
