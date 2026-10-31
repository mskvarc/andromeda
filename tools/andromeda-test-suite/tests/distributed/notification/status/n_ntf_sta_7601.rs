//! A delivery answered 500 is counted as a failure.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, source_answer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: After a notification of a creation on broker A is answered 500 by the endpoint of a subscription created
/// on broker A, Retrieve Subscription on broker A (KEEP) answers `lastFailure` and status failed
/// Clause: 5.8.6, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_sta_7601_a_delivery_answered_500_is_counted_as_a_failure() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    sink.answer(1, source_answer::status(StatusCode::INTERNAL_SERVER_ERROR)).await?;
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
        verify_that!(subscription_2.notification().output().status, some(eq(NotificationStatus::Failed)))?;
        verify_that!(subscription_2.notification().output().last_failure, some(anything()))?;
        Ok(())
    })
    .await?;
    Ok(())
}
