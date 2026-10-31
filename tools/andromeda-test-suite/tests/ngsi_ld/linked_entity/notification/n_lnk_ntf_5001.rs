//! Notification omit through a linked entity term leaves the unnamed second hop whole.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    projected_notification::projected_entities,
    subscription_document::create_request,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, OneOrMany, Representation},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription whose notification carries join, `joinLevel=2` and an omit member with a Linked Entity term
/// (`locatedAt{name}`) delivers the Entity with name removed from the Linked Entity that Relationship reaches only:
/// the Entity reached through the next, unnamed Relationship (`isInCountry`) is delivered whole, nested for inline
/// and beside the others for flat
/// Clause: 4.21, 4.5.23, 5.2.14, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_5001_notification_omit_through_a_linked_entity_term_leaves_the_unnamed_second_hop_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let country_id = namespace.urn("Country", 1)?;
    let country_type = namespace.entity_type("Country")?;
    let building_type = namespace.entity_type("Building")?;
    let building_inline_id = namespace.urn("Building", "inline")?;
    let building_flat_id = namespace.urn("Building", "flat")?;
    let subscription_inline_id = namespace.subscription_id("inline")?;
    let subscription_flat_id = namespace.subscription_id("flat")?;
    let entities = [
        Entity::builder()
            .id(city_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .relationship("isInCountry", country_id.clone())
            .build()?,
        Entity::builder()
            .id(country_id.clone())
            .entity_type(country_type.clone())
            .property("name", "France")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let subscription_inline = Subscription::builder()
        .id(subscription_inline_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_id_pattern(namespace.id_pattern_ending("Building", "inline")?)])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("inline")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .join(Join::Inline)
                .join_level(2)
                .build()?,
        )
        .build()?;
    // The model cannot write the notification's omit, so the member is added to the rendered document.
    let request = create_request(&broker, &subscription_inline, &[("omit", r#"["type", "locatedAt{name}"]"#)])?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let subscription_flat = Subscription::builder()
        .id(subscription_flat_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_id_pattern(namespace.id_pattern_ending("Building", "flat")?)])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("flat")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .join(Join::Flat)
                .join_level(2)
                .build()?,
        )
        .build()?;
    // The model cannot write the notification's omit, so the member is added to the rendered document.
    let request_2 = create_request(&broker, &subscription_flat, &[("omit", r#"["type", "locatedAt{name}"]"#)])?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let building_inline = Entity::builder()
        .id(building_inline_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building_inline).send().await?.done()?;
    let building_flat = Entity::builder()
        .id(building_flat_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", city_id.clone())
        .build()?;
    broker.entities().create(&building_flat).send().await?.done()?;
    sink.wait_for_at_least("inline", 1).await?;
    let notifications = sink.received("inline").await?;
    let notification = projected_entities(notifications.first().or_fail()?, Lossless::Normalized)?;
    let data = notification.data().as_slice();
    expect_that!(data, len(eq(1)));
    expect_that!(data, nth(0, has_id(&building_inline_id)));
    expect_that!(data.first().map(|entity| entity.entity_type.is_none()), some(eq(true)));
    let cities = linked_entities(data.first().or_fail()?, &["locatedAt"]);
    expect_that!(cities, nth(0, has_id(&city_id)));
    expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    expect_that!(cities, nth(0, lacks_attribute("name")));
    let countries = linked_entities(data.first().or_fail()?, &["locatedAt", "isInCountry"]);
    expect_that!(countries, nth(0, has_id(&country_id)));
    expect_that!(
        countries.first().map(|country| &country.entity_type),
        some(eq(&Some(OneOrMany::one(country_type.clone()))))
    );
    expect_that!(countries, nth(0, has_attribute("name", any_attribute().value("France"))));
    sink.wait_for_at_least("flat", 1).await?;
    let notifications_2 = sink.received("flat").await?;
    let notification_2 = projected_entities(notifications_2.first().or_fail()?, Lossless::Normalized)?;
    let data_2 = notification_2.data().as_slice();
    expect_that!(data_2, len(eq(3)));
    let building = data_2.iter().find(|entity| entity.id.as_ref() == Some(&building_flat_id)).or_fail()?;
    expect_that!(building, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(building.entity_type, none());
    let city = data_2.iter().find(|entity| entity.id.as_ref() == Some(&city_id)).or_fail()?;
    expect_that!(city.entity_type, some(eq(&OneOrMany::one(city_type.clone()))));
    expect_that!(city, lacks_attribute("name"));
    let country = data_2.iter().find(|entity| entity.id.as_ref() == Some(&country_id)).or_fail()?;
    expect_that!(country.entity_type, some(eq(&OneOrMany::one(country_type.clone()))));
    expect_that!(country, has_attribute("name", any_attribute().value("France")));
    Ok(())
}
