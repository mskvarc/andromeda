//! An identifier carrying a literal space, tab or line break is not a URI.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.2 makes every NGSI-LD identifier a URI
//! (IETF RFC 3986), or by NOTE 2 an IRI (IETF RFC 3987); neither grammar admits
//! whitespace in any component. Clause 5.6.1.4 raises `BadRequestData` when the
//! restrictions of clause 5.2.4 -- an Entity `id` and a Relationship `object`
//! being valid URIs -- are not met.
//!
//! The [`andromeda_ngsi_ld_lexicon::uri::Uri`] type holds the URI grammar before
//! any form is parsed. The WHATWG URL parser is not a URI validator: it keeps the
//! literal space of `urn:ngsi-ld:Building:has space` in the opaque path, and
//! silently strips a tab or line break, which would store the entity under an
//! identifier the client never sent.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.6.1.4 gives an Entity whose members break the
/// clause 5.2.4 restrictions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// A Building named `id`, related to `located_at`.
fn building(id: &str, located_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": located_at},
        "@context": [CORE_CONTEXT]
    })
}

/// Assert a refusal is the `BadRequestData` of clause 5.6.1.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// Assert no entity is stored under `id`.
async fn assert_not_stored(client: &TestClient, id: &str, scenario: &str) {
    let retrieved = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity");
    assert_eq!(retrieved.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", retrieved.body);
}

/// Clause 4.6.2: a space, a tab and a line break in an Entity `id` are each
/// refused. The WHATWG parser would have kept the space and stripped the other
/// two, so the spelling it would have stored instead is checked to be absent.
#[tokio::test]
async fn regression_whitespace_identifier_an_entity_id_with_whitespace_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let target = scope.id("City:target");

    for (name, whitespace) in [("space", " "), ("tab", "\t"), ("line break", "\n")] {
        let stripped = scope.id(&format!("Building:has-{}", name.replace(' ', "-")));
        let id = format!("{stripped}{whitespace}whitespace");
        let response = client.post_entity(&building(&id, &target)).send().await.expect("create entity request");
        assert_bad_request_data(&response, &format!("an Entity id with a {name}"));
        assert_not_stored(client, &format!("{stripped}whitespace"), &format!("the {name} is not stripped into another id")).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.2: a Relationship `object` is a URI too, so one carrying a space
/// refuses the whole creation.
#[tokio::test]
async fn regression_whitespace_identifier_a_relationship_object_with_a_space_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:spaced-object");

    let response = client
        .post_entity(&building(&id, "urn:ngsi-ld:City:has space"))
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&response, "a Relationship object with a space");
    assert_not_stored(client, &id, "a refused creation stores nothing").await;

    scope.finish().await.expect("cleanup failed");
}

/// IETF RFC 3986 section 2.1: a percent-encoded space is an identifier
/// character, so the same identifier spelled that way is created and given
/// back verbatim. The Relationship target is where that is at stake: its
/// percent-encoding is undone for a client that encoded a whole identifier,
/// and undoing this escape would have made it the refused spelling above.
#[tokio::test]
async fn regression_whitespace_identifier_a_percent_encoded_space_is_accepted_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:has%20space");
    let target = scope.id("City:has%20space");

    let response = client.post_entity(&building(&id, &target)).send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);

    let retrieved = client
        .get_entity(&id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity");
    assert_eq!(retrieved.status, StatusCode::OK, "{:?}", retrieved.body);
    let body = retrieved.json_value().expect("a retrieval answers JSON");
    assert_eq!(body["id"], json!(id), "{body}");
    assert_eq!(body["locatedAt"]["object"], json!(target), "{body}");

    client.delete_entity(&id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
