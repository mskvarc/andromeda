//! Snapshot capture, pagination, distribution, and terminal-status acceptance cases.

use crate::support::{
    broker_url,
    clean_database,
    client,
    create_entities_batch,
    create_entity,
    create_snapshot_raw,
    query_snapshot_entities,
    wait_for_snapshot_terminal,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use std::{collections::BTreeSet, time::Duration};

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const REMOTE_BASE: &str = "http://127.0.0.1:1083/ngsi-ld/v1";

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

async fn register_unreachable_entity_source(entity_type: &str, suffix: &str) {
    let registration = json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{suffix}"),
        "type": "ContextSourceRegistration",
        "endpoint": "http://host.docker.internal:1",
        "information": [{"entities": [{"type": entity_type}]}],
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
            .expect("unreachable source registration")
            .status(),
        StatusCode::CREATED
    );
}

async fn register_remote_source(entity_type: &str, suffix: &str, operations: &[&str]) {
    let registration = json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{suffix}"),
        "type": "ContextSourceRegistration",
        "endpoint": "http://snapshot-remote:1083",
        "information": [{"entities": [{"type": entity_type}]}],
        "operations": operations,
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("remote source registration")
            .status(),
        StatusCode::CREATED
    );
}

async fn create_remote_entity(body: &Value) {
    if let Some(id) = body["id"].as_str() {
        let _ = client().delete(format!("{REMOTE_BASE}/entities/{id}")).send().await;
    }
    pretty_assertions::assert_eq!(
        client()
            .post(format!("{REMOTE_BASE}/entities"))
            .header("Content-Type", "application/ld+json")
            .json(body)
            .send()
            .await
            .expect("remote entity create")
            .status(),
        StatusCode::CREATED
    );
}

async fn create_temporal_at(base: &str, body: &Value) {
    if let Some(id) = body["id"].as_str() {
        let _ = client().delete(format!("{base}/temporal/entities/{id}")).send().await;
    }
    pretty_assertions::assert_eq!(
        client()
            .post(format!("{base}/temporal/entities"))
            .header("Content-Type", "application/ld+json")
            .json(body)
            .send()
            .await
            .expect("temporal entity create")
            .status(),
        StatusCode::CREATED
    );
}

