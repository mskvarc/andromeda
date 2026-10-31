//! A percent sequence in a body q is not decoded.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose q is name=="Eiffel%20Tower" is not notified of a Building named Eiffel Tower, and is
/// notified of the Building created after it whose name holds %20 and of the first Building once its name holds %20
/// Clause: 4.9, 5.2.12, 5.8.6
/// Recorded: 4.9 percent-encodes a filter only where it is part of the HTTP binding's URI; a q in a Subscription
/// body is the raw query-language string, whose `quotedStr` is an RFC 8259 JSON String with no percent-decoding
/// step
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9019_a_percent_sequence_in_a_body_q_is_not_decoded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_percent_id = namespace.subscription_id("percent")?;
    let building_type = namespace.entity_type("Building")?;
    let building_spaced_id = namespace.urn("Building", "spaced")?;
    let building_literal_id = namespace.urn("Building", "literal")?;
    let subscription_percent = Subscription::builder()
        .id(subscription_percent_id.clone())
        .entities([building_type.as_str()])
        .watched_attributes(["airQualityLevel"])
        .q(q::attr("name").eq("Eiffel%20Tower").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("percent")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_percent).send().await?;
    let building_spaced = Entity::builder()
        .id(building_spaced_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("airQualityLevel", 4)
        .build()?;
    broker.entities().create(&building_spaced).send().await?.done()?;
    let building_literal = Entity::builder()
        .id(building_literal_id)
        .entity_type(building_type.clone())
        .property("name", "Eiffel%20Tower")
        .property("airQualityLevel", 4)
        .build()?;
    broker.entities().create(&building_literal).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("name", "Eiffel%20Tower")
        .property("airQualityLevel", 5)
        .build()?;
    broker.entities().update_attributes(&building_spaced_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Building:{namespace}-literal"))];
    sink.wait_for_at_least_where("percent", &selected, 1).await?;
    // The renaming is owed the next notification of the first Building; the notifications of one Entity are delivered in order, so one owed for its creation would be in the journal before it.
    let selected_2 = [BodyPattern::Contains(format!("urn:ngsi-ld:Building:{namespace}-spaced"))];
    let notifications = sink.wait_for_at_least_where("percent", &selected_2, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("airQualityLevel", any_attribute().value(5)))
    );
    broker.subscriptions().delete(&subscription_percent_id).send().await?;
    Ok(())
}
