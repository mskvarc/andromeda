//! Modify Attribute instance takes the `EntityTemporal` Fragment clause 5.6.14.3
//! defines.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.14.3: "A JSON-LD document representing an
//! NGSI-LD Fragment of `EntityTemporal`, including only the new Attribute
//! instance, contained by an Array of exactly one item", typed `EntityTemporal
//! Fragment` by Table 6.22.3.1-1. Clause 5.4 makes a Fragment a JSON-LD object
//! mirroring its target, whose `id` "shall be equal to the id of the target".
//! So the body is `{"speed": [{instance}]}`, and a bare instance, a bare array,
//! an instance not contained by an Array, other than one instance, other than
//! one Attribute, or another Entity's `id` is `BadRequestData`. Clause 5.6.14.4
//! raises `ResourceNotFound` for an unknown Entity, Attribute or instance, so a
//! Fragment naming another Attribute is refused only once the target is known.
//! Clause 5.6.14.4 then replaces the instance, keeping its `createdAt` and
//! setting `modifiedAt`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::{
    context::CORE_CONTEXT,
    problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND},
};
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use urlencoding::encode;

/// The instance the modifications write.
fn corrected() -> Value {
    json!({"type": "Property", "value": 129, "observedAt": "2025-01-01T00:00:00Z"})
}

/// A temporal Vehicle with one `speed` instance and one `fuel` instance.
struct Recorded {
    /// The Entity identifier.
    id: String,
    /// The `instanceId` of the `speed` instance.
    speed_instance: String,
    /// The `createdAt` of the `speed` instance.
    created_at: Value,
}

/// Create the Vehicle and read back its `speed` instance.
async fn record(scope: &TestEnvironment, name: &str) -> Recorded {
    let id = scope.id(&format!("Vehicle:{name}"));
    let created = scope
        .client()
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"}],
            "fuel": [{"type": "Property", "value": 40, "observedAt": "2025-01-01T00:00:00Z"}],
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let speed = read_speed(scope, &id).await;
    Recorded {
        speed_instance: speed["instanceId"].as_str().expect("an instanceId").to_owned(),
        created_at: speed["createdAt"].clone(),
        id,
    }
}

/// The one `speed` instance of `id`, with its system attributes.
async fn read_speed(scope: &TestEnvironment, id: &str) -> Value {
    let history = scope
        .client()
        .get_temporal_entity(id, "options=sysAttrs", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(history.status, StatusCode::OK, "{:?}", history.body);
    let body = history.json_value().expect("a temporal answer is JSON");
    let speed = &body["speed"];
    speed.as_array().and_then(|instances| instances.first()).unwrap_or(speed).clone()
}

/// PATCH `body` onto `attribute`'s `instance` of `id`, returning the status and
/// the problem type of a refusal.
async fn modify(scope: &TestEnvironment, id: &str, attribute: &str, instance: &str, body: &Value) -> (StatusCode, Option<String>) {
    let response = scope
        .client()
        .raw(
            Method::PATCH,
            &format!("/ngsi-ld/v1/temporal/entities/{}/attrs/{attribute}/{}", encode(id), encode(instance)),
        )
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("modify attribute instance request");
    let problem = response.json_value().ok().and_then(|problem| problem["type"].as_str().map(str::to_owned));
    (response.status, problem)
}

/// The clause 5.6.14.3 Fragment, carrying the target's `id` and `type` as
/// clause 5.4 allows, replaces the instance: its `instanceId` and `createdAt`
/// stay, and its `modifiedAt` is set (clause 5.6.14.4).
#[tokio::test]
async fn regression_temporal_instance_fragment_the_specified_fragment_replaces_the_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let recorded = record(&scope, "fragment-replaces").await;

    let body = json!({"id": recorded.id, "type": "Vehicle", "speed": [corrected()], "@context": [CORE_CONTEXT]});
    let (status, problem) = modify(&scope, &recorded.id, "speed", &recorded.speed_instance, &body).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{problem:?}");

    let speed = read_speed(&scope, &recorded.id).await;
    assert_eq!(speed["value"], json!(129), "{speed}");
    assert_eq!(speed["instanceId"], json!(recorded.speed_instance), "{speed}");
    assert_eq!(speed["createdAt"], recorded.created_at, "{speed}");
    assert!(speed["modifiedAt"].is_string(), "{speed}");

    scope.finish().await.expect("cleanup failed");
}

/// Every body that is not the clause 5.6.14.3 Fragment is `BadRequestData`, and
/// the instance keeps its value.
#[tokio::test]
async fn regression_temporal_instance_fragment_every_other_body_shape_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let recorded = record(&scope, "fragment-refusals").await;
    let with_context = |mut body: Value| {
        if let Some(members) = body.as_object_mut() {
            members.insert("@context".to_owned(), json!([CORE_CONTEXT]));
        }
        body
    };
    let refused = [
        ("a bare instance", with_context(corrected())),
        ("a bare array", json!([corrected()])),
        ("an instance not contained by an array", with_context(json!({"speed": corrected()}))),
        ("two instances", with_context(json!({"speed": [corrected(), corrected()]}))),
        ("an empty array", with_context(json!({"speed": []}))),
        ("two attributes", with_context(json!({"speed": [corrected()], "fuel": [corrected()]}))),
        ("no attribute", with_context(json!({}))),
        (
            "another entity's id",
            with_context(json!({"id": "urn:ngsi-ld:Vehicle:other", "speed": [corrected()]})),
        ),
    ];

    for (shape, body) in refused {
        let (status, problem) = modify(&scope, &recorded.id, "speed", &recorded.speed_instance, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{shape}");
        assert_eq!(problem.as_deref(), Some(BAD_REQUEST_DATA.as_str()), "{shape}");
    }
    assert_eq!(
        read_speed(&scope, &recorded.id).await["value"],
        json!(60),
        "no refused body changed the instance"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A well-formed Fragment naming another Attribute than the one the URL
/// targets carries no instance of the target: `BadRequestData`.
#[tokio::test]
async fn regression_temporal_instance_fragment_a_fragment_naming_another_attribute_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let recorded = record(&scope, "fragment-other-attribute").await;

    let body = json!({"fuel": [corrected()], "@context": [CORE_CONTEXT]});
    let (status, problem) = modify(&scope, &recorded.id, "speed", &recorded.speed_instance, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem.as_deref(), Some(BAD_REQUEST_DATA.as_str()));
    assert_eq!(read_speed(&scope, &recorded.id).await["value"], json!(60));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.14.4 raises `ResourceNotFound` for an unknown Attribute or
/// instance before the Fragment is read, so a well-formed Fragment answers 404
/// there, whichever Attribute it names.
#[tokio::test]
async fn regression_temporal_instance_fragment_an_unknown_target_is_not_found_before_the_fragment_is_read() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let recorded = record(&scope, "fragment-not-found").await;
    let body = json!({"speed": [corrected()], "@context": [CORE_CONTEXT]});

    let (status, problem) = modify(&scope, &recorded.id, "speed2", &recorded.speed_instance, &body).await;
    assert_eq!(
        (status, problem.as_deref()),
        (StatusCode::NOT_FOUND, Some(RESOURCE_NOT_FOUND.as_str())),
        "unknown attribute"
    );

    let (status, problem) = modify(&scope, &recorded.id, "speed", "urn:ngsi-ld:01234567890123456789", &body).await;
    assert_eq!(
        (status, problem.as_deref()),
        (StatusCode::NOT_FOUND, Some(RESOURCE_NOT_FOUND.as_str())),
        "unknown instance"
    );

    scope.finish().await.expect("cleanup failed");
}
