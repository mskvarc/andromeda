//! Snapshot capture, pagination, distribution, and terminal-status acceptance cases.

use super::*;

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

fn vehicle(id: impl Into<String>, speed: i64) -> Value {
    json!({
        "id": id.into(),
        "type": "Vehicle",
        "speed": {"type": "Property", "value": speed},
        "@context": [CORE_CONTEXT, {"Vehicle": "https://example.org/Vehicle", "speed": "https://example.org/speed"}]
    })
}

async fn create_capture(id: &str, queries: Value) -> Value {
    let body = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": queries,
        "@context": [CORE_CONTEXT, {"Vehicle": "https://example.org/Vehicle", "speed": "https://example.org/speed"}]
    });
    let response = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("contextual Snapshot create");
    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(20)).await
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_retrieves_every_current_query_page_without_gaps() {
    let _serialized_environment = clean_database().await;
    let entities = (0..1_025)
        .map(|index| vehicle(format!("urn:ngsi-ld:Vehicle:page-{index:04}"), index))
        .collect::<Vec<_>>();
    for batch in entities.chunks(100) {
        let (status, result) = create_entities_batch(&client(), batch).await;
        assert!(status.is_success(), "batch create failed with {status}: {result}");
    }
    let id = "urn:ngsi-ld:Snapshot:test-current-pages";
    let terminal = create_capture(id, json!([{"type": "Query", "entities": [{"type": "Vehicle"}]}])).await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "success");
    let (first_status, first_headers, first_page) =
        query_snapshot_entities(&client(), id, "type=https%3A%2F%2Fexample.org%2FVehicle&limit=1000&offset=0&count=true").await;
    pretty_assertions::assert_eq!(first_status, StatusCode::OK);
    pretty_assertions::assert_eq!(first_headers["NGSILD-Results-Count"], "1025");
    let (second_status, second_headers, second_page) =
        query_snapshot_entities(&client(), id, "type=https%3A%2F%2Fexample.org%2FVehicle&limit=1000&offset=1000&count=true").await;
    pretty_assertions::assert_eq!(second_status, StatusCode::OK);
    pretty_assertions::assert_eq!(second_headers["NGSILD-Results-Count"], "1025");
    let mut rows = first_page.as_array().expect("first captured entity page").clone();
    rows.extend(second_page.as_array().expect("second captured entity page").iter().cloned());
    pretty_assertions::assert_eq!(rows.len(), 1_025);
    let ids = rows
        .iter()
        .filter_map(|entity| entity["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    pretty_assertions::assert_eq!(ids.len(), 1_025);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_preserves_mixed_query_detail_order() {
    let _serialized_environment = clean_database().await;
    pretty_assertions::assert_eq!(create_entity(&client(), &vehicle("urn:ngsi-ld:Vehicle:ordered", 42)).await, StatusCode::CREATED);
    let id = "urn:ngsi-ld:Snapshot:test-detail-order";
    let terminal = create_capture(
        id,
        json!([
            {"type": "Query", "entities": [{"id": "urn:ngsi-ld:Vehicle:ordered", "type": "Vehicle"}]},
            {"type": "Query", "entities": [{"type": "AbsentType"}]},
            {"type": "Query", "entities": [{"type": "Vehicle"}]}
        ]),
    )
    .await;
    let details = terminal["snapshotQueriesDetails"].as_array().expect("ordered detail array");
    pretty_assertions::assert_eq!(details.len(), 3);
    pretty_assertions::assert_eq!(details[0]["resultStatus"], "success");
    pretty_assertions::assert_eq!(details[1]["resultStatus"], "empty");
    pretty_assertions::assert_eq!(details[2]["resultStatus"], "success");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_is_success_when_all_executions_succeed_and_any_result_exists() {
    let _serialized_environment = clean_database().await;
    pretty_assertions::assert_eq!(create_entity(&client(), &vehicle("urn:ngsi-ld:Vehicle:success", 8)).await, StatusCode::CREATED);
    let terminal = create_capture(
        "urn:ngsi-ld:Snapshot:test-success-status",
        json!([
            {"type": "Query", "entities": [{"type": "Vehicle"}]},
            {"type": "Query", "entities": [{"type": "AbsentType"}]}
        ]),
    )
    .await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "success");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][0]["resultStatus"], "success");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][1]["resultStatus"], "empty");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_is_empty_when_all_successful_executions_are_empty() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-empty-status";
    let terminal = create_capture(
        id,
        json!([
            {"type": "Query", "entities": [{"type": "AbsentOne"}]},
            {"type": "Query", "entities": [{"type": "AbsentTwo"}]}
        ]),
    )
    .await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "empty");
    pretty_assertions::assert_eq!(query_snapshot_entities(&client(), id, "type=Vehicle").await.2, json!([]));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_is_failure_when_no_execution_succeeds_or_exists() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-zero-executions";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "failure");
    let (status, headers, body) = query_snapshot_entities(&client(), id, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body, json!([]));
    pretty_assertions::assert_eq!(headers["ngsild-snapshot"], id);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_resolves_user_context_and_full_query_semantics() {
    let _serialized_environment = clean_database().await;
    pretty_assertions::assert_eq!(
        create_entity(&client(), &vehicle("urn:ngsi-ld:Vehicle:context-fast", 80)).await,
        StatusCode::CREATED
    );
    pretty_assertions::assert_eq!(
        create_entity(&client(), &vehicle("urn:ngsi-ld:Vehicle:context-slow", 20)).await,
        StatusCode::CREATED
    );
    let id = "urn:ngsi-ld:Snapshot:test-context-plan";
    let body = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"}],
        "@context": [CORE_CONTEXT, {"Vehicle": "https://example.org/Vehicle", "speed": "https://example.org/speed"}]
    });
    let response = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("contextual Snapshot create");
    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), id, Duration::from_secs(10)).await["snapshotStatus"],
        "success"
    );
    let captured = query_snapshot_entities(&client(), id, "type=https%3A%2F%2Fexample.org%2FVehicle").await.2;
    let rows = captured.as_array().expect("captured array");
    pretty_assertions::assert_eq!(rows.len(), 1);
    pretty_assertions::assert_eq!(rows[0]["id"], "urn:ngsi-ld:Vehicle:context-fast");
}