async fn create_temporal_capture(id: &str, queries: Value) -> Value {
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotTemporalQueries": queries}),)
            .await
            .0,
        StatusCode::CREATED
    );
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(30)).await
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
    let ids = rows.iter().filter_map(|entity| entity["id"].as_str()).collect::<BTreeSet<_>>();
    pretty_assertions::assert_eq!(ids.len(), 1_025);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_integrates_local_and_distributed_current_entities() {
    let _serialized_environment = clean_database().await;
    let entity_type = "https://example.org/DistributedVehicle";
    let local = json!({"id": "urn:ngsi-ld:DistributedVehicle:local", "type": entity_type, "speed": {"type": "Property", "value": 1}, "@context": CORE_CONTEXT});
    let remote =
        json!({"id": "urn:ngsi-ld:DistributedVehicle:remote", "type": entity_type, "speed": {"type": "Property", "value": 2}, "@context": CORE_CONTEXT});
    pretty_assertions::assert_eq!(create_entity(&client(), &local).await, StatusCode::CREATED);
    create_remote_entity(&remote).await;
    register_remote_source(entity_type, "distributed-current", &["queryEntity"]).await;
    let id = "urn:ngsi-ld:Snapshot:distributed-current";
    let terminal = create_capture(id, json!([{"type": "Query", "entities": [{"type": entity_type}]}])).await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "success");
    let (_, _, body) = query_snapshot_entities(&client(), id, &format!("type={}", urlencoding::encode(entity_type))).await;
    let ids = body
        .as_array()
        .expect("captured entities")
        .iter()
        .filter_map(|entity| entity["id"].as_str())
        .collect::<BTreeSet<_>>();
    pretty_assertions::assert_eq!(
        ids,
        BTreeSet::from(["urn:ngsi-ld:DistributedVehicle:local", "urn:ngsi-ld:DistributedVehicle:remote"])
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_merges_overlapping_entities_and_dataset_instances() {
    let _serialized_environment = clean_database().await;
    let entity_type = "https://example.org/MergedVehicle";
    let entity_id = "urn:ngsi-ld:MergedVehicle:shared";
    let context = json!([CORE_CONTEXT, {"speed": "https://example.org/speed"}]);
    let local = json!({"id": entity_id, "type": entity_type, "speed": {"type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:Dataset:local"}, "@context": context});
    let remote = json!({"id": entity_id, "type": entity_type, "speed": {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:Dataset:remote"}, "@context": context});
    pretty_assertions::assert_eq!(create_entity(&client(), &local).await, StatusCode::CREATED);
    create_remote_entity(&remote).await;
    register_remote_source(entity_type, "merged-current", &["queryEntity"]).await;
    let id = "urn:ngsi-ld:Snapshot:merged-current";
    pretty_assertions::assert_eq!(
        create_capture(id, json!([{"type": "Query", "entities": [{"type": entity_type}]}])).await["snapshotStatus"],
        "success"
    );
    let response = client()
        .get(broker_url(&format!("/entities/{entity_id}")))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("retrieve merged capture");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("merged entity");
    let instances = body["https://example.org/speed"].as_array().expect("multiple dataset instances");
    let datasets = instances.iter().filter_map(|instance| instance["datasetId"].as_str()).collect::<BTreeSet<_>>();
    pretty_assertions::assert_eq!(datasets, BTreeSet::from(["urn:ngsi-ld:Dataset:local", "urn:ngsi-ld:Dataset:remote"]));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_integrates_distributed_temporal_evolutions() {
    let _serialized_environment = clean_database().await;
    let entity_type = "https://example.org/DistributedTemporalVehicle";
    create_temporal_at(
        &broker_url(""),
        &json!({"id": "urn:ngsi-ld:DistributedTemporalVehicle:local", "type": entity_type, "speed": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}], "@context": CORE_CONTEXT}),
    )
    .await;
    create_temporal_at(
        REMOTE_BASE,
        &json!({"id": "urn:ngsi-ld:DistributedTemporalVehicle:remote", "type": entity_type, "speed": [{"type": "Property", "value": 2, "observedAt": "2025-01-01T01:00:00Z"}], "@context": CORE_CONTEXT}),
    )
    .await;
    register_remote_source(entity_type, "distributed-temporal", &["queryTemporal"]).await;
    let id = "urn:ngsi-ld:Snapshot:distributed-temporal";
    let terminal = create_temporal_capture(
        id,
        json!([{"type": "Query", "entities": [{"type": entity_type}], "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}}]),
    )
    .await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "success");
    let response = client()
        .get(broker_url(&format!(
            "/temporal/entities?type={}&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z",
            urlencoding::encode(entity_type)
        )))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("query distributed temporal capture");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(response.json::<Value>().await.expect("temporal capture").as_array().map(Vec::len), Some(2));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_capture_retrieves_every_temporal_entity_and_range_page() {
    let _serialized_environment = clean_database().await;
    let entity_type = "https://example.org/PagedTemporalVehicle";
    for index in 0..251 {
        create_temporal_at(
            &broker_url(""),
            &json!({"id": format!("urn:ngsi-ld:PagedTemporalVehicle:{index:04}"), "type": entity_type, "speed": [{"type": "Property", "value": index, "observedAt": "2025-01-01T00:00:00Z"}], "@context": CORE_CONTEXT}),
        )
        .await;
    }
    let id = "urn:ngsi-ld:Snapshot:paged-temporal";
    pretty_assertions::assert_eq!(
        create_temporal_capture(
            id,
            json!([{"type": "Query", "entities": [{"type": entity_type}], "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}}]),
        )
        .await["snapshotStatus"],
        "success"
    );
    let response = client()
        .get(broker_url(&format!(
            "/temporal/entities?type={}&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&limit=250&offset=0&count=true",
            urlencoding::encode(entity_type)
        )))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("first temporal range page");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(response.headers()["NGSILD-Results-Count"], "251");
    pretty_assertions::assert_eq!(response.json::<Value>().await.expect("first temporal page").as_array().map(Vec::len), Some(250));
    let response = client()
        .get(broker_url(&format!(
            "/temporal/entities?type={}&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&limit=250&offset=250&count=true",
            urlencoding::encode(entity_type)
        )))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("second temporal range page");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(response.json::<Value>().await.expect("second temporal page").as_array().map(Vec::len), Some(1));
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
async fn snapshot_status_is_partial_when_nonempty_success_coexists_with_failure() {
    let _serialized_environment = clean_database().await;
    let failed_type = "https://example.org/FailedPartialVehicle";
    register_unreachable_entity_source(failed_type, "partial-status").await;
    pretty_assertions::assert_eq!(
        create_entity(&client(), &vehicle("urn:ngsi-ld:Vehicle:partial-status", 7)).await,
        StatusCode::CREATED
    );
    let terminal = create_capture(
        "urn:ngsi-ld:Snapshot:partial-status",
        json!([
            {"type": "Query", "entities": [{"type": "Vehicle"}]},
            {"type": "Query", "entities": [{"type": failed_type}]}
        ]),
    )
    .await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "partial");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][0]["resultStatus"], "success");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][1]["resultStatus"], "failure");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_is_empty_when_empty_success_coexists_with_failure() {
    let _serialized_environment = clean_database().await;
    let failed_type = "https://example.org/FailedEmptyVehicle";
    register_unreachable_entity_source(failed_type, "empty-status").await;
    let terminal = create_capture(
        "urn:ngsi-ld:Snapshot:empty-with-failure-status",
        json!([
            {"type": "Query", "entities": [{"type": "AbsentType"}]},
            {"type": "Query", "entities": [{"type": failed_type}]}
        ]),
    )
    .await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "empty");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][0]["resultStatus"], "empty");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"][1]["resultStatus"], "failure");
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
