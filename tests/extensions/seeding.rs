//! Shared seeding helpers for the extensions suite.

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The three observation instants the window tests seed, ordered `t1 < t2 < t3`.
pub const T1: &str = "2024-01-01T00:00:00Z";
pub const T2: &str = "2024-03-01T00:00:00Z";
pub const T3: &str = "2024-06-01T00:00:00Z";

/// Seed a temporal entity whose `speed` attribute has one instance at each of
/// `T1`, `T2` and `T3`.
pub async fn seed_three_instant_history(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10, "observedAt": T1},
            {"type": "Property", "value": 20, "observedAt": T2},
            {"type": "Property", "value": 30, "observedAt": T3}
        ],
        "@context": [CORE_CONTEXT]
    });
    let response = client.create_temporal_entity(&body).await.expect("seed temporal entity");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "unexpected seed status: {}",
        response.status
    );
}

/// The `observedAt` values the `speed` attribute of a temporal entity still
/// carries, read back after a delete.
pub async fn surviving_speed_instants(client: &TestClient, id: &str) -> Vec<String> {
    let response = client
        .get_temporal_entity(id, "timerel=after&timeAt=1970-01-01T00:00:00Z&attrs=speed", Some(CORE_CONTEXT))
        .await
        .expect("read temporal entity");
    if response.status == StatusCode::NOT_FOUND {
        return Vec::new();
    }
    assert_eq!(response.status, StatusCode::OK, "unexpected read status");
    let body = response.json_value().expect("temporal body is JSON");
    instant_values(&body, "speed")
}

/// The `observedAt` values one attribute of a normalized temporal representation
/// carries, whether it renders as a single object or an array of instances.
fn instant_values(entity: &Value, attribute: &str) -> Vec<String> {
    let member = &entity[attribute];
    let instances = match member {
        Value::Array(instances) => instances.clone(),
        Value::Object(_) => vec![member.clone()],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    instances
        .iter()
        .filter_map(|instance| instance["observedAt"].as_str().map(ToOwned::to_owned))
        .collect()
}

/// Seed a current-state entity whose `speed` attribute has a default instance
/// and one named `datasetId` instance.
pub async fn seed_two_dataset_instances(client: &TestClient, id: &str, dataset: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10},
            {"type": "Property", "value": 20, "datasetId": dataset}
        ],
        "brandName": {"type": "Property", "value": "Mercedes"},
        "@context": [CORE_CONTEXT]
    });
    let response = client.post_entity(&body).send().await.expect("seed entity");
    assert_eq!(response.status, StatusCode::CREATED, "unexpected seed status");
}
