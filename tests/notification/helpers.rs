use axum::{Router, body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use reqwest::{Client, StatusCode, header::HeaderMap};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

pub const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
pub const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
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

pub fn client() -> Client {
    Client::new()
}

pub async fn reset_broker_state() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

pub fn unique_entity_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

pub fn unique_subscription_id(case_id: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{case_id}:{}", Uuid::new_v4())
}

pub fn building_simple_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {"type": "Property", "value": false},
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_minimal_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "@context": [CORE_CONTEXT]
    })
}

pub fn vehicle_simple_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Vehicle",
        "brandName": {"type": "Property", "value": "Mercedes"},
        "isParked": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:OffStreetParking:Downtown1",
            "observedAt": "2017-07-29T12:00:04Z"
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_different_attribute_types_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [-8.5, 41.2]}
        },
        "street": {
            "type": "LanguageProperty",
            "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_relationship_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
        "@context": [CORE_CONTEXT]
    })
}

pub fn city_minimal_entity(city_id: &str) -> Value {
    json!({
        "id": city_id,
        "type": "City",
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_json_property_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "jsonProperty": {
            "type": "JsonProperty",
            "json": {
                "id": "some-id",
                "type": "some-type",
                "null": null,
                "anythingElse": "anything-else"
            }
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_vocab_property_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "vocabProperty": {"type": "VocabProperty", "vocab": "monument"},
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_multi_instances_entity(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "name": [
            {"type": "Property", "value": "Eiffel Tower"},
            {"type": "Property", "value": "Tour Eiffel", "datasetId": "urn:ngsi-ld:Dataset:french-name"},
            {"type": "Property", "value": "Eiffelturm", "datasetId": "urn:ngsi-ld:Dataset:german-name"}
        ],
        "locatedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
            {"type": "Relationship", "object": "urn:ngsi-ld:Ville:Paris", "datasetId": "urn:ngsi-ld:Dataset:french-name"},
            {"type": "Relationship", "object": "urn:ngsi-ld:Ciudad:París", "datasetId": "urn:ngsi-ld:Dataset:spanish-name"}
        ],
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_air_quality_level() -> Value {
    json!({
        "airQualityLevel": {
            "type": "Property",
            "value": 5,
            "unitCode": "C62",
            "observedAt": "2020-10-10T16:40:00.000Z"
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_name() -> Value {
    json!({"name": {"type": "Property", "value": "Tour Eiffel"}, "@context": [CORE_CONTEXT]})
}

pub fn fragment_located_at() -> Value {
    json!({"locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Nantes"}, "@context": [CORE_CONTEXT]})
}

pub fn fragment_location() -> Value {
    json!({
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_street_language() -> Value {
    json!({
        "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "es": "Gran Lugar"}},
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_json_property_partial() -> Value {
    json!({
        "json": {
            "id": "another-id",
            "type": "another-type",
            "anythingElse": "another-anything-else"
        },
        "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:JohnDoe"},
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_vocab_property_partial() -> Value {
    json!({
        "vocab": "sightseeing",
        "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:JohnDoe"},
        "@context": [CORE_CONTEXT]
    })
}

pub fn fragment_building_name_dataset() -> Value {
    json!({
        "name": {"type": "Property", "value": "Le Tour Eiffel", "datasetId": "urn:ngsi-ld:Dataset:french-name"},
        "@context": [CORE_CONTEXT]
    })
}

pub fn null_property_fragment() -> Value {
    json!({"name": {"type": "Property", "value": "urn:ngsi-ld:null"}, "@context": [CORE_CONTEXT]})
}

pub fn null_property_partial_fragment() -> Value {
    json!({"type": "Property", "value": "urn:ngsi-ld:null", "@context": [CORE_CONTEXT]})
}

pub fn null_relationship_fragment() -> Value {
    json!({"locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:null"}, "@context": [CORE_CONTEXT]})
}

pub fn null_relationship_partial_fragment() -> Value {
    json!({"type": "Relationship", "object": "urn:ngsi-ld:null", "@context": [CORE_CONTEXT]})
}

pub fn null_language_property_fragment() -> Value {
    json!({
        "street": {"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}},
        "@context": [CORE_CONTEXT]
    })
}

pub fn null_language_property_partial_fragment() -> Value {
    json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}, "@context": [CORE_CONTEXT]})
}

pub fn base_subscription(subscription_id: &str) -> Value {
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "normalized",
            "endpoint": {
                "uri": "http://127.0.0.1:8085/notify",
                "accept": "application/json"
            }
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn subscription_with_endpoint(subscription_id: &str, endpoint_uri: &str) -> Value {
    let mut sub = base_subscription(subscription_id);
    sub["notification"]["endpoint"]["uri"] = json!(endpoint_uri);
    sub
}

pub fn set_endpoint_uri(subscription: &mut Value, endpoint_uri: &str) {
    subscription["notification"]["endpoint"]["uri"] = json!(endpoint_uri);
}

pub fn with_time_interval(mut sub: Value, seconds: i64) -> Value {
    sub["timeInterval"] = json!(seconds);
    sub["notification"].as_object_mut().expect("notification object").remove("format");
    sub
}

pub fn with_is_active(mut sub: Value, is_active: bool) -> Value {
    sub["isActive"] = json!(is_active);
    sub
}

pub fn with_watched_attributes(mut sub: Value, watched_attributes: &[&str]) -> Value {
    sub["watchedAttributes"] = Value::Array(watched_attributes.iter().map(|attr| json!(attr)).collect());
    sub
}

pub fn with_query_filter(mut sub: Value, query: &str) -> Value {
    sub["q"] = json!(query);
    sub
}

pub fn with_notification_attributes(mut sub: Value, attributes: &[&str]) -> Value {
    sub["notification"]["attributes"] = Value::Array(attributes.iter().map(|attr| json!(attr)).collect());
    sub
}

pub fn with_notification_format(mut sub: Value, format: &str) -> Value {
    sub["notification"]["format"] = json!(format);
    sub
}

pub fn with_endpoint_accept(mut sub: Value, accept: &str) -> Value {
    sub["notification"]["endpoint"]["accept"] = json!(accept);
    sub
}

pub fn with_notification_trigger(mut sub: Value, triggers: &[&str]) -> Value {
    sub["notificationTrigger"] = Value::Array(triggers.iter().map(|trigger| json!(trigger)).collect());
    sub
}

pub fn with_show_changes(mut sub: Value) -> Value {
    sub["notification"]["showChanges"] = json!(true);
    sub
}

pub fn with_sys_attrs(mut sub: Value, sys_attrs: bool) -> Value {
    sub["notification"]["sysAttrs"] = json!(sys_attrs);
    sub
}

pub fn with_dataset_id(mut sub: Value, dataset_ids: &[&str]) -> Value {
    sub["datasetId"] = Value::Array(dataset_ids.iter().map(|dataset_id| json!(dataset_id)).collect());
    sub
}

pub fn with_jsonld_context(mut sub: Value, context_url: &str) -> Value {
    sub["jsonldContext"] = json!(context_url);
    sub
}

pub fn with_join(mut sub: Value, join_mode: &str, join_level: i64) -> Value {
    sub["notification"]["join"] = json!(join_mode);
    sub["notification"]["joinLevel"] = json!(join_level);
    sub["notification"].as_object_mut().expect("notification object").remove("format");
    sub
}

pub fn with_receiver_info(mut sub: Value, key: &str, value: &str) -> Value {
    sub["notification"]["endpoint"]["receiverInfo"] = json!([{"key": key, "value": value}]);
    sub
}

pub fn with_notifier_info(mut sub: Value, key: &str, value: &str) -> Value {
    sub["notification"]["endpoint"]["notifierInfo"] = json!([{"key": key, "value": value}]);
    sub
}

pub fn with_entity_type_selector(mut sub: Value, entity_type_selector: &str) -> Value {
    sub["entities"] = json!([{"type": entity_type_selector}]);
    sub
}

pub async fn create_entity(http_client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create entity request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn create_subscription(http_client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create subscription request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;

    if status == StatusCode::CREATED {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    (status, headers, body)
}

pub async fn retrieve_subscription(http_client: &Client, subscription_id: &str) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = http_client
        .get(format!("{BASE_URL}/subscriptions/{encoded_id}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve subscription request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn append_entity_attrs(http_client: &Client, entity_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let response = http_client
        .post(format!("{BASE_URL}/entities/{encoded_id}/attrs"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("append entity attrs request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn update_entity_attrs(http_client: &Client, entity_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let response = http_client
        .patch(format!("{BASE_URL}/entities/{encoded_id}/attrs"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("update entity attrs request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn merge_entity(http_client: &Client, entity_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let response = http_client
        .patch(format!("{BASE_URL}/entities/{encoded_id}"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("merge entity request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn partial_update_attr(http_client: &Client, entity_id: &str, attr_name: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let encoded_attr = urlencoding::encode(attr_name);
    let response = http_client
        .patch(format!("{BASE_URL}/entities/{encoded_id}/attrs/{encoded_attr}"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("partial update attr request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn delete_entity_attr(
    http_client: &Client,
    entity_id: &str,
    attr_name: &str,
    dataset_id: Option<&str>,
    delete_all: Option<bool>,
) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let encoded_attr = urlencoding::encode(attr_name);
    let mut query = Vec::<String>::new();

    if let Some(dataset_id) = dataset_id {
        query.push(format!("datasetId={}", urlencoding::encode(dataset_id)));
    }

    if let Some(delete_all) = delete_all {
        query.push(format!("deleteAll={delete_all}"));
    }

    let query_suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };

    let response = http_client
        .delete(format!("{BASE_URL}/entities/{encoded_id}/attrs/{encoded_attr}{query_suffix}"))
        .send()
        .await
        .expect("delete entity attr request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn delete_entity(http_client: &Client, entity_id: &str) -> (StatusCode, Vec<u8>) {
    let encoded_id = urlencoding::encode(entity_id);
    let response = http_client
        .delete(format!("{BASE_URL}/entities/{encoded_id}"))
        .send()
        .await
        .expect("delete entity request failed");

    let status = response.status();
    let body = response.bytes().await.unwrap_or_default().to_vec();
    (status, body)
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

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
    pub async fn start() -> Self {
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

        Self {
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

    pub async fn clear_notifications(&self) {
        self.notifications.lock().await.clear();
    }

    pub async fn wait_for_subscription_notification(&self, subscription_id: &str, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            {
                let mut notifications = self.notifications.lock().await;
                if let Some(index) = notifications
                    .iter()
                    .position(|notification| notification.body["subscriptionId"].as_str() == Some(subscription_id))
                {
                    return notifications.remove(index);
                }
            }

            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("timed out waiting for subscription notification after {timeout:?}");
            }
        }
    }

    pub async fn wait_for_no_subscription_notification(&self, subscription_id: &str, duration: Duration) {
        tokio::time::sleep(duration).await;
        let notifications = self.notifications.lock().await;
        let matching_count = notifications
            .iter()
            .filter(|notification| notification.body["subscriptionId"].as_str() == Some(subscription_id))
            .count();
        assert_eq!(
            matching_count, 0,
            "expected no notifications for subscription {subscription_id}, received {matching_count}"
        );
    }

    pub fn stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

impl Drop for NotificationReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct MqttMessage {
    pub topic: String,
    pub payload: Value,
}

pub struct MqttReceiver {
    client: AsyncClient,
    messages: Arc<Mutex<Vec<MqttMessage>>>,
    notify: Arc<Notify>,
    _event_loop: tokio::task::JoinHandle<()>,
}

impl MqttReceiver {
    pub async fn connect(host: &str, port: u16) -> Self {
        Self::connect_with_credentials(host, port, None, None).await
    }

    pub async fn connect_with_credentials(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> Self {
        let client_id = format!("notification-test-receiver-{}", Uuid::new_v4());
        let mut options = MqttOptions::new(client_id, host, port);
        options.set_keep_alive(Duration::from_secs(30));

        if let Some(username) = username {
            options.set_credentials(username, password.unwrap_or(""));
        }

        let (client, mut event_loop) = AsyncClient::new(options, 10);
        let messages = Arc::new(Mutex::new(Vec::<MqttMessage>::new()));
        let notify = Arc::new(Notify::new());

        let message_store = messages.clone();
        let signal = notify.clone();

        let event_loop_handle = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let payload = serde_json::from_slice::<Value>(&publish.payload).unwrap_or(Value::Null);
                        message_store.lock().await.push(MqttMessage { topic: publish.topic, payload });
                        signal.notify_one();
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Self {
            client,
            messages,
            notify,
            _event_loop: event_loop_handle,
        }
    }

    pub async fn subscribe(&self, topic: &str, qos: QoS) {
        self.client.subscribe(topic, qos).await.expect("mqtt subscribe failed");
    }

    pub async fn wait_for_message(&self, timeout: Duration) -> MqttMessage {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            {
                let mut messages = self.messages.lock().await;
                if !messages.is_empty() {
                    return messages.remove(0);
                }
            }

            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("timed out waiting for mqtt message after {timeout:?}");
            }
        }
    }

    pub async fn disconnect(self) {
        let _ = self.client.disconnect().await;
    }
}
