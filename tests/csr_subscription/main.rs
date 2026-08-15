#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
mod create;
mod delete;
mod query;
mod retrieve;
mod update;

use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail as error};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, Method, StatusCode, header::LOCATION};
use serde_json::{Value, json};
use uuid::Uuid;

const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;
const ALREADY_EXISTS_TYPE: &iri_rs::Iri<&str> = error::ALREADY_EXISTS;

fn client() -> Client {
    Client::new()
}

fn unique_subscription_id(scenario: &str) -> String {
    format!("urn:ngsi-ld:Subscription:csr-sub:{scenario}:{}", Uuid::new_v4())
}

pub async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

async fn read_body(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

pub async fn create_csource_subscription(client: &Client, body: &Value) -> (StatusCode, Option<String>, Value) {
    let response = client
        .post(format!("{BASE_URL}/csourceSubscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create csource subscription request failed");

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
    let response = client
        .get(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}{query}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve csource subscription request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn query_csource_subscriptions(client: &Client, query: &str) -> (StatusCode, Value) {
    let separator = if query.is_empty() { "" } else { "?" };
    let response = client
        .get(format!("{BASE_URL}/csourceSubscriptions{separator}{query}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("query csource subscriptions request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn update_csource_subscription(client: &Client, subscription_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = client
        .patch(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}"))
        .header("Content-Type", "application/json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .json(fragment)
        .send()
        .await
        .expect("update csource subscription request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn update_csource_subscription_raw(client: &Client, subscription_id: &str, raw_fragment: &[u8]) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = client
        .patch(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}"))
        .header("Content-Type", "application/json")
        .body(raw_fragment.to_vec())
        .send()
        .await
        .expect("update csource subscription raw request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub async fn delete_csource_subscription(client: &Client, subscription_id: &str) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = client
        .delete(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}"))
        .send()
        .await
        .expect("delete csource subscription request failed");

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
    let response = client
        .request(Method::GET, format!("{BASE_URL}{path_and_query}"))
        .send()
        .await
        .expect("raw get request failed");
    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

pub fn assert_problem_detail_type(body: &Value, expected_type: &str) {
    pretty_assertions::assert_eq!(body["type"], expected_type, "unexpected problem detail type: {body}");
    assert!(body["title"].is_string(), "problem details title should be present: {body}");
}

#[tokio::test]
async fn csr_subscription_sys_attrs_does_not_leak_the_internal_jsonld_context() {
    // R2-76 (clause 5.2.9, Table 5.2.9-2): `?options=sysAttrs` adds createdAt and
    // modifiedAt, but must not stop scrubbing the internal `jsonldContext` the
    // broker stores. Gating five unrelated scrubbing policies on one sysAttrs
    // flag leaked it.
    let client = client();
    let id = "urn:ngsi-ld:Subscription:r2-76-sysattrs-leak";
    let (status, _, _) = create_csource_subscription(&client, &csource_subscription_payload(id)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_csource_subscription(&client, id, Some("sysAttrs")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("jsonldContext").is_none(), "sysAttrs must not leak the internal jsonldContext: {body}");
    assert!(body.get("createdAt").is_some(), "sysAttrs must still add createdAt");

    delete_csource_subscription(&client, id).await;
}
