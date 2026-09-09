//! Distance ordering is horizontal (NGSI-LD 4.23).
//!
//! `orderBy=location;dist-asc` sorts by the same geodesic distance the `near`
//! georel measures, so co-located entities at different altitudes tie at zero
//! and an altitude never reorders a result set.
//!
//! Every query here carries `local=true`: clause 5.7.2.4 raises `BadRequestData`
//! for an `orderBy` whose execution is not limited to the local scope (clause
//! 5.5.13), which clause 6.3.18 Table 6.3.18-1 binds to the `local` parameter,
//! and clause 4.23.1 restates it as "Sort ordering is never applied to
//! distributed operations".

use crate::fixtures::{building_point_3d, create, encode_json, id_filter, ids_of, p_2d, query};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
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
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom={order_from}"),
    )
    .await;
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
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=location;dist-asc&orderFrom={order_from}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A 2D reference must produce exactly the ordering a 3D one produced.
    assert_pillar_first_then_offset(&ids_of(&body), [&ground, &mid, &top], &offset);

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.23 read with 4.10: `orderBy` may name any `GeoProperty`, not only
/// the default `location`. A named one is measured from its
/// `current_attribute_values`
/// instance rows, so the ordering has to reach the same geometry the filter
/// does.
///
/// Every entity's `location` is deliberately placed in the opposite order to
/// its `observationPoint`, so an ordering that silently fell back to the
/// default `GeoProperty` would produce exactly the reverse result.
#[tokio::test]
async fn ordering_dist_asc_on_a_custom_geoproperty_sorts_by_that_geoproperty() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let near = scope.id("Building:order_geoprop_near");
    let middle = scope.id("Building:order_geoprop_middle");
    let far = scope.id("Building:order_geoprop_far");

    // Distance from the reference by observationPoint: near < middle < far.
    // By location the order is exactly the reverse, so an ordering that fell
    // back to the default GeoProperty would invert the result.
    create(client, &building_with_observation_point(&near, [13.9, 52.52], [13.4, 52.52])).await;
    create(client, &building_with_observation_point(&middle, [13.6, 52.52], [13.5, 52.52])).await;
    create(client, &building_with_observation_point(&far, [13.4, 52.52], [13.9, 52.52])).await;

    let filter = id_filter(&[&near, &middle, &far]);
    let order_from = encode_json(&json!([13.4, 52.52]));
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=observationPoint;dist-asc&orderFrom={order_from}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        ids_of(&body),
        vec![near.clone(), middle.clone(), far.clone()],
        "a distance ordering must measure to the named GeoProperty, not to location"
    );

    // The descending direction reverses the same measurement rather than
    // producing an unrelated order.
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=observationPoint;dist-desc&orderFrom={order_from}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids_of(&body), vec![far, middle, near]);

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.23 read with 4.10: an entity whose named `GeoProperty` has several
/// `datasetId` instances is ordered by its nearest instance, which is the
/// reading that agrees with the filter counting an entity as matching when any
/// one instance does.
#[tokio::test]
async fn ordering_dist_asc_on_a_custom_geoproperty_measures_the_nearest_instance() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let multi = scope.id("Building:order_geoprop_multi");
    let single = scope.id("Building:order_geoprop_single");

    // `multi`'s default instance is the farther of the two; its tagged instance
    // is nearer the reference than `single` is.
    let multi_entity = json!({
        "id": multi,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "observationPoint": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.9, 52.52]}},
            {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.41, 52.52]},
                "datasetId": "urn:ngsi-ld:Dataset:sensor-a"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &multi_entity).await;
    create(client, &building_with_observation_point(&single, [13.4, 52.52], [13.5, 52.52])).await;

    let filter = id_filter(&[&multi, &single]);
    let order_from = encode_json(&json!([13.4, 52.52]));
    let (status, _, body) = query(
        client,
        &format!("type=Building&id={filter}&local=true&orderBy=observationPoint;dist-asc&orderFrom={order_from}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        ids_of(&body),
        vec![multi, single],
        "the nearest instance decides, so the multi-instance entity sorts first"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A `Building` whose `location` and `observationPoint` are deliberately
/// distinct points, so an ordering can tell which of the two it measured.
fn building_with_observation_point(id: &str, location: [f64; 2], observation_point: [f64; 2]) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": location}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": observation_point}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}
