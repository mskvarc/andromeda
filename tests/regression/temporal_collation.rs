//! Regression tests for the `collation` and `orderBy` parameters of "Query
//! Temporal Evolution of Entities" (ETSI GS CIM 009 v1.9.1 clauses 4.23.1,
//! 5.7.4.3, 5.7.4.4, 5.7.4.5 and Table 6.18.3.2-1).
//!
//! Both parameters must reach the statement: `ORDER BY id ASC` under the
//! column's own byte-wise `COLLATE "C"` is not an answer to them. Table
//! 6.18.3.2-1 says of `collation` that "the Entities returned in the payload
//! shall be ordered according to the collation given", and clause 5.7.4.5 that
//! "if Entity ordering is specified (see clause 4.23), then the JSON-LD array
//! returned shall be ordered according to the member names specified", so
//! neither can be answered by ignoring it.
//!
//! Every ordering query here carries local scope, which keeps each assertion
//! about the collation alone. Clause 5.7.4.4 raises `BadRequestData` when "the
//! ordering parameter is present and the execution of the operation is not
//! limited to the local scope (see clause 5.5.13)"; that condition is decided by
//! whether a registered Context Source takes part in the answer.
//!
//! The discriminating collation here is numeric ordering (`und-u-kn-true`,
//! RFC 6067 keyword `kn`) rather than the German phonebook order the entity
//! suite uses. Clause 5.7.4.3 admits only the Entity member `id`, and an NGSI-LD
//! identifier is a URN or a URL (clause 4.6.2) and so carries no umlaut for a
//! phonebook collation to reorder; a numeric collation reorders pure ASCII, and
//! it reorders it away from the byte order the identifier column declares.
//! `collation2` before `collation10` is an order that no byte-wise comparison
//! and no ICU root collation produces, so each assertion below fails unless the
//! collation genuinely reached the statement.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The entity type every fixture here carries.
const FIXTURE_TYPE: &str = "CollationReading";

/// The attribute every fixture records one instance of.
const FIXTURE_ATTRIBUTE: &str = "reading";

/// The instant every fixture's one instance is observed at.
const OBSERVED_AT: &str = "2024-01-01T00:00:00Z";

/// A temporal window that covers [`OBSERVED_AT`].
const WHOLE_WINDOW: &str = "timerel=after&timeAt=2020-01-01T00:00:00Z";

/// The ICU collation that orders digit runs by value (IETF RFC 6067 keyword
/// `kn`), which is the one order neither the byte-wise column collation nor the
/// clause 4.23.1 ICU root default produces.
const NUMERIC_COLLATION: &str = "und-u-kn-true";

/// The local suffixes whose relative order separates a numeric collation from a
/// byte-wise one: `2` sorts before `10` numerically and after it byte-wise.
const NUMBERED_SUFFIXES: [&str; 3] = ["collation2", "collation10", "collation30"];

/// The local suffixes whose relative order separates the clause 4.23.1 ICU root
/// collation from the identifier column's byte-wise one: ICU root treats case as
/// a tertiary difference and puts the lowercase letter first, so `aB` precedes
/// `Ab`, while byte-wise `A` (0x41) precedes `a` (0x61) and the pair is reversed.
const CASED_SUFFIXES: [&str; 2] = ["collationAb", "collationaB"];

/// The three fixture identifiers, in the order [`NUMBERED_SUFFIXES`] names them.
async fn create_numbered(scope: &TestEnvironment, client: &TestClient) -> Vec<String> {
    create_fixtures(scope, client, &NUMBERED_SUFFIXES).await
}

/// The two fixture identifiers, in the order [`CASED_SUFFIXES`] names them.
async fn create_cased(scope: &TestEnvironment, client: &TestClient) -> Vec<String> {
    create_fixtures(scope, client, &CASED_SUFFIXES).await
}

