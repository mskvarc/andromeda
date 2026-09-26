//! A geo-query matches an entity when any instance of the target `GeoProperty`
//! meets the restriction (NGSI-LD 5.7.2.4), including a `location` instance
//! that carries a `datasetId`.
//!
//! The default `location` instance is hoisted into dedicated indexed columns
//! and is what the in-memory spatial index holds; these tests pin that neither
//! of those can hide an entity whose only matching instance is another one.

use crate::{create, id_filter, ids_of, query};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A polygon around Berlin, `[13,52]..[14,53]`, URL-encoded.
const AROUND_BERLIN: &str = "%5B%5B%5B13.0,52.0%5D,%5B14.0,52.0%5D,%5B14.0,53.0%5D,%5B13.0,53.0%5D,%5B13.0,52.0%5D%5D%5D";

/// A point instance of `location`, carrying `dataset_id` when given.
fn location_instance(lon: f64, lat: f64, dataset_id: Option<&str>) -> Value {
    let mut instance = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [lon, lat]}});
    if let Some(dataset_id) = dataset_id {
        instance["datasetId"] = json!(dataset_id);
    }
    instance
}

/// An `expiresAt` a moment in the past: the instance is invalid from the start
/// (clause 4.22), whether or not the expiry sweep has deleted it by the time
/// the query runs, so the answer cannot depend on the sweep's cadence.
const JUST_PASSED: &str = "2000-01-01T00:00:00Z";

/// `instance` carrying the clause 4.22 `expiresAt` it becomes invalid at.
fn expiring(mut instance: Value, expires_at: &str) -> Value {
    instance["expiresAt"] = json!(expires_at);
    instance
}

/// A Building whose `location` is exactly `instances`.
fn building_located(id: &str, instances: &[Value]) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": instances,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// The entities a `within` Berlin query returns among `ids`.
async fn within_berlin(scope: &TestEnvironment, ids: &[&str]) -> Vec<String> {
    let (status, _, body) = query(
        scope.client(),
        &format!("type=Building&id={}&georel=within&geometry=Polygon&coordinates={AROUND_BERLIN}", id_filter(ids)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ids_of(&body)
}

/// The entities a `near;maxDistance==2000` Berlin-centre query returns among
/// `ids`.
async fn near_berlin_centre(scope: &TestEnvironment, ids: &[&str]) -> Vec<String> {
    let (status, _, body) = query(
        scope.client(),
        &format!(
            "type=Building&id={}&georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B13.4,52.52%5D",
            id_filter(ids)
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ids_of(&body)
}

#[tokio::test]
async fn spatial_within_an_entity_whose_only_location_carries_a_dataset_id_returns_it() {
    let scope = TestEnvironment::from_suite("spatial").await.expect("isolated test environment");
    let tagged = scope.id("Building:only-tagged-location");
    create(
        scope.client(),
        &building_located(&tagged, &[location_instance(13.4, 52.52, Some("urn:ngsi-ld:Dataset:gps"))]),
    )
    .await;

    assert_eq!(within_berlin(&scope, &[&tagged]).await, vec![tagged.clone()]);
    assert_eq!(near_berlin_centre(&scope, &[&tagged]).await, vec![tagged]);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn spatial_within_an_entity_whose_default_location_is_outside_but_a_dataset_instance_is_inside_returns_it() {
    let scope = TestEnvironment::from_suite("spatial").await.expect("isolated test environment");
    let split = scope.id("Building:default-out-tagged-in");
    let outside = scope.id("Building:both-out");
    create(
        scope.client(),
        &building_located(
            &split,
            &[
                location_instance(20.0, 40.0, None),
                location_instance(13.4, 52.52, Some("urn:ngsi-ld:Dataset:gps")),
            ],
        ),
    )
    .await;
    create(
        scope.client(),
        &building_located(
            &outside,
            &[
                location_instance(20.0, 40.0, None),
                location_instance(21.0, 41.0, Some("urn:ngsi-ld:Dataset:gps")),
            ],
        ),
    )
    .await;

    assert_eq!(within_berlin(&scope, &[&split, &outside]).await, vec![split.clone()]);
    assert_eq!(near_berlin_centre(&scope, &[&split, &outside]).await, vec![split]);
    scope.finish().await.expect("cleanup failed");
}

/// A `datasetId` instance appended to an entity already warm in the spatial
/// index changes nothing the index holds; the query must still find it.
#[tokio::test]
async fn spatial_within_a_dataset_instance_appended_to_an_indexed_entity_returns_it() {
    let scope = TestEnvironment::from_suite("spatial").await.expect("isolated test environment");
    let id = scope.id("Building:appended-tagged-location");
    create(scope.client(), &building_located(&id, &[location_instance(20.0, 40.0, None)])).await;
    assert_eq!(within_berlin(&scope, &[&id]).await, Vec::<String>::new());

    let fragment = json!({
        "location": location_instance(13.4, 52.52, Some("urn:ngsi-ld:Dataset:gps")),
        "@context": [TEST_SUITE_CONTEXT]
    });
    let appended = scope.client().append_entity_attrs(&id, &fragment).await.expect("append failed");
    assert_eq!(appended.status, StatusCode::NO_CONTENT);

    assert_eq!(within_berlin(&scope, &[&id]).await, vec![id]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.22: an instance past its own `expiresAt` is invalid. An entity whose
/// only in-area `location` instance has expired does not match, whether that
/// instance is the hoisted default one or one carrying a `datasetId`, while an
/// otherwise identical entity whose in-area instance stands does.
#[tokio::test]
async fn spatial_within_an_entity_whose_only_in_area_location_instance_has_expired_does_not_return_it() {
    let scope = TestEnvironment::from_suite("spatial").await.expect("isolated test environment");
    let expired_default = scope.id("Building:expired-default-in");
    let expired_tagged = scope.id("Building:expired-tagged-in");
    let standing = scope.id("Building:standing-tagged-in");
    create(
        scope.client(),
        &building_located(
            &expired_default,
            &[
                expiring(location_instance(13.4, 52.52, None), JUST_PASSED),
                location_instance(20.0, 40.0, Some("urn:ngsi-ld:Dataset:gps")),
            ],
        ),
    )
    .await;
    create(
        scope.client(),
        &building_located(
            &expired_tagged,
            &[
                location_instance(20.0, 40.0, None),
                expiring(location_instance(13.4, 52.52, Some("urn:ngsi-ld:Dataset:gps")), JUST_PASSED),
            ],
        ),
    )
    .await;
    create(
        scope.client(),
        &building_located(
            &standing,
            &[
                location_instance(20.0, 40.0, None),
                expiring(location_instance(13.4, 52.52, Some("urn:ngsi-ld:Dataset:gps")), "2099-01-01T00:00:00Z"),
            ],
        ),
    )
    .await;

    let ids = [expired_default.as_str(), expired_tagged.as_str(), standing.as_str()];
    assert_eq!(within_berlin(&scope, &ids).await, vec![standing.clone()]);
    assert_eq!(near_berlin_centre(&scope, &ids).await, vec![standing]);
    scope.finish().await.expect("cleanup failed");
}
