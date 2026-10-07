//! Clause 4.23.3 `orderBy` attribute paths, end to end.
//!
//! Clause 4.23.3 gives `OrderingTerm = AttrName *1(DirectionTerm)
//! *(thenOp OrderingTerm)` and says "See clause 4.9 for the definition of
//! `AttrName`". Its own examples settle what that reaches: EXAMPLE 4
//! `?orderBy=address[city]` "applies sort ordering using a sub-item within a
//! Property Value defined as a complex JSON object", and EXAMPLE 5
//! `?orderBy=name.observedAt` "applies sort ordering based on an attribute path
//! that is defined by the production rule Attribute, as a dot-separated list of
//! names". Both are the clause 4.9 `ValuePath` production,
//! `DottedPath *1([DottedPath])`.
//!
//! Both `.` and `[` are admitted in an ordering member, so both of the clause's
//! own examples are served rather than refused with `400`.
//!
//! Every query carries `local=true`: clause 4.23.1 forbids ordering on
//! distributed operations and the broker enforces it, so without it these
//! assertions would pass for the wrong reason.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "OrderPath";

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

/// Restrict a query to this test's own entities and order it by `order_by`.
///
/// The candidate set is pinned to `ids` because the suite runs its tests
/// concurrently against one database: an unrestricted query would order a
/// sibling test's fixtures alongside these and assert on the wrong sequence.
fn ordered(ids: &[String], order_by: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&orderBy={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(order_by)
    )
}

