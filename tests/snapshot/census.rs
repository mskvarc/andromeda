//! The discovery census of Snapshot branches (clauses 5.5.15 and 5.7).
//!
//! A Snapshot branch is its own schema with its own census ledgers. A capture
//! counts what it stages, a write through `NGSILD-Snapshot` counts in the
//! branch only, and a clone copies the source branch's census beside the rows
//! it counts. Each step checks the branch's census against the recomputation
//! from its search rows, and the discovery answers against the entities the
//! branch holds.

use crate::support::{broker_url, clean_database, client, clone_snapshot_raw, create_entities_batch, wait_for_snapshot_terminal};
use andromeda_test_harness::discovery_census::census_drift;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const CAR: &str = "https://example.org/census/Car";
const BUS: &str = "https://example.org/census/Bus";
const SPEED: &str = "https://example.org/census/speed";
const COLOUR: &str = "https://example.org/census/colour";

fn segment(iri: &str) -> String {
    iri.replace(':', "%3A").replace('/', "%2F")
}

fn car(id: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": CAR,
        SPEED: [{"type": "Property", "value": speed}, {"type": "Property", "value": speed + 1, "datasetId": "urn:ngsi-ld:Dataset:gps"}],
        COLOUR: {"type": "Property", "value": "red"},
        "@context": CORE_CONTEXT
    })
}

/// One discovery request, against a Snapshot's branch when `snapshot` names one.
async fn request(method: Method, path: &str, snapshot: Option<&str>, body: Option<&Value>) -> (StatusCode, Value) {
    let mut builder = client().request(method, broker_url(path));
    if let Some(snapshot) = snapshot {
        builder = builder.header("NGSILD-Snapshot", snapshot);
    }
    if let Some(body) = body {
        builder = builder.header("Content-Type", "application/ld+json").json(body);
    }
    let response = builder.send().await.expect("request failed");
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

/// `(entityCount, attributeCount of speed, attributes of Car)` in one branch.
async fn car_answers(snapshot: Option<&str>) -> (Value, Value, Value) {
    let (_, info) = request(Method::GET, &format!("/types/{}", segment(CAR)), snapshot, None).await;
    let (_, speed) = request(Method::GET, &format!("/attributes/{}", segment(SPEED)), snapshot, None).await;
    let mut names: Vec<String> = info["attributeDetails"]
        .as_array()
        .map(|details| {
            details
                .iter()
                .filter_map(|detail| detail["attributeName"].as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    (info["entityCount"].clone(), speed["attributeCount"].clone(), json!(names))
}

/// The schema of a published Snapshot's branch.
async fn branch_schema(pool: &PgPool, snapshot: &str) -> String {
    let key: i64 = sqlx::query_scalar("SELECT snapshot_key FROM snapshots WHERE id = $1")
        .bind(snapshot)
        .fetch_one(pool)
        .await
        .expect("the Snapshot's key");
    format!("snap_{key}")
}

async fn assert_census_exact(pool: &PgPool, schema: &str, step: &str) {
    let drift = census_drift(pool, schema).await.expect("the census comparison");
    assert!(drift.is_empty(), "{step}: the census of {schema} drifted: {drift:?}");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_census_counts_captures_branch_writes_and_clones() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let ids: Vec<String> = (0..4).map(|n| format!("urn:ngsi-ld:CensusCar:{n}")).collect();
    let cars: Vec<Value> = ids.iter().zip(1..).map(|(id, speed)| car(id, speed)).collect();
    let (status, body) = create_entities_batch(&client(), &cars).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let source = "urn:ngsi-ld:Snapshot:census-capture";
    let capture = json!({
        "id": source,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": CAR}]}],
        "@context": CORE_CONTEXT
    });
    let (status, body) = request(Method::POST, "/snapshots", None, Some(&capture)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), source, Duration::from_secs(20)).await["snapshotStatus"],
        "success"
    );
    let source_schema = branch_schema(pool, source).await;
    assert_census_exact(pool, &source_schema, "the capture").await;
    pretty_assertions::assert_eq!(car_answers(Some(source)).await, car_answers(None).await, "the capture holds what live held");
    pretty_assertions::assert_eq!(car_answers(Some(source)).await.0, json!(4));
    pretty_assertions::assert_eq!(car_answers(Some(source)).await.1, json!(8), "two speed instances per car");

    let bus = json!({"id": "urn:ngsi-ld:CensusBus:1", "type": BUS, SPEED: {"type": "Property", "value": 3}, "@context": CORE_CONTEXT});
    pretty_assertions::assert_eq!(request(Method::POST, "/entities", Some(source), Some(&bus)).await.0, StatusCode::CREATED);
    let (status, _) = request(Method::DELETE, &format!("/entities/{}/attrs/{}", ids[0], segment(COLOUR)), Some(source), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    assert_census_exact(pool, &source_schema, "writes through the Snapshot header").await;
    assert_census_exact(pool, "public", "live beside the branch writes").await;
    let (status, bus_type) = request(Method::GET, &format!("/types/{}", segment(BUS)), Some(source), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "{bus_type}");
    pretty_assertions::assert_eq!(
        request(Method::GET, &format!("/types/{}", segment(BUS)), None, None).await.0,
        StatusCode::NOT_FOUND,
        "live never saw the bus"
    );
    pretty_assertions::assert_eq!(car_answers(Some(source)).await.1, json!(9), "the bus adds one speed instance in the branch");
    pretty_assertions::assert_eq!(car_answers(None).await.1, json!(8), "live is untouched");

    let target = "urn:ngsi-ld:Snapshot:census-clone";
    let (status, _, body) = clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), target, Duration::from_secs(20)).await["snapshotStatus"],
        "success"
    );
    let target_schema = branch_schema(pool, target).await;
    assert_census_exact(pool, &target_schema, "the clone").await;
    pretty_assertions::assert_eq!(
        car_answers(Some(target)).await,
        car_answers(Some(source)).await,
        "the clone answers what its source answers"
    );
    let (_, source_attributes) = request(Method::GET, "/attributes?details=true", Some(source), None).await;
    let (_, clone_attributes) = request(Method::GET, "/attributes?details=true", Some(target), None).await;
    pretty_assertions::assert_eq!(clone_attributes, source_attributes);

    environment.reset_and_finish().await.expect("reset");
}
