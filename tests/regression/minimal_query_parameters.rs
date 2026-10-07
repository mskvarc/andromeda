//! The "too wide query" rule, and the inputs that narrow one.
//!
//! ETSI GS CIM 009 v1.9.1 states the rule four times in the same words. Clauses 5.6.21.4 (Purge Entities), 5.7.2.4 (Query Entities), 5.7.4.4 (Query Temporal Evolution of Entities) and 5.14.4.4 (Create EntityMap): "At least one of the following input data shall be provided: a) selector of Entity Types; b) list of Attribute names, including at least one non-system Attribute; c) NGSI-LD Query, including at least one non-system Attribute; d) NGSI-LD `GeoQuery`; e) local scope (see clause 5.5.13). If none of the above is provided, then an error of type `BadRequestData` shall be raised (too wide query)."
//!
//! What the list leaves out decides as much as what it holds, and the prose above each behaviour clause says why: "it is not possible to retrieve a set of entities by only specifying desired Entity identifiers, without further specifying restrictions on the entities' types or attributes". `id`, `idPattern` and `scopeQ` are therefore not narrowings.
//!
//! Clause 5.10.2.4 states the same rule for Query Context Source Registrations with two differences: it lists only the four content narrowings, because a registration discovery has no execution to limit to the local scope, and it drops the "non-system Attribute" qualification.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.7.2.4 names for a query that is too wide.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The temporal window clause 5.7.4.4 requires beside the narrowing under test.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The four content narrowings of clause 5.7.2.4 (a) to (d), each alone.
const CONTENT_NARROWINGS: [&str; 4] = [
    "type=Vehicle",
    "attrs=speed",
    "q=speed%3E10",
    "georel=near%3BmaxDistance%3D%3D100&geometry=Point&coordinates=%5B1,2%5D",
];

/// The parameters the clause does not list, each alone and in combination.
const NON_NARROWINGS: [&str; 4] = [
    "id=urn:ngsi-ld:Vehicle:too-wide",
    "idPattern=urn:ngsi-ld:Vehicle:.%2A",
    "scopeQ=%2FMadrid",
    "id=urn:ngsi-ld:Vehicle:too-wide&idPattern=urn:.%2A",
];

/// One Vehicle, so a narrowed query has something to answer with.
fn vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    })
}

/// Create the entity the queries address.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one read against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("read request")
}

/// Assert a query ran.
fn assert_ran(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::OK, "{scenario}: {:?}", response.body);
}

/// Assert a query was refused as too wide.
fn assert_too_wide(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    assert!(problem["title"].is_string(), "{scenario}: a problem carries a title: {problem}");
}

/// Clause 5.7.2.4 (a) to (e): each listed input narrows a query on its own, and `local=true` is one of them.
#[tokio::test]
async fn regression_too_wide_query_each_listed_input_narrows_an_entity_query() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    create_entity(client, &vehicle(&scope.id("Vehicle:narrowed"))).await;

    for narrowing in CONTENT_NARROWINGS {
        assert_ran(&read(client, &format!("entities?{narrowing}")).await, narrowing);
    }
    assert_ran(&read(client, "entities?local=true").await, "local=true");
    // Table 5.2.33-1 makes `type=*` imply local scope, so it narrows too.
    assert_ran(&read(client, "entities?type=%2A").await, "type=*");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4 lists no identifier among the narrowings, and clause 5.7.2.3 says why: identifiers alone do not restrict "the entities' types or attributes". `scopeQ` is not listed either.
#[tokio::test]
async fn regression_too_wide_query_identifiers_and_scope_do_not_narrow_an_entity_query() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for parameters in NON_NARROWINGS {
        assert_too_wide(&read(client, &format!("entities?{parameters}")).await, parameters);
    }
    assert_too_wide(&read(client, "entities").await, "no parameter at all");
    // A listed narrowing beside them settles it the other way.
    assert_ran(&read(client, "entities?id=urn:ngsi-ld:Vehicle:x&type=Vehicle").await, "id with a type selector");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 restates the rule for the temporal query, local-scope exemption included, so a query limited to this broker is admitted.
