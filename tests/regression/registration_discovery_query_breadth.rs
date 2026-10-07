//! Context Source Registration discovery needs a narrowing condition.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.10.2.4 rejects a discovery query that is "too wide" -- one carrying no condition the broker can narrow the candidate registrations by. An entity-`id` list is not such a condition: clause 5.10.2 discovery narrows by the entity *type*, attribute names, `q`, or geometry a registration advertises, so a query naming only entity identifiers still asks the broker to scan every registration. It is a `BadRequestData`, the category clause 5.5.2 gives to input that does not meet the requirements of the operation. Adding any admitted narrowing condition, such as an entity type, makes the same id list a valid discovery query answered with 200.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use urlencoding::encode;

/// Register one Context Source serving `entity_type`, and return its id.
async fn register(client: &TestClient, scope: &TestEnvironment, entity_type: &str, suffix: &str) -> String {
    let id = scope.id(&format!("ContextSourceRegistration:{suffix}"));
    let registration = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": entity_type}]}],
        "endpoint": "http://localhost:1111",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&registration).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);
    id
}

/// Clause 5.10.2.4: a discovery query narrowed only by an entity-`id` list is
/// too wide and refused with `BadRequestData`; the same list narrowed by an
/// entity `type` is accepted with 200.
#[tokio::test]
async fn regression_registration_discovery_an_id_only_query_is_too_wide() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    register(client, &scope, "Building", "breadth-a").await;
    register(client, &scope, "Building", "breadth-b").await;

    let entity_ids = format!("{},{}", encode("urn:ngsi-ld:Building:breadth-1"), encode("urn:ngsi-ld:Building:breadth-2"));

    let too_wide = client
        .query_registrations(&format!("id={entity_ids}"), Some(CORE_CONTEXT))
        .await
        .expect("id-only discovery request");
    assert_eq!(
        too_wide.status,
        StatusCode::BAD_REQUEST,
        "an id-only discovery query is too wide: {:?}",
        too_wide.body
    );
    let problem = too_wide.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA.as_str()), "the type is BadRequestData: {problem}");

    let narrowed = client
        .query_registrations(&format!("id={entity_ids}&type=Building"), Some(CORE_CONTEXT))
        .await
        .expect("narrowed discovery request");
    assert_eq!(
        narrowed.status,
        StatusCode::OK,
        "the same id list narrowed by an entity type is accepted: {:?}",
        narrowed.body
    );
    assert!(
        narrowed.json_value().expect("a discovery answer is JSON").is_array(),
        "a discovery query answers a registration array"
    );

    scope.finish().await.expect("environment cleanup");
}
