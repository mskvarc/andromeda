//! Entity ordering integration tests (ETSI GS CIM 009 clause 4.23, 5.2.43, 6.4.3.2).
//!
//! Naming convention: `entity_order_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn query(client: &NgsiLdClient, query_str: &str) -> (StatusCode, Value) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    (response.status, response.body)
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

async fn create_ordering_fixtures(scope: &TestScope, client: &NgsiLdClient) -> (String, String, String) {
    let id_a = scope.id("Building:order-A");
    let id_b = scope.id("Building:order-B");
    let id_c = scope.id("Building:order-C");

    for (id, name, lon, lat) in [
        (&id_a, "Alpha", 13.3986_f64, 52.5547_f64),
        (&id_b, "Beta", 13.509818, 52.363457),
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
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        &client,
        &format!("type=Building&id={filter}&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_a, id_b, id_c]);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_desc_with_order_from_point_orders_farthest_first() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        &client,
        &format!("type=Building&id={filter}&orderBy=location;dist-desc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_c, id_b, id_a]);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_asc_with_order_geometry_line_string_orders_by_distance_to_line() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        &client,
        &format!("type=Building&id={filter}&orderBy=location;dist-asc&orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%5D&orderGeometry=LineString"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert_eq!(ids[0], id_c, "C (Paris) must be nearest to the Paris-area LineString");
    assert_eq!(ids[1], id_b, "B (Berlin south) must be second");
    assert_eq!(ids[2], id_a, "A (Berlin north) must be farthest");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_asc_without_order_from_uses_geo_query_reference() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b]);
    let (status, body) = query(
        &client,
        &format!("type=Building&id={filter}&georel=near;maxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location;dist-asc"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], id_a);
    assert_eq!(ids[1], id_b);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_order_from_takes_precedence_over_geo_query_coordinates() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, _id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b]);
    let (status, body) = query(
        &client,
        &format!(
            "type=Building&id={filter}&georel=near;maxDistance%3D%3D1000000&geometry=Point&coordinates=%5B2.3522%2C48.8566%5D&orderBy=location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.len() >= 2);
    assert_eq!(ids[0], id_a);
    assert_eq!(ids[1], id_b);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_mixed_property_and_distance_ordering_property_takes_precedence() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, &client).await;

    let filter = id_filter(&[&id_a, &id_b, &id_c]);
    let (status, body) = query(
        &client,
        &format!("type=Building&id={filter}&orderBy=name;asc,location;dist-asc&orderFrom=%5B13.3986%2C52.5547%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_from_response(&body), vec![id_a, id_b, id_c]);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_dist_ordering_without_any_reference_returns_bad_request() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (id_a, id_b, id_c) = create_ordering_fixtures(&scope, &client).await;
    let filter = id_filter(&[&id_a, &id_b, &id_c]);

    let (status, body) = query(&client, &format!("type=Building&id={filter}&orderBy=location;dist-asc")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"].as_str().unwrap(), error::BAD_REQUEST_DATA);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_order_invalid_order_from_syntax_returns_bad_request() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (status, _) = query(&client, "type=Building&orderBy=location;dist-asc&orderFrom=notjson").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn entity_order_geometry_collection_as_order_geometry_returns_bad_request() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;
    let (status, body) = query(
        &client,
        "type=Building&orderBy=location;dist-asc&orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"].as_str().unwrap(), error::BAD_REQUEST_DATA);
}

#[tokio::test]
async fn entity_order_entities_without_location_excluded_from_dist_ordered_results() {
    let scope = TestScope::new("entity_order");
    let client = NgsiLdClient::new(&scope).await;

    let id_with = scope.id("Building:ord-with-loc");
    let id_without = scope.id("Building:ord-no-loc");
    create(
        &client,
        &json!({
            "id": id_with,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    create(
        &client,
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
        &client,
        &format!("type=Building&id={filter}&orderBy=location;dist-asc&orderFrom=%5B13.0%2C52.0%5D"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = ids_from_response(&body);
    assert!(ids.contains(&id_with), "entity with location must appear in results");
    scope.cleanup().await.expect("cleanup failed");
}
