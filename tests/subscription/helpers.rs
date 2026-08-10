#![allow(clippy::expect_used, clippy::unwrap_used)]
use andromeda_test_common::{
    config::{BASE_URL, CORE_CONTEXT, DATABASE_URL},
    db::TRUNCATE_ALL,
    error,
};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
pub const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;
pub const ALREADY_EXISTS_TYPE: &iri_rs::Iri<&str> = error::ALREADY_EXISTS;
pub const LD_CONTEXT_NOT_AVAILABLE_TYPE: &iri_rs::Iri<&str> = error::LD_CONTEXT_NOT_AVAILABLE;

pub fn client() -> Client {
    Client::new()
}

pub async fn reset_broker_state() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(&TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

pub fn unique_subscription_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{scenario}:{}", Uuid::new_v4())
}

pub fn default_subscription(subscription_id: &str) -> Value {
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "timeInterval": 5,
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": "http://127.0.0.1:8085/notify",
                "accept": "application/json"
            }
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn inactive_subscription(subscription_id: &str) -> Value {
    let mut sub = default_subscription(subscription_id);
    sub["isActive"] = json!(false);
    sub.as_object_mut().expect("subscription object").remove("timeInterval");
    sub
}

pub fn watched_attributes_subscription(subscription_id: &str) -> Value {
    let mut sub = default_subscription(subscription_id);
    sub.as_object_mut().expect("subscription object").remove("timeInterval");
    sub["watchedAttributes"] = json!(["name", "subCategory"]);
    sub
}

pub fn dataset_id_subscription(subscription_id: &str) -> Value {
    let mut sub = default_subscription(subscription_id);
    sub.as_object_mut().expect("subscription object").remove("timeInterval");
    sub["datasetId"] = json!(["urn:ngsi-ld:Dataset:french-name"]);
    sub["notification"]["format"] = json!("normalized");
    sub["notification"]["endpoint"]["accept"] = json!("application/ld+json");
    sub
}

pub fn invalid_throttling_and_time_interval_subscription(subscription_id: &str) -> Value {
    let mut sub = default_subscription(subscription_id);
    sub["throttling"] = json!(30);
    sub
}

pub fn invalid_negative_throttling_subscription(subscription_id: &str) -> Value {
    let mut sub = default_subscription(subscription_id);
    sub.as_object_mut().expect("subscription object").remove("timeInterval");
    sub["throttling"] = json!(-30);
    sub
}

pub fn invalid_jsonld_context_subscription(subscription_id: &str) -> Value {
    let mut sub = dataset_id_subscription(subscription_id);
    sub["jsonldContext"] = json!("unknownContext");
    sub
}

pub fn unavailable_jsonld_context_subscription(subscription_id: &str) -> Value {
    let mut sub = dataset_id_subscription(subscription_id);
    sub["jsonldContext"] = json!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-non-existing.jsonld");
    sub
}

pub fn update_fragment() -> Value {
    json!({
        "type": "Subscription",
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": "http://my.second.endpoint.org/notify",
                "accept": "application/json"
            }
        }
    })
}

pub fn invalid_update_fragment() -> Value {
    json!({
        "notification": {
            "format": "keyValues"
        }
    })
}

pub fn null_properties_fragment() -> Value {
    json!({
        "type": "Subscription",
        "notification": {
            "format": "keyValues",
            "endpoint": null
        }
    })
}

pub fn vehicle_entities_fragment() -> Value {
    json!({
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": "http://127.0.0.1:8085/notify",
                "accept": "application/json"
            }
        }
    })
}

pub fn activate_fragment() -> Value {
    json!({"type": "Subscription", "isActive": true})
}

pub fn pause_fragment() -> Value {
    json!({"type": "Subscription", "isActive": false})
}

pub fn activate_with_expires_fragment() -> Value {
    json!({
        "type": "Subscription",
        "isActive": true,
        "expiresAt": "2030-08-01T22:07:00Z"
    })
}

pub fn past_expires_fragment() -> Value {
    json!({
        "type": "Subscription",
        "expiresAt": "2020-08-01T22:07:00Z"
    })
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
    (status, headers, body)
}

pub async fn retrieve_subscription(http_client: &Client, subscription_id: &str, with_link_context: bool) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let mut request = http_client
        .get(format!("{BASE_URL}/subscriptions/{encoded_id}"))
        .header("Accept", "application/ld+json");

    if with_link_context {
        request = request.header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }

    let response = request.send().await.expect("retrieve subscription request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn update_subscription(http_client: &Client, subscription_id: &str, fragment: &Value, with_link_context: bool) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let mut request = http_client
        .patch(format!("{BASE_URL}/subscriptions/{encoded_id}"))
        .header("Content-Type", "application/json");

    if with_link_context {
        request = request.header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }

    let response = request.json(fragment).send().await.expect("update subscription request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn delete_subscription(http_client: &Client, subscription_id: &str) -> (StatusCode, Vec<u8>) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = http_client
        .delete(format!("{BASE_URL}/subscriptions/{encoded_id}"))
        .send()
        .await
        .expect("delete subscription request failed");

    let status = response.status();
    let bytes = response.bytes().await.unwrap_or_default().to_vec();
    (status, bytes)
}

pub async fn query_subscriptions(http_client: &Client, query: &str, with_link_context: bool) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut request = http_client
        .get(format!("{BASE_URL}/subscriptions{sep}{query}"))
        .header("Accept", "application/ld+json");

    if with_link_context {
        request = request.header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }

    let response = request.send().await.expect("query subscriptions request failed");
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn raw_subscription_request(
    http_client: &Client,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
    content_type: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = http_client.request(method, format!("{BASE_URL}{path}"));

    if let Some(content_type) = content_type {
        request = request.header("Content-Type", content_type);
    }

    if let Some(body) = body {
        request = request.body(body.to_vec());
    }

    let response = request.send().await.expect("raw subscription request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}
