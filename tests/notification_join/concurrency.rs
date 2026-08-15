//! The triggering entity's just-appended value survives concurrent load
//! (NGSI-LD 5.2.14.1, 6.4.3.2).
//!
//! `order::notification_join_flat_order_is_deterministic` asserts the same
//! invariant once, and fails only under full-suite concurrent scheduling: the
//! delivered `data[0]["name"]["value"]` arrives `null` instead of the value the
//! append just wrote. This case manufactures that concurrency in isolation, so
//! the reproducer does not depend on cross-suite scheduling and, on the first
//! failing run, prints the delivered body rather than only the failed
//! assertion.

use crate::{append_attrs, create, create_subscription, subscription_with_flat_join, trigger_fragment};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use serde_json::{Value, json};
use std::time::Duration;

/// Concurrent appenders driven against the one shared tenant.
const CONCURRENT_APPENDERS: usize = 8;

/// The distinct identifiers one appender task owns for the whole flat-join
/// chain it drives.
struct JoinChain {
    country_id: String,
    city_id: String,
    building_id: String,
    subscription_id: String,
}

/// Run one appender's full failing sequence against its own entities and its
/// own notification receiver, returning the diagnostic to print when the
/// delivered payload does not carry the value the append just wrote.
async fn drive_one_appender(client: TestClient, suffix: usize, chain: JoinChain) -> Result<(), String> {
    let JoinChain {
        country_id,
        city_id,
        building_id,
        subscription_id,
    } = chain;

    create(
        &client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Portugal"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    create(
        &client,
        &json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    create(
        &client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let endpoint_url = receiver.endpoint_url().expect("notification endpoint must be valid");
    let sub = subscription_with_flat_join(&subscription_id, &building_id, &endpoint_url, Some(2));
    create_subscription(&client, &sub).await;

    let fired_value = format!("task-{suffix}-once");
    append_attrs(&client, &building_id, &trigger_fragment(&fired_value)).await;

    let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
    let raw_body = String::from_utf8_lossy(&notification.body).into_owned();
    let body: Value = notification.json().expect("notification must be valid JSON");
    receiver.shutdown().await.expect("notification server shutdown failed");

    let expected_ids = vec![building_id, city_id, country_id];
    let data = body["data"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = data.iter().filter_map(|entity| entity["id"].as_str().map(str::to_owned)).collect();
    let delivered_name = data.first().map(|entity| &entity["name"]["value"]).cloned().unwrap_or(Value::Null);

    if ids == expected_ids && delivered_name == json!(fired_value) {
        return Ok(());
    }

    let delivered_keys = data.first().and_then(Value::as_object).map(|attributes| {
        let mut keys: Vec<&String> = attributes.keys().collect();
        keys.sort();
        keys
    });
    Err(format!(
        "appender {suffix}: delivered payload lost the just-appended value\n  expected name value: {}\n  delivered name value: {delivered_name}\n  expected ids: {expected_ids:?}\n  delivered ids: {ids:?}\n  data[0] keys: {delivered_keys:?}\n  raw body: {raw_body}\n  full body: {body:#}",
        json!(fired_value)
    ))
}

/// Every one of `CONCURRENT_APPENDERS` appenders, run at once against the shared
/// tenant, must have its triggering entity carry the value its own append
/// wrote. A single `null` triggering value fails the run and prints the body
/// that lost it.
#[tokio::test]
async fn notification_join_flat_order_survives_concurrent_appends() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");

    let mut appenders = Vec::with_capacity(CONCURRENT_APPENDERS);
    for suffix in 0..CONCURRENT_APPENDERS {
        let client = scope.client().clone(); // clone: each spawned appender owns a handle to the shared broker endpoint
        let chain = JoinChain {
            country_id: scope.id(&format!("Country:conc-{suffix}")),
            city_id: scope.id(&format!("City:conc-{suffix}")),
            building_id: scope.id(&format!("Building:conc-{suffix}")),
            subscription_id: scope.id(&format!("Subscription:conc-{suffix}")),
        };
        appenders.push(tokio::spawn(drive_one_appender(client, suffix, chain)));
    }

    let mut failures = Vec::new();
    for appender in appenders {
        match appender.await.expect("appender task must not panic") {
            Ok(()) => {}
            Err(diagnostic) => failures.push(diagnostic),
        }
    }

    let cleanup = scope.finish().await;
    assert!(failures.is_empty(), "concurrent appends lost a just-appended value:\n{}", failures.join("\n\n"));
    cleanup.expect("cleanup failed");
}
