//! A periodic csource notification carries every mandatory member.

use andromeda_test_suite::{broker, config::SuiteConfig, entity_registration::entity_registration, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Representation},
    registration::TriggerReason,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The notification a Context Source Registration Subscription with `timeInterval` 600 receives on creation
/// carries id, type `ContextSourceNotification`, data, `notifiedAt`, `subscriptionId` and `triggerReason`
/// `newlyMatching`
/// Clause: Table 5.3.2-1, 5.3.3, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_ntf_9004_a_periodic_csource_notification_carries_every_mandatory_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern_ending(".+", ".*")?,
        config.unreachable_origin().as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .time_interval(600)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    let notification = received.registrations()?;
    expect_that!(notification.id().to_string(), not(eq("")));
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
