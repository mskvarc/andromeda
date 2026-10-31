//! A before query reaches an earlier management interval.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output, Representation},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Context Source Registration Subscription whose `temporalQ` is before 2021-08-01T22:00 on `createdAt` is
/// notified of a registration whose `managementInterval` runs from August 2020 to 2021-08-01T21:07
/// Clause: 5.11.2.4, 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_css_ntf_9007_a_before_query_reaches_an_earlier_management_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?])
        .management_interval(TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .temporal_q(
            temporal_query::before("2021-08-01T22:00:00Z".parse()?)
                .time_property(TemporalProperty::CreatedAt)
                .build()?,
        )
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
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
