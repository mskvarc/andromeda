//! Clause 4.9 `RegExp`, end to end.
//!
//! Clause 4.9: "`RegExp` shall be a regular expression as mandated by
//! IEEE 1003.2 [11]", and Table 5.2.8-1 says the same of `idPattern`.
//! IEEE 1003.2 is POSIX.2, whose extended regular expressions both of the
//! broker's engines implement alike — the `regex` crate in memory, PostgreSQL's
//! `~` operator in the store.
//!
//! The defect these cover validated a pattern in one dialect and executed it in
//! the other. `idPattern=\p{L}` compiled in the `regex` crate, reached SQL, and
//! PostgreSQL's refusal surfaced as `500 InternalError` — a malformed pattern
//! is client input and owes the client a `400`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "Building";

/// The Property the pattern operators match against.
const MATCHED_ATTRIBUTE: &str = "name";

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

async fn query(client: &TestClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    let body = response.json_value().expect("query response must be valid JSON");
    (response.status, body)
}

fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have an id member").to_owned())
        .collect()
}

/// Store one entity whose matched Property holds `value`, and return its id.
async fn create_named(scope: &TestEnvironment, client: &TestClient, suffix: &str, value: &str) -> String {
    let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
    create(
        client,
        &json!({
            "id": id,
            "type": FIXTURE_TYPE,
            MATCHED_ATTRIBUTE: {"type": "Property", "value": value},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    id
}

/// A `q` pattern filter over one test's own entities, percent-encoded for the
/// HTTP binding (clause 4.9).
///
/// The candidate set is pinned to `ids` because the suite runs its tests
/// concurrently against one database: an unrestricted filter would match a
/// sibling test's fixtures and assert on their ordering rather than on the
/// pattern under test.
fn pattern_filter(ids: &[&str], pattern: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&q={}",
        id_filter(ids),
        urlencoding::encode(&format!("{MATCHED_ATTRIBUTE}~=\"{pattern}\""))
    )
}

/// The comma-separated `id` list selecting exactly these entities.
fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// The defect, reproduced through the parameter that showed it: a Unicode
/// property class compiles in the `regex` crate and PostgreSQL answers it with
/// "invalid escape \\ sequence". Executing it raised inside SQL, which the
/// broker reported as `500 InternalError`.
#[tokio::test]
async fn regression_query_pattern_a_construct_postgresql_cannot_run_is_a_bad_request_not_an_internal_error() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // Entities have to exist: an empty candidate set never reaches the pattern.
    let id = create_named(&scope, client, "dialect-a", "Mercedes").await;

    let query_str = format!(
        "type={FIXTURE_TYPE}&local=true&id={}&idPattern={}",
        id_filter(&[&id]),
        urlencoding::encode(r"\p{L}")
    );
    let (status, body) = query(client, &query_str).await;
    assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "a malformed pattern is never a broker fault: {body}");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Each of these was executed against PostgreSQL and refused. Reaching SQL with
/// any of them turns a malformed request into a database error.
#[tokio::test]
async fn regression_query_pattern_every_construct_only_one_engine_supports_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "dialect-b", "Mercedes").await;

    for pattern in [r"\p{L}", r"\P{L}", r"a\z", r"\x{61}", "(?i:ab)", "(?u)a", "(?U)a+"] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{pattern} must not be a broker fault: {body}");
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// `\b` compiles in both engines and means two different things — a word
/// boundary to the `regex` crate, a backspace character to PostgreSQL — so one
/// filter selected different entities depending on which engine answered it.
#[tokio::test]
async fn regression_query_pattern_an_assertion_the_two_engines_read_differently_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "dialect-c", "Mercedes").await;

    for pattern in [r"\bMer", r"Mer\Bced"] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{pattern} means two things: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 mandates IEEE 1003.2, so every POSIX.2 extended regular
/// expression has to be accepted and executed. This is the positive control for
/// the refusals above: PostgreSQL accepts each of these, and so does the broker.
#[tokio::test]
async fn regression_query_pattern_every_posix_extended_regular_expression_is_executed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "posix-match", "Mercedes").await;

    for pattern in [
        "^Merc",
        "M.*s$",
        "Merc[e]des",
        "Mer(c|k)edes",
        "e.{2}e",
        "[[:alpha:]]+",
        "[]M]erc",
        "Merc{1}edes",
        "Mer[a-z]+es",
        // A backslash escaping a POSIX metacharacter. Clause 4.9 delegates
        // `RegExp` to IEEE 1003.2 and only `quotedStr` to IETF RFC 8259, so
        // these must reach the pattern engine as written. Reading a `RegExp`
        // as a JSON string refused all four, because none of `\.`, `\(`, `\)`
        // or `\+` is one of RFC 8259's eight escapes.
        r"Merc\.?edes",
        r"^\(?Merc",
        r"Merc\+?edes",
        r"Mercedes\$?",
    ] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::OK, "{pattern} is a POSIX.2 extended regular expression: {body}");
        assert_eq!(ids_from_response(&body), vec![id.clone()], "{pattern} must match the stored value");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The ARE shorthands the two engines share stay available, so narrowing the
/// dialect did not turn away patterns both engines run identically.
#[tokio::test]
async fn regression_query_pattern_a_shorthand_both_engines_share_is_still_executed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "shared-shorthand", "Merc3des").await;

    for pattern in [r"Merc\dde", "(?i)MERC", r"Merc\w"] {
        let (status, body) = query(client, &pattern_filter(&[&id], pattern)).await;
        assert_eq!(status, StatusCode::OK, "{pattern} is common to both engines: {body}");
        assert_eq!(ids_from_response(&body), vec![id.clone()], "{pattern} must match the stored value");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The guard that predates the dialect walk: PostgreSQL answers a repetition
/// applied to another repetition with "quantifier operand invalid", and the
/// broker still refuses it before it reaches SQL.
#[tokio::test]
async fn regression_query_pattern_a_repeated_quantifier_is_still_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "repeated-quantifier", "Mercedes").await;

    let (status, body) = query(client, &pattern_filter(&[&id], "Merc**")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "PostgreSQL rejects a repetition operand: {body}");

    scope.finish().await.expect("cleanup failed");
}
