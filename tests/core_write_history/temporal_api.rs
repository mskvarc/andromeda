//! The Temporal API is recorded exactly as before when Core writes are not.
//!
//! The setting exists for a producer that publishes its history through the
//! Temporal API and keeps current state fresh with a Core upsert restating its
//! newest reading. Recorded, that upsert is a second instance of a reading the
//! history already holds, and every count and sum over the period is off by it.

use crate::{
    current_state::current,
    evolution::{add, assert_holds_only, evolution, expected, instances, observations, seed},
    observation::{at, core_context, entity, fragment, reading},
};
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use urlencoding::encode;

/// A day of readings as a producer publishes it: one per minute.
const DAY: [(i64, u32); 4] = [(4, 0), (6, 1), (3, 2), (7, 3)];

#[tokio::test]
async fn a_core_upsert_restating_the_newest_reading_does_not_record_it_twice() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Counter");
    let id = scope.id("Counter:restated");
    seed(scope.client(), &id, &entity_type, &DAY).await;

    let response = scope
        .client()
        .raw(Method::POST, "entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&json!([entity(&id, &entity_type, json!({"v": reading(7, 3)}))]))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_holds_only(scope.client(), &id, &DAY).await;
    let aggregated = scope
        .client()
        .get_temporal_entity(
            &id,
            "format=aggregatedValues&aggrMethods=totalCount,sum&aggrPeriodDuration=P1D",
            Some(core_context()),
        )
        .await
        .expect("the broker answers");
    assert_eq!(aggregated.status, StatusCode::OK, "{}", String::from_utf8_lossy(&aggregated.body));
    let aggregated = aggregated.json_value().expect("a JSON evolution");
    assert_eq!(aggregated["v"]["totalCount"][0][0], 4, "{aggregated}");
    assert_eq!(aggregated["v"]["sum"][0][0], 20, "{aggregated}");
    assert_eq!(current(scope.client(), &id).await.expect("the entity exists")["v"]["value"], 7);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn adding_attributes_through_the_temporal_api_is_recorded() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Counter:extended");
    seed(scope.client(), &id, &scope.entity_type("Counter"), &DAY[..2]).await;

    add(scope.client(), &id, json!({"v": [reading(3, 2), reading(7, 3)], "w": reading(1, 3)})).await;

    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "v"), expected(&DAY));
    assert_eq!(observations(&recorded, "w"), expected(&[(1, 3)]));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.5.3: a Temporal API instance newer than the one current state
/// holds becomes current state, which Core `q` then reads.
#[tokio::test]
async fn a_newer_temporal_instance_still_becomes_current_state() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Counter");
    let id = scope.id("Counter:advanced");
    seed(scope.client(), &id, &entity_type, &DAY[..2]).await;

    add(scope.client(), &id, json!({"v": reading(42, 10)})).await;

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!((stored["v"]["value"].clone(), stored["v"]["observedAt"].clone()), (json!(42), json!(at(10))));
    let matching = scope
        .client()
        .query_entities(&format!("type={entity_type}&q=v==42"), "application/json", Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(matching.json_value().expect("a JSON array")[0]["id"], json!(id));

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.14 and 5.6.15 still modify and delete a recorded instance.
#[tokio::test]
async fn modifying_and_deleting_a_recorded_instance_still_apply() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Counter:corrected");
    seed(scope.client(), &id, &scope.entity_type("Counter"), &DAY[..2]).await;
    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    let instance_ids: Vec<String> = instances(&recorded, "v")
        .iter()
        .filter_map(|instance| instance["instanceId"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(instance_ids.len(), 2, "{recorded}");

    let correction = fragment(json!({"v": [{"type": "Property", "value": 99, "observedAt": at(0)}]}));
    let response = scope
        .client()
        .raw(
            Method::PATCH,
            &format!("temporal/entities/{}/attrs/v/{}", encode(&id), encode(&instance_ids[0])),
        )
        .header("content-type", "application/ld+json")
        .json(&correction)
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
    let response = scope
        .client()
        .raw(
            Method::DELETE,
            &format!("temporal/entities/{}/attrs/v/{}", encode(&id), encode(&instance_ids[1])),
        )
        .header("link", jsonld_link_header(core_context()).expect("a Link header"))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "v"), expected(&[(99, 0)]));

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.13 and 5.6.16 still delete an attribute's and an Entity's
/// Temporal Evolution.
#[tokio::test]
async fn deleting_a_temporal_evolution_still_applies() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Counter:erased");
    seed(scope.client(), &id, &scope.entity_type("Counter"), &DAY[..2]).await;
    add(scope.client(), &id, json!({"w": reading(1, 3)})).await;

    let response = scope
        .client()
        .raw(Method::DELETE, &format!("temporal/entities/{}/attrs/w", encode(&id)))
        .header("link", jsonld_link_header(core_context()).expect("a Link header"))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "w"), Vec::new(), "{recorded}");
    assert_eq!(observations(&recorded, "v"), expected(&DAY[..2]));

    let response = scope.client().delete_temporal_entity(&id).await.expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
    assert_eq!(evolution(scope.client(), &id).await, None);

    scope.finish().await.expect("cleanup failed");
}
