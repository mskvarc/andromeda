#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, environment::TestEnvironment};
use andromeda_test_mocks::{callback_server::CallbackServerError, notification_server::NotificationServer, server_config::MockServerConfig};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, SecondsFormat, Utc};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::time::sleep;

fn client() -> Client {
    Client::new()
}

async fn read_body(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

async fn create_csource_subscription(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/csourceSubscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create csource subscription request failed");

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

    let response = http_client
        .get(format!("{BASE_URL}/csourceSubscriptions/{encoded_id}{query}"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("retrieve csource subscription request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn update_csource_subscription(http_client: &Client, subscription_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(subscription_id);
    let response = http_client
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

async fn create_registration(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create registration request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

async fn update_registration(http_client: &Client, registration_id: &str, fragment: &Value) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(registration_id);
    let response = http_client
        .patch(format!("{BASE_URL}/csourceRegistrations/{encoded_id}"))
        .header("Content-Type", "application/json")
        .json(fragment)
        .send()
        .await
        .expect("update registration request failed");

    let status = response.status();
    let body = read_body(response).await;
    (status, body)
}

fn base_subscription_payload(scope: &TestEnvironment, subscription_id: &str, endpoint_uri: &str) -> Value {
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [
            {
                "type": scope.entity_type("Building")
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

fn time_interval_subscription_payload(scope: &TestEnvironment, subscription_id: &str, endpoint_uri: &str) -> Value {
    let mut payload = base_subscription_payload(scope, subscription_id, endpoint_uri);
    payload["timeInterval"] = Value::from(2);
    payload
}

/// A Context Source Registration Subscription whose `temporalQ` selects the
/// interval-bearing registrations of clause 5.11.2.4.
///
/// Clause 5.11.2.4 matches on overlap: the period an `after` query describes
/// runs from its `timeAt` onwards, and the registration's interval has to
/// share at least one instant with it. The `timeAt` below lies inside the
/// interval the observation-interval and management-interval registration
/// payloads declare, so it overlaps whichever way the relation is read.
fn temporal_query_subscription_payload(scope: &TestEnvironment, subscription_id: &str, endpoint_uri: &str, time_property: &str) -> Value {
    let mut payload = base_subscription_payload(scope, subscription_id, endpoint_uri);
    payload["temporalQ"] = json!({
        "timerel": "after",
        "timeAt": "2020-09-01T22:07:00Z",
        "timeproperty": time_property
    });
    payload
}

fn watched_attributes_subscription_payload(scope: &TestEnvironment, subscription_id: &str, endpoint_uri: &str) -> Value {
    let mut payload = base_subscription_payload(scope, subscription_id, endpoint_uri);
    payload["watchedAttributes"] = json!(["name", "subCategory"]);
    payload
}

fn geoq_subscription_payload(scope: &TestEnvironment, subscription_id: &str, endpoint_uri: &str, include_geoproperty: bool) -> Value {
    let mut payload = base_subscription_payload(scope, subscription_id, endpoint_uri);
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

fn unreachable_endpoint_subscription_payload(scope: &TestEnvironment, subscription_id: &str) -> Value {
    base_subscription_payload(scope, subscription_id, "http://unreachable.endpoint/notify")
}

fn building_and_bus_registration_payload(scope: &TestEnvironment, registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": scope.entity_type("Building")
                    }
                ]
            },
            {
                "entities": [
                    {
                        "type": scope.entity_type("Bus")
                    }
                ]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn detailed_building_registration_payload(scope: &TestEnvironment, registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": scope.entity_type("Building")
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

fn location_registration_payload(scope: &TestEnvironment, registration_id: &str) -> Value {
    json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [
                    {
                        "type": scope.entity_type("Building")
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

fn observation_interval_registration_payload(scope: &TestEnvironment, registration_id: &str) -> Value {
    let mut payload = base_registration_for_type(registration_id, &scope.entity_type("Building"));
    payload["observationInterval"] = json!({
        "startAt": "2020-08-01T22:07:00Z",
        "endAt": "2021-08-01T21:07:00Z"
    });
    payload
}

fn management_interval_registration_payload(scope: &TestEnvironment, registration_id: &str) -> Value {
    let mut payload = base_registration_for_type(registration_id, &scope.entity_type("Building"));
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

fn registration_information_new_type_fragment(scope: &TestEnvironment) -> Value {
    json!({
        "information": [
            {
                "entities": [
                    {
                        "type": scope.entity_type("NewType")
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

fn subscription_update_vehicle_entities_fragment(scope: &TestEnvironment) -> Value {
    json!({
        "entities": [
            {
                "type": scope.entity_type("Vehicle")
            }
        ]
    })
}

fn subscription_update_bus_entities_fragment(scope: &TestEnvironment) -> Value {
    json!({
        "entities": [
            {
                "type": scope.entity_type("Bus")
            }
        ]
    })
}

fn subscription_update_vehicle_and_bus_entities_fragment(scope: &TestEnvironment) -> Value {
    json!({
        "entities": [
            {
                "type": scope.entity_type("Vehicle")
            },
            {
                "type": scope.entity_type("Bus")
            }
        ]
    })
}

fn assert_iso_timestamp(value: &Value, field_name: &str) {
    let value_str = value.as_str().expect("timestamp field should be a string");
    let parsed = DateTime::<FixedOffset>::parse_from_rfc3339(value_str);
    assert!(parsed.is_ok(), "{field_name} should be a valid RFC3339 timestamp");
}

/// Whether a notification's entity type is `expected`, the run-scoped type the
/// registration was created with, in either its compact form or the IRI the
/// broker expands it to (which ends in the same run-unique term).
fn matches_entity_type(entity_type: &str, expected: &str) -> bool {
    entity_type == expected || entity_type.ends_with(expected)
}

#[derive(Clone)]
struct CapturedNotification {
    body: Value,
}

struct NotificationReceiver {
    server: NotificationServer,
    endpoint_url: String,
}

impl NotificationReceiver {
    async fn start() -> NotificationReceiver {
        let configuration = MockServerConfig::from_environment().expect("valid notification server configuration");
        let server = NotificationServer::start(configuration).await.expect("notification server starts");
        let endpoint_url = server.endpoint_url().expect("notification endpoint resolves");
        NotificationReceiver { server, endpoint_url }
    }

    async fn wait_for_notification(&self, timeout: Duration) -> CapturedNotification {
        let request = self.server.next_request(timeout).await.expect("notification arrives before timeout");
        CapturedNotification {
            body: request.json().expect("notification body is valid JSON"),
        }
    }

    /// Wait for the first notification whose `member` equals `value`, discarding
    /// any that arrive before it. Several notifications reach one receiver in an
    /// order the broker does not guarantee, so a test awaiting a specific
    /// `triggerReason` must skip past the ones it is not asserting on rather than
    /// assume the next arrival is the one it wants.
    async fn wait_for_notification_matching(&self, member: &str, value: &str, timeout: Duration) -> CapturedNotification {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let request = self
                .server
                .next_request(remaining)
                .await
                .expect("a matching notification arrives before timeout");
            let body: Value = request.json().expect("notification body is valid JSON");
            if body[member] == Value::String(value.to_owned()) {
                return CapturedNotification { body };
            }
        }
    }

    async fn wait_for_no_notification(&self, duration: Duration) {
        let result = self.server.next_request(duration).await;
        assert!(matches!(result, Err(CallbackServerError::RequestTimeout(_))));
    }
}

#[tokio::test]
async fn csr_notification_time_interval_subscription_sends_periodic_notification() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = scope.id("Registration:047-01-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-01-subscription");
    let subscription = time_interval_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(15)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_subscription_creation_and_registration_update_emit_trigger_reasons() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = scope.id("Registration:047-02-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-02-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let first_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(first_notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(first_notification.body["triggerReason"], "newlyMatching");

    let update_fragment = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &update_fragment).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    // Drain past the earlier `newlyMatching` to the update's own notification:
    // both reach this receiver and their arrival order is not guaranteed.
    let second_notification = receiver
        .wait_for_notification_matching("triggerReason", "updated", Duration::from_secs(5))
        .await;
    pretty_assertions::assert_eq!(second_notification.body["subscriptionId"], subscription_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_includes_only_matching_registration_information() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-03-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-03-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = notification.body["data"].as_array().expect("data should be an array");
    assert!(!data.is_empty(), "data should not be empty");
    let information = data[0]["information"].as_array().expect("information should be an array");
    pretty_assertions::assert_eq!(information.len(), 1, "only matching Building information should be included");

    let building_type = scope.entity_type("Building");
    for info_item in information {
        let entities = info_item["entities"].as_array().expect("entities should be an array");
        for entity in entities {
            let entity_type = entity["type"].as_str().expect("entity type should be a string");
            assert!(
                matches_entity_type(entity_type, &building_type),
                "unexpected entity type in notification: {entity_type}"
            );
        }
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_has_required_csource_notification_structure() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-04-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-04-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    assert_iso_timestamp(&notification.body["notifiedAt"], "notifiedAt");
    assert!(notification.body["triggerReason"].is_string(), "triggerReason should be present");
    let data = notification.body["data"].as_array().expect("data should be an array");
    assert!(!data.is_empty(), "data should not be empty");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_success_updates_metadata_status_ok() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-05-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-05-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    sleep(Duration::from_millis(500)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["timesSent"], 1);
    pretty_assertions::assert_eq!(body["notification"]["status"], "ok");
    assert_iso_timestamp(&body["notification"]["lastNotification"], "lastNotification");
    assert_iso_timestamp(&body["notification"]["lastSuccess"], "lastSuccess");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_failed_delivery_updates_metadata_status_failed() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();

    let subscription_id = scope.id("Subscription:047-06-subscription");
    let subscription = unreachable_endpoint_subscription_payload(&scope, &subscription_id);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-06-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    sleep(Duration::from_secs(3)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["status"], "failed");
    assert_iso_timestamp(&body["notification"]["lastFailure"], "lastFailure");
    assert_iso_timestamp(&body["notification"]["lastNotification"], "lastNotification");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_paused_subscription_does_not_emit_notifications() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-07-paused-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let pause_fragment = subscription_update_is_active_false_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &pause_fragment).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    let registration_id = scope.id("Registration:047-07-paused-registration");
    let registration = base_registration_for_type(&registration_id, &scope.entity_type("Building"));
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    receiver.wait_for_no_notification(Duration::from_secs(5)).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_expired_subscription_does_not_emit_notifications() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-07-expired-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let expire_fragment = subscription_update_expires_at_past_fragment();
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &expire_fragment).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    // The subscription's expiry is applied by the broker's sweep, not
    // synchronously with the PATCH, so wait until it reports the subscription
    // expired before creating the registration below -- otherwise the registration
    // would match a still-active subscription and notify. The paused case above
    // needs no such wait because `isActive` is honoured synchronously.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (_, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
        if body["status"] == "expired" {
            break;
        }
        assert!(Instant::now() < deadline, "the subscription did not become expired in time");
        sleep(Duration::from_millis(200)).await;
    }

    let registration_id = scope.id("Registration:047-07-expired-registration");
    let registration = base_registration_for_type(&registration_id, &scope.entity_type("Building"));
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    receiver.wait_for_no_notification(Duration::from_secs(5)).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_matching_registration_update_emits_updated() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = scope.id("Registration:047-08-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-08-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let first_notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(first_notification.body["triggerReason"], "newlyMatching");

    let registration_patch = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &registration_patch).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    // Drain past the earlier `newlyMatching` to the update's own notification:
    // both reach this receiver and their arrival order is not guaranteed.
    let second_notification = receiver
        .wait_for_notification_matching("triggerReason", "updated", Duration::from_secs(5))
        .await;
    pretty_assertions::assert_eq!(second_notification.body["subscriptionId"], subscription_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_non_matching_registration_update_emits_no_longer_matching() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = scope.id("Registration:047-09-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-09-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let patch = registration_information_new_type_fragment(&scope);
    let (update_status, _) = update_registration(&http_client, &registration_id, &patch).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    // Drain past the earlier `newlyMatching` to the removal's own notification.
    let notification = receiver
        .wait_for_notification_matching("triggerReason", "noLongerMatching", Duration::from_secs(5))
        .await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_temporal_observed_at_matches_observation_interval() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-10-subscription");
    let subscription = temporal_query_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url, "observedAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-10-registration");
    let registration = observation_interval_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_temporal_created_at_matches_management_interval() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-11-created-subscription");
    let subscription = temporal_query_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url, "createdAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-11-created-registration");
    let registration = management_interval_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_temporal_modified_at_matches_management_interval() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-11-modified-subscription");
    let subscription = temporal_query_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url, "modifiedAt");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-11-modified-registration");
    let registration = management_interval_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_watched_attributes_filter_matches_registration() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-12-subscription");
    let subscription = watched_attributes_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-12-registration");
    let registration = detailed_building_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_without_watched_attributes_matches_any_information() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-13-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-13-registration");
    let registration = detailed_building_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_geoq_with_geoproperty_matches_location() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-14-subscription");
    let subscription = geoq_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url, true);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-14-registration");
    let registration = location_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_geoq_without_geoproperty_defaults_to_location() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:047-15-subscription");
    let subscription = geoq_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url, false);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let registration_id = scope.id("Registration:047-15-registration");
    let registration = location_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_subscription_update_to_vehicle_matches_vehicle_registration_only() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = scope.id("Registration:047-16-vehicle-registration");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, &scope.entity_type("Vehicle"));
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    pretty_assertions::assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = scope.id("Registration:047-16-bus-registration");
    let bus_registration = base_registration_for_type(&bus_registration_id, &scope.entity_type("Bus"));
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    pretty_assertions::assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-16-vehicle-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_vehicle_entities_fragment(&scope);
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = notification.body["data"].as_array().expect("data should be an array");
    pretty_assertions::assert_eq!(data.len(), 1);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_subscription_update_to_bus_matches_bus_registration_only() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = scope.id("Registration:047-16-vehicle-registration-2");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, &scope.entity_type("Vehicle"));
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    pretty_assertions::assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = scope.id("Registration:047-16-bus-registration-2");
    let bus_registration = base_registration_for_type(&bus_registration_id, &scope.entity_type("Bus"));
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    pretty_assertions::assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-16-bus-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_bus_entities_fragment(&scope);
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = notification.body["data"].as_array().expect("data should be an array");
    pretty_assertions::assert_eq!(data.len(), 1);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_subscription_update_to_vehicle_and_bus_matches_both_registrations() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let vehicle_registration_id = scope.id("Registration:047-16-vehicle-registration-3");
    let vehicle_registration = base_registration_for_type(&vehicle_registration_id, &scope.entity_type("Vehicle"));
    let (vehicle_registration_status, _) = create_registration(&http_client, &vehicle_registration).await;
    pretty_assertions::assert_eq!(vehicle_registration_status, StatusCode::CREATED);

    let bus_registration_id = scope.id("Registration:047-16-bus-registration-3");
    let bus_registration = base_registration_for_type(&bus_registration_id, &scope.entity_type("Bus"));
    let (bus_registration_status, _) = create_registration(&http_client, &bus_registration).await;
    pretty_assertions::assert_eq!(bus_registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:047-16-vehicle-and-bus-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let patch = subscription_update_vehicle_and_bus_entities_fragment(&scope);
    let (update_status, _) = update_csource_subscription(&http_client, &subscription_id, &patch).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");

    let data = notification.body["data"].as_array().expect("data should be an array");
    pretty_assertions::assert_eq!(data.len(), 2);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_metadata_times_sent_increments_after_multiple_successes() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let registration_id = scope.id("Registration:extra-metadata-registration");
    let registration = building_and_bus_registration_payload(&scope, &registration_id);
    let (registration_status, _) = create_registration(&http_client, &registration).await;
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:extra-metadata-subscription");
    let subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    let update_fragment = registration_endpoint_update_fragment();
    let (update_status, _) = update_registration(&http_client, &registration_id, &update_fragment).await;
    pretty_assertions::assert_eq!(update_status, StatusCode::NO_CONTENT);

    let _ = receiver.wait_for_notification(Duration::from_secs(5)).await;

    sleep(Duration::from_millis(500)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, Some("sysAttrs")).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    let times_sent = body["notification"]["timesSent"].as_i64().expect("timesSent should be an integer");
    assert!(times_sent >= 2, "timesSent should be >= 2 after two successful deliveries, got {times_sent}");
    assert_iso_timestamp(&body["notification"]["lastSuccess"], "lastSuccess");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_notification_expires_at_in_future_results_in_active_status_before_deadline() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();

    let subscription_id = scope.id("Subscription:extra-expiration-status");
    let endpoint = "http://127.0.0.1:8085/notify";
    let mut subscription = base_subscription_payload(&scope, &subscription_id, endpoint);
    let future_expiration = (Utc::now() + ChronoDuration::seconds(20)).to_rfc3339_opts(SecondsFormat::Secs, true);
    subscription["expiresAt"] = Value::String(future_expiration);

    let (create_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: a subscription's `csf` targets the Context Source Properties of
/// a registration (clause 5.11.2.4), and a trailing path addresses the raw JSON
/// a registrant wrote by its literal keys -- so `coverage[region]` matches the
/// `region` member, and the filter reads back as it was written.
#[tokio::test]
async fn csr_notification_csf_reads_a_compound_context_source_property_by_its_written_key() {
    let scope = TestEnvironment::from_suite("csr_notification").await.expect("isolated test environment");
    let http_client = client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:r67-csf");
    let mut subscription = base_subscription_payload(&scope, &subscription_id, &receiver.endpoint_url);
    subscription["csf"] = json!("coverage[region]==\"north\"");
    let (subscription_status, _) = create_csource_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(subscription_status, StatusCode::CREATED);
    let (retrieved_status, retrieved) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieved_status, StatusCode::OK);
    pretty_assertions::assert_eq!(retrieved["csf"], json!("coverage[region]==\"north\""));

    let building_type = scope.entity_type("Building");
    let south = scope.id("Registration:r67-south");
    let mut south_registration = base_registration_for_type(&south, &building_type);
    south_registration["coverage"] = json!({"type": "Property", "value": {"region": "south"}});
    let (south_status, _) = create_registration(&http_client, &south_registration).await;
    pretty_assertions::assert_eq!(south_status, StatusCode::CREATED);
    receiver.wait_for_no_notification(Duration::from_secs(2)).await;

    let north = scope.id("Registration:r67-north");
    let mut north_registration = base_registration_for_type(&north, &building_type);
    north_registration["coverage"] = json!({"type": "Property", "value": {"region": "north"}});
    let (north_status, _) = create_registration(&http_client, &north_registration).await;
    pretty_assertions::assert_eq!(north_status, StatusCode::CREATED);
    let notification = receiver.wait_for_notification(Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["triggerReason"], "newlyMatching");
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], json!(north));
    scope.finish().await.expect("cleanup failed");
}