/// One temporal entity per suffix, in the order the suffixes are given.
async fn create_fixtures(scope: &TestEnvironment, client: &TestClient, suffixes: &[&str]) -> Vec<String> {
    let mut ids = Vec::with_capacity(suffixes.len());
    for suffix in suffixes {
        let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
        let response = client
            .create_temporal_entity(&json!({
                "id": id,
                "type": FIXTURE_TYPE,
                FIXTURE_ATTRIBUTE: [{"type": "Property", "value": 1, "observedAt": OBSERVED_AT}],
                "@context": CORE_CONTEXT
            }))
            .await
            .expect("create temporal entity failed");
        assert_eq!(response.status, StatusCode::CREATED, "create temporal entity: {:?}", response.body);
        ids.push(id);
    }
    ids
}

/// `GET /temporal/entities` with the core context.
async fn get_query(client: &TestClient, query: &str) -> (StatusCode, Value) {
    let response = client
        .query_temporal_entities(query, Some(CORE_CONTEXT))
        .await
        .expect("query_temporal_entities failed");
    let body = response.json_value().expect("temporal query response must be valid JSON");
    (response.status, body)
}

/// `POST /temporal/entityOperations/query`, with the URL half of the query the
/// clause 5.2.23 body has no member for.
async fn post_query(client: &TestClient, url_query: &str, body: &Value) -> (StatusCode, Value) {
    let response = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entityOperations/query?{url_query}"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("valid JSON-LD Link header"))
        .json(body)
        .send()
        .await
        .expect("post temporal query failed");
    let body = response.json_value().expect("temporal query response must be valid JSON");
    (response.status, body)
}

/// The clause 5.2.23 body that selects the three fixtures over the whole window.
fn query_body(ids: &[String], ordering: &Value) -> Value {
    json!({
        "type": "Query",
        "entities": [{"type": FIXTURE_TYPE, "id": ids}],
        "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"},
        "ordering": ordering
    })
}

/// The `id` member of every entity in an array body, in response order.
fn ids_from_response(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a temporal query answers with a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("every entity carries an id member").to_owned())
        .collect()
}

