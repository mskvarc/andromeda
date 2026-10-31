//! A subscription q on a percent encoded object is notified of that spelling alone.

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
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose q equals a percent-encoded Relationship object is not notified of a Room whose object
/// is the decoded spelling, and is notified, with the object exactly as written, once the Room's object becomes the
/// encoded spelling
/// Clause: 4.9, 5.8.6, 4.5.3.2; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9028_a_subscription_q_on_a_percent_encoded_object_is_notified_of_that_spelling_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_encoded_id = namespace.subscription_id("encoded")?;
    let room_type = namespace.entity_type("Room")?;
    let room_id = namespace.urn("Room", 1)?;
    let building_id = namespace.urn("Building", 1)?;
    let ngsi_ld_3_abuilding_id = format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1").parse::<EntityId>()?;
    let subscription_encoded = Subscription::builder()
        .id(subscription_encoded_id.clone())
        .entities([room_type.as_str()])
        .watched_attributes(["locatedIn"])
        .q(q::attr("locatedIn").eq(format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1").as_str()).build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("encoded")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_encoded).send().await?;
    let room = Entity::builder()
        .id(room_id.clone())
        .entity_type(room_type.clone())
        .relationship("locatedIn", building_id)
        .build()?;
    broker.entities().create(&room).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .relationship("locatedIn", format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1").parse::<EntityId>()?)
        .build()?;
    broker.entities().update_attributes(&room_id, &fragment).send().await?.done()?;
    // The notifications of one Entity are delivered in order, so one owed for the creation would be in the journal before the one owed for the change.
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Room:{namespace}-1"))];
    let notifications = sink.wait_for_at_least_where("encoded", &selected, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("locatedIn", any_attribute().object(ngsi_ld_3_abuilding_id)))
    );
    broker.subscriptions().delete(&subscription_encoded_id).send().await?;
    Ok(())
}
