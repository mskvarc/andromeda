//! Snapshot integration tests for the NGSI-LD context broker.
//!
//! Covers the full Snapshot lifecycle (spec 5.16, 6.36-6.38), status
//! notifications (spec 5.16.6, 5.3.4), and the NGSILD-Snapshot header
//! scoping of entity and temporal queries (spec 5.5.15, 6.3.22).
//!
//! Naming convention: `snapshot_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{db::TRUNCATE_ALL, prelude::*};
use axum::{body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Infrastructure
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

// ---------------------------------------------------------------------------
// Snapshot HTTP helpers
// ---------------------------------------------------------------------------

async fn create_snapshot(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/snapshots"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("create_snapshot request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn get_snapshot(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/snapshots/{id}"))
        .send()
        .await
        .expect("get_snapshot request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn update_snapshot(client: &Client, id: &str, body: &Value) -> (StatusCode, Value) {
    let resp = client
        .patch(format!("{BASE_URL}/snapshots/{id}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("update_snapshot request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_snapshot(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/snapshots/{id}"))
        .send()
        .await
        .expect("delete_snapshot request failed")
        .status()
}

async fn purge_snapshots(client: &Client) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/snapshots"))
        .send()
        .await
        .expect("purge_snapshots request failed")
        .status()
}

async fn purge_snapshots_with_q(client: &Client, q: &str) -> (StatusCode, Value) {
    let resp = client
        .delete(format!("{BASE_URL}/snapshots?q={}", urlencoding::encode(q)))
        .send()
        .await
        .expect("purge_snapshots_with_q request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn clone_snapshot(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/snapshots/{id}/clone"))
        .send()
        .await
        .expect("clone_snapshot request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

// ---------------------------------------------------------------------------
// Entity + temporal query helpers (for scoping tests)
// ---------------------------------------------------------------------------

async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed")
        .status()
}

async fn get_entity_scoped(client: &Client, id: &str, attrs: Option<&str>, snapshot_id: &str) -> (StatusCode, HeaderMap, Value) {
    let url = match attrs {
        Some(a) => format!("{BASE_URL}/entities/{id}?attrs={a}"),
        None => format!("{BASE_URL}/entities/{id}"),
    };
    let resp = client
        .get(url)
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_entity_scoped request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn query_entities_scoped(client: &Client, type_filter: Option<&str>, snapshot_id: &str) -> (StatusCode, HeaderMap, Value) {
    let qs = type_filter.map(|t| format!("?type={t}")).unwrap_or_default();
    let resp = client
        .get(format!("{BASE_URL}/entities{qs}"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("query_entities_scoped request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn query_temporal_entities_scoped(client: &Client, type_filter: Option<&str>, snapshot_id: &str) -> (StatusCode, HeaderMap, Value) {
    let qs = type_filter.map(|t| format!("?type={t}")).unwrap_or_default();
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities{qs}"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("query_temporal_entities_scoped request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn post_batch_entity_query_scoped(client: &Client, body: &Value, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityOperations/query"))
        .header("Content-Type", "application/json")
        .header("NGSILD-Snapshot", snapshot_id)
        .json(body)
        .send()
        .await
        .expect("post_batch_entity_query_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn post_batch_temporal_query_scoped(client: &Client, body: &Value, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entityOperations/query"))
        .header("Content-Type", "application/json")
        .header("NGSILD-Snapshot", snapshot_id)
        .json(body)
        .send()
        .await
        .expect("post_batch_temporal_query_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_types_scoped(client: &Client, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/types"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_types_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_type_scoped(client: &Client, type_name: &str, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/types/{type_name}"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_type_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_attributes_scoped(client: &Client, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/attributes"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_attributes_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_attribute_scoped(client: &Client, attr_name: &str, snapshot_id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/attributes/{attr_name}"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_attribute_scoped request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_temporal_entity_scoped(client: &Client, id: &str, snapshot_id: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities/{id}"))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("get_temporal_entity_scoped request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

// ---------------------------------------------------------------------------
// Database seeding helpers (for scoping tests)
// ---------------------------------------------------------------------------

async fn seed_snapshot_entities(pool: &PgPool, snapshot_id: &str, entities: &[(String, Value)]) {
    for (entity_id, entity_data) in entities {
        sqlx::query(
            "INSERT INTO snapshot_entities (snapshot_id, entity_id, entity_data) \
             VALUES ($1, $2, $3)",
        )
        .bind(snapshot_id)
        .bind(entity_id)
        .bind(entity_data)
        .execute(pool)
        .await
        .expect("failed to insert snapshot entity");
    }
    sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
        .bind(snapshot_id)
        .execute(pool)
        .await
        .expect("failed to update snapshot status");
}

async fn seed_snapshot_temporal_entities(pool: &PgPool, snapshot_id: &str, entities: &[(String, Value)]) {
    for (entity_id, temporal_data) in entities {
        sqlx::query(
            "INSERT INTO snapshot_temporal_entities (snapshot_id, entity_id, temporal_data) \
             VALUES ($1, $2, $3)",
        )
        .bind(snapshot_id)
        .bind(entity_id)
        .bind(temporal_data)
        .execute(pool)
        .await
        .expect("failed to insert snapshot temporal entity");
    }
    sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
        .bind(snapshot_id)
        .execute(pool)
        .await
        .expect("failed to update snapshot status");
}

// ---------------------------------------------------------------------------
// Notification receiver (for notification tests)
// ---------------------------------------------------------------------------

pub struct CapturedNotification {
    pub headers: HeaderMap,
    pub body: Value,
}

pub struct NotificationReceiver {
    pub port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    notifications: Arc<Mutex<Vec<CapturedNotification>>>,
    notify: Arc<Notify>,
}

impl NotificationReceiver {
    pub async fn start() -> Self {
        let notifications: Arc<Mutex<Vec<CapturedNotification>>> = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let notifs = notifications.clone();
        let ntf = notify.clone();

        let app = axum::Router::new().route(
            "/notify",
            post(move |headers: AxumHeaderMap, body: Bytes| {
                let notifs = notifs.clone();
                let ntf = ntf.clone();
                async move {
                    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let mut reqwest_headers = HeaderMap::new();
                    for (k, v) in &headers {
                        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes())
                            && let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes())
                        {
                            reqwest_headers.insert(name, val);
                        }
                    }
                    notifs.lock().await.push(CapturedNotification {
                        headers: reqwest_headers,
                        body: json,
                    });
                    ntf.notify_one();
                    axum::http::StatusCode::OK.into_response()
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self {
            port,
            shutdown: Some(shutdown_tx),
            notifications,
            notify,
        }
    }

    pub fn endpoint_url(&self) -> String {
        callback_url(self.port, "/notify")
    }

    pub async fn wait_for_notification(&self, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut notifs = self.notifications.lock().await;
                if !notifs.is_empty() {
                    return notifs.remove(0);
                }
            }
            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                unreachable!("timed out waiting for notification after {timeout:?}");
            }
        }
    }

    pub async fn wait_for_no_notification(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
        let notifs = self.notifications.lock().await;
        assert!(notifs.is_empty(), "expected no notifications but received {}", notifs.len());
    }

    fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for NotificationReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// API tests (spec 5.16, 6.36-6.38)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn snapshot_api_create_returns_201_with_location() {
    clean_database().await;
    let c = client();

    let body = json!({"snapshotLifetime": "PT1H", "snapshotPriority": 5});
    let (status, headers, resp_body) = create_snapshot(&c, &body).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let location = headers.get("location").and_then(|v| v.to_str().ok());
    assert!(location.is_some_and(|l| l.contains("/snapshots/")), "Location header must contain /snapshots/");
    pretty_assertions::assert_eq!(resp_body["status"], "preparing");
    pretty_assertions::assert_eq!(resp_body["snapshotPriority"], 5);

    delete_snapshot(&c, resp_body["id"].as_str().unwrap()).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_create_with_custom_id_uses_provided_id() {
    clean_database().await;
    let c = client();

    let custom_id = format!("urn:ngsi-ld:Snapshot:custom-{}", Uuid::new_v4());
    let body = json!({"id": custom_id, "snapshotLifetime": "PT30M"});
    let (status, _, resp_body) = create_snapshot(&c, &body).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(resp_body["id"], custom_id);

    delete_snapshot(&c, &custom_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_create_duplicate_returns_409() {
    clean_database().await;
    let c = client();

    let id = format!("urn:ngsi-ld:Snapshot:dup-{}", Uuid::new_v4());
    let body = json!({"id": id, "snapshotLifetime": "PT1H"});
    let (status, _, _) = create_snapshot(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = create_snapshot(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);

    delete_snapshot(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_get_returns_200_with_metadata() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, body) = get_snapshot(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["id"], snap_id);
    assert!(body.get("createdAt").is_some(), "must have createdAt");
    assert!(body.get("expiresAt").is_some(), "must have expiresAt");

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_get_nonexistent_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _) = get_snapshot(&c, "urn:ngsi-ld:Snapshot:doesnotexist").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_update_lifetime_and_priority_returns_200() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 3})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, updated) = update_snapshot(&c, &snap_id, &json!({"snapshotLifetime": "PT2H", "snapshotPriority": 8})).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(updated["snapshotPriority"], 8);

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_update_invalid_priority_returns_400() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, _) = update_snapshot(&c, &snap_id, &json!({"snapshotPriority": 99})).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_delete_returns_204_and_removes_snapshot() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    pretty_assertions::assert_eq!(delete_snapshot(&c, &snap_id).await, StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_delete_nonexistent_returns_404() {
    clean_database().await;
    let c = client();

    pretty_assertions::assert_eq!(delete_snapshot(&c, "urn:ngsi-ld:Snapshot:ghost").await, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_clone_returns_201_with_new_id_and_inherited_priority() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 7})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, cloned) = clone_snapshot(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let cloned_id = cloned["id"].as_str().unwrap().to_string();
    pretty_assertions::assert_ne!(cloned_id, snap_id, "cloned snapshot must have a different id");
    pretty_assertions::assert_eq!(cloned["snapshotPriority"], 7, "clone must inherit priority");

    pretty_assertions::assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::OK);
    pretty_assertions::assert_eq!(get_snapshot(&c, &cloned_id).await.0, StatusCode::OK);

    delete_snapshot(&c, &snap_id).await;
    delete_snapshot(&c, &cloned_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_without_q_returns_400() {
    // Clause 5.16.7 / 6.36.3.2: DELETE /snapshots requires a `q` query parameter.
    clean_database().await;
    let c = client();

    for _ in 0..2 {
        let (status, _, _) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    pretty_assertions::assert_eq!(purge_snapshots(&c).await, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_no_match_returns_404() {
    // Clause 6.36.3.2: no matching snapshot -> 404 ResourceNotFound.
    clean_database().await;
    let c = client();

    let (status, _, _) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 5})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = purge_snapshots_with_q(&c, "snapshotPriority==10").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"));
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_full_match_returns_204() {
    // Clause 6.36.3.2: all matched snapshots deleted -> 204.
    clean_database().await;
    let c = client();

    for _ in 0..3 {
        let (status, _, _) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 5})).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, _) = purge_snapshots_with_q(&c, "snapshotPriority==5").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // Subsequent purge with same filter should now return 404.
    let (status, _) = purge_snapshots_with_q(&c, "snapshotPriority==5").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_malformed_q_returns_400() {
    // Clause 6.3.2: invalid `q` expression -> 400 BadRequestData.
    clean_database().await;
    let c = client();

    let (status, body) = purge_snapshots_with_q(&c, "bogusField==value").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"));
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_by_priority_does_not_touch_other_priorities() {
    // Clause 5.16.7: purge must only delete snapshots matching the filter.
    clean_database().await;
    let c = client();

    let (_, _, keep) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 9})).await;
    let keep_id = keep["id"].as_str().unwrap().to_string();

    for _ in 0..2 {
        let (status, _, _) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 1})).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, _) = purge_snapshots_with_q(&c, "snapshotPriority==1").await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = get_snapshot(&c, &keep_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "non-matching snapshot must survive purge");

    delete_snapshot(&c, &keep_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_default_priority_is_5() {
    clean_database().await;
    let c = client();

    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(created["snapshotPriority"], 5);

    delete_snapshot(&c, created["id"].as_str().unwrap()).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_full_lifecycle_create_get_update_clone_delete() {
    clean_database().await;
    let c = client();

    // 1. Create
    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 3})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    // 2. Get
    let (status, got) = get_snapshot(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(got["snapshotPriority"], 3);

    // 3. Update
    let (status, updated) = update_snapshot(&c, &snap_id, &json!({"snapshotPriority": 9})).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(updated["snapshotPriority"], 9);

    // 4. Clone
    let (status, cloned) = clone_snapshot(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let cloned_id = cloned["id"].as_str().unwrap().to_string();
    pretty_assertions::assert_eq!(cloned["snapshotPriority"], 9, "clone must inherit updated priority");

    // 5. Delete both
    pretty_assertions::assert_eq!(delete_snapshot(&c, &snap_id).await, StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(delete_snapshot(&c, &cloned_id).await, StatusCode::NO_CONTENT);

    // 6. Verify gone
    pretty_assertions::assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(get_snapshot(&c, &cloned_id).await.0, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Notification tests (spec 5.16.6, 5.3.4)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn snapshot_notification_create_sends_notification_with_correct_fields() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(
        &c,
        &json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 7,
            "endpoint": receiver.endpoint_url()
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notif.body["type"], "SnapshotNotification");
    pretty_assertions::assert_eq!(notif.body["snapshotId"], snap_id);
    pretty_assertions::assert_eq!(notif.body["snapshotStatus"], "preparing");
    pretty_assertions::assert_eq!(notif.body["snapshotPriority"], 7);
    assert!(notif.body["expiresAt"].is_string(), "expiresAt must be present");
    assert!(notif.body["notifiedAt"].is_string(), "notifiedAt must be present");
    assert!(
        notif.body["id"].as_str().unwrap().starts_with("urn:ngsi-ld:SnapshotNotification:"),
        "notification id must follow URN pattern"
    );

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_notification_create_without_endpoint_sends_no_notification() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 5})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, _) = update_snapshot(&c, &snap_id, &json!({"snapshotPriority": 8})).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    receiver.wait_for_no_notification(Duration::from_secs(2)).await;

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_notification_update_sends_notification_with_updated_fields() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(
        &c,
        &json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 3,
            "endpoint": receiver.endpoint_url()
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    // Drain the create notification.
    let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let (status, _) = update_snapshot(&c, &snap_id, &json!({"snapshotLifetime": "PT2H", "snapshotPriority": 8})).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notif.body["type"], "SnapshotNotification");
    pretty_assertions::assert_eq!(notif.body["snapshotId"], snap_id);
    pretty_assertions::assert_eq!(notif.body["snapshotPriority"], 8, "notification must reflect updated priority");
    assert!(notif.body["expiresAt"].is_string(), "expiresAt must be present");
    assert!(notif.body["notifiedAt"].is_string(), "notifiedAt must be present");

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_notification_receiver_info_headers_forwarded_to_endpoint() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(
        &c,
        &json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 5,
            "endpoint": receiver.endpoint_url(),
            "receiverInfo": [{"key": "X-Custom-Header", "value": "test-value-123"}]
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notif.body["type"], "SnapshotNotification");

    let custom = notif.headers.get("x-custom-header").expect("X-Custom-Header must be forwarded to endpoint");
    pretty_assertions::assert_eq!(custom.to_str().unwrap(), "test-value-123");

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_notification_clone_sends_notification_for_cloned_snapshot() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(
        &c,
        &json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 6,
            "endpoint": receiver.endpoint_url()
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let source_id = created["id"].as_str().unwrap().to_string();

    // Drain the create notification.
    let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let (status, cloned) = clone_snapshot(&c, &source_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let cloned_id = cloned["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notif.body["type"], "SnapshotNotification");
    pretty_assertions::assert_eq!(notif.body["snapshotId"], cloned_id, "notification must reference the cloned snapshot");
    pretty_assertions::assert_ne!(
        notif.body["snapshotId"].as_str().unwrap(),
        source_id.as_str(),
        "notification must not reference the source snapshot"
    );
    pretty_assertions::assert_eq!(notif.body["snapshotPriority"], 6, "clone must inherit source priority");

    delete_snapshot(&c, &source_id).await;
    delete_snapshot(&c, &cloned_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_notification_clone_without_endpoint_sends_no_notification() {
    clean_database().await;
    let c = client();
    let receiver = NotificationReceiver::start().await;

    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 4})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let source_id = created["id"].as_str().unwrap().to_string();

    let (status, cloned) = clone_snapshot(&c, &source_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let cloned_id = cloned["id"].as_str().unwrap().to_string();

    receiver.wait_for_no_notification(Duration::from_secs(2)).await;

    delete_snapshot(&c, &source_id).await;
    delete_snapshot(&c, &cloned_id).await;
}

// ---------------------------------------------------------------------------
// Scoping tests (spec 5.5.15, 6.3.22)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn snapshot_scoping_query_entities_returns_snapshot_entities() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid1 = unique_id("urn:ngsi-ld:Vehicle:snap-q");
    let eid2 = unique_id("urn:ngsi-ld:Vehicle:snap-q");
    let e1 = json!({"id": eid1, "type": "Vehicle", "speed": {"type": "Property", "value": 80}});
    let e2 = json!({"id": eid2, "type": "Vehicle", "speed": {"type": "Property", "value": 120}});
    seed_snapshot_entities(&pool, &snap_id, &[(eid1.clone(), e1), (eid2.clone(), e2)]).await;

    let (status, headers, body) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");

    let entities = body.as_array().expect("response must be array");
    pretty_assertions::assert_eq!(entities.len(), 2, "must return exactly 2 snapshot entities");
    let ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&eid1.as_str()));
    assert!(ids.contains(&eid2.as_str()));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_get_entity_scoped_to_snapshot_returns_entity() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-get");
    let entity = json!({"id": eid, "type": "Vehicle", "speed": {"type": "Property", "value": 55}});
    seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), entity)]).await;

    let (status, headers, body) = get_entity_scoped(&c, &eid, None, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");
    pretty_assertions::assert_eq!(body["id"], eid);
    pretty_assertions::assert_eq!(body["type"], "Vehicle");

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_get_live_entity_not_in_snapshot_returns_404() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let snap_eid = unique_id("urn:ngsi-ld:Vehicle:snap-exists");
    seed_snapshot_entities(&pool, &snap_id, &[(snap_eid.clone(), json!({"id": snap_eid, "type": "Vehicle"}))]).await;

    // Live entity must not be visible via snapshot scoping.
    let live_eid = unique_id("urn:ngsi-ld:Vehicle:live");
    create_entity(
        &c,
        &json!({
            "id": live_eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 99},
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let (status, _, _) = get_entity_scoped(&c, &live_eid, None, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "live entity must not be visible in snapshot scope");

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_nonexistent_snapshot_id_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _, _) = query_entities_scoped(&c, Some("Vehicle"), "urn:ngsi-ld:Snapshot:doesnotexist").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_snapshot_isolates_from_live_entities() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let snap_eid = unique_id("urn:ngsi-ld:Vehicle:snap-local");
    let snap_entity = json!({
        "id": snap_eid,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 42}
    });
    seed_snapshot_entities(&pool, &snap_id, &[(snap_eid.clone(), snap_entity)]).await;

    let live_eid = unique_id("urn:ngsi-ld:Vehicle:live-local");
    create_entity(
        &c,
        &json!({
            "id": live_eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 200},
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let (status, _, body) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response must be array");
    let ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&snap_eid.as_str()), "snapshot entity must be present");
    assert!(!ids.contains(&live_eid.as_str()), "live entity must not appear in snapshot-scoped query");

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_query_with_type_filter_returns_matching_entities_only() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let vid = unique_id("urn:ngsi-ld:Vehicle:snap-type");
    let bid = unique_id("urn:ngsi-ld:Building:snap-type");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[
            (vid.clone(), json!({"id": vid, "type": "Vehicle", "speed": {"type": "Property", "value": 60}})),
            (bid.clone(), json!({"id": bid, "type": "Building", "name": {"type": "Property", "value": "HQ"}})),
        ],
    )
    .await;

    let (status, _, body) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response must be array");
    pretty_assertions::assert_eq!(entities.len(), 1, "type filter must return only Vehicle entities");
    pretty_assertions::assert_eq!(entities[0]["id"], vid);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_get_entity_with_attrs_filter_projects_attributes() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-attrs");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[(
            eid.clone(),
            json!({
                "id": eid,
                "type": "Vehicle",
                "speed": {"type": "Property", "value": 80},
                "fuelLevel": {"type": "Property", "value": 50}
            }),
        )],
    )
    .await;

    let (status, _, body) = get_entity_scoped(&c, &eid, Some("speed"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed must be present");
    assert!(body.get("fuelLevel").is_none(), "fuelLevel must be filtered out by attrs");
    pretty_assertions::assert_eq!(body["id"], eid);
    pretty_assertions::assert_eq!(body["type"], "Vehicle");

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_query_temporal_entities_scoped_to_snapshot() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
        .bind(&snap_id)
        .execute(&pool)
        .await
        .unwrap();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-temp");
    let temporal = json!({
        "id": eid,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 80, "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": 90, "observedAt": "2024-01-01T01:00:00Z"}
        ]
    });
    seed_snapshot_temporal_entities(&pool, &snap_id, &[(eid.clone(), temporal)]).await;

    let (status, headers, body) = query_temporal_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");

    let entities = body.as_array().expect("response must be array");
    pretty_assertions::assert_eq!(entities.len(), 1);
    pretty_assertions::assert_eq!(entities[0]["id"], eid);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_get_temporal_entity_scoped_to_snapshot() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
        .bind(&snap_id)
        .execute(&pool)
        .await
        .unwrap();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-tget");
    let temporal = json!({
        "id": eid,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 70, "observedAt": "2024-06-01T12:00:00Z"}]
    });
    seed_snapshot_temporal_entities(&pool, &snap_id, &[(eid.clone(), temporal)]).await;

    let (status, headers, body) = get_temporal_entity_scoped(&c, &eid, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");
    pretty_assertions::assert_eq!(body["id"], eid);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_temporal_nonexistent_snapshot_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _, _) = query_temporal_entities_scoped(&c, Some("Vehicle"), "urn:ngsi-ld:Snapshot:nope").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_empty_snapshot_header_returns_400() {
    clean_database().await;
    let c = client();

    let resp = c
        .get(format!("{BASE_URL}/entities?type=Vehicle"))
        .header("NGSILD-Snapshot", "")
        .send()
        .await
        .expect("request failed");
    pretty_assertions::assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_preparing_snapshot_not_available_for_scoping() {
    clean_database().await;
    let c = client();

    // Newly created snapshots default to status "preparing".
    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, _, _) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "snapshot in preparing status must not be available for scoping");

    delete_snapshot(&c, &snap_id).await;
}

// ---------------------------------------------------------------------------
// Phase C scoping: batch query + type / attribute discovery
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn snapshot_scoping_entity_batch_query_returns_frozen_entities() {
    // Clause 5.5.15 + 5.7.11: POST /entityOperations/query honours NGSILD-Snapshot.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-batch");
    let entity = json!({"id": eid, "type": "Vehicle", "speed": {"type": "Property", "value": 42}});
    seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), entity)]).await;

    let body = json!({"type": "Query", "entities": [{"type": "Vehicle"}]});
    let (status, response) = post_batch_entity_query_scoped(&c, &body, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = response.as_array().expect("collection response");
    pretty_assertions::assert_eq!(entities.len(), 1);
    pretty_assertions::assert_eq!(entities[0]["id"].as_str(), Some(eid.as_str()));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_temporal_batch_query_returns_frozen_history() {
    // Clause 5.5.15 + 5.7.3.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-tempbatch");
    let temporal = json!({"id": eid, "type": "Vehicle", "speed": [{"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}]});
    seed_snapshot_temporal_entities(&pool, &snap_id, &[(eid.clone(), temporal)]).await;

    let body = json!({
        "type": "Query",
        "entities": [{"type": "Vehicle"}],
        "temporalQ": {"timerel": "after", "timeAt": "2023-01-01T00:00:00Z"}
    });
    let (status, response) = post_batch_temporal_query_scoped(&c, &body, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let entities = response.as_array().expect("collection response");
    pretty_assertions::assert_eq!(entities.len(), 1);
    pretty_assertions::assert_eq!(entities[0]["id"].as_str(), Some(eid.as_str()));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_types_query_returns_frozen_types() {
    // Clause 5.7.5 + 5.5.15.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid1 = unique_id("urn:ngsi-ld:Vehicle:snap-types-v");
    let eid2 = unique_id("urn:ngsi-ld:Sensor:snap-types-s");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[
            (eid1.clone(), json!({"id": eid1, "type": "Vehicle"})),
            (eid2.clone(), json!({"id": eid2, "type": "Sensor"})),
        ],
    )
    .await;

    let (status, body) = get_types_scoped(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("EntityTypeList"));
    let list = body["typeList"].as_array().expect("typeList array");
    let names: Vec<&str> = list.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"Vehicle"));
    assert!(names.contains(&"Sensor"));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_type_detail_returns_entity_count_and_attributes() {
    // Clause 5.7.6 + 5.5.15.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid1 = unique_id("urn:ngsi-ld:Vehicle:snap-type-detail");
    let eid2 = unique_id("urn:ngsi-ld:Vehicle:snap-type-detail");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[
            (eid1.clone(), json!({"id": eid1, "type": "Vehicle", "speed": {"type": "Property", "value": 1}})),
            (eid2.clone(), json!({"id": eid2, "type": "Vehicle", "fuel": {"type": "Property", "value": 50}})),
        ],
    )
    .await;

    let (status, body) = get_type_scoped(&c, "Vehicle", &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("EntityType"));
    pretty_assertions::assert_eq!(body["typeName"].as_str(), Some("Vehicle"));
    pretty_assertions::assert_eq!(body["entityCount"].as_i64(), Some(2));
    let attrs = body["attributeNames"].as_array().expect("attributeNames array");
    let names: Vec<&str> = attrs.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"speed"));
    assert!(names.contains(&"fuel"));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_type_detail_returns_404_when_absent() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();
    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-type-missing");
    seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), json!({"id": eid, "type": "Vehicle"}))]).await;

    let (status, _) = get_type_scoped(&c, "Spaceship", &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_attributes_query_returns_frozen_attrs() {
    // Clause 5.7.7 + 5.5.15.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-attrs");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[(
            eid.clone(),
            json!({"id": eid, "type": "Vehicle", "speed": {"type": "Property", "value": 10}, "fuel": {"type": "Property", "value": 50}}),
        )],
    )
    .await;

    let (status, body) = get_attributes_scoped(&c, &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("AttributeList"));
    let list = body["attributeList"].as_array().expect("attributeList array");
    let names: Vec<&str> = list.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"speed"));
    assert!(names.contains(&"fuel"));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_attribute_detail_returns_count_and_types() {
    // Clause 5.7.8 + 5.5.15.
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid1 = unique_id("urn:ngsi-ld:Vehicle:snap-attr-detail");
    let eid2 = unique_id("urn:ngsi-ld:Sensor:snap-attr-detail");
    seed_snapshot_entities(
        &pool,
        &snap_id,
        &[
            (eid1.clone(), json!({"id": eid1, "type": "Vehicle", "speed": {"type": "Property", "value": 1}})),
            (eid2.clone(), json!({"id": eid2, "type": "Sensor", "speed": {"type": "Property", "value": 2}})),
        ],
    )
    .await;

    let (status, body) = get_attribute_scoped(&c, "speed", &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some("Attribute"));
    pretty_assertions::assert_eq!(body["attributeName"].as_str(), Some("speed"));
    pretty_assertions::assert_eq!(body["attributeCount"].as_i64(), Some(2));
    let types = body["typeNames"].as_array().expect("typeNames array");
    let names: Vec<&str> = types.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"Vehicle"));
    assert!(names.contains(&"Sensor"));

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_attribute_detail_returns_404_when_absent() {
    clean_database().await;
    let c = client();
    let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();
    let eid = unique_id("urn:ngsi-ld:Vehicle:snap-attr-missing");
    seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), json!({"id": eid, "type": "Vehicle"}))]).await;

    let (status, _) = get_attribute_scoped(&c, "nonexistent", &snap_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_subscription_query_ignores_snapshot_header() {
    // Clause 5.5.15: scoping MUST NOT apply to management resources.
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let resp = c
        .get(format!("{BASE_URL}/subscriptions"))
        .header("NGSILD-Snapshot", &snap_id)
        .send()
        .await
        .expect("subscriptions GET failed");
    // A subscriptions list endpoint is unaffected by the snapshot header;
    // success here confirms the header was not consumed by this route.
    pretty_assertions::assert_eq!(resp.status(), StatusCode::OK);

    delete_snapshot(&c, &snap_id).await;
}
