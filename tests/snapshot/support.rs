//! Public-API fixtures shared by the focused Snapshot acceptance modules.

use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::{aggregate::count_all, statement::Statement};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, Method, Response, StatusCode, header::HeaderMap};
use sea_query::{Expr, ExprTrait, Query, TableName};
use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval, timeout};

/// The second broker process over the primary's database (`snapshot-replica`).
pub(crate) const REPLICA_BASE: &str = "http://127.0.0.1:1082/ngsi-ld/v1";

pub(crate) fn client() -> Client {
    Client::new()
}

pub(crate) fn broker_url(path: &str) -> String {
    format!("{BASE_URL}{path}")
}

pub(crate) async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

pub(crate) async fn create_snapshot_raw(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .post(format!("{BASE_URL}/snapshots"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("create Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(broker_url("/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create Entity request")
        .status()
}

pub(crate) async fn create_entities_batch(client: &Client, entities: &[Value]) -> (StatusCode, Value) {
    let response = client
        .post(broker_url("/entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(entities)
        .send()
        .await
        .expect("batch create Entities request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn query_snapshot_entities(client: &Client, snapshot_id: &str, query: &str) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .get(broker_url(&format!("/entities?{query}")))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("Snapshot-scoped Entity query");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn get_snapshot(client: &Client, id: &str) -> (StatusCode, Value) {
    let response = client.get(format!("{BASE_URL}/snapshots/{id}")).send().await.expect("get Snapshot request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn update_snapshot(client: &Client, id: &str, body: &Value) -> (StatusCode, Value) {
    let response = client
        .patch(format!("{BASE_URL}/snapshots/{id}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("patch Snapshot request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn clone_snapshot_raw(client: &Client, source_id: &str, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .post(broker_url(&format!("/snapshots/{source_id}/clone")))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("clone Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn delete_snapshot_raw(client: &Client, id: &str) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .delete(broker_url(&format!("/snapshots/{id}")))
        .send()
        .await
        .expect("delete Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

/// One request scoped to a Snapshot through `NGSILD-Snapshot`, carrying `body`
/// as JSON-LD when there is one.
pub(crate) async fn scoped(method: Method, path: &str, snapshot: &str, body: Option<&Value>) -> Response {
    let mut request = client().request(method, broker_url(path)).header("NGSILD-Snapshot", snapshot);
    if let Some(body) = body {
        request = request.header("Content-Type", "application/ld+json").json(body);
    }
    request.send().await.expect("Snapshot-scoped request")
}

/// One request scoped to a Snapshot through `NGSILD-Snapshot`, carrying `body`
/// as plain JSON, as a body that cannot name a `@context` is sent (clause
/// 6.3.5).
pub(crate) async fn scoped_json(method: Method, path: &str, snapshot: &str, body: &Value) -> Response {
    client()
        .request(method, broker_url(path))
        .header("NGSILD-Snapshot", snapshot)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("Snapshot-scoped request")
}

/// The control-table key of the Snapshot `snapshot_id`.
pub(crate) async fn snapshot_key(pool: &PgPool, snapshot_id: &str) -> i64 {
    Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(snapshot_id))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the Snapshot's key")
}

/// The schema holding a published Snapshot's branch, `snap_<snapshot_key>`.
pub(crate) async fn published_branch_schema(pool: &PgPool, snapshot_id: &str) -> String {
    format!("snap_{}", snapshot_key(pool, snapshot_id).await)
}

/// The number of rows `relation` holds.
pub(crate) async fn relation_row_count(pool: &PgPool, relation: TableName) -> i64 {
    Query::select()
        .expr(count_all())
        .from(relation)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the relation's row count")
}

pub(crate) async fn wait_for_snapshot_terminal(client: &Client, id: &str, deadline: Duration) -> Value {
    timeout(deadline, async {
        let mut changes = interval(Duration::from_millis(20));
        changes.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            changes.tick().await;
            let (status, snapshot) = get_snapshot(client, id).await;
            pretty_assertions::assert_eq!(status, StatusCode::OK);
            if snapshot["snapshotStatus"] != "preparing" {
                return snapshot;
            }
        }
    })
    .await
    .expect("Snapshot reaches a terminal state before the test deadline")
}

pub(crate) async fn assert_snapshot_create_rejected(client: &Client, body: Value) {
    let (status, _, problem) = create_snapshot_raw(client, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/BadRequestData", StatusCode::BAD_REQUEST);
}

pub(crate) fn assert_problem_details(problem: &Value, problem_type: &str, status: StatusCode) {
    pretty_assertions::assert_eq!(problem["type"], problem_type);
    pretty_assertions::assert_eq!(problem["status"], status.as_u16());
    assert!(problem["title"].as_str().is_some_and(|title| !title.is_empty()));
    assert!(problem["detail"].as_str().is_some_and(|detail| !detail.is_empty()));
    assert!(problem["instance"].as_str().is_some_and(|instance| !instance.is_empty()));
}
