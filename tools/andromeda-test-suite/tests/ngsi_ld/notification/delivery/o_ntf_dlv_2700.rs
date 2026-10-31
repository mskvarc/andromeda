//! A receiver info link pair joins the context link in one line.

use andromeda_test_suite::{broker, header_condition::HeaderCondition, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: an HTTP notification whose subscription carries a `receiverInfo` pair keyed Link arrives with one Link
/// field line holding the @context link and the `receiverInfo` link-value
/// Clause: 6.3.6, 5.2.15
/// Recorded: Andromeda delivers the Link field on one line; RFC 9110 clause 5.3 makes one combined line and several
/// lines equivalent, and the test pins the single line. The specification does not say how a `receiverInfo` pair that
/// names Link is delivered, so a refusal at creation is equally acceptable to the broker
#[gtest]
#[tokio::test]
async fn o_ntf_dlv_2700_a_receiver_info_link_pair_joins_the_context_link_in_one_line() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .receiver_info("Link", format!("<https://example.org/{namespace}>; rel=\"describedby\""))
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("name", "Hall")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let fields = [HeaderCondition::exactly(HeaderName::from_static("link"), ".*json-ld#context.*describedby.*")?];
    let notifications = sink.received_matching(1, &[], &fields).await?;
    expect_that!(notifications, len(ge(1)));
    Ok(())
}
