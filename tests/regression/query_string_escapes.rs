//! Clause 4.9 `quotedStr` and its `char` production, end to end.
//!
//! Clause 4.9 defines `quotedStr = String` and says "String shall be a text string as mandated by the JSON Specification, following the ABNF Grammar, production rule named String, section 7 of IETF RFC 8259" and "char shall be a character as mandated by the JSON Specification, ABNF Grammar, production rule named char, section 7 of IETF RFC 8259". RFC 8259 section 7 admits exactly eight two-character escapes (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`) and the six-character `\uXXXX` form, requires a surrogate pair for a character outside the Basic Multilingual Plane, and forbids an unescaped control character.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "Building";

/// The Property whose string value the filters match against.
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

/// Percent-encode a `q` filter so the HTTP binding delivers it verbatim, over
/// one test's own entities.
///
/// Clause 4.9: "whenever the string acting as a filter is part of the HTTP
/// binding's URI, then it shall be URI-encoded".
///
/// The candidate set is pinned to `ids` because the suite runs its tests
/// concurrently against one database: an unrestricted filter would match a
/// sibling test's fixtures, or one an interrupted run left behind, and assert
/// on those rather than on the escape under test.
fn filter(ids: &[&str], q: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&q={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(q)
    )
}

/// RFC 8259 section 7 admits eight two-character escapes. Each has to reach the
/// stored value as the character it denotes, not as its second character.
#[tokio::test]
async fn regression_query_string_every_rfc8259_two_character_escape_matches_its_character() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    // (escape as written in the filter, the character it denotes)
    let cases = [
        ("\\\"", "\""),
        ("\\\\", "\\"),
        ("\\/", "/"),
        ("\\b", "\u{8}"),
        ("\\f", "\u{c}"),
        ("\\n", "\n"),
        ("\\r", "\r"),
        ("\\t", "\t"),
    ];

    for (index, (escape, character)) in cases.iter().enumerate() {
        let stored = format!("x{character}y");
        let id = create_named(&scope, client, &format!("escape-{index}"), &stored).await;
        let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"x{escape}y\""))).await;
        assert_eq!(status, StatusCode::OK, "escape {escape} must be a well-formed filter: {body}");
        assert_eq!(ids_from_response(&body), vec![id], "escape {escape} must match the character it denotes");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The `\uXXXX` form denotes one code point, so it has to match a stored
/// non-ASCII character rather than the six literal characters of the escape.
#[tokio::test]
async fn regression_query_string_a_unicode_escape_matches_a_non_ascii_stored_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "unicode-escape", "aé").await;

    let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"a\\u00e9\""))).await;
    assert_eq!(status, StatusCode::OK, "a \\uXXXX escape is a well-formed filter: {body}");
    assert_eq!(ids_from_response(&body), vec![id.clone()], "\\u00e9 must match the stored 'é'");

    // The escape must denote U+00E9, not the literal text `au00e9`.
    let literal = create_named(&scope, client, "unicode-literal", "au00e9").await;
    let (status, body) = query(client, &filter(&[&id, &literal], &format!("{MATCHED_ATTRIBUTE}==\"a\\u00e9\""))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let matched = ids_from_response(&body);
    assert!(matched.contains(&id), "the escape still matches the character it denotes");
    assert!(!matched.contains(&literal), "the escape must not match the letters it is written with");

    scope.finish().await.expect("cleanup failed");
}

/// RFC 8259 section 7: a character outside the Basic Multilingual Plane "may be
/// represented as a twelve-character sequence, encoding the UTF-16 surrogate
/// pair". The pair denotes one code point, not two.
#[tokio::test]
async fn regression_query_string_a_surrogate_pair_matches_one_supplementary_character() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // U+1D11E MUSICAL SYMBOL G CLEF, written 𝄞.
    let id = create_named(&scope, client, "surrogate-pair", "clef-\u{1d11e}").await;

    let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"clef-\\ud834\\udd1e\""))).await;
    assert_eq!(status, StatusCode::OK, "a surrogate pair is a well-formed filter: {body}");
    assert_eq!(ids_from_response(&body), vec![id], "the pair denotes one code point");

    scope.finish().await.expect("cleanup failed");
}

/// A malformed escape is outside the clause 4.9 `String` production, so the
/// filter is not a well-formed Query and the request is refused. Accepting one
/// as data made a typo silently match nothing, which reads as an empty result.
#[tokio::test]
async fn regression_query_string_a_malformed_escape_is_refused_rather_than_read_as_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "malformed-escape", "anything").await;

    // A `\p` is not one of the eight escapes; `\u00` is a truncated `\uXXXX`;
    // `\uZZZZ` carries non-hexadecimal digits; a lone high surrogate has no
    // trailing surrogate to pair with, and a lone low surrogate no leading one.
    for malformed in ["\\p", "\\u00", "\\uZZZZ", "\\ud834", "\\udd1e", "\\ud834\\u0041"] {
        let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"a{malformed}\""))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{malformed} is outside RFC 8259 section 7: {body}");
        assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{malformed}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// RFC 8259 section 7 puts the unescaped range at `%x20-21 / %x23-5B / %x5D-10FFFF`,
/// so a raw control character below U+0020 has to be escaped and is otherwise
/// not a `char` at all.
#[tokio::test]
async fn regression_query_string_a_raw_control_character_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "raw-control", "anything").await;

    for control in ['\u{0}', '\u{1}', '\n', '\t', '\u{1f}'] {
        let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"a{control}b\""))).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "U+{:04X} must be escaped to appear in a quoted string: {body}",
            u32::from(control)
        );
        assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "U+{:04X}: {body}", u32::from(control));
    }

    scope.finish().await.expect("cleanup failed");
}

/// The positive control for the rejections above: the same filter shape with a
/// well-formed escape is accepted, so the refusals are attributable to the
/// escape alone rather than to the surrounding Query Term.
#[tokio::test]
async fn regression_query_string_a_well_formed_escape_in_the_same_shape_is_accepted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = create_named(&scope, client, "control-group", "a\tb").await;

    let (status, body) = query(client, &filter(&[&id], &format!("{MATCHED_ATTRIBUTE}==\"a\\tb\""))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![id]);

    scope.finish().await.expect("cleanup failed");
}
