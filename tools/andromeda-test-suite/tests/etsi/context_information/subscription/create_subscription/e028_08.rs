//! A create-subscription request whose notification names `pick`, `omit` and `attributes` in combinations the
//! specification does not allow is refused with 400 `BadRequestData` (clauses 4.21 and 5.8.1).

use crate::{subscription_payloads::building_subscription, unchecked_requests::refused};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, SubscriptionId},
};

/// Sends a create-subscription request for a Building subscription whose `notification` member is replaced by the
/// endpoint of the sink followed by `members`, and requires 400 `BadRequestData` with a title.
async fn refused_notification(namespace: &Namespace, members: &str) -> Result<()> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let uri = sink.endpoint(1)?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(uri.as_str()).accept(NgsiLdMediaType::Json).build()?;
    let subscription = building_subscription(id, endpoint, Representation::Simplified)?;
    let notification: RawJson = format!(r#"{{"endpoint":{{"uri":"{uri}","accept":"application/json"}},{members}}}"#).parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("notification", &notification);
    let client = broker.with_context(vocabulary.context())?;
    let _subscription = Cleanup::of(&client).subscription_of(&subscription);
    let request = client.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    refused(&client, request, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create a subscription with with invalid pick / omit / attrs parameters
/// Clause: 5.8.1, 4.21
#[gtest]
#[tokio::test]
async fn e028_08_01_subscription_with_same_entity_member_in_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    refused_notification(&namespace, r#""pick":["id"],"omit":["id"]"#).await
}

/// Pins: Check that one cannot create a subscription with with invalid pick / omit / attrs parameters
/// Clause: 5.8.1, 4.21
#[gtest]
#[tokio::test]
async fn e028_08_02_subscription_with_pick_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    refused_notification(&namespace, r#""pick":["id"],"attributes":["id"]"#).await
}

/// Pins: Check that one cannot create a subscription with with invalid pick / omit / attrs parameters
/// Clause: 5.8.1, 4.21
#[gtest]
#[tokio::test]
async fn e028_08_03_subscription_with_omit_and_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    refused_notification(&namespace, r#""omit":["id"],"attributes":["id"]"#).await
}
