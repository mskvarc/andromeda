#![allow(clippy::expect_used, clippy::unwrap_used)]
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
pub const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
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

pub fn unique_subscription_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{scenario}:{}", Uuid::new_v4())
}

pub fn unique_registration_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:ContextSourceRegistration:{scenario}:{}", Uuid::new_v4())
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

pub fn default_registration(registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"type": "Building"}],
                "propertyNames": ["name"]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
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

pub async fn create_registration(http_client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create registration request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn delete_subscription(http_client: &Client, id: &str) -> StatusCode {
    let encoded = urlencoding::encode(id);
    http_client
        .delete(format!("{BASE_URL}/subscriptions/{encoded}"))
        .send()
        .await
        .expect("delete subscription request failed")
        .status()
}

pub async fn delete_registration(http_client: &Client, id: &str) -> StatusCode {
    let encoded = urlencoding::encode(id);
    http_client
        .delete(format!("{BASE_URL}/csourceRegistrations/{encoded}"))
        .send()
        .await
        .expect("delete registration request failed")
        .status()
}

pub async fn query_subscriptions(http_client: &Client, query_string: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query_string.is_empty() { "" } else { "?" };
    let response = http_client
        .get(format!("{BASE_URL}/subscriptions{sep}{query_string}"))
        .header("Accept", accept)
        .send()
        .await
        .expect("query subscriptions request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn query_registrations(http_client: &Client, query_string: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query_string.is_empty() { "" } else { "?" };
    let response = http_client
        .get(format!("{BASE_URL}/csourceRegistrations{sep}{query_string}"))
        .header("Accept", accept)
        .send()
        .await
        .expect("query registrations request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

/// Return all `Link` header values as strings.
pub fn collect_link_headers(headers: &HeaderMap) -> Vec<String> {
    headers.get_all("link").iter().filter_map(|v| v.to_str().ok()).map(|s| s.to_owned()).collect()
}

/// Find the `Link` header containing pagination rels (prev/next/first/last), not JSON-LD context.
pub fn find_pagination_link_header(headers: &HeaderMap) -> Option<String> {
    collect_link_headers(headers).into_iter().find(|h| {
        let lower = h.to_ascii_lowercase();
        (lower.contains("rel=\"prev\"")
            || lower.contains("rel=\"next\"")
            || lower.contains("rel=\"first\"")
            || lower.contains("rel=\"last\"")
            || lower.contains("rel=\"prev ")
            || lower.contains("rel=\"next ")
            || lower.contains(" prev\"")
            || lower.contains(" next\"")
            || lower.contains(" first\"")
            || lower.contains(" last\""))
            && !lower.contains("json-ld#context")
    })
}

/// Find the `Link` header containing JSON-LD context rel.
pub fn find_context_link_header(headers: &HeaderMap) -> Option<String> {
    collect_link_headers(headers).into_iter().find(|h| h.contains("json-ld#context"))
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}
