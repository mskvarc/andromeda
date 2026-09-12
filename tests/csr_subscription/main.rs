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

/// Resets the shared test database and returns the serialized environment guard.
///
/// # Panics
///
/// Panics when the test database environment cannot be acquired or reset.
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

/// Creates a context source subscription and returns the status, `Location` header and body.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Retrieves a single context source subscription, optionally passing `?options=`.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Queries context source subscriptions with the given raw query string.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Patches a context source subscription with the given fragment.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Patches a context source subscription with an unparsed request body.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Deletes a context source subscription.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

#[must_use]
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

#[must_use]
pub fn inactive_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["isActive"] = Value::Bool(false);
    payload
}

#[must_use]
pub fn expires_at_csource_subscription_payload(id: &str, expires_at: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["expiresAt"] = Value::String(expires_at.to_owned());
    payload
}

#[must_use]
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

#[must_use]
pub fn invalid_type_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["type"] = Value::String("RandomType".to_owned());
    payload
}

#[must_use]
pub fn invalid_query_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["q"] = Value::String("invalidQuery".to_owned());
    payload
}

#[must_use]
pub fn empty_watched_attributes_csource_subscription_payload(id: &str) -> Value {
    let mut payload = csource_subscription_payload(id);
    payload["watchedAttributes"] = Value::Array(Vec::new());
    payload
}

#[must_use]
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

#[must_use]
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

#[must_use]
pub fn update_invalid_notification_fragment() -> Value {
    json!({
        "notification": {
            "format": "keyValues"
        }
    })
}

#[must_use]
pub fn update_entities_vehicle_fragment() -> Value {
    json!({
        "entities": [
            {
                "type": "Vehicle"
            }
        ]
    })
}

/// Issues a bare `GET` against the broker with no negotiated headers.
///
/// # Panics
///
/// Panics when the HTTP request cannot be sent or the response cannot be received.
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

/// Asserts that a `ProblemDetails` body carries the expected `type` and a `title`.
///
/// # Panics
///
/// Panics when the `type` member differs from `expected_type` or `title` is not a string.
pub fn assert_problem_detail_type(body: &Value, expected_type: &str) {
    pretty_assertions::assert_eq!(body["type"], expected_type, "unexpected problem detail type: {body}");
    assert!(body["title"].is_string(), "problem details title should be present: {body}");
}

#[tokio::test]
async fn csr_subscription_sys_attrs_adds_the_timestamps_without_changing_the_other_members() {
    // Clause 4.8: `?options=sysAttrs` governs the system timestamps and nothing
    // else. Every other policy the renderer applies -- hiding `isActive` and
    // `localOnly` at their defaults -- is independent of it, and the clause
    // 5.2.12 members are served either way. `jsonldContext` is one of them:
    // clause 5.11.2.3 takes the Subscription data type unchanged and clause
    // 5.11.4.5 returns "the subscription details as mandated by clause 5.2.12",
    // while clause 5.8.1.4 requires the member to be initialised from the
    // @context the subscription was created under. The broker used to scrub it
    // here as though it were internal.
    let client = client();
    let id = "urn:ngsi-ld:Subscription:r2-76-sysattrs-leak";
    let (status, _, _) = create_csource_subscription(&client, &csource_subscription_payload(id)).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, plain) = retrieve_csource_subscription(&client, id, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, with_sys_attrs) = retrieve_csource_subscription(&client, id, Some("sysAttrs")).await;
    assert_eq!(status, StatusCode::OK);

    assert!(with_sys_attrs.get("createdAt").is_some(), "sysAttrs adds createdAt: {with_sys_attrs}");
    assert!(plain.get("createdAt").is_none(), "and only sysAttrs does: {plain}");
    pretty_assertions::assert_eq!(
        with_sys_attrs.get("jsonldContext"),
        plain.get("jsonldContext"),
        "the clause 5.2.12 members do not depend on sysAttrs: {with_sys_attrs}"
    );
    assert!(
        plain.get("jsonldContext").is_some(),
        "clause 5.8.1.4 initialises the member from the applicable @context: {plain}"
    );

    delete_csource_subscription(&client, id).await;
}
