//! ETSI `047_07`: a Context Source Registration Subscription that is paused or expired sends no csource notification
//! (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        registered_any,
        teardown,
    },
    suite_keywords::{create_registration_subscription, csource_endpoint, ld_json_body_client},
    suite_payloads::{building_registration, building_registration_subscription},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    model::DateTime,
    subscription::{Activation, RegistrationSubscription, SubscriptionFragment, SubscriptionId},
};
use std::time::Duration;

/// Pins: Check that a cSourceNotification shall only be sent if and only if the status of the corresponding
/// subscription is active, neither paused or expired
/// Clause: 5.11.7
/// Corrected: `fix(047_07)`: register a matching context source so the status check can fail (5.12, 5.8.6)
/// Absence: Wait for no notification 5 s
#[gtest]
#[tokio::test]
async fn e047_07_01_paused_subscription() -> Result<()> {
    silenced(SubscriptionFragment::registration_builder().activation(Activation::Paused).build()?).await
}

/// Pins: Check that a cSourceNotification shall only be sent if and only if the status of the corresponding
/// subscription is active, neither paused or expired
/// Clause: 5.11.7
/// Corrected: `fix(047_07)`: register a matching context source so the status check can fail (5.12, 5.8.6)
/// Absence: Wait for no notification 5 s
#[gtest]
#[tokio::test]
async fn e047_07_02_expired_subscription() -> Result<()> {
    silenced(
        SubscriptionFragment::registration_builder()
            .expires_at("2020-08-01T22:07:00Z".parse::<DateTime>()?)
            .build()?,
    )
    .await
}

/// Creates the Building subscription `urn:ngsi-ld:Subscription:<ns>-1`, updates it with `change` in the vocabulary's
/// `Link` context, registers a matching Building source, requires no notification within 5 s, and tears both down.
async fn silenced(change: SubscriptionFragment<RegistrationSubscription>) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let linked = broker.with_context(vocabulary.context())?;
    linked.registration_subscriptions().update(&id, &change).send().await.any_answer();
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = building_registration(&namespace, 1, &endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    expect_that!(sink.count_after(1, Duration::from_secs(5)).await?, eq(0));

    teardown(&broker, registration_id, &id).await
}
