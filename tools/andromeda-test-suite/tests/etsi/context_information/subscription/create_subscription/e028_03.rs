//! A create-subscription request whose subscription id is not an IRI, or is empty, is refused with 400 (clause 5.8.1).

use crate::{entity_operations::ld_json_body_client, subscription_payloads::building_subscription, unchecked_requests::refused};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, SubscriptionId},
};

/// Pins: Check that one cannot create a subscription with an invalid/empty id
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_03_01_invalid_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Simplified)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("id", "invalidId");
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    refused(&client, request, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create a subscription with an invalid/empty id
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_03_02_empty_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Simplified)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_text("id", "");
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    refused(&client, request, ErrorType::BadRequestData).await
}
