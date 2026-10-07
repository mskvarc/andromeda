//! What a Context Source Registration discovery returns of a registration it matched.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.10.2.5 recommends the narrow answer: implementations "should return filtered Context Source Registrations, which only contain context source registration information relevant for the query, in particular only matching `RegistrationInfo` elements". Clause 5.11.7 recommends the same of a Context Source Notification. A discovery therefore answers with only the `RegistrationInfo` elements the query matched.
//!
//! Clause 5.12 still decides which registrations are returned at all: one whose `information` matches on none of its elements is not in the answer.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A registration covering two unrelated kinds of Entity, so a query naming
/// one of them leaves the other to be trimmed away.
fn registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {"entities": [{"type": "Vehicle"}], "propertyNames": ["brandName"]},
            {"entities": [{"type": "OffStreetParking"}], "propertyNames": ["availableSpotNumber"]}
        ],
        "endpoint": "http://localhost:9999/source",
        "@context": [CORE_CONTEXT]
    })
}

/// The registration a discovery answered with, or `None` when it answered
/// without it.
async fn discovered(client: &TestClient, query: &str, id: &str) -> Option<Value> {
    let listed = client.query_registrations(query, Some(CORE_CONTEXT)).await.expect("query registrations");
    assert_eq!(listed.status, StatusCode::OK, "query: {:?}", listed.body);
    listed
        .json_value()
        .expect("a registration query answers JSON")
        .as_array()
        .expect("a registration query answers an array")
        .iter()
        .find(|candidate| candidate["id"] == json!(id))
        .cloned() // clone: the response body is dropped with the request
}

/// Clause 5.10.2.5: a matched registration is trimmed to the `RegistrationInfo`
/// elements relevant to the query, so a discovery by one Entity type answers
/// with only that element and hides the source's unrelated coverage.
#[tokio::test]
async fn regression_registration_discovery_a_matched_registration_is_trimmed_to_matching_information() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ContextSourceRegistration:discovery-information");

    let created = client.create_registration(&registration(&id)).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let found = discovered(client, "type=Vehicle", &id).await.expect("the registration matches on Vehicle");
    let information = found["information"].as_array().expect("the registration carries its information");
    assert_eq!(information.len(), 1, "only the RegistrationInfo element the query matched survives: {found}");
    let entity_types = information[0]["entities"]
        .as_array()
        .expect("the surviving element carries its entities")
        .iter()
        .filter_map(|entity| entity["type"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(entity_types, ["Vehicle"], "the surviving element is the one the query matched: {found}");

    client.delete_registration(&id).await.expect("delete registration");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.12 still decides membership: a registration none of whose
/// `RegistrationInfo` elements match is not in the answer at all.
#[tokio::test]
async fn regression_registration_discovery_a_registration_matching_nothing_is_not_returned() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ContextSourceRegistration:discovery-unmatched");

    let created = client.create_registration(&registration(&id)).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let found = discovered(client, "type=NothingIsRegisteredUnderThis", &id).await;
    assert!(found.is_none(), "a registration that matches nothing is not discovered: {found:?}");

    client.delete_registration(&id).await.expect("delete registration");
    scope.finish().await.expect("cleanup failed");
}
