//! A Subscription past its `expiresAt` sends nothing more.
//!
//! Clause 5.8.1.4: once `expiresAt` is reached the Subscription is expired "so
//! that notifications will no longer be sent", and clause 5.8.6 sends a
//! periodic notification only for a Subscription that is active. The
//! periodic-notification sweep must not select due `timeInterval` Subscriptions
//! by status alone, or an expired one would keep notifying until the expiry
//! sweep caught up with it.
//!
//! Each test is decided by the notifications' own `notifiedAt` rather than by
//! how long the receiver waited: a sentinel Subscription without an expiry
//! must be seen notifying after the other one's `expiresAt`, which proves the
//! sweep ran past that instant, and no notification of the expiring
//! Subscription may carry a `notifiedAt` at or after it.

use andromeda_ngsi_ld_lexicon::time::{date_time::NgsiLdDateTime, rendering::instant_text};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// How far in the future the expiring Subscription's `expiresAt` lies:
/// long enough to create both Subscriptions and see the first periodic
/// notifications before it.
const EXPIRES_IN: Duration = Duration::from_secs(3);

/// How long one wait for the next notification may take: the periodic interval
/// (one second) plus the base stack's one-second sweep cadence, twice over.
const NEXT_NOTIFICATION: Duration = Duration::from_secs(4);

/// How many sentinel notifications after the expiry instant prove the sweep ran
/// past it more than once.
const SENTINEL_NOTIFICATIONS_AFTER_EXPIRY: usize = 2;

/// One captured notification: its Subscription and its `notifiedAt`.
struct Notified {
    /// The notifying Subscription's identifier.
    subscription_id: String,
    /// The instant the broker stamped on the notification.
    notified_at: DateTime<Utc>,
}

async fn receiver() -> NotificationServer {
    NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start")
}

async fn create_vehicle(client: &TestClient, id: &str) {
    let response = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
}

