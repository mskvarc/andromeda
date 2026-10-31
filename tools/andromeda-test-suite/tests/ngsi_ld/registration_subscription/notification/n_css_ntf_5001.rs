//! Updating a subscription notifies all matching registrations once.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_registration::entity_registration,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    registration::{CSourceNotification, CSourceRegistration, RegistrationFragment, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Every notification the sink holds at endpoint 1, read as a context source notification.
async fn read_notifications(sink: &NotificationSink) -> Result<Vec<CSourceNotification>> {
    sink.received(1).await?.iter().map(|received| Ok(received.registrations()?)).collect()
}

/// Pins: after a Context Source Registration Subscription is updated to select two registrations created while it
/// did not select them, exactly one `newlyMatching` notification carrying both is sent, and none carrying one
/// registration alone; a later change to one registration follows as the next notification
/// Clause: 5.11.2.4, 5.11.3.4, 5.11.7, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_5001_updating_a_subscription_notifies_all_matching_registrations_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let registration_1_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration_1 = entity_registration(
        &registration_1_id,
        &vehicle_type,
        namespace.id_pattern("Vehicle")?,
        source.endpoint("1")?.as_str(),
    )?;
    broker.registrations().register(&registration_1).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_1_cleanup = Cleanup::of(&broker).registration(&registration_1_id);
    let registration_2 = entity_registration(&registration_2_id, &bus_type, namespace.id_pattern("Bus")?, source.endpoint("2")?.as_str())?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let widened = SubscriptionFragment::registration_builder()
        .entities([vehicle_type.as_str(), bus_type.as_str()])
        .build()?;
    broker.registration_subscriptions().update(&subscription_id, &widened).send().await?;
    retried(async || {
        let notifications = read_notifications(&sink).await?;
        verify_that!(
            notifications
                .iter()
                .filter(|notification| notification.trigger_reason() == TriggerReason::NewlyMatching)
                .count(),
            ge(1)
        )?;
        Ok(())
    })
    .await?;
    let changed = RegistrationFragment::builder().endpoint(source.endpoint("1-changed")?.as_str()).build()?;
    broker.registrations().update(&registration_1_id, &changed).send().await?;
    retried(async || {
        let notifications = read_notifications(&sink).await?;
        verify_that!(
            notifications
                .iter()
                .filter(|notification| notification.trigger_reason() == TriggerReason::Updated)
                .count(),
            ge(1)
        )?;
        Ok(())
    })
    .await?;
    let notifications = read_notifications(&sink).await?;
    let newly_matching = notifications
        .iter()
        .filter(|notification| notification.trigger_reason() == TriggerReason::NewlyMatching)
        .collect::<Vec<_>>();
    expect_that!(newly_matching, len(eq(1)));
    let first = newly_matching.first().or_fail()?;
    expect_that!(first.subscription_id(), eq(&subscription_id));
    expect_that!(first.data(), len(eq(2)));
    expect_that!(
        first.data().iter().map(CSourceRegistration::<Output>::id).cloned().collect::<Vec<_>>(),
        unordered_elements_are![eq(&registration_1_id), eq(&registration_2_id)]
    );
    let updated = notifications
        .iter()
        .filter(|notification| notification.trigger_reason() == TriggerReason::Updated)
        .collect::<Vec<_>>();
    expect_that!(updated, len(eq(1)));
    let second = updated.first().or_fail()?;
    expect_that!(second.data(), len(eq(1)));
    expect_that!(second.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_1_id)));
    Ok(())
}
