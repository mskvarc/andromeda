//! Temporal `pick` and `omit` projection tests (NGSI-LD 6.3.15).
//!
//! Clause 6.3.15 narrows an entity to the members the projection names. The
//! Temporal API narrows the same way, and the projection reaches the rendered
//! temporal representation, so an attribute it drops takes all of its instances
//! with it.
//!
//! `id`, `type` and `scope` are members the clause names alongside attributes,
//! read literally here: a non-empty `pick` keeps one of them only when it lists
//! it, and a non-empty `omit` drops whichever of them it lists.

use crate::{TEST_SUITE_CONTEXT, clean_database, client, create_temporal_entity, query_temporal_entities, unique_id, vehicle_temporal_august};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

/// The window the August fixture falls inside.
const WINDOW: &str = "timerel=between&timeAt=2020-08-01T00:00:00Z&endTimeAt=2020-08-02T00:00:00Z";

/// The single member of a one-entity query answer.
fn only_member(body: &Value) -> &Value {
    let entities = body.as_array().expect("a temporal query answers with an array");
    assert_eq!(entities.len(), 1, "expected exactly one entity: {body}");
    entities.first().expect("the array length was just asserted to be one")
}

/// Clause 6.3.15: `pick` keeps only the attributes it names, and `omit` drops
/// only the ones it names — over the temporal representation, instances and
/// all.
#[tokio::test]
#[serial]
async fn temporal_pick_omit_now_applied() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:tproj-pick-omit");

    assert_eq!(create_temporal_entity(&c, &vehicle_temporal_august(&id)).await, StatusCode::CREATED);

    let (status, body, _) = query_temporal_entities(&c, &format!("id={id}&type=Vehicle&{WINDOW}&pick=speed"), Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let picked = only_member(&body);
    assert!(picked["speed"].is_array(), "the picked attribute keeps its instances: {picked}");
    assert!(picked.get("fuelLevel").is_none(), "an attribute outside `pick` must be stripped: {picked}");
    // Read literally, a `pick` that names no structural member keeps none.
    assert!(picked.get("id").is_none(), "`id` was not picked: {picked}");
    assert!(picked.get("type").is_none(), "`type` was not picked: {picked}");

    let (status, body, _) = query_temporal_entities(&c, &format!("id={id}&type=Vehicle&{WINDOW}&omit=speed"), Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let kept = only_member(&body);
    assert!(kept.get("speed").is_none(), "the omitted attribute must be stripped: {kept}");
    assert!(kept["fuelLevel"].is_array(), "an attribute outside `omit` keeps its instances: {kept}");
    // `omit` names no structural member here, so all of them stay.
    assert_eq!(kept["id"].as_str(), Some(id.as_str()));
    assert_eq!(kept["type"].as_str(), Some("Vehicle"));
}
