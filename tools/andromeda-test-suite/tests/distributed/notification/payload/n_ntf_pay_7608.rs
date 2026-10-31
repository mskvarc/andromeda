//! The simplified format notifies bare values.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, simplified_matcher::has_value};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a creation on broker A to a subscription created on broker A with format simplified
/// carries speed as the bare value 20
/// Clause: 5.8.6, 4.5.4, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7608_the_simplified_format_notifies_bare_values() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type)
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.simplified()?;
    expect_that!(notification.data().first(), some(has_value("speed", 20)));
    Ok(())
}
