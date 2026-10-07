//! What the deprecated `attrs` parameter admits beside Attribute names.
//!
//! Every `attrs` row in ETSI GS CIM 009 v1.9.1 carries the same restriction: "Each String is an Attribute (Property or Relationship) name." Table 6.4.3.2-1 says it of Query Entities, Table 6.5.3.1-1 of Retrieve Entity, Table 6.18.3.2-1 of Query Temporal Evolution of Entities and Table 6.19.3.1-1 of Retrieve Temporal Evolution of an Entity; Table 5.2.23-1 restricts the `Query` datatype's `attrs` member the same way in its Restrictions column, "Attribute name as short hand strings or URIs". `pick` is the parameter that is not so restricted -- "Each String is an Entity member ("id", "type", "scope" or an Attribute name)" -- which is the contrast the two draw.
//!
//! Whether `scope` is an Attribute name is then a question clause 4.18 answers per representation: "The Scope is represented as a special scope Property that is non-reified in the normalized Entity representation and reified in the temporal representation of an Entity." So the temporal query admits it and the Entity operations do not -- except that Table 6.19.3.1-1 withdraws it for the temporal retrieval in so many words, "except that id, type, scope are not allowed", and an explicit prohibition beats an inference from the representation.
//!
//! Every test carries the `pick` spelling beside the `attrs` one, so the contrast the two tables draw is asserted rather than assumed.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives a request whose content is incorrect.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The temporal window every temporal query here carries (clause 4.11).
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// A Vehicle carrying a Scope and one ordinary Property.
fn scoped_vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "scope": ["/Madrid/Centro"],
        "speed": {"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    })
}

/// Create the entity every test reads back.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one `application/json` read against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .send()
        .await
        .expect("read request")
}

/// Assert a refusal is the `BadRequestData` of clause 5.5.4 and names `member`.
fn assert_refused(response: &TestResponse, member: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    assert!(
        problem["detail"].as_str().is_some_and(|detail| detail.contains(member)),
        "{scenario}: the refusal must name '{member}': {problem}"
    );
}

/// Assert a read succeeded and answer with its body.
fn accepted(response: &TestResponse, scenario: &str) -> Value {
    assert_eq!(response.status, StatusCode::OK, "{scenario}: {:?}", response.body);
    response.json_value().expect("an accepted read answers with JSON")
}

/// Table 6.4.3.2-1 restricts `attrs` on Query Entities to "an Attribute (Property or Relationship) name", and clause 4.18 leaves the Scope non-reified in the Entity representation a query answers in, so `scope` names no Attribute there. `pick` in the same table takes "an Entity member ("id", "type", "scope" or a projected Attribute name)" and selects it.
#[tokio::test]
async fn regression_attrs_scope_the_entity_query_refuses_scope_in_attrs_and_selects_it_with_pick() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-scope-query");
    create_entity(client, &scoped_vehicle(&id)).await;
    let encoded = urlencoding::encode(&id).into_owned();

    let refused = read(client, &format!("/ngsi-ld/v1/entities?id={encoded}&attrs=speed,scope")).await;
    let picked = read(client, &format!("/ngsi-ld/v1/entities?id={encoded}&pick=id,type,speed,scope&options=keyValues")).await;
    let attribute_only = read(client, &format!("/ngsi-ld/v1/entities?id={encoded}&attrs=speed&options=keyValues")).await;

    assert_refused(&refused, "scope", "attrs=speed,scope on Query Entities");

    let picked = accepted(&picked, "pick=id,type,speed,scope on Query Entities");
    assert_eq!(picked[0]["scope"], json!("/Madrid/Centro"), "{picked}");

    // The control: `attrs` itself works, so the refusal above is about `scope` and not about the parameter.
    let attribute_only = accepted(&attribute_only, "attrs=speed on Query Entities");
    assert_eq!(attribute_only[0]["speed"], json!(60), "{attribute_only}");

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.5.3.1-1 says it twice for Retrieve Entity: `attrs` is "a synonym for the pick parameter, except that id, type, scope are not allowed", and "Each String is an Attribute (Property or Relationship) name".
#[tokio::test]
async fn regression_attrs_scope_the_entity_retrieval_refuses_scope_in_attrs_and_selects_it_with_pick() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-scope-retrieve");
    create_entity(client, &scoped_vehicle(&id)).await;
    let encoded = urlencoding::encode(&id).into_owned();

    let refused = read(client, &format!("/ngsi-ld/v1/entities/{encoded}?attrs=speed,scope")).await;
    let picked = read(client, &format!("/ngsi-ld/v1/entities/{encoded}?pick=id,type,speed,scope&options=keyValues")).await;

    assert_refused(&refused, "scope", "attrs=speed,scope on Retrieve Entity");

    let picked = accepted(&picked, "pick=id,type,speed,scope on Retrieve Entity");
    assert_eq!(picked["scope"], json!("/Madrid/Centro"), "{picked}");

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-1 restricts `attrs` on Query Temporal Evolution of Entities to "an Attribute (Property or Relationship) name" and withdraws nothing further, and clause 4.18 reifies the Scope in the representation this operation answers in, so `scope` is an Attribute name here. Annex C clause C.5.16 prints exactly this request; the annex is informative, so it corroborates the row rather than deciding it.
#[tokio::test]
async fn regression_attrs_scope_the_temporal_query_admits_the_reified_scope_property_in_attrs() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-scope-temporal-query");
    create_entity(client, &scoped_vehicle(&id)).await;
    let encoded = urlencoding::encode(&id).into_owned();

    let with_attrs = read(client, &format!("/ngsi-ld/v1/temporal/entities?id={encoded}&attrs=speed,scope&{WINDOW}")).await;
    let with_pick = read(client, &format!("/ngsi-ld/v1/temporal/entities?id={encoded}&pick=id,type,speed,scope&{WINDOW}")).await;

    let with_attrs = accepted(&with_attrs, "attrs=speed,scope on the temporal query");
    let with_pick = accepted(&with_pick, "pick=id,type,speed,scope on the temporal query");

    // Clause 4.18: the Scope is reified in the temporal representation, so what `attrs` selected is a Property carrying the Scope value.
    assert_eq!(with_attrs[0]["scope"][0]["type"], json!("Property"), "{with_attrs}");
    assert_eq!(with_attrs[0]["scope"][0]["value"], json!("/Madrid/Centro"), "{with_attrs}");
    assert_eq!(with_attrs[0]["speed"][0]["value"], json!(60), "{with_attrs}");

    // The two spellings select the same members, which is what makes `attrs` a synonym for a combination of `pick` and `q` in the first place.
    assert_eq!(with_attrs[0]["scope"], with_pick[0]["scope"], "{with_pick}");

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.19.3.1-1 withdraws it again for Retrieve Temporal Evolution of an Entity alone -- "A synonym for the pick parameter, except that id, type, scope are not allowed" -- and an explicit prohibition beats the inference the representation would otherwise support. `pick` still selects the reified Scope there, so the refusal is about `attrs` and not about the member.
#[tokio::test]
async fn regression_attrs_scope_the_temporal_retrieval_refuses_scope_in_attrs_and_selects_it_with_pick() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-scope-temporal-retrieve");
    create_entity(client, &scoped_vehicle(&id)).await;
    let encoded = urlencoding::encode(&id).into_owned();

    let refused = read(client, &format!("/ngsi-ld/v1/temporal/entities/{encoded}?attrs=speed,scope")).await;
    let picked = read(client, &format!("/ngsi-ld/v1/temporal/entities/{encoded}?pick=id,type,speed,scope")).await;
    let attribute_only = read(client, &format!("/ngsi-ld/v1/temporal/entities/{encoded}?attrs=speed")).await;

    assert_refused(&refused, "scope", "attrs=speed,scope on the temporal retrieval");

    let picked = accepted(&picked, "pick=id,type,speed,scope on the temporal retrieval");
    assert_eq!(picked["scope"][0]["value"], json!("/Madrid/Centro"), "{picked}");

    let attribute_only = accepted(&attribute_only, "attrs=speed on the temporal retrieval");
    assert_eq!(attribute_only["speed"][0]["value"], json!(60), "{attribute_only}");

    scope.finish().await.expect("cleanup failed");
}

