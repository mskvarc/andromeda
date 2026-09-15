//! Clause 4.9 `q` over a system sub-attribute (`observedAt`, `unitCode`).
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.9 lets a Query Term address a sub-attribute
//! by a dotted path, and its list of "system generated" sub-attributes includes
//! `observedAt` and `unitCode`. These are stored as bare value nodes
//! (`[{"@type":…,"@value":…}]`) rather than as attributes with a `hasValue`
//! carrier, so the payload jsonpath that answers an ordinary (non-local) query
//! has to terminate at their `@value` directly. It used to insert a `hasValue`
//! hop for every sub-attribute alike, so `q=<attr>.observedAt>…` reached nothing
//! and selected no entity.
//!
//! Every query below is an ordinary `GET /entities` with `q`, which is the
//! request the PostgreSQL pushdown answers, and each names the entity by `id` so
//! the answer is about the seeded entities alone.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// One `Building` whose `temperature` Property carries the two system
/// sub-attributes under test plus a user-defined one (`accuracy`) that is stored
/// behind a `hasValue` carrier, so the two storage shapes are exercised side by
/// side.
fn building(id: &str, observed_at: &str, unit_code: &str, accuracy: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "temperature": {
            "type": "Property",
            "value": 20,
            "observedAt": observed_at,
            "unitCode": unit_code,
            "accuracy": {"type": "Property", "value": accuracy}
        },
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one building and assert it was stored.
async fn create(scope: &TestEnvironment, entity: &Value) {
    let created = scope.client().post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Whether one `q` expression selects the entity named by `id`.
///
/// The query names the entity by `id`, so the answer is about that entity alone
/// and no concurrently running suite can contribute a row.
async fn matches(scope: &TestEnvironment, id: &str, q: &str) -> bool {
    let query = format!("id={}&q={}", urlencoding::encode(id), urlencoding::encode(q));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("entity query failed");
    assert_eq!(response.status, StatusCode::OK, "q={q}: {:?}", response.body);
    let body = response.json_value().expect("query response must be JSON");
    let entities = body.as_array().expect("query response must be an array");
    assert!(entities.len() <= 1, "q={q} selected more than the one named entity: {body}");
    entities.len() == 1
}

/// Clause 4.9: a `q` on the system sub-attribute `observedAt` selects by its
/// stored `DateTime` value, under every ordering operator and a range of them.
#[tokio::test]
async fn regression_q_system_sub_attribute_observed_at_selects_by_its_datetime_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let hot = scope.id("Building:observed-at-hot");
    let cold = scope.id("Building:observed-at-cold");
    create(&scope, &building(&hot, "2024-06-01T00:00:00Z", "CEL", 0.5)).await;
    create(&scope, &building(&cold, "2024-01-01T00:00:00Z", "FAH", 0.9)).await;

    // `>` selects the later reading and excludes the earlier one.
    assert!(
        matches(&scope, &hot, "temperature.observedAt>\"2024-03-01T00:00:00Z\"").await,
        "the June reading is after March"
    );
    assert!(
        !matches(&scope, &cold, "temperature.observedAt>\"2024-03-01T00:00:00Z\"").await,
        "the January reading is not"
    );

    // `==` selects the exact instant.
    assert!(
        matches(&scope, &hot, "temperature.observedAt==\"2024-06-01T00:00:00Z\"").await,
        "the June reading matches exactly"
    );
    assert!(
        !matches(&scope, &cold, "temperature.observedAt==\"2024-06-01T00:00:00Z\"").await,
        "the January reading does not"
    );

    // A bounded range selects only the reading inside it.
    let range = "temperature.observedAt>\"2024-03-01T00:00:00Z\";temperature.observedAt<\"2024-09-01T00:00:00Z\"";
    assert!(matches(&scope, &hot, range).await, "the June reading is inside the range");
    assert!(!matches(&scope, &cold, range).await, "the January reading is outside it");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: a `q` on the system sub-attribute `unitCode` selects by its
/// stored string value.
#[tokio::test]
async fn regression_q_system_sub_attribute_unit_code_selects_by_its_string_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let celsius = scope.id("Building:unit-celsius");
    let fahrenheit = scope.id("Building:unit-fahrenheit");
    create(&scope, &building(&celsius, "2024-06-01T00:00:00Z", "CEL", 0.5)).await;
    create(&scope, &building(&fahrenheit, "2024-06-01T00:00:00Z", "FAH", 0.5)).await;

    assert!(
        matches(&scope, &celsius, "temperature.unitCode==\"CEL\"").await,
        "the Celsius reading matches CEL"
    );
    assert!(
        !matches(&scope, &fahrenheit, "temperature.unitCode==\"CEL\"").await,
        "the Fahrenheit reading does not"
    );
    assert!(
        matches(&scope, &fahrenheit, "temperature.unitCode==\"FAH\"").await,
        "the Fahrenheit reading matches FAH"
    );
    assert!(
        !matches(&scope, &celsius, "temperature.unitCode==\"FAH\"").await,
        "the Celsius reading does not"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The control: a user-defined sub-attribute is still reached through its
/// `hasValue` carrier, so the fix does not regress the ordinary case.
#[tokio::test]
async fn regression_q_system_sub_attribute_a_user_defined_sub_attribute_still_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let precise = scope.id("Building:accuracy-precise");
    let coarse = scope.id("Building:accuracy-coarse");
    create(&scope, &building(&precise, "2024-06-01T00:00:00Z", "CEL", 0.9)).await;
    create(&scope, &building(&coarse, "2024-06-01T00:00:00Z", "CEL", 0.2)).await;

    assert!(
        matches(&scope, &precise, "temperature.accuracy>0.7").await,
        "the precise reading is above the threshold"
    );
    assert!(!matches(&scope, &coarse, "temperature.accuracy>0.7").await, "the coarse reading is not");

    scope.finish().await.expect("cleanup failed");
}
