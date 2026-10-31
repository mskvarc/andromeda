//! Times sent counts every delivered csource notification.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    entity_registration::entity_registration,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    registration_update_request::update_without_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Output},
    registration::{CSourceRegistration, RegistrationFragment, TriggerReason},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: After a `newlyMatching` and an updated csource notification are delivered, the Context Source Registration
/// Subscription reads back with `timesSent` at least 2, `lastSuccess` stamped and notification status ok
/// Clause: 5.11.7, 5.2.14.2, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_0002_times_sent_counts_every_delivered_csource_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern("Building")?,
        source.endpoint("1")?.as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let moved = source.endpoint("moved")?;
    let fragment = RegistrationFragment::builder().endpoint(moved.as_str()).build()?;
    let request = update_without_type(&broker, &registration_id, &fragment)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    sink.wait_for_at_least(1, 2).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::Updated));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::endpoint), some(eq(&moved)));
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registration_subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        let params = retrieved.notification().output();
        verify_that!(params.times_sent, some(ge(2)))?;
        verify_that!(params.last_success, some(anything()))?;
        verify_that!(params.status, some(eq(NotificationStatus::Ok)))?;
        Ok(())
    })
    .await?;
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
