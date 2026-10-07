//! The `type` URL parameter of the entity write operations.
//!
//! ETSI GS CIM 009 v1.9.1 Tables 6.5.3.2-1, 6.5.3.3-1, 6.6.3.1-1, 6.6.3.2-1,
//! 6.7.3.1-1, 6.7.3.2-1 and 6.7.3.3-1 each list `type` ("Selection of Entity
//! Types as per clause 4.17") among "the URL parameters that shall be
//! supported", and clauses 5.6.6.4, 5.6.18.4, 5.6.3.4, 5.6.2.4, 5.6.4.4,
//! 5.6.5.4 and 5.6.19.4 know the target as the entity whose "id (URI), and where
//! specified type, is equivalent": an entity outside the selector is a
//! `ResourceNotFound`, and the parameter is not an unknown one.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.2 names for input the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The problem type clause 6.3.17 names for a target no registration or local
/// entity supplies.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// The selectors the seeded Vehicle satisfies (clause 4.17).
const INSIDE: [&str; 2] = ["Vehicle", "Vehicle|Other"];

/// A selector the seeded Vehicle does not satisfy.
const OUTSIDE: &str = "Other";

/// An entity write operation Tables 6.5.3.2-1 to 6.7.3.3-1 give a `type`
/// parameter.
#[derive(Clone, Copy, Debug)]
enum WriteOperation {
    /// Delete Entity, clause 5.6.6.
    DeleteEntity,
    /// Replace Entity, clause 5.6.18.
    ReplaceEntity,
    /// Append Entity Attributes, clause 5.6.3.
    AppendAttributes,
    /// Update Entity Attributes, clause 5.6.2.
    UpdateAttributes,
    /// Partial Attribute Update, clause 5.6.4.
    PartialAttributeUpdate,
    /// Delete Attribute, clause 5.6.5.
    DeleteAttribute,
    /// Replace Attribute, clause 5.6.19.
    ReplaceAttribute,
}

/// Every operation under test.
const OPERATIONS: [WriteOperation; 7] = [
    WriteOperation::DeleteEntity,
    WriteOperation::ReplaceEntity,
    WriteOperation::AppendAttributes,
    WriteOperation::UpdateAttributes,
    WriteOperation::PartialAttributeUpdate,
    WriteOperation::DeleteAttribute,
    WriteOperation::ReplaceAttribute,
];

impl WriteOperation {
    /// The HTTP method of the operation.
    const fn method(self) -> Method {
        match self {
            WriteOperation::DeleteEntity | WriteOperation::DeleteAttribute => Method::DELETE,
            WriteOperation::ReplaceEntity | WriteOperation::ReplaceAttribute => Method::PUT,
            WriteOperation::AppendAttributes => Method::POST,
            WriteOperation::UpdateAttributes | WriteOperation::PartialAttributeUpdate => Method::PATCH,
        }
    }

    /// The path of the operation under `/entities/{id}`.
    const fn path_suffix(self) -> &'static str {
        match self {
            WriteOperation::DeleteEntity | WriteOperation::ReplaceEntity => "",
            WriteOperation::AppendAttributes | WriteOperation::UpdateAttributes => "/attrs",
            WriteOperation::PartialAttributeUpdate | WriteOperation::DeleteAttribute | WriteOperation::ReplaceAttribute => "/attrs/speed",
        }
    }

    /// The request body of the operation, when it takes one.
    fn body(self) -> Option<Value> {
        match self {
            WriteOperation::DeleteEntity | WriteOperation::DeleteAttribute => None,
            WriteOperation::ReplaceEntity => Some(json!({"type": "Vehicle", "speed": {"type": "Property", "value": 1}})),
            WriteOperation::AppendAttributes => Some(json!({"color": {"type": "Property", "value": "red"}})),
            WriteOperation::UpdateAttributes => Some(json!({"speed": {"type": "Property", "value": 70}})),
            WriteOperation::PartialAttributeUpdate => Some(json!({"value": 71})),
            WriteOperation::ReplaceAttribute => Some(json!({"type": "Property", "value": 72})),
        }
    }

    /// Whether `entity`, the stored entity after the operation (`None` once
    /// deleted), shows the operation's effect.
    fn applied_to(self, entity: Option<&Value>) -> bool {
        match (self, entity) {
            (WriteOperation::DeleteEntity, entity) => entity.is_none(),
            (WriteOperation::ReplaceEntity, Some(entity)) => entity["speed"]["value"] == json!(1) && entity.get("plate").is_none(),
            (WriteOperation::AppendAttributes, Some(entity)) => entity["color"]["value"] == json!("red"),
            (WriteOperation::UpdateAttributes, Some(entity)) => entity["speed"]["value"] == json!(70),
            (WriteOperation::PartialAttributeUpdate, Some(entity)) => entity["speed"]["value"] == json!(71),
            (WriteOperation::DeleteAttribute, Some(entity)) => entity.get("speed").is_none(),
            (WriteOperation::ReplaceAttribute, Some(entity)) => entity["speed"]["value"] == json!(72),
            (
                WriteOperation::ReplaceEntity
                | WriteOperation::AppendAttributes
                | WriteOperation::UpdateAttributes
                | WriteOperation::PartialAttributeUpdate
                | WriteOperation::DeleteAttribute
                | WriteOperation::ReplaceAttribute,
                None,
            ) => false,
        }
    }
}

