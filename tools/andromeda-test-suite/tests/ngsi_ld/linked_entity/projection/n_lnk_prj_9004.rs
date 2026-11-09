//! A group addresses only the entity its relationship reaches.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick id,owner{name,employer}, join flat and `joinLevel` 2 answers the owner holding
/// only name and employer and the employer whole, and with owner{name,employer{size}} the employer holding only
/// size
/// Clause: 4.21, 5.7.1.5, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9004_a_group_addresses_only_the_entity_its_relationship_reaches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r13_id = namespace.urn("Company", "r13")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r13_id = namespace.urn("Person", "r13")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r13_id = namespace.urn("City", "r13")?;
    let city_type = namespace.entity_type("City")?;
    let building_r13_id = namespace.urn("Building", "r13")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r13 = Entity::builder()
        .id(company_r13_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r13).send().await?.done()?;
    let person_r13 = Entity::builder()
        .id(person_r13_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r13_id.clone())
        .build()?;
    broker.entities().create(&person_r13).send().await?.done()?;
    let city_r13 = Entity::builder()
        .id(city_r13_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r13).send().await?.done()?;
    let building_r13 = Entity::builder()
        .id(building_r13_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r13_id.clone())
        .relationship("tenant", person_r13_id.clone())
        .relationship("locatedIn", city_r13_id.clone())
        .build()?;
    broker.entities().create(&building_r13).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r13_id)
        .pick("id,owner{name,employer}".parse::<Projection>()?)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(3)));
    expect_that!(entities.get(1).map(projected_member_count), some(eq(2)));
    expect_that!(entities.get(2).map(projected_member_count), some(eq(4)));
    let item = entities.get(2).or_fail()?;
    expect_that!(item.id, some(anything()));
    expect_that!(item.entity_type, some(anything()));
    expect_that!(
        entities.as_slice(),
        nth(1, all!(has_attribute("name", any_attribute()), has_attribute("employer", any_attribute())))
    );
    expect_that!(
        entities.as_slice(),
        nth(2, all!(has_attribute("name", any_attribute()), has_attribute("size", any_attribute())))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r13_id)
        .pick("id,owner{name,employer{size}}".parse::<Projection>()?)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body();
    expect_that!(entities_2.as_slice(), len(eq(3)));
    expect_that!(entities_2.get(2).map(projected_member_count), some(eq(1)));
    expect_that!(entities_2.as_slice(), nth(2, has_attribute("size", any_attribute())));
    Ok(())
}
