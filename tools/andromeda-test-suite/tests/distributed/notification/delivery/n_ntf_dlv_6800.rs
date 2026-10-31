//! A receiver info pair is delivered as a header of the notification.

use andromeda_test_suite::{broker, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a subscription whose endpoint carries `receiverInfo` x-scope=acme reaches the sink with
/// an x-scope header of acme
/// Clause: 5.8.6, 5.2.22
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_6800_a_receiver_info_pair_is_delivered_as_a_header_of_the_notification() -> Result<()> {
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
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("x-scope", "acme")
                    .build()?,
            )
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
    let fields = [HeaderCondition::Is(HeaderName::from_static("x-scope"), "acme".to_owned())];
    sink.wait_for_at_least_matching(1, &[], &fields, 1).await?;
    Ok(())
}
