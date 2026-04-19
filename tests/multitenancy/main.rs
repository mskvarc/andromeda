//! Multitenancy integration tests using the standard Rust harness.
//!
//! Verifies tenant isolation, implicit tenant creation, cross-tenant visibility
//! rules, and notification header propagation per the NGSI-LD multitenancy
//! specification (ETSI GS CIM 009 section 5.5.6).
//!
//! Naming convention: `multitenancy_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{db::TRUNCATE_ALL, prelude::*};
use axum::{Router, body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

const NONEXISTENT_TENANT_ERROR: &str = error::NONEXISTENT_TENANT;

fn client() -> Client {
    Client::new()
}

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

fn replace_database_name(url: &str, new_db: &str) -> Result<String, ()> {
    let mut parts = url.rsplitn(2, '/');
    let _current_db = parts.next().ok_or(())?;
    let prefix = parts.next().ok_or(())?;
    Ok(format!("{prefix}/{new_db}"))
}

async fn clean_named_tenant_databases(main_db_url: &str, main_pool: &PgPool) {
    let has_tenants_table: bool = sqlx::query_scalar("SELECT to_regclass('public.tenants') IS NOT NULL")
        .fetch_one(main_pool)
        .await
        .unwrap_or(false);
    if !has_tenants_table {
        return;
    }
    let tenant_dbs: Vec<String> = sqlx::query_scalar("SELECT db_name FROM tenants WHERE is_unnamed_default = false")
        .fetch_all(main_pool)
        .await
        .unwrap_or_default();
    for db_name in tenant_dbs {
        let Ok(tenant_url) = replace_database_name(main_db_url, &db_name) else {
            continue;
        };
        let Ok(tenant_pool) = PgPool::connect(&tenant_url).await else {
            continue;
        };
        let _ = sqlx::query(TRUNCATE_ALL).execute(&tenant_pool).await;
        tenant_pool.close().await;
    }
    let _ = sqlx::query("DELETE FROM tenants WHERE is_unnamed_default = false").execute(main_pool).await;
}

async fn reset_broker_state() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    clean_named_tenant_databases(DATABASE_URL, &pool).await;
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

fn unique_entity_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn unique_subscription_id(case_id: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{case_id}:{}", Uuid::new_v4())
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

/// Sends an NGSI-LD request. An empty `tenant` string omits the `NGSILD-Tenant` header.
async fn ngsi_request(
    http_client: &Client,
    method: Method,
    path: &str,
    body: Option<&Value>,
    tenant: &str,
    accept_ld_json: bool,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = http_client.request(method, format!("{BASE_URL}{path}"));

    if accept_ld_json {
        request = request.header("Accept", "application/ld+json").header("Link", link_header());
    }

    if !tenant.is_empty() {
        request = request.header("NGSILD-Tenant", tenant);
    }

    if let Some(body) = body {
        request = request.header("Content-Type", "application/ld+json").json(body);
    }

    let response = request.send().await.expect("ngsi request failed");
    let status = response.status();
    let headers = response.headers().clone();
    let response_body = read_json_or_null(response).await;
    (status, headers, response_body)
}

/// GET /tenants and decode JSON body.
async fn list_tenants(http_client: &Client) -> (StatusCode, Value) {
    let response = http_client
        .get(format!("{BASE_URL}/tenants"))
        .send()
        .await
        .expect("list_tenants request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

fn vehicle_entity(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "name": {"type": "Property", "value": name},
        "@context": [CORE_CONTEXT]
    })
}

fn temporal_vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "values": [{"value": 55, "observedAt": "2020-08-01T12:00:00Z"}]
        },
        "@context": [CORE_CONTEXT]
    })
}

