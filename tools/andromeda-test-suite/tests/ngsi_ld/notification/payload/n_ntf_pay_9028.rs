//! A notification pick naming only an attribute drops the id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder, projection::Projection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription with notification.pick `["name"]` carries each Entity reduced to name
/// alone, without id, type or category
/// Clause: 5.8.6, 4.21, Table 5.2.12-1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9028_a_notification_pick_naming_only_an_attribute_drops_the_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "pick-only-attributes")?;
    let subscription_id = namespace.subscription_id("pick-only-attributes")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .property("category", "office")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("pick-only-attributes")?)
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .format(Representation::Normalized)
            .pick("name".parse::<Projection>()?)
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Le Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("Le Tour Eiffel".to_owned())];
    sink.wait_for_at_least_where("pick-only-attributes", &selected, 1).await?;
    let notifications = sink.received_where("pick-only-attributes", &selected).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.projected_entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity, has_attribute("name", any_attribute()));
    expect_that!(entity.id, none());
    expect_that!(entity.entity_type, none());
    expect_that!(entity, lacks_attribute("category"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
