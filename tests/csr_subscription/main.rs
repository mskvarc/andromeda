mod create;
mod delete;
mod query;
mod retrieve;
mod update;

use reqwest::{Client, Method, StatusCode, header::LOCATION};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const BAD_REQUEST_DATA_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
const INVALID_REQUEST_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
const RESOURCE_NOT_FOUND_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
const ALREADY_EXISTS_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists";

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
    format!("urn:ngsi-ld:Subscription:csr-sub:{scenario}:{}", Uuid::new_v4())
}

pub async fn clean_database() {
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

pub async fn create_csource_subscription(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let response = match client
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
    let location = response.headers().get(LOCATION).and_then(|value| value.to_str().ok()).map(ToOwned::to_owned);
    let body = read_body(response).await;
    (status, location, body)
}

pub async fn retrieve_csource_subscription(client: &Client, subscription_id: &str, options: Option<&str>) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let query = match options {
        Some(options) => format!("?options={options}"),
        None => String::new(),
    };
    let response = match client
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

pub async fn query_csource_subscriptions(client: &Client, query: &str) -> (StatusCode, Value) {
    let separator = if query.is_empty() { "" } else { "?" };
    let response = match client
        .get(format!("{BASE_URL}/csourceSubscriptions{separator}{query}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("query csource subscriptions request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn update_csource_subscription(client: &Client, subscription_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = match client
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

pub async fn update_csource_subscription_raw(client: &Client, subscription_id: &str, raw_fragment: &[u8]) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = match client
        .patch(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}"))
        .header("Content-Type", "application/json")
        .body(raw_fragment.to_vec())
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => panic!("update csource subscription raw request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn delete_csource_subscription(client: &Client, subscription_id: &str) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = match client.delete(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}")).send().await {
        Ok(response) => response,
        Err(err) => panic!("delete csource subscription request failed: {err}"),
    };

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub fn csource_subscription_payload(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [
            {
                "type": "Building"
            }
        ],
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

pub fn inactive_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["isActive"] = Value::Bool(false);
    payload
}

pub fn expires_at_csource_subscription_payload(id: &str, expires_at: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["expiresAt"] = Value::String(expires_at.to_owned());
    payload
}

pub fn csource_subscription_without_notification_payload(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [
            {
                "type": "Building"
            }
        ],
        "@context": [CORE_CONTEXT]
    })
}

pub fn invalid_type_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["type"] = Value::String("RandomType".to_owned());
    payload
}

pub fn invalid_query_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["q"] = Value::String("invalidQuery".to_owned());
    payload
}

pub fn empty_watched_attributes_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["watchedAttributes"] = Value::Array(Vec::new());
    payload
}

pub fn update_notification_fragment() -> Value {
    json!({
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": "http://my.second.endpoint.org/notify",
                "accept": "application/json"
            }
        }
    })
}

pub fn update_invalid_type_fragment() -> Value {
    json!({
        "type": Value::Null,
        "notification": {
            "format": "keyValues",
            "endpoint": {
                "uri": "http://my.second.endpoint.org/notify",
                "accept": "application/json"
            }
        }
    })
}

pub fn update_invalid_notification_fragment() -> Value {
    json!({
        "notification": {
            "format": "keyValues"
        }
    })
}

pub fn update_entities_vehicle_fragment() -> Value {
    json!({
        "entities": [
            {
                "type": "Vehicle"
            }
        ]
    })
}

pub async fn raw_get(client: &Client, path_and_query: &str) -> (StatusCode, Value) {
    let response = match client.request(Method::GET, format!("{BASE_URL}{path_and_query}")).send().await {
        Ok(response) => response,
        Err(err) => panic!("raw get request failed: {err}"),
    };
    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub fn assert_problem_detail_type(body: &Value, expected_type: &str) {
    assert_eq!(body["type"], expected_type, "unexpected problem detail type: {body}");
    assert!(body["title"].is_string(), "problem details title should be present: {body}");
}
