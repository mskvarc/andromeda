use axum::{Router, body::Bytes, http::StatusCode as AxumStatusCode, routing::post};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, SecondsFormat, Utc};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
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

fn client() -> Client {
    Client::new()
}

fn unique_subscription_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:Subscription:csr-notif:{scenario}:{}", Uuid::new_v4())
}

fn unique_registration_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:ContextSourceRegistration:csr-notif:{scenario}:{}", Uuid::new_v4())
}

async fn clean_database() {
    let pool = match PgPool::connect(DATABASE_URL).await {
        Ok(pool) => pool,
        Err(err) => panic!("failed to connect to database for cleanup: {err}"),
    };
    if let Err(err) = sqlx::query(TRUNCATE_ALL).execute(&pool).await {
        panic!("failed to truncate tables: {err}");
    }
    pool.close().await;
}

async fn read_body(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

async fn create_csource_subscription(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = match http_client
        .post(format!("{BASE_URL}/csourceSubscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("create csource subscription request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn retrieve_csource_subscription(http_client: &Client, subscription_id: &str, options: Option<&str>) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let query = match options {
        Some(options) => format!("?options={options}"),
        None => String::new(),
    };

    let response = match http_client
        .get(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}{query}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("retrieve csource subscription request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn update_csource_subscription(http_client: &Client, subscription_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = match http_client
        .patch(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}"))
        .header("Content-Type", "application/json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .json(fragment)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("update csource subscription request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn create_registration(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = match http_client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("create registration request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn update_registration(http_client: &Client, registration_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(registration_id);
    let response = match http_client
        .patch(format!("{BASE_URL}/csourceRegistrations/{encoded_id}"))
        .header("Content-Type", "application/json")
        .json(fragment)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("update registration request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

fn base_subscription_payload(subscription_id: &str, endpoint_uri: &str) -> Value {
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [
            {
                "type": "Building"
            }
        ],
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": endpoint_uri,
                "accept": "application/json"
            }
        },
        "@context": [CORE_CONTEXT]
    })
}

fn time_interval_subscription_payload(subscription_id: &str, endpoint_uri: &str) -> Value {
    let mut payload = base_subscription_payload(subscription_id, endpoint_uri);
    payload["timeInterval"] = Value::from(10);
    payload
}

fn temporal_query_subscription_payload(subscription_id: &str, endpoint_uri: &str, time_property: &str) -> Value {
    let mut payload = base_subscription_payload(subscription_id, endpoint_uri);
    payload["temporalQ"] = json!({
        "timerel": "after",
        "timeAt": "2020-06-01T22:07:00Z",
        "timeproperty": time_property
    });
    payload
}

fn watched_attributes_subscription_payload(subscription_id: &str, endpoint_uri: &str) -> Value {
    let mut payload = base_subscription_payload(subscription_id, endpoint_uri);
    payload["watchedAttributes"] = json!(["name", "subCategory"]);
    payload
}

fn geoq_subscription_payload(subscription_id: &str, endpoint_uri: &str, include_geoproperty: bool) -> Value {
    let mut payload = base_subscription_payload(subscription_id, endpoint_uri);
    payload["geoQ"] = if include_geoproperty {
        json!({
            "georel": "near;maxDistance==2000",
            "geometry": "Point",
            "coordinates": [-8.52, 41.2],
            "geoproperty": "location"
        })
    } else {
        json!({
            "georel": "near;maxDistance==2000",
            "geometry": "Point",
            "coordinates": [-8.52, 41.2]
        })
    };
    payload
}

fn unreachable_endpoint_subscription_payload(subscription_id: &str) -> Value {
    base_subscription_payload(subscription_id, "http://unreachable.endpoint/notify")
}

fn building_and_bus_registration_payload(registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": "Building"
                    }
                ]
            },
            {
                "entities": [
                    {
                        "type": "Bus"
                    }
                ]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn detailed_building_registration_payload(registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": "Building"
                    }
                ],
                "propertyNames": ["name", "subCategory"],
                "relationshipNames": ["locatedAt"]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn location_registration_payload(registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": "Building"
                    }
                ]
            }
        ],
        "location": {
            "type": "Point",
            "coordinates": [-8.521, 41.2]
        },
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn observation_interval_registration_payload(registration_id: &str) -> Value {
    let mut payload = base_registration_for_type(registration_id, "Building");
    payload["observationInterval"] = json!({
        "startAt": "2020-08-01T22:07:00Z",
        "endAt": "2021-08-01T21:07:00Z"
    });
    payload
}

