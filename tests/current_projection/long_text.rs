//! Current projection long strings: every string has one canonical column.
//!
//! `value_string` is keyed by a B-tree, which refuses an entry past 2704
//! bytes, so a String longer than the indexed bound is held in
//! `value_long_text` under the same String value kind. The two length checks
//! make the placement canonical: every string has exactly one column it may
//! sit in, and a write that changes a value's length across the bound moves the
//! clause 4.5.5 grain between the columns rather than adding a second row.

use crate::projection_rows::{rows_for_attribute, string_placement};
use andromeda_postgresql_schema::projected_string_column::INDEXED_STRING_BYTE_LIMIT;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, high_entropy_text::high_entropy_text};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use urlencoding::encode;

/// Length of the long description, well past the indexed bound and the 2 KiB
/// TOAST threshold alike.
const LONG_TEXT_LENGTH: usize = 6000;

/// Migration-owned value-kind code of a String.
const STRING_VALUE_KIND: i16 = 2;

#[tokio::test]
async fn current_projection_long_string_is_stored_in_the_overflow_column() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-text");
    let description = high_entropy_text(LONG_TEXT_LENGTH, 31);

    let entity = json!({
        "id": id,
        "type": "Building",
        "description": {"type": "Property", "value": description},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let rows = rows_for_attribute(scope.database_pool(), &id, "description").await;
    assert_eq!(rows.len(), 1, "the long string is one grain: {rows:?}");
    assert_eq!(rows[0].value_kind, STRING_VALUE_KIND, "a long string keeps the String value kind");
    assert_eq!(
        string_placement(scope.database_pool(), &id, "description").await,
        (None, Some(description)),
        "a string past the indexed bound must sit whole in the overflow column"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_string_at_the_limit_stays_in_the_indexed_column() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-text-boundary");

    // A two-octet character makes the bound an octet count, not a character
    // count: this is half as many characters as the bound has octets.
    let at_limit = "\u{e9}".repeat(INDEXED_STRING_BYTE_LIMIT / 2);
    assert_eq!(at_limit.len(), INDEXED_STRING_BYTE_LIMIT, "the fixture must sit exactly at the bound");
    let past_limit = format!("{at_limit}a");

    let entity = json!({
        "id": id,
        "type": "Building",
        "atLimit": {"type": "Property", "value": at_limit},
        "pastLimit": {"type": "Property", "value": past_limit},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    assert_eq!(
        string_placement(scope.database_pool(), &id, "atLimit").await,
        (Some(at_limit), None),
        "a string of exactly the bound must stay in the indexed column"
    );
    assert_eq!(
        string_placement(scope.database_pool(), &id, "pastLimit").await,
        (None, Some(past_limit)),
        "one octet past the bound must move the string to the overflow column"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_replacing_a_long_string_with_a_short_one_moves_the_grain_between_columns() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-text-move");
    let first_long = high_entropy_text(LONG_TEXT_LENGTH, 32);
    let second_long = high_entropy_text(LONG_TEXT_LENGTH, 33);

    let entity = json!({
        "id": id,
        "type": "Building",
        "description": {"type": "Property", "value": first_long},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // Clause 5.6.19 replaces the attribute whole.
    let replaced = client
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/description", encode(&id)))
        .header("Content-Type", "application/ld+json")
        .json(&json!({"type": "Property", "value": "short", "@context": [TEST_SUITE_CONTEXT]}))
        .send()
        .await
        .expect("replace attribute request failed");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "replace attribute: {:?}", replaced.body);
    assert_eq!(
        string_placement(scope.database_pool(), &id, "description").await,
        (Some("short".to_owned()), None),
        "a short replacement must leave the overflow column empty"
    );
    assert_eq!(
        rows_for_attribute(scope.database_pool(), &id, "description").await.len(),
        1,
        "the replacement must rewrite the one grain, not add one"
    );

    // Clause 5.6.4 updates the value in place.
    let updated = client
        .partial_update_attr(
            &id,
            "description",
            &json!({"type": "Property", "value": second_long, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial attribute update request failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial attribute update: {:?}", updated.body);
    assert_eq!(
        string_placement(scope.database_pool(), &id, "description").await,
        (None, Some(second_long)),
        "a long update must leave the indexed column empty"
    );
    assert_eq!(
        rows_for_attribute(scope.database_pool(), &id, "description").await.len(),
        1,
        "the update must rewrite the one grain, not add one"
    );

    scope.finish().await.expect("cleanup failed");
}