#[tokio::test]
async fn regression_too_wide_query_the_temporal_query_carries_the_same_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    create_entity(client, &vehicle(&scope.id("Vehicle:temporal-narrowed"))).await;

    for narrowing in CONTENT_NARROWINGS {
        assert_ran(&read(client, &format!("temporal/entities?{WINDOW}&{narrowing}")).await, narrowing);
    }
    assert_ran(&read(client, &format!("temporal/entities?{WINDOW}&local=true")).await, "local=true");

    for parameters in NON_NARROWINGS {
        assert_too_wide(&read(client, &format!("temporal/entities?{WINDOW}&{parameters}")).await, parameters);
    }
    assert_too_wide(&read(client, &format!("temporal/entities?{WINDOW}")).await, "the window alone");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.10.2.4 lists only the four content narrowings for a registration discovery: it has no execution to limit, so `local` narrows nothing there.
#[tokio::test]
async fn regression_too_wide_query_registration_discovery_has_no_local_scope_exemption() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for narrowing in CONTENT_NARROWINGS {
        assert_ran(&read(client, &format!("csourceRegistrations?{narrowing}")).await, narrowing);
    }
    for parameters in NON_NARROWINGS {
        assert_too_wide(&read(client, &format!("csourceRegistrations?{parameters}")).await, parameters);
    }
    assert_too_wide(&read(client, "csourceRegistrations").await, "no parameter at all");

    scope.finish().await.expect("cleanup failed");
}

/// Inputs (b) and (c) are qualified with "including at least one non-system Attribute" wherever an entity is being selected, so an `attrs` or `q` that names only the clause 4.8 temporal properties narrows nothing. Clause 5.10.2.4 carries no such qualification.
#[tokio::test]
async fn regression_too_wide_query_a_system_attribute_narrows_only_a_registration_discovery() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for parameters in ["attrs=createdAt", "attrs=modifiedAt,deletedAt", "q=createdAt%3E2020-01-01T00:00:00Z"] {
        assert_too_wide(&read(client, &format!("temporal/entities?{WINDOW}&{parameters}")).await, parameters);
        assert_ran(&read(client, &format!("csourceRegistrations?{parameters}")).await, parameters);
    }
    // One ordinary Attribute beside them narrows the query after all.
    assert_ran(
        &read(client, &format!("temporal/entities?{WINDOW}&attrs=createdAt,speed")).await,
        "a system attribute beside speed",
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.21.3 gives Purge Entities no `attrs`: its lists of Attribute names are `drop` and `keep`, so those are what input (b) names there.
#[tokio::test]
async fn regression_too_wide_query_a_purge_narrows_by_the_attributes_it_names() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:purge-narrowed");
    create_entity(client, &vehicle(&id)).await;

    let purged = client
        .raw(Method::DELETE, &format!("entities?id={id}&keep=speed"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("purge request");
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "a purge naming attributes to keep: {:?}", purged.body);

    let too_wide = client
        .raw(Method::DELETE, &format!("entities?id={id}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("purge request");
    assert_too_wide(&too_wide, "a purge naming only an identifier");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.14.4.4 restates the clause 5.7.2.4 rule for Entity Map creation: local scope narrows, and identifiers do not.
#[tokio::test]
async fn regression_too_wide_query_entity_map_creation_carries_the_same_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    create_entity(client, &vehicle(&scope.id("Vehicle:map-narrowed"))).await;

    for narrowing in CONTENT_NARROWINGS {
        let created = read(client, &format!("entityMaps?{narrowing}")).await;
        assert_eq!(created.status, StatusCode::CREATED, "{narrowing}: {:?}", created.body);
    }
    let local = read(client, "entityMaps?local=true").await;
    assert_eq!(local.status, StatusCode::CREATED, "the local scope admits a creation: {:?}", local.body);
    for parameters in NON_NARROWINGS {
        assert_too_wide(&read(client, &format!("entityMaps?{parameters}")).await, parameters);
    }

    scope.finish().await.expect("cleanup failed");
}
