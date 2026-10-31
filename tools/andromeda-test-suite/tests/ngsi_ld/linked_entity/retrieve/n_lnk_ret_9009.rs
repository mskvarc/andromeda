//! A local target joins without an object type.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join flat of a Building whose Relationships carry no `objectType` appends the Person
/// and the City it names, and not the Company one level further
/// Clause: 4.5.23.1, 4.5.23.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9009_a_local_target_joins_without_an_object_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_r17_id = namespace.urn("Company", "r17")?;
    let company_type = namespace.entity_type("Company")?;
    let person_r17_id = namespace.urn("Person", "r17")?;
    let person_type = namespace.entity_type("Person")?;
    let city_r17_id = namespace.urn("City", "r17")?;
    let city_type = namespace.entity_type("City")?;
    let building_r17_id = namespace.urn("Building", "r17")?;
    let building_type = namespace.entity_type("Building")?;
    let company_r17 = Entity::builder()
        .id(company_r17_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company_r17).send().await?.done()?;
    let person_r17 = Entity::builder()
        .id(person_r17_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_r17_id.clone())
        .build()?;
    broker.entities().create(&person_r17).send().await?.done()?;
    let city_r17 = Entity::builder()
        .id(city_r17_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city_r17).send().await?.done()?;
    let building_r17 = Entity::builder()
        .id(building_r17_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("owner", person_r17_id.clone())
        .relationship("tenant", person_r17_id.clone())
        .relationship("locatedIn", city_r17_id.clone())
        .build()?;
    broker.entities().create(&building_r17).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_r17_id)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), contains(has_id(&person_r17_id)));
    expect_that!(entities.as_slice(), contains(has_id(&city_r17_id)));
    expect_that!(entities.as_slice(), not(contains(has_id(&company_r17_id))));
    Ok(())
}
