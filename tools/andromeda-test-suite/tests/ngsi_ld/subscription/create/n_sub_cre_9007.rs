//! A nested pick reads back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{
        EntitySelector,
        Join,
        builders::SelectorTypeBuilder,
        projection::{self, EntityMember, Projection},
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Subscription of a Subscription created with pick `["id", "owner{name,employer{size}}"]`, join
/// flat and `joinLevel` 2 answers that pick as written
/// Clause: Table 5.2.14.1-1, 4.21, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_cre_9007_a_nested_pick_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let company_id = namespace.urn("Company", "r6")?;
    let person_id = namespace.urn("Person", "r6")?;
    let city_id = namespace.urn("City", "r6")?;
    let building_id = namespace.urn("Building", "r6")?;
    let subscription_id = namespace.subscription_id("r6")?;
    let company_type = namespace.entity_type("Company")?;
    let person_type = namespace.entity_type("Person")?;
    let city_type = namespace.entity_type("City")?;
    let building_type = namespace.entity_type("Building")?;
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
        .relationship("employer", company_id)
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).property("name", "Paris").build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("owner", person_id.clone())
        .relationship("tenant", person_id)
        .relationship("locatedIn", city_id)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let pick = projection::member(EntityMember::Id)
        .linked("owner", projection::attributes(["name"]).linked("employer", projection::attributes(["size"])))
        .build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id)])
        .notification(
            NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?)
                .pick(pick.clone())
                .join(Join::Flat)
                .join_level(2)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let notification = stored.notification();
    expect_that!(notification.pick(), some(eq(&pick)));
    expect_that!(notification.pick().map(Projection::terms), some(len(eq(2))));
    expect_that!(
        notification.pick().and_then(|picked| picked.terms().get(1)).map(ToString::to_string),
        some(eq("owner{name,employer{size}}"))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