/// `id` and `type` stay Entity members in every representation, so no `attrs` row admits them anywhere -- including the temporal query, which is the one operation whose row admits `scope`.
#[tokio::test]
async fn regression_attrs_scope_no_operation_admits_id_or_type_in_attrs() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-identity-members");
    create_entity(client, &scoped_vehicle(&id)).await;
    let encoded = urlencoding::encode(&id).into_owned();

    for member in ["id", "type"] {
        for (path, operation) in [
            (format!("/ngsi-ld/v1/entities?id={encoded}&attrs=speed,{member}"), "Query Entities"),
            (format!("/ngsi-ld/v1/entities/{encoded}?attrs=speed,{member}"), "Retrieve Entity"),
            (
                format!("/ngsi-ld/v1/temporal/entities?id={encoded}&attrs=speed,{member}&{WINDOW}"),
                "Query Temporal Evolution of Entities",
            ),
            (
                format!("/ngsi-ld/v1/temporal/entities/{encoded}?attrs=speed,{member}"),
                "Retrieve Temporal Evolution of an Entity",
            ),
        ] {
            let refused = read(client, &path).await;
            assert_refused(&refused, member, &format!("attrs=speed,{member} on {operation}"));
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.23-1 restricts the `Query` datatype's `attrs` member the same way the URL parameter tables do, and clause 6.23.3.1 and clause 6.24.3.1 make the two POST bodies exhibit the behaviour of Query Entities and Query Temporal Evolution of Entities respectively. A body carrying a `temporalQ` is answered in the representation clause 4.18 reifies the Scope in; one without is not.
#[tokio::test]
async fn regression_attrs_scope_the_post_query_bodies_follow_their_own_operations() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:attrs-scope-post-body");
    create_entity(client, &scoped_vehicle(&id)).await;

    let entity_body = json!({
        "type": "Query",
        "entities": [{"id": id, "type": "Vehicle"}],
        "attrs": ["speed", "scope"],
        "@context": [CORE_CONTEXT]
    });
    let temporal_body = json!({
        "type": "Query",
        "entities": [{"id": id, "type": "Vehicle"}],
        "attrs": ["speed", "scope"],
        "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    });

    let refused = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/json")
        .json(&entity_body)
        .send()
        .await
        .expect("entity query body request");
    let admitted = client
        .raw(Method::POST, "/ngsi-ld/v1/temporal/entityOperations/query")
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/json")
        .json(&temporal_body)
        .send()
        .await
        .expect("temporal query body request");

    assert_refused(&refused, "scope", "attrs [speed, scope] in an entity Query body");

    let admitted = accepted(&admitted, "attrs [speed, scope] in a temporal Query body");
    assert_eq!(admitted[0]["scope"][0]["value"], json!("/Madrid/Centro"), "{admitted}");

    scope.finish().await.expect("cleanup failed");
}
