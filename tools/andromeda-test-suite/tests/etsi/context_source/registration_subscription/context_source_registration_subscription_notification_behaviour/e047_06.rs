//! A csource notification that cannot be delivered updates the `notification` member of the subscription (clause
//! 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_delivery_state,
        registered_any,
        teardown,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_payloads::building_registration,
    registration_subscription_operations::{create_registration_subscription, once_delivery_recorded, registration_subscription_json},
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};
use std::time::Duration;
use url::Url;

/// Pins: Check that if a cSourceNotification is not sent successfully, the "notification.timesSent" member shall be
/// incremented by one and the notification.lastFailure" and "notification.lastNotification" members shall be updated
/// with the current timestamp and the status of the context source registration subscription shall be updated to
/// "failed"
/// Clause: 5.11.7
/// Expected: a matching context source is registered, so the subscription sends an initial notification, which is
/// consumed first (clause 5.12)
/// Absence: no notification within 5 s
/// Timing: the Retrieve Context Source Registration Subscription after the absence window is repeated every 250 ms
/// until its `notification` records the failed delivery (`lastNotification` present): clause 5.11.7 records the failure
/// once the broker gives up on the endpoint, which the absence window does not bound
#[gtest]
#[tokio::test]
async fn e047_06_01_if_a_csource_notification_is_not_successfully_sent_the_notification_member_shall_be_updated() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let unreachable = Url::parse("http://unreachable.endpoint/notify")?;
    let subscription = building_registration_subscription(Some(id.clone()), &unreachable, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &unreachable).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = building_registration(&namespace, 1, &endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    expect_that!(sink.count_after(1, Duration::from_secs(5)).await?, eq(0));

    let reader = broker.with_accept(NgsiLdMediaType::Json);
    let body = once_delivery_recorded(|| registration_subscription_json(&reader, &id)).await?;
    expect_delivery_state(body, "failed", ["lastNotification", "lastFailure"]);

    teardown(&broker, registration_id, &id).await
}
