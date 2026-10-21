//! Clause 4.22: an attribute instance past its `expiresAt` is invalid, while
//! its deletion "will always lag the expiresAt timestamp to a certain extent".
//! The projection records each instance's own expiry, and a `q` filter reads
//! only the instances that still stand, so the answer does not depend on
//! whether the attribute-expiry sweep has run yet.

use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use urlencoding::encode;

/// An `expiresAt` far enough ahead that no test run reaches it.
const FAR_FUTURE: &str = "2099-01-01T00:00:00Z";

/// An `expiresAt` a moment in the past: the instance is invalid from the start,
/// whether or not the sweep has deleted it by the time the query runs.
const JUST_PASSED: &str = "2000-01-01T00:00:00Z";

/// A Vehicle whose `speed` is 80 until `expires_at`.
fn vehicle(id: &str, expires_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 80, "expiresAt": expires_at},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// The `expires_at` of the projection row of one Entity's `speed`, absent when
/// no row exists.
async fn projected_speed_expiry(pool: &PgPool, entity_id: &str) -> Option<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "SELECT c.expires_at FROM current_attributes AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE e.id = $1 AND an.name LIKE '%speed'",
    )
    .bind(entity_id)
    .fetch_optional(pool)
    .await
    .expect("the projected expiry must be readable")
}

#[tokio::test]
async fn current_projection_records_each_instances_own_expiry() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Vehicle:expiring-speed");
    let created = scope
        .client()
        .post_entity(&vehicle(&id, FAR_FUTURE))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let expected = FAR_FUTURE.parse::<NgsiLdDateTime>().expect("instant parses").get();
    assert_eq!(projected_speed_expiry(scope.database_pool(), &id).await, Some(Some(expected)));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_q_does_not_match_an_expired_instance() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let expired = scope.id("Vehicle:expired-speed");
    let standing = scope.id("Vehicle:standing-speed");
    for entity in [vehicle(&expired, JUST_PASSED), vehicle(&standing, FAR_FUTURE)] {
        let created = scope.client().post_entity(&entity).send().await.expect("create entity failed");
        assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    }

    let query = format!("type=Vehicle&id={},{}&q=speed%3E50", encode(&expired), encode(&standing));
    let response = scope
        .client()
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query failed");
    assert_eq!(response.status, StatusCode::OK);
    let body = response.json_value().expect("a JSON body");
    let ids: Vec<&str> = body
        .as_array()
        .expect("an entity array")
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str))
        .collect();
    assert_eq!(ids, vec![standing.as_str()], "an expired instance matches no q filter: {body}");
    scope.finish().await.expect("cleanup failed");
}