/// Three entities exercising every axis clause 4.23.3 reaches, each ordering
/// them differently so a key that resolved nothing cannot pass by accident.
///
/// Returns the ids in creation order.
async fn create_path_fixtures(scope: &TestEnvironment, client: &TestClient) -> Vec<String> {
    // (suffix, address.city, name.observedAt, blob.k, label@en, rawdata.particulate)
    let fixtures = [
        ("a", "Amsterdam", "2024-03-01T00:00:00Z", "alpha", "zebra", "gamma"),
        ("b", "Berlin", "2024-01-01T00:00:00Z", "charlie", "apple", "alpha"),
        ("c", "Copenhagen", "2024-02-01T00:00:00Z", "bravo", "mango", "beta"),
    ];
    let mut ids = Vec::with_capacity(fixtures.len());
    for (suffix, city, observed_at, raw_key, english, particulate) in fixtures {
        let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
        create(
            client,
            &json!({
                "id": id,
                "type": FIXTURE_TYPE,
                "address": {"type": "Property", "value": {"city": city, "street": "Ulrich Strasse"}},
                "name": {"type": "Property", "value": format!("name-{suffix}"), "observedAt": observed_at},
                "blob": {"type": "JsonProperty", "json": {"k": raw_key}},
                "label": {"type": "LanguageProperty", "languageMap": {"en": english, "de": "zzz"}},
                "sensor": {
                    "type": "Property",
                    "value": 40,
                    "rawdata": {"type": "Property", "value": {"particulate": particulate}}
                },
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        ids.push(id);
    }
    ids
}

/// Clause 4.23.3 EXAMPLE 4: `?orderBy=address[city]` sorts by a sub-item within
/// a Property value defined as a complex JSON object.
#[tokio::test]
async fn regression_ordering_path_a_compound_value_member_orders_the_result() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "address[city]")).await;
    assert_eq!(status, StatusCode::OK, "clause 4.23.3 EXAMPLE 4 is a well-formed orderBy: {body}");
    // Amsterdam, Berlin, Copenhagen.
    assert_eq!(ids_from_response(&body), vec![ids[0].clone(), ids[1].clone(), ids[2].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.3 EXAMPLE 5: `?orderBy=name.observedAt` sorts by an attribute
/// path "defined by the production rule Attribute, as a dot-separated list of
/// names".
#[tokio::test]
async fn regression_ordering_path_a_sub_attribute_orders_the_result() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "name.observedAt")).await;
    assert_eq!(status, StatusCode::OK, "clause 4.23.3 EXAMPLE 5 is a well-formed orderBy: {body}");
    // 2024-01 (b), 2024-02 (c), 2024-03 (a).
    assert_eq!(ids_from_response(&body), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 EXAMPLE 10 shows the two axes combined,
/// `sensor.rawdata[airquality.particulate]`: a dotted list followed by one
/// bracketed group.
#[tokio::test]
async fn regression_ordering_path_a_sub_attribute_and_a_compound_member_combine() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "sensor.rawdata[particulate]")).await;
    assert_eq!(status, StatusCode::OK, "the two clause 4.9 axes combine: {body}");
    // alpha (b), beta (c), gamma (a).
    assert_eq!(ids_from_response(&body), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// A `JsonProperty`'s value is opaque JSON whose keys are never expanded
/// through the `@context`, so the member is addressed by the literal text the
/// client wrote.
#[tokio::test]
async fn regression_ordering_path_a_json_property_member_orders_by_its_literal_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "blob[k]")).await;
    assert_eq!(status, StatusCode::OK, "a raw JSON key is addressable: {body}");
    // alpha (a), bravo (c), charlie (b).
    assert_eq!(ids_from_response(&body), vec![ids[0].clone(), ids[2].clone(), ids[1].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9's second reading of `attr[member]`: "If the target element is a
/// `LanguageProperty` and a target language is specified, the target value is
/// defined as the Value associated to the matching key-value pair held within
/// the languageMap".
#[tokio::test]
async fn regression_ordering_path_a_target_language_orders_by_that_languages_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "label[en]")).await;
    assert_eq!(status, StatusCode::OK, "a target language is addressable: {body}");
    // apple (b), mango (c), zebra (a) — the German values would order a, b, c.
    assert_eq!(ids_from_response(&body), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.3 `DirectionTerm` applies to the whole path, not to its last
/// segment.
#[tokio::test]
async fn regression_ordering_path_a_direction_reverses_a_path_ordering() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "address[city];desc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![ids[2].clone(), ids[1].clone(), ids[0].clone()]);

    let (status, body) = query(client, &ordered(&ids, "name.observedAt;desc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![ids[0].clone(), ids[2].clone(), ids[1].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.3 `thenOp`: elements separated by a comma "shall be applied
/// sequentially", and a path is an element like any other.
#[tokio::test]
async fn regression_ordering_path_multiple_terms_apply_sequentially() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    // Every entity carries the same `sensor` value, so the first key ties for
    // all three and the second key decides the whole sequence.
    let (status, body) = query(client, &ordered(&ids, "sensor,name.observedAt")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![ids[1].clone(), ids[2].clone(), ids[0].clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// The clause 4.9 `LinkedEntityRelation` axis crosses an entity boundary.
/// Clause 4.23.3's grammar is `AttrName` based and its prose names only the
/// dotted and bracketed paths, so `rel{…}` is not an ordering member.
#[tokio::test]
async fn regression_ordering_path_a_linked_entity_traversal_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    for rejected in ["rel{name}", "rel{Building,Store:name}"] {
        let (status, body) = query(client, &ordered(&ids, rejected)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected} crosses an entity boundary: {body}");
        assert_eq!(body["type"], error::BAD_REQUEST_DATA.as_str(), "{rejected}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A malformed ordering member is refused, and never with `500`: a query
/// parameter is client input whatever it contains.
#[tokio::test]
async fn regression_ordering_path_a_malformed_member_is_a_bad_request_not_an_internal_error() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    // A dangling separator, an empty bracket group, a second bracket group, a
    // trailing dotted segment after one, and the `[*]` marker clause 4.9 gives
    // to `q` alone.
    for rejected in ["a.", "a[]", "a[b][c]", "a[b].c", "name[*]", "ex:address[city]["] {
        let (status, body) = query(client, &ordered(&ids, rejected)).await;
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{rejected} is client input: {body}");
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A path deeper than the broker's budget is refused rather than compiled into
/// a jsonpath that grows with its depth.
#[tokio::test]
async fn regression_ordering_path_a_path_beyond_the_depth_cap_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    // The cap mirrors the `q` engine's, which is 16 segments.
    let at_limit = format!("a{}", ".s".repeat(16));
    let (status, body) = query(client, &ordered(&ids, &at_limit)).await;
    assert_eq!(status, StatusCode::OK, "a path at the limit is admissible: {body}");

    let beyond = format!("a{}", ".s".repeat(17));
    let (status, body) = query(client, &ordered(&ids, &beyond)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a path beyond the limit is refused: {body}");

    scope.finish().await.expect("cleanup failed");
}

/// The clause 4.23 keywords name structural members and are spelled as bare
/// names, so admitting paths does not turn `id` into an attribute.
#[tokio::test]
async fn regression_ordering_path_the_structural_keywords_still_order_their_members() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_path_fixtures(&scope, client).await;

    let (status, body) = query(client, &ordered(&ids, "id;desc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![ids[2].clone(), ids[1].clone(), ids[0].clone()]);

    scope.finish().await.expect("cleanup failed");
}