/// The `id=` selector that narrows a query to exactly the fixtures created.
fn id_filter(ids: &[String]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// Assert a refusal is the clause 6.3.2 Problem Details body for
/// `BadRequestData`, and not some other 400 the URL happens to earn.
fn assert_bad_request_data(status: StatusCode, body: &Value, context: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{context}: {body}");
    assert_eq!(
        body["type"].as_str().expect("a problem detail body carries a type member"),
        error::BAD_REQUEST_DATA.as_str(),
        "{context}: {body}"
    );
}

/// Table 6.18.3.2-1, `collation`: "An ICU collation (see IETF RFC 6067 [36]),
/// When defined, the Entities returned in the payload shall be ordered
/// according to the collation given."
///
/// Ordering by the raw `temporal_entities.id` column, which is declared
/// `COLLATE "C"`, puts `collation10` first, so this sequence is unreachable
/// unless the collation reaches the `ORDER BY`.
#[tokio::test]
async fn regression_temporal_collation_a_get_query_orders_identifiers_under_the_collation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;
    let (two, ten, thirty) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let (status, body) = get_query(
        client,
        &format!(
            "{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={filter}&local=true&orderBy=id&collation={NUMERIC_COLLATION}",
            filter = id_filter(&ids)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![two, ten, thirty]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23.1: "for strings the default collation order shall be defined as
/// using ICU \"root\" collation order (\"und-x-icu\")". An `orderBy` that names
/// no collation is therefore not the same as no ordering at all: it orders under
/// the root collation, which is a different comparison from the `COLLATE "C"`
/// the identifier column declares.
///
/// The pair of identifiers here is what separates the two. ICU root makes case a
/// tertiary difference and puts the lowercase letter first, so `…collationaB`
/// precedes `…collationAb`; byte-wise the uppercase `A` (0x41) precedes the
/// lowercase `a` (0x61) and the pair comes back the other way round. A default
/// that fell through to the column's own collation would produce the reverse of
/// this sequence.
#[tokio::test]
async fn regression_temporal_collation_a_get_query_without_a_collation_keeps_the_root_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_cased(&scope, client).await;
    let (upper_first, lower_first) = (ids[0].clone(), ids[1].clone());

    let (status, body) = get_query(
        client,
        &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={filter}&local=true&orderBy=id", filter = id_filter(&ids)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        ids_from_response(&body),
        vec![lower_first, upper_first],
        "the ICU root collation puts the lowercase letter first; the byte-wise column collation would not"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-1, `orderBy`: "The Entity member ("id") appended with an
/// optional sorting style ("asc", "desc") as per clause 4.23. When defined, the
/// Entities returned in the payload shall be ordered according to members
/// defined."
///
/// The descending sequence is the reverse of the collated ascending one, which
/// is a different sequence from the reverse of the byte-wise one: an ordering
/// that dropped either the direction or the collation produces neither.
#[tokio::test]
async fn regression_temporal_collation_a_descending_order_by_reverses_the_collated_sequence() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;
    let (two, ten, thirty) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let (status, body) = get_query(
        client,
        &format!(
            "{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={filter}&local=true&orderBy=id;desc&collation={NUMERIC_COLLATION}",
            filter = id_filter(&ids)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![thirty, ten, two]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.24.1: the POST binding's "behaviour ... mirrors the one in clause
/// 6.18.3.2", so the collation the body carries orders the payload exactly as
/// the query string's does.
#[tokio::test]
async fn regression_temporal_collation_a_post_query_orders_identifiers_under_the_collation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;
    let (two, ten, thirty) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let (status, body) = post_query(
        client,
        "local=true",
        &query_body(&ids, &json!({"orderBy": "id", "collation": NUMERIC_COLLATION})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![two.clone(), ten.clone(), thirty.clone()]);

    // The same control the query-string binding gets: without the collation the
    // sequence is the root one, so the body's collation is what reordered it.
    let (status, body) = post_query(client, "local=true", &query_body(&ids, &json!({"orderBy": "id"}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), vec![ten, two, thirty]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4: "If a preferred collation setting is present and it does not
/// conform to a valid ICU collation (see IETF RFC 6067 [36]) then an error of
/// type `BadRequestData` shall be raised." A Unicode keyword that selects a
/// numbering system or a calendar states no collation preference, and text that
/// is no language tag at all names nothing.
#[tokio::test]
async fn regression_temporal_collation_an_invalid_collation_is_bad_request_data_on_the_get_binding() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for invalid in [
        "de-u-nu-latn",
        "de-u-ca-buddhist",
        "not%20a%20collation",
        "und-u-zz-nonsense",
        // IETF RFC 6067 registers the keyword types as well as the keys, and
        // ICU accepts an unregistered collation type in silence, sorting by the
        // key's default. Neither PostgreSQL nor ICU refuses this one at any
        // validation level, so only the broker can.
        "de-u-co-nonsense",
        "de-u-ks-zzz",
        // A well-formed BCP47 tag whose language ICU does not know. It names no
        // collation, and creating one for it would silently sort by the root
        // order instead of answering the request.
        "zz-ZZ",
    ] {
        let (status, body) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&local=true&orderBy=id&collation={invalid}")).await;
        assert_bad_request_data(status, &body, invalid);
    }

    // The identical request with a valid collation is answered, so each
    // rejection above is the collation identifier and nothing else about the URL.
    let (status, body) = get_query(
        client,
        &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&local=true&orderBy=id&collation={NUMERIC_COLLATION}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    scope.finish().await.expect("cleanup failed");
}

/// The same clause 5.7.4.4 rule on the POST binding, which reads the collation
/// out of the clause 5.2.23 body rather than out of the URL.
#[tokio::test]
async fn regression_temporal_collation_an_invalid_collation_is_bad_request_data_on_the_post_binding() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;

    for invalid in [
        "de-u-nu-latn",
        "de-u-ca-buddhist",
        "not a collation",
        "und-u-zz-nonsense",
        "de-u-co-nonsense",
        "de-u-ks-zzz",
        "zz-ZZ",
    ] {
        let (status, body) = post_query(client, "local=true", &query_body(&ids, &json!({"orderBy": "id", "collation": invalid}))).await;
        assert_bad_request_data(status, &body, invalid);
    }

    let (status, body) = post_query(
        client,
        "local=true",
        &query_body(&ids, &json!({"orderBy": "id", "collation": NUMERIC_COLLATION})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-1 makes `collation` "part of Entity ordering", so a collation
/// with no `orderBy` orders nothing: the answer is the one the same query
/// without it gives, and it needs no local scope because no ordering parameter
/// is present for clause 5.7.4.4 to hold to one.
#[tokio::test]
async fn regression_temporal_collation_a_collation_without_an_order_by_changes_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;
    let (two, ten, thirty) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    let (status, collated) = get_query(
        client,
        &format!(
            "{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={filter}&collation={NUMERIC_COLLATION}",
            filter = id_filter(&ids)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{collated}");

    let (status, plain) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={filter}", filter = id_filter(&ids))).await;
    assert_eq!(status, StatusCode::OK, "{plain}");

    assert_eq!(ids_from_response(&collated), ids_from_response(&plain));
    assert_eq!(ids_from_response(&collated), vec![ten, two, thirty], "the broker's own stable order stands");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.3 lists among the operation's inputs "A preferred ordering of
/// Entities retrieved - only the Entity member name "id" can be used", and
/// clause 5.7.4.4: "If the ordering parameter is present and refers an entity
/// name other than "id", then an error of type `BadRequestData` shall be
/// raised."
#[tokio::test]
async fn regression_temporal_collation_an_order_by_other_than_id_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;

    for member in ["type", "scope", FIXTURE_ATTRIBUTE] {
        let (status, body) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&local=true&orderBy={member}")).await;
        assert_bad_request_data(status, &body, member);

        let (status, body) = post_query(client, "local=true", &query_body(&ids, &json!({"orderBy": member}))).await;
        assert_bad_request_data(status, &body, member);
    }
    scope.finish().await.expect("cleanup failed");
}

/// Table 6.18.3.2-1 gives the sorting styles of `orderBy` as "asc", "desc", and
/// clause 4.23.1 limits sorting by distance to `GeoProperties` — which the `id`
/// member clause 5.7.4.3 confines the ordering to is not. A distance style is
/// therefore refused rather than accepted and left without effect.
#[tokio::test]
async fn regression_temporal_collation_a_distance_sorting_style_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;

    for style in ["dist-asc", "dist-desc"] {
        let (status, body) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&local=true&orderBy=id;{style}")).await;
        assert_bad_request_data(status, &body, style);

        let (status, body) = post_query(client, "local=true", &query_body(&ids, &json!({"orderBy": format!("id;{style}")}))).await;
        assert_bad_request_data(status, &body, style);
    }
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4's local-scope condition is about the answer being assembled
/// from this broker alone, which clause 5.5.13 defines as the Context Registry
/// not being used. No registration covers these fixtures, so both bindings order
/// the answer without the request having to ask for the local scope.
#[tokio::test]
async fn regression_temporal_collation_an_order_by_without_local_scope_still_orders_the_answer() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_numbered(&scope, client).await;

    let selected = id_filter(&ids);
    let (status, body) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={selected}&orderBy=id")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ascending = ids_from_response(&body);
    assert_eq!(ascending.len(), ids.len(), "every fixture is in the answer: {body}");

    let (status, body) = get_query(client, &format!("{WHOLE_WINDOW}&type={FIXTURE_TYPE}&id={selected}&orderBy=id%3Bdesc")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut descending = ascending.clone(); // clone: the ascending answer is asserted against the body binding below too.
    descending.reverse();
    assert_eq!(ids_from_response(&body), descending, "a descending ordering reverses the ascending one: {body}");

    let (status, body) = post_query(client, "", &query_body(&ids, &json!({"orderBy": "id;desc"}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids_from_response(&body), descending, "{body}");
    scope.finish().await.expect("cleanup failed");
}
