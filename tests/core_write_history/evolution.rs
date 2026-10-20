//! Writing and reading the Temporal Evolution of an Entity.

use crate::observation::{at, entity, fragment, reading};
use andromeda_test_client::test_client::TestClient;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use urlencoding::encode;

/// Create the Temporal Evolution of `id` through the Temporal API (clause
/// 5.6.11): one `v` instance per `(value, minute)` in `readings`.
pub async fn seed(client: &TestClient, id: &str, entity_type: &str, readings: &[(i64, u32)]) {
    let instances: Vec<Value> = readings.iter().map(|&(value, minute)| reading(value, minute)).collect();
    let response = client
        .create_temporal_entity(&entity(id, entity_type, json!({"v": instances})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&response.body));
}

/// Add `attributes` to the Temporal Evolution of `id` (clause 5.6.12).
pub async fn add(client: &TestClient, id: &str, attributes: Value) {
    let response = client
        .raw(Method::POST, &format!("temporal/entities/{}/attrs", encode(id)))
        .header("content-type", "application/ld+json")
        .json(&fragment(attributes))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
}

/// The stored Temporal Evolution of `id` in the normalized representation,
/// or `None` when the broker holds none (clause 5.7.3: 404 when "no Temporal
/// Evolution of the Entity is available").
pub async fn evolution(client: &TestClient, id: &str) -> Option<Value> {
    let response = client.get_temporal_entity(id, "", None).await.expect("the broker answers");
    if response.status == StatusCode::NOT_FOUND {
        return None;
    }
    assert_eq!(response.status, StatusCode::OK, "{}", String::from_utf8_lossy(&response.body));
    Some(response.json_value().expect("a JSON evolution"))
}

/// Every stored instance of `attribute`.
pub fn instances(evolution: &Value, attribute: &str) -> Vec<Value> {
    match &evolution[attribute] {
        Value::Array(instances) => instances.clone(), // clone: the caller inspects instances the evolution keeps.
        Value::Null => Vec::new(),
        single @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => vec![single.clone()], // clone: same as above.
    }
}

/// The `(value, observedAt)` of every instance of `attribute` that is not a
/// deletion, in the order the broker returns them.
pub fn observations(evolution: &Value, attribute: &str) -> Vec<(Value, String)> {
    instances(evolution, attribute)
        .into_iter()
        .filter(|instance| instance.get("deletedAt").is_none())
        .map(|instance| {
            let observed_at = instance["observedAt"].as_str().unwrap_or_default().to_owned();
            (instance["value"].clone(), observed_at) // clone: the pair owns the value it reports.
        })
        .collect()
}

/// The number of deletion instances (clause 4.5.8 `deletedAt`) of `attribute`.
pub fn deletions(evolution: &Value, attribute: &str) -> usize {
    instances(evolution, attribute)
        .iter()
        .filter(|instance| instance.get("deletedAt").is_some())
        .count()
}

/// The Temporal Evolution of `id` holds exactly the `v` instances `readings`
/// stands for, and no deletion of `v`: nothing written since the readings
/// reached history.
pub async fn assert_holds_only(client: &TestClient, id: &str, readings: &[(i64, u32)]) {
    let stored = evolution(client, id).await.expect("the evolution is kept");
    assert_eq!(observations(&stored, "v"), expected(readings), "{stored}");
    assert_eq!(deletions(&stored, "v"), 0, "{stored}");
}

/// The `(value, observedAt)` pairs `readings` stands for.
pub fn expected(readings: &[(i64, u32)]) -> Vec<(Value, String)> {
    readings.iter().map(|&(value, minute)| (json!(value), at(minute))).collect()
}