/// Create the Vehicle every operation writes to.
async fn seed(client: &TestClient, id: &str) {
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 60},
            "plate": {"type": "Property", "value": "ABC"},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// The entity as stored, or `None` when it does not exist.
async fn stored(client: &TestClient, id: &str) -> Option<Value> {
    let retrieved = client
        .get_entity(id, "", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert!(
        matches!(retrieved.status, StatusCode::OK | StatusCode::NOT_FOUND),
        "retrieve answered {}: {:?}",
        retrieved.status,
        retrieved.body
    );
    (retrieved.status == StatusCode::OK).then(|| retrieved.json_value().expect("a retrieved entity is JSON"))
}

/// Send `operation` to the entity `id` with the query string `query`.
async fn send(client: &TestClient, operation: WriteOperation, id: &str, query: &str) -> TestResponse {
    let path = format!("/ngsi-ld/v1/entities/{}{}?{query}", urlencoding::encode(id), operation.path_suffix());
    let request = client.raw(operation.method(), &path);
    match operation.body() {
        Some(body) => request.header("content-type", "application/json").body(body.to_string().into_bytes()),
        None => request,
    }
    .send()
    .await
    .expect("entity write request")
}

/// The `type=` query string selecting `selector`.
fn type_query(selector: &str) -> String {
    format!("type={}", urlencoding::encode(selector))
}

/// Tables 6.5.3.2-1 to 6.7.3.3-1 and clause 4.17: an entity satisfying the
/// selector is the target, so every operation applies to it, whether the
/// selector names its type alone or among others.
#[tokio::test]
async fn regression_type_selection_on_writes_an_entity_inside_the_selector_is_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for operation in OPERATIONS {
        for selector in INSIDE {
            let id = scope.id(&format!("Vehicle:inside-{operation:?}-{}", selector.len()));
            seed(client, &id).await;

            let answer = send(client, operation, &id, &type_query(selector)).await;

            assert_eq!(answer.status, StatusCode::NO_CONTENT, "{operation:?} with type={selector}: {:?}", answer.body);
            assert!(
                operation.applied_to(stored(client, &id).await.as_ref()),
                "{operation:?} with type={selector} left the entity unchanged"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.2.4 to 5.6.6.4, 5.6.18.4 and 5.6.19.4: "no existing Entity whose
/// id (URI), and where specified type, is equivalent held locally" is a
/// `ResourceNotFound`, and the entity is as it was.
#[tokio::test]
async fn regression_type_selection_on_writes_an_entity_outside_the_selector_is_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for operation in OPERATIONS {
        let id = scope.id(&format!("Vehicle:outside-{operation:?}"));
        seed(client, &id).await;
        let before = stored(client, &id).await;

        let answer = send(client, operation, &id, &type_query(OUTSIDE)).await;

        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{operation:?} with type={OUTSIDE}: {:?}", answer.body);
        let problem = answer.json_value().expect("a refusal carries a problem details body");
        assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{operation:?}: {problem}");
        assert_eq!(stored(client, &id).await, before, "{operation:?} changed an entity outside its selector");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.2: a `type` value that is no clause 4.17 selector is input the
/// operation cannot accept, a value inside a parameter it defines.
#[tokio::test]
async fn regression_type_selection_on_writes_a_malformed_selector_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for operation in OPERATIONS {
        let id = scope.id(&format!("Vehicle:malformed-{operation:?}"));
        seed(client, &id).await;
        let before = stored(client, &id).await;

        let answer = send(client, operation, &id, "type=Vehicle%7C").await;

        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{operation:?}: {:?}", answer.body);
        let problem = answer.json_value().expect("a refusal carries a problem details body");
        assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{operation:?}: {problem}");
        assert_eq!(stored(client, &id).await, before, "{operation:?} changed the entity");
    }

    scope.finish().await.expect("cleanup failed");
}
