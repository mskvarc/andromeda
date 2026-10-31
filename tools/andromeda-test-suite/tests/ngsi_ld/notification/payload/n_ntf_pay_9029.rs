//! A notification omit drops the type it names.

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

/// Pins: A notification of a Subscription with notification.omit `["type", "name"]` carries each Entity with id and
/// category, and neither type nor name
/// Clause: 5.8.6, 4.21
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9029_a_notification_omit_drops_the_type_it_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", "omit-members")?;
    let subscription_id = namespace.subscription_id("omit-members")?;
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
            NotificationParams::builder(Endpoint::builder(sink.endpoint("omit-members")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .omit("type,name".parse::<Projection>()?)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("name", "Le Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("office".to_owned())];
    sink.wait_for_at_least_where("omit-members", &selected, 1).await?;
    let notifications = sink.received_where("omit-members", &selected).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.projected_entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, some(anything()));
    expect_that!(entity, has_attribute("category", any_attribute()));
    expect_that!(entity.entity_type, none());
    expect_that!(entity, lacks_attribute("name"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
