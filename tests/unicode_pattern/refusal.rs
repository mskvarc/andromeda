//! What stays refused beside the case-insensitive option and the shorthand
//! classes: scoping or clearing any other option, a repetition count past
//! IEEE 1003.2's bound, and a repeated shorthand class too large to compile.

use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Only `i` may be scoped to a group or cleared; the other options may only be
/// set for the rest of a pattern, in `q` and in `idPattern` alike.
#[tokio::test]
async fn unicode_pattern_refusal_scoping_or_clearing_another_option_is_a_bad_request() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("RefusedBee");

    for pattern in ["(?m:^a)", "(?s:a.b)", "(?x:a b)", "(?-s)a.b", "(?i-m)a", "(?-ix)a"] {
        let filter = urlencoding::encode(&format!("name~=\"{pattern}\"")).into_owned();
        for query in [
            format!("type={entity_type}&q={filter}"),
            format!("type={entity_type}&idPattern={}", urlencoding::encode(pattern)),
        ] {
            let response = scope
                .client()
                .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
                .await
                .expect("query request");
            assert_eq!(response.status, StatusCode::BAD_REQUEST, "{query}: {:?}", response.body);
            let body = response.json_value().expect("a refusal is a problem detail");
            assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{query}: {body}");
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// IEEE 1003.2 guarantees repetition counts up to 255, and PostgreSQL refuses
/// larger ones with "invalid repetition count(s)", which reached the client as
/// `500 InternalError`. The restatement carries the count unchanged, so the
/// broker refuses it as client input.
#[tokio::test]
async fn unicode_pattern_refusal_a_repetition_count_past_the_posix_bound_is_a_bad_request_not_an_internal_error() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("CountedBee");
    // A query over a type with no Entity never reaches the pattern, so one
    // Entity has to exist for the pre-fix broker to fail inside SQL.
    let entity = json!({"id": scope.id("Counted:1"), "type": entity_type, "name": {"type": "Property", "value": "a"}, "@context": [TEST_SUITE_CONTEXT]});
    let created = scope.client().post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let filter = urlencoding::encode("name~=\"a{256}\"").into_owned();
    for query in [
        format!("type={entity_type}&q={filter}"),
        format!("type={entity_type}&idPattern={}", urlencoding::encode("a{256}")),
    ] {
        let response = scope
            .client()
            .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("query request");
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{query}: {:?}", response.body);
    }
    let admitted = urlencoding::encode("name~=\"^a{0,255}$\"").into_owned();
    let response = scope
        .client()
        .query_entities(&format!("type={entity_type}&q={admitted}"), "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query request");
    assert_eq!(response.status, StatusCode::OK, "255 is within the bound: {:?}", response.body);

    scope.finish().await.expect("cleanup failed");
}

/// `\w` is hundreds of Unicode ranges, and 255 repetitions of it exceed the
/// size the `regex` crate compiles a pattern to. That is the client's pattern
/// being too large, a `400`, and never reaches the store.
#[tokio::test]
async fn unicode_pattern_refusal_a_shorthand_class_repeated_past_the_compiled_size_is_a_bad_request() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("OversizedBee");
    let entity = json!({"id": scope.id("Oversized:1"), "type": entity_type, "name": {"type": "Property", "value": "a"}, "@context": [TEST_SUITE_CONTEXT]});
    let created = scope.client().post_entity(&entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let filter = urlencoding::encode(r#"name~="^\w{255}$""#).into_owned();
    let response = scope
        .client()
        .query_entities(&format!("type={entity_type}&q={filter}"), "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query request");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    let body = response.json_value().expect("a refusal is a problem detail");
    assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// A registration declaring a refused pattern is refused when it is created,
/// so nothing it would store can reach the store's statements.
#[tokio::test]
async fn unicode_pattern_refusal_a_registration_declaring_a_refused_pattern_is_a_bad_request() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let registration = json!({
        "id": scope.id("Registration:refused"),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": scope.entity_type("RefusedHive"), "idPattern": "(?m:^hive)"}]}],
        "endpoint": "http://csource.invalid:1026",
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_registration(&registration).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::BAD_REQUEST, "{:?}", created.body);

    scope.finish().await.expect("cleanup failed");
}
