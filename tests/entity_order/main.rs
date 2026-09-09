//! Entity ordering integration tests (ETSI GS CIM 009 clause 4.23, 5.2.43, 6.4.3.2).
//!
//! Every query here carries `local=true`. Clause 5.7.2.4 states that "if the
//! ordering parameter is present and the execution of the operation is not
//! limited to the local scope (see clause 5.5.13) then an error of type
//! `BadRequestData` shall be raised", and clause 4.23.1 says the same thing from
//! the other side: "Sort ordering is never applied to distributed operations".
//! Clause 6.3.18 Table 6.3.18-1 binds the local scope to the `local` query
//! parameter, so an `orderBy` request that omits it is a request the broker is
//! required to reject and no ordering assertion could be made about it.
//!
//! Naming convention: `entity_order_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
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
        .expect("response must be JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have id field").to_owned())
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
///
/// Every negative test here sends `orderBy`, so without `local=true` each one
/// would be answered by the local-scope refusal before the broker ever reached
/// the condition under test, and the assertion would hold for the wrong reason.
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

async fn create_ordering_fixtures(scope: &TestEnvironment, client: &TestClient) -> (String, String, String) {
    let id_a = scope.id("Building:order-A");
    let id_b = scope.id("Building:order-B");
    let id_c = scope.id("Building:order-C");

    for (id, name, lon, lat) in [
        (&id_a, "Alpha", 13.3986_f64, 52.5547_f64),
        (&id_b, "Beta", 13.509_818, 52.363_457),
        (&id_c, "Gamma", 2.3522, 48.8566),
    ] {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "name": {"type": "Property", "value": name},
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [lon, lat]}},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    (id_a, id_b, id_c)
}

#[tokio::test]
async fn entity_order_dist_asc_with_order_from_point_orders_nearest_first() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_a, id_b, id_c]);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_desc_with_order_from_point_orders_farthest_first() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-desc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_c, id_b, id_a]);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_asc_with_order_geometry_line_string_orders_by_distance_to_line() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert_eq!(ids[0], id_c, "C (Paris) must be nearest to the Paris-area LineString");
    assert_eq!(ids[1], id_b, "B (Berlin south) must be second");
    assert_eq!(ids[2], id_a, "A (Berlin north) must be farthest");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_asc_without_order_from_uses_geo_query_reference() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b]);
    let (status, body) = query(
        client,
        &format!(
            "type=Building&id={filter}&local=true&georel=near;maxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location;dist-asc"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], id_a);
    assert_eq!(ids[1], id_b);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_order_from_takes_precedence_over_geo_query_coordinates() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b]);
    let (status, body) = query(
        client,
        &format!(
            "type=Building&id={filter}&local=true&georel=near;maxDistance%3D%3D1000000&geometry=Point&coordinates=%5B2.3522%2C48.8566%5D&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.len() >= 2);
    assert_eq!(ids[0], id_a);
    assert_eq!(ids[1], id_b);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_mixed_property_and_distance_ordering_property_takes_precedence() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=name;asc,location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_a, id_b, id_c]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: "If the ordering parameter is present and refers to ordering
/// by distance and no location is present, then an error of type
/// `BadRequestData` shall be raised."
#[tokio::test]
async fn entity_order_dist_ordering_without_any_reference_returns_bad_request() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, client).await;
    let filter = id_filter(&[&id_a, &id_b, &id_c]);

    let (status, body) = query(client, &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_not_the_local_scope_refusal(&body);

    // The identical request with a reference geometry is accepted, so the
    // rejection above is the missing reference and nothing else about the URL.
    let (status, _) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: "If a location to be used when applying ordering of Entities
/// setting is present and it is not syntactically valid (as per the referred
/// clauses 4.9 and 4.10) [...] an error of type `BadRequestData` shall be
/// raised." Clause 4.23.3 defines `orderFrom` as RFC 7946 coordinates, which
/// `notjson` is not.
#[tokio::test]
async fn entity_order_invalid_order_from_syntax_returns_bad_request() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (status, body) = query(client, "type=Building&local=true&orderBy=location;dist-asc&orderFrom=notjson").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_not_the_local_scope_refusal(&body);

    // The same request with well-formed coordinates is accepted, so the
    // rejection above is the reading of `notjson` alone.
    let (status, _) = query(client, "type=Building&local=true&orderBy=location;dist-asc&orderFrom=%5B8%2C40%5D").await;
    assert_eq!(status, StatusCode::OK);
}

/// Clause 4.23.3 spells `orderGeometry` as an RFC 7946 geometry type, and
/// clause 4.10 admits every one of them "except `GeometryCollection`", which
/// clause 5.7.2.4 makes a `BadRequestData`.
#[tokio::test]
async fn entity_order_geometry_collection_as_order_geometry_returns_bad_request() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();
    let (status, body) = query(
        client,
        "type=Building&local=true&orderBy=location;dist-asc&orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_not_the_local_scope_refusal(&body);
    assert!(
        detail_of(&body).contains("GeometryCollection"),
        "the rejection must name the geometry type it refused: {}",
        detail_of(&body)
    );

    // An admitted geometry type on the same request is accepted, so the
    // rejection above is the `GeometryCollection` token alone.
    let (status, _) = query(
        client,
        "type=Building&local=true&orderBy=location;dist-asc&orderFrom=%5B8%2C40%5D&orderGeometry=Point",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn entity_order_entities_without_location_excluded_from_dist_ordered_results() {
    let scope = TestEnvironment::from_suite("entity_order").await.expect("isolated test environment");
    let client = scope.client();

    let id_with = scope.id("Building:ord-with-loc");
    let id_without = scope.id("Building:ord-no-loc");
    create(
        client,
        &json!({
            "id": id_with,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    create(
        client,
        &json!({
            "id": id_without,
            "type": "Building",
            "name": {"type": "Property", "value": "NoLocation"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let filter = id_filter(&[&id_with, &id_without]);
    let (status, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom=%5B13.0%2C52.0%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.contains(&id_with), "entity with location must appear in results");
    scope.finish().await.expect("cleanup failed");
}
