//! Attrs reaches the flat linked entities.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    type_matcher::any_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with attrs name,owner and join flat answers the owner holding only id, type and name
/// Clause: Table 6.5.3.1-1, 5.7.1.5, 4.5.23.3
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9005_attrs_reaches_the_flat_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r14_id = namespace.urn("Company", "r14")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r14_id = namespace.urn("Person", "r14")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r14_id = namespace.urn("City", "r14")?;
    let city_type = namespace.entity_type("City")?;
    let building_r14_id = namespace.urn("Building", "r14")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r14 = Entity::builder()
        .id(company_r14_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r14).send().await?.done()?;
    let person_r14 = Entity::builder()
        .id(person_r14_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r14_id.clone())
        .build()?;
    broker.entities().create(&person_r14).send().await?.done()?;
    let city_r14 = Entity::builder()
        .id(city_r14_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r14).send().await?.done()?;
    let building_r14 = Entity::builder()
        .id(building_r14_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r14_id.clone())
        .relationship("tenant", person_r14_id.clone())
        .relationship("locatedIn", city_r14_id.clone())
        .build()?;
    broker.entities().create(&building_r14).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_r14_id)
        .attrs(vec!["name".parse()?, "owner".parse()?])
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(
        entities
            .as_slice()
            .iter()
            .filter(|item| item.id == person_r14_id)
            .map(member_count)
            .sum::<usize>(),
        eq(3)
    );
    expect_that!(
        entities.as_slice(),
        contains(all!(has_id(&person_r14_id), has_attribute("name", any_attribute()), any_type()))
    );
    Ok(())
}
