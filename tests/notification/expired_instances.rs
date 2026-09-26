//! Clause 4.22: an attribute instance "shall become invalid" at its
//! `expiresAt`, while its deletion may lag. A subscription therefore cannot
//! match through an expired instance the sweep has yet to delete, and a
//! notification does not carry one (clause 5.8.6 notifies the entity as it is).

use crate::{
    helpers::{base_subscription, client, create_entity, create_subscription, set_endpoint_uri, update_entity_attrs, with_watched_attributes},
    notification_adapter::NotificationAdapter as NotificationReceiver,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// An `expiresAt` a moment in the past: the instance is invalid from the start,
/// whether or not the expiry sweep has deleted it by the time the update lands.
const JUST_PASSED: &str = "2000-01-01T00:00:00Z";

/// A Building whose only `location` lies in Berlin and has already expired.
fn building_with_expired_location(entity_id: &str) -> Value {
    json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Hall"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.52]},
            "expiresAt": JUST_PASSED
        },
        "@context": [CORE_CONTEXT]
    })
}

/// A Building subscription watching `name`, optionally restricted to Berlin.
fn watching_name(subscription_id: &str, receiver: &NotificationReceiver, within_berlin: bool) -> Value {
    let mut subscription = with_watched_attributes(base_subscription(subscription_id), &["name"]);
    if within_berlin {
        subscription["geoQ"] = json!({
            "georel": "within",
            "geometry": "Polygon",
            "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
        });
    }
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    subscription
}

#[tokio::test]
async fn notification_geoq_matching_only_an_expired_instance_does_not_fire() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;

    let entity_id = scope.id("Building:expired-location");
    let (status, _, _) = create_entity(&http_client, &building_with_expired_location(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let sentinel_id = scope.id("Subscription:expired-location-sentinel");
    let geo_id = scope.id("Subscription:expired-location-geo");
    for subscription in [watching_name(&sentinel_id, &receiver, false), watching_name(&geo_id, &receiver, true)] {
        let (status, _, body) = create_subscription(&http_client, &scope, &subscription).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    receiver.clear_notifications().await;

    // An update of an attribute unrelated to the expired `location`.
    let fragment = json!({"name": {"type": "Property", "value": "Renamed hall"}, "@context": [CORE_CONTEXT]});
    let (status, _) = update_entity_attrs(&http_client, &entity_id, &fragment).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    // The sentinel is evaluated against the same change, so its arrival shows
    // the change was processed; the geo subscription's silence is then its
    // answer rather than a delay.
    let notification = receiver.wait_for_subscription_notification(&sentinel_id, Duration::from_secs(5)).await;
    let notified = &notification.body["data"][0];
    pretty_assertions::assert_eq!(notified["id"], json!(entity_id));
    assert!(
        notified.get("location").is_none(),
        "an expired instance is not part of the notified entity: {notified}"
    );
    receiver.wait_for_no_subscription_notification(&geo_id, Duration::from_secs(1)).await;

    receiver.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
