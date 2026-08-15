#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use andromeda_test_mocks::mqtt_receiver::{MqttCredentials, MqttHost, MqttReceiver as RawMqttReceiver, MqttTopic};
use reqwest::{Client, StatusCode, header::HeaderMap};
use rumqttc::QoS;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub fn client() -> Client {
    Client::new()
}

pub async fn reset_broker_state() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
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
        sleep(Duration::from_millis(150)).await;
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

pub struct MqttMessage {
    pub topic: String,
    pub payload: Value,
}

pub struct MqttReceiver {
    receiver: RawMqttReceiver,
}

impl MqttReceiver {
    pub async fn connect(host: &str, port: u16) -> Self {
        Self::connect_with_credentials(host, port, None, None).await
    }

    pub async fn connect_with_credentials(host: &str, port: u16, username: Option<&str>, password: Option<&str>) -> Self {
        let credentials = username.map(|username| MqttCredentials::new(username, password.unwrap_or("")));
        Self {
            receiver: RawMqttReceiver::connect(MqttHost::new(host), port, credentials),
        }
    }

    pub async fn subscribe(&self, topic: &str, qos: QoS) {
        self.receiver.subscribe(&MqttTopic::new(topic), qos).await.expect("MQTT subscription succeeds");
    }

    pub async fn wait_for_message(&self, timeout: Duration) -> MqttMessage {
        let message = self.receiver.next_message(timeout).await.expect("MQTT notification arrives before timeout");
        MqttMessage {
            topic: message.topic.as_str().to_owned(),
            payload: message.json().expect("MQTT notification body is valid JSON"),
        }
    }

    pub async fn disconnect(self) {
        self.receiver.shutdown().await.expect("MQTT receiver shuts down");
    }
}
