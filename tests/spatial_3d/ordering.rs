//! Distance ordering is horizontal (NGSI-LD 4.23).
//!
//! `orderBy=location;dist-asc` sorts by the same geodesic distance the `near`
//! georel measures, so co-located entities at different altitudes tie at zero
//! and an altitude never reorders a result set.

use crate::fixtures::{building_point_3d, create, encode_json, id_filter, ids_of, p_2d, query};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::collections::HashSet;

/// Creates the pillar of three co-located entities plus one horizontally offset
/// entity, and returns their identifiers in creation order.
async fn seed_pillar_and_offset(scope: &TestEnvironment) -> (String, String, String, String) {
    let client = scope.client();
    let ground = scope.id("Building:order_e0");
    let mid = scope.id("Building:order_e200");
    let top = scope.id("Building:order_e400");
    let offset = scope.id("Building:order_f");

    create(client, &building_point_3d(&ground, 13.4, 52.52, 0.0)).await;
    create(client, &building_point_3d(&mid, 13.4, 52.52, 200.0)).await;
    create(client, &building_point_3d(&top, 13.4, 52.52, 400.0)).await;
    // 0.000502 degrees of longitude at latitude 52.52 is roughly 34 m.
    create(client, &building_point_3d(&offset, 13.400_502, 52.52, 0.0)).await;

    (ground, mid, top, offset)
}

/// Asserts the pillar entities occupy the first three positions in any order,
/// with the offset entity last.
fn assert_pillar_first_then_offset(ordered: &[String], pillar: [&String; 3], offset: &String) {
    assert_eq!(ordered.len(), 4, "all four seeded entities must be returned, got {ordered:?}");

    let leading: HashSet<&str> = ordered[..3].iter().map(String::as_str).collect();
    let expected: HashSet<&str> = pillar.into_iter().map(String::as_str).collect();
    assert_eq!(leading, expected, "the three co-located entities must tie at distance zero");
    assert_eq!(&ordered[3], offset, "the horizontally offset entity must sort last");
}

#[tokio::test]
async fn ordering_dist_asc_3d_orderfrom_is_horizontal() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let (ground, mid, top, offset) = seed_pillar_and_offset(&scope).await;

    let filter = id_filter(&[&ground, &mid, &top, &offset]);
    let order_from = encode_json(&json!([13.4, 52.52, 0.0]));
    let (status, _, body) = query(client, &format!("type=Building&id={filter}&orderBy=location;dist-asc&orderFrom={order_from}")).await;
    assert_eq!(status, StatusCode::OK);

    assert_pillar_first_then_offset(&ids_of(&body), [&ground, &mid, &top], &offset);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn ordering_dist_2d_orderfrom_with_3d_entities() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let (ground, mid, top, offset) = seed_pillar_and_offset(&scope).await;

    let filter = id_filter(&[&ground, &mid, &top, &offset]);
    let order_from = encode_json(&p_2d());
    let (status, _, body) = query(client, &format!("type=Building&id={filter}&orderBy=location;dist-asc&orderFrom={order_from}")).await;
    assert_eq!(status, StatusCode::OK);

    // A 2D reference must produce exactly the ordering a 3D one produced.
    assert_pillar_first_then_offset(&ids_of(&body), [&ground, &mid, &top], &offset);

    scope.finish().await.expect("cleanup failed");
}