fn entity_subscription(id: &str, endpoint_url: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {"uri": endpoint_url, "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

fn csr_registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn csr_subscription(id: &str, endpoint_url: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {"uri": endpoint_url, "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

// --- NotificationReceiver ---

pub struct CapturedNotification {
    pub headers: AxumHeaderMap,
    pub body: Value,
}

pub struct NotificationReceiver {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    pub port: u16,
    endpoint_path: String,
    notifications: Arc<Mutex<Vec<CapturedNotification>>>,
    notify: Arc<Notify>,
}

impl NotificationReceiver {
    pub async fn start() -> NotificationReceiver {
        let notifications = Arc::new(Mutex::new(Vec::<CapturedNotification>::new()));
        let notify = Arc::new(Notify::new());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let notification_store = notifications.clone();
        let signal = notify.clone();

        let endpoint_path = format!("/notify-{}", Uuid::new_v4());
        let app = Router::new().route(
            &endpoint_path,
            post(move |headers: AxumHeaderMap, body: Bytes| {
                let notification_store = notification_store.clone();
                let signal = signal.clone();
                async move {
                    let payload = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
                    notification_store.lock().await.push(CapturedNotification { headers, body: payload });
                    signal.notify_one();
                    axum::http::StatusCode::OK.into_response()
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.expect("bind notification receiver");
        let port = listener.local_addr().expect("notification receiver local address").port();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("notification receiver task failure");
        });

        NotificationReceiver {
            shutdown: Some(shutdown_tx),
            port,
            endpoint_path,
            notifications,
            notify,
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!("http://host.docker.internal:{}{}", self.port, self.endpoint_path)
    }

    pub async fn wait_for_subscription_notification(&self, subscription_id: &str, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut notifications = self.notifications.lock().await;
                if let Some(index) = notifications.iter().position(|n| n.body["subscriptionId"].as_str() == Some(subscription_id)) {
                    return notifications.remove(index);
                }
            }
            let notified = tokio::time::timeout_at(deadline, self.notify.notified()).await;
            assert!(notified.is_ok(), "timed out waiting for subscription notification after {timeout:?}");
        }
    }

    pub async fn wait_for_no_notification_for_subscription(&self, subscription_id: &str, duration: Duration) {
        tokio::time::sleep(duration).await;
        let notifications = self.notifications.lock().await;
        let count = notifications
            .iter()
            .filter(|n| n.body["subscriptionId"].as_str() == Some(subscription_id))
            .count();
        pretty_assertions::assert_eq!(count, 0, "expected no notifications for subscription {subscription_id}, got {count}");
    }

    pub fn stop(&mut self) {
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

// --- Tests ---

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_unnamed_and_named_default_tenants_are_isolated() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:tenant-isolation");
    let unnamed_entity = vehicle_entity(&id, "unnamed-default");
    let named_entity = vehicle_entity(&id, "named-default");

    // Create in unnamed default tenant (no NGSILD-Tenant header)
    let (status, create_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&unnamed_entity), "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(
        create_headers.get("NGSILD-Tenant").is_none(),
        "unnamed default tenant response must omit NGSILD-Tenant header"
    );

    // Create same ID in named "default" tenant
    let (status, create_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&named_entity), "default", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        create_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some("default"),
        "named default tenant response must include NGSILD-Tenant header"
    );

    // Retrieve from unnamed default → should see unnamed version
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "unnamed-default");

    // Retrieve from named "default" tenant → should see named version
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, "default", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "named-default");
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_non_create_on_unknown_tenant_returns_nonexistent_tenant_error() {
    let http_client = client();
    reset_broker_state().await;

    let tenant = format!("unknown-tenant-{}", Uuid::new_v4());
    let id = unique_entity_id("urn:ngsi-ld:Vehicle:noncreate");

    let (status, resp_headers, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, &tenant, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(
        body["type"],
        NONEXISTENT_TENANT_ERROR,
        "accessing unknown tenant via non-create operation must return NonexistentTenant"
    );
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "response must echo NGSILD-Tenant header"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_create_operations_implicitly_create_named_tenant() {
    let http_client = client();
    reset_broker_state().await;

    // 1. Create Entity
    let tenant_1 = format!("tenant-create-entity-{}", Uuid::new_v4());
    let id_1 = unique_entity_id("urn:ngsi-ld:Vehicle:create-entity");
    let (status, headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&vehicle_entity(&id_1, "v1")), &tenant_1, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_1.as_str()));
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_1}"), None, &tenant_1, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    // Entity not visible in unnamed default tenant
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_1}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 2. Batch Entity Creation
    let tenant_2 = format!("tenant-batch-create-{}", Uuid::new_v4());
    let id_2 = unique_entity_id("urn:ngsi-ld:Vehicle:batch-create");
    let batch = json!([vehicle_entity(&id_2, "v2")]);
    let (status, headers, _) = ngsi_request(&http_client, Method::POST, "/entityOperations/create", Some(&batch), &tenant_2, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_2.as_str()));
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_2}"), None, &tenant_2, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_2}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 3. Create Temporal Entity
    let tenant_3 = format!("tenant-temporal-create-{}", Uuid::new_v4());
    let id_3 = unique_entity_id("urn:ngsi-ld:Vehicle:temporal-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/temporal/entities",
        Some(&temporal_vehicle_entity(&id_3)),
        &tenant_3,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_3.as_str()));

    // 4. Create Subscription
    let tenant_4 = format!("tenant-sub-create-{}", Uuid::new_v4());
    let id_4 = unique_subscription_id("sub-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/subscriptions",
        Some(&entity_subscription(&id_4, "http://127.0.0.1:65535/notify")),
        &tenant_4,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_4.as_str()));
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/subscriptions/{id_4}"), None, &tenant_4, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/subscriptions/{id_4}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 5. Register Context Source
    let tenant_5 = format!("tenant-csr-create-{}", Uuid::new_v4());
    let id_5 = unique_entity_id("urn:ngsi-ld:ContextSourceRegistration:create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/csourceRegistrations",
        Some(&csr_registration(&id_5)),
        &tenant_5,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_5.as_str()));
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceRegistrations/{id_5}"), None, &tenant_5, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceRegistrations/{id_5}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 6. Create Context Source Subscription
    let tenant_6 = format!("tenant-csr-sub-create-{}", Uuid::new_v4());
    let id_6 = unique_subscription_id("csr-sub-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/csourceSubscriptions",
        Some(&csr_subscription(&id_6, "http://127.0.0.1:65535/notify")),
        &tenant_6,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_6.as_str()));
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceSubscriptions/{id_6}"), None, &tenant_6, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceSubscriptions/{id_6}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_named_tenant_error_response_echoes_ngsild_tenant_header() {
    let http_client = client();
    reset_broker_state().await;

    let tenant = format!("tenant-error-echo-{}", Uuid::new_v4());
    // Missing required `id` field → BadRequestData, but tenant header must still be echoed
    let invalid = json!({"type": "Vehicle", "@context": [CORE_CONTEXT]});
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&invalid), &tenant, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "error responses in named tenant must echo NGSILD-Tenant header"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_cross_tenant_entity_access_returns_not_found() {
    let http_client = client();
    reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:cross-visibility");
    let tenant_a = format!("tenant-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-b-{}", Uuid::new_v4());

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&id, "tenant-a")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Accessing the entity from tenant B returns 404 NonexistentTenant (tenant B does not exist yet)
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, &tenant_b, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(
        body["type"],
        NONEXISTENT_TENANT_ERROR,
        "cross-tenant access to entity in non-existent tenant B must return NonexistentTenant"
    );
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_unnamed_default_tenant_notifications_omit_tenant_header() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("unnamed-notif");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(
        resp_headers.get("NGSILD-Tenant").is_none(),
        "create subscription in unnamed default tenant must omit NGSILD-Tenant header"
    );

    // Small delay to let subscription activate before creating entity
    tokio::time::sleep(Duration::from_millis(150)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:unnamed-notif");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "unnamed-trigger")),
        "",
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;
    assert!(
        notification.headers.get("NGSILD-Tenant").is_none(),
        "notifications for unnamed default tenant must omit NGSILD-Tenant header"
    );

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_named_tenant_notifications_include_ngsild_tenant_header() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    let tenant = format!("tenant-notif-{}", Uuid::new_v4());

    let subscription_id = unique_subscription_id("named-notif");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), &tenant, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "create subscription in named tenant must include NGSILD-Tenant header"
    );

    tokio::time::sleep(Duration::from_millis(150)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:named-notif");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "named-trigger")),
        &tenant,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;
    pretty_assertions::assert_eq!(
        notification.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "notifications for named tenant must include NGSILD-Tenant header"
    );

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_subscription_does_not_receive_events_from_different_tenant() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    let tenant_a = format!("tenant-sub-isolation-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-sub-isolation-b-{}", Uuid::new_v4());

    // Subscription in tenant A
    let subscription_id = unique_subscription_id("isolation");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, _, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), &tenant_a, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Create entity in tenant B — must NOT trigger subscription in tenant A
    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:sub-isolation");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "tenant-b-entity")),
        &tenant_b,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver
        .wait_for_no_notification_for_subscription(&subscription_id, Duration::from_secs(2))
        .await;

    // Create entity in tenant A — must trigger subscription in tenant A
    let entity_id_a = unique_entity_id("urn:ngsi-ld:Vehicle:sub-isolation-a");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id_a, "tenant-a-entity")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_tenants_endpoint_returns_tenant_list_object_shape() {
    let http_client = client();
    reset_broker_state().await;

    let tenant_a = format!("tenant-list-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-list-b-{}", Uuid::new_v4());

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&unique_entity_id("urn:ngsi-ld:Vehicle:tenant-list-a"), "tenant-a-entity")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&unique_entity_id("urn:ngsi-ld:Vehicle:tenant-list-b"), "tenant-b-entity")),
        &tenant_b,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_tenants(&http_client).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "TenantList");

    let id = body["id"].as_str().unwrap_or_default();
    assert!(id.starts_with("urn:ngsi-ld:TenantList:"), "expected TenantList URN id, got: {id}");

    let tenant_list = body["tenantList"].as_array().cloned().unwrap_or_default();
    let tenant_values: Vec<&str> = tenant_list.iter().filter_map(|value| value.as_str()).collect();
    assert!(
        tenant_values.contains(&tenant_a.as_str()),
        "tenantList must contain {tenant_a}, got: {tenant_values:?}"
    );
    assert!(
        tenant_values.contains(&tenant_b.as_str()),
        "tenantList must contain {tenant_b}, got: {tenant_values:?}"
    );
}
