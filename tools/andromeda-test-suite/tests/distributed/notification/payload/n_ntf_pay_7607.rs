//! A json endpoint receives json with a context link.

use andromeda_test_suite::{broker, link_matcher::link_value, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{ContentType, Link, RelationType},
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a creation on broker A to a subscription created on broker A with endpoint accept
/// application/json is delivered as application/json with a Link header naming an @context
/// Clause: 5.8.6, 5.2.15, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7607_a_json_endpoint_receives_json_with_a_context_link() -> Result<()> {
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
    let notifications = sink.received(1).await?;
    let notification_head = notifications.first().or_fail()?.head();
    expect_that!(
        notification_head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
    );
    expect_that!(header_values(notification_head.headers(), &LINK), len(eq(1)));
    expect_that!(notification_head.typed::<Link>().valid(), some(link_value(RelationType::JsonLdContext)));
    Ok(())
}
