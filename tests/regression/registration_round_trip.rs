//! A Context Source Registration reads back as it was written, through its typed model, including a `location` geometry and an additional member.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.9.2 (Register Context Source) stores the members of Table 5.2.9-1 and clause 5.9.3 (Retrieve Context Source Registration) serves them back. Clause 5.2.2 makes the members a Context Producer supplies, additional ones included, members the broker stores and serves, so a POST followed by a GET answers the same members with the same values, apart from the read-only ones of Table 5.2.9-2 and the timestamps the broker generates.
//!
//! Needs the host broker.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::round_trip_comparison::assert_round_trips;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Clauses 5.9.2 and 5.9.3: a registration is served back with every member the
/// client supplied.
#[tokio::test]
async fn regression_registration_round_trips_through_its_typed_model() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ContextSourceRegistration:typed-round-trip");
    // Clause 4.3.6.3: an exclusive registration defines an entity id and
    // specific attributes; an id pattern or a type alone is not supported.
    let covered = scope.id("Vehicle:typed-round-trip");
    let posted = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "registrationName": "typed round trip",
        "description": "every member the client may supply",
        "endpoint": "http://my.csource.org:1026",
        "mode": "exclusive",
        "operations": ["retrieveEntity", "queryEntity"],
        "information": [{
            "entities": [{"id": covered, "type": scope.entity_type("Vehicle")}],
            "propertyNames": ["speed"]
        }],
        "location": {
            "type": "Polygon",
            "coordinates": [[[100.0, 0.0], [101.0, 0.0], [101.0, 1.0], [100.0, 1.0], [100.0, 0.0]]]
        },
        "expiresAt": "2099-01-01T00:00:00Z",
        "customNote": "an additional member the broker does not know",
        "@context": [TEST_SUITE_CONTEXT]
    });

    let created = client.create_registration(&posted).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let retrieved = client
        .retrieve_registration(&id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve registration request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let returned = retrieved.json_value().expect("a retrieved registration is JSON");

    assert_round_trips(posted, returned, "Context Source Registration");

    client.delete_registration(&id).await.expect("delete registration request");
    scope.finish().await.expect("environment cleanup");
}
