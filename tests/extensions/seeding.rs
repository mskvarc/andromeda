//! Shared seeding helpers for the extensions suite.

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The three observation instants the window tests seed, ordered `t1 < t2 < t3`.
pub const T1: &str = "2024-01-01T00:00:00Z";
pub const T2: &str = "2024-03-01T00:00:00Z";
pub const T3: &str = "2024-06-01T00:00:00Z";

/// POST a temporal entity body and assert the broker accepted it, tolerating
/// either creation status (`201` for a new entity, `204` when the history was
/// appended to an existing one). `context` labels the failure if the request
/// itself does not complete.
async fn create_seed_entity(client: &TestClient, body: &Value, context: &'static str) {
    let response = client.create_temporal_entity(body).await.expect(context);
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "unexpected seed status: {}",
        response.status
    );
}

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
    create_seed_entity(client, &body, "seed temporal entity").await;
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

/// A `datasetId` used by the grouping seed.
pub const DATASET_A: &str = "urn:ngsi-ld:Dataset:A";
/// A second `datasetId` used by the grouping seed.
pub const DATASET_B: &str = "urn:ngsi-ld:Dataset:B";

/// Seed a temporal entity whose `speed` history spans two `datasetId`s (each a
/// separate instance series with its own uniform `unitCode`) across several
/// months, plus a `zone` attribute whose value changes over time (for the
/// value-in-effect key).
///
/// Because aggregation and grouping run per `datasetId`, the two datasets
/// multiply the group count: grouping by `datasetId,observedAt;month` yields
/// four groups, which the over-the-bound case relies on to trip
/// `TooManyResults`.
pub async fn seed_grouping_history(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10, "unitCode": "KMH", "datasetId": DATASET_A, "observedAt": "2024-01-01T08:00:00Z"},
            {"type": "Property", "value": 20, "unitCode": "KMH", "datasetId": DATASET_A, "observedAt": "2024-02-01T09:00:00Z"},
            {"type": "Property", "value": 30, "unitCode": "HM", "datasetId": DATASET_B, "observedAt": "2024-01-15T08:30:00Z"},
            {"type": "Property", "value": 40, "unitCode": "HM", "datasetId": DATASET_B, "observedAt": "2024-03-01T10:00:00Z"}
        ],
        "zone": [
            {"type": "Property", "value": "north", "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": "south", "observedAt": "2024-02-15T00:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    });
    create_seed_entity(client, &body, "seed grouping history").await;
}

/// Seed a temporal entity whose `speed` history carries two `unitCode`s on a
/// single (default) instance series. Aggregating the series as one refuses the
/// request — the broker does not convert between `KMH` and `HM` — while grouping
/// by `unitCode` partitions it into two internally uniform, aggregatable groups.
pub async fn seed_mixed_unit_history(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10, "unitCode": "KMH", "observedAt": "2024-01-01T08:00:00Z"},
            {"type": "Property", "value": 20, "unitCode": "KMH", "observedAt": "2024-02-01T09:00:00Z"},
            {"type": "Property", "value": 30, "unitCode": "HM", "observedAt": "2024-03-01T10:00:00Z"},
            {"type": "Property", "value": 40, "unitCode": "HM", "observedAt": "2024-04-01T11:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    });
    create_seed_entity(client, &body, "seed mixed-unit history").await;
}

/// Seed a temporal entity whose `speed` history spans two calendar months on a
/// single instance series, with two January instants that a `observedAt;month`
/// grouping folds into one group. One series keeps the group count below the
/// configured bound.
pub async fn seed_monthly_history(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10, "observedAt": "2024-01-01T08:00:00Z"},
            {"type": "Property", "value": 15, "observedAt": "2024-01-15T08:30:00Z"},
            {"type": "Property", "value": 20, "observedAt": "2024-02-01T09:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    });
    create_seed_entity(client, &body, "seed monthly history").await;
}

/// Seed a temporal entity whose `speed` history repeats an hour across days on a
/// single instance series: two instants at hour 8 on different days fold into
/// one cyclic group, and one at hour 9 forms a second. One series keeps the
/// group count below the configured bound.
pub async fn seed_hourly_history(client: &TestClient, id: &str) {
    let body = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 10, "observedAt": "2024-01-01T08:00:00Z"},
            {"type": "Property", "value": 30, "observedAt": "2024-03-05T08:15:00Z"},
            {"type": "Property", "value": 20, "observedAt": "2024-01-02T09:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    });
    create_seed_entity(client, &body, "seed hourly history").await;
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
