//! Whether a temporal `EntityMap` creation is held to the "too wide query"
//! narrowing rule the current-entity `EntityMap` creation enforces.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.14.5.4 creates the map from a Query
//! Temporal Evolution request and so inherits that operation's parameter
//! validation, which clause 5.7.4.4 states as the clause 5.7.2.4 rule verbatim:
//! a query is too wide, and rejected with `BadRequestData`, unless it names a
//! type, an identifier, an identifier pattern, an attribute, a `q`, a geoquery
//! or the local scope. The temporal map validator applies this rule as well as
//! the join and Linked-Entity constraints, so a temporal map creation whose only
//! input is the temporal window is refused, as a temporal query is.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

/// The endpoint that creates an `EntityMap` over a temporal query (clause 5.14.5).
const TEMPORAL_ENTITY_MAPS: &str = "/ngsi-ld/v1/temporal/entityMaps";
/// A temporal window with no other narrowing input of its own.
const WINDOW: &str = "timerel=before&timeAt=2099-01-01T00:00:00Z";

/// Attempt a temporal `EntityMap` creation over `query`.
async fn create_temporal_map(client: &TestClient, query: &str) -> TestResponse {
    client
        .raw(Method::GET, &format!("{TEMPORAL_ENTITY_MAPS}?{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("temporal EntityMap creation request failed")
}

/// Clause 5.14.5.4 with clause 5.7.4.4: a temporal map creation whose only
/// input is the temporal window is too wide and refused with `BadRequestData`.
#[tokio::test]
async fn regression_temporal_entity_map_a_creation_without_narrowing_is_a_bad_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let refused = create_temporal_map(client, WINDOW).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "too-wide temporal map creation: {:?}", refused.body);
    let problem = refused.json_value().expect("a refusal answers a ProblemDetails body");
    assert_eq!(
        problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "the refusal is of type BadRequestData: {problem}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The same creation, once a type narrows it, is accepted: the narrowing rule
/// is satisfied and the map is minted (clause 5.14.5.4).
#[tokio::test]
async fn regression_temporal_entity_map_a_narrowed_creation_is_accepted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let created = create_temporal_map(client, &format!("type=Building&{WINDOW}")).await;
    assert_eq!(created.status, StatusCode::CREATED, "narrowed temporal map creation: {:?}", created.body);
    let map_id = created.json_value().expect("EntityMap body must be JSON")["id"]
        .as_str()
        .expect("the created EntityMap must carry an id")
        .to_owned();

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}
