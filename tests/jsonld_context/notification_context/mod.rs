//! The `@context` a notification names (study group D).
//!
//! Clauses 5.8.1.4 and 5.8.6 make the notification context "the one specified
//! in the jsonldContext field", initialised from the context of the request
//! that created the subscription, and clause 5.2.3 lists Notifications among
//! what carries the core `@context`. An `application/ld+json` notification
//! names `jsonldContext` under the rendering rule, on the Notification and on
//! every Entity in `data`; one that is itself a core release is the
//! subscriber's explicit choice and named as written. An `application/json`
//! notification carries one `Link` to it (clause 6.3.8).

mod body_context;
mod geojson;
mod hosted_jsonld_context;
mod link;
mod mqtt;

use crate::{broker_endpoints::broker_url, context_api::extract_id, ngsi_requests::create_subscription, unique_ids::unique_id, user_contexts::DEVICE_IRI};
use andromeda_test_mocks::{callback_server::CapturedRequest, notification_server::NotificationServer, server_config::MockServerConfig};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// How long a notification that is owed is waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// A receiver the broker delivers HTTP notifications to.
async fn receiver() -> NotificationServer {
    NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("the notification receiver starts")
}

/// Subscribe `receiver` to every newly created `Device` (named by its full
/// IRI, so the subscription's own `@context` does not decide the match),
/// delivered as `accept`, with the subscription body extended by `extra`.
/// Returns the subscription's identifier.
async fn subscribe(client: &Client, receiver: &NotificationServer, accept: &str, extra: Value) -> String {
    let mut body = json!({
        "type": "Subscription",
        "entities": [{ "type": DEVICE_IRI }],
        "notificationTrigger": ["entityCreated"],
        "notification": { "endpoint": { "uri": receiver.endpoint_url().expect("an endpoint"), "accept": accept } }
    });
    for (member, value) in extra.as_object().expect("an object of extra members") {
        body[member] = value.clone(); // clone: the body owns each extra member
    }
    let (status, location) = create_subscription(client, &body).await;
    assert_eq!(status, StatusCode::CREATED, "the subscription is created");
    extract_id(location.as_deref().expect("a Location header"))
}

/// Create a `Device` named by its full IRI, which every subscription of this
/// module notifies about.
async fn create_notified_device(client: &Client) -> String {
    let id = unique_id("urn:ngsi-ld:Device:notified");
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .json(&json!({ "id": id, "type": DEVICE_IRI, "https://example.org/terms/speed": { "type": "Property", "value": 3 } }))
        .send()
        .await
        .expect("create request failed");
    assert_eq!(response.status(), StatusCode::CREATED);
    id
}

/// The next notification `receiver` is delivered.
async fn next_notification(receiver: &NotificationServer) -> CapturedRequest {
    receiver.next_request(DELIVERY_TIMEOUT).await.expect("a notification arrives")
}
