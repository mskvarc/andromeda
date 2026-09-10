//! Regression tests for entity ordering and its collation (ETSI GS CIM 009
//! v1.9.1 clauses 4.23.1, 4.23.2, 4.23.3, 5.7.2.4).
//!
//! Every query here carries `local=true`. Clause 5.7.2.4 states that "if the
//! ordering parameter is present and the execution of the operation is not
//! limited to the local scope (see clause 5.5.13) then an error of type
//! `BadRequestData` shall be raised", so an `orderBy` request that omits it is
//! one the broker is required to reject and no ordering assertion could be made
//! about it.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture here carries.
const FIXTURE_TYPE: &str = "Building";

/// The attribute every ordering under test sorts on.
const ORDERED_ATTRIBUTE: &str = "name";

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

fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// The RFC 7807 `detail` member of a Problem Details body (clause 6.3.2).
fn detail_of(body: &Value) -> &str {
    body["detail"].as_str().expect("a problem detail body must carry a detail member")
}

/// Asserts a rejection is the one the test is about and not the clause 5.7.2.4
/// local-scope refusal standing in for it.
fn assert_not_the_local_scope_refusal(body: &Value) {
    assert_eq!(
        body["type"].as_str().expect("a problem detail body must carry a type member"),
        error::BAD_REQUEST_DATA.as_str()
    );
    assert!(
        !detail_of(body).contains("local scope"),
        "the rejection must be the one under test, not the clause 5.7.2.4 local-scope refusal: {}",
        detail_of(body)
    );
}