fn management_interval_registration_payload(registration_id: &str) -> Value {
    let mut payload = base_registration_for_type(registration_id, "Building");
    payload["managementInterval"] = json!({
        "startAt": "2020-08-01T22:07:00Z",
        "endAt": "2021-08-01T21:07:00Z"
    });
    payload
}

fn base_registration_for_type(registration_id: &str, entity_type: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": entity_type
                    }
                ]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn registration_endpoint_update_fragment() -> Value {
    json!({
        "endpoint": "http://my.new.csource.org:1026"
    })
}

fn registration_information_new_type_fragment() -> Value {
    json!({
        "information": [
            {
                "entities": [
                    {
                        "type": "NewType"
                    }
                ]
            }
        ]
    })
}

fn subscription_update_is_active_false_fragment() -> Value {
    json!({
        "isActive": false
    })
}

fn subscription_update_expires_at_past_fragment() -> Value {
    json!({
        "expiresAt": "2020-08-01T22:07:00Z"
    })
}

fn subscription_update_vehicle_entities_fragment() -> Value {
    json!({
        "entities": [
            {
                "type": "Vehicle"
            }
        ]
    })
}

fn subscription_update_bus_entities_fragment() -> Value {
    json!({
        "entities": [
            {
                "type": "Bus"
            }
        ]
    })
}

fn subscription_update_vehicle_and_bus_entities_fragment() -> Value {
    json!({
        "entities": [
            {
                "type": "Vehicle"
            },
            {
                "type": "Bus"
            }
        ]
    })
}

fn assert_iso_timestamp(value: &Value, field_name: &str) {
    let value_str = match value.as_str() {
        Some(value_str) => value_str,
        None => panic!("{field_name} should be a string: {value}"),
    };
    if DateTime::<FixedOffset>::parse_from_rfc3339(value_str).is_err() {
        panic!("{field_name} should be a valid RFC3339 timestamp: {value_str}");
    }
}

fn is_building_entity_type(entity_type: &str) -> bool {
    entity_type == "Building" || entity_type.ends_with("#Building") || entity_type.ends_with("/Building") || entity_type.ends_with(":Building")
}

#[derive(Clone)]
struct CapturedNotification {
    body: Value,
}

struct NotificationReceiver {
    endpoint_url: String,
    notifications: Arc<Mutex<Vec<CapturedNotification>>>,
    notify: Arc<Notify>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl NotificationReceiver {
    async fn start() -> NotificationReceiver {
        let notifications = Arc::new(Mutex::new(Vec::<CapturedNotification>::new()));
        let notify = Arc::new(Notify::new());
        let shared_notifications = Arc::clone(&notifications);
        let shared_notify = Arc::clone(&notify);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let app = Router::new().route(
            "/notify",
            post(move |body: Bytes| {
                let shared_notifications = Arc::clone(&shared_notifications);
                let shared_notify = Arc::clone(&shared_notify);
                async move {
                    let parsed_body = match serde_json::from_slice::<Value>(&body) {
                        Ok(parsed) => parsed,
                        Err(_) => Value::Null,
                    };
                    let mut guard = shared_notifications.lock().await;
                    guard.push(CapturedNotification { body: parsed_body });
                    shared_notify.notify_one();
                    AxumStatusCode::OK
                }
            }),
        );

        let listener = match tokio::net::TcpListener::bind("0.0.0.0:0").await {
            Ok(listener) => listener,
            Err(err) => panic!("failed to bind notification receiver: {err}"),
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(err) => panic!("failed to read notification receiver local address: {err}"),
        };

        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            let _ = server.await;
        });

        NotificationReceiver {
            endpoint_url: format!("http://host.docker.internal:{port}/notify"),
            notifications,
            notify,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    async fn wait_for_notification(&self, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut guard = self.notifications.lock().await;
                if !guard.is_empty() {
                    return guard.remove(0);
                }
            }
            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("timed out waiting for notification after {timeout:?}");
            }
        }
    }

    async fn wait_for_no_notification(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
        let guard = self.notifications.lock().await;
        assert!(guard.is_empty(), "expected no notification, got {}", guard.len());
    }
}

