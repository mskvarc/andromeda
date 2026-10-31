//! A delivery is counted as a success on both instances.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: After a notification of a creation on broker A is delivered for a subscription created on broker A,
/// Retrieve Subscription on broker A (KEEP) answers `timesSent` at least 1, `lastNotification`, `lastSuccess` and
/// status ok
/// Clause: 5.8.6, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_sta_7600_a_delivery_is_counted_as_a_success_on_both_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    retried(async || {
        let subscription_2 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
        verify_that!(subscription_2.notification().output().times_sent, some(ge(1)))?;
        verify_that!(subscription_2.notification().output().last_notification, some(anything()))?;
        verify_that!(subscription_2.notification().output().last_success, some(anything()))?;
        verify_that!(subscription_2.notification().output().status, some(eq(NotificationStatus::Ok)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
