//! Snapshot integration tests for the NGSI-LD context broker.
//!
//! Covers the full Snapshot lifecycle (spec 5.16, 6.36-6.38), status
//! notifications (spec 5.16.6, 5.3.4), and the NGSILD-Snapshot header
//! scoping of entity and temporal queries (spec 5.5.15, 6.3.22).
//!
//! Naming convention: `snapshot_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use axum::{body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

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
                        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes()) {
                            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                                reqwest_headers.insert(name, val);
                            }
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
        format!("http://host.docker.internal:{}/notify", self.port)
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
                panic!("timed out waiting for notification after {timeout:?}");
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

    assert_eq!(status, StatusCode::CREATED);
    let location = headers.get("location").and_then(|v| v.to_str().ok());
    assert!(
        location.map(|l| l.contains("/snapshots/")).unwrap_or(false),
        "Location header must contain /snapshots/"
    );
    assert_eq!(resp_body["status"], "preparing");
    assert_eq!(resp_body["snapshotPriority"], 5);

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

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(resp_body["id"], custom_id);

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
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = create_snapshot(&c, &body).await;
    assert_eq!(status, StatusCode::CONFLICT);

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
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], snap_id);
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
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_update_lifetime_and_priority_returns_200() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 3})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, updated) = update_snapshot(&c, &snap_id, &json!({"snapshotLifetime": "PT2H", "snapshotPriority": 8})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["snapshotPriority"], 8);

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
    assert_eq!(status, StatusCode::BAD_REQUEST);

    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_delete_returns_204_and_removes_snapshot() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    assert_eq!(delete_snapshot(&c, &snap_id).await, StatusCode::NO_CONTENT);
    assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_delete_nonexistent_returns_404() {
    clean_database().await;
    let c = client();

    assert_eq!(delete_snapshot(&c, "urn:ngsi-ld:Snapshot:ghost").await, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn snapshot_api_clone_returns_201_with_new_id_and_inherited_priority() {
    clean_database().await;
    let c = client();

    let (_, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 7})).await;
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, cloned) = clone_snapshot(&c, &snap_id).await;
    assert_eq!(status, StatusCode::CREATED);

    let cloned_id = cloned["id"].as_str().unwrap().to_string();
    assert_ne!(cloned_id, snap_id, "cloned snapshot must have a different id");
    assert_eq!(cloned["snapshotPriority"], 7, "clone must inherit priority");

    assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::OK);
    assert_eq!(get_snapshot(&c, &cloned_id).await.0, StatusCode::OK);

    delete_snapshot(&c, &snap_id).await;
    delete_snapshot(&c, &cloned_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_purge_returns_204() {
    clean_database().await;
    let c = client();

    for _ in 0..2 {
        let (status, _, _) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    assert_eq!(purge_snapshots(&c).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn snapshot_api_default_priority_is_5() {
    clean_database().await;
    let c = client();

    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["snapshotPriority"], 5);

    delete_snapshot(&c, created["id"].as_str().unwrap()).await;
}

#[tokio::test]
#[serial]
async fn snapshot_api_full_lifecycle_create_get_update_clone_delete() {
    clean_database().await;
    let c = client();

    // 1. Create
    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H", "snapshotPriority": 3})).await;
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    // 2. Get
    let (status, got) = get_snapshot(&c, &snap_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["snapshotPriority"], 3);

    // 3. Update
    let (status, updated) = update_snapshot(&c, &snap_id, &json!({"snapshotPriority": 9})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["snapshotPriority"], 9);

    // 4. Clone
    let (status, cloned) = clone_snapshot(&c, &snap_id).await;
    assert_eq!(status, StatusCode::CREATED);
    let cloned_id = cloned["id"].as_str().unwrap().to_string();
    assert_eq!(cloned["snapshotPriority"], 9, "clone must inherit updated priority");

    // 5. Delete both
    assert_eq!(delete_snapshot(&c, &snap_id).await, StatusCode::NO_CONTENT);
    assert_eq!(delete_snapshot(&c, &cloned_id).await, StatusCode::NO_CONTENT);

    // 6. Verify gone
    assert_eq!(get_snapshot(&c, &snap_id).await.0, StatusCode::NOT_FOUND);
    assert_eq!(get_snapshot(&c, &cloned_id).await.0, StatusCode::NOT_FOUND);
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
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notif.body["type"], "SnapshotNotification");
    assert_eq!(notif.body["snapshotId"], snap_id);
    assert_eq!(notif.body["snapshotStatus"], "preparing");
    assert_eq!(notif.body["snapshotPriority"], 7);
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
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, _) = update_snapshot(&c, &snap_id, &json!({"snapshotPriority": 8})).await;
    assert_eq!(status, StatusCode::OK);

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
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    // Drain the create notification.
    let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let (status, _) = update_snapshot(&c, &snap_id, &json!({"snapshotLifetime": "PT2H", "snapshotPriority": 8})).await;
    assert_eq!(status, StatusCode::OK);

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notif.body["type"], "SnapshotNotification");
    assert_eq!(notif.body["snapshotId"], snap_id);
    assert_eq!(notif.body["snapshotPriority"], 8, "notification must reflect updated priority");
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
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notif.body["type"], "SnapshotNotification");

    let custom = notif.headers.get("x-custom-header").expect("X-Custom-Header must be forwarded to endpoint");
    assert_eq!(custom.to_str().unwrap(), "test-value-123");

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
    assert_eq!(status, StatusCode::CREATED);
    let source_id = created["id"].as_str().unwrap().to_string();

    // Drain the create notification.
    let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let (status, cloned) = clone_snapshot(&c, &source_id).await;
    assert_eq!(status, StatusCode::CREATED);
    let cloned_id = cloned["id"].as_str().unwrap().to_string();

    let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notif.body["type"], "SnapshotNotification");
    assert_eq!(notif.body["snapshotId"], cloned_id, "notification must reference the cloned snapshot");
    assert_ne!(
        notif.body["snapshotId"].as_str().unwrap(),
        source_id.as_str(),
        "notification must not reference the source snapshot"
    );
    assert_eq!(notif.body["snapshotPriority"], 6, "clone must inherit source priority");

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
    assert_eq!(status, StatusCode::CREATED);
    let source_id = created["id"].as_str().unwrap().to_string();

    let (status, cloned) = clone_snapshot(&c, &source_id).await;
    assert_eq!(status, StatusCode::CREATED);
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
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let eid1 = unique_id("urn:ngsi-ld:Vehicle:snap-q");
    let eid2 = unique_id("urn:ngsi-ld:Vehicle:snap-q");
    let e1 = json!({"id": eid1, "type": "Vehicle", "speed": {"type": "Property", "value": 80}});
    let e2 = json!({"id": eid2, "type": "Vehicle", "speed": {"type": "Property", "value": 120}});
    seed_snapshot_entities(&pool, &snap_id, &[(eid1.clone(), e1), (eid2.clone(), e2)]).await;

    let (status, headers, body) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");

    let entities = body.as_array().expect("response must be array");
    assert_eq!(entities.len(), 2, "must return exactly 2 snapshot entities");
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
    assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");
    assert_eq!(body["id"], eid);
    assert_eq!(body["type"], "Vehicle");

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
    assert_eq!(status, StatusCode::NOT_FOUND, "live entity must not be visible in snapshot scope");

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_nonexistent_snapshot_id_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _, _) = query_entities_scoped(&c, Some("Vehicle"), "urn:ngsi-ld:Snapshot:doesnotexist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
    assert_eq!(status, StatusCode::OK);

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
    assert_eq!(status, StatusCode::OK);

    let entities = body.as_array().expect("response must be array");
    assert_eq!(entities.len(), 1, "type filter must return only Vehicle entities");
    assert_eq!(entities[0]["id"], vid);

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
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "speed must be present");
    assert!(body.get("fuelLevel").is_none(), "fuelLevel must be filtered out by attrs");
    assert_eq!(body["id"], eid);
    assert_eq!(body["type"], "Vehicle");

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
    assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");

    let entities = body.as_array().expect("response must be array");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0]["id"], eid);

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
    assert_eq!(status, StatusCode::OK);

    let echo = headers.get("ngsild-snapshot").and_then(|v| v.to_str().ok());
    assert_eq!(echo, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");
    assert_eq!(body["id"], eid);

    pool.close().await;
    delete_snapshot(&c, &snap_id).await;
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_temporal_nonexistent_snapshot_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _, _) = query_temporal_entities_scoped(&c, Some("Vehicle"), "urn:ngsi-ld:Snapshot:nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn snapshot_scoping_preparing_snapshot_not_available_for_scoping() {
    clean_database().await;
    let c = client();

    // Newly created snapshots default to status "preparing".
    let (status, _, created) = create_snapshot(&c, &json!({"snapshotLifetime": "PT1H"})).await;
    assert_eq!(status, StatusCode::CREATED);
    let snap_id = created["id"].as_str().unwrap().to_string();

    let (status, _, _) = query_entities_scoped(&c, Some("Vehicle"), &snap_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "snapshot in preparing status must not be available for scoping");

    delete_snapshot(&c, &snap_id).await;
}
