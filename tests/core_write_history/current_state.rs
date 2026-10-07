//! Current state, and every query that reads it, follows the Core writes
//! history leaves out.
//!
//! The current-attribute projection that answers Core `q` must move with every
//! write even though the write's history is not recorded, or Core queries
//! would answer from the state before it.

use crate::{
    evolution::{assert_holds_only, evolution, instances, seed},
    observation::{core_context, fragment, reading},
};
use andromeda_test_client::{entity::results_count, entity_attribute::DatasetSelection, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use urlencoding::encode;

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// The Core representation of `id`, or `None` when the Entity does not exist.
pub async fn current(client: &TestClient, id: &str) -> Option<Value> {
    let response = client.get_entity(id, "", "application/json", None).await.expect("the broker answers");
    if response.status == StatusCode::NOT_FOUND {
        return None;
    }
    assert_eq!(response.status, StatusCode::OK, "{}", String::from_utf8_lossy(&response.body));
    Some(response.json_value().expect("a JSON entity"))
}

/// The identifiers a Core query of `entity_type` narrowed by `query` answers.
async fn matching(scope: &TestEnvironment, entity_type: &str, query: &str) -> Vec<String> {
    let response = scope
        .client()
        .query_entities(&format!("type={entity_type}&count=true&{query}"), "application/json", Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::OK, "{query}: {}", String::from_utf8_lossy(&response.body));
    let entities = response.json_value().expect("a JSON array");
    let ids: Vec<String> = entities
        .as_array()
        .expect("an array of entities")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(results_count(&response.headers), Some(ids.len()), "{query}");
    ids
}

#[tokio::test]
async fn q_follows_an_unrecorded_update() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:q-updated");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;
    assert_eq!(matching(&scope, &entity_type, "q=v==2").await, vec![id.clone()]);

    let response = scope
        .client()
        .update_entity_attrs(&id, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    assert_eq!(matching(&scope, &entity_type, "q=v==5").await, vec![id.clone()]);
    assert_eq!(matching(&scope, &entity_type, "q=v==2").await, Vec::<String>::new());
    assert_eq!(matching(&scope, &entity_type, &format!("q={}", encode("v>4"))).await, vec![id.clone()]);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn q_follows_an_unrecorded_attribute_deletion_and_entity_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:q-deleted");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;
    let response = scope
        .client()
        .append_entity_attrs(&id, &fragment(json!({"w": reading(3, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(matching(&scope, &entity_type, "q=w==3").await, vec![id.clone()]);

    let response = scope
        .client()
        .delete_entity_attr(&id, "w", None, DatasetSelection::Default, Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(matching(&scope, &entity_type, "q=w==3").await, Vec::<String>::new());
    assert_eq!(matching(&scope, &entity_type, "q=w").await, Vec::<String>::new(), "no w instance remains");
    assert_eq!(matching(&scope, &entity_type, "q=v==2").await, vec![id.clone()]);

    let response = scope.client().delete_entity(&id).await.expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(matching(&scope, &entity_type, "q=v==2").await, Vec::<String>::new());
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

/// `scopeQ` (clause 4.19) reads the Scope current state holds, which an
/// unrecorded merge sets and an unrecorded replacement of it moves.
#[tokio::test]
async fn scope_queries_follow_an_unrecorded_scope() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:scoped");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;

    for (placed, other) in [
        ("/CoreWriteHistory/North", "/CoreWriteHistory/South"),
        ("/CoreWriteHistory/South", "/CoreWriteHistory/North"),
    ] {
        let response = scope
            .client()
            .merge_entity(&id, &fragment(json!({"scope": placed})), None)
            .await
            .expect("the broker answers");
        assert_eq!(response.status, StatusCode::NO_CONTENT);
        assert_eq!(matching(&scope, &entity_type, &format!("scopeQ={}", encode(placed))).await, vec![id.clone()]);
        assert_eq!(matching(&scope, &entity_type, &format!("scopeQ={}", encode(other))).await, Vec::<String>::new());
    }
    assert_holds_only(scope.client(), &id, &SEEDED).await;
    let recorded = evolution(scope.client(), &id).await.expect("the evolution is kept");
    assert_eq!(recorded.get("scope"), None, "clause 4.18: no Scope was recorded: {recorded}");

    scope.finish().await.expect("cleanup failed");
}

/// A geo-query (clause 4.10) reads the location current state holds.
#[tokio::test]
async fn geo_queries_follow_an_unrecorded_move() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:moved");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;
    let near = |longitude: f64, latitude: f64| {
        format!(
            "georel={}&geometry=Point&coordinates={}",
            encode("near;maxDistance==1000"),
            encode(&format!("[{longitude},{latitude}]"))
        )
    };

    for (longitude, latitude, away) in [(4.30, 52.08, (14.50, 46.05)), (14.50, 46.05, (4.30, 52.08))] {
        let location = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, latitude]}});
        let response = scope
            .client()
            .merge_entity(&id, &fragment(json!({"location": location})), None)
            .await
            .expect("the broker answers");
        assert_eq!(response.status, StatusCode::NO_CONTENT);
        assert_eq!(matching(&scope, &entity_type, &near(longitude, latitude)).await, vec![id.clone()]);
        assert_eq!(matching(&scope, &entity_type, &near(away.0, away.1)).await, Vec::<String>::new());
    }
    assert_holds_only(scope.client(), &id, &SEEDED).await;
    let recorded = evolution(scope.client(), &id).await.expect("the evolution is kept");
    assert_eq!(instances(&recorded, "location"), Vec::<Value>::new(), "no location was recorded: {recorded}");

    scope.finish().await.expect("cleanup failed");
}