/// Create one entity per `(local id suffix, name value)` pair and return the
/// identifiers in the order they were given.
async fn create_named(scope: &TestEnvironment, client: &TestClient, fixtures: &[(&str, Value)]) -> Vec<String> {
    let mut ids = Vec::with_capacity(fixtures.len());
    for (suffix, value) in fixtures {
        let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
        create(
            client,
            &json!({
                "id": id,
                "type": FIXTURE_TYPE,
                ORDERED_ATTRIBUTE: {"type": "Property", "value": value},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
        ids.push(id);
    }
    ids
}

/// The three German surnames that separate a phonebook collation from the ICU
/// root one: root sorts `ä` immediately after `a`, phonebook sorts it as `ae`
/// and so ahead of `af`.
fn german_surnames() -> [(&'static str, Value); 3] {
    [
        ("collation-schaefer", json!("Schäfer")),
        ("collation-schafer", json!("Schafer")),
        ("collation-schaffer", json!("Schaffer")),
    ]
}

/// Clause 4.23.1: "for strings the default collation order shall be defined as
/// using ICU \"root\" collation order (\"und-x-icu\")". The identifier the
/// specification itself names must be accepted and applied.
#[tokio::test]
async fn regression_ordering_the_icu_root_collation_identifier_is_accepted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_named(&scope, client, &german_surnames()).await;
    let (umlaut, plain, doubled_f) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let filter = id_filter(&[&umlaut, &plain, &doubled_f]);
    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}&collation=und-x-icu"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "und-x-icu is the collation clause 4.23.1 names: {body}");
    assert_eq!(ids_from_response(&body), vec![plain, umlaut, doubled_f]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.3 EXAMPLE 6: `?orderBy=name&collation=und-u-ks-identic`. The
/// collation the specification demonstrates has to reach the store and order
/// the result, not fail the request.
#[tokio::test]
async fn regression_ordering_the_identical_strength_collation_example_orders_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_named(&scope, client, &german_surnames()).await;
    let (umlaut, plain, doubled_f) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let filter = id_filter(&[&umlaut, &plain, &doubled_f]);
    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}&collation=und-u-ks-identic"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "clause 4.23.3 EXAMPLE 6 must be answered, not refused: {body}");
    // Identical strength only breaks ties the root collation already resolves,
    // so the sequence is the root one; what is under test is that the collation
    // is provisioned and applied rather than aborting the statement.
    assert_eq!(ids_from_response(&body), vec![plain, umlaut, doubled_f]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.3 EXAMPLE 7: `?orderBy=name&collation=de-u-co-phonebk` sorts
/// "according to German phonebook collation ordering", which compares `ä` as
/// `ae` and so ranks `Schäfer` ahead of `Schafer` — the opposite of the ICU
/// root order.
#[tokio::test]
async fn regression_ordering_the_german_phonebook_collation_example_orders_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_named(&scope, client, &german_surnames()).await;
    let (umlaut, plain, doubled_f) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let filter = id_filter(&[&umlaut, &plain, &doubled_f]);
    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}&collation=de-u-co-phonebk"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "clause 4.23.3 EXAMPLE 7 must be answered, not refused: {body}");
    assert_eq!(ids_from_response(&body), vec![umlaut, plain, doubled_f]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: "If a preferred collation setting is present and it does not
/// conform to a valid ICU collation (see IETF RFC 6067) then an error of type
/// `BadRequestData` shall be raised." A Unicode keyword that selects a
/// numbering system states no collation preference, and text that is no
/// language tag at all names nothing.
#[tokio::test]
async fn regression_ordering_an_invalid_collation_is_rejected_as_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for invalid in ["de-u-nu-latn", "not%20a%20collation", "und-u-zz-nonsense"] {
        let (status, body) = query(
            client,
            &format!("type={FIXTURE_TYPE}&local=true&orderBy={ORDERED_ATTRIBUTE}&collation={invalid}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid} must be refused: {body}");
        assert_not_the_local_scope_refusal(&body);
    }

    // The identical request with a valid collation is accepted, so each
    // rejection above is the collation identifier and nothing else about the
    // URL — and no input reaches the store as a failing statement.
    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&local=true&orderBy={ORDERED_ATTRIBUTE}&collation=und-x-icu"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.1: with no `collation` supplied, strings sort under the ICU
/// "root" collation order, which places `Schäfer` between `Schafer` and
/// `Schaffer`. A byte-wise comparison would place it after both.
#[tokio::test]
async fn regression_ordering_a_string_sort_without_a_collation_uses_icu_root_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_named(&scope, client, &german_surnames()).await;
    let (umlaut, plain, doubled_f) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let filter = id_filter(&[&umlaut, &plain, &doubled_f]);
    let (status, body) = query(client, &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids_from_response(&body),
        vec![plain, umlaut, doubled_f],
        "the default collation is ICU root, not the database's byte-wise order"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.2: "When sorting by value, and the values of Attributes are of
/// mixed datatypes, the following comparison order (null values last), shall be
/// implicitly applied: Numbers, Strings, Object, Array, Boolean, Time, Date,
/// `DateTime`, Null, Attribute does not exist."
#[tokio::test]
async fn regression_ordering_mixed_datatypes_follow_the_clause_4_23_2_comparison_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids = create_named(
        &scope,
        client,
        &[
            ("datatype-number", json!(42)),
            ("datatype-string", json!("text")),
            ("datatype-object", json!({"nested": 1})),
            ("datatype-array", json!([1, 2, 3])),
            ("datatype-boolean", json!(true)),
            ("datatype-time", json!({"@type": "Time", "@value": "10:00:00Z"})),
            ("datatype-date", json!({"@type": "Date", "@value": "2020-01-01"})),
            ("datatype-datetime", json!({"@type": "DateTime", "@value": "2020-01-01T00:00:00Z"})),
        ],
    )
    .await;

    // An NGSI-LD Null: a JSON literal whose value is null, which JSON-LD keeps
    // rather than dropping during expansion.
    let null_valued = scope.id(&format!("{FIXTURE_TYPE}:datatype-null"));
    create(
        client,
        &json!({
            "id": null_valued,
            "type": FIXTURE_TYPE,
            ORDERED_ATTRIBUTE: {"type": "JsonProperty", "json": Value::Null},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let absent = scope.id(&format!("{FIXTURE_TYPE}:datatype-absent"));
    create(
        client,
        &json!({
            "id": absent,
            "type": FIXTURE_TYPE,
            "unrelated": {"type": "Property", "value": 1},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let mut expected = ids.clone();
    expected.push(null_valued.clone());
    expected.push(absent.clone());

    let borrowed: Vec<&str> = expected.iter().map(String::as_str).collect();
    let filter = id_filter(&borrowed);
    let (status, body) = query(client, &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), expected);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.2 puts "Attribute does not exist" at the end of the comparison
/// order and pins null values last, which is a statement about the order
/// itself: reversing the sort reverses the datatypes, not the position of the
/// entities that carry no such attribute.
#[tokio::test]
async fn regression_ordering_a_missing_attribute_sorts_last_in_both_directions() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_named(&scope, client, &[("absent-a", json!("alpha")), ("absent-b", json!("beta"))]).await;
    let (alpha, beta) = (ids[0].clone(), ids[1].clone());

    let absent = scope.id(&format!("{FIXTURE_TYPE}:absent-none"));
    create(
        client,
        &json!({
            "id": absent,
            "type": FIXTURE_TYPE,
            "unrelated": {"type": "Property", "value": 1},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let filter = id_filter(&[&alpha, &beta, &absent]);

    let (status, body) = query(client, &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![alpha.clone(), beta.clone(), absent.clone()]);

    let (status, body) = query(client, &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy={ORDERED_ATTRIBUTE};desc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids_from_response(&body),
        vec![beta, alpha, absent],
        "an entity without the attribute sorts last in both directions"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.2: "when sorting by distance, the following comparison order
/// shall be applied: Attribute is a `GeoProperty`; Attribute is not a
/// `GeoProperty` - sorted by value as shown above."
#[tokio::test]
async fn regression_ordering_by_distance_ranks_geoproperties_before_other_attributes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let near = scope.id(&format!("{FIXTURE_TYPE}:distance-near"));
    let far = scope.id(&format!("{FIXTURE_TYPE}:distance-far"));
    let not_geo = scope.id(&format!("{FIXTURE_TYPE}:distance-not-geo"));
    let absent = scope.id(&format!("{FIXTURE_TYPE}:distance-absent"));

    for (id, longitude, latitude) in [(&near, 13.3986_f64, 52.5547_f64), (&far, 2.3522, 48.8566)] {
        create(
            client,
            &json!({
                "id": id,
                "type": FIXTURE_TYPE,
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, latitude]}},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }
    create(
        client,
        &json!({
            "id": not_geo,
            "type": FIXTURE_TYPE,
            "location": {"type": "Property", "value": "somewhere"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    create(
        client,
        &json!({
            "id": absent,
            "type": FIXTURE_TYPE,
            "unrelated": {"type": "Property", "value": 1},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let filter = id_filter(&[&near, &far, &not_geo, &absent]);
    let reference = urlencoding::encode("[13.3986,52.5547]").into_owned();

    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy=location;dist-asc&orderFrom={reference}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![near.clone(), far.clone(), not_geo.clone(), absent.clone()]);

    // Reversing the distance reverses the entities that have one; it does not
    // promote the entities whose attribute is not a GeoProperty.
    let (status, body) = query(
        client,
        &format!("type={FIXTURE_TYPE}&id={filter}&local=true&orderBy=location;dist-desc&orderFrom={reference}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![far, near, not_geo, absent]);
    scope.finish().await.expect("cleanup failed");
}
