//! Subscription notification joinLevel integration tests using the standard Rust harness.
//!
//! Verifies that subscription notification.joinLevel (NGSI-LD spec 5.2.14.1) controls how
//! many relationship hops are traversed and included in flat-joined notifications.
//!
//! Naming convention: `notification_join_level_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use axum::{Router, body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
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

fn client() -> Client {
    Client::new()
}

async fn reset_broker_state() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

fn unique_entity_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn unique_subscription_id(case_id: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{case_id}:{}", Uuid::new_v4())
}

async fn create_entity(http_client: &Client, body: &Value) -> StatusCode {
    http_client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create entity request failed")
        .status()
}

async fn append_entity_attrs(http_client: &Client, entity_id: &str, fragment: &Value) -> StatusCode {
    let encoded_id = urlencoding::encode(entity_id);
    http_client
        .post(format!("{BASE_URL}/entities/{encoded_id}/attrs"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("append entity attrs request failed")
        .status()
}

async fn create_subscription(http_client: &Client, body: &Value) -> StatusCode {
    let response = http_client
        .post(format!("{BASE_URL}/subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create subscription request failed");
    let status = response.status();
    if status == StatusCode::CREATED {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    status
}

pub struct CapturedNotification {
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
            post(move |_headers: AxumHeaderMap, body: Bytes| {
                let notification_store = notification_store.clone();
                let signal = signal.clone();
                async move {
                    let payload = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
                    notification_store.lock().await.push(CapturedNotification { body: payload });
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
                if let Some(index) = notifications
                    .iter()
                    .position(|n| n.body["subscriptionId"].as_str() == Some(subscription_id))
                {
                    return notifications.remove(index);
                }
            }
            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("timed out waiting for subscription notification after {timeout:?}");
            }
        }
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

fn subscription_with_flat_join(subscription_id: &str, endpoint_url: &str, join_level: Option<u32>) -> Value {
    let mut notification = json!({
        "endpoint": {"uri": endpoint_url, "accept": "application/json"},
        "join": "flat"
    });
    if let Some(level) = join_level {
        notification["joinLevel"] = json!(level);
    }
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": notification,
        "@context": [CORE_CONTEXT]
    })
}

fn trigger_fragment() -> Value {
    json!({
        "name": {"type": "Property", "value": "Updated"},
        "@context": [CORE_CONTEXT]
    })
}

// --- Tests ---

#[serial_test::serial]
#[tokio::test]
async fn notification_join_level_3_delivers_three_depth_levels_of_linked_entities() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    // Chain: Building → City → Country (3 levels deep from Building's perspective)
    let country_id = "urn:ngsi-ld:Country:notif-jl3-France";
    let country = json!({
        "id": country_id,
        "type": "Country",
        "name": {"type": "Property", "value": "France"},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &country).await;
    assert_eq!(status, StatusCode::CREATED);

    let city_id = "urn:ngsi-ld:City:notif-jl3-Paris";
    let city = json!({
        "id": city_id,
        "type": "City",
        "name": {"type": "Property", "value": "Paris"},
        "locatedIn": {"type": "Relationship", "object": country_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &city).await;
    assert_eq!(status, StatusCode::CREATED);

    let building_id = unique_entity_id("urn:ngsi-ld:Building:notif-jl3");
    let building = json!({
        "id": building_id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &building).await;
    assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("jl3");
    let sub = subscription_with_flat_join(&subscription_id, &receiver.endpoint_url(), Some(3));
    let status = create_subscription(&http_client, &sub).await;
    assert_eq!(status, StatusCode::CREATED);

    let status = append_entity_attrs(&http_client, &building_id, &trigger_fragment()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("data must be an array");

    assert!(
        data.len() >= 3,
        "joinLevel=3 must include building, city, and country; got {} entries: {:?}",
        data.len(),
        data.iter().map(|d| d["id"].as_str().unwrap_or("?")).collect::<Vec<_>>()
    );

    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()), "notification data must contain building");
    assert!(ids.contains(&city_id), "notification data must contain city at depth 1");
    assert!(ids.contains(&country_id), "notification data must contain country at depth 2");

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_level_default_is_1_when_omitted_from_subscription() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    // Chain: Building → City → Country
    let country_id = "urn:ngsi-ld:Country:notif-jldef-Germany";
    let country = json!({
        "id": country_id,
        "type": "Country",
        "name": {"type": "Property", "value": "Germany"},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &country).await;
    assert_eq!(status, StatusCode::CREATED);

    let city_id = "urn:ngsi-ld:City:notif-jldef-Berlin";
    let city = json!({
        "id": city_id,
        "type": "City",
        "locatedIn": {"type": "Relationship", "object": country_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &city).await;
    assert_eq!(status, StatusCode::CREATED);

    let building_id = unique_entity_id("urn:ngsi-ld:Building:notif-jldef");
    let building = json!({
        "id": building_id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &building).await;
    assert_eq!(status, StatusCode::CREATED);

    // Subscription without explicit joinLevel → defaults to 1
    let subscription_id = unique_subscription_id("jldef");
    let sub = subscription_with_flat_join(&subscription_id, &receiver.endpoint_url(), None);
    let status = create_subscription(&http_client, &sub).await;
    assert_eq!(status, StatusCode::CREATED);

    let status = append_entity_attrs(&http_client, &building_id, &trigger_fragment()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("data must be an array");

    assert_eq!(
        data.len(),
        2,
        "default joinLevel=1 must return building plus one level of linked entities"
    );
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()), "notification data must contain building");
    assert!(ids.contains(&city_id), "notification data must contain city at depth 1");
    assert!(!ids.contains(&country_id), "country at depth 2 must not appear when joinLevel defaults to 1");

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_level_1_delivers_one_depth_level_of_linked_entities() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    // Chain: Building → City → Country
    let country_id = "urn:ngsi-ld:Country:notif-jl1-Spain";
    let country = json!({
        "id": country_id,
        "type": "Country",
        "name": {"type": "Property", "value": "Spain"},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &country).await;
    assert_eq!(status, StatusCode::CREATED);

    let city_id = "urn:ngsi-ld:City:notif-jl1-Madrid";
    let city = json!({
        "id": city_id,
        "type": "City",
        "locatedIn": {"type": "Relationship", "object": country_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &city).await;
    assert_eq!(status, StatusCode::CREATED);

    let building_id = unique_entity_id("urn:ngsi-ld:Building:notif-jl1");
    let building = json!({
        "id": building_id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &building).await;
    assert_eq!(status, StatusCode::CREATED);

    // Explicit joinLevel=1
    let subscription_id = unique_subscription_id("jl1");
    let sub = subscription_with_flat_join(&subscription_id, &receiver.endpoint_url(), Some(1));
    let status = create_subscription(&http_client, &sub).await;
    assert_eq!(status, StatusCode::CREATED);

    let status = append_entity_attrs(&http_client, &building_id, &trigger_fragment()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("data must be an array");

    assert_eq!(data.len(), 2, "joinLevel=1 must return building plus the directly linked city");
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()));
    assert!(ids.contains(&city_id));
    assert!(!ids.contains(&country_id), "joinLevel=1 must not traverse beyond one hop");

    receiver.stop();
}

#[serial_test::serial]
#[tokio::test]
async fn notification_join_level_2_delivers_two_depth_levels_of_linked_entities() {
    let http_client = client();
    reset_broker_state().await;
    let mut receiver = NotificationReceiver::start().await;

    // Chain: Building → City → Country → Continent
    let continent_id = "urn:ngsi-ld:Continent:notif-jl2-Europe";
    let continent = json!({
        "id": continent_id,
        "type": "Continent",
        "name": {"type": "Property", "value": "Europe"},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &continent).await;
    assert_eq!(status, StatusCode::CREATED);

    let country_id = "urn:ngsi-ld:Country:notif-jl2-Italy";
    let country = json!({
        "id": country_id,
        "type": "Country",
        "name": {"type": "Property", "value": "Italy"},
        "partOf": {"type": "Relationship", "object": continent_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &country).await;
    assert_eq!(status, StatusCode::CREATED);

    let city_id = "urn:ngsi-ld:City:notif-jl2-Rome";
    let city = json!({
        "id": city_id,
        "type": "City",
        "locatedIn": {"type": "Relationship", "object": country_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &city).await;
    assert_eq!(status, StatusCode::CREATED);

    let building_id = unique_entity_id("urn:ngsi-ld:Building:notif-jl2");
    let building = json!({
        "id": building_id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let status = create_entity(&http_client, &building).await;
    assert_eq!(status, StatusCode::CREATED);

    // Explicit joinLevel=2
    let subscription_id = unique_subscription_id("jl2");
    let sub = subscription_with_flat_join(&subscription_id, &receiver.endpoint_url(), Some(2));
    let status = create_subscription(&http_client, &sub).await;
    assert_eq!(status, StatusCode::CREATED);

    let status = append_entity_attrs(&http_client, &building_id, &trigger_fragment()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    let data = notification.body["data"].as_array().expect("data must be an array");

    assert_eq!(
        data.len(),
        3,
        "joinLevel=2 must return building, city, and country (three entities total)"
    );
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()), "building must be in data");
    assert!(ids.contains(&city_id), "city at depth 1 must be in data");
    assert!(ids.contains(&country_id), "country at depth 2 must be in data");
    assert!(!ids.contains(&continent_id), "continent at depth 3 must not appear when joinLevel=2");

    receiver.stop();
}
