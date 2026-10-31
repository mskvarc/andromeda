//! An inline notification nests per relationship.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
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
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::{EntitySelector, Join, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a Subscription with pick `["owner{name}", "tenant", "locatedIn{size}"]` and join inline
/// carries one Entity whose owner nests a copy holding only name, whose tenant nests the whole Person, and whose
/// `locatedIn` keeps its object and nests nothing, the City holding no size
/// Clause: 5.8.6, 4.21, 4.5.23.2
/// Recorded: clause 4.21 does not say what becomes of a Linked Entity its projection leaves with no member;
/// Andromeda writes no entity member for it inline, keeping the Relationship and its object, and does not append it
/// flat
#[gtest]
#[tokio::test]
async fn o_lnk_ntf_9005_an_inline_notification_nests_per_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let company_id = namespace.urn("Company", "r3")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r3")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r3")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r3")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id("r3")?;
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
            NotificationParams::builder(Endpoint::builder(sink.endpoint("r3")?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Inline)
                .build()?,
        )
        .build()?;
    // The model cannot write the notification's projection, so the members are added to the rendered document.
    let request = create_request(&broker, &subscription, &[("pick", r#"["owner{name}", "tenant", "locatedIn{size}"]"#)])?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let fragment = EntityFragment::builder().property("name", "Warehouse").build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("r3", 1).await?;
    let notifications = sink.received("r3").await?;
    let notification = projected_entities(notifications.first().or_fail()?, Lossless::Normalized)?;
    let data = notification.data().as_slice();
    expect_that!(data, len(eq(1)));
    let building_entity = data.first().or_fail()?;
    let owners = linked_entities(building_entity, &["owner"]);
    expect_that!(owners.first().map(projected_member_count), some(eq(1)));
    expect_that!(owners, nth(0, has_attribute("name", any_attribute())));
    let tenants = linked_entities(building_entity, &["tenant"]);
    expect_that!(tenants.first().map(projected_member_count), some(eq(5)));
    expect_that!(tenants, nth(0, has_id(&person_id)));
    expect_that!(tenants.first().and_then(|tenant| tenant.entity_type.as_ref()), some(anything()));
    expect_that!(tenants, nth(0, has_attribute("name", any_attribute())));
    expect_that!(tenants, nth(0, has_attribute("email", any_attribute())));
    expect_that!(tenants, nth(0, has_attribute("employer", any_attribute())));
    expect_that!(building_entity, has_attribute("locatedIn", any_attribute().object(city_id.clone())));
    expect_that!(linked_entities(building_entity, &["locatedIn"]), is_empty());
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
