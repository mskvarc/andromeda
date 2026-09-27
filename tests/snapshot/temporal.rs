//! Snapshot-scoped Temporal API acceptance cases.

use crate::support::{broker_url, clean_database, client, create_snapshot_raw, wait_for_snapshot_terminal};
use reqwest::{Method, Response, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const VEHICLE_TYPE: &str = "https://example.org/TemporalVehicle";
const REMOTE_BASE: &str = "http://127.0.0.1:1083/ngsi-ld/v1";

fn temporal_vehicle(id: &str, values: &[(i64, &str)]) -> Value {
    json!({
        "id": id,
        "type": VEHICLE_TYPE,
        "speed": values.iter().map(|(value, observed_at)| json!({"type": "Property", "value": value, "observedAt": observed_at})).collect::<Vec<_>>(),
        "@context": CORE_CONTEXT
    })
}

async fn empty_branch(id: &str) {
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
}

async fn scoped(method: Method, path: &str, snapshot: &str, body: Option<&Value>) -> Response {
    let mut request = client().request(method, broker_url(path)).header("NGSILD-Snapshot", snapshot);
    if let Some(body) = body {
        request = request.header("Content-Type", "application/ld+json").json(body);
    }
    request.send().await.expect("Snapshot-scoped Temporal request")
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_temporal_provision_matrix_mutates_only_branch_history() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:temporal-provision";
    let entity_id = "urn:ngsi-ld:TemporalVehicle:provision";
    empty_branch(snapshot).await;
    let initial = temporal_vehicle(entity_id, &[(10, "2025-01-01T00:00:00Z")]);
    pretty_assertions::assert_eq!(
        scoped(Method::POST, "/temporal/entities", snapshot, Some(&initial)).await.status(),
        StatusCode::CREATED
    );
    let addition = json!({"speed": [{"type": "Property", "value": 20, "observedAt": "2025-01-01T01:00:00Z"}], "@context": CORE_CONTEXT});
    pretty_assertions::assert_eq!(
        scoped(Method::POST, &format!("/temporal/entities/{entity_id}/attrs"), snapshot, Some(&addition))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let branch = scoped(Method::GET, &format!("/temporal/entities/{entity_id}"), snapshot, None).await;
    pretty_assertions::assert_eq!(branch.status(), StatusCode::OK);
    assert!(branch.json::<Value>().await.expect("branch temporal entity").to_string().contains("20"));
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url(&format!("/temporal/entities/{entity_id}")))
            .send()
            .await
            .expect("live temporal get")
            .status(),
        StatusCode::NOT_FOUND
    );
    pretty_assertions::assert_eq!(
        scoped(Method::DELETE, &format!("/temporal/entities/{entity_id}/attrs/speed"), snapshot, None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(
        scoped(Method::DELETE, &format!("/temporal/entities/{entity_id}"), snapshot, None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_temporal_retrieve_and_query_use_branch_history() {
    let _serialized_environment = clean_database().await;
    let first = "urn:ngsi-ld:Snapshot:temporal-first";
    let second = "urn:ngsi-ld:Snapshot:temporal-second";
    let entity_id = "urn:ngsi-ld:TemporalVehicle:isolated";
    empty_branch(first).await;
    empty_branch(second).await;
    pretty_assertions::assert_eq!(
        scoped(
            Method::POST,
            "/temporal/entities",
            first,
            Some(&temporal_vehicle(entity_id, &[(1, "2025-01-01T00:00:00Z")]))
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    pretty_assertions::assert_eq!(
        scoped(Method::GET, &format!("/temporal/entities/{entity_id}"), first, None).await.status(),
        StatusCode::OK
    );
    pretty_assertions::assert_eq!(
        scoped(Method::GET, &format!("/temporal/entities/{entity_id}"), second, None).await.status(),
        StatusCode::NOT_FOUND
    );
    let query = scoped(
        Method::GET,
        "/temporal/entities?type=https%3A%2F%2Fexample.org%2FTemporalVehicle&timerel=between&timeAt=2024-12-31T00%3A00%3A00Z&endTimeAt=2025-01-02T00%3A00%3A00Z",
        first,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(query.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(query.json::<Value>().await.expect("temporal query").as_array().map(Vec::len), Some(1));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_temporal_query_applies_time_projection_aggregation_and_pagination() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:temporal-query";
    empty_branch(snapshot).await;
    for suffix in ["a", "b"] {
        let body = temporal_vehicle(
            &format!("urn:ngsi-ld:TemporalVehicle:{suffix}"),
            &[(10, "2025-01-01T00:00:00Z"), (20, "2025-01-01T01:00:00Z")],
        );
        pretty_assertions::assert_eq!(
            scoped(Method::POST, "/temporal/entities", snapshot, Some(&body)).await.status(),
            StatusCode::CREATED
        );
    }
    let response = scoped(
        Method::GET,
        "/temporal/entities?type=https%3A%2F%2Fexample.org%2FTemporalVehicle&attrs=speed&options=aggregatedValues&timerel=between&timeAt=2025-01-01T00%3A00%3A00Z&endTimeAt=2025-01-01T02%3A00%3A00Z&aggrMethods=sum&aggrPeriodDuration=PT1H&limit=1&offset=1&count=true",
        snapshot,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(response.headers()["NGSILD-Results-Count"], "2");
    let body = response.json::<Value>().await.expect("aggregated temporal page");
    pretty_assertions::assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert!(
        body[0]["speed"]["sum"].as_array().is_some_and(|values| values.len() == 2),
        "aggregated temporal response: {body}"
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_temporal_operations_ignore_local_false_and_never_forward() {
    let _serialized_environment = clean_database().await;
    let snapshot = "urn:ngsi-ld:Snapshot:temporal-local-false";
    let remote_id = "urn:ngsi-ld:TemporalVehicle:remote-local-false";
    empty_branch(snapshot).await;
    let _ignored = client().delete(format!("{REMOTE_BASE}/temporal/entities/{remote_id}")).send().await;
    pretty_assertions::assert_eq!(
        client()
            .post(format!("{REMOTE_BASE}/temporal/entities"))
            .header("Content-Type", "application/ld+json")
            .json(&temporal_vehicle(remote_id, &[(99, "2025-01-01T00:00:00Z")]))
            .send()
            .await
            .expect("remote temporal local=false fixture")
            .status(),
        StatusCode::CREATED
    );
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:temporal-local-false",
        "type": "ContextSourceRegistration",
        "endpoint": "http://snapshot-remote:1083",
        "information": [{"entities": [{"type": VEHICLE_TYPE}]}],
        "operations": ["queryTemporal"],
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("matching temporal source registration")
            .status(),
        StatusCode::CREATED
    );
    let response = scoped(
        Method::POST,
        "/temporal/entities?local=false",
        snapshot,
        Some(&temporal_vehicle("urn:ngsi-ld:TemporalVehicle:local-false", &[(1, "2025-01-01T00:00:00Z")])),
    )
    .await;
    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(response.headers()["NGSILD-Snapshot"], snapshot);
    let query = scoped(
        Method::GET,
        "/temporal/entities?type=https%3A%2F%2Fexample.org%2FTemporalVehicle&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&local=false",
        snapshot,
        None,
    )
    .await;
    pretty_assertions::assert_eq!(query.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(query.headers()["NGSILD-Snapshot"], snapshot);
    let entities = query.json::<Value>().await.expect("Snapshot temporal local=false query body");
    pretty_assertions::assert_eq!(entities.as_array().map(Vec::len), Some(1));
    assert!(!entities.to_string().contains(remote_id));
}
