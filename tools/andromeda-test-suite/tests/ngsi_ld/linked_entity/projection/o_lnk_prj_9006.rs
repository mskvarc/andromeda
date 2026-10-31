//! A linked entity its projection empties is not written.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick locatedIn{size},owner{name} and join inline keeps the object of `locatedIn` and
/// nests nothing under it, the City holding no size, while owner nests a copy holding only name; with pick
/// id,locatedIn{size} and join flat the City is not appended
/// Clause: 4.21, 4.5.23.2, 4.5.23.3, 5.7.1.5
/// Recorded: clause 4.21 does not say what becomes of a Linked Entity its projection leaves with no member;
/// Andromeda writes no entity member for it inline, keeping the Relationship and its object, and does not append it
/// flat
#[gtest]
#[tokio::test]
async fn o_lnk_prj_9006_a_linked_entity_its_projection_empties_is_not_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_id = namespace.urn("Company", "r16")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r16")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r16")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r16")?;
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
        .relationship("employer", company_id.clone())
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).property("name", "Paris").build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_id.clone())
        .relationship("tenant", person_id.clone())
        .relationship("locatedIn", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .join(InlineJoin::Inline)
        .pick("locatedIn{size},owner{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedIn", relationship().object(city_id.clone())));
    expect_that!(linked_entities(entity, &["locatedIn"]), is_empty());
    let owners = linked_entities(entity, &["owner"]);
    expect_that!(owners.first().map(projected_member_count), some(eq(1)));
    expect_that!(owners, nth(0, has_attribute("name", any_attribute())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_id)
        .pick("id,locatedIn{size}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(retrieved_2.body().as_slice(), len(eq(1)));
    Ok(())
}
