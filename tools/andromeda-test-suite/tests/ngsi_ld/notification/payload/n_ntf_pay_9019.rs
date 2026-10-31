//! A normalized notification keeps a non ascii relationship object verbatim.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A normalized notification carries the object of a Relationship naming an IRI with a non-ASCII character
/// exactly as written
/// Clause: 4.6.2, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9019_a_normalized_notification_keeps_a_non_ascii_relationship_object_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_non_ascii_normalized_id = namespace.urn("Building", "non-ascii-normalized")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_non_ascii_normalized_id = namespace.subscription_id("non-ascii-normalized")?;
    let ngsi_ld_ciudad_id = format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?;
    let building_non_ascii_normalized = Entity::builder()
        .id(building_non_ascii_normalized_id.clone())
        .entity_type(building_type.clone())
        .relationship("locatedAt", format!("urn:ngsi-ld:Ciudad:{namespace}-París").parse::<EntityId>()?)
        .property("category", "office")
        .build()?;
    broker.entities().create(&building_non_ascii_normalized).send().await?.done()?;
    let subscription_non_ascii_normalized = Subscription::builder()
        .id(subscription_non_ascii_normalized_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_non_ascii_normalized_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("non-ascii-normalized")?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .format(Representation::Normalized)
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_non_ascii_normalized).send().await?;
    let fragment = EntityFragment::builder().property("category", "retail").build()?;
    broker
        .entities()
        .append_attributes(&building_non_ascii_normalized_id, &fragment)
        .send()
        .await?
        .done()?;
    let selected = [BodyPattern::Contains("\"retail\"".to_owned())];
    sink.wait_for_at_least_where("non-ascii-normalized", &selected, 1).await?;
    let selected_2 = [BodyPattern::Contains("\"retail\"".to_owned())];
    let notifications = sink.received_where("non-ascii-normalized", &selected_2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_non_ascii_normalized_id),
                has_attribute("locatedAt", any_attribute().object(ngsi_ld_ciudad_id))
            )
        )
    );
    broker.subscriptions().delete(&subscription_non_ascii_normalized_id).send().await?;
    Ok(())
}
