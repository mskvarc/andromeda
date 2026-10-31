//! A create-subscription request whose throttling is combined with a time interval, or is negative, is refused with 400
//! `BadRequestData` (clause 5.8.1).

use crate::unchecked_requests::refused;
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, RawJson},
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check that one cannot create a subscription with invalid throttling
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_05_01_throttling_and_time_interval_conjunction() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "randomUUID").parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).build()?;
    let subscription = Subscription::builder()
        .id(id)
        .time_interval(10)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let throttling: RawJson = "30".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("throttling", &throttling);
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document_as_is(&document);
    refused(&broker, request, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create a subscription with invalid throttling
/// Clause: 5.8.1
#[gtest]
#[tokio::test]
async fn e028_05_02_negative_throttling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", "randomUUID").parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).build()?;
    let subscription = Subscription::builder()
        .id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let throttling: RawJson = "-30".parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("throttling", &throttling);
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document_as_is(&document);
    refused(&broker, request, ErrorType::BadRequestData).await
}
