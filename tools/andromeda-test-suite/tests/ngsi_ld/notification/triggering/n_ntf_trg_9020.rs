//! A percent sequence in a body q matches a literal percent value.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose q is name=="Eiffel%20Tower" is notified of a Building created with the name
/// Eiffel%20Tower
/// Clause: 4.9, 5.2.12, 5.8.6
/// Recorded: 4.9 percent-encodes a filter only where it is part of the HTTP binding's URI; a q in a Subscription
/// body is the raw query-language string, whose `quotedStr` is an RFC 8259 JSON String with no percent-decoding
/// step
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9020_a_percent_sequence_in_a_body_q_matches_a_literal_percent_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_literal_id = namespace.subscription_id("literal")?;
    let building_type = namespace.entity_type("Building")?;
    let building_literal_id = namespace.urn("Building", "literal")?;
    let subscription_literal = Subscription::builder()
        .id(subscription_literal_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["airQualityLevel"])
        .q(q::attr("name").eq("Eiffel%20Tower").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("literal")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_literal).send().await?;
    let building_literal = Entity::builder()
        .id(building_literal_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel%20Tower")
        .property("airQualityLevel", 4)
        .build()?;
    broker.entities().create(&building_literal).send().await?.done()?;
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Building:{namespace}-literal"))];
    sink.wait_for_at_least_where("literal", &selected, 1).await?;
    let notifications = sink.received("literal").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&building_literal_id)));
    broker.subscriptions().delete(&subscription_literal_id).send().await?;
    Ok(())
}
