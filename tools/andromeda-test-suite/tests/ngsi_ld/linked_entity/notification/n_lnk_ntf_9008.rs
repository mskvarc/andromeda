//! A notification and a query narrow a linked entity alike.

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
    http::headers::Accept,
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder, projection::Projection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription with pick `["id", "owner{name}"]` and join flat and a Query Entities with
/// the same pick and join both answer the Building and the owner holding only name
/// Clause: 5.7.2.5, 5.8.6, 4.21
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_9008_a_notification_and_a_query_narrow_a_linked_entity_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let company_id = namespace.urn("Company", "r15")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r15")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r15")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r15")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("r15")?;
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
            NotificationParams::builder(Endpoint::builder(sink.endpoint("r15")?).accept(NgsiLdMediaType::Json).build()?)
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
    sink.wait_for_at_least("r15", 1).await?;
    let notifications = sink.received("r15").await?;
    let notification = projected_entities(notifications.first().or_fail()?, Lossless::Normalized)?;
    let data = notification.data().as_slice();
    expect_that!(data, len(eq(2)));
    expect_that!(data.get(1).map(projected_member_count), some(eq(1)));
    expect_that!(data, nth(1, has_attribute("name", any_attribute())));
    broker.subscriptions().delete(&subscription_id).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .pick("id,owner{name}".parse::<Projection>()?)
        .join(Join::Flat)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items().get(1).map(projected_member_count), some(eq(1)));
    expect_that!(page.items(), nth(1, has_attribute("name", any_attribute())));
    Ok(())
}
