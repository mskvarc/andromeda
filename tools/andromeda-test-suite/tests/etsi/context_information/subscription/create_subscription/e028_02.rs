//! A create-subscription request whose body is not JSON, or is an empty object, is refused with 400 (clause 5.8.1).

use crate::{
    entity_operations::ld_json_body_client,
    subscription_payloads::building_subscription,
    unchecked_requests::{refused, with_doubled_separator},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, Representation},
    subscription::{Endpoint, SubscriptionId},
};

/// Pins: Check that one cannot create a subscription with an invalid request
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_02_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "randomUUID").parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Simplified)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?;
    refused(&client, with_doubled_separator(request), ErrorType::InvalidRequest).await
}

/// Pins: Check that one cannot create a subscription with an invalid request
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_02_02_empty_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "randomUUID").parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Simplified)?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let request = broker
        .unchecked(&CreateSubscription { subscription: &subscription })?
        .document_as_is(&UncheckedDocument::empty());
    refused(&broker, request, ErrorType::BadRequestData).await
}
