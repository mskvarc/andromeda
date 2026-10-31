//! A notified registration is compacted.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    entity_registration::entity_registration,
    namespace::Namespace,
    notification_sink::NotificationSink,
    spelling_matcher::alone,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output, Representation},
    registration::{CSourceRegistration, EntityInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The notification a Context Source Registration Subscription receives about a matching registration carries
/// the registration with its entity type compacted to the term it was written with
/// Clause: 5.11.7, 5.3.2, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_ntf_9005_a_notified_registration_is_compacted() -> Result<()> {
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
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&registration_id)));
    expect_that!(
        notification
            .data()
            .first()
            .and_then(|registration| registration.information().first())
            .and_then(|information| information.entities().first())
            .map(EntityInfo::entity_type),
        some(alone(&building_type))
    );
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
