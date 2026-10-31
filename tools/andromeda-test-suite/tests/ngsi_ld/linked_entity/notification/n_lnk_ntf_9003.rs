//! A change notification applies a nested pick.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
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
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription with pick `["id", "owner{name}"]` and join flat carries the Building
/// holding only id and owner, followed by the owner holding only name
/// Clause: 5.8.6, 4.21, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_9003_a_change_notification_applies_a_nested_pick() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let company_id = namespace.urn("Company", "r1")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r1")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r1")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r1")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("r1")?;
    let company = Entity::builder()
        .id(company_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company).send().await?.done()?;
    let person = Entity::builder()
        .id(person_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_id.clone())
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).property("name", "Paris").build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("owner", person_id.clone())
        .relationship("tenant", person_id.clone())
        .relationship("locatedIn", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("r1")?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .build()?,
        )
        .build()?;
    // The model cannot write the notification's projection, so the members are added to the rendered document.
    let request = create_request(&broker, &subscription, &[("pick", r#"["id", "owner{name}"]"#)])?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("name", "Warehouse").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("r1", 1).await?;
    let notifications = sink.received("r1").await?;
    let notification = projected_entities(notifications.first().or_fail()?, Lossless::Normalized)?;
    let data = notification.data().as_slice();
    expect_that!(data, len(eq(2)));
    expect_that!(data.first().map(projected_member_count), some(eq(2)));
    expect_that!(data.first().and_then(|entity| entity.id.as_ref()), some(eq(&building_id)));
    expect_that!(data, nth(0, has_attribute("owner", any_attribute())));
    expect_that!(data.get(1).map(projected_member_count), some(eq(1)));
    expect_that!(data, nth(1, has_attribute("name", any_attribute())));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
