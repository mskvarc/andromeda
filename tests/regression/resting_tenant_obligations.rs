//! A quiet tenant's paced background work still meets every new obligation.
//!
//! A resident tenant's timer-driven background work runs only when an
//! obligation is due: one read of the tenant's earliest obligation decides for
//! every timer until it expires or the tenant is used again, and a tenant with
//! nothing outstanding rests for up to sixteen sweep cadences. An obligation a
//! request creates -- a periodic notification's first period, an entity's
//! `expiresAt` -- must void that rest as soon as the request's commit is
//! visible, or it waits for the ceiling:
//!
//! - clause 5.8.6: "a Notification shall be sent periodically, when the time
//!   interval ... is reached";
//! - clause 4.22: deletion lags `expiresAt` only by the clean-up cadence.
//!
//! Each test uses a private tenant, so no other test's obligations keep its
//! pacer awake, and lets the tenant idle for several sweep cadences first, so
//! the pacer has read "nothing due" and is resting when the obligation is
//! created. The race between a tick and an in-flight request is pinned
//! deterministically by the pacer's own unit tests; these tests pin the
//! end-to-end guarantee. Every bound is the obligation's own time plus a small
//! multiple of the base stack's one-second sweep cadence, far below the
//! sixteen-cadence ceiling a missed wake would fall back to.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use chrono::{SecondsFormat, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// The base stack's `temporal.expiration_check_interval`, the cadence every
/// paced background timer of a tenant runs on.
const SWEEP_CADENCE: Duration = Duration::from_secs(1);

/// How long the tenant idles before the obligation is created: enough ticks
/// for its pacer to have read "nothing due" and started resting.
const IDLE_BEFORE: Duration = Duration::from_secs(4);

/// The periodic Subscription's interval, in seconds.
const INTERVAL_SECONDS: u64 = 2;

/// Clause 5.8.6 on a resting tenant: a `timeInterval` Subscription created
/// after the tenant went quiet sends its first notification within one
/// interval and a few sweep cadences, not after the pacer's ceiling.
#[tokio::test]
async fn regression_resting_tenant_a_new_periodic_subscription_notifies_within_its_interval() {
    let env = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let client = env.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start");
    let entity_id = env.id("Vehicle:resting-periodic");
    let created = client
        .post_entity(&json!({"id": entity_id, "type": "Vehicle", "speed": {"type": "Property", "value": 1}, "@context": [TEST_SUITE_CONTEXT]}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    sleep(IDLE_BEFORE).await;

    let subscription_id = env.id("Subscription:resting-periodic");
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": entity_id, "type": "Vehicle"}],
            "timeInterval": INTERVAL_SECONDS,
            "notification": {"endpoint": {"uri": receiver.endpoint_url().expect("endpoint URL"), "accept": "application/json"}},
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    let subscribed_at = Instant::now();

    let bound = Duration::from_secs(INTERVAL_SECONDS) + SWEEP_CADENCE * 3;
    let first = receiver.next_request(bound).await;
    let waited = subscribed_at.elapsed();
    receiver.shutdown().await.expect("notification receiver shutdown");
    env.finish().await.expect("cleanup failed");

    let first = first.expect("the first periodic notification arrives within one interval and three sweep cadences");
    let body: serde_json::Value = first.json().expect("a notification is JSON");
    assert_eq!(body["subscriptionId"], json!(subscription_id), "{body}");
    assert!(waited <= bound, "first periodic notification after {waited:?}, bound {bound:?}");
}

/// Clause 4.22 on a resting tenant: an Entity created with a near `expiresAt`
/// after the tenant went quiet is gone within its `expiresAt` plus a few sweep
/// cadences.
#[tokio::test]
async fn regression_resting_tenant_a_new_entity_expiry_is_swept_within_the_cadence() {
    let env = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let client = env.client();
    let warm_id = env.id("Vehicle:resting-warm");
    let warmed = client
        .post_entity(&json!({"id": warm_id, "type": "Vehicle", "@context": [TEST_SUITE_CONTEXT]}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(warmed.status, StatusCode::CREATED, "{:?}", warmed.body);
    sleep(IDLE_BEFORE).await;

    let expires_in = Duration::from_secs(2);
    let expires_at = Utc::now() + expires_in;
    let entity_id = env.id("Vehicle:resting-expiring");
    let created = client
        .post_entity(&json!({
            "id": entity_id,
            "type": "Vehicle",
            "expiresAt": expires_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let deadline = Instant::now() + expires_in + SWEEP_CADENCE * 3;
    let swept = loop {
        let response = client
            .get_entity(&entity_id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("retrieve entity request");
        if response.status == StatusCode::NOT_FOUND {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        sleep(Duration::from_millis(200)).await;
    };
    env.finish().await.expect("cleanup failed");
    assert!(swept, "the expired Entity is deleted within its expiresAt plus three sweep cadences");
}
