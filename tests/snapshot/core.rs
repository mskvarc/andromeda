//! Snapshot-scoped Core API acceptance cases.

use crate::support::{broker_url, clean_database, client, create_entity, create_snapshot_raw, scoped, wait_for_snapshot_terminal};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;
use tokio::time::sleep;

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const VEHICLE_TYPE: &str = "https://example.org/Vehicle";
const SPEED: &str = "https://example.org/speed";
const REMOTE_BASE: &str = "http://127.0.0.1:1083/ngsi-ld/v1";

fn vehicle(id: &str, speed: i64) -> Value {
    json!({"id": id, "type": "Vehicle", "speed": {"type": "Property", "value": speed}, "@context": [CORE_CONTEXT, {"Vehicle": VEHICLE_TYPE, "speed": SPEED}]})
}

fn default_vehicle(id: &str, speed: i64) -> Value {
    json!({"id": id, "type": VEHICLE_TYPE, "speed": {"type": "Property", "value": speed}, "@context": CORE_CONTEXT})
}

async fn empty_branch(id: &str) {
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await["snapshotStatus"],
        "failure"
    );
}

async fn scoped_get(path: &str, snapshot: &str) -> (StatusCode, Value) {
    let response = scoped(Method::GET, path, snapshot, None).await;
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_create_and_delete_entity_mutate_only_the_branch() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-create-delete";
    let entity_id = "urn:ngsi-ld:Vehicle:core-create-delete";
    empty_branch(snapshot).await;
    let created = scoped(Method::POST, "/entities", snapshot, Some(&vehicle(entity_id, 10))).await;
    pretty_assertions::assert_eq!(created.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(created.headers()["NGSILD-Snapshot"], snapshot);
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url(&format!("/entities/{entity_id}")))
            .send()
            .await
            .expect("live get")
            .status(),
        StatusCode::NOT_FOUND
    );
    pretty_assertions::assert_eq!(scoped_get(&format!("/entities/{entity_id}"), snapshot).await.0, StatusCode::OK);
    pretty_assertions::assert_eq!(
        scoped(Method::DELETE, &format!("/entities/{entity_id}"), snapshot, None).await.status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(scoped_get(&format!("/entities/{entity_id}"), snapshot).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_attribute_mutation_matrix_is_branch_local() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-attrs";
    let entity_id = "urn:ngsi-ld:Vehicle:core-attrs";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entities", snapshot, Some(&vehicle(entity_id, 10))).await.status(),
        StatusCode::CREATED
    );
    let color = json!({"color": {"type": "Property", "value": "red"}, "@context": [CORE_CONTEXT, {"color": "https://example.org/color"}]});
    pretty_assertions::assert_eq!(
        scoped(Method::POST, &format!("/entities/{entity_id}/attrs"), snapshot, Some(&color))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let speed = json!({"value": 20, "@context": [CORE_CONTEXT, {"speed": SPEED}]});
    pretty_assertions::assert_eq!(
        scoped(Method::PATCH, &format!("/entities/{entity_id}/attrs/speed"), snapshot, Some(&speed))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(
        scoped(
            Method::DELETE,
            &format!("/entities/{entity_id}/attrs/https%3A%2F%2Fexample.org%2Fcolor"),
            snapshot,
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let (_, branch) = scoped_get(&format!("/entities/{entity_id}"), snapshot).await;
    pretty_assertions::assert_eq!(branch[SPEED]["value"], 20);
    assert!(branch.get("https://example.org/color").is_none());
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_merge_replace_entity_and_attribute_matrix_is_branch_local() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-merge-replace";
    let entity_id = "urn:ngsi-ld:Vehicle:core-merge-replace";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entities", snapshot, Some(&vehicle(entity_id, 1))).await.status(),
        StatusCode::CREATED
    );
    let merge = json!({"speed": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT, {"speed": SPEED}]});
    pretty_assertions::assert_eq!(
        scoped(Method::PATCH, &format!("/entities/{entity_id}"), snapshot, Some(&merge)).await.status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(
        scoped(Method::PUT, &format!("/entities/{entity_id}"), snapshot, Some(&vehicle(entity_id, 3)))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let attribute = json!({"type": "Property", "value": 4, "@context": [CORE_CONTEXT, {"speed": SPEED}]});
    pretty_assertions::assert_eq!(
        scoped(Method::PUT, &format!("/entities/{entity_id}/attrs/speed"), snapshot, Some(&attribute))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(scoped_get(&format!("/entities/{entity_id}"), snapshot).await.1[SPEED]["value"], 4);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_batch_provision_matrix_is_branch_local() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-batch";
    empty_branch(snapshot).await;
    let entities = json!([vehicle("urn:ngsi-ld:Vehicle:batch-one", 1), vehicle("urn:ngsi-ld:Vehicle:batch-two", 2)]);
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entityOperations/create", snapshot, Some(&entities)).await.status(),
        StatusCode::CREATED
    );
    let update = json!([{"id": "urn:ngsi-ld:Vehicle:batch-one", "type": "Vehicle", "speed": {"type": "Property", "value": 11}, "@context": [CORE_CONTEXT, {"Vehicle": VEHICLE_TYPE, "speed": SPEED}]}]);
    assert!(
        scoped(Method::POST, "/entityOperations/update", snapshot, Some(&update))
            .await
            .status()
            .is_success()
    );
    pretty_assertions::assert_eq!(
        scoped(
            Method::POST,
            "/entityOperations/delete",
            snapshot,
            Some(&json!(["urn:ngsi-ld:Vehicle:batch-two"]))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(
        scoped_get("/entities?type=https%3A%2F%2Fexample.org%2FVehicle", snapshot)
            .await
            .1
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_retrieve_and_query_remain_frozen_after_live_mutation() {
    let _serialized_environment = clean_database().await;
    let entity_id = "urn:ngsi-ld:Vehicle:frozen";
    pretty_assertions::assert_eq!(create_entity(&client(), &vehicle(entity_id, 7)).await, StatusCode::CREATED);
    let snapshot = "urn:ngsi-ld:Snapshot:core-frozen";
    let body = json!({"id": snapshot, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": VEHICLE_TYPE}]}]});
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), snapshot, Duration::from_secs(5)).await["snapshotStatus"],
        "success"
    );
    let live_patch = json!({"speed": {"type": "Property", "value": 99}, "@context": [CORE_CONTEXT, {"speed": SPEED}]});
    pretty_assertions::assert_eq!(
        client()
            .patch(broker_url(&format!("/entities/{entity_id}")))
            .header("Content-Type", "application/ld+json")
            .json(&live_patch)
            .send()
            .await
            .expect("live patch")
            .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(scoped_get(&format!("/entities/{entity_id}"), snapshot).await.1[SPEED]["value"], 7);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_query_applies_entity_q_geo_and_scope_constraints() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-query-constraints";
    empty_branch(snapshot).await;
    let mut entity = default_vehicle("urn:ngsi-ld:Vehicle:constraints", 30);
    entity["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 46.0]}});
    entity["scope"] = json!(["/fleet/west"]);
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity)).await.status(), StatusCode::CREATED);
    for query in [
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle",
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle&q=speed%3E20",
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B13.0%2C46.0%5D",
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle&scopeQ=%2Ffleet%2Fwest",
    ] {
        let (status, result) = scoped_get(query, snapshot).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK, "query failed: {query}: {result}");
        pretty_assertions::assert_eq!(result.as_array().map(Vec::len), Some(1), "query mismatch: {query}: {result}");
    }
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_query_applies_dataset_projection_and_linked_entity_semantics() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-dataset";
    empty_branch(snapshot).await;
    let entity = json!({
        "id": "urn:ngsi-ld:Vehicle:dataset",
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:alternate"}],
        "@context": [CORE_CONTEXT, {"Vehicle": VEHICLE_TYPE, "speed": SPEED}]
    });
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity)).await.status(), StatusCode::CREATED);
    let (_, projected) = scoped_get(
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle&attrs=https%3A%2F%2Fexample.org%2Fspeed&datasetId=urn%3Angsi-ld%3ADataset%3Aalternate",
        snapshot,
    )
    .await;
    pretty_assertions::assert_eq!(projected[0][SPEED]["value"], 2);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_query_applies_pagination_ordering_and_count() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-pagination";
    empty_branch(snapshot).await;
    for (suffix, speed) in [("a", 3), ("b", 1), ("c", 2)] {
        pretty_assertions::assert_eq!(
            scoped(
                Method::POST,
                "/entities",
                snapshot,
                Some(&default_vehicle(&format!("urn:ngsi-ld:Vehicle:{suffix}"), speed))
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }
    let response = scoped(
        Method::GET,
        "/entities?type=https%3A%2F%2Fexample.org%2FVehicle&limit=1&offset=1&count=true&orderBy=speed%3Basc",
        snapshot,
        None,
    )
    .await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.expect("ordered page");
    pretty_assertions::assert_eq!(status, StatusCode::OK, "ordering response: {body}");
    pretty_assertions::assert_eq!(headers["NGSILD-Results-Count"], "3");
    pretty_assertions::assert_eq!(body[0]["id"], "urn:ngsi-ld:Vehicle:c");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_query_renders_normalized_concise_simplified_and_geojson() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-representations";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entities", snapshot, Some(&vehicle("urn:ngsi-ld:Vehicle:representations", 8)))
            .await
            .status(),
        StatusCode::CREATED
    );
    for query in ["", "?options=concise", "?options=keyValues", "?options=normalized"] {
        pretty_assertions::assert_eq!(
            scoped(Method::GET, &format!("/entities/urn:ngsi-ld:Vehicle:representations{query}"), snapshot, None)
                .await
                .status(),
            StatusCode::OK
        );
    }
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url("/entities?type=https%3A%2F%2Fexample.org%2FVehicle&options=keyValues"))
            .header("NGSILD-Snapshot", snapshot)
            .header("Accept", "application/geo+json")
            .send()
            .await
            .expect("GeoJSON query")
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_type_and_attribute_metadata_matrix_uses_branch_state() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-metadata";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entities", snapshot, Some(&vehicle("urn:ngsi-ld:Vehicle:metadata", 4)))
            .await
            .status(),
        StatusCode::CREATED
    );
    for path in [
        "/types",
        "/types/https%3A%2F%2Fexample.org%2FVehicle",
        "/attributes",
        "/attributes/https%3A%2F%2Fexample.org%2Fspeed",
    ] {
        pretty_assertions::assert_eq!(scoped(Method::GET, path, snapshot, None).await.status(), StatusCode::OK);
    }
    let live_types = client()
        .get(broker_url("/types"))
        .send()
        .await
        .expect("live types")
        .json::<Value>()
        .await
        .expect("live type body");
    assert!(!live_types.to_string().contains(VEHICLE_TYPE));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_mutation_records_temporal_evolution_inside_branch() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-temporal-side-effect";
    let entity_id = "urn:ngsi-ld:Vehicle:core-temporal-side-effect";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/entities", snapshot, Some(&vehicle(entity_id, 12))).await.status(),
        StatusCode::CREATED
    );
    sleep(Duration::from_millis(100)).await;
    pretty_assertions::assert_eq!(
        scoped(Method::GET, &format!("/temporal/entities/{entity_id}"), snapshot, None).await.status(),
        StatusCode::OK
    );
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url(&format!("/temporal/entities/{entity_id}")))
            .send()
            .await
            .expect("live temporal get")
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_core_operations_ignore_local_false_skip_csrs_and_echo_header() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:core-local-false";
    let remote_id = "urn:ngsi-ld:Vehicle:remote-local-false";
    empty_branch(snapshot).await;
    let _ignored = client().delete(format!("{REMOTE_BASE}/entities/{remote_id}")).send().await;
    pretty_assertions::assert_eq!(
        client()
            .post(format!("{REMOTE_BASE}/entities"))
            .header("Content-Type", "application/ld+json")
            .json(&vehicle(remote_id, 99))
            .send()
            .await
            .expect("remote local=false fixture")
            .status(),
        StatusCode::CREATED
    );
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:core-local-false",
        "type": "ContextSourceRegistration",
        "endpoint": "http://snapshot-remote:1083",
        "information": [{"entities": [{"type": VEHICLE_TYPE}]}],
        "operations": ["queryEntity"],
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("matching remote source registration")
            .status(),
        StatusCode::CREATED
    );
    let response = client()
        .post(broker_url("/entities?local=false"))
        .header("NGSILD-Snapshot", snapshot)
        .header("Content-Type", "application/ld+json")
        .json(&vehicle("urn:ngsi-ld:Vehicle:local-false", 5))
        .send()
        .await
        .expect("Snapshot local=false create");
    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(response.headers()["NGSILD-Snapshot"], snapshot);
    let query = client()
        .get(broker_url("/entities?type=https%3A%2F%2Fexample.org%2FVehicle&local=false"))
        .header("NGSILD-Snapshot", snapshot)
        .send()
        .await
        .expect("Snapshot local=false query");
    pretty_assertions::assert_eq!(query.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(query.headers()["NGSILD-Snapshot"], snapshot);
    let entities = query.json::<Value>().await.expect("Snapshot local=false query body");
    pretty_assertions::assert_eq!(entities.as_array().map(Vec::len), Some(1));
    assert!(!entities.to_string().contains(remote_id));
}
