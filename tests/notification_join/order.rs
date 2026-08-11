//! The order a notification delivers its joined entities in (NGSI-LD 5.2.14.1,
//! 6.4.3.2).
//!
//! Clause 6.4.3.2 requires the linked entities to accompany the notified
//! entity without fixing their order. Andromeda fixes it — the traversal's
//! breadth-first, first-encounter order is the delivery order — because a
//! notification whose payload was ordered differently on every trigger would
//! be irreproducible for any receiver that diffs what it gets.

use crate::{append_attrs, create, create_subscription, subscription_with_flat_join, trigger_fragment};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn notification_join_flat_order_is_deterministic() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let country_id = scope.id("Country:ord-C");
    create(
        client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Portugal"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let city_id = scope.id("City:ord-B");
    create(
        client,
        &json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let building_id = scope.id("Building:ord-A");
    create(
        client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let subscription_id = scope.id("Subscription:ord");
    let sub = subscription_with_flat_join(
        &subscription_id,
        &building_id,
        &receiver.endpoint_url().expect("notification endpoint must be valid"),
        Some(2),
    );
    create_subscription(client, &sub).await;

    let expected = vec![building_id.clone(), city_id.clone(), country_id.clone()];
    for trigger in ["Updated once", "Updated twice"] {
        append_attrs(client, &building_id, &trigger_fragment(trigger)).await;

        let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
        let notification_body: Value = notification.json().expect("notification must be valid JSON");
        assert_eq!(notification_body["subscriptionId"], subscription_id);
        let data = notification_body["data"].as_array().expect("data must be an array");
        let ids: Vec<String> = data.iter().filter_map(|entity| entity["id"].as_str().map(str::to_owned)).collect();
        assert_eq!(ids, expected, "the joined payload order must be stable across triggers ({trigger})");
        assert_eq!(
            data[0]["name"]["value"], trigger,
            "the delivered payload's triggering entity must carry the attribute value that fired this notification ({trigger})"
        );
    }

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