impl Drop for NotificationReceiver {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_time_interval_subscription_sends_periodic_notification() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = unique_registration_id("047-01-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-01-subscription");
    let subscription = time_interval_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(15)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_subscription_creation_and_registration_update_emit_trigger_reasons() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = unique_registration_id("047-02-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-02-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let first_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(first_notification.body["subscriptionId"], subscription_id);
    assert_eq!(first_notification.body["triggerReason"], "newlyMatching");

    let update_fragment = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &update_fragment).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let second_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(second_notification.body["subscriptionId"], subscription_id);
    assert_eq!(second_notification.body["triggerReason"], "updated");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_includes_only_matching_registration_information() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-03-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-03-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = match notification.body["data"].as_array() {
        Some(data) => data,
        None => panic!("data should be an array: {}", notification.body),
    };
    assert!(!data.is_empty(), "data should not be empty");
    let information = match data[0]["information"].as_array() {
        Some(information) => information,
        None => panic!("information should be an array: {}", data[0]),
    };
    assert_eq!(information.len(), 1, "only matching Building information should be included");

    for info_item in information {
        let entities = match info_item["entities"].as_array() {
            Some(entities) => entities,
            None => panic!("entities should be an array: {info_item}"),
        };
        for entity in entities {
            let entity_type = match entity["type"].as_str() {
                Some(entity_type) => entity_type,
                None => panic!("entity type should be a string: {entity}"),
            };
            assert!(is_building_entity_type(entity_type), "unexpected entity type in notification: {entity_type}");
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_has_required_csource_notification_structure() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-04-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-04-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_iso_timestamp(&notification.body["notifiedAt"], "notifiedAt");
    assert!(notification.body["triggerReason"].is_string(), "triggerReason should be present");
    let data = match notification.body["data"].as_array() {
        Some(data) => data,
        None => panic!("data should be an array: {}", notification.body),
    };
    assert!(!data.is_empty(), "data should not be empty");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_success_updates_metadata_status_ok() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-05-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-05-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    assert_eq!(retrieve_status, StatusCode::OK);
    assert_eq!(body["notification"]["timesSent"], 1);
    assert_eq!(body["notification"]["status"], "ok");
    assert_iso_timestamp(&body["notification"]["lastNotification"], "lastNotification");
    assert_iso_timestamp(&body["notification"]["lastSuccess"], "lastSuccess");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_failed_delivery_updates_metadata_status_failed() {
    clean_database().await;
    let http_client = client();

    let subscription_id = unique_subscription_id("047-06-subscription");
    let subscription = unreachable_endpoint_subscription_payload(&subscription_id);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-06-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    tokio::time::sleep(Duration::from_secs(3)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    assert_eq!(retrieve_status, StatusCode::OK);
    assert_eq!(body["notification"]["status"], "failed");
    assert_iso_timestamp(&body["notification"]["lastFailure"], "lastFailure");
    assert_iso_timestamp(&body["notification"]["lastNotification"], "lastNotification");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_paused_subscription_does_not_emit_notifications() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-07-paused-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let pause_fragment = subscription_update_is_active_false_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &pause_fragment).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let registration_id = unique_registration_id("047-07-paused-registration");
    let registration = base_registration_for_type(&registration_id, "Building");
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    receiver.wait_for_no_notification(Duration::from_secs(5)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_expired_subscription_does_not_emit_notifications() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-07-expired-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let expire_fragment = subscription_update_expires_at_past_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &expire_fragment).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let registration_id = unique_registration_id("047-07-expired-registration");
    let registration = base_registration_for_type(&registration_id, "Building");
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    receiver.wait_for_no_notification(Duration::from_secs(5)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_matching_registration_update_emits_updated() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = unique_registration_id("047-08-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-08-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let first_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(first_notification.body["triggerReason"], "newlyMatching");

    let registration_patch = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &registration_patch).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let second_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(second_notification.body["subscriptionId"], subscription_id);
    assert_eq!(second_notification.body["triggerReason"], "updated");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_non_matching_registration_update_emits_no_longer_matching() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = unique_registration_id("047-09-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-09-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let patch = registration_information_new_type_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &patch).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "noLongerMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_temporal_observed_at_matches_observation_interval() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-10-subscription");
    let subscription = temporal_query_subscription_payload(&subscription_id, &receiver.endpoint_url, "observedAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-10-registration");
    let registration = observation_interval_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_temporal_created_at_matches_management_interval() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-11-created-subscription");
    let subscription = temporal_query_subscription_payload(&subscription_id, &receiver.endpoint_url, "createdAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-11-created-registration");
    let registration = management_interval_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_temporal_modified_at_matches_management_interval() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-11-modified-subscription");
    let subscription = temporal_query_subscription_payload(&subscription_id, &receiver.endpoint_url, "modifiedAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-11-modified-registration");
    let registration = management_interval_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_watched_attributes_filter_matches_registration() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-12-subscription");
    let subscription = watched_attributes_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-12-registration");
    let registration = detailed_building_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_without_watched_attributes_matches_any_information() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-13-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-13-registration");
    let registration = detailed_building_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_geoq_with_geoproperty_matches_location() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-14-subscription");
    let subscription = geoq_subscription_payload(&subscription_id, &receiver.endpoint_url, true);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-14-registration");
    let registration = location_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_geoq_without_geoproperty_defaults_to_location() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("047-15-subscription");
    let subscription = geoq_subscription_payload(&subscription_id, &receiver.endpoint_url, false);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = unique_registration_id("047-15-registration");
    let registration = location_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_subscription_update_to_vehicle_matches_vehicle_registration_only() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = unique_registration_id("047-16-vehicle-registration");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, "Vehicle");
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = unique_registration_id("047-16-bus-registration");
    let bus_registration = base_registration_for_type(&bus_registration_id, "Bus");
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-16-vehicle-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_vehicle_entities_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = match notification.body["data"].as_array() {
        Some(data) => data,
        None => panic!("data should be an array: {}", notification.body),
    };
    assert_eq!(data.len(), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_subscription_update_to_bus_matches_bus_registration_only() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = unique_registration_id("047-16-vehicle-registration-2");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, "Vehicle");
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = unique_registration_id("047-16-bus-registration-2");
    let bus_registration = base_registration_for_type(&bus_registration_id, "Bus");
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-16-bus-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_bus_entities_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = match notification.body["data"].as_array() {
        Some(data) => data,
        None => panic!("data should be an array: {}", notification.body),
    };
    assert_eq!(data.len(), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_subscription_update_to_vehicle_and_bus_matches_both_registrations() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = unique_registration_id("047-16-vehicle-registration-3");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, "Vehicle");
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = unique_registration_id("047-16-bus-registration-3");
    let bus_registration = base_registration_for_type(&bus_registration_id, "Bus");
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("047-16-vehicle-and-bus-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_vehicle_and_bus_entities_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = match notification.body["data"].as_array() {
        Some(data) => data,
        None => panic!("data should be an array: {}", notification.body),
    };
    assert_eq!(data.len(), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_metadata_times_sent_increments_after_multiple_successes() {
    clean_database().await;
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = unique_registration_id("extra-metadata-registration");
    let registration = building_and_bus_registration_payload(&registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("extra-metadata-subscription");
    let subscription = base_subscription_payload(&subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(subscription_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let update_fragment = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &update_fragment).await;
    assert_eq!(update_status, StatusCode::NO_CONTENT);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    assert_eq!(retrieve_status, StatusCode::OK);
    let times_sent = match body["notification"]["timesSent"].as_i64() {
        Some(times_sent) => times_sent,
        None => panic!("timesSent should be an integer: {body}"),
    };
    assert!(times_sent >= 2, "timesSent should be >= 2 after two successful deliveries, got {times_sent}");
    assert_iso_timestamp(&body["notification"]["lastSuccess"], "lastSuccess");
}

#[tokio::test]
#[serial_test::serial]
async fn csr_notification_expires_at_in_future_results_in_active_status_before_deadline() {
    clean_database().await;
    let http_client = client();

    let subscription_id = unique_subscription_id("extra-expiration-status");
    let endpoint = "http://127.0.0.1:8085/notify";
    let mut subscription = base_subscription_payload(&subscription_id, endpoint);
    let future_expiration = (Utc::now() + ChronoDuration::seconds(20)).to_rfc3339_opts(SecondsFormat::Secs, true);
    subscription["expiresAt"] = Value::String(future_expiration);

    let (create_status, _) = create_csource_subscription(&http_client, &subscription).await;
    assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    assert_eq!(retrieve_status, StatusCode::OK);
    assert_eq!(body["status"], "active");
}
