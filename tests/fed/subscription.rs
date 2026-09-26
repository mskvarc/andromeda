//! Cross-domain subscription forwarding acceptance cases.

use crate::{
    fixtures::{FedRole, assert_status, body, broker, entity, registration, topology},
    notification_adapter::NotificationAdapter,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::{task::yield_now, time::timeout};

fn subscription(id: &str, endpoint: &str, entity_type: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notification": {"endpoint": {"uri": endpoint, "accept": "application/ld+json"}},
        "localOnly": false,
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

// Cross-domain notification delivery competes with heavy broker load, so these
// subscription tests share the stress serial key: they never run alongside a
// stress test (or each other), keeping their notification waits within budget,
// while the lighter query/count tests run in parallel around them.
#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_subscription_two_domain_lifecycle_delivers_original_id() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let receiver = NotificationAdapter::start().await;
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let berlin = broker(&topology, FedRole::Berlin);
    receiver.clear_notifications().await;
    for (domain, name) in [(madrid, "madrid"), (berlin, "berlin")] {
        assert_status(
            &hub.client()
                .create_registration(&registration(
                    topology.tenant(),
                    &topology.id(&format!("registration:subscription:{name}")),
                    domain.peer_url(),
                    &btype,
                    None,
                    "inclusive",
                    Some(&["queryEntity", "createSubscription", "updateSubscription", "deleteSubscription"]),
                ))
                .await
                .expect("subscription registration"),
            201,
        );
    }
    let subscription_id = topology.id("subscription:two-domain");
    assert_status(
        &hub.client()
            .create_subscription(&subscription(&subscription_id, &receiver.endpoint_url(), &btype))
            .await
            .expect("subscription create"),
        201,
    );
    let id = topology.id("Building:subscription-madrid");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity(&id, &btype, 30))
            .with_ld_json()
            .send()
            .await
            .expect("watched entity"),
        201,
    );
    let notification = receiver.wait_for_notification(Duration::from_secs(20)).await;
    assert_eq!(notification.body["subscriptionId"].as_str(), Some(subscription_id.as_str()));
    assert!(notification.headers.contains_key("content-type"));
    assert_status(
        &hub.client()
            .update_subscription(&subscription_id, &json!({"entities": [{"type": btype, "idPattern": "^never$"}]}), None)
            .await
            .expect("subscription update"),
        204,
    );
    receiver
        .wait_for_no_notification_for_subscription(&subscription_id, Duration::from_millis(1))
        .await;
    assert_status(&hub.client().delete_subscription(&subscription_id).await.expect("subscription delete"), 204);
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_subscription_discovers_domain_registered_after_creation() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let receiver = NotificationAdapter::start().await;
    let hub = broker(&topology, FedRole::Hub);
    let madrid = broker(&topology, FedRole::Madrid);
    let subscription_id = topology.id("subscription:late-registration");
    assert_status(
        &hub.client()
            .create_subscription(&subscription(&subscription_id, &receiver.endpoint_url(), &btype))
            .await
            .expect("early subscription"),
        201,
    );
    receiver.clear_notifications().await;
    assert_status(
        &hub.client()
            .create_registration(&registration(
                topology.tenant(),
                &topology.id("registration:late-madrid"),
                madrid.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity", "createSubscription", "updateSubscription", "deleteSubscription"]),
            ))
            .await
            .expect("late registration"),
        201,
    );
    timeout(Duration::from_secs(20), async {
        loop {
            let response = madrid.client().query_subscriptions("", None).await.expect("query late forwarded subscriptions");
            if response.status.is_success() && body(&response).as_array().is_some_and(|items| !items.is_empty()) {
                break;
            }
            yield_now().await;
        }
    })
    .await
    .expect("late registration creates the remote subscription within the bounded wait");
    let id = topology.id("Building:late-subscription");
    assert_status(
        &madrid
            .client()
            .post_entity(&entity(&id, &btype, 31))
            .with_ld_json()
            .send()
            .await
            .expect("late watched entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(20)).await;
    assert_eq!(notification.body["subscriptionId"].as_str(), Some(subscription_id.as_str()));
    receiver.wait_for_no_subscription_notification(&subscription_id, Duration::from_millis(1)).await;
    assert_status(&hub.client().delete_subscription(&subscription_id).await.expect("subscription delete"), 204);
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

#[serial_test::serial(fed_stress)]
#[tokio::test]
async fn fed_nested_subscription_delivers_from_subdomain_and_cleans_up() {
    let topology = topology().await;
    let btype = topology.id("Building");
    let receiver = NotificationAdapter::start().await;
    let hub = broker(&topology, FedRole::Hub);
    let nordic = broker(&topology, FedRole::Nordic);
    let north = broker(&topology, FedRole::North);
    assert_status(
        &nordic
            .client()
            .create_registration(&registration(
                topology.tenant(),
                &topology.id("registration:sub-north"),
                north.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity", "createSubscription", "updateSubscription", "deleteSubscription"]),
            ))
            .await
            .expect("nested registration"),
        201,
    );
    assert_status(
        &hub.client()
            .create_registration(&registration(
                topology.tenant(),
                &topology.id("registration:sub-nordic"),
                nordic.peer_url(),
                &btype,
                None,
                "inclusive",
                Some(&["queryEntity", "createSubscription", "updateSubscription", "deleteSubscription"]),
            ))
            .await
            .expect("hub registration"),
        201,
    );
    let subscription_id = topology.id("subscription:nested");
    assert_status(
        &hub.client()
            .create_subscription(&subscription(&subscription_id, &receiver.endpoint_url(), &btype))
            .await
            .expect("nested subscription"),
        201,
    );
    let id = topology.id("Building:nested-notification");
    assert_status(
        &north
            .client()
            .post_entity(&entity(&id, &btype, 32))
            .with_ld_json()
            .send()
            .await
            .expect("nested entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(20)).await;
    assert_eq!(notification.body["subscriptionId"].as_str(), Some(subscription_id.as_str()));
    assert_status(
        &hub.client().delete_subscription(&subscription_id).await.expect("nested subscription delete"),
        204,
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
