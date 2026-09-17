//! A Context Source Registration created without an `id` is assigned one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.9.2 (Register Context Source) takes a
//! Context Source Registration whose `id` is optional (Table 5.2.9-1 gives it
//! cardinality 0..1), and clause 5.2.7 states that "if no entity `id` ... is
//! present ... a new one shall be assigned". The HTTP binding of clause 6.9.3.1
//! answers 201 Created with the assigned resource in the `Location` header, and
//! the registration is then retrievable under it.
//!
//! Corresponds to the official suite case `033_11`. This pins the assignment
//! behaviour: an anonymous registration is accepted, given an identifier, and
//! reachable under it.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Clause 5.9.2 with clause 5.2.7: a registration with no `id` is created, its
/// assigned identifier is returned in `Location`, and it is retrievable there.
#[tokio::test]
async fn regression_anonymous_registration_id_is_assigned_and_retrievable() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let registration = json!({
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://localhost:1111",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&registration).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::CREATED, "an anonymous registration is created: {:?}", created.body);

    let location = created
        .location()
        .expect("a Location header is valid text")
        .expect("clause 6.9.3.1 returns the assigned resource in Location");
    let assigned = location.rsplit('/').next().expect("the Location names the assigned identifier");
    assert!(
        assigned.starts_with("urn:ngsi-ld:"),
        "the broker assigned an NGSI-LD identifier, got {location}"
    );

    let retrieved = client
        .retrieve_registration(assigned, Some(CORE_CONTEXT))
        .await
        .expect("retrieve registration request");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "the assigned registration is retrievable under its Location: {:?}",
        retrieved.body
    );
    let body = retrieved.json_value().expect("a registration body is JSON");
    assert_eq!(body["id"], json!(assigned), "the retrieved registration carries the assigned id: {body}");

    client.delete_registration(assigned).await.expect("delete registration request");
    scope.finish().await.expect("environment cleanup");
}