async fn create_subscription(client: &TestClient, body: &Value) {
    let response = client.create_subscription(body).await.expect("create subscription request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
}

/// A Subscription on exactly one Entity, notified to `endpoint`.
fn subscription(id: &str, entity_id: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": "Vehicle"}],
        "notification": {"endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

fn rfc3339(instant: DateTime<Utc>) -> String {
    instant_text(instant)
}

async fn next_notification(receiver: &NotificationServer) -> Notified {
    let captured = receiver
        .next_request(NEXT_NOTIFICATION)
        .await
        .expect("the sentinel Subscription keeps notifying");
    let body: Value = captured.json().expect("a notification is JSON");
    let subscription_id = body["subscriptionId"].as_str().expect("a notification names its Subscription").to_owned();
    let notified_at = body["notifiedAt"]
        .as_str()
        .and_then(|text| text.parse::<NgsiLdDateTime>().ok())
        .expect("a notification carries notifiedAt")
        .get();
    Notified { subscription_id, notified_at }
}

/// Collect notifications until the sentinel has notified
/// [`SENTINEL_NOTIFICATIONS_AFTER_EXPIRY`] times after `expires_at`, returning
/// every notification seen.
async fn notifications_past(receiver: &NotificationServer, sentinel_id: &str, expires_at: DateTime<Utc>) -> Vec<Notified> {
    let mut seen = Vec::new();
    loop {
        let sentinel_after_expiry = seen
            .iter()
            .filter(|notified: &&Notified| notified.subscription_id == sentinel_id && notified.notified_at > expires_at)
            .count();
        if sentinel_after_expiry >= SENTINEL_NOTIFICATIONS_AFTER_EXPIRY {
            return seen;
        }
        seen.push(next_notification(receiver).await);
    }
}

/// Clause 5.8.1.4 with clause 5.8.6: a `timeInterval` Subscription stops
/// notifying the moment its `expiresAt` passes, whether or not the expiry
/// sweep has marked it expired yet.
#[tokio::test]
async fn regression_subscription_expiry_a_periodic_subscription_stops_notifying_at_its_expires_at() {
    let env = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let client = env.client();
    let receiver = receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let entity_id = env.id("Vehicle:periodic-expiry");
    create_vehicle(client, &entity_id).await;

    let expires_at = Utc::now() + EXPIRES_IN;
    let expiring_id = env.id("Subscription:periodic-expiring");
    let mut expiring = subscription(&expiring_id, &entity_id, &endpoint);
    expiring["timeInterval"] = json!(1);
    expiring["expiresAt"] = json!(rfc3339(expires_at));
    create_subscription(client, &expiring).await;

    let sentinel_id = env.id("Subscription:periodic-sentinel");
    let mut sentinel = subscription(&sentinel_id, &entity_id, &endpoint);
    sentinel["timeInterval"] = json!(1);
    create_subscription(client, &sentinel).await;

    let seen = notifications_past(&receiver, &sentinel_id, expires_at).await;
    receiver.shutdown().await.expect("notification receiver shutdown");
    env.finish().await.expect("cleanup failed");

    assert!(
        seen.iter().any(|notified| notified.subscription_id == expiring_id),
        "the expiring Subscription notifies periodically before it expires"
    );
    let late: Vec<String> = seen
        .iter()
        .filter(|notified| notified.subscription_id == expiring_id && notified.notified_at >= expires_at)
        .map(|notified| rfc3339(notified.notified_at))
        .collect();
    assert_eq!(
        late,
        Vec::<String>::new(),
        "no notification of an expired Subscription (expiresAt {})",
        rfc3339(expires_at)
    );
}

/// Clause 5.8.1.4: a change-triggered Subscription past its `expiresAt` is not
/// notified of a later change, while a sentinel on the same Entity is.
#[tokio::test]
async fn regression_subscription_expiry_a_change_after_expires_at_notifies_only_the_unexpired_subscription() {
    let env = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let client = env.client();
    let receiver = receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let entity_id = env.id("Vehicle:change-expiry");
    create_vehicle(client, &entity_id).await;

    let expires_at = Utc::now() + EXPIRES_IN;
    let expiring_id = env.id("Subscription:change-expiring");
    let mut expiring = subscription(&expiring_id, &entity_id, &endpoint);
    expiring["expiresAt"] = json!(rfc3339(expires_at));
    create_subscription(client, &expiring).await;
    let sentinel_id = env.id("Subscription:change-sentinel");
    create_subscription(client, &subscription(&sentinel_id, &entity_id, &endpoint)).await;

    // Wait out the expiry on the broker's own record of it rather than on the
    // test's clock: once the Subscription reads back expired, a change made
    // afterwards is unambiguously after its expiresAt.
    let deadline = Instant::now() + EXPIRES_IN + NEXT_NOTIFICATION;
    loop {
        let status = client
            .retrieve_subscription(&expiring_id, None)
            .await
            .expect("retrieve subscription request")
            .json_value()
            .map_or(Value::Null, |document| document["status"].clone()); // clone: the status outlives the parsed document
        if status == json!("expired") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the Subscription is marked expired within the sweep bound, last status {status}"
        );
        sleep(Duration::from_millis(200)).await;
    }

    for value in [2, 3] {
        let response = client
            .update_entity_attrs(
                &entity_id,
                &json!({"speed": {"type": "Property", "value": value}, "@context": [TEST_SUITE_CONTEXT]}),
            )
            .await
            .expect("update attributes request");
        assert_eq!(response.status, StatusCode::NO_CONTENT, "{:?}", response.body);
    }
    let seen = notifications_past(&receiver, &sentinel_id, expires_at).await;
    receiver.shutdown().await.expect("notification receiver shutdown");
    env.finish().await.expect("cleanup failed");

    let late: Vec<String> = seen
        .iter()
        .filter(|notified| notified.subscription_id == expiring_id && notified.notified_at >= expires_at)
        .map(|notified| rfc3339(notified.notified_at))
        .collect();
    assert_eq!(
        late,
        Vec::<String>::new(),
        "no notification of an expired Subscription (expiresAt {})",
        rfc3339(expires_at)
    );
}
