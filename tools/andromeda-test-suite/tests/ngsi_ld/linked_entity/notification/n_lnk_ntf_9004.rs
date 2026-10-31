//! A change notification applies a plain pick to linked entities.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
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

/// Pins: A notification of a Subscription with pick `["id", "name", "owner"]` and join flat carries the owner
/// holding only id and name, and no City, the `locatedIn` Relationship not being returned
/// Clause: 5.8.6, 4.21, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_9004_a_change_notification_applies_a_plain_pick_to_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let company_id = namespace.urn("Company", "r2")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r2")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r2")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r2")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("r2")?;
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
            NotificationParams::builder(Endpoint::builder(sink.endpoint("r2")?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .build()?,
        )
        .build()?;
    // The model cannot write the notification's projection, so the members are added to the rendered document.
    let request = create_request(&broker, &subscription, &[("pick", r#"["id", "name", "owner"]"#)])?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("name", "Warehouse").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("r2", 1).await?;
    let notifications = sink.received("r2").await?;
    let notification = projected_entities(notifications.first().or_fail()?, Lossless::Normalized)?;
    let data = notification.data().as_slice();
    let person_entity = data.iter().find(|entity| entity.id.as_ref() == Some(&person_id)).or_fail()?;
    expect_that!(projected_member_count(person_entity), eq(2));
    expect_that!(person_entity, has_attribute("name", any_attribute()));
    expect_that!(data, not(contains(has_id(&city_id))));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
